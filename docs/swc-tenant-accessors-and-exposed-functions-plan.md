# Plan: SWC tenant accessors and exposed helper-function internals

## Problem and intended result

Two separate lexical boundaries currently make otherwise-simple code difficult to
inline or make easier to inline at **collection time**, when the embedder
obtains the relevant `Function.prototype.toString()`/class source and prepares
the bundle for the runtime:

1. A tenant class may keep its representation in JavaScript `#private` fields.
   That is a good representation choice, but
   [`tenant_inline::extract_tenant_methods`](../crates/jade-vm-frontend/src/tenant_inline.rs)
   must reject a public tenant method containing a `PrivateName`: after the
   method body is spliced into generated code, the private name is no longer in
   the declaring class's lexical scope.
2. A helper function may close over a small number of values.  Its ordinary JS
   closure is correct, but its implementation cannot be inspected and invoked
   independently by the WASM backend's direct function path.  The collection
   pipeline already obtains source via `toString()` so tenant methods can be
   extracted; it needs an explicit representation of the helper's body and
   captures before that source reaches the WASM inliner.  The current backend
   records private `WeakMap<Function, { v, j, s }>` metadata in
   [`jade-vm-wasm`](../crates/jade-vm-wasm/src/lib.rs); that helps only for
   functions it created itself and says nothing about a helper supplied from
   JavaScript.

Add an **opt-in SWC source transform** that makes these two cases explicit,
without weakening the lexical/privacy rules that make them safe:

- For selected tenant classes, retain each selected `#private` field and add a
  **public but heavily mangled JavaScript accessor property** (a computed
  `get` and, when writes are selected, `set` class member) which forwards to
  it.  Rewrite eligible public tenant methods to use that mangled public
  property rather than spelling the private name.  This keeps the existing
  private representation available to other class consumers that the pass
  cannot support, while transformed methods use ordinary member reads/writes
  and therefore avoid private lexical syntax without custom field-to-method
  expression lowering.  The transformed public method is then suitable for
  the existing `this` rewrite and tenant-method extractor.
- For selected helper functions, expose a non-closing implementation plus an
  explicit environment record on a non-enumerable property of the callable.
  The original callable is replaced by a semantics-preserving shim which reads
  that property and invokes the exposed implementation with its environment.
  During source collection, the bundle pre-pass rewrites the obtained source
  to use the exposed implementation; the WASM collector/inliner consumes that
  rewritten source.  There is no runtime metadata probe or runtime fast path.

This is an optimization and tooling facility.  A class/function which does not
meet the proof obligations is left untouched; normal tenant calls and normal JS
function invocation continue to be correct fallbacks.

## Terminology and invariant

Use the following names consistently in the implementation and documentation:

- **tenant accessor property**: a generated public-but-heavily-mangled
  computed JavaScript `get`/`set` property which forwards to a selected
  private field in an opted-in tenant class.  It is intentionally obscure and
  collision-checked, but it is ordinary public accessor syntax—not a private
  name—and lets unsupported consumers continue to use the original `#field`.
  Transformed public methods access it as a normal member expression, so they
  retain JavaScript's native read/write/update semantics.
- **exposed function record**: the non-enumerable value stored under a
  collision-resistant property key on a transformed function.  It has a
  versioned, documented shape.
- **implementation**: the exposed function body after every captured binding
  has been replaced by an explicit environment lookup.  It must not contain a
  reference to a non-global binding from the definition site.
- **shim**: the guest-visible callable that preserves ordinary call and
  construct behavior.  It delegates to the exposed record.  Collection-time
  source rewriting can replace a call/inlining candidate with its exposed
  implementation; no runtime backend dispatch is introduced.

The key soundness rule is:

> A transformed public tenant method contains no `PrivateName`; an exposed
> implementation contains no free lexical binding.  If either proof fails, do
> not transform that unit.

