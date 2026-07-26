//! IR -> Rust source emission, targeting `jade-tenant-rt`'s `Tenant` trait. This is the new
//! artifact this crate exists to produce (see the primordial-IR plan's "Rust `Tenant` /
//! `HostAsyncCapability` traits" section for the trait this code calls into).
//!
//! Coverage is the closed construct set surveyed in `packages/jade-js/primordials/*.ts` (see
//! `ir.rs`'s module doc comment) — a construct outside that set is a hard `IrError::Unsupported`,
//! never a best-effort guess.
//!
//! A few source idioms need translation strategies with no line-for-line TS equivalent; each is
//! called out at its point of use, but the shared themes are:
//!
//! - **TS narrowing has no Rust counterpart.** `descriptor === undefined ? undefined : USE
//!   (descriptor)` and `if (!descriptor) continue; ...USE(descriptor)...` both rely on TS
//!   narrowing `descriptor`'s type after the check. Rust gets the same effect for free from
//!   `match`/`let-else` *pattern binding*, which shadows the outer name with the unwrapped
//!   payload under the identical identifier — see `emit_cond`'s Option-narrowing case and
//!   `emit_block`'s `if (!X) continue;` peephole. No general narrowing/dataflow analysis exists;
//!   only these two specific recognized shapes get it.
//! - **The per-tenant `WeakMap<Tenant, X>` cache has no Rust counterpart, and doesn't need one.**
//!   "One cache per tenant" becomes "the cache lives in whatever the caller already keeps alive
//!   alongside that tenant" — a `<X>Cache<T>` struct the generated factory function takes as an
//!   explicit extra parameter, only when its body actually uses the cache. See `emit_item`'s
//!   `PerTenantCache` arm and `emit_block`'s cache-lookup peephole.
//! - **A closure lexically captures outer state in TS; a Rust closure can't borrow non-`'static`
//!   locals across `make_builtin`'s `'static` bound.** Each captured name gets cloned into a
//!   fresh binding of the same name immediately before the closure literal (shadowing), so the
//!   closure body needs no rewriting at all — see `emit_closure`.
//! - **`Tenant::Value` is opaque**, so anything TS gets for free from raw host values
//!   (`typeof`, `Number()`, an object/key identity comparison) needs an explicit `Tenant`
//!   primitive instead — `typeof_tag`, `to_number`, `to_property_key`, `nullable`, etc. See
//!   `emit_eq_cmp`/`emit_cast`.

use std::cell::RefCell;
use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::cross_file;
use crate::intrinsics;
use crate::ir::*;
use crate::shims;
use crate::IrError;

const DESCRIPTOR_FIELDS: &[&str] = &["value", "writable", "get", "set", "enumerable", "configurable"];
/// `TenantPropertyDescriptor` fields whose Rust type is `Option<T::Value>` (needs `.clone()`
/// when read from a borrowed source) rather than `Option<bool>` (already `Copy`).
const VALUE_TYPED_DESCRIPTOR_FIELDS: &[&str] = &["value", "get", "set"];

thread_local! {
    /// Bare-identifier -> import-source map for the module currently being emitted, consulted
    /// only by `emit_call` to resolve a shimmed external call (see `shims.rs`). Set once at the
    /// top of `try_emit_module`.
    static IMPORT_SOURCES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// Locally-defined top-level function name -> its own parameter types, so `emit_call` can
    /// derive each argument's passing convention from the callee's *own* declared signature
    /// (`installMethod`/`lock`), the same way `tenant_method`/`shims` do for the other two kinds
    /// of callee this emitter recognizes.
    static LOCAL_FN_PARAMS: RefCell<HashMap<String, Vec<Option<TypeRef>>>> = RefCell::new(HashMap::new());
    /// `interface` name -> its field list, populated from every `Item::StructDef` in the module
    /// currently being emitted. Consulted by `rust_type` (a `TenantGenerator<ObjectPrimordial>`
    /// return type needs to resolve to the generated `ObjectPrimordial<T>` struct) and by
    /// `emit_object_literal` (matching an object literal's key set against a known struct's
    /// field set to tell a struct literal apart from a `TenantPropertyDescriptor` literal).
    static STRUCT_DEFS: RefCell<HashMap<String, StructDef>> = RefCell::new(HashMap::new());
    /// This module's own per-tenant-cache variable name and cached value type
    /// (`Item::PerTenantCache`'s two fields), if it has one. At most one per file in every
    /// surveyed source.
    static PER_TENANT_CACHE: RefCell<Option<(String, String)>> = RefCell::new(None);
    /// Local-variable-name (snake_case) -> "is this already a Rust reference" map for the
    /// function/closure body currently being emitted. Reset at the top of every `emit_fn_decl`;
    /// closures temporarily add their own params' entries on top rather than pushing a real
    /// scope (see `emit_closure`) — sound for the surveyed source because no closure parameter
    /// name ever collides with an outer local name, not a fully general scope stack.
    static LOCAL_REFNESS: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    /// Local-variable-name (snake_case) -> "this is `Option<_>`-typed" set for the function body
    /// currently being emitted, populated as each `let` binding is emitted (see `emit_stmt`).
    /// Drives the `=== undefined`/`!== undefined` -> `.is_none()`/`.is_some()` lowering and the
    /// Option-narrowing `Cond`/`if (!X) continue;` peepholes.
    static OPTION_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// Local-variable-name (snake_case) -> "this is a `&str`-typed local" set, reset per function
    /// (not per closure, matching `LOCAL_REFNESS`'s simplification — see its own doc comment).
    /// `installMethod`'s `name: string` parameter is the one occurrence so far: it's a plain
    /// string, not a `PropertyKey`, but reaches `defineData`'s `RefKey`-kind `key` argument —
    /// `emit_call_arg`'s `RefKey` case consults this to know it needs `PropertyKey::from(...)`
    /// conversion (already applied automatically for a literal string argument) rather than a
    /// bare `&`-borrow (correct for an argument that's already `PropertyKey`-typed, e.g. a `for`
    /// loop variable bound from `ownPropertyKeys`).
    static STRING_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
}

fn imported_source(name: &str) -> Option<String> {
    IMPORT_SOURCES.with(|map| map.borrow().get(name).cloned())
}

fn is_option_local(name: &str) -> bool {
    OPTION_LOCALS.with(|set| set.borrow().contains(name))
}

fn is_known_ref(name: &str) -> bool {
    LOCAL_REFNESS.with(|map| map.borrow().get(name).copied().unwrap_or(false))
}

fn is_string_typed(name: &str) -> bool {
    STRING_TYPED_LOCALS.with(|set| set.borrow().contains(name))
}

/// Tenant trait methods this emitter knows how to call, and each positional argument's passing
/// convention (see `shims::ArgKind`) to match `jade-tenant-rt::Tenant`'s signatures. See the
/// trait definition in `crates/jade-tenant-rt/src/lib.rs`.
fn tenant_method(name: &str) -> Option<(&'static str, &'static [shims::ArgKind])> {
    use shims::ArgKind::{OptionalValue, Owned, Ref, RefKey};
    Some(match name {
        "make" => ("make", &[OptionalValue] as &[_]),
        "get" => ("get", &[Ref, RefKey]),
        "set" => ("set", &[Ref, RefKey, Owned]),
        "has" => ("has", &[Ref, RefKey]),
        "delete" => ("delete", &[Ref, RefKey]),
        "ownKeys" => ("own_keys", &[Ref]),
        "ownPropertyKeys" => ("own_property_keys", &[Ref]),
        "getOwnPropertyDescriptor" => ("get_own_property_descriptor", &[Ref, RefKey]),
        "defineProperty" => ("define_property", &[Ref, RefKey, Owned]),
        "getPrototypeOf" => ("get_prototype_of", &[Ref]),
        "setPrototypeOf" => ("set_prototype_of", &[Ref, OptionalValue]),
        "isExtensible" => ("is_extensible", &[Ref]),
        "preventExtensions" => ("prevent_extensions", &[Ref]),
        "define" => ("define", &[Ref, Ref]),
        "assign" => ("assign", &[Ref, Ref]),
        "invoke" => ("invoke", &[Ref, Owned]),
        "invokeTrap" => ("invoke_trap", &[Ref, Ref, Owned]),
        _ => return None,
    })
}

