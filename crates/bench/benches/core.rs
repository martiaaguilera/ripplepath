//! Criterion micro-benchmarks of the pure, in-memory stages: per-file extraction, resolution, graph
//! construction and impact traversal, on deterministic synthetic sources (1,000 files, seed 42).
//! End-to-end workloads (Git, SQLite, processes) live in the `ripplepath-bench` binary.

#![allow(clippy::expect_used)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ripplepath_bench::rng::SplitMix64;
use ripplepath_bench::synth::{SynthParams, SynthRepo};
use ripplepath_core::SymbolId;
use ripplepath_graph::{CodeGraph, ImpactOptions, impact};
use ripplepath_lang::java::{self, facts::JavaFile};
use ripplepath_lang::ts::{self, facts::TsFile};
use ripplepath_storage::{IndexedGraph, Store};

const BUDGET: Duration = Duration::from_secs(2);

struct Corpus {
    sources: Vec<(String, String)>,
    java: Vec<JavaFile>,
    ts: Vec<TsFile>,
}

fn corpus() -> Corpus {
    let sources: Vec<(String, String)> = SynthRepo::generate(SynthParams::new(1_000, 42))
        .render_all()
        .into_iter()
        .filter(|(p, _)| p.ends_with(".java") || p.ends_with(".ts"))
        .collect();
    let java = sources
        .iter()
        .filter(|(p, _)| p.ends_with(".java"))
        .map(|(p, s)| java::extract(p, s, BUDGET).expect("generated Java parses"))
        .collect();
    let ts = sources
        .iter()
        .filter(|(p, _)| p.ends_with(".ts"))
        .map(|(p, s)| ts::extract(p, s, BUDGET).expect("generated TypeScript parses"))
        .collect();
    Corpus { sources, java, ts }
}

fn graph_of(corpus: &Corpus) -> CodeGraph {
    let java = java::resolve(&corpus.java.iter().collect::<Vec<_>>());
    let ts = ts::resolve(&corpus.ts.iter().collect::<Vec<_>>());
    CodeGraph::new([java.symbols, ts.symbols].concat(), [java.edges, ts.edges].concat())
}

fn benches(c: &mut Criterion) {
    let corpus = corpus();
    // A service file of each language: the most common and largest generated shape.
    let pick = |suffix: &str| {
        corpus.sources.iter().find(|(p, s)| p.ends_with(suffix) && s.contains("compute(")).expect("service file exists")
    };
    let (java_path, java_src) = pick("x10.java");
    let (ts_path, ts_src) = pick("svc10.ts");
    c.bench_function("extract/java_service_file", |b| b.iter(|| java::extract(java_path, black_box(java_src), BUDGET)));
    c.bench_function("extract/ts_service_file", |b| b.iter(|| ts::extract(ts_path, black_box(ts_src), BUDGET)));

    let java_refs: Vec<&JavaFile> = corpus.java.iter().collect();
    let ts_refs: Vec<&TsFile> = corpus.ts.iter().collect();
    c.bench_function("resolve/java_500_files", |b| b.iter(|| java::resolve(black_box(&java_refs))));
    c.bench_function("resolve/ts_500_files", |b| b.iter(|| ts::resolve(black_box(&ts_refs))));

    let graph = graph_of(&corpus);
    let symbols: Vec<_> = graph.symbols().cloned().collect();
    let edges = graph.edges().to_vec();
    c.bench_function("graph/code_graph_new_1k_files", |b| {
        b.iter_batched(
            || (symbols.clone(), edges.clone()),
            |(s, e)| CodeGraph::new(s, e),
            criterion::BatchSize::LargeInput,
        )
    });

    let ids: Vec<SymbolId> = graph.symbols().map(|s| s.id.clone()).collect();
    let mut rng = SplitMix64::new(7);
    let roots: Vec<SymbolId> = (0..256).map(|_| ids[rng.below(ids.len())].clone()).collect();
    let mut next = 0usize;
    c.bench_function("graph/impact_random_root_1k_files", |b| {
        b.iter(|| {
            next = (next + 1) % roots.len();
            impact(&graph, std::slice::from_ref(&roots[next]), ImpactOptions::default())
        })
    });
}

/// The persisted graph: writing it into an empty database (cold index), loading it, and applying
/// an unchanged graph (the diff every warm index run performs).
fn storage(c: &mut Criterion) {
    let graph = graph_of(&corpus());
    let indexed = IndexedGraph {
        tree: "t".into(),
        commit: None,
        files: Vec::new(),
        symbols: graph.symbols().cloned().collect(),
        edges: graph.edges().to_vec(),
        unresolved: Vec::new(),
    };
    let dir = tempfile::tempdir().expect("temp dir");
    let mut group = c.benchmark_group("storage");
    group.sample_size(10);
    let mut fresh = 0usize;
    group.bench_function("apply_graph_into_empty_db_1k_files", |b| {
        b.iter_batched(
            || {
                fresh += 1;
                (Store::open(&dir.path().join(format!("cold-{fresh}.db"))).expect("open"), indexed.clone())
            },
            |(mut store, next)| {
                store.apply_graph(next).expect("apply");
                store
            },
            criterion::BatchSize::PerIteration,
        )
    });
    let mut store = Store::open(&dir.path().join("warm.db")).expect("open");
    store.apply_graph(indexed.clone()).expect("apply");
    group.bench_function("load_graph_1k_files", |b| b.iter(|| store.load_graph().expect("load")));
    group.bench_function("apply_graph_unchanged_1k_files", |b| {
        b.iter_batched(|| indexed.clone(), |next| store.apply_graph(next).expect("apply"), criterion::BatchSize::LargeInput)
    });
    group.finish();
}

criterion_group!(core, benches, storage);
criterion_main!(core);
