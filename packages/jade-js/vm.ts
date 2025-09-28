
/* This is GENERATED code by `update.mjs` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties} = Object;
const {fromCodePoint} = String;

export async function* runVirtualizedAG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: typeof window,nt?: any,tenant:Tenant},...args: any[]): AsyncGenerator<any,any,any>{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || op === 1 || op === 2 || op === 3 ) ? arg() : undefined;
        switch(op){
            case 0: return val
            case 1: state[code().getUint32(ip,true)]=await val;ip += 4;break;
            case 2: state[code().getUint32(ip,true)]=yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* val;ip += 4;break;
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: {
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
                        {
                            ip:j,
                            globalThis,
                            nt: new.target,
                            tenant
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
            }
            case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;
        }
    }
}

export async function runVirtualizedA(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: typeof window,nt?: any,tenant:Tenant},...args: any[]): Promise<any>{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || op === 1 || false) ? arg() : undefined;
        switch(op){
            case 0: return val
            case 1: state[code().getUint32(ip,true)]=await val;ip += 4;break;
            case 2: return apply(runVirtualizedAG,this,[code,state,{ip:ip-2,globalThis,nt,tenant},...args]);
            case 3: return apply(runVirtualizedAG,this,[code,state,{ip:ip-2,globalThis,nt,tenant},...args]);
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: {
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
                        {
                            ip:j,
                            globalThis,
                            nt: new.target,
                            tenant
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
            }
            case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;
        }
    }
}

export  function* runVirtualizedG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: typeof window,nt?: any,tenant:Tenant},...args: any[]): any{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || false || op === 2 || op === 3 ) ? arg() : undefined;
        switch(op){
            case 0: return val
            case 1: return apply(runVirtualizedAG,this,[code,state,{ip:ip-2,globalThis,nt,tenant},...args]);
            case 2: state[code().getUint32(ip,true)]=yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* val;ip += 4;break;
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: {
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
                        {
                            ip:j,
                            globalThis,
                            nt: new.target,
                            tenant
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
            }
            case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;
        }
    }
}

export  function runVirtualized(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: typeof window,nt?: any,tenant:Tenant},...args: any[]): any{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || false || false) ? arg() : undefined;
        switch(op){
            case 0: return val
            case 1: return apply(runVirtualizedA,this,[code,state,{ip:ip-2,globalThis,nt,tenant},...args]);
            case 2: return apply(runVirtualizedG,this,[code,state,{ip:ip-2,globalThis,nt,tenant},...args]);
            case 3: return apply(runVirtualizedG,this,[code,state,{ip:ip-2,globalThis,nt,tenant},...args]);
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: {
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
                        {
                            ip:j,
                            globalThis,
                            nt: new.target,
                            tenant
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
            }
            case 10: state[code().getUint32(ip,true)]=nt;ip += 4;break;
        }
    }
}