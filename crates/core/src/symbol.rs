use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Fingerprint;

/// Stable identity of a program element.
///
/// Built from language, qualified name and (for callables) parameter types — never from line
/// numbers — so that unrelated edits do not change it. See docs/SPEC.md §2.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SymbolId(String);

impl SymbolId {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn file(path: &str) -> Self {
        Self(format!("file:{path}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Java,
    TypeScript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    File,
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Method,
    Constructor,
    Field,
}

impl SymbolKind {
    pub fn is_type(self) -> bool {
        matches!(self, Self::Class | Self::Interface | Self::Enum | Self::Record | Self::Annotation)
    }

    pub fn is_callable(self) -> bool {
        matches!(self, Self::Method | Self::Constructor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Protected,
    Package,
    Private,
}

/// 1-based, inclusive line range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub end_line: u32,
}

impl Span {
    pub fn contains_line(self, line: u32) -> bool {
        self.start_line <= line && line <= self.end_line
    }

    pub fn overlaps(self, start: u32, end: u32) -> bool {
        self.start_line <= end && start <= self.end_line
    }

    pub fn len(self) -> u32 {
        self.end_line + 1 - self.start_line
    }

    pub fn is_empty(self) -> bool {
        self.end_line < self.start_line
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub kind: SymbolKind,
    /// Short display name, e.g. `charge(Money,String)` or `BillingService`.
    pub name: String,
    pub language: Language,
    /// Grouping unit used for clustering and architecture layers: Java package, TS directory.
    pub module: String,
    pub file: String,
    pub span: Span,
    pub parent: Option<SymbolId>,
    pub visibility: Visibility,
    pub is_test: bool,
    pub fingerprint: Fingerprint,
}
