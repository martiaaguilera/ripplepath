//! Path-level and symbol-level change detection between two snapshots.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ripplepath_core::{Span, Symbol, SymbolId, SymbolKind};
use ripplepath_git::{BlobContent, ObjectId, RenameCandidate, Repo, line_hunks, pair_renames};

use crate::analysis::AnalysisError;
use crate::report::{ChangeKind, ChangedSymbol, FileStatus, HunkReport};
use crate::snapshot::{Snapshot, SnapshotFile};

#[derive(Clone, Debug)]
pub struct PathChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub similarity: Option<u8>,
    pub base_blob: Option<ObjectId>,
    pub head_blob: Option<ObjectId>,
}

impl PathChange {
    pub fn base_path(&self) -> Option<&str> {
        match self.status {
            FileStatus::Added => None,
            FileStatus::Renamed => self.old_path.as_deref(),
            FileStatus::Deleted | FileStatus::Modified => Some(&self.path),
        }
    }

    pub fn head_path(&self) -> Option<&str> {
        match self.status {
            FileStatus::Deleted => None,
            _ => Some(&self.path),
        }
    }
}

/// Blob texts read during one analysis, shared by rename detection and hunk computation.
pub struct TextStore<'r> {
    repo: &'r Repo,
    max_bytes: u64,
    texts: HashMap<ObjectId, Option<String>>,
}

impl<'r> TextStore<'r> {
    pub fn new(repo: &'r Repo, max_bytes: u64) -> Self {
        Self { repo, max_bytes, texts: HashMap::new() }
    }

    pub fn load(&mut self, blob: ObjectId) -> Result<(), AnalysisError> {
        if !self.texts.contains_key(&blob) {
            let text = match self.repo.read_text(blob, self.max_bytes)? {
                BlobContent::Text(text) => Some(text),
                BlobContent::Binary | BlobContent::TooLarge { .. } => None,
            };
            self.texts.insert(blob, text);
        }
        Ok(())
    }

    pub fn get(&self, blob: ObjectId) -> Option<&str> {
        self.texts.get(&blob).and_then(|t| t.as_deref())
    }
}

