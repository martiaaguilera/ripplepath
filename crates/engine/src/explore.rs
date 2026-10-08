//! Single-revision queries for interactive callers such as the MCP adapter: what a symbol is, what
//! depends on it, how two symbols are connected, and what a change to a symbol that has not been
//! edited yet would reach ("what if").
//!
//! Everything here reuses the analysis pipeline's own pieces — snapshot, persisted test evidence,
//! impact traversal, test recommendation — so an answer about a hypothetical change is the answer
//! `analyze` would give once that change is committed, minus what only a real diff can tell
//! (signature changes, deletions, hunks).

use std::collections::BTreeSet;
use std::path::Path;

use ripplepath_core::{Edge, Symbol, SymbolId};
use ripplepath_git::Repo;
use ripplepath_graph::{CodeGraph, ImpactOptions, PathOptions, PathSearch, impact};
use serde::{Deserialize, Serialize};

use crate::analysis::recommend_tests;
use crate::architecture::{self, FoundViolation};
use crate::config::Config;
use crate::report::{
    ChangeKind, ChangedSymbol, CoverageStatus, EvidenceSummary, GraphSide, ImpactedSymbolReport, RevisionInfo,
    Severity, TestRecommendation, Uncertainty, UncertaintyKind,
};
use crate::snapshot::{FactCache, IndexStatus, Snapshot, build_snapshot};
use crate::test_evidence::LoadedEvidence;
use crate::{AnalysisError, Limits};

/// Unresolved references listed individually per answer; the rest are counted.
const MAX_LISTED_UNRESOLVED: usize = 50;

/// One revision, indexed and ready to query. Built once, queried many times.
pub struct RevisionView {
    pub revision: RevisionInfo,
    snapshot: Snapshot,
    config: Config,
    /// Why the revision's `ripplepath.yml` could not be applied (defaults are used instead).
    pub config_error: Option<String>,
    evidence: LoadedEvidence,
}

/// Resolves a revision spec without indexing it: cheap, so callers can key caches by tree.
pub fn resolve_revision(repo_path: &Path, spec: &str) -> Result<RevisionInfo, AnalysisError> {
    let repo = Repo::open(repo_path)?;
    let revision = repo.resolve(spec)?;
    Ok(RevisionInfo { spec: revision.spec, commit: revision.commit, tree: revision.tree.to_string() })
}

/// A fact cache backed by the database at `db` when given (facts persist across loads), else
/// in memory only.
pub fn fact_cache(db: Option<&Path>) -> Result<FactCache, AnalysisError> {
    Ok(match db {
        Some(path) => FactCache::with_store(ripplepath_storage::Store::open(path)?),
        None => FactCache::default(),
    })
}

/// Indexes `spec`. With a store attached to `cache`, facts are reused from it and measured coverage
/// joins the graph as `TESTS` edges with `COVERAGE_OBSERVED` evidence, exactly as in `analyze`.
pub fn load_revision(
    repo_path: &Path,
    spec: &str,
    cache: &mut FactCache,
    limits: &Limits,
) -> Result<RevisionView, AnalysisError> {
    let repo = Repo::open(repo_path)?;
    let resolved = repo.resolve(spec)?;
    let mut snapshot = build_snapshot(&repo, resolved, cache, limits)?;
    let (config, config_error) = match crate::assess::config_at(&repo, &snapshot) {
        Ok(config) => (config.unwrap_or_default(), None),
        Err(AnalysisError::Config(e)) => (Config::default(), Some(e.to_string())),
        Err(other) => return Err(other),
    };
    let mut evidence = match cache.store_mut() {
        Some(store) => LoadedEvidence::load(store)?,
        None => LoadedEvidence::default(),
    };
    evidence.mark_other_commits(snapshot.revision.commit.as_deref(), None);
    let coverage_edges = evidence.coverage_edges(&snapshot.graph);
    if !coverage_edges.is_empty() {
        evidence.summary.coverage_edges = coverage_edges.len();
        let mut edges = snapshot.graph.edges().to_vec();
        edges.extend(coverage_edges);
        snapshot.graph = CodeGraph::new(snapshot.graph.symbols().cloned().collect(), edges);
    }
    let revision = RevisionInfo {
        spec: snapshot.revision.spec.clone(),
        commit: snapshot.revision.commit.clone(),
        tree: snapshot.revision.tree.to_string(),
    };
    Ok(RevisionView { revision, snapshot, config, config_error, evidence })
}

/// A rule-breaking dependency that exists in this revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionViolation {
    pub rule: usize,
    pub description: String,
    pub from_layer: String,
    pub to_layer: String,
    pub edge: Edge,
}

