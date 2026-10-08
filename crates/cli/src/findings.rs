//! Findings that have a source location: the subset of a report that SARIF and GitHub annotations
//! can express. Graph-level results (blast radius, test selection, cycles, risk) have no single
//! line to point at and stay in the JSON and Markdown outputs instead of being forced into a file
//! location.

use ripplepath_engine::architecture::DeltaStatus;
use ripplepath_engine::config::Gate;
use ripplepath_engine::policy::GateStatus;
use ripplepath_engine::{AnalysisReport, ConfigChange, Severity, UncertaintyKind};

use crate::output::wire_name;
use crate::text::display;

/// Every rule a finding can carry. The ids are a public contract: GitHub code scanning keys alert
/// history on them, so they never change meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    NewArchitectureViolation,
    BreakingApiChange,
    ParseFailureInChangedFile,
    ConfigInvalid,
}

impl Rule {
    pub const ALL: [Rule; 4] =
        [Rule::NewArchitectureViolation, Rule::BreakingApiChange, Rule::ParseFailureInChangedFile, Rule::ConfigInvalid];

    pub fn id(self) -> &'static str {
        match self {
            Rule::NewArchitectureViolation => "ripplepath/new-architecture-violation",
            Rule::BreakingApiChange => "ripplepath/breaking-api-change",
            Rule::ParseFailureInChangedFile => "ripplepath/parse-failure-in-changed-file",
            Rule::ConfigInvalid => "ripplepath/config-invalid",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rule::NewArchitectureViolation => "NewArchitectureViolation",
            Rule::BreakingApiChange => "BreakingApiChange",
            Rule::ParseFailureInChangedFile => "ParseFailureInChangedFile",
            Rule::ConfigInvalid => "ConfigInvalid",
        }
    }

    pub fn short_description(self) -> &'static str {
        match self {
            Rule::NewArchitectureViolation => "A dependency introduced by this change breaks a layer rule",
            Rule::BreakingApiChange => "Public API removed, narrowed or with a changed signature",
            Rule::ParseFailureInChangedFile => "A changed file could not be fully parsed",
            Rule::ConfigInvalid => "ripplepath.yml is invalid",
        }
    }

    pub fn full_description(self) -> &'static str {
        match self {
            Rule::NewArchitectureViolation => {
                "The head revision contains a symbol-level dependency that breaks a layer rule of the base \
                 revision's ripplepath.yml and that the base revision did not contain. Pre-existing \
                 violations are not reported here."
            }
            Rule::BreakingApiChange => {
                "A public API symbol was removed, its visibility was reduced, or its parameter list \
                 changed. Callers outside this repository may break without any trace in its graph."
            }
            Rule::ParseFailureInChangedFile => {
                "A changed file has syntax errors or could not be parsed in the head revision; symbols and \
                 dependencies inside it may be missing from the analysis."
            }
            Rule::ConfigInvalid => {
                "ripplepath.yml could not be parsed or validated. An invalid base configuration means the \
                 analysis used built-in defaults."
            }
        }
    }

    /// The merge-policy gate whose status decides this rule's level.
    pub fn gate(self) -> Gate {
        match self {
            Rule::NewArchitectureViolation => Gate::NewArchitectureViolation,
            Rule::BreakingApiChange => Gate::BreakingApiChange,
            Rule::ParseFailureInChangedFile => Gate::ParseFailureInChangedFile,
            Rule::ConfigInvalid => Gate::ConfigInvalid,
        }
    }

    /// Anchor in docs/GITHUB_INTEGRATION.md.
    pub fn help_anchor(self) -> &'static str {
        match self {
            Rule::NewArchitectureViolation => "new-architecture-violation",
            Rule::BreakingApiChange => "breaking-api-change",
            Rule::ParseFailureInChangedFile => "parse-failure-in-changed-file",
            Rule::ConfigInvalid => "config-invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub rule: Rule,
    /// Repository-relative path with forward slashes, including `--path-prefix`.
    pub file: String,
    /// `None` for file-level findings.
    pub line: Option<u32>,
    pub level: Level,
    pub title: String,
    pub message: String,
    /// Stable across runs and unaffected by line moves: identifies the finding for deduplication.
    pub fingerprint: String,
}

/// A finding's level follows its policy gate, so annotations, SARIF and the exit status never
/// disagree: a gate that fails the policy yields errors, a warning gate warnings, anything else
/// (gate off) notes.
fn level_of(report: &AnalysisReport, gate: Gate) -> Level {
    match report.policy.gates.iter().find(|g| g.gate == gate).map(|g| g.status) {
        Some(GateStatus::Fail) => Level::Error,
        Some(GateStatus::Warn) => Level::Warning,
        _ => Level::Note,
    }
}

