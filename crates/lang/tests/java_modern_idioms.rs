#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Modern Java idioms found while dogfooding on a Spring Boot service (docs/DOGFOODING_QUANTARUN.md):
//! records, `var`, enums, JDK containers and stream lambdas. Each test is a minimal reproduction
//! written for this suite, not code copied from the analysed project.

use std::time::{Duration, Instant};

use ripplepath_core::{Edge, EdgeKind, Evidence, SymbolId};
use ripplepath_lang::LanguageGraph;
use ripplepath_lang::java;

const BUDGET: Duration = Duration::from_secs(5);

/// `Job` record, `Status` enum and a `Repo` returning them in several shapes.
const MODEL: &[(&str, &str)] = &[
    (
        "p/Job.java",
        "package p;\nimport java.util.UUID;\npublic record Job(UUID id, Status status, Owner owner) {\n  public void cancel() {}\n  public boolean live() { return status().isFinal(); }\n}\n",
    ),
    ("p/Owner.java", "package p;\npublic record Owner(String name) {\n  public String name() { return name; }\n}\n"),
    (
        "p/Status.java",
        "package p;\npublic enum Status {\n  QUEUED, DONE;\n  public boolean isFinal() { return this == DONE; }\n}\n",
    ),
    (
        "p/Repo.java",
        "package p;\nimport java.util.*;\npublic class Repo {\n  public Job find(UUID id) { return null; }\n  public Optional<Job> lookup(UUID id) { return Optional.empty(); }\n  public List<Job> all() { return List.of(); }\n  public Map<UUID, Job> byId() { return Map.of(); }\n  public int count() { return 0; }\n}\n",
    ),
];

fn graph(extra: &[(&str, &str)]) -> LanguageGraph {
    let facts: Vec<_> =
        MODEL.iter().chain(extra).map(|(path, source)| java::extract(path, source, BUDGET).unwrap()).collect();
    java::resolve(&facts.iter().collect::<Vec<_>>())
}

fn edge<'g>(g: &'g LanguageGraph, from: &str, to: &str, kind: EdgeKind) -> Option<&'g Edge> {
    g.edges.iter().find(|e| e.from == SymbolId::new(from) && e.to == SymbolId::new(to) && e.kind == kind)
}

fn unresolved(g: &LanguageGraph) -> Vec<String> {
    g.unresolved.iter().map(|u| format!("{}:{}", u.line, u.detail)).collect()
}

#[test]
fn record_accessors_reference_the_component_and_type_chained_calls() {
    let user = "package p;\nclass User {\n  void run(Job job) {\n    job.id();\n    job.status().isFinal();\n    job.owner().name();\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    let from = "java:p.User#run(Job)";

    let id = edge(&g, from, "java:p.Job#id", EdgeKind::References).expect("implicit accessor");
    assert_eq!((id.evidence, id.rule.as_str(), id.line), (Evidence::ResolvedExact, "java.record.accessor", 4));
    assert!(edge(&g, from, "java:p.Status#isFinal()", EdgeKind::Calls).is_some(), "typed through the component");
    // An explicitly declared accessor is an ordinary method, not the component.
    assert!(edge(&g, from, "java:p.Owner#name()", EdgeKind::Calls).is_some());
    assert!(edge(&g, from, "java:p.Owner#name", EdgeKind::References).is_none());
    // Inside the record, the unqualified accessor resolves too.
    assert!(edge(&g, "java:p.Job#live()", "java:p.Job#status", EdgeKind::References).is_some());
    assert_eq!(unresolved(&g), Vec::<String>::new());
}

#[test]
fn enum_members_inherited_from_java_lang_enum_are_external() {
    let user = "package p;\nclass User {\n  String run(Status s) {\n    Status.values();\n    Status.valueOf(\"DONE\");\n    s.ordinal();\n    return s.name();\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    assert_eq!(unresolved(&g), Vec::<String>::new());
}

