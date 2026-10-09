//! Whole-snapshot name resolution for Java.
//!
//! This is deliberately *not* a compiler. It implements the subset of JLS scoping that can be
//! decided from syntax plus declarations in the repository: nested/imported/same-package/wildcard
//! type lookup, member lookup up the in-repo type hierarchy, and receiver typing for locals,
//! parameters, fields, `this`/`super`, `new`, casts and chained calls with declared return types.
//! Anything outside that subset is classified as either *external* (a library type — no edge
//! needed) or *unresolved* (surfaced as uncertainty), never guessed.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use ripplepath_core::{EdgeKind, Evidence, Span, Symbol, SymbolId, SymbolKind, Visibility};

use super::TEST_ANNOTATIONS;
use super::facts::{BodyRef, FieldDecl, JavaFile, Local, LocalInit, MethodDecl, Receiver, TypeDecl, TypeUse};
use crate::LanguageGraph;
use crate::output::Output;

/// `java.lang.Object` members every class inherits; calls to them are external, not unresolved.
const OBJECT_METHODS: &[&str] =
    &["equals", "hashCode", "toString", "getClass", "notify", "notifyAll", "wait", "clone", "finalize"];

const MAX_RECEIVER_DEPTH: usize = 64;
/// Receiver-typing steps per reference; see `Scope::fuel`. Real code needs a handful per chain link.
const RECEIVER_FUEL: u32 = 256;
/// Declarations of one local name compared before giving up; see `local_recv`.
const MAX_SAME_NAME_LOCALS: usize = 8;

pub fn resolve(files: &[&JavaFile]) -> LanguageGraph {
    let index = Index::build(files);
    let mut out = Output::default();
    for file in files {
        index.emit_file(file, &mut out);
    }
    out.finish()
}

struct TypeEntry<'a> {
    id: SymbolId,
    file: &'a JavaFile,
    decl: &'a TypeDecl,
    outer: Option<String>,
    /// Member lookups by name. Every call site does one; scanning the member list instead made
    /// large classes quadratic (4x the methods cost 15x the time).
    methods: HashMap<&'a str, Vec<&'a MethodDecl>>,
    constructors: Vec<&'a MethodDecl>,
    fields: HashMap<&'a str, &'a FieldDecl>,
}

struct Index<'a> {
    types: BTreeMap<String, TypeEntry<'a>>,
    /// Simple names of all repository types. A name that is not reachable by scoping rules but
    /// exists here is *unresolved* (likely a scoping case we do not model); one that does not exist
    /// here is *external* (a library type).
    simple_names: BTreeSet<&'a str>,
    /// Names of every method and record component declared in the repository. A call whose name
    /// is not here cannot bind to repository code whatever its receiver is, so an untyped receiver
    /// is no evidence of a missing edge: such calls are external, not unresolved.
    member_names: BTreeSet<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TypeRes {
    Internal { fqn: String, evidence: Evidence },
    External,
    Unresolved,
}

#[derive(Clone, Debug)]
enum RecvType {
    Internal {
        fqn: String,
        evidence: Evidence,
    },
    /// A `java.util` container whose element types are known from its type arguments. For every
    /// other purpose it is an external type.
    Container {
        family: Family,
        args: Vec<RecvType>,
    },
    External,
    Unknown,
}

/// JDK container shapes whose element-returning methods are modelled by a fixed table (see
/// `container_result`). The JDK's signatures are stable, but they are a table rather than code
/// we read, so anything typed through them is `STATIC_INFERRED`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// `Iterable<E>` and its subtypes: lists, sets, queues, deques.
    Collection,
    Optional,
    Map,
    /// `java.util.stream.Stream<E>`, reached through `stream()` on a collection.
    Stream,
}

const JDK_CONTAINERS: &[(&str, &str, Family)] = &[
    ("java.lang", "Iterable", Family::Collection),
    ("java.util", "Collection", Family::Collection),
    ("java.util", "SequencedCollection", Family::Collection),
    ("java.util", "List", Family::Collection),
    ("java.util", "ArrayList", Family::Collection),
    ("java.util", "LinkedList", Family::Collection),
    ("java.util", "Set", Family::Collection),
    ("java.util", "SequencedSet", Family::Collection),
    ("java.util", "HashSet", Family::Collection),
    ("java.util", "LinkedHashSet", Family::Collection),
    ("java.util", "SortedSet", Family::Collection),
    ("java.util", "NavigableSet", Family::Collection),
    ("java.util", "TreeSet", Family::Collection),
    ("java.util", "Queue", Family::Collection),
    ("java.util", "Deque", Family::Collection),
    ("java.util", "ArrayDeque", Family::Collection),
    ("java.util", "PriorityQueue", Family::Collection),
    ("java.util.concurrent", "BlockingQueue", Family::Collection),
    ("java.util.concurrent", "LinkedBlockingQueue", Family::Collection),
    ("java.util.concurrent", "ArrayBlockingQueue", Family::Collection),
    ("java.util.concurrent", "ConcurrentLinkedQueue", Family::Collection),
    ("java.util.concurrent", "ConcurrentLinkedDeque", Family::Collection),
    ("java.util.concurrent", "CopyOnWriteArrayList", Family::Collection),
    ("java.util", "Optional", Family::Optional),
    ("java.util.stream", "Stream", Family::Stream),
    ("java.util", "Map", Family::Map),
    ("java.util", "SequencedMap", Family::Map),
    ("java.util", "HashMap", Family::Map),
    ("java.util", "LinkedHashMap", Family::Map),
    ("java.util", "SortedMap", Family::Map),
    ("java.util", "NavigableMap", Family::Map),
    ("java.util", "TreeMap", Family::Map),
    ("java.util", "EnumMap", Family::Map),
    ("java.util.concurrent", "ConcurrentMap", Family::Map),
    ("java.util.concurrent", "ConcurrentHashMap", Family::Map),
    ("java.util.concurrent", "ConcurrentSkipListMap", Family::Map),
];

