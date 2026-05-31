
/* This is GENERATED code by `regen.ts` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties,freeze} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
const unshift = Array.prototype.unshift.call.bind(Array.prototype.unshift);
// Sentinel returned by a VM variant when it reaches its block end-bound (as
// opposed to a RET, which returns the actual value to propagate to the caller).
const BLOCK_DONE: unique symbol = Symbol("BLOCK_DONE");

export async function* runVirtualizedAG(code: () => DataView, state: {[a: number]: any},{ip=0,end=undefined,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,end?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): AsyncGenerator<any,any,any>{
    for(;;){
        if(end!==undefined && ip>=end) return BLOCK_DONE;
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || op === 1 || op === 2 || op === 3 ) ? arg() : undefined;
        switch(op){

            case 1: state[code().getUint32(ip,true)]=await val;ip += 4;break;
            case 2: state[code().getUint32(ip,true)]=yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* val;ip += 4;break;
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                // [LSB cond][raw body_len][body...][LSB next]
                const condRaw=code().getUint32(ip,true);ip+=4;
                const len=code().getUint32(ip,true);ip+=4;
                const body=ip;ip+=len;
                const nextRaw=code().getUint32(ip,true);ip+=4;
                const evalRaw=(x:number)=> x&1 ? state[x>>>1] : x>>>1;
                let c=evalRaw(condRaw);
                while(c){
                    const r=yield* runVirtualizedAG(code,state,{ip:body,end:body+len,globalThis,nt,tenant});
                    if(r!==BLOCK_DONE) return r;
                    c=evalRaw(nextRaw);
                }
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                const start=cond?ip:ip+tl,len=cond?tl:el;
                const r=yield* runVirtualizedAG(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip+=tl+el;
                break;
            }case 22: {
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
                const r=yield* runVirtualizedAG(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip=scanIp+dl;
                break;
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}

export async function runVirtualizedA(code: () => DataView, state: {[a: number]: any},{ip=0,end=undefined,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,end?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): Promise<any>{
    for(;;){
        if(end!==undefined && ip>=end) return BLOCK_DONE;
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || op === 1 || false) ? arg() : undefined;
        switch(op){

            case 1: state[code().getUint32(ip,true)]=await val;ip += 4;break;
            case 2: return apply(runVirtualizedAG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args));
            case 3: return apply(runVirtualizedAG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args));
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                // [LSB cond][raw body_len][body...][LSB next]
                const condRaw=code().getUint32(ip,true);ip+=4;
                const len=code().getUint32(ip,true);ip+=4;
                const body=ip;ip+=len;
                const nextRaw=code().getUint32(ip,true);ip+=4;
                const evalRaw=(x:number)=> x&1 ? state[x>>>1] : x>>>1;
                let c=evalRaw(condRaw);
                while(c){
                    const r=await runVirtualizedA(code,state,{ip:body,end:body+len,globalThis,nt,tenant});
                    if(r!==BLOCK_DONE) return r;
                    c=evalRaw(nextRaw);
                }
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                const start=cond?ip:ip+tl,len=cond?tl:el;
                const r=await runVirtualizedA(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip+=tl+el;
                break;
            }case 22: {
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
                const r=await runVirtualizedA(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip=scanIp+dl;
                break;
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}

export  function* runVirtualizedG(code: () => DataView, state: {[a: number]: any},{ip=0,end=undefined,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,end?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): any{
    for(;;){
        if(end!==undefined && ip>=end) return BLOCK_DONE;
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || false || op === 2 || op === 3 ) ? arg() : undefined;
        switch(op){

            case 1: return apply(runVirtualizedAG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args));
            case 2: state[code().getUint32(ip,true)]=yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* val;ip += 4;break;
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                // [LSB cond][raw body_len][body...][LSB next]
                const condRaw=code().getUint32(ip,true);ip+=4;
                const len=code().getUint32(ip,true);ip+=4;
                const body=ip;ip+=len;
                const nextRaw=code().getUint32(ip,true);ip+=4;
                const evalRaw=(x:number)=> x&1 ? state[x>>>1] : x>>>1;
                let c=evalRaw(condRaw);
                while(c){
                    const r=yield* runVirtualizedG(code,state,{ip:body,end:body+len,globalThis,nt,tenant});
                    if(r!==BLOCK_DONE) return r;
                    c=evalRaw(nextRaw);
                }
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                const start=cond?ip:ip+tl,len=cond?tl:el;
                const r=yield* runVirtualizedG(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip+=tl+el;
                break;
            }case 22: {
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
                const r=yield* runVirtualizedG(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip=scanIp+dl;
                break;
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}

export  function runVirtualized(code: () => DataView, state: {[a: number]: any},{ip=0,end=undefined,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,end?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): any{
    for(;;){
        if(end!==undefined && ip>=end) return BLOCK_DONE;
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || false || false) ? arg() : undefined;
        switch(op){

            case 1: return apply(runVirtualizedA,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args));
            case 2: return apply(runVirtualizedG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args));
            case 3: return apply(runVirtualizedG,this,(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args));
    case 0: return val;case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;case 5:  {
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
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                // [LSB cond][raw body_len][body...][LSB next]
                const condRaw=code().getUint32(ip,true);ip+=4;
                const len=code().getUint32(ip,true);ip+=4;
                const body=ip;ip+=len;
                const nextRaw=code().getUint32(ip,true);ip+=4;
                const evalRaw=(x:number)=> x&1 ? state[x>>>1] : x>>>1;
                let c=evalRaw(condRaw);
                while(c){
                    const r=runVirtualized(code,state,{ip:body,end:body+len,globalThis,nt,tenant});
                    if(r!==BLOCK_DONE) return r;
                    c=evalRaw(nextRaw);
                }
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                const start=cond?ip:ip+tl,len=cond?tl:el;
                const r=runVirtualized(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip+=tl+el;
                break;
            }case 22: {
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
                const r=runVirtualized(code,state,{ip:start,end:start+len,globalThis,nt,tenant});
                if(r!==BLOCK_DONE) return r;
                ip=scanIp+dl;
                break;
            }case 23: { const o=arg(),k=arg(); state[code().getUint32(ip,true)]=tenant.get(o,k); ip+=4; break; }case 24: { const o=arg(),k=arg(),v=arg(); tenant.set(o,k,v); state[code().getUint32(ip,true)]=v; ip+=4; break; }
        }
    }
}