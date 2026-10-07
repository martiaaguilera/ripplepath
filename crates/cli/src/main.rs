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
    },
    /// Build the bundled demo repository and analyse its change.
    Demo {
        /// Where to create the demo repository (must not exist yet).
        #[arg(long, default_value = "ripplepath-demo")]
        dir: PathBuf,
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
            let report = analyze(&options).map_err(|e| e.to_string())?;
            emit(&report, args.format, args.output.as_deref())
        }
        Command::Serve { repo, addr, web_dir, base, head } => {
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
            let config =
                ripplepath_server::ServerConfig { repo, addr, web_dir, default_base: base, default_head: head };
            let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            runtime.block_on(ripplepath_server::serve(config)).map_err(|e| e.to_string())
        }
        Command::Demo { dir, format } => {
            if dir.exists() {
                return Err(format!("{} already exists; choose another --dir", dir.display()));
            }
            let fixtures = demo_fixture_root()?;
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
fn demo_fixture_root() -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from("fixtures/java-banking"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking"),
    ];
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