#[test]
fn var_is_typed_from_the_declared_type_of_its_initializer() {
    let user = "package p;\nimport java.util.UUID;\nclass User {\n  Repo repo;\n  void run(UUID id) {\n    var job = repo.find(id);\n    job.cancel();\n    var copy = job;\n    copy.live();\n    var n = repo.count();\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    let from = "java:p.User#run(UUID)";
    let cancel = edge(&g, from, "java:p.Job#cancel()", EdgeKind::Calls).expect("var from call");
    assert_eq!((cancel.evidence, cancel.rule.as_str()), (Evidence::ResolvedExact, "java.call.var-inferred"));
    assert!(edge(&g, from, "java:p.Job#live()", EdgeKind::Calls).is_some(), "var from another var");
    assert_eq!(unresolved(&g), Vec::<String>::new());
}

#[test]
fn primitive_locals_are_not_typed_from_their_initializer() {
    // `int` has no declared class type either; only the `var` keyword may borrow the initializer's.
    let user = "package p;\nimport java.util.UUID;\nclass User {\n  Repo repo;\n  void run(UUID id) {\n    int job = repo.count();\n  }\n}\n";
    let file = java::extract("p/User.java", user, BUDGET).unwrap();
    let local = file.types[0].methods[0].locals.iter().find(|l| l.name == "job").unwrap();
    assert_eq!((local.ty.as_ref(), local.init.as_ref()), (None, None));
}

#[test]
fn jdk_container_elements_are_typed_and_labelled_inferred() {
    let user = "package p;\nimport java.util.*;\nclass User {\n  Repo repo;\n  void run(UUID id, List<Job> jobs, Map<UUID, Job> index) {\n    index.get(id).cancel();\n    repo.lookup(id).orElseThrow().cancel();\n    for (var j : jobs) { j.live(); }\n    jobs.getFirst().status();\n    repo.byId().values().forEach(v -> v.cancel());\n    jobs.stream().filter(s -> s.live()).findFirst().get().owner();\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    let from = "java:p.User#run(UUID,List,Map)";
    let cancel = edge(&g, from, "java:p.Job#cancel()", EdgeKind::Calls).expect("map value / optional / lambda");
    assert_eq!(cancel.evidence, Evidence::StaticInferred);
    for (target, kind) in [
        ("java:p.Job#live()", EdgeKind::Calls),
        ("java:p.Job#status", EdgeKind::References),
        ("java:p.Job#owner", EdgeKind::References),
    ] {
        let e = edge(&g, from, target, kind).unwrap_or_else(|| panic!("missing {target}: {:#?}", g.edges));
        assert_eq!(e.evidence, Evidence::StaticInferred, "{target}");
    }
    assert_eq!(unresolved(&g), Vec::<String>::new());
}

#[test]
fn a_same_named_container_from_another_library_gets_no_jdk_semantics() {
    let user = "package p;\nimport com.other.List;\nclass User {\n  void run(List<Job> jobs) {\n    jobs.get(0).cancel();\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    assert!(edge(&g, "java:p.User#run(List)", "java:p.Job#cancel()", EdgeKind::Calls).is_none(), "{:#?}", g.edges);
}

#[test]
fn locals_declared_twice_with_different_types_are_not_guessed() {
    // Scopes are flattened per method: `x` is a Job in one block and an Owner in the other.
    let user = "package p;\nimport java.util.*;\nclass User {\n  void run(Job a, Owner b) {\n    { var x = a; x.cancel(); }\n    { var x = b; x.name(); }\n    for (var j : List.of(a)) {}\n    { var y = a; y.live(); }\n    { var y = a; y.live(); }\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    let from = "java:p.User#run(Job,Owner)";
    assert!(edge(&g, from, "java:p.Job#cancel()", EdgeKind::Calls).is_none());
    assert!(edge(&g, from, "java:p.Owner#name()", EdgeKind::Calls).is_none());
    assert_eq!(unresolved(&g), vec!["5:call cancel/0", "6:call name/0"]);
    // Two declarations that agree on the type are fine.
    assert!(edge(&g, from, "java:p.Job#live()", EdgeKind::Calls).is_some());
}

#[test]
fn untyped_calls_to_names_no_repository_type_declares_are_not_unresolved() {
    // `rs` is an untyped lambda parameter of an external API. No repository type declares
    // `getString`, so no repository edge can be missing; `cancel` exists, so that one is reported.
    let user = "package p;\nclass User {\n  void run(Lib lib) {\n    lib.query(rs -> rs.getString(1));\n    lib.each(j -> j.cancel());\n  }\n}\n";
    let g = graph(&[("p/User.java", user)]);
    assert_eq!(unresolved(&g), vec!["5:call cancel/0"]);
}

