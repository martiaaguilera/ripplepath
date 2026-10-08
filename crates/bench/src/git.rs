//! Writes generated files and commits them with the `git` executable.
//!
//! Same rules as `ripplepath_engine::fixture`: `git` only ever runs on a repository this harness has
//! just created from generated content, never on a repository under analysis; arguments go through
//! a vector, never a shell; hooks are disabled and identity/dates are fixed so commit ids are
//! reproducible. A separate driver exists because the benchmark commits *edits* on top of a
//! 20k-file tree; the fixture builder re-copies whole snapshot directories per commit.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`git {args}` failed: {stderr}")]
    Git { args: String, stderr: String },
}

/// Creates `dir` as an empty repository on branch `main`; it must not contain a repository yet.
pub fn init(dir: &Path) -> Result<(), GitError> {
    std::fs::create_dir_all(dir).map_err(|source| GitError::Io { path: dir.to_owned(), source })?;
    run(dir, &["init", "--quiet", "--initial-branch=main"], 0).map(drop)
}

/// Writes `files` (relative, `/`-separated paths) under `dir`, creating directories as needed.
pub fn write_files(dir: &Path, files: &[(String, String)]) -> Result<(), GitError> {
    for (path, content) in files {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| GitError::Io { path: parent.to_owned(), source })?;
        }
        std::fs::write(&target, content).map_err(|source| GitError::Io { path: target.clone(), source })?;
    }
    Ok(())
}

/// Stages everything and commits; `sequence` fixes the commit date. Returns the commit id.
pub fn commit_all(dir: &Path, message: &str, sequence: usize) -> Result<String, GitError> {
    run(dir, &["add", "--all"], sequence)?;
    run(dir, &["commit", "--quiet", "--allow-empty", "-m", message], sequence)?;
    Ok(run(dir, &["rev-parse", "HEAD"], sequence)?.trim().to_owned())
}

fn run(dir: &Path, args: &[&str], sequence: usize) -> Result<String, GitError> {
    let date = format!("{} +0000", 1_767_225_600 + sequence * 86_400);
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-c", "core.hooksPath="])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Ripplepath Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@ripplepath.invalid")
        .env("GIT_COMMITTER_NAME", "Ripplepath Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@ripplepath.invalid")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|source| GitError::Io { path: dir.to_owned(), source })?;
    if !output.status.success() {
        return Err(GitError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
