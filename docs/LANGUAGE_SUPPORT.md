# Language support

| Language | Status |
|---|---|
| Java | **Implemented** (static subset below) |
| TypeScript / JavaScript | Planned (next) |
| Python | Not planned until Java and TypeScript are strong |

## Java

Parser: tree-sitter-java 0.23.5. Extraction runs under a per-file time budget; files with syntax
errors are still indexed (tree-sitter recovers) and flagged.

### Symbols
Packages (as `module`), files, classes, interfaces, enums, records, annotation types (including
nested types as `Outer.Inner`), methods, constructors (`<init>`), fields, enum constants, record
components. Test methods: annotated `@Test`, `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`,
`@TestTemplate` (JUnit 4/5, TestNG `@Test`).

Identity: `java:<package>.<Type>#<member>(<erased param types>)`. See docs/SPEC.md §2.

### Resolved references
- Type names: nested/member types of enclosing types, single-type imports, same package, on-demand
  (`*`) imports, fully qualified names, type parameters (ignored).
- Supertypes: `extends`, `implements`, interface `extends`.
- Calls: unqualified (enclosing type hierarchy, then outer types, then static imports), `this.`,
  `super.`, locals and parameters with declared types, `var` with `new`/cast initialisers, fields
  (including `this.field`), static calls on types, chained calls through declared return types,
  `new T(..).m()`, casts, method references.
- Constructors: `new T(..)` by arity (implicit default constructor → the type), `this(..)`/`super(..)`.
- Fields: `this.f`, `obj.f`, unqualified field names, `Type.CONSTANT`.
- Overrides: same name and erased parameter simple names, across the in-repo hierarchy.

### Known limitations (honest list)
- **Not a compiler.** No full overload resolution: candidates are matched by name and arity; ties
  produce multiple `STATIC_INFERRED` edges rather than a guess.
- **Generics are erased.** `List<Account> xs; xs.get(0).withdraw()` cannot type the receiver
  (`List` is external) — the call is reported as unresolved, never guessed.
- Lambda parameters with inferred types are untyped receivers (unresolved).
- Block scopes inside a method are flattened; shadowing in nested blocks may mistype a receiver.
- Members inherited from *external* supertypes (e.g. a framework base class) are external.
- Reflection, dependency injection, proxies and annotation-processor-generated code create runtime
  edges this analysis cannot see.
- Bare identifiers that are neither locals nor fields (e.g. enum constants in `switch` labels) are
  not counted as unresolved, to avoid noise; only unresolved calls and instantiations are.
- Two types with the same fully qualified name in one snapshot (e.g. multi-module builds) get
  path-qualified ids (`…@path`), and only the first by path is indexed.
- Anonymous and local classes are attributed to their enclosing method.
- Kotlin, Groovy and Scala sources in the same repository are not analysed.