/// Which [`shims::ArgKind`] a top-level function's own declared parameter type implies — shared
/// by `emit_param` (deciding how *this* function's parameter is bound) and `emit_call`'s
/// local-function-call branch (deciding how to pass an argument *to* a locally-defined
/// function, from its own recorded `LOCAL_FN_PARAMS` signature).
fn arg_kind_for_type(ty: &TypeRef) -> shims::ArgKind {
    match ty {
        TypeRef::Named(n) if n == "object" || n == "Function" || n == "unknown" => shims::ArgKind::Ref,
        TypeRef::Named(n) if n == "PropertyKey" => shims::ArgKind::RefKey,
        _ => shims::ArgKind::Owned,
    }
}

/// Emits one call argument per its [`shims::ArgKind`] passing convention. Shared by
/// `tenant.<method>(...)` calls, shimmed external calls, and locally-defined function calls.
fn emit_call_arg(expr: &Expr, kind: shims::ArgKind) -> Result<TokenStream, IrError> {
    let tokens = emit_expr(expr)?;
    let already_ref = matches!(expr, Expr::Ident(name) if is_known_ref(&to_snake_case(name)));
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
            let needs_property_key_conversion = matches!(expr, Expr::Lit(Lit::Str(_)))
                || matches!(expr, Expr::Ident(name) if is_string_typed(&to_snake_case(name)));
            if needs_property_key_conversion {
                quote! { &PropertyKey::from(#tokens) }
            } else if already_ref {
                tokens
            } else {
                quote! { &#tokens }
            }
        }
        // Both check for an explicit `undefined` argument first (`makeBuiltin(tenant, "call",
        // applyClosure, undefined, FunctionPrototype)` — `call`/`apply`/`bind` all omit a real
        // construct closure this way) and emit plain `None` rather than `Some(Box::new(tenant
        // .undefined_value()))`/`Some((tenant.undefined_value()).clone())`, matching
        // `OptionalValue`'s same distinction below.
        shims::ArgKind::OptionRef => {
            if is_nullish_lit(expr) {
                quote! { None }
            } else {
                quote! { Some((#tokens).clone()) }
            }
        }
        // `make_builtin`'s `construct: Option<ConstructFn<T>>` — `ConstructFn<T>` is a boxed
        // trait object (`Box<dyn FnMut(...)>`), unlike `apply`'s unboxed `impl FnMut(...) +
        // 'static` — so a *real* closure literal needs an explicit `Box::new`.
        shims::ArgKind::OptionOwned => {
            if is_nullish_lit(expr) {
                quote! { None }
            } else {
                quote! { Some(Box::new(#tokens)) }
            }
        }
        shims::ArgKind::OptionalValue => {
            if is_nullish_lit(expr) {
                quote! { None }
            } else if matches!(expr, Expr::Cast { target: TypeRef::Optional(_), .. }) {
                tokens
            } else {
                quote! { Some((#tokens).clone()) }
            }
        }
    })
}

pub fn emit_module(module: &Module, module_name: &str) -> String {
    let header = format!(
        "/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/{module_name}.ts. */\n"
    );
    header + try_emit_module(module).to_string().as_str()
}

/// Emits every item independently: one item's `IrError` is printed to stderr (naming which
/// top-level declaration failed and why) and the item is **omitted** from the generated file,
/// rather than a single not-yet-supported construct anywhere in one factory function blanking
/// out the whole module (a `compile_error!` embedded in the output would do that just as
/// thoroughly as dropping the whole file, since one anywhere fails the *entire crate's* build,
/// not just that item's use sites) — a partially-generated file that actually compiles and is
/// testable is far more useful while emitter coverage is still growing. Still a hard, loudly
/// reported rejection at the point it happens, never a silent guess — it just surfaces as a
/// build-time diagnostic instead of a build-breaking one, since nothing calls the omitted item.
fn try_emit_module(module: &Module) -> TokenStream {
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
    STRUCT_DEFS.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for item in &module.items {
            if let Item::StructDef(def) = item {
                map.insert(def.name.clone(), def.clone());
            }
        }
    });
    PER_TENANT_CACHE.with(|cache| {
        *cache.borrow_mut() = module.items.iter().find_map(|item| match item {
            Item::PerTenantCache { name, value_ty } => Some((name.clone(), value_ty.clone())),
            _ => None,
        });
    });
    LOCAL_FN_PARAMS.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for item in &module.items {
            if let Item::FnDecl(func) = item
                && let Some(name) = &func.name
            {
                map.insert(name.clone(), func.params.iter().map(|p| p.ty.clone()).collect());
            }
        }
    });

    let mut items = Vec::new();
    items.push(quote! {
        #[allow(unused_imports)]
        use portal_solutions_jade_tenant_rt::{Tenant, PropertyKey, TenantError, TenantPropertyDescriptor, TenantInvocation, DynFields, ValueTag};
    });
    // Every registered cross-file factory's struct, unconditionally — small, fixed registry, so
    // this is simpler than scanning for which ones a given file actually destructures (see
    // `cross_file.rs`).
    for factory in cross_file::TABLE {
        let path: TokenStream = factory.struct_path.parse().unwrap_or_default();
        items.push(quote! {
            #[allow(unused_imports)]
            use #path;
        });
    }
    for item in &module.items {
        match emit_item(item) {
            Ok(Some(tokens)) => items.push(tokens),
            Ok(None) => {}
            Err(err) => {
                eprintln!("gen-primordials: skipping {}: {err}", item_label(item));
            }
        }
    }
    quote! { #(#items)* }
}

fn item_label(item: &Item) -> String {
    match item {
        Item::FnDecl(func) => format!("fn `{}`", func.name.as_deref().unwrap_or("<anonymous>")),
        Item::StructDef(def) => format!("interface `{}`", def.name),
        Item::PerTenantCache { name, .. } => format!("cache `{name}`"),
        Item::ModuleConst { name, .. } => format!("const `{name}`"),
        Item::TypeImport { .. } | Item::ValueImport { .. } => "import".to_string(),
    }
}

fn emit_item(item: &Item) -> Result<Option<TokenStream>, IrError> {
    match item {
        Item::TypeImport { .. } | Item::ValueImport { .. } => Ok(None),
        Item::StructDef(def) => Ok(Some(emit_struct_def(def)?)),
        Item::PerTenantCache { value_ty, .. } => {
            let cache_ident = format_ident!("{value_ty}Cache");
            let value_ident = format_ident!("{value_ty}");
            Ok(Some(quote! {
                pub struct #cache_ident<T: Tenant> { entry: Option<#value_ident<T>> }
                impl<T: Tenant> Default for #cache_ident<T> {
                    fn default() -> Self { Self { entry: None } }
                }
            }))
        }
        Item::ModuleConst { .. } => Err(IrError::Unsupported {
            file: String::new(),
            construct: "module-level const emission".into(),
        }),
        Item::FnDecl(func) => Ok(Some(emit_fn_decl(func)?)),
    }
}

/// Emits an `interface`'s generated struct plus a hand-written `Clone` impl (**not**
/// `#[derive(Clone)]`: the derive would add an overly strict `T: Clone` bound on the struct's
/// own generic parameter, when only `T::Value: Clone` — already guaranteed by the `Tenant`
/// supertrait bound — is actually needed by any field).
fn emit_struct_def(def: &StructDef) -> Result<TokenStream, IrError> {
    let struct_ident = format_ident!("{}", def.name);
    let mut field_decls = Vec::new();
    let mut field_idents = Vec::new();
    for (name, ty) in &def.fields {
        let field_ident = format_ident!("{}", to_snake_case(name));
        let field_ty = rust_type(ty)?;
        field_decls.push(quote! { pub #field_ident: #field_ty });
        field_idents.push(field_ident);
    }
    Ok(quote! {
        pub struct #struct_ident<T: Tenant> { #(#field_decls),* }
        impl<T: Tenant> Clone for #struct_ident<T> {
            fn clone(&self) -> Self {
                Self { #(#field_idents: self.#field_idents.clone()),* }
            }
        }
    })
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
                if STRUCT_DEFS.with(|d| d.borrow().contains_key(other)) {
                    let ident = format_ident!("{other}");
                    quote! { #ident<T> }
                } else {
                    return Err(IrError::Unsupported {
                        file: String::new(),
                        construct: format!("type reference `{other}`"),
                    });
                }
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
/// generator/yield ceremony hides that) and any call into a shimmed/local function that itself
/// returns `Result` — recognized here regardless of nesting depth, since either one appearing
/// anywhere in the body means the emitted Rust needs `?` to propagate it, which in turn means
/// the function signature must be fallible.
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
        Stmt::Continue => false,
    }
}

fn expr_throws(expr: &Expr) -> bool {
    match expr {
        Expr::TenantYield(_) => true,
        Expr::Call { callee, args } => {
            let is_shim_call = matches!(callee.as_ref(), Expr::Ident(name)
                if imported_source(name).is_some_and(|source| shims::lookup(&source, name).is_some()));
            let is_local_call = matches!(callee.as_ref(), Expr::Ident(name)
                if LOCAL_FN_PARAMS.with(|m| m.borrow().contains_key(name)));
            let is_cross_file_call = matches!(callee.as_ref(), Expr::Ident(name)
                if imported_source(name).is_some_and(|source| cross_file::lookup(&source, name).is_some()));
            is_shim_call || is_local_call || is_cross_file_call || expr_throws(callee) || args.iter().any(call_arg_throws)
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
        Expr::Sequence(exprs) => exprs.iter().any(expr_throws),
        Expr::Cast { expr, .. } => expr_throws(expr),
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

    LOCAL_REFNESS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        for param in &func.params {
            if let Pattern::Ident(pname) = &param.pattern {
                let is_string = matches!(param.ty, Some(TypeRef::Named(ref n)) if n == "string");
                let is_ref = pname == "tenant"
                    || is_string
                    || param
                        .ty
                        .as_ref()
                        .is_some_and(|ty| matches!(arg_kind_for_type(ty), shims::ArgKind::Ref | shims::ArgKind::RefKey));
                m.insert(to_snake_case(pname), is_ref);
            }
        }
    });
    OPTION_LOCALS.with(|m| m.borrow_mut().clear());
    STRING_TYPED_LOCALS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        for param in &func.params {
            if let Pattern::Ident(pname) = &param.pattern
                && matches!(param.ty, Some(TypeRef::Named(ref n)) if n == "string")
            {
                m.insert(to_snake_case(pname));
            }
        }
    });

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
    if throws && is_void {
        body = quote! { #body Ok(()) };
    }

    let cache = PER_TENANT_CACHE.with(|c| c.borrow().clone());
    if let Some((cache_var, value_ty)) = &cache
        && word_referenced(&body.to_string(), cache_var)
    {
        let cache_ty_ident = format_ident!("{value_ty}Cache");
        param_tokens.push(quote! { cache: &mut #cache_ty_ident<T> });
    }
    // Any registered cross-file factory this function's body calls needs its cache threaded in
    // too, one extra parameter per distinct factory referenced — see `cross_file.rs`.
    let body_str = body.to_string();
    for factory in cross_file::TABLE {
        let cache_param = cross_file::cache_param_name(factory);
        if word_referenced(&body_str, &cache_param) {
            let cache_ident = format_ident!("{cache_param}");
            let cache_ty: TokenStream = format!("{}Cache", factory.struct_path).parse().map_err(|e| IrError::Unsupported {
                file: String::new(),
                construct: format!("cross-file factory `{}` struct_path did not parse as Rust: {e}", factory.name),
            })?;
            param_tokens.push(quote! { #cache_ident: &mut #cache_ty<T> });
        }
    }

    // `+ 'static` unconditionally: any factory that (transitively) reaches `make_builtin` needs
    // it (`ConstructFn<T>`/`impl FnMut(...) + 'static` both require it), and it's a harmless
    // superset requirement for the rest — every real `Tenant` impl in an actual embedding
    // satisfies it anyway, so this sidesteps tracking "does this specific function's call graph
    // reach a closure-taking shim" as its own analysis.
    Ok(quote! {
        pub fn #fn_ident<T: Tenant + 'static>(#(#param_tokens),*) #return_ty {
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
    // Sentinels recognized structurally rather than through the ordinary `rust_type`/`ArgKind`
    // machinery — see `lower.rs`'s `lower_ts_type`/`lower_closure_bearing_call` doc comments for
    // why these two shapes are special-cased at lowering time in the first place.
    if matches!(ty, TypeRef::Named(n) if n == "__ApplyClosure") {
        return Ok(quote! {
            #ident: impl FnMut(&mut T, T::Value, &[T::Value]) -> Result<T::Value, TenantError> + 'static
        });
    }
    if let TypeRef::Array(inner) = ty
        && matches!(inner.as_ref(), TypeRef::Named(n) if n == "unknown")
    {
        return Ok(quote! { #ident: &[T::Value] });
    }
    // `&str`, not `rust_type`'s `String` (that mapping is correct for a return/field position,
    // not a borrowed parameter) — every real call site passes a string literal, which is
    // already `&str`-compatible with no conversion needed.
    if matches!(ty, TypeRef::Named(n) if n == "string") {
        return Ok(quote! { #ident: &str });
    }
    let base = rust_type(ty)?;
    Ok(match arg_kind_for_type(ty) {
        shims::ArgKind::Ref | shims::ArgKind::RefKey => quote! { #ident: &#base },
        _ => quote! { #ident: #base },
    })
}

/// Whether `init` is a `TenantYield`-wrapped call to a `Tenant` method whose Rust return type is
/// `Option<_>` — the two the surveyed source narrows immediately afterward
/// (`getOwnPropertyDescriptor`'s ternary, `getPrototypeOf`, though only the former is exercised
/// today). Drives `OPTION_LOCALS` population in `emit_stmt`'s `Stmt::Let` handling.
fn returns_option(init: &Expr) -> bool {
    tenant_call_method_name(init).is_some_and(|m| matches!(m, "getOwnPropertyDescriptor" | "getPrototypeOf"))
}

/// If `expr` is `yield tenant.yieldTenant(tenant.<method>(...))`, the bare method name — used to
/// recognize which `Tenant` methods return which non-`T::Value` Rust type, for `returns_option`
/// and `coerce_return_value`.
fn tenant_call_method_name(expr: &Expr) -> Option<&str> {
    let Expr::TenantYield(inner) = expr else { return None };
    let Expr::Call { callee, .. } = inner.as_ref() else { return None };
    let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee.as_ref() else { return None };
    matches!(obj.as_ref(), Expr::Ident(n) if n == "tenant").then_some(method.as_str())
}

/// Every closure's declared return type is a plain guest `T::Value` (see `emit_closure`), but a
/// handful of `Tenant` methods return something else in Rust — `bool`
/// (`has`/`isExtensible`/`preventExtensions`/`defineProperty`/`setPrototypeOf`) or
/// `Option<T::Value>` (`getPrototypeOf`) — where the *TS* source gets away with returning the
/// bare value directly because in the current TS-hosted execution model a host `boolean`/
/// primitive *is* already a valid guest-observable value with no marshaling boundary at all
/// (only objects/functions are tenant-managed). Rust's `Tenant::Value` is opaque, so returning
/// one of these from a closure needs an explicit conversion at exactly this boundary: `bool` via
/// `Tenant::boolean_value`, `Option<T::Value>` by unwrapping to `Tenant::null_value()`.
/// `ownKeys`/`ownPropertyKeys` (`Vec<PropertyKey>`) have no such conversion available — a raw
/// host key list has no guest-visible Array representation without a guest Array primordial,
/// which doesn't exist yet (see `docs/primordials-plan.md`'s own non-goals) — so returning one
/// directly is a named, honest rejection rather than a guess.
fn coerce_return_value(expr: &Expr, tokens: TokenStream) -> Result<TokenStream, IrError> {
    const BOOL_RETURNING: &[&str] = &["has", "isExtensible", "preventExtensions", "defineProperty", "setPrototypeOf"];
    const OPTION_VALUE_RETURNING: &[&str] = &["getPrototypeOf"];
    const KEY_LIST_RETURNING: &[&str] = &["ownKeys", "ownPropertyKeys"];
    if let Some(method) = tenant_call_method_name(expr) {
        if BOOL_RETURNING.contains(&method) {
            return Ok(quote! { tenant.boolean_value(#tokens) });
        }
        if OPTION_VALUE_RETURNING.contains(&method) {
            return Ok(quote! { (#tokens).unwrap_or_else(|| tenant.null_value()) });
        }
        if KEY_LIST_RETURNING.contains(&method) {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!(
                    "returning tenant.{method}(...)'s host-side Vec<PropertyKey> directly as a guest value needs a guest Array primordial, which doesn't exist yet"
                ),
            });
        }
    }
    // Returning a bare identifier that's already a Rust reference (a by-ref parameter, or a
    // loop/match binding — see `LOCAL_REFNESS`) needs an explicit `.clone()`: the function's own
    // declared return type is always an owned `T::Value`/struct, never a reference.
    if matches!(expr, Expr::Ident(name) if is_known_ref(&to_snake_case(name))) {
        return Ok(quote! { (#tokens).clone() });
    }
    Ok(tokens)
}

fn emit_block(block: &Block) -> Result<TokenStream, IrError> {
    let mut stmts = Vec::new();
    let mut i = 0;
    while i < block.0.len() {
        if let Some(tokens) = try_emit_cache_lookup_pair(&block.0[i..])? {
            stmts.push(tokens);
            i += 2;
            continue;
        }
        stmts.push(emit_stmt(&block.0[i])?);
        i += 1;
    }
    Ok(quote! { #(#stmts)* })
}

/// Recognizes `const existing = cache.get(tenant); if (existing) return existing;` — the
/// two-statement cache-lookup idiom every per-tenant-cached factory opens with — and emits the
/// single-parameter-based equivalent: `if let Some(existing) = cache.entry.clone() { return Ok
/// (existing); }`. See this module's doc comment on why the cache becomes an explicit parameter
/// rather than a `WeakMap`. Returns `None` (falling through to ordinary per-statement emission)
/// for anything that isn't exactly this shape, including when the module has no per-tenant cache
/// at all.
fn try_emit_cache_lookup_pair(stmts: &[Stmt]) -> Result<Option<TokenStream>, IrError> {
    let Some((cache_name, _)) = PER_TENANT_CACHE.with(|c| c.borrow().clone()) else { return Ok(None) };
    let [
        Stmt::Let { pattern: Pattern::Ident(existing_name), init: Some(Expr::Call { callee, args }) },
        Stmt::If { cond: Expr::Ident(cond_name), then_branch, else_branch: None },
        ..,
    ] = stmts
    else {
        return Ok(None);
    };
    let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee.as_ref() else { return Ok(None) };
    let Expr::Ident(obj_name) = obj.as_ref() else { return Ok(None) };
    if obj_name != &cache_name || method != "get" || cond_name != existing_name {
        return Ok(None);
    }
    let [CallArg::Normal(Expr::Ident(tenant_arg))] = args.as_slice() else { return Ok(None) };
    if tenant_arg != "tenant" {
        return Ok(None);
    }
    let [Stmt::Return(Some(Expr::Ident(ret_name)))] = then_branch.0.as_slice() else { return Ok(None) };
    if ret_name != existing_name {
        return Ok(None);
    }
    let ident = format_ident!("{}", to_snake_case(existing_name));
    Ok(Some(quote! {
        if let Some(#ident) = cache.entry.clone() { return Ok(#ident); }
    }))
}

/// Recognizes `cache.set(tenant, result);` (the one-statement half of the cache idiom — see
/// `try_emit_cache_lookup_pair` for the other half) inside `emit_stmt`.
fn try_emit_cache_set(stmt: &Stmt) -> Result<Option<TokenStream>, IrError> {
    let Some((cache_name, _)) = PER_TENANT_CACHE.with(|c| c.borrow().clone()) else { return Ok(None) };
    let Stmt::Expr(Expr::Call { callee, args }) = stmt else { return Ok(None) };
    let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee.as_ref() else { return Ok(None) };
    let Expr::Ident(obj_name) = obj.as_ref() else { return Ok(None) };
    if obj_name != &cache_name || method != "set" {
        return Ok(None);
    }
    let [CallArg::Normal(Expr::Ident(_tenant_arg)), CallArg::Normal(result_expr)] = args.as_slice() else {
        return Ok(None);
    };
    let result_tokens = emit_expr(result_expr)?;
    Ok(Some(quote! { cache.entry = Some((#result_tokens).clone()); }))
}

/// Recognizes `if (!IDENT) continue;` where `IDENT` is a known `Option<_>`-typed local (`lock`'s
/// early-exit guard on a per-key `getOwnPropertyDescriptor` result) and emits Rust's `let-else`
/// form instead, which both performs the guard and shadows `IDENT` with its unwrapped payload
/// for the rest of the enclosing block — see this module's doc comment on why this sidesteps
/// needing real narrowing/dataflow analysis.
fn option_narrow_continue(cond: &Expr, then_branch: &Block) -> Option<TokenStream> {
    let Expr::Un { op: UnOp::Not, arg } = cond else { return None };
    let Expr::Ident(name) = arg.as_ref() else { return None };
    if !is_option_local(&to_snake_case(name)) {
        return None;
    }
    if !matches!(then_branch.0.as_slice(), [Stmt::Continue]) {
        return None;
    }
    let ident = format_ident!("{}", to_snake_case(name));
    Some(quote! { let Some(#ident) = #ident else { continue; }; })
}

/// `const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));` — the one
/// destructuring shape in the surveyed source, always over a cross-file factory's result (see
/// `cross_file.rs`). Rust struct-pattern destructuring gives this the same effect with no
/// further rewriting needed downstream: `let ObjectPrimordial { object_prototype, .. } = ...;`
/// binds `object_prototype` for the rest of the block exactly like the TS `const` does.
fn emit_destructuring_let(bindings: &[ShallowBinding], init_expr: &Expr) -> Result<TokenStream, IrError> {
    let struct_name = destructure_struct_name(init_expr)?;
    let struct_ident = format_ident!("{struct_name}");
    let init_tokens = emit_expr(init_expr)?;
    let mut field_pats = Vec::new();
    for binding in bindings {
        let field_ident = format_ident!("{}", to_snake_case(&binding.key));
        let binding_ident = format_ident!("{}", to_snake_case(&binding.binding));
        field_pats.push(quote! { #field_ident: #binding_ident });
        // Destructured struct fields are owned `T::Value` — see `emit_struct_def`.
        LOCAL_REFNESS.with(|m| m.borrow_mut().insert(to_snake_case(&binding.binding), false));
    }
    Ok(quote! { let #struct_ident { #(#field_pats),*, .. } = #init_tokens; })
}

/// Which generated struct type `init_expr` (a destructuring `let`'s initializer) produces — the
/// only recognized shape is `yield tenant.yieldTenant(<registered cross-file factory>(tenant))`.
fn destructure_struct_name(init_expr: &Expr) -> Result<&'static str, IrError> {
    let Expr::TenantYield(inner) = init_expr else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "destructuring a non-`yield tenant.yieldTenant(...)` initializer".into(),
        });
    };
    let Expr::Call { callee, .. } = inner.as_ref() else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "destructuring a non-call initializer".into(),
        });
    };
    let Expr::Ident(name) = callee.as_ref() else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "destructuring a call through a non-identifier callee".into(),
        });
    };
    let source = imported_source(name).ok_or_else(|| IrError::Unsupported {
        file: String::new(),
        construct: format!("destructuring the result of `{name}(...)`, which isn't an imported cross-file factory"),
    })?;
    let factory = cross_file::lookup(&source, name).ok_or_else(|| IrError::Unsupported {
        file: String::new(),
        construct: format!("destructuring the result of `{name}(...)`, which isn't a registered cross-file factory"),
    })?;
    Ok(factory.struct_name)
}

fn emit_stmt(stmt: &Stmt) -> Result<TokenStream, IrError> {
    if let Some(tokens) = try_emit_cache_set(stmt)? {
        return Ok(tokens);
    }
    match stmt {
        Stmt::Let { pattern: Pattern::ObjectShallow(bindings), init: Some(init_expr) } => {
            emit_destructuring_let(bindings, init_expr)
        }
        Stmt::Let { pattern: Pattern::ObjectShallow(_), init: None } => Err(IrError::Unsupported {
            file: String::new(),
            construct: "uninitialized destructured let-binding".into(),
        }),
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
            LOCAL_REFNESS.with(|m| m.borrow_mut().insert(to_snake_case(name), false));
            if init.as_ref().is_some_and(returns_option) {
                OPTION_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(name)));
            }
            // Always `mut`: whether a given binding is later reassigned (a struct field write
            // through it, e.g. `next.writable = false`) isn't tracked separately, and an unused
            // `mut` is a warning, never a build error — see the module doc comment's framing.
            Ok(quote! { let mut #ident = #value; })
        }
        Stmt::Expr(expr) => {
            let value = emit_expr(expr)?;
            Ok(quote! { #value; })
        }
        Stmt::Return(value) => match value {
            Some(expr) => {
                let value = emit_expr(expr)?;
                let value = coerce_return_value(expr, value)?;
                Ok(quote! { return Ok(#value); })
            }
            None => Ok(quote! { return Ok(()); }),
        },
        Stmt::If { cond, then_branch, else_branch: None } if option_narrow_continue(cond, then_branch).is_some() => {
            Ok(option_narrow_continue(cond, then_branch).unwrap())
        }
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
        Stmt::Continue => Ok(quote! { continue; }),
        Stmt::ForOf { binding, iter, body } => {
            let Pattern::Ident(name) = binding else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "destructured for-of binding".into(),
                });
            };
            let ident = format_ident!("{}", to_snake_case(name));
            let iter_tokens = emit_expr(iter)?;
            // Iterating a `&[T::Value]` slice (`args.slice(N)`, already emitted as `&args[N..]`)
            // yields `&T::Value` items; iterating an owned `Vec<_>` (`ownKeys`/`ownPropertyKeys`)
            // yields owned items — Rust's own `IntoIterator` selection already gets this right
            // from the expression's type with no extra `&` needed either way, so only the
            // *bookkeeping* (what ref-ness to record for the loop-local) differs by shape.
            let binding_is_ref = matches!(iter, Expr::HostIntrinsic { name, .. } if *name == intrinsics::ARRAY_SLICE_FROM);
            let saved = LOCAL_REFNESS.with(|m| m.borrow().clone());
            LOCAL_REFNESS.with(|m| {
                m.borrow_mut().insert(to_snake_case(name), binding_is_ref);
            });
            let body_tokens = emit_block(body)?;
            LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved);
            Ok(quote! { for #ident in #iter_tokens { #body_tokens } })
        }
        Stmt::ForCounting { .. } | Stmt::TryCatch { .. } => Err(IrError::Unsupported {
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
        // The guest `null`/`undefined` value itself, by default — `None` is only correct in the
        // specific `Option<T::Value>`-typed positions `ArgKind::OptionalValue` recognizes (see
        // its doc comment), which check the raw IR node directly rather than going through this
        // general expression path.
        Expr::Lit(Lit::Null) => Ok(quote! { tenant.null_value() }),
        Expr::Lit(Lit::Undefined) => Ok(quote! { tenant.undefined_value() }),
        Expr::TemplateLiteral(parts) => emit_template(parts),
        Expr::Paren(inner) => {
            let inner = emit_expr(inner)?;
            Ok(quote! { (#inner) })
        }
        Expr::Member { obj, prop } => emit_member(obj, prop),
        Expr::Object(props) => emit_object_literal(props),
        Expr::Array(elements) => emit_array_literal(elements),
        Expr::Call { callee, args } => emit_call(callee, args),
        Expr::TenantYield(inner) => {
            let inner = emit_expr(inner)?;
            Ok(quote! { (#inner)? })
        }
        Expr::Bin { op, lhs, rhs } => emit_bin(*op, lhs, rhs),
        Expr::Un { op: UnOp::Not, arg } => {
            let arg = emit_expr(arg)?;
            Ok(quote! { (!#arg) })
        }
        Expr::Cond { test, cons, alt } => emit_cond(test, cons, alt),
        Expr::Assign { target, value } => emit_assign(target, value),
        Expr::Sequence(exprs) => {
            let mut parts = Vec::new();
            for (i, e) in exprs.iter().enumerate() {
                let tokens = emit_expr(e)?;
                parts.push(if i + 1 == exprs.len() { quote! { #tokens } } else { quote! { #tokens; } });
            }
            Ok(quote! { { #(#parts)* } })
        }
        Expr::Cast { expr, target } => emit_cast(expr, target),
        Expr::HostIntrinsic { name, args } => emit_host_intrinsic(name, args),
        Expr::Closure(func) => emit_closure(func),
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
    match prop {
        MemberProp::Ident(field) => {
            let obj_tokens = emit_expr(obj)?;
            let field_ident = format_ident!("{}", to_snake_case(field));
            Ok(quote! { #obj_tokens.#field_ident })
        }
        // A numeric-literal computed index (`args[0]`) is the one recognized computed-member
        // shape — every occurrence in the surveyed source indexes an apply/construct closure's
        // `&[T::Value]` arguments slice, which (unlike a real JS array) panics on an
        // out-of-bounds index rather than yielding `undefined`; `.get(...).cloned().unwrap_or
        // (undefined)` restores JS's permissive semantics explicitly. Any other computed-member
        // shape (a dynamic string-keyed lookup, e.g. `descriptor[key]`) remains unsupported — it
        // would need a real by-key dynamic accessor this emitter doesn't have.
        MemberProp::Computed(index) if matches!(index.as_ref(), Expr::Lit(Lit::Num(_))) => {
            let obj_tokens = emit_expr(obj)?;
            let Expr::Lit(Lit::Num(n)) = index.as_ref() else { unreachable!() };
            let n = *n as usize;
            Ok(quote! { (#obj_tokens).get(#n).cloned().unwrap_or_else(|| tenant.undefined_value()) })
        }
        MemberProp::Computed(_) => Err(IrError::Unsupported {
            file: String::new(),
            construct: "dynamic (non-numeric-literal) computed member access is not yet covered by the Rust emitter".into(),
        }),
    }
}

/// Dispatches an object literal to whichever of the three recognized shapes it matches: a
/// `TenantInvocation` (`{ kind: "apply", thisArg, args }` / `{ kind: "construct", args,
/// newTarget }`, always the second argument to `tenant.invoke(...)` — see
/// `try_emit_tenant_invocation_literal`), a `TenantPropertyDescriptor` control record (every key
/// drawn from `DESCRIPTOR_FIELDS`, see `emit_descriptor_object_literal`), or a literal matching
/// some locally-`interface`-declared struct's exact field set (`{ Object: ObjectFn,
/// ObjectPrototype }`, see `emit_struct_literal`). Anything else is unsupported.
fn emit_object_literal(props: &[ObjectProp]) -> Result<TokenStream, IrError> {
    if let Some(tokens) = try_emit_tenant_invocation_literal(props)? {
        return Ok(tokens);
    }
    let keys: Vec<&str> = props
        .iter()
        .filter_map(|p| match p {
            ObjectProp::KeyValue { key: PropKey::Ident(k), .. } => Some(k.as_str()),
            _ => None,
        })
        .collect();
    if !keys.is_empty() && keys.iter().all(|k| DESCRIPTOR_FIELDS.contains(k)) {
        return emit_descriptor_object_literal(props);
    }
    let matching_struct = STRUCT_DEFS.with(|defs| {
        defs.borrow()
            .values()
            .find(|def| def.fields.len() == keys.len() && def.fields.iter().all(|(name, _)| keys.contains(&name.as_str())))
            .cloned()
    });
    if let Some(def) = matching_struct {
        return emit_struct_literal(&def, props);
    }
    Err(IrError::Unsupported {
        file: String::new(),
        construct: "object literal doesn't match a known TenantPropertyDescriptor or locally-declared interface shape".into(),
    })
}

/// Recognizes `{ kind: "apply", thisArg, args }` / `{ kind: "construct", args, newTarget }` —
/// the two `TenantInvocation` shapes constructed as `tenant.invoke(...)`'s second argument.
/// `args`'s value is `.to_vec()`-ed regardless of whether the source expression is already an
/// owned `Vec<T::Value>` (an array literal, `guestArrayLike`'s result) or a borrowed slice
/// (`args.slice(N)`) — a redundant clone in the former case, but uniform and always correct,
/// which matters more here than avoiding one extra `Vec` allocation in a builtin-call path.
fn try_emit_tenant_invocation_literal(props: &[ObjectProp]) -> Result<Option<TokenStream>, IrError> {
    let mut kind: Option<&str> = None;
    let mut this_arg: Option<&Expr> = None;
    let mut args: Option<&Expr> = None;
    let mut new_target: Option<&Expr> = None;
    for prop in props {
        let ObjectProp::KeyValue { key: PropKey::Ident(k), value } = prop else { return Ok(None) };
        match k.as_str() {
            "kind" => {
                let Expr::Lit(Lit::Str(s)) = value else { return Ok(None) };
                kind = Some(s.as_str());
            }
            "thisArg" => this_arg = Some(value),
            "args" => args = Some(value),
            "newTarget" => new_target = Some(value),
            _ => return Ok(None),
        }
    }
    match kind {
        Some("apply") => {
            let (Some(this_arg), Some(args)) = (this_arg, args) else { return Ok(None) };
            if new_target.is_some() {
                return Ok(None);
            }
            let this_arg_tokens = emit_expr(this_arg)?;
            let args_tokens = emit_expr(args)?;
            Ok(Some(quote! {
                TenantInvocation::Apply { this_arg: (#this_arg_tokens).clone(), args: (#args_tokens).to_vec() }
            }))
        }
        Some("construct") => {
            let (Some(args), Some(new_target)) = (args, new_target) else { return Ok(None) };
            if this_arg.is_some() {
                return Ok(None);
            }
            let new_target_tokens = emit_expr(new_target)?;
            let args_tokens = emit_expr(args)?;
            Ok(Some(quote! {
                TenantInvocation::Construct { args: (#args_tokens).to_vec(), new_target: (#new_target_tokens).clone() }
            }))
        }
        _ => Ok(None),
    }
}

/// Recognizes an object literal whose keys are drawn entirely from `TenantPropertyDescriptor`'s
/// six known fields and emits a real struct literal (`Some(...)`-wrapping each present field,
/// defaulting absent ones to `None`) — these literals are host-side control records, per
/// `tenants/types.ts`'s own doc comment on `TenantPropertyDescriptor`, not guest-visible objects.
/// A single leading `...spread` (`{ ...descriptor, configurable: false }`, `lock`'s clone-and-
/// override idiom) is also recognized: the base struct's own value is cloned via `..` struct-
/// update syntax and only the explicitly-listed fields override it.
fn emit_descriptor_object_literal(props: &[ObjectProp]) -> Result<TokenStream, IrError> {
    let mut base: Option<&Expr> = None;
    let mut fields: Vec<(&str, &Expr)> = Vec::new();
    for prop in props {
        match prop {
            ObjectProp::Spread(e) => {
                if base.is_some() {
                    return Err(IrError::Unsupported {
                        file: String::new(),
                        construct: "object literal with more than one spread".into(),
                    });
                }
                base = Some(e);
            }
            ObjectProp::KeyValue { key: PropKey::Ident(key), value } if DESCRIPTOR_FIELDS.contains(&key.as_str()) => {
                fields.push((key.as_str(), value));
            }
            _ => {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "descriptor object literal member outside the recognized shape".into(),
                });
            }
        }
    }
    match base {
        None => {
            let mut entries = Vec::new();
            for name in DESCRIPTOR_FIELDS {
                let ident = format_ident!("{}", name);
                entries.push(match fields.iter().find(|(key, _)| key == name) {
                    Some((_, value)) => {
                        let value = emit_expr(value)?;
                        if VALUE_TYPED_DESCRIPTOR_FIELDS.contains(name) {
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
        Some(base_expr) => {
            let base_tokens = emit_expr(base_expr)?;
            let mut entries = Vec::new();
            for (name, value) in &fields {
                let ident = format_ident!("{}", name);
                let value_tokens = emit_expr(value)?;
                let wrapped = if VALUE_TYPED_DESCRIPTOR_FIELDS.contains(name) {
                    quote! { Some((#value_tokens).clone()) }
                } else {
                    quote! { Some(#value_tokens) }
                };
                entries.push(quote! { #ident: #wrapped });
            }
            Ok(quote! { TenantPropertyDescriptor { #(#entries,)* ..(#base_tokens).clone() } })
        }
    }
}

fn emit_struct_literal(def: &StructDef, props: &[ObjectProp]) -> Result<TokenStream, IrError> {
    let struct_ident = format_ident!("{}", def.name);
    let mut entries = Vec::new();
    for (field_name, _ty) in &def.fields {
        let value_expr = props.iter().find_map(|p| match p {
            ObjectProp::KeyValue { key: PropKey::Ident(k), value } if k == field_name => Some(value),
            _ => None,
        });
        let Some(value_expr) = value_expr else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("struct literal for `{}` missing field `{field_name}`", def.name),
            });
        };
        let field_ident = format_ident!("{}", to_snake_case(field_name));
        let value_tokens = emit_expr(value_expr)?;
        entries.push(quote! { #field_ident: (#value_tokens).clone() });
    }
    Ok(quote! { #struct_ident { #(#entries),* } })
}

/// `[...a, ...b]`/`[a, b]` on host-native bookkeeping arrays (`Function.prototype.bind`'s
/// `[...prefix, ...callArgs]`) — never a guest-visible array. Each spread element is required
/// (a plain non-spread element mixed into a spread-containing literal would need `once((x,))`
/// wrapping, not observed in the surveyed source) so every element is uniformly chained.
fn emit_array_literal(elements: &[ArrayElement]) -> Result<TokenStream, IrError> {
    if elements.iter().any(|e| matches!(e, ArrayElement::Spread(_))) {
        let mut chain: Option<TokenStream> = None;
        for element in elements {
            let ArrayElement::Spread(e) = element else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "array literal mixing spread and non-spread elements".into(),
                });
            };
            let tokens = emit_expr(e)?;
            let part = quote! { (#tokens).iter().cloned() };
            chain = Some(match chain {
                None => part,
                Some(prev) => quote! { (#prev).chain(#part) },
            });
        }
        let chain = chain.unwrap();
        return Ok(quote! { (#chain).collect::<Vec<_>>() });
    }
    let mut items = Vec::new();
    for element in elements {
        let ArrayElement::Normal(e) = element else { unreachable!() };
        items.push(emit_expr(e)?);
    }
    Ok(quote! { vec![#(#items),*] })
}

fn emit_assign(target: &Expr, value: &Expr) -> Result<TokenStream, IrError> {
    match target {
        Expr::Member { obj, prop: MemberProp::Ident(field) } if DESCRIPTOR_FIELDS.contains(&field.as_str()) => {
            let obj_tokens = emit_expr(obj)?;
            let field_ident = format_ident!("{}", field);
            let value_tokens = emit_expr(value)?;
            let wrapped = if VALUE_TYPED_DESCRIPTOR_FIELDS.contains(&field.as_str()) {
                quote! { Some((#value_tokens).clone()) }
            } else {
                quote! { Some(#value_tokens) }
            };
            Ok(quote! { #obj_tokens.#field_ident = #wrapped })
        }
        Expr::Ident(name) => {
            let ident = format_ident!("{}", to_snake_case(name));
            let value_tokens = emit_expr(value)?;
            Ok(quote! { #ident = #value_tokens })
        }
        _ => Err(IrError::Unsupported {
            file: String::new(),
            construct: "assignment target is not yet covered by the Rust emitter".into(),
        }),
    }
}

fn emit_cond(test: &Expr, cons: &Expr, alt: &Expr) -> Result<TokenStream, IrError> {
    if let Some((name, none_branch, some_branch)) = option_narrow_cond(test, cons, alt)
        && matches!(none_branch, Expr::Lit(Lit::Null) | Expr::Lit(Lit::Undefined))
    {
        let ident = format_ident!("{}", to_snake_case(&name));
        // The ternary's own result is an ordinary guest `T::Value`, not `Option<T::Value>` — the
        // "none" arm is the literal `null`/`undefined` guest value (already correctly rendered
        // by `emit_expr`'s `Expr::Lit` handling as `tenant.null_value()`/`undefined_value()`),
        // and the "some" arm's own expression is used unwrapped. Only the *match scrutinee* —
        // the outer host-side `Option<_>` being narrowed — is real Rust `Option` machinery.
        let none_tokens = emit_expr(none_branch)?;
        let some_tokens = emit_expr(some_branch)?;
        return Ok(quote! {
            match #ident {
                None => #none_tokens,
                Some(ref #ident) => #some_tokens,
            }
        });
    }
    let test_tokens = emit_expr(test)?;
    let cons_tokens = emit_expr(cons)?;
    let alt_tokens = emit_expr(alt)?;
    Ok(quote! { (if #test_tokens { #cons_tokens } else { #alt_tokens }) })
}

/// If `test` is `IDENT === null|undefined` or `IDENT !== null|undefined` for a known
/// `Option<_>`-typed `IDENT`, returns `(name, none_branch_expr, some_branch_expr)` — the ternary
/// arm that runs when `IDENT` is `None`, and the one that runs when it's `Some`, in that order
/// regardless of which literal `Eq`/`NotEq` puts on which side. See `emit_cond`'s doc comment.
fn option_narrow_cond<'a>(test: &Expr, cons: &'a Expr, alt: &'a Expr) -> Option<(String, &'a Expr, &'a Expr)> {
    let Expr::Bin { op: op @ (BinOp::Eq | BinOp::NotEq), lhs, rhs } = test else { return None };
    if !matches!(rhs.as_ref(), Expr::Lit(Lit::Null) | Expr::Lit(Lit::Undefined)) {
        return None;
    }
    let Expr::Ident(name) = lhs.as_ref() else { return None };
    if !is_option_local(&to_snake_case(name)) {
        return None;
    }
    Some(if *op == BinOp::Eq { (name.clone(), cons, alt) } else { (name.clone(), alt, cons) })
}

fn emit_bin(op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<TokenStream, IrError> {
    match op {
        BinOp::Nullish => {
            let lhs_t = emit_expr(lhs)?;
            let rhs_t = emit_expr(rhs)?;
            Ok(quote! { (#lhs_t).unwrap_or(#rhs_t) })
        }
        BinOp::In => {
            let Expr::Lit(Lit::Str(key)) = lhs else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "`in` operator with a non-string-literal left-hand side".into(),
                });
            };
            let rhs_t = emit_call_arg(rhs, shims::ArgKind::Ref)?;
            Ok(quote! { (#rhs_t).has_field(#key) })
        }
        BinOp::Eq | BinOp::NotEq => emit_eq_cmp(op, lhs, rhs),
        _ => {
            let lhs_t = emit_expr(lhs)?;
            let rhs_t = emit_expr(rhs)?;
            let op_t = bin_op_tokens(op)?;
            Ok(quote! { (#lhs_t #op_t #rhs_t) })
        }
    }
}

fn is_nullish_lit(e: &Expr) -> bool {
    matches!(e, Expr::Lit(Lit::Null) | Expr::Lit(Lit::Undefined))
}

fn value_tag_variant(tag: &str) -> Option<TokenStream> {
    Some(match tag {
        "undefined" => quote! { Undefined },
        "object" => quote! { Object },
        "function" => quote! { Function },
        "string" => quote! { String },
        "number" => quote! { Number },
        "boolean" => quote! { Boolean },
        "symbol" => quote! { Symbol },
        _ => return None,
    })
}

/// `===`/`!==` need several distinct Rust translations depending on what's being compared to
/// what — see this module's doc comment's "`Tenant::Value` is opaque" point. Tried in order:
/// `typeof X === "<tag>"` (-> `Tenant::typeof_tag`), a `TenantPropertyDescriptor` field read
/// against `undefined` (-> `Option::is_some`/`is_none`, the field's Rust type is already
/// `Option<_>`), a known `Option<_>`-typed local against `null`/`undefined` (same), and finally a
/// generic opaque `T::Value` against `null`/`undefined` (-> `Tenant::typeof_tag` again, since
/// there's no other way to ask an opaque value "are you null").
fn emit_eq_cmp(op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<TokenStream, IrError> {
    if let Expr::Un { op: UnOp::TypeOf, arg } = lhs
        && let Expr::Lit(Lit::Str(tag)) = rhs
        && let Some(variant) = value_tag_variant(tag)
    {
        let arg_t = emit_call_arg(arg, shims::ArgKind::Ref)?;
        let cmp = if op == BinOp::Eq { quote! { == } } else { quote! { != } };
        return Ok(quote! { (tenant.typeof_tag(#arg_t) #cmp ValueTag::#variant) });
    }
    if is_nullish_lit(rhs) {
        if let Expr::Member { obj, prop: MemberProp::Ident(field) } = lhs
            && DESCRIPTOR_FIELDS.contains(&field.as_str())
        {
            let obj_t = emit_expr(obj)?;
            let field_ident = format_ident!("{}", field);
            return Ok(if op == BinOp::Eq {
                quote! { (#obj_t.#field_ident.is_none()) }
            } else {
                quote! { (#obj_t.#field_ident.is_some()) }
            });
        }
        if let Expr::Ident(name) = lhs
            && is_option_local(&to_snake_case(name))
        {
            let lhs_t = emit_expr(lhs)?;
            return Ok(if op == BinOp::Eq { quote! { (#lhs_t.is_none()) } } else { quote! { (#lhs_t.is_some()) } });
        }
        let lhs_t = emit_call_arg(lhs, shims::ArgKind::Ref)?;
        let variant = if matches!(rhs, Expr::Lit(Lit::Null)) { quote! { Null } } else { quote! { Undefined } };
        let cmp = if op == BinOp::Eq { quote! { == } } else { quote! { != } };
        return Ok(quote! { (tenant.typeof_tag(#lhs_t) #cmp ValueTag::#variant) });
    }
    if is_nullish_lit(lhs) {
        return emit_eq_cmp(op, rhs, lhs);
    }
    let lhs_t = emit_expr(lhs)?;
    let rhs_t = emit_expr(rhs)?;
    let op_t = bin_op_tokens(op)?;
    Ok(quote! { (#lhs_t #op_t #rhs_t) })
}

/// `EXPR as TARGET` — the two recognized non-erasable targets are `PropertyKey` (->
/// `Tenant::to_property_key`) and `object | null` (-> `Tenant::nullable`); anything else is
/// treated as pure type-level erasure, same as before this node existed.
fn emit_cast(expr: &Expr, target: &TypeRef) -> Result<TokenStream, IrError> {
    match target {
        TypeRef::Named(n) if n == "PropertyKey" => {
            let e = emit_call_arg(expr, shims::ArgKind::Ref)?;
            Ok(quote! { tenant.to_property_key(#e) })
        }
        TypeRef::Optional(inner) if matches!(inner.as_ref(), TypeRef::Named(n) if n == "object" || n == "Function") => {
            let e = emit_call_arg(expr, shims::ArgKind::Ref)?;
            Ok(quote! { tenant.nullable(#e) })
        }
        _ => emit_expr(expr),
    }
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

    if let Expr::Ident(name) = callee
        && let Some(source) = imported_source(name)
        && let Some(factory) = cross_file::lookup(&source, name)
    {
        let [CallArg::Normal(Expr::Ident(tenant_arg))] = args else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("{name}(...) called with an argument shape other than the bare `tenant` this crate expects"),
            });
        };
        if tenant_arg != "tenant" {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("{name}(...) called with a non-`tenant` argument"),
            });
        }
        let path: TokenStream = factory.rust_fn_path.parse().map_err(|e| IrError::Unsupported {
            file: String::new(),
            construct: format!("cross-file factory `{name}` rust_fn_path did not parse as Rust: {e}"),
        })?;
        let cache_ident = format_ident!("{}", cross_file::cache_param_name(factory));
        // Never `?`-suffixed here either — same reasoning as the local-function-call and shim
        // branches: every real call site wraps this in `yield tenant.yieldTenant(...)`, which
        // already appends the `?`.
        return Ok(quote! { #path(tenant, #cache_ident) });
    }

    if let Expr::Ident(name) = callee
        && let Some(param_tys) = LOCAL_FN_PARAMS.with(|m| m.borrow().get(name).cloned())
    {
        let fn_ident = format_ident!("{}", to_snake_case(name));
        if args.len() != param_tys.len() {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("{name}(...) called with {} args, expected {}", args.len(), param_tys.len()),
            });
        }
        let mut arg_tokens = Vec::new();
        for (arg, ty) in args.iter().zip(param_tys.iter()) {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("spread argument to local function call `{name}`"),
                });
            };
            let kind = ty.as_ref().map(arg_kind_for_type).unwrap_or(shims::ArgKind::Owned);
            arg_tokens.push(emit_call_arg(expr, kind)?);
        }
        // Never `?`-suffixed here: every local function in the surveyed source is only ever
        // called wrapped in `TenantYield` (`yield tenant.yieldTenant(installMethod(...))`),
        // which already appends the `?` — see `emit_shim_call`'s matching note.
        return Ok(quote! { #fn_ident(#(#arg_tokens),*) });
    }

    if let Expr::Closure(func) = callee {
        // An immediately-invoked closure with no captured/injected `tenant` reference of its
        // own is not a shape this crate's IR lowering ever actually produces (every closure in
        // the surveyed source is passed to `makeBuiltin`/`installMethod`, never IIFE'd) — kept
        // as an explicit rejection rather than falling through to the generic error below, so a
        // future real occurrence gets a specific message instead of a generic one.
        let _ = func;
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "immediately-invoked closure expression".into(),
        });
    }

    Err(IrError::Unsupported {
        file: String::new(),
        construct: "call expression is not yet covered by the Rust emitter (only tenant.<method>(...) calls, shimmed external calls, and locally-defined function calls are)".into(),
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

/// A closure/function-expression argument (every occurrence in the surveyed source is passed
/// straight to `makeBuiltin`/`installMethod` as its `apply`/`construct` argument — see
/// `lower.rs`'s `lower_closure_bearing_call`, which is also what gives every closure's own
/// params their types). Emits `make_builtin`'s expected shape directly: `fn(&mut T, T::Value,
/// &[T::Value]) -> Result<T::Value, TenantError>`, with `tenant` injected as an explicit leading
/// closure parameter (TS closes over it lexically instead) and every other outer name the body
/// references captured by cloning it into a same-named shadow binding just before the closure
/// literal — see this module's doc comment's closure-capture point for why cloning-and-shadowing
/// needs no rewriting of the closure body itself.
/// The closure body's own declared parameter names — used to pad a JS closure that declares
/// fewer than 2 params (e.g. `Function`'s own `function* () { throw ...; }` apply/construct,
/// which ignores both) out to the fixed 2-param Rust shape `make_builtin` always requires:
/// unlike JS, a Rust closure's arity is part of its type, so the trailing parameters missing
/// from the source still need *some* declared (unused) name.
const SYNTHETIC_PARAM_NAMES: [&str; 2] = ["_unused_this_arg", "_unused_args"];

fn emit_closure(func: &FnDecl) -> Result<TokenStream, IrError> {
    if func.params.len() > 2 {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!(
                "closure with {} params (only the 2-param apply/construct convention — thisArg/newTarget, args — is recognized)",
                func.params.len()
            ),
        });
    }
    let mut param_tokens = Vec::new();
    let mut own_param_names = Vec::new();
    for i in 0..2 {
        let name = match func.params.get(i) {
            Some(Param { pattern: Pattern::Ident(name), .. }) => to_snake_case(name),
            Some(Param { pattern: Pattern::ObjectShallow(_), .. }) => {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "destructured closure parameter".into(),
                });
            }
            None => SYNTHETIC_PARAM_NAMES[i].to_string(),
        };
        let ty_tokens = if i == 0 { quote! { T::Value } } else { quote! { &[T::Value] } };
        let ident = format_ident!("{name}");
        param_tokens.push(quote! { #ident: #ty_tokens });
        own_param_names.push((name, i == 1));
    }

    let saved_refness = LOCAL_REFNESS.with(|m| m.borrow().clone());
    let saved_options = OPTION_LOCALS.with(|m| m.borrow().clone());
    // The *candidate* capture set is exactly the outer scope's own locals, snapshotted before
    // this closure's own params (and, once the body below is emitted, its own internal `let`s)
    // get inserted into the same flat table — using the table's state *after* emitting the body
    // would wrongly treat the closure's own internal locals as captures too (anything the
    // closure itself binds is, by definition, referenced in its own body).
    let outer_names: Vec<String> = saved_refness.keys().cloned().collect();
    LOCAL_REFNESS.with(|m| {
        let mut m = m.borrow_mut();
        for (name, is_slice) in &own_param_names {
            m.insert(name.clone(), *is_slice);
        }
    });

    let body = emit_block(&func.body)?;

    let body_str = body.to_string();
    let captured: Vec<String> = outer_names
        .into_iter()
        .filter(|name| name != "tenant")
        .filter(|name| word_referenced(&body_str, name))
        .collect();

    LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved_refness);
    OPTION_LOCALS.with(|m| *m.borrow_mut() = saved_options);

    let clone_prelude: Vec<TokenStream> = captured
        .iter()
        .map(|name| {
            let ident = format_ident!("{name}");
            quote! { let #ident = #ident.clone(); }
        })
        .collect();

    Ok(quote! {
        {
            #(#clone_prelude)*
            move |tenant: &mut T, #(#param_tokens),*| -> Result<T::Value, TenantError> {
                #body
            }
        }
    })
}

/// Whether `word` (an exact identifier, not a substring) appears anywhere in `haystack` — used
/// both for closure free-variable capture (`emit_closure`) and per-tenant-cache-parameter
/// injection (`emit_fn_decl`). Operating on the already-emitted token string rather than walking
/// the IR a second time is a deliberately coarse approximation: it can't distinguish a genuine
/// reference from an unrelated local that happens to share a name in a *different* scope, but no
/// name collision like that occurs anywhere in the surveyed source, and a false-positive capture
/// only costs an extra harmless `.clone()`/parameter — never a wrong answer.
fn word_referenced(haystack: &str, word: &str) -> bool {
    haystack.split(|c: char| !c.is_alphanumeric() && c != '_').any(|token| token == word)
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

/// Rust reserved words that plausibly collide with a TS identifier in this source (`const fn =
/// ...` in `types.ts`'s `makeBuiltin`/`installMethod` idiom is the one real occurrence so far).
/// Not exhaustive of Rust's full keyword list — extended as a real collision is found, the same
/// "hard rejection over silent guessing" spirit as everywhere else in this emitter, except here
/// a rename is unambiguously correct rather than a guess.
const RUST_KEYWORDS: &[&str] = &[
    "fn", "type", "match", "move", "loop", "impl", "trait", "struct", "enum", "let", "mut", "ref", "self", "Self",
    "super", "crate", "dyn", "async", "await", "as", "in", "for", "if", "else", "while", "return", "break",
    "continue", "true", "false", "where", "use", "mod", "pub", "static", "const", "unsafe", "extern", "box", "yield",
];

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
    if RUST_KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}
