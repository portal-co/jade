import { createNativeHostAsyncCapability } from "./async-host.ts";
import { promisePrimordial } from "./primordials/promise.ts";
import { vm, MultiTenant } from "./index.ts";
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
    litobjEmpty(keyOperand: number) {
        this.u16(9);
        this.u32(0);
        this.u32(keyOperand);
    }
    set(obj: number, key: number, val: number, dest: number) {
        this.u16(24);
        this.u32(obj);
        this.u32(key);
        this.u32(val);
        this.u32(dest);
    }
    get(obj: number, key: number, dest: number) {
        this.u16(23);
        this.u32(obj);
        this.u32(key);
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
function assert(cond: unknown, msg: string) {
    if (!cond) throw new Error("FAIL: " + msg);
}
function runtime(tenant: MultiTenant) {
    return tenant.driveTenant(promisePrimordial(tenant, hostAsync), false, false).promiseRuntime;
}
const hostAsync = createNativeHostAsyncCapability();
function buildProgram(retOperand: number): () => DataView {
    const b = new Buf();
    b.lit32(0, 42);
    b.lit32(1, 7);
    b.litobjEmpty(LIT(2));
    b.set(REF(2), REF(1), REF(0), 3);
    b.get(REF(2), REF(1), 4);
    b.ret(retOperand);
    return b.view();
}
{
    const tenant = new MultiTenant();
    const out = vm.runVirtualized(buildProgram(REF(4)), {}, {
        tenant,
        promiseRuntime: runtime(tenant)
    });
    assert(out === 42, `expected GET to return 42, got ${out}`);
}{
    const tenant = new MultiTenant();
    const obj = vm.runVirtualized(buildProgram(REF(2)), {}, {
        tenant,
        promiseRuntime: runtime(tenant)
    }) as object;
    assert(typeof obj === "object" && obj !== null, "expected an object");
    assert(Object.keys(obj).length === 0, "host should see NO own keys (foreign by nature)");
    assert(!(7 in (obj as any)), "host should not see the tenant property natively");
    assert(tenant.driveTenant(tenant.get(obj, 7), false, false) === 42, "tenant.get must return the stored value");
}console.log("PASS: tenant object-manager e2e (LITOBJ/SET/GET through interpreter)");
