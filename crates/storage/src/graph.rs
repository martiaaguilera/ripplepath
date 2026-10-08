//! The indexed graph of one revision, updated by diff.

use std::collections::{BTreeMap, BTreeSet};

use ripplepath_core::{Edge, EdgeKind, Fingerprint, Language, Span, Symbol, SymbolId};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;

use crate::{StorageError, Store};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexedFile {
    pub path: String,
    pub blob: String,
    pub language: Option<Language>,
    /// `indexed`, `not_source`, `too_large`, `binary`, `parse_failed`, `symlink`, `submodule`,
    /// `excluded`.
    pub status: String,
}

/// Everything the index stores for one revision. Collections are sorted, so two values compare
/// equal exactly when the stored rows are equal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexedGraph {
    pub tree: String,
    pub commit: Option<String>,
    pub files: Vec<IndexedFile>,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    /// (from, file, line, detail)
    pub unresolved: Vec<(SymbolId, String, u32, String)>,
}

impl IndexedGraph {
    pub fn normalize(&mut self) {
        self.files.sort();
        self.symbols.sort_by(|a, b| a.id.cmp(&b.id));
        self.edges.sort();
        self.unresolved.sort();
        self.unresolved.dedup();
    }
}

/// What an index run changed. Counts rows, so a one-file edit should show small numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphDelta {
    pub files_changed: usize,
    pub symbols_added: usize,
    pub symbols_removed: usize,
    pub symbols_updated: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    pub edges_updated: usize,
    pub unresolved_added: usize,
    pub unresolved_removed: usize,
}

fn text<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

fn parse<T: serde::de::DeserializeOwned>(column: &'static str, raw: String) -> Result<T, StorageError> {
    // Deserialize straight from the borrowed text: going through a `serde_json::Value` cost an
    // allocation and a clone per enum column, which dominated loading a large graph.
    let deserializer = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(&raw);
    T::deserialize(deserializer).map_err(|_| corrupt(column, &raw))
}

/// Error for an unreadable stored value. The value is escaped and truncated: it came from a file on
/// disk and ends up in terminal output and logs.
pub(crate) fn corrupt(column: &'static str, raw: &str) -> StorageError {
    let value: String = raw.chars().take(64).flat_map(char::escape_debug).collect();
    StorageError::Corrupt { column, value }
}

type EdgeKey<'a> = (&'a SymbolId, &'a SymbolId, EdgeKind);

