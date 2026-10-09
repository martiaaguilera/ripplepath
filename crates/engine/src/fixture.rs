//! Builds Git repositories from Ripplepath's own snapshot fixtures (`fixtures/<name>/v1`, `v2`, …).
//!
//! This is the only place Ripplepath runs the `git` executable, and it only ever does so on a
//! repository it has just created from files shipped in this project — never on a user's or an
//! analysed repository (see docs/SECURITY_MODEL.md). Arguments are passed as a vector, never through
//! a shell, and author/committer identity and dates are fixed so commit ids are reproducible.
//!
//! The `git` process gets no global or system configuration and a hooks directory that does not
//! exist: a user's `filter.*`, `core.fsmonitor` or hooks must not run, and must not change the
//! commit ids, while the demo builds. Snapshot files are written by this module from memory (never
//! copied as directory trees), so a symlink or a `.git` entry in a snapshot cannot make it read
//! outside the snapshot or plant Git configuration.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("fixture snapshot directory {0} does not exist")]
    MissingSnapshot(PathBuf),
    #[error("fixture snapshot entry {path} is refused: {reason}")]
    Refused { path: String, reason: String },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`git {args}` failed: {stderr}")]
    Git { args: String, stderr: String },
}

/// One snapshot: a name (used in the commit message) and its files as (repository path, bytes).
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Bounds for reading a snapshot directory: fixtures are a few hundred small files.
const MAX_SNAPSHOT_FILES: usize = 10_000;
const MAX_SNAPSHOT_DEPTH: usize = 32;

/// Creates `target` as a Git repository with one commit per snapshot directory, in order, on branch
/// `main`. Returns the commit ids.
pub fn build_fixture_repo(snapshots: &[&Path], target: &Path) -> Result<Vec<String>, FixtureError> {
    let loaded = snapshots.iter().map(|dir| read_snapshot(dir)).collect::<Result<Vec<_>, _>>()?;
    build_repo_from_snapshots(&loaded, target)
}

/// Reads a snapshot directory into memory. Symlinks and `.git` entries are refused rather than
/// followed or copied.
pub fn read_snapshot(dir: &Path) -> Result<Snapshot, FixtureError> {
    if !dir.is_dir() {
        return Err(FixtureError::MissingSnapshot(dir.to_path_buf()));
    }
    let name = dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut files = Vec::new();
    read_dir_into(dir, "", 0, &mut files)?;
    files.sort();
    Ok(Snapshot { name, files })
}

