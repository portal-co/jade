//! `swc_ecma_ast` -> [`crate::ir`] lowering, with hard-reject validation. Follows the same
//! "parse via `swc_ecma_parser`, walk the AST, fail loud on anything outside the recognized
//! subset" philosophy as `crates/jade-vm-frontend/src/tenant_inline.rs`'s
//! `extract_tenant_methods` — see that module's doc comment for the precedent this follows.
//!
//! Coverage is intentionally the closed construct set surveyed in `packages/jade-js/
//! primordials/*.ts` (see `ir.rs`'s module doc comment), not general TypeScript. A source
//! construct outside that set produces `IrError::Unsupported`, never a best-effort guess.

use swc_common::comments::NoopComments;
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast as ast;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};

use crate::intrinsics;
use crate::ir::*;
use crate::IrError;

/// `Str`/`TplElement` values are `Wtf8Atom` (WTF-8, to allow unpaired surrogates), not a plain
/// UTF-8 string type — this lossily converts to an ordinary Rust `String`, which is fine for
/// the primordials survey (no unpaired-surrogate string literals appear in that source).
fn atom_string(atom: &swc_atoms::Wtf8Atom) -> String {
    atom.as_wtf8().to_string_lossy().into_owned()
}

pub fn lower_module(file_name: &str, source: &str) -> Result<Module, IrError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Custom(file_name.to_string())), source.to_string());
    let syntax = Syntax::Typescript(TsSyntax {
        tsx: false,
        decorators: false,
        ..Default::default()
    });
    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let module = parser
        .parse_typescript_module()
        .map_err(|e| IrError::Parse(format!("{file_name}: {e:?}")))?;

    let mut items = Vec::new();
    for item in &module.body {
        if let Some(lowered) = lower_module_item(file_name, item)? {
            items.push(lowered);
        }
    }
    Ok(Module { items })
}

fn unsupported(file: &str, construct: impl Into<String>) -> IrError {
    IrError::Unsupported {
        file: file.to_string(),
        construct: construct.into(),
    }
}

fn lower_module_item(file: &str, item: &ast::ModuleItem) -> Result<Option<Item>, IrError> {
    match item {
        ast::ModuleItem::ModuleDecl(decl) => lower_module_decl(file, decl),
        ast::ModuleItem::Stmt(stmt) => lower_top_level_stmt(file, stmt),
    }
}

fn import_names(specifiers: &[ast::ImportSpecifier]) -> Vec<String> {
    specifiers
        .iter()
        .filter_map(|spec| match spec {
            ast::ImportSpecifier::Named(named) => Some(named.local.sym.to_string()),
            ast::ImportSpecifier::Default(default) => Some(default.local.sym.to_string()),
            ast::ImportSpecifier::Namespace(ns) => Some(ns.local.sym.to_string()),
        })
        .collect()
}

fn lower_module_decl(file: &str, decl: &ast::ModuleDecl) -> Result<Option<Item>, IrError> {
    match decl {
        ast::ModuleDecl::Import(import) => {
            let source = atom_string(&import.src.value);
            let names = import_names(&import.specifiers);
            Ok(Some(if import.type_only {
                Item::TypeImport { source, names }
            } else {
                Item::ValueImport { source, names }
            }))
        }
        ast::ModuleDecl::ExportDecl(export) => lower_decl(file, &export.decl),
        other => Err(unsupported(file, format!("module declaration {other:?}"))),
    }
}

fn lower_top_level_stmt(file: &str, stmt: &ast::Stmt) -> Result<Option<Item>, IrError> {
    match stmt {
        ast::Stmt::Decl(decl) => lower_decl(file, decl),
        other => Err(unsupported(file, format!("top-level statement {other:?}"))),
    }
}

fn lower_decl(file: &str, decl: &ast::Decl) -> Result<Option<Item>, IrError> {
    match decl {
        // Type-only declarations are erased entirely — TS emission re-declares the original
        // source's own interfaces/type aliases verbatim instead of round-tripping them through
        // the IR (see `emit_ts.rs`).
        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_) => Ok(None),
        ast::Decl::Fn(fn_decl) => {
            let name = fn_decl.ident.sym.to_string();
            let func = lower_function(file, &fn_decl.function, Some(name.clone()))?;
            Ok(Some(Item::FnDecl(func)))
        }
        ast::Decl::Var(var_decl) => lower_module_var_decl(file, var_decl),
        other => Err(unsupported(file, format!("declaration {other:?}"))),
    }
}

