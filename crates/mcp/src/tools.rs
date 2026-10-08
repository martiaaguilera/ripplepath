//! Tool definitions, argument validation and the projections of engine output they return.
//!
//! Outputs reuse the engine's serialized types (`Hop`, `Edge`, `Uncertainty`, …) so field names
//! match `analysis.json`. Every list is capped and every cap that bites is announced in
//! `truncation`: an agent must be able to tell "nothing more" from "more not shown".

use std::collections::VecDeque;
use std::rc::Rc;

use ripplepath_core::{Symbol, SymbolId};
use ripplepath_engine::architecture::DeltaStatus;
use ripplepath_engine::explore::{RevisionView, fact_cache, load_revision, resolve_revision};
use ripplepath_engine::{
    AnalysisReport, AnalyzeOptions, Limits, RevisionInfo, SelectionMode, analyze, check_architecture,
};
use ripplepath_graph::{ImpactOptions, PathOptions};
use serde_json::{Map, Value, json};

use crate::{McpConfig, McpError};

/// Largest serialized tool result; larger answers are refused with advice to narrow the query.
pub const MAX_RESULT_BYTES: usize = 512 * 1024;
pub const TOOL_NAMES: &[&str] = &[
    "analyze_change",
    "impacted_symbols",
    "tests_for_change",
    "architecture_status",
    "find_symbols",
    "symbol_info",
    "dependency_path",
    "what_if",
];

const MAX_REVISION_LEN: usize = 256;
const MAX_SYMBOL_LEN: usize = 2048;
/// Indexed revisions and analysis reports kept in memory: an agent typically asks several
/// questions about the same change in a row.
const CACHED_VIEWS: usize = 4;
const CACHED_REPORTS: usize = 4;

const SUMMARY_CHANGED: usize = 50;
const SUMMARY_IMPACTED: usize = 25;
const SUMMARY_TESTS: usize = 25;
const SUMMARY_UNCERTAINTY: usize = 30;
const SUMMARY_VIOLATIONS: usize = 20;
const LIST_VIOLATIONS: usize = 100;
const LIST_EDGES: usize = 50;
const LIST_TESTS: usize = 100;
const SUGGESTIONS: usize = 10;

const NOT_A_PROOF: &str = "Static analysis plus ingested evidence: reflection, dependency injection wiring, \
string-based dispatch and unresolved references can hide dependents. Treat lists as a lower bound when \
`uncertainty` is non-empty.";

pub(crate) enum CallOutcome {
    UnknownTool,
    Done(Result<Value, String>),
}

pub(crate) struct Tools {
    config: McpConfig,
    limits: Limits,
    views: VecDeque<(String, Rc<RevisionView>)>,
    reports: VecDeque<(String, Rc<AnalysisReport>)>,
}

impl Tools {
    pub(crate) fn new(config: McpConfig) -> Result<Self, McpError> {
        // Fail at startup, not on the first call, when the fixed repository is unusable.
        resolve_revision(&config.repo, &config.default_head).map_err(|e| McpError::Repository(e.to_string()))?;
        Ok(Self { config, limits: Limits::default(), views: VecDeque::new(), reports: VecDeque::new() })
    }

