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
