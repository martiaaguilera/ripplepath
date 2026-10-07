-- Fact cache and the indexed graph of the most recently indexed revision.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- Per-file extraction results. `payload` is the serialized fact record for this exact
-- (blob, path, extractor) — an opaque cache entry, never queried by content. Everything the
-- product queries lives in the structured tables below.
CREATE TABLE file_facts (
    blob      TEXT NOT NULL,
    path      TEXT NOT NULL,
    extractor TEXT NOT NULL,
    outcome   TEXT NOT NULL CHECK (outcome IN ('ok', 'error')),
    payload   BLOB NOT NULL,
    PRIMARY KEY (blob, path, extractor)
) STRICT, WITHOUT ROWID;

CREATE TABLE index_state (
    id        INTEGER PRIMARY KEY CHECK (id = 1),
    tree      TEXT NOT NULL,
    commit_id TEXT
) STRICT;

CREATE TABLE indexed_files (
    path     TEXT PRIMARY KEY,
    blob     TEXT NOT NULL,
    language TEXT,
    status   TEXT NOT NULL
) STRICT;

CREATE TABLE symbols (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    language    TEXT NOT NULL,
    module      TEXT NOT NULL,
    file        TEXT NOT NULL,
    start_line  INTEGER NOT NULL,
    end_line    INTEGER NOT NULL,
    parent      TEXT,
    visibility  TEXT NOT NULL,
    is_test     INTEGER NOT NULL CHECK (is_test IN (0, 1)),
    fingerprint TEXT NOT NULL
) STRICT;
CREATE INDEX symbols_by_file ON symbols (file);

CREATE TABLE edges (
    from_id  TEXT NOT NULL,
    to_id    TEXT NOT NULL,
    kind     TEXT NOT NULL,
    evidence TEXT NOT NULL,
    file     TEXT NOT NULL,
    line     INTEGER NOT NULL,
    rule     TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id, kind)
) STRICT, WITHOUT ROWID;
CREATE INDEX edges_by_target ON edges (to_id);

CREATE TABLE unresolved (
    from_id TEXT NOT NULL,
    file    TEXT NOT NULL,
    line    INTEGER NOT NULL,
    detail  TEXT NOT NULL,
    PRIMARY KEY (from_id, file, line, detail)
) STRICT, WITHOUT ROWID;