    pub(crate) fn definitions(&self) -> Value {
        let rev = |what: &str, default: &str| {
            json!({ "type": "string", "minLength": 1, "maxLength": MAX_REVISION_LEN,
                    "description": format!("{what} (branch, tag, SHA, HEAD~1, ...). Default: {default}.") })
        };
        let base = rev("Base revision of the change", &self.config.default_base);
        let head = rev("Head revision of the change", &self.config.default_head);
        let at = rev("Revision whose graph is queried", &self.config.default_head);
        let mode = json!({ "type": "string", "enum": ["conservative", "balanced", "fast_feedback"],
            "description": "Test selection policy. Default: tests.mode from the base revision's ripplepath.yml, else balanced." });
        let symbol = |what: &str| {
            json!({ "type": "string", "minLength": 1, "maxLength": MAX_SYMBOL_LEN,
                    "description": format!("{what}: an exact symbol id such as java:com.acme.Foo#bar(String) or ts:src/a.ts#fn (use find_symbols to get one).") })
        };
        let int = |min: u32, max: u32, default: u32, what: &str| json!({ "type": "integer", "minimum": min, "maximum": max, "default": default, "description": what });
        let annotations =
            json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false });
        let tool = |name: &str, title: &str, description: &str, properties: Value, required: &[&str]| {
            json!({
                "name": name,
                "title": title,
                "description": description,
                "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
                "annotations": annotations,
            })
        };
        json!([
            tool(
                "analyze_change",
                "Analyze a change",
                "Compact summary of the change between two revisions: risk decomposition (not a probability), changed symbols, top impacted symbols with evidence paths, test selection with reasons, NEW architecture violations, API surface, policy verdict and uncertainty.",
                json!({ "base": base, "head": head, "mode": mode }),
                &[]
            ),
            tool(
                "impacted_symbols",
                "Impacted symbols of a change",
                "Symbols that depend (transitively) on what changed between two revisions, nearest first, each with the shortest evidence-carrying path from a changed symbol.",
                json!({ "base": base, "head": head, "limit": int(1, 500, 50, "Maximum symbols listed.") }),
                &[]
            ),
            tool(
                "tests_for_change",
                "Tests for a change",
                "Which tests to run for a change, in order, with the reason, evidence tier and dependency path for each, and whether the evidence is good enough to run fewer than all tests.",
                json!({ "base": base, "head": head, "mode": mode }),
                &[]
            ),
            tool(
                "architecture_status",
                "Architecture status",
                "With base/head: architecture violations and layer cycles INTRODUCED by the change (rules from the base revision). Otherwise: all violations of one revision against its own ripplepath.yml.",
                json!({ "rev": at, "base": base, "head": head }),
                &[]
            ),
            tool(
                "find_symbols",
                "Find symbols",
                "Find symbol ids by case-insensitive substring of id or name, or by exact file path.",
                json!({ "query": { "type": "string", "minLength": 1, "maxLength": MAX_SYMBOL_LEN }, "rev": at, "limit": int(1, 100, 20, "Maximum symbols listed.") }),
                &["query"]
            ),
            tool(
                "symbol_info",
                "Symbol info",
                "Kind, location, layer and coverage of a symbol; its incoming and outgoing dependency edges with evidence; architecture violations it takes part in; tests with evidence of reaching it.",
                json!({ "symbol": symbol("Symbol"), "rev": at }),
                &["symbol"]
            ),
            tool(
                "dependency_path",
                "Dependency path",
                "Shortest chain of dependency edges between two symbols (from depends on ... depends on to), each hop with kind, evidence class and file:line. Falls back to the reverse direction when no forward chain exists.",
                json!({ "from": symbol("Dependent symbol"), "to": symbol("Dependency"), "rev": at, "max_depth": int(1, 16, 8, "Maximum hops.") }),
                &["from", "to"]
            ),
            tool(
                "what_if",
                "What if I modify this symbol",
                "Impact of modifying a symbol that has not been changed yet: dependents to inspect and tests to run, each with its evidence path, plus the modules and architecture layers reached. Computed on the revision's graph as if the symbol's body were modified.",
                json!({ "symbol": symbol("Symbol you intend to modify"), "rev": at, "max_depth": int(1, 12, 6, "Maximum dependency depth."), "limit": int(1, 500, 50, "Maximum impacted symbols and tests listed.") }),
                &["symbol"]
            ),
        ])
    }

    pub(crate) fn call(&mut self, name: &str, arguments: &Map<String, Value>) -> CallOutcome {
        let result = match name {
            "analyze_change" => self.analyze_change(arguments),
            "impacted_symbols" => self.impacted_symbols(arguments),
            "tests_for_change" => self.tests_for_change(arguments),
            "architecture_status" => self.architecture_status(arguments),
            "find_symbols" => self.find_symbols(arguments),
            "symbol_info" => self.symbol_info(arguments),
            "dependency_path" => self.dependency_path(arguments),
            "what_if" => self.what_if(arguments),
            _ => return CallOutcome::UnknownTool,
        };
        CallOutcome::Done(result)
    }

    fn analyze_change(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["base", "head", "mode"])?;
        let report = self.report(&args)?;
        let mut truncation = Vec::new();
        let r = report.as_ref();
        let risk_signals: Vec<Value> = r
            .risk
            .signals
            .iter()
            .filter(|s| s.evaluated && s.points > 0)
            .map(|s| {
                json!({ "id": s.id, "points": s.points, "value": s.value, "definition": s.definition,
                        "evidence": s.evidence, "evidence_truncated": s.evidence_truncated })
            })
            .collect();
        let not_evaluated: Vec<Value> =
            r.risk.signals.iter().filter(|s| !s.evaluated).map(|s| json!({ "id": s.id, "note": s.note })).collect();
        let changed: Vec<Value> = r
            .changed_symbols
            .iter()
            .take(SUMMARY_CHANGED)
            .map(|s| {
                json!({ "id": s.id, "change": s.change, "kind": s.kind, "location": location(&s.file, s.span),
                        "previous_id": s.previous_id, "probable_move": s.probable_move, "is_test": s.is_test, "coverage": s.coverage })
            })
            .collect();
        note_cap(&mut truncation, "changed_symbols", r.changed_symbols.len(), SUMMARY_CHANGED, "impacted_symbols");
        let impacted: Vec<Value> = r.impacted_symbols.iter().take(SUMMARY_IMPACTED).map(impacted_json).collect();
        note_cap(&mut truncation, "impacted_symbols", r.impacted_symbols.len(), SUMMARY_IMPACTED, "impacted_symbols");
        let tests: Vec<Value> = r.tests.iter().take(SUMMARY_TESTS).map(test_json).collect();
        note_cap(&mut truncation, "tests", r.tests.len(), SUMMARY_TESTS, "tests_for_change");
        let new_violations: Vec<&ripplepath_engine::architecture::Violation> =
            r.architecture.violations.iter().filter(|v| v.status == DeltaStatus::New).collect();
        note_cap(&mut truncation, "new_violations", new_violations.len(), SUMMARY_VIOLATIONS, "architecture_status");
        note_cap(&mut truncation, "uncertainty", r.uncertainty.len(), SUMMARY_UNCERTAINTY, "");
        let failing_gates: Vec<_> = r
            .policy
            .gates
            .iter()
            .filter(|g| {
                matches!(
                    g.status,
                    ripplepath_engine::policy::GateStatus::Fail | ripplepath_engine::policy::GateStatus::Warn
                )
            })
            .collect();
        Ok(json!({
            "base": r.base,
            "head": r.head,
            "summary": r.summary,
            "risk": {
                "score": r.risk.score,
                "level": r.risk.level,
                "model_version": r.risk.model_version,
                "interpretation": r.risk.interpretation,
                "contributing_signals": risk_signals,
                "not_evaluated": not_evaluated,
            },
            "policy": { "result": r.policy.result, "triggered_gates": failing_gates, "note": r.policy.note },
            "changed_symbols": changed,
            "impacted_symbols": impacted,
            "test_selection": selection_json(r, SUMMARY_TESTS, &mut truncation),
            "tests": tests,
            "architecture": {
                "configured": r.architecture.configured,
                "summary": r.architecture.summary,
                "new_violations": new_violations.into_iter().take(SUMMARY_VIOLATIONS).collect::<Vec<_>>(),
            },
            "api_surface": { "breaking": r.api_surface.breaking, "added": r.api_surface.added },
            "uncertainty": r.uncertainty.iter().take(SUMMARY_UNCERTAINTY).collect::<Vec<_>>(),
            "evidence": r.evidence,
            "truncation": truncation,
            "note": NOT_A_PROOF,
        }))
    }

    fn impacted_symbols(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["base", "head", "limit"])?;
        let limit = args.int("limit", 1, 500, 50)?;
        let report = self.report(&args)?;
        let r = report.as_ref();
        let mut truncation = Vec::new();
        note_cap(&mut truncation, "impacted_symbols", r.impacted_symbols.len(), limit, "");
        Ok(json!({
            "base": r.base,
            "head": r.head,
            "changed_symbols": r.changed_symbols.len(),
            "total": r.impacted_symbols.len(),
            "impact_truncated": r.summary.impact_truncated,
            "max_depth": r.summary.max_depth,
            "impacted_symbols": r.impacted_symbols.iter().take(limit).map(impacted_json).collect::<Vec<_>>(),
            "uncertainty": r.uncertainty.iter().take(SUMMARY_UNCERTAINTY).collect::<Vec<_>>(),
            "truncation": truncation,
            "note": NOT_A_PROOF,
        }))
    }

    fn tests_for_change(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["base", "head", "mode"])?;
        let report = self.report(&args)?;
        let r = report.as_ref();
        let mut truncation = Vec::new();
        note_cap(&mut truncation, "tests", r.tests.len(), LIST_TESTS, "");
        Ok(json!({
            "base": r.base,
            "head": r.head,
            "test_selection": selection_json(r, LIST_TESTS, &mut truncation),
            "tests": r.tests.iter().take(LIST_TESTS).map(test_json).collect::<Vec<_>>(),
            "evidence": r.evidence,
            "truncation": truncation,
        }))
    }

    fn architecture_status(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["rev", "base", "head"])?;
        if args.has("base") || args.has("head") {
            if args.has("rev") {
                return Err("pass either `rev` (state of one revision) or `base`/`head` (a change), not both".into());
            }
            let report = self.report(&args)?;
            let a = &report.architecture;
            let new: Vec<_> = a.violations.iter().filter(|v| v.status == DeltaStatus::New).collect();
            let mut truncation = Vec::new();
            note_cap(&mut truncation, "new_violations", new.len(), LIST_VIOLATIONS, "");
            if a.violations_truncated {
                truncation.push(format!(
                    "the engine listed at most {} violations",
                    ripplepath_engine::architecture::MAX_LISTED_VIOLATIONS
                ));
            }
            return Ok(json!({
                "base": report.base,
                "head": report.head,
                "configured": a.configured,
                "config": { "source": report.config.source, "revision": report.config.revision, "errors": report.config.errors, "head_change": report.config.head_change },
                "rules": a.rules,
                "summary": a.summary,
                "new_violations": new.into_iter().take(LIST_VIOLATIONS).collect::<Vec<_>>(),
                "new_cycles": a.cycles.iter().filter(|c| c.status == DeltaStatus::New).collect::<Vec<_>>(),
                "truncation": truncation,
            }));
        }
        let rev = args.revision("rev", &self.config.default_head)?;
        let check = check_architecture(&self.config.repo, &rev, &self.limits).map_err(|e| e.to_string())?;
        let mut truncation = Vec::new();
        note_cap(&mut truncation, "violations", check.violations.len(), LIST_VIOLATIONS, "");
        Ok(json!({
            "revision": check.revision,
            "config_found": check.config_found,
            "rules": check.rules,
            "layers": check.layers,
            "violations": check.violations.iter().take(LIST_VIOLATIONS).collect::<Vec<_>>(),
            "violations_total": check.violations.len(),
            "cycles": check.cycles,
            "truncation": truncation,
        }))
    }

    fn find_symbols(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["query", "rev", "limit"])?;
        let query = args.symbol("query")?;
        let limit = args.int("limit", 1, 100, 20)?;
        let view = self.view(&args.revision("rev", &self.config.default_head)?)?;
        let (found, total) = view.find_symbols(&query, limit);
        let mut truncation = Vec::new();
        note_cap(&mut truncation, "symbols", total, limit, "");
        Ok(json!({
            "revision": view.revision,
            "total": total,
            "symbols": found.iter().map(|s| symbol_brief(&view, s)).collect::<Vec<_>>(),
            "truncation": truncation,
        }))
    }

    fn symbol_info(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["symbol", "rev"])?;
        let id = SymbolId::new(args.symbol("symbol")?);
        let view = self.view(&args.revision("rev", &self.config.default_head)?)?;
        let symbol = existing(&view, &id)?;
        let incoming: Vec<_> = view.graph().incoming(&id).collect();
        let outgoing: Vec<_> = view.graph().outgoing(&id).collect();
        let what_if = view.what_if(&id, ImpactOptions::default());
        let tests: Vec<Value> =
            what_if.as_ref().map(|w| w.tests.iter().take(SUMMARY_TESTS).map(test_json).collect()).unwrap_or_default();
        let tests_total = what_if.as_ref().map_or(0, |w| w.tests.len());
        let mut truncation = Vec::new();
        note_cap(&mut truncation, "incoming", incoming.len(), LIST_EDGES, "");
        note_cap(&mut truncation, "outgoing", outgoing.len(), LIST_EDGES, "");
        note_cap(&mut truncation, "tests", tests_total, SUMMARY_TESTS, "what_if");
        Ok(json!({
            "revision": view.revision,
            "symbol": {
                "id": symbol.id, "kind": symbol.kind, "name": symbol.name, "language": symbol.language,
                "module": symbol.module, "location": location(&symbol.file, symbol.span), "file": symbol.file,
                "span": symbol.span, "visibility": symbol.visibility, "is_test": symbol.is_test, "parent": symbol.parent,
            },
            "layer": view.layer_of(symbol),
            "layers_configured": view.has_layers(),
            "generated": view.is_generated(&symbol.file),
            "coverage": view.coverage_status(symbol),
            "incoming_total": incoming.len(),
            "incoming": incoming.into_iter().take(LIST_EDGES).collect::<Vec<_>>(),
            "outgoing_total": outgoing.len(),
            "outgoing": outgoing.into_iter().take(LIST_EDGES).collect::<Vec<_>>(),
            "architecture_violations": view.violations_involving(&id),
            "tests_total": tests_total,
            "tests": tests,
            "uncertainty": view.unresolved_from(&id),
            "config_error": view.config_error,
            "truncation": truncation,
        }))
    }

    fn dependency_path(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["from", "to", "rev", "max_depth"])?;
        let from = SymbolId::new(args.symbol("from")?);
        let to = SymbolId::new(args.symbol("to")?);
        let max_depth = args.int("max_depth", 1, 16, 8)?;
        let view = self.view(&args.revision("rev", &self.config.default_head)?)?;
        existing(&view, &from)?;
        existing(&view, &to)?;
        let options = PathOptions { max_depth: u32::try_from(max_depth).unwrap_or(8), ..PathOptions::default() };
        let forward = view.dependency_path(&from, &to, options);
        let (direction, search) = if forward.path.is_some() {
            ("forward", forward)
        } else {
            let reverse = view.dependency_path(&to, &from, options);
            if reverse.path.is_some() { ("reverse", reverse) } else { ("none", forward) }
        };
        let mut uncertainty = view.unresolved_from(&from);
        uncertainty.extend(view.unresolved_from(&to));
        let note = match direction {
            "forward" => format!("{from} depends on {to} through these hops (each edge is `from` depends on `to`)."),
            "reverse" => format!("No chain from {from} to {to}; the reverse chain shows {to} depends on {from}."),
            _ => format!(
                "No dependency chain within {max_depth} hops in either direction{}. Absence is not proof of independence: {NOT_A_PROOF}",
                if search.truncated { " (search hit its node limit: inconclusive)" } else { "" }
            ),
        };
        Ok(json!({
            "revision": view.revision,
            "from": from,
            "to": to,
            "found": search.path.is_some(),
            "direction": direction,
            "hops": search.path.as_ref().map(|p| p.hops.clone()).unwrap_or_default(),
            "weakest_evidence": search.path.as_ref().map(|p| p.weakest_evidence),
            "search_truncated": search.truncated,
            "max_depth": max_depth,
            "uncertainty": uncertainty,
            "note": note,
        }))
    }

    fn what_if(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let args = Args::new(args, &["symbol", "rev", "max_depth", "limit"])?;
        let id = SymbolId::new(args.symbol("symbol")?);
        let max_depth = args.int("max_depth", 1, 12, 6)?;
        let limit = args.int("limit", 1, 500, 50)?;
        let view = self.view(&args.revision("rev", &self.config.default_head)?)?;
        let symbol = existing(&view, &id)?;
        let options = ImpactOptions { max_depth: u32::try_from(max_depth).unwrap_or(6), ..ImpactOptions::default() };
        let Some(result) = view.what_if(&id, options) else {
            return Err(format!("symbol {id} not found"));
        };
        let impacted_tests = result.impacted.iter().filter(|i| i.is_test).count();
        let mut truncation = Vec::new();
        note_cap(&mut truncation, "impacted_symbols", result.impacted.len(), limit, "");
        note_cap(&mut truncation, "tests", result.tests.len(), limit, "");
        note_cap(&mut truncation, "uncertainty", result.uncertainty.len(), SUMMARY_UNCERTAINTY, "");
        Ok(json!({
            "revision": result.revision,
            "root": { "id": symbol.id, "kind": symbol.kind, "location": location(&symbol.file, symbol.span), "layer": view.layer_of(symbol), "is_test": symbol.is_test },
            "summary": {
                "impacted": result.impacted.len(),
                "impacted_tests": impacted_tests,
                "tests_recommended": result.tests.len(),
                "modules": result.modules,
                "layers": result.layers,
                "impact_truncated": result.truncated,
                "max_depth": result.max_depth,
            },
            "impacted_symbols": result.impacted.iter().take(limit).map(impacted_json).collect::<Vec<_>>(),
            "tests": result.tests.iter().take(limit).map(test_json).collect::<Vec<_>>(),
            "uncertainty": result.uncertainty.iter().take(SUMMARY_UNCERTAINTY).collect::<Vec<_>>(),
            "evidence": result.evidence,
            "truncation": truncation,
            "note": format!(
                "Hypothetical: computed on {}'s graph as if the body of {id} were modified. Changing its signature or deleting it additionally breaks direct (depth 1) dependents bound to the current signature. {NOT_A_PROOF}",
                result.revision.spec
            ),
        }))
    }

    /// The analysis report for (base, head, mode), from the in-memory cache when possible.
    fn report(&mut self, args: &Args) -> Result<Rc<AnalysisReport>, String> {
        let base = args.revision("base", &self.config.default_base)?;
        let head = args.revision("head", &self.config.default_head)?;
        let mode = args.mode()?;
        let resolve = |spec: &str| resolve_revision(&self.config.repo, spec).map_err(|e| e.to_string());
        // Keyed by spec and resolved object: a branch that moved must not be answered from cache.
        let key = format!("{base}\0{}\0{head}\0{}\0{mode:?}", ident(&resolve(&base)?), ident(&resolve(&head)?));
        if let Some((_, report)) = self.reports.iter().find(|(k, _)| *k == key) {
            return Ok(Rc::clone(report));
        }
        let mut options = AnalyzeOptions::new(&self.config.repo, base, head);
        options.db.clone_from(&self.config.db);
        options.mode = mode;
        let report = Rc::new(analyze(&options).map_err(|e| e.to_string())?);
        if self.reports.len() >= CACHED_REPORTS {
            self.reports.pop_front();
        }
        self.reports.push_back((key, Rc::clone(&report)));
        Ok(report)
    }

    /// The indexed revision `spec`, from the in-memory cache when possible.
    fn view(&mut self, spec: &str) -> Result<Rc<RevisionView>, String> {
        let resolved = resolve_revision(&self.config.repo, spec).map_err(|e| e.to_string())?;
        let key = format!("{spec}\0{}", ident(&resolved));
        if let Some((_, view)) = self.views.iter().find(|(k, _)| *k == key) {
            return Ok(Rc::clone(view));
        }
        // A fresh cache per load keeps memory bounded by the cached views; with a database, facts
        // persist there and re-loading is cheap.
        let mut cache = fact_cache(self.config.db.as_deref()).map_err(|e| e.to_string())?;
        let view =
            Rc::new(load_revision(&self.config.repo, spec, &mut cache, &self.limits).map_err(|e| e.to_string())?);
        if self.views.len() >= CACHED_VIEWS {
            self.views.pop_front();
        }
        self.views.push_back((key, Rc::clone(&view)));
        Ok(view)
    }
}

