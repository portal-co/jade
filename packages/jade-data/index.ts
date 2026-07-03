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
  WHILE:      freeze({ id: 20, args: "while_block" }),  // [LSB cond][raw body_len][body bytes...][LSB next]
  IF:         freeze({ id: 21, args: "if_block" }),  // [LSB cond][raw then_len][raw else_len][then...][else...]
  SWITCH:     freeze({ id: 22, args: "switch_block" }), // [LSB val][raw n][n×(raw case_val, raw len, body...)][raw default_len][default...]
  GET:        freeze({ id: 23, args: "member_get"  }),  // [LSB obj][LSB key][raw dest]
  SET:        freeze({ id: 24, args: "member_set"  }),  // [LSB obj][LSB key][LSB val][raw dest]
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
  | "WHILE"
  | "IF"
  | "SWITCH"
  | "GET"
  | "SET";
export type OpcodeInfo = {
  id: number;
  args: "src" | "src_dest" | "dest" | "fn" | "lit32" | "array" | "object" | "call"
      | "bool" | "binop" | "sel" | "while_block" | "if_block" | "switch_block"
      | "member_get" | "member_set";
};
export type Handler = string;
export const handlers: { [Op in Opcode]?: Handler} = freeze({
  RET: `return val;`,
  GLOBAL: `state[code().getUint32(ip,true)]=globalThis;ip += 4;break;`,
  FN: ` {
                const declaredVariant = arg()&3;
                const effectiveVariant = declaredVariant | (addAsync ? 1 : 0) | (addGen ? 2 : 0);
                const childDoubleGen = addGen && !!(declaredVariant & 2);
                const val = [runVirtualized,runVirtualizedA,runVirtualizedG,runVirtualizedAG][effectiveVariant]
                    ,closureArgs:number[]=[...arg()]
                    ,[spanner,...spans]=arg()??[(a:any)=>a];
                const j = code().getUint32(ip,true);
                ip+=4;
                state[code().getUint32(ip,true)]=markGuestFn(spanner(function(this: any,...args: any[]): any{
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
                            tenant,
                            addAsync,
                            addGen,
                            doubleGen: childDoubleGen,
                        }),
                        ...args
                    ]);
                },...spans),{abi:"closure"});
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
                let c=code().getInt32(ip,true);ip+=4;
                const obj=tenant.make(null);
                if(c<0){c=-c;tenant.assign(obj,arg());}
                while(c--){
                    const k=arg();
                    tenant.set(obj,k,arg());
                }
                const key = code().getUint32(ip,true);
                if(key & 1){
                    tenant.define(state[key >>> 1],obj);
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
                const _callRaw = apply(fn, undefined, callArgs);
                state[code().getUint32(ip,true)] = (addGen && _callRaw && typeof _callRaw.next === 'function')
                    ? createGuestGen(_callRaw, tenant)
                    : _callRaw;
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
  WHILE: `{
                // [LSB cond][raw body_len][body...][LSB next]
                const condRaw=code().getUint32(ip,true);ip+=4;
                const len=code().getUint32(ip,true);ip+=4;
                const body=ip;ip+=len;
                const nextRaw=code().getUint32(ip,true);ip+=4;
                const evalRaw=(x:number)=> x&1 ? state[x>>>1] : x>>>1;
                let c=evalRaw(condRaw);
                while(c){
                    const r=__DRIVE____SELF__(code,state,{ip:body,end:body+len,globalThis,nt,tenant,addAsync,addGen,doubleGen});
                    if(r!==BLOCK_DONE) return r;
                    c=evalRaw(nextRaw);
                }
                break;
            }`,
  IF:   `{
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                const start=cond?ip:ip+tl,len=cond?tl:el;
                const r=__DRIVE____SELF__(code,state,{ip:start,end:start+len,globalThis,nt,tenant,addAsync,addGen,doubleGen});
                if(r!==BLOCK_DONE) return r;
                ip+=tl+el;
                break;
            }`,
  SWITCH: `{
                const sv=arg();
                let n=code().getUint32(ip,true);ip+=4;
                // scan table to find lengths and matching case
                let scanIp=ip;
                let matched=false,matchStart=-1,matchLen=0;
                while(n--){
                    const cv=code().getUint32(scanIp,true);
                    const cl=code().getUint32(scanIp+4,true);
                    scanIp+=8;
                    if(!matched&&cv===sv){matched=true;matchStart=scanIp;matchLen=cl;}
                    scanIp+=cl;
                }
                const dl=code().getUint32(scanIp,true);scanIp+=4;
                const start=matched?matchStart:scanIp,len=matched?matchLen:dl;
                const r=__DRIVE____SELF__(code,state,{ip:start,end:start+len,globalThis,nt,tenant,addAsync,addGen,doubleGen});
                if(r!==BLOCK_DONE) return r;
                ip=scanIp+dl;
                break;
            }`,
  GET:  `{ const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }`,
  SET:  `{ const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }`,
});
