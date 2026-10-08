//! Test reliability from recorded CI results. Pure.
//!
//! The strong signal for flakiness is *the same code producing different outcomes*: two runs at one
//! commit that disagree, or a failure that passed on retry within one run. A test that fails often
//! but always at commits where it fails consistently is broken (or catching real bugs), not flaky.

use std::collections::{BTreeMap, BTreeSet};

use ripplepath_storage::HistoryEntry;
use serde::{Deserialize, Serialize};

/// Fewer recorded runs than this, without a same-commit flip, cannot support a classification.
pub const MIN_RUNS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Reliability {
    Stable,
    Flaky,
    ConsistentlyFailing,
    InsufficientData,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestHistory {
    pub runs: usize,
    pub failures: usize,
    /// Commits at which this test both passed and failed (including pass-after-retry).
    pub flaky_commits: usize,
    pub median_duration_ms: Option<u64>,
    pub last_outcome: Option<String>,
    pub reliability: Reliability,
}

fn failed(outcome: &str) -> bool {
    matches!(outcome, "FAILED" | "ERROR")
}

pub fn summarize(entries: &[HistoryEntry]) -> TestHistory {
    let executed: Vec<&HistoryEntry> = entries.iter().filter(|e| e.outcome != "SKIPPED").collect();
    let failures = executed.iter().filter(|e| failed(&e.outcome)).count();

    let mut outcomes_by_commit: BTreeMap<&str, BTreeSet<bool>> = BTreeMap::new();
    for entry in &executed {
        let set = outcomes_by_commit.entry(entry.commit.as_str()).or_default();
        set.insert(failed(&entry.outcome));
        // Failed attempts before a final pass are a flip inside a single run.
        if entry.failed_attempts > 0 && !failed(&entry.outcome) {
            set.insert(true);
        }
    }
    let flaky_commits = outcomes_by_commit.values().filter(|s| s.len() > 1).count();

    let mut durations: Vec<u64> = executed.iter().map(|e| e.duration_ms).collect();
    durations.sort_unstable();
    let median_duration_ms = (!durations.is_empty()).then(|| durations[durations.len() / 2]);

    let last = executed.last();
    let reliability = if flaky_commits > 0 {
        Reliability::Flaky
    } else if executed.len() < MIN_RUNS {
        Reliability::InsufficientData
    } else if let Some(latest) = last {
        // Every run at the most recent commit failed: broken at that commit.
        let latest_runs: Vec<&&HistoryEntry> = executed.iter().filter(|e| e.commit == latest.commit).collect();
        if latest_runs.iter().all(|e| failed(&e.outcome)) {
            Reliability::ConsistentlyFailing
        } else {
            Reliability::Stable
        }
    } else {
        Reliability::InsufficientData
    };

    TestHistory {
        runs: executed.len(),
        failures,
        flaky_commits,
        median_duration_ms,
        last_outcome: last.map(|e| e.outcome.clone()),
        reliability,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: i64, commit: &str, outcome: &str) -> HistoryEntry {
        HistoryEntry {
            run_id: id,
            commit: commit.into(),
            outcome: outcome.into(),
            duration_ms: 10 * id as u64,
            failed_attempts: 0,
        }
    }

    #[test]
    fn same_commit_flip_is_flaky_even_with_few_runs() {
        let h = summarize(&[run(1, "a", "PASSED"), run(2, "a", "FAILED")]);
        assert_eq!(h.reliability, Reliability::Flaky);
        assert_eq!(h.flaky_commits, 1);
    }

    #[test]
    fn retry_pass_is_flaky() {
        let mut entry = run(1, "a", "PASSED");
        entry.failed_attempts = 2;
        assert_eq!(summarize(&[entry]).reliability, Reliability::Flaky);
    }

    #[test]
    fn frequent_failures_at_distinct_commits_are_not_flaky() {
        let h =
            summarize(&[run(1, "a", "FAILED"), run(2, "b", "PASSED"), run(3, "c", "FAILED"), run(4, "c", "FAILED")]);
        assert_eq!(h.reliability, Reliability::ConsistentlyFailing);
        let fixed = summarize(&[run(1, "a", "FAILED"), run(2, "b", "FAILED"), run(3, "c", "PASSED")]);
        assert_eq!(fixed.reliability, Reliability::Stable);
    }

    #[test]
    fn small_samples_are_labelled_and_skips_ignored() {
        let h = summarize(&[run(1, "a", "PASSED"), run(2, "b", "SKIPPED")]);
        assert_eq!(h.reliability, Reliability::InsufficientData);
        assert_eq!(h.runs, 1);
        assert_eq!(h.median_duration_ms, Some(10));
    }
}
