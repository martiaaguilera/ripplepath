use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use ripplepath_engine::{AnalysisReport, AnalyzeOptions, analyze, fixture};
use ripplepath_graph::ImpactOptions;

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
        /// Database file [default: <repo>/.ripplepath/index.db].
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Serve the web UI and the read-only HTTP API for one repository (localhost by default).
    Serve {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Address to bind. Binding beyond localhost exposes source code to the network.
        #[arg(long, default_value = "127.0.0.1:7878")]
        addr: std::net::SocketAddr,
        /// Built web UI directory.
        #[arg(long, default_value = "web/dist")]
        web_dir: PathBuf,
        /// Revisions the UI opens with.
        #[arg(long, default_value = "HEAD~1")]
        base: String,
        #[arg(long, default_value = "HEAD")]
        head: String,
        /// Additional Host header value to accept (repeatable). Loopback names are always accepted;
        /// everything else is refused to block DNS-rebinding attacks from web pages.
        #[arg(long = "allow-host")]
        allowed_hosts: Vec<String>,
    },
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
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    /// Write the report to this file instead of stdout.
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Maximum dependency depth to follow from changed symbols.
    #[arg(long, default_value_t = 6, value_parser = clap::value_parser!(u32).range(1..=32))]
    max_depth: u32,
    /// Reuse and fill a persistent fact cache (e.g. `.ripplepath/index.db`).
    #[arg(long)]
    db: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum DemoFixture {
    JavaBanking,
    TypescriptCheckout,
}

impl DemoFixture {
    fn dir_name(self) -> &'static str {
        match self {
            Self::JavaBanking => "java-banking",
            Self::TypescriptCheckout => "typescript-checkout",
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
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

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Analyze(args) => {
            let mut options = AnalyzeOptions::new(&args.repo, &args.base, &args.head);
            options.impact = ImpactOptions { max_depth: args.max_depth, ..ImpactOptions::default() };
            options.db = args.db;
            let report = analyze(&options).map_err(|e| e.to_string())?;
            emit(&report, args.format, args.output.as_deref())
        }
        Command::Index { repo, rev, db, format } => {
            let db = db.unwrap_or_else(|| repo.join(".ripplepath").join("index.db"));
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
        Command::Serve { repo, addr, web_dir, base, head, allowed_hosts } => {
            if !addr.ip().is_loopback() {
                eprintln!("warning: listening on {addr}; anyone who can reach it can read analysed source metadata");
            }
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
            };
            let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            runtime.block_on(ripplepath_server::serve(config)).map_err(|e| e.to_string())
        }
        Command::Demo { dir, fixture, format } => {
            if dir.exists() {
                return Err(format!("{} already exists; choose another --dir", dir.display()));
            }
            let fixtures = demo_fixture_root(fixture.dir_name())?;
            fixture::build_fixture_repo(&[&fixtures.join("v1"), &fixtures.join("v2")], &dir)
                .map_err(|e| e.to_string())?;
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

/// The demo ships with the source tree; look next to the binary's workspace or the current dir.
fn demo_fixture_root(name: &str) -> Result<PathBuf, String> {
    let candidates =
        [Path::new("fixtures").join(name), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)];
    candidates
        .into_iter()
        .find(|p| p.join("v1").is_dir())
        .ok_or_else(|| "demo fixtures not found; run from the Ripplepath source directory".to_owned())
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
    match output {
        Some(path) => std::fs::write(path, rendered).map_err(|e| format!("cannot write {}: {e}", path.display())),
        None => {
            let mut stdout = std::io::stdout().lock();
            // A closed pipe (`ripplepath ... | head`) is not an error worth reporting.
            match stdout.write_all(rendered.as_bytes()) {
                Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.to_string()),
                _ => Ok(()),
            }
        }
    }
}
