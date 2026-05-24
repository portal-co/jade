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
  BOOL:       freeze({ id: 12, args: "bool"     }),  // [raw val (0/1)][raw dest]
  EQ:         freeze({ id: 13, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  NE:         freeze({ id: 14, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  LT:         freeze({ id: 15, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  LE:         freeze({ id: 16, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  GT:         freeze({ id: 17, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  GE:         freeze({ id: 18, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  SEL:        freeze({ id: 19, args: "sel"      }),  // [LSB cond][LSB then][LSB else_][raw dest]
  FIXPOINT:   freeze({ id: 20, args: "fixpoint_block" }),  // [raw body_len][body bytes...]
  IF:         freeze({ id: 21, args: "if_block" }),  // [LSB cond][raw then_len][raw else_len][then...][else...]
  SWITCH:     freeze({ id: 22, args: "switch_block" }), // [LSB val][raw n][n×(raw case_val, raw len, body...)][raw default_len][default...]
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
  | "CALL"
  | "BOOL"
  | "EQ"
  | "NE"
  | "LT"
  | "LE"
  | "GT"
  | "GE"
  | "SEL"
  | "FIXPOINT"
  | "IF"
  | "SWITCH";
export type OpcodeInfo = {
  id: number;
  args: "src" | "src_dest" | "dest" | "fn" | "lit32" | "array" | "object" | "call"
      | "bool" | "binop" | "sel" | "fixpoint_block" | "if_block" | "switch_block";
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
  BOOL: `state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;`,
  EQ:   `{ const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }`,
  NE:   `{ const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }`,
  LT:   `{ const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }`,
  LE:   `{ const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }`,
  GT:   `{ const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }`,
  GE:   `{ const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }`,
  SEL:  `{ const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }`,
  FIXPOINT: `{
                const len=code().getUint32(ip,true);ip+=4;
                execBlock(code,state,ip,len,globalThis,nt,tenant);
                ip+=len;
                break;
            }`,
  IF:   `{
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                execBlock(code,state,cond?ip:ip+tl,cond?tl:el,globalThis,nt,tenant);
                ip+=tl+el;
                break;
            }`,
  SWITCH: `{
                const sv=arg();
                let n=code().getUint32(ip,true);ip+=4;
                let matched=false,skip=0;
                const base=ip;
                // scan table to find lengths and matching case
                let scanIp=base;
                let matchStart=-1,matchLen=0;
                while(n--){
                    const cv=code().getUint32(scanIp,true);
                    const cl=code().getUint32(scanIp+4,true);
                    scanIp+=8;
                    if(!matched&&cv===sv){matched=true;matchStart=scanIp;matchLen=cl;}
                    scanIp+=cl;
                }
                const dl=code().getUint32(scanIp,true);scanIp+=4;
                if(matched){execBlock(code,state,matchStart,matchLen,globalThis,nt,tenant);}
                else{execBlock(code,state,scanIp,dl,globalThis,nt,tenant);}
                ip=scanIp+dl;
                break;
            }`,
});