The pass must use SWC binding identity (`Id` / syntax context after resolver),
not spelling alone, for every free-variable decision.  A local `Object`,
`tenant`, or `x` is not a global merely because it has a familiar spelling.

## Scope and explicit non-goals

The first implementation is deliberately selective.

### In scope

- Class declarations and class expressions positively selected as tenant
  classes by configuration.
- Private **fields** accessed as `this.#field` from directly selected public
  methods.
- Reads, direct writes, and compound/update writes once their evaluation order
  is covered by tests.
- Ordinary helper function declarations and named/assigned function expressions
  positively selected by configuration.
- Sync, `async`, generator, and async-generator helpers, provided each variant
  has a tested shim and implementation lowering.
- Collection-time use of the version-1 exposed record by the WASM function
  source collector/inliner for a trusted local helper.

### Out of scope for version 1

- A generic `Proxy` transform or exposing arbitrary user functions merely
  because they happen to be callable.
- Private methods, private accessors, `obj.#field` where `obj` is not exactly
  the current `this`, `super.#x`, decorators, and private names used in nested
  functions/classes.  These are skipped rather than partially rewritten.
- Arrow functions, classes used as constructors, methods/accessors, functions
  using `arguments`, direct `eval`, `with`, or `new.target` until each has a
  dedicated semantics-preserving lowering.
- Captures with live mutable-binding semantics unless the pass also owns a
  proven cell conversion for that binding.  The initial capture policy accepts
  only immutable captures (normally `const` bindings); it must not silently
  turn a JavaScript live binding into a snapshot.
- Cross-origin/WSDOM serialization of exposed records.  An exposed record is a
  local trusted optimization descriptor, not a new remote function protocol.

The scope may expand only by adding a config flag, a proof/transform for the
new construct, and bundle-prepass plus AST-level tests.

## Configuration

The Rust pass owns one forward-compatible configuration struct.  The public
entry point should accept it explicitly rather than hiding policy in class-name
heuristics:

```rust
pub struct TenantExposureConfig {
    /// Class bindings or source tags selected as tenant implementations.
    pub tenant_classes: BTreeSet<Id>,
    /// Public method names whose bodies may receive private-field rewrites.
    /// Default: the tenant operation set known to the JIT, not every method.
    pub tenant_methods: BTreeSet<Atom>,
    /// Helper bindings explicitly allowed to expose an implementation.
    pub helper_functions: BTreeSet<Id>,
    /// Stable namespace used to derive generated property names and metadata.
    pub mangling_namespace: Atom,
    /// Feature gates; all default to conservative `false`.
    pub allow_compound_private_writes: bool,
    pub allow_async_helpers: bool,
    pub allow_generator_helpers: bool,
    pub allow_immutable_let_captures: bool,
    /// Metadata property. Default is Symbol.for("jade.exposedFunction.v1").
    pub exposed_function_key: ExposedFunctionKey,
}
```

The exact Rust types can evolve, but the policy separation must remain:
**tenants** get public-but-heavily-mangled accessor properties over retained
private fields, **helpers** get exposed internals.  The pass must not rewrite
every class or function in an input program.  A host can build
`tenant_classes`/`helper_functions` from explicit source annotations, an
embedding API, or resolved binding IDs; string-name matching alone is not a
safe long-term selector.

Generated accessor property names are deterministic from the class binding,
private field `Id`, and namespace.  They must be valid but extremely unlikely
public names, for example `__jade$tenant$<hash>$shadow`.  Before emitting,
check every class member name (including computed literal names) and
regenerate/reject on collision.  Do not use an SWC private identifier: the
transformed method must contain only ordinary public member syntax after source
collection.

## Tenant private-field transformation

### Eligibility analysis

For each selected class:

1. Collect declared private fields used by selected public tenant methods.
   The original private field declaration is retained exactly so private
   methods, unsupported public methods, and any other lexical consumers keep
   their current semantics.
