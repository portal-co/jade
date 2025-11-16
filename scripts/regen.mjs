import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { writeFileSync } from "node:fs";

import { opcodes,handlers } from "../packages/jade-data/dist/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

const vmcode = [{ async: true, gen: true }, { async: true }, { gen: true }, {}]
  .map((o) => {
    const ak = "async" in o;
    const gk = "gen" in o;
    const n = ({ ak: ak_ = ak, gk: gk_ = gk }) =>
      `runVirtualized${ak_ ? "A" : ""}${gk_ ? "G" : ""}`;
    const si = `[code,state,{ip:ip-2,globalThis,nt,tenant},...args]`;
    return `
export ${ak ? "async" : ""} function${gk ? "*" : ""} runVirtualized${
      ak ? "A" : ""
    }${
      gk ? "G" : ""
    }(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): ${
      ak ? (gk ? `AsyncGenerator<any,any,any>` : `Promise<any>`) : `any`
    }{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || ${ak ? "op === 1" : "false"} || ${
      gk ? "op === 2 || op === 3 " : "false"
    }) ? arg() : undefined;
        switch(op){
            
            case ${opcodes.AWAIT.id}: ${
      ak
        ? `state[code().getUint32(ip,true)]=await val;ip += 4;break;`
        : `return apply(${n({
            ak: true,
          })},this,${si});`
    }
            case ${opcodes.YIELD.id}: ${
      gk
        ? `state[code().getUint32(ip,true)]=yield val;ip += 4;break;`
        : `return apply(${n({
            gk: true,
          })},this,${si});`
    }
            case ${opcodes.YIELDSTAR.id}: ${
      gk
        ? `state[code().getUint32(ip,true)]=yield* val;ip += 4;break;`
        : `return apply(${n({
            gk: true,
          })},this,${si});`
    }
    ${Reflect.ownKeys(opcodes)
      .filter((op) => op !== "AWAIT" && op !== "YIELD" && op !== "YIELDSTAR")
      .map((op) => `case ${opcodes[op].id}: ${handlers[op]}`)
      .join("")}
        }
    }
}`;
  })
  .join("\n");
writeFileSync(
  `${__dirname}/../packages/jade-js/vm.ts`,
  `
/* This is GENERATED code by \`update.mjs\` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
${vmcode}`
);
writeFileSync(
  `${__dirname}/../crates/jade-vm/src/data.rs`,
  `
/* This is GENERATED code by \`update.mjs\` */
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, IntoPrimitive, TryFromPrimitive,
)]
#[repr(u16)]
#[non_exhaustive]
pub enum Opcode {
    ${[...Object.entries(opcodes)]
      .map(([a, b]) => `${a}=${b.id}`)
      .join(",\n    ")}
}
impl Opcode{
  pub const LEN: u16 = ${Object.entries(opcodes).length};
}
`
);
