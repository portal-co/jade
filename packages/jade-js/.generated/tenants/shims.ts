import type { Tenant } from "./types.ts";
export const THROUGH: symbol = Symbol.for("jade.through");
export const GUEST_NEXT: symbol = Symbol.for("jade.guest.next");
export function* createGuestGen(this: Tenant, nativeGen: Generator): any {
    const obj = (yield this.yieldTenant(this.make(null))) as object;
    const nextFn = function*(sent: any): Generator<any, {
        value: any;
        done: boolean;
    }, any> {
        let step = (nativeGen as any).next(sent);
        while(!step.done && (step.value as any)?.[THROUGH] !== undefined){
            sent = yield step.value;
            step = (nativeGen as any).next(sent);
        }
        return step;
    };
    const returnFn = function*(val: any): Generator<never, {
        value: any;
        done: boolean;
    }, any> {
        const step = typeof (nativeGen as any).return === "function" ? (nativeGen as any).return(val) : {
            value: val,
            done: true
        };
        return step;
    };
    const throwFn = function*(err: any): Generator<never, {
        value: any;
        done: boolean;
    }, any> {
        if (typeof (nativeGen as any).throw === "function") {
            return (nativeGen as any).throw(err);
        }
        throw err;
    };
    yield this.yieldTenant(this.set(obj, "next", nextFn));
    yield this.yieldTenant(this.set(obj, "return", returnFn));
    yield this.yieldTenant(this.set(obj, "throw", throwFn));
    (obj as any)[GUEST_NEXT] = nextFn;
    return obj;
}
export function isGuestGen(g: unknown): boolean {
    return (typeof g === "object" && g !== null && typeof (g as any)[GUEST_NEXT] === "function");
}
export function* unpackGuestGen(g: any): Generator {
    const nextFn: Function = (g as any)[GUEST_NEXT];
    if (typeof nextFn !== "function") {
        return yield* g;
    }
    let sent: any;
    while(true){
        const result: {
            value: any;
            done: boolean;
        } = yield* (nextFn as any).call(g, sent);
        if (result.done) return result.value;
        sent = yield result.value;
    }
}
