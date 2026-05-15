// Bytecode interpreter for the Jade VM.
//
// Four public variants mirror the JS vm.ts exports:
//   run_virtualized      - sync
//   run_virtualized_a    - async (awaits JS Promises)
//   run_virtualized_g    - sync generator  (returns a JS iterator object)
//   run_virtualized_ag   - async generator (returns a JS async-iterator object)
//
// Optimisations over the original version:
//
//   StateCache – each interpreter invocation keeps a Vec<Option<JsValue>> that
//   mirrors the numeric-keyed JS state object.  Reads populate the cache on miss;
//   writes go to the cache and mark a dirty list.  The cache is flushed to the JS
//   object only on exit or before a foreign call.  This cuts the number of
//   Reflect::get / Reflect::set bridge crossings to O(distinct-slots) instead of
//   O(accesses).
//
//   FN_REGISTRY WeakMap – when FN creates a JS wrapper function, it stores the
//   jade-specific metadata (variant index, entry ip, closure slot list) in a
//   module-level WeakMap keyed on the wrapper Function.  The CALL opcode checks
//   the WeakMap: if the callee is jade-generated and synchronous, it dispatches
//   directly to run_sync in Rust (no Rust→JS→Rust round-trip, no state flush).
//   Non-sync jade callees and truly foreign functions still go through
//   Reflect::apply after flushing the cache.

#![allow(unused_macros, unused_imports)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::cell::RefCell;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint32Array, WeakMap};
pub use portal_solutions_jade_vm::{Operand, Operation, SignedOperand};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

// ---------------------------------------------------------------------------
// Raw JS bindings
// ---------------------------------------------------------------------------

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Object, js_name = create)]
    fn js_object_create(proto: &JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = Object, js_name = assign)]
    fn js_object_assign(target: &JsValue, source: &JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = Object, js_name = defineProperties)]
    fn js_define_properties(target: &JsValue, props: &JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = Object, js_name = freeze)]
    fn js_object_freeze(obj: &JsValue) -> JsValue;
}

// ---------------------------------------------------------------------------
// Bytecode reading helpers
// ---------------------------------------------------------------------------

#[inline]
fn read_u16_le(buf: &[u8], off: usize) -> Option<(u16, usize)> {
    if off + 2 > buf.len() {
        return None;
    }
    Some((u16::from_le_bytes([buf[off], buf[off + 1]]), off + 2))
}

#[inline]
fn read_u32_le(buf: &[u8], off: usize) -> Option<(u32, usize)> {
    if off + 4 > buf.len() {
        return None;
    }
    Some((
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]),
        off + 4,
    ))
}

#[inline]
fn read_i32_le(buf: &[u8], off: usize) -> Option<(i32, usize)> {
    if off + 4 > buf.len() {
        return None;
    }
    Some((
        i32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]),
        off + 4,
    ))
}

#[inline]
fn js_err(s: &str) -> JsValue {
    JsValue::from_str(s)
}

// ---------------------------------------------------------------------------
// Low-level state helpers (direct Reflect bridge – used only for flush/fill)
// ---------------------------------------------------------------------------

#[inline]
fn s_get(state: &JsValue, idx: u32) -> JsValue {
    Reflect::get(state, &JsValue::from_f64(idx as f64)).unwrap_or(JsValue::UNDEFINED)
}

#[inline]
fn s_set(state: &JsValue, idx: u32, val: JsValue) {
    let _ = Reflect::set(state, &JsValue::from_f64(idx as f64), &val);
}

// ---------------------------------------------------------------------------
// StateCache
//
// Caches numeric-keyed slots of the JS state object.  Reads populate on miss;
// writes go to the Vec and mark a dirty list.  Flush writes dirty slots back.
// ---------------------------------------------------------------------------

struct StateCache {
    /// The underlying JS state object (cheap clone – just a JS reference).
    state: JsValue,
    /// Slot values; index == slot number. None = not yet read from JS.
    slots: Vec<Option<JsValue>>,
    /// Slot indices that have been written and need to be flushed.
    dirty: Vec<u32>,
}

impl StateCache {
    fn new(state: JsValue) -> Self {
        Self { state, slots: Vec::new(), dirty: Vec::new() }
    }

    fn get(&mut self, idx: u32) -> JsValue {
        let i = idx as usize;
        if i < self.slots.len() {
            if let Some(v) = &self.slots[i] {
                return v.clone();
            }
        }
        let v = s_get(&self.state, idx);
        if i >= self.slots.len() {
            self.slots.resize(i + 1, None);
        }
        self.slots[i] = Some(v.clone());
        v
    }

    fn set(&mut self, idx: u32, val: JsValue) {
        let i = idx as usize;
        if i >= self.slots.len() {
            self.slots.resize(i + 1, None);
        }
        self.slots[i] = Some(val);
        self.dirty.push(idx);
    }

    /// Write all dirty slots to the JS state object.
    fn flush(&self) {
        for &slot in &self.dirty {
            if let Some(Some(v)) = self.slots.get(slot as usize) {
                s_set(&self.state, slot, v.clone());
            }
        }
    }