/// Recognizes the `const cache = new WeakMap<Tenant, X>();` per-tenant-cache idiom specially
/// (see `ir.rs`'s `Item::PerTenantCache` doc comment); any other module-level const/let becomes
/// a plain `Item::ModuleConst`.
fn lower_module_var_decl(file: &str, var_decl: &ast::VarDecl) -> Result<Option<Item>, IrError> {
    if var_decl.decls.len() != 1 {
        return Err(unsupported(file, "module-level var decl with != 1 declarator"));
    }
    let declarator = &var_decl.decls[0];
    let name = match &declarator.name {
        ast::Pat::Ident(id) => id.id.sym.to_string(),
        other => return Err(unsupported(file, format!("module-level destructuring {other:?}"))),
    };
    let Some(init) = &declarator.init else {
        return Err(unsupported(file, "module-level var decl with no initializer"));
    };
    if let ast::Expr::New(new_expr) = init.as_ref()
        && let ast::Expr::Ident(callee) = new_expr.callee.as_ref()
        && callee.sym.as_ref() == "WeakMap"
    {
        // The per-tenant cache idiom is always `new WeakMap<Tenant, ValueType>()` with no
        // constructor arguments; the cached value type is carried purely in the TS type
        // arguments (erased at runtime), so record it as an opaque placeholder name — the
        // generated Rust struct field's real type comes from the enclosing primordial's own
        // factory function return type, resolved by `emit_rust.rs`, not from this node.
        return Ok(Some(Item::PerTenantCache {
            name,
            value_ty: "Cached".to_string(),
        }));
    }
    let mutable = matches!(var_decl.kind, ast::VarDeclKind::Let | ast::VarDeclKind::Var);
    let value = lower_expr(file, init)?;
    Ok(Some(Item::ModuleConst {
        name,
        mutable,
        init: value,
    }))
}

fn lower_function(file: &str, function: &ast::Function, name: Option<String>) -> Result<FnDecl, IrError> {
    if function.is_async {
        return Err(unsupported(file, "async function (guest-visible async is host-only)"));
    }
    let params = function
        .params
        .iter()
        .map(|p| lower_param(file, p))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(body) = &function.body else {
        return Err(unsupported(file, "function with no body"));
    };
    let stmts = lower_block(file, body)?;
    let return_type = function
        .return_type
        .as_deref()
        .map(|ann| lower_ts_type(file, &ann.type_ann))
        .transpose()?;
    Ok(FnDecl {
        name,
        is_generator: function.is_generator,
        params,
        body: stmts,
        captures: Vec::new(),
        return_type,
    })
}

fn lower_param(file: &str, param: &ast::Param) -> Result<Param, IrError> {
    lower_pat_as_param(file, &param.pat)
}

fn lower_pat_as_param(file: &str, pat: &ast::Pat) -> Result<Param, IrError> {
    match pat {
        ast::Pat::Ident(id) => Ok(Param {
            pattern: Pattern::Ident(id.id.sym.to_string()),
            default: None,
            ty: id.type_ann.as_deref().map(|ann| lower_ts_type(file, &ann.type_ann)).transpose()?,
        }),
        ast::Pat::Object(obj) => {
            let mut bindings = Vec::new();
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => {
                        let key = assign.key.sym.to_string();
                        bindings.push(ShallowBinding {
                            key: key.clone(),
                            binding: key,
                        });
                    }
                    other => return Err(unsupported(file, format!("destructuring pattern {other:?}"))),
                }
            }
            Ok(Param {
                pattern: Pattern::ObjectShallow(bindings),
                default: None,
                ty: None,
            })
        }
        ast::Pat::Assign(assign) => {
            let mut inner = lower_pat_as_param(file, &assign.left)?;
            inner.default = Some(lower_expr(file, &assign.right)?);
            Ok(inner)
        }
        other => Err(unsupported(file, format!("parameter pattern {other:?}"))),
    }
}

