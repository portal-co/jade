import type { Tenant } from "./index.ts";
import { guestFnMeta, type NarrowSpec } from "./narrow.ts";

/**
 * Combines a function's *declared* return shape (already `Promise`/`Generator`/
 * `AsyncGenerator`-wrapped, or plain) with the *ambient* `addAsync`/`addGen` upgrade
 * flags — the exact declared-bits-OR-ambient-bits idiom `fn_variant`/`effectiveVariant`
 * use on the Rust JIT (`Config.add_async`/`add_gen`) and `vm.ts` sides. Both unset leaves
 * `R` as-is; `addAsync` wraps in `Promise` (or `AsyncGenerator` if `addGen` is also set);
 * `addGen` alone wraps in `Generator`. If `R` is already one of these wrapper shapes, the
 * ambient flags upgrade it further (e.g. `Generator<T>` + `addAsync` becomes
 * `AsyncGenerator<T>`) rather than double-wrapping.
 */
export type FnResult<R, AA extends boolean = false, AG extends boolean = false> =
  R extends AsyncGenerator<infer Y, infer Ret, infer Next>
    ? AsyncGenerator<Y, Ret, Next>
    : R extends Generator<infer Y, infer Ret, infer Next>
      ? (AA extends true ? AsyncGenerator<Y, Ret, Next> : Generator<Y, Ret, Next>)
      : R extends Promise<infer U>
        ? (AG extends true ? AsyncGenerator<U, void, unknown> : Promise<U>)
        : AA extends true
          ? (AG extends true ? AsyncGenerator<R, void, unknown> : Promise<R>)
          : AG extends true
            ? Generator<R, void, unknown>
            : R;

/**
 * The type-level counterpart of `hostToGuest`: given a host-side type `T`, compute the
 * type as observed from the guest side after conversion. Recurses structurally, mirroring
 * `hostToGuest`'s own runtime recursion (object fields convert in place; a function's
 * parameters flow the *opposite* direction — `GuestToHost` — since they're inputs the
 * guest side supplies, while its return flows `HostToGuest` like the function itself
 * does; `Promise`/`Generator`/`AsyncGenerator` unwrap-and-rewrap their own type
 * parameters). `AA`/`AG` are only consulted for a bare function's return type, combining
 * with whatever the declared return shape already is via `FnResult`.
 *
 * `Args` (defaulted to `never`, meaning "infer from `T` itself") is an escape hatch for a
 * loosely-typed callable (e.g. `Function`/`(...a: any[]) => R`) whose real parameter tuple
 * can't be recovered structurally — supply it explicitly to override the inferred tuple.
 *
 * Note: deep recursion over very large or self-referential object types can hit
 * TypeScript's recursion-depth limit — an accepted tradeoff, not something to route
 * around with an ad-hoc depth counter.
 */
export type HostToGuest<
  T,
  AA extends boolean = false,
  AG extends boolean = false,
  Args extends readonly unknown[] = never,
> = T extends (...args: infer A) => infer R
  ? (
      // `[Args] extends [never]`, not `Args extends never`: a bare-type-parameter check
      // against `never` distributes and collapses the whole conditional to `never` when
      // `Args` (the default) actually *is* `never` — wrapping both sides in a tuple
      // suppresses that distribution.
      ...args: [Args] extends [never] ? { [K in keyof A]: GuestToHost<A[K]> } : Args
    ) => FnResult<HostToGuest<R>, AA, AG>
  : T extends Promise<infer U>
    ? Promise<HostToGuest<U>>
    : T extends AsyncGenerator<infer Y, infer Ret, infer Next>
      ? AsyncGenerator<HostToGuest<Y>, HostToGuest<Ret>, GuestToHost<Next>>
      : T extends Generator<infer Y, infer Ret, infer Next>
        ? Generator<HostToGuest<Y>, HostToGuest<Ret>, GuestToHost<Next>>
        : T extends readonly (infer U)[]
          ? HostToGuest<U>[]
          : T extends object
            ? { [K in keyof T]: HostToGuest<T[K]> }
            : T; // primitives pass through unchanged

/** The reverse of `HostToGuest`: the type-level counterpart of `guestToHost`. */
export type GuestToHost<
  T,
  AA extends boolean = false,
  AG extends boolean = false,
  Args extends readonly unknown[] = never,
