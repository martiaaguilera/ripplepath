use std::collections::HashMap;
use std::sync::Arc;

use rayon::prelude::*;
use ripplepath_core::Language;
use ripplepath_git::{BlobContent, EntryKind, ObjectId, PathRejection, Repo, Revision};
use ripplepath_graph::CodeGraph;
use ripplepath_lang::java::{self, facts::JavaFile};
use ripplepath_lang::ts::{self, facts::TsFile};
use ripplepath_lang::{LanguageGraph, UnresolvedRef};
use ripplepath_storage::{CachedFacts, StorageError, Store};

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
    /// Source in a supported language that is deliberately not indexed (vendored or minified).
    Excluded {
        reason: &'static str,
    },
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
/// is parsed once. The path is part of the key because facts record it and TypeScript module
/// resolution depends on it. Failures are cached in memory for the run (base and head share
/// unchanged files, and re-parsing a file that just timed out would time out again) but are never
/// persisted: every extraction failure is a wall-clock timeout or a grammar load error, neither a
/// property of the file, and a stored timeout from a loaded machine would make every later run
/// differ from a clean one.
///
/// With a [`Store`] attached, results also persist across runs: that is what makes re-indexing
/// after a small change cheap. Persisted entries are keyed by extractor version, so a new extractor
/// never reuses facts produced by an old one.
#[derive(Default)]
pub struct FactCache {
    facts: HashMap<CacheKey, Result<Facts, String>>,
    store: Option<Store>,
    pending: Vec<(String, String, String, CachedFacts)>,
    /// Found in memory or in the store.
    pub hits: usize,
    /// Parsed in this run.
    pub misses: usize,
}

impl FactCache {
    pub fn with_store(store: Store) -> Self {
        Self { store: Some(store), ..Self::default() }
    }

    pub fn store_mut(&mut self) -> Option<&mut Store> {
        self.store.as_mut()
    }

    pub fn into_store(self) -> Option<Store> {
        self.store
    }

    /// Writes facts parsed in this run to the store.
    pub fn flush(&mut self) -> Result<(), StorageError> {
        if let Some(store) = self.store.as_mut()
            && !self.pending.is_empty()
        {
            store.put_facts(&self.pending)?;
        }
        self.pending.clear();
        Ok(())
    }

    fn lookup(&mut self, key: &CacheKey, language: Language) -> bool {
        if self.facts.contains_key(key) {
            return true;
        }
        let Some(store) = self.store.as_ref() else {
            return false;
        };
        let cached = store.cached_facts(&key.0.to_string(), &key.1, extractor_key(language));
        let restored = match cached {
            Ok(Some(CachedFacts::Ok(bytes))) => decode(language, &bytes).map(Ok),
            // Failures stored by earlier versions are not trusted (see the type's comment); a read
            // error or an undecodable entry is a cache miss too: parsing again is always correct.
            Ok(Some(CachedFacts::Error(_)) | None) | Err(_) => None,
        };
        match restored {
            Some(result) => {
                self.facts.insert(key.clone(), result);
                true
            }
            None => false,
        }
    }

    fn insert(&mut self, key: CacheKey, language: Language, result: Result<Facts, String>) {
        if self.store.is_some() {
            let entry = match &result {
                Ok(facts) => encode(facts).map(CachedFacts::Ok),
                Err(_) => None,
            };
            if let Some(entry) = entry {
                self.pending.push((key.0.to_string(), key.1.clone(), extractor_key(language).to_owned(), entry));
            }
        }
        self.facts.insert(key, result);
    }
}

fn extractor_key(language: Language) -> &'static str {
    // Bump together with `EXTRACTOR_VERSION` in the frontends.
    const _: () = assert!(java::EXTRACTOR_VERSION == 4 && ts::EXTRACTOR_VERSION == 2);
    match language {
        Language::Java => "java/4",
        Language::TypeScript | Language::JavaScript => "ts/2",
    }
}

fn encode(facts: &Facts) -> Option<Vec<u8>> {
    match facts {
        Facts::Java(f) => serde_json::to_vec(f.as_ref()).ok(),
        Facts::Ts(f) => serde_json::to_vec(f.as_ref()).ok(),
    }
}

fn decode(language: Language, bytes: &[u8]) -> Option<Facts> {
    match language {
        Language::Java => serde_json::from_slice(bytes).ok().map(|f| Facts::Java(Arc::new(f))),
        Language::TypeScript | Language::JavaScript => {
            serde_json::from_slice(bytes).ok().map(|f| Facts::Ts(Arc::new(f)))
        }
    }
}

#[derive(Clone)]
enum Facts {
    Java(Arc<JavaFile>),
    Ts(Arc<TsFile>),
}

impl Facts {
    fn syntax_error_lines(&self) -> &[u32] {
        match self {
            Self::Java(f) => &f.syntax_error_lines,
            Self::Ts(f) => &f.syntax_error_lines,
        }
    }
}

pub(crate) fn language_of(path: &str) -> Option<Language> {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("java") => Some(Language::Java),
        Some("ts" | "tsx" | "mts" | "cts") => Some(Language::TypeScript),
        Some("js" | "jsx" | "mjs" | "cjs") => Some(Language::JavaScript),
        _ => None,
    }
}

