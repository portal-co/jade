import { MultiTenant, markGuestFn, guestFnMeta } from "./index.ts";
function assert(cond: unknown, msg: string) {
    if (!cond) throw new Error("FAIL: " + msg);
}
function driveSync<T>(t: MultiTenant, gen: Generator<any, T, any>): T {
    return t.driveTenant(gen, false, false) as T;
}
{
    const t = new MultiTenant();
    const target = t.make(null);
    const descriptors = t.make(null);
    const desc = t.make(null);
    let calls = 0;
    driveSync(t, t.set(desc, "get", ()=>{
        calls++;
        return 99;
    }));
    driveSync(t, t.set(desc, "enumerable", true));
    driveSync(t, t.set(desc, "configurable", true));
    driveSync(t, t.set(descriptors, "k", desc));
    driveSync(t, t.define(target, descriptors));
    const v = driveSync(t, t.get(target, "k"));
    assert(v === 99, `expected getter trap to return 99, got ${v}`);
    assert(calls === 1, `expected getter to run exactly once, got ${calls}`);
}{
    const t = new MultiTenant();
    const target = t.make(null);
    const descriptors = t.make(null);
    const desc = t.make(null);
    let seen: unknown;
    driveSync(t, t.set(desc, "set", (v: unknown)=>{
        seen = v;
    }));
    driveSync(t, t.set(desc, "enumerable", true));
    driveSync(t, t.set(desc, "configurable", true));
    driveSync(t, t.set(descriptors, "k", desc));
    driveSync(t, t.define(target, descriptors));
    driveSync(t, t.set(target, "k", 123));
    assert(seen === 123, `expected setter trap to receive 123, got ${seen}`);
    assert(driveSync(t, t.get(target, "k")) === undefined, "set-only accessor get() should be undefined");
}{
    const t = new MultiTenant();
    const target = t.make(null);
    const descriptors = t.make(null);
    const desc = t.make(null);
    let seenTenant: unknown;
    let seenExtraArgs = -1;
    const guestGetter = function(tenantArg: unknown, _nt: unknown, ...rest: unknown[]) {
        seenTenant = tenantArg;
        seenExtraArgs = rest.length;
        return "guest-value";
    };
    markGuestFn(guestGetter, {
        abi: "leading-tenant-nt"
    });
    driveSync(t, t.set(desc, "get", guestGetter));
    driveSync(t, t.set(desc, "enumerable", true));
    driveSync(t, t.set(desc, "configurable", true));
    driveSync(t, t.set(descriptors, "k", desc));
    driveSync(t, t.define(target, descriptors));
    const v = driveSync(t, t.get(target, "k"));
    assert(v === "guest-value", `expected guest getter trap result, got ${v}`);
    assert(seenTenant === t, "guest getter trap should receive the tenant as its leading param");
    assert(seenExtraArgs === 0, `guest getter trap should receive no extra args, got ${seenExtraArgs}`);
    assert(guestFnMeta(guestGetter)?.abi === "leading-tenant-nt", "guest getter's registered ABI must not be clobbered by narrowing/invocation");
}{
    const t = new MultiTenant();
    const src = t.make(null);
    const dst = t.make(null);
    driveSync(t, t.set(src, "alpha", 1));
    driveSync(t, t.set(src, "beta", 2));
    const keys = driveSync(t, t.ownKeys(src));
    assert(new Set(keys).size === 2 && keys.every((k)=>k === "alpha" || k === "beta"), `expected ownKeys to report original string keys, got ${JSON.stringify(keys)}`);
    driveSync(t, t.assign(dst, src));
    assert(driveSync(t, t.get(dst, "alpha")) === 1 && driveSync(t, t.get(dst, "beta")) === 2, "assign() should copy string-keyed properties");
}console.log("PASS: multi_tenant trap e2e (getter/setter invocation, ownKeys string keys)");