/// Lowers a TS type annotation into a [`TypeRef`]. Deliberately shallow — only the shapes
/// actually used in parameter/return position across the surveyed source (named types,
/// single-type-argument generics, `T | null`/`T | undefined`, `T[]`) are recognized; anything
/// else is a hard rejection rather than a guess.
fn lower_ts_type(file: &str, ty: &ast::TsType) -> Result<TypeRef, IrError> {
    match ty {
        ast::TsType::TsKeywordType(kw) => Ok(TypeRef::Named(ts_keyword_name(kw.kind).to_string())),
        ast::TsType::TsTypeRef(type_ref) => {
            let name = match &type_ref.type_name {
                ast::TsEntityName::Ident(id) => id.sym.to_string(),
                ast::TsEntityName::TsQualifiedName(qualified) => qualified.right.sym.to_string(),
            };
            match &type_ref.type_params {
                Some(params) if !params.params.is_empty() => {
                    let args = params
                        .params
                        .iter()
                        .map(|p| lower_ts_type(file, p))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(TypeRef::Generic { name, args })
                }
                _ => Ok(TypeRef::Named(name)),
            }
        }
        ast::TsType::TsArrayType(array) => Ok(TypeRef::Array(Box::new(lower_ts_type(file, &array.elem_type)?))),
        ast::TsType::TsParenthesizedType(paren) => lower_ts_type(file, &paren.type_ann),
        ast::TsType::TsUnionOrIntersectionType(ast::TsUnionOrIntersectionType::TsUnionType(union)) => {
            let mut non_null: Vec<&ast::TsType> = Vec::new();
            let mut saw_null_ish = false;
            for member in &union.types {
                match member.as_ref() {
                    ast::TsType::TsKeywordType(kw)
                        if matches!(
                            kw.kind,
                            ast::TsKeywordTypeKind::TsNullKeyword | ast::TsKeywordTypeKind::TsUndefinedKeyword
                        ) =>
                    {
                        saw_null_ish = true;
                    }
                    other => non_null.push(other),
                }
            }
            if non_null.len() == 1 && saw_null_ish {
                Ok(TypeRef::Optional(Box::new(lower_ts_type(file, non_null[0])?)))
            } else {
                Err(unsupported(file, format!("union type {union:?}")))
            }
        }
        other => Err(unsupported(file, format!("type annotation {other:?}"))),
    }
}

fn ts_keyword_name(kind: ast::TsKeywordTypeKind) -> &'static str {
    match kind {
        ast::TsKeywordTypeKind::TsVoidKeyword => "void",
        ast::TsKeywordTypeKind::TsBooleanKeyword => "boolean",
        ast::TsKeywordTypeKind::TsStringKeyword => "string",
        ast::TsKeywordTypeKind::TsNumberKeyword => "number",
        ast::TsKeywordTypeKind::TsUnknownKeyword => "unknown",
        ast::TsKeywordTypeKind::TsObjectKeyword => "object",
        ast::TsKeywordTypeKind::TsUndefinedKeyword => "undefined",
        ast::TsKeywordTypeKind::TsNullKeyword => "null",
        ast::TsKeywordTypeKind::TsAnyKeyword => "any",
        _ => "unknown",
    }
}

fn lower_pat_as_pattern(file: &str, pat: &ast::Pat) -> Result<Pattern, IrError> {
    Ok(lower_pat_as_param(file, pat)?.pattern)
}

fn lower_block(file: &str, block: &ast::BlockStmt) -> Result<Block, IrError> {
    let mut out = Vec::new();
    for stmt in &block.stmts {
        out.push(lower_stmt(file, stmt)?);
    }
    Ok(Block(out))
}

