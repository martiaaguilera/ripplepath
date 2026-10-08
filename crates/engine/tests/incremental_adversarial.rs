#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Adversarial histories for the incremental-index invariant (docs/FINAL_REVIEW.md): case-only
//! renames, file ↔ symlink mode changes, duplicate fully qualified names that change which file is
//! canonical, deletions, and a parse failure caused by the time budget rather than the input.
//!
//! Commits are written with Git plumbing (`hash-object`, `update-index --cacheinfo`, `write-tree`,
//! `commit-tree`), so symlink entries and case-only renames are recorded exactly, independent of
//! what the test machine's filesystem supports.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use ripplepath_engine::{AnalyzeOptions, Limits, analyze, index_revision};
use ripplepath_storage::Store;

const FILE: &str = "100644";
const LINK: &str = "120000";

fn git(repo: &Path, args: &[&str], stdin: Option<&str>) -> String {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/nonexistent"])
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t.invalid")
        .env("GIT_AUTHOR_DATE", "1767225600 +0000")
        .env("GIT_COMMITTER_DATE", "1767225600 +0000")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Commits exactly `entries` (mode, path, content) as the next commit on `main`.
fn commit(repo: &Path, entries: &[(&str, &str, &str)], parent: Option<&str>) -> String {
    let index = repo.join(".git").join("index");
    let _ = std::fs::remove_file(&index);
    for (mode, path, content) in entries {
        let blob = git(repo, &["hash-object", "-w", "--stdin"], Some(content));
        git(repo, &["update-index", "--add", "--cacheinfo", &format!("{mode},{blob},{path}")], None);
    }
    let tree = git(repo, &["write-tree"], None);
    let mut args = vec!["commit-tree", tree.as_str(), "-m", "step"];
    if let Some(p) = parent {
        args.extend(["-p", p]);
    }
    let id = git(repo, &args, None);
    git(repo, &["update-ref", "refs/heads/main", &id], None);
    id
}

fn stored(db: &Path) -> ripplepath_storage::IndexedGraph {
    Store::open(db).unwrap().load_graph().unwrap().unwrap()
}

const A: &str = "package p;\npublic class A {\n  public int run() { return new B().value(); }\n}\n";
const B_SRC: &str = "package p;\npublic class B {\n  public int value() { return 1; }\n}\n";
const B_LIB: &str =
    "package p;\npublic class B {\n  public int value() { return 2; }\n  public int other() { return 3; }\n}\n";
const TS_A: &str = "export function a(): number {\n  return 1;\n}\n";
const TS_B: &str = "import { a } from \"./a\";\n\nexport function b(): number {\n  return a() + 1;\n}\n";
const TS_A_UPPER: &str = "export function A(): number {\n  return 7;\n}\n";

#[test]
fn adversarial_history_index_equals_clean_index_at_every_step() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"], None);

    let steps: Vec<Vec<(&str, &str, &str)>> = vec![
        // Two declarations of p.B: src/ is canonical (first by path is lib/, see below).
        vec![
            (FILE, "src/p/A.java", A),
            (FILE, "src/p/B.java", B_SRC),
            (FILE, "lib/p/B.java", B_LIB),
            (FILE, "ts/a.ts", TS_A),
            (FILE, "ts/b.ts", TS_B),
        ],
        // Case-only rename of a Java file; the first p.B deleted so the other becomes canonical;
        // ts/a.ts becomes a symlink with the same blob; a case-variant module appears.
        vec![
            (FILE, "src/p/a.java", A),
            (FILE, "src/p/B.java", B_SRC),
            (LINK, "ts/a.ts", TS_A),
            (FILE, "ts/A.ts", TS_A_UPPER),
            (FILE, "ts/b.ts", TS_B),
        ],
        // Symlink back to a regular file; duplicate FQN re-added; the case variant removed.
        vec![
            (FILE, "src/p/a.java", A),
            (FILE, "src/p/B.java", B_SRC),
            (FILE, "lib/p/B.java", B_LIB),
            (FILE, "ts/a.ts", TS_A),
            (FILE, "ts/b.ts", TS_B),
        ],
        // Everything Java deleted, TS module renamed (stale edges must go).
        vec![(FILE, "ts/alpha.ts", TS_A), (FILE, "ts/b.ts", TS_B)],
    ];

    let limits = Limits::default();
    let incremental = dir.path().join("incremental.db");
    let mut parent: Option<String> = None;
    for (i, entries) in steps.iter().enumerate() {
        let id = commit(&repo, entries, parent.as_deref());
        index_revision(&repo, &id, &incremental, &limits).unwrap();
        let clean = dir.path().join(format!("clean{i}.db"));
        index_revision(&repo, &id, &clean, &limits).unwrap();
        assert_eq!(stored(&incremental), stored(&clean), "step {i}");
        parent = Some(id);
    }

    // The last step must not keep edges into the deleted Java files or the renamed module.
    let last = stored(&incremental);
    assert!(last.symbols.iter().all(|s| !s.id.as_str().starts_with("java:")), "{:?}", last.symbols);
    assert!(last.edges.iter().all(|e| !e.to.as_str().contains("ts/a.ts")), "{:?}", last.edges);

    // Walking back to an earlier revision with the warm database also equals a clean index.
    let first = git(&repo, &["rev-list", "--max-parents=0", "main"], None);
    index_revision(&repo, &first, &incremental, &limits).unwrap();
    let clean = dir.path().join("clean-back.db");
    index_revision(&repo, &first, &clean, &limits).unwrap();
    assert_eq!(stored(&incremental), stored(&clean));
}