/// Added/deleted/modified paths, with deleted+added pairs collapsed into renames.
/// Returns the changes sorted by path and whether similarity-based rename detection was skipped.
pub fn diff_paths(
    base: &Snapshot,
    head: &Snapshot,
    texts: &mut TextStore<'_>,
) -> Result<(Vec<PathChange>, bool), AnalysisError> {
    let base_files: BTreeMap<&str, &SnapshotFile> = base.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let head_files: BTreeMap<&str, &SnapshotFile> = head.files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut changes = Vec::new();
    let mut deleted = Vec::new();
    let mut added = Vec::new();
    for (path, file) in &base_files {
        match head_files.get(path) {
            Some(other) if other.blob == file.blob && other.status_kind() == file.status_kind() => {}
            Some(other) => changes.push(PathChange {
                path: (*path).to_owned(),
                old_path: None,
                status: FileStatus::Modified,
                similarity: None,
                base_blob: Some(file.blob),
                head_blob: Some(other.blob),
            }),
            None => deleted.push(*file),
        }
    }
    for (path, file) in &head_files {
        if !base_files.contains_key(path) {
            added.push(*file);
        }
    }

    for file in deleted.iter().chain(added.iter()) {
        texts.load(file.blob)?;
    }
    let blob_hex: HashMap<ObjectId, String> =
        deleted.iter().chain(added.iter()).map(|f| (f.blob, f.blob.to_string())).collect();
    let candidates = |files: &[&SnapshotFile]| -> Vec<(String, String, Option<String>)> {
        files
            .iter()
            .map(|f| (f.path.clone(), blob_hex[&f.blob].clone(), texts.get(f.blob).map(str::to_owned)))
            .collect()
    };
    let deleted_owned = candidates(&deleted);
    let added_owned = candidates(&added);
    let (pairs, skipped) = pair_renames(&as_candidates(&deleted_owned), &as_candidates(&added_owned));

    let renamed_from: BTreeSet<&str> = pairs.iter().map(|p| p.from.as_str()).collect();
    let renamed_to: BTreeSet<&str> = pairs.iter().map(|p| p.to.as_str()).collect();
    for pair in &pairs {
        changes.push(PathChange {
            path: pair.to.clone(),
            old_path: Some(pair.from.clone()),
            status: FileStatus::Renamed,
            similarity: Some(pair.similarity),
            base_blob: base_files.get(pair.from.as_str()).map(|f| f.blob),
            head_blob: head_files.get(pair.to.as_str()).map(|f| f.blob),
        });
    }
    for file in deleted.iter().filter(|f| !renamed_from.contains(f.path.as_str())) {
        changes.push(PathChange {
            path: file.path.clone(),
            old_path: None,
            status: FileStatus::Deleted,
            similarity: None,
            base_blob: Some(file.blob),
            head_blob: None,
        });
    }
    for file in added.iter().filter(|f| !renamed_to.contains(f.path.as_str())) {
        changes.push(PathChange {
            path: file.path.clone(),
            old_path: None,
            status: FileStatus::Added,
            similarity: None,
            base_blob: None,
            head_blob: Some(file.blob),
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((changes, skipped))
}

fn as_candidates(owned: &[(String, String, Option<String>)]) -> Vec<RenameCandidate<'_>> {
    owned.iter().map(|(path, blob, text)| RenameCandidate { path, blob, text: text.as_deref() }).collect()
}

impl SnapshotFile {
    /// Mode changes (e.g. file ↔ symlink) count as modifications even with an identical blob.
    fn status_kind(&self) -> u8 {
        match self.status {
            crate::snapshot::IndexStatus::Symlink => 1,
            crate::snapshot::IndexStatus::Submodule => 2,
            _ => 0,
        }
    }
}

fn symbols_in<'a>(snapshot: &'a Snapshot, paths: &BTreeSet<&str>) -> BTreeMap<&'a SymbolId, &'a Symbol> {
    snapshot.graph.symbols().filter(|s| paths.contains(s.file.as_str())).map(|s| (&s.id, s)).collect()
}

fn changed(symbol: &Symbol, change: ChangeKind) -> ChangedSymbol {
    ChangedSymbol {
        id: symbol.id.clone(),
        change,
        previous_id: None,
        probable_move: None,
        kind: symbol.kind,
        name: symbol.name.clone(),
        language: symbol.language,
        module: symbol.module.clone(),
        file: symbol.file.clone(),
        span: symbol.span,
        visibility: symbol.visibility,
        is_test: symbol.is_test,
    }
}

/// `java:a.B#name(Params)` → (`java:a.B`, `name`). Only meaningful for callables.
fn owner_and_name(id: &SymbolId) -> Option<(&str, &str)> {
    let (owner, member) = id.as_str().split_once('#')?;
    let name = member.split_once('(').map_or(member, |(name, _)| name);
    Some((owner, name))
}

/// Classifies symbols of changed files. Unchanged files contribute identical symbols to both
/// snapshots by construction (same blob, same path ⇒ same facts), so only changed paths are compared.
pub fn classify_symbols(base: &Snapshot, head: &Snapshot, changes: &[PathChange]) -> Vec<ChangedSymbol> {
    let base_paths: BTreeSet<&str> = changes.iter().filter_map(PathChange::base_path).collect();
    let head_paths: BTreeSet<&str> = changes.iter().filter_map(PathChange::head_path).collect();
    let before = symbols_in(base, &base_paths);
    let after = symbols_in(head, &head_paths);

    let mut result = Vec::new();
    let mut deleted: Vec<&Symbol> = Vec::new();
    let mut added: Vec<&Symbol> = Vec::new();
    for (id, old) in &before {
        // A symbol can also reappear in a file that did not change only when ids collide across
        // files; compare against wherever head has it rather than reporting a phantom deletion.
        match after.get(id).copied().or_else(|| head.graph.symbol(id)) {
            Some(new) if new.fingerprint != old.fingerprint => result.push(changed(new, ChangeKind::Modified)),
            Some(_) => {}
            None => deleted.push(old),
        }
    }
    for (id, new) in &after {
        if !before.contains_key(id) && !base.graph.contains(id) {
            added.push(new);
        }
    }

    // Signature changes: exactly one deleted and one added callable with the same owner and name.
    let key = |s: &Symbol| owner_and_name(&s.id).map(|(o, n)| (o.to_owned(), n.to_owned()));
    let mut deleted_by_key: BTreeMap<(String, String), Vec<&Symbol>> = BTreeMap::new();
    let mut added_by_key: BTreeMap<(String, String), Vec<&Symbol>> = BTreeMap::new();
    for s in deleted.iter().filter(|s| s.kind.is_callable()) {
        if let Some(k) = key(s) {
            deleted_by_key.entry(k).or_default().push(s);
        }
    }
    for s in added.iter().filter(|s| s.kind.is_callable()) {
        if let Some(k) = key(s) {
            added_by_key.entry(k).or_default().push(s);
        }
    }
    let mut paired: BTreeSet<&SymbolId> = BTreeSet::new();
    for (k, olds) in &deleted_by_key {
        if let (1, Some(news)) = (olds.len(), added_by_key.get(k))
            && news.len() == 1
        {
            let mut entry = changed(news[0], ChangeKind::SignatureChanged);
            entry.previous_id = Some(olds[0].id.clone());
            result.push(entry);
            paired.insert(&olds[0].id);
            paired.insert(&news[0].id);
        }
    }
    deleted.retain(|s| !paired.contains(&s.id));
    added.retain(|s| !paired.contains(&s.id));

    // Probable moves: identical fingerprint and kind, unique in both directions.
    type DeletedAndAdded<'s> = (Vec<&'s Symbol>, Vec<&'s Symbol>);
    let mut by_fingerprint: BTreeMap<(SymbolKind, ripplepath_core::Fingerprint), DeletedAndAdded<'_>> = BTreeMap::new();
    for s in deleted.iter().filter(|s| s.kind != SymbolKind::File) {
        by_fingerprint.entry((s.kind, s.fingerprint)).or_default().0.push(s);
    }
    for s in added.iter().filter(|s| s.kind != SymbolKind::File) {
        by_fingerprint.entry((s.kind, s.fingerprint)).or_default().1.push(s);
    }
    let mut moves: BTreeMap<&SymbolId, &SymbolId> = BTreeMap::new();
    for (olds, news) in by_fingerprint.values() {
        if olds.len() == 1 && news.len() == 1 {
            moves.insert(&olds[0].id, &news[0].id);
            moves.insert(&news[0].id, &olds[0].id);
        }
    }

    for s in deleted {
        let mut entry = changed(s, ChangeKind::Deleted);
        entry.probable_move = moves.get(&s.id).map(|id| (*id).clone());
        result.push(entry);
    }
    for s in added {
        let mut entry = changed(s, ChangeKind::Added);
        entry.probable_move = moves.get(&s.id).map(|id| (*id).clone());
        result.push(entry);
    }
    result.sort_by(|a, b| (&a.file, a.span.start_line, &a.id).cmp(&(&b.file, b.span.start_line, &b.id)));
    result
}

/// Innermost-symbol lookup for one file of one snapshot.
pub struct LineIndex<'a> {
    /// Non-file symbols of the file, with their spans.
    symbols: Vec<(Span, &'a SymbolId)>,
    file_symbol: Option<&'a SymbolId>,
}

impl<'a> LineIndex<'a> {
    pub fn new(snapshot: &'a Snapshot, path: &str) -> Self {
        let mut symbols = Vec::new();
        let mut file_symbol = None;
        for symbol in snapshot.graph.symbols().filter(|s| s.file == path) {
            if symbol.kind == SymbolKind::File {
                file_symbol = Some(&symbol.id);
            } else {
                symbols.push((symbol.span, &symbol.id));
            }
        }
        Self { symbols, file_symbol }
    }

    fn innermost(&self, contains: impl Fn(Span) -> bool) -> Option<&'a SymbolId> {
        self.symbols
            .iter()
            .filter(|(span, _)| contains(*span))
            .min_by_key(|(span, id)| (span.len(), *id))
            .map(|(_, id)| *id)
            .or(self.file_symbol)
    }

    /// Symbols touched by lines `start..start+len`. For a pure insertion (`len == 0`) the symbol
    /// spanning the gap between line `start` and `start + 1` is used, so code inserted just after a
    /// method's closing brace is attributed to the class, not to that method.
    pub fn symbols_for(&self, start: u32, len: u32) -> Vec<SymbolId> {
        let mut found = BTreeSet::new();
        if len == 0 {
            if let Some(id) = self.innermost(|span| span.contains_line(start) && span.contains_line(start + 1)) {
                found.insert(id.clone());
            }
        } else {
            for line in start..start + len {
                if let Some(id) = self.innermost(|span| span.contains_line(line)) {
                    found.insert(id.clone());
                }
            }
        }
        found.into_iter().collect()
    }
}

pub fn hunks_for(change: &PathChange, base: &Snapshot, head: &Snapshot, texts: &TextStore<'_>) -> Vec<HunkReport> {
    let before = match change.base_blob {
        Some(blob) => match texts.get(blob) {
            Some(text) => text,
            None => return Vec::new(),
        },
        None => "",
    };
    let after = match change.head_blob {
        Some(blob) => match texts.get(blob) {
            Some(text) => text,
            None => return Vec::new(),
        },
        None => "",
    };
    let base_index = change.base_path().map(|p| LineIndex::new(base, p));
    let head_index = change.head_path().map(|p| LineIndex::new(head, p));
    line_hunks(before, after)
        .into_iter()
        .map(|h| HunkReport {
            old_start: h.old_start,
            old_len: h.old_len,
            new_start: h.new_start,
            new_len: h.new_len,
            base_symbols: base_index.as_ref().map(|i| i.symbols_for(h.old_start, h.old_len)).unwrap_or_default(),
            head_symbols: head_index.as_ref().map(|i| i.symbols_for(h.new_start, h.new_len)).unwrap_or_default(),
        })
        .collect()
}
