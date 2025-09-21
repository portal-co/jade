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
    }(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): ${
      ak ? (gk ? `AsyncGenerator<any,any,any>` : `Promise<any>`) : `any`
    }{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return state[val];
        }
        switch(op){
            case 0: return arg()
            case 1: ${
              ak
                ? `state[code().getUint32(ip,true)]=await arg();ip += 4;break;`
                : `return ${n({
                    ak: true,
                  })}(code,state,{ip:ip-2,globalThis},...args);`
            }
            case 2: ${
              gk
                ? `state[code().getUint32(ip,true)]=yield arg();ip += 4;break;`
                : `return ${n({
                    gk: true,
                  })}(code,state,{ip:ip-2,globalThis},...args);`
            }
            case 3: ${
              gk
                ? `state[code().getUint32(ip,true)]=yield* arg();ip += 4;break;`
                : `return ${n({
                    gk: true,
                  })}(code,state,{ip:ip-2,globalThis},...args);`
            }
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: 
            const val = arg(),closureArgs:number[]=[...arg()],[spanner,...spans]=arg();
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
                return apply(val,this,[code,defineProperties(create(null),o),{ip:j,globalThis},...args]);
            },...spans);
            ip += 4;
            break;
        }
    }
}`;
  })
  .join("\n");
writeFileSync(
  `${__dirname}/packages/jade-js/vm.ts`,
  `
  const {apply} = Reflect;
  const {create,defineProperties} = Object;
  ${vmcode}`
);