impl Store {
    pub fn load_graph(&self) -> Result<Option<IndexedGraph>, StorageError> {
        let conn = self.conn();
        let state: Option<(String, Option<String>)> = conn
            .query_row("SELECT tree, commit_id FROM index_state WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((tree, commit)) = state else {
            return Ok(None);
        };
        let mut graph = IndexedGraph { tree, commit, ..IndexedGraph::default() };

        let mut stmt = conn.prepare("SELECT path, blob, language, status FROM indexed_files")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?))
        })?;
        for row in rows {
            let (path, blob, language, status) = row?;
            let language = language.map(|l| parse("indexed_files.language", l)).transpose()?;
            graph.files.push(IndexedFile { path, blob, language, status });
        }

        let mut stmt = conn.prepare(
            "SELECT id, kind, name, language, module, file, start_line, end_line, parent, visibility, is_test, fingerprint FROM symbols",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, u32>(6)?,
                r.get::<_, u32>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, bool>(10)?,
                r.get::<_, String>(11)?,
            ))
        })?;
        for row in rows {
            let (id, kind, name, language, module, file, start, end, parent, visibility, is_test, fingerprint) = row?;
            graph.symbols.push(Symbol {
                id: SymbolId::new(id),
                kind: parse("symbols.kind", kind)?,
                name,
                language: parse("symbols.language", language)?,
                module,
                file,
                span: Span { start_line: start, end_line: end },
                parent: parent.map(SymbolId::new),
                visibility: parse("symbols.visibility", visibility)?,
                is_test,
                fingerprint: Fingerprint::try_from(fingerprint.clone())
                    .map_err(|_| corrupt("symbols.fingerprint", &fingerprint))?,
            });
        }

        let mut stmt = conn.prepare("SELECT from_id, to_id, kind, evidence, file, line, rule FROM edges")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, u32>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        for row in rows {
            let (from, to, kind, evidence, file, line, rule) = row?;
            graph.edges.push(Edge {
                from: SymbolId::new(from),
                to: SymbolId::new(to),
                kind: parse("edges.kind", kind)?,
                evidence: parse("edges.evidence", evidence)?,
                file,
                line,
                rule,
            });
        }

        let mut stmt = conn.prepare("SELECT from_id, file, line, detail FROM unresolved")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                SymbolId::new(r.get::<_, String>(0)?),
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            graph.unresolved.push(row?);
        }
        graph.normalize();
        Ok(Some(graph))
    }

    /// Makes the stored index equal to `next`, touching only rows that differ.
    ///
    /// Resolution is recomputed for the whole snapshot (it depends on every file's imports), but
    /// rows are written by difference: removed symbols and their edges are deleted, unchanged rows
    /// are left alone. Correctness rests on one invariant, tested in the engine: the stored result
    /// after any sequence of index runs equals a clean index of the final revision.
    pub fn apply_graph(&mut self, mut next: IndexedGraph) -> Result<GraphDelta, StorageError> {
        next.normalize();
        let previous = self.load_graph()?.unwrap_or_default();
        let mut delta = GraphDelta::default();
        let tx = self.conn_mut().transaction()?;

        let old_files: BTreeMap<&str, &IndexedFile> = previous.files.iter().map(|f| (f.path.as_str(), f)).collect();
        let new_files: BTreeMap<&str, &IndexedFile> = next.files.iter().map(|f| (f.path.as_str(), f)).collect();
        for (path, old) in &old_files {
            match new_files.get(path) {
                Some(new) if new == old => {}
                Some(_) => {}
                None => {
                    tx.prepare_cached("DELETE FROM indexed_files WHERE path = ?1")?.execute(params![path])?;
                    delta.files_changed += 1;
                }
            }
        }
        for (path, new) in &new_files {
            if old_files.get(path) != Some(new) {
                tx.prepare_cached(
                    "INSERT OR REPLACE INTO indexed_files (path, blob, language, status) VALUES (?1, ?2, ?3, ?4)",
                )?
                .execute(params![path, new.blob, new.language.as_ref().map(text), new.status])?;
                delta.files_changed += 1;
            }
        }

        let old_symbols: BTreeMap<&SymbolId, &Symbol> = previous.symbols.iter().map(|s| (&s.id, s)).collect();
        let new_symbols: BTreeMap<&SymbolId, &Symbol> = next.symbols.iter().map(|s| (&s.id, s)).collect();
        for id in old_symbols.keys().filter(|id| !new_symbols.contains_key(*id)) {
            tx.prepare_cached("DELETE FROM symbols WHERE id = ?1")?.execute(params![id.as_str()])?;
            delta.symbols_removed += 1;
        }
        for (id, symbol) in &new_symbols {
            match old_symbols.get(id) {
                Some(old) if old == symbol => continue,
                Some(_) => delta.symbols_updated += 1,
                None => delta.symbols_added += 1,
            }
            write_symbol(&tx, symbol)?;
        }

        fn key(e: &Edge) -> EdgeKey<'_> {
            (&e.from, &e.to, e.kind)
        }
        let old_edges: BTreeMap<EdgeKey<'_>, &Edge> = previous.edges.iter().map(|e| (key(e), e)).collect();
        let new_edges: BTreeMap<EdgeKey<'_>, &Edge> = next.edges.iter().map(|e| (key(e), e)).collect();
        for (from, to, kind) in old_edges.keys().filter(|k| !new_edges.contains_key(*k)) {
            tx.prepare_cached("DELETE FROM edges WHERE from_id = ?1 AND to_id = ?2 AND kind = ?3")?
                .execute(params![from.as_str(), to.as_str(), text(kind)])?;
            delta.edges_removed += 1;
        }
        for (k, edge) in &new_edges {
            match old_edges.get(k) {
                Some(old) if old == edge => continue,
                Some(_) => delta.edges_updated += 1,
                None => delta.edges_added += 1,
            }
            tx.prepare_cached(
                "INSERT OR REPLACE INTO edges (from_id, to_id, kind, evidence, file, line, rule) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?
            .execute(params![
                edge.from.as_str(),
                edge.to.as_str(),
                text(&edge.kind),
                text(&edge.evidence),
                edge.file,
                edge.line,
                edge.rule
            ])?;
        }

        let old_unresolved: BTreeSet<_> = previous.unresolved.iter().collect();
        let new_unresolved: BTreeSet<_> = next.unresolved.iter().collect();
        for (from, file, line, detail) in old_unresolved.difference(&new_unresolved) {
            tx.prepare_cached("DELETE FROM unresolved WHERE from_id = ?1 AND file = ?2 AND line = ?3 AND detail = ?4")?
                .execute(params![from.as_str(), file, line, detail])?;
            delta.unresolved_removed += 1;
        }
        for (from, file, line, detail) in new_unresolved.difference(&old_unresolved) {
            tx.prepare_cached("INSERT INTO unresolved (from_id, file, line, detail) VALUES (?1, ?2, ?3, ?4)")?
                .execute(params![from.as_str(), file, line, detail])?;
            delta.unresolved_added += 1;
        }

        tx.execute(
            "INSERT OR REPLACE INTO index_state (id, tree, commit_id) VALUES (1, ?1, ?2)",
            params![next.tree, next.commit],
        )?;
        tx.commit()?;
        Ok(delta)
    }
}

