//! `ripplepath-bench`: reproducible performance measurements of Ripplepath (docs/BENCHMARKS.md).
//!
//! `suite` orchestrates; every timed run happens in a fresh child process (`measure ...`) so that
//! "cold" really is cold for in-process state, and so each run's peak memory is its own. Timings are
//! taken inside the child around the library call only — process start-up, repository generation
//! and database preparation are never inside a measured interval.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use rayon::prelude::*;
use ripplepath_bench::host::{host_info, probe_self, system_load};
use ripplepath_bench::rng::SplitMix64;
use ripplepath_bench::stats::{Summary, summarize};
use ripplepath_bench::synth::{SynthParams, SynthRepo};
use ripplepath_core::SymbolId;
use ripplepath_engine::{AnalyzeOptions, FactCache, Limits, analyze, build_snapshot, index_revision, indexed_graph};
use ripplepath_graph::{CodeGraph, ImpactOptions, impact};
use ripplepath_storage::{GraphDelta, Store};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(name = "ripplepath-bench", about = "Ripplepath benchmark harness")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a synthetic repository (three commits: original, 1-file edit, 100-file edit).
    Synth {
        #[arg(long)]
        files: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long)]
        dir: PathBuf,
    },
    /// Run every workload and write results (JSON) plus Markdown tables (stdout).
    Suite(SuiteArgs),
    /// One measured run (used by `suite`; prints one JSON object).
    #[command(subcommand)]
    Measure(Measure),
    /// In-memory phase breakdown on a synthetic dataset: parse, cache encode/decode, resolve, graph.
    Profile {
        #[arg(long)]
        files: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 3)]
        iterations: usize,
    },
    /// Print the host description used in reports.
    Host,
}

#[derive(Args)]
struct SuiteArgs {
    /// Scratch directory for repositories and databases.
    #[arg(long)]
    work: PathBuf,
    /// Synthetic dataset size in source files (ignored with --repo).
    #[arg(long, default_value_t = 1000)]
    files: usize,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Benchmark an existing repository instead of a synthetic one (read-only; never modified).
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long, default_value = "HEAD~1")]
    base: String,
    #[arg(long, default_value = "HEAD")]
    head: String,
    #[arg(long, default_value_t = 5)]
    iterations: usize,
    /// Impact queries for the graph workload.
    #[arg(long, default_value_t = 1000)]
    queries: usize,
    /// Skip the per-run process probes (peak memory, CPU time); they add a PowerShell call per run
    /// on Windows.
    #[arg(long)]
    no_probe: bool,
    /// Results file (JSON).
    #[arg(long)]
    out: PathBuf,
}

#[derive(Subcommand)]
enum Measure {
    Index {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        rev: String,
        #[arg(long)]
        db: PathBuf,
        /// Time the pipeline stages separately (re-composes `index_revision` from its public parts).
        #[arg(long)]
        phases: bool,
        #[arg(long)]
        no_probe: bool,
    },
    Analyze {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        base: String,
        #[arg(long)]
        head: String,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        no_probe: bool,
    },
    Graph {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 1000)]
        queries: usize,
        #[arg(long, default_value_t = 7)]
        seed: u64,
        #[arg(long, default_value_t = 5)]
        builds: usize,
        #[arg(long)]
        no_probe: bool,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Synth { files, seed, dir } => synth(files, seed, &dir).map(|v| println!("{v:#}")),
        Cmd::Suite(args) => suite(&args),
        Cmd::Measure(m) => measure(m).map(|v| println!("{v}")),
        Cmd::Profile { files, seed, iterations } => profile(files, seed, iterations).map(|v| println!("{v:#}")),
        Cmd::Host => {
            println!("{:#}", json!(host_info()));
            Ok(())
        }
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

const SMALL_EDIT: usize = 1;
const LARGE_EDIT: usize = 100;

