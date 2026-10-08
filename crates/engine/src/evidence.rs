//! Ingestion of test evidence: coverage reports and JUnit results, mapped onto the symbols of the
//! revision they were measured at.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use ripplepath_core::{SymbolId, SymbolKind};
use ripplepath_evidence::{CoverageReport, Outcome, TestCaseResult, parse_jacoco, parse_junit, parse_lcov};
use ripplepath_git::Repo;
use ripplepath_storage::{Store, StoredCoverage, StoredResult};

use crate::changes::LineIndex;
use crate::snapshot::{FactCache, Snapshot, build_snapshot};
use crate::{AnalysisError, Limits};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageFormat {
    Jacoco,
    Lcov,
}

#[derive(Clone, Debug, Default)]
pub struct IngestOutcome {
    pub commit: String,
    pub reports: usize,
    /// Files (coverage) or test cases (JUnit) mapped onto the indexed code.
    pub mapped: usize,
    pub unmapped: usize,
    /// A few unmapped names, so a path-root or naming mismatch is diagnosable.
    pub unmapped_examples: Vec<String>,
    pub covered_symbols: usize,
}

const UNMAPPED_EXAMPLES: usize = 5;

fn open_snapshot(
    repo_path: &Path,
    revision: &str,
    db: &Path,
    limits: &Limits,
) -> Result<(Snapshot, Store, String), AnalysisError> {
    let repo = Repo::open(repo_path)?;
    let resolved = repo.resolve(revision)?;
    let commit = resolved.commit.clone().unwrap_or_else(|| resolved.tree.to_string());
    let mut cache = FactCache::with_store(Store::open(db)?);
    let snapshot = build_snapshot(&repo, resolved, &mut cache, limits)?;
    let store = cache.into_store().ok_or(AnalysisError::NoStore)?;
    Ok((snapshot, store, commit))
}

/// Maps a path as a tool wrote it onto a repository path.
///
/// Tools write absolute paths (LCOV), paths relative to some other root, or package-relative paths
/// (JaCoCo: `com/acme/A.java`). Absolute paths under the work tree are made relative; anything else
/// matches a repository path by suffix on a path-component boundary, and only when exactly one
/// file matches — a guess between two candidates would attribute coverage to the wrong code.
pub(crate) struct PathMapper<'a> {
    files: Vec<&'a str>,
    workdir: Option<String>,
}

fn normalize(raw: &str) -> String {
    raw.replace('\\', "/").trim_start_matches("./").to_owned()
}

impl<'a> PathMapper<'a> {
    pub(crate) fn new(snapshot: &'a Snapshot, workdir: Option<&Path>) -> Self {
        let workdir = workdir
            .and_then(|w| w.canonicalize().ok().or_else(|| Some(w.to_path_buf())))
            .map(|w| normalize(&w.to_string_lossy()).trim_start_matches("//?/").trim_end_matches('/').to_owned());
        Self { files: snapshot.files.iter().map(|f| f.path.as_str()).collect(), workdir }
    }

    pub(crate) fn map(&self, raw: &str) -> Option<&'a str> {
        let mut path = normalize(raw);
        if let Some(root) = &self.workdir {
            let lower = path.to_ascii_lowercase();
            let root_lower = root.to_ascii_lowercase();
            if lower.starts_with(&format!("{root_lower}/")) {
                path = path[root.len() + 1..].to_owned();
            }
        }
        if let Some(exact) = self.files.iter().find(|f| **f == path) {
            return Some(exact);
        }
        let suffix = format!("/{path}");
        let mut matches = self.files.iter().filter(|f| f.ends_with(&suffix));
        let first = matches.next()?;
        matches.next().is_none().then_some(*first)
    }
}

/// Resolves a test selector: a symbol id, a repository file path, or a Java class name.
fn resolve_test(snapshot: &Snapshot, mapper: &PathMapper<'_>, selector: &str) -> Option<SymbolId> {
    let graph = &snapshot.graph;
    let direct = SymbolId::new(selector);
    if graph.contains(&direct) {
        return Some(direct);
    }
    if let Some(path) = mapper.map(selector) {
        let file = SymbolId::file(path);
        if graph.contains(&file) {
            return Some(file);
        }
    }
    let java = SymbolId::new(format!("java:{selector}"));
    graph.contains(&java).then_some(java)
}

/// One coverage file to ingest.
pub struct CoverageInput<'a> {
    pub format: CoverageFormat,
    pub text: &'a str,
    /// Where it came from (file name), recorded for traceability.
    pub source: &'a str,
    /// The test that produced it, when known (see [`resolve_test`]).
    pub test: Option<&'a str>,
}