fn lower_stmt(file: &str, stmt: &ast::Stmt) -> Result<Stmt, IrError> {
    match stmt {
        ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
            if var_decl.decls.len() != 1 {
                return Err(unsupported(file, "local var decl with != 1 declarator"));
            }
            let declarator = &var_decl.decls[0];
            let pattern = lower_pat_as_pattern(file, &declarator.name)?;
            let init = declarator.init.as_deref().map(|e| lower_expr(file, e)).transpose()?;
            Ok(Stmt::Let { pattern, init })
        }
        ast::Stmt::Expr(expr_stmt) => Ok(Stmt::Expr(lower_expr(file, &expr_stmt.expr)?)),
        ast::Stmt::Return(ret) => Ok(Stmt::Return(
            ret.arg.as_deref().map(|e| lower_expr(file, e)).transpose()?,
        )),
        ast::Stmt::If(if_stmt) => {
            let cond = lower_expr(file, &if_stmt.test)?;
            let then_branch = lower_stmt_as_block(file, &if_stmt.cons)?;
            let else_branch = if_stmt
                .alt
                .as_deref()
                .map(|s| lower_stmt_as_block(file, s))
                .transpose()?;
            Ok(Stmt::If {
                cond,
                then_branch,
                else_branch,
            })
        }
        ast::Stmt::ForOf(for_of) => {
            if for_of.is_await {
                return Err(unsupported(file, "for-await-of"));
            }
            let binding = match &for_of.left {
                ast::ForHead::VarDecl(var_decl) if var_decl.decls.len() == 1 => {
                    lower_pat_as_pattern(file, &var_decl.decls[0].name)?
                }
                other => return Err(unsupported(file, format!("for-of left-hand side {other:?}"))),
            };
            let iter = lower_expr(file, &for_of.right)?;
            let body = lower_stmt_as_block(file, &for_of.body)?;
            Ok(Stmt::ForOf { binding, iter, body })
        }
        ast::Stmt::For(for_stmt) => lower_counting_for(file, for_stmt),
        ast::Stmt::Throw(throw_stmt) => Ok(Stmt::Throw(lower_expr(file, &throw_stmt.arg)?)),
        ast::Stmt::Try(try_stmt) => {
            let try_block = lower_block(file, &try_stmt.block)?;
            let (catch_param, catch_block) = match &try_stmt.handler {
                Some(handler) => {
                    let param = match &handler.param {
                        Some(ast::Pat::Ident(id)) => Some(id.id.sym.to_string()),
                        None => None,
                        other => return Err(unsupported(file, format!("catch parameter {other:?}"))),
                    };
                    (param, lower_block(file, &handler.body)?)
                }
                None => return Err(unsupported(file, "try without catch")),
            };
            if try_stmt.finalizer.is_some() {
                return Err(unsupported(file, "try/finally"));
            }
            Ok(Stmt::TryCatch {
                try_block,
                catch_param,
                catch_block,
            })
        }
        ast::Stmt::Block(block) => {
            // A bare nested block is treated as a single-iteration scope boundary the source
            // never actually depends on for control flow; represent it as an `if (true)` block
            // would be misleading, so instead just splice its statements — none of the
            // surveyed primordials use bare blocks for shadowing, only for real control-flow
            // constructs already covered above.
            return Err(unsupported(file, format!("bare block statement {block:?}")));
        }
        other => Err(unsupported(file, format!("statement {other:?}"))),
    }
}

fn lower_stmt_as_block(file: &str, stmt: &ast::Stmt) -> Result<Block, IrError> {
    match stmt {
        ast::Stmt::Block(block) => lower_block(file, block),
        other => Ok(Block(vec![lower_stmt(file, other)?])),
    }
}

fn lower_counting_for(file: &str, for_stmt: &ast::ForStmt) -> Result<Stmt, IrError> {
    let Some(ast::VarDeclOrExpr::VarDecl(init_decl)) = for_stmt.init.as_ref() else {
        return Err(unsupported(file, "for-loop without a `let i = ...` initializer"));
    };
    if init_decl.decls.len() != 1 {
        return Err(unsupported(file, "counting for-loop with != 1 declarator"));
    }
    let declarator = &init_decl.decls[0];
    let binding = match &declarator.name {
        ast::Pat::Ident(id) => id.id.sym.to_string(),
        other => return Err(unsupported(file, format!("counting for-loop binding {other:?}"))),
    };
    let Some(start) = declarator.init.as_deref().map(|e| lower_expr(file, e)).transpose()? else {
        return Err(unsupported(file, "counting for-loop with no start value"));
    };
    let Some(test) = &for_stmt.test else {
        return Err(unsupported(file, "counting for-loop with no test"));
    };
    // Only `i < BOUND` is recognized — the one shape observed in the source
    // (`typed-arrays.ts`'s `for (let i = 0; i < source.length; i++)`).
    let ast::Expr::Bin(bin) = test.as_ref() else {
        return Err(unsupported(file, "counting for-loop test must be `i < BOUND`"));
    };
    if bin.op != ast::BinaryOp::Lt {
        return Err(unsupported(file, "counting for-loop test must use `<`"));
    }
    let bound = lower_expr(file, &bin.right)?;
    let body = lower_stmt_as_block(file, &for_stmt.body)?;
    Ok(Stmt::ForCounting {
        binding,
        start,
        bound,
        body,
    })
}

