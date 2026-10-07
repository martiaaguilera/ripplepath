use serde::{Deserialize, Serialize};

use crate::SymbolId;

/// Relation kinds. An edge `from → to` always means "`from` depends on `to`".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EdgeKind {
    Contains,
    Imports,
    Extends,
    Implements,
    Overrides,
    Calls,
    Instantiates,
    References,
    Tests,
}

impl EdgeKind {
    /// Containment is structure, not dependency: editing one method must not make every
    /// sibling method's callers "impacted" by way of the enclosing class.
    pub fn propagates_impact(self) -> bool {
        !matches!(self, Self::Contains)
    }
}

/// How an edge is known. Ordinal labels, deliberately not probabilities: nothing here has been
/// calibrated against outcomes, so presenting a percentage would be fabricated precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Evidence {
    /// A name resolved to exactly one declaration through language scoping rules.
    ResolvedExact,
    /// Measured by a coverage tool while a test ran.
    CoverageObserved,
    /// Statically plausible but ambiguous (overloads, inferred receiver type, dispatch).
    StaticInferred,
    /// Files historically changed together.
    HistoryCochange,
    /// Inferred from names only, e.g. `FooTest` ↔ `Foo`.
    NamingHeuristic,
}

impl Evidence {
    /// Higher is stronger. Used to pick the most defensible explanation among equal-length paths.
    pub fn strength(self) -> u8 {
        match self {
            Self::ResolvedExact => 5,
            Self::CoverageObserved => 4,
            Self::StaticInferred => 3,
            Self::HistoryCochange => 2,
            Self::NamingHeuristic => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: EdgeKind,
    pub evidence: Evidence,
    pub file: String,
    pub line: u32,
    /// Identifier of the extractor rule that produced the edge, e.g. `java.call.receiver-typed`.
    pub rule: String,
}
