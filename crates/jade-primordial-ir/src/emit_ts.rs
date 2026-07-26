//! IR -> TypeScript re-emission, used as a losslessness/verification check (the hand-written
//! `packages/jade-js/primordials/*.ts` remains the canonical source; this is never checked in
//! as a replacement — see the primordial-IR plan's "Package layout" section).
//!
//! Emits source text directly rather than rebuilding `swc_ecma_ast` nodes and going through
//! `swc_ecma_codegen`. This mirrors existing precedent in this codebase: `jade-vm-jit`'s Tier 0
//! and Tier 1 backends already emit textual JavaScript directly (`ops_to_js`); `jade-vm-jit-swc`
//! only re-parses generated text into real AST nodes when it specifically needs `swc-cfg`
//! structure for further passes, which nothing downstream of this emitter needs — the output
//! only has to be valid TypeScript that `tsc`/Node's TS-stripping mode can run, not feed a
//! further Rust-side structural pass.

use crate::ir::*;

pub fn emit_module(module: &Module) -> String {
    let mut out = String::new();
    for item in &module.items {
        emit_item(&mut out, item, 0);
        out.push('\n');
    }
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn emit_item(out: &mut String, item: &Item, level: usize) {
    match item {
        Item::TypeImport { source, names } => {
            out.push_str(&format!("import type {{ {} }} from \"{source}\";\n", names.join(", ")));
        }
        Item::ValueImport { source, names } => {
            out.push_str(&format!("import {{ {} }} from \"{source}\";\n", names.join(", ")));
        }
        Item::StructDef(_) => {
            // Interfaces are erased from the IR and re-declared from the original source text
            // by the caller, not round-tripped through this emitter.
        }
        Item::PerTenantCache { name, value_ty } => {
            out.push_str(&format!("const {name} = new WeakMap<Tenant, {value_ty}>();\n"));
        }
        Item::ModuleConst { name, mutable, init } => {
            let kw = if *mutable { "let" } else { "const" };
            out.push_str(&format!("{kw} {name} = "));
            emit_expr(out, init, level);
            out.push_str(";\n");
        }
        Item::FnDecl(func) => {
            emit_fn_decl(out, func, level);
            out.push('\n');
        }
    }
}

fn emit_fn_decl(out: &mut String, func: &FnDecl, level: usize) {
    let star = if func.is_generator { "*" } else { "" };
    let name = func.name.as_deref().unwrap_or("");
    out.push_str(&format!("function{star} {name}("));
    emit_params(out, &func.params);
    out.push(')');
    if let Some(ret) = &func.return_type {
        out.push_str(": ");
        out.push_str(&emit_type_ref(ret));
    }
    out.push_str(" {\n");
    emit_block(out, &func.body, level + 1);
    indent(out, level);
    out.push('}');
}

fn emit_params(out: &mut String, params: &[Param]) {
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        emit_pattern(out, &param.pattern);
        if let Some(ty) = &param.ty {
            out.push_str(": ");
            out.push_str(&emit_type_ref(ty));
        }
        if let Some(default) = &param.default {
            out.push_str(" = ");
            emit_expr(out, default, 0);
        }
    }
}

/// Re-renders a [`TypeRef`] as TS syntax — a straightforward pass-through, since every
/// `TypeRef` was itself lowered from exactly this kind of annotation (see `lower_ts_type` in
/// `lower.rs`).
fn emit_type_ref(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Named(name) => name.clone(),
        TypeRef::Generic { name, args } => {
            let args = args.iter().map(emit_type_ref).collect::<Vec<_>>().join(", ");
            format!("{name}<{args}>")
        }
        TypeRef::Optional(inner) => format!("{} | null", emit_type_ref(inner)),
        TypeRef::Array(inner) => format!("{}[]", emit_type_ref(inner)),
    }
}

fn emit_pattern(out: &mut String, pattern: &Pattern) {
    match pattern {
        Pattern::Ident(name) => out.push_str(name),
        Pattern::ObjectShallow(bindings) => {
            out.push_str("{ ");
            for (i, binding) in bindings.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if binding.key == binding.binding {
                    out.push_str(&binding.key);
                } else {
                    out.push_str(&format!("{}: {}", binding.key, binding.binding));
                }
            }
            out.push_str(" }");
        }
    }
}

fn emit_block(out: &mut String, block: &Block, level: usize) {
    for stmt in &block.0 {
        emit_stmt(out, stmt, level);
    }
}

