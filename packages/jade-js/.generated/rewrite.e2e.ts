import { MultiTenant, single_tenant, guestFnMeta, hostToGuest, guestToHost, type NarrowSpec } from "./index.ts";
function assert(cond: unknown, msg: string) {
    if (!cond) throw new Error("FAIL: " + msg);
}
function driveSync<T>(t: MultiTenant, gen: Generator<any, T, any>): T {
    return t.driveTenant(gen, false, false) as T;
}
{
    const t = new MultiTenant();
    const spec: NarrowSpec<{
        a: number;
        b: string;
    }> = {
        kind: "object",
        fields: {
            a: {
                kind: "typeof",
                tag: "number"
            },
            b: {
                kind: "typeof",
                tag: "string"
            }
        }
    };
    const guestObj = hostToGuest<{
        a: number;
        b: string;
    }>(spec, {
        a: 1,
        b: "x"
    }, t) as object;
    assert(driveSync(t, t.get(guestObj, "a")) === 1, "hostToGuest should convert `a` into the tenant-managed object");
    assert(driveSync(t, t.get(guestObj, "b")) === "x", "hostToGuest should convert `b` into the tenant-managed object");
    assert(Object.keys(guestObj).length === 0, "the guest object should be foreign by nature (no own keys visible)");
}{
    const t = new MultiTenant();
    const spec: NarrowSpec<{
        a: number;
    }> = {
        kind: "object",
        fields: {
            a: {
                kind: "typeof",
                tag: "number"
            }
        }
    };
    const guestObj = driveSync(t, t.make(null));
    driveSync(t, t.set(guestObj, "a", 42));
    const hostObj = guestToHost<{
        a: number;
    }>(spec, guestObj, t);
    assert(hostObj.a === 42, "guestToHost should read the tenant-managed field back into a plain host object");
}{
    const t = new MultiTenant();
    let seenTenant: unknown;
    const guestGetter = function(tenantArg: unknown, _nt: unknown, ...rest: unknown[]) {
        seenTenant = tenantArg;
        return "guest-value";
    };
    t.markGuestFn(guestGetter, {
        abi: "leading-tenant-nt"
    });
    const spec: NarrowSpec<() => string> = {
        kind: "guestFn"
    };
    const adapted = guestToHost<() => string>(spec, guestGetter, t);
    assert(typeof adapted === "function", "guestToHost should return a plain-callable function");
    assert(adapted() === "guest-value", "the adapted function should invoke the guest function with the correct ABI");
    assert(seenTenant === t, "the guest function should receive the tenant as its leading param, via invokeGuestAware");
    assert(guestFnMeta(adapted)?.abi === "closure", "the adapter should be re-marked with the closure ABI");
}for (const t of [
    new MultiTenant(),
    single_tenant
]){
    assert(typeof t.markGuestFn === "function", "markGuestFn should be injected onto the tenant");
    assert(typeof t.invokeGuestAware === "function", "invokeGuestAware should be injected onto the tenant");
    assert(typeof t.invokeTrap === "function", "invokeTrap should be injected onto the tenant");
    assert(typeof t.createGuestGen === "function", "createGuestGen should be injected onto the tenant");
    assert(typeof t.unpackGuestGen === "function", "unpackGuestGen should be injected onto the tenant");
    assert(typeof t.yieldTenant === "function", "yieldTenant should be injected onto the tenant");
    assert(typeof t.driveTenant === "function", "driveTenant should be injected onto the tenant");
}
{
    const t = new MultiTenant();
    function* nativeGen() {
        yield 1;
        yield 2;
        return 3;
    }
    const guestGen = driveSync(t, t.createGuestGen(nativeGen()));
    const values: unknown[] = [];
    for (const v of t.unpackGuestGen(guestGen))values.push(v);
    assert(JSON.stringify(values) === JSON.stringify([
        1,
        2
    ]), `expected [1,2], got ${JSON.stringify(values)}`);
}console.log("PASS: rewrite.ts (hostToGuest/guestToHost) + injected ABI/shim methods e2e");
