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
import * as single_tenant from "./single_tenant.ts";
import * as vm from "./vm.ts";
export { isPolyfillKey, single_tenant, vm };
export * from "./gc.ts";
