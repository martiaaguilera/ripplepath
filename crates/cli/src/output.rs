//! Report formats of `analyze` and `--output-dir`, the multi-file output CI consumes.

use std::path::Path;

use clap::ValueEnum;
use ripplepath_engine::AnalysisReport;

use crate::{annotations, markdown, sarif, text};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReportFormat {
    Text,
    Json,
    /// GitHub-flavoured Markdown summary (step summary, PR comment).
    Markdown,
    /// SARIF 2.1.0 with the findings that have a source location.
    Sarif,
    /// GitHub Actions workflow commands (`::error file=…`).
    GithubAnnotations,
}

/// Files written by `--output-dir`, one per format except text.
pub const OUTPUT_FILES: [(&str, ReportFormat); 4] = [
    ("analysis.json", ReportFormat::Json),
    ("summary.md", ReportFormat::Markdown),
    ("ripplepath.sarif", ReportFormat::Sarif),
    ("annotations.txt", ReportFormat::GithubAnnotations),
];

/// `path_prefix` is prepended to file paths in SARIF and annotations: the repository's location
/// relative to the directory the consumer resolves paths against (the CI workspace).
pub fn render(report: &AnalysisReport, format: ReportFormat, path_prefix: &str) -> Result<String, String> {
    Ok(match format {
        ReportFormat::Text => text::render(report),
        ReportFormat::Json => {
            let mut json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
            json.push('\n');
            json
        }
        ReportFormat::Markdown => markdown::render(report),
        ReportFormat::Sarif => sarif::render(report, path_prefix)?,
        ReportFormat::GithubAnnotations => annotations::render(report, path_prefix),
    })
}

/// The name a value has in `analysis.json` (`RESOLVED_EXACT`, `signature_changed` …), so every
/// output spells enums the same way.
pub fn wire_name<T: serde::Serialize + std::fmt::Debug>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => format!("{value:?}"),
    }
}

pub fn write_output_dir(report: &AnalysisReport, dir: &Path, path_prefix: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for (name, format) in OUTPUT_FILES {
        let path = dir.join(name);
        let rendered = render(report, format, path_prefix)?;
        std::fs::write(&path, rendered).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}