fn write_symbol(tx: &Transaction<'_>, s: &Symbol) -> Result<(), StorageError> {
    // Cached: a cold index writes one row per symbol, and re-preparing the statement per row was
    // a measurable share of `apply_graph` (docs/ENGINEERING_LOG.md, performance).
    tx.prepare_cached(
        "INSERT OR REPLACE INTO symbols (id, kind, name, language, module, file, start_line, end_line, parent, visibility, is_test, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?
    .execute(params![
            s.id.as_str(),
            text(&s.kind),
            s.name,
            text(&s.language),
            s.module,
            s.file,
            s.span.start_line,
            s.span.end_line,
            s.parent.as_ref().map(SymbolId::as_str),
            text(&s.visibility),
            s.is_test,
            s.fingerprint.to_string(),
        ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use ripplepath_core::{EdgeKind, Evidence, FingerprintBuilder, SymbolKind, Visibility};

    use super::*;

    fn symbol(id: &str, token: &str) -> Symbol {
        let mut fp = FingerprintBuilder::new();
        fp.token(token);
        Symbol {
            id: SymbolId::new(id),
            kind: SymbolKind::Method,
            name: id.to_owned(),
            language: Language::Java,
            module: "m".into(),
            file: "A.java".into(),
            span: Span { start_line: 1, end_line: 2 },
            parent: None,
            visibility: Visibility::Public,
            is_test: false,
            fingerprint: fp.finish(),
        }
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: SymbolId::new(from),
            to: SymbolId::new(to),
            kind: EdgeKind::Calls,
            evidence: Evidence::ResolvedExact,
            file: "A.java".into(),
            line: 1,
            rule: "r".into(),
        }
    }

    fn graph(symbols: Vec<Symbol>, edges: Vec<Edge>) -> IndexedGraph {
        IndexedGraph { tree: "t".into(), commit: None, files: Vec::new(), symbols, edges, unresolved: Vec::new() }
    }

    #[test]
    fn round_trips_and_applies_minimal_deltas() {
        let mut store = Store::open_in_memory().unwrap();
        let first = graph(vec![symbol("a", "1"), symbol("b", "1")], vec![edge("a", "b")]);
        let delta = store.apply_graph(first.clone()).unwrap();
        assert_eq!((delta.symbols_added, delta.edges_added), (2, 1));
        assert_eq!(store.load_graph().unwrap(), Some(first));

        // b changes, a is removed with its edge, c is added.
        let second = graph(vec![symbol("b", "2"), symbol("c", "1")], vec![edge("c", "b")]);
        let delta = store.apply_graph(second.clone()).unwrap();
        assert_eq!(delta.symbols_added, 1);
        assert_eq!(delta.symbols_removed, 1);
        assert_eq!(delta.symbols_updated, 1);
        assert_eq!((delta.edges_added, delta.edges_removed), (1, 1));
        assert_eq!(store.load_graph().unwrap(), Some(second.clone()));

        let unchanged = store.apply_graph(second).unwrap();
        assert_eq!(unchanged, GraphDelta::default());
    }

    #[test]
    fn migrations_are_idempotent_and_newer_schemas_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.db");
        drop(Store::open(&path).unwrap());
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), crate::SCHEMA_VERSION);
        store.conn().pragma_update(None, "user_version", 99).unwrap();
        drop(store);
        assert!(matches!(Store::open(&path), Err(StorageError::TooNew { found: 99, .. })));
    }
}
