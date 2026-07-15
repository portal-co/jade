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

## Addendum: `this`-rewrite, narrowed rejection rule, and the ABI/shim mixin

`crates/jade-vm-frontend/src/tenant_inline.rs`'s `extract_tenant_methods` (and
`InlinableTenantMethod`/`inline_call` in `crates/jade-vm-jit`) now implement a real
(not just `#private`-checking) version of the scan above:

- **`#private` references** — unchanged, still a hard rejection (no rewrite can fix it).
- **`this` references are no longer a rejection reason.** The method body is parsed,
  every `this` is rewritten (via an SWC `VisitMut`) into a new leading parameter
  (`__this`), and the body is re-serialized (`swc_ecma_codegen`) rather than byte-sliced
  verbatim. `InlinableTenantMethod` gained a `needs_tenant_self: bool` field so the JIT's
  splice site (`op_get`/`op_set` in `jade-vm-jit`) knows to prepend the real `tenant`
  reference as that leading argument, and to expect one more param than the bare
  operation's own arity.
- **Calling a captured external identifier as a function** (a free, non-parameter,
  non-`this`-derived, non-global bare-identifier callee — e.g. a stray module-level
  import used as a callee) is now the *other* hard rejection, alongside `#private` — the
  JIT's splice site has no way to supply "a function we don't have."

Together these mean a tenant method's only remaining legal references, once inlined, are:
its own parameters, `this` (now `__this`, an ordinary parameter), and a small allow-list of
JS globals (`Reflect`, `Object`, `Array`, `WeakMap`, `Symbol`, ...; see
`ALLOWED_GLOBAL_CALLEES` in `tenant_inline.rs`).

This is also why `markGuestFn`/`invokeGuestAware`/`invokeTrap` (`packages/jade-js/
narrow.ts`) and `createGuestGen`/`unpackGuestGen` (`packages/jade-js/shims.ts`) are now
injected onto every `Tenant` implementation as real methods (`guestAbiMixin`,
`Object.assign`'d onto `MultiTenant.prototype` and `single_tenant`) instead of being free
module-level imports: a tenant method (e.g. `get`/`set` invoking a getter/setter trap) can
now reach them via `this.invokeGuestAware(...)` etc., which — after the `this`-rewrite
above — is a reference to the `__this` parameter, not a free import the splice site
couldn't otherwise resolve.

**Future consideration** (not implemented, noted for later): baking these five directly
onto every `Tenant` implementation is the simplest thing that satisfies the inlining
invariant today; a later refactor could instead thread them as a separate injected
parameter/object alongside `tenant` rather than attaching them to the tenant itself.

**IIFE inlining**: the `this`-rewrite produces splices shaped like `(function(__this,
...params){ ... })(tenantRef, ...args)` — a genuine IIFE. `SCfg::inline_iifes`
(`crates/swc-ssa/src/simplify.rs` in the separate, vendored `jsaw-core` dependency)
eliminates these: it walks the callee as a straight-line chain of blocks (no branching,
loops, nested closures, or non-arrow `this`/`arguments`), splicing its body directly into
the caller and aliasing the call's own result to whatever the callee returned (handling
both an ordinary `Return` and the `Tail`-call shape `return f(...)` lowers to). Wired into
`jade-cfg-opt::optimize_tfunc` (after `simplify_justs`), so Tier 2 actually eliminates these
wrappers rather than merely tolerating them — see
`crates/jade-vm-jit-swc`'s `tenant_method_this_rewrite_iife_is_inlined_by_tier2` test for the
end-to-end proof (extracts + rewrites a `this`-referencing tenant method, compiles it through
Tier 2, and asserts the IIFE wrapper is gone from the emitted JS while the call through to
the tenant's own method still executes correctly). Anything outside that narrow shape (real
branching, a loop, an active `catch`, a nested closure, non-arrow `this`/`arguments`) is left
as an ordinary call — inlining is purely an optimization, never required for correctness.

## Addendum: generator tenant methods and the driver splice (`docs/tenant-generator-driver.md`)

After the refactor in `packages/jade-js/driver.ts`, every tenant operation is a
generator and the JIT no longer emits bare `tenant.get(...)` / `tenant.set(...)`
calls. Instead, inlined generator methods are emitted as `(function*(...){...})`
and wrapped with `tenant.driveTenant(...)` at the call site. The inlining
machinery still uses the same `this`-rewrite and `needs_tenant_self` mechanism;
`InlinableTenantMethod` now also carries an `is_generator` flag parsed from the
method's source. The driver protocol is documented in full in
`docs/tenant-generator-driver.md`; add that doc to any agent prompt when touching
tenant operations or the JIT/WASM back-ends.
