use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use ripplepath_core::{ANALYSIS_SCHEMA_VERSION, Edge, EdgeKind, SymbolId, SymbolKind};
use ripplepath_git::{GitError, Repo};
use ripplepath_graph::{ImpactOptions, impact};

use crate::changes::{PathChange, TextStore, classify_symbols, diff_paths, hunks_for};
use crate::report::*;
use crate::snapshot::{FactCache, IndexStatus, Snapshot, build_snapshot};
use crate::{Limits, TOOL_VERSION};

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("snapshot has {count} files, above the limit of {limit}")]
    TooManyFiles { count: usize, limit: usize },
}

#[derive(Clone, Debug)]
pub struct AnalyzeOptions {
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    pub impact: ImpactOptions,
    pub limits: Limits,
    /// Maximum nodes in the visualisation slice.
    pub graph_node_cap: usize,
    /// Maximum individually listed unresolved references; the rest are summarised.
    pub unresolved_cap: usize,
}

impl AnalyzeOptions {
    pub fn new(repo: impl Into<PathBuf>, base: impl Into<String>, head: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            base: base.into(),
            head: head.into(),
            impact: ImpactOptions::default(),
            limits: Limits::default(),
            graph_node_cap: 400,
            unresolved_cap: 200,
        }
    }
}

pub fn analyze(options: &AnalyzeOptions) -> Result<AnalysisReport, AnalysisError> {
    let started = Instant::now();
    let repo = Repo::open(&options.repo)?;
    let base_rev = repo.resolve(&options.base)?;
    let head_rev = repo.resolve(&options.head)?;

    let mut cache = FactCache::default();
    let base = build_snapshot(&repo, base_rev, &mut cache, &options.limits)?;
    let head = build_snapshot(&repo, head_rev, &mut cache, &options.limits)?;
    let indexed_at = started.elapsed();

    let mut texts = TextStore::new(&repo, options.limits.max_file_bytes);
    let (path_changes, rename_skipped) = diff_paths(&base, &head, &mut texts)?;
    for change in &path_changes {
        for blob in [change.base_blob, change.head_blob].into_iter().flatten() {
            texts.load(blob)?;
        }
    }

    let files: Vec<FileChange> = path_changes
        .iter()
        .map(|change| FileChange {
            path: change.path.clone(),
            old_path: change.old_path.clone(),
            status: change.status,
            similarity: change.similarity,
            language: head
                .file(&change.path)
                .or_else(|| change.base_path().and_then(|p| base.file(p)))
                .and_then(|f| f.language),
            hunks: hunks_for(change, &base, &head, &texts),
        })
        .collect();

    let changed_symbols = classify_symbols(&base, &head, &path_changes);

    let mut head_roots = Vec::new();
    let mut base_roots = Vec::new();
    let mut changed_ids: BTreeSet<SymbolId> = BTreeSet::new();
    for symbol in &changed_symbols {
        changed_ids.insert(symbol.id.clone());
        match symbol.change {
            ChangeKind::Deleted => base_roots.push(symbol.id.clone()),
            ChangeKind::SignatureChanged => {
                head_roots.push(symbol.id.clone());
                if let Some(previous) = &symbol.previous_id {
                    base_roots.push(previous.clone());
                    changed_ids.insert(previous.clone());
                }
            }
            ChangeKind::Added | ChangeKind::Modified => head_roots.push(symbol.id.clone()),
        }
    }

    let head_impact = impact(&head.graph, &head_roots, options.impact);
    let base_impact = impact(&base.graph, &base_roots, options.impact);

    let mut impacted: BTreeMap<SymbolId, ImpactedSymbolReport> = BTreeMap::new();
    for (side, result, snapshot) in [(GraphSide::Head, &head_impact, &head), (GraphSide::Base, &base_impact, &base)] {
        for item in &result.impacted {
            if changed_ids.contains(&item.id) {
                continue;
            }
            // Prefer the head graph's explanation; describe the symbol as it exists in head when
            // it still does, since that is the code a reviewer will look at.
            let Some(symbol) = head.graph.symbol(&item.id).or_else(|| snapshot.graph.symbol(&item.id)) else {
                continue;
            };
            let candidate = ImpactedSymbolReport {
                id: item.id.clone(),
                kind: symbol.kind,
                name: symbol.name.clone(),
                module: symbol.module.clone(),
                file: symbol.file.clone(),
                span: symbol.span,
                is_test: symbol.is_test,
                depth: item.depth,
                root: item.root.clone(),
                weakest_evidence: item.weakest_evidence,
                graph: side,
                path: item.path.clone(),
            };
            match impacted.get(&item.id) {
                Some(existing) if existing.depth <= candidate.depth => {}
                _ => {
                    impacted.insert(item.id.clone(), candidate);
                }
            }
        }
    }
    let mut impacted_symbols: Vec<ImpactedSymbolReport> = impacted.into_values().collect();
    impacted_symbols.sort_by(|a, b| (a.depth, &a.id).cmp(&(b.depth, &b.id)));

    let tests = recommend_tests(&changed_symbols, &impacted_symbols);
    let tests_total = head.graph.symbols().filter(|s| s.is_test && s.kind == SymbolKind::Method).count();

    let truncated = head_impact.truncated || base_impact.truncated;
    let uncertainty = collect_uncertainty(
        &base,
        &head,
        &path_changes,
        &changed_ids,
        &impacted_symbols,
        rename_skipped,
        truncated,
        options.unresolved_cap,
    );

    let graph = graph_slice(&head, &base, &changed_symbols, &impacted_symbols, options.graph_node_cap);

    let modules: BTreeSet<&str> = changed_symbols
        .iter()
        .map(|s| s.module.as_str())
        .chain(impacted_symbols.iter().map(|s| s.module.as_str()))
        .collect();

    tracing::info!(
        base = %options.base,
        head = %options.head,
        files_changed = files.len(),
        symbols_changed = changed_symbols.len(),
        symbols_impacted = impacted_symbols.len(),
        tests_recommended = tests.len(),
        cache_hits = cache.hits,
        cache_misses = cache.misses,
        index_ms = indexed_at.as_millis() as u64,
        total_ms = started.elapsed().as_millis() as u64,
        "analysis complete"
    );

    Ok(AnalysisReport {
        schema_version: ANALYSIS_SCHEMA_VERSION,
        tool_version: TOOL_VERSION.to_owned(),
        base: revision_info(&base),
        head: revision_info(&head),
        summary: Summary {
            files_changed: files.len(),
            symbols_changed: changed_symbols.len(),
            symbols_impacted: impacted_symbols.len(),
            modules_impacted: modules.len(),
            tests_recommended: tests.len(),
            tests_total,
            uncertainty_items: uncertainty.len(),
            impact_truncated: truncated,
            max_depth: options.impact.max_depth,
        },
        files,
        changed_symbols,
        impacted_symbols,
        tests,
        uncertainty,
        graph,
    })
}

