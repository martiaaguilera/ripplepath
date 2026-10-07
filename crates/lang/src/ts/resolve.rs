//! Whole-snapshot resolution for TypeScript and JavaScript.
//!
//! Not a type checker. It follows what can be decided from syntax plus declarations in the
//! repository: relative module specifiers, named/default/namespace imports, re-exports (including
//! `export *`), lexical bindings, declared type annotations and `new` initializers, members along
//! in-repo `extends`/`implements`, and declared return types for chained calls. Everything else is
//! either external (packages, globals) or reported as unresolved — never guessed.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ripplepath_core::{EdgeKind, Evidence, Language, Span, Symbol, SymbolId, SymbolKind, Visibility};

use super::facts::{Body, Decl, Expr, Imported, Local, Member, ReExport, Ref, TsFile, TypeRef};
use crate::LanguageGraph;
use crate::output::Output;

/// Re-export chains and type-hierarchy walks stop here; cycles are legal in JS modules.
const MAX_HOPS: usize = 16;
const MAX_EXPR_DEPTH: usize = 64;

/// Extensions tried, in TypeScript's order, for an extensionless relative specifier.
const RESOLVE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs"];

pub fn resolve(files: &[&TsFile]) -> LanguageGraph {
    let index = Index::build(files);
    let mut out = Output::default();
    for file in index.files.values() {
        index.emit_file(file, &mut out);
    }
    out.finish()
}

