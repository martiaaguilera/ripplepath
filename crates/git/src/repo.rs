use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gix::traverse::tree::Recorder;

use crate::path::{PathRejection, validate_repo_path};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("cannot open Git repository at {path}: {message}")]
    Open { path: PathBuf, message: String },
    #[error("revision '{spec}' not found{hint}")]
    RevisionNotFound { spec: String, hint: &'static str },
    #[error("revision '{spec}' does not point to a commit or tree")]
    NotATree { spec: String },
    #[error("failed to read Git object {id}: {message}")]
    Object { id: String, message: String },
    #[error("'{0}' is not a commit id")]
    NotACommit(String),
    #[error("history of {commit} has more than {limit} commits")]
    HistoryTooLong { commit: String, limit: usize },
}

pub struct Repo {
    inner: gix::Repository,
}

/// A resolved revision: the commit (when the spec named one) and the tree analysed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub spec: String,
    pub commit: Option<String>,
    pub tree: gix::ObjectId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Executable,
    Symlink,
    Submodule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeFile {
    pub path: String,
    pub blob: gix::ObjectId,
    pub kind: EntryKind,
}

/// All entries of a tree, plus the entries that were refused and why.
#[derive(Debug, Default)]
pub struct TreeListing {
    pub files: Vec<TreeFile>,
    pub rejected: Vec<(String, PathRejection)>,
}

pub enum BlobContent {
    Text(String),
    /// Contains NUL bytes or is not UTF-8. Not parsed.
    Binary,
    TooLarge {
        size: u64,
    },
}

