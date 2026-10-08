//! Offline replay evaluation of test selection (docs/TEST_INTELLIGENCE.md, "Offline evaluation").
//!
//! Each historical change `base → head` is analysed again with only the test evidence that existed
//! before it — coverage and CI results recorded at `base` or one of its ancestors — and the
//! resulting selection is scored against the tests that actually failed at `head`. Scoring
//! ([`score`]) is pure; the rest is a thin adapter over the analysis.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ripplepath_core::SymbolId;
use ripplepath_evidence::{Outcome, parse_junit};
use ripplepath_git::{GitError, Repo};
use ripplepath_graph::{CodeGraph, ImpactOptions};
use ripplepath_storage::Store;
use serde::{Deserialize, Serialize};

use crate::evidence::{MappedCase, map_junit};
use crate::report::{EvidenceSummary, SelectionDecision, SelectionMode, TestSelection};
use crate::snapshot::{FactCache, Snapshot, build_snapshot};
use crate::{AnalysisError, AnalyzeOptions, Limits, TOOL_VERSION, analyze};

pub const EVALUATION_SCHEMA_VERSION: u32 = 1;
/// Below this many cases with an observed failure, aggregate figures are labelled as anecdotal.
pub const SMALL_SAMPLE_FAILING_CASES: usize = 30;
pub const SMALL_SAMPLE_LABEL: &str = "small sample — not statistically meaningful";
pub const MAX_CASES: usize = 10_000;
pub const MAX_JUNIT_FILES_PER_CASE: usize = 1_000;
const MAX_CASES_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Ancestor walks beyond this are refused, not truncated: a partial set would silently hide
/// evidence that legitimately existed before a change.
const MAX_ANCESTORS: usize = 1_000_000;

#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Storage(#[from] ripplepath_storage::StorageError),
    #[error(transparent)]
    Evidence(#[from] ripplepath_evidence::EvidenceError),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is {size} bytes, above the limit of {limit}")]
    TooLarge { path: PathBuf, size: u64, limit: u64 },
    #[error("invalid cases file {path}: {message}")]
    CasesFile { path: PathBuf, message: String },
    #[error("case '{case}': '{spec}' does not name a commit")]
    NotACommit { case: String, spec: String },
    #[error("case '{case}': head is base or one of its ancestors, so evidence from head would count as prior evidence")]
    HeadNotAfterBase { case: String },
    #[error("case '{case}' lists no JUnit file: the outcome at head is unknown")]
    NoObservedOutcome { case: String },
    #[error("case '{case}' lists {count} JUnit files, above the limit of {limit}")]
    TooManyJunitFiles { case: String, count: usize, limit: usize },
    #[error("no cases to evaluate")]
    NoCases,
    #[error("{count} cases, above the limit of {limit}")]
    TooManyCases { count: usize, limit: usize },
}

/// One observed result file, already read.
#[derive(Clone, Debug)]
pub struct JunitInput {
    pub source: String,
    pub text: String,
}

