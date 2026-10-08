//! GitHub-flavoured Markdown summary for `$GITHUB_STEP_SUMMARY` and pull-request comments.
//!
//! Everything taken from the analysed repository (symbol names, paths, layer names, details that
//! quote them, revision specs) is untrusted: a pull request controls it. It is rendered either
//! inside a code span (literal: no Markdown, no HTML, no @mentions or #references are processed
//! there) or through [`text`], which backslash-escapes every ASCII punctuation character and breaks
//! mentions and issue references. Line breaks never survive, so no value can start a block of its
//! own.

use std::fmt::Write;

use ripplepath_engine::architecture::DeltaStatus;
use ripplepath_engine::policy::GateStatus;
use ripplepath_engine::{
    AnalysisReport, ChangeKind, CoverageStatus, SelectionDecision, Severity, TestReason, TestRecommendation,
};

use crate::text::{display, duration, neutralize_terminal_controls};

/// GitHub rejects issue comments above 65 536 characters; the step summary allows 1 MiB. One
/// budget for both keeps the comment and the summary identical.
pub const MAX_BYTES: usize = 60_000;
/// Items listed inside a collapsible section.
const DETAIL_LIMIT: usize = 25;
/// Items listed outside collapsible sections.
const INLINE_LIMIT: usize = 5;
/// Characters kept from one repository-derived value.
const VALUE_LIMIT: usize = 160;

fn clean(value: &str) -> String {
    let flat = neutralize_terminal_controls(&value.replace("\r\n", "\n").replace('\n', " "));
    if flat.chars().count() > VALUE_LIMIT {
        let mut short: String = flat.chars().take(VALUE_LIMIT - 1).collect();
        short.push('…');
        short
    } else {
        flat
    }
}

/// A literal code span. `in_table` escapes `|`, which GFM splits table cells on even inside code
/// spans (the backslash is then removed by the table parser, not shown).
pub fn code(value: &str, in_table: bool) -> String {
    let mut content = clean(value);
    if in_table {
        content = content.replace('|', "\\|");
    }
    if content.is_empty() {
        return "(empty)".to_owned();
    }
    let longest_run = content.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run + 1);
    // CommonMark strips one space from each side of a code span; padding keeps a leading or
    // trailing backtick from merging with the fence and keeps edge spaces visible.
    let pad = content.starts_with(['`', ' ']) || content.ends_with(['`', ' ']);
    if pad { format!("{fence} {content} {fence}") } else { format!("{fence}{content}{fence}") }
}

/// Inline text. Every ASCII punctuation character is backslash-escaped (CommonMark guarantees a
/// literal for each), which neutralises emphasis, links, HTML, entities and table pipes. `@` and
/// `#` are additionally followed by U+2060 WORD JOINER: GitHub links mentions and issue references
/// after Markdown rendering, so an escape alone would still notify a user named in a symbol.
pub fn text(value: &str) -> String {
    let mut out = String::new();
    for c in clean(value).chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
        if c == '@' || c == '#' {
            out.push('\u{2060}');
        }
    }
    out
}

fn upper<T: serde::Serialize + std::fmt::Debug>(value: T) -> String {
    crate::output::wire_name(&value).to_uppercase()
}

fn short_rev(spec: &str, commit: Option<&str>) -> String {
    match commit {
        Some(c) => format!("{} ({})", code(spec, false), code(&c[..c.len().min(10)], false)),
        None => code(spec, false),
    }
}

fn details(out: &mut String, summary: &str, body: &str) {
    let _ = write!(out, "<details><summary>{summary}</summary>\n\n{body}\n</details>\n\n");
}

fn more_line(total: usize, shown: usize) -> String {
    if total > shown { format!("- … {} more in `analysis.json`\n", total - shown) } else { String::new() }
}

pub fn render(report: &AnalysisReport) -> String {
    let full = render_with(report, true);
    if full.len() <= MAX_BYTES {
        return full;
    }
    // Bounded by construction: without collapsible lists every list is capped at INLINE_LIMIT
    // values of at most VALUE_LIMIT characters.
    let mut compact = render_with(report, false);
    compact.push_str("_Collapsible lists omitted to respect GitHub's size limit; see `analysis.json`._\n");
    compact
}

