//! Free-identifier global resolution, as an AST pass run before TAC conversion.
//!
//! The TAC represents every identifier as an anonymous register, and `optimize_tfunc`'s
//! SSA round-trip mangles all names (`assert` → `$k1p0`), so by the time the frontend
//! lowers to bytecode, "read of an undeclared variable" and "read of a local" are
//! indistinguishable — and both became reads of never-written state slots (`undefined`).
//! The only name-preserving point in the pipeline is *before* `TFunc::try_from`, so this
//! pass rewrites free-identifier reads at the AST level:
//!
//! - `assert` (a read of an undeclared name) → `globalThis["assert"]` — the string
//!   literal key survives every later stage, so the member read lowers to a tenant-
//!   mediated `GET` of the realm global's property.
//! - `globalThis` (itself undeclared) → the same identifier with a dedicated
//!   [`SyntaxContext`] mark. The mark survives the SSA mangle (names don't), and the
//!   bytecode lowering recognizes it to emit the `GLOBAL` opcode instead of a slot read.
//!
//! **The scope analysis is deliberately conservative.** It runs at parse time, before
//! any hygiene exists, and unions *every* declaration in the program into one textual
//! set: a name declared anywhere is never rewritten anywhere. That can only
//! *under*-rewrite (a same-named local shadowing a global keeps the old unwritten-slot
//! behavior — no regression, and never a misresolution of a local as a global). This is
//! the SWC-hygiene rule's inverse case handled the same way in spirit: distinct bindings
//! that share text are never *merged upward* into a global; they stay slot-local.

use std::collections::HashSet;

use swc_atoms::Atom;
use swc_common::{Mark, SyntaxContext};
use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

/// What the pass leaves behind for the lowering: the mark identifying "the realm global
/// object" idents, or `None` when the program references no globals at all.
pub struct GlobalResolution {
    pub ctxt: SyntaxContext,
}

/// Resolve free-identifier reads in `stmts`. Returns the mark the lowering must
/// recognize, if any global reference was introduced (or `globalThis` itself read).
pub fn resolve_globals(stmts: &mut Vec<Stmt>) -> Option<GlobalResolution> {
    let mut collector = DeclCollector::default();
    stmts.visit_with(&mut collector);

    let mark = Mark::fresh(Mark::root());
    let ctxt = SyntaxContext::empty().apply_mark(mark);
    let mut rewriter = GlobalRewriter {
        declared: &collector.names,
        ctxt,
        used: false,
    };
    stmts.visit_mut_with(&mut rewriter);
    rewriter.used.then_some(GlobalResolution { ctxt })
}

/// Conservative union of every declared name in the program. Textual on purpose — see
/// the module comment.
#[derive(Default)]
struct DeclCollector {
    names: HashSet<Atom>,
}

fn collect_pat(pat: &Pat, names: &mut HashSet<Atom>) {
    match pat {
        Pat::Ident(b) => {
            names.insert(b.id.sym.clone());
        }
        Pat::Array(a) => {
            for el in a.elems.iter().flatten() {
                collect_pat(el, names);
            }
        }
        Pat::Object(o) => {
            for p in &o.props {
                match p {
                    ObjectPatProp::KeyValue(kv) => collect_pat(&kv.value, names),
                    ObjectPatProp::Assign(a) => {
                        names.insert(a.key.id.sym.clone());
                    }
                    ObjectPatProp::Rest(r) => collect_pat(&r.arg, names),
                }
            }
        }
        Pat::Rest(r) => collect_pat(&r.arg, names),
        Pat::Assign(a) => collect_pat(&a.left, names),
        Pat::Invalid(_) | Pat::Expr(_) => {}
    }
}

impl Visit for DeclCollector {
    fn visit_var_declarator(&mut self, d: &VarDeclarator) {
        collect_pat(&d.name, &mut self.names);
        d.visit_children_with(self);
    }
    fn visit_fn_decl(&mut self, f: &FnDecl) {
        self.names.insert(f.ident.sym.clone());
        f.visit_children_with(self);
    }
    fn visit_class_decl(&mut self, c: &ClassDecl) {
        self.names.insert(c.ident.sym.clone());
        c.visit_children_with(self);
    }
    fn visit_param(&mut self, p: &Param) {
        collect_pat(&p.pat, &mut self.names);
        p.visit_children_with(self);
    }
    fn visit_catch_clause(&mut self, c: &CatchClause) {
        if let Some(param) = &c.param {
            collect_pat(param, &mut self.names);
        }
        c.visit_children_with(self);
    }
    fn visit_import_decl(&mut self, i: &ImportDecl) {
        for s in &i.specifiers {
            self.names.insert(s.local().sym.clone());
        }
    }
}

