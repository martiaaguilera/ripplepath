//! Persisted test evidence as seen by one analysis: coverage edges, coverage status, test history.

use std::collections::{BTreeMap, BTreeSet};

use ripplepath_core::{Edge, EdgeKind, Evidence, Symbol, SymbolId};
use ripplepath_graph::CodeGraph;
use ripplepath_storage::{HistoryEntry, StorageError, Store, StoredCoverage};

use crate::evidence::coverable;
use crate::history::{TestHistory, summarize};
use crate::report::{CoverageStatus, EvidenceSummary};

#[derive(Default)]
pub(crate) struct LoadedEvidence {
    coverage: Vec<StoredCoverage>,
    covered: BTreeSet<String>,
    measured_files: BTreeSet<String>,
    history: BTreeMap<String, Vec<HistoryEntry>>,
    pub(crate) summary: EvidenceSummary,
}

impl LoadedEvidence {
    pub(crate) fn load(store: &Store) -> Result<Self, StorageError> {
        let coverage = store.latest_coverage()?;
        let history = store.test_history()?;
        let covered = coverage.iter().flat_map(|r| r.covered.iter().cloned()).collect();
        let measured_files = coverage.iter().flat_map(|r| r.files.iter().cloned()).collect();
        let runs: BTreeSet<i64> = history.values().flatten().map(|h| h.run_id).collect();
        let summary = EvidenceSummary {
            coverage_reports: coverage.len(),
            test_runs: runs.len(),
            tests_with_history: history.len(),
            ..EvidenceSummary::default()
        };
        Ok(Self { coverage, covered, measured_files, history, summary })
    }

    pub(crate) fn has_coverage(&self) -> bool {
        !self.coverage.is_empty()
    }

    /// `TESTS` edges from each test whose execution was measured to the code it executed, limited
    /// to symbols that exist in `graph`. Coverage of test code itself is dropped: a test executing
    /// its own body is not evidence about the change.
    pub(crate) fn coverage_edges(&self, graph: &CodeGraph) -> Vec<Edge> {
        let mut edges = Vec::new();
        for report in &self.coverage {
            let Some(test) = report.test_symbol.as_ref().map(SymbolId::new) else {
                continue;
            };
            let Some(test_symbol) = graph.symbol(&test) else {
                continue;
            };
            let commit: String = report.commit.chars().take(10).collect();
            for covered in &report.covered {
                let target = SymbolId::new(covered.as_str());
                match graph.symbol(&target) {
                    Some(symbol) if !symbol.is_test && target != test => edges.push(Edge {
                        from: test.clone(),
                        to: target,
                        kind: EdgeKind::Tests,
                        evidence: Evidence::CoverageObserved,
                        file: test_symbol.file.clone(),
                        line: test_symbol.span.start_line,
                        rule: format!("coverage.{}@{commit}", report.format),
                    }),
                    _ => {}
                }
            }
        }
        edges.sort();
        edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);
        edges
    }

    pub(crate) fn mark_other_commits(&mut self, base: Option<&str>, head: Option<&str>) {
        self.summary.coverage_reports_other_commits =
            self.coverage.iter().filter(|r| Some(r.commit.as_str()) != base && Some(r.commit.as_str()) != head).count();
    }

    pub(crate) fn coverage_status(&self, symbol: &Symbol) -> Option<CoverageStatus> {
        if !self.has_coverage() || !coverable(symbol.kind) {
            return None;
        }
        Some(if self.covered.contains(symbol.id.as_str()) {
            CoverageStatus::Covered
        } else if self.measured_files.contains(&symbol.file) {
            CoverageStatus::NotCovered
        } else {
            CoverageStatus::NoData
        })
    }

    pub(crate) fn history(&self, test: &SymbolId) -> Option<TestHistory> {
        self.history.get(test.as_str()).map(|entries| summarize(entries))
    }
}
