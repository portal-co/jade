//! Full-pipeline (JS source -> frontend -> bytecode -> JIT -> browser) end-to-end tests.
//!
//! Unlike `crates/jade-vm-frontend`'s own unit tests (which shell out to Node), these run
//! the compiled JS *inside a real browser JS engine* via `wasm-bindgen-test`, using
//! `js_sys::Function` to build and invoke it. Run with:
//!
//! ```sh
//! wasm-pack test --headless --chrome crates/jade-vm-e2e-tests
//! ```
//!
//! (or `--firefox`/`--safari`), or equivalently:
//!
//! ```sh
//! CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
//!   cargo test -p portal-solutions-jade-vm-e2e-tests --target wasm32-unknown-unknown
//! ```
//!
//! with `wasm-bindgen-test-runner` (from `cargo install wasm-bindgen-cli`, matching the
//! `wasm-bindgen` version this workspace resolves to) as the configured test runner, and a
//! chromedriver/geckodriver/safaridriver on `PATH`. The driver is auto-detected on `PATH` in
//! the order geckodriver, safaridriver, chromedriver, msedgedriver — on macOS this means the
//! built-in `safaridriver` wins by default even with chromedriver also installed, and Safari
//! needs `sudo safaridriver --enable` (a one-time, interactive step this can't do
//! non-interactively) before it'll actually run. To force Chrome instead, set
//! `CHROMEDRIVER=/path/to/chromedriver` (e.g. `brew install --cask chromedriver`, then
//! `xattr -d com.apple.quarantine "$(which chromedriver)"` since the cask isn't
//! Gatekeeper-notarized) — its major.build version must match the installed Chrome's (check
//! both with `--version`; mismatches fail session creation). Verified passing against Chrome
//! 150.0.7871.47 / chromedriver 150.0.7871.46.

use portal_solutions_jade_vm_frontend::compile_to_bytecode;
use portal_solutions_jade_vm_jit::{compile, Config, VecRegistry};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

/// Compile `src` end-to-end (SWC parse -> TAC -> reloop -> Jade bytecode -> JIT) and build
/// a real, callable `js_sys::Function` from the result, in whatever JS engine this test is
/// currently running under (a real browser, per `wasm_bindgen_test_configure!(run_in_browser)`
/// above).
fn compile_to_js_function(src: &str) -> js_sys::Function {
    let bytecode = compile_to_bytecode(src).expect("frontend compile failed");
    let (body, _reg) = compile(&bytecode, VecRegistry::new(), Config::default()).expect("JIT compile failed");
    js_sys::Function::new_with_args("state", &body)
}

fn call0(f: &js_sys::Function) -> JsValue {
    let state = js_sys::Array::new();
    f.call1(&JsValue::UNDEFINED, &state).expect("compiled function threw")
}

#[wasm_bindgen_test]
fn returns_a_literal() {
    let f = compile_to_js_function("return 42;");
    assert_eq!(call0(&f).as_f64(), Some(42.0));
}

#[wasm_bindgen_test]
fn if_else_picks_the_right_branch() {
    let f = compile_to_js_function("if (true) { return 1; } else { return 2; }");
    assert_eq!(call0(&f).as_f64(), Some(1.0));

    let f = compile_to_js_function("if (false) { return 1; } else { return 2; }");
    assert_eq!(call0(&f).as_f64(), Some(2.0));
}

#[wasm_bindgen_test]
fn while_loop_runs_to_completion() {
    let f = compile_to_js_function("var x = true; while (x) { x = false; } return x;");
    assert_eq!(call0(&f).as_bool(), Some(false));
}

#[wasm_bindgen_test]
fn nested_if_inside_while() {
    let f = compile_to_js_function(
        "var x = true; \
         while (x) { \
           x = false; \
           if (true) { var y = 1; } else { var y = 2; } \
         } \
         return x;",
    );
    assert_eq!(call0(&f).as_bool(), Some(false));
}

#[wasm_bindgen_test]
fn comparison_operators_produce_real_booleans() {
    let f = compile_to_js_function("if (1 < 2) { return true; } else { return false; }");
    assert_eq!(call0(&f).as_bool(), Some(true));
}

/// Object operations (`GET`/`SET`/`LITOBJ`) need a `tenant` object in scope at the call
/// site; the JIT threads it as the compiled function's first real argument (see
/// `docs/pluggable-tenant-interface-plan.md` for the full interface this stands in for).
/// This is a minimal, plain-`Map`-backed stand-in — not `packages/jade-js`'s
/// `MultiTenant` — sufficient to exercise the bytecode/JIT's own calling convention in a
/// real browser without pulling in a JS build step for this Rust-only test crate.
fn minimal_tenant() -> JsValue {
    js_sys::eval(
        "({
            make(proto) { return new Map(); },
            get(obj, key) { return obj.get(key); },
            set(obj, key, val) { obj.set(key, val); },
        })",
    )
    .expect("failed to build minimal tenant stub")
}

#[wasm_bindgen_test]
fn object_literal_and_member_access_via_tenant() {
    let bytecode = compile_to_bytecode("var o = {}; o[1] = 42; return o[1];").expect("frontend compile failed");
    let (body, _reg) = compile(&bytecode, VecRegistry::new(), Config::default()).expect("JIT compile failed");
    let f = js_sys::Function::new_with_args("tenant, nt, state", &body);
    let tenant = minimal_tenant();
    let state = js_sys::Array::new();
    let result = f.call3(&JsValue::UNDEFINED, &tenant, &JsValue::UNDEFINED, &state).expect("compiled function threw");
    assert_eq!(result.as_f64(), Some(42.0));
}