fn revision_info(snapshot: &Snapshot) -> RevisionInfo {
    RevisionInfo {
        spec: snapshot.revision.spec.clone(),
        commit: snapshot.revision.commit.clone(),
        tree: snapshot.revision.tree.to_string(),
    }
}

fn recommend_tests(changed: &[ChangedSymbol], impacted: &[ImpactedSymbolReport]) -> Vec<TestRecommendation> {
    let mut tests: Vec<TestRecommendation> = Vec::new();
    for symbol in
        changed.iter().filter(|s| s.is_test && s.kind == SymbolKind::Method && s.change != ChangeKind::Deleted)
    {
        tests.push(TestRecommendation {
            id: symbol.id.clone(),
            name: symbol.name.clone(),
            file: symbol.file.clone(),
            reason: TestReason::ChangedTest,
            depth: 0,
            root: symbol.id.clone(),
            weakest_evidence: ripplepath_core::Evidence::ResolvedExact,
            path: Vec::new(),
        });
    }
    let listed_methods: BTreeSet<&str> =
        impacted.iter().filter(|s| s.is_test && s.kind == SymbolKind::Method).map(|s| s.id.as_str()).collect();
    for symbol in impacted.iter().filter(|s| s.is_test) {
        // A test class is recommended on its own only when none of its methods is: it then runs as
        // a whole (e.g. because a field or fixture type it declares changed).
        if symbol.kind != SymbolKind::Method {
            let prefix = format!("{}#", symbol.id);
            if listed_methods.iter().any(|m| m.starts_with(&prefix)) {
                continue;
            }
        }
        tests.push(TestRecommendation {
            id: symbol.id.clone(),
            name: symbol.name.clone(),
            file: symbol.file.clone(),
            reason: TestReason::StaticPath,
            depth: symbol.depth,
            root: symbol.root.clone(),
            weakest_evidence: symbol.weakest_evidence,
            path: symbol.path.clone(),
        });
    }
    tests.sort_by(|a, b| {
        (a.reason, a.depth, std::cmp::Reverse(a.weakest_evidence.strength()), &a.id).cmp(&(
            b.reason,
            b.depth,
            std::cmp::Reverse(b.weakest_evidence.strength()),
            &b.id,
        ))
    });
    tests.dedup_by(|a, b| a.id == b.id);
    tests
}

