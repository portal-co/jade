import { isPolyfillKey } from "@portal-solutions/semble-common";
import type { GuestFnMeta } from "./narrow.ts";

export function isCamoKey(a: any): boolean {
  return (
    a === "location" ||
    a === "eval" ||
    a === "parent" ||
    a === "top" ||
    a === "ActiveXObject"
  );
}
/**
 * A tenant is an abstract object manager: it isolates a virtual environment by
 * owning the representation of the objects it creates. Implementations are free
 * to back objects natively (single-tenant) or out-of-band in a `WeakMap`
 * shadow (multi-tenant) — the VM only ever manipulates objects through these
 * methods, never via direct property access.
 *
 * `markGuestFn`/`invokeGuestAware`/`invokeTrap`/`createGuestGen`/`unpackGuestGen` are the
 * guest/host ABI + generator-shim helpers (see `narrow.ts`/`shims.ts` for their single
 * shared implementations, injected onto every `Tenant` implementation via
 * `guestAbiMixin` rather than duplicated) — attached here, rather than left as free
 * module-level imports, so that a tenant method body referencing them via `this.<method>`
 * remains inlinable by the JIT's tenant-method-inlining feature (see
 * `docs/pluggable-tenant-interface-plan.md`): the inlining scanner
 * (`crates/jade-vm-frontend/src/tenant_inline.rs`) rejects a method for referencing any
 * free (non-parameter, non-`this`-derived, non-global) identifier, since such a reference
 * can't resolve at the JIT's arbitrary splice site — putting every non-builtin dependency
 * a tenant method could need onto `this` closes that gap generally, not just for these
 * two ABI-dispatch helpers specifically.
 */
export interface Tenant {
  /** Create a fresh isolated object with the given prototype (null by default). */
  make(proto?: object | null): object;
  /** Read a property. */
  get<R = unknown>(obj: object, key: PropertyKey): R;
  /** Write a (data) property. */
  set<V = unknown>(obj: object, key: PropertyKey, value: V): void;
  /** Whether the object has an own property `key`. */
  has(obj: object, key: PropertyKey): boolean;
  /** Delete an own property. */
  delete(obj: object, key: PropertyKey): void;
  /** Own enumerable keys. */
  ownKeys(obj: object): PropertyKey[];
  /**
   * Apply property descriptors to `target`. `descriptors` is itself a
   * tenant-managed object mapping keys to descriptor objects (as produced by the
   * LITOBJ "define" path), so it is read through this tenant.
   */
  define(target: object, descriptors: object): void;
  /** Copy own enumerable properties from `src` into `dst` (object spread). */
  assign(dst: object, src: object): void;
  /** Register `fn`'s guest calling convention. See `narrow.ts`. */
  markGuestFn<F extends Function>(fn: F, meta: GuestFnMeta): F;
  /** Invoke `fn` respecting its actual registered ABI. See `narrow.ts`. */
  invokeGuestAware<Args extends readonly unknown[], R = unknown>(
    fn: Function,
    thisArg: unknown,
    args: Args,
  ): R;
  /** Invoke a property-descriptor getter/setter ("trap"). See `narrow.ts`. */
  invokeTrap<Args extends readonly unknown[], R = unknown>(
    fn: Function,
    receiver: object,
    args: Args,
  ): R;
  /** Wrap a native generator as a guest-side generator object. See `shims.ts`. */
  createGuestGen<Y, Ret = unknown>(nativeGen: Generator<Y, Ret, unknown>): unknown;
  /** Adapt a guest-side generator object back to the native generator protocol. See `shims.ts`. */
  unpackGuestGen(g: unknown): Generator;
}
import { single_tenant } from "./single_tenant.ts";
export { single_tenant };
import * as vm from "./vm.ts";
export { isPolyfillKey, vm };
export * from "./gc.ts";
export { Tenant as MultiTenant } from "./multi_tenant.ts";
export * from "./narrow.ts";
export * from "./rewrite.ts";