    /// Flush then clear – used before a foreign call that may mutate state.
    fn flush_and_invalidate(&mut self) {
        self.flush();
        self.slots.clear();
        self.dirty.clear();
    }
}

/// Consume one LSB-encoded operand from bytecode.
/// Advances `*ip` by 4.  Literal operands are returned as JS numbers;
/// state-ref operands are resolved through the cache.
#[inline]
fn read_arg(code: &[u8], ip: &mut usize, cache: &mut StateCache) -> Option<JsValue> {
    let (raw, new_off) = read_u32_le(code, *ip)?;
    *ip = new_off;
    Some(if raw & 1 != 0 {
        cache.get(raw >> 1)
    } else {
        JsValue::from_f64((raw >> 1) as f64)
    })
}

// ---------------------------------------------------------------------------
// FN_REGISTRY – WeakMap<Function, {v, j, s: Uint32Array}>
//
// Populated by the FN opcode; consulted by the CALL opcode to decide whether
// the callee can be dispatched directly in Rust.
// ---------------------------------------------------------------------------

thread_local! {
    static FN_REGISTRY: WeakMap = WeakMap::new();
}

/// Store jade-fn metadata in the registry, keyed on the JS wrapper function.
fn registry_set(fn_val: &JsValue, variant: u32, j: u32, closure_slots: &[u32]) {
    FN_REGISTRY.with(|wm| {
        let entry = create_null_obj();
        let _ = Reflect::set(&entry, &JsValue::from_str("v"), &JsValue::from_f64(variant as f64));
        let _ = Reflect::set(&entry, &JsValue::from_str("j"), &JsValue::from_f64(j as f64));
        let arr = Uint32Array::new_with_length(closure_slots.len() as u32);
        for (i, &s) in closure_slots.iter().enumerate() {
            arr.set_index(i as u32, s);
        }
        let _ = Reflect::set(&entry, &JsValue::from_str("s"), &arr);
        wm.set(fn_val.unchecked_ref::<Object>(), &entry);
    });
}

/// Look up jade-fn metadata; returns None if fn_val is not a jade function.
fn registry_get(fn_val: &JsValue) -> Option<(u32, u32, Vec<u32>)> {
    FN_REGISTRY.with(|wm| {
        let entry = wm.get(fn_val.unchecked_ref::<Object>());
        if entry.is_undefined() {
            return None;
        }
        let v = Reflect::get(&entry, &JsValue::from_str("v")).ok()?.as_f64()? as u32;
        let j = Reflect::get(&entry, &JsValue::from_str("j")).ok()?.as_f64()? as u32;
        let s_val = Reflect::get(&entry, &JsValue::from_str("s")).ok()?;
        let s_arr: Uint32Array = s_val.unchecked_into();
        let slots: Vec<u32> = (0..s_arr.length()).map(|i| s_arr.get_index(i)).collect();
        Some((v, j, slots))
    })
}

// ---------------------------------------------------------------------------
// tenant_clean
// ---------------------------------------------------------------------------

fn tenant_clean(tenant: &JsValue, obj: &JsValue, key: JsValue) -> JsValue {
    if tenant.is_falsy() {
        return key;
    }
    match Reflect::get(tenant, &JsValue::from_str("clean")) {
        Ok(f) if f.is_function() => {
            let a = Array::of2(obj, &key);
            Reflect::apply(f.unchecked_ref::<Function>(), tenant, &a).unwrap_or(key)
        }
        _ => key,
    }
}

#[inline]
fn create_null_obj() -> JsValue {
    js_object_create(&JsValue::null())
}

fn make_iter_result(value: JsValue, done: bool) -> JsValue {
    let obj = create_null_obj();
    let _ = Reflect::set(&obj, &JsValue::from_str("value"), &value);
    let _ = Reflect::set(&obj, &JsValue::from_str("done"), &JsValue::from_bool(done));
    obj
}

// ---------------------------------------------------------------------------
// Child-state construction
//
// Two variants:
//   build_child_state       – getter/setter descriptors referencing parent state.
//                             Used by the JS-callable wrapper function (FN opcode).
//   build_child_state_fast  – direct slot copies from the parent cache.
//                             Used by the sync jade-to-jade CALL fast path.
// ---------------------------------------------------------------------------

fn build_child_state(parent: &JsValue, closure_slots: &[u32]) -> JsValue {
    let child = create_null_obj();
    if closure_slots.is_empty() {
        return child;
    }
    let descs = create_null_obj();
    for &slot in closure_slots {
        let slot_key = JsValue::from_f64(slot as f64);
        let desc = create_null_obj();

        let pg = parent.clone();
        let ps = parent.clone();

        let get_fn = Closure::<dyn Fn() -> JsValue>::new(move || s_get(&pg, slot));
        let set_fn = Closure::<dyn Fn(JsValue)>::new(move |v: JsValue| s_set(&ps, slot, v));

        let _ = Reflect::set(&desc, &JsValue::from_str("get"), get_fn.as_ref());
        let _ = Reflect::set(&desc, &JsValue::from_str("set"), set_fn.as_ref());
        let _ = Reflect::set(&desc, &JsValue::from_str("enumerable"), &JsValue::TRUE);
        let _ = Reflect::set(&desc, &JsValue::from_str("configurable"), &JsValue::FALSE);

        get_fn.forget();
        set_fn.forget();

        let _ = Reflect::set(&descs, &slot_key, &desc);
    }
    js_define_properties(&child, &descs);
    child
}