fn ident(revision: &RevisionInfo) -> String {
    format!("{}:{}", revision.commit.as_deref().unwrap_or("-"), revision.tree)
}

fn location(file: &str, span: ripplepath_core::Span) -> String {
    if span.start_line == span.end_line {
        format!("{file}:{}", span.start_line)
    } else {
        format!("{file}:{}-{}", span.start_line, span.end_line)
    }
}

fn note_cap(notes: &mut Vec<String>, field: &str, total: usize, shown: usize, more: &str) {
    if total > shown {
        let hint = if more.is_empty() { String::new() } else { format!("; call {more} for more") };
        notes.push(format!("{field}: showing {shown} of {total}{hint}"));
    }
}

fn impacted_json(s: &ripplepath_engine::ImpactedSymbolReport) -> Value {
    json!({
        "id": s.id, "kind": s.kind, "location": location(&s.file, s.span), "module": s.module, "is_test": s.is_test,
        "depth": s.depth, "root": s.root, "weakest_evidence": s.weakest_evidence, "graph": s.graph,
        "coverage": s.coverage, "path": s.path,
    })
}

fn test_json(t: &ripplepath_engine::TestRecommendation) -> Value {
    json!({
        "id": t.id, "file": t.file, "reason": t.reason, "tier": t.tier, "depth": t.depth, "root": t.root,
        "weakest_evidence": t.weakest_evidence, "coverage_observed": t.coverage_observed, "history": t.history,
        "path": t.path,
    })
}

