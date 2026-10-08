//! Layer rules, layer cycles and module coupling, evaluated per revision and compared base → head.
//! Pure: graphs and configuration in, findings out. docs/ARCHITECTURE_RULES.md is the contract.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ripplepath_core::{Edge, EdgeKind, Evidence, SymbolId};
use ripplepath_graph::CodeGraph;
use serde::{Deserialize, Serialize};

use crate::config::{ArchitectureConfig, CycleMode, PathMatcher, RuleKind};

/// Listed violations per report; counts in the summary are always complete.
pub const MAX_LISTED_VIOLATIONS: usize = 500;
/// Listed module pairs whose coupling changed.
pub const MAX_LISTED_COUPLING: usize = 100;

/// Edge kinds that constitute an architectural dependency.
///
/// Unlike impact propagation, IMPORTS counts here: a Java `import` of another layer's type is a
/// compile-time dependency even before (or without) any call, and it is what tools such as ArchUnit
/// check. CONTAINS is structure inside one file, and TESTS is evidence about test execution — some
/// of it measured at runtime — not a dependency written in the code.
pub fn is_architecture_dependency(kind: EdgeKind) -> bool {
    !matches!(kind, EdgeKind::Contains | EdgeKind::Tests)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeltaStatus {
    /// In head, not in base: introduced by this change.
    New,
    /// In both revisions.
    PreExisting,
    /// In base, not in head: removed by this change.
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleReport {
    pub index: usize,
    pub from: String,
    pub kind: RuleKind,
    pub layers: Vec<String>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerReport {
    pub name: String,
    pub patterns: Vec<String>,
    /// Symbols assigned to the layer in each revision.
    pub base_symbols: usize,
    pub head_symbols: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub status: DeltaStatus,
    /// Index of the first rule (in configuration order) the dependency breaks.
    pub rule: usize,
    pub description: String,
    pub from_layer: String,
    pub to_layer: String,
    /// The symbol-level dependency with its source location: the head edge, or the base edge for
    /// `REMOVED`.
    pub edge: Edge,
    /// For `PRE_EXISTING` findings whose base edge connected different symbol ids that the change
    /// classification identifies with these (signature change or probable move).
    pub base_edge: Option<Edge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerEdgeEvidence {
    pub from: String,
    pub to: String,
    /// Symbol-level dependencies behind this layer edge.
    pub edges: usize,
    /// The first of them in (from, to, kind) order.
    pub example: Edge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerCycle {
    pub status: DeltaStatus,
    /// Sorted names of the layers in one strongly connected component.
    pub layers: Vec<String>,
    /// Layer edges inside the component in the revision that has it (head, or base when removed).
    pub edges: Vec<LayerEdgeEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerDependency {
    pub from: String,
    pub to: String,
    pub base_edges: usize,
    pub head_edges: usize,
    /// Head depends `from → to` where base only depended `to → from`: the direction flipped.
    pub direction_reversed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleCoupling {
    pub from: String,
    pub to: String,
    pub base_edges: usize,
    pub head_edges: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureSummary {
    pub new_violations: usize,
    /// New violations whose edge is `RESOLVED_EXACT`; the rest rest on inferred edges.
    pub new_violations_exact: usize,
    pub pre_existing_violations: usize,
    pub removed_violations: usize,
    pub new_cycles: usize,
    pub pre_existing_cycles: usize,
    pub removed_cycles: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureReport {
    /// False when the configuration declares no layers; layer findings are then empty, module
    /// coupling is still reported.
    pub configured: bool,
    pub cycles_mode: CycleMode,
    pub rules: Vec<RuleReport>,
    /// In configuration order (first match wins).
    pub layers: Vec<LayerReport>,
    pub summary: ArchitectureSummary,
    /// Sorted by (status, from_layer, to_layer, edge); at most [`MAX_LISTED_VIOLATIONS`].
    pub violations: Vec<Violation>,
    pub violations_truncated: bool,
    /// Sorted by (status, layers).
    pub cycles: Vec<LayerCycle>,
    /// Every layer pair with a dependency in either revision, sorted by (from, to).
    pub layer_dependencies: Vec<LayerDependency>,
    /// Module pairs whose cross-module dependency count changed, largest change first.
    pub module_coupling: Vec<ModuleCoupling>,
    pub module_coupling_truncated: bool,
}

/// One rule-breaking dependency in one revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundViolation {
    pub rule: usize,
    pub from_layer: String,
    pub to_layer: String,
    pub edge: Edge,
}

/// Architecture facts of one revision.
#[derive(Clone, Debug, Default)]
pub struct LayerState {
    pub layer_symbols: BTreeMap<String, usize>,
    /// Keyed by (from, to, kind): symbol ids are line-free, so the key survives unrelated edits.
    pub violations: BTreeMap<(SymbolId, SymbolId, EdgeKind), FoundViolation>,
    /// (from layer, to layer) → (count, first edge).
    pub layer_edges: BTreeMap<(String, String), (usize, Edge)>,
    /// Strongly connected components with more than one layer; each sorted, list sorted.
    pub cycles: Vec<Vec<String>>,
    /// Cross-module dependency counts.
    pub module_edges: BTreeMap<(String, String), usize>,
}

/// Evaluates one revision. Symbols in `generated` paths take part in no layer and no module pair:
/// findings in generated code are not actionable by editing it.
pub fn evaluate(graph: &CodeGraph, config: &ArchitectureConfig, generated: &PathMatcher) -> LayerState {
    let mut state = LayerState::default();
    // `first match wins` is evaluated once per file, not once per symbol.
    let mut file_layers: HashMap<&str, Option<&str>> = HashMap::new();
    let mut symbol_layer: HashMap<&SymbolId, Option<&str>> = HashMap::new();
    for symbol in graph.symbols() {
        let file = symbol.file.as_str();
        let layer = *file_layers
            .entry(file)
            .or_insert_with(|| if generated.is_match(file) { None } else { config.layer_of(file) });
        if let Some(name) = layer {
            *state.layer_symbols.entry(name.to_owned()).or_default() += 1;
        }
        symbol_layer.insert(&symbol.id, layer);
    }

    for edge in graph.edges().iter().filter(|e| is_architecture_dependency(e.kind)) {
        let (Some(from), Some(to)) = (graph.symbol(&edge.from), graph.symbol(&edge.to)) else {
            continue;
        };
        if generated.is_match(&from.file) || generated.is_match(&to.file) {
            continue;
        }
        if from.module != to.module {
            *state.module_edges.entry((from.module.clone(), to.module.clone())).or_default() += 1;
        }
        let layer = |id: &SymbolId| symbol_layer.get(id).copied().flatten().map(str::to_owned);
        let (Some(from_layer), Some(to_layer)) = (layer(&edge.from), layer(&edge.to)) else {
            continue;
        };
        if from_layer == to_layer {
            continue;
        }
        state
            .layer_edges
            .entry((from_layer.clone(), to_layer.clone()))
            .and_modify(|(count, _)| *count += 1)
            .or_insert_with(|| (1, edge.clone()));
        if let Some(rule) = config.rules.iter().find(|r| r.from == from_layer && r.forbids(&to_layer)) {
            state.violations.insert(
                (edge.from.clone(), edge.to.clone(), edge.kind),
                FoundViolation { rule: rule.index, from_layer, to_layer, edge: edge.clone() },
            );
        }
    }

    if config.cycles != CycleMode::Off {
        state.cycles = layer_cycles(state.layer_edges.keys());
    }
    state
}

/// Tarjan's strongly connected components over the layer graph. Nodes and neighbours are visited
/// in sorted order, so the result does not depend on map iteration order.
pub fn layer_cycles<'a>(edges: impl IntoIterator<Item = &'a (String, String)>) -> Vec<Vec<String>> {
    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (from, to) in edges {
        adjacency.entry(from.as_str()).or_default().insert(to.as_str());
        adjacency.entry(to.as_str()).or_default();
    }
    struct Tarjan<'g> {
        adjacency: &'g BTreeMap<&'g str, BTreeSet<&'g str>>,
        index: BTreeMap<&'g str, usize>,
        low: BTreeMap<&'g str, usize>,
        stack: Vec<&'g str>,
        on_stack: BTreeSet<&'g str>,
        next: usize,
        components: Vec<Vec<String>>,
    }
    impl<'g> Tarjan<'g> {
        // Recursion depth is bounded by the number of layers (config::MAX_LAYERS).
        fn visit(&mut self, node: &'g str) {
            self.index.insert(node, self.next);
            self.low.insert(node, self.next);
            self.next += 1;
            self.stack.push(node);
            self.on_stack.insert(node);
            let neighbours: Vec<&'g str> =
                self.adjacency.get(node).map(|n| n.iter().copied().collect()).unwrap_or_default();
            for next in neighbours {
                if !self.index.contains_key(next) {
                    self.visit(next);
                    let low = self.low[next].min(self.low[node]);
                    self.low.insert(node, low);
                } else if self.on_stack.contains(next) {
                    let low = self.index[next].min(self.low[node]);
                    self.low.insert(node, low);
                }
            }
            if self.low[node] == self.index[node] {
                let mut component = Vec::new();
                while let Some(member) = self.stack.pop() {
                    self.on_stack.remove(member);
                    component.push(member.to_owned());
                    if member == node {
                        break;
                    }
                }
                if component.len() > 1 {
                    component.sort();
                    self.components.push(component);
                }
            }
        }
    }
    let mut tarjan = Tarjan {
        adjacency: &adjacency,
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        next: 0,
        components: Vec::new(),
    };
    for node in adjacency.keys() {
        if !tarjan.index.contains_key(node) {
            tarjan.visit(node);
        }
    }
    let mut components = tarjan.components;
    components.sort();
    components
}

fn cycle_edges(state: &LayerState, layers: &[String]) -> Vec<LayerEdgeEvidence> {
    let members: BTreeSet<&str> = layers.iter().map(String::as_str).collect();
    state
        .layer_edges
        .iter()
        .filter(|((from, to), _)| members.contains(from.as_str()) && members.contains(to.as_str()))
        .map(|((from, to), (count, example))| LayerEdgeEvidence {
            from: from.clone(),
            to: to.clone(),
            edges: *count,
            example: example.clone(),
        })
        .collect()
}

/// Compares the two revisions. `aliases` maps base symbol ids to the head ids the change
/// classification identifies them with (signature changes, probable moves), so that a violation
/// carried along by such a change is not reported as removed-and-new.
pub fn delta(
    config: &ArchitectureConfig,
    base: &LayerState,
    head: &LayerState,
    aliases: &BTreeMap<SymbolId, SymbolId>,
) -> ArchitectureReport {
    let canonical = |id: &SymbolId| aliases.get(id).cloned().unwrap_or_else(|| id.clone());
    let base_by_head_key: BTreeMap<(SymbolId, SymbolId, EdgeKind), &FoundViolation> =
        base.violations.iter().map(|((from, to, kind), v)| ((canonical(from), canonical(to), *kind), v)).collect();
    let describe = |rule: usize| config.rules.get(rule).map(|r| r.describe()).unwrap_or_default();

    let mut violations = Vec::new();
    let mut summary = ArchitectureSummary::default();
    for (key, found) in &head.violations {
        let base_match = base_by_head_key.get(key);
        let status = if base_match.is_some() { DeltaStatus::PreExisting } else { DeltaStatus::New };
        match status {
            DeltaStatus::New => {
                summary.new_violations += 1;
                if found.edge.evidence == Evidence::ResolvedExact {
                    summary.new_violations_exact += 1;
                }
            }
            _ => summary.pre_existing_violations += 1,
        }
        let base_edge = base_match.filter(|b| b.edge.from != found.edge.from || b.edge.to != found.edge.to);
        violations.push(Violation {
            status,
            rule: found.rule,
            description: describe(found.rule),
            from_layer: found.from_layer.clone(),
            to_layer: found.to_layer.clone(),
            edge: found.edge.clone(),
            base_edge: base_edge.map(|b| b.edge.clone()),
        });
    }
    for (key, found) in &base_by_head_key {
        if !head.violations.contains_key(key) {
            summary.removed_violations += 1;
            violations.push(Violation {
                status: DeltaStatus::Removed,
                rule: found.rule,
                description: describe(found.rule),
                from_layer: found.from_layer.clone(),
                to_layer: found.to_layer.clone(),
                edge: found.edge.clone(),
                base_edge: None,
            });
        }
    }
    violations.sort_by(|a, b| {
        (a.status, &a.from_layer, &a.to_layer, &a.edge).cmp(&(b.status, &b.from_layer, &b.to_layer, &b.edge))
    });
    let violations_truncated = violations.len() > MAX_LISTED_VIOLATIONS;
    violations.truncate(MAX_LISTED_VIOLATIONS);

    let base_cycles: BTreeSet<&Vec<String>> = base.cycles.iter().collect();
    let head_cycles: BTreeSet<&Vec<String>> = head.cycles.iter().collect();
    let mut cycles = Vec::new();
    for layers in &head_cycles {
        let status = if base_cycles.contains(layers) { DeltaStatus::PreExisting } else { DeltaStatus::New };
        match status {
            DeltaStatus::New => summary.new_cycles += 1,
            _ => summary.pre_existing_cycles += 1,
        }
        cycles.push(LayerCycle { status, layers: (*layers).clone(), edges: cycle_edges(head, layers) });
    }
    for layers in base_cycles.difference(&head_cycles) {
        summary.removed_cycles += 1;
        cycles.push(LayerCycle {
            status: DeltaStatus::Removed,
            layers: (*layers).clone(),
            edges: cycle_edges(base, layers),
        });
    }
    cycles.sort_by(|a, b| (a.status, &a.layers).cmp(&(b.status, &b.layers)));

    let pairs: BTreeSet<&(String, String)> = base.layer_edges.keys().chain(head.layer_edges.keys()).collect();
    let count = |state: &LayerState, from: &str, to: &str| {
        state.layer_edges.get(&(from.to_owned(), to.to_owned())).map_or(0, |(n, _)| *n)
    };
    let layer_dependencies = pairs
        .into_iter()
        .map(|(from, to)| {
            let (base_edges, head_edges) = (count(base, from, to), count(head, from, to));
            LayerDependency {
                from: from.clone(),
                to: to.clone(),
                base_edges,
                head_edges,
                direction_reversed: base_edges == 0 && head_edges > 0 && count(base, to, from) > 0,
            }
        })
        .collect();

    let module_pairs: BTreeSet<&(String, String)> = base.module_edges.keys().chain(head.module_edges.keys()).collect();
    let mut module_coupling: Vec<ModuleCoupling> = module_pairs
        .into_iter()
        .filter_map(|pair| {
            let base_edges = base.module_edges.get(pair).copied().unwrap_or(0);
            let head_edges = head.module_edges.get(pair).copied().unwrap_or(0);
            (base_edges != head_edges).then(|| ModuleCoupling {
                from: pair.0.clone(),
                to: pair.1.clone(),
                base_edges,
                head_edges,
            })
        })
        .collect();
    module_coupling.sort_by(|a, b| {
        let change = |m: &ModuleCoupling| m.base_edges.abs_diff(m.head_edges);
        (std::cmp::Reverse(change(a)), &a.from, &a.to).cmp(&(std::cmp::Reverse(change(b)), &b.from, &b.to))
    });
    let module_coupling_truncated = module_coupling.len() > MAX_LISTED_COUPLING;
    module_coupling.truncate(MAX_LISTED_COUPLING);

    ArchitectureReport {
        configured: !config.layers.is_empty(),
        cycles_mode: config.cycles,
        rules: rule_reports(config),
        layers: config
            .layers
            .iter()
            .map(|l| LayerReport {
                name: l.name.clone(),
                patterns: l.matcher.patterns().to_vec(),
                base_symbols: base.layer_symbols.get(&l.name).copied().unwrap_or(0),
                head_symbols: head.layer_symbols.get(&l.name).copied().unwrap_or(0),
            })
            .collect(),
        summary,
        violations,
        violations_truncated,
        cycles,
        layer_dependencies,
        module_coupling,
        module_coupling_truncated,
    }
}

pub fn rule_reports(config: &ArchitectureConfig) -> Vec<RuleReport> {
    config
        .rules
        .iter()
        .map(|r| RuleReport {
            index: r.index,
            from: r.from.clone(),
            kind: r.kind,
            layers: r.layers.clone(),
            description: r.describe(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use ripplepath_core::{FingerprintBuilder, Language, Span, Symbol, SymbolKind, Visibility};

    use super::*;
    use crate::config::parse;

    fn symbol(id: &str, file: &str, module: &str) -> Symbol {
        Symbol {
            id: SymbolId::new(id),
            kind: SymbolKind::Class,
            name: id.to_owned(),
            language: Language::Java,
            module: module.to_owned(),
            file: file.to_owned(),
            span: Span { start_line: 1, end_line: 2 },
            parent: None,
            visibility: Visibility::Public,
            is_test: false,
            fingerprint: FingerprintBuilder::new().finish(),
        }
    }

    fn edge(from: &str, to: &str, kind: EdgeKind, evidence: Evidence) -> Edge {
        Edge {
            from: SymbolId::new(from),
            to: SymbolId::new(to),
            kind,
            evidence,
            file: format!("{from}.java"),
            line: 3,
            rule: "test".to_owned(),
        }
    }

    const CONFIG: &str = r#"
version: 1
architecture:
  layers:
    - name: api
      match: ["api/**"]
    - name: domain
      match: ["domain/**"]
    - name: infra
      match: ["infra/**", "domain/legacy/**"]
  rules:
    - from: domain
      deny: [api, infra]
    - from: infra
      allow: [domain]
generated: ["**/gen/**"]
"#;

    fn graph(edges: Vec<Edge>) -> CodeGraph {
        let symbols = vec![
            symbol("A", "api/A.java", "api"),
            symbol("A2", "api/A2.java", "api"),
            symbol("D", "domain/D.java", "domain"),
            symbol("D2", "domain/D2.java", "domain"),
            symbol("L", "domain/legacy/L.java", "domain.legacy"),
            symbol("I", "infra/I.java", "infra"),
            symbol("G", "domain/gen/G.java", "domain.gen"),
            symbol("U", "other/U.java", "other"),
        ];
        CodeGraph::new(symbols, edges)
    }

    fn state(edges: Vec<Edge>) -> LayerState {
        let config = parse(CONFIG).unwrap();
        evaluate(&graph(edges), &config.architecture, &config.generated)
    }

    #[test]
    fn first_matching_layer_wins_and_generated_code_is_ignored() {
        let config = parse(CONFIG).unwrap();
        // domain/legacy matches `domain/**` first.
        assert_eq!(config.architecture.layer_of("domain/legacy/L.java"), Some("domain"));
        let s = state(vec![
            edge("D", "A", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("G", "A", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("D", "U", EdgeKind::Calls, Evidence::ResolvedExact),
        ]);
        assert_eq!(s.layer_symbols.get("domain"), Some(&3), "D, D2 and legacy L; generated G excluded");
        assert_eq!(s.violations.len(), 1, "generated and unlayered endpoints are never violations");
    }

    #[test]
    fn deny_and_allow_rules_and_excluded_kinds() {
        let s = state(vec![
            edge("D", "A", EdgeKind::Imports, Evidence::ResolvedExact),
            edge("D", "I", EdgeKind::References, Evidence::StaticInferred),
            edge("I", "A", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("I", "D", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("A", "D", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("D", "A2", EdgeKind::Contains, Evidence::ResolvedExact),
            edge("D", "A2", EdgeKind::Tests, Evidence::CoverageObserved),
        ]);
        let found: Vec<(&str, &str, usize)> =
            s.violations.values().map(|v| (v.edge.from.as_str(), v.edge.to.as_str(), v.rule)).collect();
        assert_eq!(found, vec![("D", "A", 0), ("D", "I", 0), ("I", "A", 1)]);
    }

    #[test]
    fn delta_classifies_new_removed_and_pre_existing() {
        let base = state(vec![
            edge("D", "A", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("D2", "A", EdgeKind::Calls, Evidence::ResolvedExact),
        ]);
        let head = state(vec![
            edge("D", "A", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("D", "I", EdgeKind::Calls, Evidence::StaticInferred),
        ]);
        let config = parse(CONFIG).unwrap();
        let report = delta(&config.architecture, &base, &head, &BTreeMap::new());
        let statuses: Vec<(DeltaStatus, &str, &str)> =
            report.violations.iter().map(|v| (v.status, v.edge.from.as_str(), v.edge.to.as_str())).collect();
        assert_eq!(
            statuses,
            vec![(DeltaStatus::New, "D", "I"), (DeltaStatus::PreExisting, "D", "A"), (DeltaStatus::Removed, "D2", "A")]
        );
        assert_eq!(report.summary.new_violations, 1);
        assert_eq!(report.summary.new_violations_exact, 0, "the new one rests on an inferred edge");
        assert_eq!(report.violations[0].description, "domain must not depend on api, infra");
    }

    #[test]
    fn aliases_keep_carried_violations_pre_existing() {
        let base = state(vec![edge("D2", "A", EdgeKind::Calls, Evidence::ResolvedExact)]);
        let head = state(vec![edge("D", "A", EdgeKind::Calls, Evidence::ResolvedExact)]);
        let config = parse(CONFIG).unwrap();
        let aliases = BTreeMap::from([(SymbolId::new("D2"), SymbolId::new("D"))]);
        let report = delta(&config.architecture, &base, &head, &aliases);
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].status, DeltaStatus::PreExisting);
        assert_eq!(report.violations[0].base_edge.as_ref().map(|e| e.from.as_str()), Some("D2"));
    }

    #[test]
    fn cycles_are_found_deterministically_and_compared() {
        let edges = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
        };
        let found = layer_cycles(&edges(&[("a", "b"), ("b", "c"), ("c", "a"), ("d", "e"), ("e", "d"), ("c", "d")]));
        assert_eq!(found, vec![vec!["a", "b", "c"], vec!["d", "e"]]);
        let reversed = layer_cycles(&edges(&[("e", "d"), ("d", "e"), ("c", "a"), ("c", "d"), ("b", "c"), ("a", "b")]));
        assert_eq!(found, reversed);
        assert!(layer_cycles(&edges(&[("a", "b"), ("b", "c")])).is_empty());

        let base = state(vec![edge("A", "D", EdgeKind::Calls, Evidence::ResolvedExact)]);
        let head = state(vec![
            edge("A", "D", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("D", "A", EdgeKind::Calls, Evidence::ResolvedExact),
        ]);
        let config = parse(CONFIG).unwrap();
        let report = delta(&config.architecture, &base, &head, &BTreeMap::new());
        assert_eq!(report.summary.new_cycles, 1);
        assert_eq!(report.cycles[0].layers, vec!["api", "domain"]);
        assert_eq!(report.cycles[0].edges.len(), 2);
        let reversed: Vec<&LayerDependency> =
            report.layer_dependencies.iter().filter(|d| d.direction_reversed).collect();
        assert_eq!(reversed.len(), 1);
        assert_eq!((reversed[0].from.as_str(), reversed[0].to.as_str()), ("domain", "api"));
    }

    #[test]
    fn module_coupling_reports_changed_pairs_only() {
        let base = state(vec![edge("A", "D", EdgeKind::Calls, Evidence::ResolvedExact)]);
        let head = state(vec![
            edge("A", "D", EdgeKind::Calls, Evidence::ResolvedExact),
            edge("A", "U", EdgeKind::Calls, Evidence::ResolvedExact),
        ]);
        let config = parse(CONFIG).unwrap();
        let report = delta(&config.architecture, &base, &head, &BTreeMap::new());
        assert_eq!(
            report.module_coupling,
            vec![ModuleCoupling { from: "api".into(), to: "other".into(), base_edges: 0, head_edges: 1 }]
        );
    }
}
