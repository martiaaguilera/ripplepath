use std::time::Duration;

use ripplepath_core::{SymbolKind, Visibility};
use tree_sitter::Node;

use super::facts::{
    BodyRef, FieldDecl, Import, JavaFile, Local, LocalInit, MethodDecl, Param, Receiver, TypeDecl, TypeUse,
};
use crate::syntax::{self, ParseError, line, named_children, span, text};

/// Nested type declarations deeper than this are not indexed. Real code rarely nests beyond 3;
/// the cap only exists so a crafted file cannot drive unbounded recursion.
const MAX_TYPE_NESTING: usize = 32;
/// Receiver chains (`a.b().c().d()...`) longer than this resolve to `Unknown`, for the same reason.
const MAX_RECEIVER_DEPTH: usize = 64;

pub fn extract(path: &str, source: &str, budget: Duration) -> Result<JavaFile, ParseError> {
    let tree = syntax::parse(&tree_sitter_java::LANGUAGE.into(), source, budget)?;
    let root = tree.root_node();

    let mut package = None;
    let mut imports = Vec::new();
    let mut types = Vec::new();
    let mut type_node_ids = Vec::new();
    for child in named_children(root) {
        match child.kind() {
            "package_declaration" => {
                package = named_children(child)
                    .into_iter()
                    .find(|n| matches!(n.kind(), "scoped_identifier" | "identifier"))
                    .map(|n| compact(text(n, source)));
            }
            "import_declaration" => imports.push(import(child, source)),
            kind if is_type_declaration(kind) => {
                type_node_ids.push(child.id());
                let cx = TypeContext { prefix: None, in_interface: false, depth: 0 };
                extract_type(child, source, &cx, &mut types);
            }
            _ => {}
        }
    }

    Ok(JavaFile {
        path: path.to_owned(),
        package,
        imports,
        types,
        header_fingerprint: syntax::fingerprint(root, source, &type_node_ids, is_comment),
        line_count: source.lines().count() as u32,
        syntax_error_lines: syntax::syntax_error_lines(&tree),
    })
}

fn is_comment(kind: &str) -> bool {
    matches!(kind, "line_comment" | "block_comment")
}

fn is_type_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
    )
}

/// Qualified names may legally contain whitespace and comments between segments.
fn compact(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_whitespace()).collect()
}

fn import(node: Node<'_>, source: &str) -> Import {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    let path = children
        .iter()
        .find(|n| matches!(n.kind(), "scoped_identifier" | "identifier"))
        .map(|n| compact(text(*n, source)))
        .unwrap_or_default();
    Import {
        path,
        is_static: children.iter().any(|n| n.kind() == "static"),
        wildcard: children.iter().any(|n| n.kind() == "asterisk"),
        line: line(node),
    }
}

struct TypeContext<'a> {
    prefix: Option<&'a str>,
    in_interface: bool,
    depth: usize,
}

struct Modifiers {
    visibility: Option<Visibility>,
    is_static: bool,
    annotations: Vec<TypeUse>,
}

