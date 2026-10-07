use std::collections::HashMap;
use std::time::Duration;

use ripplepath_core::{SymbolKind, Visibility};
use tree_sitter::Node;

use super::facts::{
    Body, Decl, Expr, Import, Imported, Local, LocalExport, Member, ReExport, Ref, TestCase, TsFile, TypeRef,
};
use crate::syntax::{self, ParseError, line, named_children, span, text};

/// Expression chains deeper than this become `Expr::Unknown`; real chains are a handful of links,
/// the cap only stops crafted input from driving unbounded recursion.
const MAX_EXPR_DEPTH: usize = 64;
/// `describe` nesting deeper than this is not searched for test cases.
const MAX_SUITE_DEPTH: usize = 16;
/// Suite-level locals visible to each test. Each test gets its own copy, so without a cap a suite
/// with N locals and M tests would cost N×M; the most recent declarations are kept because they
/// shadow earlier ones.
const MAX_SUITE_LOCALS: usize = 256;
/// Identifiers collected from one destructuring pattern.
const MAX_PATTERN_NAMES: usize = 256;

const SUITE_FUNCTIONS: &[&str] = &["describe", "suite", "context"];
const TEST_FUNCTIONS: &[&str] = &["it", "test", "specify"];

pub fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem_parts: Vec<&str> = name.split('.').collect();
    let marked =
        stem_parts.len() >= 3 && stem_parts[..stem_parts.len() - 1].iter().any(|p| *p == "test" || *p == "spec");
    marked || path.split('/').any(|component| component == "__tests__")
}

/// TSX is used for `.tsx` and for all JavaScript: modern JS is a syntactic subset of TSX, while the
/// plain TypeScript grammar rejects JSX that `.js` React code commonly contains.
fn grammar_for(path: &str) -> tree_sitter::Language {
    let ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext {
        "ts" | "mts" | "cts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        _ => tree_sitter_typescript::LANGUAGE_TSX.into(),
    }
}

fn is_comment(kind: &str) -> bool {
    kind == "comment"
}

pub fn extract(path: &str, source: &str, budget: Duration) -> Result<TsFile, ParseError> {
    let tree = syntax::parse(&grammar_for(path), source, budget)?;
    let root = tree.root_node();
    let mut cx = FileCx {
        source,
        is_test_file: is_test_path(path),
        imports: Vec::new(),
        reexports: Vec::new(),
        local_exports: Vec::new(),
        decls: Vec::new(),
        tests: Vec::new(),
        module_body: Body::default(),
        owned_nodes: Vec::new(),
        suite_code: Vec::new(),
    };
    for statement in named_children(root) {
        cx.statement(statement);
    }
    // The file symbol stands for module-level code: everything outside declarations and tests,
    // plus suite-level statements (hooks, fixtures). `describe` wrappers themselves are excluded,
    // so adding a new suite does not mark every other test in the file as changed.
    let mut header = ripplepath_core::FingerprintBuilder::new();
    header.token(&syntax::fingerprint(root, source, &cx.owned_nodes, is_comment).to_string());
    for fingerprint in &cx.suite_code {
        header.token(&fingerprint.to_string());
    }
    let header_fingerprint = header.finish();
    let mut tests = cx.tests;
    disambiguate_titles(&mut tests);
    Ok(TsFile {
        path: path.to_owned(),
        is_test_file: cx.is_test_file,
        imports: cx.imports,
        reexports: cx.reexports,
        local_exports: cx.local_exports,
        decls: cx.decls,
        tests,
        module_body: cx.module_body,
        header_fingerprint,
        line_count: source.lines().count() as u32,
        syntax_error_lines: syntax::syntax_error_lines(&tree),
    })
}

/// Two tests with the same title in one file would collide on identity; the second becomes
/// `title #2`. Order-based, so reordering identical titles changes ids — a rare, documented case.
fn disambiguate_titles(tests: &mut [TestCase]) {
    let mut seen: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for test in tests {
        let count = seen.entry(test.title.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            test.title = format!("{} #{}", test.title, count);
        }
    }
}

struct FileCx<'s> {
    source: &'s str,
    is_test_file: bool,
    imports: Vec<Import>,
    reexports: Vec<ReExport>,
    local_exports: Vec<LocalExport>,
    decls: Vec<Decl>,
    tests: Vec<TestCase>,
    module_body: Body,
    /// Nodes fingerprinted as their own symbols, excluded from the file header fingerprint.
    owned_nodes: Vec<usize>,
    /// Fingerprints of suite-level statements, folded into the file header fingerprint.
    suite_code: Vec<ripplepath_core::Fingerprint>,
}

