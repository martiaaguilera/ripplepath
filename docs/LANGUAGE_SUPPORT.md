# Language support

| Language | Status |
|---|---|
| Java | **Implemented** (static subset below) |
| TypeScript / JavaScript | **Implemented** (static subset below) |
| Python | Not planned until Java and TypeScript are strong |

## Java

Parser: tree-sitter-java 0.23.5. Extraction runs under a per-file time budget; files with syntax
errors are still indexed (tree-sitter recovers) and flagged.

The grammar rejects record patterns with a qualified head (`case Outer.Rec(var x) ->`,
`o instanceof Outer.Rec(var x)`), which is valid Java 21. When a file has syntax errors, it is
re-parsed with the dots of such heads replaced by `_` (same length, so every offset and line still
matches the original text, from which all names are read); the re-parse is kept only if it has
fewer error lines. Nested qualified patterns (`case A(B.C(var x))`) are not rewritten and remain
syntax errors.

### Symbols
Packages (as `module`), files, classes, interfaces, enums, records, annotation types (including
nested types as `Outer.Inner`), methods, constructors (`<init>`), fields, enum constants, record
components (each fingerprinted on its own declaration, so adding a method to a record does not
modify its components). Test methods: annotated `@Test`, `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`,
`@TestTemplate` (JUnit 4/5, TestNG `@Test`).

Identity: `java:<package>.<Type>#<member>(<erased param types>)`. See docs/SPEC.md §2.

### Resolved references
- Type names: nested/member types of enclosing types, single-type imports, same package, on-demand
  (`*`) imports, fully qualified names, type parameters (ignored).
- Supertypes: `extends`, `implements`, interface `extends`.
- Calls: unqualified (enclosing type hierarchy, then outer types, then static imports), `this.`,
  `super.`, locals and parameters with declared types, fields (including `this.field`), static
  calls on types, chained calls through declared return types, `new T(..).m()`, casts, method
  references.
- `var`: typed from a `new`/cast initialiser, or from the declared type of whatever the initialiser
  resolves to (`var job = repo.find(id)`, rule `java.call.var-inferred`). Primitives are never
  typed this way. Record pattern components (`case Rec(var a, var b)`) take the record component's
  declared type.
- Records: `rec.c()` with no declared `c()` is the implicit accessor of component `c`: a
  `REFERENCES` edge to the component (rule `java.record.accessor`), and chained calls are typed by
  the component's type. Enums inherit `java.lang.Enum`: `name()`, `ordinal()`, `values()`,
  `valueOf()` are external, not unresolved.
- JDK containers: element types of `java.util` collections (`List`, `Set`, `Queue`, `Deque` and
  their common implementations), `Map` (values and keys), `Optional`, `java.lang.Iterable` and
  `java.util.stream.Stream` reached from them, taken from the declared type arguments:
  `list.get(i)`, `getFirst()`, `map.get(k)`, `map.values()`, `opt.orElseThrow()`,
  `stream().filter(..).findFirst()`, for-each over them (`for (var j : jobs)`), and parameters of
  lambdas passed to `forEach`/`removeIf`/`filter`/`map`/`anyMatch`/... and `Map.forEach`/`compute*`.
  These signatures come from a fixed table, not from code the tool reads, so every edge typed
  through them is `STATIC_INFERRED`. The simple name must be imported from the JDK package
  (singly or on demand); a `List` from another library gets no JDK semantics.
- Constructors: `new T(..)` by arity (implicit default constructor → the type), `this(..)`/`super(..)`.
- Fields: `this.f`, `obj.f`, unqualified field names, `Type.CONSTANT`.
- Overrides: same name and erased parameter simple names, across the in-repo hierarchy.

### Known limitations (honest list)
- **Not a compiler.** No full overload resolution: candidates are matched by name and arity; ties
  produce multiple `STATIC_INFERRED` edges rather than a guess.
- **Generics beyond the JDK table are erased.** Only top-level type arguments are kept
  (`Map<UUID, List<Job>>` gives `List`, not `List<Job>`), type parameters of in-repo generic
  types are not substituted, and `new ArrayList<Job>()` keeps no element type.
- Lambda parameters are typed only for the container methods above. Lambdas passed to other
  APIs (AssertJ `satisfies(x -> ...)`, JDBC `(rs, n) -> ...`, Spring callbacks) are untyped.
- Block scopes inside a method are flattened. A local name declared more than once is typed only
  when every declaration gives the same type; otherwise it is unknown (and calls on it are
  reported), never "whichever came last".
- A call on an untyped receiver is reported as unresolved only when some repository type declares
  a method (or record component) with that name; otherwise no repository edge can be missing.
- Receiver typing has a per-reference step budget (256); a chain that exhausts it is unknown.
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
  (Not yet needed by a dogfooding target: QuantaRun's console uses relative imports only.)
- CommonJS (`require`, `module.exports`) and dynamic `import()` are not resolved.
- Properties of an object literal assigned to a module-level `const`/`let`/`var` are members of that
  variable (`export const api = { jobs: () => ..., cancel(id) {...} }` gives `ts:path#api.jobs`,
  `ts:path#api.cancel`), through `as const`, `satisfies T` and parentheses. A property reached
  through a spread (`{ ...base }`) or a computed key is not a member: such accesses are unresolved,
  not guessed. Nested object literals are not split further. Object literals elsewhere (arguments,
  return values) and prototype assignment are not symbols.
- Array/tuple annotations (`Row[]`, `[A, B]`, `readonly T[]`) and primitive annotations (`string`,
  `number`, ...) mark a value as external; `any`, `unknown` and `object` leave it untyped.
- Duplicate test titles in a file are disambiguated by order (`title #2`); test titles built from
  template substitutions get a line-based name and therefore no stable identity.
