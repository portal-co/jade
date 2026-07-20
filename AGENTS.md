# Agent notes for working on Jade

This file is for future agents (and humans) touching the JS JIT/interpreter/tenant
infrastructure. It's a map of load-bearing invariants that aren't obvious from reading any
single file — read it before changing anything in `crates/jade-vm-jit*`,
`crates/jade-vm-frontend`, or `packages/jade-js`. See also `goals.md` (object-model/tenant
overview and dispatch architecture) and the docs linked throughout below.

## Three-tier JIT, and where `add_async`/`add_gen` must be threaded

- **Tier 0** (`crates/jade-vm-jit`, always on): block-dispatch loop (`switch (__ip) { ... }`).
- **Tier 1** (`crates/jade-vm-jit`, `reloop` feature): the same per-op emission, restructured
  into real `if`/`while` via `ssa-reloop2`.
- **Tier 2** (`crates/jade-vm-jit-swc`): reconstructs a genuine `swc-cfg` `Func` from the
  bytecode, round-trips it through TAC/SSA (`crates/jade-cfg-opt::optimize_tfunc`), and
  codegens real structured JS via `swc_ecma_codegen`.

`Config.add_async`/`add_gen` (ambient capability flags) are read in exactly one place per
tier's per-op emission (`jade-vm-jit`'s `op_await`/`op_yield`/`op_yieldstar` and the function
wrapper's declared-variant combination) — Tier 2 *reuses that exact per-op emission*
(`ops_to_js`) rather than reimplementing it, so a bug fixed once in `jade-vm-jit` is fixed for
every tier. If you add a new ambient capability, thread it the same way: read once in
`jade-vm-jit`, and make sure Tier 2's `ops_to_stmts`/`build_cfg_func` pass through the
function's own *declared* variant bits separately from the ambient ones (a nested function
has both; the top level only has ambient ones — see "doubleGen" below).

**Nested `Fn` bodies recurse through whichever tier is currently compiling**, via
`Config.nested_body_compiler` (an injected closure) — never silently downgraded to Tier 0.
See `docs/bytecode-cfg-plan.md`'s "Nested closures recurse through the *active* tier"
section. If you add a fourth tier, wire its own recursive closure the same way.

**doubleGen**: a *declared* generator function running under *ambient* `add_gen` — only
reachable for a *nested* function (top-level bytecode has no declared-variant bit of its
own). Its yields get THROUGH-tagged (`{value, [Symbol.for("jade.through")]: true}`) so an
outer consumer can tell a real yield from a pass-through one. See
`crates/jade-vm-jit-swc`'s `nested_gen_fn_runs_via_tier2_under_ambient_add_gen` test.

**Closure capture is not implemented yet** — nested functions only see `tenant`/`nt` and
their own locals, no free-variable capture. See `docs/closure-capture-plan.md` for what a
real implementation needs before you attempt it.

## Tier 2 is a "clean code" tier, not a "readable code" tier

The point of Tier 2's output is that a downstream JS engine's own optimizer can chew on it
effectively (real `if`/`while`/`switch`, no block-dispatch loop, no residual IIFE wrappers —
see below) — not that a human enjoys reading it. Don't "simplify" its output in a way that
reintroduces indirection a JS engine would otherwise inline away itself.

## The tenant ABI and the inlining soundness invariant

`crates/jade-vm-frontend/src/tenant_inline.rs` can splice a tenant method's own body directly
into JIT-compiled code instead of calling `tenant.<method>(...)` (see
`docs/pluggable-tenant-interface-plan.md`). The scanner's rejection rule is narrow and
deliberate:

- **`#private` field/method references** — hard rejection, always. No rewrite can fix a
  private name only reachable from lexically-nested code.
- **Calling a captured external identifier** (a free, non-parameter, non-`this`-derived,
  non-global bare-identifier callee) — hard rejection. The splice site has no way to supply
  "a function it doesn't have."
- **`this` references are *not* a rejection reason.** They're rewritten (via an SWC
  `VisitMut`) into a leading `__this` parameter; `InlinableTenantMethod.needs_tenant_self`
  tells the JIT's splice site (`inline_call` in `crates/jade-vm-jit`) to prepend the real
  `tenant` reference as that argument.

This means the only things a spliced tenant method body can legally reference are its own
parameters, `this`/`__this`, and a small JS-global allow-list (`ALLOWED_GLOBAL_CALLEES`).