/// A historical change and what its test run observed at head.
#[derive(Clone, Debug)]
pub struct EvaluationCase {
    pub name: String,
    pub base: String,
    pub head: String,
    /// JUnit files of the test run at head. Several files are parts of one run (e.g. one per
    /// test class); a test listed more than once failed if any entry failed.
    pub junit: Vec<JunitInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CasesFile {
    cases: Vec<CaseEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseEntry {
    name: Option<String>,
    base: String,
    head: String,
    junit: Vec<PathBuf>,
}

fn read_bounded(path: &Path, limit: u64) -> Result<String, EvaluationError> {
    let io = |source| EvaluationError::Io { path: path.to_owned(), source };
    let size = std::fs::metadata(path).map_err(io)?.len();
    if size > limit {
        return Err(EvaluationError::TooLarge { path: path.to_owned(), size, limit });
    }
    std::fs::read_to_string(path).map_err(io)
}

/// Reads a cases file (JSON, `{"cases": [{"name", "base", "head", "junit": [paths]}]}`). JUnit
/// paths are relative to the cases file's directory.
pub fn load_cases(path: &Path) -> Result<Vec<EvaluationCase>, EvaluationError> {
    let text = read_bounded(path, MAX_CASES_FILE_BYTES)?;
    let file: CasesFile = serde_json::from_str(&text)
        .map_err(|e| EvaluationError::CasesFile { path: path.to_owned(), message: e.to_string() })?;
    if file.cases.len() > MAX_CASES {
        return Err(EvaluationError::TooManyCases { count: file.cases.len(), limit: MAX_CASES });
    }
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    file.cases
        .into_iter()
        .map(|entry| {
            let name = entry.name.unwrap_or_else(|| format!("{}..{}", entry.base, entry.head));
            if entry.junit.len() > MAX_JUNIT_FILES_PER_CASE {
                return Err(EvaluationError::TooManyJunitFiles {
                    case: name,
                    count: entry.junit.len(),
                    limit: MAX_JUNIT_FILES_PER_CASE,
                });
            }
            let junit = entry
                .junit
                .iter()
                .map(|relative| {
                    let file = dir.join(relative);
                    let text = read_bounded(&file, crate::MAX_EVIDENCE_BYTES as u64)?;
                    Ok(JunitInput { source: relative.to_string_lossy().replace('\\', "/"), text })
                })
                .collect::<Result<_, EvaluationError>>()?;
            Ok(EvaluationCase { name, base: entry.base, head: entry.head, junit })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct EvaluationOptions {
    pub repo: PathBuf,
    /// Index and evidence database holding the history's coverage and CI results.
    pub db: PathBuf,
    pub modes: Vec<SelectionMode>,
    pub impact: ImpactOptions,
    pub limits: Limits,
}

impl EvaluationOptions {
    pub fn new(repo: impl Into<PathBuf>, db: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            db: db.into(),
            modes: vec![SelectionMode::Conservative, SelectionMode::Balanced, SelectionMode::FastFeedback],
            impact: ImpactOptions::default(),
            limits: Limits::default(),
        }
    }
}

/// What the test run at head observed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    /// Test units that failed or errored, by symbol id.
    pub failed: BTreeSet<SymbolId>,
    /// Failures that map to no test unit of head. No selection can name them, so they count as
    /// missed unless the full suite runs.
    pub unmapped_failed: BTreeSet<String>,
    /// Recorded duration of each test unit at head.
    pub durations: BTreeMap<SymbolId, u64>,
}

/// A selection as the tests it would run.
pub struct RunPlan<'a> {
    pub decision: SelectionDecision,
    /// Test units in run order, each once.
    pub order: &'a [SymbolId],
    /// Every test unit in head.
    pub all_units: &'a [SymbolId],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub selected_tests: usize,
    pub total_tests: usize,
    /// `1 - selected/total`; absent when head has no test.
    pub selected_test_reduction: Option<f64>,
    pub failed_tests: usize,
    pub caught_failures: usize,
    /// `caught/failed`; absent when nothing failed.
    pub failing_test_recall: Option<f64>,
    pub missed_failures: Vec<String>,
    /// 1-based position of the first failing test in run order.
    pub first_failure_position: Option<usize>,
    /// Recorded duration of every test up to and including the first failing one.
    pub time_to_first_failure_ms: Option<u64>,
    /// Recorded durations at head; present only when every counted test has one.
    pub selected_runtime_ms: Option<u64>,
    pub full_runtime_ms: Option<u64>,
    pub runtime_reduction: Option<f64>,
}

/// Ratios are rounded to four decimals so the report reads the same everywhere it is rendered.
fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| round4(numerator as f64 / denominator as f64))
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn sum_durations<'a>(units: impl IntoIterator<Item = &'a SymbolId>, observed: &Observed) -> Option<u64> {
    units.into_iter().try_fold(0u64, |acc, unit| observed.durations.get(unit).map(|d| acc.saturating_add(*d)))
}