fn string_value(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "string" => Some(
            named_children(node)
                .into_iter()
                .filter(|n| n.kind() == "string_fragment")
                .map(|n| text(n, source))
                .collect(),
        ),
        // Template literals are titles only when they have no substitutions.
        "template_string" => {
            let parts = named_children(node);
            if parts.iter().any(|n| n.kind() == "template_substitution") {
                None
            } else {
                Some(parts.into_iter().filter(|n| n.kind() == "string_fragment").map(|n| text(n, source)).collect())
            }
        }
        _ => None,
    }
}

impl<'s> FileCx<'s> {
    fn statement(&mut self, node: Node<'_>) {
        let source = self.source;
        match node.kind() {
            "import_statement" => self.import(node),
            "export_statement" => self.export(node),
            "expression_statement" if self.is_test_file => {
                let mut handled = false;
                if let Some(call) = named_children(node).into_iter().find(|n| n.kind() == "call_expression") {
                    handled = self.test_call(call, &[], &[], 0);
                }
                if handled {
                    self.owned_nodes.push(node.id());
                } else {
                    walk(node, source, &mut self.module_body);
                }
            }
            "comment" => {}
            kind => {
                let before = self.decls.len();
                self.declaration(node, false, false);
                if self.decls.len() == before && !is_declaration(kind) {
                    walk(node, source, &mut self.module_body);
                }
            }
        }
    }

    fn import(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(spec) = node.child_by_field_name("source").and_then(|s| string_value(s, source)) else {
            return;
        };
        let line = line(node);
        let Some(clause) = named_children(node).into_iter().find(|n| n.kind() == "import_clause") else {
            return; // side-effect import: no bindings
        };
        for part in named_children(clause) {
            match part.kind() {
                "identifier" => self.imports.push(Import {
                    local: text(part, source).to_owned(),
                    source: spec.clone(),
                    imported: Imported::Default,
                    line,
                }),
                "namespace_import" => {
                    if let Some(ident) = named_children(part).into_iter().find(|n| n.kind() == "identifier") {
                        self.imports.push(Import {
                            local: text(ident, source).to_owned(),
                            source: spec.clone(),
                            imported: Imported::Namespace,
                            line,
                        });
                    }
                }
                "named_imports" => {
                    for specifier in named_children(part).into_iter().filter(|n| n.kind() == "import_specifier") {
                        let Some(name) = specifier.child_by_field_name("name") else {
                            continue;
                        };
                        let name = string_value(name, source).unwrap_or_else(|| text(name, source).to_owned());
                        let local = specifier
                            .child_by_field_name("alias")
                            .map_or_else(|| name.clone(), |a| text(a, source).to_owned());
                        let imported = if name == "default" { Imported::Default } else { Imported::Named(name) };
                        self.imports.push(Import { local, source: spec.clone(), imported, line });
                    }
                }
                _ => {}
            }
        }
    }

    fn export(&mut self, node: Node<'_>) {
        let source = self.source;
        let line = line(node);
        let mut cursor = node.walk();
        let is_default = node.children(&mut cursor).any(|c| c.kind() == "default");
        let reexport_source = node.child_by_field_name("source").and_then(|s| string_value(s, source));

        if let Some(declaration) = node.child_by_field_name("declaration") {
            self.declaration(declaration, true, is_default);
            return;
        }
        if let Some(value) = node.child_by_field_name("value") {
            match value.kind() {
                "identifier" => self.local_exports.push(LocalExport {
                    local: text(value, source).to_owned(),
                    exported: "default".to_owned(),
                    line,
                }),
                _ => self.default_expression(value),
            }
            return;
        }
        let children = named_children(node);
        if let Some(clause) = children.iter().find(|n| n.kind() == "export_clause") {
            for specifier in named_children(*clause).into_iter().filter(|n| n.kind() == "export_specifier") {
                let Some(name) = specifier.child_by_field_name("name") else {
                    continue;
                };
                let name = string_value(name, source).unwrap_or_else(|| text(name, source).to_owned());
                let alias = specifier.child_by_field_name("alias").map_or_else(
                    || name.clone(),
                    |a| string_value(a, source).unwrap_or_else(|| text(a, source).to_owned()),
                );
                match &reexport_source {
                    Some(spec) => self.reexports.push(ReExport::Named { name, alias, source: spec.clone(), line }),
                    None => self.local_exports.push(LocalExport { local: name, exported: alias, line }),
                }
            }
            return;
        }
        if let Some(spec) = reexport_source {
            match children.iter().find(|n| n.kind() == "namespace_export") {
                Some(ns) => {
                    let alias = named_children(*ns)
                        .into_iter()
                        .next()
                        .map(|a| string_value(a, source).unwrap_or_else(|| text(a, source).to_owned()))
                        .unwrap_or_default();
                    self.reexports.push(ReExport::Namespace { alias, source: spec, line });
                }
                None => self.reexports.push(ReExport::All { source: spec, line }),
            }
        }
    }