fn emit_stmt(out: &mut String, stmt: &Stmt, level: usize) {
    indent(out, level);
    match stmt {
        Stmt::Let { pattern, init } => {
            out.push_str("const ");
            emit_pattern(out, pattern);
            if let Some(init) = init {
                out.push_str(" = ");
                emit_expr(out, init, level);
            }
            out.push_str(";\n");
        }
        Stmt::Expr(expr) => {
            emit_expr(out, expr, level);
            out.push_str(";\n");
        }
        Stmt::Return(value) => {
            out.push_str("return");
            if let Some(value) = value {
                out.push(' ');
                emit_expr(out, value, level);
            }
            out.push_str(";\n");
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            out.push_str("if (");
            emit_expr(out, cond, level);
            out.push_str(") {\n");
            emit_block(out, then_branch, level + 1);
            indent(out, level);
            out.push('}');
            if let Some(else_branch) = else_branch {
                out.push_str(" else {\n");
                emit_block(out, else_branch, level + 1);
                indent(out, level);
                out.push('}');
            }
            out.push('\n');
        }
        Stmt::ForOf { binding, iter, body } => {
            out.push_str("for (const ");
            emit_pattern(out, binding);
            out.push_str(" of ");
            emit_expr(out, iter, level);
            out.push_str(") {\n");
            emit_block(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::ForCounting {
            binding,
            start,
            bound,
            body,
        } => {
            out.push_str(&format!("for (let {binding} = "));
            emit_expr(out, start, level);
            out.push_str(&format!("; {binding} < "));
            emit_expr(out, bound, level);
            out.push_str(&format!("; {binding}++) {{\n"));
            emit_block(out, body, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::TryCatch {
            try_block,
            catch_param,
            catch_block,
        } => {
            out.push_str("try {\n");
            emit_block(out, try_block, level + 1);
            indent(out, level);
            out.push_str("} catch ");
            if let Some(param) = catch_param {
                out.push_str(&format!("({param}) "));
            }
            out.push_str("{\n");
            emit_block(out, catch_block, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Throw(expr) => {
            out.push_str("throw ");
            emit_expr(out, expr, level);
            out.push_str(";\n");
        }
    }
}

fn emit_expr(out: &mut String, expr: &Expr, level: usize) {
    match expr {
        Expr::Ident(name) => out.push_str(name),
        Expr::ThisArg => out.push_str("this"),
        Expr::Lit(lit) => emit_lit(out, lit),
        Expr::TemplateLiteral(parts) => {
            out.push('`');
            for part in parts {
                match part {
                    TemplatePart::Str(s) => out.push_str(s),
                    TemplatePart::Expr(e) => {
                        out.push_str("${");
                        emit_expr(out, e, level);
                        out.push('}');
                    }
                }
            }
            out.push('`');
        }
        Expr::Array(elements) => {
            out.push('[');
            for (i, el) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                match el {
                    ArrayElement::Normal(e) => emit_expr(out, e, level),
                    ArrayElement::Spread(e) => {
                        out.push_str("...");
                        emit_expr(out, e, level);
                    }
                }
            }
            out.push(']');
        }
        Expr::Object(props) => {
            out.push_str("{ ");
            for (i, prop) in props.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                match prop {
                    ObjectProp::KeyValue { key, value } => {
                        emit_prop_key(out, key);
                        out.push_str(": ");
                        emit_expr(out, value, level);
                    }
                    ObjectProp::Method { key, func } => {
                        if func.is_generator {
                            out.push('*');
                        }
                        emit_prop_key(out, key);
                        out.push('(');
                        emit_params(out, &func.params);
                        out.push_str(") {\n");
                        emit_block(out, &func.body, level + 1);
                        indent(out, level);
                        out.push('}');
                    }
                    ObjectProp::Spread(e) => {
                        out.push_str("...");
                        emit_expr(out, e, level);
                    }
                }
            }
            out.push_str(" }");
        }
        Expr::Member { obj, prop } => {
            emit_expr(out, obj, level);
            match prop {
                MemberProp::Ident(name) => out.push_str(&format!(".{name}")),
                MemberProp::Computed(e) => {
                    out.push('[');
                    emit_expr(out, e, level);
                    out.push(']');
                }
            }
        }
        Expr::Call { callee, args } => {
            emit_expr(out, callee, level);
            out.push('(');
            emit_call_args(out, args, level);
            out.push(')');
        }
        Expr::New { callee, args } => {
            out.push_str("new ");
            emit_expr(out, callee, level);
            out.push('(');
            emit_call_args(out, args, level);
            out.push(')');
        }
        Expr::Closure(func) => {
            out.push_str("function");
            if func.is_generator {
                out.push('*');
            }
            out.push_str(" (");
            emit_params(out, &func.params);
            out.push_str(") {\n");
            emit_block(out, &func.body, level + 1);
            indent(out, level);
            out.push('}');
        }
        Expr::TenantYield(inner) => {
            out.push_str("yield tenant.yieldTenant(");
            emit_expr(out, inner, level);
            out.push(')');
        }
        Expr::Bin { op, lhs, rhs } => {
            emit_expr(out, lhs, level);
            out.push_str(&format!(" {} ", bin_op_str(*op)));
            emit_expr(out, rhs, level);
        }
        Expr::Un { op, arg } => {
            out.push_str(un_op_str(*op));
            emit_expr(out, arg, level);
        }
        Expr::Cond { test, cons, alt } => {
            emit_expr(out, test, level);
            out.push_str(" ? ");
            emit_expr(out, cons, level);
            out.push_str(" : ");
            emit_expr(out, alt, level);
        }
        Expr::Assign { target, value } => {
            emit_expr(out, target, level);
            out.push_str(" = ");
            emit_expr(out, value, level);
        }
        Expr::Spread(inner) => {
            out.push_str("...");
            emit_expr(out, inner, level);
        }
        Expr::Paren(inner) => {
            out.push('(');
            emit_expr(out, inner, level);
            out.push(')');
        }
        Expr::HostIntrinsic { name, args } => emit_host_intrinsic(out, name, args, level),
    }
}

fn emit_call_args(out: &mut String, args: &[CallArg], level: usize) {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        match arg {
            CallArg::Normal(e) => emit_expr(out, e, level),
            CallArg::Spread(e) => {
                out.push_str("...");
                emit_expr(out, e, level);
            }
        }
    }
}

fn emit_prop_key(out: &mut String, key: &PropKey) {
    match key {
        PropKey::Ident(name) => out.push_str(name),
        PropKey::Computed(e) => {
            out.push('[');
            emit_expr(out, e, 0);
            out.push(']');
        }
    }
}

fn emit_lit(out: &mut String, lit: &Lit) {
    match lit {
        Lit::Str(s) => out.push_str(&format!("{:?}", s)),
        Lit::Num(n) => out.push_str(&n.to_string()),
        Lit::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Lit::Null => out.push_str("null"),
        Lit::Undefined => out.push_str("undefined"),
    }
}

fn bin_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Mod => "%",
        BinOp::Eq => "===",
        BinOp::NotEq => "!==",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::Nullish => "??",
        BinOp::In => "in",
    }
}

fn un_op_str(op: UnOp) -> &'static str {
    match op {
        UnOp::Not => "!",
        UnOp::Neg => "-",
        UnOp::TypeOf => "typeof ",
    }
}