fn read_dir_into(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), FixtureError> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| FixtureError::Io { path, source }
    };
    if depth > MAX_SNAPSHOT_DEPTH {
        return Err(FixtureError::Refused { path: prefix.to_owned(), reason: "nested too deeply".to_owned() });
    }
    for entry in std::fs::read_dir(dir).map_err(io(dir))? {
        let entry = entry.map_err(io(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        // `symlink_metadata` does not follow links, unlike `Path::is_dir`.
        let kind = entry.file_type().map_err(io(&entry.path()))?;
        if kind.is_symlink() {
            return Err(FixtureError::Refused { path: relative, reason: "symbolic links are not followed".to_owned() });
        }
        if kind.is_dir() {
            read_dir_into(&entry.path(), &relative, depth + 1, out)?;
        } else {
            out.push((relative, std::fs::read(entry.path()).map_err(io(&entry.path()))?));
        }
        if out.len() > MAX_SNAPSHOT_FILES {
            return Err(FixtureError::Refused { path: prefix.to_owned(), reason: "too many files".to_owned() });
        }
    }
    Ok(())
}

/// Creates `target` as a Git repository with one commit per in-memory snapshot, in order, on branch
/// `main`. Returns the commit ids.
pub fn build_repo_from_snapshots(snapshots: &[Snapshot], target: &Path) -> Result<Vec<String>, FixtureError> {
    // Validate everything before touching the disk.
    for snapshot in snapshots {
        for (path, _) in &snapshot.files {
            ripplepath_git::validate_repo_path(path.as_bytes())
                .map_err(|reason| FixtureError::Refused { path: path.clone(), reason: reason.to_string() })?;
        }
    }
    std::fs::create_dir_all(target).map_err(|source| FixtureError::Io { path: target.to_owned(), source })?;
    git(target, &["init", "--quiet", "--initial-branch=main"], 0)?;
    let mut commits = Vec::new();
    for (i, snapshot) in snapshots.iter().enumerate() {
        clear_worktree(target)?;
        write_snapshot(snapshot, target)?;
        git(target, &["add", "--all"], i)?;
        let name = if snapshot.name.is_empty() { format!("v{}", i + 1) } else { snapshot.name.clone() };
        git(target, &["commit", "--quiet", "--allow-empty", "-m", &format!("fixture: {name}")], i)?;
        commits.push(git(target, &["rev-parse", "HEAD"], i)?.trim().to_owned());
    }
    Ok(commits)
}

fn git(dir: &Path, args: &[&str], sequence: usize) -> Result<String, FixtureError> {
    // One day apart per commit, from a fixed epoch.
    let date = format!("{} +0000", 1_767_225_600 + sequence * 86_400);
    // Neither exists: Git treats a missing config file as empty and a missing hooks directory as
    // "no hooks". Both live inside the repository being created, which nothing else writes to.
    let git_dir = dir.join(".git");
    let no_config = git_dir.join("ripplepath-no-global-config");
    let no_hooks = git_dir.join("ripplepath-no-hooks");
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-c", "core.fsmonitor=false"])
        .arg("-c")
        .arg(format!("core.hooksPath={}", no_hooks.display()))
        .args(args)
        .env("GIT_AUTHOR_NAME", "Ripplepath Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@ripplepath.invalid")
        .env("GIT_COMMITTER_NAME", "Ripplepath Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@ripplepath.invalid")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &no_config)
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|source| FixtureError::Io { path: dir.to_owned(), source })?;
    if !output.status.success() {
        return Err(FixtureError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn clear_worktree(dir: &Path) -> Result<(), FixtureError> {
    let io = |source| FixtureError::Io { path: dir.to_owned(), source };
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        // `remove_dir_all` does not follow symlinks; a link is removed as a file.
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let result = if is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        result.map_err(|source| FixtureError::Io { path: path.clone(), source })?;
    }
    Ok(())
}

fn write_snapshot(snapshot: &Snapshot, target: &Path) -> Result<(), FixtureError> {
    for (path, bytes) in &snapshot.files {
        let dest = target.join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|source| FixtureError::Io { path: parent.to_owned(), source })?;
        }
        // Normalise line endings: a Windows checkout of this project may have converted the
        // fixtures to CRLF, which would change blob ids and line-based expectations.
        let normalized =
            std::str::from_utf8(bytes).map_or_else(|_| bytes.clone(), |text| text.replace("\r\n", "\n").into_bytes());
        std::fs::write(&dest, normalized).map_err(|source| FixtureError::Io { path: dest.clone(), source })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn snapshots_with_git_entries_or_traversal_are_refused_before_anything_is_written() {
        for bad in [".git/config", "a/../b", "x/.GIT/hooks/pre-commit", "/abs"] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("repo");
            let snapshot = Snapshot { name: "v1".to_owned(), files: vec![(bad.to_owned(), b"x".to_vec())] };
            let error = build_repo_from_snapshots(&[snapshot], &target).unwrap_err();
            assert!(matches!(error, FixtureError::Refused { .. }), "{bad}: {error}");
            assert!(!target.exists(), "{bad}: nothing may be created");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_in_a_snapshot_directory_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = dir.path().join("v1");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::os::unix::fs::symlink("/etc", snapshot.join("escape")).unwrap();
        let error = read_snapshot(&snapshot).unwrap_err();
        assert!(matches!(error, FixtureError::Refused { .. }), "{error}");
    }
}
