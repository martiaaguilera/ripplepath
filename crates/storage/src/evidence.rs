//! Persisted test evidence.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::params;

use crate::{StorageError, Store};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredResult {
    pub test_key: String,
    pub mapped: bool,
    /// `PASSED`, `FAILED`, `ERROR`, `SKIPPED`.
    pub outcome: String,
    pub duration_ms: u64,
    pub failed_attempts: u32,
    pub failure_fingerprint: Option<String>,
}

/// One recorded execution of one test, in ingestion order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub run_id: i64,
    pub commit: String,
    pub outcome: String,
    pub duration_ms: u64,
    pub failed_attempts: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredCoverage {
    pub commit: String,
    pub format: String,
    pub test_symbol: Option<String>,
    pub source: String,
    pub unmapped_files: u32,
    pub files: BTreeSet<String>,
    pub covered: BTreeSet<String>,
}

impl Store {
    pub fn add_test_run(&mut self, commit: &str, source: &str, results: &[StoredResult]) -> Result<i64, StorageError> {
        let tx = self.conn_mut().transaction()?;
        tx.execute("INSERT INTO test_runs (commit_id, source) VALUES (?1, ?2)", params![commit, source])?;
        let run = tx.last_insert_rowid();
        {
            // A report can list the same test twice (parameterised display names collapsing to
            // one key); the last entry wins, like the console output the developer saw.
            let mut insert = tx.prepare_cached(
                "INSERT OR REPLACE INTO test_results (run_id, test_key, mapped, outcome, duration_ms, failed_attempts, failure_fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for r in results {
                insert.execute(params![
                    run,
                    r.test_key,
                    r.mapped,
                    r.outcome,
                    r.duration_ms as i64,
                    r.failed_attempts,
                    r.failure_fingerprint
                ])?;
            }
        }
        tx.commit()?;
        Ok(run)
    }