/// Recognized intrinsics re-emit as their original JS spelling — trivial, since each one was
/// recognized from exactly this shape during lowering (see `lower.rs`).
fn emit_host_intrinsic(out: &mut String, name: &str, args: &[Expr], level: usize) {
    match name {
        crate::intrinsics::IS_ARRAY_INDEX_STRING => {
            out.push_str("/^(0|[1-9][0-9]*)$/.test(");
            emit_expr(out, &args[0], level);
            out.push(')');
        }
        crate::intrinsics::IS_STRING_KEY => {
            out.push_str("typeof ");
            emit_expr(out, &args[0], level);
            out.push_str(" === \"string\"");
        }
        crate::intrinsics::TO_NUMBER => {
            out.push_str("Number(");
            emit_expr(out, &args[0], level);
            out.push(')');
        }
        crate::intrinsics::TO_STRING => {
            out.push_str("String(");
            emit_expr(out, &args[0], level);
            out.push(')');
        }
        crate::intrinsics::MAP_GET | crate::intrinsics::MAP_SET | crate::intrinsics::MAP_HAS | crate::intrinsics::MAP_DELETE
        | crate::intrinsics::ARRAY_FILTER | crate::intrinsics::ARRAY_SORT_BY | crate::intrinsics::ARRAY_PUSH
        | crate::intrinsics::ARRAY_SLICE_FROM => {
            let method = match name {
                x if x == crate::intrinsics::MAP_GET => "get",
                x if x == crate::intrinsics::MAP_SET => "set",
                x if x == crate::intrinsics::MAP_HAS => "has",
                x if x == crate::intrinsics::MAP_DELETE => "delete",
                x if x == crate::intrinsics::ARRAY_FILTER => "filter",
                x if x == crate::intrinsics::ARRAY_SORT_BY => "sort",
                x if x == crate::intrinsics::ARRAY_PUSH => "push",
                x if x == crate::intrinsics::ARRAY_SLICE_FROM => "slice",
                _ => unreachable!(),
            };
            emit_expr(out, &args[0], level);
            out.push_str(&format!(".{method}("));
            for (i, arg) in args[1..].iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, arg, level);
            }
            out.push(')');
        }
        crate::intrinsics::NUMBER_IS_INTEGER => {
            out.push_str("Number.isInteger(");
            emit_expr(out, &args[0], level);
            out.push(')');
        }
        crate::intrinsics::MATH_MIN => {
            out.push_str("Math.min(");
            emit_expr(out, &args[0], level);
            out.push_str(", ");
            emit_expr(out, &args[1], level);
            out.push(')');
        }
        crate::intrinsics::MATH_MAX => {
            out.push_str("Math.max(");
            emit_expr(out, &args[0], level);
            out.push_str(", ");
            emit_expr(out, &args[1], level);
            out.push(')');
        }
        crate::intrinsics::MATH_ROUND => {
            out.push_str("Math.round(");
            emit_expr(out, &args[0], level);
            out.push(')');
        }
        other => out.push_str(&format!("/* unrecognized intrinsic {other} */")),
    }
}