> = T extends (...args: infer A) => infer R
  ? (
      ...args: [Args] extends [never] ? { [K in keyof A]: HostToGuest<A[K]> } : Args
    ) => FnResult<GuestToHost<R>, AA, AG>
  : T extends Promise<infer U>
    ? Promise<GuestToHost<U>>
    : T extends AsyncGenerator<infer Y, infer Ret, infer Next>
      ? AsyncGenerator<GuestToHost<Y>, GuestToHost<Ret>, HostToGuest<Next>>
      : T extends Generator<infer Y, infer Ret, infer Next>
        ? Generator<GuestToHost<Y>, GuestToHost<Ret>, HostToGuest<Next>>
        : T extends readonly (infer U)[]
          ? GuestToHost<U>[]
          : T extends object
            ? { [K in keyof T]: GuestToHost<T[K]> }
            : T;

/**
 * Recursively remap a host-side value for guest consumption through `tenant`. Function
 * fields pass through unchanged: from the guest side, calling a host function is always a
 * plain call, and `typeof` must not change either way.
 *
 * Typed against `HostToGuest<T>` (rather than bare `unknown`) so a hand-written guest-side
 * stub function can be typechecked for assignment-compatibility against
 * `HostToGuest<SomeHostFnType>` directly — manually writing a typechecked guest function
 * is possible without running this conversion at all.
 */
export function hostToGuest<T>(spec: NarrowSpec<T>, value: T, tenant: Tenant): HostToGuest<T> {
  switch (spec.kind) {
    case "any":
    case "hostFn":
    case "guestFn":
    case "typeof":
      return value as HostToGuest<T>;
    case "optional":
      return (
        value === undefined
          ? undefined
          : hostToGuest(spec.inner, value as Exclude<T, undefined>, tenant)
      ) as HostToGuest<T>;
    case "defaulted":
      return hostToGuest(
        spec.inner,
        (value === undefined ? spec.default : value) as Exclude<T, undefined>,
        tenant,
      ) as HostToGuest<T>;
    case "object": {
      // Build the result as a genuine tenant-managed object: guest code must only ever
      // observe it through `tenant.get`, never via raw property access.
      const obj = tenant.driveTenant(tenant.make(null), false, false);
      for (const k of Object.keys(spec.fields)) {
        const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
        const converted = hostToGuest(fieldSpec, (value as Record<string, unknown>)[k], tenant);
        tenant.driveTenant(tenant.set(obj, k, converted), false, false);
      }
      return obj as HostToGuest<T>;
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
export function guestToHost<T>(spec: NarrowSpec<T>, value: unknown, tenant: Tenant): GuestToHost<T> {
  switch (spec.kind) {
    case "any":
    case "hostFn":
      return value as GuestToHost<T>;
    case "guestFn": {
      if (typeof value !== "function") return value as GuestToHost<T>;
      // Only adapt a function with a real, registered "leading-tenant-nt" ABI. An
      // unregistered function is treated as already plain-callable (`spec.meta` is
      // never used as a guess here, for the same reason `narrow`'s "guestFn" case
      // doesn't speculatively register it — see there).
      const meta = guestFnMeta(value);
      if (!meta || meta.abi === "closure") return value as GuestToHost<T>;
      const adapter = (...args: unknown[]) =>
        tenant.driveTenant(tenant.invokeGuestAware(value, undefined, args), false, false);
      tenant.markGuestFn(adapter, { abi: "closure" });
      return adapter as GuestToHost<T>;
    }
    case "optional": {
      if (value === undefined) return undefined as GuestToHost<T>;
      return guestToHost(spec.inner, value, tenant) as GuestToHost<T>;
    }
    case "defaulted": {
      if (value === undefined) return spec.default as GuestToHost<T>;
      return guestToHost(spec.inner, value, tenant) as GuestToHost<T>;
    }
    case "object": {
      // `value` is a tenant-managed (guest-visible) object — read its fields through the
      // tenant; the result is a genuine plain host-side object for host consumption.
      const isGuestObject = typeof value === "object" && value !== null;
      const out: Record<string, unknown> = {};
      for (const k of Object.keys(spec.fields)) {
        const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
        const raw = isGuestObject
        ? tenant.driveTenant(tenant.get(value as object, k), false, false)
        : undefined;
        out[k] = guestToHost(fieldSpec, raw, tenant);
      }
      return out as GuestToHost<T>;
    }
    case "typeof":
      return value as GuestToHost<T>;
  }
}