/// What a call on a JDK container returns, for the methods whose result is an element (or a view
/// of elements). `None` for anything else: such calls are external like any library call.
fn container_result(family: Family, args: &[RecvType], name: &str, arity: u32) -> Option<RecvType> {
    let arg = |i: usize| match args.get(i) {
        Some(t) => inferred(t.clone()),
        None if args.is_empty() => RecvType::External, // raw type: elements are Object
        None => RecvType::Unknown,
    };
    match (family, name, arity) {
        (
            Family::Collection,
            "getFirst" | "getLast" | "removeFirst" | "removeLast" | "peek" | "poll" | "element" | "pop" | "first"
            | "last" | "peekFirst" | "peekLast" | "pollFirst" | "pollLast" | "take",
            0,
        )
        | (Family::Collection, "get", 1)
        | (Family::Optional, "get" | "orElseThrow", 0)
        | (Family::Optional, "orElseThrow" | "orElse", 1) => Some(arg(0)),
        (Family::Map, "get" | "remove", 1)
        | (
            Family::Map,
            "getOrDefault" | "put" | "putIfAbsent" | "computeIfAbsent" | "computeIfPresent" | "compute" | "replace",
            2,
        )
        | (Family::Map, "merge", 3) => Some(arg(1)),
        (Family::Collection, "stream" | "parallelStream", 0) | (Family::Optional, "stream", 0) => {
            Some(RecvType::Container { family: Family::Stream, args: args.to_vec() })
        }
        (Family::Optional, "filter", 1) => Some(RecvType::Container { family, args: args.to_vec() }),
        (
            Family::Stream,
            "filter" | "sorted" | "peek" | "distinct" | "limit" | "skip" | "takeWhile" | "dropWhile" | "parallel"
            | "sequential" | "unordered",
            _,
        ) => Some(RecvType::Container { family, args: args.to_vec() }),
        (Family::Stream, "findFirst" | "findAny", 0) | (Family::Stream, "min" | "max", 1) => {
            Some(RecvType::Container { family: Family::Optional, args: args.to_vec() })
        }
        (Family::Stream, "toList", 0) => Some(RecvType::Container { family: Family::Collection, args: args.to_vec() }),
        (Family::Map, "values", 0) => Some(RecvType::Container { family: Family::Collection, args: vec![arg(1)] }),
        (Family::Map, "keySet" | "sequencedKeySet" | "navigableKeySet", 0) => {
            Some(RecvType::Container { family: Family::Collection, args: vec![arg(0)] })
        }
        _ => None,
    }
}

/// Type of parameter `index` of a lambda passed to `method` on a container: the element for
/// element-consuming methods (`forEach`, `filter`, `map`, comparators...), key or value for maps.
fn lambda_param(container: RecvType, method: &str, index: u32) -> RecvType {
    let RecvType::Container { family, args } = container else {
        return RecvType::Unknown;
    };
    let arg = |i: usize| match args.get(i) {
        Some(t) => inferred(t.clone()),
        None if args.is_empty() => RecvType::External,
        None => RecvType::Unknown,
    };
    match (family, method, index) {
        (Family::Collection, "forEach" | "removeIf", 0)
        | (
            Family::Stream,
            "filter" | "map" | "forEach" | "forEachOrdered" | "anyMatch" | "allMatch" | "noneMatch" | "peek"
            | "mapToInt" | "mapToLong" | "mapToDouble" | "mapToObj" | "flatMap" | "mapMulti" | "takeWhile"
            | "dropWhile",
            0,
        )
        | (Family::Stream, "sorted" | "min" | "max", 0 | 1)
        | (Family::Optional, "map" | "flatMap" | "filter" | "ifPresent" | "ifPresentOrElse", 0) => arg(0),
        (Family::Map, "forEach" | "compute" | "computeIfPresent" | "replaceAll", 0)
        | (Family::Map, "computeIfAbsent", 0) => arg(0),
        (Family::Map, "forEach" | "compute" | "computeIfPresent" | "replaceAll", 1) | (Family::Map, "merge", 0 | 1) => {
            arg(1)
        }
        _ => RecvType::Unknown,
    }
}

/// Element of an iterable container (`for (var x : xs)`).
fn iteration_element(container: RecvType) -> RecvType {
    match container {
        RecvType::Container { family: Family::Collection, args } => match args.into_iter().next() {
            Some(t) => inferred(t),
            None => RecvType::External,
        },
        // Arrays erase to their element type in `TypeUse`, so an iterated array of an in-repo
        // type is indistinguishable from a non-iterable value of that type. Do not guess.
        _ => RecvType::Unknown,
    }
}

fn inferred(t: RecvType) -> RecvType {
    weaken(t, Evidence::StaticInferred)
}

fn weaken(t: RecvType, by: Evidence) -> RecvType {
    match t {
        RecvType::Internal { fqn, evidence } => RecvType::Internal { fqn, evidence: weaker(evidence, by) },
        other => other,
    }
}

enum Lookup<T> {
    Found(Vec<T>),
    External,
    NotFound,
}

struct Scope<'a> {
    file: &'a JavaFile,
    type_fqn: &'a str,
    type_params: Vec<&'a str>,
    locals: &'a [Local],
    /// Receiver-typing steps left for the reference being resolved. Typing a local whose name is
    /// declared several times resolves every declaration, which branches; the depth bound alone
    /// would allow exponential work on crafted input.
    fuel: Cell<u32>,
}

fn fqn_of(file: &JavaFile, relative: &str) -> String {
    match &file.package {
        Some(package) => format!("{package}.{relative}"),
        None => relative.to_owned(),
    }
}