fn selection_json(r: &AnalysisReport, cap: usize, truncation: &mut Vec<String>) -> Value {
    let s = &r.test_selection;
    note_cap(truncation, "test_selection.ordered", s.ordered.len(), cap, "");
    json!({
        "mode": s.mode, "decision": s.decision, "fallback_reasons": s.fallback_reasons,
        "ordered": s.ordered.iter().take(cap).collect::<Vec<_>>(),
        "selected_units": s.selected_units, "total_units": s.total_units,
        "selected_runtime_ms": s.selected_runtime_ms, "full_runtime_ms": s.full_runtime_ms, "notes": s.notes,
    })
}

fn symbol_brief(view: &RevisionView, s: &Symbol) -> Value {
    json!({ "id": s.id, "kind": s.kind, "name": s.name, "location": location(&s.file, s.span), "is_test": s.is_test, "layer": view.layer_of(s) })
}

/// Looks up `id`; when absent, the error lists close matches so the caller can retry with an exact id.
fn existing<'a>(view: &'a RevisionView, id: &SymbolId) -> Result<&'a Symbol, String> {
    if let Some(symbol) = view.symbol(id) {
        return Ok(symbol);
    }
    // Try the whole id, then its last segment without parameters (`Foo#bar(Baz)` → `bar`), then
    // the members of its owner (`Foo#barr()` → everything in `Foo`).
    let raw = id.as_str();
    let tail = raw.rsplit(['#', '.', ':', '/']).next().unwrap_or(raw);
    let tail = tail.split('(').next().unwrap_or(tail);
    let owner = raw.split_once('#').map_or("", |(owner, _)| owner);
    let found = [raw, tail, owner]
        .into_iter()
        .filter(|q| !q.is_empty())
        .map(|q| view.find_symbols(q, SUGGESTIONS).0)
        .find(|found| !found.is_empty())
        .unwrap_or_default();
    let suggestions: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
    Err(if suggestions.is_empty() {
        format!("symbol {raw:?} not found in {}; use find_symbols to look up exact ids", view.revision.spec)
    } else {
        format!("symbol {raw:?} not found in {}; similar ids: {}", view.revision.spec, suggestions.join(", "))
    })
}

