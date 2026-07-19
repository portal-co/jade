import type { Tenant, TenantInvocation } from "./index.ts";
import { createGuestGen, isGuestGen, unpackGuestGen } from "./shims.ts";
import { yieldTenant, driveTenant } from "./driver.ts";

/**
 * The calling convention a guest (Jade-VM-produced) function actually uses, as observed
 * from outside the VM. The two backends currently differ:
 *  - "closure": the TS interpreter's `FN` opcode closes over `tenant`/`nt`/`code` lexically,
 *    so the produced function is plain-callable as `fn(...args)`.
 *  - "leading-tenant-nt": the JIT's `op_fn` compiles functions as
 *    `function(tenant, nt, ...args){...}`, so a caller must thread `tenant`/`nt` explicitly.
 * This is metadata only — it never changes `typeof fn`, and a guest function is never
 * wrapped to *look* like a host function. It just tells callers how to invoke it correctly.
 */
export type GuestFnMeta = { abi: "closure" } | { abi: "leading-tenant-nt" };

const GUEST_FN_REGISTRY = new WeakMap<Function, GuestFnMeta>();

/** Register `fn`'s guest calling convention. Called by every backend's `FN` opcode.
 *  Returns `fn` itself so call sites can register inline at the point of assignment. */
export function markGuestFn<F extends Function>(fn: F, meta: GuestFnMeta): F {
  GUEST_FN_REGISTRY.set(fn, meta);
  return fn;
}

/** Look up a previously-registered guest calling convention, if any. */
export function guestFnMeta(fn: Function): GuestFnMeta | undefined {
  return GUEST_FN_REGISTRY.get(fn);
}

/**
 * Invoke `fn` respecting its actual ABI: a registered guest function gets its guest-ABI
 * leading parameters threaded; anything else (including an unregistered guest function,
 * which only the "closure"-ABI interpreter ever produces) is called as a plain function.
 * Never uses `.call()`/`.apply()` assuming host semantics without checking first.
 *
 * Takes the tenant via `this` (called as `tenant.invokeGuestAware(...)`, per `guestAbiMixin`)
 * rather than an explicit parameter — see `Tenant`'s doc comment (`index.ts`) for why.
 */
export function* invokeGuestAware<Args extends readonly unknown[], R = unknown>(
  this: Tenant,
  fn: Function,
  thisArg: unknown,
  args: Args,
  invocation: TenantInvocation = { kind: "apply", thisArg, args },
): Generator<any, R, any> {
  const meta = guestFnMeta(fn);
  const raw = invocation.kind === "construct"
    ? meta?.abi === "leading-tenant-nt"
      ? (Reflect.construct(fn, [this, invocation.newTarget, ...args], invocation.newTarget) as R)
      : (Reflect.construct(fn, [...args], invocation.newTarget) as R)
    : meta?.abi === "leading-tenant-nt"
      ? (Reflect.apply(fn, invocation.thisArg, [this, undefined, ...args]) as R)
      : (Reflect.apply(fn, invocation.thisArg, args) as R);
  // Hand asynchronous and native-generator results to the driver.  A guest-gen
  // object is already an ABI value and must not be wrapped a second time.
  if (
    (raw !== null &&
      (typeof raw === "object" || typeof raw === "function") &&
      typeof (raw as any).then === "function") ||
    (raw !== null &&
      typeof raw === "object" &&
      typeof (raw as any).next === "function" &&
      !isGuestGen(raw))
  ) {
    return yield raw as any;
  }
  return raw;
}

/**
 * Invoke a property-descriptor getter/setter ("trap") with the correct receiver and ABI,
 * whether it turns out to be a guest function or a plain host function. Takes the tenant
 * via `this`, same as `invokeGuestAware`.
 */
export function* invokeTrap<Args extends readonly unknown[], R = unknown>(
  this: Tenant,
  fn: Function,
  receiver: object,
  args: Args,
): Generator<any, R, any> {
  return yield this.yieldTenant(this.invokeGuestAware(fn, receiver, args));
}

/**
 * Every ABI/shim helper a tenant method might need — `markGuestFn`/`invokeGuestAware`/
 * `invokeTrap`/`createGuestGen`/`unpackGuestGen` plus the new `yieldTenant`/`driveTenant`
 * driver helpers — bundled for injection onto every `Tenant` implementation
 * (`Object.assign(Tenant.prototype, guestAbiMixin)` for a class, `Object.assign(obj,
 * guestAbiMixin)` for an object literal).  Each function keeps this single shared
 * implementation; nothing is duplicated per tenant.  See `Tenant`'s own doc comment
 * (`index.ts`) for why these live on the tenant instance itself rather than as free
 * module-level imports.
 */