#[test]
fn many_same_named_locals_cannot_make_receiver_typing_exponential() {
    // `v0` is declared 8 ways, each from `v1`, which is declared 8 ways from `v2`, and so on. Every
    // declaration agrees on the type, so each must be resolved: 8^20 paths without a step budget.
    let mut body = String::new();
    for i in 0..20 {
        for k in 0..8 {
            let method = if k % 2 == 0 { "next" } else { "other" };
            body.push_str(&format!("    {{ var v{i} = v{}.{method}(); }}\n", i + 1));
        }
    }
    body.push_str("    v0.next();\n");
    let user = format!(
        "package p;\nclass Node {{\n  Node next() {{ return this; }}\n  Node other() {{ return this; }}\n}}\nclass User {{\n  void run(Node v20) {{\n{body}  }}\n}}\n"
    );
    let started = Instant::now();
    let g = graph(&[("p/User.java", &user)]);
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    // Short chains resolve; the budget only cuts off the deep end.
    assert!(edge(&g, "java:p.User#run(Node)", "java:p.Node#next()", EdgeKind::Calls).is_some());
}

#[test]
fn qualified_record_patterns_parse_and_type_their_components() {
    // tree-sitter-java 0.23.5 rejects a qualified record pattern head; the frontend re-parses with
    // the dots flattened. Spans and names must still come from the original text.
    let outcome = "package p;\npublic sealed interface Outcome {\n  record Done(Job job, int code) implements Outcome {}\n  record Lost(String why) implements Outcome {}\n}\n";
    let user = "package p;\nclass User {\n  int run(Outcome o) {\n    if (o instanceof Outcome.Lost(var why)) { return 0; }\n    return switch (o) {\n      case Outcome.Done(var job, var code) when code > 0 -> { job.cancel(); yield code; }\n      case Outcome.Done(Job job, int code) -> code;\n      default -> 1;\n    };\n  }\n}\n";
    let file = java::extract("p/User.java", user, BUDGET).unwrap();
    assert_eq!(file.syntax_error_lines, Vec::<u32>::new());
    let g = graph(&[("p/Outcome.java", outcome), ("p/User.java", user)]);
    let from = "java:p.User#run(Outcome)";
    let cancel = edge(&g, from, "java:p.Job#cancel()", EdgeKind::Calls).expect("component typed from the record");
    assert_eq!((cancel.evidence, cancel.line), (Evidence::ResolvedExact, 6));
    for record in ["java:p.Outcome.Done", "java:p.Outcome.Lost"] {
        assert!(edge(&g, from, record, EdgeKind::References).is_some(), "{record}");
    }
    assert_eq!(unresolved(&g), Vec::<String>::new());
}

#[test]
fn files_with_other_syntax_errors_are_still_flagged() {
    let src =
        "package p;\nclass Broken {\n  int f(Object o) { return o instanceof A.B(var x) ? 1 : 0; }\n  void g( {\n}\n";
    let file = java::extract("p/Broken.java", src, BUDGET).unwrap();
    assert!(file.syntax_error_lines.contains(&4), "{:?}", file.syntax_error_lines);
}

#[test]
fn adding_a_method_to_a_record_does_not_modify_its_components() {
    let before = "package p;\npublic record Settings(int retries, String name) {\n}\n";
    let after = "package p;\npublic record Settings(int retries, String name) {\n  public int twice() { return retries * 2; }\n}\n";
    let renamed = "package p;\npublic record Settings(int retries, String label) {\n}\n";
    let fingerprints = |src: &str| {
        let file = java::extract("p/Settings.java", src, BUDGET).unwrap();
        let g = java::resolve(&[&file]);
        let fp = |id: &str| g.symbols.iter().find(|s| s.id == SymbolId::new(id)).map(|s| s.fingerprint);
        (fp("java:p.Settings#retries"), fp("java:p.Settings#name"))
    };
    assert_eq!(fingerprints(before), fingerprints(after));
    // Renaming one component leaves the other untouched.
    assert_eq!(fingerprints(before).0, fingerprints(renamed).0);
}
