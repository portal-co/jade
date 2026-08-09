import { createNativeHostAsyncCapability, hostTaskFromPromise, hostTaskToPromise } from "./async-host.ts";
import { promisePrimordial } from "./primordials/promise.ts";
import { MultiTenant, single_tenant, markGuestFn, vm } from "./index.ts";
function assert(cond: unknown, msg: string) {
    if (!cond) throw new Error("FAIL: " + msg);
}
const LIT = (v: number)=>(v << 1) >>> 0;
const REF = (i: number)=>((i << 1) | 1) >>> 0;
class Buf {
    bytes: number[] = [];
    u16(n: number) {
        this.bytes.push(n & 0xff, (n >>> 8) & 0xff);
    }
    u32(n: number) {
        this.bytes.push(n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff);
    }
    lit32(dest: number, val: number) {
        this.u16(6);
        this.u32(dest);
        this.u32(val);
    }
    get(obj: number, key: number, dest: number) {
        this.u16(23);
        this.u32(obj);
        this.u32(key);
        this.u32(dest);
    }
    set(obj: number, key: number, val: number, dest: number) {
        this.u16(24);
        this.u32(obj);
        this.u32(key);
        this.u32(val);
        this.u32(dest);
    }
    ret(valOperand: number) {
        this.u16(0);
        this.u32(valOperand);
    }
    view(): () => DataView {
        const dv = new DataView(new Uint8Array(this.bytes).buffer);
        return ()=>dv;
    }
}
function driveSync<T>(t: MultiTenant, gen: Generator<any, T, any>): T {
    return t.driveTenant(gen, false, false) as T;
}
const hostAsync = createNativeHostAsyncCapability();
function runtime(t: MultiTenant) {
    return driveSync(t, promisePrimordial(t, hostAsync)).promiseRuntime;
}
function buildAccessorScaffold(t: MultiTenant): {
    target: object;
    descriptors: object;
    desc: object;
    stateBase: {
        0: object;
        1: string;
    };
} {
    const target = driveSync(t, t.make(null));
    const descriptors = driveSync(t, t.make(null));
    const desc = driveSync(t, t.make(null));
    driveSync(t, t.set(desc, "enumerable", true));
    driveSync(t, t.set(desc, "configurable", true));
    driveSync(t, t.set(descriptors, "k", desc));
    return {
        target: target as object,
        descriptors,
        desc,
        stateBase: {
            0: target as object,
            1: "k"
        }
    };
}
{
    const t = new MultiTenant();
    const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
    driveSync(t, t.set(desc, "get", ()=>t.yieldHostTask(runtime(t).async, hostTaskFromPromise(runtime(t).async, Promise.resolve(77)))));
    driveSync(t, t.define(target, descriptors));
    const b = new Buf();
    b.get(REF(0), REF(1), 2);
    b.ret(REF(2));
    const out = vm.runVirtualizedA(b.view(), stateBase, {
        tenant: t,
        promiseRuntime: runtime(t),
        addAsync: true,
        addGen: false
    });
    assert((await hostTaskToPromise(runtime(t).async, out)) === 77, "expected async getter to resolve to 77");
}{
    const t = new MultiTenant();
    const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
    let seen: unknown;
    driveSync(t, t.set(desc, "set", (v: unknown)=>{
        seen = v;
        return t.yieldHostTask(runtime(t).async, hostTaskFromPromise(runtime(t).async, Promise.resolve()));
    }));
    driveSync(t, t.define(target, descriptors));
    const b = new Buf();
    b.set(REF(0), REF(1), LIT(42), 2);
    b.ret(REF(2));
    const out = vm.runVirtualizedA(b.view(), stateBase, {
        tenant: t,
        promiseRuntime: runtime(t),
        addAsync: true,
        addGen: false
    });
    const resolved = await hostTaskToPromise(runtime(t).async, out);
    assert(resolved === 42, `expected async setter to return assigned value 42, got ${resolved}`);
    assert(seen === 42, `expected async setter trap to receive 42, got ${seen}`);
}{
    const t = new MultiTenant();
    const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
    driveSync(t, t.set(desc, "get", function*() {
        yield 10;
        yield 20;
        return 30;
    }));
    driveSync(t, t.define(target, descriptors));
    const b = new Buf();
    b.get(REF(0), REF(1), 2);
    b.ret(REF(2));
    const topGen = vm.runVirtualizedG(b.view(), stateBase, {
        tenant: t,
        promiseRuntime: runtime(t),
        addAsync: false,
        addGen: true
    }) as Generator;
    assert(typeof topGen.next === "function", "runVirtualizedG must return a generator when addGen is true");
    let step = topGen.next();
    while(!step.done)step = topGen.next(step.value);
    const gen = step.value;
    assert(typeof (gen as any)?.[Symbol.for("jade.guest.next")] === "function", "gen GET must return a guest-gen object when addGen is true");
    const yielded: unknown[] = [];
    for (const v of t.unpackGuestGen(gen))yielded.push(v);
    assert(JSON.stringify(yielded) === "[10,20]", `expected [10,20], got ${JSON.stringify(yielded)}`);
}{
    const t = new MultiTenant();
    const target = driveSync(t, t.make(null));
    const getter = ()=>t.yieldHostTask(runtime(t).async, hostTaskFromPromise(runtime(t).async, Promise.resolve(123)));
    const b = new Buf();
    b.u16(9);
    b.u32(3);
    b.u32(REF(1));
    b.u32(REF(2));
    b.u32(REF(3));
    b.u32(REF(4));
    b.u32(REF(5));
    b.u32(REF(6));
    b.u32(LIT(7));
    b.u16(9);
    b.u32(1);
    b.u32(REF(8));
    b.u32(REF(7));
    b.u32(REF(0) | 1);
    b.ret(LIT(0));
    const out = await hostTaskToPromise(runtime(t).async, vm.runVirtualizedA(b.view(), {
        0: target,
        1: "get",
        2: getter,
        3: "enumerable",
        4: true,
        5: "configurable",
        6: true,
        7: undefined,
        8: "k"
    }, {
        tenant: t,
        promiseRuntime: runtime(t),
        addAsync: true,
        addGen: false
    }));
    void out;
    assert((await t.driveTenant(t.get(target, "k"), true, false)) === 123, "async accessor descriptor from LITOBJ define should resolve to 123");
}{
    const t = new MultiTenant();
    const { target, descriptors, desc, stateBase } = buildAccessorScaffold(t);
    const guestGetter = function(_tenant: MultiTenant, _nt: unknown) {
        return _tenant.yieldHostTask(runtime(_tenant).async, hostTaskFromPromise(runtime(_tenant).async, Promise.resolve("guest-async")));
    };
    markGuestFn(guestGetter, {
        abi: "leading-tenant-nt"
    });
    driveSync(t, t.set(desc, "get", guestGetter));
    driveSync(t, t.define(target, descriptors));
    const b = new Buf();
    b.get(REF(0), REF(1), 2);
    b.ret(REF(2));
    const out = await hostTaskToPromise(runtime(t).async, vm.runVirtualizedA(b.view(), stateBase, {
        tenant: t,
        promiseRuntime: runtime(t),
        addAsync: true,
        addGen: false
    }));
    assert(out === "guest-async", `expected guest async getter to return 'guest-async', got ${out}`);
}{
    assert(typeof single_tenant.driveTenant === "function", "single_tenant should expose driveTenant");
    const obj = single_tenant.driveTenant(single_tenant.make(null), false, false) as object;
    single_tenant.driveTenant(single_tenant.set(obj, "x", 7), false, false);
    assert(single_tenant.driveTenant(single_tenant.get(obj, "x"), false, false) === 7, "single_tenant GET/SET through driver");
}{
    for (const t of [
        new MultiTenant(),
        single_tenant
    ]){
        assert(typeof t.driveTenant === "function", "driveTenant mixin should be present");
        assert(typeof t.yieldTenant === "function", "yieldTenant mixin should be present");
    }
}{
    const t = new MultiTenant();
    const { target, descriptors, desc } = buildAccessorScaffold(t);
    const raw = Promise.resolve("raw guest data");
    driveSync(t, t.set(desc, "get", ()=>raw));
    driveSync(t, t.define(target, descriptors));
    assert(driveSync(t, t.get(target, "k")) === raw, "raw native Promise must not be assimilated by driver");
}{
    const t = new MultiTenant();
    const capability = createNativeHostAsyncCapability();
    const task = hostTaskFromPromise(capability, Promise.resolve(88));
    const operation = function*() {
        return yield t.yieldHostTask(capability, task);
    };
    let rejected = false;
    try {
        driveSync(t, operation());
    } catch (error) {
        rejected = error instanceof TypeError && /HostTask/.test(error.message);
    }
    assert(rejected, "sync driver must reject explicit HostTask suspension");
    assert((await t.driveTenant(operation(), true, false)) === 88, "async driver must observe explicit HostTask");
}{
    const t = new MultiTenant();
    const thenable = Object.assign(()=>undefined, {
        then () {
            throw new Error("must not be called");
        }
    });
    const { target, descriptors, desc } = buildAccessorScaffold(t);
    driveSync(t, t.set(desc, "get", ()=>thenable));
    driveSync(t, t.define(target, descriptors));
    assert(driveSync(t, t.get(target, "k")) === thenable, "driver must not inspect guest thenables");
}console.log("PASS: tenant generator driver composition e2e");