    /// `export default <expression>` that is not a bare identifier.
    fn default_expression(&mut self, value: Node<'_>) {
        match value.kind() {
            "function_expression" | "arrow_function" | "generator_function" => {
                let name = value.child_by_field_name("name").map_or("default", |n| text(n, self.source)).to_owned();
                self.function(value, name, true, true);
            }
            "class" => self.class(value, true, true),
            _ => {
                let mut decl = self.empty_decl(value, "default".to_owned(), SymbolKind::Variable, true, true);
                walk(value, self.source, &mut decl.body);
                self.owned_nodes.push(value.id());
                self.decls.push(decl);
            }
        }
    }

    fn empty_decl(&self, node: Node<'_>, name: String, kind: SymbolKind, exported: bool, default_export: bool) -> Decl {
        Decl {
            name,
            kind,
            exported,
            default_export,
            span: span(node),
            fingerprint: syntax::fingerprint(node, self.source, &[], is_comment),
            type_params: type_params(node, self.source),
            extends: Vec::new(),
            implements: Vec::new(),
            members: Vec::new(),
            ty: None,
            inferred_ty: None,
            body: Body::default(),
        }
    }

    fn declaration(&mut self, node: Node<'_>, exported: bool, default_export: bool) {
        let source = self.source;
        match node.kind() {
            "function_declaration" | "generator_function_declaration" | "function_signature" => {
                let name = node.child_by_field_name("name").map_or("default", |n| text(n, source)).to_owned();
                self.function(node, name, exported, default_export);
            }
            "class_declaration" | "abstract_class_declaration" => self.class(node, exported, default_export),
            "interface_declaration" => self.interface(node, exported, default_export),
            "type_alias_declaration" | "enum_declaration" => {
                let Some(name) = node.child_by_field_name("name") else {
                    return;
                };
                let kind = if node.kind() == "enum_declaration" { SymbolKind::Enum } else { SymbolKind::TypeAlias };
                let mut decl = self.empty_decl(node, text(name, source).to_owned(), kind, exported, default_export);
                for field in ["value", "body"] {
                    if let Some(child) = node.child_by_field_name(field) {
                        walk(child, source, &mut decl.body);
                    }
                }
                self.owned_nodes.push(node.id());
                self.decls.push(decl);
            }
            "lexical_declaration" | "variable_declaration" => {
                for declarator in named_children(node).into_iter().filter(|n| n.kind() == "variable_declarator") {
                    self.variable(declarator, exported);
                }
            }
            // `declare module "x" { … }` and `declare const x: T` describe external code.
            "ambient_declaration" => {
                for inner in named_children(node) {
                    self.declaration(inner, exported, default_export);
                }
            }
            _ => {}
        }
    }

    fn function(&mut self, node: Node<'_>, name: String, exported: bool, default_export: bool) {
        let mut decl = self.empty_decl(node, name, SymbolKind::Function, exported, default_export);
        decl.ty = node.child_by_field_name("return_type").and_then(|t| annotation_type(t, self.source));
        function_body(node, self.source, &mut decl.body);
        self.owned_nodes.push(node.id());
        self.decls.push(decl);
    }

    fn variable(&mut self, declarator: Node<'_>, exported: bool) {
        let source = self.source;
        let Some(name) = declarator.child_by_field_name("name") else {
            return;
        };
        if name.kind() != "identifier" {
            // Destructuring at module level: the bound names are not symbols, but the code still runs.
            walk(declarator, source, &mut self.module_body);
            return;
        }
        let value = declarator.child_by_field_name("value");
        let is_function =
            value.is_some_and(|v| matches!(v.kind(), "arrow_function" | "function_expression" | "generator_function"));
        let kind = if is_function { SymbolKind::Function } else { SymbolKind::Variable };
        let mut decl = self.empty_decl(declarator, text(name, source).to_owned(), kind, exported, false);
        if let (true, Some(function)) = (is_function, value) {
            decl.ty = function.child_by_field_name("return_type").and_then(|t| annotation_type(t, source));
            decl.type_params = type_params(function, source);
            if let Some(annotation) = declarator.child_by_field_name("type") {
                walk(annotation, source, &mut decl.body);
            }
            function_body(function, source, &mut decl.body);
        } else {
            decl.ty = declarator.child_by_field_name("type").and_then(|t| annotation_type(t, source));
            decl.inferred_ty = value.and_then(|v| new_type(v, source));
            walk(declarator, source, &mut decl.body);
        }
        self.owned_nodes.push(declarator.id());
        self.decls.push(decl);
    }

