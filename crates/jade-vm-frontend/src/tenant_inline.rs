//! Extract inlinable method bodies from a tenant implementation's own source text.
//!
//! See `docs/pluggable-tenant-interface-plan.md` for the full design this supports: the
//! JIT (`crates/jade-vm-jit`) can splice a tenant method's *actual current* body directly
//! into compiled code instead of emitting a `tenant.<method>(...)` call. A method is only
//! inlinable if the JIT's splice site (a plain, detached function call in a different
//! lexical scope than the tenant class body — see `inline_call` in `jade-vm-jit`/
//! `jade-vm-jit-swc`) can actually resolve everything the body references:
//!
//! - **`#private` field/method references** are a hard rejection — `#x` is only
//!   reachable from code lexically nested inside the declaring class, so splicing the
//!   body out would break it. No rewrite can fix this.
//! - **`this` references** are *not* a rejection reason: the body is rewritten (parsed,
//!   `this` replaced with a new leading parameter, re-serialized) rather than byte-sliced
//!   verbatim, so the JIT's splice site can pass the real tenant reference as an ordinary
//!   argument instead of needing special `this`-binding call-form support.
//! - **Calling a captured external identifier as a function** (e.g. a stray free-standing
//!   import used as a callee) is a hard rejection — the JIT's splice site has no way to
//!   supply "a function we don't have." Only a call whose callee is a parameter, a
//!   now-legal `this`-derived reference, or a known JS global is allowed.
//!
//! `tenant_source` is expected to be a class's own source text, as returned by
//! `SomeTenantClass.toString()` in a real browser (see the doc above for why this is
//! browser-only in practice) — a plain object's `.toString()` doesn't return its own
//! declaration source the way a function/class's does, so a class expression/declaration
//! is the only shape this needs to handle.
//!
//! Tenant operations are generator methods. The extracted `is_generator` bit lets the JIT
//! emit a `function*` IIFE and drive it at the outer call site; the ABI/driver mixin methods
//! (`yieldTenant`, `driveTenant`, and the existing shim helpers) deliberately stay outside
//! `TENANT_METHOD_NAMES`, so an inlined operation still reaches them through `__this`.

use std::collections::HashMap;

use swc_common::sync::Lrc;
use swc_common::{DUMMY_SP, FileName, SourceMap};
use swc_ecma_ast::{Callee, Class, ClassMember, Expr, Ident, Param, Pat, PrivateName, ThisExpr};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax};
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

/// The tenant interface methods the JIT knows how to call — see `TenantInterface` in
/// `packages/jade-js` (per `docs/pluggable-tenant-interface-plan.md`).
pub const TENANT_METHOD_NAMES: &[&str] = &["make", "get", "set", "define", "assign", "ownKeys"];

/// JS globals a tenant method body may freely call without being rejected as "calling a
/// captured external function we don't have" — not exhaustive, just the ones plausible in
/// a tenant implementation's own bookkeeping code today; extend as real methods need more.
const ALLOWED_GLOBAL_CALLEES: &[&str] = &[
    "Reflect",
    "Object",
    "Array",
    "WeakMap",
    "WeakSet",
    "Map",
    "Set",
    "Symbol",
    "JSON",
    "Math",
    "Promise",
    "String",
    "Number",
    "Boolean",
    "Proxy",
    "Function",
    "RegExp",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
];

/// The parameter name a rewritten `this` reference becomes — see the module doc comment.
const THIS_PARAM_NAME: &str = "__this";

/// One method extracted from a tenant's source, ready to be spliced into generated code
/// as `(function(${params.join(",")})${body_block})(${args...})`.
#[derive(Debug, Clone)]
pub struct InlinableTenantMethod {
    /// Parameter names, in declaration order. If the original method referenced `this`,
    /// its rewritten form (`THIS_PARAM_NAME`) is prepended here — the caller must pass the
    /// real tenant reference as the corresponding leading argument.
    pub params: Vec<String>,
    /// The method body's `{ ... }` text, braces included — the exact original source
    /// byte-sliced verbatim if `this` wasn't referenced, or re-serialized (via
    /// `swc_ecma_codegen`) after rewriting `this` into `THIS_PARAM_NAME` otherwise.
    pub body_block: String,
    /// Whether the method was declared as a generator (`*get() { ... }`). The JIT's
    /// splice site uses this to emit `function*` for the inlined IIFE and to wrap it
    /// with `tenant.driveTenant(...)` — every real tenant operation is a generator.
    pub is_generator: bool,
}

struct PrivateNameFinder {
    found: bool,
}
impl Visit for PrivateNameFinder {
    fn visit_private_name(&mut self, _node: &PrivateName) {
        self.found = true;
    }
}

fn references_private_name(body: &swc_ecma_ast::BlockStmt) -> bool {
    let mut finder = PrivateNameFinder { found: false };
    body.visit_with(&mut finder);
    finder.found
}

struct ThisFinder {
    found: bool,
}
impl Visit for ThisFinder {
    fn visit_this_expr(&mut self, _node: &ThisExpr) {
        self.found = true;
    }
}

