import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { writeFileSync } from "node:fs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const vmcode = [{ async: true, gen: true }, { async: true }, { gen: true }, {}]
  .map((o) => {
    const ak = "async" in o;
    const gk = "gen" in o;
    const n = ({ ak: ak_ = ak, gk: gk_ = gk }) =>
      `runVirtualized${ak_ ? "A" : ""}${gk_ ? "G" : ""}`;
    return `
export ${ak ? "async" : ""} function${gk ? "*" : ""} runVirtualized${
      ak ? "A" : ""
    }${
      gk ? "G" : ""
    }(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined}:{ip?:number,globalThis?: typeof window,nt?: [any] |undefined},...args: any[]): ${
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
            case 0: return val
            case 1: ${
              ak
                ? `state[code().getUint32(ip,true)]=await val;ip += 4;break;`
                : `return ${n({
                    ak: true,
                  })}(code,state,{ip:ip-2,globalThis,nt},...args);`
            }
            case 2: ${
              gk
                ? `state[code().getUint32(ip,true)]=yield val;ip += 4;break;`
                : `return ${n({
                    gk: true,
                  })}(code,state,{ip:ip-2,globalThis,nt},...args);`
            }
            case 3: ${
              gk
                ? `state[code().getUint32(ip,true)]=yield* val;ip += 4;break;`
                : `return ${n({
                    gk: true,
                  })}(code,state,{ip:ip-2,globalThis,nt},...args);`
            }
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: {
                const val = [runVirtualized,runVirtualizedA,runVirtualizedG,runVirtualizedAG][arg()&3],closureArgs:number[]=[...arg()],[spanner,...spans]=arg();
                const j = code().getUint32(ip,true);
                ip+=4;
                state[code().getUint32(ip,true)]=spanner(function(this: any,...args: any[]): any{
                    const o=create(null);
                    for(const a in closureArgs)o[closureArgs[a]]={
                        get:()=>state[closureArgs[a]],
                        set:(v:any)=>state[closureArgs[a]]=v,
                        enumerable:true,
                        configurable:false
                    };
                    const s=create(null);
                    return apply(val,this,[
                        code,
                        (defineProperties(s,o),s),
                        {
                            ip:j,
                            globalThis,
                            nt: new.target === undefined ? undefined : [new.target]
                        },
                        ...args
                    ]);
                },...spans);
                ip += 4;
                break;
            }
            case 6: state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;
            case 7: {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }
            case 8: {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }
            case 9: {
                let c=code().getInt32(ip,true),obj:any=(ip+=4,(c >= 0 ? {} : {
                    ...(c=-c,arg())
                }));
                while(c--){
                    obj[arg()]=arg();
                }
                const key = code().getUint32(ip,true);
                if(key & 1){
                    defineProperties(state[key >>> 1],obj);
                }else{
                    state[key >>> 1]=obj;
                }
                ip+=4;
                break;
            }
        }
    }
}`;
  })
  .join("\n");
writeFileSync(
  `${__dirname}/packages/jade-js/vm.ts`,
  `
/* This is GENERATED code by \`update.mjs\` */

const {apply} = Reflect;
const {create,defineProperties} = Object;
const {fromCodePoint} = String;
${vmcode}`
);
