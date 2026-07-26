//! IR -> Rust source emission, targeting `jade-tenant-rt`'s `Tenant` trait. This is the new
//! artifact this crate exists to produce (see the primordial-IR plan's "Rust `Tenant` /
//! `HostAsyncCapability` traits" section for the trait this code calls into).
//!
//! Coverage in this first pass is intentionally narrower than `lower.rs`'s full IR node set —
//! it handles what's needed for the "pure tenant-operation helper" shape (`types.ts`'s
//! `defineData` is the first fully-covered example) and returns a clear `IrError::Unsupported`
//! for constructs it doesn't yet map to Rust, rather than guessing. Notable gaps, discovered
//! while implementing rather than assumed up front, are called out inline where they bite:
//! dynamic per-field access on a `TenantPropertyDescriptor` by a runtime key (`descriptor[key]`,
//! used by `readGuestDescriptor`/`descriptorObject`) and `typeof`/`Number()` coercion on an
//! opaque `T::Value` (used by `assertObject`/`toIndex`/`guestArrayLike`) both need additional
//! `Tenant`-trait primitives (a value-tag introspection method, a numeric-coercion method, and a
//! by-key dynamic accessor for descriptors) that aren't designed yet — see the plan's phased
//! rollout for where that lands.

use std::cell::RefCell;
use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::intrinsics;
use crate::ir::*;
use crate::shims;
use crate::IrError;

const DESCRIPTOR_FIELDS: &[&str] = &["value", "writable", "get", "set", "enumerable", "configurable"];