fn references_this(body: &swc_ecma_ast::BlockStmt) -> bool {
    let mut finder = ThisFinder { found: false };
    body.visit_with(&mut finder);
    finder.found
}

/// Finds a call whose callee is a *bare identifier* (`foo(...)`, not `obj.foo(...)` or
/// `this.foo(...)` — those are `MemberExpr` callees, unaffected by this check) that isn't
/// one of `allowed` (the method's own parameter names, plus `THIS_PARAM_NAME` since a
/// `this`-rewrite may introduce a call through it, plus `ALLOWED_GLOBAL_CALLEES`) — i.e. a
/// captured external function reference the JIT's splice site has no way to supply.
struct CapturedCalleeFinder<'a> {
    allowed: &'a [String],
    found: bool,
}
impl Visit for CapturedCalleeFinder<'_> {
    fn visit_callee(&mut self, node: &Callee) {
        if let Callee::Expr(callee) = node
            && let Expr::Ident(id) = &**callee
        {
            let name = id.sym.as_str();
            let is_allowed = name == THIS_PARAM_NAME
                || self.allowed.iter().any(|p| p == name)
                || ALLOWED_GLOBAL_CALLEES.contains(&name);
            if !is_allowed {
                self.found = true;
            }
        }
        node.visit_children_with(self);
    }
}

fn calls_captured_external_fn(body: &swc_ecma_ast::BlockStmt, params: &[String]) -> bool {
    let mut finder = CapturedCalleeFinder {
        allowed: params,
        found: false,
    };
    body.visit_with(&mut finder);
    finder.found
}

/// Replaces every `this` reference with an `Ident` named `THIS_PARAM_NAME`.
struct ThisReplacer;
impl VisitMut for ThisReplacer {
    fn visit_mut_expr(&mut self, node: &mut Expr) {
        if matches!(node, Expr::This(_)) {
            *node = Expr::Ident(Ident::new(
                THIS_PARAM_NAME.into(),
                DUMMY_SP,
                Default::default(),
            ));
            return;
        }
        node.visit_mut_children_with(self);
    }
}

/// Parse `source` (a tenant class's own `.toString()` output) and extract the subset of
/// `TENANT_METHOD_NAMES` that are safe to inline: present, with only plain-identifier
/// parameters, no `#private` member reference anywhere in the body, and no call to a
/// captured external function reference (see the module doc comment) — `this` references
/// are rewritten rather than rejected.
///
/// Returns an empty map (never an error) for anything that doesn't parse as a class
/// expression/declaration, or has no recognized methods — inlining is purely an
/// optimization, so "extract nothing" is always a safe, silent fallback to the JIT's
/// normal `tenant.<method>(...)` call form.
pub fn extract_tenant_methods(source: &str) -> HashMap<String, InlinableTenantMethod> {
    let mut out = HashMap::new();
    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let Some((class, base_pos)) = parse_class(source) else {
            return;
        };
        for member in &class.body {
            let ClassMember::Method(method) = member else {
                continue;
            };
            let swc_ecma_ast::PropName::Ident(name) = &method.key else {
                continue;
            };
            if !TENANT_METHOD_NAMES.contains(&name.sym.as_str()) {
                continue;
            }
            let Some(params) = simple_param_names(&method.function.params) else {
                continue;
            };
            let Some(body) = &method.function.body else {
                continue;
            };
            if references_private_name(body) {
                continue;
            }
            if calls_captured_external_fn(body, &params) {
                continue;
            }
            if params.iter().any(|p| p == THIS_PARAM_NAME) {
                // Vanishingly unlikely in practice, but a real parameter already named
                // `THIS_PARAM_NAME` would collide with a `this`-rewrite — skip rather
                // than risk silently shadowing it.
                continue;
            }
            let is_generator = method.function.is_generator;
            if references_this(body) {
                let mut rewritten = body.clone();
                rewritten.visit_mut_with(&mut ThisReplacer);
                let Some(body_block) = codegen_block(&rewritten) else {
                    continue;
                };
                let mut params = params;
                params.insert(0, THIS_PARAM_NAME.to_string());
                out.insert(
                    name.sym.to_string(),
                    InlinableTenantMethod {
                        params,
                        body_block,
                        is_generator,
                    },
                );
            } else {
                let lo = (body.span.lo.0 - base_pos.0) as usize;
                let hi = (body.span.hi.0 - base_pos.0) as usize;
                let Some(body_block) = source.get(lo..hi) else {
                    continue;
                };
                out.insert(
                    name.sym.to_string(),
                    InlinableTenantMethod {
                        params,
                        body_block: body_block.to_string(),
                        is_generator,
                    },
                );
            }
        }
    });
    out
}

fn simple_param_names(params: &[Param]) -> Option<Vec<String>> {
    params
        .iter()
        .map(|p| match &p.pat {
            Pat::Ident(id) => Some(id.id.sym.to_string()),
            _ => None,
        })
        .collect()
}

