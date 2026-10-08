//! GitHub Actions workflow commands (`::error file=…,line=…,title=…::message`).
//!
//! Workflow commands are parsed from the job log line by line, so any repository-derived text that
//! reached the log unescaped could end the current command and start another one (`::add-mask::`,
//! `::stop-commands::` …). Escaping follows `@actions/core`: message data escapes `%`, CR and LF;
//! property values additionally escape `:` and `,`, which delimit properties.

use std::fmt::Write;

use ripplepath_engine::AnalysisReport;
use ripplepath_engine::policy::{GateStatus, PolicyResult};

use crate::findings::{self, Level};
use crate::text::neutralize_terminal_controls;

/// GitHub displays at most 10 errors and 10 warnings per step and 50 per job; more only adds log
/// noise. The remainder is counted in a final notice and is complete in SARIF and JSON.
pub const MAX_ANNOTATIONS: usize = 50;

pub fn escape_data(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_property(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            ':' => out.push_str("%3A"),
            ',' => out.push_str("%2C"),
            _ => out.push(c),
        }
    }
    out
}

/// Bidi and other invisible characters would make the annotation misleading in the GitHub UI;
/// they are shown as visible escapes before command escaping.
fn data(text: &str) -> String {
    escape_data(&neutralize_terminal_controls(text))
}

fn property(text: &str) -> String {
    escape_property(&neutralize_terminal_controls(text))
}

fn command(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Note => "notice",
    }
}

pub fn render(report: &AnalysisReport, path_prefix: &str) -> String {
    let mut out = String::new();
    let findings = findings::collect(report, path_prefix);
    // Errors first so that GitHub's per-step display limit never hides an error behind notices.
    let mut ordered: Vec<&findings::Finding> = findings.iter().collect();
    ordered.sort_by_key(|f| f.level);
    for finding in ordered.iter().take(MAX_ANNOTATIONS) {
        let mut properties = format!("file={}", property(&finding.file));
        if let Some(line) = finding.line {
            let _ = write!(properties, ",line={line}");
        }
        let _ = write!(properties, ",title={}", property(&finding.title));
        let _ = writeln!(out, "::{} {properties}::{}", command(finding.level), data(&finding.message));
    }
    if findings.len() > MAX_ANNOTATIONS {
        let _ = writeln!(
            out,
            "::notice title={}::{}",
            property("Ripplepath: more findings"),
            data(&format!(
                "{} more located finding(s) are not annotated; see ripplepath.sarif or analysis.json.",
                findings.len() - MAX_ANNOTATIONS
            ))
        );
    }

    let policy = &report.policy;
    let gates = |status: GateStatus| -> Vec<&str> {
        policy.gates.iter().filter(|g| g.status == status).map(|g| g.gate.name()).collect()
    };
    let (level, gates) = match policy.result {
        PolicyResult::Fail => ("error", gates(GateStatus::Fail)),
        PolicyResult::Warn => ("warning", gates(GateStatus::Warn)),
        PolicyResult::Pass => return out,
    };
    let _ = writeln!(
        out,
        "::{level} title={}::{}",
        property(&format!("Ripplepath policy {}", format!("{:?}", policy.result).to_uppercase())),
        data(&format!("Gates: {}. Risk {}/100 (not a probability).", gates.join(", "), report.risk.score))
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_annotations_follow_policy_levels_and_end_with_the_verdict() {
        let report = crate::test_support::java_banking_report();
        let out = render(&report, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.iter().filter(|l| l.starts_with("::error file=")).count(), 2, "{out}");
        assert_eq!(lines.iter().filter(|l| l.starts_with("::warning file=")).count(), 3, "{out}");
        assert!(
            lines[0].starts_with(
                "::error file=src/main/java/com/acme/bank/domain/Account.java,line=3,title=New architecture violation%3A domain -> api::"
            ),
            "{out}"
        );
        let verdict = format!(
            "::error title=Ripplepath policy FAIL::Gates: new_architecture_violation, new_cycle. Risk {}/100 (not a probability).",
            report.risk.score
        );
        assert_eq!(lines.last().copied(), Some(verdict.as_str()));
        assert_eq!(out, render(&report, ""));
    }

    #[test]
    fn data_escapes_percent_and_line_breaks_only() {
        assert_eq!(escape_data("50% done\r\nnext::line, a:b"), "50%25 done%0D%0Anext::line, a:b");
    }

    #[test]
    fn properties_also_escape_their_delimiters() {
        assert_eq!(escape_property("src/a,b:c%d\ne.java"), "src/a%2Cb%3Ac%25d%0Ae.java");
    }

    #[test]
    fn percent_is_escaped_first_so_existing_escapes_survive_literally() {
        // A path that already looks escaped must not be decoded by the runner into a newline.
        assert_eq!(escape_data("%0A"), "%250A");
        assert_eq!(escape_property("%2C"), "%252C");
    }

    #[test]
    fn hostile_text_cannot_start_a_new_workflow_command() {
        let hostile = "x\n::add-mask::secret\r\n::stop-commands::t";
        let escaped = data(hostile);
        assert!(!escaped.contains('\n') && !escaped.contains('\r'), "{escaped}");
        // The command delimiter may stay inside the message: only a line start begins a command.
        assert!(escaped.starts_with('x'));
        let prop = property("a::b,c\u{202e}");
        assert_eq!(prop, r"a%3A%3Ab%2Cc\u{202e}");
    }
}