fn last_segment(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn weaker(a: Evidence, b: Evidence) -> Evidence {
    if a.strength() <= b.strength() { a } else { b }
}

fn arity_matches(method: &MethodDecl, arity: Option<u32>) -> bool {
    match arity {
        None => true,
        Some(n) if method.is_varargs => n as usize + 1 >= method.params.len(),
        Some(n) => method.params.len() == n as usize,
    }
}

pub(crate) fn method_id(type_id: &SymbolId, method: &MethodDecl) -> SymbolId {
    SymbolId::new(format!("{type_id}#{}{}", method.name, method.signature()))
}

fn field_id(type_id: &SymbolId, field: &FieldDecl) -> SymbolId {
    SymbolId::new(format!("{type_id}#{}", field.name))
}

impl<'a> Index<'a> {
    fn build(files: &[&'a JavaFile]) -> Self {
        let mut declared: BTreeMap<String, Vec<(&'a JavaFile, &'a TypeDecl)>> = BTreeMap::new();
        let mut simple_names = BTreeSet::new();
        let mut member_names = BTreeSet::new();
        for &file in files {
            for decl in &file.types {
                member_names.extend(decl.methods.iter().map(|m| m.name.as_str()));
                if decl.kind == SymbolKind::Record {
                    member_names.extend(decl.fields.iter().filter(|f| !f.is_static).map(|f| f.name.as_str()));
                }
                declared.entry(fqn_of(file, &decl.name)).or_default().push((file, decl));
                simple_names.insert(last_segment(&decl.name));
            }
        }
        let mut types = BTreeMap::new();
        for (fqn, mut decls) in declared {
            // The same FQN declared twice (multi-module builds, copy-pasted fixtures). Identity must
            // stay unique, so every duplicate is qualified with its path — consistently, so the id
            // does not depend on which duplicate happens to sort first.
            let duplicated = decls.len() > 1;
            decls.sort_by(|a, b| a.0.path.cmp(&b.0.path));
            let Some((file, decl)) = decls.into_iter().next() else {
                continue;
            };
            let id = if duplicated {
                SymbolId::new(format!("java:{fqn}@{}", file.path))
            } else {
                SymbolId::new(format!("java:{fqn}"))
            };
            let outer = decl.name.rsplit_once('.').map(|(outer, _)| fqn_of(file, outer));
            let mut methods: HashMap<&str, Vec<&MethodDecl>> = HashMap::new();
            let mut constructors = Vec::new();
            for method in &decl.methods {
                if method.is_constructor {
                    constructors.push(method);
                } else {
                    methods.entry(method.name.as_str()).or_default().push(method);
                }
            }
            let mut fields = HashMap::new();
            for field in &decl.fields {
                fields.entry(field.name.as_str()).or_insert(field);
            }
            types.insert(fqn, TypeEntry { id, file, decl, outer, methods, constructors, fields });
        }
        Self { types, simple_names, member_names }
    }

    fn is_canonical(&self, file: &JavaFile, decl: &TypeDecl) -> bool {
        self.types.get(&fqn_of(file, &decl.name)).is_some_and(|entry| std::ptr::eq(entry.decl, decl))
    }

    fn resolve_type(&self, file: &JavaFile, enclosing: Option<&str>, type_params: &[&str], name: &str) -> TypeRes {
        if let Some((head, rest)) = name.split_once('.') {
            if self.types.contains_key(name) {
                return TypeRes::Internal { fqn: name.to_owned(), evidence: Evidence::ResolvedExact };
            }
            // `Outer.Inner` where `Outer` is itself resolved by scoping rules.
            if let TypeRes::Internal { fqn, evidence } = self.resolve_type(file, enclosing, type_params, head) {
                let nested = format!("{fqn}.{rest}");
                if self.types.contains_key(&nested) {
                    return TypeRes::Internal { fqn: nested, evidence };
                }
            }
            return if self.simple_names.contains(last_segment(name)) {
                TypeRes::Unresolved
            } else {
                TypeRes::External
            };
        }

        if type_params.contains(&name) {
            return TypeRes::External;
        }

        // Member types of the enclosing types (innermost first) and of their outer types.
        let mut current = enclosing.map(str::to_owned);
        while let Some(fqn) = current {
            let candidate = format!("{fqn}.{name}");
            if self.types.contains_key(&candidate) {
                return TypeRes::Internal { fqn: candidate, evidence: Evidence::ResolvedExact };
            }
            if let Some(entry) = self.types.get(&fqn) {
                if entry.decl.type_params.iter().any(|p| p == name) {
                    return TypeRes::External;
                }
                current = entry.outer.clone();
            } else {
                current = None;
            }
        }

        for import in file.imports.iter().filter(|i| !i.is_static && !i.wildcard) {
            if last_segment(&import.path) == name {
                return if self.types.contains_key(&import.path) {
                    TypeRes::Internal { fqn: import.path.clone(), evidence: Evidence::ResolvedExact }
                } else {
                    TypeRes::External
                };
            }
        }

        let same_package = fqn_of(file, name);
        if self.types.contains_key(&same_package) {
            return TypeRes::Internal { fqn: same_package, evidence: Evidence::ResolvedExact };
        }

        let wildcard_hits: Vec<String> = file
            .imports
            .iter()
            .filter(|i| !i.is_static && i.wildcard)
            .map(|i| format!("{}.{name}", i.path))
            .filter(|candidate| self.types.contains_key(candidate))
            .collect();
        match wildcard_hits.len() {
            0 => {}
            1 => {
                return TypeRes::Internal {
                    fqn: wildcard_hits.into_iter().next().unwrap_or_default(),
                    evidence: Evidence::ResolvedExact,
                };
            }
            // Ambiguous on-demand imports do not compile in Java, so this is a broken snapshot; pick
            // deterministically but say it is inferred.
            _ => {
                return TypeRes::Internal {
                    fqn: wildcard_hits.into_iter().next().unwrap_or_default(),
                    evidence: Evidence::StaticInferred,
                };
            }
        }

        if self.simple_names.contains(name) { TypeRes::Unresolved } else { TypeRes::External }
    }

    fn resolve_in_scope(&self, scope: &Scope<'_>, name: &str) -> TypeRes {
        self.resolve_type(scope.file, Some(scope.type_fqn), &scope.type_params, name)
    }

    /// The value type of a written type, keeping element types of JDK containers.
    fn recv_of(&self, file: &JavaFile, enclosing: Option<&str>, type_params: &[&str], ty: &TypeUse) -> RecvType {
        match self.resolve_type(file, enclosing, type_params, &ty.name) {
            TypeRes::Internal { fqn, evidence } => RecvType::Internal { fqn, evidence },
            TypeRes::Unresolved => RecvType::Unknown,
            TypeRes::External => match jdk_family(file, &ty.name) {
                Some(family) => RecvType::Container {
                    family,
                    args: ty
                        .args
                        .iter()
                        .map(|arg| match arg.as_str() {
                            "" => RecvType::Unknown,
                            arg => Self::type_res_to_recv(self.resolve_type(file, enclosing, type_params, arg)),
                        })
                        .collect(),
                },
                None => RecvType::External,
            },
        }
    }

    fn recv_in_scope(&self, scope: &Scope<'_>, ty: &TypeUse) -> RecvType {
        self.recv_of(scope.file, Some(scope.type_fqn), &scope.type_params, ty)
    }

    /// The value type of a type written in a member declared on `owner_fqn` (a field type, a return
    /// type), resolved with the declaring file's imports rather than the caller's.
    fn recv_declared(&self, owner_fqn: &str, ty: &TypeUse) -> RecvType {
        match self.types.get(owner_fqn) {
            Some(owner) => {
                let params: Vec<&str> = owner.decl.type_params.iter().map(String::as_str).collect();
                self.recv_of(owner.file, Some(owner_fqn), &params, ty)
            }
            None => RecvType::Unknown,
        }
    }

    /// Supertypes in declaration order (superclass first), split into in-repo and external.
    fn supertypes(&self, fqn: &str) -> (Vec<(String, EdgeKind, Evidence, u32)>, bool) {
        let Some(entry) = self.types.get(fqn) else {
            return (Vec::new(), false);
        };
        let params: Vec<&str> = entry.decl.type_params.iter().map(String::as_str).collect();
        let mut internal = Vec::new();
        let mut has_external = false;
        let declared = entry
            .decl
            .extends
            .iter()
            .map(|t| (t, EdgeKind::Extends))
            .chain(entry.decl.implements.iter().map(|t| (t, EdgeKind::Implements)));
        for (ty, kind) in declared {
            match self.resolve_type(entry.file, entry.outer.as_deref(), &params, &ty.name) {
                TypeRes::Internal { fqn, evidence } => internal.push((fqn, kind, evidence, ty.line)),
                TypeRes::External | TypeRes::Unresolved => has_external = true,
            }
        }
        (internal, has_external)
    }

    /// `fqn` and its in-repo supertypes, breadth-first (nearest first), and whether the hierarchy
    /// reaches a type outside the repository.
    fn hierarchy(&self, fqn: &str) -> (Vec<(String, &TypeEntry<'a>)>, bool) {
        let mut queue = VecDeque::from([fqn.to_owned()]);
        let mut seen = BTreeSet::new();
        let mut levels = Vec::new();
        let mut external_ancestor = false;
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(entry) = self.types.get(&current) else {
                continue;
            };
            let (supers, has_external) = self.supertypes(&current);
            external_ancestor |= has_external;
            // Implicit supertypes outside the repository: java.lang.Object for a class without
            // `extends`, java.lang.Enum (name(), ordinal(), compareTo()...) for every enum. An enum's
            // synthetic static values()/valueOf() land here too.
            if (entry.decl.kind == SymbolKind::Class && entry.decl.extends.is_empty())
                || entry.decl.kind == SymbolKind::Enum
            {
                external_ancestor = true;
            }
            queue.extend(supers.into_iter().map(|(s, ..)| s));
            levels.push((current, entry));
        }
        (levels, external_ancestor)
    }

    /// Breadth-first over the in-repo hierarchy; members found on the nearest level win, which
    /// is Java's rule for fields (hiding).
    fn find_member<T>(&self, fqn: &str, mut select: impl FnMut(&TypeEntry<'a>) -> Vec<T>) -> Lookup<(String, T)> {
        let (levels, external_ancestor) = self.hierarchy(fqn);
        for (current, entry) in levels {
            let found = select(entry);
            if !found.is_empty() {
                return Lookup::Found(found.into_iter().map(|m| (current.clone(), m)).collect());
            }
        }
        if external_ancestor { Lookup::External } else { Lookup::NotFound }
    }

    /// Every method of the hierarchy with this name and a compatible arity, nearest declaration of
    /// each signature only (a same-signature declaration further up is overridden, not a
    /// candidate). Java overload resolution chooses among inherited methods too, so a same-arity
    /// overload in a supertype is a second candidate and the call is no longer exact.
    fn find_methods(&self, fqn: &str, name: &str, arity: Option<u32>) -> Lookup<(String, &'a MethodDecl)> {
        let (levels, external_ancestor) = self.hierarchy(fqn);
        let mut signatures = BTreeSet::new();
        let mut found = Vec::new();
        for (current, entry) in levels {
            for method in entry.methods.get(name).into_iter().flatten().copied() {
                if arity_matches(method, arity) && signatures.insert(method.signature()) {
                    found.push((current.clone(), method));
                }
            }
        }
        if !found.is_empty() {
            Lookup::Found(found)
        } else if external_ancestor || OBJECT_METHODS.contains(&name) {
            Lookup::External
        } else {
            Lookup::NotFound
        }
    }

    fn find_field(&self, fqn: &str, name: &str) -> Lookup<(String, &'a FieldDecl)> {
        self.find_member(fqn, |entry| entry.fields.get(name).map_or_else(Vec::new, |f| vec![*f]))
    }

    fn type_id(&self, fqn: &str) -> Option<&SymbolId> {
        self.types.get(fqn).map(|e| &e.id)
    }

    fn method_symbol(&self, owner_fqn: &str, method: &MethodDecl) -> Option<SymbolId> {
        self.type_id(owner_fqn).map(|id| method_id(id, method))
    }

    fn field_symbol(&self, owner_fqn: &str, field: &FieldDecl) -> Option<SymbolId> {
        self.type_id(owner_fqn).map(|id| field_id(id, field))
    }

    fn local<'s>(scope: &'s Scope<'_>, name: &str) -> Option<&'s Local> {
        scope.locals.iter().rev().find(|l| l.name == name)
    }

    /// Type of local `name`. Scopes are flattened per method, so two declarations of one name
    /// (sibling blocks, two lambdas both calling their parameter `j`) cannot be told apart. The
    /// local is typed only when every declaration gives the same type; otherwise it is unknown,
    /// never whichever declaration came last.
    fn local_recv(&self, scope: &Scope<'_>, name: &str, depth: usize) -> RecvType {
        let declarations: Vec<&Local> = scope.locals.iter().filter(|l| l.name == name).collect();
        let Some(first) = declarations.first() else {
            return RecvType::Unknown;
        };
        let identical = |l: &&Local| {
            l.ty.as_ref().map(|t| (&t.name, &t.args)) == first.ty.as_ref().map(|t| (&t.name, &t.args))
                && l.init == first.init
        };
        if declarations.iter().all(identical) {
            return self.declared_local_recv(scope, first, depth);
        }
        if declarations.len() > MAX_SAME_NAME_LOCALS {
            return RecvType::Unknown;
        }
        let mut agreed: Option<(String, Evidence)> = None;
        for local in declarations {
            match self.declared_local_recv(scope, local, depth) {
                RecvType::Internal { fqn, evidence } => match &mut agreed {
                    None => agreed = Some((fqn, evidence)),
                    Some((seen, seen_evidence)) if *seen == fqn => *seen_evidence = weaker(*seen_evidence, evidence),
                    Some(_) => return RecvType::Unknown,
                },
                _ => return RecvType::Unknown,
            }
        }
        agreed.map_or(RecvType::Unknown, |(fqn, evidence)| RecvType::Internal { fqn, evidence })
    }

    fn declared_local_recv(&self, scope: &Scope<'_>, local: &Local, depth: usize) -> RecvType {
        match (&local.ty, &local.init) {
            (Some(ty), _) => self.recv_in_scope(scope, ty),
            // `var x = expr`: typed as the compiler would, from the declared type of what `expr`
            // resolves to. The depth bound also stops flattened-scope cycles such as `var a = b.f()`
            // in one block and `var b = a.g()` in another.
            (None, Some(LocalInit::Expr(init))) => self.receiver_type(scope, init, depth + 1),
            (None, Some(LocalInit::ElementOf(iterable))) => {
                iteration_element(self.receiver_type(scope, iterable, depth + 1))
            }
            (None, Some(LocalInit::RecordComponent { record, index })) => match self.resolve_in_scope(scope, record) {
                TypeRes::Internal { fqn, evidence } => {
                    let component = self
                        .types
                        .get(&fqn)
                        .filter(|e| e.decl.kind == SymbolKind::Record)
                        .and_then(|entry| entry.decl.fields.iter().filter(|f| !f.is_static).nth(*index as usize));
                    match component {
                        Some(field) => weaken(self.recv_declared(&fqn, &field.ty), evidence),
                        None => RecvType::Unknown,
                    }
                }
                TypeRes::External => RecvType::External,
                TypeRes::Unresolved => RecvType::Unknown,
            },
            (None, Some(LocalInit::LambdaParam { receiver, method, index })) => {
                lambda_param(self.receiver_type(scope, receiver, depth + 1), method, *index)
            }
            (None, None) => RecvType::Unknown,
        }
    }

    fn local_type<'s>(scope: &'s Scope<'_>, name: &str) -> Option<&'s Option<TypeUse>> {
        Self::local(scope, name).map(|l| &l.ty)
    }

    /// Field visible by simple name: the enclosing type's hierarchy, then lexically enclosing types.
    fn field_in_scope(&self, scope: &Scope<'_>, name: &str) -> Option<(String, &'a FieldDecl)> {
        let mut current = Some(scope.type_fqn.to_owned());
        while let Some(fqn) = current {
            if let Lookup::Found(found) = self.find_field(&fqn, name) {
                return found.into_iter().next();
            }
            current = self.types.get(&fqn).and_then(|e| e.outer.clone());
        }
        None
    }

    fn pure_name_chain(receiver: &Receiver) -> Option<String> {
        match receiver {
            Receiver::Name(name) => Some(name.clone()),
            Receiver::Field(inner, name) => Self::pure_name_chain(inner).map(|prefix| format!("{prefix}.{name}")),
            _ => None,
        }
    }

    fn type_res_to_recv(res: TypeRes) -> RecvType {
        match res {
            TypeRes::Internal { fqn, evidence } => RecvType::Internal { fqn, evidence },
            TypeRes::External => RecvType::External,
            TypeRes::Unresolved => RecvType::Unknown,
        }
    }

    fn receiver_type(&self, scope: &Scope<'_>, receiver: &Receiver, depth: usize) -> RecvType {
        let fuel = scope.fuel.get();
        if depth > MAX_RECEIVER_DEPTH || fuel == 0 {
            return RecvType::Unknown;
        }
        scope.fuel.set(fuel - 1);
        match receiver {
            Receiver::Implicit | Receiver::This => {
                RecvType::Internal { fqn: scope.type_fqn.to_owned(), evidence: Evidence::ResolvedExact }
            }
            Receiver::Super => match self.supertypes(scope.type_fqn).0.into_iter().next() {
                Some((fqn, EdgeKind::Extends, evidence, _)) => RecvType::Internal { fqn, evidence },
                _ => RecvType::External,
            },
            Receiver::Name(name) => {
                if Self::local(scope, name).is_some() {
                    return self.local_recv(scope, name, depth);
                }
                if let Some((owner, field)) = self.field_in_scope(scope, name) {
                    return self.recv_declared(&owner, &field.ty);
                }
                Self::type_res_to_recv(self.resolve_in_scope(scope, name))
            }
            Receiver::Field(inner, name) => {
                if let Some(dotted) = Self::pure_name_chain(receiver) {
                    let head = dotted.split('.').next().unwrap_or_default();
                    let head_is_variable =
                        Self::local_type(scope, head).is_some() || self.field_in_scope(scope, head).is_some();
                    if !head_is_variable
                        && let TypeRes::Internal { fqn, evidence } = self.resolve_in_scope(scope, &dotted)
                    {
                        return RecvType::Internal { fqn, evidence };
                    }
                }
                match self.receiver_type(scope, inner, depth + 1) {
                    RecvType::Internal { fqn, evidence } => match self.find_field(&fqn, name) {
                        Lookup::Found(found) => match found.into_iter().next() {
                            Some((owner, field)) => weaken(self.recv_declared(&owner, &field.ty), evidence),
                            None => RecvType::Unknown,
                        },
                        Lookup::External => RecvType::External,
                        Lookup::NotFound => RecvType::Unknown,
                    },
                    RecvType::Container { .. } => RecvType::External,
                    other => other,
                }
            }
            Receiver::Call { receiver, name, arity } => {
                match self.resolve_call(scope, receiver, name, Some(*arity), depth + 1) {
                    CallRes::Targets { methods, evidence, .. } => {
                        let mut returns =
                            methods.iter().filter_map(|(owner, m)| m.return_type.as_ref().map(|r| (owner, r)));
                        let Some((owner, first)) = returns.next() else {
                            return RecvType::External; // void or primitive: nothing to call on
                        };
                        if returns.any(|(_, r)| r.name != first.name) {
                            return RecvType::Unknown;
                        }
                        weaken(self.recv_declared(owner, first), evidence)
                    }
                    CallRes::Accessor { owner, field, evidence } => {
                        weaken(self.recv_declared(&owner, &field.ty), evidence)
                    }
                    CallRes::Container(result) => result,
                    CallRes::External => RecvType::External,
                    CallRes::Unresolved => RecvType::Unknown,
                }
            }
            // `new ArrayList<Job>()` keeps no type arguments in `Receiver::New`, so a container
            // created inline is just external.
            Receiver::New(ty) => Self::type_res_to_recv(self.resolve_in_scope(scope, ty)),
            Receiver::Unknown => RecvType::Unknown,
        }
    }

    fn resolve_call(
        &self,
        scope: &Scope<'_>,
        receiver: &Receiver,
        name: &str,
        arity: Option<u32>,
        depth: usize,
    ) -> CallRes<'a> {
        if let Receiver::Implicit = receiver {
            return self.resolve_unqualified_call(scope, name, arity);
        }
        let rule = match receiver {
            Receiver::This => "java.call.this",
            Receiver::Super => "java.call.super",
            Receiver::Name(n) if Self::local(scope, n).is_some_and(|l| l.init.is_some()) => "java.call.var-inferred",
            Receiver::Name(n) if Self::local_type(scope, n).is_some() => "java.call.local",
            Receiver::Name(n) if self.field_in_scope(scope, n).is_some() => "java.call.field",
            Receiver::Name(_) => "java.call.static",
            Receiver::Field(..) => "java.call.field-chain",
            Receiver::Call { .. } => "java.call.chained",
            Receiver::New(_) => "java.call.typed-expression",
            Receiver::Implicit | Receiver::Unknown => "java.call.unknown",
        };
        match self.receiver_type(scope, receiver, depth) {
            RecvType::Internal { fqn, evidence } => match self.find_methods(&fqn, name, arity) {
                Lookup::Found(methods) => {
                    let overload = if methods.len() == 1 { Evidence::ResolvedExact } else { Evidence::StaticInferred };
                    CallRes::Targets { methods, evidence: weaker(evidence, overload), rule }
                }
                // A record that implements an external interface has an external ancestor, so its
                // components must be checked before concluding "external".
                Lookup::External => self.record_accessor(&fqn, name, arity, evidence).unwrap_or(CallRes::External),
                Lookup::NotFound => self.record_accessor(&fqn, name, arity, evidence).unwrap_or(CallRes::Unresolved),
            },
            RecvType::Container { family, args } => {
                match arity.and_then(|n| container_result(family, &args, name, n)) {
                    Some(result) => CallRes::Container(result),
                    None => CallRes::External,
                }
            }
            RecvType::External => CallRes::External,
            RecvType::Unknown => CallRes::Unresolved,
        }
    }

    fn resolve_unqualified_call(&self, scope: &Scope<'_>, name: &str, arity: Option<u32>) -> CallRes<'a> {
        let mut saw_external = false;
        let mut current = Some(scope.type_fqn.to_owned());
        while let Some(fqn) = current {
            match self.find_methods(&fqn, name, arity) {
                Lookup::Found(methods) => {
                    let evidence = if methods.len() == 1 { Evidence::ResolvedExact } else { Evidence::StaticInferred };
                    return CallRes::Targets { methods, evidence, rule: "java.call.unqualified" };
                }
                lookup => {
                    if let Some(accessor) = self.record_accessor(&fqn, name, arity, Evidence::ResolvedExact) {
                        return accessor;
                    }
                    saw_external |= matches!(lookup, Lookup::External);
                }
            }
            current = self.types.get(&fqn).and_then(|e| e.outer.clone());
        }
        for import in scope.file.imports.iter().filter(|i| i.is_static) {
            let owner = if import.wildcard {
                import.path.as_str()
            } else if last_segment(&import.path) == name {
                import.path.rsplit_once('.').map_or("", |(owner, _)| owner)
            } else {
                continue;
            };
            if !self.types.contains_key(owner) {
                saw_external = true;
                continue;
            }
            if let Lookup::Found(methods) = self.find_methods(owner, name, arity) {
                let evidence = if methods.len() == 1 { Evidence::ResolvedExact } else { Evidence::StaticInferred };
                return CallRes::Targets { methods, evidence, rule: "java.call.static-import" };
            }
        }
        if saw_external { CallRes::External } else { CallRes::Unresolved }
    }

    /// A record component `c` has an implicit accessor `c()`. Records are final, so only the record
    /// itself can supply it; an explicitly declared `c()` is an ordinary method found earlier.
    fn record_accessor(&self, fqn: &str, name: &str, arity: Option<u32>, evidence: Evidence) -> Option<CallRes<'a>> {
        let entry = self.types.get(fqn)?;
        if entry.decl.kind != SymbolKind::Record || arity.is_some_and(|n| n != 0) {
            return None;
        }
        // Records cannot declare instance fields, so every non-static field is a component.
        let field = entry.fields.get(name).copied().filter(|f| !f.is_static)?;
        Some(CallRes::Accessor { owner: fqn.to_owned(), field, evidence })
    }

    fn constructors(&self, fqn: &str, arity: u32) -> Option<(Vec<SymbolId>, Evidence)> {
        let entry = self.types.get(fqn)?;
        let ctors = &entry.constructors;
        if ctors.is_empty() {
            // Implicit default constructor: the type itself is the target.
            return Some((vec![entry.id.clone()], Evidence::ResolvedExact));
        }
        let matching: Vec<&&MethodDecl> = ctors.iter().filter(|m| arity_matches(m, Some(arity))).collect();
        match matching.len() {
            1 => Some((vec![method_id(&entry.id, matching[0])], Evidence::ResolvedExact)),
            0 => Some((ctors.iter().map(|m| method_id(&entry.id, m)).collect(), Evidence::StaticInferred)),
            _ => Some((matching.iter().map(|m| method_id(&entry.id, m)).collect(), Evidence::StaticInferred)),
        }
    }

    fn emit_file(&self, file: &'a JavaFile, out: &mut Output) {
        let file_id = SymbolId::file(&file.path);
        let module = file.package.clone().unwrap_or_else(|| "(default)".to_owned());
        out.symbols.push(Symbol {
            id: file_id.clone(),
            kind: SymbolKind::File,
            name: file.path.rsplit('/').next().unwrap_or(&file.path).to_owned(),
            language: ripplepath_core::Language::Java,
            module: module.clone(),
            file: file.path.clone(),
            span: Span { start_line: 1, end_line: file.line_count.max(1) },
            parent: None,
            visibility: Visibility::Public,
            is_test: false,
            fingerprint: file.header_fingerprint,
        });

        for import in file.imports.iter().filter(|i| !i.wildcard) {
            let target = if import.is_static {
                import.path.rsplit_once('.').map_or("", |(owner, _)| owner)
            } else {
                import.path.as_str()
            };
            if let Some(id) = self.type_id(target) {
                out.edge(
                    &file_id,
                    id,
                    EdgeKind::Imports,
                    Evidence::ResolvedExact,
                    &file.path,
                    import.line,
                    "java.import",
                );
            }
        }

        for decl in &file.types {
            if self.is_canonical(file, decl) {
                self.emit_type(file, decl, &file_id, &module, out);
            }
        }
    }

    fn emit_type(&self, file: &'a JavaFile, decl: &'a TypeDecl, file_id: &SymbolId, module: &str, out: &mut Output) {
        let fqn = fqn_of(file, &decl.name);
        let Some(entry) = self.types.get(&fqn) else {
            return;
        };
        let type_id = entry.id.clone();
        let parent = match &entry.outer {
            Some(outer) => self.type_id(outer).cloned().unwrap_or_else(|| file_id.clone()),
            None => file_id.clone(),
        };
        let is_test_class = decl.methods.iter().any(is_test_method);
        out.symbols.push(Symbol {
            id: type_id.clone(),
            kind: decl.kind,
            name: decl.name.clone(),
            language: ripplepath_core::Language::Java,
            module: module.to_owned(),
            file: file.path.clone(),
            span: decl.span,
            parent: Some(parent.clone()),
            visibility: decl.visibility,
            is_test: is_test_class,
            fingerprint: decl.fingerprint,
        });
        out.edge(
            &parent,
            &type_id,
            EdgeKind::Contains,
            Evidence::ResolvedExact,
            &file.path,
            decl.span.start_line,
            "java.declaration",
        );

        let type_params: Vec<&str> = decl.type_params.iter().map(String::as_str).collect();
        let type_scope =
            Scope { file, type_fqn: &fqn, type_params: type_params.clone(), locals: &[], fuel: Cell::new(0) };

        for (super_fqn, kind, evidence, line) in self.supertypes(&fqn).0 {
            if let Some(super_id) = self.type_id(&super_fqn) {
                out.edge(&type_id, super_id, kind, evidence, &file.path, line, "java.supertype");
            }
        }
        for annotation in &decl.annotations {
            self.emit_type_ref(&type_scope, &type_id, annotation, "java.annotation", out);
        }
        self.emit_refs(&type_scope, &type_id, &decl.refs, out);

        for field in &decl.fields {
            let id = field_id(&type_id, field);
            out.symbols.push(Symbol {
                id: id.clone(),
                kind: SymbolKind::Field,
                name: field.name.clone(),
                language: ripplepath_core::Language::Java,
                module: module.to_owned(),
                file: file.path.clone(),
                span: field.span,
                parent: Some(type_id.clone()),
                visibility: field.visibility,
                is_test: false,
                fingerprint: field.fingerprint,
            });
            out.edge(
                &type_id,
                &id,
                EdgeKind::Contains,
                Evidence::ResolvedExact,
                &file.path,
                field.span.start_line,
                "java.declaration",
            );
            self.emit_type_ref(&type_scope, &id, &field.ty, "java.field.type", out);
            self.emit_refs(&type_scope, &id, &field.refs, out);
        }

        let simple_type_name = last_segment(&decl.name);
        for method in &decl.methods {
            let id = method_id(&type_id, method);
            let display = if method.is_constructor {
                format!("{simple_type_name}{}", method.signature())
            } else {
                format!("{}{}", method.name, method.signature())
            };
            out.symbols.push(Symbol {
                id: id.clone(),
                kind: if method.is_constructor { SymbolKind::Constructor } else { SymbolKind::Method },
                name: display,
                language: ripplepath_core::Language::Java,
                module: module.to_owned(),
                file: file.path.clone(),
                span: method.span,
                parent: Some(type_id.clone()),
                visibility: method.visibility,
                is_test: is_test_method(method),
                fingerprint: method.fingerprint,
            });
            out.edge(
                &type_id,
                &id,
                EdgeKind::Contains,
                Evidence::ResolvedExact,
                &file.path,
                method.span.start_line,
                "java.declaration",
            );

            let mut params = type_params.clone();
            params.extend(method.type_params.iter().map(String::as_str));
            let scope = Scope { file, type_fqn: &fqn, type_params: params, locals: &method.locals, fuel: Cell::new(0) };
            let signature_types =
                method.params.iter().map(|p| &p.ty).chain(method.return_type.iter()).chain(method.throws.iter());
            for ty in signature_types {
                self.emit_type_ref(&scope, &id, ty, "java.signature.type", out);
            }
            for annotation in &method.annotations {
                self.emit_type_ref(&scope, &id, annotation, "java.annotation", out);
            }
            self.emit_refs(&scope, &id, &method.refs, out);
            if !method.is_constructor && !method.is_static && method.visibility != Visibility::Private {
                self.emit_overrides(&fqn, &id, method, &file.path, out);
            }
        }
    }

    fn emit_overrides(&self, fqn: &str, method_id_: &SymbolId, method: &MethodDecl, path: &str, out: &mut Output) {
        let erased: Vec<&str> = method.params.iter().map(|p| last_segment(&p.signature_text)).collect();
        let mut queue: VecDeque<String> = self.supertypes(fqn).0.into_iter().map(|(s, ..)| s).collect();
        let mut seen = BTreeSet::new();
        while let Some(ancestor) = queue.pop_front() {
            if !seen.insert(ancestor.clone()) {
                continue;
            }
            let Some(entry) = self.types.get(&ancestor) else {
                continue;
            };
            for candidate in entry.methods.get(method.name.as_str()).into_iter().flatten().filter(|m| !m.is_static) {
                let candidate_erased: Vec<&str> =
                    candidate.params.iter().map(|p| last_segment(&p.signature_text)).collect();
                if candidate_erased != erased {
                    continue;
                }
                // Same simple names but differently qualified spellings could be different types.
                let evidence = if candidate.signature() == method.signature() {
                    Evidence::ResolvedExact
                } else {
                    Evidence::StaticInferred
                };
                out.edge(
                    method_id_,
                    &method_id(&entry.id, candidate),
                    EdgeKind::Overrides,
                    evidence,
                    path,
                    method.span.start_line,
                    "java.override",
                );
            }
            queue.extend(self.supertypes(&ancestor).0.into_iter().map(|(s, ..)| s));
        }
    }

    fn emit_type_ref(&self, scope: &Scope<'_>, from: &SymbolId, ty: &TypeUse, rule: &str, out: &mut Output) {
        if let TypeRes::Internal { fqn, evidence } = self.resolve_in_scope(scope, &ty.name)
            && let Some(id) = self.type_id(&fqn)
        {
            out.edge(from, id, EdgeKind::References, evidence, &scope.file.path, ty.line, rule);
        }
    }

    fn emit_refs(&self, scope: &Scope<'_>, from: &SymbolId, refs: &[BodyRef], out: &mut Output) {
        let path = &scope.file.path;
        for reference in refs {
            scope.fuel.set(RECEIVER_FUEL);
            match reference {
                BodyRef::Call { receiver, name, arity, line } => {
                    match self.resolve_call(scope, receiver, name, Some(*arity), 0) {
                        CallRes::Targets { methods, evidence, rule } => {
                            for (owner, method) in methods {
                                if let Some(target) = self.method_symbol(&owner, method) {
                                    out.edge(from, &target, EdgeKind::Calls, evidence, path, *line, rule);
                                }
                            }
                        }
                        CallRes::Accessor { owner, field, evidence } => {
                            if let Some(target) = self.field_symbol(&owner, field) {
                                out.edge(
                                    from,
                                    &target,
                                    EdgeKind::References,
                                    evidence,
                                    path,
                                    *line,
                                    "java.record.accessor",
                                );
                            }
                        }
                        CallRes::Container(_) | CallRes::External => {}
                        CallRes::Unresolved if self.member_names.contains(name.as_str()) => {
                            out.unresolved(from, path, *line, format!("call {name}/{arity}"));
                        }
                        CallRes::Unresolved => {}
                    }
                    self.emit_receiver_field_refs(scope, from, receiver, *line, out);
                }
                BodyRef::ConstructorCall { on_super, arity, line } => {
                    let target = if *on_super {
                        self.supertypes(scope.type_fqn)
                            .0
                            .into_iter()
                            .find(|(_, kind, ..)| *kind == EdgeKind::Extends)
                            .map(|(fqn, ..)| fqn)
                    } else {
                        Some(scope.type_fqn.to_owned())
                    };
                    if let Some((targets, evidence)) = target.and_then(|fqn| self.constructors(&fqn, *arity)) {
                        for id in targets {
                            out.edge(from, &id, EdgeKind::Calls, evidence, path, *line, "java.call.constructor");
                        }
                    }
                }
                BodyRef::New { ty, arity } => match self.resolve_in_scope(scope, &ty.name) {
                    TypeRes::Internal { fqn, evidence } => {
                        if let Some((targets, ctor_evidence)) = self.constructors(&fqn, *arity) {
                            for id in targets {
                                out.edge(
                                    from,
                                    &id,
                                    EdgeKind::Instantiates,
                                    weaker(evidence, ctor_evidence),
                                    path,
                                    ty.line,
                                    "java.new",
                                );
                            }
                        }
                    }
                    TypeRes::External => {}
                    TypeRes::Unresolved => out.unresolved(from, path, ty.line, format!("new {}", ty.name)),
                },
                BodyRef::FieldAccess { receiver, name, line } => {
                    if let RecvType::Internal { fqn, evidence } = self.receiver_type(scope, receiver, 0)
                        && let Lookup::Found(found) = self.find_field(&fqn, name)
                    {
                        for (owner, field) in found {
                            if let Some(target) = self.field_symbol(&owner, field) {
                                out.edge(
                                    from,
                                    &target,
                                    EdgeKind::References,
                                    evidence,
                                    path,
                                    *line,
                                    "java.field.access",
                                );
                            }
                        }
                    }
                }
                BodyRef::Name { name, line } => {
                    if Self::local_type(scope, name).is_some() {
                        continue;
                    }
                    if let Some((owner, field)) = self.field_in_scope(scope, name)
                        && let Some(target) = self.field_symbol(&owner, field)
                    {
                        out.edge(
                            from,
                            &target,
                            EdgeKind::References,
                            Evidence::ResolvedExact,
                            path,
                            *line,
                            "java.field.unqualified",
                        );
                    }
                }
                BodyRef::MethodRef { receiver, name, line } => {
                    if let RecvType::Internal { fqn, evidence } = self.receiver_type(scope, receiver, 0) {
                        if name == "<init>" {
                            if let Some(entry) = self.types.get(&fqn) {
                                let ctors: Vec<&MethodDecl> =
                                    entry.decl.methods.iter().filter(|m| m.is_constructor).collect();
                                let targets: Vec<SymbolId> = if ctors.is_empty() {
                                    vec![entry.id.clone()]
                                } else {
                                    ctors.iter().map(|m| method_id(&entry.id, m)).collect()
                                };
                                for target in targets {
                                    out.edge(
                                        from,
                                        &target,
                                        EdgeKind::Instantiates,
                                        Evidence::StaticInferred,
                                        path,
                                        *line,
                                        "java.method-ref",
                                    );
                                }
                            }
                        } else if let Lookup::Found(methods) = self.find_methods(&fqn, name, None) {
                            let overload =
                                if methods.len() == 1 { Evidence::ResolvedExact } else { Evidence::StaticInferred };
                            for (owner, method) in methods {
                                if let Some(target) = self.method_symbol(&owner, method) {
                                    out.edge(
                                        from,
                                        &target,
                                        EdgeKind::Calls,
                                        weaker(evidence, overload),
                                        path,
                                        *line,
                                        "java.method-ref",
                                    );
                                }
                            }
                        }
                    }
                }
                BodyRef::Type(ty) => self.emit_type_ref(scope, from, ty, "java.body.type", out),
            }
        }
    }

    /// `repo.save(x)` depends on the field `repo` as well as on `save`: changing the field's
    /// declared type changes which `save` is called.
    fn emit_receiver_field_refs(
        &self,
        scope: &Scope<'_>,
        from: &SymbolId,
        receiver: &Receiver,
        line: u32,
        out: &mut Output,
    ) {
        let name = match receiver {
            Receiver::Name(name) => name,
            Receiver::Field(inner, name) if matches!(**inner, Receiver::This) => name,
            _ => return,
        };
        if Self::local_type(scope, name).is_some() {
            return;
        }
        if let Some((owner, field)) = self.field_in_scope(scope, name)
            && let Some(target) = self.field_symbol(&owner, field)
        {
            out.edge(
                from,
                &target,
                EdgeKind::References,
                Evidence::ResolvedExact,
                &scope.file.path,
                line,
                "java.field.receiver",
            );
        }
    }
}

/// The JDK container a type name denotes in `file`, if any. A simple name counts only when it is
/// imported from the JDK package (singly or on demand), or is `java.lang.Iterable`; a same-named
/// type from another library must not borrow `java.util` semantics. Callers have already ruled out
/// in-repository types of that name.
fn jdk_family(file: &JavaFile, name: &str) -> Option<Family> {
    let lookup = |package: &str, simple: &str| {
        JDK_CONTAINERS.iter().find(|(p, n, _)| *p == package && *n == simple).map(|(_, _, family)| *family)
    };
    if let Some((package, simple)) = name.rsplit_once('.') {
        return lookup(package, simple);
    }
    let single = file.imports.iter().find(|i| !i.is_static && !i.wildcard && last_segment(&i.path) == name);
    if let Some(import) = single {
        let (package, simple) = import.path.rsplit_once('.')?;
        return lookup(package, simple);
    }
    if let Some(family) = lookup("java.lang", name) {
        return Some(family);
    }
    let mut on_demand =
        file.imports.iter().filter(|i| !i.is_static && i.wildcard).filter_map(|i| lookup(&i.path, name));
    on_demand.next()
}

fn is_test_method(method: &MethodDecl) -> bool {
    method.annotations.iter().any(|a| TEST_ANNOTATIONS.contains(&last_segment(&a.name)))
}

enum CallRes<'a> {
    Targets {
        methods: Vec<(String, &'a MethodDecl)>,
        evidence: Evidence,
        rule: &'static str,
    },
    /// The implicit accessor of a record component: the component is the dependency.
    Accessor {
        owner: String,
        field: &'a FieldDecl,
        evidence: Evidence,
    },
    /// A JDK container method returning an element: no in-repo target, but a typed result.
    Container(RecvType),
    External,
    Unresolved,
}
