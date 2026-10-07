# Engineering log

Meaningful discoveries only, newest first.

## 2026-10-07 — Session 2: TypeScript frontend

**First CI run failed on a local blind spot.** `playwright.config.ts` was added after the last local
typecheck and needs Node types. CLAUDE.md now says to re-run web checks after adding any file.

**`beforeEach` is not a test, but it is test code.** Suite-level hooks run for every test in the
file. Their references are attributed to the file symbol, and test files are marked `is_test`, so a
change can reach "run this whole file". The same gap existed in Java: a modified `@BeforeEach
setUp()` had no dependents and recommended nothing. Test *containers* are now recommended when shared
code inside them changes or is impacted (`changed_lifecycle_method_recommends_its_whole_test_class`).

**Adding a `describe` marked the whole test file changed.** The suite wrapper, and even the
statement's trailing `;`, leaked into the file fingerprint. It now covers only module-level code
plus suite-level statements.

**IMPORTS should not propagate impact.** With TypeScript every importing file became "impacted",
flooding the list. Real use of an import already yields CALLS/REFERENCES edges, so IMPORTS (like
CONTAINS) no longer propagates. Imports remain in the graph as evidence.

**Untyped receivers need a noise filter, not silence.** Reporting every call on an untyped value
(`items.reduce`, `res.json`) as unresolved would bury real gaps. A call is reported only when its
member name is declared somewhere in the repository, the case where an edge may truly be missing.

**Security review: three denial-of-service paths in the TS frontend.** (1) `extends a.b.b…` and
`new a.b.b…()` recursed per segment and overflowed the stack. (2) `export *` resolution explored
every path through layered barrels: with 6 re-exports per layer the old resolver did not finish in
120 s; a per-query visited set plus memoised results makes it 10 ms. The first version of that test
used 2 branches per layer and passed against the *old* code too — a test that cannot fail proves
nothing, so it was widened until it reproduced. (3) Several scans made big files quadratic; the
worst, shared with Java, was fingerprint exclusion via `Vec::contains` (50k members: 21 s → 4 s in a
debug build). Diamond re-exports were also wrongly marked ambiguous; providers are now deduplicated.

**Interface dispatch matters in TS too.** `Cart.total` calls `this.prices.quote()` on a `PriceSource`
injected through a constructor parameter property; the change in `PricingService.quote` reaches it
only through the forward `OVERRIDES` hop to the interface member.

## 2026-10-07 — Session 1

**Name collision.** "ImpactTrace" is an active software product (impacttrace.io); "Blastline" is an
npm package in exactly this domain. Renamed to Ripplepath before any public artifact existed.

**Git CLI is an execution vector.** Running `git diff`/`git status` on an untrusted repository can
execute programs configured in that repository (`core.fsmonitor`, `diff.external`, textconv). Chose
`gix` with isolated permissions and computed line diffs ourselves (imara-diff, histogram). Cost: we
own rename detection — implemented as exact-blob then line-Dice ≥ 50% pairing, bounded at 10k pairs.

**Line overlap is the wrong definition of "symbol changed".** Mapping hunks to enclosing symbols
marks a method as changed when only a comment above it moved. Fingerprints over tree-sitter leaf
tokens, excluding comments and nested member spans, give the intended semantics: comment/format-only
edits are not changes, and editing a method does not mark its class. Hunks are still mapped to
innermost symbols, for display only. Pure insertions map to the symbol spanning the gap so code
added after a closing brace is attributed to the class, not the previous method.

**Dispatch needs a forward edge.** Reverse traversal from a changed implementation
(`StandardFeePolicy.feeFor`) finds nothing: callers are bound to the interface method. Following
`OVERRIDES` *forward* during impact (impl → declaration, then reverse from the declaration) recovers
`TransferService.transfer → TransferController.transfer → test`, labelled "dispatch" in paths.

**Deleted symbols live in the base graph.** The caller of a deleted method no longer references it in
head (or does so in broken code). Deleted roots are traversed on the base graph; results are
labelled `graph: base`.

**Stack overflow from a crafted type name (security review finding).** Type erasure recursed through
left-nested `scoped_type_identifier`s; a 100k-segment name overflowed the stack
(`STATUS_STACK_OVERFLOW`, reproduced by the regression test against the old code). Fixed by bounding
recursion; deep receiver chains and type nesting were already capped.

**Two server findings from review.** (1) Binding to 127.0.0.1 does not stop DNS rebinding — added
`Host` validation. (2) A semaphore permit held in the request future is released when the client
disconnects, while the blocking analysis continues; moved the permit into the blocking task.

**Graph UI: isolated changes dominated the canvas.** Changed symbols without dependents (new helper
classes, private fields) formed many tiny components laid out in one strip, shrinking the meaningful
path to unreadable size. Fixed with component packing (ELK aspect ratio) and hiding isolated nodes by
default with a visible count; they remain in the left list.

**Windows line endings.** A global `core.autocrlf=true` would have given fixtures CRLF on checkout and
changed blob ids. `.gitattributes` pins LF; the fixture builder also normalises.