fn lower_expr(file: &str, expr: &ast::Expr) -> Result<Expr, IrError> {
    match expr {
        ast::Expr::Ident(id) => Ok(Expr::Ident(id.sym.to_string())),
        ast::Expr::This(_) => Ok(Expr::ThisArg),
        ast::Expr::Lit(lit) => lower_lit(file, lit),
        ast::Expr::Tpl(tpl) => lower_template(file, tpl),
        ast::Expr::Paren(paren) => Ok(Expr::Paren(Box::new(lower_expr(file, &paren.expr)?))),
        ast::Expr::Array(array) => {
            let mut elements = Vec::new();
            for elem in &array.elems {
                match elem {
                    Some(el) if el.spread.is_some() => {
                        elements.push(ArrayElement::Spread(lower_expr(file, &el.expr)?))
                    }
                    Some(el) => elements.push(ArrayElement::Normal(lower_expr(file, &el.expr)?)),
                    None => return Err(unsupported(file, "sparse array literal")),
                }
            }
            Ok(Expr::Array(elements))
        }
        ast::Expr::Object(obj) => lower_object_lit(file, obj),
        ast::Expr::Member(member) => lower_member(file, member),
        ast::Expr::Call(call) => lower_call(file, call),
        ast::Expr::New(new_expr) => {
            let callee = Box::new(lower_expr(file, &new_expr.callee)?);
            let args = new_expr
                .args
                .as_ref()
                .map(|args| lower_call_args(file, args))
                .transpose()?
                .unwrap_or_default();
            Ok(Expr::New { callee, args })
        }
        ast::Expr::Fn(fn_expr) => {
            let name = fn_expr.ident.as_ref().map(|id| id.sym.to_string());
            Ok(Expr::Closure(Box::new(lower_function(file, &fn_expr.function, name)?)))
        }
        ast::Expr::Arrow(arrow) => lower_arrow(file, arrow),
        ast::Expr::Yield(yield_expr) => lower_yield(file, yield_expr),
        ast::Expr::Bin(bin) => lower_bin(file, bin),
        ast::Expr::Unary(unary) => lower_unary(file, unary),
        ast::Expr::Cond(cond) => Ok(Expr::Cond {
            test: Box::new(lower_expr(file, &cond.test)?),
            cons: Box::new(lower_expr(file, &cond.cons)?),
            alt: Box::new(lower_expr(file, &cond.alt)?),
        }),
        ast::Expr::Assign(assign) => lower_assign(file, assign),
        ast::Expr::TsNonNull(inner) => lower_expr(file, &inner.expr),
        ast::Expr::TsAs(inner) => lower_expr(file, &inner.expr),
        ast::Expr::TsConstAssertion(inner) => lower_expr(file, &inner.expr),
        ast::Expr::TsSatisfies(inner) => lower_expr(file, &inner.expr),
        other => Err(unsupported(file, format!("expression {other:?}"))),
    }
}

fn lower_lit(file: &str, lit: &ast::Lit) -> Result<Expr, IrError> {
    match lit {
        ast::Lit::Str(s) => Ok(Expr::Lit(Lit::Str(atom_string(&s.value)))),
        ast::Lit::Num(n) => Ok(Expr::Lit(Lit::Num(n.value))),
        ast::Lit::Bool(b) => Ok(Expr::Lit(Lit::Bool(b.value))),
        ast::Lit::Null(_) => Ok(Expr::Lit(Lit::Null)),
        ast::Lit::Regex(regex) => {
            // Only the single fixed array-index pattern is recognized; anything else calling
            // `.test(...)` on a regex is a hard rejection when the call itself is lowered (see
            // `lower_call`), so a bare regex literal reaching here means it was used somewhere
            // other than that one recognized `.test(...)` call shape.
            if regex.exp.as_ref() == r"^(0|[1-9][0-9]*)$" {
                Ok(Expr::Ident("__ARRAY_INDEX_REGEX__".to_string()))
            } else {
                Err(unsupported(file, format!("regex literal /{}/ not in the recognized intrinsic table", regex.exp)))
            }
        }
        other => Err(unsupported(file, format!("literal {other:?}"))),
    }
}