#[derive(Clone, Copy)]
enum Val<'a> {
    Decl(&'a TsFile, usize),
    Member(&'a TsFile, usize, usize),
    /// An instance of a class or interface declaration.
    Instance(&'a TsFile, usize),
    Namespace(&'a TsFile),
    External,
    Unknown,
}

enum ModuleRes<'a> {
    File(&'a TsFile),
    External,
    /// A relative specifier that matches no file in the snapshot.
    Missing,
}

struct Index<'a> {
    files: BTreeMap<&'a str, &'a TsFile>,
    decl_by_name: BTreeMap<&'a str, BTreeMap<&'a str, usize>>,
    /// Every member name declared in the repository. A call on an untyped receiver is reported as
    /// unresolved only when its name exists here; `arr.map()` on plain data is not noise-worthy.
    member_names: BTreeSet<&'a str>,
    class_names: BTreeSet<&'a str>,
}

struct Scope<'a, 'l> {
    file: &'a TsFile,
    class: Option<usize>,
    locals: &'l [Local],
    type_params: Vec<&'a str>,
}

fn language_of(path: &str) -> Language {
    match path.rsplit_once('.').map(|(_, e)| e) {
        Some("js" | "jsx" | "mjs" | "cjs") => Language::JavaScript,
        _ => Language::TypeScript,
    }
}

fn module_of_path(path: &str) -> String {
    path.rsplit_once('/').map_or_else(|| ".".to_owned(), |(dir, _)| dir.to_owned())
}

fn decl_id(file: &TsFile, decl: &Decl) -> SymbolId {
    SymbolId::new(format!("ts:{}#{}", file.path, decl.name))
}

fn member_id(file: &TsFile, decl: &Decl, member: &Member) -> SymbolId {
    SymbolId::new(format!("ts:{}#{}.{}", file.path, decl.name, member.name))
}

fn weaker(a: Evidence, b: Evidence) -> Evidence {
    if a.strength() <= b.strength() { a } else { b }
}

/// Joins a relative specifier onto the importing file's directory. `None` if it escapes the
/// repository root — such an import cannot name a file we analysed.
fn join_relative(from: &str, spec: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for segment in spec.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

impl<'a> Index<'a> {
    fn build(files: &[&'a TsFile]) -> Self {
        let mut index = Index {
            files: BTreeMap::new(),
            decl_by_name: BTreeMap::new(),
            member_names: BTreeSet::new(),
            class_names: BTreeSet::new(),
        };
        for &file in files {
            index.files.insert(file.path.as_str(), file);
            let names = index.decl_by_name.entry(file.path.as_str()).or_default();
            for (i, decl) in file.decls.iter().enumerate() {
                // Declaration merging (`interface X` + `class X`) keeps the first: one identity.
                names.entry(decl.name.as_str()).or_insert(i);
                if decl.kind == SymbolKind::Class {
                    index.class_names.insert(decl.name.as_str());
                }
                for member in &decl.members {
                    index.member_names.insert(member.name.as_str());
                }
            }
        }
        index
    }

    fn module(&self, from: &str, spec: &str) -> ModuleRes<'a> {
        if !(spec.starts_with("./") || spec.starts_with("../") || spec == "." || spec == "..") {
            // Bare specifiers are packages, or tsconfig `paths` aliases we do not read yet.
            return ModuleRes::External;
        }
        let Some(base) = join_relative(from, spec) else {
            return ModuleRes::Missing;
        };
        let mut candidates = vec![base.clone()];
        // ESM-style TypeScript imports name the emitted `.js` file: `./cart.js` means `./cart.ts`.
        for (emitted, sources) in [(".js", [".ts", ".tsx"]), (".jsx", [".tsx", ".tsx"]), (".mjs", [".mts", ".mts"])] {
            if let Some(stem) = base.strip_suffix(emitted) {
                candidates.extend(sources.iter().map(|s| format!("{stem}{s}")));
            }
        }
        candidates.extend(RESOLVE_EXTENSIONS.iter().map(|ext| format!("{base}{ext}")));
        candidates.extend(RESOLVE_EXTENSIONS.iter().map(|ext| format!("{base}/index{ext}")));
        candidates
            .iter()
            .find_map(|c| self.files.get(c.trim_start_matches("./")))
            .map_or(ModuleRes::Missing, |f| ModuleRes::File(f))
    }

    fn decl_index(&self, file: &TsFile, name: &str) -> Option<usize> {
        self.decl_by_name.get(file.path.as_str()).and_then(|m| m.get(name)).copied()
    }

    /// A name bound at module level: a declaration or an import.
    fn binding(&self, file: &'a TsFile, name: &str, hops: usize) -> Option<(Val<'a>, Evidence)> {
        if let Some(i) = self.decl_index(file, name) {
            return Some((Val::Decl(file, i), Evidence::ResolvedExact));
        }
        let import = file.imports.iter().find(|i| i.local == name)?;
        Some(match self.module(&file.path, &import.source) {
            ModuleRes::External => (Val::External, Evidence::ResolvedExact),
            ModuleRes::Missing => (Val::Unknown, Evidence::ResolvedExact),
            ModuleRes::File(target) => match &import.imported {
                Imported::Namespace => (Val::Namespace(target), Evidence::ResolvedExact),
                Imported::Default => self.export(target, "default", hops + 1),
                Imported::Named(n) => self.export(target, n, hops + 1),
            },
        })
    }

    fn export(&self, file: &'a TsFile, name: &str, hops: usize) -> (Val<'a>, Evidence) {
        if hops > MAX_HOPS {
            return (Val::Unknown, Evidence::ResolvedExact);
        }
        if let Some(local) = file.local_exports.iter().find(|e| e.exported == name)
            && let Some(found) = self.binding(file, &local.local, hops)
        {
            return found;
        }
        let direct = file.decls.iter().position(|d| {
            d.exported && if name == "default" { d.default_export } else { !d.default_export && d.name == name }
        });
        if let Some(i) = direct {
            return (Val::Decl(file, i), Evidence::ResolvedExact);
        }
        for reexport in &file.reexports {
            match reexport {
                ReExport::Named { name: inner, alias, source, .. } if alias == name => {
                    return match self.module(&file.path, source) {
                        ModuleRes::File(target) => self.export(target, inner, hops + 1),
                        ModuleRes::External => (Val::External, Evidence::ResolvedExact),
                        ModuleRes::Missing => (Val::Unknown, Evidence::ResolvedExact),
                    };
                }
                ReExport::Namespace { alias, source, .. } if alias == name => {
                    return match self.module(&file.path, source) {
                        ModuleRes::File(target) => (Val::Namespace(target), Evidence::ResolvedExact),
                        ModuleRes::External => (Val::External, Evidence::ResolvedExact),
                        ModuleRes::Missing => (Val::Unknown, Evidence::ResolvedExact),
                    };
                }
                _ => {}
            }
        }
        if name == "default" {
            return (Val::Unknown, Evidence::ResolvedExact); // `export *` never forwards default
        }
        let mut found: Vec<(Val<'a>, Evidence)> = Vec::new();
        let mut external = false;
        for reexport in &file.reexports {
            if let ReExport::All { source, .. } = reexport {
                match self.module(&file.path, source) {
                    ModuleRes::File(target) => {
                        let result = self.export(target, name, hops + 1);
                        if !matches!(result.0, Val::Unknown) {
                            found.push(result);
                        }
                    }
                    ModuleRes::External => external = true,
                    ModuleRes::Missing => {}
                }
            }
        }
        match found.len() {
            0 if external => (Val::External, Evidence::ResolvedExact),
            0 => (Val::Unknown, Evidence::ResolvedExact),
            1 => found.remove(0),
            // Two `export *` providing the same name is ambiguous in TypeScript (and an error when
            // used); keep the first deterministically but do not claim certainty.
            _ => (found.remove(0).0, Evidence::StaticInferred),
        }
    }

    /// Resolves a written type name to a declaration, in the context of `file`.
    fn type_decl(&self, file: &'a TsFile, type_params: &[&str], name: &str) -> (Val<'a>, Evidence) {
        let mut segments = name.split('.');
        let Some(head) = segments.next() else {
            return (Val::Unknown, Evidence::ResolvedExact);
        };
        if type_params.contains(&head) {
            return (Val::External, Evidence::ResolvedExact);
        }
        let Some((mut val, mut evidence)) = self.binding(file, head, 0) else {
            return (Val::External, Evidence::ResolvedExact); // global/lib type: string, Promise, Date…
        };
        for segment in segments {
            (val, evidence) = match val {
                Val::Namespace(target) => {
                    let (v, e) = self.export(target, segment, 0);
                    (v, weaker(evidence, e))
                }
                Val::External => return (Val::External, evidence),
                _ => return (Val::Unknown, evidence),
            };
        }
        (val, evidence)
    }

    fn instance_of(&self, file: &'a TsFile, type_params: &[&str], ty: &TypeRef) -> (Val<'a>, Evidence) {
        match self.type_decl(file, type_params, &ty.name) {
            (Val::Decl(f, i), e) if matches!(f.decls[i].kind, SymbolKind::Class | SymbolKind::Interface) => {
                (Val::Instance(f, i), e)
            }
            (Val::External, e) => (Val::External, e),
            (_, e) => (Val::Unknown, e),
        }
    }

    fn supertypes(&self, file: &'a TsFile, decl: usize) -> Vec<(&'a TsFile, usize, EdgeKind, Evidence, u32)> {
        let d = &file.decls[decl];
        let params: Vec<&str> = d.type_params.iter().map(String::as_str).collect();
        d.extends
            .iter()
            .map(|t| (t, EdgeKind::Extends))
            .chain(d.implements.iter().map(|t| (t, EdgeKind::Implements)))
            .filter_map(|(ty, kind)| match self.type_decl(file, &params, &ty.name) {
                (Val::Decl(f, i), e) => Some((f, i, kind, e, ty.line)),
                _ => None,
            })
            .collect()
    }

    /// Nearest member named `name` along the in-repo hierarchy (breadth-first). The flag reports
    /// whether some ancestor is outside the repository, in which case "not found" means external.
    fn member(&self, file: &'a TsFile, decl: usize, name: &str) -> (Option<(&'a TsFile, usize, usize)>, bool) {
        let mut queue = VecDeque::from([(file, decl)]);
        let mut seen = BTreeSet::new();
        let mut external_ancestor = false;
        while let Some((f, d)) = queue.pop_front() {
            if !seen.insert((f.path.as_str(), d)) || seen.len() > MAX_HOPS * 4 {
                continue;
            }
            if let Some(m) = f.decls[d].members.iter().position(|m| m.name == name) {
                return (Some((f, d, m)), external_ancestor);
            }
            let supers = self.supertypes(f, d);
            let declared = f.decls[d].extends.len() + f.decls[d].implements.len();
            external_ancestor |= supers.len() < declared;
            queue.extend(supers.into_iter().map(|(sf, sd, ..)| (sf, sd)));
        }
        (None, external_ancestor)
    }

    fn resolve_ident(&self, scope: &Scope<'a, '_>, name: &str) -> (Val<'a>, Evidence) {
        // Later declarations shadow earlier ones in the flattened scope.
        if let Some(local) = scope.locals.iter().rev().find(|l| l.name == name) {
            return match &local.ty {
                Some(ty) => self.instance_of(scope.file, &scope.type_params, ty),
                None => (Val::Unknown, Evidence::ResolvedExact),
            };
        }
        self.binding(scope.file, name, 0).unwrap_or((Val::External, Evidence::ResolvedExact))
    }

    /// What member access on `val` operates on.
    fn receiver(&self, val: Val<'a>) -> (Val<'a>, Evidence) {
        let exact = Evidence::ResolvedExact;
        match val {
            Val::Decl(f, i) => {
                let decl = &f.decls[i];
                match decl.kind {
                    SymbolKind::Class | SymbolKind::Enum => (val, exact),
                    SymbolKind::Variable => match decl.ty.as_ref().or(decl.inferred_ty.as_ref()) {
                        Some(ty) => self.instance_of(f, &[], ty),
                        None => (Val::Unknown, exact),
                    },
                    _ => (Val::Unknown, exact),
                }
            }
            Val::Member(f, d, m) => {
                let decl = &f.decls[d];
                let member = &decl.members[m];
                if member.kind != SymbolKind::Field {
                    return (Val::Unknown, exact);
                }
                let params: Vec<&str> = decl.type_params.iter().map(String::as_str).collect();
                match member.ty.as_ref().or(member.inferred_ty.as_ref()) {
                    Some(ty) => self.instance_of(f, &params, ty),
                    None => (Val::Unknown, exact),
                }
            }
            other => (other, exact),
        }
    }

    fn resolve_expr(
        &self,
        scope: &Scope<'a, '_>,
        expr: &Expr,
        depth: usize,
        touched: &mut Vec<SymbolId>,
    ) -> (Val<'a>, Evidence) {
        let exact = Evidence::ResolvedExact;
        if depth > MAX_EXPR_DEPTH {
            return (Val::Unknown, exact);
        }
        match expr {
            Expr::Ident(name) => self.resolve_ident(scope, name),
            Expr::This => scope.class.map_or((Val::Unknown, exact), |c| (Val::Instance(scope.file, c), exact)),
            Expr::Super => match scope.class.and_then(|c| {
                self.supertypes(scope.file, c).into_iter().find(|(.., kind, _, _)| *kind == EdgeKind::Extends)
            }) {
                Some((f, d, _, e, _)) => (Val::Instance(f, d), e),
                None => (Val::External, exact),
            },
            Expr::Member(inner, property) => {
                let (base, base_evidence) = self.resolve_expr(scope, inner, depth + 1, touched);
                if let Val::Member(f, d, m) = base
                    && f.decls[d].members[m].kind == SymbolKind::Field
                {
                    touched.push(member_id(f, &f.decls[d], &f.decls[d].members[m]));
                }
                if let Val::Decl(f, d) = base
                    && f.decls[d].kind == SymbolKind::Variable
                {
                    touched.push(decl_id(f, &f.decls[d]));
                }
                let (target, receiver_evidence) = self.receiver(base);
                let evidence = weaker(base_evidence, receiver_evidence);
                match target {
                    Val::Namespace(f) => {
                        let (v, e) = self.export(f, property, 0);
                        (v, weaker(evidence, e))
                    }
                    Val::Instance(f, d) | Val::Decl(f, d) if f.decls[d].kind != SymbolKind::Enum => {
                        match self.member(f, d, property) {
                            (Some((mf, md, mm)), _) => (Val::Member(mf, md, mm), evidence),
                            (None, true) => (Val::External, evidence),
                            (None, false) => (Val::Unknown, evidence),
                        }
                    }
                    Val::Decl(..) => (Val::External, evidence), // enum member
                    Val::External => (Val::External, evidence),
                    _ => (Val::Unknown, evidence),
                }
            }
            Expr::CallResult(inner) => {
                let (callee, evidence) = self.resolve_expr(scope, inner, depth + 1, touched);
                let (file, params, ty) = match callee {
                    Val::Decl(f, d) => {
                        (f, f.decls[d].type_params.iter().map(String::as_str).collect(), f.decls[d].ty.as_ref())
                    }
                    Val::Member(f, d, m) => (
                        f,
                        f.decls[d].type_params.iter().map(String::as_str).collect(),
                        f.decls[d].members[m].ty.as_ref(),
                    ),
                    Val::External => return (Val::External, evidence),
                    _ => return (Val::Unknown, evidence),
                };
                let params: Vec<&str> = params;
                match ty {
                    Some(ty) => {
                        let (v, e) = self.instance_of(file, &params, ty);
                        (v, weaker(evidence, e))
                    }
                    None => (Val::Unknown, evidence),
                }
            }
            Expr::New(inner) => match self.resolve_expr(scope, inner, depth + 1, touched) {
                (Val::Decl(f, d), e) if f.decls[d].kind == SymbolKind::Class => (Val::Instance(f, d), e),
                (Val::External, e) => (Val::External, e),
                (_, e) => (Val::Unknown, e),
            },
            Expr::Unknown => (Val::Unknown, exact),
        }
    }

    fn symbol_of(&self, val: Val<'a>) -> Option<SymbolId> {
        match val {
            Val::Decl(f, d) => Some(decl_id(f, &f.decls[d])),
            Val::Member(f, d, m) => Some(member_id(f, &f.decls[d], &f.decls[d].members[m])),
            _ => None,
        }
    }

    fn constructor_of(&self, file: &'a TsFile, decl: usize) -> SymbolId {
        let d = &file.decls[decl];
        match d.members.iter().find(|m| m.kind == SymbolKind::Constructor) {
            Some(ctor) => member_id(file, d, ctor),
            // No declared constructor: the class itself is what runs (field initialisers).
            None => decl_id(file, d),
        }
    }

    fn last_name(expr: &Expr) -> Option<&str> {
        match expr {
            Expr::Ident(n) | Expr::Member(_, n) => Some(n),
            _ => None,
        }
    }

    fn emit_body(&self, scope: &Scope<'a, '_>, from: &SymbolId, body: &Body, out: &mut Output) {
        let path = scope.file.path.as_str();
        for reference in &body.refs {
            let mut touched = Vec::new();
            match reference {
                Ref::Call { callee: Expr::Super, line } => {
                    if let Some((f, d, ..)) = scope
                        .class
                        .and_then(|c| self.supertypes(scope.file, c).into_iter().find(|s| s.2 == EdgeKind::Extends))
                    {
                        out.edge(
                            from,
                            &self.constructor_of(f, d),
                            EdgeKind::Calls,
                            Evidence::ResolvedExact,
                            path,
                            *line,
                            "ts.call.super",
                        );
                    }
                }
                Ref::Call { callee, line } => match self.resolve_expr(scope, callee, 0, &mut touched) {
                    (Val::Decl(f, d), e) if f.decls[d].kind == SymbolKind::Class => {
                        // `<Component/>` on a class component, or calling a class (a runtime error).
                        out.edge(
                            from,
                            &self.constructor_of(f, d),
                            EdgeKind::Instantiates,
                            e,
                            path,
                            *line,
                            "ts.jsx.class",
                        );
                    }
                    (val @ (Val::Decl(..) | Val::Member(..)), e) => {
                        if let Some(target) = self.symbol_of(val) {
                            out.edge(from, &target, EdgeKind::Calls, e, path, *line, "ts.call");
                        }
                    }
                    (Val::Unknown, _) => self.report_unknown(scope, from, callee, *line, "call", out),
                    _ => {}
                },
                Ref::New { ctor, line } => match self.resolve_expr(scope, ctor, 0, &mut touched) {
                    (Val::Decl(f, d), e) if f.decls[d].kind == SymbolKind::Class => {
                        out.edge(from, &self.constructor_of(f, d), EdgeKind::Instantiates, e, path, *line, "ts.new");
                    }
                    (Val::Unknown, _) => self.report_unknown(scope, from, ctor, *line, "new", out),
                    _ => {}
                },
                Ref::Read { expr, line } => {
                    if let (val @ (Val::Decl(..) | Val::Member(..)), e) =
                        self.resolve_expr(scope, expr, 0, &mut touched)
                        && let Some(target) = self.symbol_of(val)
                    {
                        out.edge(from, &target, EdgeKind::References, e, path, *line, "ts.read");
                    }
                }
                Ref::Type(ty) => {
                    if let (val @ Val::Decl(..), e) = self.type_decl(scope.file, &scope.type_params, &ty.name)
                        && let Some(target) = self.symbol_of(val)
                    {
                        out.edge(from, &target, EdgeKind::References, e, path, ty.line, "ts.type");
                    }
                }
            }
            let line = match reference {
                Ref::Call { line, .. } | Ref::New { line, .. } | Ref::Read { line, .. } => *line,
                Ref::Type(ty) => ty.line,
            };
            for target in touched {
                out.edge(
                    from,
                    &target,
                    EdgeKind::References,
                    Evidence::ResolvedExact,
                    path,
                    line,
                    "ts.member.receiver",
                );
            }
        }
    }

    fn report_unknown(
        &self,
        scope: &Scope<'a, '_>,
        from: &SymbolId,
        expr: &Expr,
        line: u32,
        what: &str,
        out: &mut Output,
    ) {
        let Some(name) = Self::last_name(expr) else {
            return;
        };
        let plausible = match (what, expr) {
            ("new", _) => self.class_names.contains(name),
            (_, Expr::Ident(n)) => scope.file.imports.iter().any(|i| &i.local == n),
            _ => self.member_names.contains(name),
        };
        if plausible {
            out.unresolved(from, &scope.file.path, line, format!("{what} {name}"));
        }
    }

    fn emit_file(&self, file: &'a TsFile, out: &mut Output) {
        let path = file.path.as_str();
        let language = language_of(path);
        let module = module_of_path(path);
        let file_id = SymbolId::file(path);
        let symbol =
            |id: SymbolId, kind, name: String, span: Span, parent: Option<SymbolId>, visibility, is_test, fp| Symbol {
                id,
                kind,
                name,
                language,
                module: module.clone(),
                file: path.to_owned(),
                span,
                parent,
                visibility,
                is_test,
                fingerprint: fp,
            };
        out.symbols.push(symbol(
            file_id.clone(),
            SymbolKind::File,
            path.rsplit('/').next().unwrap_or(path).to_owned(),
            Span { start_line: 1, end_line: file.line_count.max(1) },
            None,
            Visibility::Public,
            file.is_test_file,
            file.header_fingerprint,
        ));

        for import in &file.imports {
            match self.module(path, &import.source) {
                ModuleRes::Missing => {
                    out.unresolved(&file_id, path, import.line, format!("import {}", import.source));
                }
                ModuleRes::External => {}
                ModuleRes::File(target) => {
                    let resolved = match &import.imported {
                        Imported::Namespace => Some(SymbolId::file(&target.path)),
                        Imported::Default => self.symbol_of(self.export(target, "default", 0).0),
                        Imported::Named(n) => self.symbol_of(self.export(target, n, 0).0),
                    };
                    match resolved {
                        Some(id) => out.edge(
                            &file_id,
                            &id,
                            EdgeKind::Imports,
                            Evidence::ResolvedExact,
                            path,
                            import.line,
                            "ts.import",
                        ),
                        None => {
                            let name = match &import.imported {
                                Imported::Named(n) => n.as_str(),
                                _ => "default",
                            };
                            out.unresolved(
                                &file_id,
                                path,
                                import.line,
                                format!("import {name} from {}", import.source),
                            );
                        }
                    }
                }
            }
        }

        let module_scope = Scope { file, class: None, locals: &file.module_body.locals, type_params: Vec::new() };
        self.emit_body(&module_scope, &file_id, &file.module_body, out);

        for test in &file.tests {
            let id = SymbolId::new(format!("ts:{path}#test:{}", test.title));
            out.symbols.push(symbol(
                id.clone(),
                SymbolKind::TestCase,
                test.title.clone(),
                test.span,
                Some(file_id.clone()),
                Visibility::Public,
                true,
                test.fingerprint,
            ));
            out.edge(
                &file_id,
                &id,
                EdgeKind::Contains,
                Evidence::ResolvedExact,
                path,
                test.span.start_line,
                "ts.declaration",
            );
            let scope = Scope { file, class: None, locals: &test.body.locals, type_params: Vec::new() };
            self.emit_body(&scope, &id, &test.body, out);
        }

        for (index, decl) in file.decls.iter().enumerate() {
            // Merged declarations (same name declared twice) are emitted once.
            if self.decl_index(file, &decl.name) != Some(index) {
                continue;
            }
            self.emit_decl(file, index, &file_id, out, &symbol);
        }
    }

    #[allow(clippy::type_complexity)]
    fn emit_decl(
        &self,
        file: &'a TsFile,
        index: usize,
        file_id: &SymbolId,
        out: &mut Output,
        symbol: &dyn Fn(
            SymbolId,
            SymbolKind,
            String,
            Span,
            Option<SymbolId>,
            Visibility,
            bool,
            ripplepath_core::Fingerprint,
        ) -> Symbol,
    ) {
        let path = file.path.as_str();
        let decl = &file.decls[index];
        let id = decl_id(file, decl);
        let visibility = if decl.exported { Visibility::Public } else { Visibility::Package };
        out.symbols.push(symbol(
            id.clone(),
            decl.kind,
            decl.name.clone(),
            decl.span,
            Some(file_id.clone()),
            visibility,
            false,
            decl.fingerprint,
        ));
        out.edge(
            file_id,
            &id,
            EdgeKind::Contains,
            Evidence::ResolvedExact,
            path,
            decl.span.start_line,
            "ts.declaration",
        );

        for (super_file, super_decl, kind, evidence, line) in self.supertypes(file, index) {
            out.edge(
                &id,
                &decl_id(super_file, &super_file.decls[super_decl]),
                kind,
                evidence,
                path,
                line,
                "ts.supertype",
            );
        }
        let type_params: Vec<&str> = decl.type_params.iter().map(String::as_str).collect();
        let class = matches!(decl.kind, SymbolKind::Class | SymbolKind::Interface).then_some(index);
        let decl_scope = Scope { file, class, locals: &decl.body.locals, type_params: type_params.clone() };
        if let Some(ty) = &decl.ty {
            self.emit_body(&decl_scope, &id, &Body { locals: Vec::new(), refs: vec![Ref::Type(ty.clone())] }, out);
        }
        self.emit_body(&decl_scope, &id, &decl.body, out);

        for member in &decl.members {
            let member_symbol = member_id(file, decl, member);
            out.symbols.push(symbol(
                member_symbol.clone(),
                member.kind,
                format!("{}.{}", decl.name, member.name),
                member.span,
                Some(id.clone()),
                member.visibility,
                false,
                member.fingerprint,
            ));
            out.edge(
                &id,
                &member_symbol,
                EdgeKind::Contains,
                Evidence::ResolvedExact,
                path,
                member.span.start_line,
                "ts.declaration",
            );
            let scope = Scope { file, class, locals: &member.body.locals, type_params: type_params.clone() };
            if let Some(ty) = &member.ty {
                self.emit_body(
                    &scope,
                    &member_symbol,
                    &Body { locals: Vec::new(), refs: vec![Ref::Type(ty.clone())] },
                    out,
                );
            }
            self.emit_body(&scope, &member_symbol, &member.body, out);
            if member.kind != SymbolKind::Constructor && member.visibility != Visibility::Private {
                self.emit_overrides(file, index, member, &member_symbol, out);
            }
        }
    }

    /// TypeScript has no overloading by parameter list, so a member overrides every same-named
    /// member of its supertypes.
    fn emit_overrides(
        &self,
        file: &'a TsFile,
        decl: usize,
        member: &Member,
        member_symbol: &SymbolId,
        out: &mut Output,
    ) {
        let mut queue: VecDeque<(&'a TsFile, usize)> =
            self.supertypes(file, decl).into_iter().map(|(f, d, ..)| (f, d)).collect();
        let mut seen = BTreeSet::new();
        while let Some((f, d)) = queue.pop_front() {
            if !seen.insert((f.path.as_str(), d)) || seen.len() > MAX_HOPS * 4 {
                continue;
            }
            let ancestor = &f.decls[d];
            if let Some(overridden) =
                ancestor.members.iter().find(|m| m.name == member.name && m.kind != SymbolKind::Constructor)
            {
                out.edge(
                    member_symbol,
                    &member_id(f, ancestor, overridden),
                    EdgeKind::Overrides,
                    Evidence::ResolvedExact,
                    &file.path,
                    member.span.start_line,
                    "ts.override",
                );
            }
            queue.extend(self.supertypes(f, d).into_iter().map(|(sf, sd, ..)| (sf, sd)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::join_relative;

    #[test]
    fn relative_specifiers_join_and_cannot_escape_the_root() {
        assert_eq!(join_relative("src/a/b.ts", "./c").as_deref(), Some("src/a/c"));
        assert_eq!(join_relative("src/a/b.ts", "../c/d").as_deref(), Some("src/c/d"));
        assert_eq!(join_relative("b.ts", "./c").as_deref(), Some("c"));
        assert_eq!(join_relative("src/b.ts", "../../etc/passwd"), None);
    }
}
