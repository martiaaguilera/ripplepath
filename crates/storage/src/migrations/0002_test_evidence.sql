-- Test evidence: CI results (JUnit) and measured coverage, each tied to the commit it was taken at.

CREATE TABLE test_runs (
    id         INTEGER PRIMARY KEY,
    commit_id  TEXT NOT NULL,
    source     TEXT NOT NULL
) STRICT;
CREATE INDEX test_runs_by_commit ON test_runs (commit_id);

CREATE TABLE test_results (
    run_id              INTEGER NOT NULL REFERENCES test_runs (id) ON DELETE CASCADE,
    -- The symbol id when the result could be mapped to a test in the indexed code, otherwise
    -- `junit:<classname>#<name>`.
    test_key            TEXT NOT NULL,
    mapped              INTEGER NOT NULL CHECK (mapped IN (0, 1)),
    outcome             TEXT NOT NULL CHECK (outcome IN ('PASSED', 'FAILED', 'ERROR', 'SKIPPED')),
    duration_ms         INTEGER NOT NULL,
    failed_attempts     INTEGER NOT NULL,
    failure_fingerprint TEXT,
    PRIMARY KEY (run_id, test_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX test_results_by_key ON test_results (test_key);

CREATE TABLE coverage_reports (
    id             INTEGER PRIMARY KEY,
    commit_id      TEXT NOT NULL,
    format         TEXT NOT NULL,
    -- Test symbol whose execution produced this coverage; NULL for aggregate coverage.
    test_symbol    TEXT,
    source         TEXT NOT NULL,
    unmapped_files INTEGER NOT NULL
) STRICT;

-- Repository files the report contained, so "measured but not covered" can be told apart from
-- "no coverage data".
CREATE TABLE coverage_files (
    report_id INTEGER NOT NULL REFERENCES coverage_reports (id) ON DELETE CASCADE,
    path      TEXT NOT NULL,
    PRIMARY KEY (report_id, path)
) STRICT, WITHOUT ROWID;

CREATE TABLE covered_symbols (
    report_id INTEGER NOT NULL REFERENCES coverage_reports (id) ON DELETE CASCADE,
    symbol_id TEXT NOT NULL,
    PRIMARY KEY (report_id, symbol_id)
) STRICT, WITHOUT ROWID;
