type VMOpt = { async?: boolean; gen?: boolean };

export function genVmTs(opcodes: Record<string, any>, handlers: Record<string, string>): string {
  const vmcode = [{ async: true, gen: true }, { async: true }, { gen: true }, {}]
    .map((o: VMOpt) => {
      const isAsync = "async" in o;
      const isGenerator = "gen" in o;
      const functionName = ({ isAsync: ak_ = isAsync, isGenerator: gk_ = isGenerator }: { isAsync?: boolean; isGenerator?: boolean }) =>
        `runVirtualized${ak_ ? "A" : ""}${gk_ ? "G" : ""}`;
      const publicName = functionName({});
      // The host-facing async runner returns a nominal HostTask. Its private
      // implementation may use native async syntax only after it has entered
      // the explicit host capability boundary.
      const implementationName = isAsync && !isGenerator ? `_${publicName}` : publicName;
      const self = implementationName;
      const drive = isGenerator ? "yield* " : isAsync ? "await " : "";
      const subst = (s: string) => s.replaceAll("__DRIVE__", drive).replaceAll("__SELF__", self);
      const parameters = `(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,promiseRuntime,addAsync,addGen,doubleGen,handlers:__handlers})),unshift(args,state),unshift(args,code),args)`;
      const context = `{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant,promiseRuntime,addAsync=false,addGen=false,doubleGen=false,handlers=undefined}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant,promiseRuntime:PromiseRuntime,addAsync?:boolean,addGen?:boolean,doubleGen?:boolean,handlers?: [number,number][]}`;
      const signature = `(code: () => DataView, state: {[a: number]: any},${context},...args: any[])`;
      const result = isAsync ? (isGenerator ? `AsyncGenerator<any,any,any>` : `HostTask<any>`) : `any`;
      // TRYPUSH/TRYPOP maintain a per-frame exception-handler stack (one entry
      // per live region: [catch_slot, handler_ip]); the loop's catch dispatches a
      // raised value to the innermost handler, binding it into state[catch_slot]
      // and resuming at handler_ip. An empty stack rethrows, propagating to the
      // guest caller's frame (its own loop dispatches against its own stack).
      const body = subst(`
    const __handlers: [number, number][] = handlers ?? [];
    for(;;){
        try {
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || ${isAsync ? "op === 1" : "false"} || ${isGenerator ? "op === 2 || op === 3 " : "false"}) ? arg() : undefined;
        switch(op){
            case ${opcodes.AWAIT.id}: ${isAsync
              ? `state[code().getUint32(ip,true)]=await promiseRuntime.awaitHostTask(promiseRuntime.awaitGuest(val));ip += 4;break;`
              : `return apply(${functionName({ isAsync: true })},this,${parameters});`}
            case ${opcodes.YIELD.id}: ${isGenerator
              ? `state[code().getUint32(ip,true)]=doubleGen ? yield {value:val,[THROUGH]:true} : yield val;ip += 4;break;`
              : `return apply(${functionName({ isGenerator: true })},this,${parameters});`}
            case ${opcodes.YIELDSTAR.id}: ${isGenerator
              ? `state[code().getUint32(ip,true)]=yield* (doubleGen ? unpackGuestGen(val) : val);ip += 4;break;`
              : `return apply(${functionName({ isGenerator: true })},this,${parameters});`}
    ${Object.keys(opcodes).filter((op) => op !== "AWAIT" && op !== "YIELD" && op !== "YIELDSTAR").map((op) => `case ${opcodes[op].id}: ${handlers[op]}`).join("")}
        }
        } catch (__e) {
            const __h = __handlers.pop();
            if (__h === undefined) throw __e;
            state[__h[0]] = __e;
            ip = __h[1];
        }
    }`);
      if (isAsync && !isGenerator) {
        return subst(`
async function ${implementationName}${signature}: Promise<any>{${body}}
export function ${publicName}${signature}: HostTask<any>{
  return hostTaskFromPromise(promiseRuntime.async, ${implementationName}(code,state,{ip,globalThis,nt,tenant,promiseRuntime,addAsync,addGen,doubleGen},...args));
}`);
      }
      return subst(`
export ${isAsync ? "async" : ""} function${isGenerator ? "*" : ""} ${publicName}${signature}: ${result}{${body}}`);
    }).join("\n");

  return `
/* This is GENERATED code by \`regen.ts\` */
import {type Tenant} from "./tenants/types.ts";
import {THROUGH, unpackGuestGen} from "./tenants/shims.ts";
import {markGuestFn} from "./tenants/narrow.ts";
import {hostTaskFromPromise,type HostTask} from "./async-host.ts";
import type {PromiseRuntime} from "./primordials/promise.ts";
export {THROUGH} from "./tenants/shims.ts";
const {apply} = Reflect;
const {create,defineProperties,freeze} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
const unshift = Array.prototype.unshift.call.bind(Array.prototype.unshift);
${vmcode}`;
}