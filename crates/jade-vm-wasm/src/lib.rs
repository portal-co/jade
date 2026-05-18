// Bytecode interpreter for the Jade VM.
//
// Four public variants mirror the JS vm.ts exports:
//   run_virtualized      - sync
//   run_virtualized_a    - async (awaits JS Promises)
//   run_virtualized_g    - sync generator  (returns a JS iterator object)
//   run_virtualized_ag   - async generator (returns a JS async-iterator object)
//
// Bytecode decoding uses `Operation::parse` from `portal_solutions_jade_vm` so
// there is no duplicated byte-reading logic here.
//
// Optimisations:
//
//   StateCache – each interpreter invocation keeps a `Vec<Option<JsValue>>` that
//   mirrors the numeric-keyed JS state object.  Reads populate on miss; writes
//   go to the cache and mark a dirty list.  The cache is flushed to the JS object
//   only on exit or before a foreign call.
//
//   FN_REGISTRY WeakMap – when FN creates a JS wrapper function the jade metadata
//   (variant index, entry ip, closure slot list) is stored in a module-level
//   WeakMap keyed on the wrapper.  The CALL opcode checks the WeakMap: if the
//   callee is a sync jade function it dispatches directly to `run_sync` in Rust
//   (no Rust→JS→Rust round-trip, no state flush needed).

#![allow(unused_imports)]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::cell::RefCell;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint32Array, WeakMap};
use portal_solutions_jade_vm::{Operand, Operation, SignedOperand};
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
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn js_err(s: &str) -> JsValue {
    JsValue::from_str(s)
}

/// Direct Reflect bridge – used only for StateCache flush/fill and FN closures.
#[inline]
fn s_get(state: &JsValue, idx: u32) -> JsValue {
    Reflect::get(state, &JsValue::from_f64(idx as f64)).unwrap_or(JsValue::UNDEFINED)
}

#[inline]
fn s_set(state: &JsValue, idx: u32, val: JsValue) {
    let _ = Reflect::set(state, &JsValue::from_f64(idx as f64), &val);
}