/// Validated tool arguments. Unknown keys are refused rather than ignored: a misspelt `limit`
/// silently falling back to the default would give the caller an answer to a different question.
struct Args<'a> {
    map: &'a Map<String, Value>,
}

impl<'a> Args<'a> {
    fn new(map: &'a Map<String, Value>, allowed: &[&str]) -> Result<Self, String> {
        let unknown: Vec<&str> = map.keys().map(String::as_str).filter(|k| !allowed.contains(k)).collect();
        if !unknown.is_empty() {
            return Err(format!("unknown argument(s): {}; allowed: {}", unknown.join(", "), allowed.join(", ")));
        }
        Ok(Self { map })
    }

    fn has(&self, key: &str) -> bool {
        self.map.get(key).is_some_and(|v| !v.is_null())
    }

    fn string(&self, key: &str) -> Result<Option<&'a str>, String> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(format!("`{key}` must be a string")),
        }
    }

    /// Revision specs go to gix's rev-parse, never a shell, so this is not about injection; it keeps
    /// error messages sane and rejects input no legitimate revspec contains (same rule as the HTTP API).
    fn revision(&self, key: &str, default: &str) -> Result<String, String> {
        let raw = self.string(key)?.unwrap_or(default);
        let trimmed = raw.trim_matches(' ');
        if trimmed.is_empty() || trimmed.len() > MAX_REVISION_LEN || trimmed.chars().any(char::is_control) {
            return Err(format!("`{key}` must be 1-{MAX_REVISION_LEN} printable characters"));
        }
        Ok(trimmed.to_owned())
    }

    fn symbol(&self, key: &str) -> Result<String, String> {
        let raw = self.string(key)?.ok_or_else(|| format!("`{key}` is required"))?;
        if raw.is_empty() || raw.len() > MAX_SYMBOL_LEN || raw.chars().any(char::is_control) {
            return Err(format!("`{key}` must be 1-{MAX_SYMBOL_LEN} printable characters"));
        }
        Ok(raw.to_owned())
    }

    fn int(&self, key: &str, min: u64, max: u64, default: u64) -> Result<usize, String> {
        let value = match self.map.get(key) {
            None | Some(Value::Null) => default,
            Some(v) => v.as_u64().ok_or_else(|| format!("`{key}` must be an integer between {min} and {max}"))?,
        };
        if !(min..=max).contains(&value) {
            return Err(format!("`{key}` must be an integer between {min} and {max}"));
        }
        usize::try_from(value).map_err(|_| format!("`{key}` is out of range"))
    }

    fn mode(&self) -> Result<Option<SelectionMode>, String> {
        match self.string("mode")? {
            None => Ok(None),
            Some("conservative") => Ok(Some(SelectionMode::Conservative)),
            Some("balanced") => Ok(Some(SelectionMode::Balanced)),
            Some("fast_feedback") => Ok(Some(SelectionMode::FastFeedback)),
            Some(other) => Err(format!("`mode` must be conservative, balanced or fast_feedback, not {other:?}")),
        }
    }
}
