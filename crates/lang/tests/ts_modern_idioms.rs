#![allow(clippy::unwrap_used, clippy::expect_used)]

//! React/TypeScript idioms found while dogfooding on a Vite + React console
//! (docs/DOGFOODING_QUANTARUN.md). Minimal reproductions written for this suite.

use std::time::Duration;

use ripplepath_core::{Edge, EdgeKind, Evidence, SymbolId, SymbolKind};
use ripplepath_lang::LanguageGraph;
use ripplepath_lang::ts;

const BUDGET: Duration = Duration::from_secs(5);

const CLIENT: &str =
    "export function fetchJson(url: string) { return fetch(url) }\nexport function encode(id: string) { return id }\n";

fn endpoints(cancel_body: &str) -> String {
    format!(
        "import {{ fetchJson, encode }} from './client'\n\
         const base = {{ ping: () => fetchJson('/ping') }}\n\
         export const api = {{\n  \
           jobs: (signal?: AbortSignal) => fetchJson('/jobs'),\n  \
           cancel(id: string) {{ {cancel_body} }},\n  \
           'with-quotes': () => 1,\n  \
           encode,\n\
         }}\n\
         export const keys = {{ jobs: ['jobs'] as const, job: (id: string) => ['job', id] as const }} as const\n\
         export const merged = {{ ...base, extra: 1 }} satisfies Record<string, unknown>\n"
    )
}

const PAGE: &str = "import { api, keys, merged } from '../api/endpoints'\n\
                    export function JobsPage() {\n  \
                      api.cancel('x')\n  \
                      const k = keys.jobs\n  \
                      merged.ping()\n  \
                      return <div>{String(k)}</div>\n\
                    }\n";

/// A class with a `ping` member makes `merged.ping()` a plausible missing edge.
const PINGER: &str = "export class Pinger { ping() {} push() {} at() {} }\n";

fn graph(cancel_body: &str) -> LanguageGraph {
    let endpoints = endpoints(cancel_body);
    let files = [
        ("src/api/client.ts", CLIENT),
        ("src/api/endpoints.ts", endpoints.as_str()),
        ("src/pages/JobsPage.tsx", PAGE),
        ("src/pinger.ts", PINGER),
    ];
    let facts: Vec<_> = files.iter().map(|(p, s)| ts::extract(p, s, BUDGET).unwrap()).collect();
    ts::resolve(&facts.iter().collect::<Vec<_>>())
}

fn edge<'g>(g: &'g LanguageGraph, from: &str, to: &str, kind: EdgeKind) -> Option<&'g Edge> {
    g.edges.iter().find(|e| e.from == SymbolId::new(from) && e.to == SymbolId::new(to) && e.kind == kind)
}

const PAGE_FN: &str = "ts:src/pages/JobsPage.tsx#JobsPage";

#[test]
fn object_literal_properties_are_members_of_the_variable() {
    let g = graph("return encode(id)");
    let kind = |id: &str| g.symbols.iter().find(|s| s.id == SymbolId::new(id)).map(|s| s.kind);
    assert_eq!(kind("ts:src/api/endpoints.ts#api.jobs"), Some(SymbolKind::Method));
    assert_eq!(kind("ts:src/api/endpoints.ts#api.cancel"), Some(SymbolKind::Method));
    assert_eq!(kind("ts:src/api/endpoints.ts#api.with-quotes"), Some(SymbolKind::Method));
    assert_eq!(kind("ts:src/api/endpoints.ts#api.encode"), Some(SymbolKind::Field));
    assert_eq!(kind("ts:src/api/endpoints.ts#keys.jobs"), Some(SymbolKind::Field), "through `as const`");

    let call = edge(&g, PAGE_FN, "ts:src/api/endpoints.ts#api.cancel", EdgeKind::Calls).expect("member call");
    assert_eq!(call.evidence, Evidence::ResolvedExact);
    assert!(edge(&g, PAGE_FN, "ts:src/api/endpoints.ts#keys.jobs", EdgeKind::References).is_some());
    // Members carry their own dependencies; shorthand properties depend on the bound value.
    assert!(edge(&g, "ts:src/api/endpoints.ts#api.cancel", "ts:src/api/client.ts#encode", EdgeKind::Calls).is_some());
    assert!(edge(&g, "ts:src/api/endpoints.ts#api.jobs", "ts:src/api/client.ts#fetchJson", EdgeKind::Calls).is_some());
    assert!(
        edge(&g, "ts:src/api/endpoints.ts#api.encode", "ts:src/api/client.ts#encode", EdgeKind::References).is_some()
    );
}

#[test]
fn editing_one_property_does_not_modify_the_variable_or_its_siblings() {
    let before = graph("return encode(id)");
    let after = graph("return encode(id + '!')");
    let fp = |g: &LanguageGraph, id: &str| g.symbols.iter().find(|s| s.id == SymbolId::new(id)).map(|s| s.fingerprint);
    let changed = "ts:src/api/endpoints.ts#api.cancel";
    assert_ne!(fp(&before, changed), fp(&after, changed));
    for unchanged in ["ts:src/api/endpoints.ts#api", "ts:src/api/endpoints.ts#api.jobs"] {
        assert_eq!(fp(&before, unchanged), fp(&after, unchanged), "{unchanged}");
    }
}

#[test]
fn a_property_that_may_come_from_a_spread_is_unresolved_not_guessed() {
    let g = graph("return 1");
    assert!(g.edges.iter().all(|e| !e.to.as_str().ends_with("#base.ping") || e.from != SymbolId::new(PAGE_FN)));
    let details: Vec<&str> = g.unresolved.iter().map(|u| u.detail.as_str()).collect();
    assert_eq!(details, vec!["call ping"]);
}

#[test]
fn array_and_primitive_annotations_are_external_but_any_stays_unknown() {
    let src = "export function f(name: string) {\n  const rows: string[] = []\n  rows.push(name)\n  const pair: [number, number] = [1, 2]\n  pair.at(0)\n  name.at(0)\n  const legacy: any = rows\n  legacy.push(1)\n}\n";
    let files = [("src/f.ts", src), ("src/pinger.ts", PINGER)];
    let facts: Vec<_> = files.iter().map(|(p, s)| ts::extract(p, s, BUDGET).unwrap()).collect();
    let g = ts::resolve(&facts.iter().collect::<Vec<_>>());
    let found: Vec<(u32, &str)> = g.unresolved.iter().map(|u| (u.line, u.detail.as_str())).collect();
    assert_eq!(found, vec![(8, "call push")]);
}
