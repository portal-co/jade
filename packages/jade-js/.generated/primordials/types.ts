import type { Tenant, TenantCallableExoticHandler, TenantGenerator, TenantPropertyDescriptor } from "../tenants/types.ts";
export type PrimordialFactory<T> = (tenant: Tenant) => TenantGenerator<T>;
export function* readGuestDescriptor(tenant: Tenant, source: object): TenantGenerator<TenantPropertyDescriptor> {
    const out: TenantPropertyDescriptor = {};
    for (const key of [
        "value",
        "writable",
        "get",
        "set",
        "enumerable",
        "configurable"
    ] as const){
        if (yield tenant.yieldTenant(tenant.has(source, key))) {
            (out as Record<string, unknown>)[key] = yield tenant.yieldTenant(tenant.get(source, key));
        }
    }
    return out;
}
export function* guestArrayLike(tenant: Tenant, source: object): TenantGenerator<unknown[]> {
    const keys = yield tenant.yieldTenant(tenant.ownPropertyKeys(source));
    const numeric = keys.filter((key): key is string =>typeof key === "string" && /^(0|[1-9][0-9]*)$/.test(key)).sort((a, b)=>Number(a) - Number(b));
    const out: unknown[] = [];
    for (const key of numeric)out.push(yield tenant.yieldTenant(tenant.get(source, key)));
    return out;
}
export function* descriptorObject(tenant: Tenant, descriptor: TenantPropertyDescriptor): TenantGenerator<object> {
    const out = yield tenant.yieldTenant(tenant.make(null));
    for (const key of [
        "value",
        "writable",
        "get",
        "set",
        "enumerable",
        "configurable"
    ] as const){
        if (key in descriptor) yield tenant.yieldTenant(tenant.set(out, key, descriptor[key]));
    }
    return out;
}
export function* defineData(tenant: Tenant, target: object, key: PropertyKey, value: unknown, attributes: Partial<TenantPropertyDescriptor> = {}): TenantGenerator<void> {
    const ok = yield tenant.yieldTenant(tenant.defineProperty(target, key, {
        value,
        writable: attributes.writable ?? true,
        enumerable: attributes.enumerable ?? false,
        configurable: attributes.configurable ?? true
    }));
    if (!ok) throw new TypeError(`cannot define primordial property ${String(key)}`);
}
export function* makeBuiltin(tenant: Tenant, name: string, apply: (thisArg: unknown, args: readonly unknown[]) => TenantGenerator<unknown>, construct?: (newTarget: Function, args: readonly unknown[]) => TenantGenerator<object>, proto: object | null = null): TenantGenerator<Function> {
    const descriptors = new Map<PropertyKey, TenantPropertyDescriptor>();
    let prototype = proto;
    let extensible = true;
    let shell: Function;
    const handler: TenantCallableExoticHandler = {
        *get (_receiver, key) {
            const descriptor = descriptors.get(key);
            if (!descriptor) return undefined;
            if (descriptor.get) return yield tenant.yieldTenant(tenant.invokeTrap(descriptor.get, shell, []));
            return descriptor.value;
        },
        *set (_receiver, key, value) {
            const descriptor = descriptors.get(key);
            if (descriptor && descriptor.set) {
                yield tenant.yieldTenant(tenant.invokeTrap(descriptor.set, shell, [
                    value
                ]));
            } else if ((!descriptor || descriptor.writable !== false) && (descriptor || extensible)) {
                descriptors.set(key, {
                    value,
                    writable: true,
                    enumerable: true,
                    configurable: true
                });
            }
        },
        *has (_receiver, key) {
            return descriptors.has(key);
        },
        *delete (_receiver, key) {
            if (descriptors.get(key)?.configurable !== false) descriptors.delete(key);
        },
        *ownKeys () {
            return [
                ...descriptors
            ].filter(([, descriptor])=>descriptor.enumerable).map(([key])=>key);
        },
        *ownPropertyKeys () {
            return [
                ...descriptors.keys()
            ];
        },
        *getOwnPropertyDescriptor (_receiver, key) {
            const descriptor = descriptors.get(key);
            return descriptor && {
                ...descriptor
            };
        },
        *defineProperty (_receiver, key, descriptor) {
            const current = descriptors.get(key);
            if (!current && !extensible) return false;
            if (current?.configurable === false && descriptor.configurable === true) return false;
            descriptors.set(key, {
                ...descriptor
            });
            return true;
        },
        *getPrototypeOf () {
            return prototype;
        },
        *setPrototypeOf (_receiver, next) {
            if (!extensible && prototype !== next) return false;
            prototype = next;
            return true;
        },
        *isExtensible () {
            return extensible;
        },
        *preventExtensions () {
            extensible = false;
            return true;
        },
        *define (_receiver, source) {
            for (const key of yield tenant.yieldTenant(tenant.ownKeys(source))){
                const descriptorSource = yield tenant.yieldTenant(tenant.get(source, key)) as object;
                yield tenant.yieldTenant(this.defineProperty!(shell, key, yield tenant.yieldTenant(readGuestDescriptor(tenant, descriptorSource))));
            }
        },
        *assign (_receiver, source) {
            for (const key of yield tenant.yieldTenant(tenant.ownKeys(source))){
                yield tenant.yieldTenant(this.set!(shell, key, yield tenant.yieldTenant(tenant.get(source, key))));
            }
        },
        *apply (_receiver, thisArg, args) {
            return yield tenant.yieldTenant(apply(thisArg, args));
        },
        *construct (_receiver, newTarget, args) {
            if (!construct) throw new TypeError(`${name} is not a constructor`);
            return yield tenant.yieldTenant(construct(newTarget, args));
        }
    };
    shell = yield tenant.yieldTenant(tenant.makeCallableExotic(proto, handler));
    yield tenant.yieldTenant(defineData(tenant, shell, "name", name, {
        writable: false,
        configurable: true
    }));
    return shell;
}
export function assertObject(value: unknown, message = "expected an object"): asserts value is object {
    if (value === null || (typeof value !== "object" && typeof value !== "function")) throw new TypeError(message);
}
export function toIndex(value: unknown): number {
    const number = Number(value);
    if (!Number.isInteger(number) || number < 0) throw new RangeError("expected a non-negative integer");
    return number;
}
