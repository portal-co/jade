import type { Tenant } from "./index.ts";

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
 */
export function invokeGuestAware(
  fn: Function,
  tenant: Tenant,
  thisArg: unknown,
  args: unknown[],
): unknown {
  const meta = guestFnMeta(fn);
  if (meta?.abi === "leading-tenant-nt") {
    return Reflect.apply(fn, thisArg, [tenant, undefined, ...args]);
  }
  return Reflect.apply(fn, thisArg, args);
}

/**
 * Invoke a property-descriptor getter/setter ("trap") with the correct receiver and ABI,
 * whether it turns out to be a guest function or a plain host function.
 */
export function invokeTrap(
  fn: Function,
  tenant: Tenant,
  receiver: object,
  args: unknown[],
): unknown {
  return invokeGuestAware(fn, tenant, receiver, args);
}

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
        const raw = tenant.get(value, k);
        const inner = narrow(fieldSpec, raw, tenant);
        if (inner === undefined) return undefined;
        out[k] = inner.value;
      }
      return { value: out as T };
    }
  }
}

/**
 * Recursively remap a host-side value for guest consumption through `tenant`. Function
 * fields pass through unchanged: from the guest side, calling a host function is always a
 * plain call, and `typeof` must not change either way.
 */
export function hostToGuest<T>(spec: NarrowSpec<T>, value: T, tenant: Tenant): unknown {
  switch (spec.kind) {
    case "any":
    case "hostFn":
    case "guestFn":
    case "typeof":
      return value;
    case "optional":
      return value === undefined
        ? undefined
        : hostToGuest(spec.inner, value as Exclude<T, undefined>, tenant);
    case "defaulted":
      return hostToGuest(
        spec.inner,
        (value === undefined ? spec.default : value) as Exclude<T, undefined>,
        tenant,
      );
    case "object": {
      // Build the result as a genuine tenant-managed object: guest code must only ever
      // observe it through `tenant.get`, never via raw property access.
      const obj = tenant.make(null);
      for (const k of Object.keys(spec.fields)) {
        const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
        const converted = hostToGuest(fieldSpec, (value as Record<string, unknown>)[k], tenant);
        tenant.set(obj, k, converted);
      }
      return obj;
    }
  }
}

/**
 * The reverse: remap a guest-observed value for host consumption through `tenant`. A
 * "leading-tenant-nt"-ABI guest function is adapted into a genuinely plain-callable
 * function (still `typeof === 'function'`, never a `.call()`-style unsafe cast) so a host
 * caller can invoke it directly; a "closure"-ABI guest function (or a host function) is
 * already plain-callable and passes through unchanged.
 */
export function guestToHost<T>(spec: NarrowSpec<T>, value: unknown, tenant: Tenant): T {
  switch (spec.kind) {
    case "any":
    case "hostFn":
      return value as T;
    case "guestFn": {
      if (typeof value !== "function") return value as T;
      // Only adapt a function with a real, registered "leading-tenant-nt" ABI. An
      // unregistered function is treated as already plain-callable (`spec.meta` is
      // never used as a guess here, for the same reason `narrow`'s "guestFn" case
      // doesn't speculatively register it — see there).
      const meta = guestFnMeta(value);
      if (!meta || meta.abi === "closure") return value as T;
      const adapter = (...args: unknown[]) => invokeGuestAware(value, tenant, undefined, args);
      markGuestFn(adapter, { abi: "closure" });
      return adapter as T;
    }
    case "optional": {
      if (value === undefined) return undefined as T;
      return guestToHost(spec.inner, value, tenant) as T;
    }
    case "defaulted": {
      if (value === undefined) return spec.default as T;
      return guestToHost(spec.inner, value, tenant) as T;
    }
    case "object": {
      // `value` is a tenant-managed (guest-visible) object — read its fields through the
      // tenant; the result is a genuine plain host-side object for host consumption.
      const isGuestObject = typeof value === "object" && value !== null;
      const out: Record<string, unknown> = {};
      for (const k of Object.keys(spec.fields)) {
        const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
        const raw = isGuestObject ? tenant.get(value as object, k) : undefined;
        out[k] = guestToHost(fieldSpec, raw, tenant);
      }
      return out as T;
    }
    case "typeof":
      return value as T;
  }
}
