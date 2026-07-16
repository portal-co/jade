# Tenant generator driver

## Problem

Every `Tenant` method (`make`, `get`, `set`, `has`, `delete`, `ownKeys`, `define`,
`assign`) is now a **generator**. The reason is the same ambient-upgrade problem
that already existed for Jade function calls under `addAsync`/`addGen`:

- A getter trap can be a guest function whose effective variant is `async` or
  `generator`, so `tenant.get(...)` may yield a `Promise` or a native
  `Generator` rather than a host-shaped value.
- Host-authored tenant code (`define` → `get`, `assign` → `get`/`set`, `narrow`
  → `get`) used to assume every sub-result was already a plain value.

Storing the uncomposed wrapper object in VM state (e.g. the result of a `GET`
opcode) is a bug: it makes bytecode see `Promise`/`Generator` where it expects a
value. The fix is to drive every tenant operation through a single composition
routine that applies the same declared-bits-OR-ambient-bits rule as
`FnResult`/`Config.add_async`/`add_gen`.

## Core protocol

1. **Every tenant operation is a generator.** Implementations yield
   `TenantOp` sentinels for nested operations and may yield `Promise`/native
   iterators for external results.
2. **Nested tenant calls compose with `yield this.yieldTenant(innerGen)`.**
   The sentinel (`{ [Symbol.for("jade.tenantOp")]: generator }`) tells the
   driver to recursively drive the inner generator with the same ambient
   `addAsync`/`addGen` flags.
3. **External yields (`Promise`, native `Iterator`) are composed by the driver**
   into the effective boundary shape — plain value / `Promise` / `Generator` /
   `AsyncGenerator`.
4. **No call site calls `.next()` on a tenant method.** The VM interpreter,
   the JIT, the WASM backend, and embedder host code all go through
   `tenant.driveTenant(gen, addAsync, addGen)`.

## Driver implementation

`packages/jade-js/driver.ts` is the single source of truth for the four variant
loops:

| ambient flags | driver return | handles `Promise` | handles native `Iterator` |
|---------------|---------------|-------------------|---------------------------|
| none          | sync value    | throws (requires `addAsync`) | throws (requires `addGen`) |
| `addAsync`    | `Promise`     | `await`           | throws (requires `addGen`) |
| `addGen`      | `Generator`   | `yield` (passes through to caller) | wraps with `createGuestGen`, then `yield*` |
| both          | `AsyncGenerator` | `await`        | wraps with `createGuestGen`, then `yield*` |

`yieldTenant` and `driveTenant` are injected onto every `Tenant` implementation
through `guestAbiMixin` (`packages/jade-js/narrow.ts`) so that they are reachable
as `this.yieldTenant` / `this.driveTenant` from inside tenant methods. They are
**not** free module imports and they are **not** in `TENANT_METHOD_NAMES` (so
inlining never tries to splice the driver itself).

## Back-end wiring

### TypeScript interpreter (`packages/jade-js/vm.ts`)

`packages/jade-data/index.ts` emits `__DRIVE__tenant.driveTenant(...)` markers
in the `GET`/`SET`/`LITOBJ` handlers. `scripts/gen/vm-ts.ts` maps
`__DRIVE__` per variant:

- `runVirtualized`  → `tenant.driveTenant(...)`         (sync)
- `runVirtualizedA` → `await tenant.driveTenant(...)`   (async)
- `runVirtualizedG`/`AG` → `yield* tenant.driveTenant(...)` (sync/async generator)

Run `scripts/regen.ts` after changing the handlers.

### JIT (`crates/jade-vm-jit`)

`op_get`/`op_set`/`op_litobj`/`define_properties` emit
`tenant.driveTenant(tenant.<method>(...), add_async, add_gen)` and prefix the
expression with `await`/`yield*` when the enclosing emitted function is itself
async/generator. `op_call` wraps generator-shaped call results with
`tenant.createGuestGen(...)` through the same `tenant.driveTenant(...)` path.

`InlinableTenantMethod` gained an `is_generator` flag. A real tenant method is
inlined as a `function*` IIFE, then wrapped with `tenant.driveTenant(...)` at the
splice site. Test fixtures that set `is_generator: false` keep the old
non-generator splice shape for backwards compatibility, but live tenant source
parsed by `crates/jade-vm-frontend/src/tenant_inline.rs` always produces
`is_generator: true` because real tenant operations are generators.

### WASM (`crates/jade-vm-wasm`)

`tenant_drive` calls the method to obtain a generator, then calls
`tenant.driveTenant(generator, add_async, add_gen)` via `Reflect.apply`. The
WASM interpreter otherwise keeps its existing four-variant dispatch; the
`driveTenant` step is inserted for `make`/`get`/`set`/`define`/`assign`.

## Invariants and checklist for future changes

- **Never add `driveTenant`/`yieldTenant` to `TENANT_METHOD_NAMES`.** They must
  remain real `this`-derived method calls so that inlined tenant bodies can
  reach them.
- **Never call `.next()` on a tenant method directly.** Use
  `tenant.driveTenant(...)`.
- **Inside tenant methods, compose nested ops with
  `yield this.yieldTenant(this.<method>(...))`.**
- **Embedder host code outside tenant methods** should also use
  `tenant.driveTenant(..., false, false)` for synchronous inspection.
- **Run `scripts/regen.ts`** after changing `packages/jade-data/index.ts`.
- **Thread `Config.add_async`/`add_gen` at one emission site per tier**, the same
  way `op_await`/`op_yield` already do.

## Relationship to `createGuestGen`/`unpackGuestGen`

Those shims are still in `packages/jade-js/shims.ts` and still live on the
tenant via `guestAbiMixin`. `createGuestGen` is itself a generator that yields
`TenantOp` sentinels while building the guest-side generator object. The driver
automatically consumes those sentinels; `unpackGuestGen` adapts a guest-gen
object back to the native generator protocol for `YIELDSTAR` in doubleGen
contexts.