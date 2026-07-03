import { isPolyfillKey } from "@portal-solutions/semble-common";

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
 */
export interface Tenant {
  /** Create a fresh isolated object with the given prototype (null by default). */
  make(proto?: object | null): object;
  /** Read a property. */
  get(obj: object, key: PropertyKey): unknown;
  /** Write a (data) property. */
  set(obj: object, key: PropertyKey, value: unknown): void;
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
}
import { single_tenant } from "./single_tenant.ts";
export { single_tenant };
import * as vm from "./vm.ts";
export { isPolyfillKey, vm };
export * from "./gc.ts";
export { Tenant as MultiTenant } from "./multi_tenant.ts";
export * from "./narrow.ts";
