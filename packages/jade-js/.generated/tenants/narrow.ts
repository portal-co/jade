import { isHostTaskYield } from "../async-host.ts";
import type { Tenant, TenantInvocation } from "./types.ts";
import { createGuestGen, unpackGuestGen } from "./shims.ts";
import { yieldTenant, yieldHostTask, driveTenant } from "./driver.ts";
export type GuestFnMeta = {
    abi: "closure";
} | {
    abi: "leading-tenant-nt";
};
const GUEST_FN_REGISTRY = new WeakMap<Function, GuestFnMeta>();
export function markGuestFn<F extends Function>(fn: F, meta: GuestFnMeta): F {
    GUEST_FN_REGISTRY.set(fn, meta);
    return fn;
}
export function guestFnMeta(fn: Function): GuestFnMeta | undefined {
    return GUEST_FN_REGISTRY.get(fn);
}
export function* invokeGuestAware<Args extends readonly unknown[], R = unknown>(this: Tenant, fn: Function, thisArg: unknown, args: Args, invocation: TenantInvocation = {
    kind: "apply",
    thisArg,
    args
}): Generator<any, R, any> {
    const meta = guestFnMeta(fn);
    const raw = invocation.kind === "construct" ? meta?.abi === "leading-tenant-nt" ? (Reflect.construct(fn, [
        this,
        invocation.newTarget,
        ...args
    ], invocation.newTarget) as R) : (Reflect.construct(fn, [
        ...args
    ], invocation.newTarget) as R) : meta?.abi === "leading-tenant-nt" ? (Reflect.apply(fn, invocation.thisArg, [
        this,
        undefined,
        ...args
    ]) as R) : (Reflect.apply(fn, invocation.thisArg, args) as R);
    if (isHostTaskYield(raw)) return yield raw;
    return raw;
}
export function* invokeTrap<Args extends readonly unknown[], R = unknown>(this: Tenant, fn: Function, receiver: object, args: Args): Generator<any, R, any> {
    return yield this.yieldTenant(this.invokeGuestAware(fn, receiver, args));
}
export const guestAbiMixin = {
    markGuestFn,
    invokeGuestAware,
    invokeTrap,
    createGuestGen,
    unpackGuestGen,
    yieldTenant,
    yieldHostTask,
    driveTenant
};
type TypeofTag<T> = T extends string ? "string" : T extends number ? "number" : T extends boolean ? "boolean" : T extends symbol ? "symbol" : T extends bigint ? "bigint" : T extends undefined ? "undefined" : "object";
type BaseSpec<T> = [unknown] extends [T] ? {
    kind: "any";
} : [T] extends [(...a: any[]) => any] ? {
    kind: "hostFn";
} | {
    kind: "guestFn";
} : [T] extends [object] ? {
    kind: "object";
    fields: {
        [K in keyof T]-?: NarrowSpec<T[K]>;
    };
} : {
    kind: "typeof";
    tag: TypeofTag<T>;
};
export type NarrowSpec<T> = BaseSpec<T> | {
    kind: "optional";
    inner: BaseSpec<Exclude<T, undefined>>;
} | {
    kind: "defaulted";
    inner: BaseSpec<Exclude<T, undefined>>;
    default: Exclude<T, undefined>;
};
export function narrow<T>(spec: NarrowSpec<T>, value: unknown, tenant: Tenant): {
    value: T;
} | undefined {
    switch(spec.kind){
        case "any":
            return {
                value: value as T
            };
        case "typeof":
            {
                if (spec.tag === "undefined") {
                    return value === undefined ? {
                        value: value as T
                    } : undefined;
                }
                return typeof value === spec.tag ? {
                    value: value as T
                } : undefined;
            }
        case "hostFn":
            {
                return typeof value === "function" ? {
                    value: value as T
                } : undefined;
            }
        case "guestFn":
            {
                return typeof value === "function" ? {
                    value: value as T
                } : undefined;
            }
        case "optional":
            {
                if (value === undefined) return {
                    value: undefined as T
                };
                const inner = narrow(spec.inner, value, tenant);
                return inner === undefined ? undefined : {
                    value: inner.value as T
                };
            }
        case "defaulted":
            {
                if (value === undefined) return {
                    value: spec.default as T
                };
                const inner = narrow(spec.inner, value, tenant);
                return inner === undefined ? {
                    value: spec.default as T
                } : {
                    value: inner.value as T
                };
            }
        case "object":
            {
                if (typeof value !== "object" || value === null) return undefined;
                const out: Record<string, unknown> = {};
                for (const k of Object.keys(spec.fields)){
                    const fieldSpec = (spec.fields as Record<string, NarrowSpec<unknown>>)[k];
                    const raw = tenant.driveTenant(tenant.get(value, k), false, false);
                    const inner = narrow(fieldSpec, raw, tenant);
                    if (inner === undefined) return undefined;
                    out[k] = inner.value;
                }
                return {
                    value: out as T
                };
            }
    }
}