pub fn ingest_coverage(
    repo_path: &Path,
    revision: &str,
    db: &Path,
    coverage: CoverageInput<'_>,
    limits: &Limits,
) -> Result<IngestOutcome, AnalysisError> {
    let CoverageInput { format, text: input, source, test } = coverage;
    let reports: Vec<CoverageReport> = match format {
        CoverageFormat::Jacoco => vec![parse_jacoco(input)?],
        CoverageFormat::Lcov => parse_lcov(input)?,
    };
    let (snapshot, mut store, commit) = open_snapshot(repo_path, revision, db, limits)?;
    let workdir = Repo::open(repo_path)?.workdir().map(Path::to_path_buf);
    let mapper = PathMapper::new(&snapshot, workdir.as_deref().or(Some(repo_path)));
    let format_name = match format {
        CoverageFormat::Jacoco => "jacoco",
        CoverageFormat::Lcov => "lcov",
    };

    let mut outcome = IngestOutcome { commit: commit.clone(), ..IngestOutcome::default() };
    for report in reports {
        // An explicit `--test` wins; otherwise LCOV's TN names the test; otherwise aggregate.
        let selector = test.map(str::to_owned).or(report.test.clone());
        let test_symbol = match selector {
            Some(selector) => Some(
                resolve_test(&snapshot, &mapper, &selector)
                    .ok_or_else(|| AnalysisError::UnknownTest(selector.clone()))?,
            ),
            None => None,
        };
        let mut files = BTreeSet::new();
        let mut covered = BTreeSet::new();
        let mut unmapped = 0u32;
        for file in &report.files {
            let Some(path) = mapper.map(&file.path) else {
                unmapped += 1;
                if outcome.unmapped_examples.len() < UNMAPPED_EXAMPLES {
                    outcome.unmapped_examples.push(file.path.clone());
                }
                continue;
            };
            files.insert(path.to_owned());
            let index = LineIndex::new(&snapshot, path);
            for (&line, &hit) in &file.lines {
                if hit {
                    covered.extend(index.symbols_for(line, 1).into_iter().map(|id| id.as_str().to_owned()));
                }
            }
        }
        outcome.reports += 1;
        outcome.mapped += files.len();
        outcome.unmapped += unmapped as usize;
        outcome.covered_symbols += covered.len();
        store.add_coverage(&StoredCoverage {
            commit: commit.clone(),
            format: format_name.to_owned(),
            test_symbol: test_symbol.map(|s| s.as_str().to_owned()),
            source: source.to_owned(),
            unmapped_files: unmapped,
            files,
            covered,
        })?;
    }
    Ok(outcome)
}

/// Lookup from JUnit (classname, name) to test symbols.
struct TestIndex<'a> {
    /// `java:<class>#<method>` (parameters stripped) → symbol ids
    java: HashMap<String, Vec<&'a SymbolId>>,
}

impl<'a> TestIndex<'a> {
    fn new(snapshot: &'a Snapshot) -> Self {
        let mut java: HashMap<String, Vec<&SymbolId>> = HashMap::new();
        for symbol in snapshot.graph.symbols().filter(|s| s.is_test && s.kind.is_test_unit()) {
            if let Some((prefix, _)) = symbol.id.as_str().split_once('(')
                && prefix.starts_with("java:")
            {
                java.entry(prefix.to_owned()).or_default().push(&symbol.id);
            }
        }
        Self { java }
    }

    fn map(&self, snapshot: &Snapshot, mapper: &PathMapper<'_>, case: &TestCaseResult) -> Option<SymbolId> {
        // TS reporters (Vitest, Jest) name the file in `classname` or `file` and the full title in `name`.
        for candidate in [case.file.as_deref(), Some(case.classname.as_str())].into_iter().flatten() {
            if let Some(path) = mapper.map(candidate) {
                let id = SymbolId::new(format!("ts:{path}#test:{}", case.name));
                if snapshot.graph.contains(&id) {
                    return Some(id);
                }
            }
        }
        // JUnit Platform: classname = FQN, name = `method()` or `method(Type)`.
        let method = case.name.split('(').next().unwrap_or(&case.name);
        match self.java.get(&format!("java:{}#{method}", case.classname)).map(Vec::as_slice) {
            Some([only]) => Some((*only).clone()),
            _ => None,
        }
    }
}

pub fn ingest_junit(
    repo_path: &Path,
    revision: &str,
    db: &Path,
    input: &str,
    source: &str,
    limits: &Limits,
) -> Result<IngestOutcome, AnalysisError> {
    let cases = parse_junit(input)?;
    let (snapshot, mut store, commit) = open_snapshot(repo_path, revision, db, limits)?;
    let workdir = Repo::open(repo_path)?.workdir().map(Path::to_path_buf);
    let mapper = PathMapper::new(&snapshot, workdir.as_deref().or(Some(repo_path)));
    let index = TestIndex::new(&snapshot);

    let mut outcome = IngestOutcome { commit: commit.clone(), reports: 1, ..IngestOutcome::default() };
    let mut results: BTreeMap<String, StoredResult> = BTreeMap::new();
    for case in &cases {
        let mapped = index.map(&snapshot, &mapper, case);
        let key = match &mapped {
            Some(id) => {
                outcome.mapped += 1;
                id.as_str().to_owned()
            }
            None => {
                outcome.unmapped += 1;
                let key = format!("junit:{}#{}", case.classname, case.name);
                if outcome.unmapped_examples.len() < UNMAPPED_EXAMPLES {
                    outcome.unmapped_examples.push(key.clone());
                }
                key
            }
        };
        let outcome_text = match case.outcome {
            Outcome::Passed => "PASSED",
            Outcome::Failed => "FAILED",
            Outcome::Error => "ERROR",
            Outcome::Skipped => "SKIPPED",
        };
        results.insert(
            key.clone(),
            StoredResult {
                test_key: key,
                mapped: mapped.is_some(),
                outcome: outcome_text.to_owned(),
                duration_ms: case.duration_ms,
                failed_attempts: case.failed_attempts,
                failure_fingerprint: case.failure_fingerprint.clone(),
            },
        );
    }
    store.add_test_run(&commit, source, &results.into_values().collect::<Vec<_>>())?;
    Ok(outcome)
}

/// Whether coverage data could say anything about `kind` (declarations without executable code,
/// like interfaces, never show up as covered lines).
pub(crate) fn coverable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Method | SymbolKind::Constructor | SymbolKind::Function | SymbolKind::Field | SymbolKind::Variable
    )
}
