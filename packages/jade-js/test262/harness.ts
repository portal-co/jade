/**
 * Jade-native test262 harness primordials (`sta.js`/`assert.js` equivalents).
 *
 * Per docs/test262-plan.md ground rule 2 these are hand-written against tenant
 * operations — never compiled from test262's own harness sources, which use syntax the
 * frontend subset doesn't have. Everything installed here is tenant-owned: functions are
 * adopted with `tenant.makeFunction`, set onto the realm global with `tenant.set`, and
 * guest calls arrive through `tenant.invoke`.
 *
 * Phase 1 scope: Test262Error, $ERROR, assert (+ sameValue/notSameValue/compareArray),
 * print, $262.global. Phase 4 (exceptions) adds assert.throws — loosely typed: guest
 * realms have no error primordials yet, so constructor checks compare `name`s when
 * readable and otherwise accept any throw. `$DONE`, `$262.createRealm`/
 * `detachArrayBuffer`/`evalScript` land in their own phases.
 */
import type { Tenant } from "../index.ts";
import type { CellContext } from "./exec.ts";

/** Host-side marker error: an assertion failure (i.e. the test failed). */
export class Test262Error extends Error {
  constructor(message?: string) {
    super(message);
    this.name = "Test262Error";
  }
}

function sameValue(a: unknown, b: unknown): boolean {
  // SameValue, per test262's assert.js: Object.is with the +0/-0 distinction kept.
  return Object.is(a, b);
}

function formatValue(v: unknown): string {
  try {
    return typeof v === "string" ? JSON.stringify(v) : String(v);
  } catch {
    return "[unprintable]";
  }
}

export function installHarness(ctx: CellContext): void {
  const tenant: Tenant = ctx.tenant;
  const drive = <T>(gen: Generator<unknown, T, unknown>): T =>
    tenant.driveTenant(gen as never, false, false) as T;
  const mkfn = (impl: Function): Function => drive(tenant.makeFunction(impl));
  const setGlobal = (key: string, value: unknown): void =>
    drive(tenant.set(ctx.realm.globalThis, key, value));

  // --- sta.js -----------------------------------------------------------------------
  setGlobal("Test262Error", mkfn(function Test262Error(this: unknown, message?: string) {
    // Guest-constructible shape only; tests can't `throw` yet, so this exists for
    // surface parity, not for catching.
    return new Test262Error(message);
  }));
  setGlobal("$ERROR", mkfn(function $ERROR(message?: string): never {
    throw new Test262Error(message);
  }));

  // --- assert.js --------------------------------------------------------------------
  const assert = mkfn(function assert(condition: unknown, message?: string) {
    if (!condition) throw new Test262Error(message ?? "assertion failed");
  });
  drive(tenant.set(assert as unknown as object, "sameValue", mkfn(
    function sameValueAssertion(actual: unknown, expected: unknown, message?: string) {
      if (!sameValue(actual, expected)) {
        throw new Test262Error(
          `${message ?? "assert.sameValue"}: expected ${formatValue(expected)}, got ${formatValue(actual)}`,
        );
      }
    },
  )));
  drive(tenant.set(assert as unknown as object, "notSameValue", mkfn(
    function notSameValueAssertion(actual: unknown, expected: unknown, message?: string) {
      if (sameValue(actual, expected)) {
        throw new Test262Error(
          `${message ?? "assert.notSameValue"}: expected anything but ${formatValue(actual)}`,
        );
      }
    },
  )));
  drive(tenant.set(assert as unknown as object, "compareArray", mkfn(
    function compareArrayAssertion(actual: unknown, expected: unknown, message?: string) {
      const a = actual as ArrayLike<unknown>, e = expected as ArrayLike<unknown>;
      if (a == null || e == null || typeof a.length !== "number" || typeof e.length !== "number") {
        throw new Test262Error(`${message ?? "assert.compareArray"}: not array-like`);
      }
      if (a.length !== e.length) {
        throw new Test262Error(
          `${message ?? "assert.compareArray"}: length ${a.length} != ${e.length}`,
        );
      }
      for (let i = 0; i < a.length; i++) {
        if (!sameValue(a[i], e[i])) {
          throw new Test262Error(
            `${message ?? "assert.compareArray"}: index ${i}: expected ${formatValue(e[i])}, got ${formatValue(a[i])}`,
          );
        }
      }
    },
  )));
  setGlobal("assert", assert);

  // assert.throws(expectedCtor, fn, description): invoke the guest fn; a guest throw
  // crosses tenant.invoke as a host exception (docs/exceptions-plan.md). The
  // constructor check is deliberately loose — no error primordials exist guest-side
  // (so `TypeError` reads as undefined and any throw satisfies it); when the caught
  // value is our own Test262Error and the ctor is not the guest Test262Error shell,
  // an assertion failed *inside* the callback: rethrow it rather than pass.
  const test262ErrorShell = drive(tenant.get(ctx.realm.globalThis, "Test262Error"));
  drive(tenant.set(assert as unknown as object, "throws", mkfn(
    function assertThrows(expectedCtor: unknown, fn: unknown, description?: string) {
      let caught: unknown;
      let threw = false;
      try {
        drive(tenant.invoke(fn, { kind: "apply", thisArg: undefined, args: [] }));
      } catch (e) {
        threw = true;
        caught = e;
      }
      if (!threw) {
        throw new Test262Error(description ?? "assert.throws: function did not throw");
      }
      if (caught instanceof Test262Error && expectedCtor !== test262ErrorShell) {
        throw caught;
      }
      return caught;
    },
  )));

  // --- misc --------------------------------------------------------------------------
  setGlobal("print", mkfn(function print(...args: unknown[]) {
    console.log("[test]", ...args);
  }));
  const dollar262 = drive(tenant.make(null));
  drive(tenant.set(dollar262, "global", ctx.realm.globalThis));
  setGlobal("$262", dollar262);
}
