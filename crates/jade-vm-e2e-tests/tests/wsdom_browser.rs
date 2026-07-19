//! Browser-only WSDOM integration tests.
//!
//! The test page itself drives both halves of WSDOM: Rust owns a `Browser`, a
//! small JS transport loop drains its outgoing `Stream`, and the in-browser
//! WSDOM client feeds replies back through `receive_incoming_message`. No HTTP
//! or WebSocket server is involved, so this verifies the transport-generic
//! `jade-vm-wsdom` crate rather than an integration-specific server harness.

use core::pin::Pin;
use core::task::{Context, Poll};

use futures_util::Stream;
use js_sys::Reflect;
use portal_solutions_jade_vm_frontend::compile_to_bytecode;
use portal_solutions_jade_vm_wsdom::{ExecutionTier, RemotePromise, RunOptions, WsdomRuntime};
use px_wsdom::{
    Browser, JsCast,
    js_types::{JsNumber, JsValue},
};
use wasm_bindgen::JsValue as WasmValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(inline_js = r#"
export function startWsdomLoop(browser) {
  const values = new Map();
  let next = Number.MAX_SAFE_INTEGER;
  const api = Object.freeze({
    a(value) { const id = next--; values.set(id, { value, error: false }); return id; },
    g(id) { const item = values.get(id); if (item?.error) throw item.value; return item?.value; },
    s(id, value) { values.set(id, { value, error: false }); },
    d(id) { values.delete(id); },
    r(id, value) { browser.receive(`p${id}:${JSON.stringify(value)}`); },
    rp() {},
    c(id) { const item = values.get(id); return item?.error ? { slot: this.a(item.value) } : { value: item?.value }; },
    e(id, value) { values.set(id, { value, error: true }); },
    x: Object.freeze(Object.create(null)),
  });
  Object.freeze(api);
  const tick = () => {
    const message = browser.nextMessage();
    if (message !== undefined) new Function("_w", message)(api);
    queueMicrotask(tick);
  };
  queueMicrotask(tick);
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = startWsdomLoop)]
    fn start_wsdom_loop(browser: &WasmValue);
}

struct BrowserPump {
    browser: Browser,
    _next_message: Closure<dyn FnMut() -> WasmValue>,
    _receive: Closure<dyn FnMut(String)>,
}

impl BrowserPump {
    fn new() -> Self {
        let browser = Browser::new();
        let stream_browser = browser.clone();
        let next_message = Closure::wrap(Box::new(move || -> WasmValue {
            let waker = futures_util::task::noop_waker_ref();
            let mut cx = Context::from_waker(waker);
            let mut stream = stream_browser.clone();
            match Pin::new(&mut stream).poll_next(&mut cx) {
                Poll::Ready(Some(message)) => WasmValue::from_str(&message),
                Poll::Ready(None) | Poll::Pending => WasmValue::UNDEFINED,
            }
        }) as Box<dyn FnMut() -> WasmValue>);

        let bridge = js_sys::Object::new();
        Reflect::set(&bridge, &"nextMessage".into(), next_message.as_ref()).unwrap();
        let receive_browser = browser.clone();
        let receive = Closure::wrap(Box::new(move |message: String| {
            receive_browser.receive_incoming_message(message);
        }) as Box<dyn FnMut(String)>);
        Reflect::set(&bridge, &"receive".into(), receive.as_ref()).unwrap();
        start_wsdom_loop(&bridge);
        Self {
            browser,
            _next_message: next_message,
            _receive: receive,
        }
    }
}

async fn browser_number(value: JsValue) -> f64 {
    let number: JsNumber = JsCast::unchecked_from_js(value);
    number
        .retrieve_float()
        .await
        .expect("retrieve browser number")
}

async fn browser_promise_number(promise: RemotePromise) -> f64 {
    browser_number(promise.await).await
}

