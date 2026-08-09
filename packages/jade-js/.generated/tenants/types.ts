import { isPolyfillKey } from "@portal-solutions/semble-common";
import type { GuestFnMeta } from "./narrow.ts";
import type { HostAsyncCapability, HostTask, HostTaskYield } from "../async-host.ts";
import type { GuestFnResult } from "./rewrite.ts";
export { isPolyfillKey };
export function isCamoKey(a: any): boolean {
    return (a === "location" || a === "eval" || a === "parent" || a === "top" || a === "ActiveXObject");
}
export const TENANT_OP: unique symbol = Symbol.for("jade.tenantOp");
export interface TenantOp<T> {
    readonly [TENANT_OP]: Generator<any, T, any>;
}
type TenantYield = TenantOp<any> | HostTaskYield<any> | Iterator<any, any, any>;
export type TenantGenerator<R> = Generator<any, R, any>;
export type TenantOpResult<R, AA extends boolean, AG extends boolean> = GuestFnResult<R, AA, AG>;
export type TenantInvocation = {
    kind: "apply";
    thisArg: unknown;
    args: readonly unknown[];
} | {
    kind: "construct";
    args: readonly unknown[];
    newTarget: Function;
};
export interface TenantPropertyDescriptor {
    value?: unknown;
    writable?: boolean;
    get?: Function;
    set?: Function;
    enumerable?: boolean;
    configurable?: boolean;
}
export interface TenantExoticHandler {
    get?(receiver: object, key: PropertyKey): TenantGenerator<unknown>;
    set?(receiver: object, key: PropertyKey, value: unknown): TenantGenerator<void>;
    has?(receiver: object, key: PropertyKey): TenantGenerator<boolean>;
    delete?(receiver: object, key: PropertyKey): TenantGenerator<void>;
    ownKeys?(receiver: object): TenantGenerator<PropertyKey[]>;
    ownPropertyKeys?(receiver: object): TenantGenerator<PropertyKey[]>;
    getOwnPropertyDescriptor?(receiver: object, key: PropertyKey): TenantGenerator<TenantPropertyDescriptor | undefined>;
    defineProperty?(receiver: object, key: PropertyKey, descriptor: TenantPropertyDescriptor): TenantGenerator<boolean>;
    getPrototypeOf?(receiver: object): TenantGenerator<object | null>;
    setPrototypeOf?(receiver: object, prototype: object | null): TenantGenerator<boolean>;
    isExtensible?(receiver: object): TenantGenerator<boolean>;
    preventExtensions?(receiver: object): TenantGenerator<boolean>;
    define?(receiver: object, descriptors: object): TenantGenerator<void>;
    assign?(receiver: object, source: object): TenantGenerator<void>;
}
export interface TenantCallableExoticHandler extends TenantExoticHandler {
    apply?(receiver: Function, thisArg: unknown, args: readonly unknown[]): TenantGenerator<unknown>;
    construct?(receiver: Function, newTarget: Function, args: readonly unknown[]): TenantGenerator<object>;
}
export interface TenantProvider extends Tenant {
    ownsObject(value: object): boolean;
}
export interface Tenant {
    make(proto?: object | null): TenantGenerator<object>;
    get<R = unknown>(obj: object, key: PropertyKey): TenantGenerator<R>;
    set<V = unknown>(obj: object, key: PropertyKey, value: V): TenantGenerator<void>;
    has(obj: object, key: PropertyKey): TenantGenerator<boolean>;
    delete(obj: object, key: PropertyKey): TenantGenerator<void>;
    ownKeys(obj: object): TenantGenerator<PropertyKey[]>;
    ownPropertyKeys(obj: object): TenantGenerator<PropertyKey[]>;
    getOwnPropertyDescriptor(obj: object, key: PropertyKey): TenantGenerator<TenantPropertyDescriptor | undefined>;
    defineProperty(obj: object, key: PropertyKey, descriptor: TenantPropertyDescriptor): TenantGenerator<boolean>;
    getPrototypeOf(obj: object): TenantGenerator<object | null>;
    setPrototypeOf(obj: object, prototype: object | null): TenantGenerator<boolean>;
    isExtensible(obj: object): TenantGenerator<boolean>;
    preventExtensions(obj: object): TenantGenerator<boolean>;
    define(target: object, descriptors: object): TenantGenerator<void>;
    assign(dst: object, src: object): TenantGenerator<void>;
    markGuestFn<F extends Function>(fn: F, meta: GuestFnMeta): F;
    makeExotic(proto: object | null | undefined, handler: TenantExoticHandler): TenantGenerator<object>;
    makeFunction(implementation: Function, options?: {
        proto?: object | null;
        guestMeta?: GuestFnMeta;
    }): TenantGenerator<Function>;
    makeCallableExotic(proto: object | null | undefined, handler: TenantCallableExoticHandler): TenantGenerator<Function>;
    invoke(callee: Function, invocation: TenantInvocation): TenantGenerator<unknown>;
    invokeGuestAware<Args extends readonly unknown[], R = unknown>(fn: Function, thisArg: unknown, args: Args, invocation?: TenantInvocation): TenantGenerator<R>;
    invokeTrap<Args extends readonly unknown[], R = unknown>(fn: Function, receiver: object, args: Args): TenantGenerator<R>;
    createGuestGen<Y, Ret = unknown>(nativeGen: Generator<Y, Ret, unknown>): TenantGenerator<unknown>;
    unpackGuestGen(g: unknown): Generator;
    yieldHostTask<T>(capability: HostAsyncCapability, task: HostTask<T>): HostTaskYield<T>;
    yieldTenant<T>(gen: Generator<any, T, any>): TenantOp<T>;
    driveTenant<T>(gen: Generator<any, T, any>, addAsync: boolean, addGen: boolean): any;
}
