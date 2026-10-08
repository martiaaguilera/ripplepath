//! Public API surface changes (docs/RISK_MODEL.md, `public_api_changed`). Pure.
//!
//! A symbol is *API* when it is not test code and it and every enclosing declaration up to the
//! file are `public` or `protected`, the outermost one `public`:
//! - Java: public/protected members of public (or public/protected nested) types; interface members
//!   are implicitly public.
//! - TypeScript/JavaScript: exported module-level declarations and their non-private members
//!   (the frontend records "exported" as `public`).

use std::collections::BTreeSet;

use ripplepath_core::{Language, Symbol, SymbolId, SymbolKind, Visibility};
use ripplepath_graph::CodeGraph;
use serde::{Deserialize, Serialize};

use crate::report::{ChangeKind, ChangedSymbol};

const MAX_LISTED: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApiChangeKind {
    /// An API symbol of base no longer exists (only the outermost removed symbol is listed).
    Removed,
    /// An API method/constructor's parameter list changed.
    SignatureChanged,
    /// Still declared, but no longer API (visibility reduced or no longer exported).
    Narrowed,
    /// New API. Not breaking; listed for review.
    Added,
}

impl ApiChangeKind {
    pub fn is_breaking(self) -> bool {
        self != Self::Added
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiChange {
    pub kind: ApiChangeKind,
    /// Head id, or the base id for `REMOVED`.
    pub id: SymbolId,
    pub previous_id: Option<SymbolId>,
    pub symbol_kind: SymbolKind,
    pub language: Language,
    pub file: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiSurfaceReport {
    /// Removed + signature changed + narrowed.
    pub breaking: usize,
    pub added: usize,
    /// Sorted by (kind, id); at most 500.
    pub changes: Vec<ApiChange>,
    pub truncated: bool,
}

fn exposes(visibility: Visibility) -> bool {
    matches!(visibility, Visibility::Public | Visibility::Protected)
}

pub fn is_api(graph: &CodeGraph, symbol: &Symbol) -> bool {
    if matches!(symbol.kind, SymbolKind::File | SymbolKind::TestCase) {
        return false;
    }
    let mut current = symbol;
    loop {
        if current.is_test || !exposes(current.visibility) {
            return false;
        }
        match current.parent.as_ref().and_then(|p| graph.symbol(p)) {
            Some(parent) if parent.kind == SymbolKind::File => {
                // Test files mark their file symbol; their declarations are not API either.
                return !parent.is_test && current.visibility == Visibility::Public;
            }
            Some(parent) => current = parent,
            None => return current.visibility == Visibility::Public,
        }
    }
}

pub fn surface_changes(base: &CodeGraph, head: &CodeGraph, changed: &[ChangedSymbol]) -> ApiSurfaceReport {
    let deleted_api: BTreeSet<&SymbolId> = changed
        .iter()
        .filter(|c| c.change == ChangeKind::Deleted)
        .filter(|c| base.symbol(&c.id).is_some_and(|s| is_api(base, s)))
        .map(|c| &c.id)
        .collect();
    let mut changes = Vec::new();
    let mut push = |kind, symbol: &Symbol, previous_id: Option<SymbolId>| {
        changes.push(ApiChange {
            kind,
            id: symbol.id.clone(),
            previous_id,
            symbol_kind: symbol.kind,
            language: symbol.language,
            file: symbol.file.clone(),
            line: symbol.span.start_line,
        });
    };
    for change in changed {
        match change.change {
            ChangeKind::Deleted => {
                let Some(symbol) = base.symbol(&change.id) else { continue };
                // Removing a public class removes its members too; report the class once.
                let parent_removed = symbol.parent.as_ref().is_some_and(|p| deleted_api.contains(p));
                if deleted_api.contains(&change.id) && !parent_removed {
                    push(ApiChangeKind::Removed, symbol, None);
                }
            }
            ChangeKind::SignatureChanged => {
                let was_api = change.previous_id.as_ref().and_then(|p| base.symbol(p)).is_some_and(|s| is_api(base, s));
                if let (true, Some(symbol)) = (was_api, head.symbol(&change.id)) {
                    push(ApiChangeKind::SignatureChanged, symbol, change.previous_id.clone());
                }
            }
            ChangeKind::Modified => {
                if let (Some(before), Some(after)) = (base.symbol(&change.id), head.symbol(&change.id))
                    && is_api(base, before)
                    && !is_api(head, after)
                {
                    push(ApiChangeKind::Narrowed, after, None);
                }
            }
            ChangeKind::Added => {
                if let Some(symbol) = head.symbol(&change.id).filter(|s| is_api(head, s)) {
                    let parent_added = symbol
                        .parent
                        .as_ref()
                        .is_some_and(|p| changed.iter().any(|c| &c.id == p && c.change == ChangeKind::Added));
                    if !parent_added {
                        push(ApiChangeKind::Added, symbol, None);
                    }
                }
            }
        }
    }
    changes.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    let breaking = changes.iter().filter(|c| c.kind.is_breaking()).count();
    let added = changes.len() - breaking;
    let truncated = changes.len() > MAX_LISTED;
    changes.truncate(MAX_LISTED);
    ApiSurfaceReport { breaking, added, changes, truncated }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use ripplepath_core::{FingerprintBuilder, Span};

    use super::*;

    fn symbol(id: &str, kind: SymbolKind, parent: Option<&str>, visibility: Visibility) -> Symbol {
        Symbol {
            id: SymbolId::new(id),
            kind,
            name: id.to_owned(),
            language: Language::Java,
            module: "p".to_owned(),
            file: "p/A.java".to_owned(),
            span: Span { start_line: 1, end_line: 1 },
            parent: parent.map(SymbolId::new),
            visibility,
            is_test: false,
            fingerprint: FingerprintBuilder::new().finish(),
        }
    }

    #[test]
    fn api_requires_an_exposed_chain_with_a_public_outermost_type() {
        use SymbolKind::*;
        use Visibility::*;
        let graph = CodeGraph::new(
            vec![
                symbol("file:p/A.java", File, None, Public),
                symbol("A", Class, Some("file:p/A.java"), Public),
                symbol("A#m", Method, Some("A"), Public),
                symbol("A#p", Method, Some("A"), Protected),
                symbol("A#x", Method, Some("A"), Private),
                symbol("A#q", Method, Some("A"), Package),
                symbol("A.N", Class, Some("A"), Protected),
                symbol("A.N#m", Method, Some("A.N"), Public),
                symbol("B", Class, Some("file:p/A.java"), Package),
                symbol("B#m", Method, Some("B"), Public),
            ],
            Vec::new(),
        );
        let api = |id: &str| is_api(&graph, graph.symbol(&SymbolId::new(id)).unwrap());
        assert!(api("A") && api("A#m") && api("A#p") && api("A.N#m"));
        assert!(!api("A#x") && !api("A#q") && !api("B") && !api("B#m") && !api("file:p/A.java"));
    }
}
