
/* This is GENERATED code by `update.mjs` */

const {apply} = Reflect;
const {create,defineProperties} = Object;

export async function* runVirtualizedAG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): AsyncGenerator<any,any,any>{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return state[val];
        }
        switch(op){
            case 0: return arg()
            case 1: state[code().getUint32(ip,true)]=await arg();ip += 4;break;
            case 2: state[code().getUint32(ip,true)]=yield arg();ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* arg();ip += 4;break;
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: 
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
    }
}

export async function runVirtualizedA(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): Promise<any>{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return state[val];
        }
        switch(op){
            case 0: return arg()
            case 1: state[code().getUint32(ip,true)]=await arg();ip += 4;break;
            case 2: return runVirtualizedAG(code,state,{ip:ip-2,globalThis},...args);
            case 3: return runVirtualizedAG(code,state,{ip:ip-2,globalThis},...args);
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: 
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
    }
}

export  function* runVirtualizedG(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): any{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return state[val];
        }
        switch(op){
            case 0: return arg()
            case 1: return runVirtualizedAG(code,state,{ip:ip-2,globalThis},...args);
            case 2: state[code().getUint32(ip,true)]=yield arg();ip += 4;break;
            case 3: state[code().getUint32(ip,true)]=yield* arg();ip += 4;break;
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: 
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
    }
}

export  function runVirtualized(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this')}:{ip?:number,globalThis?: typeof window},...args: any[]): any{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return state[val];
        }
        switch(op){
            case 0: return arg()
            case 1: return runVirtualizedA(code,state,{ip:ip-2,globalThis},...args);
            case 2: return runVirtualizedG(code,state,{ip:ip-2,globalThis},...args);
            case 3: return runVirtualizedG(code,state,{ip:ip-2,globalThis},...args);
            case 4: state[code().getUint32(ip,true)]=globalThis;ip += 4;break;
            case 5: 
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
    }
}