/// Parse `source` as an expression and return its `Class` if it's a class
/// expression/declaration, along with the base `BytePos` of the source file (needed to
/// convert the AST's global byte positions back into offsets into `source` itself).
fn parse_class(source: &str) -> Option<(Class, swc_common::BytePos)> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("tenant.js".into())),
        source.to_string(),
    );
    let base_pos = fm.start_pos;
    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let expr = parser.parse_expr().ok()?;
    match *expr {
        Expr::Class(class_expr) => Some((*class_expr.class, base_pos)),
        _ => None,
    }
}

/// Serialize `block` (a rewritten method body) back to `{ ... }` source text. `None` on any
/// codegen failure — inlining is purely an optimization, so the caller falls back to
/// treating the method as non-inlinable rather than propagating an error.
fn codegen_block(block: &swc_ecma_ast::BlockStmt) -> Option<String> {
    use swc_ecma_codegen::text_writer::JsWriter;
    use swc_ecma_codegen::{Config as CgConfig, Emitter, Node};
    let cm: Lrc<SourceMap> = Default::default();
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut emitter = Emitter {
            cfg: CgConfig::default(),
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm.clone(), "\n", &mut buf, None),
        };
        block.emit_with(&mut emitter).ok()?;
    }
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_inlinable_methods() {
        let src = "class T { get(obj, key) { return obj.get(key); } make(proto) { return {}; } }";
        let methods = extract_tenant_methods(src);
        let get = methods.get("get").expect("get should be inlinable");
        assert_eq!(get.params, vec!["obj", "key"]);
        assert!(
            !get.is_generator,
            "plain method should not be marked generator"
        );
        assert!(
            get.body_block.contains("obj.get(key)"),
            "got: {}",
            get.body_block
        );
        assert!(methods.contains_key("make"));
    }

    #[test]
    fn extracts_generator_methods_as_inlinable() {
        // Real tenant operations are generators (`*get`, `*set`). The parser must still
        // extract them and set `is_generator` so the JIT can emit `function*` + drive.
        let src =
            "class T { *get(obj, key) { yield __this.yieldTenant(__this.invokeTrap(obj, key)); } }";
        let methods = extract_tenant_methods(src);
        let get = methods
            .get("get")
            .expect("generator get() should be inlinable");
        assert_eq!(get.params, vec!["obj", "key"]);
        assert!(get.is_generator, "generator method must be flagged");
        assert!(get.body_block.contains("yield"), "got: {}", get.body_block);
    }

    #[test]
    fn rejects_methods_referencing_private_fields() {
        let src = "class T { get(obj, key) { return this.#slots.get(key); } }";
        let methods = extract_tenant_methods(src);
        assert!(
            !methods.contains_key("get"),
            "method using #private should not be inlinable"
        );
    }

    #[test]
    fn rejects_methods_with_non_ident_params() {
        let src = "class T { set(obj, { key, value }) { } }";
        let methods = extract_tenant_methods(src);
        assert!(
            !methods.contains_key("set"),
            "destructured params should not be inlinable"
        );
    }

    #[test]
    fn ignores_unrecognized_methods_and_non_class_input() {
        assert!(extract_tenant_methods("class T { helper() {} }").is_empty());
        assert!(extract_tenant_methods("not valid js class {{{").is_empty());
        assert!(extract_tenant_methods("({ get(o,k) { return o.get(k); } })").is_empty());
    }

    /// A method referencing `this` (but not any `#private` member) is now inlinable: `this`
    /// is rewritten into a leading parameter instead of rejecting the method outright.
    #[test]
    fn rewrites_this_into_a_leading_parameter_instead_of_rejecting() {
        let src = "class T { get(obj, key) { return this.helper(obj, key); } }";
        let methods = extract_tenant_methods(src);
        let get = methods
            .get("get")
            .expect("method referencing `this` (no #private) should now be inlinable");
        assert_eq!(
            get.params,
            vec!["__this", "obj", "key"],
            "got: {:?}",
            get.params
        );
        assert!(
            get.body_block.contains("__this.helper(obj, key)"),
            "got: {}",
            get.body_block
        );
        assert!(
            !get.body_block.contains("return this."),
            "no bare `this` reference should remain, got: {}",
            get.body_block
        );
    }

    /// A method calling a captured external identifier (a free function it has no access
    /// to at the JIT's splice site) is rejected — distinct from the private-field
    /// rejection case above.
    #[test]
    fn rejects_methods_calling_a_captured_external_function() {
        let src = "class T { get(obj, key) { return invokeTrap(obj, key); } }";
        let methods = extract_tenant_methods(src);
        assert!(
            !methods.contains_key("get"),
            "method calling a captured external function should not be inlinable"
        );
    }

    /// A method calling a JS global (not captured/external in any meaningful sense) is
    /// still inlinable.
    #[test]
    fn allows_methods_calling_known_globals() {
        let src = "class T { ownKeys(obj) { return Reflect.ownKeys(obj); } }";
        let methods = extract_tenant_methods(src);
        assert!(
            methods.contains_key("ownKeys"),
            "method calling Reflect (a known global) should be inlinable"
        );
    }
}