/// The impact of modifying one symbol, computed on one revision's graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhatIf {
    pub revision: RevisionInfo,
    pub root: SymbolId,
    /// Sorted by (depth, id); every entry carries its explaining path.
    pub impacted: Vec<ImpactedSymbolReport>,
    /// Same ordering and semantics as `AnalysisReport::tests`.
    pub tests: Vec<TestRecommendation>,
    /// Sorted.
    pub uncertainty: Vec<Uncertainty>,
    pub truncated: bool,
    pub max_depth: u32,
    /// Sorted, distinct modules and configured layers of the root and the impacted symbols.
    pub modules: Vec<String>,
    pub layers: Vec<String>,
    pub evidence: EvidenceSummary,
}

impl RevisionView {
    pub fn graph(&self) -> &CodeGraph {
        &self.snapshot.graph
    }

    pub fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.snapshot.graph.symbol(id)
    }

    pub fn evidence_summary(&self) -> &EvidenceSummary {
        &self.evidence.summary
    }

    /// The configured layer owning `symbol`'s file. Generated code belongs to no layer, matching
    /// how architecture rules are evaluated.
    pub fn layer_of(&self, symbol: &Symbol) -> Option<&str> {
        if self.config.generated.is_match(&symbol.file) {
            return None;
        }
        self.config.architecture.layer_of(&symbol.file)
    }

    pub fn is_generated(&self, path: &str) -> bool {
        self.config.generated.is_match(path)
    }

    pub fn has_layers(&self) -> bool {
        !self.config.architecture.layers.is_empty()
    }

    pub fn coverage_status(&self, symbol: &Symbol) -> Option<CoverageStatus> {
        self.evidence.coverage_status(symbol)
    }

    /// Symbols whose id or name contains `query` (case-insensitive), or whose file is `query`.
    /// Sorted by id; returns the first `limit` and the total count.
    pub fn find_symbols(&self, query: &str, limit: usize) -> (Vec<&Symbol>, usize) {
        let needle = query.to_lowercase();
        let matches: Vec<&Symbol> = self
            .snapshot
            .graph
            .symbols()
            .filter(|s| {
                s.file == query
                    || s.id.as_str().to_lowercase().contains(&needle)
                    || s.name.to_lowercase().contains(&needle)
            })
            .collect();
        let total = matches.len();
        (matches.into_iter().take(limit).collect(), total)
    }

    /// Rule-breaking dependencies of this revision that start or end at `id`, sorted by edge.
    pub fn violations_involving(&self, id: &SymbolId) -> Vec<RevisionViolation> {
        if self.config.architecture.rules.is_empty() {
            return Vec::new();
        }
        let state = architecture::evaluate(&self.snapshot.graph, &self.config.architecture, &self.config.generated);
        let mut found: Vec<RevisionViolation> = state
            .violations
            .into_values()
            .filter(|v| &v.edge.from == id || &v.edge.to == id)
            .map(|FoundViolation { rule, from_layer, to_layer, edge }| RevisionViolation {
                rule,
                description: self.config.architecture.rules.get(rule).map(|r| r.describe()).unwrap_or_default(),
                from_layer,
                to_layer,
                edge,
            })
            .collect();
        found.sort_by(|a, b| a.edge.cmp(&b.edge));
        found
    }

    /// Unresolved references made from inside `id`, as uncertainty items.
    pub fn unresolved_from(&self, id: &SymbolId) -> Vec<Uncertainty> {
        let ids = BTreeSet::from([id]);
        self.unresolved_items(&ids)
    }

    pub fn dependency_path(&self, from: &SymbolId, to: &SymbolId, options: PathOptions) -> PathSearch {
        ripplepath_graph::dependency_path(&self.snapshot.graph, from, to, options)
    }

    /// What modifying `root` would impact, and which tests have evidence of exercising it.
    /// `None` when the symbol does not exist in this revision.
    pub fn what_if(&self, root: &SymbolId, options: ImpactOptions) -> Option<WhatIf> {
        let graph = &self.snapshot.graph;
        let symbol = graph.symbol(root)?;
        let result = impact(graph, std::slice::from_ref(root), options);
        let impacted: Vec<ImpactedSymbolReport> = result
            .impacted
            .iter()
            .filter_map(|item| {
                let s = graph.symbol(&item.id)?;
                Some(ImpactedSymbolReport {
                    id: item.id.clone(),
                    kind: s.kind,
                    name: s.name.clone(),
                    module: s.module.clone(),
                    file: s.file.clone(),
                    span: s.span,
                    is_test: s.is_test,
                    depth: item.depth,
                    root: item.root.clone(),
                    weakest_evidence: item.weakest_evidence,
                    graph: GraphSide::Head,
                    path: item.path.clone(),
                    coverage: self.evidence.coverage_status(s),
                })
            })
            .collect();
        // The hypothetical edit is a modification: same id before and after.
        let changed = ChangedSymbol {
            id: root.clone(),
            change: ChangeKind::Modified,
            previous_id: None,
            probable_move: None,
            kind: symbol.kind,
            name: symbol.name.clone(),
            language: symbol.language,
            module: symbol.module.clone(),
            file: symbol.file.clone(),
            span: symbol.span,
            visibility: symbol.visibility,
            is_test: symbol.is_test,
            coverage: self.evidence.coverage_status(symbol),
        };
        let mut tests = recommend_tests(graph, std::slice::from_ref(&changed), &impacted);
        for test in &mut tests {
            test.history = self.evidence.history(&test.id);
        }

        let mut relevant: BTreeSet<&SymbolId> = impacted.iter().map(|i| &i.id).collect();
        relevant.insert(root);
        let mut uncertainty = self.unresolved_items(&relevant);
        uncertainty.extend(self.revision_uncertainty(symbol, result.truncated));
        uncertainty.sort();
        uncertainty.dedup();

        let involved = std::iter::once(symbol).chain(impacted.iter().filter_map(|i| graph.symbol(&i.id)));
        let mut modules = BTreeSet::new();
        let mut layers = BTreeSet::new();
        for s in involved {
            modules.insert(s.module.clone());
            if let Some(layer) = self.layer_of(s) {
                layers.insert(layer.to_owned());
            }
        }

        Some(WhatIf {
            revision: self.revision.clone(),
            root: root.clone(),
            impacted,
            tests,
            uncertainty,
            truncated: result.truncated,
            max_depth: options.max_depth,
            modules: modules.into_iter().collect(),
            layers: layers.into_iter().collect(),
            evidence: self.evidence.summary.clone(),
        })
    }

    fn unresolved_items(&self, relevant: &BTreeSet<&SymbolId>) -> Vec<Uncertainty> {
        let mut unresolved: Vec<_> = self.snapshot.unresolved.iter().filter(|u| relevant.contains(&u.from)).collect();
        unresolved.sort();
        let total = unresolved.len();
        let mut items: Vec<Uncertainty> = unresolved
            .into_iter()
            .take(MAX_LISTED_UNRESOLVED)
            .map(|u| Uncertainty {
                severity: Severity::Medium,
                kind: UncertaintyKind::UnresolvedReference,
                file: Some(u.file.clone()),
                line: Some(u.line),
                symbol: Some(u.from.clone()),
                detail: format!("{} could not be resolved; an impact edge may be missing", u.detail),
            })
            .collect();
        if total > MAX_LISTED_UNRESOLVED {
            items.push(Uncertainty {
                severity: Severity::Medium,
                kind: UncertaintyKind::UnresolvedReference,
                file: None,
                line: None,
                symbol: None,
                detail: format!("{} further unresolved references", total - MAX_LISTED_UNRESOLVED),
            });
        }
        items
    }

    /// Conditions of the revision that can hide dependents of `root`.
    fn revision_uncertainty(&self, root: &Symbol, truncated: bool) -> Vec<Uncertainty> {
        let item = |severity, kind, file: Option<&str>, line, detail: String| Uncertainty {
            severity,
            kind,
            file: file.map(str::to_owned),
            line,
            symbol: None,
            detail,
        };
        let mut items = Vec::new();
        if let Some(file) = self.snapshot.file(&root.file)
            && let Some(first) = file.syntax_error_lines.first()
        {
            items.push(item(
                Severity::Medium,
                UncertaintyKind::SyntaxError,
                Some(&file.path),
                Some(*first),
                format!(
                    "{} syntax error region(s) in this file; declarations or references in them may be missing",
                    file.syntax_error_lines.len()
                ),
            ));
        }
        let unindexed = self
            .snapshot
            .files
            .iter()
            .filter(|f| matches!(f.status, IndexStatus::ParseFailed { .. } | IndexStatus::TooLarge { .. }))
            .count();
        if unindexed > 0 {
            items.push(item(
                Severity::Medium,
                UncertaintyKind::ParseFailure,
                None,
                None,
                format!("{unindexed} source file(s) could not be indexed; dependents in them are invisible"),
            ));
        }
        for (path, reason) in &self.snapshot.rejected_paths {
            items.push(item(
                Severity::Medium,
                UncertaintyKind::RejectedPath,
                None,
                None,
                format!("tree entry {path:?} ignored: {reason}"),
            ));
        }
        if truncated {
            items.push(item(
                Severity::High,
                UncertaintyKind::ImpactTruncated,
                None,
                None,
                "impact traversal reached its node limit; the impacted set is a lower bound".to_owned(),
            ));
        }
        if self.evidence.summary.coverage_reports_other_commits > 0 {
            items.push(item(
                Severity::Low,
                UncertaintyKind::StaleCoverage,
                None,
                None,
                format!(
                    "{} coverage report(s) were measured at other commits; code may have moved since",
                    self.evidence.summary.coverage_reports_other_commits
                ),
            ));
        }
        items
    }
}
