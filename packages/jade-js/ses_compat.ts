import { protoChained } from "@portal-solutions/semble-common";
export const isSES = "harden" in globalThis;
export function shadowChained<
  T extends object,
  X extends keyof T,
  U,
  Args extends unknown[]
>(
  f: <T2 extends { [X2 in X]: any }>(t: T2, key: X, ...args: Args) => U,
  shadow: WeakMap<any, any>,
  {
    Reflect = {
      getPrototypeOf: Object?.getPrototypeOf,
      getOwnPropertyDescriptor: Object?.getOwnPropertyDescriptor,
    },
    freeze = false,
  }: {
    Reflect?: {
      getPrototypeOf: typeof globalThis.Reflect.getPrototypeOf;
      getOwnPropertyDescriptor: typeof globalThis.Reflect.getOwnPropertyDescriptor;
    };
    freeze?: boolean;
  } = {}
): (val: T, key: X, ...args: Args) => U {
  return (val, key, ...args) => {
    let vals = [val];
    for (;;) {
      [val, ...vals] = vals;
      if (freeze && Object.isFrozen(val)) continue;
      if (Reflect.getOwnPropertyDescriptor(val, key)) {
        return f(val, key, ...args);
      }
      vals = [...vals, Reflect.getPrototypeOf(val) as T]; //Simulate tail recursion
      if (shadow.has(val)) vals = [...vals, shadow.get(val) as T];
    }
  };
}
export const shadowMap = new WeakMap();
export const shadowReflect = ({
  Reflect = globalThis.Reflect,
}: { Reflect?: typeof globalThis.Reflect } = {}) => ({
  ...new Proxy(Reflect, {
    get(target, p, receiver) {
      if (
        p === "get" ||
        p === "set" ||
        p === "has" ||
        p === "getOwnPropertyDescriptor"
      )
        return shadowChained((target as any)[p], shadowMap, {
          Reflect,
          freeze: p === "set",
        });
      return (target as any)[p];
    },
  }),
});