fn modifiers(node: Node<'_>, source: &str) -> Modifiers {
    let mut result = Modifiers { visibility: None, is_static: false, annotations: Vec::new() };
    let Some(mods) = named_children(node).into_iter().find(|n| n.kind() == "modifiers") else {
        return result;
    };
    let mut cursor = mods.walk();
    for child in mods.children(&mut cursor) {
        match child.kind() {
            "public" => result.visibility = Some(Visibility::Public),
            "protected" => result.visibility = Some(Visibility::Protected),
            "private" => result.visibility = Some(Visibility::Private),
            "static" => result.is_static = true,
            "marker_annotation" | "annotation" => {
                if let Some(name) = child.child_by_field_name("name") {
                    result.annotations.push(TypeUse {
                        name: compact(text(name, source)),
                        line: line(child),
                        args: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }
    result
}

fn extract_type(node: Node<'_>, source: &str, cx: &TypeContext<'_>, out: &mut Vec<TypeDecl>) {
    if cx.depth > MAX_TYPE_NESTING {
        return;
    }
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let simple = text(name_node, source);
    let name = match cx.prefix {
        Some(prefix) => format!("{prefix}.{simple}"),
        None => simple.to_owned(),
    };
    let kind = match node.kind() {
        "interface_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        "record_declaration" => SymbolKind::Record,
        "annotation_type_declaration" => SymbolKind::Annotation,
        _ => SymbolKind::Class,
    };
    let is_interface = matches!(kind, SymbolKind::Interface | SymbolKind::Annotation);
    let mods = modifiers(node, source);
    let default_visibility = if cx.in_interface { Visibility::Public } else { Visibility::Package };

    let mut extends = Vec::new();
    let mut implements = Vec::new();
    if let Some(superclass) = node.child_by_field_name("superclass") {
        extends.extend(named_children(superclass).into_iter().filter_map(|t| type_use(t, source)));
    }
    if let Some(interfaces) = node.child_by_field_name("interfaces") {
        implements.extend(type_list(interfaces, source));
    }
    for child in named_children(node) {
        if child.kind() == "extends_interfaces" {
            extends.extend(type_list(child, source));
        }
    }

    let mut decl = TypeDecl {
        name: name.clone(),
        kind,
        visibility: mods.visibility.unwrap_or(default_visibility),
        span: span(node),
        fingerprint: syntax::fingerprint(node, source, &[], is_comment),
        type_params: type_params(node, source),
        annotations: mods.annotations,
        extends,
        implements,
        fields: Vec::new(),
        methods: Vec::new(),
        refs: Vec::new(),
    };

    if kind == SymbolKind::Record
        && let Some(params) = node.child_by_field_name("parameters")
    {
        // Each component is fingerprinted on its own declaration. Using the whole record's
        // fingerprint marked every component modified (and every accessor caller impacted) when
        // an unrelated method was added to the record.
        for (param, component) in parameter_nodes(params, source).0 {
            decl.fields.push(FieldDecl {
                name: param.name,
                ty: param.ty,
                visibility: Visibility::Private,
                is_static: false,
                span: span(component),
                fingerprint: syntax::fingerprint(component, source, &[], is_comment),
                refs: Vec::new(),
            });
        }
    }

    let mut member_ids = Vec::new();
    let mut nested = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let member_cx = MemberContext { type_name: &name, in_interface: is_interface, source };
        let mut members = named_children(body);
        // Enum members after the constants live in a nested `enum_body_declarations` node.
        if let Some(pos) = members.iter().position(|n| n.kind() == "enum_body_declarations") {
            let declarations = members.remove(pos);
            members.extend(named_children(declarations));
        }
        for member in members {
            match member.kind() {
                "field_declaration" | "constant_declaration" => {
                    member_ids.push(member.id());
                    decl.fields.extend(fields(member, &member_cx));
                }
                "enum_constant" => {
                    member_ids.push(member.id());
                    decl.fields.push(enum_constant(member, &member_cx));
                }
                "method_declaration" | "annotation_type_element_declaration" => {
                    member_ids.push(member.id());
                    decl.methods.push(method(member, false, &member_cx));
                }
                "constructor_declaration" | "compact_constructor_declaration" => {
                    member_ids.push(member.id());
                    let mut ctor = method(member, true, &member_cx);
                    if member.kind() == "compact_constructor_declaration" {
                        // A compact constructor takes the record components as parameters.
                        if let Some(params) = node.child_by_field_name("parameters") {
                            ctor.params = parameters(params, source).0;
                        }
                    }
                    decl.methods.push(ctor);
                }
                "static_initializer" | "block" => {
                    walk_body(member, source, &mut decl.refs, &mut Vec::new());
                }
                kind if is_type_declaration(kind) => {
                    member_ids.push(member.id());
                    nested.push(member);
                }
                _ => {}
            }
        }
    }
    decl.fingerprint = syntax::fingerprint(node, source, &member_ids, is_comment);
    out.push(decl);

    let nested_cx = TypeContext { prefix: Some(&name), in_interface: is_interface, depth: cx.depth + 1 };
    for member in nested {
        extract_type(member, source, &nested_cx, out);
    }
}

struct MemberContext<'a> {
    type_name: &'a str,
    in_interface: bool,
    source: &'a str,
}

fn fields(node: Node<'_>, cx: &MemberContext<'_>) -> Vec<FieldDecl> {
    let source = cx.source;
    let mods = modifiers(node, source);
    let Some(ty) = node.child_by_field_name("type").and_then(|t| type_use(t, source)).or_else(|| {
        // Primitive fields have no type to resolve but are still symbols worth tracking.
        node.child_by_field_name("type").map(|t| TypeUse {
            name: text(t, source).to_owned(),
            line: line(t),
            args: Vec::new(),
        })
    }) else {
        return Vec::new();
    };
    let fingerprint = syntax::fingerprint(node, source, &[], is_comment);
    let mut cursor = node.walk();
    let declarators: Vec<Node<'_>> = node.children_by_field_name("declarator", &mut cursor).collect();
    declarators
        .into_iter()
        .filter_map(|declarator| {
            let name = text(declarator.child_by_field_name("name")?, source).to_owned();
            let mut refs = Vec::new();
            if let Some(value) = declarator.child_by_field_name("value") {
                walk_body(value, source, &mut refs, &mut Vec::new());
            }
            Some(FieldDecl {
                name,
                ty: ty.clone(),
                visibility: mods.visibility.unwrap_or(if cx.in_interface {
                    Visibility::Public
                } else {
                    Visibility::Package
                }),
                is_static: mods.is_static || cx.in_interface,
                span: span(node),
                fingerprint,
                refs,
            })
        })
        .collect()
}

fn enum_constant(node: Node<'_>, cx: &MemberContext<'_>) -> FieldDecl {
    let source = cx.source;
    let name = node.child_by_field_name("name").map(|n| text(n, source)).unwrap_or_default();
    let enum_simple = cx.type_name.rsplit('.').next().unwrap_or(cx.type_name);
    let mut refs = Vec::new();
    let arity = node.child_by_field_name("arguments").map_or(0, argument_count);
    refs.push(BodyRef::New { ty: TypeUse { name: enum_simple.to_owned(), line: line(node), args: Vec::new() }, arity });
    if let Some(arguments) = node.child_by_field_name("arguments") {
        walk_body(arguments, source, &mut refs, &mut Vec::new());
    }
    if let Some(body) = node.child_by_field_name("body") {
        walk_body(body, source, &mut refs, &mut Vec::new());
    }
    FieldDecl {
        name: name.to_owned(),
        ty: TypeUse { name: enum_simple.to_owned(), line: line(node), args: Vec::new() },
        visibility: Visibility::Public,
        is_static: true,
        span: span(node),
        fingerprint: syntax::fingerprint(node, source, &[], is_comment),
        refs,
    }
}

fn method(node: Node<'_>, is_constructor: bool, cx: &MemberContext<'_>) -> MethodDecl {
    let source = cx.source;
    let mods = modifiers(node, source);
    let name = if is_constructor {
        "<init>".to_owned()
    } else {
        node.child_by_field_name("name").map(|n| text(n, source).to_owned()).unwrap_or_default()
    };
    let (params, is_varargs) =
        node.child_by_field_name("parameters").map(|p| parameters(p, source)).unwrap_or_default();
    let return_type =
        if is_constructor { None } else { node.child_by_field_name("type").and_then(|t| type_use(t, source)) };
    let throws = named_children(node)
        .into_iter()
        .filter(|n| n.kind() == "throws")
        .flat_map(|n| named_children(n).into_iter().filter_map(|t| type_use(t, source)).collect::<Vec<_>>())
        .collect();
    // Interface methods without a body are implicitly public; with `private` they are not.
    let default_visibility = if cx.in_interface { Visibility::Public } else { Visibility::Package };

    let mut locals: Vec<Local> =
        params.iter().map(|p| Local { name: p.name.clone(), ty: Some(p.ty.clone()), init: None }).collect();
    let mut refs = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        walk_body(body, source, &mut refs, &mut locals);
    }

    MethodDecl {
        name,
        is_constructor,
        is_static: mods.is_static,
        is_varargs,
        visibility: mods.visibility.unwrap_or(default_visibility),
        type_params: type_params(node, source),
        params,
        return_type,
        throws,
        annotations: mods.annotations,
        span: span(node),
        fingerprint: syntax::fingerprint(node, source, &[], is_comment),
        locals,
        refs,
    }
}

fn parameters(node: Node<'_>, source: &str) -> (Vec<Param>, bool) {
    let (params, varargs) = parameter_nodes(node, source);
    (params.into_iter().map(|(param, _)| param).collect(), varargs)
}

/// Parameters with the syntax node each came from.
fn parameter_nodes<'t>(node: Node<'t>, source: &str) -> (Vec<(Param, Node<'t>)>, bool) {
    let mut params = Vec::new();
    let mut varargs = false;
    for child in named_children(node) {
        match child.kind() {
            "formal_parameter" => {
                let (Some(ty_node), Some(name_node)) =
                    (child.child_by_field_name("type"), child.child_by_field_name("name"))
                else {
                    continue;
                };
                let dims = child.child_by_field_name("dimensions").map_or(0, |d| text(d, source).matches('[').count());
                params.push((
                    Param {
                        name: text(name_node, source).to_owned(),
                        ty: type_use(ty_node, source).unwrap_or(TypeUse {
                            name: text(ty_node, source).to_owned(),
                            line: line(ty_node),
                            args: Vec::new(),
                        }),
                        signature_text: format!("{}{}", signature_text(ty_node, source), "[]".repeat(dims)),
                    },
                    child,
                ));
            }
            "spread_parameter" => {
                varargs = true;
                let children = named_children(child);
                let Some(ty_node) = children.iter().find(|n| is_type_node(n.kind())) else {
                    continue;
                };
                let name = children
                    .iter()
                    .find(|n| n.kind() == "variable_declarator")
                    .and_then(|d| d.child_by_field_name("name"))
                    .map(|n| text(n, source).to_owned())
                    .unwrap_or_default();
                params.push((
                    Param {
                        name,
                        ty: type_use(*ty_node, source).unwrap_or(TypeUse {
                            name: text(*ty_node, source).to_owned(),
                            line: line(*ty_node),
                            args: Vec::new(),
                        }),
                        signature_text: format!("{}...", signature_text(*ty_node, source)),
                    },
                    child,
                ));
            }
            _ => {}
        }
    }
    (params, varargs)
}

fn type_params(node: Node<'_>, source: &str) -> Vec<String> {
    let Some(params) = node.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    named_children(params)
        .into_iter()
        .filter_map(|p| named_children(p).into_iter().find(|n| matches!(n.kind(), "type_identifier" | "identifier")))
        .map(|n| text(n, source).to_owned())
        .collect()
}

fn type_list(node: Node<'_>, source: &str) -> Vec<TypeUse> {
    named_children(node)
        .into_iter()
        .flat_map(|list| if list.kind() == "type_list" { named_children(list) } else { vec![list] })
        .filter_map(|t| type_use(t, source))
        .collect()
}

fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "type_identifier"
            | "scoped_type_identifier"
            | "generic_type"
            | "array_type"
            | "integral_type"
            | "floating_point_type"
            | "boolean_type"
            | "void_type"
            | "annotated_type"
    )
}