2. For every selected field, append a computed public getter with the mangled
   key and, if the selected public methods write it, a matching setter:
   `get [M]() { return this.#field; }` and
   `set [M](value) { this.#field = value; }`.  This uses JavaScript's native
   accessor machinery; it is not a bespoke field-to-method expression
   lowering.
3. Inspect only selected public tenant methods.  Rewrite direct `this.#field`
   reads and writes to `this[M]`.  Do not descend through a nested non-arrow
   function, nested class, or nested method; their `this` and private-name
   scope need independent analysis.
4. If any selected use is unsupported, leave that whole public method
   unrewritten.  It may retain its private syntax and therefore correctly stay
   non-inlinable; the private field and generated accessor remain valid for
   every other consumer.
5. A field used only by private helpers creates no mangled accessor property.

This intentionally adds an opted-in, public-but-heavily-mangled view onto a
private representation.  The private field remains authoritative and retains
its lexical privacy for non-transformed consumers.  The generated public name
is collision-resistant and not part of the tenant ABI, but it is still public
surface on the selected trusted class.

### Generated source shape

For a field `#shadow`, the bundle pre-pass retains the private field and adds
a public mangled accessor view, then changes only selected direct uses:

```js
// input
class T {
  #shadow = new Map();
  *get(obj, key) { return this.#shadow.get(obj)?.[key]; }
}

// bundle-prepass output (illustrative; actual key is collision-checked)
class T {
  #shadow = new Map();
  get ["__jade$tenant$A1$shadow"]() { return this.#shadow; }
  set ["__jade$tenant$A1$shadow"](value) { this.#shadow = value; }
  *get(obj, key) {
    return this["__jade$tenant$A1$shadow"].get(obj)?.[key];
  }
}
```

The transformed `get` contains no `PrivateName`; the generated accessor bodies
are the only new syntax that contains `#shadow`, and they remain lexically in
the class.  `tenant_inline` can rewrite `get`'s `this` to `__this` and emit the
ordinary IIFE splice; the mangled property access remains a plain member
expression on `__this`.  Existing or unsupported class consumers can continue
to read/write `this.#shadow` directly.

For writes, native accessor semantics preserve JavaScript's result and
ordering rules without generated temporaries:

```js
this.#x = rhs      // this[MANGLED_X] = rhs
this.#x += rhs     // this[MANGLED_X] += rhs
++this.#x          // ++this[MANGLED_X]
this.#x++          // this[MANGLED_X]++
```

The pass must generate a setter before rewriting a selected write.  It must
still avoid non-`this` receivers (out of scope), and must retain the private
field declaration unchanged.  Logical assignment and destructuring can be
included when their SWC AST rewrite is covered by tests; the built-in getter/
setter pair, rather than custom temporary-producing lowering, handles the
ordinary member-operation semantics.

### Interaction with existing tenant inlining

The bundle pre-pass runs **before** transformed tenant class source is retained
or handed to `extract_tenant_methods`.  The extractor remains defensive: it
continues to reject any residual `PrivateName`, captured bare callee, or
unsupported parameter list.  The representation expansion is an opportunity
to make a method inlinable, never permission to relax the extractor's checks.

`TENANT_METHOD_NAMES` stays the semantic operation list.  Mangled accessor
properties, ABI helpers (`markGuestFn`, `invokeGuestAware`, `invokeTrap`,
`createGuestGen`, `unpackGuestGen`, `yieldTenant`, `driveTenant`), and callable
helpers must not be added to it.  The inlined operation performs ordinary
member access to its own mangled accessor property; the accessor body remains
an ordinary class member and is never recursively spliced.

## Exposed helper-function transformation

### Record ABI

Use a non-enumerable, non-writable, non-configurable property keyed by
`Symbol.for("jade.exposedFunction.v1")` by default.  Symbol avoids ordinary
own-key collisions; it is local process metadata and must not be passed over
WSDOM unless future protocol work explicitly supports opaque symbols.

