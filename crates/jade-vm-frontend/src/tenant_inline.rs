//! Extract inlinable method bodies from a tenant implementation's own source text.
//!
//! See `docs/pluggable-tenant-interface-plan.md` for the full design this supports: the
//! JIT (`crates/jade-vm-jit`) can splice a tenant method's *actual current* body directly
//! into compiled code instead of emitting a `tenant.<method>(...)` call, but only when the
//! method doesn't reference any `#private` field (those are only accessible from code
//! lexically nested inside the declaring class, so splicing the body out would break them).
//!
//! `tenant_source` is expected to be a class's own source text, as returned by
//! `SomeTenantClass.toString()` in a real browser (see the doc above for why this is
//! browser-only in practice) — a plain object's `.toString()` doesn't return its own
//! declaration source the way a function/class's does, so a class expression/declaration
//! is the only shape this needs to handle.

use std::collections::HashMap;

use swc_common::sync::Lrc;
use swc_common::{FileName, SourceMap};
use swc_ecma_ast::{Class, ClassMember, Expr, Pat, PrivateName};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax};
use swc_ecma_visit::{Visit, VisitWith};

/// The tenant interface methods the JIT knows how to call — see `TenantInterface` in
/// `packages/jade-js` (per `docs/pluggable-tenant-interface-plan.md`).
pub const TENANT_METHOD_NAMES: &[&str] = &["make", "get", "set", "define", "assign", "ownKeys"];

/// One method extracted from a tenant's source, ready to be spliced into generated code
/// as `(function(${params.join(",")})${body_block})(${args...})`.
#[derive(Debug, Clone)]
pub struct InlinableTenantMethod {
    /// Parameter names, in declaration order. Only methods whose every parameter is a
    /// plain identifier (no destructuring, defaults, or rest) are extracted at all.
    pub params: Vec<String>,
    /// The exact original source text of the method's `{ ... }` body, byte-sliced
    /// straight out of `tenant_source` (not re-serialized), braces included.
    pub body_block: String,
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

/// Parse `source` (a tenant class's own `.toString()` output) and extract the subset of
/// `TENANT_METHOD_NAMES` that are safe to inline: present, with only plain-identifier
/// parameters, and with no `#private` member reference anywhere in the body.
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
            let lo = (body.span.lo.0 - base_pos.0) as usize;
            let hi = (body.span.hi.0 - base_pos.0) as usize;
            let Some(body_block) = source.get(lo..hi) else {
                continue;
            };
            out.insert(
                name.sym.to_string(),
                InlinableTenantMethod { params, body_block: body_block.to_string() },
            );
        }
    });
    out
}

fn simple_param_names(params: &[swc_ecma_ast::Param]) -> Option<Vec<String>> {
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
    let fm = cm.new_source_file(Lrc::new(FileName::Custom("tenant.js".into())), source.to_string());
    let base_pos = fm.start_pos;
    let lexer = Lexer::new(Syntax::Es(Default::default()), Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let expr = parser.parse_expr().ok()?;
    match *expr {
        Expr::Class(class_expr) => Some((*class_expr.class, base_pos)),
        _ => None,
    }
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
        assert!(get.body_block.contains("obj.get(key)"), "got: {}", get.body_block);
        assert!(methods.contains_key("make"));
    }

    #[test]
    fn rejects_methods_referencing_private_fields() {
        let src = "class T { get(obj, key) { return this.#slots.get(key); } }";
        let methods = extract_tenant_methods(src);
        assert!(!methods.contains_key("get"), "method using #private should not be inlinable");
    }

    #[test]
    fn rejects_methods_with_non_ident_params() {
        let src = "class T { set(obj, { key, value }) { } }";
        let methods = extract_tenant_methods(src);
        assert!(!methods.contains_key("set"), "destructured params should not be inlinable");
    }

    #[test]
    fn ignores_unrecognized_methods_and_non_class_input() {
        assert!(extract_tenant_methods("class T { helper() {} }").is_empty());
        assert!(extract_tenant_methods("not valid js class {{{").is_empty());
        assert!(extract_tenant_methods("({ get(o,k) { return o.get(k); } })").is_empty());
    }
}