/// Erased, dotted type name, or `None` for primitives/void/`var` which never resolve to a symbol.
fn erase(node: Node<'_>, source: &str) -> Option<String> {
    erase_bounded(node, source, 0)
}

/// Qualified type names nest left-recursively (`a.b.c.D` is four levels), so the recursion depth is
/// attacker-controlled. Real names are a handful of segments; anything deeper than the receiver
/// limit is treated as unresolvable rather than risking a stack overflow.
fn erase_bounded(node: Node<'_>, source: &str, depth: usize) -> Option<String> {
    if depth > MAX_RECEIVER_DEPTH {
        return None;
    }
    let next = depth + 1;
    match node.kind() {
        "type_identifier" => {
            let name = text(node, source);
            (name != "var").then(|| name.to_owned())
        }
        "scoped_type_identifier" => {
            let mut parts = Vec::new();
            for child in named_children(node) {
                if matches!(child.kind(), "annotation" | "marker_annotation") {
                    continue;
                }
                parts.push(erase_bounded(child, source, next)?);
            }
            (!parts.is_empty()).then(|| parts.join("."))
        }
        "generic_type" => named_children(node)
            .into_iter()
            .find(|n| matches!(n.kind(), "type_identifier" | "scoped_type_identifier"))
            .and_then(|n| erase_bounded(n, source, next)),
        "array_type" => node.child_by_field_name("element").and_then(|n| erase_bounded(n, source, next)),
        "annotated_type" => named_children(node).into_iter().last().and_then(|n| erase_bounded(n, source, next)),
        _ => None,
    }
}

