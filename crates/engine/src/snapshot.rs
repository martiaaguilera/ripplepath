use std::collections::HashMap;
use std::sync::Arc;

use rayon::prelude::*;
use ripplepath_core::Language;
use ripplepath_git::{BlobContent, EntryKind, ObjectId, PathRejection, Repo, Revision};
use ripplepath_graph::CodeGraph;
use ripplepath_lang::UnresolvedRef;
use ripplepath_lang::java::{self, facts::JavaFile};

use crate::Limits;
use crate::analysis::AnalysisError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexStatus {
    Indexed,
    /// Not a language Ripplepath analyses.
    NotSource,
    TooLarge {
        size: u64,
    },
    Binary,
    ParseFailed {
        message: String,
    },
    Symlink,
    Submodule,
}

#[derive(Clone, Debug)]
pub struct SnapshotFile {
    pub path: String,
    pub blob: ObjectId,
    pub language: Option<Language>,
    pub status: IndexStatus,
    pub syntax_error_lines: Vec<u32>,
}

pub struct Snapshot {
    pub revision: Revision,
    /// Sorted by path.
    pub files: Vec<SnapshotFile>,
    pub rejected_paths: Vec<(String, PathRejection)>,
    pub graph: CodeGraph,
    pub unresolved: Vec<UnresolvedRef>,
}

impl Snapshot {
    pub fn file(&self, path: &str) -> Option<&SnapshotFile> {
        self.files.binary_search_by(|f| f.path.as_str().cmp(path)).ok().map(|i| &self.files[i])
    }
}

type CacheKey = (ObjectId, String);

/// Per-file extraction results keyed by (blob id, path).
///
/// Blob ids are content hashes, so a file unchanged between base and head — or between two runs —
/// is parsed once. The path is part of the key because Java facts record it (and TypeScript module
/// resolution will depend on it). Failures are cached too: re-parsing a file that timed out would
/// just time out again.
#[derive(Default)]
pub struct FactCache {
    java: HashMap<CacheKey, Result<Arc<JavaFile>, String>>,
    pub hits: usize,
    pub misses: usize,
}

fn language_of(path: &str) -> Option<Language> {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("java") => Some(Language::Java),
        _ => None,
    }
}

pub fn build_snapshot(
    repo: &Repo,
    revision: Revision,
    cache: &mut FactCache,
    limits: &Limits,
) -> Result<Snapshot, AnalysisError> {
    let listing = repo.list_files(revision.tree)?;
    if listing.files.len() > limits.max_files {
        return Err(AnalysisError::TooManyFiles { count: listing.files.len(), limit: limits.max_files });
    }

    let mut files = Vec::with_capacity(listing.files.len());
    let mut to_parse: Vec<(CacheKey, String)> = Vec::new();
    for entry in &listing.files {
        let language = language_of(&entry.path);
        let blob = entry.blob;
        let mut status = match (entry.kind, language) {
            (EntryKind::Symlink, _) => IndexStatus::Symlink,
            (EntryKind::Submodule, _) => IndexStatus::Submodule,
            (_, None) => IndexStatus::NotSource,
            (_, Some(_)) => IndexStatus::Indexed,
        };
        if status == IndexStatus::Indexed {
            let key = (blob, entry.path.clone());
            if cache.java.contains_key(&key) {
                cache.hits += 1;
            } else {
                cache.misses += 1;
                match repo.read_text(entry.blob, limits.max_file_bytes)? {
                    BlobContent::Text(text) => to_parse.push((key, text)),
                    BlobContent::Binary => status = IndexStatus::Binary,
                    BlobContent::TooLarge { size } => status = IndexStatus::TooLarge { size },
                }
            }
        }
        files.push(SnapshotFile { path: entry.path.clone(), blob, language, status, syntax_error_lines: Vec::new() });
    }

    // Parsing dominates indexing time and is independent per file, so it is the one parallel
    // stage. Rayon's pool is bounded by the number of cores.
    let budget = limits.parse_budget;
    let parsed: Vec<(CacheKey, Result<Arc<JavaFile>, String>)> = to_parse
        .into_par_iter()
        .map(|(key, text)| {
            let result = java::extract(&key.1, &text, budget).map(Arc::new).map_err(|e| e.to_string());
            (key, result)
        })
        .collect();
    cache.java.extend(parsed);

    let mut facts: Vec<Arc<JavaFile>> = Vec::new();
    for file in &mut files {
        if file.status != IndexStatus::Indexed {
            continue;
        }
        match cache.java.get(&(file.blob, file.path.clone())) {
            Some(Ok(java_file)) => {
                file.syntax_error_lines = java_file.syntax_error_lines.clone();
                facts.push(Arc::clone(java_file));
            }
            Some(Err(message)) => file.status = IndexStatus::ParseFailed { message: message.clone() },
            // Read as binary/oversized above, so never inserted.
            None => {}
        }
    }

    let borrowed: Vec<&JavaFile> = facts.iter().map(Arc::as_ref).collect();
    let resolved = java::resolve(&borrowed);
    let graph = CodeGraph::new(resolved.symbols, resolved.edges);
    tracing::debug!(
        revision = %revision.spec,
        files = files.len(),
        symbols = graph.symbol_count(),
        edges = graph.edges().len(),
        "snapshot indexed"
    );
    Ok(Snapshot { revision, files, rejected_paths: listing.rejected, graph, unresolved: resolved.unresolved })
}