The version-1 record is conceptually:

```ts
interface ExposedFunctionV1 {
  readonly version: 1;
  readonly kind: "sync" | "async" | "generator" | "async-generator";
  readonly impl: Function;
  readonly captures: readonly unknown[];
  readonly captureNames: readonly string[]; // diagnostic only, never authoritative
}
```

`impl` takes the environment first, followed by the original call parameters:

```js
function __jade_impl(env, a, b) {
  return env[0].apply(a, [b]);
}
```

It has no lexical dependency on the defining scope.  Global references remain
global references and are checked by resolver identity, not text.  The array
rather than an object gives the pass a stable binding-to-slot map and avoids
user-controlled capture property names.

The callable owns the record:

```js
function helper(a, b) {
  const r = helper[Symbol.for("jade.exposedFunction.v1")];
  return Reflect.apply(r.impl, this, [r.captures, a, b]);
}
Object.defineProperty(helper, Symbol.for("jade.exposedFunction.v1"), {
  value: {
    version: 1,
    kind: "sync",
    impl: __jade_impl,
    captures: [capturedApply],
    captureNames: ["apply"],
  },
  enumerable: false,
  writable: false,
  configurable: false,
});
```

The self-name used by the shim is a generated, hygienic name for anonymous or
assigned functions.  It is not an external captured variable.  The shim is
therefore the normal JS function boundary; the record explicitly supplies both
components which used to be implicit in a closure: implementation and
captures.

For a constructible normal function, `new helper(...)` allocates `this` through
the shim, invokes `impl` with that `this`, and returns the internal result.
Normal JavaScript constructor-result rules therefore still occur at the shim
boundary.  The initial eligibility rules reject `new.target` in the original
body; adding it later requires an explicit hidden implementation parameter and
coverage for subclass/non-default `newTarget` behavior.  Apply never imports an
outer `nt`; this is the same callable ABI rule used by `TenantInvocation`.

Async and generator shims use the corresponding declaration form and compose
the result without eager driving:

```js
async function helper(a) { /* return await-equivalent impl result */ }
function* helper(a) { return yield* Reflect.apply(/* ... */); }
async function* helper(a) { return yield* Reflect.apply(/* ... */); }
```

Exact lowering must preserve `return`, rejection, yield, and delegation
semantics.  Until those tests land, their config flags stay off.

### Capture analysis and correctness

The pass resolves the module/function first, then visits the candidate function
with a lexical scope stack.  A referenced identifier is one of:

- local to the function or a nested scope: leave it untouched;
- a true global/unresolved binding: leave it untouched;
- the function's own recursion binding: initially reject (future work may put
  the shim itself in an explicit capture slot);
- an outer binding: a candidate capture.

For every candidate capture, the pass must prove it has snapshot-safe semantics.
Version 1 accepts `const` bindings, including a mutable object referenced by a
const binding (the reference itself is stable), and optionally a `let` binding
only after whole-binding write analysis proves no write can occur after helper
creation.  It rejects `var`, parameters, imports/live bindings, and any binding
whose identity cannot be proven.  Replacing a true mutable closure cell with
`captures[i]` would be an observable miscompile and is forbidden.

After capture slots are assigned in stable declaration order, rewrite every
captured identifier in `impl` to `env[i]`.  Then perform a final free-variable
walk over the implementation.  Anything besides allowed globals, its own
parameters, and its own locals rejects the transform.  This second check is
required even if the first analysis believes it found all captures.

Direct `eval`, `with`, `arguments`, and `new.target` invalidate the proof and
reject the candidate.  The pass should emit structured reason codes in debug
mode (`MutableCapture`, `NestedPrivateUse`, `Arguments`, `Recursion`, etc.) so
callers can explain why a requested helper was not exposed without making a
rejection fatal.

### Observable function shape