/// Build a child state by copying current values from the parent cache.
/// Used by the sync jade-to-jade CALL fast path – avoids getter/setter bouncing.
fn build_child_state_fast(closure_slots: &[u32], cache: &mut StateCache) -> JsValue {
    let child = create_null_obj();
    for &slot in closure_slots {
        let val = cache.get(slot);
        let _ = Reflect::set(&child, &JsValue::from_f64(slot as f64), &val);
    }
    child
}

// ---------------------------------------------------------------------------
// Generator state machine
// ---------------------------------------------------------------------------

enum StepResult {
    Yielded(JsValue, u32),
    Returned(JsValue),
}

struct GenMachine {
    code: Vec<u8>,
    state: JsValue,
    ip: usize,
    pending_dest: Option<u32>,
    delegating: Option<(JsValue, u32)>,
    done: bool,
    global_this: JsValue,
    nt: JsValue,
    tenant: JsValue,
}

// ---------------------------------------------------------------------------
// Shared opcode body macro
//
// Handles opcodes 4-11. Expects in scope:
//   $op     : u16
//   $code   : &[u8]
//   $cache  : StateCache  (mutable)
//   $ip     : usize       (mutable)
//   $gt     : &JsValue    (globalThis)
//   $nt     : &JsValue    (new.target)
//   $tenant : &JsValue
// ---------------------------------------------------------------------------

