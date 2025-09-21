import { isPolyfillKey } from "@portal-solutions/semble-common";
import { isCamoKey } from "./index.ts";

export function dirty(a: any): boolean {
    return isPolyfillKey(a) || isCamoKey(a);
}
export function clean<T extends object>(a: keyof T, object: T): keyof T {
    if (!dirty(a)) return a;
    if (typeof a === "string") {
        if (isPolyfillKey(a) || (object === globalThis && isCamoKey(a))) return `$$jade_js$$${a}` as any;
    }
    return a;
}