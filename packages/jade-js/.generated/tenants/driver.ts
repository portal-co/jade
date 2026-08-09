import { hostTaskPromise, hostTaskYield, isHostTaskYield, type HostAsyncCapability, type HostTask } from "../async-host.ts";
import type { Tenant, TenantOp } from "./types.ts";
import { isGuestGen } from "./shims.ts";
import { TENANT_OP } from "./types.ts";
export { TENANT_OP };
function isTenantOp(value: unknown): value is TenantOp<unknown> {
    return value !== null && typeof value === "object" && (value as any)[TENANT_OP] !== undefined;
}
function isNativeIterator(value: unknown): value is Iterator<unknown, any, any> {
    return value !== null && (typeof value === "object" || typeof value === "function") && typeof (value as any).next === "function" && !isGuestGen(value);
}
export function yieldTenant<T>(this: Tenant, gen: Generator<any, T, any>): TenantOp<T> {
    const op: TenantOp<T> = {} as TenantOp<T>;
    (op as any)[TENANT_OP] = gen;
    return op;
}
export function yieldHostTask<T>(this: Tenant, capability: HostAsyncCapability, task: HostTask<T>) {
    void this;
    return hostTaskYield(capability, task);
}
function driveTenantSync<T>(this: Tenant, gen: Generator<any, T, any>): T {
    let step = gen.next();
    while(!step.done){
        const value = step.value;
        if (isTenantOp(value)) step = gen.next(this.driveTenant(value[TENANT_OP], false, false));
        else if (isHostTaskYield(value)) throw new TypeError("tenant operation yielded a HostTask without addAsync");
        else if (isNativeIterator(value)) throw new TypeError("tenant operation yielded an iterator without addGen");
        else step = gen.next(value);
    }
    return step.value;
}
async function driveTenantAsync<T>(this: Tenant, gen: Generator<any, T, any>): Promise<T> {
    let step = gen.next();
    while(!step.done){
        const value = step.value;
        if (isTenantOp(value)) step = gen.next(await this.driveTenant(value[TENANT_OP], true, false));
        else if (isHostTaskYield(value)) step = gen.next(await hostTaskPromise(value.capability, value.task));
        else if (isNativeIterator(value)) throw new TypeError("tenant operation yielded an iterator without addGen");
        else step = gen.next(value);
    }
    return step.value;
}
function* driveTenantGen<T>(this: Tenant, gen: Generator<any, T, any>): Generator<any, T, any> {
    let step = gen.next();
    while(!step.done){
        const value = step.value;
        if (isTenantOp(value)) {
            let result: any = yield* this.driveTenant(value[TENANT_OP], false, true) as Generator<any, any, any>;
            if (isNativeIterator(result)) result = yield* this.driveTenant(this.createGuestGen(result as Generator), false, true) as Generator<any, any, any>;
            step = gen.next(result);
        } else if (isHostTaskYield(value)) {
            throw new TypeError("tenant operation yielded a HostTask without addAsync");
        } else if (isNativeIterator(value)) {
            step = gen.next(yield* this.driveTenant(this.createGuestGen(value as Generator), false, true) as Generator<any, any, any>);
        } else step = gen.next(yield value);
    }
    return step.value;
}
async function* driveTenantAsyncGen<T>(this: Tenant, gen: Generator<any, T, any>): AsyncGenerator<any, T, any> {
    let step = gen.next();
    while(!step.done){
        const value = step.value;
        if (isTenantOp(value)) {
            let result: any = yield* this.driveTenant(value[TENANT_OP], true, true) as AsyncGenerator<any, any, any>;
            if (isNativeIterator(result)) result = yield* this.driveTenant(this.createGuestGen(result as Generator), true, true) as AsyncGenerator<any, any, any>;
            step = gen.next(result);
        } else if (isHostTaskYield(value)) {
            step = gen.next(await hostTaskPromise(value.capability, value.task));
        } else if (isNativeIterator(value)) {
            step = gen.next(yield* this.driveTenant(this.createGuestGen(value as Generator), true, true) as AsyncGenerator<any, any, any>);
        } else step = gen.next(yield value);
    }
    return step.value;
}
export function driveTenant<T>(this: Tenant, gen: Generator<any, T, any>, addAsync: boolean, addGen: boolean): any {
    if (addGen) return addAsync ? driveTenantAsyncGen.call(this, gen) : driveTenantGen.call(this, gen);
    return addAsync ? driveTenantAsync.call(this, gen) : driveTenantSync.call(this, gen);
}
