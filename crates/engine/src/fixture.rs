//! Builds Git repositories from Ripplepath's own snapshot fixtures (`fixtures/<name>/v1`, `v2`, …).
//!
//! This is the only place Ripplepath runs the `git` executable, and it only ever does so on a
//! repository it has just created from files shipped in this project — never on a user's or an
//! analysed repository (see docs/SECURITY_MODEL.md). Arguments are passed as a vector, never through
//! a shell, and author/committer identity and dates are fixed so commit ids are reproducible.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("fixture snapshot directory {0} does not exist")]
    MissingSnapshot(PathBuf),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`git {args}` failed: {stderr}")]
    Git { args: String, stderr: String },
}

/// Creates `target` as a Git repository with one commit per snapshot directory, in order, on branch
/// `main`. Returns the commit ids.
pub fn build_fixture_repo(snapshots: &[&Path], target: &Path) -> Result<Vec<String>, FixtureError> {
    std::fs::create_dir_all(target).map_err(|source| FixtureError::Io { path: target.to_owned(), source })?;
    git(target, &["init", "--quiet", "--initial-branch=main"], 0)?;
    let mut commits = Vec::new();
    for (i, snapshot) in snapshots.iter().enumerate() {
        if !snapshot.is_dir() {
            return Err(FixtureError::MissingSnapshot(snapshot.to_path_buf()));
        }
        clear_worktree(target)?;
        copy_dir(snapshot, target)?;
        git(target, &["add", "--all"], i)?;
        let name = snapshot.file_name().map_or_else(|| format!("v{}", i + 1), |n| n.to_string_lossy().into_owned());
        git(target, &["commit", "--quiet", "--allow-empty", "-m", &format!("fixture: {name}")], i)?;
        commits.push(git(target, &["rev-parse", "HEAD"], i)?.trim().to_owned());
    }
    Ok(commits)
}

fn git(dir: &Path, args: &[&str], sequence: usize) -> Result<String, FixtureError> {
    // One day apart per commit, from a fixed epoch.
    let date = format!("{} +0000", 1_767_225_600 + sequence * 86_400);
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-c", "core.hooksPath="])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Ripplepath Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@ripplepath.invalid")
        .env("GIT_COMMITTER_NAME", "Ripplepath Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@ripplepath.invalid")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
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
        let path = entry.map_err(io)?.path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        let result = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        result.map_err(|source| FixtureError::Io { path: path.clone(), source })?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), FixtureError> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| FixtureError::Io { path, source }
    };
    for entry in std::fs::read_dir(from).map_err(io(from))? {
        let entry = entry.map_err(io(from))?;
        let source = entry.path();
        let dest = to.join(entry.file_name());
        if source.is_dir() {
            std::fs::create_dir_all(&dest).map_err(io(&dest))?;
            copy_dir(&source, &dest)?;
        } else {
            // Normalise line endings: a Windows checkout of this project may have converted the
            // fixtures to CRLF, which would change blob ids and line-based expectations.
            let bytes = std::fs::read(&source).map_err(io(&source))?;
            let normalized =
                String::from_utf8(bytes.clone()).map_or(bytes, |text| text.replace("\r\n", "\n").into_bytes());
            std::fs::write(&dest, normalized).map_err(io(&dest))?;
        }
    }
    Ok(())
}
