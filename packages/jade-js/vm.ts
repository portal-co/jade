
/* This is GENERATED code by `regen.ts` */
import {type Tenant} from "./index.ts"
import {THROUGH, createGuestGen, unpackGuestGen} from "./shims.ts"
import {markGuestFn} from "./narrow.ts"
export {THROUGH} from "./shims.ts"
const {apply} = Reflect;
const {create,defineProperties,freeze} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
const unshift = Array.prototype.unshift.call.bind(Array.prototype.unshift);

export async function* runVirtualizedAG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant,addAsync=false,addGen=false,doubleGen=false}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant,addAsync?:boolean,addGen?:boolean,doubleGen?:boolean},...args: any[]): AsyncGenerator<any,any,any>{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || op === 1 || op === 2 || op === 3 ) ? arg() : undefined;
        switch(op){

            case 1: state[code().getUint32(ip,true)]=await val;ip += 4;break;
            case 2: state[code().getUint32(ip,true)]=doubleGen ? yield {value:val,[THROUGH]:true} : yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* (doubleGen ? unpackGuestGen(val) : val);ip += 4;break;
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
            }case 6: state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;case 7:  {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }case 8:  {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }case 9: {
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
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
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: ip=code().getUint32(ip,true);break;case 21: {
                const cond=arg();
                const ifTrue=code().getUint32(ip,true),ifFalse=code().getUint32(ip+4,true);
                ip=cond?ifTrue:ifFalse;
                break;
            }case 22: {
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
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}

export async function runVirtualizedA(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant,addAsync=false,addGen=false,doubleGen=false}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant,addAsync?:boolean,addGen?:boolean,doubleGen?:boolean},...args: any[]): Promise<any>{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || op === 1 || false) ? arg() : undefined;
        switch(op){

            case 1: state[code().getUint32(ip,true)]=await val;ip += 4;break;
            case 2: return apply(runVirtualizedAG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,addAsync,addGen,doubleGen})),unshift(args,state),unshift(args,code),args));
            case 3: return apply(runVirtualizedAG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,addAsync,addGen,doubleGen})),unshift(args,state),unshift(args,code),args));
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
            }case 6: state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;case 7:  {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }case 8:  {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }case 9: {
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
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
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: ip=code().getUint32(ip,true);break;case 21: {
                const cond=arg();
                const ifTrue=code().getUint32(ip,true),ifFalse=code().getUint32(ip+4,true);
                ip=cond?ifTrue:ifFalse;
                break;
            }case 22: {
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
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}

export  function* runVirtualizedG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant,addAsync=false,addGen=false,doubleGen=false}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant,addAsync?:boolean,addGen?:boolean,doubleGen?:boolean},...args: any[]): any{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || false || op === 2 || op === 3 ) ? arg() : undefined;
        switch(op){

            case 1: return apply(runVirtualizedAG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,addAsync,addGen,doubleGen})),unshift(args,state),unshift(args,code),args));
            case 2: state[code().getUint32(ip,true)]=doubleGen ? yield {value:val,[THROUGH]:true} : yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* (doubleGen ? unpackGuestGen(val) : val);ip += 4;break;
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
            }case 6: state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;case 7:  {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }case 8:  {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }case 9: {
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
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
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: ip=code().getUint32(ip,true);break;case 21: {
                const cond=arg();
                const ifTrue=code().getUint32(ip,true),ifFalse=code().getUint32(ip+4,true);
                ip=cond?ifTrue:ifFalse;
                break;
            }case 22: {
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
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}

export  function runVirtualized(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant,addAsync=false,addGen=false,doubleGen=false}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant,addAsync?:boolean,addGen?:boolean,doubleGen?:boolean},...args: any[]): any{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || false || false) ? arg() : undefined;
        switch(op){

            case 1: return apply(runVirtualizedA,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,addAsync,addGen,doubleGen})),unshift(args,state),unshift(args,code),args));
            case 2: return apply(runVirtualizedG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,addAsync,addGen,doubleGen})),unshift(args,state),unshift(args,code),args));
            case 3: return apply(runVirtualizedG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant,addAsync,addGen,doubleGen})),unshift(args,state),unshift(args,code),args));
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
            }case 6: state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;case 7:  {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }case 8:  {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }case 9: {
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
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
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: ip=code().getUint32(ip,true);break;case 21: {
                const cond=arg();
                const ifTrue=code().getUint32(ip,true),ifFalse=code().getUint32(ip+4,true);
                ip=cond?ifTrue:ifFalse;
                break;
            }case 22: {
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
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}