macro_rules! common_ops {
    ($op:ident, $code:ident, $cache:ident, $ip:ident, $gt:ident, $nt:ident, $tenant:ident) => {
        match $op {
            // GLOBAL
            4 => {
                let (dest, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("GLOBAL: dest"))?;
                $ip = no;
                $cache.set(dest, $gt.clone());
            }
            // FN
            5 => {
                let variant_val =
                    read_arg($code, &mut $ip, &mut $cache).ok_or_else(|| js_err("FN: variant"))?;
                let closure_args_val =
                    read_arg($code, &mut $ip, &mut $cache).ok_or_else(|| js_err("FN: closureArgs"))?;
                let spanner_val =
                    read_arg($code, &mut $ip, &mut $cache).ok_or_else(|| js_err("FN: spanner"))?;
                let (j_raw, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("FN: j"))?;
                $ip = no;
                let (dest_raw, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("FN: dest"))?;
                $ip = no;

                let variant_idx = (variant_val.as_f64().unwrap_or(0.0) as u32) & 3;
                let j = j_raw as usize;

                let closure_slots: Vec<u32> = {
                    let arr: Array = closure_args_val.unchecked_into();
                    (0..arr.length())
                        .map(|i| arr.get(i).as_f64().unwrap_or(0.0) as u32)
                        .collect()
                };
                // Clone before moving into the closure so registry_set can use it.
                let closure_slots_reg = closure_slots.clone();

                let (spanner, spans): (JsValue, Vec<JsValue>) =
                    if spanner_val.is_null() || spanner_val.is_undefined() {
                        let id = Function::new_with_args("a", "return a");
                        (id.into(), Vec::new())
                    } else {
                        let arr: Array = spanner_val.unchecked_into();
                        let sp = arr.get(0);
                        let extra: Vec<JsValue> = (1..arr.length()).map(|i| arr.get(i)).collect();
                        (sp, extra)
                    };

                let code_c: Vec<u8> = $code.to_vec();
                // Flush before capturing parent_state: the getter/setter closures
                // below reference the JS state object, which must be up-to-date.
                $cache.flush();
                let parent_state = $cache.state.clone();
                let gt_c = $gt.clone();
                let tenant_c = $tenant.clone();

                let inner = Closure::<dyn Fn(JsValue, Array) -> JsValue>::new(
                    move |js_this: JsValue, js_args: Array| {
                        let child = build_child_state(&parent_state, &closure_slots);
                        dispatch_variant(
                            variant_idx,
                            &code_c,
                            &child,
                            j as u32,
                            &gt_c,
                            &js_this,
                            &tenant_c,
                            &js_args,
                        )
                    },
                );

                let inner_js = inner.into_js_value();
                let factory =
                    Function::new_with_args("f", "return function(...a){return f(this,a)}");
                let wrapper = factory.call1(&JsValue::null(), &inner_js).map_err(|e| e)?;

                let spanned = if spans.is_empty() {
                    wrapper
                } else if let Some(sp_fn) = spanner.dyn_ref::<Function>() {
                    let call_args = Array::new();
                    call_args.push(&wrapper);
                    for s in &spans {
                        call_args.push(s);
                    }
                    Reflect::apply(sp_fn, &JsValue::null(), &call_args).unwrap_or(wrapper)
                } else {
                    wrapper
                };

                // Register in the WeakMap so CALL can dispatch directly in Rust.
                registry_set(&spanned, variant_idx, j as u32, &closure_slots_reg);

                $cache.set(dest_raw, spanned);
            }
            // LIT32
            6 => {
                let (dest, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("LIT32: dest"))?;
                $ip = no;
                let (val, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("LIT32: val"))?;
                $ip = no;
                $cache.set(dest, JsValue::from_f64(val as f64));
            }
            // ARR
            7 => {
                let (len, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("ARR: len"))?;
                $ip = no;
                let arr = Array::new_with_length(len);
                for i in 0..len {
                    let item =
                        read_arg($code, &mut $ip, &mut $cache).ok_or_else(|| js_err("ARR: item"))?;
                    arr.set(i, item);
                }
                let (dest, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("ARR: dest"))?;
                $ip = no;
                $cache.set(dest, arr.into());
            }
            // STR
            8 => {
                let (len, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("STR: len"))?;
                $ip = no;
                let mut cps: Vec<char> = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let cp_val =
                        read_arg($code, &mut $ip, &mut $cache).ok_or_else(|| js_err("STR: cp"))?;
                    let cp = cp_val.as_f64().unwrap_or(0.0) as u32;
                    if let Some(ch) = char::from_u32(cp) {
                        cps.push(ch);
                    }
                }
                let (dest, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("STR: dest"))?;
                $ip = no;
                let s: String = cps.into_iter().collect();
                $cache.set(dest, JsValue::from_str(&s));
            }
            // LITOBJ
            9 => {
                let (c_raw, no) = read_i32_le($code, $ip).ok_or_else(|| js_err("LITOBJ: c"))?;
                $ip = no;
                let mut c = c_raw;

                let obj = if c >= 0 {
                    create_null_obj()
                } else {
                    c = -c;
                    let spread = read_arg($code, &mut $ip, &mut $cache)
                        .ok_or_else(|| js_err("LITOBJ: spread"))?;
                    let base = create_null_obj();
                    js_object_assign(&base, &spread);
                    base
                };

                for _ in 0..c.unsigned_abs() {
                    let k = read_arg($code, &mut $ip, &mut $cache)
                        .ok_or_else(|| js_err("LITOBJ: key"))?;
                    let v = read_arg($code, &mut $ip, &mut $cache)
                        .ok_or_else(|| js_err("LITOBJ: val"))?;
                    let ck = tenant_clean($tenant, &obj, k);
                    let _ = Reflect::set(&obj, &ck, &v);
                }

                let (key_raw, no) =
                    read_u32_le($code, $ip).ok_or_else(|| js_err("LITOBJ: dest"))?;
                $ip = no;

                if key_raw & 1 != 0 {
                    let target = $cache.get(key_raw >> 1);
                    js_define_properties(&target, &obj);
                } else {
                    $cache.set(key_raw >> 1, obj);
                }
            }
            // NEW_TARGET
            10 => {
                let (dest, no) =
                    read_u32_le($code, $ip).ok_or_else(|| js_err("NEW_TARGET: dest"))?;
                $ip = no;
                $cache.set(dest, $nt.clone());
            }
            // CALL
            11 => {
                let fn_val = read_arg($code, &mut $ip, &mut $cache)
                    .ok_or_else(|| js_err("CALL: fn"))?;
                let (arg_count, no) =
                    read_u32_le($code, $ip).ok_or_else(|| js_err("CALL: argc"))?;
                $ip = no;
                let call_args = Array::new_with_length(arg_count);
                for i in 0..arg_count {
                    let a = read_arg($code, &mut $ip, &mut $cache)
                        .ok_or_else(|| js_err("CALL: arg"))?;
                    call_args.set(i, a);
                }
                let (dest, no) = read_u32_le($code, $ip).ok_or_else(|| js_err("CALL: dest"))?;
                $ip = no;

                let result = if let Some((variant_idx, j, closure_slots)) = registry_get(&fn_val) {
                    if variant_idx == 0 {
                        // Sync jade-to-jade: dispatch directly in Rust.
                        // Build child state from cache values (no getter/setter bounce).
                        let child = build_child_state_fast(&closure_slots, &mut $cache);
                        let res = run_sync(
                            $code,
                            &child,
                            j as usize,
                            $gt,
                            &JsValue::UNDEFINED,
                            $tenant,
                            &call_args,
                        )
                        .unwrap_or(JsValue::UNDEFINED);
                        // Propagate mutations on shared closure slots back to parent cache.
                        for &slot in &closure_slots {
                            $cache.set(slot, s_get(&child, slot));
                        }
                        res
                    } else {
                        // Non-sync jade callee: flush and call via JS wrapper.
                        $cache.flush_and_invalidate();
                        Reflect::apply(
                            fn_val.unchecked_ref::<Function>(),
                            &JsValue::UNDEFINED,
                            &call_args,
                        )
                        .unwrap_or(JsValue::UNDEFINED)
                    }
                } else {
                    // Foreign function: flush, call, invalidate.
                    $cache.flush_and_invalidate();
                    Reflect::apply(
                        fn_val.unchecked_ref::<Function>(),
                        &JsValue::UNDEFINED,
                        &call_args,
                    )
                    .unwrap_or(JsValue::UNDEFINED)
                };

                $cache.set(dest, result);
            }
            _ => return Err(js_err(&format!("unknown opcode: {}", $op))),
        }
    };
}

