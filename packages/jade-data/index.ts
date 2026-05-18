const {freeze} = Object;
export const opcodes: { [Op in Opcode]: OpcodeInfo } = freeze({
  RET:        freeze({ id: 0,  args: "src"      }),  // [LSB val]
  AWAIT:      freeze({ id: 1,  args: "src_dest" }),  // [LSB val] [raw dest]
  YIELD:      freeze({ id: 2,  args: "src_dest" }),  // [LSB val] [raw dest]
  YIELDSTAR:  freeze({ id: 3,  args: "src_dest" }),  // [LSB val] [raw dest]
  GLOBAL:     freeze({ id: 4,  args: "dest"     }),  // [raw dest]
  FN:         freeze({ id: 5,  args: "fn"       }),  // [LSB variant][LSB closure_args][LSB spanner][raw j][raw dest]
  LIT32:      freeze({ id: 6,  args: "lit32"    }),  // [raw dest][raw val]
  ARR:        freeze({ id: 7,  args: "array"    }),  // [raw len][LSB items…][raw dest]
  STR:        freeze({ id: 8,  args: "array"    }),  // [raw len][LSB items…][raw dest]
  LITOBJ:     freeze({ id: 9,  args: "object"   }),  // [i32 c][spread?][pairs…][LSB key]
  NEW_TARGET: freeze({ id: 10, args: "dest"     }),  // [raw dest]
  CALL:       freeze({ id: 11, args: "call"     }),  // [LSB fn][raw n][LSB args…][raw dest]
});
export type Opcode =
  | "RET"
  | "AWAIT"
  | "YIELD"
  | "YIELDSTAR"
  | "GLOBAL"
  | "FN"
  | "LIT32"
  | "ARR"
  | "STR"
  | "LITOBJ"
  | "NEW_TARGET"
  | "CALL";
export type OpcodeInfo = {
  id: number;
  args: "src" | "src_dest" | "dest" | "fn" | "lit32" | "array" | "object" | "call";
};
export type Handler = string;
export const handlers: { [Op in Opcode]?: Handler} = freeze({
  RET: `return val;`,
  GLOBAL: `state[code().getUint32(ip,true)]=globalThis;ip += 4;break;`,
  FN: ` {
                const val = [runVirtualized,runVirtualizedA,runVirtualizedG,runVirtualizedAG][arg()&3]
                    ,closureArgs:number[]=[...arg()]
                    ,[spanner,...spans]=arg()??[(a:any)=>a];
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
                        freeze({
                            __proto__: null,
                            ip:j,
                            globalThis,
                            nt: new.target,
                            tenant
                        }),
                        ...args
                    ]);
                },...spans);
                ip += 4;
                break;
            }`,
  LIT32: `state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;`,
  ARR: ` {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }`,
  STR: ` {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }`,
  LITOBJ: `{
                let c=code().getInt32(ip,true),obj:any=(ip+=4,(c >= 0 ? {__proto__: null} : {
                    __proto__: null,
                    ...(c=-c,arg())
                }));
                while(c--){
                    obj[tenant.clean(obj,arg())]=arg();
                }
                const key = code().getUint32(ip,true);
                if(key & 1){
                    defineProperties(state[key >>> 1],obj);
                }else{
                    state[key >>> 1]=obj;
                }
                ip+=4;
                break;
            }`,
  NEW_TARGET: `state[code().getUint32(ip,true)]=nt;ip += 4;break;`,
  CALL: `{
                const fn = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }`,
});