fn fingerprint(rule: Rule, identity: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(rule.id().as_bytes());
    for part in identity {
        // Length-prefixed so that ("ab", "c") and ("a", "bc") never collide.
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher.finalize().to_hex()[..32].to_owned()
}

pub fn with_prefix(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() { path.to_owned() } else { format!("{prefix}/{path}") }
}

/// All located findings, sorted by (file, line, rule, fingerprint).
pub fn collect(report: &AnalysisReport, path_prefix: &str) -> Vec<Finding> {
    let mut out = Vec::new();

    let level = level_of(report, Gate::NewArchitectureViolation);
    for v in report.architecture.violations.iter().filter(|v| v.status == DeltaStatus::New) {
        let rule = Rule::NewArchitectureViolation;
        let kind = wire_name(&v.edge.kind);
        out.push(Finding {
            rule,
            file: with_prefix(path_prefix, &v.edge.file),
            line: Some(v.edge.line.max(1)),
            level,
            title: format!("New architecture violation: {} -> {}", v.from_layer, v.to_layer),
            message: format!(
                "Layer rule {} ({}) is broken by a dependency this change introduces: {} -{kind}-> {} \
                 (evidence {}, extractor rule {}).",
                v.rule + 1,
                v.description,
                display(&v.edge.from),
                display(&v.edge.to),
                wire_name(&v.edge.evidence),
                v.edge.rule
            ),
            fingerprint: fingerprint(rule, &[v.edge.from.as_str(), v.edge.to.as_str(), &kind, &v.description]),
        });
    }

    let level = level_of(report, Gate::BreakingApiChange);
    for change in report.api_surface.changes.iter().filter(|c| c.kind.is_breaking()) {
        let rule = Rule::BreakingApiChange;
        let kind = wire_name(&change.kind);
        out.push(Finding {
            rule,
            file: with_prefix(path_prefix, &change.file),
            line: Some(change.line.max(1)),
            level,
            title: format!("Breaking API change: {kind}"),
            message: match &change.previous_id {
                Some(previous) => format!(
                    "Public API {kind}: {} (was {}). Callers outside this repository may break.",
                    display(&change.id),
                    display(previous)
                ),
                None => {
                    format!("Public API {kind}: {}. Callers outside this repository may break.", display(&change.id))
                }
            },
            fingerprint: fingerprint(rule, &[&kind, change.id.as_str()]),
        });
    }

    // Exactly the uncertainty the `parse_failure_in_changed_file` gate counts: high severity is
    // the head side, so the line (when known) points into the revision under review.
    let level = level_of(report, Gate::ParseFailureInChangedFile);
    for item in report.uncertainty.iter().filter(|u| {
        u.severity == Severity::High
            && matches!(
                u.kind,
                UncertaintyKind::SyntaxError | UncertaintyKind::ParseFailure | UncertaintyKind::FileTooLarge
            )
    }) {
        let Some(file) = &item.file else { continue };
        let rule = Rule::ParseFailureInChangedFile;
        let kind = wire_name(&item.kind);
        out.push(Finding {
            rule,
            file: with_prefix(path_prefix, file),
            line: item.line.map(|l| l.max(1)),
            level,
            title: format!("Changed file not fully parsed: {kind}"),
            message: format!("{}. Changed or impacted symbols in this file may be missing.", item.detail),
            fingerprint: fingerprint(rule, &[file, &kind]),
        });
    }

    // Configuration errors carry no line. They are attached to the file only when head still has
    // it, so that the location exists in the revision being reviewed.
    let config = &report.config;
    if !matches!(config.head_change, ConfigChange::Removed | ConfigChange::Absent) {
        let level = level_of(report, Gate::ConfigInvalid);
        let sides = config.errors.iter().map(|e| ("base", e)).chain(config.head_errors.iter().map(|e| ("head", e)));
        for (side, error) in sides {
            let rule = Rule::ConfigInvalid;
            let consequence = if side == "base" {
                "This analysis used built-in defaults instead."
            } else {
                "It would apply after merge."
            };
            out.push(Finding {
                rule,
                file: with_prefix(path_prefix, &config.path),
                line: None,
                level,
                title: format!("Invalid {} in {side}", config.path),
                message: format!("{side} revision: {error}. {consequence}"),
                fingerprint: fingerprint(rule, &[side, error]),
            });
        }
    }

    out.sort_by(|a, b| (&a.file, a.line, a.rule, &a.fingerprint).cmp(&(&b.file, b.line, b.rule, &b.fingerprint)));
    // One result per identity: code scanning merges results that share a fingerprint anyway.
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|f| seen.insert((f.rule, f.fingerprint.clone())));
    out
}