fn dispatch_variant(
    variant_idx: u32,
    code: &[u8],
    state: &JsValue,
    ip: u32,
    global_this: &JsValue,
    nt: &JsValue,
    tenant: &JsValue,
    args: &Array,
) -> JsValue {
    match variant_idx {
        0 => run_sync(code, state, ip as usize, global_this, nt, tenant, args)
            .unwrap_or(JsValue::UNDEFINED),
        1 => {
            let code = code.to_vec();
            let state = state.clone();
            let gt = global_this.clone();
            let nt = nt.clone();
            let tenant = tenant.clone();
            let args = args.clone();
            future_to_promise(async move {
                run_async_internal(&code, &state, ip as usize, &gt, &nt, &tenant, &args).await
            })
            .into()
        }
        2 => create_sync_gen(code, state, ip as usize, global_this, nt, tenant)
            .unwrap_or(JsValue::UNDEFINED),
        3 => create_async_gen(code, state, ip as usize, global_this, nt, tenant)
            .unwrap_or(JsValue::UNDEFINED),
        _ => JsValue::UNDEFINED,
    }
}

fn run_sync(
    code: &[u8],
    state: &JsValue,
    start_ip: usize,
    global_this: &JsValue,
    nt: &JsValue,
    tenant: &JsValue,
    _args: &Array,
) -> Result<JsValue, JsValue> {
    let mut ip = start_ip;
    let mut cache = StateCache::new(state.clone());
    loop {
        let (op, no) =
            read_u16_le(code, ip).ok_or_else(|| js_err("run_sync: unexpected end at opcode"))?;
        ip = no;

        match op {
            0 => {
                let val =
                    read_arg(code, &mut ip, &mut cache).ok_or_else(|| js_err("RET: val"))?;
                cache.flush();
                return Ok(val);
            }
            1 => {
                let restart = ip - 2;
                cache.flush();
                let code_c = code.to_vec();
                let state_c = state.clone();
                let gt_c = global_this.clone();
                let nt_c = nt.clone();
                let tenant_c = tenant.clone();
                let promise = future_to_promise(async move {
                    run_async_internal(
                        &code_c,
                        &state_c,
                        restart,
                        &gt_c,
                        &nt_c,
                        &tenant_c,
                        &Array::new(),
                    )
                    .await
                });
                return Ok(JsValue::from(promise));
            }
            2 | 3 => {
                let restart = ip - 2;
                cache.flush();
                return create_sync_gen(code, state, restart, global_this, nt, tenant);
            }
            _ => {
                common_ops!(op, code, cache, ip, global_this, nt, tenant);
            }
        }
    }
}

async fn run_async_internal(
    code: &[u8],
    state: &JsValue,
    start_ip: usize,
    global_this: &JsValue,
    nt: &JsValue,
    tenant: &JsValue,
    _args: &Array,
) -> Result<JsValue, JsValue> {
    let mut ip = start_ip;
    let mut cache = StateCache::new(state.clone());
    loop {
        let (op, no) =
            read_u16_le(code, ip).ok_or_else(|| js_err("run_async: unexpected end at opcode"))?;
        ip = no;

        match op {
            0 => {
                let val =
                    read_arg(code, &mut ip, &mut cache).ok_or_else(|| js_err("RET: val"))?;
                cache.flush();
                return Ok(val);
            }
            1 => {
                let promise_val =
                    read_arg(code, &mut ip, &mut cache).ok_or_else(|| js_err("AWAIT: val"))?;
                let (dest, no) = read_u32_le(code, ip).ok_or_else(|| js_err("AWAIT: dest"))?;
                ip = no;
                // Flush before suspending so the JS state object is coherent.
                cache.flush_and_invalidate();
                let resolved = JsFuture::from(Promise::resolve(&promise_val)).await?;
                cache.set(dest, resolved);
            }
            2 | 3 => {
                let restart = ip - 2;
                cache.flush();
                let ag = create_async_gen(code, state, restart, global_this, nt, tenant)?;
                return Ok(ag);
            }
            _ => {
                common_ops!(op, code, cache, ip, global_this, nt, tenant);
            }
        }
    }
}