fn render_with(report: &AnalysisReport, with_details: bool) -> String {
    let mut out = String::new();
    let s = &report.summary;
    let risk = &report.risk;
    let policy = &report.policy;
    let arch = &report.architecture;
    let selection = &report.test_selection;

    let _ = writeln!(out, "## Ripplepath — Change Intelligence\n");
    let _ = writeln!(
        out,
        "**Policy: {}** · **Risk: {} ({}/100)** — a decomposition of review signals (model v{}), not a probability\n",
        upper(policy.result),
        upper(risk.level),
        risk.score,
        risk.model_version
    );
    let _ = writeln!(
        out,
        "Base {} → head {}\n",
        short_rev(&report.base.spec, report.base.commit.as_deref()),
        short_rev(&report.head.spec, report.head.commit.as_deref())
    );

    // ---- headline table ----
    let tests = match selection.decision {
        SelectionDecision::Selected => format!("{} of {}", selection.selected_units, selection.total_units),
        SelectionDecision::FullSuite => format!("**FULL SUITE** ({} tests)", selection.total_units),
    };
    let blast = format!(
        "{}{} symbol(s) in {} module(s)",
        if s.impact_truncated { "≥ " } else { "" },
        s.symbols_impacted,
        s.modules_impacted
    );
    let architecture = if arch.configured {
        format!("{} new violation(s), {} new cycle(s)", arch.summary.new_violations, arch.summary.new_cycles)
    } else {
        "no layers configured".to_owned()
    };
    let gaps = coverage_gaps(report);
    let coverage = match &gaps {
        None => "not measured (no coverage ingested)".to_owned(),
        Some(list) => format!("{} changed symbol(s) without measured coverage", list.len()),
    };
    let _ = writeln!(out, "| Changed | Blast radius | Tests to run | Architecture | Coverage gaps |");
    let _ = writeln!(out, "|---|---|---|---|---|");
    let _ = writeln!(
        out,
        "| {} symbol(s) in {} file(s) | {blast} | {tests} | {architecture} | {coverage} |\n",
        s.symbols_changed, s.files_changed
    );

    // ---- policy ----
    let _ = writeln!(out, "### Policy gates: {}\n", upper(policy.result));
    let _ = writeln!(out, "| Gate | Status | Detail |");
    let _ = writeln!(out, "|---|---|---|");
    for gate in &policy.gates {
        let status = match gate.status {
            GateStatus::Fail => "**FAIL**",
            GateStatus::Warn => "**WARN**",
            GateStatus::Pass => "pass",
            GateStatus::Off => "off (not enforced)",
            GateStatus::NotEvaluated => "not evaluated",
        };
        let _ = writeln!(out, "| `{}` | {status} | {} |", gate.gate.name(), text(&gate.detail));
    }
    let _ = writeln!(out, "\n{}\n", text(&policy.note));

    // ---- architecture ----
    if arch.configured {
        let new: Vec<_> = arch.violations.iter().filter(|v| v.status == DeltaStatus::New).collect();
        let new_cycles: Vec<_> = arch.cycles.iter().filter(|c| c.status == DeltaStatus::New).collect();
        if !new.is_empty() || !new_cycles.is_empty() {
            let _ = writeln!(out, "### Architecture: new violations\n");
            let mut list = String::new();
            for v in &new {
                let _ = writeln!(
                    list,
                    "- {} → {} breaks {}: {} —{}→ {} at {} ({})",
                    code(&v.from_layer, false),
                    code(&v.to_layer, false),
                    text(&v.description),
                    code(&display(&v.edge.from), false),
                    upper(v.edge.kind),
                    code(&display(&v.edge.to), false),
                    code(&format!("{}:{}", v.edge.file, v.edge.line), false),
                    upper(v.edge.evidence)
                );
            }
            for cycle in &new_cycles {
                let layers: Vec<String> = cycle.layers.iter().map(|l| code(l, false)).collect();
                let _ = writeln!(list, "- new layer cycle: {}", layers.join(" ↔ "));
            }
            list_section(&mut out, &list, with_details, "All new architecture findings");
        }
    }

    // ---- test selection ----
    let runtime = match (selection.selected_runtime_ms, selection.full_runtime_ms) {
        (Some(selected), Some(full)) => {
            format!(" · recorded runtime {} of {}", duration(selected), duration(full))
        }
        _ => String::new(),
    };
    let decision = match selection.decision {
        SelectionDecision::Selected => {
            format!("run **{} of {}** tests", selection.selected_units, selection.total_units)
        }
        SelectionDecision::FullSuite => "run the **FULL SUITE**".to_owned(),
    };
    let _ = writeln!(out, "### Test selection ({} mode): {decision}{runtime}\n", upper(selection.mode));
    for reason in &selection.fallback_reasons {
        let _ = writeln!(out, "- {} `{}`: {}", severity(reason.severity), text(&reason.code), text(&reason.detail));
    }
    if !selection.fallback_reasons.is_empty() {
        out.push('\n');
    }
    if with_details && !report.tests.is_empty() {
        let by_id: std::collections::BTreeMap<_, _> = report.tests.iter().map(|t| (&t.id, t)).collect();
        let mut list = String::new();
        let ordered: Vec<&TestRecommendation> =
            selection.ordered.iter().filter_map(|id| by_id.get(id).copied()).collect();
        for test in ordered.iter().take(DETAIL_LIMIT) {
            let why = match test.reason {
                TestReason::ChangedTest => "test changed".to_owned(),
                TestReason::StaticPath => format!(
                    "depth {}, weakest evidence {}{}",
                    test.depth,
                    upper(test.weakest_evidence),
                    if test.coverage_observed { ", measured coverage" } else { "" }
                ),
            };
            let _ = writeln!(list, "- {} {} — {why}", upper(test.tier), code(&display(&test.id), false));
        }
        list.push_str(&more_line(ordered.len(), DETAIL_LIMIT));
        details(&mut out, &format!("Recommended tests in run order ({})", ordered.len()), &list);
    }

    // ---- one evidence path ----
    // The first static-path test in run order: the selection ranks the strongest evidence first.
    let first_path = selection
        .ordered
        .iter()
        .filter_map(|id| report.tests.iter().find(|t| &t.id == id))
        .find(|t| t.reason == TestReason::StaticPath);
    if let Some(test) = first_path {
        let _ = writeln!(out, "### Most important evidence path\n");
        let _ = writeln!(
            out,
            "Why {} is selected: a chain of {} dependency edge(s) from the change, weakest evidence {}.\n",
            code(&display(&test.id), false),
            test.path.len(),
            upper(test.weakest_evidence)
        );
        let mut lines = vec![format!("changed       {}", clean(&display(&test.root)))];
        for hop in &test.path {
            let relation = if hop.via_dispatch { "DISPATCH".to_owned() } else { upper(hop.edge.kind) };
            lines.push(format!(
                "← {relation:<11} {}  ({}:{})",
                clean(&display(&hop.symbol)),
                clean(&hop.edge.file),
                hop.edge.line
            ));
        }
        code_block(&mut out, &lines);
    }

    // ---- coverage gaps ----
    if let Some(gaps) = gaps.filter(|g| !g.is_empty()) {
        let mut list = String::new();
        for (id, status) in gaps.iter().take(DETAIL_LIMIT) {
            let _ = writeln!(list, "- {} ({status})", code(id, false));
        }
        list.push_str(&more_line(gaps.len(), DETAIL_LIMIT));
        if with_details {
            details(&mut out, &format!("Coverage gaps ({})", gaps.len()), &list);
        }
    }

    if with_details {
        // ---- changed symbols ----
        let mut list = String::new();
        for symbol in report.changed_symbols.iter().take(DETAIL_LIMIT) {
            let _ = writeln!(
                list,
                "- {} {} ({})",
                upper(symbol.change).replace('_', " ").to_lowercase(),
                code(&display(&symbol.id), false),
                code(&format!("{}:{}", symbol.file, symbol.span.start_line), false)
            );
        }
        list.push_str(&more_line(report.changed_symbols.len(), DETAIL_LIMIT));
        details(&mut out, &format!("Changed symbols ({})", report.changed_symbols.len()), &list);

        // ---- blast radius ----
        let impacted: Vec<_> = report.impacted_symbols.iter().filter(|s| !s.is_test).collect();
        if !impacted.is_empty() {
            let mut list = String::new();
            for symbol in impacted.iter().take(DETAIL_LIMIT) {
                let _ = writeln!(
                    list,
                    "- depth {} {} (weakest evidence {})",
                    symbol.depth,
                    code(&display(&symbol.id), false),
                    upper(symbol.weakest_evidence)
                );
            }
            list.push_str(&more_line(impacted.len(), DETAIL_LIMIT));
            details(&mut out, &format!("Impacted non-test symbols ({})", impacted.len()), &list);
        }

        // ---- risk ----
        let mut list = String::from("| Signal | Value | Points |\n|---|---|---|\n");
        for signal in risk.signals.iter().filter(|s| s.points > 0) {
            let _ = writeln!(list, "| `{}` | {} | +{} |", signal_name(signal.id), signal.value, signal.points);
        }
        let skipped: Vec<String> =
            risk.signals.iter().filter(|s| !s.evaluated).map(|s| format!("`{}`", signal_name(s.id))).collect();
        if !skipped.is_empty() {
            let _ = writeln!(list, "\nNot evaluated (input missing, scored 0): {}", skipped.join(", "));
        }
        let _ = writeln!(list, "\n{}", text(&risk.interpretation));
        details(&mut out, &format!("Risk decomposition ({}/100)", risk.score), &list);

        // ---- uncertainty ----
        if !report.uncertainty.is_empty() {
            let mut list = String::new();
            for item in report.uncertainty.iter().take(DETAIL_LIMIT) {
                let location = match (&item.file, item.line) {
                    (Some(file), Some(line)) => format!("{} ", code(&format!("{file}:{line}"), false)),
                    (Some(file), None) => format!("{} ", code(file, false)),
                    _ => String::new(),
                };
                let _ = writeln!(list, "- {} {location}{}", severity(item.severity), text(&item.detail));
            }
            list.push_str(&more_line(report.uncertainty.len(), DETAIL_LIMIT));
            let high = report.uncertainty.iter().filter(|u| u.severity == Severity::High).count();
            details(&mut out, &format!("Uncertainty ({} item(s), {high} high)", report.uncertainty.len()), &list);
        }
    }

    let _ = writeln!(
        out,
        "<sub>Ripplepath {} · analysis schema {} · decision support, not proof of safety · full report in \
         <code>analysis.json</code></sub>",
        text(&report.tool_version),
        report.schema_version
    );
    out
}

