# Language support

| Language | Status |
|---|---|
| Java | **Implemented** (static subset below) |
| TypeScript / JavaScript | **Implemented** (static subset below) |
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

## TypeScript / JavaScript

Parser: tree-sitter-typescript 0.23.2. `.ts/.mts/.cts` use the TypeScript grammar; `.tsx` and all
JavaScript (`.js/.jsx/.mjs/.cjs`) use the TSX grammar. Files under `node_modules/` and `*.min.js` /
`*.bundle.js` are not indexed (reported if changed).

### Symbols
Module-level functions (including `const f = () => ...`), variables, classes, class members
(methods, fields, getters/setters, constructor parameter properties such as
`constructor(private repo: Repo)`), interfaces and their members, type aliases, enums. Test cases:
calls to `it`/`test`/`specify` (including `.only`, `.skip`, `.each`) inside files named `*.test.*`,
`*.spec.*` or under `__tests__/`, nested in `describe`/`suite`/`context`.

Identity: `ts:<path>#<Name>`, `ts:<path>#<Class>.<member>`, `ts:<path>#test:<suite> > <title>`.
Overloads, getter/setter pairs and static/instance members with the same name share one identity.

### Resolved references
- Modules: relative specifiers with TypeScript's extension and `index` lookup, ESM-style `./x.js`
  meaning `./x.ts`. Imports: named, default, namespace, `type`-only. Exports: declarations,
  `export { a as b }`, `export default`, re-exports `export { ... } from`, `export * from`
  (ambiguous star exports are `STATIC_INFERRED`), `export * as ns from`.
- Calls and reads through lexical bindings, imports, `this`, `super`, typed parameters/locals/fields,
  `new` initializers, namespace members, static members, and chained calls through declared return
  types. JSX `<Component/>` counts as a call of the component.
- Members are looked up along in-repo `extends`/`implements`; a class member `OVERRIDES` every
  same-named member of its supertypes (TypeScript has no overloading by parameter list).
- Suite-level code (`beforeEach`, fixtures) is attributed to the test file symbol; suite-level
  locals (`let cart: Cart`) are visible in the tests of that suite.

### Known limitations
- **Not a type checker.** No inference: an untyped value (`const x = f()` without annotation, a
  callback parameter, `any`) is an unknown receiver. A call on one is reported as unresolved only if
  some in-repo class/interface declares a member with that name; otherwise it is assumed external
  (`arr.map`, `res.json`) to avoid noise.
- Union, intersection, mapped and conditional types are not followed; type aliases are not expanded.
- `tsconfig.json` `paths`/`baseUrl` aliases and package self-references are treated as external.
- CommonJS (`require`, `module.exports`) and dynamic `import()` are not resolved.
- Object-literal methods and prototype assignment are not symbols.
- Duplicate test titles in a file are disambiguated by order (`title #2`); test titles built from
  template substitutions get a line-based name and therefore no stable identity.
