//! Human-readable report. Plain text, no colour: it is read in terminals, CI logs and pasted into
//! PRs, and colour codes would corrupt the latter two.

use std::fmt::Write;

use ripplepath_core::SymbolId;
use ripplepath_engine::{AnalysisReport, ChangeKind, Severity, TestReason};
use ripplepath_graph::Hop;

const LIST_LIMIT: usize = 25;

pub fn render(report: &AnalysisReport) -> String {
    neutralize_terminal_controls(&render_raw(report))
}

/// Symbol names, paths and revision specs come from the analysed repository or the command line.
/// Escape sequences in them could rewrite the terminal (ANSI/OSC) or visually reorder text
/// (Unicode bidi overrides, "Trojan Source"), so every control and bidi-formatting character except
/// newline is shown as a visible `\u{..}` escape instead.
pub fn neutralize_terminal_controls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if (c.is_control() && c != '\n') || is_invisible_format(c) {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// Unicode general category Cf (format: bidi controls, zero-width characters, BOM, tag characters,
/// …) plus the line/paragraph separators, which some terminals render as line breaks. `std` does
/// not expose general categories, so the Cf ranges are listed explicitly (Unicode 16).
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

fn render_raw(report: &AnalysisReport) -> String {
    let mut out = String::new();
    let s = &report.summary;
    let short = |commit: &Option<String>, spec: &str| {
        commit.as_deref().map_or_else(|| spec.to_owned(), |c| format!("{spec} ({})", &c[..c.len().min(10)]))
    };
    let _ = writeln!(out, "Ripplepath change analysis");
    let _ = writeln!(out, "  base  {}", short(&report.base.commit, &report.base.spec));
    let _ = writeln!(out, "  head  {}", short(&report.head.commit, &report.head.spec));
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Files changed {}  |  symbols changed {}  |  impacted {} across {} module(s)  |  tests {} of {}",
        s.files_changed, s.symbols_changed, s.symbols_impacted, s.modules_impacted, s.tests_recommended, s.tests_total
    );
    if s.impact_truncated {
        let _ = writeln!(out, "NOTE: impact traversal was truncated; the impacted set is a lower bound.");
    }

    section(&mut out, "Changed symbols");
    for symbol in report.changed_symbols.iter().take(LIST_LIMIT) {
        let label = match symbol.change {
            ChangeKind::Added => "added",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Modified => "modified",
            ChangeKind::SignatureChanged => "signature",
        };
        let _ = writeln!(out, "  {label:<9} {}  ({}:{})", display(&symbol.id), symbol.file, symbol.span.start_line);
        if let Some(previous) = &symbol.previous_id {
            let _ = writeln!(out, "            was {}", display(previous));
        }
        if let Some(moved) = &symbol.probable_move {
            let _ = writeln!(out, "            identical body to {} (probable move)", display(moved));
        }
    }
    more(&mut out, report.changed_symbols.len());

    section(&mut out, "Recommended tests (static evidence only; not a guarantee of coverage)");
    if report.tests.is_empty() {
        let _ = writeln!(out, "  none found — no test has a static dependency path to the change");
    }
    for test in report.tests.iter().take(LIST_LIMIT) {
        match test.reason {
            TestReason::ChangedTest => {
                let _ = writeln!(out, "  {}  — test itself changed", display(&test.id));
            }
            TestReason::StaticPath => {
                let _ = writeln!(
                    out,
                    "  {}  — depth {}, weakest evidence {:?}",
                    display(&test.id),
                    test.depth,
                    test.weakest_evidence
                );
                let _ = writeln!(out, "      changed      {}", display(&test.root));
                path(&mut out, &test.path);
            }
        }
    }
    more(&mut out, report.tests.len());

    section(&mut out, "Impacted symbols");
    for symbol in report.impacted_symbols.iter().filter(|s| !s.is_test).take(LIST_LIMIT) {
        let _ = writeln!(out, "  d{} {}  ({:?})", symbol.depth, display(&symbol.id), symbol.weakest_evidence);
    }
    more(&mut out, report.impacted_symbols.iter().filter(|s| !s.is_test).count());

    section(&mut out, "Uncertainty");
    if report.uncertainty.is_empty() {
        let _ = writeln!(out, "  none recorded");
    }
    for item in report.uncertainty.iter().take(LIST_LIMIT) {
        let severity = match item.severity {
            Severity::High => "HIGH",
            Severity::Medium => "MED ",
            Severity::Low => "LOW ",
        };
        let location = match (&item.file, item.line) {
            (Some(file), Some(line)) => format!("{file}:{line}: "),
            (Some(file), None) => format!("{file}: "),
            _ => String::new(),
        };
        let _ = writeln!(out, "  {severity} {location}{}", item.detail);
    }
    more(&mut out, report.uncertainty.len());
    out
}