/// Resolve an `Operand` to a JS value using the state cache.
#[inline]
fn resolve(op: Operand, cache: &mut StateCache) -> JsValue {
    match op {
        Operand::Literal(val) => JsValue::from_f64(val as f64),
        Operand::StateRef(idx) => cache.get(idx),
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

// ---------------------------------------------------------------------------
// StateCache
// ---------------------------------------------------------------------------

struct StateCache {
    state: JsValue,
    slots: Vec<Option<JsValue>>,
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

    fn flush(&self) {
        for &slot in &self.dirty {
            if let Some(Some(v)) = self.slots.get(slot as usize) {
                s_set(&self.state, slot, v.clone());
            }
        }
    }

    fn flush_and_invalidate(&mut self) {
        self.flush();
        self.slots.clear();
        self.dirty.clear();
    }
}

// ---------------------------------------------------------------------------
// FN_REGISTRY – WeakMap<Function, {v, j, s: Uint32Array}>
// ---------------------------------------------------------------------------

thread_local! {
    static FN_REGISTRY: WeakMap = WeakMap::new();
}

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
// Child-state construction
// ---------------------------------------------------------------------------

/// Getter/setter descriptors referencing the parent state – used by the JS
/// wrapper function created in FN so external JS calls work correctly.
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

/// Direct slot copies from the parent cache – used by the sync jade-to-jade
/// CALL fast path (avoids the JS getter/setter bounce).
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
// exec_common – opcodes shared across all four interpreter variants
//
// Handles GLOBAL, FN, LIT32, ARR, STR, LITOBJ, NEW_TARGET, CALL.
// Returns Err only on unknown opcode or internal decode error.
// ---------------------------------------------------------------------------

fn exec_common(
    op: Operation,
    code: &[u8],
    cache: &mut StateCache,
    global_this: &JsValue,
    nt: &JsValue,
    tenant: &JsValue,
) -> Result<(), JsValue> {
    match op {
        Operation::Global(dest) => {
            cache.set(dest, global_this.clone());
        }

        Operation::Fn { variant, closure_args, spanner, j, dest } => {
            let variant_idx = (resolve(variant, cache).as_f64().unwrap_or(0.0) as u32) & 3;
            let closure_args_val = resolve(closure_args, cache);
            let spanner_val = resolve(spanner, cache);

            let closure_slots: Vec<u32> = {
                let arr: Array = closure_args_val.unchecked_into();
                (0..arr.length())
                    .map(|i| arr.get(i).as_f64().unwrap_or(0.0) as u32)
                    .collect()
            };
            let closure_slots_reg = closure_slots.clone();

            let (spanner_fn, spans): (JsValue, Vec<JsValue>) =
                if spanner_val.is_null() || spanner_val.is_undefined() {
                    (Function::new_with_args("a", "return a").into(), Vec::new())
                } else {
                    let arr: Array = spanner_val.unchecked_into();
                    let sp = arr.get(0);
                    let extra: Vec<JsValue> = (1..arr.length()).map(|i| arr.get(i)).collect();
                    (sp, extra)
                };

            let code_c: Vec<u8> = code.to_vec();
            // Flush before capturing parent_state: getter/setter closures below
            // read from the JS state object directly.
            cache.flush();
            let parent_state = cache.state.clone();
            let gt_c = global_this.clone();
            let tenant_c = tenant.clone();

            let inner = Closure::<dyn Fn(JsValue, Array) -> JsValue>::new(
                move |js_this: JsValue, js_args: Array| {
                    let child = build_child_state(&parent_state, &closure_slots);
                    dispatch_variant(
                        variant_idx,
                        &code_c,
                        &child,
                        j,
                        &gt_c,
                        &js_this,
                        &tenant_c,
                        &js_args,
                    )
                },
            );
            let inner_js = inner.into_js_value();
            let factory = Function::new_with_args("f", "return function(...a){return f(this,a)}");
            let wrapper = factory.call1(&JsValue::null(), &inner_js).map_err(|e| e)?;

            let spanned = if spans.is_empty() {
                wrapper
            } else if let Some(sp_fn) = spanner_fn.dyn_ref::<Function>() {
                let call_args = Array::new();
                call_args.push(&wrapper);
                for s in &spans {
                    call_args.push(s);
                }
                Reflect::apply(sp_fn, &JsValue::null(), &call_args).unwrap_or(wrapper)
            } else {
                wrapper
            };

            registry_set(&spanned, variant_idx, j, &closure_slots_reg);
            cache.set(dest, spanned);
        }

        Operation::Lit32 { dest, val } => {
            cache.set(dest, JsValue::from_f64(val as f64));
        }

        Operation::Arr(items, dest) => {
            let arr = Array::new_with_length(items.len() as u32);
            for (i, item) in items.into_iter().enumerate() {
                arr.set(i as u32, resolve(item, cache));
            }
            cache.set(dest, arr.into());
        }

        Operation::Str(codepoints, dest) => {
            let s: String = codepoints
                .into_iter()
                .filter_map(|cp| {
                    char::from_u32(resolve(cp, cache).as_f64().unwrap_or(0.0) as u32)
                })
                .collect();
            cache.set(dest, JsValue::from_str(&s));
        }

        Operation::Litobj { c, pairs, key } => {
            let spread = c.as_i32() < 0;
            let count = c.as_i32().unsigned_abs() as usize;

            // pairs encoding for spread: first pair is (spread_src, _), rest are k/v
            let mut pair_iter = pairs.into_iter();

            let obj = if spread {
                let src = pair_iter
                    .next()
                    .map(|(src_op, _)| resolve(src_op, cache))
                    .unwrap_or(JsValue::UNDEFINED);
                let base = create_null_obj();
                js_object_assign(&base, &src);
                base
            } else {
                create_null_obj()
            };

            for (k_op, v_op) in pair_iter.take(count) {
                let k = resolve(k_op, cache);
                let v = resolve(v_op, cache);
                let ck = tenant_clean(tenant, &obj, k);
                let _ = Reflect::set(&obj, &ck, &v);
            }

            // Operand::StateRef(idx) → defineProperties on existing slot
            // Operand::Literal(idx) → assign to slot
            match key {
                Operand::StateRef(idx) => {
                    let target = cache.get(idx);
                    js_define_properties(&target, &obj);
                }
                Operand::Literal(idx) => {
                    cache.set(idx, obj);
                }
            }
        }

        Operation::NewTarget(dest) => {
            cache.set(dest, nt.clone());
        }

        Operation::Call { fn_op, args: arg_ops, dest } => {
            let fn_val = resolve(fn_op, cache);
            let call_args = Array::new_with_length(arg_ops.len() as u32);
            for (i, a) in arg_ops.into_iter().enumerate() {
                call_args.set(i as u32, resolve(a, cache));
            }

            let result = if let Some((variant_idx, j, closure_slots)) = registry_get(&fn_val) {
                if variant_idx == 0 {
                    // Sync jade-to-jade: dispatch directly in Rust.
                    let child = build_child_state_fast(&closure_slots, cache);
                    let res = run_sync(
                        code,
                        &child,
                        j as usize,
                        global_this,
                        &JsValue::UNDEFINED,
                        tenant,
                        &call_args,
                    )
                    .unwrap_or(JsValue::UNDEFINED);
                    for &slot in &closure_slots {
                        cache.set(slot, s_get(&child, slot));
                    }
                    res
                } else {
                    // Non-sync jade callee: flush and use JS wrapper.
                    cache.flush_and_invalidate();
                    Reflect::apply(
                        fn_val.unchecked_ref::<Function>(),
                        &JsValue::UNDEFINED,
                        &call_args,
                    )
                    .unwrap_or(JsValue::UNDEFINED)
                }
            } else {
                // Foreign function: flush, call, invalidate.
                cache.flush_and_invalidate();
                Reflect::apply(
                    fn_val.unchecked_ref::<Function>(),
                    &JsValue::UNDEFINED,
                    &call_args,
                )
                .unwrap_or(JsValue::UNDEFINED)
            };

            cache.set(dest, result);
        }

        _ => return Err(js_err("exec_common: unexpected opcode")),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// dispatch_variant
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// run_sync
// ---------------------------------------------------------------------------

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
        let old_ip = ip;
        let (op, rest) = Operation::parse(&code[ip..])
            .ok_or_else(|| js_err("run_sync: unexpected end"))?;
        ip = code.len() - rest.len();

        match op {
            Operation::Ret(val_op) => {
                let val = resolve(val_op, &mut cache);
                cache.flush();
                return Ok(val);
            }
            Operation::Await { .. } => {
                // Encountered AWAIT in a sync context: upgrade to async.
                cache.flush();
                let restart = old_ip;
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
                return Ok(promise.into());
            }
            Operation::Yield { .. } | Operation::Yieldstar { .. } => {
                cache.flush();
                return create_sync_gen(code, state, old_ip, global_this, nt, tenant);
            }
            _ => {
                exec_common(op, code, &mut cache, global_this, nt, tenant)?;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// run_async_internal
// ---------------------------------------------------------------------------

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
        let old_ip = ip;
        let (op, rest) = Operation::parse(&code[ip..])
            .ok_or_else(|| js_err("run_async: unexpected end"))?;
        ip = code.len() - rest.len();

        match op {
            Operation::Ret(val_op) => {
                let val = resolve(val_op, &mut cache);
                cache.flush();
                return Ok(val);
            }
            Operation::Await { val: val_op, dest } => {
                let promise_val = resolve(val_op, &mut cache);
                cache.flush_and_invalidate();
                let resolved = JsFuture::from(Promise::resolve(&promise_val)).await?;
                cache.set(dest, resolved);
            }
            Operation::Yield { .. } | Operation::Yieldstar { .. } => {
                cache.flush();
                return Ok(create_async_gen(code, state, old_ip, global_this, nt, tenant)?);
            }
            _ => {
                exec_common(op, code, &mut cache, global_this, nt, tenant)?;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// gen_step_sync
// ---------------------------------------------------------------------------

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
    let mut cache = StateCache::new(m.state.clone());

    loop {
        let old_ip = m.ip;
        let (op, rest) = Operation::parse(&code[m.ip..])
            .ok_or_else(|| js_err("gen_step: unexpected end"))?;
        m.ip = code.len() - rest.len();

        match op {
            Operation::Ret(val_op) => {
                let val = resolve(val_op, &mut cache);
                m.done = true;
                cache.flush();
                return Ok(StepResult::Returned(val));
            }
            Operation::Await { .. } => {
                cache.flush();
                m.ip = old_ip;
                return Err(js_err("__jade_vm__upgrade_to_ag__"));
            }
            Operation::Yield { val: val_op, dest } => {
                let val = resolve(val_op, &mut cache);
                m.pending_dest = Some(dest);
                cache.flush();
                return Ok(StepResult::Yielded(val, dest));
            }
            Operation::Yieldstar { val: val_op, dest } => {
                let sub = resolve(val_op, &mut cache);
                cache.flush();
                m.delegating = Some((sub, dest));
                return gen_step_sync(m, JsValue::UNDEFINED);
            }
            _ => {
                exec_common(op, &code, &mut cache, &global_this, &nt, &tenant)?;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// gen_step_async
// ---------------------------------------------------------------------------

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
        let old_ip = ip;
        let (op, rest) = Operation::parse(&code[ip..])
            .ok_or_else(|| js_err("gen_step_async: unexpected end"))?;
        ip = code.len() - rest.len();

        match op {
            Operation::Ret(val_op) => {
                let val = resolve(val_op, &mut cache);
                cache.flush();
                return Ok((StepResult::Returned(val), ip));
            }
            Operation::Await { val: val_op, dest } => {
                let promise_val = resolve(val_op, &mut cache);
                cache.flush_and_invalidate();
                let resolved = JsFuture::from(Promise::resolve(&promise_val)).await?;
                cache.set(dest, resolved);
            }
            Operation::Yield { val: val_op, dest } => {
                let val = resolve(val_op, &mut cache);
                cache.flush();
                return Ok((StepResult::Yielded(val, dest), ip));
            }
            Operation::Yieldstar { val: val_op, dest } => {
                let sub_iter = resolve(val_op, &mut cache);
                cache.flush_and_invalidate();
                let next_fn = Reflect::get(&sub_iter, &JsValue::from_str("next"))
                    .unwrap_or(JsValue::UNDEFINED);
                let raw_result = if next_fn.is_function() {
                    Reflect::apply(
                        next_fn.unchecked_ref::<Function>(),
                        &sub_iter,
                        &Array::of1(&JsValue::UNDEFINED),
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
                    cache.set(dest, sub_value);
                } else {
                    cache.flush();
                    return Ok((StepResult::Yielded(sub_value, dest), ip));
                }
            }
            _ => {
                exec_common(op, &code, &mut cache, &global_this, &nt, &tenant)?;
                let _ = old_ip;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// create_sync_gen
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// create_async_gen
// ---------------------------------------------------------------------------

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
                    mc.borrow_mut().delegating = None;
                } else {
                    mc.borrow_mut().delegating = Some((sub, dest));
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

// ---------------------------------------------------------------------------
// Public WASM exports
// ---------------------------------------------------------------------------

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
            run_async_internal(&code, &state, ip as usize, &global_this, &nt, &tenant, &args)
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
    run_sync(&code, &state, ip as usize, &global_this, &nt, &tenant, &args)
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