A replacement function can affect `name`, `length`, `.prototype`, property
descriptors, and identity.  Only transform declaration/definition sites before
the value can escape, preserve the original declared parameter list where
possible, and preserve the function's declaration variant.  The initial pass
rejects decorated/spanned functions and functions whose value is observed or
reassigned during initialization.  Existing own properties continue to attach
to the shim, which is the intended guest-visible callable.  The metadata symbol
is deliberately non-enumerable so tenant `ownKeys` and ordinary user code do
not gain a string-keyed property.

## Collection-time exposed-function inliner

The exposed-function inliner is **not** a `WasmPlatform::op_call` runtime
optimization.  It runs during collection, at the same boundary where the
embedder obtains `Function.prototype.toString()`/class source and prepares
sources for tenant extraction or WASM compilation.  The bundle pre-pass uses
the V1 record only to locate an explicitly exposed `impl` and its capture
layout, then rewrites the collected source/inlining candidate before it reaches
the runtime.  Consequently, `jade-vm-wasm` retains its existing direct
Jade-function registry path and tenant-aware `tenant.invoke` fallback; it does
not read user function metadata, invoke getters, or dispatch a new exposed
function ABI while executing guest code.

Collection steps for a selected trusted local helper are:

1. Obtain the callable's own exposed V1 record and validate its descriptor
   **outside guest execution**: non-accessor data property, `version === 1`,
   supported `kind`, callable `impl`, and an array whose length/layout matches
   the collector's capture map.  Remote WSDOM descriptors, bridge exotics, and
   tenant callable exotics are not collection candidates.
2. Obtain `impl.toString()` and parse it in the separate bundle-prepass crate.
   Reject it unless the resolver-aware post-pass proof confirms that it has no
   free lexical bindings beyond its explicit `env` parameter and true globals.
3. Substitute or bind the recorded capture values according to the collection
   representation, then rewrite the *collected* function source to call/inline
   `impl(env, ...args)` rather than the shim.  Preserve the real apply versus
   construct distinction: apply has no `nt`; construction uses the actual
   `newTarget` and must never be rewritten as an apply.
4. Feed only the rewritten source/collected representation to the WASM function
   inliner.  If validation, source extraction, parsing, or rewriting fails,
   collect the original shim normally.  At execution it will follow the normal
   `tenant.invoke(callee, TenantInvocation)` path.

This makes the bundle runtime happier with a simpler, already-normalized
function representation.  It also makes the trust boundary auditable: metadata
is inspected once by the collector, never dynamically supplied by a guest
function call.  A malformed/spoofed record is a collection cache miss, not a
reason to call attacker-controlled `impl`.

A later enhancement may attach optional, trusted Jade bytecode/region metadata
to the collection result and feed it into the existing `FN_REGISTRY`/
`build_child_state` fast path.  That is separate from V1: `impl` must first be
usable as plain collected JavaScript without pretending arbitrary JS source is
Jade bytecode.

## Implementation layout

Create a separate workspace crate, for example
`crates/jade-swc-tenant-exposure`, rather than placing the pass in
`jade-vm-frontend`.  It is a **bundle-level pre-pass**: it parses/resolves a
whole source bundle, applies the selected tenant representation expansion and
helper exposure transforms, and emits normalized source plus collection
metadata before the frontend/JIT/WASM runtime sees it.

The crate depends on SWC parser/AST/visit/codegen facilities and exposes an API
along these lines:

```rust
pub fn transform_bundle(
    program: &mut swc_ecma_ast::Program,
    config: &TenantExposureConfig,
) -> TenantExposureReport;

pub fn collect_exposed_function(
    function_source: &str,
    record: &ExposedFunctionV1,
    config: &TenantExposureConfig,
) -> Result<CollectedExposedFunction, ExposureSkipReason>;
```

