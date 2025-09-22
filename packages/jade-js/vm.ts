
/* This is GENERATED code by `update.mjs` */

const {apply} = Reflect;
const {create,defineProperties} = Object;
const {fromCodePoint} = String;

export async function* runVirtualizedAG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): AsyncGenerator<any,any,any>{
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
                    return apply(val,this,[code,defineProperties(create(null),o),{ip:j,globalThis},...args]);
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
                let l=code().getInt32(ip,true),obj:any=l > 0? {} : {
                    ...state[l=-l,arg()]
                };ip+=4;
                while(l--){
                    obj[arg()]=arg();
                }
                state[code().getUint32(ip,true)]=obj;
                ip+=4;
                break;
            }
        }
    }
}

export async function runVirtualizedA(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): Promise<any>{
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
            case 2: return runVirtualizedAG(code,state,{ip:ip-2,globalThis},...args);
            case 3: return runVirtualizedAG(code,state,{ip:ip-2,globalThis},...args);
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
                    return apply(val,this,[code,defineProperties(create(null),o),{ip:j,globalThis},...args]);
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
                let l=code().getInt32(ip,true),obj:any=l > 0? {} : {
                    ...state[l=-l,arg()]
                };ip+=4;
                while(l--){
                    obj[arg()]=arg();
                }
                state[code().getUint32(ip,true)]=obj;
                ip+=4;
                break;
            }
        }
    }
}

export  function* runVirtualizedG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): any{
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
            case 1: return runVirtualizedAG(code,state,{ip:ip-2,globalThis},...args);
            case 2: state[code().getUint32(ip,true)]=yield val;ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* val;ip += 4;break;
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
                    return apply(val,this,[code,defineProperties(create(null),o),{ip:j,globalThis},...args]);
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
                let l=code().getInt32(ip,true),obj:any=l > 0? {} : {
                    ...state[l=-l,arg()]
                };ip+=4;
                while(l--){
                    obj[arg()]=arg();
                }
                state[code().getUint32(ip,true)]=obj;
                ip+=4;
                break;
            }
        }
    }
}

export  function runVirtualized(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): any{
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
            case 1: return runVirtualizedA(code,state,{ip:ip-2,globalThis},...args);
            case 2: return runVirtualizedG(code,state,{ip:ip-2,globalThis},...args);
            case 3: return runVirtualizedG(code,state,{ip:ip-2,globalThis},...args);
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
                    return apply(val,this,[code,defineProperties(create(null),o),{ip:j,globalThis},...args]);
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
                let l=code().getInt32(ip,true),obj:any=l > 0? {} : {
                    ...state[l=-l,arg()]
                };ip+=4;
                while(l--){
                    obj[arg()]=arg();
                }
                state[code().getUint32(ip,true)]=obj;
                ip+=4;
                break;
            }
        }
    }
}