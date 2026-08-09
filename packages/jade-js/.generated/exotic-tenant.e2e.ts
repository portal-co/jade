import { MergedTenant, MultiTenant, single_tenant } from "./index.ts";
function assert(condition: unknown, message: string): asserts condition {
    if (!condition) throw new Error(`FAIL: ${message}`);
}
function drive<T>(tenant: MultiTenant | typeof single_tenant | MergedTenant, gen: Generator<any, T, any>): T {
    return tenant.driveTenant(gen, false, false) as T;
}
{
    const tenant = new MultiTenant();
    let value = 0;
    const exotic = drive(tenant, tenant.makeExotic(null, {
        *get (_receiver, key) {
            return key === "x" ? value : undefined;
        },
        *set (_receiver, key, next) {
            if (key === "x") value = next as number;
        },
        *has (_receiver, key) {
            return key === "x";
        },
        *delete () {
            value = -1;
        },
        *ownKeys () {
            return [
                "x"
            ];
        },
        *define () {},
        *assign () {}
    }));
    drive(tenant, tenant.set(exotic, "x", 7));
    assert(drive(tenant, tenant.get(exotic, "x")) === 7, "exotic get/set must dispatch to handler");
    assert(Object.keys(exotic).length === 0, "exotic metadata must remain off the native shell");
    const missing = drive(tenant, tenant.makeExotic(null, {}));
    let failed = false;
    try {
        drive(tenant, tenant.get(missing, "x"));
    } catch (error) {
        failed = error instanceof TypeError;
    }
    assert(failed, "missing exotic trap must fail closed");
}{
    const tenant = new MultiTenant();
    let applyThis: unknown;
    let applied = 0;
    let constructed: Function | undefined;
    const fn = drive(tenant, tenant.makeCallableExotic(null, {
        *apply (_receiver, thisArg, args) {
            applyThis = thisArg;
            applied = args[0] as number;
            return applied + 1;
        },
        *construct (_receiver, newTarget, args) {
            constructed = newTarget;
            return {
                value: args[0]
            };
        },
        *get () {},
        *set () {},
        *has () {
            return false;
        },
        *delete () {},
        *ownKeys () {
            return [];
        },
        *define () {},
        *assign () {}
    }));
    assert(drive(tenant, tenant.invoke(fn, {
        kind: "apply",
        thisArg: "receiver",
        args: [
            4
        ]
    })) === 5, "callable exotic apply result");
    assert(applyThis === "receiver" && applied === 4, "apply receiver and arguments preserved");
    const instance = drive(tenant, tenant.invoke(fn, {
        kind: "construct",
        newTarget: fn,
        args: [
            8
        ]
    })) as {
        value: number;
    };
    assert(instance.value === 8 && constructed === fn, "construct must preserve newTarget");
}{
    const primary = new MultiTenant();
    const secondary = new MultiTenant();
    const foreign = drive(secondary, secondary.make(null));
    drive(secondary, secondary.set(foreign, "answer", 42));
    const merged = new MergedTenant({
        primary,
        providers: [
            primary,
            secondary
        ]
    });
    const holder = drive(merged, merged.make(null));
    drive(merged, merged.set(holder, "foreign", foreign));
    const seen = drive(merged, merged.get(holder, "foreign")) as object;
    assert(seen !== foreign, "cross-provider object must be represented by a bridge exotic");
    assert(drive(merged, merged.get(seen, "answer")) === 42, "bridge forwards reads to secondary owner");
}console.log("PASS: exotic objects, callable ABI, and merged tenant routing");
