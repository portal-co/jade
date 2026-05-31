import type { Tenant } from "./index.ts";

/** Tag on a yielded value that marks it as a pass-through from an inner
 * declared-generator running under addGen. Consumers check for this to
 * distinguish inner-gen yields from yields produced by upgraded-sync functions.
 * Uses the global Symbol registry so WASM (via Symbol.for / js_sys::Symbol::for_)
 * can reference the same symbol without sharing the TypeScript binding directly. */
export const THROUGH: symbol = Symbol.for("jade.through");

/** Internal symbol for VM-side direct access to a guest-gen's `next` generator
 * function without going through the tenant shadow. Also in the global registry
 * so WASM can detect guest-gen objects if needed. */
const _GUEST_NEXT: symbol = Symbol.for("jade.guest.next");

/**
 * Wrap a native generator (produced by calling a function under the gen
 * effective variant) in a **guest-side generator object**: a tenant-created
 * object whose `next`/`return`/`throw` methods are generator functions.
 *
 * Because `next` is a `function*`, callers can `yield*` through its result,
 * which lets any THROUGH-tagged pass-through yields bubble up transparently
 * while the non-tagged (native) yield becomes the expression value.
 */
export function createGuestGen(nativeGen: Generator, tenant: Tenant): any {
  const obj = tenant.make(null);

  const nextFn = function* (sent: any): Generator<any, { value: any; done: boolean }, any> {
    let step = (nativeGen as any).next(sent);
    // Re-yield any THROUGH-tagged pass-through values so they propagate to the
    // outer generator context. Stop at the first non-THROUGH step (the native
    // yield or the final return).
    while (!step.done && (step.value as any)?.[THROUGH] !== undefined) {
      sent = yield step.value;
      step = (nativeGen as any).next(sent);
    }
    return step; // {value, done} — native yield or termination
  };

  const returnFn = function* (val: any): Generator<never, { value: any; done: boolean }, any> {
    const step = typeof (nativeGen as any).return === "function"
      ? (nativeGen as any).return(val)
      : { value: val, done: true };
    return step;
  };

  const throwFn = function* (err: any): Generator<never, { value: any; done: boolean }, any> {
    if (typeof (nativeGen as any).throw === "function") {
      return (nativeGen as any).throw(err);
    }
    throw err;
  };

  tenant.set(obj, "next", nextFn);
  tenant.set(obj, "return", returnFn);
  tenant.set(obj, "throw", throwFn);
  // Store a direct reference for fast VM-internal access; this bypasses the
  // tenant shadow deliberately (the Symbol is not guest-visible).
  (obj as any)[_GUEST_NEXT] = nextFn;

  return obj;
}

/**
 * Generator function that adapts a **guest-side generator** (created by
 * `createGuestGen`) back to the native JS generator protocol, for use in
 * `YIELDSTAR` in a doubleGen context.
 *
 * Iterates the guest gen by `yield*`-ing each call to its `next` generator
 * method. THROUGH-tagged values yielded inside `next` propagate up to the
 * outer generator naturally. Non-tagged (native) values are surfaced to the
 * caller via `yield`.
 */
export function* unpackGuestGen(g: any): Generator {
  const nextFn: Function = (g as any)[_GUEST_NEXT];
  if (typeof nextFn !== "function") {
    // Not a guest-gen — fall back to standard iteration.
    return yield* g;
  }
  let sent: any;
  while (true) {
    // nextFn(sent) returns a Generator; yield* it to propagate THROUGH yields
    // and evaluate to the {value, done} step result.
    const result: { value: any; done: boolean } = yield* (nextFn as any).call(g, sent);
    if (result.done) return result.value;
    sent = yield result.value;
  }
}