/// Source code in a language Ripplepath does not analyse. A change to it can break code and tests
/// through edges the graph does not have, so it is uncertainty that widens the test selection, not
/// a footnote like a changed README.
pub(crate) fn unanalysed_source_language(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.').map(|(_, ext)| ext)?;
    Some(match ext {
        "kt" | "kts" => "Kotlin",
        "scala" | "sc" => "Scala",
        "groovy" | "gvy" => "Groovy",
        "vue" | "svelte" | "astro" => "a web component format",
        "py" => "Python",
        "go" => "Go",
        "rs" => "Rust",
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" => "C/C++",
        "cs" | "fs" => ".NET",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "dart" => "Dart",
        "ex" | "exs" => "Elixir",
        "clj" | "cljs" => "Clojure",
        _ => return None,
    })
}

/// Dependencies checked into the tree and minified bundles are not the repository's own code;
/// indexing them would multiply analysis time and drown real dependents. Reported, not hidden.
fn exclusion(path: &str) -> Option<&'static str> {
    if path.split('/').any(|c| c == "node_modules") {
        Some("vendored dependency (node_modules)")
    } else if path.ends_with(".min.js") || path.ends_with(".bundle.js") {
        Some("minified bundle")
    } else {
        None
    }
}

fn extract(language: Language, path: &str, text: &str, limits: &Limits) -> Result<Facts, String> {
    let budget = limits.parse_budget;
    match language {
        Language::Java => java::extract(path, text, budget).map(|f| Facts::Java(Arc::new(f))),
        Language::TypeScript | Language::JavaScript => ts::extract(path, text, budget).map(|f| Facts::Ts(Arc::new(f))),
    }
    .map_err(|e| e.to_string())
}

/// Each language resolves on its own; there are no cross-language edges.
fn merge(parts: [LanguageGraph; 2]) -> LanguageGraph {
    let mut merged = LanguageGraph::default();
    for part in parts {
        merged.symbols.extend(part.symbols);
        merged.edges.extend(part.edges);
        merged.unresolved.extend(part.unresolved);
    }
    merged.symbols.sort_by(|a, b| a.id.cmp(&b.id));
    merged.edges.sort();
    merged.unresolved.sort();
    merged
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
    let mut to_parse: Vec<(CacheKey, String, Option<Language>)> = Vec::new();
    for entry in &listing.files {
        let language = language_of(&entry.path);
        let blob = entry.blob;
        let mut status = match (entry.kind, language) {
            (EntryKind::Symlink, _) => IndexStatus::Symlink,
            (EntryKind::Submodule, _) => IndexStatus::Submodule,
            (_, None) => IndexStatus::NotSource,
            (_, Some(_)) => {
                exclusion(&entry.path).map_or(IndexStatus::Indexed, |reason| IndexStatus::Excluded { reason })
            }
        };
        if let (IndexStatus::Indexed, Some(lang)) = (&status, language) {
            let key = (blob, entry.path.clone());
            if cache.lookup(&key, lang) {
                cache.hits += 1;
            } else {
                cache.misses += 1;
                match repo.read_text(entry.blob, limits.max_file_bytes)? {
                    BlobContent::Text(text) => to_parse.push((key, text, language)),
                    BlobContent::Binary => status = IndexStatus::Binary,
                    BlobContent::TooLarge { size } => status = IndexStatus::TooLarge { size },
                }
            }
        }
        files.push(SnapshotFile { path: entry.path.clone(), blob, language, status, syntax_error_lines: Vec::new() });
    }

    // Parsing dominates indexing time and is independent per file, so it is the one parallel
    // stage. Rayon's pool is bounded by the number of cores.
    let parsed: Vec<(CacheKey, Language, Result<Facts, String>)> = to_parse
        .into_par_iter()
        .filter_map(|(key, text, language)| {
            let language = language?;
            let result = extract(language, &key.1, &text, limits);
            Some((key, language, result))
        })
        .collect();
    for (key, language, result) in parsed {
        cache.insert(key, language, result);
    }
    cache.flush()?;

    let mut java_facts: Vec<Arc<JavaFile>> = Vec::new();
    let mut ts_facts: Vec<Arc<TsFile>> = Vec::new();
    for file in &mut files {
        if file.status != IndexStatus::Indexed {
            continue;
        }
        match cache.facts.get(&(file.blob, file.path.clone())) {
            Some(Ok(facts)) => {
                file.syntax_error_lines = facts.syntax_error_lines().to_vec();
                match facts {
                    Facts::Java(f) => java_facts.push(Arc::clone(f)),
                    Facts::Ts(f) => ts_facts.push(Arc::clone(f)),
                }
            }
            Some(Err(message)) => file.status = IndexStatus::ParseFailed { message: message.clone() },
            // Read as binary/oversized above, so never inserted.
            None => {}
        }
    }

    let java_refs: Vec<&JavaFile> = java_facts.iter().map(Arc::as_ref).collect();
    let ts_refs: Vec<&TsFile> = ts_facts.iter().map(Arc::as_ref).collect();
    let resolved = merge([java::resolve(&java_refs), ts::resolve(&ts_refs)]);
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