fn type_use(node: Node<'_>, source: &str) -> Option<TypeUse> {
    erase(node, source).map(|name| TypeUse { name, line: line(node), args: type_args(node, source) })
}

/// Erased top-level type arguments; see `TypeUse::args`.
fn type_args(node: Node<'_>, source: &str) -> Vec<String> {
    let generic = match node.kind() {
        "annotated_type" => named_children(node).into_iter().last().filter(|n| n.kind() == "generic_type"),
        "generic_type" => Some(node),
        _ => None,
    };
    let Some(arguments) = generic.and_then(|g| named_children(g).into_iter().find(|n| n.kind() == "type_arguments"))
    else {
        return Vec::new();
    };
    named_children(arguments)
        .into_iter()
        .filter(|n| !is_comment(n.kind()) && !matches!(n.kind(), "annotation" | "marker_annotation"))
        .map(|arg| match arg.kind() {
            // `? extends X` reads as an X; `?` and `? super X` give no usable element type.
            "wildcard" if has_child_kind(arg, "extends") => {
                named_children(arg).into_iter().last().and_then(|bound| erase(bound, source)).unwrap_or_default()
            }
            // Arrays erase to their element type elsewhere, which would mistype `List<Job[]>`.
            "wildcard" | "array_type" => String::new(),
            _ => erase(arg, source).unwrap_or_default(),
        })
        .collect()
}

