//! Configuration loading and the review-facing assessment of a change: architecture delta, owners,
//! API surface, risk decomposition and merge policy. The adapters here read blobs through the git
//! crate; everything they compute is delegated to the pure modules.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ripplepath_core::{EdgeKind, Evidence, SymbolId, SymbolKind};
use ripplepath_git::{BlobContent, GitError, Repo};
use ripplepath_graph::CodeGraph;
use serde::{Deserialize, Serialize};

use crate::api_surface::{ApiSurfaceReport, surface_changes};
use crate::architecture::{self, ArchitectureReport, DeltaStatus};
use crate::config::{self, CONFIG_PATH, Config, ConfigError, CycleMode, Gate, MAX_CONFIG_BYTES};
use crate::owners::{self, CODEOWNERS_PATHS, CodeOwners, MAX_CODEOWNERS_BYTES, OwnersReport};
use crate::policy::{self, GateFinding, PolicyReport};
use crate::report::{
    ChangeKind, ChangedSymbol, CoverageStatus, FileCategory, FileChange, ImpactedSymbolReport, Reliability,
    SelectionMode, Severity, TestRecommendation, Uncertainty, UncertaintyKind,
};
use crate::risk::{self, Measurement, RiskReport, SignalId};
use crate::snapshot::{IndexStatus, Snapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfigSource {
    /// No `ripplepath.yml` in the base revision: built-in defaults.
    Defaults,
    /// Loaded from the base revision.
    BaseRevision,
    /// The base revision's file is invalid: defaults are used and the `config_invalid` gate fails.
    InvalidUsingDefaults,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfigChange {
    /// Neither revision has the file.
    Absent,
    Unchanged,
    Added,
    Modified,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigReport {
    pub path: String,
    pub source: ConfigSource,
    /// The revision the configuration was read from: always base, so a change cannot relax the
    /// rules it is checked against (docs/adr/0005-config-from-base-revision.md).
    pub revision: String,
    pub blob: Option<String>,
    /// How head differs. Head's file is validated but never applied.
    pub head_change: ConfigChange,
    pub errors: Vec<String>,
    pub head_errors: Vec<String>,
    pub critical: Vec<String>,
    pub generated: Vec<String>,
    pub tests_mode: Option<SelectionMode>,
    /// Sorted.
    pub fail_on: Vec<Gate>,
    pub warn_on: Vec<Gate>,
}

pub(crate) struct LoadedConfig {
    pub config: Config,
    pub report: ConfigReport,
}

/// A configuration file's blob id and its parse result.
type ConfigFile = (String, Result<Config, ConfigError>);

/// Reads and validates `ripplepath.yml` of a snapshot. `Ok(None)`: no such file.
fn read_config(repo: &Repo, snapshot: &Snapshot) -> Result<Option<ConfigFile>, GitError> {
    let Some(file) = snapshot.file(CONFIG_PATH) else {
        return Ok(None);
    };
    let blob = file.blob.to_string();
    if matches!(file.status, IndexStatus::Symlink | IndexStatus::Submodule) {
        return Ok(Some((blob, Err(ConfigError::Invalid("is a symlink or submodule; it is not followed".to_owned())))));
    }
    let parsed = match repo.read_text(file.blob, MAX_CONFIG_BYTES)? {
        BlobContent::Text(text) => config::parse(&text),
        BlobContent::Binary => Err(ConfigError::NotText),
        BlobContent::TooLarge { size } => Err(ConfigError::TooLarge { size }),
    };
    Ok(Some((blob, parsed)))
}

pub(crate) fn load_config(repo: &Repo, base: &Snapshot, head: &Snapshot) -> Result<LoadedConfig, GitError> {
    let base_file = read_config(repo, base)?;
    let head_file = read_config(repo, head)?;
    let head_change = match (&base_file, &head_file) {
        (None, None) => ConfigChange::Absent,
        (None, Some(_)) => ConfigChange::Added,
        (Some(_), None) => ConfigChange::Removed,
        (Some((a, _)), Some((b, _))) if a == b => ConfigChange::Unchanged,
        (Some(_), Some(_)) => ConfigChange::Modified,
    };
    let head_errors = match &head_file {
        Some((_, Err(e))) if head_change != ConfigChange::Unchanged => vec![e.to_string()],
        _ => Vec::new(),
    };
    let (source, blob, config, errors) = match base_file {
        None => (ConfigSource::Defaults, None, Config::default(), Vec::new()),
        Some((blob, Ok(config))) => (ConfigSource::BaseRevision, Some(blob), config, Vec::new()),
        Some((blob, Err(e))) => {
            (ConfigSource::InvalidUsingDefaults, Some(blob), Config::default(), vec![e.to_string()])
        }
    };
    let report = ConfigReport {
        path: CONFIG_PATH.to_owned(),
        source,
        revision: base.revision.spec.clone(),
        blob,
        head_change,
        errors,
        head_errors,
        critical: config.critical.patterns().to_vec(),
        generated: config.generated.patterns().to_vec(),
        tests_mode: config.tests_mode,
        fail_on: config.policy.fail_on.iter().copied().collect(),
        warn_on: config.policy.warn_on.iter().copied().collect(),
    };
    Ok(LoadedConfig { config, report })
}

/// Configuration of one revision, for single-revision checks: invalid is an error there.
pub(crate) fn config_at(repo: &Repo, snapshot: &Snapshot) -> Result<Option<Config>, crate::AnalysisError> {
    match read_config(repo, snapshot)? {
        None => Ok(None),
        Some((_, Ok(config))) => Ok(Some(config)),
        Some((_, Err(e))) => Err(crate::AnalysisError::Config(e)),
    }
}

/// The parsed file (path, rules), whether the change edits it, and why it could not be read.
type CodeOwnersFile = (Option<(String, CodeOwners)>, bool, Option<String>);

fn read_codeowners(repo: &Repo, base: &Snapshot, head: &Snapshot) -> Result<CodeOwnersFile, GitError> {
    let Some(file) = CODEOWNERS_PATHS
        .iter()
        .filter_map(|p| head.file(p))
        .find(|f| !matches!(f.status, IndexStatus::Symlink | IndexStatus::Submodule))
    else {
        return Ok((None, false, None));
    };
    let changed = base.file(&file.path).map(|f| f.blob) != Some(file.blob);
    match repo.read_text(file.blob, MAX_CODEOWNERS_BYTES)? {
        BlobContent::Text(text) => Ok((Some((file.path.clone(), CodeOwners::parse(&text))), changed, None)),
        BlobContent::Binary => Ok((None, changed, Some(format!("{} is not UTF-8 text; ignored", file.path)))),
        BlobContent::TooLarge { size } => Ok((
            None,
            changed,
            Some(format!(
                "{} is {size} bytes; GitHub ignores CODEOWNERS files above 3 MB, and so does Ripplepath",
                file.path
            )),
        )),
    }
}

pub(crate) struct AssessInput<'a> {
    pub repo: &'a Repo,
    pub base: &'a Snapshot,
    pub head: &'a Snapshot,
    pub config: &'a LoadedConfig,
    pub files: &'a [FileChange],
    pub changed: &'a [ChangedSymbol],
    pub impacted: &'a [ImpactedSymbolReport],
    pub tests: &'a [TestRecommendation],
    pub uncertainty: &'a [Uncertainty],
    pub coverage_available: bool,
    pub max_depth: u32,
}

pub(crate) struct Assessment {
    pub architecture: ArchitectureReport,
    pub owners: OwnersReport,
    pub api_surface: ApiSurfaceReport,
    pub risk: RiskReport,
    pub policy: PolicyReport,
}

/// Base ids the change classification identifies with head ids: signature changes and probable
/// moves. Used so a finding carried along by such a change is not reported as removed-and-new.
fn identity_aliases(changed: &[ChangedSymbol]) -> BTreeMap<SymbolId, SymbolId> {
    let mut aliases = BTreeMap::new();
    for symbol in changed {
        match (symbol.change, &symbol.previous_id, &symbol.probable_move) {
            (ChangeKind::SignatureChanged, Some(previous), _) => {
                aliases.insert(previous.clone(), symbol.id.clone());
            }
            (ChangeKind::Added, _, Some(moved_from)) => {
                aliases.insert(moved_from.clone(), symbol.id.clone());
            }
            _ => {}
        }
    }
    aliases
}

fn dependents(graph: &CodeGraph, id: &SymbolId) -> usize {
    graph
        .incoming(id)
        .filter(|e| e.kind.propagates_impact() && e.kind != EdgeKind::Tests)
        .map(|e| &e.from)
        .collect::<BTreeSet<_>>()
        .len()
}

/// First critical symbol a deleted test reached in base, following its dependencies forward.
fn critical_reach(
    graph: &CodeGraph,
    test: &SymbolId,
    critical: &config::PathMatcher,
    max_depth: u32,
) -> Option<SymbolId> {
    const MAX_VISITED: usize = 5_000;
    let mut seen = BTreeSet::from([test.clone()]);
    let mut queue = VecDeque::from([(test.clone(), 0u32)]);
    while let Some((id, depth)) = queue.pop_front() {
        if depth > 0 && graph.symbol(&id).is_some_and(|s| critical.is_match(&s.file)) {
            return Some(id);
        }
        if depth >= max_depth || seen.len() >= MAX_VISITED {
            continue;
        }
        for edge in graph.outgoing(&id).filter(|e| e.kind.propagates_impact() && e.kind != EdgeKind::Tests) {
            if seen.insert(edge.to.clone()) {
                queue.push_back((edge.to.clone(), depth + 1));
            }
        }
    }
    None
}

fn edge_evidence(edge: &ripplepath_core::Edge) -> String {
    format!("{} -> {} ({:?}) {}:{}", edge.from, edge.to, edge.kind, edge.file, edge.line)
}

pub(crate) fn assess(input: &AssessInput<'_>) -> Result<Assessment, GitError> {
    let config = &input.config.config;
    let arch = &config.architecture;
    let generated = &config.generated;

    let base_state = architecture::evaluate(&input.base.graph, arch, generated);
    let head_state = architecture::evaluate(&input.head.graph, arch, generated);
    let architecture = architecture::delta(arch, &base_state, &head_state, &identity_aliases(input.changed));
    let layers_configured = architecture.configured;

    let (codeowners, codeowners_changed, read_error) = read_codeowners(input.repo, input.base, input.head)?;
    let changed_files: BTreeSet<String> = input.files.iter().map(|f| f.path.clone()).collect();
    let impacted_files: BTreeSet<String> = input.impacted.iter().map(|s| s.file.clone()).collect();
    let owners = owners::report(
        codeowners.as_ref().map(|(path, file)| (path.as_str(), file)),
        codeowners_changed,
        &changed_files,
        &impacted_files,
        read_error,
    );

    let api_surface = surface_changes(&input.base.graph, &input.head.graph, input.changed);

    // ---- risk measurements ----
    let mut m: Vec<(SignalId, Measurement)> = Vec::new();
    m.push((
        SignalId::PublicApiChanged,
        Measurement::count(
            api_surface
                .changes
                .iter()
                .filter(|c| c.kind.is_breaking())
                .map(|c| format!("{:?} {}", c.kind, c.id))
                .collect(),
        ),
    ));
    let files_in = |categories: &[FileCategory]| -> Vec<String> {
        input
            .files
            .iter()
            .filter(|f| f.category.is_some_and(|c| categories.contains(&c)))
            .map(|f| f.path.clone())
            .collect()
    };
    m.push((SignalId::MigrationChanged, Measurement::count(files_in(&[FileCategory::Migration]))));
    m.push((
        SignalId::BuildOrDependencyChanged,
        Measurement::count(files_in(&[
            FileCategory::Build,
            FileCategory::Lockfile,
            FileCategory::Ci,
            FileCategory::Container,
            FileCategory::Config,
        ])),
    ));
    let impacted_code: Vec<String> = input.impacted.iter().filter(|s| !s.is_test).map(|s| s.id.to_string()).collect();
    m.push((SignalId::BlastRadius, Measurement::of(impacted_code.len() as u64, impacted_code)));

    let mut hub: Option<(usize, &SymbolId)> = None;
    for symbol in input.changed.iter().filter(|s| !s.is_test && s.kind != SymbolKind::File) {
        let count = match symbol.change {
            ChangeKind::Deleted => dependents(&input.base.graph, &symbol.id),
            ChangeKind::SignatureChanged => {
                let before = symbol.previous_id.as_ref().map_or(0, |p| dependents(&input.base.graph, p));
                before.max(dependents(&input.head.graph, &symbol.id))
            }
            ChangeKind::Added | ChangeKind::Modified => dependents(&input.head.graph, &symbol.id),
        };
        // Ties keep the smallest id: changed symbols arrive sorted, and only a strictly larger
        // count replaces the current hub.
        if hub.is_none_or(|(best, _)| count > best) {
            hub = Some((count, &symbol.id));
        }
    }
    m.push((
        SignalId::ChangedSymbolCentrality,
        match hub {
            Some((count, id)) => Measurement::of(count as u64, vec![format!("{id} ({count} direct dependents)")]),
            None => Measurement::of(0, Vec::new()),
        },
    ));

    let critical_configured = !config.critical.is_empty();
    // A source file counts only when a symbol in it changed: a comment-only edit to critical code
    // is not a critical change. Files Ripplepath does not parse (migrations, SQL, …) always count.
    let files_with_changed_symbols: BTreeSet<&str> = input.changed.iter().map(|s| s.file.as_str()).collect();
    let critical_files: Vec<String> = input
        .files
        .iter()
        .filter(|f| {
            config.critical.is_match(&f.path) || f.old_path.as_deref().is_some_and(|p| config.critical.is_match(p))
        })
        .filter(|f| {
            f.language.is_none()
                || files_with_changed_symbols.contains(f.path.as_str())
                || f.old_path.as_deref().is_some_and(|p| files_with_changed_symbols.contains(p))
        })
        .map(|f| f.path.clone())
        .collect();
    m.push((
        SignalId::CriticalPathChanged,
        if critical_configured {
            Measurement::count(critical_files.clone())
        } else {
            Measurement::not_evaluated("no critical paths configured")
        },
    ));

    let new_violations: Vec<&architecture::Violation> =
        architecture.violations.iter().filter(|v| v.status == DeltaStatus::New).collect();
    m.push((
        SignalId::NewArchitectureViolation,
        if layers_configured {
            Measurement::of(
                architecture.summary.new_violations as u64,
                new_violations.iter().map(|v| edge_evidence(&v.edge)).collect(),
            )
        } else {
            Measurement::not_evaluated("no layers configured")
        },
    ));
    let cycles_evaluated = layers_configured && arch.cycles != CycleMode::Off;
    let new_cycles: Vec<String> =
        architecture.cycles.iter().filter(|c| c.status == DeltaStatus::New).map(|c| c.layers.join(" <-> ")).collect();
    m.push((
        SignalId::NewLayerCycle,
        if cycles_evaluated {
            Measurement::count(new_cycles.clone())
        } else {
            Measurement::not_evaluated("no layers configured, or cycles: off")
        },
    ));

    m.push((
        SignalId::ChangedCodeWithoutCoverage,
        if input.coverage_available {
            Measurement::count(
                input
                    .changed
                    .iter()
                    .filter(|s| !s.is_test && s.change != ChangeKind::Deleted && !generated.is_match(&s.file))
                    .filter(|s| matches!(s.coverage, Some(CoverageStatus::NotCovered | CoverageStatus::NoData)))
                    .map(|s| s.id.to_string())
                    .collect(),
            )
        } else {
            Measurement::not_evaluated("no coverage ingested")
        },
    ));
    let any_history = input.tests.iter().any(|t| t.history.is_some());
    m.push((
        SignalId::FlakyImpactedTests,
        if any_history {
            Measurement::count(
                input
                    .tests
                    .iter()
                    .filter(|t| t.history.as_ref().is_some_and(|h| h.reliability == Reliability::Flaky))
                    .map(|t| t.id.to_string())
                    .collect(),
            )
        } else {
            Measurement::not_evaluated("no CI history for the recommended tests")
        },
    ));
    let deleted_tests: Vec<&ChangedSymbol> = input
        .changed
        .iter()
        .filter(|s| s.change == ChangeKind::Deleted && s.is_test && s.kind.is_test_unit())
        .collect();
    m.push((SignalId::DeletedTests, Measurement::count(deleted_tests.iter().map(|s| s.id.to_string()).collect())));
    let unparsed_files: Vec<String> = input
        .uncertainty
        .iter()
        .filter(|u| u.severity == Severity::High)
        .filter(|u| {
            matches!(
                u.kind,
                UncertaintyKind::SyntaxError | UncertaintyKind::ParseFailure | UncertaintyKind::FileTooLarge
            )
        })
        .filter_map(|u| u.file.clone())
        .collect();
    m.push((SignalId::ParserUncertainty, Measurement::count(unparsed_files.clone())));
    let changed_ids: BTreeSet<&SymbolId> =
        input.changed.iter().flat_map(|s| std::iter::once(&s.id).chain(s.previous_id.as_ref())).collect();
    let unresolved: Vec<String> = input
        .uncertainty
        .iter()
        .filter(|u| u.kind == UncertaintyKind::UnresolvedReference)
        .filter(|u| u.symbol.as_ref().is_some_and(|s| changed_ids.contains(s)))
        .map(|u| format!("{}:{} {}", u.file.as_deref().unwrap_or("?"), u.line.unwrap_or(0), u.detail))
        .collect();
    m.push((SignalId::UnresolvedReferencesInChangedCode, Measurement::of(unresolved.len() as u64, unresolved)));
    let config_report = &input.config.report;
    let config_changed =
        matches!(config_report.head_change, ConfigChange::Added | ConfigChange::Modified | ConfigChange::Removed);
    m.push((
        SignalId::ConfigChanged,
        Measurement::count(if config_changed { vec![CONFIG_PATH.to_owned()] } else { Vec::new() }),
    ));
    let risk = risk::score(&m);

    // ---- policy gates ----
    let mut findings = Vec::new();
    if layers_configured {
        let s = &architecture.summary;
        let mut finding = GateFinding::new(
            Gate::NewArchitectureViolation,
            s.new_violations > 0,
            format!(
                "{} new layer-rule violation(s), {} on exactly resolved edges",
                s.new_violations, s.new_violations_exact
            ),
            new_violations.iter().map(|v| format!("{}: {}", v.description, edge_evidence(&v.edge))).collect(),
        );
        finding.inferred_only = s.new_violations_exact == 0;
        findings.push(finding);
        let present: Vec<&architecture::Violation> =
            architecture.violations.iter().filter(|v| v.status != DeltaStatus::Removed).collect();
        let mut finding = GateFinding::new(
            Gate::ArchitectureViolation,
            !present.is_empty(),
            format!(
                "{} layer-rule violation(s) in head ({} new, {} pre-existing)",
                s.new_violations + s.pre_existing_violations,
                s.new_violations,
                s.pre_existing_violations
            ),
            present.iter().map(|v| format!("{}: {}", v.description, edge_evidence(&v.edge))).collect(),
        );
        finding.inferred_only = present.iter().all(|v| v.edge.evidence != Evidence::ResolvedExact);
        findings.push(finding);
    } else {
        findings.push(GateFinding::not_evaluated(Gate::NewArchitectureViolation, "no layers configured"));
        findings.push(GateFinding::not_evaluated(Gate::ArchitectureViolation, "no layers configured"));
    }
    findings.push(if cycles_evaluated {
        GateFinding::new(
            Gate::NewCycle,
            !new_cycles.is_empty(),
            format!("{} new layer cycle(s)", new_cycles.len()),
            new_cycles,
        )
    } else {
        GateFinding::not_evaluated(Gate::NewCycle, "no layers configured, or cycles: off")
    });
    findings.push(GateFinding::new(
        Gate::ParseFailureInChangedFile,
        !unparsed_files.is_empty(),
        format!("{} changed file(s) not fully parsed", BTreeSet::from_iter(&unparsed_files).len()),
        unparsed_files,
    ));
    findings.push(if critical_configured {
        let mut hits = Vec::new();
        for test in &deleted_tests {
            if config.critical.is_match(&test.file) {
                hits.push(format!("{} (in critical path {})", test.id, test.file));
            } else if let Some(reached) = critical_reach(&input.base.graph, &test.id, &config.critical, input.max_depth)
            {
                hits.push(format!("{} (reached critical {reached} in base)", test.id));
            }
        }
        GateFinding::new(
            Gate::RemovedTestOnCriticalPath,
            !hits.is_empty(),
            format!("{} deleted test(s) on a critical path", hits.len()),
            hits,
        )
    } else {
        GateFinding::not_evaluated(Gate::RemovedTestOnCriticalPath, "no critical paths configured")
    });
    findings.push(GateFinding::new(
        Gate::BreakingApiChange,
        api_surface.breaking > 0,
        format!("{} public API symbol(s) removed, narrowed or with a changed signature", api_surface.breaking),
        api_surface
            .changes
            .iter()
            .filter(|c| c.kind.is_breaking())
            .map(|c| format!("{:?} {} ({}:{})", c.kind, c.id, c.file, c.line))
            .collect(),
    ));
    findings.push(GateFinding::new(
        Gate::ConfigChanged,
        config_changed,
        format!(
            "{CONFIG_PATH} is {:?} in head; this analysis used the base revision's rules",
            config_report.head_change
        ),
        if config_changed { vec![CONFIG_PATH.to_owned()] } else { Vec::new() },
    ));
    let config_errors: Vec<String> = config_report
        .errors
        .iter()
        .map(|e| format!("base: {e}"))
        .chain(config_report.head_errors.iter().map(|e| format!("head: {e}")))
        .collect();
    findings.push(GateFinding::new(
        Gate::ConfigInvalid,
        !config_errors.is_empty(),
        if config_errors.is_empty() { "configuration valid".to_owned() } else { "invalid configuration".to_owned() },
        config_errors,
    ));
    let policy = policy::evaluate(&config.policy, arch.cycles, findings);

    Ok(Assessment { architecture, owners, api_surface, risk, policy })
}

/// Architecture state of one revision (`ripplepath architecture check`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureCheck {
    pub schema_version: u32,
    pub tool_version: String,
    pub revision: crate::report::RevisionInfo,
    pub config_found: bool,
    pub rules: Vec<architecture::RuleReport>,
    /// (layer, symbols) in configuration order.
    pub layers: Vec<(String, usize)>,
    pub violations: Vec<architecture::Violation>,
    pub violations_truncated: bool,
    pub cycles: Vec<architecture::LayerCycle>,
}

pub(crate) fn check_state(snapshot: &Snapshot, config: &Config) -> (Vec<(String, usize)>, ArchitectureReport) {
    let state = architecture::evaluate(&snapshot.graph, &config.architecture, &config.generated);
    // Compared with itself every finding is PRE_EXISTING: the report then describes the state.
    let report = architecture::delta(&config.architecture, &state, &state, &BTreeMap::new());
    let layers = config
        .architecture
        .layers
        .iter()
        .map(|l| (l.name.clone(), state.layer_symbols.get(&l.name).copied().unwrap_or(0)))
        .collect();
    (layers, report)
}

/// Evaluates the layer rules of one revision against that revision's own `ripplepath.yml`.
pub fn check_architecture(
    repo_path: &std::path::Path,
    revision: &str,
    limits: &crate::Limits,
) -> Result<ArchitectureCheck, crate::AnalysisError> {
    let repo = Repo::open(repo_path)?;
    let resolved = repo.resolve(revision)?;
    let snapshot = crate::snapshot::build_snapshot(&repo, resolved, &mut crate::FactCache::default(), limits)?;
    let config = config_at(&repo, &snapshot)?;
    let config_found = config.is_some();
    let config = config.unwrap_or_default();
    let (layers, report) = check_state(&snapshot, &config);
    Ok(ArchitectureCheck {
        schema_version: ripplepath_core::ANALYSIS_SCHEMA_VERSION,
        tool_version: crate::TOOL_VERSION.to_owned(),
        revision: crate::report::RevisionInfo {
            spec: snapshot.revision.spec.clone(),
            commit: snapshot.revision.commit.clone(),
            tree: snapshot.revision.tree.to_string(),
        },
        config_found,
        rules: report.rules,
        layers,
        violations: report.violations,
        violations_truncated: report.violations_truncated,
        cycles: report.cycles,
    })
}