fn gen_step_sync(m: &mut GenMachine, sent: JsValue) -> Result<StepResult, JsValue> {
    if let Some(dest) = m.pending_dest.take() {
        s_set(&m.state, dest, sent.clone());
    }

    if let Some((ref sub, dest)) = m.delegating.clone() {
        let next_fn = Reflect::get(sub, &JsValue::from_str("next")).unwrap_or(JsValue::UNDEFINED);
        let result = if next_fn.is_function() {
            Reflect::apply(next_fn.unchecked_ref::<Function>(), sub, &Array::of1(&sent))
                .unwrap_or(JsValue::UNDEFINED)
        } else {
            JsValue::UNDEFINED
        };
        let sub_done = Reflect::get(&result, &JsValue::from_str("done"))
            .unwrap_or(JsValue::FALSE)
            .is_truthy();
        let sub_value =
            Reflect::get(&result, &JsValue::from_str("value")).unwrap_or(JsValue::UNDEFINED);
        if sub_done {
            s_set(&m.state, dest, sub_value);
            m.delegating = None;
        } else {
            m.delegating = Some((sub.clone(), dest));
            return Ok(StepResult::Yielded(sub_value, u32::MAX));
        }
    }

    let code = m.code.clone();
    let global_this = m.global_this.clone();
    let nt = m.nt.clone();
    let tenant = m.tenant.clone();
    // Build a per-step cache. Flush on each exit point so m.state stays coherent.
    let mut cache = StateCache::new(m.state.clone());

    loop {
        let (op, no) =
            read_u16_le(&code, m.ip).ok_or_else(|| js_err("gen_step: unexpected end"))?;
        m.ip = no;

        match op {
            0 => {
                let val =
                    read_arg(&code, &mut m.ip, &mut cache).ok_or_else(|| js_err("gen RET: val"))?;
                m.done = true;
                cache.flush();
                return Ok(StepResult::Returned(val));
            }
            1 => {
                cache.flush();
                m.ip -= 2;
                return Err(js_err("__jade_vm__upgrade_to_ag__"));
            }
            2 => {
                let val =
                    read_arg(&code, &mut m.ip, &mut cache).ok_or_else(|| js_err("YIELD: val"))?;
                let (dest, no) =
                    read_u32_le(&code, m.ip).ok_or_else(|| js_err("YIELD: dest"))?;
                m.ip = no;
                m.pending_dest = Some(dest);
                cache.flush();
                return Ok(StepResult::Yielded(val, dest));
            }
            3 => {
                let val = read_arg(&code, &mut m.ip, &mut cache)
                    .ok_or_else(|| js_err("YIELDSTAR: val"))?;
                let (dest, no) =
                    read_u32_le(&code, m.ip).ok_or_else(|| js_err("YIELDSTAR: dest"))?;
                m.ip = no;
                cache.flush();
                m.delegating = Some((val, dest));
                return gen_step_sync(m, JsValue::UNDEFINED);
            }
            _ => {
                let mut ip = m.ip;
                let code_ref = &code;
                let global_this_ref = &global_this;
                let nt_ref = &nt;
                let tenant_ref = &tenant;
                common_ops!(op, code_ref, cache, ip, global_this_ref, nt_ref, tenant_ref);
                m.ip = ip;
            }
        }
    }
}

async fn gen_step_async(
    code: Vec<u8>,
    state: JsValue,
    mut ip: usize,
    global_this: JsValue,
    nt: JsValue,
    tenant: JsValue,
) -> Result<(StepResult, usize), JsValue> {
    let mut cache = StateCache::new(state.clone());
    loop {
        let (op, no) =
            read_u16_le(&code, ip).ok_or_else(|| js_err("async_gen_step: unexpected end"))?;
        ip = no;

        match op {
            0 => {
                let val =
                    read_arg(&code, &mut ip, &mut cache).ok_or_else(|| js_err("async RET: val"))?;
                cache.flush();
                return Ok((StepResult::Returned(val), ip));
            }
            1 => {
                let promise_val = read_arg(&code, &mut ip, &mut cache)
                    .ok_or_else(|| js_err("async AWAIT: val"))?;
                let (dest, no) =
                    read_u32_le(&code, ip).ok_or_else(|| js_err("async AWAIT: dest"))?;
                ip = no;
                cache.flush_and_invalidate();
                let resolved = JsFuture::from(Promise::resolve(&promise_val)).await?;
                cache.set(dest, resolved);
            }
            2 => {
                let val = read_arg(&code, &mut ip, &mut cache)
                    .ok_or_else(|| js_err("async YIELD: val"))?;
                let (dest, no) =
                    read_u32_le(&code, ip).ok_or_else(|| js_err("async YIELD: dest"))?;
                ip = no;
                cache.flush();
                return Ok((StepResult::Yielded(val, dest), ip));
            }
            3 => {
                let sub_iter = read_arg(&code, &mut ip, &mut cache)
                    .ok_or_else(|| js_err("async YIELDSTAR: val"))?;
                let (dest, no) =
                    read_u32_le(&code, ip).ok_or_else(|| js_err("async YIELDSTAR: dest"))?;
                ip = no;

                let sent: JsValue = JsValue::UNDEFINED;
                let next_fn = Reflect::get(&sub_iter, &JsValue::from_str("next"))
                    .unwrap_or(JsValue::UNDEFINED);
                let raw_result = if next_fn.is_function() {
                    Reflect::apply(
                        next_fn.unchecked_ref::<Function>(),
                        &sub_iter,
                        &Array::of1(&sent),
                    )
                    .unwrap_or(JsValue::UNDEFINED)
                } else {
                    JsValue::UNDEFINED
                };
                cache.flush_and_invalidate();
                let result = JsFuture::from(Promise::resolve(&raw_result)).await?;
                let sub_done = Reflect::get(&result, &JsValue::from_str("done"))
                    .unwrap_or(JsValue::FALSE)
                    .is_truthy();
                let sub_value = Reflect::get(&result, &JsValue::from_str("value"))
                    .unwrap_or(JsValue::UNDEFINED);
                if sub_done {
                    cache.set(dest, sub_value);
                    continue;
                }
                cache.flush();
                return Ok((StepResult::Yielded(sub_value, dest), ip));
            }
            _ => {
                let code_ref = &code;
                let global_this_ref = &global_this;
                let nt_ref = &nt;
                let tenant_ref = &tenant;
                common_ops!(op, code_ref, cache, ip, global_this_ref, nt_ref, tenant_ref);
            }
        }
    }
}