export const guestAbiMixin = {
  markGuestFn,
  invokeGuestAware,
  invokeTrap,
  createGuestGen,
  unpackGuestGen,
  yieldTenant,
  driveTenant,
};

type TypeofTag<T> = T extends string
  ? "string"
  : T extends number
    ? "number"
    : T extends boolean
      ? "boolean"
      : T extends symbol
        ? "symbol"
        : T extends bigint
          ? "bigint"
          : T extends undefined
            ? "undefined"
            : "object";

/**
 * The non-optional core of the "type function": describes how to interpret an
 * `unknown` guest-observed shape as `T`, including where function-typed fields sit and
 * which calling convention applies there.
 */
type BaseSpec<T> = [unknown] extends [T]
  ? { kind: "any" } // T is `unknown`/`any` — pass through unvalidated (e.g. a descriptor's `value`)
  : [T] extends [(...a: any[]) => any]
    ? { kind: "hostFn" } | { kind: "guestFn" }
    : [T] extends [object]
      ? { kind: "object"; fields: { [K in keyof T]-?: NarrowSpec<T[K]> } }
      : { kind: "typeof"; tag: TypeofTag<T> };

/**
 * The recursive "type function" itself: `BaseSpec<T>` plus, at every level, the option to
 * instead describe `T` as possibly-absent (`"optional"`, wrapping a spec for `T` with
 * `undefined` excluded). Reused by `narrow`/`hostToGuest`/`guestToHost` — replaces ad-hoc
 * `as` casts everywhere a tenant reads guest-controlled data.
 *
 * Note: optionality is *not* auto-derived from an `undefined extends T`-style check —
 * this repo doesn't enable `strictNullChecks`, under which such a check is trivially
 * `true` for every `T`. Instead `"optional"` is always an available alternative; callers
 * pick it explicitly for fields whose value type includes `undefined` (see
 * `DESCRIPTOR_SPEC` in `multi_tenant.ts` for an example).
 */
export type NarrowSpec<T> =
  | BaseSpec<T>
  | { kind: "optional"; inner: BaseSpec<Exclude<T, undefined>> }
  | { kind: "defaulted"; inner: BaseSpec<Exclude<T, undefined>>; default: Exclude<T, undefined> };

/**
 * Validate + convert an `unknown` value read from tenant-managed storage into `T`.
 * Flat success/failure result — no thrown exceptions, no unchecked `as`.
 */
export function narrow<T>(
  spec: NarrowSpec<T>,
  value: unknown,
  tenant: Tenant,
): { value: T } | undefined {
  switch (spec.kind) {
    case "any":
      return { value: value as T };
    case "typeof": {
      if (spec.tag === "undefined") {
        return value === undefined ? { value: value as T } : undefined;
      }
      return typeof value === spec.tag ? { value: value as T } : undefined;
    }
    case "hostFn": {
      return typeof value === "function" ? { value: value as T } : undefined;
    }
    case "guestFn": {
      // Only validates `typeof`. Deliberately does *not* speculatively register
      // `spec.meta` for an unregistered function: a value read out of tenant storage
      // that nothing has registered might just as well be a genuine plain host
      // function, and guessing a guest ABI for it would corrupt later invocations
      // (see `invokeGuestAware`'s registry-driven dispatch). Only the actual creation
      // site (a backend's `FN` opcode) may call `markGuestFn`.
      return typeof value === "function" ? { value: value as T } : undefined;
    }
    case "optional": {
      if (value === undefined) return { value: undefined as T };
      const inner = narrow(spec.inner, value, tenant);
      return inner === undefined ? undefined : { value: inner.value as T };
    }
    case "defaulted": {
      // Missing *or* wrong-typed input falls back to `spec.default` — this never fails
      // narrow() outright, matching descriptor fields (e.g. `writable`) that guest code
      // commonly omits and which should default rather than reject the whole shape.
      if (value === undefined) return { value: spec.default as T };
      const inner = narrow(spec.inner, value, tenant);
      return inner === undefined ? { value: spec.default as T } : { value: inner.value as T };
    }
    case "object": {
      if (typeof value !== "object" || value === null) return undefined;
      // `value` is a tenant-managed (guest-visible) object — its fields are only
      // observable through the tenant, never via raw property access.
      const out: Record<string, unknown> = {};
      for (const k of Object.keys(spec.fields)) {
        const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
        const raw = tenant.driveTenant(tenant.get(value, k), false, false);
        const inner = narrow(fieldSpec, raw, tenant);
        if (inner === undefined) return undefined;
        out[k] = inner.value;
      }
      return { value: out as T };
    }
  }
}

// `hostToGuest`/`guestToHost` (the type-*rewrite* functions, as opposed to this module's
// validate-only `narrow`) live in `./rewrite.ts` — see that module for the recursive
// `HostToGuest`/`GuestToHost` type functions they're typed against.