    /// Every recorded execution, grouped by test key, oldest first.
    pub fn test_history(&self) -> Result<BTreeMap<String, Vec<HistoryEntry>>, StorageError> {
        let mut stmt = self.conn().prepare(
            "SELECT r.test_key, t.id, t.commit_id, r.outcome, r.duration_ms, r.failed_attempts
             FROM test_results r JOIN test_runs t ON t.id = r.run_id ORDER BY t.id, r.test_key",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                HistoryEntry {
                    run_id: r.get(1)?,
                    commit: r.get(2)?,
                    outcome: r.get(3)?,
                    duration_ms: r.get::<_, i64>(4)?.max(0) as u64,
                    failed_attempts: r.get(5)?,
                },
            ))
        })?;
        let mut out: BTreeMap<String, Vec<HistoryEntry>> = BTreeMap::new();
        for row in rows {
            let (key, entry) = row?;
            out.entry(key).or_default().push(entry);
        }
        Ok(out)
    }

    pub fn add_coverage(&mut self, report: &StoredCoverage) -> Result<i64, StorageError> {
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "INSERT INTO coverage_reports (commit_id, format, test_symbol, source, unmapped_files) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![report.commit, report.format, report.test_symbol, report.source, report.unmapped_files],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut file = tx.prepare_cached("INSERT INTO coverage_files (report_id, path) VALUES (?1, ?2)")?;
            for path in &report.files {
                file.execute(params![id, path])?;
            }
            let mut symbol = tx.prepare_cached("INSERT INTO covered_symbols (report_id, symbol_id) VALUES (?1, ?2)")?;
            for covered in &report.covered {
                symbol.execute(params![id, covered])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// The most recent report per test symbol (and the most recent aggregate report). Older
    /// reports for the same test are superseded: coverage describes the code as it was measured.
    pub fn latest_coverage(&self) -> Result<Vec<StoredCoverage>, StorageError> {
        self.latest_coverage_among(None)
    }

    /// Like [`Store::latest_coverage`], but only reports measured at one of `commits` are
    /// candidates (all reports with `None`). The restriction applies *before* choosing the latest
    /// report per test, so a newer report from an excluded commit cannot hide an older visible one.
    pub fn latest_coverage_among(
        &self,
        commits: Option<&BTreeSet<String>>,
    ) -> Result<Vec<StoredCoverage>, StorageError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, commit_id, format, test_symbol, source, unmapped_files FROM coverage_reports ORDER BY id",
        )?;
        type Row = (i64, String, String, Option<String>, String, u32);
        let rows: Vec<Row> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<Result<_, _>>()?;
        // Rows are in ingestion order, so the last one kept per test is the latest. `None` (the
        // aggregate report) sorts first, as NULL does in SQLite.
        let mut latest: BTreeMap<Option<String>, Row> = BTreeMap::new();
        for row in rows {
            if commits.is_none_or(|allowed| allowed.contains(&row.1)) {
                latest.insert(row.3.clone(), row);
            }
        }
        let reports: Vec<Row> = latest.into_values().collect();
        let mut files_stmt = conn.prepare("SELECT path FROM coverage_files WHERE report_id = ?1")?;
        let mut covered_stmt = conn.prepare("SELECT symbol_id FROM covered_symbols WHERE report_id = ?1")?;
        let mut out = Vec::with_capacity(reports.len());
        for (id, commit, format, test_symbol, source, unmapped_files) in reports {
            let files = files_stmt.query_map([id], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
            let covered = covered_stmt.query_map([id], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
            out.push(StoredCoverage { commit, format, test_symbol, source, unmapped_files, files, covered });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn result(key: &str, outcome: &str) -> StoredResult {
        StoredResult {
            test_key: key.into(),
            mapped: true,
            outcome: outcome.into(),
            duration_ms: 10,
            failed_attempts: 0,
            failure_fingerprint: None,
        }
    }

    #[test]
    fn history_is_grouped_and_ordered() {
        let mut store = Store::open_in_memory().unwrap();
        store.add_test_run("c1", "a.xml", &[result("t1", "PASSED"), result("t2", "FAILED")]).unwrap();
        store.add_test_run("c1", "b.xml", &[result("t1", "FAILED")]).unwrap();
        let history = store.test_history().unwrap();
        let t1: Vec<&str> = history["t1"].iter().map(|h| h.outcome.as_str()).collect();
        assert_eq!(t1, vec!["PASSED", "FAILED"]);
        assert_eq!(history["t2"].len(), 1);
    }

    #[test]
    fn latest_coverage_supersedes_older_reports_per_test() {
        let mut store = Store::open_in_memory().unwrap();
        let report = |test: Option<&str>, covered: &[&str]| StoredCoverage {
            commit: "c".into(),
            format: "lcov".into(),
            test_symbol: test.map(str::to_owned),
            source: "x".into(),
            unmapped_files: 0,
            files: BTreeSet::from(["a.ts".to_owned()]),
            covered: covered.iter().map(|s| (*s).to_owned()).collect(),
        };
        store.add_coverage(&report(Some("t"), &["old"])).unwrap();
        store.add_coverage(&report(Some("t"), &["new"])).unwrap();
        store.add_coverage(&report(None, &["agg"])).unwrap();
        let latest = store.latest_coverage().unwrap();
        assert_eq!(latest.len(), 2);
        assert!(latest.iter().any(|r| r.test_symbol.is_none() && r.covered.contains("agg")));
        assert!(latest.iter().any(|r| r.test_symbol.as_deref() == Some("t") && r.covered.contains("new")));
        assert!(!latest.iter().any(|r| r.covered.contains("old")));
    }

    #[test]
    fn restricting_commits_happens_before_choosing_the_latest_report() {
        let mut store = Store::open_in_memory().unwrap();
        let report = |commit: &str, covered: &str| StoredCoverage {
            commit: commit.into(),
            format: "jacoco".into(),
            test_symbol: Some("t".into()),
            source: "x".into(),
            unmapped_files: 0,
            files: BTreeSet::new(),
            covered: BTreeSet::from([covered.to_owned()]),
        };
        store.add_coverage(&report("old", "a")).unwrap();
        store.add_coverage(&report("new", "b")).unwrap();
        let visible = BTreeSet::from(["old".to_owned()]);
        let latest = store.latest_coverage_among(Some(&visible)).unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].commit, "old", "the excluded newer report must not hide the visible one");
        assert!(store.latest_coverage_among(Some(&BTreeSet::new())).unwrap().is_empty());
        assert_eq!(store.latest_coverage().unwrap()[0].commit, "new");
    }
}
