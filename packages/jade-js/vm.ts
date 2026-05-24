
/* This is GENERATED code by `regen.ts` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties,freeze} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
const unshift = Array.prototype.unshift.call.bind(Array.prototype.unshift);

function execBlock(code: () => DataView, state: {[a: number]: any}, start: number, len: number, globalThis: _globalThis, nt: any, tenant: Tenant): void {
    let ip = start;
    const end = start + len;
    while(ip < end){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        switch(op){
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;            case 5:  {
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
            }            case 6: state[code().getUint32(ip,true)]=code().getUint32(ip+4,true);ip+=8;break;            case 7:  {
                let l=code().getUint32(ip,true),arr:any[]=[];ip+=4;
                while(l--)arr=[...arr,arg()];
                state[code().getUint32(ip,true)]=arr;
                ip+=4;
                break;
            }            case 8:  {
                let l=code().getUint32(ip,true),arr:number[]=[];ip+=4;
                while(l--){
                    arr=[...arr,arg()];
                }
                state[code().getUint32(ip,true)]=fromCodePoint(...arr);
                ip+=4;
                break;
            }            case 9: {
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
            }            case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;            case 11: {
                const fn = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }            case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;            case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }            case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }            case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }            case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }            case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }            case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }            case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }            case 20: {
                const len=code().getUint32(ip,true);ip+=4;
                execBlock(code,state,ip,len,globalThis,nt,tenant);
                ip+=len;
                break;
            }            case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                execBlock(code,state,cond?ip:ip+tl,cond?tl:el,globalThis,nt,tenant);
                ip+=tl+el;
                break;
            }            case 22: {
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
            }
            default: break;
        }
    }
}

export async function* runVirtualizedAG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): AsyncGenerator<any,any,any>{
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
                const fn = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                const len=code().getUint32(ip,true);ip+=4;
                execBlock(code,state,ip,len,globalThis,nt,tenant);
                ip+=len;
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                execBlock(code,state,cond?ip:ip+tl,cond?tl:el,globalThis,nt,tenant);
                ip+=tl+el;
                break;
            }case 22: {
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
            }
        }
    }
}

export async function runVirtualizedA(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): Promise<any>{
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
                const fn = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                const len=code().getUint32(ip,true);ip+=4;
                execBlock(code,state,ip,len,globalThis,nt,tenant);
                ip+=len;
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                execBlock(code,state,cond?ip:ip+tl,cond?tl:el,globalThis,nt,tenant);
                ip+=tl+el;
                break;
            }case 22: {
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
            }
        }
    }
}

export  function* runVirtualizedG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): any{
    for(;;){
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
                const fn = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                const len=code().getUint32(ip,true);ip+=4;
                execBlock(code,state,ip,len,globalThis,nt,tenant);
                ip+=len;
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                execBlock(code,state,cond?ip:ip+tl,cond?tl:el,globalThis,nt,tenant);
                ip+=tl+el;
                break;
            }case 22: {
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
            }
        }
    }
}

export  function runVirtualized(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): any{
    for(;;){
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
            }case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;case 11: {
                const fn = arg();
                let n = code().getUint32(ip,true); ip += 4;
                const callArgs: any[] = [];
                while(n--) callArgs.push(arg());
                state[code().getUint32(ip,true)] = apply(fn, undefined, callArgs);
                ip += 4;
                break;
            }case 12: state[code().getUint32(ip+4,true)]=!!code().getUint32(ip,true);ip+=8;break;case 13: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a===b; ip+=4; break; }case 14: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a!==b; ip+=4; break; }case 15: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<b;   ip+=4; break; }case 16: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a<=b;  ip+=4; break; }case 17: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>b;   ip+=4; break; }case 18: { const a=arg(),b=arg(); state[code().getUint32(ip,true)]=a>=b;  ip+=4; break; }case 19: { const c=arg(),t=arg(),e=arg(); state[code().getUint32(ip,true)]=c?t:e; ip+=4; break; }case 20: {
                const len=code().getUint32(ip,true);ip+=4;
                execBlock(code,state,ip,len,globalThis,nt,tenant);
                ip+=len;
                break;
            }case 21: {
                const cond=arg();
                const tl=code().getUint32(ip,true),el=code().getUint32(ip+4,true);ip+=8;
                execBlock(code,state,cond?ip:ip+tl,cond?tl:el,globalThis,nt,tenant);
                ip+=tl+el;
                break;
            }case 22: {
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
            }
        }
    }
}