fn create_sync_gen(
    code: &[u8],
    state: &JsValue,
    start_ip: usize,
    global_this: &JsValue,
    nt: &JsValue,
    tenant: &JsValue,
) -> Result<JsValue, JsValue> {
    use alloc::rc::Rc;

    let machine = Rc::new(RefCell::new(GenMachine {
        code: code.to_vec(),
        state: state.clone(),
        ip: start_ip,
        pending_dest: None,
        delegating: None,
        done: false,
        global_this: global_this.clone(),
        nt: nt.clone(),
        tenant: tenant.clone(),
    }));

    let gen_obj = create_null_obj();

    let m_next = machine.clone();
    let next_fn = Closure::<dyn Fn(JsValue) -> JsValue>::new(move |sent: JsValue| {
        let mut m = m_next.borrow_mut();
        if m.done {
            return make_iter_result(JsValue::UNDEFINED, true);
        }
        match gen_step_sync(&mut m, sent) {
            Ok(StepResult::Yielded(val, _)) => make_iter_result(val, false),
            Ok(StepResult::Returned(val)) => {
                m.done = true;
                make_iter_result(val, true)
            }
            Err(upgrade) if upgrade == JsValue::from_str("__jade_vm__upgrade_to_ag__") => {
                let restart_ip = m.ip;
                let code_c = m.code.clone();
                let state_c = m.state.clone();
                let gt_c = m.global_this.clone();
                let nt_c = m.nt.clone();
                let tenant_c = m.tenant.clone();
                drop(m);
                let ag = create_async_gen(&code_c, &state_c, restart_ip, &gt_c, &nt_c, &tenant_c)
                    .unwrap_or(JsValue::UNDEFINED);
                make_iter_result(ag, false)
            }
            Err(e) => {
                m.done = true;
                make_iter_result(e, true)
            }
        }
    });

    let m_ret = machine.clone();
    let return_fn = Closure::<dyn Fn(JsValue) -> JsValue>::new(move |val: JsValue| {
        m_ret.borrow_mut().done = true;
        make_iter_result(val, true)
    });

    let m_throw = machine;
    let throw_fn =
        Closure::<dyn Fn(JsValue) -> Result<JsValue, JsValue>>::new(move |err: JsValue| {
            m_throw.borrow_mut().done = true;
            Err(err)
        });

    let _ = Reflect::set(&gen_obj, &JsValue::from_str("next"), next_fn.as_ref());
    let _ = Reflect::set(&gen_obj, &JsValue::from_str("return"), return_fn.as_ref());
    let _ = Reflect::set(&gen_obj, &JsValue::from_str("throw"), throw_fn.as_ref());

    next_fn.forget();
    return_fn.forget();
    throw_fn.forget();

    let sym_iter = js_sys::Symbol::iterator();
    let self_iter_factory = Function::new_with_args("g", "return function(){return g}")
        .call1(&JsValue::null(), &gen_obj)?;
    let _ = Reflect::set(&gen_obj, &sym_iter, &self_iter_factory);

    Ok(gen_obj)
}