The report lists transformed classes/functions and non-fatal skip reasons;
`CollectedExposedFunction` contains source/AST and the explicit capture layout
for the downstream WASM inliner.  The crate owns resolver-aware analysis,
deterministic mangling, and `VisitMut` rewriting.  `jade-vm-frontend` retains
only bytecode lowering and `tenant_inline` retains source extraction and
conservative splice validation after transformation.  Neither runtime crate
should gain a dependency on this bundle preparation machinery.

Suggested delivery order:

1. Create the standalone crate, config/report types, resolver plumbing,
   deterministic mangling, and bundle parse/codegen test helpers.
2. Implement selected private-field-to-public-mangled-accessor conversion and
   direct selected-public-method uses, while retaining the original private
   field.  Verify transformed class source extracts through `tenant_inline`
   where the original source was rejected.
3. Cover direct assignment, compound/update, logical assignment, and
   destructuring one AST form at a time with side-effect tests.
4. Implement sync helper exposure for simple identifier parameters and immutable
   `const` captures.  Add the final no-free-binding assertion.
5. Implement collection-time use of the record and feed its normalized result
   to the WASM inliner; verify no `op_call` runtime metadata probe exists.
6. Add generator/async variants behind their flags.
7. Consider mutable capture cells, recursion, arrows, and transport only as
   separately designed extensions.

## Tests

### SWC/pass unit tests

- A tenant method containing `this.#shadow` is rejected by
  `extract_tenant_methods` before transformation and accepted after selected
  uses are rewritten through a public mangled accessor, while unsupported
  consumers retain the original private field.
- Only selected tenant classes/methods change; unselected classes and private
  fields are byte-for-byte/AST-equivalent.
- A converted field retains its initializer and private declaration; one
  deterministic collision-free public mangled key has a generated getter and,
  when selected writes require it, setter that both forward to that same
  private field.
- Reads, direct assignment, compound assignment, prefix/postfix update, and
  rejected unsupported forms preserve return values and evaluate RHS once.
- Nested functions/private names, non-`this` receivers, and collisions skip the
  affected method rather than emitting invalid source.
- Exposed implementation has no free binding according to a resolver-aware
  post-pass walker; captures are ordered deterministically and rewritten to the
  matching environment slots.
- Mutable/import/parameter/recursive/eval/arguments/new.target helper cases
  are rejected with the expected reason code.

### Runtime integration tests

- Run transformed `MultiTenant`-like classes through normal tenant operations
  and through Tier 0, Tier 1 (`reloop`), and Tier 2 extraction; all produce the
  same values and the transformed public method contains no private syntax.
- Verify mangled accessor-property accesses remain tenant-derived after the
  existing `this -> __this` rewrite, that the accessor body itself stays inside
  its class, and that ABI/driver helpers remain real methods.
- For sync helpers, compare direct apply and direct construction of transformed
  and untransformed fixtures, including `this` and constructor object-return
  behavior.
- Verify async/generator variants only when enabled, including error/rejection,
  `yield*`, and ambient `addAsync`/`addGen` composition.
- At collection time, assert a valid local exposed record produces normalized
  implementation source for the WASM inliner, while a malformed record, a
  callable exotic, a bridge, and a WSDOM remote function collect the original
  shim and execute through `tenant.invoke`.  Assert `WasmPlatform::op_call`
  performs no exposed-record lookup.
- Add a regression that an apply made from inside a constructor still observes
  `nt === undefined`; only construction receives its explicit `newTarget`.

## Acceptance criteria

The feature is ready when selected tenants can retain their private
representation while exposing collision-safe public mangled accessor properties
for eligible public-method extraction/inlining; selected helpers expose a
verifiably non-closing implementation plus explicit immutable environment; and
the **collection-time** WASM inliner consumes only validated, normalized source
while the runtime retains its established tenant-aware fallback.  No
unsupported syntax may be transformed on a best-effort basis, and no path may
weaken the established `tenant`/`nt` or generator-driver ABI.