    fn class(&mut self, node: Node<'_>, exported: bool, default_export: bool) {
        let source = self.source;
        let name = node.child_by_field_name("name").map_or("default", |n| text(n, source)).to_owned();
        let mut decl = self.empty_decl(node, name, SymbolKind::Class, exported, default_export);
        for heritage in named_children(node).into_iter().filter(|n| n.kind() == "class_heritage") {
            for clause in named_children(heritage) {
                match clause.kind() {
                    "extends_clause" => {
                        if let Some(value) = clause.child_by_field_name("value")
                            && let Some(name) = dotted_name(value, source)
                        {
                            decl.extends.push(TypeRef { name, line: line(value) });
                        }
                        if let Some(args) = clause.child_by_field_name("type_arguments") {
                            walk(args, source, &mut decl.body);
                        }
                    }
                    "implements_clause" => {
                        for ty in named_children(clause) {
                            if let Some(tr) = type_ref(ty, source) {
                                decl.implements.push(tr);
                            }
                            walk_type_arguments(ty, source, &mut decl.body);
                        }
                    }
                    _ => {}
                }
            }
        }
        for decorator in named_children(node).into_iter().filter(|n| n.kind() == "decorator") {
            walk(decorator, source, &mut decl.body);
        }

        let mut member_ids = Vec::new();
        let mut by_name = HashMap::new();
        if let Some(body) = node.child_by_field_name("body") {
            for member in named_children(body) {
                let members = match member.kind() {
                    "method_definition" | "abstract_method_signature" | "method_signature" => {
                        let mut out = vec![method(member, source)];
                        if out[0].kind == SymbolKind::Constructor {
                            out.extend(parameter_properties(member, source));
                        }
                        out
                    }
                    "public_field_definition" => vec![field(member, source)],
                    "class_static_block" => {
                        walk(member, source, &mut decl.body);
                        Vec::new()
                    }
                    _ => Vec::new(),
                };
                if !members.is_empty() {
                    member_ids.push(member.id());
                }
                for m in members {
                    merge_member(&mut decl.members, &mut by_name, m);
                }
            }
        }
        decl.fingerprint = syntax::fingerprint(node, source, &member_ids, is_comment);
        self.owned_nodes.push(node.id());
        self.decls.push(decl);
    }

    fn interface(&mut self, node: Node<'_>, exported: bool, default_export: bool) {
        let source = self.source;
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let mut decl =
            self.empty_decl(node, text(name, source).to_owned(), SymbolKind::Interface, exported, default_export);
        for clause in named_children(node).into_iter().filter(|n| n.kind() == "extends_type_clause") {
            for ty in named_children(clause) {
                if let Some(tr) = type_ref(ty, source) {
                    decl.extends.push(tr);
                }
                walk_type_arguments(ty, source, &mut decl.body);
            }
        }
        let mut member_ids = Vec::new();
        let mut by_name = HashMap::new();
        if let Some(body) = node.child_by_field_name("body") {
            for member in named_children(body) {
                let kind = match member.kind() {
                    "method_signature" => SymbolKind::Method,
                    "property_signature" => SymbolKind::Field,
                    _ => continue,
                };
                let Some(member_name) = member.child_by_field_name("name") else {
                    continue;
                };
                member_ids.push(member.id());
                let mut body = Body::default();
                walk_signature_types(member, source, &mut body);
                let ty_field = if kind == SymbolKind::Method { "return_type" } else { "type" };
                merge_member(
                    &mut decl.members,
                    &mut by_name,
                    Member {
                        name: text(member_name, source).to_owned(),
                        kind,
                        is_static: false,
                        visibility: Visibility::Public,
                        span: span(member),
                        fingerprint: syntax::fingerprint(member, source, &[], is_comment),
                        ty: member.child_by_field_name(ty_field).and_then(|t| annotation_type(t, source)),
                        inferred_ty: None,
                        body,
                    },
                );
            }
        }
        decl.fingerprint = syntax::fingerprint(node, source, &member_ids, is_comment);
        self.owned_nodes.push(node.id());
        self.decls.push(decl);
    }