fn section(out: &mut String, title: &str) {
    let _ = writeln!(out, "\n{title}");
}

fn more(out: &mut String, total: usize) {
    if total > LIST_LIMIT {
        let _ = writeln!(out, "  ... {} more (use --format json for everything)", total - LIST_LIMIT);
    }
}

fn path(out: &mut String, hops: &[Hop]) {
    for hop in hops {
        let relation = if hop.via_dispatch {
            "dispatched via OVERRIDES".to_owned()
        } else {
            format!("{:?}", hop.edge.kind).to_uppercase()
        };
        let _ = writeln!(out, "      ← {relation:<12} {}  ({}:{})", display(&hop.symbol), hop.edge.file, hop.edge.line);
    }
}

/// Shortens ids for terminals; JSON output keeps full ids.
/// - `java:com.acme.bank.domain.Account#withdraw(Money)` → `domain.Account#withdraw(Money)`
/// - `ts:src/pricing/discount.ts#applyDiscount` → `discount.ts#applyDiscount`
/// - `file:src/a/B.java` → `B.java`
fn display(id: &SymbolId) -> String {
    let raw = id.as_str();
    let (prefix, body) = raw.split_once(':').unwrap_or(("", raw));
    let (qualified, member) = body.split_once('#').map_or((body, None), |(q, m)| (q, Some(m)));
    let short = match prefix {
        "ts" | "file" => qualified.rsplit('/').next().unwrap_or(qualified).to_owned(),
        _ => {
            let segments: Vec<&str> = qualified.split('.').collect();
            let first_type =
                segments.iter().position(|s| s.chars().next().is_some_and(char::is_uppercase)).unwrap_or(0);
            segments[first_type.saturating_sub(1)..].join(".")
        }
    };
    match member {
        Some(member) => format!("{short}#{member}"),
        None => short,
    }
}

#[cfg(test)]
mod tests {
    use super::neutralize_terminal_controls;

    #[test]
    fn escapes_ansi_and_bidi_but_keeps_newlines_and_unicode() {
        let hostile = "ok\n\u{1b}[31mred\u{1b}[0m \u{202e}evil\u{2066} ünïcode";
        assert_eq!(
            neutralize_terminal_controls(hostile),
            r"ok
\u{1b}[31mred\u{1b}[0m \u{202e}evil\u{2066} ünïcode"
        );
    }

    #[test]
    fn shortens_ids_per_language() {
        use super::display;
        use ripplepath_core::SymbolId;
        let short = |raw: &str| display(&SymbolId::new(raw));
        assert_eq!(short("java:com.acme.bank.domain.Account#withdraw(Money)"), "domain.Account#withdraw(Money)");
        assert_eq!(short("ts:src/pricing/discount.ts#applyDiscount"), "discount.ts#applyDiscount");
        assert_eq!(short("ts:src/cart.test.ts#test:Cart > totals"), "cart.test.ts#test:Cart > totals");
        assert_eq!(short("file:src/a/B.java"), "B.java");
    }

    #[test]
    fn escapes_invisible_format_characters_and_separators() {
        let hostile = "a\u{200b}b\u{061c}c\u{feff}d\u{2028}e\u{e0041}f\u{9b}g";
        assert_eq!(neutralize_terminal_controls(hostile), r"a\u{200b}b\u{61c}c\u{feff}d\u{2028}e\u{e0041}f\u{9b}g");
    }
}