/// Scores one selection against what failed. Pure.
pub fn score(plan: &RunPlan<'_>, observed: &Observed) -> Score {
    let runs_everything = plan.decision == SelectionDecision::FullSuite;
    let selected: BTreeSet<&SymbolId> = plan.order.iter().collect();
    let mut missed: Vec<String> =
        observed.failed.iter().filter(|f| !selected.contains(f)).map(|f| f.as_str().to_owned()).collect();
    if !runs_everything {
        missed.extend(observed.unmapped_failed.iter().cloned());
    }
    missed.sort();
    let failed_tests = observed.failed.len() + observed.unmapped_failed.len();
    let caught_failures = failed_tests - missed.len();

    let first = plan.order.iter().position(|unit| observed.failed.contains(unit));
    let time_to_first_failure_ms = first.and_then(|i| sum_durations(&plan.order[..=i], observed));
    let selected_runtime_ms = sum_durations(plan.order, observed);
    let full_runtime_ms = sum_durations(plan.all_units, observed);
    let runtime_reduction = match (selected_runtime_ms, full_runtime_ms) {
        (Some(selected), Some(full)) if full > 0 => Some(round4(1.0 - selected as f64 / full as f64)),
        _ => None,
    };
    Score {
        selected_tests: plan.order.len(),
        total_tests: plan.all_units.len(),
        selected_test_reduction: ratio(plan.all_units.len().saturating_sub(plan.order.len()), plan.all_units.len()),
        failed_tests,
        caught_failures,
        failing_test_recall: ratio(caught_failures, failed_tests),
        missed_failures: missed,
        first_failure_position: first.map(|i| i + 1),
        time_to_first_failure_ms,
        selected_runtime_ms,
        full_runtime_ms,
        runtime_reduction,
    }
}

/// Every test unit of `graph`, sorted by id, with the ids of the symbols that contain it.
fn test_units(graph: &CodeGraph) -> Vec<(SymbolId, BTreeSet<SymbolId>)> {
    let mut units: Vec<(SymbolId, BTreeSet<SymbolId>)> = graph
        .symbols()
        .filter(|s| s.is_test && s.kind.is_test_unit())
        .map(|s| {
            let mut owners = BTreeSet::new();
            let mut current = s.parent.clone();
            while let Some(parent) = current {
                current = graph.symbol(&parent).and_then(|p| p.parent.clone());
                if !owners.insert(parent) {
                    break;
                }
            }
            (s.id.clone(), owners)
        })
        .collect();
    units.sort_by(|a, b| a.0.cmp(&b.0));
    units
}

/// The test units a selection runs, in order: each listed test (a container contributes its units
/// by id), then — on a full-suite decision — every other unit by id. Mirrors how
/// `summary.tests_recommended` counts units.
fn run_order(selection: &TestSelection, units: &[(SymbolId, BTreeSet<SymbolId>)]) -> Vec<SymbolId> {
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for listed in &selection.ordered {
        for (unit, owners) in units {
            if (unit == listed || owners.contains(listed)) && seen.insert(unit.clone()) {
                order.push(unit.clone());
            }
        }
    }
    if selection.decision == SelectionDecision::FullSuite {
        for (unit, _) in units {
            if seen.insert(unit.clone()) {
                order.push(unit.clone());
            }
        }
    }
    order
}

