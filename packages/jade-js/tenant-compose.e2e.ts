// End-to-end coverage of the generator-driver composition path: tenant ops are all
// generators, and every backend (including the TS interpreter here) must call through
// `tenant.driveTenant(gen, addAsync, addGen)` rather than assuming raw trap results are
// already host-shaped values.
//
// Run with: node --experimental-strip-types packages/jade-js/tenant-compose.e2e.ts

import { MultiTenant, single_tenant, markGuestFn, vm } from "./index.ts";

function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error("FAIL: " + msg);
}

const LIT = (v: number) => (v << 1) >>> 0;
const REF = (i: number) => ((i << 1) | 1) >>> 0;

class Buf {
  bytes: number[] = [];
  u16(n: number) {
    this.bytes.push(n & 0xff, (n >>> 8) & 0xff);
  }
  u32(n: number) {
    this.bytes.push(n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff);
  }
  lit32(dest: number, val: number) {
    this.u16(6);
    this.u32(dest);
    this.u32(val);
  }
  get(obj: number, key: number, dest: number) {
    this.u16(23);
    this.u32(obj);
    this.u32(key);
    this.u32(dest);
  }
  set(obj: number, key: number, val: number, dest: number) {
    this.u16(24);
    this.u32(obj);
    this.u32(key);
    this.u32(val);
    this.u32(dest);
  }
  ret(valOperand: number) {
    this.u16(0);
    this.u32(valOperand);
  }
  view(): () => DataView {
    const dv = new DataView(new Uint8Array(this.bytes).buffer);
    return () => dv;
  }
}

function driveSync<T>(t: MultiTenant, gen: Generator<any, T, any>): T {
  return t.driveTenant(gen, false, false) as T;
}

// Build the scaffolding for an accessor-backed target: a target object and a
// descriptors map containing key "k" -> empty descriptor. Callers fill in get/set
// on the descriptor and then call define themselves, so mutations are captured
// in the tenant shadow.
function buildAccessorScaffold(t: MultiTenant): { target: object; descriptors: object; desc: object; stateBase: { 0: object; 1: string } } {
  const target = driveSync(t, t.make(null));
  const descriptors = driveSync(t, t.make(null));
  const desc = driveSync(t, t.make(null));
  driveSync(t, t.set(desc, "enumerable", true));
  driveSync(t, t.set(desc, "configurable", true));
  driveSync(t, t.set(descriptors, "k", desc));
  return { target: target as object, descriptors, desc, stateBase: { 0: target as object, 1: "k" } };
}

// 1) Async getter trap under runVirtualizedA + addAsync resolves to a plain value.
{
  const t = new MultiTenant();
  const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
  driveSync(t, t.set(desc, "get", () => Promise.resolve(77)));
  driveSync(t, t.define(target, descriptors));

  const b = new Buf();
  b.get(REF(0), REF(1), 2);
  b.ret(REF(2));

  const out = vm.runVirtualizedA(
    b.view(),
    stateBase,
    { tenant: t, addAsync: true, addGen: false },
  );
  assert(out instanceof Promise, "async GET must return a Promise when addAsync is true");
  assert((await out) === 77, "expected async getter to resolve to 77");
}

// 2) Async setter trap under runVirtualizedA completes and side effect is visible.
{
  const t = new MultiTenant();
  const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
  let seen: unknown;
  driveSync(
    t,
    t.set(desc, "set", (v: unknown) => {
      seen = v;
      return Promise.resolve();
    }),
  );
  driveSync(t, t.define(target, descriptors));

  const b = new Buf();
  b.set(REF(0), REF(1), LIT(42), 2);
  b.ret(REF(2));

  const out = vm.runVirtualizedA(
    b.view(),
    stateBase,
    { tenant: t, addAsync: true, addGen: false },
  );
  assert(out instanceof Promise, "async SET must return a Promise");
  const resolved = await out;
  assert(resolved === 42, `expected async setter to return assigned value 42, got ${resolved}`);
  assert(seen === 42, `expected async setter trap to receive 42, got ${seen}`);
}

