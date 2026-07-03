# Plan: move Jade bytecode to a CFG-native representation

## Current state

Jade bytecode (`packages/jade-data/index.ts`, generated into
`crates/jade-vm/src/data.rs` and `crates/jade-vm-core/src/dispatch.rs`) is a
**structured, nested** format: `IF`/`WHILE`/`SWITCH` each embed their body as a
length-prefixed byte blob, executed by recursive descent (`exec_op` in
`jade-vm-core::dispatch`, and the equivalent recursive codegen in
`jade-vm-jit`). There is no `JMP`/`GOTO` opcode and no way to jump into the
middle of a body from outside it. `RET` is only valid as the very last
operation of the *whole* function — `exec_op`'s dispatch has no arm for it,
since nested bodies are driven by a loop that only knows how to run "the next
op" (see `crates/jade-vm-core/src/dispatch.rs`'s `Operation::While`/`If`/
`Switch` arms, each recursing into `exec_op` for their own body bytes).

`crates/jade-vm-frontend` compiles JS source to this format by:

1. Parsing to a `swc_ecma_ast::Function` and lowering through `portal-jsc-swc-tac`
   (which internally goes through `portal-jsc-swc-cfg`) to get a genuine CFG
   (`TCfg`: basic blocks with real jump/branch/switch terminators, arbitrary
   graph shape, back edges and all).
2. Running `ssa-reloop2` (a from-scratch Stackifier, in
   `codegen-utils/crates/ssa-reloop2`) over that CFG to *re-impose* nested
   structure: it turns arbitrary control flow back into `Simple`/`Loop`/
   `Multiple` nodes, synthesizing `SWITCH`-based dispatch (a "cff" — control
   flow flag — slot) wherever a block has more than one non-trivial forward
   successor.
3. Walking that structured tree, emitting Jade opcodes directly.

## Why this is fragile

Structuring an arbitrary CFG into a strictly-nested, `goto`-free tree is
exactly the Relooper/Stackifier problem, and it has a genuine hard case: **a
block with zero further successors** (a `return`, in TAC terms) looks
identical, from the structuring algorithm's point of view, to any other block
that's "done." `ssa-reloop2`'s reconverge heuristic (`find_reconverge`)
assumes that when a branch region is scanned and every remaining block is
already "owned" by some other branch, the *last* owned entry is the natural
join point, and everything after it is the unconditional tail. That
assumption is exactly backwards when a `return`'s value needs to *not* be a
shared tail — see `reject_return_inside_loop`'s doc comment in
`crates/jade-vm-frontend/src/lib.rs` for the specific shape this breaks
(`return` reached via a branch nested inside a loop body: the return's own
content ends up neither in its own dispatch arm nor in the loop's `next` —
it's silently dropped).

The frontend works around the *milder* version of this (a plain, non-loop
`if (c) { return a; } else { return b; }`, where the hoisted-into-`next`
content just needs to be guarded so it doesn't overwrite an already-returned
value) with a `not_returned_flag` that gates every continuation. But this is
a patch on top of a structural mismatch, not a fix: it only works because a
"return the flag hasn't fired yet" check is representable as another `IF`.
The loop-nested case doesn't have even that escape hatch, because the content
is missing outright, not merely misplaced. Fully general support for
`return`/`break`/`continue`/exceptions from arbitrary nesting depth, on top
of a bytecode format with no `goto`, is exactly the kind of thing that
motivates rewriting the *target* format instead of hardening the structuring
step further.

## Proposed direction

Give Jade bytecode a second, CFG-native representation:

- A function body is a **flat array of basic blocks**, each block a sequence
  of the existing value-producing ops (`LIT32`, `BOOL`, `SEL`, `EQ`/`NE`/…,
  `GET`/`SET`, `CALL`, `ARR`/`STR`/`LITOBJ`, `AWAIT`/`YIELD`, `FN`, …) ending
  in one of:
  - `JMP <block>`
  - `CONDJMP <cond> <block> <block>`
  - `SWITCH <val> <(case, block)...> <default: block>`
  - `RET <val>`
- Execution becomes a **trampoline over block index** rather than recursive
  descent: `exec_op` stops being nested, and the top-level driver
  (`emit_program` in `jade-vm-jit`, the equivalent loop in `jade-vm-wasm`)
  just follows the terminator to the next block index. `RET` becomes valid
  *anywhere*, because there's no "nested body" context left to be inside of.
- This is a strict superset of expressiveness: the existing `WHILE`/`IF`/
  `SWITCH` opcodes' semantics are all directly expressible as
  `JMP`/`CONDJMP`/`SWITCH` between blocks; they'd become a legacy encoding
  (or could be dropped in a breaking bytecode version bump — TBD once real
  usage data exists).
- The frontend's job shrinks correspondingly: TAC's own `TCfg` *already is*
  this shape (`TBlock`s with `TTerm::{Jmp,CondJmp,Switch,Return}`
  terminators, see `portal-jsc-swc-tac`). Compiling to CFG-native Jade
  bytecode becomes close to a 1:1 block-by-block, terminator-by-terminator
  translation, and **the entire `ssa-reloop2` structuring step goes away** —
  along with the class of bugs described above, since there's no reconverge
  heuristic to get wrong.

## Migration sketch

1. Extend `packages/jade-data/index.ts`'s opcode spec with the new
   terminator-carrying block format (or a new "v2" bytecode container
   distinguished by a header byte, so old and new can coexist during
   migration); regenerate `crates/jade-vm/src/data.rs` and
   `crates/jade-vm-core/src/dispatch.rs` via `scripts/gen/vm-ts.ts`.
2. Update `jade-vm-core::dispatch::exec_op` to drive block dispatch instead of
   recursive nested-body execution; `RET`/`AWAIT`/`YIELD`/`YIELDSTAR` move
   from `emit_program`'s special-cased top-level match into ordinary
   per-block terminator handling.
3. Update `jade-vm-jit` codegen to emit a JS `switch`-over-block-index driven
   by a loop (or, more in the spirit of a JIT, structure-detect *simple*
   patterns like a single self-loop back-edge into a native `while` for
   readability/perf, falling back to the block-switch for anything else) —
   this is itself a (much smaller, single-purpose) relooper, but one that
   only needs to produce *efficient* JS, not correct JS; correctness no
   longer depends on getting it right, since the block-switch fallback is
   always available and always correct.
4. Update `jade-vm-wasm` similarly.
5. Rewrite `crates/jade-vm-frontend` to lower `TCfg` directly (drop the
   `ssa-reloop2` dependency, `StructuredBlock` walk, `cff_slot`,
   `not_returned_flag`, `reject_return_inside_loop`, and the arm/dispatch
   plumbing in `lower_terminator` — all of that machinery exists purely to
   paper over the structured-format mismatch).
6. Re-run the frontend's existing test suite
   (`crates/jade-vm-frontend/src/lib.rs`'s `tests` module) against the new
   lowering; the `rejects_return_inside_loop_body` test should flip to
   *accepting and correctly executing* that program.

Not started; this document exists so the `not_returned_flag`/`ssa-reloop2`
workarounds currently in `crates/jade-vm-frontend` are understood as a
deliberate, temporary trade-off rather than the intended long-term shape of
the bytecode.
