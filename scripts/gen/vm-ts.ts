type VMOpt = { async?: boolean; gen?: boolean };

export function genVmTs(opcodes: Record<string, any>, handlers: Record<string, string>): string {
  const vmcode = [{ async: true, gen: true }, { async: true }, { gen: true }, {}]
    .map((o: VMOpt) => {
      const isAsync = "async" in o;
      const isGenerator = "gen" in o;
      const functionName = ({ isAsync: ak_ = isAsync, isGenerator: gk_ = isGenerator }: { isAsync?: boolean; isGenerator?: boolean }) =>
        `runVirtualized${ak_ ? "A" : ""}${gk_ ? "G" : ""}`;
      const self = functionName({});
      // How a block body re-enters this variant: generators delegate with
      // `yield*`, async functions await, sync functions call directly.
      const drive = isGenerator ? "yield* " : isAsync ? "await " : "";
      // Substitute the block-handler placeholders for this variant.
      const subst = (s: string) => s.replaceAll("__DRIVE__", drive).replaceAll("__SELF__", self);
      const parameters = `(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args)`;
      return subst(`
export ${isAsync ? "async" : ""} function${isGenerator ? "*" : ""} runVirtualized${
        isAsync ? "A" : ""
      }${
        isGenerator ? "G" : ""
      }(code: () => DataView, state: {[a: number]: any},{ip=0,end=undefined,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,end?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): ${
        isAsync ? (isGenerator ? `AsyncGenerator<any,any,any>` : `Promise<any>`) : `any`
      }{
    for(;;){
        if(end!==undefined && ip>=end) return BLOCK_DONE;
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || ${isAsync ? "op === 1" : "false"} || ${
        isGenerator ? "op === 2 || op === 3 " : "false"
      }) ? arg() : undefined;
        switch(op){

            case ${opcodes.AWAIT.id}: ${
        isAsync
          ? `state[code().getUint32(ip,true)]=await val;ip += 4;break;`
          : `return apply(${functionName({
              isAsync: true,
            })},this,${parameters});`
      }
            case ${opcodes.YIELD.id}: ${
        isGenerator
          ? `state[code().getUint32(ip,true)]=yield val;ip += 4;break;`
          : `return apply(${functionName({
              isGenerator: true,
            })},this,${parameters});`
      }
            case ${opcodes.YIELDSTAR.id}: ${
        isGenerator
          ? `state[code().getUint32(ip,true)]=yield* val;ip += 4;break;`
          : `return apply(${functionName({
              isGenerator: true,
            })},this,${parameters});`
      }
    ${Reflect.ownKeys(opcodes)
        .filter((op) => op !== "AWAIT" && op !== "YIELD" && op !== "YIELDSTAR")
        .map((op) => `case ${opcodes[op].id}: ${handlers[op]}`)
        .join("")}
        }
    }
}`);
    })
    .join("\n");

  return `
/* This is GENERATED code by \`regen.ts\` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties,freeze} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
const unshift = Array.prototype.unshift.call.bind(Array.prototype.unshift);
// Sentinel returned by a VM variant when it reaches its block end-bound (as
// opposed to a RET, which returns the actual value to propagate to the caller).
const BLOCK_DONE: unique symbol = Symbol("BLOCK_DONE");
${vmcode}`;
}