// 3) Sync getter returning a native generator under runVirtualizedG + addGen yields a
// guest-gen object that can be unwrapped with unpackGuestGen.
{
  const t = new MultiTenant();
  const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
  driveSync(
    t,
    t.set(desc, "get", function* () {
      yield 10;
      yield 20;
      return 30;
    }),
  );
  driveSync(t, t.define(target, descriptors));

  const b = new Buf();
  b.get(REF(0), REF(1), 2);
  b.ret(REF(2));

  const topGen = vm.runVirtualizedG(
    b.view(),
    stateBase,
    { tenant: t, addAsync: false, addGen: true },
  ) as Generator;
  assert(typeof topGen.next === "function", "runVirtualizedG must return a generator when addGen is true");

  // Drive the top-level VM generator to completion; its return value is the guest-gen
  // object produced by `createGuestGen` wrapping the native generator from the trap.
  let step = topGen.next();
  while (!step.done) step = topGen.next(step.value);
  const gen = step.value;
  assert(
    typeof (gen as any)?.[Symbol.for("jade.guest.next")] === "function",
    "gen GET must return a guest-gen object when addGen is true",
  );

  const yielded: unknown[] = [];
  for (const v of t.unpackGuestGen(gen)) yielded.push(v);
  assert(
    JSON.stringify(yielded) === "[10,20]",
    `expected [10,20], got ${JSON.stringify(yielded)}`,
  );
}

// 4) LITOBJ define path with an async accessor descriptor under addAsync.
{
  const t = new MultiTenant();
  const target = driveSync(t, t.make(null));
  const getter = () => Promise.resolve(123);

  const b = new Buf();
  // First LITOBJ: build innerDesc = { get: getter, enumerable: true, configurable: true }
  // and store it in state[7] (key operand low bit clear -> assignment, not define).
  b.u16(9);
  b.u32(3); // 3 pairs, no spread
  b.u32(REF(1));
  b.u32(REF(2));
  b.u32(REF(3));
  b.u32(REF(4));
  b.u32(REF(5));
  b.u32(REF(6));
  b.u32(LIT(7)); // state[7] = innerDesc

  // Second LITOBJ: build descriptors = { k: innerDesc } and define target with it.
  b.u16(9);
  b.u32(1); // 1 pair, no spread
  b.u32(REF(8)); // key "k"
  b.u32(REF(7)); // value innerDesc
  b.u32(REF(0) | 1); // define target (state[0])
  b.ret(LIT(0));

  const out = await vm.runVirtualizedA(
    b.view(),
    { 0: target, 1: "get", 2: getter, 3: "enumerable", 4: true, 5: "configurable", 6: true, 7: undefined, 8: "k" },
    { tenant: t, addAsync: true, addGen: false },
  );
  void out; // LITOBJ define path mutates target; return value depends on caller encoding.
  assert(
    (await driveSync(t, t.get(target, "k"))) === 123,
    "async accessor descriptor from LITOBJ define should resolve to 123",
  );
}

// 5) Guest leading-tenant-nt async getter trap under runVirtualizedA.
{
  const t = new MultiTenant();
  const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
  const guestGetter = function (_tenant: unknown, _nt: unknown) {
    return Promise.resolve("guest-async");
  };
  markGuestFn(guestGetter, { abi: "leading-tenant-nt" });
  driveSync(t, t.set(desc, "get", guestGetter));
  driveSync(t, t.define(target, descriptors));

  const b = new Buf();
  b.get(REF(0), REF(1), 2);
  b.ret(REF(2));

  const out = await vm.runVirtualizedA(
    b.view(),
    stateBase,
    { tenant: t, addAsync: true, addGen: false },
  );
  assert(out === "guest-async", `expected guest async getter to return 'guest-async', got ${out}`);
}

// 6) single_tenant works through the same driver.
{
  assert(typeof single_tenant.driveTenant === "function", "single_tenant should expose driveTenant");
  const obj = single_tenant.driveTenant(single_tenant.make(null), false, false) as object;
  single_tenant.driveTenant(single_tenant.set(obj, "x", 7), false, false);
  assert(
    single_tenant.driveTenant(single_tenant.get(obj, "x"), false, false) === 7,
    "single_tenant GET/SET through driver",
  );
}

// 7) Mixin present on both tenants.
{
  for (const t of [new MultiTenant(), single_tenant]) {
    assert(typeof t.driveTenant === "function", "driveTenant mixin should be present");
    assert(typeof t.yieldTenant === "function", "yieldTenant mixin should be present");
  }
}

console.log("PASS: tenant generator driver composition e2e");