/// A list shown in full when short, otherwise its first items inline and the rest collapsed.
fn list_section(out: &mut String, list: &str, with_details: bool, summary: &str) {
    let lines: Vec<&str> = list.lines().collect();
    for line in lines.iter().take(INLINE_LIMIT) {
        let _ = writeln!(out, "{line}");
    }
    out.push('\n');
    if lines.len() > INLINE_LIMIT {
        if with_details {
            let rest: Vec<&str> = lines.iter().skip(INLINE_LIMIT).take(DETAIL_LIMIT).copied().collect();
            let mut body = rest.join("\n");
            body.push('\n');
            body.push_str(&more_line(lines.len() - INLINE_LIMIT, rest.len()));
            details(out, &format!("{summary} ({} more)", lines.len() - INLINE_LIMIT), &body);
        } else {
            out.push_str(&more_line(lines.len(), INLINE_LIMIT));
            out.push('\n');
        }
    }
}

/// A fenced block is literal like a code span; its fence is longer than any backtick run inside so
/// that no line can close it early.
fn code_block(out: &mut String, lines: &[String]) {
    let longest_run = lines.iter().flat_map(|l| l.split(|c| c != '`').map(str::len)).max().unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    let _ = writeln!(out, "{fence}text");
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out, "{fence}\n");
}