    /// Returns true when `call` registered a test or suite.
    fn test_call(&mut self, call: Node<'_>, titles: &[String], scope: &[Local], depth: usize) -> bool {
        if depth > MAX_SUITE_DEPTH {
            return false;
        }
        let source = self.source;
        let Some(root) = call.child_by_field_name("function").and_then(|f| root_identifier(f, source)) else {
            return false;
        };
        let is_suite = SUITE_FUNCTIONS.contains(&root);
        let is_test = TEST_FUNCTIONS.contains(&root);
        if !is_suite && !is_test {
            return false;
        }
        let args = call.child_by_field_name("arguments").map(named_children).unwrap_or_default();
        let title = args
            .first()
            .and_then(|a| string_value(*a, source))
            .unwrap_or_else(|| format!("<dynamic title at line {}>", line(call)));
        let callback = args.iter().find(|a| matches!(a.kind(), "arrow_function" | "function_expression"));
        let mut path = titles.to_vec();
        path.push(title);

        if is_test {
            let visible = &scope[scope.len().saturating_sub(MAX_SUITE_LOCALS)..];
            let mut body = Body { locals: visible.to_vec(), refs: Vec::new() };
            for arg in &args {
                walk(*arg, source, &mut body);
            }
            self.owned_nodes.push(call.id());
            self.tests.push(TestCase {
                title: path.join(" > "),
                span: span(call),
                fingerprint: syntax::fingerprint(call, source, &[], is_comment),
                body,
            });
            return true;
        }

        // Suite: statements of its callback are either nested tests/suites or suite-level code
        // (hooks, fixtures), which runs for every test in the file and is attributed to the file.
        let Some(callback) = callback else {
            return false;
        };
        let mut suite_locals = scope.to_vec();
        let Some(block) = callback.child_by_field_name("body") else {
            return false;
        };
        self.owned_nodes.push(call.id());
        for statement in named_children(block) {
            let nested = (statement.kind() == "expression_statement")
                .then(|| named_children(statement).into_iter().find(|n| n.kind() == "call_expression"))
                .flatten();
            let handled = nested.is_some_and(|c| self.test_call(c, &path, &suite_locals, depth + 1));
            if !handled {
                self.suite_code.push(syntax::fingerprint(statement, source, &[], is_comment));
                let mut body = Body::default();
                walk(statement, source, &mut body);
                suite_locals.extend(body.locals.iter().cloned());
                self.module_body.refs.extend(body.refs);
                self.module_body.locals.extend(body.locals);
            }
        }
        true
    }
}

fn is_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "function_signature"
            | "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "ambient_declaration"
    )
}

/// Overloads and get/set pairs share a name and therefore one identity: spans are united and
/// fingerprints combined so a change to any of them marks the member modified.
fn merge_member(members: &mut Vec<Member>, by_name: &mut HashMap<String, usize>, member: Member) {
    // Keyed by name only: a static and an instance member with the same name share one identity.
    // The index keeps this linear for classes with many members.
    let Some(&position) = by_name.get(&member.name) else {
        by_name.insert(member.name.clone(), members.len());
        members.push(member);
        return;
    };
    let existing = &mut members[position];
    let mut builder = ripplepath_core::FingerprintBuilder::new();
    builder.token(&existing.fingerprint.to_string());
    builder.token(&member.fingerprint.to_string());
    existing.fingerprint = builder.finish();
    existing.span.start_line = existing.span.start_line.min(member.span.start_line);
    existing.span.end_line = existing.span.end_line.max(member.span.end_line);
    existing.ty = existing.ty.take().or(member.ty);
    existing.body.locals.extend(member.body.locals);
    existing.body.refs.extend(member.body.refs);
    if member.kind == SymbolKind::Method {
        existing.kind = SymbolKind::Method;
    }
}

fn visibility_of(node: Node<'_>, name: Option<Node<'_>>, source: &str) -> Visibility {
    if name.is_some_and(|n| n.kind() == "private_property_identifier") {
        return Visibility::Private;
    }
    named_children(node).into_iter().find(|n| n.kind() == "accessibility_modifier").map_or(Visibility::Public, |m| {
        match text(m, source) {
            "private" => Visibility::Private,
            "protected" => Visibility::Protected,
            _ => Visibility::Public,
        }
    })
}

fn has_token(node: Node<'_>, token: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind() == token)
}

fn method(node: Node<'_>, source: &str) -> Member {
    let name_node = node.child_by_field_name("name");
    let name = name_node.map_or_else(String::new, |n| text(n, source).to_owned());
    let kind = if name == "constructor" { SymbolKind::Constructor } else { SymbolKind::Method };
    let mut body = Body::default();
    function_body(node, source, &mut body);
    for decorator in named_children(node).into_iter().filter(|n| n.kind() == "decorator") {
        walk(decorator, source, &mut body);
    }
    Member {
        name,
        kind,
        is_static: has_token(node, "static"),
        visibility: visibility_of(node, name_node, source),
        span: span(node),
        fingerprint: syntax::fingerprint(node, source, &[], is_comment),
        ty: node.child_by_field_name("return_type").and_then(|t| annotation_type(t, source)),
        inferred_ty: None,
        body,
    }
}

fn field(node: Node<'_>, source: &str) -> Member {
    let name_node = node.child_by_field_name("name");
    let mut body = Body::default();
    walk_children_except(node, source, &mut body, name_node.map(|n| n.id()));
    let value = node.child_by_field_name("value");
    // A field initialised with a function behaves like a method for callers.
    let is_function = value.is_some_and(|v| matches!(v.kind(), "arrow_function" | "function_expression"));
    Member {
        name: name_node.map_or_else(String::new, |n| text(n, source).to_owned()),
        kind: if is_function { SymbolKind::Method } else { SymbolKind::Field },
        is_static: has_token(node, "static"),
        visibility: visibility_of(node, name_node, source),
        span: span(node),
        fingerprint: syntax::fingerprint(node, source, &[], is_comment),
        ty: node.child_by_field_name("type").and_then(|t| annotation_type(t, source)),
        inferred_ty: value.and_then(|v| new_type(v, source)),
        body,
    }
}