fn has_child_kind(node: Node<'_>, kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| c.kind() == kind)
}

/// Parameter type text for symbol identity: erased, but keeping array dimensions because
/// `f(String)` and `f(String[])` are distinct overloads.
fn signature_text(node: Node<'_>, source: &str) -> String {
    match node.kind() {
        "array_type" => {
            // The element of an array type is never itself an array type in this grammar (extra
            // dimensions live in `dimensions`), so this does not recurse further.
            let element = node
                .child_by_field_name("element")
                .map(|n| erase(n, source).unwrap_or_else(|| compact(text(n, source))))
                .unwrap_or_default();
            let dims = node.child_by_field_name("dimensions").map_or(1, |d| text(d, source).matches('[').count());
            format!("{element}{}", "[]".repeat(dims))
        }
        "integral_type" | "floating_point_type" | "boolean_type" | "void_type" => text(node, source).to_owned(),
        _ => erase(node, source).unwrap_or_else(|| compact(text(node, source))),
    }
}

fn argument_count(arguments: Node<'_>) -> u32 {
    named_children(arguments).into_iter().filter(|n| !is_comment(n.kind())).count() as u32
}

fn receiver(node: Node<'_>, source: &str, depth: usize) -> Receiver {
    if depth > MAX_RECEIVER_DEPTH {
        return Receiver::Unknown;
    }
    match node.kind() {
        "this" => Receiver::This,
        "super" => Receiver::Super,
        "identifier" => Receiver::Name(text(node, source).to_owned()),
        "field_access" => match (node.child_by_field_name("object"), node.child_by_field_name("field")) {
            (Some(object), Some(field)) => {
                Receiver::Field(Box::new(receiver(object, source, depth + 1)), text(field, source).to_owned())
            }
            _ => Receiver::Unknown,
        },
        "method_invocation" => {
            let Some(name) = node.child_by_field_name("name") else {
                return Receiver::Unknown;
            };
            let inner =
                node.child_by_field_name("object").map_or(Receiver::Implicit, |o| receiver(o, source, depth + 1));
            Receiver::Call {
                receiver: Box::new(inner),
                name: text(name, source).to_owned(),
                arity: node.child_by_field_name("arguments").map_or(0, argument_count),
            }
        }
        "object_creation_expression" => {
            node.child_by_field_name("type").and_then(|t| erase(t, source)).map_or(Receiver::Unknown, Receiver::New)
        }
        "cast_expression" => {
            node.child_by_field_name("type").and_then(|t| erase(t, source)).map_or(Receiver::Unknown, Receiver::New)
        }
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .find(|n| !is_comment(n.kind()))
            .map_or(Receiver::Unknown, |inner| receiver(inner, source, depth + 1)),
        _ => Receiver::Unknown,
    }
}

/// Is an identifier with this parent/field an expression that reads a variable?
fn is_value_identifier(parent: &str, field: Option<&str>) -> bool {
    match parent {
        "assignment_expression"
        | "binary_expression"
        | "unary_expression"
        | "update_expression"
        | "argument_list"
        | "return_statement"
        | "parenthesized_expression"
        | "array_access"
        | "ternary_expression"
        | "expression_statement"
        | "array_initializer"
        | "throw_statement"
        | "switch_label"
        | "element_value_pair"
        | "synchronized_statement" => true,
        "variable_declarator" => field == Some("value"),
        "cast_expression" => field == Some("value"),
        "instanceof_expression" => field == Some("left"),
        "lambda_expression" => field == Some("body"),
        "enhanced_for_statement" => field == Some("value"),
        _ => false,
    }
}