/// Changed, non-test, still-existing symbols that coverage did not see execute. `None` when no
/// coverage was ingested: absence of measurement is not a gap in coverage.
fn coverage_gaps(report: &AnalysisReport) -> Option<Vec<(String, &'static str)>> {
    if report.evidence.coverage_reports == 0 {
        return None;
    }
    Some(
        report
            .changed_symbols
            .iter()
            .filter(|s| !s.is_test && s.change != ChangeKind::Deleted)
            .filter_map(|s| match s.coverage {
                Some(CoverageStatus::NotCovered) => Some((display(&s.id), "not covered by any recorded test")),
                Some(CoverageStatus::NoData) => Some((display(&s.id), "no coverage data for its file")),
                _ => None,
            })
            .collect(),
    )
}

fn signal_name(id: ripplepath_engine::risk::SignalId) -> String {
    crate::output::wire_name(&id)
}

fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::High => "**HIGH**",
        Severity::Medium => "MEDIUM",
        Severity::Low => "LOW",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_spans_survive_backticks_pipes_and_line_breaks() {
        assert_eq!(code("a`b", false), "``a`b``");
        assert_eq!(code("`x`", false), "`` `x` ``");
        assert_eq!(code("a|b", true), "`a\\|b`");
        assert_eq!(code("a|b", false), "`a|b`");
        assert_eq!(code("x\ny", false), "`x y`");
        assert_eq!(code("", false), "(empty)");
    }

    #[test]
    fn text_escapes_markdown_html_and_breaks_mentions() {
        let hostile = "<img src=x onerror=alert(1)> **bold** [link](http://e.vil) @octocat #12 a|b";
        let escaped = text(hostile);
        let html = to_html(&escaped);
        for element in ["<img", "<strong", "<a "] {
            assert!(!html.contains(element), "{html}");
        }
        assert!(escaped.contains("\\<img"), "{escaped}");
        assert!(escaped.contains("\\*\\*bold\\*\\*"), "{escaped}");
        assert!(escaped.contains("\\[link\\]\\(http\\:"), "{escaped}");
        assert!(escaped.contains("\\@\u{2060}octocat"), "{escaped}");
        assert!(escaped.contains("\\#\u{2060}12"), "{escaped}");
        assert!(escaped.contains("a\\|b"), "{escaped}");
    }

    #[test]
    fn values_cannot_open_blocks_or_hide_text() {
        let hostile = "x\n\n</details>\n# Heading\r\n\u{202e}gnp.exe";
        let escaped = text(hostile);
        assert!(!escaped.contains('\n') && !escaped.contains('\r'), "{escaped}");
        assert!(escaped.contains("\\<\\/details\\>"), "{escaped}");
        assert!(escaped.contains("\\\\u\\{202e\\}"), "{escaped}");
        let span = code(hostile, false);
        assert!(!span.contains('\n'), "{span}");
    }

    #[test]
    fn long_values_are_truncated() {
        let long = "a".repeat(1000);
        assert_eq!(clean(&long).chars().count(), VALUE_LIMIT);
    }

    #[test]
    fn hostile_repository_text_cannot_inject_markup() {
        use ripplepath_core::SymbolId;
        let mut report = crate::test_support::java_banking_report();
        let evil = "x<script>alert(1)</script>\n# pwned\n</details><img src=x onerror=alert(1)>@octocat";
        report.changed_symbols[0].id = SymbolId::new(format!("java:a.{evil}#m"));
        report.changed_symbols[0].file = format!("src/`a`|b\n{evil}.java");
        for v in &mut report.architecture.violations {
            v.description = evil.to_owned();
            v.from_layer = evil.to_owned();
            v.edge.file = evil.to_owned();
        }
        for gate in &mut report.policy.gates {
            gate.detail = evil.to_owned();
        }
        report.base.spec = evil.to_owned();
        let clean_html = to_html(&render(&crate::test_support::java_banking_report()));
        let html = to_html(&render(&report));
        // Rendered by a CommonMark + GFM-tables parser: hostile values must come out as text.
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("<h1"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        // Same document structure as without hostile values: no element was added or closed early.
        for tag in ["<details>", "</details>", "<h2", "<h3", "<table>", "<tr>", "<td", "<li>", "<pre>"] {
            assert_eq!(html.matches(tag).count(), clean_html.matches(tag).count(), "{tag}\n{html}");
        }
    }

    fn to_html(md: &str) -> String {
        let parser = pulldown_cmark::Parser::new_ext(md, pulldown_cmark::Options::ENABLE_TABLES);
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        html
    }

    #[test]
    fn output_is_deterministic_and_bounded() {
        let report = crate::test_support::java_banking_report();
        assert_eq!(render(&report), render(&report));
        let mut huge = report.clone();
        let template = huge.changed_symbols[0].clone();
        for i in 0..5000 {
            let mut symbol = template.clone();
            symbol.id = ripplepath_core::SymbolId::new(format!("java:a.{}#m{i}", "Long".repeat(60)));
            huge.changed_symbols.push(symbol);
            huge.uncertainty.push(ripplepath_engine::Uncertainty {
                severity: Severity::Low,
                kind: ripplepath_engine::UncertaintyKind::UnresolvedReference,
                file: Some(format!("src/{}.java", "deep/".repeat(50))),
                line: Some(i),
                symbol: None,
                detail: "y".repeat(500),
            });
        }
        let md = render(&huge);
        assert!(md.len() <= MAX_BYTES, "{}", md.len());
    }

    #[test]
    fn demo_summary_reports_the_policy_failure_and_its_evidence() {
        let md = render(&crate::test_support::java_banking_report());
        assert!(md.starts_with("## Ripplepath — Change Intelligence\n"), "{md}");
        assert!(md.contains("**Policy: FAIL**"), "{md}");
        assert!(md.contains("not a probability"), "{md}");
        assert!(md.contains("| `new_architecture_violation` | **FAIL** |"), "{md}");
        assert!(md.contains("`domain` → `api`"), "{md}");
        assert!(md.contains("run the **FULL SUITE**"), "{md}");
        assert!(md.contains("### Most important evidence path"), "{md}");
        // No CI history was ingested, so no runtime may be claimed.
        assert!(!md.contains("recorded runtime"), "{md}");
    }

    #[test]
    fn code_block_fence_outgrows_content() {
        let mut out = String::new();
        code_block(&mut out, &["```` evil".to_owned()]);
        assert!(out.starts_with("`````text\n"), "{out}");
    }
}
