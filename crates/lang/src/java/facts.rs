//! Per-file facts extracted from Java syntax, before any cross-file resolution.
//!
//! Facts depend only on (path, content, extractor version), which is what makes them cacheable by
//! blob hash: resolution, which depends on *other* files, is a separate pass.

use ripplepath_core::{Fingerprint, Span, SymbolKind, Visibility};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaFile {
    pub path: String,
    pub package: Option<String>,
    pub imports: Vec<Import>,
    pub types: Vec<TypeDecl>,
    /// Tokens outside type declarations: package and imports.
    pub header_fingerprint: Fingerprint,
    pub line_count: u32,
    /// Lines holding ERROR/MISSING nodes. Non-empty means some declarations may be absent.
    pub syntax_error_lines: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub path: String,
    pub is_static: bool,
    pub wildcard: bool,
    pub line: u32,
}

/// A type name as written, with generics stripped: `Map`, `java.util.Map`, `Outer.Inner`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypeUse {
    pub name: String,
    pub line: u32,
    /// Erased top-level type arguments (`Map<UUID, List<Job>>` gives `UUID`, `List`). Empty for
    /// a non-generic or raw type; an empty string for an argument that names no single type
    /// (`?`, `? super T`, arrays). Only used to type elements of well-known JDK containers.
    pub args: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeDecl {
    /// Name relative to the package; nested types are dotted: `Outer.Inner`.
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub span: Span,
    pub fingerprint: Fingerprint,
    pub type_params: Vec<String>,
    pub annotations: Vec<TypeUse>,
    pub extends: Vec<TypeUse>,
    pub implements: Vec<TypeUse>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<MethodDecl>,
    /// References from initializer blocks, attributed to the type itself.
    pub refs: Vec<BodyRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeUse,
    pub visibility: Visibility,
    pub is_static: bool,
    pub span: Span,
    pub fingerprint: Fingerprint,
    /// References from the initializer expression.
    pub refs: Vec<BodyRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: TypeUse,
    /// Text used in the symbol signature: erased type plus `[]`/`...` suffixes.
    pub signature_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodDecl {
    pub name: String,
    pub is_constructor: bool,
    pub is_static: bool,
    pub is_varargs: bool,
    pub visibility: Visibility,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeUse>,
    pub throws: Vec<TypeUse>,
    pub annotations: Vec<TypeUse>,
    pub span: Span,
    pub fingerprint: Fingerprint,
    pub locals: Vec<Local>,
    pub refs: Vec<BodyRef>,
}

impl MethodDecl {
    pub fn signature(&self) -> String {
        let params: Vec<&str> = self.params.iter().map(|p| p.signature_text.as_str()).collect();
        format!("({})", params.join(","))
    }
}

/// A local variable or lambda/catch/for parameter. Scopes are flattened per method: shadowing
/// inside nested blocks is rare enough in practice that tracking block scopes is not worth the
/// complexity, and the failure mode (an inferred receiver type) is labelled STATIC_INFERRED anyway.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Local {
    pub name: String,
    /// `None` when declared with `var` and the initializer type is not syntactically obvious, or
    /// for lambda parameters with inferred types.
    pub ty: Option<TypeUse>,
    /// Where the type of a `var` comes from when it is not syntactically evident. Never set when
    /// `ty` is.
    pub init: Option<LocalInit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalInit {
    /// `var x = <expr>`: typed from the declared return/field type of what `expr` resolves to.
    Expr(Receiver),
    /// `for (var x : <expr>)`: typed as an element of the container `expr` resolves to.
    ElementOf(Receiver),
    /// Parameter `index` of a lambda passed to `<receiver>.<method>(...)`, e.g. `j` in
    /// `jobs.forEach(j -> ...)`: typed from the container `receiver` resolves to.
    LambdaParam { receiver: Receiver, method: String, index: u32 },
}

/// The expression a member is accessed on, reduced to what static resolution can use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Receiver {
    /// Unqualified: `foo()`.
    Implicit,
    This,
    Super,
    /// An identifier: a local, a field, or a type name for static access.
    Name(String),
    Field(Box<Receiver>, String),
    Call {
        receiver: Box<Receiver>,
        name: String,
        arity: u32,
    },
    New(String),
    /// Anything else (array access, ternaries, literals, casts of complex expressions...).
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyRef {
    Call {
        receiver: Receiver,
        name: String,
        arity: u32,
        line: u32,
    },
    /// `this(...)` or `super(...)` as the first statement of a constructor.
    ConstructorCall {
        on_super: bool,
        arity: u32,
        line: u32,
    },
    New {
        ty: TypeUse,
        arity: u32,
    },
    FieldAccess {
        receiver: Receiver,
        name: String,
        line: u32,
    },
    /// A bare identifier in expression position: a local, or a field of the enclosing types.
    Name {
        name: String,
        line: u32,
    },
    MethodRef {
        receiver: Receiver,
        name: String,
        line: u32,
    },
    /// Types named in casts, `instanceof`, class literals, generic arguments, local declarations.
    Type(TypeUse),
}