fn create_async_gen(
    code: &[u8],
    state: &JsValue,
    start_ip: usize,
    global_this: &JsValue,
    nt: &JsValue,
    tenant: &JsValue,
) -> Result<JsValue, JsValue> {
    use alloc::rc::Rc;

    let machine = Rc::new(RefCell::new(GenMachine {
        code: code.to_vec(),
        state: state.clone(),
        ip: start_ip,
        pending_dest: None,
        delegating: None,
        done: false,
        global_this: global_this.clone(),
        nt: nt.clone(),
        tenant: tenant.clone(),
    }));

    let gen_obj = create_null_obj();

    let m_next = machine.clone();
    let next_fn = Closure::<dyn Fn(JsValue) -> Promise>::new(move |sent: JsValue| {
        let mc = m_next.clone();

        future_to_promise(async move {
            let (code, state, pending_dest, delegating, gt, nt, tenant) = {
                let mut m = mc.borrow_mut();
                if m.done {
                    return Ok(make_iter_result(JsValue::UNDEFINED, true));
                }
                (
                    m.code.clone(),
                    m.state.clone(),
                    m.pending_dest.take(),
                    m.delegating.take(),
                    m.global_this.clone(),
                    m.nt.clone(),
                    m.tenant.clone(),
                )
            };

            if let Some(dest) = pending_dest {
                s_set(&state, dest, sent.clone());
            }

            if let Some((sub, dest)) = delegating {
                let next_js =
                    Reflect::get(&sub, &JsValue::from_str("next")).unwrap_or(JsValue::UNDEFINED);
                let raw_result = if next_js.is_function() {
                    Reflect::apply(
                        next_js.unchecked_ref::<Function>(),
                        &sub,
                        &Array::of1(&sent),
                    )
                    .unwrap_or(JsValue::UNDEFINED)
                } else {
                    JsValue::UNDEFINED
                };
                let result = JsFuture::from(Promise::resolve(&raw_result)).await?;
                let sub_done = Reflect::get(&result, &JsValue::from_str("done"))
                    .unwrap_or(JsValue::FALSE)
                    .is_truthy();
                let sub_value = Reflect::get(&result, &JsValue::from_str("value"))
                    .unwrap_or(JsValue::UNDEFINED);
                if sub_done {
                    s_set(&state, dest, sub_value);
                    let mut m = mc.borrow_mut();
                    m.delegating = None;
                } else {
                    let mut m = mc.borrow_mut();
                    m.delegating = Some((sub, dest));
                    return Ok(make_iter_result(sub_value, false));
                }
            }

            let ip = mc.borrow().ip;
            let (step_result, new_ip) =
                gen_step_async(code, state, ip, gt, nt, tenant).await?;

            {
                let mut m = mc.borrow_mut();
                m.ip = new_ip;
                match &step_result {
                    StepResult::Yielded(_, dest) if *dest != u32::MAX => {
                        m.pending_dest = Some(*dest)
                    }
                    StepResult::Returned(_) => m.done = true,
                    _ => {}
                }
            }

            Ok(match step_result {
                StepResult::Yielded(val, _) => make_iter_result(val, false),
                StepResult::Returned(val) => make_iter_result(val, true),
            })
        })
    });

    let m_ret = machine.clone();
    let return_fn = Closure::<dyn Fn(JsValue) -> Promise>::new(move |val: JsValue| {
        m_ret.borrow_mut().done = true;
        Promise::resolve(&make_iter_result(val, true))
    });

    let m_throw = machine;
    let throw_fn = Closure::<dyn Fn(JsValue) -> Promise>::new(move |err: JsValue| {
        m_throw.borrow_mut().done = true;
        Promise::reject(&err)
    });

    let _ = Reflect::set(&gen_obj, &JsValue::from_str("next"), next_fn.as_ref());
    let _ = Reflect::set(&gen_obj, &JsValue::from_str("return"), return_fn.as_ref());
    let _ = Reflect::set(&gen_obj, &JsValue::from_str("throw"), throw_fn.as_ref());

    next_fn.forget();
    return_fn.forget();
    throw_fn.forget();

    let sym_async_iter = js_sys::Symbol::async_iterator();
    let self_iter_factory = Function::new_with_args("g", "return function(){return g}")
        .call1(&JsValue::null(), &gen_obj)?;
    let _ = Reflect::set(&gen_obj, &sym_async_iter, &self_iter_factory);

    Ok(gen_obj)
}

macro_rules! make_async_variant {
    ($name:ident) => {
        #[wasm_bindgen]
        pub async fn $name(
            code: Vec<u8>,
            state: JsValue,
            ip: u32,
            global_this: JsValue,
            nt: JsValue,
            tenant: JsValue,
            args: Array,
        ) -> Result<JsValue, JsValue> {
            run_async_internal(
                &code,
                &state,
                ip as usize,
                &global_this,
                &nt,
                &tenant,
                &args,
            )
            .await
        }
    };
}

#[wasm_bindgen]
pub fn run_virtualized(
    code: Vec<u8>,
    state: JsValue,
    ip: u32,
    global_this: JsValue,
    nt: JsValue,
    tenant: JsValue,
    args: Array,
) -> Result<JsValue, JsValue> {
    run_sync(
        &code,
        &state,
        ip as usize,
        &global_this,
        &nt,
        &tenant,
        &args,
    )
}

make_async_variant!(run_virtualized_a);

#[wasm_bindgen]
pub fn run_virtualized_g(
    code: Vec<u8>,
    state: JsValue,
    ip: u32,
    global_this: JsValue,
    nt: JsValue,
    tenant: JsValue,
    _args: Array,
) -> Result<JsValue, JsValue> {
    create_sync_gen(&code, &state, ip as usize, &global_this, &nt, &tenant)
}

#[wasm_bindgen]
pub fn run_virtualized_ag(
    code: Vec<u8>,
    state: JsValue,
    ip: u32,
    global_this: JsValue,
    nt: JsValue,
    tenant: JsValue,
    _args: Array,
) -> Result<JsValue, JsValue> {
    create_async_gen(&code, &state, ip as usize, &global_this, &nt, &tenant)
}