fn observe(snapshot: &Snapshot, workdir: Option<&Path>, junit: &[JunitInput]) -> Result<Observed, EvaluationError> {
    let mut observed = Observed::default();
    for input in junit {
        let cases = parse_junit(&input.text)?;
        for MappedCase { key, mapped, case } in map_junit(snapshot, workdir, &cases) {
            let failed = matches!(case.outcome, Outcome::Failed | Outcome::Error);
            if mapped {
                let id = SymbolId::new(key);
                if failed {
                    observed.failed.insert(id.clone());
                }
                let duration = observed.durations.entry(id).or_insert(0);
                *duration = (*duration).max(case.duration_ms);
            } else if failed {
                observed.unmapped_failed.insert(key);
            }
        }
    }
    Ok(observed)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseRevision {
    pub spec: String,
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservedSummary {
    /// Test units of head with a recorded result.
    pub tests_with_results: usize,
    /// Failing tests: symbol ids, then unmapped JUnit keys.
    pub failed: Vec<String>,
    pub unmapped_failed: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModeResult {
    pub mode: SelectionMode,
    pub decision: SelectionDecision,
    /// Codes of the fallback reasons the analysis reported.
    pub fallback_reasons: Vec<String>,
    #[serde(flatten)]
    pub score: Score,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseResult {
    pub name: String,
    pub base: CaseRevision,
    pub head: CaseRevision,
    /// Evidence the analysis could see: recorded at base or its ancestors only.
    pub visible_evidence: EvidenceSummary,
    pub observed: ObservedSummary,
    pub modes: Vec<ModeResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModeAggregate {
    pub mode: SelectionMode,
    pub cases: usize,
    pub failing_cases: usize,
    pub failed_tests: usize,
    pub caught_failures: usize,
    /// Pooled over every failing test of every case.
    pub failing_test_recall: Option<f64>,
    pub missed_failures: usize,
    /// Failing cases in which every failing test was selected.
    pub failing_cases_fully_caught: usize,
    pub full_suite_fallbacks: usize,
    pub mean_selected_test_reduction: Option<f64>,
    /// Cases with a runtime reduction (every test of head had a recorded duration).
    pub runtime_cases: usize,
    pub mean_runtime_reduction: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub cases: usize,
    /// Cases whose run at head had at least one failing test.
    pub failing_cases: usize,
    pub failed_tests: usize,
    /// Present when `failing_cases` is below [`SMALL_SAMPLE_FAILING_CASES`].
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub evaluation_schema_version: u32,
    pub tool_version: String,
    pub sample: Sample,
    pub modes: Vec<ModeAggregate>,
    pub cases: Vec<CaseResult>,
}

fn mean(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.into_iter().fold((0.0, 0usize), |(s, c), v| (s + v, c + 1));
    (count > 0).then(|| round4(sum / count as f64))
}

/// Aggregates per-case results. Pure.
pub fn aggregate(modes: &[SelectionMode], cases: &[CaseResult]) -> (Sample, Vec<ModeAggregate>) {
    let failing_cases = cases.iter().filter(|c| !c.observed.failed.is_empty()).count();
    let sample = Sample {
        cases: cases.len(),
        failing_cases,
        failed_tests: cases.iter().map(|c| c.observed.failed.len()).sum(),
        label: (failing_cases < SMALL_SAMPLE_FAILING_CASES).then(|| SMALL_SAMPLE_LABEL.to_owned()),
    };
    let aggregates = modes
        .iter()
        .map(|&mode| {
            let results: Vec<&ModeResult> =
                cases.iter().filter_map(|c| c.modes.iter().find(|m| m.mode == mode)).collect();
            let failed_tests: usize = results.iter().map(|r| r.score.failed_tests).sum();
            let caught_failures: usize = results.iter().map(|r| r.score.caught_failures).sum();
            let runtime: Vec<f64> = results.iter().filter_map(|r| r.score.runtime_reduction).collect();
            ModeAggregate {
                mode,
                cases: results.len(),
                failing_cases: results.iter().filter(|r| r.score.failed_tests > 0).count(),
                failed_tests,
                caught_failures,
                failing_test_recall: ratio(caught_failures, failed_tests),
                missed_failures: failed_tests - caught_failures,
                failing_cases_fully_caught: results
                    .iter()
                    .filter(|r| r.score.failed_tests > 0 && r.score.missed_failures.is_empty())
                    .count(),
                full_suite_fallbacks: results.iter().filter(|r| r.decision == SelectionDecision::FullSuite).count(),
                mean_selected_test_reduction: mean(results.iter().filter_map(|r| r.score.selected_test_reduction)),
                runtime_cases: runtime.len(),
                mean_runtime_reduction: mean(runtime),
            }
        })
        .collect();
    (sample, aggregates)
}

fn commit_of(repo: &Repo, case: &str, spec: &str) -> Result<CaseRevision, EvaluationError> {
    let commit = repo
        .resolve(spec)?
        .commit
        .ok_or_else(|| EvaluationError::NotACommit { case: case.to_owned(), spec: spec.to_owned() })?;
    Ok(CaseRevision { spec: spec.to_owned(), commit })
}

/// The commits whose evidence may inform the analysis of `base → head`: base and its ancestors.
/// Head, and anything recorded only at head or later, is outside this set by construction.
pub fn visible_commits(repo: &Repo, case: &str, base: &str, head: &str) -> Result<BTreeSet<String>, EvaluationError> {
    let visible = repo.ancestors(base, MAX_ANCESTORS)?;
    if visible.contains(head) {
        return Err(EvaluationError::HeadNotAfterBase { case: case.to_owned() });
    }
    Ok(visible)
}

fn evaluate_case(
    options: &EvaluationOptions,
    repo: &Repo,
    case: &EvaluationCase,
) -> Result<CaseResult, EvaluationError> {
    if case.junit.is_empty() {
        return Err(EvaluationError::NoObservedOutcome { case: case.name.clone() });
    }
    let base = commit_of(repo, &case.name, &case.base)?;
    let head = commit_of(repo, &case.name, &case.head)?;
    let visible = visible_commits(repo, &case.name, &base.commit, &head.commit)?;

    // The database doubles as the fact cache; parsed facts are code structure, not test evidence.
    let snapshot = {
        let mut cache = FactCache::with_store(Store::open(&options.db)?);
        let snapshot = build_snapshot(repo, repo.resolve(&head.commit)?, &mut cache, &options.limits)?;
        cache.flush()?;
        snapshot
    };
    let units = test_units(&snapshot.graph);
    let all_units: Vec<SymbolId> = units.iter().map(|(id, _)| id.clone()).collect();
    let observed = observe(&snapshot, repo.workdir().or(Some(&options.repo)), &case.junit)?;

    let mut visible_evidence = EvidenceSummary::default();
    let mut modes = Vec::with_capacity(options.modes.len());
    for &mode in &options.modes {
        let mut analysis = AnalyzeOptions::new(&options.repo, &base.commit, &head.commit);
        analysis.db = Some(options.db.clone());
        analysis.mode = Some(mode);
        analysis.impact = options.impact;
        analysis.limits = options.limits;
        analysis.evidence_commits = Some(visible.clone());
        let report = analyze(&analysis)?;
        let order = run_order(&report.test_selection, &units);
        let plan = RunPlan { decision: report.test_selection.decision, order: &order, all_units: &all_units };
        let mut codes: Vec<String> = report.test_selection.fallback_reasons.iter().map(|r| r.code.clone()).collect();
        codes.sort();
        codes.dedup();
        modes.push(ModeResult {
            mode,
            decision: report.test_selection.decision,
            fallback_reasons: codes,
            score: score(&plan, &observed),
        });
        visible_evidence = report.evidence;
    }

    let mut failed: Vec<String> = observed.failed.iter().map(|id| id.as_str().to_owned()).collect();
    failed.extend(observed.unmapped_failed.iter().cloned());
    Ok(CaseResult {
        name: case.name.clone(),
        base,
        head,
        visible_evidence,
        observed: ObservedSummary {
            tests_with_results: observed.durations.len(),
            failed,
            unmapped_failed: observed.unmapped_failed.len(),
        },
        modes,
    })
}

/// Replays `cases` in order. Evidence for each case is restricted to commits that are base or its
/// ancestors, whatever the database holds.
pub fn evaluate(options: &EvaluationOptions, cases: &[EvaluationCase]) -> Result<EvaluationReport, EvaluationError> {
    if cases.is_empty() {
        return Err(EvaluationError::NoCases);
    }
    if cases.len() > MAX_CASES {
        return Err(EvaluationError::TooManyCases { count: cases.len(), limit: MAX_CASES });
    }
    let repo = Repo::open(&options.repo)?;
    let results = cases.iter().map(|case| evaluate_case(options, &repo, case)).collect::<Result<Vec<_>, _>>()?;
    let (sample, modes) = aggregate(&options.modes, &results);
    Ok(EvaluationReport {
        evaluation_schema_version: EVALUATION_SCHEMA_VERSION,
        tool_version: TOOL_VERSION.to_owned(),
        sample,
        modes,
        cases: results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<SymbolId> {
        names.iter().map(|n| SymbolId::new(*n)).collect()
    }

    fn observed(failed: &[&str], durations: &[(&str, u64)]) -> Observed {
        Observed {
            failed: failed.iter().map(|f| SymbolId::new(*f)).collect(),
            unmapped_failed: BTreeSet::new(),
            durations: durations.iter().map(|(id, d)| (SymbolId::new(*id), *d)).collect(),
        }
    }

    fn plan<'a>(decision: SelectionDecision, order: &'a [SymbolId], all: &'a [SymbolId]) -> RunPlan<'a> {
        RunPlan { decision, order, all_units: all }
    }

    #[test]
    fn no_failures_means_no_recall_and_no_first_failure() {
        let all = ids(&["a", "b", "c", "d"]);
        let order = ids(&["b"]);
        let s = score(
            &plan(SelectionDecision::Selected, &order, &all),
            &observed(&[], &[("a", 1), ("b", 2), ("c", 3), ("d", 4)]),
        );
        assert_eq!((s.failed_tests, s.caught_failures), (0, 0));
        assert_eq!(s.failing_test_recall, None, "recall is undefined without failures, not 100%");
        assert_eq!((s.first_failure_position, s.time_to_first_failure_ms), (None, None));
        assert_eq!(s.selected_test_reduction, Some(0.75));
        assert_eq!((s.selected_runtime_ms, s.full_runtime_ms), (Some(2), Some(10)));
        assert_eq!(s.runtime_reduction, Some(0.8));
    }

    #[test]
    fn every_failure_missed() {
        let all = ids(&["a", "b", "c"]);
        let order = ids(&["a"]);
        let s = score(&plan(SelectionDecision::Selected, &order, &all), &observed(&["c", "b"], &[]));
        assert_eq!((s.failed_tests, s.caught_failures), (2, 0));
        assert_eq!(s.failing_test_recall, Some(0.0));
        assert_eq!(s.missed_failures, vec!["b", "c"]);
        assert_eq!(s.first_failure_position, None);
    }

    #[test]
    fn first_failure_is_the_earliest_in_run_order_and_its_time_is_cumulative() {
        let all = ids(&["a", "b", "c", "d"]);
        // Two failures; run order, not id order, decides which is first.
        let order = ids(&["d", "c", "b"]);
        let s = score(
            &plan(SelectionDecision::Selected, &order, &all),
            &observed(&["b", "c"], &[("a", 5), ("b", 7), ("c", 11), ("d", 13)]),
        );
        assert_eq!(s.first_failure_position, Some(2));
        assert_eq!(s.time_to_first_failure_ms, Some(24));
        assert_eq!(s.failing_test_recall, Some(1.0));
    }

    #[test]
    fn unknown_durations_leave_runtime_absent_not_zero() {
        let all = ids(&["a", "b", "c"]);
        let order = ids(&["a", "b"]);
        let s = score(&plan(SelectionDecision::Selected, &order, &all), &observed(&["b"], &[("b", 3), ("c", 4)]));
        assert_eq!(s.first_failure_position, Some(2));
        assert_eq!(s.time_to_first_failure_ms, None, "a has no recorded duration");
        assert_eq!((s.selected_runtime_ms, s.full_runtime_ms, s.runtime_reduction), (None, None, None));
        // All durations zero: a reduction of 0/0 is not claimed.
        let zero =
            score(&plan(SelectionDecision::Selected, &order, &all), &observed(&[], &[("a", 0), ("b", 0), ("c", 0)]));
        assert_eq!((zero.full_runtime_ms, zero.runtime_reduction), (Some(0), None));
    }

    #[test]
    fn unmapped_failures_are_missed_unless_the_full_suite_runs() {
        let all = ids(&["a", "b"]);
        let mut obs = observed(&["a"], &[]);
        obs.unmapped_failed.insert("junit:Gone#test()".into());
        let order = ids(&["a"]);
        let selected = score(&plan(SelectionDecision::Selected, &order, &all), &obs);
        assert_eq!((selected.failed_tests, selected.caught_failures), (2, 1));
        assert_eq!(selected.missed_failures, vec!["junit:Gone#test()"]);
        assert_eq!(selected.failing_test_recall, Some(0.5));
        let full = score(&plan(SelectionDecision::FullSuite, &all, &all), &obs);
        assert_eq!((full.caught_failures, full.failing_test_recall), (2, Some(1.0)));
        assert_eq!(full.selected_test_reduction, Some(0.0));
    }

    #[test]
    fn ratios_are_rounded_and_empty_suites_have_no_reduction() {
        let all = ids(&["a", "b", "c"]);
        let order = ids(&["a"]);
        let s = score(&plan(SelectionDecision::Selected, &order, &all), &observed(&[], &[]));
        assert_eq!(s.selected_test_reduction, Some(0.6667));
        let none = score(&plan(SelectionDecision::Selected, &[], &[]), &observed(&[], &[]));
        assert_eq!(none.selected_test_reduction, None);
    }

    #[test]
    fn containers_expand_to_their_units_and_full_suite_appends_the_rest() {
        let units = vec![
            (SymbolId::new("C#a"), BTreeSet::from([SymbolId::new("C")])),
            (SymbolId::new("C#b"), BTreeSet::from([SymbolId::new("C")])),
            (SymbolId::new("D#c"), BTreeSet::from([SymbolId::new("D")])),
            (SymbolId::new("E#d"), BTreeSet::from([SymbolId::new("E")])),
        ];
        let selection = |decision, ordered: &[&str]| TestSelection {
            mode: SelectionMode::Balanced,
            decision,
            fallback_reasons: Vec::new(),
            ordered: ids(ordered),
            selected_units: 0,
            total_units: 4,
            selected_runtime_ms: None,
            full_runtime_ms: None,
            notes: Vec::new(),
        };
        // A unit listed before its container is not run twice.
        let order = run_order(&selection(SelectionDecision::Selected, &["D#c", "C", "D"]), &units);
        assert_eq!(order, ids(&["D#c", "C#a", "C#b"]));
        let full = run_order(&selection(SelectionDecision::FullSuite, &["D#c"]), &units);
        assert_eq!(full, ids(&["D#c", "C#a", "C#b", "E#d"]));
    }

    fn case(name: &str, failed: usize, results: Vec<ModeResult>) -> CaseResult {
        let revision = CaseRevision { spec: "x".into(), commit: "x".into() };
        CaseResult {
            name: name.into(),
            base: revision.clone(),
            head: revision,
            visible_evidence: EvidenceSummary::default(),
            observed: ObservedSummary {
                tests_with_results: 3,
                failed: (0..failed).map(|i| format!("t{i}")).collect(),
                unmapped_failed: 0,
            },
            modes: results,
        }
    }

    fn mode_result(decision: SelectionDecision, failed: usize, caught: usize, reduction: f64) -> ModeResult {
        ModeResult {
            mode: SelectionMode::Balanced,
            decision,
            fallback_reasons: Vec::new(),
            score: Score {
                selected_tests: 1,
                total_tests: 3,
                selected_test_reduction: Some(reduction),
                failed_tests: failed,
                caught_failures: caught,
                failing_test_recall: ratio(caught, failed),
                missed_failures: (caught..failed).map(|i| format!("t{i}")).collect(),
                first_failure_position: None,
                time_to_first_failure_ms: None,
                selected_runtime_ms: None,
                full_runtime_ms: None,
                runtime_reduction: None,
            },
        }
    }

    #[test]
    fn aggregate_pools_failures_and_labels_small_samples() {
        let cases = vec![
            case("one", 0, vec![mode_result(SelectionDecision::Selected, 0, 0, 0.5)]),
            case("two", 2, vec![mode_result(SelectionDecision::Selected, 2, 1, 0.25)]),
            case("three", 1, vec![mode_result(SelectionDecision::FullSuite, 1, 1, 0.0)]),
        ];
        let (sample, modes) = aggregate(&[SelectionMode::Balanced], &cases);
        assert_eq!((sample.cases, sample.failing_cases, sample.failed_tests), (3, 2, 3));
        assert_eq!(sample.label.as_deref(), Some(SMALL_SAMPLE_LABEL));
        let balanced = &modes[0];
        assert_eq!((balanced.failed_tests, balanced.caught_failures, balanced.missed_failures), (3, 2, 1));
        assert_eq!(balanced.failing_test_recall, Some(0.6667));
        assert_eq!((balanced.failing_cases, balanced.failing_cases_fully_caught), (2, 1));
        assert_eq!(balanced.full_suite_fallbacks, 1);
        assert_eq!(balanced.mean_selected_test_reduction, Some(0.25));
        assert_eq!((balanced.runtime_cases, balanced.mean_runtime_reduction), (0, None));

        let many: Vec<CaseResult> = (0..SMALL_SAMPLE_FAILING_CASES)
            .map(|i| case(&i.to_string(), 1, vec![mode_result(SelectionDecision::Selected, 1, 1, 0.5)]))
            .collect();
        assert_eq!(aggregate(&[SelectionMode::Balanced], &many).0.label, None);
    }
}
