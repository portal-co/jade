const {freeze} = Object;
export const opcodes: { [Op in Opcode]: OpcodeInfo } = freeze({
  RET:        freeze({ id: 0,  args: "src"      }),  // [LSB val]
  AWAIT:      freeze({ id: 1,  args: "src_dest" }),  // [LSB val] [raw dest]
  YIELD:      freeze({ id: 2,  args: "src_dest" }),  // [LSB val] [raw dest]
  YIELDSTAR:  freeze({ id: 3,  args: "src_dest" }),  // [LSB val] [raw dest]
  GLOBAL:     freeze({ id: 4,  args: "dest"     }),  // [raw dest]
  FN:         freeze({ id: 5,  args: "fn"       }),  // [LSB variant][LSB closure_args][LSB spanner][LSB params][raw j][raw dest]
  LIT32:      freeze({ id: 6,  args: "lit32"    }),  // [raw dest][raw val]
  ARR:        freeze({ id: 7,  args: "array"    }),  // [raw len][LSB items…][raw dest]
  STR:        freeze({ id: 8,  args: "array"    }),  // [raw len][LSB items…][raw dest]
  LITOBJ:     freeze({ id: 9,  args: "object"   }),  // [i32 c][spread?][pairs…][LSB key]
  NEW_TARGET: freeze({ id: 10, args: "dest"     }),  // [raw dest]
  CALL:       freeze({ id: 11, args: "call"     }),  // [LSB fn][LSB this][raw n][LSB args…][raw dest]
  BOOL:       freeze({ id: 12, args: "bool"     }),  // [raw val (0/1)][raw dest]
  EQ:         freeze({ id: 13, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  NE:         freeze({ id: 14, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  LT:         freeze({ id: 15, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  LE:         freeze({ id: 16, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  GT:         freeze({ id: 17, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  GE:         freeze({ id: 18, args: "binop"    }),  // [LSB a][LSB b][raw dest]
  SEL:        freeze({ id: 19, args: "sel"      }),  // [LSB cond][LSB then][LSB else_][raw dest]
  JMP:        freeze({ id: 20, args: "jmp"      }),  // [raw target]
  COND_JMP:    freeze({ id: 21, args: "condjmp"  }),  // [LSB cond][raw if_true][raw if_false]
  SWITCH:     freeze({ id: 22, args: "switch_jump" }), // [LSB val][raw n][n×(raw case_val, raw target)][raw default_target]
  GET:        freeze({ id: 23, args: "member_get"  }),  // [LSB obj][LSB key][raw dest]
  SET:        freeze({ id: 24, args: "member_set"  }),  // [LSB obj][LSB key][LSB val][raw dest]
  // Exception regions (docs/exceptions-plan.md): THROW raises, TRYPUSH/TRYPOP
  // delimit a protected region whose handler starts at handler_ip and binds the
  // exception into state slot catch_slot. All three are loop-level.
  THROW:      freeze({ id: 25, args: "throw_src"   }),  // [LSB val]
  TRYPUSH:    freeze({ id: 26, args: "trypush"    }),  // [raw catch_slot][raw handler_ip]
  TRYPOP:     freeze({ id: 27, args: "none"       }),  // (no operands)
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
  | "JMP"
  | "COND_JMP"
  | "SWITCH"
  | "GET"
  | "SET"
  | "THROW"
  | "TRYPUSH"
  | "TRYPOP";
export type OpcodeInfo = {
  id: number;
  args: "src" | "src_dest" | "dest" | "fn" | "lit32" | "array" | "object" | "call"
      | "bool" | "binop" | "sel" | "jmp" | "condjmp" | "switch_jump"
      | "member_get" | "member_set" | "throw_src" | "trypush" | "none";
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
                    // closure_args/spanner are Operand::Literal(0) when unwired
                    // (the only encoding the frontend emits today — see
                    // docs/closure-capture-plan.md); a StateRef to an unwritten slot
                    // also reads as undefined. Both mean "absent": tolerate falsy.
                    ,closureArgs:number[]=[...(arg()||[])]
                    ,[spanner,...spans]=(arg()||[(a:any)=>a])
                    // params is Operand::Literal(0) for a parameterless function,
                    // else a StateRef to an array of the nested function's own state
                    // slot ids (one per declared parameter, in argument order).
                    ,paramSlots:number[]=[...(arg()||[])];
                const j = code().getUint32(ip,true);
                ip+=4;
                state[code().getUint32(ip,true)]=__DRIVE__tenant.driveTenant(tenant.makeFunction(markGuestFn(spanner(function(this: any,...args: any[]): any{
                    const o=create(null);
                    for(const a in closureArgs)o[closureArgs[a]]={
                        get:()=>state[closureArgs[a]],
                        set:(v:any)=>state[closureArgs[a]]=v,
                        enumerable:true,
                        configurable:false
                    };
                    const s=create(null);
                    // Bind call arguments into the child's parameter slots; a missing
                    // argument writes undefined (same as an unwritten slot), extras are
                    // dropped — ordinary JS parameter semantics.
                    for(let i=0;i<paramSlots.length;i++)s[paramSlots[i]]=args[i];
                    return apply(val,this,[
                        code,
                        (defineProperties(s,o),s),
                        freeze({
                            __proto__: null,
                            ip:j,
                            globalThis,
                            // The nested frame runs tenant ops through the *same*
                            // tenant — omitting this leaves tenant undefined in the
                            // child and every GET/SET/CALL inside a nested function
                            // crashes with "Cannot read properties of undefined".
                            tenant,
                            nt: new.target,
                            promiseRuntime,
                            addAsync,
                            addGen,
                            doubleGen: childDoubleGen,
                        }),
                        ...args
                    ]);
                },...spans),{abi:"closure"})),addAsync,addGen);
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
                const obj=__DRIVE__tenant.driveTenant(tenant.make(null),addAsync,addGen);
                if(c<0){c=-c;__DRIVE__tenant.driveTenant(tenant.assign(obj,arg()),addAsync,addGen);}
                while(c--){
                    const k=arg();
                    __DRIVE__tenant.driveTenant(tenant.set(obj,k,arg()),addAsync,addGen);
                }
                const key = code().getUint32(ip,true);
                if(key & 1){
                    __DRIVE__tenant.driveTenant(tenant.define(state[key >>> 1],obj),addAsync,addGen);
                }else{
                    state[key >>> 1]=obj;
                }
                ip+=4;
                break;
            }`,
  NEW_TARGET: `state[code().getUint32(ip,true)]=nt;ip += 4;break;`,
  CALL: `{
                const fn = arg();
                const thisArg = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = __DRIVE__tenant.driveTenant(
                    tenant.invoke(fn, {kind:"apply", thisArg, args:callArgs}),
                    addAsync,
                    addGen,
                );
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
  JMP: `ip=code().getUint32(ip,true);break;`,
  COND_JMP: `{
                const cond=arg();
                const ifTrue=code().getUint32(ip,true),ifFalse=code().getUint32(ip+4,true);
                ip=cond?ifTrue:ifFalse;
                break;
            }`,
  SWITCH: `{
                const sv=arg();
                const n=code().getUint32(ip,true);ip+=4;
                let target=-1;
                for(let i=0;i<n;i++){
                    const cv=code().getUint32(ip,true);ip+=4;
                    const tgt=code().getUint32(ip,true);ip+=4;
                    if(target<0&&cv===sv)target=tgt;
                }
                const defaultTarget=code().getUint32(ip,true);ip+=4;
                ip=target>=0?target:defaultTarget;
                break;
            }`,
  GET:  `{ const o=arg(),k=arg(); state[code().getUint32(ip,true)]=__DRIVE__tenant.driveTenant(tenant.get(o,k),addAsync,addGen); ip+=4; break; }`,
  SET:  `{ const o=arg(),k=arg(),v=arg(); __DRIVE__tenant.driveTenant(tenant.set(o,k,v),addAsync,addGen); state[code().getUint32(ip,true)]=v; ip+=4; break; }`,
  // Guest `throw`: raise on the host channel so the driving loop's catch
  // dispatches it into the innermost region (see scripts/gen/vm-ts.ts).
  THROW: `{ throw arg(); }`,
  TRYPUSH: `{ __handlers.push([code().getUint32(ip,true), code().getUint32(ip+4,true)]); ip+=8; break; }`,
  TRYPOP: `{ __handlers.pop(); break; }`,
});
