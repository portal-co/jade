import { isPolyfillKey } from "@portal-solutions/semble-common";
import { isCamoKey, type Tenant } from "./index.ts";
import { guestAbiMixin } from "./narrow.ts";

export function dirty(a: any): boolean {
    return isPolyfillKey(a) || isCamoKey(a);
}
export function clean<T extends object>(object: T, a: keyof T): keyof T {
    if (!dirty(a)) return a;
    if (typeof a === "string") {
        if (isPolyfillKey(a) || (object === globalThis && isCamoKey(a))) return `$$jade_js$$${a}` as any;
    }
    return a;
}

// Single-tenant object manager: objects are backed natively; isolation is
// limited to mangling "dirty" (polyfill / camo) keys so they cannot collide
// with or shadow host keys. Clean keys are stored verbatim.
//
// `guestAbiMixin` (markGuestFn/invokeGuestAware/invokeTrap/createGuestGen/unpackGuestGen)
// is injected via `Object.assign` below rather than duplicated here — see `Tenant`'s doc
// comment (`index.ts`).
export const single_tenant: Tenant = Object.assign({
    *make(proto: object | null = null) {
        return Object.create(proto);
    },
    *get<R = unknown>(obj: object, key: PropertyKey) {
        return (obj as any)[clean(obj as any, key as any)] as R;
    },
    *set<V = unknown>(obj: object, key: PropertyKey, value: V) {
        (obj as any)[clean(obj as any, key as any)] = value;
    },
    *has(obj: object, key: PropertyKey) {
        return clean(obj as any, key as any) in obj;
    },
    *delete(obj: object, key: PropertyKey) {
        delete (obj as any)[clean(obj as any, key as any)];
    },
    *ownKeys(obj: object) {
        return Reflect.ownKeys(obj).filter((k) =>
            Object.getOwnPropertyDescriptor(obj, k)?.enumerable
        );
    },
    *define(target: object, descriptors: object) {
        const keys = yield this.yieldTenant(this.ownKeys(descriptors));
        for (const k of keys as PropertyKey[]) {
            Object.defineProperty(
                target,
                clean(target as any, k as any) as any,
                (yield this.yieldTenant(this.get(descriptors, k))) as PropertyDescriptor,
            );
        }
    },
    *assign(dst: object, src: object) {
        const keys = yield this.yieldTenant(this.ownKeys(src));
        for (const k of keys as PropertyKey[]) {
            yield this.yieldTenant(
                this.set(dst, k, yield this.yieldTenant(this.get(src, k))),
            );
        }
    },
}, guestAbiMixin);
