// End-to-end check of getter/setter ("trap") invocation through the multi-tenant object
// manager. Run with:  node --experimental-strip-types packages/jade-js/trap.e2e.ts
//
// Exercises the fixes in `multi_tenant.ts`: `get()`/`set()` route accessor traps through
// `invokeTrap` (ABI-aware, not a bare `.call()`), `set()` now actually invokes an existing
// setter trap instead of always overwriting with a data descriptor, and `define()` builds
// descriptors via `narrow()` instead of unchecked `as` casts.
//
// After the generator-driver refactor, every tenant method returns a generator; host code
// must compose through `tenant.driveTenant(..., false, false)` unless it is going through
// the VM, which drives the tenant op with the ambient variant flags.

import { MultiTenant, markGuestFn, guestFnMeta } from "./index.ts";

function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error("FAIL: " + msg);
}

// Helper for synchronous host-side tenant composition.
function driveSync<T>(t: MultiTenant, gen: Generator<any, T, any>): T {
  return t.driveTenant(gen, false, false) as T;
}

// 1) A plain host getter trap is invoked (through `invokeTrap`), not left inert.
{
  const t = new MultiTenant();
  const target = t.make(null);
  const descriptors = t.make(null);
  const desc = t.make(null);
  let calls = 0;
  driveSync(t, t.set(desc, "get", () => {
    calls++;
    return 99;
  }));
  // A descriptor read by `define()` must be enumerable to become an own key of the
  // target, so mark the required descriptor metadata fields explicitly.
  driveSync(t, t.set(desc, "enumerable", true));
  driveSync(t, t.set(desc, "configurable", true));
  driveSync(t, t.set(descriptors, "k", desc));
  driveSync(t, t.define(target, descriptors));

  const v = driveSync(t, t.get(target, "k"));
  assert(v === 99, `expected getter trap to return 99, got ${v}`);
  assert(calls === 1, `expected getter to run exactly once, got ${calls}`);
}

// 2) A setter trap is invoked on `set()` instead of being silently overwritten with a data
//    descriptor (the original bug: `set()` never checked for/called an existing setter).
{
  const t = new MultiTenant();
  const target = t.make(null);
  const descriptors = t.make(null);
  const desc = t.make(null);
  let seen: unknown;
  driveSync(t, t.set(desc, "set", (v: unknown) => {
    seen = v;
  }));
  driveSync(t, t.set(desc, "enumerable", true));
  driveSync(t, t.set(desc, "configurable", true));
  driveSync(t, t.set(descriptors, "k", desc));
  driveSync(t, t.define(target, descriptors));

  driveSync(t, t.set(target, "k", 123));
  assert(seen === 123, `expected setter trap to receive 123, got ${seen}`);
  // A set-only accessor has no getter: reading it must come back `undefined`, not throw.
  assert(driveSync(t, t.get(target, "k")) === undefined, "set-only accessor get() should be undefined");
}

// 3) A getter trap that is itself a *guest* function (JIT-style "leading-tenant-nt" ABI)
//    is invoked with its real ABI — tenant threaded as the first argument — rather than
//    being called as if it were a plain host function via `.call(obj)`.
{
  const t = new MultiTenant();
  const target = t.make(null);
  const descriptors = t.make(null);
  const desc = t.make(null);
  let seenTenant: unknown;
  let seenExtraArgs = -1;

  const guestGetter = function (tenantArg: unknown, _nt: unknown, ...rest: unknown[]) {
    seenTenant = tenantArg;
    seenExtraArgs = rest.length;
    return "guest-value";
  };
  markGuestFn(guestGetter, { abi: "leading-tenant-nt" });

  driveSync(t, t.set(desc, "get", guestGetter));
  driveSync(t, t.set(desc, "enumerable", true));
  driveSync(t, t.set(desc, "configurable", true));
  driveSync(t, t.set(descriptors, "k", desc));
  driveSync(t, t.define(target, descriptors));

  const v = driveSync(t, t.get(target, "k"));
  assert(v === "guest-value", `expected guest getter trap result, got ${v}`);
  assert(seenTenant === t, "guest getter trap should receive the tenant as its leading param");
  assert(seenExtraArgs === 0, `guest getter trap should receive no extra args, got ${seenExtraArgs}`);
  assert(
    guestFnMeta(guestGetter)?.abi === "leading-tenant-nt",
    "guest getter's registered ABI must not be clobbered by narrowing/invocation",
  );
}

// 4) `assign`/`define`/`ownKeys` round-trip non-numeric string keys correctly (regression
//    check for the `$`-prefix double-encoding bug found while fixing the traps above).
{
  const t = new MultiTenant();
  const src = t.make(null);
  const dst = t.make(null);
  driveSync(t, t.set(src, "alpha", 1));
  driveSync(t, t.set(src, "beta", 2));
  const keys = driveSync(t, t.ownKeys(src));
  assert(
    new Set(keys).size === 2 && keys.every((k) => k === "alpha" || k === "beta"),
    `expected ownKeys to report original string keys, got ${JSON.stringify(keys)}`,
  );
  driveSync(t, t.assign(dst, src));
  assert(
    driveSync(t, t.get(dst, "alpha")) === 1 && driveSync(t, t.get(dst, "beta")) === 2,
    "assign() should copy string-keyed properties",
  );
}

console.log("PASS: multi_tenant trap e2e (getter/setter invocation, ownKeys string keys)");