/// Builds (or reuses, when already built with the same parameters) the synthetic repository.
fn synth(files: usize, seed: u64, dir: &Path) -> Result<Value> {
    let marker = dir.join(".git").join("ripplepath-bench.json");
    let params = SynthParams::new(files, seed);
    if let Ok(existing) = std::fs::read_to_string(&marker) {
        let value: Value = serde_json::from_str(&existing)?;
        if value["params"] == json!(params) {
            return Ok(value);
        }
        return Err(format!("{} holds a different synthetic repository; remove it", dir.display()).into());
    }
    if dir.exists() && std::fs::read_dir(dir)?.next().is_some() {
        return Err(format!("{} exists and is not empty", dir.display()).into());
    }
    let started = Instant::now();
    let mut repo = SynthRepo::generate(params);
    ripplepath_bench::git::init(dir)?;
    ripplepath_bench::git::write_files(dir, &repo.render_all())?;
    let c0 = ripplepath_bench::git::commit_all(dir, "synth: original", 0)?;
    ripplepath_bench::git::write_files(dir, &repo.edit(SMALL_EDIT, 1))?;
    let c1 = ripplepath_bench::git::commit_all(dir, "synth: 1-file edit", 1)?;
    ripplepath_bench::git::write_files(dir, &repo.edit(LARGE_EDIT, 2))?;
    let c2 = ripplepath_bench::git::commit_all(dir, "synth: 100-file edit", 2)?;
    let value = json!({
        "params": params,
        "source_files": repo.source_files(),
        "commits": [c0, c1, c2],
        "build_seconds": started.elapsed().as_secs_f64(),
    });
    std::fs::write(&marker, value.to_string())?;
    Ok(value)
}

fn measure(m: Measure) -> Result<Value> {
    let limits = Limits::default();
    let (mut value, no_probe) = match m {
        Measure::Index { repo, rev, db, phases, no_probe } => {
            let value =
                if phases { index_phases(&repo, &rev, &db, &limits)? } else { index_once(&repo, &rev, &db, &limits)? };
            (value, no_probe)
        }
        Measure::Analyze { repo, base, head, db, no_probe } => {
            let mut options = AnalyzeOptions::new(&repo, base, head);
            options.db = db;
            let started = Instant::now();
            let report = analyze(&options)?;
            let elapsed_ms = ms(started);
            let s = &report.summary;
            let value = json!({
                "elapsed_ms": elapsed_ms,
                "files_changed": s.files_changed,
                "symbols_changed": s.symbols_changed,
                "symbols_impacted": s.symbols_impacted,
                "tests_recommended": s.tests_recommended,
                "tests_total": s.tests_total,
                "impact_truncated": s.impact_truncated,
            });
            (value, no_probe)
        }
        Measure::Graph { db, queries, seed, builds, no_probe } => {
            (graph_queries(&db, queries, seed, builds)?, no_probe)
        }
    };
    if !no_probe {
        let probe = probe_self();
        value["peak_rss_bytes"] = json!(probe.peak_rss_bytes);
        value["cpu_ms"] = json!(probe.cpu_ms);
    }
    Ok(value)
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1e3
}

fn delta_json(d: &GraphDelta) -> Value {
    json!({
        "files_changed": d.files_changed,
        "symbols": [d.symbols_added, d.symbols_removed, d.symbols_updated],
        "edges": [d.edges_added, d.edges_removed, d.edges_updated],
        "unresolved": [d.unresolved_added, d.unresolved_removed],
    })
}