impl Repo {
    /// Opens with `isolated` permissions: only the repository's own config is read — never the
    /// user's global or system config or environment overrides. Analysis results must depend on
    /// the repository, not on whose machine runs it.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let inner = gix::open_opts(path, gix::open::Options::isolated())
            .map_err(|e| GitError::Open { path: path.to_owned(), message: e.to_string() })?;
        Ok(Self { inner })
    }

    pub fn workdir(&self) -> Option<&Path> {
        self.inner.workdir()
    }

    pub fn is_shallow(&self) -> bool {
        self.inner.is_shallow()
    }

    pub fn resolve(&self, spec: &str) -> Result<Revision, GitError> {
        let id = self.inner.rev_parse_single(spec).map_err(|_| GitError::RevisionNotFound {
            spec: spec.to_owned(),
            // The most common cause in CI is `actions/checkout` with its default depth of 1.
            hint: if self.inner.is_shallow() {
                " (the repository is a shallow clone; fetch more history, e.g. `fetch-depth: 0`)"
            } else {
                ""
            },
        })?;
        let object = id.object().map_err(|e| object_error(id.detach(), e))?;
        let commit = match object.kind {
            gix::object::Kind::Commit => Some(id.detach().to_string()),
            _ => None,
        };
        let tree = object.peel_to_tree().map_err(|_| GitError::NotATree { spec: spec.to_owned() })?;
        Ok(Revision { spec: spec.to_owned(), commit, tree: tree.id })
    }

    /// `commit` and every commit reachable from it through parents, as hex ids.
    ///
    /// Refuses (rather than truncates) histories above `limit`: callers use this set to decide
    /// which evidence existed before a commit, and a silently partial set would change results
    /// without saying so. In a shallow clone the walk stops at the shallow boundary.
    pub fn ancestors(&self, commit: &str, limit: usize) -> Result<BTreeSet<String>, GitError> {
        let start = gix::ObjectId::from_hex(commit.as_bytes()).map_err(|_| GitError::NotACommit(commit.to_owned()))?;
        let mut seen: BTreeSet<gix::ObjectId> = BTreeSet::new();
        let mut queue = vec![start];
        while let Some(id) = queue.pop() {
            if !seen.insert(id) {
                continue;
            }
            if seen.len() > limit {
                return Err(GitError::HistoryTooLong { commit: commit.to_owned(), limit });
            }
            let found = match self.inner.find_commit(id) {
                Ok(found) => found,
                // Parents beyond a shallow boundary are absent from the object database.
                Err(_) if id != start && self.inner.is_shallow() => continue,
                Err(e) => return Err(object_error(id, e)),
            };
            queue.extend(found.parent_ids().map(|p| p.detach()));
        }
        Ok(seen.into_iter().map(|id| id.to_string()).collect())
    }

    /// Lists every non-directory entry of `tree`, sorted by path.
    pub fn list_files(&self, tree: gix::ObjectId) -> Result<TreeListing, GitError> {
        let tree = self.inner.find_tree(tree).map_err(|e| object_error(tree, e))?;
        let mut recorder = Recorder::default();
        tree.traverse().breadthfirst(&mut recorder).map_err(|e| object_error(tree.id, e))?;

        let mut listing = TreeListing::default();
        for entry in recorder.records {
            let kind = match entry.mode.kind() {
                gix::object::tree::EntryKind::Tree => continue,
                gix::object::tree::EntryKind::Blob => EntryKind::File,
                gix::object::tree::EntryKind::BlobExecutable => EntryKind::Executable,
                gix::object::tree::EntryKind::Link => EntryKind::Symlink,
                gix::object::tree::EntryKind::Commit => EntryKind::Submodule,
            };
            match validate_repo_path(&entry.filepath) {
                Ok(path) => listing.files.push(TreeFile { path, blob: entry.oid, kind }),
                Err(reason) => listing.rejected.push((entry.filepath.to_string(), reason)),
            }
        }
        listing.files.sort_by(|a, b| a.path.cmp(&b.path));
        listing.rejected.sort();
        Ok(listing)
    }

    /// Looks up one path in `tree` without listing the whole tree. `None` when the path is absent
    /// or names a directory. The path must already satisfy [`validate_repo_path`]; anything else
    /// is treated as absent rather than looked up, so a caller cannot reach entries a listing
    /// would have refused.
    pub fn find_file(&self, tree: gix::ObjectId, path: &str) -> Result<Option<TreeFile>, GitError> {
        if validate_repo_path(path.as_bytes()).as_deref() != Ok(path) {
            return Ok(None);
        }
        // Walked by hand rather than with `lookup_entry`, which loads every intermediate object
        // before checking that it is a tree: `big.bin/x` would inflate `big.bin` whatever its size.
        let mut current = self.inner.find_tree(tree).map_err(|e| object_error(tree, e))?;
        let mut components = path.split('/').peekable();
        while let Some(component) = components.next() {
            let Some(entry) = current.find_entry(component) else { return Ok(None) };
            let (mode, oid) = (entry.mode(), entry.object_id());
            if components.peek().is_some() {
                if !mode.is_tree() {
                    return Ok(None);
                }
                current = self.inner.find_tree(oid).map_err(|e| object_error(oid, e))?;
                continue;
            }
            let kind = match mode.kind() {
                gix::object::tree::EntryKind::Tree => return Ok(None),
                gix::object::tree::EntryKind::Blob => EntryKind::File,
                gix::object::tree::EntryKind::BlobExecutable => EntryKind::Executable,
                gix::object::tree::EntryKind::Link => EntryKind::Symlink,
                gix::object::tree::EntryKind::Commit => EntryKind::Submodule,
            };
            return Ok(Some(TreeFile { path: path.to_owned(), blob: oid, kind }));
        }
        Ok(None)
    }

    /// Reads a blob as text, refusing anything above `max_bytes` *before* inflating it so that a
    /// repository bomb (a single multi-gigabyte blob) cannot exhaust memory.
    pub fn read_text(&self, blob: gix::ObjectId, max_bytes: u64) -> Result<BlobContent, GitError> {
        let header = self.inner.find_header(blob).map_err(|e| object_error(blob, e))?;
        let size = header.size();
        if size > max_bytes {
            return Ok(BlobContent::TooLarge { size });
        }
        let mut object = self.inner.find_blob(blob).map_err(|e| object_error(blob, e))?;
        let data = object.take_data();
        if data.contains(&0) {
            return Ok(BlobContent::Binary);
        }
        Ok(match String::from_utf8(data) {
            Ok(text) => BlobContent::Text(text),
            Err(_) => BlobContent::Binary,
        })
    }
}

fn object_error(id: gix::ObjectId, error: impl std::fmt::Display) -> GitError {
    GitError::Object { id: id.to_string(), message: error.to_string() }
}