/// `constructor(private readonly repo: Repo)` declares a field `repo` — the dominant injection
/// style in Angular/NestJS code, so missing it would hide most service-to-service edges.
fn parameter_properties(constructor: Node<'_>, source: &str) -> Vec<Member> {
    let Some(params) = constructor.child_by_field_name("parameters") else {
        return Vec::new();
    };
    named_children(params)
        .into_iter()
        .filter(|p| matches!(p.kind(), "required_parameter" | "optional_parameter"))
        .filter(|p| {
            has_token(*p, "readonly") || named_children(*p).iter().any(|n| n.kind() == "accessibility_modifier")
        })
        .filter_map(|p| {
            let pattern = p.child_by_field_name("pattern").filter(|n| n.kind() == "identifier")?;
            Some(Member {
                name: text(pattern, source).to_owned(),
                kind: SymbolKind::Field,
                is_static: false,
                visibility: visibility_of(p, None, source),
                span: span(p),
                fingerprint: syntax::fingerprint(p, source, &[], is_comment),
                ty: p.child_by_field_name("type").and_then(|t| annotation_type(t, source)),
                inferred_ty: None,
                body: Body::default(),
            })
        })
        .collect()
}

fn type_params(node: Node<'_>, source: &str) -> Vec<String> {
    node.child_by_field_name("type_parameters")
        .map(|params| {
            named_children(params)
                .into_iter()
                .filter_map(|p| p.child_by_field_name("name"))
                .map(|n| text(n, source).to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Locals and references of a function-like node: parameters, return type and body.
fn function_body(node: Node<'_>, source: &str, body: &mut Body) {
    body.locals.extend(type_params(node, source).into_iter().map(|name| Local { name, ty: None }));
    if let Some(param) = node.child_by_field_name("parameter") {
        body.locals.push(Local { name: text(param, source).to_owned(), ty: None });
    }
    for field in ["parameters", "return_type", "body"] {
        if let Some(child) = node.child_by_field_name(field) {
            walk(child, source, body);
        }
    }
}

/// Erased name of a type node, or `None` for unions, literals, functions and other shapes that do
/// not name one declaration.
fn type_ref(node: Node<'_>, source: &str) -> Option<TypeRef> {
    let name = match node.kind() {
        "type_identifier" => text(node, source).to_owned(),
        "nested_type_identifier" => compact(text(node, source)),
        "generic_type" => return node.child_by_field_name("name").and_then(|n| type_ref(n, source)),
        _ => return None,
    };
    Some(TypeRef { name, line: line(node) })
}

fn annotation_type(annotation: Node<'_>, source: &str) -> Option<TypeRef> {
    named_children(annotation).into_iter().find_map(|t| type_ref(t, source))
}

fn compact(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `Foo` / `ns.Foo` written as an expression (class `extends` clauses take expressions).
/// Iterative and capped: the chain length is attacker-controlled.
fn dotted_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut current = node;
    for _ in 0..=MAX_EXPR_DEPTH {
        match current.kind() {
            "identifier" => {
                parts.push(text(current, source));
                parts.reverse();
                return Some(parts.join("."));
            }
            "member_expression" => {
                parts.push(text(current.child_by_field_name("property")?, source));
                current = current.child_by_field_name("object")?;
            }
            _ => return None,
        }
    }
    None
}

fn new_type(value: Node<'_>, source: &str) -> Option<TypeRef> {
    if value.kind() != "new_expression" {
        return None;
    }
    let ctor = value.child_by_field_name("constructor")?;
    Some(TypeRef { name: dotted_name(ctor, source)?, line: line(ctor) })
}

fn root_identifier<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    let mut current = node;
    for _ in 0..MAX_EXPR_DEPTH {
        current = match current.kind() {
            "identifier" => return Some(text(current, source)),
            "member_expression" => current.child_by_field_name("object")?,
            "call_expression" => current.child_by_field_name("function")?,
            _ => return None,
        };
    }
    None
}

fn expr_of(node: Node<'_>, source: &str, depth: usize) -> Expr {
    if depth > MAX_EXPR_DEPTH {
        return Expr::Unknown;
    }
    let next = depth + 1;
    match node.kind() {
        "identifier" => Expr::Ident(text(node, source).to_owned()),
        "this" => Expr::This,
        "super" => Expr::Super,
        "member_expression" => match (node.child_by_field_name("object"), node.child_by_field_name("property")) {
            (Some(object), Some(property)) => {
                Expr::Member(Box::new(expr_of(object, source, next)), text(property, source).to_owned())
            }
            _ => Expr::Unknown,
        },
        "call_expression" => node
            .child_by_field_name("function")
            .map_or(Expr::Unknown, |f| Expr::CallResult(Box::new(expr_of(f, source, next)))),
        "new_expression" => node
            .child_by_field_name("constructor")
            .map_or(Expr::Unknown, |c| Expr::New(Box::new(expr_of(c, source, next)))),
        "parenthesized_expression"
        | "non_null_expression"
        | "await_expression"
        | "as_expression"
        | "satisfies_expression" => named_children(node)
            .into_iter()
            .find(|n| !is_comment(n.kind()) && n.kind() != "type_annotation")
            .map_or(Expr::Unknown, |inner| expr_of(inner, source, next)),
        _ => Expr::Unknown,
    }
}

/// Identifiers in these (parent, field) positions declare names rather than read them.
fn is_binding_position(parent: &str, field: Option<&str>) -> bool {
    matches!(
        (parent, field),
        ("variable_declarator", Some("name"))
            | ("required_parameter", Some("pattern"))
            | ("optional_parameter", Some("pattern"))
            | ("arrow_function", Some("parameter"))
            | ("function_declaration", Some("name"))
            | ("function_expression", Some("name"))
            | ("generator_function_declaration", Some("name"))
            | ("catch_clause", Some("parameter"))
            | ("for_in_statement", Some("left"))
            | ("labeled_statement", Some("label"))
            | ("assignment_pattern", Some("left"))
            | ("pair_pattern", Some("value"))
    ) || matches!(
        parent,
        "import_specifier"
            | "export_specifier"
            | "namespace_import"
            | "import_clause"
            | "object_pattern"
            | "array_pattern"
            | "rest_pattern"
            | "break_statement"
            | "continue_statement"
            | "nested_identifier"
            | "nested_type_identifier"
    )
}

fn is_type_declaration_name(parent: &str, field: Option<&str>) -> bool {
    field == Some("name")
        && matches!(
            parent,
            "class_declaration"
                | "abstract_class_declaration"
                | "class"
                | "interface_declaration"
                | "type_alias_declaration"
                | "type_parameter"
        )
}

/// Names bound by a destructuring pattern, without types.
fn pattern_names(pattern: Node<'_>, source: &str, out: &mut Vec<Local>) {
    let mut stack = vec![pattern];
    let mut found = 0;
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "identifier" | "shorthand_property_identifier_pattern") {
            out.push(Local { name: text(node, source).to_owned(), ty: None });
            found += 1;
            if found >= MAX_PATTERN_NAMES {
                return;
            }
            continue;
        }
        // Default values inside patterns are expressions, not bindings.
        if node.kind() == "assignment_pattern" {
            if let Some(left) = node.child_by_field_name("left") {
                stack.push(left);
            }
            continue;
        }
        if node.kind() == "pair_pattern" {
            if let Some(value) = node.child_by_field_name("value") {
                stack.push(value);
            }
            continue;
        }
        stack.extend(named_children(node));
    }
}

fn walk_type_arguments(node: Node<'_>, source: &str, body: &mut Body) {
    if let Some(args) = node.child_by_field_name("type_arguments") {
        walk(args, source, body);
    }
}

fn walk_signature_types(node: Node<'_>, source: &str, body: &mut Body) {
    for field in ["parameters", "return_type", "type"] {
        if let Some(child) = node.child_by_field_name(field) {
            walk(child, source, body);
        }
    }
}

fn walk_children_except(node: Node<'_>, source: &str, body: &mut Body, except: Option<usize>) {
    for child in named_children(node) {
        if Some(child.id()) != except {
            walk(child, source, body);
        }
    }
}

struct Frame<'t> {
    node: Node<'t>,
    parent: &'t str,
    field: Option<&'t str>,
    /// Inside the object part of a member chain: the chain itself is recorded once, by the
    /// outermost call or read, so inner identifiers must not be recorded again.
    in_chain: bool,
}