/// Collects references and local declarations under `root` without recursion, so that deeply
/// nested expressions in hostile input cannot overflow the stack.
fn walk_body<'tree>(root: Node<'tree>, source: &str, refs: &mut Vec<BodyRef>, locals: &mut Vec<Local>) {
    let mut stack: Vec<(Node<'tree>, &'tree str, Option<&'tree str>)> = vec![(root, "", None)];
    while let Some((node, parent, field)) = stack.pop() {
        let mut skip_child: Option<usize> = None;
        let mut replace_child: Option<(usize, Node<'tree>)> = None;
        match node.kind() {
            "method_invocation" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let receiver_node = node.child_by_field_name("object");
                    refs.push(BodyRef::Call {
                        receiver: receiver_node.map_or(Receiver::Implicit, |o| receiver(o, source, 0)),
                        name: text(name, source).to_owned(),
                        arity: node.child_by_field_name("arguments").map_or(0, argument_count),
                        line: line(node),
                    });
                }
            }
            "explicit_constructor_invocation" => {
                let on_super = node.child_by_field_name("constructor").is_some_and(|c| c.kind() == "super");
                refs.push(BodyRef::ConstructorCall {
                    on_super,
                    arity: node.child_by_field_name("arguments").map_or(0, argument_count),
                    line: line(node),
                });
            }
            "object_creation_expression" => {
                if let Some(ty_node) = node.child_by_field_name("type") {
                    if let Some(ty) = type_use(ty_node, source) {
                        refs.push(BodyRef::New {
                            ty,
                            arity: node.child_by_field_name("arguments").map_or(0, argument_count),
                        });
                    }
                    // The created type is already an INSTANTIATES reference; only its generic
                    // arguments still need visiting.
                    match named_children(ty_node).into_iter().find(|n| n.kind() == "type_arguments") {
                        Some(args) => replace_child = Some((ty_node.id(), args)),
                        None => skip_child = Some(ty_node.id()),
                    }
                }
            }
            "field_access" => {
                if let (Some(object), Some(field_node)) =
                    (node.child_by_field_name("object"), node.child_by_field_name("field"))
                {
                    refs.push(BodyRef::FieldAccess {
                        receiver: receiver(object, source, 0),
                        name: text(field_node, source).to_owned(),
                        line: line(node),
                    });
                }
            }
            "method_reference" => {
                let children = named_children(node);
                if let Some(first) = children.first() {
                    let name = match children.last() {
                        Some(last) if children.len() > 1 && last.kind() == "identifier" => {
                            text(*last, source).to_owned()
                        }
                        _ => "<init>".to_owned(),
                    };
                    let receiver = if is_type_node(first.kind()) {
                        erase(*first, source).map_or(Receiver::Unknown, Receiver::New)
                    } else {
                        receiver(*first, source, 0)
                    };
                    refs.push(BodyRef::MethodRef { receiver, name, line: line(node) });
                }
            }
            "identifier" if is_value_identifier(parent, field) => {
                refs.push(BodyRef::Name { name: text(node, source).to_owned(), line: line(node) });
            }
            "type_identifier" if parent != "scoped_type_identifier" => {
                if let Some(ty) = type_use(node, source) {
                    refs.push(BodyRef::Type(ty));
                }
            }
            "scoped_type_identifier" if parent != "scoped_type_identifier" => {
                if let Some(ty) = type_use(node, source) {
                    refs.push(BodyRef::Type(ty));
                }
                continue;
            }
            "local_variable_declaration" | "field_declaration" => {
                let ty_node = node.child_by_field_name("type");
                let declared = ty_node.and_then(|t| type_use(t, source));
                let mut cursor = node.walk();
                for declarator in node.children_by_field_name("declarator", &mut cursor) {
                    let Some(name) = declarator.child_by_field_name("name") else {
                        continue;
                    };
                    // `var x = new Foo()` / `var x = (Foo) y`: the type is syntactically evident.
                    let inferred = declared.clone().or_else(|| {
                        declarator
                            .child_by_field_name("value")
                            .filter(|v| matches!(v.kind(), "object_creation_expression" | "cast_expression"))
                            .and_then(|v| v.child_by_field_name("type"))
                            .and_then(|t| type_use(t, source))
                    });
                    // `var x = repo.find(id)`: the type is whatever `find` declares, which only
                    // resolution can know. Keep the initializer as a receiver expression for it.
                    // Primitives also have no `declared` type, so check for `var` explicitly.
                    let is_var = ty_node.is_some_and(|t| t.kind() == "type_identifier" && text(t, source) == "var");
                    let init = match (is_var && inferred.is_none(), declarator.child_by_field_name("value")) {
                        (true, Some(value)) => {
                            Some(receiver(value, source, 0)).filter(|r| *r != Receiver::Unknown).map(LocalInit::Expr)
                        }
                        _ => None,
                    };
                    locals.push(Local { name: text(name, source).to_owned(), ty: inferred, init });
                }
            }
            "formal_parameter" | "enhanced_for_statement" | "resource" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let ty_node = node.child_by_field_name("type");
                    let ty = ty_node.and_then(|t| type_use(t, source));
                    let is_var = ty_node.is_some_and(|t| t.kind() == "type_identifier" && text(t, source) == "var");
                    let init = match (node.kind(), is_var, node.child_by_field_name("value")) {
                        ("enhanced_for_statement", true, Some(iterable)) => Some(receiver(iterable, source, 0))
                            .filter(|r| *r != Receiver::Unknown)
                            .map(LocalInit::ElementOf),
                        ("resource", true, Some(value)) => {
                            Some(receiver(value, source, 0)).filter(|r| *r != Receiver::Unknown).map(LocalInit::Expr)
                        }
                        _ => None,
                    };
                    locals.push(Local { name: text(name, source).to_owned(), ty, init });
                }
            }
            "catch_formal_parameter" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let ty = named_children(node)
                        .into_iter()
                        .find(|n| n.kind() == "catch_type")
                        .and_then(|c| named_children(c).into_iter().find_map(|t| type_use(t, source)));
                    locals.push(Local { name: text(name, source).to_owned(), ty, init: None });
                }
            }
            "lambda_expression" => {
                // Typed parameters `(Job j) -> ...` are `formal_parameter`s, handled above.
                let params = match node.child_by_field_name("parameters") {
                    Some(p) if p.kind() == "identifier" => vec![p],
                    Some(p) if p.kind() == "inferred_parameters" => {
                        named_children(p).into_iter().filter(|n| n.kind() == "identifier").collect()
                    }
                    _ => Vec::new(),
                };
                // `xs.forEach(x -> ...)`: the call the lambda is an argument of says what `x` is.
                let call = node
                    .parent()
                    .filter(|p| p.kind() == "argument_list")
                    .and_then(|args| args.parent())
                    .filter(|c| c.kind() == "method_invocation");
                for (index, param) in params.into_iter().enumerate() {
                    let init = call.and_then(|call| {
                        let method = text(call.child_by_field_name("name")?, source).to_owned();
                        let receiver = receiver(call.child_by_field_name("object")?, source, 0);
                        (receiver != Receiver::Unknown).then(|| LocalInit::LambdaParam {
                            receiver,
                            method,
                            index: u32::try_from(index).unwrap_or(u32::MAX),
                        })
                    });
                    locals.push(Local { name: text(param, source).to_owned(), ty: None, init });
                }
            }
            "instanceof_expression" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let ty = node.child_by_field_name("right").and_then(|t| type_use(t, source));
                    locals.push(Local { name: text(name, source).to_owned(), ty, init: None });
                }
            }
            "type_pattern" => {
                let children = named_children(node);
                if let (Some(ty), Some(name)) = (
                    children.iter().find(|n| is_type_node(n.kind())),
                    children.iter().find(|n| n.kind() == "identifier"),
                ) {
                    locals.push(Local { name: text(*name, source).to_owned(), ty: type_use(*ty, source), init: None });
                }
            }
            kind if is_comment(kind) => continue,
            _ => {}
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                let child_field = cursor.field_name();
                if child.is_named() && skip_child != Some(child.id()) {
                    match replace_child {
                        Some((id, replacement)) if id == child.id() => {
                            stack.push((replacement, node.kind(), None));
                        }
                        _ => stack.push((child, node.kind(), child_field)),
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}
