import { isPolyfillKey } from "@portal-solutions/semble-common";
import { isCamoKey, type Tenant } from "./index.ts";

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
export const single_tenant: Tenant = {
    make(proto: object | null = null): object {
        return Object.create(proto);
    },
    get(obj: object, key: PropertyKey): unknown {
        return (obj as any)[clean(obj as any, key as any)];
    },
    set(obj: object, key: PropertyKey, value: unknown): void {
        (obj as any)[clean(obj as any, key as any)] = value;
    },
    has(obj: object, key: PropertyKey): boolean {
        return clean(obj as any, key as any) in obj;
    },
    delete(obj: object, key: PropertyKey): void {
        delete (obj as any)[clean(obj as any, key as any)];
    },
    ownKeys(obj: object): PropertyKey[] {
        return Reflect.ownKeys(obj).filter((k) =>
            Object.getOwnPropertyDescriptor(obj, k)?.enumerable
        );
    },
    define(target: object, descriptors: object): void {
        for (const k of single_tenant.ownKeys(descriptors)) {
            Object.defineProperty(
                target,
                clean(target as any, k as any) as any,
                single_tenant.get(descriptors, k) as PropertyDescriptor
            );
        }
    },
    assign(dst: object, src: object): void {
        for (const k of single_tenant.ownKeys(src)) {
            single_tenant.set(dst, k, single_tenant.get(src, k));
        }
    },
};
