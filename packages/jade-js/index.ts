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
export interface Tenant {
  clean<T extends object>(object: T, a: keyof T): keyof T;
}
import * as _single_tenant_check from "./single_tenant.ts";
export const single_tenant: Tenant = _single_tenant_check;
import * as vm from "./vm.ts";
export { isPolyfillKey, vm };
export * from "./gc.ts";
export { Tenant as MultiTenant } from "./multi_tenant.ts";