#[allow(clippy::too_many_arguments)]
fn collect_uncertainty(
    base: &Snapshot,
    head: &Snapshot,
    changes: &[PathChange],
    changed_ids: &BTreeSet<SymbolId>,
    impacted: &[ImpactedSymbolReport],
    rename_skipped: bool,
    truncated: bool,
    unresolved_cap: usize,
) -> Vec<Uncertainty> {
    let mut items = Vec::new();
    let item = |severity, kind, file: Option<&str>, line, symbol: Option<&SymbolId>, detail: String| Uncertainty {
        severity,
        kind,
        file: file.map(str::to_owned),
        line,
        symbol: symbol.cloned(),
        detail,
    };

    let changed_head_paths: BTreeSet<&str> = changes.iter().filter_map(PathChange::head_path).collect();
    for change in changes {
        let sides = [(change.head_path(), head, "head"), (change.base_path(), base, "base")];
        for (path, snapshot, side) in sides {
            let Some(file) = path.and_then(|p| snapshot.file(p)) else {
                continue;
            };
            match &file.status {
                IndexStatus::Indexed if !file.syntax_error_lines.is_empty() => items.push(item(
                    if side == "head" { Severity::High } else { Severity::Medium },
                    UncertaintyKind::SyntaxError,
                    Some(&file.path),
                    file.syntax_error_lines.first().copied(),
                    None,
                    format!(
                        "{} syntax error region(s) in the {side} revision; declarations in them may be missing",
                        file.syntax_error_lines.len()
                    ),
                )),
                IndexStatus::ParseFailed { message } => items.push(item(
                    Severity::High,
                    UncertaintyKind::ParseFailure,
                    Some(&file.path),
                    None,
                    None,
                    format!("{side} revision not parsed: {message}"),
                )),
                IndexStatus::TooLarge { size } => items.push(item(
                    Severity::High,
                    UncertaintyKind::FileTooLarge,
                    Some(&file.path),
                    None,
                    None,
                    format!("{side} revision is {size} bytes, above the parse limit; its symbols are unknown"),
                )),
                IndexStatus::Binary if file.language.is_some() => items.push(item(
                    Severity::Medium,
                    UncertaintyKind::BinaryFile,
                    Some(&file.path),
                    None,
                    None,
                    format!("{side} revision is not valid UTF-8 text"),
                )),
                IndexStatus::NotSource if side == "head" || change.head_path().is_none() => items.push(item(
                    Severity::Low,
                    UncertaintyKind::UnsupportedLanguage,
                    Some(&file.path),
                    None,
                    None,
                    "changed file is not in a supported language; its effects are not traced".to_owned(),
                )),
                IndexStatus::Symlink | IndexStatus::Submodule => items.push(item(
                    Severity::Low,
                    UncertaintyKind::SymlinkOrSubmodule,
                    Some(&file.path),
                    None,
                    None,
                    "symlinks and submodules are not followed".to_owned(),
                )),
                _ => {}
            }
        }
    }

    // Parse failures outside the change still remove edges from the graph.
    let unchanged_failures = head
        .files
        .iter()
        .filter(|f| !changed_head_paths.contains(f.path.as_str()))
        .filter(|f| matches!(f.status, IndexStatus::ParseFailed { .. } | IndexStatus::TooLarge { .. }))
        .count();
    if unchanged_failures > 0 {
        items.push(item(
            Severity::Medium,
            UncertaintyKind::ParseFailure,
            None,
            None,
            None,
            format!(
                "{unchanged_failures} unchanged source file(s) could not be indexed; dependents in them are invisible"
            ),
        ));
    }

    let relevant: BTreeSet<&SymbolId> = changed_ids.iter().chain(impacted.iter().map(|i| &i.id)).collect();
    let mut unresolved: Vec<_> = head.unresolved.iter().filter(|u| relevant.contains(&u.from)).collect();
    unresolved.sort();
    let total = unresolved.len();
    for reference in unresolved.into_iter().take(unresolved_cap) {
        items.push(item(
            Severity::Medium,
            UncertaintyKind::UnresolvedReference,
            Some(&reference.file),
            Some(reference.line),
            Some(&reference.from),
            format!("{} could not be resolved; an impact edge may be missing", reference.detail),
        ));
    }
    if total > unresolved_cap {
        items.push(item(
            Severity::Medium,
            UncertaintyKind::UnresolvedReference,
            None,
            None,
            None,
            format!("{} further unresolved references in changed or impacted symbols", total - unresolved_cap),
        ));
    }

    for (path, reason) in head.rejected_paths.iter().chain(base.rejected_paths.iter()) {
        items.push(item(
            Severity::Medium,
            UncertaintyKind::RejectedPath,
            None,
            None,
            None,
            format!("tree entry {path:?} ignored: {reason}"),
        ));
    }
    if rename_skipped {
        items.push(item(
            Severity::Medium,
            UncertaintyKind::RenameDetectionSkipped,
            None,
            None,
            None,
            "too many added/deleted files for similarity rename detection; only exact renames were paired".to_owned(),
        ));
    }
    if truncated {
        items.push(item(
            Severity::High,
            UncertaintyKind::ImpactTruncated,
            None,
            None,
            None,
            "impact traversal reached its node limit; the impacted set is a lower bound".to_owned(),
        ));
    }
    items.sort();
    items.dedup();
    items
}

