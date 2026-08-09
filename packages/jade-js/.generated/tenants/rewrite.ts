import type { HostTask } from "../async-host.ts";
import type { GuestPromise } from "../primordials/promise.ts";
import type { Tenant } from "./types.ts";
import { guestFnMeta, type NarrowSpec } from "./narrow.ts";
export type HostFnResult<R, AA extends boolean = false, AG extends boolean = false> = R extends AsyncGenerator<infer Y, infer Ret, infer Next> ? AsyncGenerator<Y, Ret, Next> : R extends Generator<infer Y, infer Ret, infer Next> ? (AA extends true ? AsyncGenerator<Y, Ret, Next> : Generator<Y, Ret, Next>) : R extends GuestPromise<infer U> ? (AG extends true ? AsyncGenerator<U, void, unknown> : HostTask<U>) : AA extends true ? (AG extends true ? AsyncGenerator<R, void, unknown> : HostTask<R>) : AG extends true ? Generator<R, void, unknown> : R;
export type GuestFnResult<R, AA extends boolean = false, AG extends boolean = false> = R extends AsyncGenerator<infer Y, infer Ret, infer Next> ? AsyncGenerator<Y, Ret, Next> : R extends Generator<infer Y, infer Ret, infer Next> ? (AA extends true ? AsyncGenerator<Y, Ret, Next> : Generator<Y, Ret, Next>) : R extends HostTask<infer U> ? (AG extends true ? AsyncGenerator<U, void, unknown> : GuestPromise<U>) : AA extends true ? (AG extends true ? AsyncGenerator<R, void, unknown> : GuestPromise<R>) : AG extends true ? Generator<R, void, unknown> : R;
export type FnResult<R, AA extends boolean = false, AG extends boolean = false> = HostFnResult<R, AA, AG>;
export type HostToGuest<T, AA extends boolean = false, AG extends boolean = false, Args extends readonly unknown[] = never> = T extends (...args: infer A) => infer R ? (...args: [Args] extends [never] ? {
    [K in keyof A]: GuestToHost<A[K]>;
} : Args) => GuestFnResult<HostToGuest<R>, AA, AG> : T extends HostTask<infer U> ? GuestPromise<HostToGuest<U>> : T extends AsyncGenerator<infer Y, infer Ret, infer Next> ? AsyncGenerator<HostToGuest<Y>, HostToGuest<Ret>, GuestToHost<Next>> : T extends Generator<infer Y, infer Ret, infer Next> ? Generator<HostToGuest<Y>, HostToGuest<Ret>, GuestToHost<Next>> : T extends readonly (infer U)[] ? HostToGuest<U>[] : T extends object ? {
    [K in keyof T]: HostToGuest<T[K]>;
} : T;
export type GuestToHost<T, AA extends boolean = false, AG extends boolean = false, Args extends readonly unknown[] = never> = T extends (...args: infer A) => infer R ? (...args: [Args] extends [never] ? {
    [K in keyof A]: HostToGuest<A[K]>;
} : Args) => HostFnResult<GuestToHost<R>, AA, AG> : T extends GuestPromise<infer U> ? HostTask<GuestToHost<U>> : T extends AsyncGenerator<infer Y, infer Ret, infer Next> ? AsyncGenerator<GuestToHost<Y>, GuestToHost<Ret>, HostToGuest<Next>> : T extends Generator<infer Y, infer Ret, infer Next> ? Generator<GuestToHost<Y>, GuestToHost<Ret>, HostToGuest<Next>> : T extends readonly (infer U)[] ? GuestToHost<U>[] : T extends object ? {
    [K in keyof T]: GuestToHost<T[K]>;
} : T;
export function hostToGuest<T>(spec: NarrowSpec<T>, value: T, tenant: Tenant): HostToGuest<T> {
    switch(spec.kind){
        case "any":
        case "hostFn":
        case "guestFn":
        case "typeof":
            return value as HostToGuest<T>;
        case "optional":
            return (value === undefined ? undefined : hostToGuest(spec.inner, value as Exclude<T, undefined>, tenant)) as HostToGuest<T>;
        case "defaulted":
            return hostToGuest(spec.inner, (value === undefined ? spec.default : value) as Exclude<T, undefined>, tenant) as HostToGuest<T>;
        case "object":
            {
                const obj = tenant.driveTenant(tenant.make(null), false, false);
                for (const k of Object.keys(spec.fields)){
                    const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
                    const converted = hostToGuest(fieldSpec, (value as Record<string, unknown>)[k], tenant);
                    tenant.driveTenant(tenant.set(obj, k, converted), false, false);
                }
                return obj as HostToGuest<T>;
            }
    }
}
export function guestToHost<T>(spec: NarrowSpec<T>, value: unknown, tenant: Tenant): GuestToHost<T> {
    switch(spec.kind){
        case "any":
        case "hostFn":
            return value as GuestToHost<T>;
        case "guestFn":
            {
                if (typeof value !== "function") return value as GuestToHost<T>;
                const meta = guestFnMeta(value);
                if (!meta || meta.abi === "closure") return value as GuestToHost<T>;
                const adapter = (...args: unknown[])=>tenant.driveTenant(tenant.invokeGuestAware(value, undefined, args), false, false);
                tenant.markGuestFn(adapter, {
                    abi: "closure"
                });
                return adapter as GuestToHost<T>;
            }
        case "optional":
            {
                if (value === undefined) return undefined as GuestToHost<T>;
                return guestToHost(spec.inner, value, tenant) as GuestToHost<T>;
            }
        case "defaulted":
            {
                if (value === undefined) return spec.default as GuestToHost<T>;
                return guestToHost(spec.inner, value, tenant) as GuestToHost<T>;
            }
        case "object":
            {
                const isGuestObject = typeof value === "object" && value !== null;
                const out: Record<string, unknown> = {};
                for (const k of Object.keys(spec.fields)){
                    const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
                    const raw = isGuestObject ? tenant.driveTenant(tenant.get(value as object, k), false, false) : undefined;
                    out[k] = guestToHost(fieldSpec, raw, tenant);
                }
                return out as GuestToHost<T>;
            }
        case "typeof":
            return value as GuestToHost<T>;
    }
}