thread_local! {
    /// Bare-identifier -> import-source map for the module currently being emitted, consulted
    /// only by `emit_call` to resolve a shimmed external call (see `shims.rs`). Set once at the
    /// top of `try_emit_module` and never mutated concurrently — emission is single-threaded and
    /// non-reentrant, so a thread-local avoids threading an extra parameter through every
    /// `emit_*` function purely to reach the one call site that needs it.
    static IMPORT_SOURCES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn imported_source(name: &str) -> Option<String> {
    IMPORT_SOURCES.with(|map| map.borrow().get(name).cloned())
}

/// Tenant trait methods this emitter knows how to call, and each positional argument's passing
/// convention (see `shims::ArgKind`) to match `jade-tenant-rt::Tenant`'s signatures. See the
/// trait definition in `crates/jade-tenant-rt/src/lib.rs`.
fn tenant_method(name: &str) -> Option<(&'static str, &'static [shims::ArgKind])> {
    use shims::ArgKind::{Owned, Ref, RefKey};
    Some(match name {
        "make" => ("make", &[Owned] as &[_]),
        "get" => ("get", &[Ref, RefKey]),
        "set" => ("set", &[Ref, RefKey, Owned]),
        "has" => ("has", &[Ref, RefKey]),
        "delete" => ("delete", &[Ref, RefKey]),
        "ownKeys" => ("own_keys", &[Ref]),
        "ownPropertyKeys" => ("own_property_keys", &[Ref]),
        "getOwnPropertyDescriptor" => ("get_own_property_descriptor", &[Ref, RefKey]),
        "defineProperty" => ("define_property", &[Ref, RefKey, Owned]),
        "getPrototypeOf" => ("get_prototype_of", &[Ref]),
        "setPrototypeOf" => ("set_prototype_of", &[Ref, Owned]),
        "isExtensible" => ("is_extensible", &[Ref]),
        "preventExtensions" => ("prevent_extensions", &[Ref]),
        "define" => ("define", &[Ref, Ref]),
        "assign" => ("assign", &[Ref, Ref]),
        "invoke" => ("invoke", &[Ref, Owned]),
        "invokeTrap" => ("invoke_trap", &[Ref, Ref, Owned]),
        _ => return None,
    })
}

/// Emits one call argument per its [`shims::ArgKind`] passing convention. Shared by
/// `tenant.<method>(...)` calls and shimmed external calls (`emit_shim_call`).
fn emit_call_arg(expr: &Expr, kind: shims::ArgKind) -> Result<TokenStream, IrError> {
    let tokens = emit_expr(expr)?;
    let already_ref = matches!(expr, Expr::Ident(_));
    Ok(match kind {
        shims::ArgKind::Owned => tokens,
        shims::ArgKind::Ref => {
            if already_ref {
                tokens
            } else {
                quote! { &#tokens }
            }
        }
        shims::ArgKind::RefKey => {
            if matches!(expr, Expr::Lit(Lit::Str(_))) {
                quote! { &PropertyKey::from(#tokens) }
            } else if already_ref {
                tokens
            } else {
                quote! { &#tokens }
            }
        }
    })
}

pub fn emit_module(module: &Module, module_name: &str) -> String {
    match try_emit_module(module) {
        Ok(tokens) => {
            let header = format!(
                "/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/{module_name}.ts. */\n"
            );
            header + tokens.to_string().as_str()
        }
        Err(err) => format!("compile_error!(\"gen-primordials: {err}\");"),
    }
}

fn try_emit_module(module: &Module) -> Result<TokenStream, IrError> {
    IMPORT_SOURCES.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for item in &module.items {
            if let Item::ValueImport { source, names } = item {
                for name in names {
                    map.insert(name.clone(), source.clone());
                }
            }
        }
    });

    let mut items = Vec::new();
    items.push(quote! {
        #[allow(unused_imports)]
        use portal_solutions_jade_tenant_rt::{Tenant, PropertyKey, TenantError, TenantPropertyDescriptor, DynFields};
    });
    for item in &module.items {
        if let Some(tokens) = emit_item(item)? {
            items.push(tokens);
        }
    }
    Ok(quote! { #(#items)* })
}

fn emit_item(item: &Item) -> Result<Option<TokenStream>, IrError> {
    match item {
        Item::TypeImport { .. } | Item::ValueImport { .. } | Item::StructDef(_) => Ok(None),
        Item::PerTenantCache { .. } => Err(IrError::Unsupported {
            file: String::new(),
            construct: "PerTenantCache emission (needs the per-tenant PrimordialCache struct design, Phase 7)".into(),
        }),
        Item::ModuleConst { .. } => Err(IrError::Unsupported {
            file: String::new(),
            construct: "module-level const emission".into(),
        }),
        Item::FnDecl(func) => Ok(Some(emit_fn_decl(func)?)),
    }
}

fn rust_type(ty: &TypeRef) -> Result<TokenStream, IrError> {
    match ty {
        TypeRef::Named(name) => Ok(match name.as_str() {
            "object" | "Function" | "unknown" => quote! { T::Value },
            "PropertyKey" => quote! { PropertyKey },
            "boolean" => quote! { bool },
            "string" => quote! { String },
            "number" => quote! { f64 },
            "void" | "undefined" => quote! { () },
            "TenantPropertyDescriptor" => quote! { TenantPropertyDescriptor<T::Value> },
            other => {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("type reference `{other}`"),
                });
            }
        }),
        TypeRef::Generic { name, args } if name == "Partial" && args.len() == 1 => rust_type(&args[0]),
        TypeRef::Generic { name, .. } => Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("generic type `{name}<...>`"),
        }),
        TypeRef::Optional(inner) => {
            let inner = rust_type(inner)?;
            Ok(quote! { Option<#inner> })
        }
        TypeRef::Array(inner) => {
            let inner = rust_type(inner)?;
            Ok(quote! { Vec<#inner> })
        }
    }
}

/// Whether `func`'s Rust translation needs to return `Result<_, TenantError>`. Broader than
/// scanning for a literal `throw`: a direct `throw` statement is one source of fallibility, but
/// so is *any* `TenantYield` (every tenant operation is fallible in Rust, even though TS's
/// generator/yield ceremony hides that) and any call into a shimmed function that itself returns
/// `Result` (see `shims.rs`) — both recognized here regardless of nesting depth, since either
/// one appearing anywhere in the body means the emitted Rust needs `?` to propagate it, which in
/// turn means the function signature must be fallible.
fn block_throws(block: &Block) -> bool {
    block.0.iter().any(stmt_throws)
}

fn stmt_throws(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Throw(_) => true,
        Stmt::If { cond, then_branch, else_branch } => {
            expr_throws(cond) || block_throws(then_branch) || else_branch.as_ref().is_some_and(block_throws)
        }
        Stmt::ForOf { iter, body, .. } => expr_throws(iter) || block_throws(body),
        Stmt::ForCounting { start, bound, body, .. } => expr_throws(start) || expr_throws(bound) || block_throws(body),
        Stmt::TryCatch { try_block, catch_block, .. } => block_throws(try_block) || block_throws(catch_block),
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_throws),
        Stmt::Expr(expr) => expr_throws(expr),
        Stmt::Return(value) => value.as_ref().is_some_and(expr_throws),
    }
}

fn expr_throws(expr: &Expr) -> bool {
    match expr {
        Expr::TenantYield(_) => true,
        Expr::Call { callee, args } => {
            let is_shim_call = matches!(callee.as_ref(), Expr::Ident(name)
                if imported_source(name).is_some_and(|source| shims::lookup(&source, name).is_some()));
            is_shim_call || expr_throws(callee) || args.iter().any(call_arg_throws)
        }
        Expr::New { callee, args } => expr_throws(callee) || args.iter().any(call_arg_throws),
        Expr::Member { obj, prop } => {
            expr_throws(obj) || matches!(prop, MemberProp::Computed(e) if expr_throws(e))
        }
        Expr::Bin { lhs, rhs, .. } => expr_throws(lhs) || expr_throws(rhs),
        Expr::Un { arg, .. } | Expr::Spread(arg) | Expr::Paren(arg) => expr_throws(arg),
        Expr::Cond { test, cons, alt } => expr_throws(test) || expr_throws(cons) || expr_throws(alt),
        Expr::Assign { target, value } => expr_throws(target) || expr_throws(value),
        Expr::TemplateLiteral(parts) => parts.iter().any(|p| matches!(p, TemplatePart::Expr(e) if expr_throws(e))),
        Expr::Array(elements) => elements.iter().any(|e| match e {
            ArrayElement::Normal(e) | ArrayElement::Spread(e) => expr_throws(e),
        }),
        Expr::Object(props) => props.iter().any(|p| match p {
            ObjectProp::KeyValue { value, .. } => expr_throws(value),
            ObjectProp::Spread(e) => expr_throws(e),
            ObjectProp::Method { .. } => false,
        }),
        Expr::HostIntrinsic { args, .. } => args.iter().any(expr_throws),
        Expr::Ident(_) | Expr::Lit(_) | Expr::ThisArg | Expr::Closure(_) => false,
    }
}

fn call_arg_throws(arg: &CallArg) -> bool {
    match arg {
        CallArg::Normal(e) | CallArg::Spread(e) => expr_throws(e),
    }
}

fn emit_fn_decl(func: &FnDecl) -> Result<TokenStream, IrError> {
    let Some(name) = &func.name else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "anonymous top-level function".into(),
        });
    };
    let fn_ident = format_ident!("{}", to_snake_case(name));

    let mut param_tokens = Vec::new();
    for param in &func.params {
        param_tokens.push(emit_param(param)?);
    }

    let throws = block_throws(&func.body);
    let (declared_return, is_void) = match &func.return_type {
        Some(TypeRef::Generic { name, args }) if name == "TenantGenerator" && args.len() == 1 => {
            (rust_type(&args[0])?, matches!(args[0], TypeRef::Named(ref n) if n == "void"))
        }
        Some(other) => (rust_type(other)?, matches!(other, TypeRef::Named(n) if n == "void")),
        None => (quote! { () }, true),
    };
    let return_ty = if throws {
        quote! { -> Result<#declared_return, TenantError> }
    } else {
        quote! { -> #declared_return }
    };

    let mut body = emit_block(&func.body)?;
    // A void-returning TS generator is allowed to fall off the end of its body with no
    // explicit `return;` (the implicit `undefined` return); a non-void one always has an
    // explicit `return X;` on every path by construction (TS wouldn't type-check otherwise),
    // which `Stmt::Return`'s own lowering already turns into `return Ok(X);`. So the only gap
    // to patch here is the void+throws case, where the fallback `Ok(())` needs to be supplied
    // as the block's trailing expression — safe to always append even when unreachable (e.g.
    // after an earlier unconditional `return`), since Rust does not treat that as a hard error.
    if throws && is_void {
        body = quote! { #body Ok(()) };
    }

    Ok(quote! {
        pub fn #fn_ident<T: Tenant>(#(#param_tokens),*) #return_ty {
            #body
        }
    })
}

fn emit_param(param: &Param) -> Result<TokenStream, IrError> {
    let Pattern::Ident(name) = &param.pattern else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "destructured parameter in Rust emission".into(),
        });
    };
    let ident = format_ident!("{}", to_snake_case(name));
    if name == "tenant" {
        return Ok(quote! { #ident: &mut T });
    }
    let Some(ty) = &param.ty else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("untyped parameter `{name}`"),
        });
    };
    let base = rust_type(ty)?;
    // An object/key parameter is borrowed, matching `Tenant`'s own method signatures; a
    // descriptor-shaped parameter (e.g. `defineData`'s `attributes`) is taken by value since
    // it's freshly constructed at most call sites and cheap to move.
    let by_ref = matches!(ty, TypeRef::Named(n) if n == "object" || n == "Function" || n == "PropertyKey" || n == "unknown");
    Ok(if by_ref {
        quote! { #ident: &#base }
    } else if param.default.is_some() {
        // Defaulted parameters (`attributes: Partial<TenantPropertyDescriptor> = {}`) become a
        // plain owned parameter; call sites that omitted the argument in TS must pass
        // `Default::default()` explicitly in Rust — there is no Rust-level default-argument
        // sugar to lower this into instead.
        quote! { #ident: #base }
    } else {
        quote! { #ident: #base }
    })
}

fn emit_block(block: &Block) -> Result<TokenStream, IrError> {
    let mut stmts = Vec::new();
    for stmt in &block.0 {
        stmts.push(emit_stmt(stmt)?);
    }
    Ok(quote! { #(#stmts)* })
}

fn emit_stmt(stmt: &Stmt) -> Result<TokenStream, IrError> {
    match stmt {
        Stmt::Let { pattern, init } => {
            let Pattern::Ident(name) = pattern else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "destructured let-binding in Rust emission".into(),
                });
            };
            let ident = format_ident!("{}", to_snake_case(name));
            let value = match init {
                Some(expr) => emit_expr(expr)?,
                None => {
                    return Err(IrError::Unsupported {
                        file: String::new(),
                        construct: "uninitialized let-binding".into(),
                    });
                }
            };
            Ok(quote! { let #ident = #value; })
        }
        Stmt::Expr(expr) => {
            let value = emit_expr(expr)?;
            Ok(quote! { #value; })
        }
        Stmt::Return(value) => match value {
            Some(expr) => {
                let value = emit_expr(expr)?;
                Ok(quote! { return Ok(#value); })
            }
            None => Ok(quote! { return Ok(()); }),
        },
        Stmt::If { cond, then_branch, else_branch } => {
            let cond = emit_expr(cond)?;
            let then_branch = emit_block(then_branch)?;
            match else_branch {
                Some(else_branch) => {
                    let else_branch = emit_block(else_branch)?;
                    Ok(quote! { if #cond { #then_branch } else { #else_branch } })
                }
                None => Ok(quote! { if #cond { #then_branch } }),
            }
        }
        Stmt::Throw(expr) => {
            let value = emit_throw(expr)?;
            Ok(quote! { return Err(#value); })
        }
        Stmt::ForOf { .. } | Stmt::ForCounting { .. } | Stmt::TryCatch { .. } => Err(IrError::Unsupported {
            file: String::new(),
            construct: "this statement kind is not yet covered by the Rust emitter".into(),
        }),
    }
}

/// `throw new TypeError(...)`/`throw new RangeError(...)` — the only two error constructors in
/// the surveyed source (see the plan's host-intrinsic mapping table).
fn emit_throw(expr: &Expr) -> Result<TokenStream, IrError> {
    let Expr::New { callee, args } = expr else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "throw of a non-`new` expression".into(),
        });
    };
    let Expr::Ident(ctor) = callee.as_ref() else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "throw of a non-identifier constructor".into(),
        });
    };
    let [CallArg::Normal(message)] = args.as_slice() else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "throw constructor with != 1 argument".into(),
        });
    };
    let message = emit_expr(message)?;
    match ctor.as_str() {
        "TypeError" => Ok(quote! { TenantError::TypeError((#message).to_string()) }),
        "RangeError" => Ok(quote! { TenantError::RangeError((#message).to_string()) }),
        other => Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("throw of `new {other}(...)`, only TypeError/RangeError are recognized"),
        }),
    }
}

fn emit_expr(expr: &Expr) -> Result<TokenStream, IrError> {
    match expr {
        Expr::Ident(name) => {
            let ident = format_ident!("{}", to_snake_case(name));
            Ok(quote! { #ident })
        }
        Expr::Lit(Lit::Str(s)) => Ok(quote! { #s }),
        Expr::Lit(Lit::Num(n)) => Ok(quote! { #n }),
        Expr::Lit(Lit::Bool(b)) => Ok(quote! { #b }),
        Expr::Lit(Lit::Null) | Expr::Lit(Lit::Undefined) => Ok(quote! { None }),
        Expr::TemplateLiteral(parts) => emit_template(parts),
        Expr::Paren(inner) => {
            let inner = emit_expr(inner)?;
            Ok(quote! { (#inner) })
        }
        Expr::Member { obj, prop } => emit_member(obj, prop),
        Expr::Object(props) => emit_descriptor_object_literal(props),
        Expr::Call { callee, args } => emit_call(callee, args),
        Expr::TenantYield(inner) => {
            let inner = emit_expr(inner)?;
            Ok(quote! { (#inner)? })
        }
        Expr::Bin { op: BinOp::Nullish, lhs, rhs } => {
            let lhs = emit_expr(lhs)?;
            let rhs = emit_expr(rhs)?;
            Ok(quote! { (#lhs).unwrap_or(#rhs) })
        }
        Expr::Bin { op, lhs, rhs } => {
            let lhs = emit_expr(lhs)?;
            let rhs = emit_expr(rhs)?;
            let op = bin_op_tokens(*op)?;
            Ok(quote! { (#lhs #op #rhs) })
        }
        Expr::Un { op: UnOp::Not, arg } => {
            let arg = emit_expr(arg)?;
            Ok(quote! { (!#arg) })
        }
        Expr::HostIntrinsic { name, args } => emit_host_intrinsic(name, args),
        other => Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("expression kind {other:?} is not yet covered by the Rust emitter"),
        }),
    }
}

fn emit_template(parts: &[TemplatePart]) -> Result<TokenStream, IrError> {
    let mut format_str = String::new();
    let mut args = Vec::new();
    for part in parts {
        match part {
            TemplatePart::Str(s) => format_str.push_str(&s.replace('{', "{{").replace('}', "}}")),
            TemplatePart::Expr(e) => {
                format_str.push_str("{}");
                args.push(emit_expr(e)?);
            }
        }
    }
    Ok(quote! { format!(#format_str, #(#args),*) })
}

fn emit_member(obj: &Expr, prop: &MemberProp) -> Result<TokenStream, IrError> {
    let MemberProp::Ident(field) = prop else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "computed member access is not yet covered by the Rust emitter".into(),
        });
    };
    let obj_tokens = emit_expr(obj)?;
    // A descriptor-field read (`attributes.writable`) is a genuine Rust struct field of type
    // `Option<_>` already, so it passes through as a plain field access — this is exactly what
    // lets `Bin::Nullish` above lower to `.unwrap_or(...)`.
    let field_ident = format_ident!("{}", to_snake_case(field));
    Ok(quote! { #obj_tokens.#field_ident })
}

/// Recognizes an object literal whose keys are drawn entirely from `TenantPropertyDescriptor`'s
/// six known fields and emits a real struct literal (`Some(...)`-wrapping each present field,
/// defaulting absent ones to `None`) — see this module's doc comment on why this is the correct
/// translation (these literals are host-side control records, per `tenants/types.ts`'s own doc
/// comment on `TenantPropertyDescriptor`, not guest-visible objects) rather than a special case.
fn emit_descriptor_object_literal(props: &[ObjectProp]) -> Result<TokenStream, IrError> {
    let mut fields: Vec<(&str, &Expr)> = Vec::new();
    for prop in props {
        let ObjectProp::KeyValue { key, value } = prop else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: "non-key-value object literal member is not yet covered by the Rust emitter".into(),
            });
        };
        let PropKey::Ident(key) = key else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: "computed object literal key is not yet covered by the Rust emitter".into(),
            });
        };
        if !DESCRIPTOR_FIELDS.contains(&key.as_str()) {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!(
                    "object literal with key `{key}` outside TenantPropertyDescriptor's field set is not yet covered by the Rust emitter"
                ),
            });
        }
        fields.push((key.as_str(), value));
    }
    // `value`/`get`/`set` are `Option<T::Value>` — `T::Value`-owning fields — while the source
    // expression is very often a borrowed `&T::Value` parameter (per `emit_param`'s by-ref
    // convention for `object`/`unknown`-typed parameters); `.clone()` is required there and
    // harmless when the expression was already owned (`Tenant::Value: Clone` is a supertrait
    // bound specifically so this is always available). `writable`/`enumerable`/`configurable`
    // are `Option<bool>`, already `Copy`, so no clone is needed or emitted for those.
    let value_typed_fields = ["value", "get", "set"];
    let mut entries = Vec::new();
    for name in DESCRIPTOR_FIELDS {
        let ident = format_ident!("{}", name);
        entries.push(match fields.iter().find(|(key, _)| key == name) {
            Some((_, value)) => {
                let value = emit_expr(value)?;
                if value_typed_fields.contains(name) {
                    quote! { #ident: Some((#value).clone()) }
                } else {
                    quote! { #ident: Some(#value) }
                }
            }
            None => quote! { #ident: None },
        });
    }
    Ok(quote! { TenantPropertyDescriptor { #(#entries),* } })
}

fn emit_call(callee: &Expr, args: &[CallArg]) -> Result<TokenStream, IrError> {
    if let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee
        && let Expr::Ident(recv) = obj.as_ref()
        && recv == "tenant"
        && let Some((rust_name, arg_refs)) = tenant_method(method)
    {
        let method_ident = format_ident!("{}", rust_name);
        if args.len() != arg_refs.len() {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("tenant.{method}(...) called with {} args, expected {}", args.len(), arg_refs.len()),
            });
        }
        let mut arg_tokens = Vec::new();
        for (arg, kind) in args.iter().zip(arg_refs) {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "spread argument to a tenant method".into(),
                });
            };
            arg_tokens.push(emit_call_arg(expr, *kind)?);
        }
        return Ok(quote! { tenant.#method_ident(#(#arg_tokens),*) });
    }

    if let Expr::Ident(name) = callee
        && let Some(source) = imported_source(name)
        && let Some(spec) = shims::lookup(&source, name)
    {
        return emit_shim_call(spec, args);
    }

    Err(IrError::Unsupported {
        file: String::new(),
        construct: "call expression is not yet covered by the Rust emitter (only tenant.<method>(...) calls and shimmed external calls are)".into(),
    })
}

/// Emits a call resolved through the shim registry (see `shims.rs`) — `tenant` is either passed
/// through bare (already `&mut T`, matching how `emit_param` types it) or injected as a leading
/// argument the TS call site never supplied, depending on `spec.inject_tenant`.
fn emit_shim_call(spec: &shims::ShimSpec, args: &[CallArg]) -> Result<TokenStream, IrError> {
    if args.len() > spec.args.len() {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!(
                "{}(...) called with {} args, at most {} are recognized",
                spec.name,
                args.len(),
                spec.args.len()
            ),
        });
    }
    let mut arg_tokens = Vec::new();
    if spec.inject_tenant {
        arg_tokens.push(quote! { tenant });
    }
    for (i, arg_spec) in spec.args.iter().enumerate() {
        let tokens = match args.get(i) {
            Some(CallArg::Normal(expr)) => emit_call_arg(expr, arg_spec.kind)?,
            Some(CallArg::Spread(_)) => {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("spread argument to shimmed call `{}`", spec.name),
                });
            }
            None => {
                let Some(default_literal) = arg_spec.default_literal else {
                    return Err(IrError::Unsupported {
                        file: String::new(),
                        construct: format!("{}(...) is missing its required argument #{i}", spec.name),
                    });
                };
                default_literal.parse::<TokenStream>().map_err(|e| IrError::Unsupported {
                    file: String::new(),
                    construct: format!("shim `{}` default literal did not parse as Rust: {e}", spec.name),
                })?
            }
        };
        arg_tokens.push(tokens);
    }
    let path: TokenStream = spec.rust_path.parse().map_err(|e| IrError::Unsupported {
        file: String::new(),
        construct: format!("shim `{}` rust_path did not parse as Rust: {e}", spec.name),
    })?;
    // Every current shim target returns `Result<_, TenantError>` in Rust. A generator-shaped TS
    // export (`inject_tenant: false`: `defineData`/`readGuestDescriptor`/...) is only ever
    // called wrapped in `yield tenant.yieldTenant(...)` per this codebase's own invariant (every
    // `TenantGenerator`-returning call is composed that way — see `tenants/types.ts`'s doc
    // comment), and `Expr::TenantYield`'s own lowering already appends the `?` there. A
    // synchronous TS export (`inject_tenant: true`: `assertObject`/`toIndex`) is never wrapped
    // that way — TS gets ordinary unchecked exception propagation for free, which Rust's
    // `Result` needs an explicit `?` to match, so it's added here instead.
    if spec.inject_tenant {
        Ok(quote! { (#path(#(#arg_tokens),*))? })
    } else {
        Ok(quote! { #path(#(#arg_tokens),*) })
    }
}

fn emit_host_intrinsic(name: &str, args: &[Expr]) -> Result<TokenStream, IrError> {
    let Some(spec) = intrinsics::spec(name) else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("host intrinsic `{name}` has no Rust lowering registered"),
        });
    };
    let mut rendered = spec.rust_template.to_string();
    for (i, arg) in args.iter().enumerate() {
        let tokens = emit_expr(arg)?;
        rendered = rendered.replace(&format!("{{{i}}}"), &tokens.to_string());
    }
    rendered
        .parse::<TokenStream>()
        .map_err(|e| IrError::Unsupported {
            file: String::new(),
            construct: format!("intrinsic `{name}` template did not parse as Rust: {e}"),
        })
}

fn bin_op_tokens(op: BinOp) -> Result<TokenStream, IrError> {
    Ok(match op {
        BinOp::Add => quote! { + },
        BinOp::Sub => quote! { - },
        BinOp::Mul => quote! { * },
        BinOp::Mod => quote! { % },
        BinOp::Eq => quote! { == },
        BinOp::NotEq => quote! { != },
        BinOp::Lt => quote! { < },
        BinOp::Le => quote! { <= },
        BinOp::Gt => quote! { > },
        BinOp::Ge => quote! { >= },
        BinOp::And => quote! { && },
        BinOp::Or => quote! { || },
        BinOp::Nullish | BinOp::In => {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("binary operator {op:?} has no direct Rust token form"),
            });
        }
    })
}

fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
