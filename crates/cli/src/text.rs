//! Human-readable report. Plain text, no colour: it is read in terminals, CI logs and pasted into
//! PRs, and colour codes would corrupt the latter two.

use std::fmt::Write;

use ripplepath_core::SymbolId;
use ripplepath_engine::architecture::{ArchitectureReport, DeltaStatus, Violation};
use ripplepath_engine::policy::GateStatus;
use ripplepath_engine::{
    AnalysisReport, ArchitectureCheck, ChangeKind, ConfigChange, ConfigSource, CoverageStatus, Reliability,
    SelectionDecision, Severity, TestReason,
};
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
        let coverage = match symbol.coverage {
            Some(CoverageStatus::Covered) => "  [covered]",
            Some(CoverageStatus::NotCovered) => "  [NOT covered by any recorded test]",
            Some(CoverageStatus::NoData) => "  [no coverage data]",
            None => "",
        };
        let label = match symbol.change {
            ChangeKind::Added => "added",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Modified => "modified",
            ChangeKind::SignatureChanged => "signature",
        };
        let _ = writeln!(
            out,
            "  {label:<9} {}  ({}:{}){coverage}",
            display(&symbol.id),
            symbol.file,
            symbol.span.start_line
        );
        if let Some(previous) = &symbol.previous_id {
            let _ = writeln!(out, "            was {}", display(previous));
        }
        if let Some(moved) = &symbol.probable_move {
            let _ = writeln!(out, "            identical body to {} (probable move)", display(moved));
        }
    }
    more(&mut out, report.changed_symbols.len());

    let selection = &report.test_selection;
    let decision = match selection.decision {
        SelectionDecision::Selected => format!("run {} of {} tests", selection.selected_units, selection.total_units),
        SelectionDecision::FullSuite => "run the FULL SUITE".to_owned(),
    };
    let runtime = match (selection.selected_runtime_ms, selection.full_runtime_ms) {
        (Some(selected), Some(full)) => format!("  |  recorded runtime {} of {}", duration(selected), duration(full)),
        _ => String::new(),
    };
    section(&mut out, &format!("Test selection ({:?} mode): {decision}{runtime}", selection.mode));
    for reason in &selection.fallback_reasons {
        let _ = writeln!(out, "  {} {}: {}", severity(reason.severity), reason.code, reason.detail);
    }
    for note in &selection.notes {
        let _ = writeln!(out, "  note: {note}");
    }
    let ev = &report.evidence;
    if ev.coverage_reports > 0 || ev.test_runs > 0 {
        let _ = writeln!(
            out,
            "  evidence: {} coverage report(s) ({} from other commits), {} coverage edges, {} CI run(s)",
            ev.coverage_reports, ev.coverage_reports_other_commits, ev.coverage_edges, ev.test_runs
        );
    } else {
        let _ = writeln!(out, "  evidence: static analysis only (no coverage or CI history ingested)");
    }

    section(&mut out, "Recommended tests, in run order (decision support, not proof of safety)");
    if report.tests.is_empty() {
        let _ = writeln!(out, "  none found — no test has evidence of exercising the change");
    }
    let by_id: std::collections::BTreeMap<_, _> = report.tests.iter().map(|t| (&t.id, t)).collect();
    let ordered = selection.ordered.iter().filter_map(|id| by_id.get(id).copied());
    for test in ordered.take(LIST_LIMIT) {
        let history = test.history.as_ref().map_or_else(String::new, |h| {
            let reliability = match h.reliability {
                Reliability::Stable => "stable",
                Reliability::Flaky => "FLAKY",
                Reliability::ConsistentlyFailing => "FAILING",
                Reliability::InsufficientData => "few runs",
            };
            let median = h.median_duration_ms.map_or_else(String::new, |d| format!(", median {}", duration(d)));
            format!("  [{reliability}: {}/{} runs failed{median}]", h.failures, h.runs)
        });
        let _ = writeln!(out, "  {:?}  {}{history}", test.tier, display(&test.id));
        match test.reason {
            TestReason::ChangedTest => {
                let _ = writeln!(out, "      test code changed");
            }
            TestReason::StaticPath => {
                let measured = if test.coverage_observed { ", includes measured coverage" } else { "" };
                let _ =
                    writeln!(out, "      depth {}, weakest evidence {:?}{measured}", test.depth, test.weakest_evidence);
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
        let severity = severity(item.severity);
        let location = match (&item.file, item.line) {
            (Some(file), Some(line)) => format!("{file}:{line}: "),
            (Some(file), None) => format!("{file}: "),
            _ => String::new(),
        };
        let _ = writeln!(out, "  {severity} {location}{}", item.detail);
    }
    more(&mut out, report.uncertainty.len());

    render_governance(&mut out, report);
    out
}

fn render_governance(out: &mut String, report: &AnalysisReport) {
    let risk = &report.risk;
    section(
        out,
        &format!(
            "Risk {}/100 ({}), model v{}",
            risk.score,
            format!("{:?}", risk.level).to_uppercase(),
            risk.model_version
        ),
    );
    let _ = writeln!(out, "  {}", risk.interpretation);
    for signal in risk.signals.iter().filter(|s| s.points > 0) {
        let _ = writeln!(
            out,
            "  +{:<3} {:?}: value {} -> {} unit(s) x {} (cap {})",
            signal.points, signal.id, signal.value, signal.units, signal.weight, signal.cap
        );
        for item in signal.evidence.iter().take(3) {
            let _ = writeln!(out, "         {item}");
        }
        if signal.evidence.len() > 3 || signal.evidence_truncated > 0 {
            let _ = writeln!(
                out,
                "         ... {} more",
                signal.evidence.len() - 3.min(signal.evidence.len()) + signal.evidence_truncated
            );
        }
    }
    let skipped: Vec<String> = risk.signals.iter().filter(|s| !s.evaluated).map(|s| format!("{:?}", s.id)).collect();
    if !skipped.is_empty() {
        let _ = writeln!(out, "  not evaluated (missing input, scored 0): {}", skipped.join(", "));
    }

    let api = &report.api_surface;
    if !api.changes.is_empty() {
        section(out, &format!("Public API: {} breaking, {} added", api.breaking, api.added));
        for change in api.changes.iter().filter(|c| c.kind.is_breaking()).take(LIST_LIMIT) {
            let _ = writeln!(out, "  {:?} {}  ({}:{})", change.kind, display(&change.id), change.file, change.line);
        }
    }

    render_architecture(out, &report.architecture);

    let owners = &report.owners;
    match &owners.source {
        None => section(out, "Owners: no CODEOWNERS file in head"),
        Some(source) => {
            let changed = if owners.changed_in_head { " (edited by this change)" } else { "" };
            section(out, &format!("Owners (from {source}{changed}; review routing, not authorization)"));
            for owner in owners.owners.iter().take(LIST_LIMIT) {
                let _ = writeln!(
                    out,
                    "  {}  changed {} file(s), impacted {} file(s)",
                    owner.owner, owner.changed_files, owner.impacted_files
                );
            }
            if owners.unowned_changed_files > 0 {
                let _ = writeln!(out, "  {} changed file(s) have no owner", owners.unowned_changed_files);
            }
        }
    }
    for error in owners.errors.iter().take(5) {
        let _ = writeln!(out, "  CODEOWNERS line {}: {}", error.line, error.message);
    }

    let config = &report.config;
    let source = match config.source {
        ConfigSource::Defaults => "built-in defaults (no ripplepath.yml in base)".to_owned(),
        ConfigSource::BaseRevision => format!("{} from base {}", config.path, config.revision),
        ConfigSource::InvalidUsingDefaults => format!("defaults: {} in base is INVALID", config.path),
    };
    let policy = &report.policy;
    section(out, &format!("Policy: {}  (config: {source})", format!("{:?}", policy.result).to_uppercase()));
    if config.head_change != ConfigChange::Unchanged && config.head_change != ConfigChange::Absent {
        let _ = writeln!(out, "  note: head {:?} {}; it applies only after merge", config.head_change, config.path);
    }
    for error in config.errors.iter().chain(&config.head_errors) {
        let _ = writeln!(out, "  config error: {error}");
    }
    for gate in &policy.gates {
        let status = match gate.status {
            GateStatus::Pass => "pass",
            GateStatus::Warn => "WARN",
            GateStatus::Fail => "FAIL",
            GateStatus::Off => "off ",
            GateStatus::NotEvaluated => "n/a ",
        };
        let _ = writeln!(out, "  {status} {}: {}", gate.gate.name(), gate.detail);
        if matches!(gate.status, GateStatus::Warn | GateStatus::Fail) {
            for item in gate.evidence.iter().take(3) {
                let _ = writeln!(out, "       {item}");
            }
        }
    }
    let _ = writeln!(out, "  {}", policy.note);
}

/// `with_status` is false for single-revision checks, where every finding is simply present.
fn violation_line(out: &mut String, v: &Violation, with_status: bool) {
    let status = if with_status { format!("{:?} ", v.status) } else { String::new() };
    let _ = writeln!(
        out,
        "  {status}{} -> {}: {}  {} -{:?}-> {}  ({}:{}, {:?})",
        v.from_layer,
        v.to_layer,
        v.description,
        display(&v.edge.from),
        v.edge.kind,
        display(&v.edge.to),
        v.edge.file,
        v.edge.line,
        v.edge.evidence
    );
}

fn render_architecture(out: &mut String, arch: &ArchitectureReport) {
    if !arch.configured {
        section(out, "Architecture: no layers configured (see docs/ARCHITECTURE_RULES.md)");
    } else {
        let s = &arch.summary;
        section(
            out,
            &format!(
                "Architecture: {} new, {} pre-existing, {} removed violation(s); {} new, {} pre-existing cycle(s)",
                s.new_violations, s.pre_existing_violations, s.removed_violations, s.new_cycles, s.pre_existing_cycles
            ),
        );
        let shown = arch.violations.iter().filter(|v| v.status != DeltaStatus::PreExisting);
        for v in shown.take(LIST_LIMIT) {
            violation_line(out, v, true);
        }
        for cycle in arch.cycles.iter().filter(|c| c.status != DeltaStatus::PreExisting) {
            let _ = writeln!(out, "  {:?} cycle: {}", cycle.status, cycle.layers.join(" <-> "));
        }
        for dep in arch.layer_dependencies.iter().filter(|d| d.direction_reversed) {
            let _ = writeln!(out, "  direction reversed: {} now depends on {}", dep.from, dep.to);
        }
    }
    if !arch.module_coupling.is_empty() {
        let _ = writeln!(out, "  module coupling changed for {} pair(s):", arch.module_coupling.len());
        for pair in arch.module_coupling.iter().take(5) {
            let _ =
                writeln!(out, "    {} -> {}: {} -> {} edge(s)", pair.from, pair.to, pair.base_edges, pair.head_edges);
        }
    }
}

pub fn render_architecture_check(check: &ArchitectureCheck) -> String {
    let mut out = String::new();
    let rev = check.revision.commit.as_deref().unwrap_or(&check.revision.tree);
    let _ = writeln!(out, "Ripplepath architecture check of {} ({})", check.revision.spec, &rev[..rev.len().min(10)]);
    if !check.config_found {
        let _ = writeln!(out, "  no ripplepath.yml in this revision; nothing to check");
        return neutralize_terminal_controls(&out);
    }
    for (layer, symbols) in &check.layers {
        let _ = writeln!(out, "  layer {layer}: {symbols} symbol(s)");
    }
    for rule in &check.rules {
        let _ = writeln!(out, "  rule {}: {}", rule.index + 1, rule.description);
    }
    section(&mut out, &format!("{} violation(s)", check.violations.len()));
    for v in check.violations.iter().take(LIST_LIMIT) {
        violation_line(&mut out, v, false);
    }
    more(&mut out, check.violations.len());
    section(&mut out, &format!("{} layer cycle(s)", check.cycles.len()));
    for cycle in &check.cycles {
        let _ = writeln!(out, "  {}", cycle.layers.join(" <-> "));
    }
    neutralize_terminal_controls(&out)
}

fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::High => "HIGH",
        Severity::Medium => "MED ",
        Severity::Low => "LOW ",
    }
}

fn duration(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1000)
    } else if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
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
