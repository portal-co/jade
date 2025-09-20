import { isPolyfillKey } from "@portal-solutions/semble-common"
export function isCamoKey(a: any): boolean {
    return a === "location" || a === "eval" || a === "parent" || a === "top" || a === "ActiveXObject";
}
import * as _single_tenant from './single_tenant.ts'
export const single_tenant = _single_tenant;