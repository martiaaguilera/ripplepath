//! Persistent, incremental indexing of one revision.

use std::path::Path;
use std::time::Instant;

use ripplepath_git::Repo;
use ripplepath_storage::{GraphDelta, IndexedFile, IndexedGraph, Store};

use crate::snapshot::{FactCache, IndexStatus, Snapshot, build_snapshot};
use crate::{AnalysisError, Limits};

#[derive(Clone, Debug)]
pub struct IndexOutcome {
    pub tree: String,
    pub commit: Option<String>,
    pub files: usize,
    /// Source files parsed in this run.
    pub parsed: usize,
    /// Source files whose facts came from the cache.
    pub reused: usize,
    pub symbols: usize,
    pub edges: usize,
    pub delta: GraphDelta,
    pub elapsed_ms: u128,
}

/// Indexes `revision` into the store at `db`, reusing cached facts and writing only changed rows.
pub fn index_revision(
    repo_path: &Path,
    revision: &str,
    db: &Path,
    limits: &Limits,
) -> Result<IndexOutcome, AnalysisError> {
    let started = Instant::now();
    let repo = Repo::open(repo_path)?;
    let resolved = repo.resolve(revision)?;
    let mut cache = FactCache::with_store(Store::open(db)?);
    let snapshot = build_snapshot(&repo, resolved, &mut cache, limits)?;
    let (parsed, reused) = (cache.misses, cache.hits);
    let graph = indexed_graph(&snapshot);
    let (tree, commit, files, symbols, edges) =
        (graph.tree.clone(), graph.commit.clone(), graph.files.len(), graph.symbols.len(), graph.edges.len());
    let mut store = cache.into_store().ok_or(AnalysisError::NoStore)?;
    let delta = store.apply_graph(graph)?;
    let outcome = IndexOutcome {
        tree,
        commit,
        files,
        parsed,
        reused,
        symbols,
        edges,
        delta,
        elapsed_ms: started.elapsed().as_millis(),
    };
    tracing::info!(?outcome, "index updated");
    Ok(outcome)
}

pub fn indexed_graph(snapshot: &Snapshot) -> IndexedGraph {
    let status = |s: &IndexStatus| match s {
        IndexStatus::Indexed => "indexed",
        IndexStatus::NotSource => "not_source",
        IndexStatus::TooLarge { .. } => "too_large",
        IndexStatus::Binary => "binary",
        IndexStatus::ParseFailed { .. } => "parse_failed",
        IndexStatus::Symlink => "symlink",
        IndexStatus::Submodule => "submodule",
        IndexStatus::Excluded { .. } => "excluded",
    };
    let mut graph = IndexedGraph {
        tree: snapshot.revision.tree.to_string(),
        commit: snapshot.revision.commit.clone(),
        files: snapshot
            .files
            .iter()
            .map(|f| IndexedFile {
                path: f.path.clone(),
                blob: f.blob.to_string(),
                language: f.language,
                status: status(&f.status).to_owned(),
            })
            .collect(),
        symbols: snapshot.graph.symbols().cloned().collect(),
        edges: snapshot.graph.edges().to_vec(),
        unresolved: snapshot
            .unresolved
            .iter()
            .map(|u| (u.from.clone(), u.file.clone(), u.line, u.detail.clone()))
            .collect(),
    };
    graph.normalize();
    graph
}