fn lower_template(file: &str, tpl: &ast::Tpl) -> Result<Expr, IrError> {
    let mut parts = Vec::new();
    for (i, quasi) in tpl.quasis.iter().enumerate() {
        let raw = quasi.cooked.as_ref().map(atom_string).unwrap_or_default();
        if !raw.is_empty() {
            parts.push(TemplatePart::Str(raw));
        }
        if let Some(expr) = tpl.exprs.get(i) {
            parts.push(TemplatePart::Expr(lower_expr(file, expr)?));
        }
    }
    Ok(Expr::TemplateLiteral(parts))
}

fn lower_object_lit(file: &str, obj: &ast::ObjectLit) -> Result<Expr, IrError> {
    let mut props = Vec::new();
    for prop in &obj.props {
        match prop {
            ast::PropOrSpread::Spread(spread) => {
                props.push(ObjectProp::Spread(lower_expr(file, &spread.expr)?));
            }
            ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                ast::Prop::KeyValue(kv) => {
                    let key = lower_prop_name(file, &kv.key)?;
                    let value = lower_expr(file, &kv.value)?;
                    props.push(ObjectProp::KeyValue { key, value });
                }
                ast::Prop::Method(method) => {
                    let key = lower_prop_name(file, &method.key)?;
                    let func = lower_function(file, &method.function, None)?;
                    props.push(ObjectProp::Method { key, func });
                }
                ast::Prop::Shorthand(id) => {
                    props.push(ObjectProp::KeyValue {
                        key: PropKey::Ident(id.sym.to_string()),
                        value: Expr::Ident(id.sym.to_string()),
                    });
                }
                other => return Err(unsupported(file, format!("object property {other:?}"))),
            },
        }
    }
    Ok(Expr::Object(props))
}

fn lower_prop_name(file: &str, name: &ast::PropName) -> Result<PropKey, IrError> {
    match name {
        ast::PropName::Ident(id) => Ok(PropKey::Ident(id.sym.to_string())),
        ast::PropName::Str(s) => Ok(PropKey::Ident(atom_string(&s.value))),
        ast::PropName::Computed(computed) => Ok(PropKey::Computed(Box::new(lower_expr(file, &computed.expr)?))),
        other => Err(unsupported(file, format!("property name {other:?}"))),
    }
}

fn lower_member(file: &str, member: &ast::MemberExpr) -> Result<Expr, IrError> {
    let obj = Box::new(lower_expr(file, &member.obj)?);
    let prop = match &member.prop {
        ast::MemberProp::Ident(id) => MemberProp::Ident(id.sym.to_string()),
        ast::MemberProp::Computed(computed) => MemberProp::Computed(Box::new(lower_expr(file, &computed.expr)?)),
        ast::MemberProp::PrivateName(_) => {
            return Err(unsupported(file, "#private field reference"));
        }
    };
    Ok(Expr::Member { obj, prop })
}

fn lower_call_args(file: &str, args: &[ast::ExprOrSpread]) -> Result<Vec<CallArg>, IrError> {
    args.iter()
        .map(|arg| {
            let lowered = lower_expr(file, &arg.expr)?;
            Ok(if arg.spread.is_some() {
                CallArg::Spread(lowered)
            } else {
                CallArg::Normal(lowered)
            })
        })
        .collect()
}