/// Records references and locals under `root`. Iterative so that deeply nested expressions cannot
/// exhaust the stack.
fn walk<'t>(root: Node<'t>, source: &str, body: &mut Body) {
    let mut stack = vec![Frame { node: root, parent: "", field: None, in_chain: false }];
    while let Some(Frame { node, parent, field, in_chain }) = stack.pop() {
        let kind = node.kind();
        let mut chain_child: Option<usize> = None;
        let mut skip_child: Option<usize> = None;
        let mut children_in_chain = false;
        match kind {
            "comment" => continue,
            "call_expression" => {
                if let Some(callee) = node.child_by_field_name("function") {
                    if callee.kind() != "import" {
                        body.refs.push(Ref::Call { callee: expr_of(callee, source, 0), line: line(node) });
                    }
                    match callee.kind() {
                        "member_expression" => chain_child = Some(callee.id()),
                        "call_expression" => chain_child = Some(callee.id()),
                        _ => skip_child = Some(callee.id()),
                    }
                }
            }
            "new_expression" => {
                if let Some(ctor) = node.child_by_field_name("constructor") {
                    body.refs.push(Ref::New { ctor: expr_of(ctor, source, 0), line: line(node) });
                    match ctor.kind() {
                        "member_expression" => chain_child = Some(ctor.id()),
                        _ => skip_child = Some(ctor.id()),
                    }
                }
            }
            "member_expression" => {
                if !in_chain {
                    body.refs.push(Ref::Read { expr: expr_of(node, source, 0), line: line(node) });
                }
                children_in_chain = true;
            }
            "identifier" => {
                if !in_chain && !is_binding_position(parent, field) {
                    body.refs.push(Ref::Read { expr: Expr::Ident(text(node, source).to_owned()), line: line(node) });
                }
                continue;
            }
            "shorthand_property_identifier" => {
                body.refs.push(Ref::Read { expr: Expr::Ident(text(node, source).to_owned()), line: line(node) });
                continue;
            }
            "parenthesized_expression"
            | "non_null_expression"
            | "await_expression"
            | "as_expression"
            | "satisfies_expression" => children_in_chain = in_chain,
            "type_identifier" => {
                if !is_type_declaration_name(parent, field)
                    && let Some(tr) = type_ref(node, source)
                {
                    body.refs.push(Ref::Type(tr));
                }
                continue;
            }
            "nested_type_identifier" => {
                if let Some(tr) = type_ref(node, source) {
                    body.refs.push(Ref::Type(tr));
                }
                continue;
            }
            "jsx_opening_element" | "jsx_self_closing_element" => {
                // Rendering `<Component/>` runs the component; lowercase names are DOM elements.
                if let Some(name) = node.child_by_field_name("name") {
                    let is_component = text(name, source).chars().next().is_some_and(char::is_uppercase);
                    if is_component {
                        body.refs.push(Ref::Call { callee: expr_of(name, source, 0), line: line(node) });
                    }
                    skip_child = Some(name.id());
                }
            }
            "jsx_closing_element" => continue,
            "variable_declarator" => {
                if let Some(name) = node.child_by_field_name("name") {
                    if name.kind() == "identifier" {
                        let declared = node.child_by_field_name("type").and_then(|t| annotation_type(t, source));
                        let inferred = node.child_by_field_name("value").and_then(|v| new_type(v, source));
                        body.locals.push(Local { name: text(name, source).to_owned(), ty: declared.or(inferred) });
                    } else {
                        pattern_names(name, source, &mut body.locals);
                        skip_child = Some(name.id());
                    }
                }
            }
            "required_parameter" | "optional_parameter" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    if pattern.kind() == "identifier" {
                        let ty = node.child_by_field_name("type").and_then(|t| annotation_type(t, source));
                        body.locals.push(Local { name: text(pattern, source).to_owned(), ty });
                    } else {
                        pattern_names(pattern, source, &mut body.locals);
                        skip_child = Some(pattern.id());
                    }
                }
            }
            "arrow_function" => {
                if let Some(param) = node.child_by_field_name("parameter") {
                    body.locals.push(Local { name: text(param, source).to_owned(), ty: None });
                }
            }
            "catch_clause" => {
                if let Some(param) = node.child_by_field_name("parameter") {
                    pattern_names(param, source, &mut body.locals);
                    skip_child = Some(param.id());
                }
            }
            "for_in_statement" => {
                if let Some(left) = node.child_by_field_name("left") {
                    pattern_names(left, source, &mut body.locals);
                    skip_child = Some(left.id());
                }
            }
            "function_declaration" | "function_expression" | "generator_function_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    body.locals.push(Local { name: text(name, source).to_owned(), ty: None });
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.is_named() && skip_child != Some(child.id()) {
                    let child_field = cursor.field_name();
                    let child_in_chain = if chain_child == Some(child.id()) {
                        // For a member callee, the chain starts at its object.
                        if child.kind() == "member_expression" {
                            if let Some(object) = child.child_by_field_name("object") {
                                stack.push(Frame {
                                    node: object,
                                    parent: child.kind(),
                                    field: Some("object"),
                                    in_chain: true,
                                });
                            }
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                            continue;
                        }
                        true
                    } else {
                        children_in_chain && child_field != Some("property")
                    };
                    stack.push(Frame { node: child, parent: kind, field: child_field, in_chain: child_in_chain });
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_test_path;

    #[test]
    fn recognises_test_file_names() {
        for path in ["src/cart.test.ts", "a/b.spec.tsx", "x/__tests__/y.ts", "util.test.js"] {
            assert!(is_test_path(path), "{path}");
        }
        for path in ["src/test.ts", "src/latest.ts", "spec/helper.ts", "src/contest.ts"] {
            assert!(!is_test_path(path), "{path}");
        }
    }
}