**The five ABI/shim helpers — `markGuestFn`, `invokeGuestAware`, `invokeTrap`,
`createGuestGen`, `unpackGuestGen` — are injected onto every `Tenant` implementation**
(`guestAbiMixin` in `packages/jade-js/narrow.ts`, `Object.assign`'d onto `MultiTenant`'s and
`single_tenant`'s prototypes) instead of being free module-level imports. This is *required*
by the invariant above: after the `this`-rewrite, a tenant method can only reach them via
`this.<method>(...)`, never a free import the splice site can't resolve. **Never add any of
these five to `TENANT_METHOD_NAMES`** (`make`, `get`, `set`, `define`, `assign`, `ownKeys`) —
that list is exactly the set of methods ever spliced away; the five ABI/shim methods must
stay real method calls inside every spliced body, or the guest-function-invocation boundary
(`Reflect.apply`/`fn(...)` inside `invokeGuestAware`, etc.) could itself get textually
inlined out of existence.

**IIFE inlining**: the `this`-rewrite produces `(function(__this, ...){ ... })(tenantRef,
...)` splices — a genuine IIFE. `SCfg::inline_iifes` (in the vendored `jsaw-core` dependency,
`crates/swc-ssa/src/simplify.rs`), wired into `jade-cfg-opt::optimize_tfunc`, eliminates
these for Tier 2 when the callee is a straight-line chain of blocks (no branching/loops/
nested closures/non-arrow `this`-or-`arguments`) — anything else is left as an ordinary call
(inlining is an optimization, never required for correctness). See
`docs/pluggable-tenant-interface-plan.md`'s addendum for the full design, and
`crates/jade-vm-jit-swc`'s `tenant_method_this_rewrite_iife_is_inlined_by_tier2` test for the
end-to-end proof. `jsaw-core` is a genuinely separate GitHub repo (`portal-co/jsaw-core`,
patched to a local path via `.cargo/config.toml`) — treat changes there with the same care as
any other externally-hosted dependency: don't push without the user reviewing the diff first.

## The guest/host type boundary (`packages/jade-js/rewrite.ts`)

`HostToGuest<T, AA, AG>` / `GuestToHost<T, AA, AG>` (recursive conditional types) are the
source of truth for how a value's *type* changes crossing the guest/host boundary — a
function's parameters flow the opposite direction from its return, `Promise`/`Generator`/
`AsyncGenerator` unwrap-and-rewrap their own type parameter, and `AA`/`AG` (the same
`addAsync`/`addGen` ambient-upgrade convention as `Config.add_async`/`add_gen` on the Rust
side) combine with whatever the declared return shape already is via `FnResult`, never
double-wrapping. `hostToGuest`/`guestToHost` are the runtime counterpart, typed against these
so a hand-written guest-side stub can be typechecked directly against `HostToGuest<T>`.

`narrow.ts` (validation/`NarrowSpec`) and `rewrite.ts` (conversion) are deliberately separate
modules — don't merge them back together.

## Tenant-scoped primordials (`packages/jade-js/primordials/`)

Primordials are tenant-owned values, never ambient host intrinsics. Every
factory caches by the **exact** execution-facing `Tenant`; values for a
`MergedTenant` must retain and operate through that router. Guest-visible
function shells, prototypes, descriptor objects, and buffer shells are
allocated through tenant operations, and guest callbacks use `tenant.invoke`.

`BufferHooks` is an explicit embedder capability. `ArrayBuffer`, optional
`SharedArrayBuffer`, and typed arrays are absent unless `createPrimordialRealm`
receives hooks. Hooks may be asynchronous; compose them with
`yield tenant.yieldTenant(...)`, never expose raw handles. `nativeBufferHooks`
is an opt-in adapter, not a host fallback. Reject unsupported behavior instead
of using host property access, host `Reflect.apply`, native `Proxy`, or ambient
intrinsics. Dynamic `Function`/`new Function`, primitive boxing, and BigInt
arrays are deliberately unavailable.

Native exotic traps remain fail-closed. The guest `Proxy` installs every native
exotic trap and only then performs guest-handler fallback to its target.
`Proxy.revocable` has private revocation state and relies on normal GC.
Descriptor/prototype/extensibility methods, including `Object.seal` and
`Object.freeze`, use explicit Tenant methods; do not shortcut through host
reflection.

## Tenant generator-driver protocol (`packages/jade-js/tenants/driver.ts`)

All tenant operations (`make`, `get`, `set`, `has`, `delete`, `ownKeys`, `define`,
`assign`) are **generators**. The VM, the JIT, and the WASM backend never call
`.next()` directly; they compose every tenant method through
`tenant.driveTenant(gen, addAsync, addGen)`. Inside tenant methods, nested
tenant calls are composed with `yield this.yieldTenant(innerGen)`.

`driveTenant`/`yieldTenant` are part of `guestAbiMixin` (`packages/jade-js/narrow.ts`)
— injected onto every `Tenant` implementation like the five earlier ABI/shim
helpers. **They must stay `this`-derived method calls:** never free imports at VM
or JIT call sites, and **never** add them to `TENANT_METHOD_NAMES`
(`crates/jade-vm-frontend/src/tenant_inline.rs`). If they were treated as
inlinable tenant methods, the driver could be spliced into its own body.

The driver composes nested `TenantOp` sentinels, **explicitly tagged
`HostTask` yields**, and native iterators consistently with the
 declared-bits-OR-ambient-bits rule already used by `Config.add_async`/`add_gen`
and `FnResult`. It never probes `.then`, awaits a raw native `Promise`, or
assimilates a guest promise/thenable: those are ordinary guest data.  A host
provider that needs suspension must create a task with its explicit
`HostAsyncCapability` and yield it via `this.yieldHostTask(capability, task)`.
The standard realm requires that capability to install its guest `Promise`;
`hostTaskToPromise` is the separately exported opt-in host interoperability
adapter, never a Jade runtime return type. Thread the flags at exactly one
emission site per tier, the same as `op_await`/`op_yield`/`op_yieldstar`.

For inlining: real tenant methods are generators, so the JIT emits a
`function*` IIFE and wraps it with `tenant.driveTenant(...)` at the call site.
The inlined body itself uses `__this.yieldTenant` / `__this.driveTenant` after
`this`-rewrite. See `docs/tenant-generator-driver.md` and
`docs/pluggable-tenant-interface-plan.md` for the full design.

- If you add a new tenant method, make it a generator and add it to the
  `Tenant` interface in `packages/jade-js/index.ts`.
- If you change `packages/jade-data/index.ts` handlers, run `scripts/regen.ts`
  to update `packages/jade-js/vm.ts`.
- If you update the inlining rules, preserve the `is_generator` flag on
  `InlinableTenantMethod` and the "never add driver helpers to
  `TENANT_METHOD_NAMES`" invariant.