/// Recognizes calls against the host-intrinsic table (regex `.test`, `Number(...)`,
/// `Map`/`WeakMap` accessor methods, array bookkeeping methods) before falling back to an
/// ordinary tenant/user call. See `intrinsics.rs`.
fn lower_call(file: &str, call: &ast::CallExpr) -> Result<Expr, IrError> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Err(unsupported(file, "super()/import() call"));
    };
    let args = lower_call_args(file, &call.args)?;

    if let ast::Expr::Member(member) = callee_expr.as_ref()
        && let ast::MemberProp::Ident(method) = &member.prop
    {
        let method_name = method.sym.as_ref();
        // `/regex/.test(x)` — the one recognized regex intrinsic.
        if method_name == "test"
            && matches!(member.obj.as_ref(), ast::Expr::Lit(ast::Lit::Regex(r)) if r.exp.as_ref() == r"^(0|[1-9][0-9]*)$")
            && let [CallArg::Normal(subject)] = args.as_slice()
        {
            return Ok(Expr::HostIntrinsic {
                name: intrinsics::IS_ARRAY_INDEX_STRING,
                args: vec![subject.clone()],
            });
        }
        // `typeof X === "string"` is handled in `lower_bin`, not here.
        let map_methods = ["get", "set", "has", "delete"];
        if map_methods.contains(&method_name) {
            let recv = lower_expr(file, &member.obj)?;
            let mapped = match method_name {
                "get" => intrinsics::MAP_GET,
                "set" => intrinsics::MAP_SET,
                "has" => intrinsics::MAP_HAS,
                "delete" => intrinsics::MAP_DELETE,
                _ => unreachable!(),
            };
            let mut full_args = vec![recv];
            full_args.extend(args.iter().map(|a| match a {
                CallArg::Normal(e) | CallArg::Spread(e) => e.clone(),
            }));
            return Ok(Expr::HostIntrinsic {
                name: mapped,
                args: full_args,
            });
        }
        let array_methods = [
            ("filter", intrinsics::ARRAY_FILTER),
            ("sort", intrinsics::ARRAY_SORT_BY),
            ("push", intrinsics::ARRAY_PUSH),
            ("slice", intrinsics::ARRAY_SLICE_FROM),
        ];
        if let Some((_, mapped)) = array_methods.iter().find(|(name, _)| *name == method_name) {
            let recv = lower_expr(file, &member.obj)?;
            let mut full_args = vec![recv];
            full_args.extend(args.iter().map(|a| match a {
                CallArg::Normal(e) | CallArg::Spread(e) => e.clone(),
            }));
            return Ok(Expr::HostIntrinsic {
                name: mapped,
                args: full_args,
            });
        }
    }

    if let ast::Expr::Ident(id) = callee_expr.as_ref() {
        if id.sym.as_ref() == "Number" && args.len() == 1 {
            let CallArg::Normal(arg) = &args[0] else {
                return Err(unsupported(file, "Number(...spread)"));
            };
            return Ok(Expr::HostIntrinsic {
                name: intrinsics::TO_NUMBER,
                args: vec![arg.clone()],
            });
        }
        if id.sym.as_ref() == "String" && args.len() == 1 {
            let CallArg::Normal(arg) = &args[0] else {
                return Err(unsupported(file, "String(...spread)"));
            };
            return Ok(Expr::HostIntrinsic {
                name: intrinsics::TO_STRING,
                args: vec![arg.clone()],
            });
        }
    }

    let callee = Box::new(lower_expr(file, callee_expr)?);
    Ok(Expr::Call { callee, args })
}

fn lower_arrow(file: &str, arrow: &ast::ArrowExpr) -> Result<Expr, IrError> {
    if arrow.is_async {
        return Err(unsupported(file, "async arrow function"));
    }
    let params = arrow
        .params
        .iter()
        .map(|p| lower_pat_as_param(file, p))
        .collect::<Result<Vec<_>, _>>()?;
    let body = match arrow.body.as_ref() {
        ast::BlockStmtOrExpr::BlockStmt(block) => lower_block(file, block)?,
        ast::BlockStmtOrExpr::Expr(expr) => Block(vec![Stmt::Return(Some(lower_expr(file, expr)?))]),
    };
    Ok(Expr::Closure(Box::new(FnDecl {
        name: None,
        is_generator: false,
        params,
        body,
        captures: Vec::new(),
        return_type: None,
    })))
}

/// Recognizes `yield tenant.yieldTenant(EXPR)` as the first-class `TenantYield` node (see
/// `ir.rs`'s doc comment on `Expr::TenantYield`); any other `yield` is a hard rejection, since
/// no primordial in the surveyed source has a real guest-visible generator yield of its own.
fn lower_yield(file: &str, yield_expr: &ast::YieldExpr) -> Result<Expr, IrError> {
    if yield_expr.delegate {
        return Err(unsupported(file, "yield* (delegated yield)"));
    }
    let Some(arg) = &yield_expr.arg else {
        return Err(unsupported(file, "bare `yield` with no argument"));
    };
    let ast::Expr::Call(call) = arg.as_ref() else {
        return Err(unsupported(file, "yield of a non-call expression (expected `tenant.yieldTenant(...)`)"));
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return Err(unsupported(file, "yield of super()/import()"));
    };
    let ast::Expr::Member(member) = callee.as_ref() else {
        return Err(unsupported(file, "yield of a non-method call (expected `tenant.yieldTenant(...)`)"));
    };
    let ast::MemberProp::Ident(method) = &member.prop else {
        return Err(unsupported(file, "yield of a computed-member call"));
    };
    if method.sym.as_ref() != "yieldTenant" {
        return Err(unsupported(file, format!("yield of `.{}(...)`, expected `.yieldTenant(...)`", method.sym)));
    }
    let [inner] = call.args.as_slice() else {
        return Err(unsupported(file, "yieldTenant(...) with != 1 argument"));
    };
    Ok(Expr::TenantYield(Box::new(lower_expr(file, &inner.expr)?)))
}

