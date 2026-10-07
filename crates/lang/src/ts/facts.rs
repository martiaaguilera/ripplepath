//! Per-file facts extracted from TypeScript/JavaScript syntax. Pure function of (path, content).

use ripplepath_core::{Fingerprint, Span, SymbolKind, Visibility};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TsFile {
    pub path: String,
    /// `*.test.*`, `*.spec.*` or under `__tests__/`.
    pub is_test_file: bool,
    pub imports: Vec<Import>,
    pub reexports: Vec<ReExport>,
    /// `export { a as b }` and `export default a` for local names.
    pub local_exports: Vec<LocalExport>,
    pub decls: Vec<Decl>,
    pub tests: Vec<TestCase>,
    /// Code at module top level that is neither a declaration nor a test case.
    pub module_body: Body,
    pub header_fingerprint: Fingerprint,
    pub line_count: u32,
    pub syntax_error_lines: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Imported {
    Named(String),
    Default,
    Namespace,
}

/// One local binding introduced by an import declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub local: String,
    pub source: String,
    pub imported: Imported,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReExport {
    /// `export { name as alias } from "source"`.
    Named { name: String, alias: String, source: String, line: u32 },
    /// `export * from "source"`.
    All { source: String, line: u32 },
    /// `export * as alias from "source"`.
    Namespace { alias: String, source: String, line: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalExport {
    pub local: String,
    /// `default` for a default export.
    pub exported: String,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decl {
    /// `default` for anonymous default exports.
    pub name: String,
    pub kind: SymbolKind,
    pub exported: bool,
    pub default_export: bool,
    pub span: Span,
    pub fingerprint: Fingerprint,
    pub type_params: Vec<String>,
    pub extends: Vec<TypeRef>,
    pub implements: Vec<TypeRef>,
    pub members: Vec<Member>,
    /// Declared type of a variable, or the return type of a function.
    pub ty: Option<TypeRef>,
    /// Type evident from a `new C()` initializer when no type is declared.
    pub inferred_ty: Option<TypeRef>,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// `constructor` for constructors.
    pub name: String,
    pub kind: SymbolKind,
    pub is_static: bool,
    pub visibility: Visibility,
    pub span: Span,
    pub fingerprint: Fingerprint,
    /// Field type, or method return type.
    pub ty: Option<TypeRef>,
    pub inferred_ty: Option<TypeRef>,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCase {
    /// Enclosing `describe` titles followed by the test title, joined with ` > `.
    pub title: String,
    pub span: Span,
    pub fingerprint: Fingerprint,
    pub body: Body,
}

/// A type as written, generics stripped: `Cart`, `ns.Cart`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypeRef {
    pub name: String,
    pub line: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    pub locals: Vec<Local>,
    pub refs: Vec<Ref>,
}

/// Function-scoped binding (parameters, `const`/`let`/`var`, catch and loop variables). Block
/// scopes are flattened, as in the Java frontend; see docs/LANGUAGE_SUPPORT.md.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Local {
    pub name: String,
    pub ty: Option<TypeRef>,
}

/// An expression reduced to what static resolution can follow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expr {
    Ident(String),
    This,
    Super,
    Member(Box<Expr>, String),
    /// The value returned by calling the inner expression.
    CallResult(Box<Expr>),
    /// `new X(...)`: an instance of X.
    New(Box<Expr>),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ref {
    Call {
        callee: Expr,
        line: u32,
    },
    New {
        ctor: Expr,
        line: u32,
    },
    /// A value read that is not the callee of a call: a function passed as a callback, a constant.
    Read {
        expr: Expr,
        line: u32,
    },
    Type(TypeRef),
}
