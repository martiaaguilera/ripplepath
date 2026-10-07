#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The mandatory invariant: for any sequence of index runs, the stored index equals a clean index
//! of the final revision — and analyses with a warm persistent cache equal analyses without one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{AnalyzeOptions, Limits, analyze, index_revision};
use ripplepath_storage::Store;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

fn stored(db: &Path) -> ripplepath_storage::IndexedGraph {
    Store::open(db).unwrap().load_graph().unwrap().unwrap()
}

#[test]
fn incremental_index_of_fixtures_equals_clean_index() {
    for name in ["java-banking", "typescript-checkout"] {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        build_fixture_repo(&[&fixture(name).join("v1"), &fixture(name).join("v2")], &repo).unwrap();
        let limits = Limits::default();

        let incremental = dir.path().join("incremental.db");
        let first = index_revision(&repo, "main~1", &incremental, &limits).unwrap();
        assert_eq!(first.reused, 0);
        let second = index_revision(&repo, "main", &incremental, &limits).unwrap();
        assert!(second.reused > 0, "{name}: unchanged files must come from the cache");
        assert!(second.parsed < first.parsed, "{name}: only changed files are parsed");

        let clean = dir.path().join("clean.db");
        index_revision(&repo, "main", &clean, &limits).unwrap();
        assert_eq!(stored(&incremental), stored(&clean), "{name}");

        // Re-indexing the same revision is a no-op.
        let again = index_revision(&repo, "main", &incremental, &limits).unwrap();
        assert_eq!(again.parsed, 0);
        assert_eq!(again.delta, Default::default());
    }
}

#[test]
fn warm_cache_analysis_equals_cold_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let root = fixture("typescript-checkout");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], &repo).unwrap();

    let cold = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();
    let mut with_db = AnalyzeOptions::new(&repo, "main~1", "main");
    with_db.db = Some(dir.path().join("cache.db"));
    let first = analyze(&with_db).unwrap();
    let warm = analyze(&with_db).unwrap();
    let json = |r| serde_json::to_string(&r).unwrap();
    assert_eq!(json(cold.clone()), json(first));
    assert_eq!(json(cold), json(warm));
}

/// A small project of 5 Java classes and 5 TS modules whose dependencies depend on a per-file
/// variant, so edits add and remove edges, symbols and unresolved references.
#[derive(Clone, Debug)]
enum Op {
    Set { file: usize, variant: u8 },
    Remove { file: usize },
    Move { file: usize },
}

const FILES: usize = 10;

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..FILES, 0u8..4).prop_map(|(file, variant)| Op::Set { file, variant }),
        1 => (0..FILES).prop_map(|file| Op::Remove { file }),
        1 => (0..FILES).prop_map(|file| Op::Move { file }),
    ]
}

/// file index → (variant, moved)
type State = BTreeMap<usize, (u8, bool)>;

fn render(state: &State, root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    for (&file, &(variant, moved)) in state {
        let dir = if moved { "moved" } else { "src" };
        let (path, text) = if file < 5 {
            let target = (file + variant as usize) % 5;
            (
                format!("{dir}/p/C{file}.java"),
                format!(
                    "package p;\n\npublic class C{file} {{\n    public int run() {{\n        return new C{target}().value() + {variant};\n    }}\n\n    public int value() {{\n        return {variant};\n    }}\n}}\n"
                ),
            )
        } else {
            let k = file - 5;
            let target = (k + variant as usize) % 5;
            let import =
                if target == k { String::new() } else { format!("import {{ f{target} }} from \"./m{target}\";\n\n") };
            (
                format!("{dir}/ts/m{k}.ts"),
                format!(
                    "{import}export function f{k}(): number {{\n  return {variant}{};\n}}\n",
                    if target == k { String::new() } else { format!(" + f{target}()") }
                ),
            )
        };
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    // An empty snapshot still needs one file so that `git commit` has a tree to record.
    std::fs::write(root.join("README"), "fixture\n").unwrap();
}

fn apply(state: &mut State, op: &Op) {
    match *op {
        Op::Set { file, variant } => {
            let moved = state.get(&file).is_some_and(|&(_, m)| m);
            state.insert(file, (variant, moved));
        }
        Op::Remove { file } => {
            state.remove(&file);
        }
        Op::Move { file } => {
            if let Some(entry) = state.get_mut(&file) {
                entry.1 = !entry.1;
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, ..ProptestConfig::default() })]

    #[test]
    fn incremental_index_equals_clean_index_for_random_histories(ops in prop::collection::vec(prop::collection::vec(op(), 1..4), 1..4)) {
        let dir = tempfile::tempdir().unwrap();
        let mut state: State = (0..FILES).map(|f| (f, (0, false))).collect();
        let mut snapshots = Vec::new();
        render(&state, &dir.path().join("s0"));
        snapshots.push(dir.path().join("s0"));
        for (i, batch) in ops.iter().enumerate() {
            for op in batch {
                apply(&mut state, op);
            }
            let snapshot = dir.path().join(format!("s{}", i + 1));
            render(&state, &snapshot);
            snapshots.push(snapshot);
        }
        let repo = dir.path().join("repo");
        let refs: Vec<&Path> = snapshots.iter().map(PathBuf::as_path).collect();
        build_fixture_repo(&refs, &repo).unwrap();

        let limits = Limits::default();
        let incremental = dir.path().join("incremental.db");
        for back in (0..snapshots.len()).rev() {
            let rev = if back == 0 { "main".to_owned() } else { format!("main~{back}") };
            index_revision(&repo, &rev, &incremental, &limits).unwrap();
        }
        let clean = dir.path().join("clean.db");
        index_revision(&repo, "main", &clean, &limits).unwrap();
        prop_assert_eq!(stored(&incremental), stored(&clean));
    }
}