fn lower_bin(file: &str, bin: &ast::BinExpr) -> Result<Expr, IrError> {
    // `typeof X === "string"` is recognized as the `is_string_key` intrinsic (see
    // `intrinsics.rs`) before falling through to a generic comparison.
    if bin.op == ast::BinaryOp::EqEqEq
        && let ast::Expr::Unary(unary) = bin.left.as_ref()
        && unary.op == ast::UnaryOp::TypeOf
        && let ast::Expr::Lit(ast::Lit::Str(s)) = bin.right.as_ref()
        && atom_string(&s.value) == "string"
    {
        return Ok(Expr::HostIntrinsic {
            name: intrinsics::IS_STRING_KEY,
            args: vec![lower_expr(file, &unary.arg)?],
        });
    }
    let op = match bin.op {
        ast::BinaryOp::Add => BinOp::Add,
        ast::BinaryOp::Sub => BinOp::Sub,
        ast::BinaryOp::Mul => BinOp::Mul,
        ast::BinaryOp::Mod => BinOp::Mod,
        ast::BinaryOp::EqEqEq | ast::BinaryOp::EqEq => BinOp::Eq,
        ast::BinaryOp::NotEqEq | ast::BinaryOp::NotEq => BinOp::NotEq,
        ast::BinaryOp::Lt => BinOp::Lt,
        ast::BinaryOp::LtEq => BinOp::Le,
        ast::BinaryOp::Gt => BinOp::Gt,
        ast::BinaryOp::GtEq => BinOp::Ge,
        ast::BinaryOp::LogicalAnd => BinOp::And,
        ast::BinaryOp::LogicalOr => BinOp::Or,
        ast::BinaryOp::NullishCoalescing => BinOp::Nullish,
        ast::BinaryOp::In => BinOp::In,
        other => return Err(unsupported(file, format!("binary operator {other:?}"))),
    };
    Ok(Expr::Bin {
        op,
        lhs: Box::new(lower_expr(file, &bin.left)?),
        rhs: Box::new(lower_expr(file, &bin.right)?),
    })
}

fn lower_unary(file: &str, unary: &ast::UnaryExpr) -> Result<Expr, IrError> {
    let op = match unary.op {
        ast::UnaryOp::Bang => UnOp::Not,
        ast::UnaryOp::Minus => UnOp::Neg,
        ast::UnaryOp::TypeOf => UnOp::TypeOf,
        other => return Err(unsupported(file, format!("unary operator {other:?}"))),
    };
    Ok(Expr::Un {
        op,
        arg: Box::new(lower_expr(file, &unary.arg)?),
    })
}

fn lower_assign(file: &str, assign: &ast::AssignExpr) -> Result<Expr, IrError> {
    if assign.op != ast::AssignOp::Assign {
        return Err(unsupported(file, format!("compound assignment operator {:?}", assign.op)));
    }
    let target = match &assign.left {
        ast::AssignTarget::Simple(simple) => match simple {
            ast::SimpleAssignTarget::Ident(id) => Expr::Ident(id.id.sym.to_string()),
            ast::SimpleAssignTarget::Member(member) => lower_member(file, member)?,
            other => return Err(unsupported(file, format!("assignment target {other:?}"))),
        },
        other => return Err(unsupported(file, format!("assignment pattern target {other:?}"))),
    };
    Ok(Expr::Assign {
        target: Box::new(target),
        value: Box::new(lower_expr(file, &assign.right)?),
    })
}

// Silence unused-import warnings for the comment-carrying parser entry point some swc
// versions require even when comments aren't consumed.
#[allow(dead_code)]
fn _unused(_: NoopComments) {}
