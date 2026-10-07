//! Read-only Git access.
//!
//! Everything is read from the object database through `gix`; the `git` executable is never
//! invoked. That is a security decision, not a convenience one: repository-local config can make
//! the `git` CLI run arbitrary programs (`core.fsmonitor`, `diff.external`, textconv drivers,
//! hooks), and Ripplepath must be safe to point at an untrusted repository.

mod diff;
mod path;
mod rename;
mod repo;

pub use diff::{LineHunk, line_hunks, line_similarity};
pub use gix::ObjectId;
pub use path::{PathRejection, validate_repo_path};
pub use rename::{RenameCandidate, RenamePair, pair_renames};
pub use repo::{BlobContent, EntryKind, GitError, Repo, Revision, TreeFile, TreeListing};