fn graph_slice(
    head: &Snapshot,
    base: &Snapshot,
    changed: &[ChangedSymbol],
    impacted: &[ImpactedSymbolReport],
    cap: usize,
) -> GraphSlice {
    let mut nodes: BTreeMap<SymbolId, GraphNode> = BTreeMap::new();
    let total = changed.len() + impacted.len();
    // Changed symbols first, then impacted by depth: the cap drops the most distant context first.
    for symbol in changed.iter().filter(|s| s.kind != SymbolKind::File).take(cap) {
        nodes.insert(
            symbol.id.clone(),
            GraphNode {
                id: symbol.id.clone(),
                name: symbol.name.clone(),
                kind: symbol.kind,
                module: symbol.module.clone(),
                file: symbol.file.clone(),
                line: symbol.span.start_line,
                role: NodeRole::Changed,
                change: Some(symbol.change),
                depth: 0,
                is_test: symbol.is_test,
            },
        );
    }
    for symbol in impacted {
        if nodes.len() >= cap {
            break;
        }
        nodes.insert(
            symbol.id.clone(),
            GraphNode {
                id: symbol.id.clone(),
                name: symbol.name.clone(),
                kind: symbol.kind,
                module: symbol.module.clone(),
                file: symbol.file.clone(),
                line: symbol.span.start_line,
                role: NodeRole::Impacted,
                change: None,
                depth: symbol.depth,
                is_test: symbol.is_test,
            },
        );
    }

    // Path hops whose endpoints are base-only ids (a deleted symbol's old signature) are attached to
    // the changed node that replaced it so the explanation stays connected in the picture.
    let alias: BTreeMap<&SymbolId, &SymbolId> =
        changed.iter().filter_map(|s| s.previous_id.as_ref().map(|prev| (prev, &s.id))).collect();
    let canonical = |id: &SymbolId| -> SymbolId { alias.get(id).map_or_else(|| id.clone(), |a| (*a).clone()) };

    let mut edges: BTreeMap<(SymbolId, SymbolId, EdgeKind), Edge> = BTreeMap::new();
    let mut add = |edge: &Edge| {
        let from = canonical(&edge.from);
        let to = canonical(&edge.to);
        if from != to && nodes.contains_key(&from) && nodes.contains_key(&to) {
            let mut edge = edge.clone();
            edge.from = from.clone();
            edge.to = to.clone();
            edges.entry((from, to, edge.kind)).or_insert(edge);
        }
    };
    for snapshot in [head, base] {
        for id in nodes.keys() {
            for edge in snapshot.graph.outgoing(id).filter(|e| e.kind.propagates_impact()) {
                add(edge);
            }
        }
    }
    for symbol in impacted {
        for hop in &symbol.path {
            add(&hop.edge);
        }
    }

    GraphSlice {
        nodes: nodes.into_values().collect(),
        edges: edges.into_values().collect(),
        clamped: total > cap,
        node_cap: cap,
    }
}