struct GlobalRewriter<'a> {
    declared: &'a HashSet<Atom>,
    ctxt: SyntaxContext,
    used: bool,
}

impl GlobalRewriter<'_> {
    /// Names that are implicitly bound in any function scope (`arguments`) or get their
    /// own unsupported-construct handling elsewhere (`eval` as a call target).
    fn is_exempt(&self, sym: &Atom) -> bool {
        matches!(&**sym, "arguments" | "eval")
    }

    fn global_member(&self, span: swc_common::Span, sym: &Atom) -> Expr {
        Expr::Member(MemberExpr {
            span,
            obj: Box::new(Expr::Ident(Ident::new("globalThis".into(), span, self.ctxt))),
            prop: MemberProp::Computed(ComputedPropName {
                span,
                expr: Box::new(Expr::Lit(Lit::Str(Str {
                    span,
                    value: sym.clone().into(),
                    raw: None,
                }))),
            }),
        })
    }
}

impl VisitMut for GlobalRewriter<'_> {
    fn visit_mut_expr(&mut self, e: &mut Expr) {
        if let Expr::Ident(id) = e {
            let sym = id.sym.clone();
            if &*sym == "globalThis" && !self.declared.contains(&sym) {
                id.ctxt = self.ctxt;
                self.used = true;
                return;
            }
            if !self.declared.contains(&sym) && !self.is_exempt(&sym) {
                let span = id.span;
                *e = self.global_member(span, &sym);
                self.used = true;
                return;
            }
        }
        e.visit_mut_children_with(self);
    }

    fn visit_mut_prop(&mut self, p: &mut Prop) {
        // `{assert}` shorthand reads the variable — expand so the read can be rewritten;
        // the property *name* stays the original text.
        if let Prop::Shorthand(id) = p {
            let sym = id.sym.clone();
            if !self.declared.contains(&sym) && !self.is_exempt(&sym) && &*sym != "globalThis" {
                let span = id.span;
                let key = PropName::Ident(IdentName::from(id.clone()));
                *p = Prop::KeyValue(KeyValueProp {
                    key,
                    value: Box::new(self.global_member(span, &sym)),
                });
                self.used = true;
                return;
            }
        }
        p.visit_mut_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(src: &str) -> (Vec<Stmt>, Option<GlobalResolution>) {
        swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
            let mut stmts = crate::parse_script(src).unwrap();
            let r = resolve_globals(&mut stmts);
            (stmts, r)
        })
    }

    fn has_member(stmts: &[Stmt], name: &str) -> bool {
        let mut found = false;
        struct F<'a> {
            name: &'a str,
            found: bool,
        }
        impl Visit for F<'_> {
            fn visit_computed_prop_name(&mut self, c: &ComputedPropName) {
                if let Expr::Lit(Lit::Str(s)) = &*c.expr {
                    if &*s.value == self.name {
                        self.found = true;
                    }
                }
                c.visit_children_with(self);
            }
        }
        let mut f = F { name, found: false };
        stmts.visit_with(&mut f);
        found = f.found;
        found
    }

    #[test]
    fn free_ident_becomes_global_member() {
        let (stmts, r) = rewrite("assert(1);");
        assert!(r.is_some());
        assert!(has_member(&stmts, "assert"));
    }

    #[test]
    fn declared_names_are_not_rewritten() {
        let (stmts, _) = rewrite("var assert = 1; return assert;");
        assert!(!has_member(&stmts, "assert"));
    }

    #[test]
    fn params_shadow_globally() {
        let (stmts, _) = rewrite("var f = function(assert) { return assert; };");
        assert!(!has_member(&stmts, "assert"));
    }

    #[test]
    fn global_this_is_marked_not_wrapped() {
        let (stmts, r) = rewrite("return globalThis;");
        let r = r.expect("global read");
        let mut found = false;
        struct F {
            ctxt: SyntaxContext,
            found: bool,
        }
        impl Visit for F {
            fn visit_ident(&mut self, i: &Ident) {
                if &*i.sym == "globalThis" && i.ctxt == self.ctxt {
                    self.found = true;
                }
            }
        }
        let mut f = F { ctxt: r.ctxt, found: false };
        stmts.visit_with(&mut f);
        found = f.found;
        assert!(found);
    }
}
