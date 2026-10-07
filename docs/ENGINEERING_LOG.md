# Engineering log

Meaningful discoveries only, newest first.

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