fn db_bytes(db: &Path) -> u64 {
    let size = |p: PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    size(db.to_owned()) + size(sidecar(db, "-wal"))
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn index_once(repo: &Path, rev: &str, db: &Path, limits: &Limits) -> Result<Value> {
    let started = Instant::now();
    let outcome = index_revision(repo, rev, db, limits)?;
    let elapsed_ms = ms(started);
    Ok(json!({
        "elapsed_ms": elapsed_ms,
        "files": outcome.files,
        "parsed": outcome.parsed,
        "reused": outcome.reused,
        "symbols": outcome.symbols,
        "edges": outcome.edges,
        "delta": delta_json(&outcome.delta),
        "db_bytes": db_bytes(db),
    }))
}

/// The stages of `index_revision`, timed separately. Kept in step with `ripplepath_engine::index`;
/// the headline numbers always come from `index_once`, which calls the real function.
fn index_phases(repo: &Path, rev: &str, db: &Path, limits: &Limits) -> Result<Value> {
    let started = Instant::now();
    let t = Instant::now();
    let git = ripplepath_git::Repo::open(repo)?;
    let revision = git.resolve(rev)?;
    let store = Store::open(db)?;
    let open_ms = ms(t);

    // Measured on its own because `apply_graph` loads the previous graph internally.
    let t = Instant::now();
    let previous = store.load_graph()?;
    let load_previous_ms = ms(t);
    drop(previous);

    let t = Instant::now();
    let mut cache = FactCache::with_store(store);
    let snapshot = build_snapshot(&git, revision, &mut cache, limits)?;
    let snapshot_ms = ms(t);
    let (parsed, reused) = (cache.misses, cache.hits);

    let t = Instant::now();
    let graph = indexed_graph(&snapshot);
    let convert_ms = ms(t);

    let t = Instant::now();
    let mut store = cache.into_store().ok_or("fact cache lost its store")?;
    let delta = store.apply_graph(graph)?;
    let apply_ms = ms(t);
    let t = Instant::now();
    drop(store);
    let close_ms = ms(t);
    Ok(json!({
        "elapsed_ms": ms(started) - load_previous_ms,
        "phases_ms": {
            "open": open_ms,
            "load_previous_graph (separate call)": load_previous_ms,
            "build_snapshot": snapshot_ms,
            "indexed_graph": convert_ms,
            "apply_graph": apply_ms,
            "close": close_ms,
        },
        "parsed": parsed,
        "reused": reused,
        "symbols": snapshot.graph.symbol_count(),
        "edges": snapshot.graph.edges().len(),
        "delta": delta_json(&delta),
    }))
}

fn graph_queries(db: &Path, queries: usize, seed: u64, builds: usize) -> Result<Value> {
    let store = Store::open(db)?;
    let t = Instant::now();
    let stored = store.load_graph()?.ok_or("database holds no index")?;
    let load_ms = ms(t);

    let mut build_ms = Vec::with_capacity(builds);
    let mut graph = CodeGraph::default();
    for _ in 0..builds.max(1) {
        let (symbols, edges) = (stored.symbols.clone(), stored.edges.clone());
        let t = Instant::now();
        graph = CodeGraph::new(symbols, edges);
        build_ms.push(ms(t));
    }

    let ids: Vec<SymbolId> = graph.symbols().map(|s| s.id.clone()).collect();
    if ids.is_empty() {
        return Err("graph has no symbols".into());
    }
    let mut rng = SplitMix64::new(seed);
    let mut latency_us = Vec::with_capacity(queries);
    let (mut impacted_total, mut truncated, mut max_impacted) = (0usize, 0usize, 0usize);
    for _ in 0..queries {
        let root = ids[rng.below(ids.len())].clone();
        let t = Instant::now();
        let result = impact(&graph, std::slice::from_ref(&root), ImpactOptions::default());
        latency_us.push(t.elapsed().as_secs_f64() * 1e6);
        impacted_total += result.impacted.len();
        max_impacted = max_impacted.max(result.impacted.len());
        truncated += usize::from(result.truncated);
    }
    Ok(json!({
        "symbols": graph.symbol_count(),
        "edges": graph.edges().len(),
        "load_graph_ms": load_ms,
        "build_ms": summarize(&build_ms),
        "query_us": summarize(&latency_us),
        "queries": queries,
        "mean_impacted": impacted_total as f64 / queries.max(1) as f64,
        "max_impacted": max_impacted,
        "truncated_queries": truncated,
    }))
}

/// Parse / encode / decode / resolve / graph-build costs measured directly on generated sources,
/// without Git or SQLite, to attribute the index pipeline's time.
fn profile(files: usize, seed: u64, iterations: usize) -> Result<Value> {
    use ripplepath_lang::{java, ts};
    let sources: Vec<(String, String)> = SynthRepo::generate(SynthParams::new(files, seed))
        .render_all()
        .into_iter()
        .filter(|(p, _)| p.ends_with(".java") || p.ends_with(".ts"))
        .collect();
    let budget = Limits::default().parse_budget;
    let bytes: usize = sources.iter().map(|(_, s)| s.len()).sum();
    let mut samples: std::collections::BTreeMap<&str, Vec<f64>> = Default::default();
    let mut record = |name: &'static str, started: Instant| samples.entry(name).or_default().push(ms(started));
    let mut sizes = json!({});
    for _ in 0..iterations.max(1) {
        let t = Instant::now();
        let mut java_files = Vec::new();
        let mut ts_files = Vec::new();
        for (path, text) in &sources {
            if path.ends_with(".java") {
                java_files.push(java::extract(path, text, budget)?);
            } else {
                ts_files.push(ts::extract(path, text, budget)?);
            }
        }
        record("parse_sequential", t);

        let t = Instant::now();
        let parallel: Vec<bool> = sources
            .par_iter()
            .map(|(path, text)| {
                if path.ends_with(".java") {
                    java::extract(path, text, budget).is_ok()
                } else {
                    ts::extract(path, text, budget).is_ok()
                }
            })
            .collect();
        record("parse_parallel", t);
        drop(parallel);

        let t = Instant::now();
        let java_json: Vec<Vec<u8>> =
            java_files.iter().map(serde_json::to_vec).collect::<std::result::Result<_, _>>()?;
        let ts_json: Vec<Vec<u8>> = ts_files.iter().map(serde_json::to_vec).collect::<std::result::Result<_, _>>()?;
        record("encode_facts_json", t);
        sizes["facts_json_bytes"] = json!(java_json.iter().chain(&ts_json).map(Vec::len).sum::<usize>());

        let t = Instant::now();
        let java_back: Vec<java::facts::JavaFile> =
            java_json.iter().map(|b| serde_json::from_slice(b)).collect::<std::result::Result<_, _>>()?;
        let ts_back: Vec<ts::facts::TsFile> =
            ts_json.iter().map(|b| serde_json::from_slice(b)).collect::<std::result::Result<_, _>>()?;
        record("decode_facts_json", t);

        let t = Instant::now();
        let java_refs: Vec<&java::facts::JavaFile> = java_back.iter().collect();
        let java_graph = java::resolve(&java_refs);
        record("resolve_java", t);
        let t = Instant::now();
        let ts_refs: Vec<&ts::facts::TsFile> = ts_back.iter().collect();
        let ts_graph = ts::resolve(&ts_refs);
        record("resolve_ts", t);

        let mut symbols = java_graph.symbols;
        symbols.extend(ts_graph.symbols);
        let mut edges = java_graph.edges;
        edges.extend(ts_graph.edges);
        sizes["symbols"] = json!(symbols.len());
        sizes["edges"] = json!(edges.len());
        sizes["unresolved"] = json!(java_graph.unresolved.len() + ts_graph.unresolved.len());
        let t = Instant::now();
        let graph = CodeGraph::new(symbols, edges);
        record("code_graph_new", t);
        drop(graph);
    }
    let phases: serde_json::Map<String, Value> =
        samples.iter().map(|(name, s)| ((*name).to_owned(), json!(summarize(s)))).collect();
    Ok(json!({
        "files": sources.len(),
        "source_bytes": bytes,
        "threads": rayon::current_num_threads(),
        "sizes": sizes,
        "phases_ms": phases,
    }))
}

// ---------------------------------------------------------------------------------------------
// Suite orchestration

struct Dataset {
    description: Value,
    repo: PathBuf,
    /// Revision indexed by the cold/warm workloads and used for graph queries.
    full_rev: String,
    /// (label, base, head): incremental index base→head, and analysis base..head.
    scenarios: Vec<(String, String, String)>,
}

struct Workload {
    kind: &'static str,
    scenario: String,
    samples: Vec<Value>,
}

fn suite(args: &SuiteArgs) -> Result<()> {
    std::fs::create_dir_all(&args.work)?;
    let work = std::path::absolute(&args.work)?;
    let load_at_start = system_load();
    let dbs = work.join("db");
    std::fs::create_dir_all(&dbs)?;
    let dataset = match &args.repo {
        Some(repo) => {
            let git = ripplepath_git::Repo::open(repo)?;
            let base = git.resolve(&args.base)?;
            let head = git.resolve(&args.head)?;
            let label = format!("{}..{}", args.base, args.head);
            Dataset {
                description: json!({
                    "kind": "repository",
                    "path": repo,
                    "base": {"spec": args.base, "commit": base.commit},
                    "head": {"spec": args.head, "commit": head.commit},
                }),
                repo: repo.clone(),
                full_rev: args.head.clone(),
                scenarios: vec![(label, args.base.clone(), args.head.clone())],
            }
        }
        None => {
            let repo = work.join(format!("synth-{}-{}", args.files, args.seed));
            eprintln!("generating synthetic repository in {} ...", repo.display());
            let info = synth(args.files, args.seed, &repo)?;
            let commit = |i: usize| info["commits"][i].as_str().map(str::to_owned).ok_or("missing commit");
            let (c0, c1, c2) = (commit(0)?, commit(1)?, commit(2)?);
            Dataset {
                description: json!({"kind": "synthetic", "generator": info}),
                repo,
                full_rev: c0.clone(),
                scenarios: vec![
                    (format!("{SMALL_EDIT} file"), c0, c1.clone()),
                    (format!("{LARGE_EDIT} files"), c1, c2),
                ],
            }
        }
    };
    let repo_arg = dataset.repo.to_string_lossy().into_owned();
    let memory_flag: &[&str] = if args.no_probe { &["--no-probe"] } else { &[] };
    let iterations = args.iterations.max(1);
    let mut workloads: Vec<Workload> = Vec::new();

    // Cold index: a new database every iteration.
    let full_db = dbs.join("full.db");
    let mut samples = Vec::new();
    for i in 0..iterations {
        let db = dbs.join(format!("cold-{i}.db"));
        remove_db(&db)?;
        eprintln!("index cold {}/{iterations}", i + 1);
        samples.push(child(
            &[&["measure", "index", "--repo", &repo_arg, "--rev", &dataset.full_rev, "--db", &path(&db)], memory_flag]
                .concat(),
        )?);
        if i + 1 == iterations {
            copy_db(&db, &full_db)?;
        }
        remove_db(&db)?;
    }
    workloads.push(Workload { kind: "index", scenario: "cold (empty database)".into(), samples });

    // Warm, unchanged revision: everything comes from the cache, nothing is written.
    let warm_db = dbs.join("warm.db");
    copy_db(&full_db, &warm_db)?;
    let mut samples = Vec::new();
    for i in 0..iterations {
        eprintln!("index warm {}/{iterations}", i + 1);
        samples.push(child(
            &[
                &["measure", "index", "--repo", &repo_arg, "--rev", &dataset.full_rev, "--db", &path(&warm_db)],
                memory_flag,
            ]
            .concat(),
        )?);
    }
    workloads.push(Workload { kind: "index", scenario: "warm, unchanged revision".into(), samples });

    for (label, base, head) in &dataset.scenarios {
        // The database as it was after indexing `base`; each iteration starts from a copy.
        let template = dbs.join("template.db");
        if *base == dataset.full_rev {
            copy_db(&full_db, &template)?;
        } else {
            remove_db(&template)?;
            child(&["measure", "index", "--repo", &repo_arg, "--rev", base, "--db", &path(&template), "--no-probe"])?;
        }
        let mut samples = Vec::new();
        for i in 0..iterations {
            let db = dbs.join("update.db");
            copy_db(&template, &db)?;
            eprintln!("index update {label} {}/{iterations}", i + 1);
            samples.push(child(
                &[&["measure", "index", "--repo", &repo_arg, "--rev", head, "--db", &path(&db)], memory_flag].concat(),
            )?);
        }
        workloads.push(Workload { kind: "index", scenario: format!("update: {label}"), samples });
    }

    for (label, base, head) in &dataset.scenarios {
        let mut samples = Vec::new();
        for i in 0..iterations {
            eprintln!("analyze cold {label} {}/{iterations}", i + 1);
            samples.push(child(
                &[&["measure", "analyze", "--repo", &repo_arg, "--base", base, "--head", head], memory_flag].concat(),
            )?);
        }
        workloads.push(Workload { kind: "analyze", scenario: format!("{label}, no database"), samples });

        let db = dbs.join("analyze.db");
        copy_db(&full_db, &db)?;
        // Prime: facts of both revisions are cached after one run.
        child(&[
            "measure",
            "analyze",
            "--repo",
            &repo_arg,
            "--base",
            base,
            "--head",
            head,
            "--db",
            &path(&db),
            "--no-probe",
        ])?;
        let mut samples = Vec::new();
        for i in 0..iterations {
            eprintln!("analyze warm {label} {}/{iterations}", i + 1);
            samples.push(child(
                &[
                    &["measure", "analyze", "--repo", &repo_arg, "--base", base, "--head", head, "--db", &path(&db)],
                    memory_flag,
                ]
                .concat(),
            )?);
        }
        workloads.push(Workload { kind: "analyze", scenario: format!("{label}, warm database"), samples });
    }

    eprintln!("graph queries");
    let queries = args.queries.to_string();
    let builds = iterations.to_string();
    let graph = child(
        &[&["measure", "graph", "--db", &path(&full_db), "--queries", &queries, "--builds", &builds], memory_flag]
            .concat(),
    )?;
    workloads.push(Workload {
        kind: "graph",
        scenario: format!("{} random single-symbol roots", args.queries),
        samples: vec![graph],
    });

    let results: Vec<Value> = workloads
        .iter()
        .map(|w| {
            let elapsed: Vec<f64> = w.samples.iter().filter_map(|s| s["elapsed_ms"].as_f64()).collect();
            let peak: Vec<f64> = w.samples.iter().filter_map(|s| s["peak_rss_bytes"].as_f64()).collect();
            let cpu: Vec<f64> = w.samples.iter().filter_map(|s| s["cpu_ms"].as_f64()).collect();
            json!({
                "kind": w.kind,
                "scenario": w.scenario,
                "elapsed_ms": summarize(&elapsed),
                "peak_rss_bytes": summarize(&peak),
                "cpu_ms": summarize(&cpu),
                "samples": w.samples,
            })
        })
        .collect();
    let output = json!({
        "harness": {"version": env!("CARGO_PKG_VERSION"), "iterations": iterations, "profile": if cfg!(debug_assertions) { "debug" } else { "release" }},
        "host": host_info(),
        "system_load": {"start": load_at_start, "end": system_load()},
        "dataset": dataset.description,
        "results": results,
    });
    std::fs::write(&args.out, format!("{output:#}\n"))?;
    print!("{}", markdown(&output));
    Ok(())
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn child(args: &[&str]) -> Result<Value> {
    let exe = std::env::current_exe()?;
    let output = Command::new(exe).args(args).output()?;
    if !output.status.success() {
        return Err(format!("`{}` failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn remove_db(db: &Path) -> Result<()> {
    for p in [db.to_owned(), sidecar(db, "-wal"), sidecar(db, "-shm")] {
        match std::fs::remove_file(&p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

fn copy_db(from: &Path, to: &Path) -> Result<()> {
    remove_db(to)?;
    std::fs::copy(from, to)?;
    for suffix in ["-wal", "-shm"] {
        let source = sidecar(from, suffix);
        if source.exists() {
            std::fs::copy(&source, sidecar(to, suffix))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Markdown

fn f(v: &Value) -> String {
    v.as_f64().map_or_else(|| "–".to_owned(), |x| if x >= 100.0 { format!("{x:.0}") } else { format!("{x:.1}") })
}

fn mib(v: &Value) -> String {
    v.as_f64().map_or_else(|| "–".to_owned(), |b| format!("{:.0}", b / (1024.0 * 1024.0)))
}

fn summary_of(v: &Value) -> Option<Summary> {
    serde_json::from_value(v.clone()).ok()
}

fn markdown(output: &Value) -> String {
    let mut md = String::new();
    let results = output["results"].as_array().cloned().unwrap_or_default();
    md.push_str("### Index\n\n| Workload | n | median ms | p95 ms | min ms | max ms | source files | parsed | reused | files/s | symbols | edges | symbols/s | delta symbols +/-/~ | delta edges +/-/~ | DB MiB | CPU ms (median) | peak RSS MiB (median) |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in results.iter().filter(|r| r["kind"] == "index") {
        let Some(s) = summary_of(&r["elapsed_ms"]) else { continue };
        let first = &r["samples"][0];
        let source = first["parsed"].as_u64().unwrap_or(0) + first["reused"].as_u64().unwrap_or(0);
        let secs = s.median / 1e3;
        let triple = |v: &Value| {
            v.as_array().map(|a| a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("/")).unwrap_or_default()
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.0} | {} | {} | {:.0} | {} | {} | {} | {} | {} |\n",
            r["scenario"].as_str().unwrap_or(""),
            s.n,
            f(&json!(s.median)),
            f(&json!(s.p95)),
            f(&json!(s.min)),
            f(&json!(s.max)),
            source,
            first["parsed"],
            first["reused"],
            source as f64 / secs,
            first["symbols"],
            first["edges"],
            first["symbols"].as_f64().unwrap_or(0.0) / secs,
            triple(&first["delta"]["symbols"]),
            triple(&first["delta"]["edges"]),
            first["db_bytes"].as_f64().map_or("–".to_owned(), |b| format!("{:.1}", b / (1024.0 * 1024.0))),
            f(&r["cpu_ms"]["median"]),
            mib(&r["peak_rss_bytes"]["median"]),
        ));
    }
    md.push_str("\n### Analyze\n\n| Workload | n | median ms | p95 ms | min ms | max ms | files changed | symbols changed | impacted | tests recommended | CPU ms (median) | peak RSS MiB (median) |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in results.iter().filter(|r| r["kind"] == "analyze") {
        let Some(s) = summary_of(&r["elapsed_ms"]) else { continue };
        let first = &r["samples"][0];
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r["scenario"].as_str().unwrap_or(""),
            s.n,
            f(&json!(s.median)),
            f(&json!(s.p95)),
            f(&json!(s.min)),
            f(&json!(s.max)),
            first["files_changed"],
            first["symbols_changed"],
            first["symbols_impacted"],
            first["tests_recommended"],
            f(&r["cpu_ms"]["median"]),
            mib(&r["peak_rss_bytes"]["median"]),
        ));
    }
    md.push_str("\n### Graph\n\n| Symbols | Edges | load_graph ms | CodeGraph::new median ms | p95 ms | queries | query p50 µs | p95 µs | max µs | mean impacted | max impacted | truncated | peak RSS MiB |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in results.iter().filter(|r| r["kind"] == "graph") {
        let g = &r["samples"][0];
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            g["symbols"],
            g["edges"],
            f(&g["load_graph_ms"]),
            f(&g["build_ms"]["median"]),
            f(&g["build_ms"]["p95"]),
            g["queries"],
            f(&g["query_us"]["median"]),
            f(&g["query_us"]["p95"]),
            f(&g["query_us"]["max"]),
            f(&g["mean_impacted"]),
            g["max_impacted"],
            g["truncated_queries"],
            mib(&g["peak_rss_bytes"]),
        ));
    }
    md
}
