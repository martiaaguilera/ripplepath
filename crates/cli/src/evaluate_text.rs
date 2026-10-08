//! Plain-text rendering of an offline evaluation report. The caller neutralises terminal controls.

use std::fmt::Write;

use ripplepath_engine::SelectionMode;
use ripplepath_engine::evaluation::{EvaluationReport, ModeResult};

fn mode_name(mode: SelectionMode) -> &'static str {
    match mode {
        SelectionMode::Conservative => "conservative",
        SelectionMode::Balanced => "balanced",
        SelectionMode::FastFeedback => "fast_feedback",
    }
}

fn ratio(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |v| format!("{v:.4}"))
}

fn cell(result: &ModeResult) -> String {
    let s = &result.score;
    let mut text = format!("{}/{}", s.selected_tests, s.total_tests);
    if result.decision == ripplepath_engine::SelectionDecision::FullSuite {
        text.push_str(" FULL");
    }
    if s.failed_tests > 0 {
        let _ = write!(text, " caught {}/{}", s.caught_failures, s.failed_tests);
        if let Some(position) = s.first_failure_position {
            let _ = write!(text, " first@{position}");
        }
    }
    text
}

pub fn render(report: &EvaluationReport) -> String {
    let mut out = String::new();
    let sample = &report.sample;
    let _ = writeln!(
        out,
        "Offline evaluation: {} case(s), {} with failing tests, {} failing test(s)",
        sample.cases, sample.failing_cases, sample.failed_tests
    );
    if let Some(label) = &sample.label {
        let _ = writeln!(out, "  {label}");
    }
    out.push('\n');
    let _ = writeln!(
        out,
        "{:<14} {:>14} {:>7} {:>11} {:>17} {:>17}",
        "mode", "recall", "missed", "full suite", "mean tests saved", "mean time saved"
    );
    for m in &report.modes {
        let _ = writeln!(
            out,
            "{:<14} {:>14} {:>7} {:>11} {:>17} {:>17}",
            mode_name(m.mode),
            format!("{}/{} {}", m.caught_failures, m.failed_tests, ratio(m.failing_test_recall)),
            m.missed_failures,
            format!("{}/{}", m.full_suite_fallbacks, m.cases),
            ratio(m.mean_selected_test_reduction),
            format!("{} ({} cases)", ratio(m.mean_runtime_reduction), m.runtime_cases),
        );
    }
    out.push_str("\nPer case (selected/total tests; caught/failed; position of first failing test):\n");
    for case in &report.cases {
        let _ = writeln!(out, "  {}  [{} failing]", case.name, case.observed.failed.len());
        for result in &case.modes {
            let _ = writeln!(out, "    {:<14} {}", mode_name(result.mode), cell(result));
            for missed in &result.score.missed_failures {
                let _ = writeln!(out, "      missed {missed}");
            }
        }
    }
    out
}