#[test]
fn analyses_of_an_adversarial_history_are_identical_with_and_without_a_warm_cache() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"], None);
    let base = commit(
        &repo,
        &[(FILE, "src/p/A.java", A), (FILE, "src/p/B.java", B_SRC), (FILE, "ts/a.ts", TS_A), (FILE, "ts/b.ts", TS_B)],
        None,
    );
    let head = commit(
        &repo,
        &[(FILE, "src/p/a.java", A), (FILE, "lib/p/B.java", B_LIB), (LINK, "ts/a.ts", TS_A), (FILE, "ts/b.ts", TS_B)],
        Some(&base),
    );
    let cold = analyze(&AnalyzeOptions::new(&repo, &base, &head)).unwrap();
    let mut warm_options = AnalyzeOptions::new(&repo, &base, &head);
    warm_options.db = Some(dir.path().join("cache.db"));
    let first = analyze(&warm_options).unwrap();
    let warm = analyze(&warm_options).unwrap();
    let json = |r| serde_json::to_string(&r).unwrap();
    assert_eq!(json(cold.clone()), json(first));
    assert_eq!(json(cold), json(warm));
}

/// A parse that runs out of wall-clock time says nothing about the input: under load the same file
/// may parse next time. Persisting that failure would make every later run with this database
/// differ from a clean run (regression for a finding of the final review).
#[test]
fn a_timed_out_parse_is_not_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"], None);
    // Large enough that tree-sitter reports progress (and so checks the deadline) while parsing.
    let mut big = String::from("package p;\npublic class Big {\n");
    for i in 0..4000 {
        big.push_str(&format!("  public int m{i}(int x) {{ return x + {i}; }}\n"));
    }
    big.push_str("}\n");
    let id = commit(&repo, &[(FILE, "src/p/A.java", A), (FILE, "src/p/Big.java", &big)], None);

    let db = dir.path().join("index.db");
    let starved = Limits { parse_budget: Duration::ZERO, ..Limits::default() };
    index_revision(&repo, &id, &db, &starved).unwrap();
    let starved_graph = stored(&db);
    assert!(
        starved_graph.files.iter().any(|f| f.status == "parse_failed"),
        "a zero budget must time out: {:?}",
        starved_graph.files
    );

    let limits = Limits::default();
    let again = index_revision(&repo, &id, &db, &limits).unwrap();
    let failed = starved_graph.files.iter().filter(|f| f.status == "parse_failed").count();
    assert!(again.parsed >= failed, "timed-out files are parsed again, not reused: {again:?}");
    let clean = dir.path().join("clean.db");
    index_revision(&repo, &id, &clean, &limits).unwrap();
    assert_eq!(stored(&db), stored(&clean));
}
