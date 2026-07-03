# Plan: pluggable tenant interface + conditional inlining

## Current state

JIT-compiled function bodies (`crates/jade-vm-jit`) reference a free variable
`tenant` at every object operation — `tenant.make(null)`, `tenant.get(obj,
key)`, `tenant.set(obj, key, val)`, `tenant.define(target, descriptors)`,
`tenant.assign(dst, src)` (see `JsJit`'s `op_get`/`op_set`/`op_litobj`/
`op_define_properties` in `crates/jade-vm-jit/src/lib.rs`). `tenant` is
threaded as the leading parameter of every emitted function alongside `nt`
(see `FnRegistry::register`'s doc comment and the `emits_function_with_
tenant_nt_params` test). The only implementation today is `MultiTenant` in
`packages/jade-js/multi_tenant.ts`.

This indirection exists so the object model (property storage, trap/
accessor dispatch, proxy-like behavior) is swappable independently of the
bytecode and the JIT — `MultiTenant` is not a fixed part of the ABI, just the
current implementation of it. That's deliberate: the tenant system is still
under active development (see recent history: `g/sets, tenant system 2`,
`finish gc move`), and pinning the JIT's codegen to one concrete class's
internals would make every tenant-side refactor a JIT-side refactor too.

## The interface, made explicit

`tenant` is currently duck-typed — nothing enforces that an object passed as
`tenant` actually implements the methods the JIT emits calls to. Formalize
this as a TS interface in `packages/jade-js` (e.g. `TenantInterface`),
covering exactly the method set the JIT depends on:

```ts
interface TenantInterface {
  make(proto: object | null): object;
  get(obj: object, key: PropertyKey): unknown;
  set(obj: object, key: PropertyKey, value: unknown): void;
  define(target: object, descriptors: object): void;
  assign(dst: object, src: object): void;
  ownKeys(obj: object): PropertyKey[];
}
```

`MultiTenant implements TenantInterface` becomes a compile-time check (TS)
that the class hasn't drifted from what the JIT actually calls; any
alternative tenant (a simpler single-tenant fast path with no proxy/trap
machinery, or a future native-backed implementation) just needs to satisfy
the same shape. `jade-vm-jit`'s Rust side doesn't need to know about this
directly — it only ever emits `tenant.<method>(...)` source text — but its
doc comments and tests should reference the interface by name so the ABI
contract is discoverable from either side.

## Inlining: why it has to be conditional

Calling through `tenant.get`/`tenant.set` on every property access has real
overhead (a real method call plus whatever trap-dispatch logic
`MultiTenant.get`/`set` do internally — see `invokeTrap` in
`packages/jade-js/multi_tenant.ts`). When the *specific* tenant
implementation in use is simple enough (no active traps on the object, or a
tenant implementation that's just a thin wrapper), inlining its method
bodies directly into JIT-compiled code would let the JS engine's own
optimizer see through the whole thing.

But this can't be a static, always-on optimization, for two reasons:

1. **Churn**: the tenant system's internals are still changing. Baking in
   assumptions about `MultiTenant`'s current method bodies at JIT-compile
   time would silently go stale the next time those methods change shape —
   worse, it could silently miscompile (the classic inlining hazard: caching
   a snapshot of behavior that the source of truth has since diverged from).
   So inlining must derive from the tenant's *actual current* source, not a
   copy baked into the JIT.
2. **`#private` fields**: JS private class fields (`#x`) are only accessible
   from code lexically nested inside the declaring class body. A tenant
   method that touches `this.#slots` (for instance) cannot have its body
   spliced out and re-inserted elsewhere as free-standing code — the `#x`
   reference would be a `SyntaxError` (or, if the name happens to collide,
   silently wrong) outside the class. Inlining is only sound for methods that
   don't reference any private member.

## Proposed mechanism

1. Add a `tenant_source: Option<String>` field to `jade-vm-jit::Config`,
   holding the actual source text of the tenant implementation in use (via
   `TenantClass.toString()` at the call site, in the browser — see below for
   why browser-only).
2. Before compiling, if `tenant_source` is present: parse it with SWC (the
   same `swc_ecma_parser` dependency `jade-vm-frontend` already uses),
   extract each method relevant to the interface above (`make`, `get`,
   `set`, `define`, `assign`, `ownKeys`), and for each one, walk its AST
   checking for any `PrivateName` reference (SWC's node for `#x`). A method
   with no private references is *inlinable*; one with any is not — no
   partial/best-effort inlining, to keep the "sound or rejected, never
   silently wrong" invariant `jade-vm-frontend` already follows.
3. For each inlinable method, splice its (parsed, then re-serialized via
   `swc_ecma_codegen`, already a workspace dependency) body directly at each
   call site in the JIT's emitted JS, substituting the call's actual
   argument expressions for the method's parameters (simple textual/AST
   substitution — no need for a general inliner, since call sites are
   JIT-generated and shape-known). Methods that aren't inlinable keep
   emitting the current `tenant.<method>(...)` call form — this is always
   correct, just not optimized, so it's a safe fallback for every method the
   scan rejects (or when `tenant_source` is absent entirely, which must
   remain the default and today's fully-supported path).
4. Scope to actual browsers only, for now: this mechanism depends on
   `Function.prototype.toString()` returning genuine, re-parseable source
   (true for any real JS engine, but not guaranteed for the TS-interpreter
   fallback path or other non-browser hosts in this repo), and on
   `eval`/`new Function`-style dynamic code being available and inspectable.
   Gate it behind a runtime check (e.g. `typeof window !== "undefined"`) so
   non-browser callers unconditionally get the always-correct `tenant.get(…)`
   call form.

## Where this plugs in

- `crates/jade-vm-jit`: `Config` gains `tenant_source`; `op_get`/`op_set`/
  etc. consult a precomputed "is this method inlinable, and if so what's its
  body" table (built once per `compile()` call from `tenant_source`, not
  per-call-site) instead of unconditionally emitting `tenant.<method>(...)`.
- `packages/jade-js`: export the `TenantInterface` type; no runtime change
  needed there beyond `MultiTenant implements TenantInterface`.
- Tests: alongside the existing structural JIT tests
  (`crates/jade-vm-jit/src/lib.rs`'s `tests` module), add cases feeding a
  synthetic tenant source both with and without `#private` fields, asserting
  the private-field one falls back to `tenant.get(...)` call form and the
  plain one gets its body inlined.

Not started; this document is the design to follow when implementing the
`tenant_source`/`#private`-scan/splice work.