fn runtime(pump: &BrowserPump) -> WsdomRuntime {
    let browser = pump.browser.clone();
    let tenant = browser.value_from_raw_code(format_args!(
        "({{make:function*(){{return Object.create(null);}},get:function*(o,k){{return o[k];}},set:function*(o,k,v){{o[k]=v;}},define:function*(o,p){{Object.defineProperties(o,p);}},assign:function*(o,s){{Object.assign(o,s);}},driveTenant:function(g){{let step=g.next();while(!step.done)step=g.next(step.value);return step.value;}},createGuestGen:function*(g){{return g;}}}})"
    ));
    let nt = browser.value_from_raw_code(format_args!("undefined"));
    let global = browser.value_from_raw_code(format_args!("globalThis"));
    WsdomRuntime::new(browser, tenant, nt, global).expect("same connection runtime")
}

#[wasm_bindgen_test(async)]
async fn tier_zero_executes_in_an_in_browser_wsdom_transport() {
    let pump = BrowserPump::new();
    let runtime = runtime(&pump);
    let code = compile_to_bytecode("var o = {}; o[1] = 42; return o[1];").unwrap();
    let state = runtime.default_state();
    let result = runtime
        .run_virtualized(&code, &state, RunOptions::default())
        .expect("compile and enqueue Tier 0");
    assert_eq!(browser_number(result).await, 42.0);
}

#[wasm_bindgen_test(async)]
async fn tier_one_executes_in_an_in_browser_wsdom_transport() {
    let pump = BrowserPump::new();
    let runtime = runtime(&pump);
    let code = compile_to_bytecode("var x = true; while (x) { x = false; } return 17;").unwrap();
    let state = runtime.default_state();
    let mut options = RunOptions::default();
    options.tier = ExecutionTier::Tier1;
    let result = runtime
        .run_virtualized(&code, &state, options)
        .expect("compile and enqueue Tier 1");
    assert_eq!(browser_number(result).await, 17.0);
}

#[wasm_bindgen_test(async)]
async fn tier_one_async_wrapper_executes_in_an_in_browser_wsdom_transport() {
    let pump = BrowserPump::new();
    let runtime = runtime(&pump);
    let code = compile_to_bytecode("return 31;").unwrap();
    let state = runtime.default_state();
    let mut options = RunOptions::default();
    options.tier = ExecutionTier::Tier1;
    let result = runtime
        .run_virtualized_a(&code, &state, options)
        .expect("compile and enqueue Tier 1 async wrapper");
    assert_eq!(browser_promise_number(result).await, 31.0);
}

#[wasm_bindgen_test(async)]
async fn tier_two_executes_in_an_in_browser_wsdom_transport() {
    let pump = BrowserPump::new();
    let runtime = runtime(&pump);
    let code = compile_to_bytecode("if (true) { return 23; } else { return 0; }").unwrap();
    let state = runtime.default_state();
    let mut options = RunOptions::default();
    options.tier = ExecutionTier::Tier2;
    let result = runtime
        .run_virtualized(&code, &state, options)
        .expect("compile and enqueue Tier 2");
    assert_eq!(browser_number(result).await, 23.0);
}

#[wasm_bindgen_test(async)]
async fn tier_two_async_wrapper_executes_in_an_in_browser_wsdom_transport() {
    let pump = BrowserPump::new();
    let runtime = runtime(&pump);
    let code = compile_to_bytecode("return 37;").unwrap();
    let state = runtime.default_state();
    let mut options = RunOptions::default();
    options.tier = ExecutionTier::Tier2;
    let result = runtime
        .run_virtualized_a(&code, &state, options)
        .expect("compile and enqueue Tier 2 async wrapper");
    assert_eq!(browser_promise_number(result).await, 37.0);
}

#[wasm_bindgen_test(async)]
async fn tier_zero_preserves_state_across_wsdom_invocations() {
    let pump = BrowserPump::new();
    let runtime = runtime(&pump);
    let state = runtime.default_state();
    let first = compile_to_bytecode("var x = 7; return x;").unwrap();
    let second = compile_to_bytecode("return 9;").unwrap();
    let result = runtime
        .run_virtualized(&first, &state, RunOptions::default())
        .unwrap();
    assert_eq!(browser_number(result).await, 7.0);
    let result = runtime
        .run_virtualized(&second, &state, RunOptions::default())
        .unwrap();
    assert_eq!(browser_number(result).await, 9.0);
}
