use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use ripplepath_engine::{AnalysisReport, AnalyzeOptions, analyze, fixture};
use ripplepath_graph::ImpactOptions;

mod annotations;
mod evaluate_text;
mod findings;
mod markdown;
mod output;
mod sarif;
#[cfg(test)]
mod test_support;
mod text;

/// Change intelligence for Git repositories: what a change touches, what depends on it, and which
/// tests have evidence of exercising it.
#[derive(Parser)]
#[command(name = "ripplepath", version, propagate_version = true)]
struct Cli {
    /// Log progress to stderr (repeat for more detail). RUST_LOG overrides.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyse the change between two revisions.
    Analyze(AnalyzeArgs),
    /// Index one revision into the local database, reusing everything unchanged since the last run.
    Index {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        /// Database file [default: a per-repository file in the user cache directory].
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Record test evidence (coverage, CI results) for a revision.
    #[command(subcommand)]
    Ingest(IngestCommand),
    /// Replay historical changes and score the test selection against the tests that failed.
    Evaluate {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// JSON file listing cases: {"cases": [{"name", "base", "head", "junit": [files]}]}; the
        /// JUnit files (relative to the cases file) are the observed run at head.
        #[arg(long)]
        cases: PathBuf,
        /// Index and evidence database holding the history's coverage and CI results.
        #[arg(long)]
        db: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Write the report to this file instead of stdout.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Serve the web UI and the read-only HTTP API for one repository (localhost by default).
    Serve {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Address to bind. Binding beyond localhost exposes source code to the network.
        #[arg(long, default_value = "127.0.0.1:7878")]
        addr: std::net::SocketAddr,
        /// Built web UI directory [default: `web/dist` of the source tree this binary was built
        /// from]. Never resolved against the current directory: `ripplepath serve` run inside a
        /// project with its own `web/dist` would otherwise serve that project's scripts as the UI,
        /// on the same origin as the API.
        #[arg(long)]
        web_dir: Option<PathBuf>,
        /// Revisions the UI opens with.
        #[arg(long, default_value = "HEAD~1")]
        base: String,
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Additional Host header value to accept (repeatable). Loopback names are always accepted;
        /// everything else is refused to block DNS-rebinding attacks from web pages.
        #[arg(long = "allow-host")]
        allowed_hosts: Vec<String>,
        /// Index and evidence database [default: the per-repository file `ripplepath index` uses, if
        /// it exists], so the UI shows the same coverage and CI evidence as `analyze`.
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Serve read-only Model Context Protocol tools over stdio for coding agents (one repository).
    Mcp {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Index and evidence database [default: the per-repository file `ripplepath index` uses, if it
        /// exists].
        #[arg(long)]
        db: Option<PathBuf>,
        /// Revisions used when a tool call omits them.
        #[arg(long, default_value = "HEAD~1")]
        base: String,
        #[arg(long, default_value = "HEAD")]
        head: String,
    },
    /// Architecture rules of one revision.
    #[command(subcommand)]
    Architecture(ArchitectureCommand),
    /// Build the bundled demo repository and analyse its change.
    Demo {
        /// Where to create the demo repository (must not exist yet).
        #[arg(long, default_value = "ripplepath-demo")]
        dir: PathBuf,
        /// Which bundled fixture to build.
        #[arg(long, value_enum, default_value_t = DemoFixture::JavaBanking)]
        fixture: DemoFixture,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Subcommand)]
enum ArchitectureCommand {
    /// Check one revision against its own ripplepath.yml layer rules (state, not delta).
    Check {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        rev: String,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Subcommand)]
enum IngestCommand {
    /// Ingest a coverage report (JaCoCo XML or LCOV) measured at a revision.
    Coverage {
        file: PathBuf,
        #[arg(long, value_enum)]
        format: CoverageArg,
        #[command(flatten)]
        target: IngestTarget,
        /// The test whose execution produced this report: a symbol id, test file path or Java test
        /// class. Without it, LCOV test names (TN) are used, else the report is aggregate.
        #[arg(long)]
        test: Option<String>,
    },
    /// Ingest a JUnit XML results file from a CI run at a revision.
    Junit {
        file: PathBuf,
        #[command(flatten)]
        target: IngestTarget,
    },
}

#[derive(clap::Args)]
struct IngestTarget {
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Revision the evidence was produced at.
    #[arg(long, default_value = "HEAD")]
    rev: String,
    /// Database file [default: a per-repository file in the user cache directory].
    #[arg(long)]
    db: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum CoverageArg {
    Jacoco,
    Lcov,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Conservative,
    Balanced,
    FastFeedback,
}

#[derive(clap::Args)]
struct AnalyzeArgs {
    /// Repository to analyse.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Base revision (branch, tag, SHA, `HEAD~1`, ...).
    #[arg(long)]
    base: String,
    /// Head revision.
    #[arg(long, default_value = "HEAD")]
    head: String,
    #[arg(long, value_enum, default_value_t = output::ReportFormat::Text)]
    format: output::ReportFormat,
    /// Write the report to this file instead of stdout.
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Also write analysis.json, summary.md, ripplepath.sarif and annotations.txt into this
    /// directory (created if missing). `--format` output still goes to stdout or `--output`.
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Prefix for file paths in SARIF and annotations: the repository's path relative to the CI
    /// workspace, when it is not checked out at the workspace root.
    #[arg(long, default_value = "")]
    path_prefix: String,
    /// Maximum dependency depth to follow from changed symbols.
    #[arg(long, default_value_t = 6, value_parser = clap::value_parser!(u32).range(1..=32))]
    max_depth: u32,
    /// Index and evidence database [default: the per-repository file `ripplepath index` uses, if it
    /// exists]. Must not live inside an untrusted repository.
    #[arg(long)]
    db: Option<PathBuf>,
    /// Test selection policy [default: `tests.mode` from the base revision's ripplepath.yml, else
    /// balanced].
    #[arg(long, value_enum)]
    mode: Option<ModeArg>,
    /// Exit with status 2 when the merge policy fails (the report is still written).
    #[arg(long)]
    fail_on_policy: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum DemoFixture {
    JavaBanking,
    TypescriptCheckout,
}

/// The demo fixtures, embedded at build time (see build.rs): the demo never reads the current
/// directory, so running it inside an untrusted checkout cannot substitute that checkout's files.
mod demo_fixtures {
    /// (repository path, contents) of one file.
    type File = (&'static str, &'static [u8]);
    /// (snapshot name, files) in commit order.
    type Snapshots = &'static [(&'static str, &'static [File])];
    include!(concat!(env!("OUT_DIR"), "/demo_fixtures.rs"));
}

impl DemoFixture {
    fn snapshots(self) -> Vec<fixture::Snapshot> {
        let embedded = match self {
            Self::JavaBanking => demo_fixtures::JAVA_BANKING,
            Self::TypescriptCheckout => demo_fixtures::TYPESCRIPT_CHECKOUT,
        };
        embedded
            .iter()
            .map(|(name, files)| fixture::Snapshot {
                name: (*name).to_owned(),
                files: files.iter().map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec())).collect(),
            })
            .collect()
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// Exit statuses: 0 success, 1 error, 2 merge policy failed (`analyze --fail-on-policy`).
const EXIT_POLICY_FAILED: u8 = 2;

enum Outcome {
    Done,
    PolicyFailed,
}

fn main() -> ExitCode {
    // clap exits with status 2 on a usage error, which is the policy-FAIL status: a CI job with a
    // misspelled `--mode` would then look like a policy failure (or pass, with fail-on-policy
    // off) without any analysis having run. Usage errors are ordinary errors here.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return if error.use_stderr() { ExitCode::FAILURE } else { ExitCode::SUCCESS };
        }
    };
    init_tracing(cli.verbose);
    match run(cli.command) {
        Ok(Outcome::Done) => ExitCode::SUCCESS,
        Ok(Outcome::PolicyFailed) => ExitCode::from(EXIT_POLICY_FAILED),
        Err(message) => {
            // Errors can quote repository content (paths, revisions, stored values).
            eprintln!("error: {}", text::neutralize_terminal_controls(&message));
            ExitCode::FAILURE
        }
    }
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
}

fn run(command: Command) -> Result<Outcome, String> {
    match command {
        Command::Analyze(args) => {
            let fail_on_policy = args.fail_on_policy;
            let report = run_analyze(args)?;
            Ok(if fail_on_policy && report.policy.result == ripplepath_engine::policy::PolicyResult::Fail {
                Outcome::PolicyFailed
            } else {
                Outcome::Done
            })
        }
        Command::Architecture(ArchitectureCommand::Check { repo, rev, format }) => {
            let check = ripplepath_engine::check_architecture(&repo, &rev, &ripplepath_engine::Limits::default())
                .map_err(|e| e.to_string())?;
            let rendered = match format {
                Format::Json => {
                    let mut json = serde_json::to_string_pretty(&check).map_err(|e| e.to_string())?;
                    json.push('\n');
                    json
                }
                Format::Text => text::render_architecture_check(&check),
            };
            write_stdout(&rendered)?;
            Ok(Outcome::Done)
        }
        other => run_other(other).map(|()| Outcome::Done),
    }
}

fn run_analyze(args: AnalyzeArgs) -> Result<AnalysisReport, String> {
    let mut options = AnalyzeOptions::new(&args.repo, &args.base, &args.head);
    options.impact = ImpactOptions { max_depth: args.max_depth, ..ImpactOptions::default() };
    options.db = match args.db {
        Some(db) => Some(db),
        None => default_db(&args.repo).ok().filter(|db| db.is_file()),
    };
    options.mode = args.mode.map(|mode| match mode {
        ModeArg::Conservative => ripplepath_engine::SelectionMode::Conservative,
        ModeArg::Balanced => ripplepath_engine::SelectionMode::Balanced,
        ModeArg::FastFeedback => ripplepath_engine::SelectionMode::FastFeedback,
    });
    let report = analyze(&options).map_err(|e| e.to_string())?;
    if let Some(dir) = &args.output_dir {
        output::write_output_dir(&report, dir, &args.path_prefix)?;
    }
    let rendered = output::render(&report, args.format, &args.path_prefix)?;
    match &args.output {
        Some(path) => std::fs::write(path, rendered).map_err(|e| format!("cannot write {}: {e}", path.display()))?,
        None => write_stdout(&rendered)?,
    }
    Ok(report)
}

fn run_other(command: Command) -> Result<(), String> {
    match command {
        Command::Analyze(_) | Command::Architecture(_) => Ok(()),
        Command::Index { repo, rev, db, format } => {
            let db = match db {
                Some(db) => db,
                None => default_db(&repo)?,
            };
            let outcome = ripplepath_engine::index_revision(&repo, &rev, &db, &ripplepath_engine::Limits::default())
                .map_err(|e| e.to_string())?;
            let d = outcome.delta;
            let rendered = match format {
                Format::Json => format!(
                    "{}
",
                    serde_json::json!({
                        "tree": outcome.tree,
                        "commit": outcome.commit,
                        "files": outcome.files,
                        "parsed": outcome.parsed,
                        "reused": outcome.reused,
                        "symbols": outcome.symbols,
                        "edges": outcome.edges,
                        "delta": {
                            "files_changed": d.files_changed,
                            "symbols_added": d.symbols_added,
                            "symbols_removed": d.symbols_removed,
                            "symbols_updated": d.symbols_updated,
                            "edges_added": d.edges_added,
                            "edges_removed": d.edges_removed,
                            "edges_updated": d.edges_updated,
                        },
                        "elapsed_ms": outcome.elapsed_ms,
                    })
                ),
                Format::Text => format!(
                    "Indexed {rev} ({}) into {}
  files {}  |  parsed {}  |  reused from cache {}
  symbols {} (+{} -{} ~{})  |  edges {} (+{} -{} ~{})
  {} ms
",
                    outcome.commit.as_deref().unwrap_or(&outcome.tree).chars().take(10).collect::<String>(),
                    db.display(),
                    outcome.files,
                    outcome.parsed,
                    outcome.reused,
                    outcome.symbols,
                    d.symbols_added,
                    d.symbols_removed,
                    d.symbols_updated,
                    outcome.edges,
                    d.edges_added,
                    d.edges_removed,
                    d.edges_updated,
                    outcome.elapsed_ms,
                ),
            };
            print!("{}", text::neutralize_terminal_controls(&rendered));
            Ok(())
        }
        Command::Ingest(command) => {
            let limits = ripplepath_engine::Limits::default();
            let (outcome, kind) = match command {
                IngestCommand::Coverage { file, format, target, test } => {
                    let input = read_input(&file)?;
                    let db = target.db.map_or_else(|| default_db(&target.repo), Ok)?;
                    let format = match format {
                        CoverageArg::Jacoco => ripplepath_engine::CoverageFormat::Jacoco,
                        CoverageArg::Lcov => ripplepath_engine::CoverageFormat::Lcov,
                    };
                    let source = file.to_string_lossy();
                    let outcome = ripplepath_engine::ingest_coverage(
                        &target.repo,
                        &target.rev,
                        &db,
                        ripplepath_engine::CoverageInput {
                            format,
                            text: &input,
                            source: &source,
                            test: test.as_deref(),
                        },
                        &limits,
                    )
                    .map_err(|e| e.to_string())?;
                    (outcome, "coverage")
                }
                IngestCommand::Junit { file, target } => {
                    let input = read_input(&file)?;
                    let db = target.db.map_or_else(|| default_db(&target.repo), Ok)?;
                    let outcome = ripplepath_engine::ingest_junit(
                        &target.repo,
                        &target.rev,
                        &db,
                        &input,
                        &file.to_string_lossy(),
                        &limits,
                    )
                    .map_err(|e| e.to_string())?;
                    (outcome, "junit")
                }
            };
            let mut text = format!(
                "Ingested {kind} at {}: {} report(s), {} mapped, {} unmapped{}\n",
                outcome.commit.chars().take(10).collect::<String>(),
                outcome.reports,
                outcome.mapped,
                outcome.unmapped,
                if kind == "coverage" {
                    format!(", {} covered symbols", outcome.covered_symbols)
                } else {
                    String::new()
                },
            );
            for example in &outcome.unmapped_examples {
                text.push_str(&format!("  unmapped: {example}\n"));
            }
            print!("{}", text::neutralize_terminal_controls(&text));
            Ok(())
        }
        Command::Evaluate { repo, cases, db, format, output } => {
            use ripplepath_engine::evaluation;
            let cases = evaluation::load_cases(&cases).map_err(|e| e.to_string())?;
            let report = evaluation::evaluate(&evaluation::EvaluationOptions::new(&repo, &db), &cases)
                .map_err(|e| e.to_string())?;
            let rendered = match format {
                Format::Json => {
                    let mut json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                    json.push('\n');
                    json
                }
                Format::Text => text::neutralize_terminal_controls(&evaluate_text::render(&report)),
            };
            write_output(&rendered, output.as_deref())
        }
        Command::Serve { repo, addr, web_dir, base, head, allowed_hosts, db } => {
            let db = db.or_else(|| default_db(&repo).ok().filter(|db| db.is_file()));
            if !addr.ip().is_loopback() {
                eprintln!("warning: listening on {addr}; anyone who can reach it can read analysed source metadata");
            }
            let web_dir = web_dir.unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist"));
            let web_dir = if web_dir.join("index.html").is_file() {
                Some(web_dir)
            } else {
                eprintln!(
                    "note: {} has no built UI (run `npm run build` in web/); serving the API only",
                    web_dir.display()
                );
                None
            };
            eprintln!("Ripplepath listening on http://{addr}");
            let config = ripplepath_server::ServerConfig {
                repo,
                addr,
                web_dir,
                default_base: base,
                default_head: head,
                allowed_hosts,
                db,
            };
            let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            runtime.block_on(ripplepath_server::serve(config)).map_err(|e| e.to_string())
        }
        Command::Mcp { repo, db, base, head } => {
            let db = match db {
                Some(db) => Some(db),
                None => default_db(&repo).ok().filter(|db| db.is_file()),
            };
            let config = ripplepath_mcp::McpConfig { repo, db, default_base: base, default_head: head };
            // stdout carries protocol messages only; diagnostics go to stderr through tracing.
            ripplepath_mcp::serve(config, std::io::stdin().lock(), std::io::stdout().lock()).map_err(|e| e.to_string())
        }
        Command::Demo { dir, fixture, format } => {
            if dir.exists() {
                return Err(format!("{} already exists; choose another --dir", dir.display()));
            }
            fixture::build_repo_from_snapshots(&fixture.snapshots(), &dir).map_err(|e| e.to_string())?;
            let report = analyze(&AnalyzeOptions::new(&dir, "main~1", "main")).map_err(|e| e.to_string())?;
            emit(&report, format, None)?;
            eprintln!(
                "\ndemo repository: {}\nre-run with: ripplepath analyze --repo {} --base main~1 --head main",
                dir.display(),
                dir.display()
            );
            Ok(())
        }
    }
}

/// Evidence files come from CI artifacts; bounded before parsing.
fn read_input(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let error = |e: std::io::Error| format!("cannot read {}: {e}", path.display());
    let limit = ripplepath_evidence_limit();
    let size = std::fs::metadata(path).map_err(error)?.len();
    if size > limit {
        return Err(format!("{} is {size} bytes, above the input limit", path.display()));
    }
    // The metadata size of a pipe or device (`/dev/zero`, a FIFO) is 0; the read itself is bounded
    // too so such an input cannot grow memory without limit.
    let mut text = String::new();
    std::fs::File::open(path).map_err(error)?.take(limit + 1).read_to_string(&mut text).map_err(error)?;
    if text.len() as u64 > limit {
        return Err(format!("{} is above the {limit} byte input limit", path.display()));
    }
    Ok(text)
}

fn ripplepath_evidence_limit() -> u64 {
    ripplepath_engine::MAX_EVIDENCE_BYTES as u64
}

/// Default index location: the user's cache directory, keyed by the repository's canonical path.
///
/// Never inside the analysed repository: a repository could commit its own `.ripplepath/index.db`
/// with forged facts for its own blob ids and so control the analysis of itself.
fn default_db(repo: &Path) -> Result<PathBuf, String> {
    let canonical = repo.canonicalize().map_err(|e| format!("cannot resolve {}: {e}", repo.display()))?;
    let base = std::env::var_os("RIPPLEPATH_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("ripplepath")))
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(|d| PathBuf::from(d).join("ripplepath")))
        .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".cache").join("ripplepath")))
        .ok_or("no cache directory found; pass --db or set RIPPLEPATH_CACHE_DIR")?;
    let key = blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex();
    Ok(base.join(format!("{}.db", &key[..16])))
}

fn emit(report: &AnalysisReport, format: Format, output: Option<&Path>) -> Result<(), String> {
    let rendered = match format {
        Format::Json => {
            let mut json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
            json.push('\n');
            json
        }
        Format::Text => text::render(report),
    };
    write_output(&rendered, output)
}

fn write_output(rendered: &str, output: Option<&Path>) -> Result<(), String> {
    match output {
        Some(path) => std::fs::write(path, rendered).map_err(|e| format!("cannot write {}: {e}", path.display())),
        None => write_stdout(rendered),
    }
}

fn write_stdout(rendered: &str) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    // A closed pipe (`ripplepath ... | head`) is not an error worth reporting.
    match stdout.write_all(rendered.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.to_string()),
        _ => Ok(()),
    }
}
