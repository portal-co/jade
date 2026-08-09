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
    /// Locally-defined top-level function name -> its own declared return type (unwrapped from
    /// `TenantGenerator<X>` to just `X`, matching how `emit_fn_decl` reads it) — consulted by
    /// `returns_option` so a local bound from `yield tenant.yieldTenant(someLocalFn(...))` (e.g.
    /// `proxy.ts`'s `const result = ...trap(...)`) gets marked `Option`-typed when the callee's
    /// own return type resolves that way (`TrapResult` -> `Option<T::Value>`), the same as a
    /// `Tenant`/cross-file-class method already does.
    static LOCAL_FN_RETURN_TYPES: RefCell<HashMap<String, TypeRef>> = RefCell::new(HashMap::new());
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
    /// Local-variable-names (snake_case) bound from a call to a function returning `TrapResult`
    /// specifically (`proxy.ts`'s `trap`) — a narrower set than `OPTION_LOCALS` (every one of
    /// these is also in `OPTION_LOCALS`, but not every `OPTION_LOCALS` member has `.found`/
    /// `.value` sub-fields — `TypedArrayPrimordialImpl`'s `bufferRecord`, from `BufferPrimordial
    /// .record`, is directly the `Option<BufferRecord>` itself, checked with a bare `if
    /// (bufferRecord)`). Drives `emit_member`'s `.found` -> `.is_some()`/`.value` -> `.unwrap()`
    /// translation.
    static TRAP_RESULT_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// Unique-name counter for generated `TenantExoticHandler`-implementing structs (see
    /// `emit_exotic_handler_literal`) — one file can construct more than one exotic in different
    /// factory functions (`array-buffer.ts`'s buffer shell and, per kind, `typed-arrays.ts`'s
    /// typed-array records), so a flat per-module counter (not reset per function) keeps every
    /// generated struct name distinct.
    static EXOTIC_HANDLER_COUNTER: RefCell<u32> = RefCell::new(0);
    /// Which return-value coercion `Stmt::Return`'s emission applies — see `coerce_return_value`
    /// (the `Closure` default, matching `emit_closure`'s hardcoded `Result<T::Value,
    /// TenantError>` shape, used for every top-level function too since every one IR-lowered so
    /// far happens to also return a `T::Value`-shaped thing) vs `coerce_trap_return_value` (each
    /// `TenantExoticHandler` trap has its *own* real Rust return type — `bool`, `Vec<PropertyKey>`,
    /// `Option<T::Value>`, ... — so the closure-oriented coercion is actively wrong there, not
    /// just unnecessary: unwrapping an already-`Option<T::Value>` `getPrototypeOf` result to a
    /// bare `T::Value` would be a real type/semantic error, not a no-op). Set for the duration of
    /// each trap method's own body in `emit_exotic_handler_literal`, restored immediately after.
    static RETURN_COERCION: RefCell<ReturnCoercion> = RefCell::new(ReturnCoercion::Closure);
    /// Local-variable-name (snake_case) -> "this is a `&str`-typed local" set, reset per function
    /// (not per closure, matching `LOCAL_REFNESS`'s simplification — see its own doc comment).
    /// `installMethod`'s `name: string` parameter is the one occurrence so far: it's a plain
    /// string, not a `PropertyKey`, but reaches `defineData`'s `RefKey`-kind `key` argument —
    /// `emit_call_arg`'s `RefKey` case consults this to know it needs `PropertyKey::from(...)`
    /// conversion (already applied automatically for a literal string argument) rather than a
    /// bare `&`-borrow (correct for an argument that's already `PropertyKey`-typed, e.g. a `for`
    /// loop variable bound from `ownPropertyKeys`).
    static STRING_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// Local-variable-name (snake_case) -> "this is a `PropertyKey`-typed local" set — currently
    /// populated only by `emit_exotic_handler_literal` for each trap's own `key`-named parameter
    /// (`&PropertyKey`, per `EXOTIC_TRAPS`). `emit_eq_cmp` consults this the same way it consults
    /// `STRING_TYPED_LOCALS`: a bare identifier compared against a string literal
    /// (`key === "byteLength"`, common in exotic-handler traps) needs the literal converted via
    /// `PropertyKey::from(...)` before the two sides can be compared at all.
    static PROPERTY_KEY_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// Local-variable-name (snake_case) -> "this is specifically `Option<T::Value>`" set —
    /// narrower than `OPTION_LOCALS` (which also covers `Option<f64>` and other non-guest-value
    /// `Option`s). Populated only by `emit_exotic_handler_literal` for a trap parameter declared
    /// `"Option<T::Value>"` in [`EXOTIC_TRAPS`] (currently just `setPrototypeOf`'s `prototype`).
    /// Consulted wherever a bare guest-`Option<T::Value>` local needs different handling than an
    /// ordinary `T::Value` local: `emit_call_arg`'s `OptionalValue` case must pass it through
    /// as-is instead of re-wrapping in `Some(...)` (it's already `Option`-shaped), and
    /// `emit_value_slice_array_literal` must unwrap it to a real guest value via
    /// `Tenant::null_value()` for `None` (a `readonly unknown[]` slot needs a genuine `T::Value`,
    /// never a Rust `Option`).
    static OPTION_VALUE_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// Local-variable-name (snake_case) -> "this is a `BufferKind`-typed local" set, the same
    /// role as `PROPERTY_KEY_TYPED_LOCALS` but for `TYPE_SHIMS`'s `BufferKind` entry: `kind ===
    /// "shared-array-buffer"` needs to become `kind == BufferKind::SharedArrayBuffer`, not a
    /// (non-existent) `PartialEq<str>` comparison.
    static BUFFER_KIND_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// Local-variable-name (snake_case) -> "this is a bare host `f64`, not a guest `T::Value`"
    /// set — populated for a `let` binding whose initializer is a `BufferHooks::byte_length`
    /// call (see `is_buffer_hooks_usize_returning_call`; that call's own Rust translation is a
    /// real `f64`, not yet a guest value — see `BUFFER_HOOKS_METHODS`'s `returns_usize` handling
    /// in `emit_call`). Consulted by `emit_bin`'s `args[N] ?? LOCAL` handling: unlike a bare
    /// numeric *literal* fallback (`coerce_literal_to_value`), an f64-typed *local* fallback
    /// needs the same `Tenant::number_value` wrapping applied at read time, not at the `let`
    /// site, since the local is also used directly as a host `f64` elsewhere (`Math.min`/`max`).
    static F64_TYPED_LOCALS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    /// This module's own `class` declarations (`Item::ClassDef`s), by name — populated once at
    /// the top of `try_emit_module`, the same way `STRUCT_DEFS` is. Consulted by
    /// `class_instance_obj` and by field/method lookups throughout class-body emission.
    static CLASS_DEFS: RefCell<HashMap<String, ClassDef>> = RefCell::new(HashMap::new());
    /// This module's own string-literal-union type aliases (`Item::StringEnumDef`s), by name —
    /// populated once at the top of `try_emit_module`, the same way `STRUCT_DEFS`/`CLASS_DEFS`
    /// are. Consulted by `rust_type`'s fallback chain (after `STRUCT_DEFS`/`class_backing_interface`
    /// find no match) to resolve a reference to one of these as its generated enum type.
    static STRING_ENUM_DEFS: RefCell<HashMap<String, StringEnumDef>> = RefCell::new(HashMap::new());
    /// Local-variable-name (snake_case) -> class name, for locals known to hold an instance of
    /// one of this module's own classes — either from `new ClassName(...)` (`emit_stmt`'s
    /// `Stmt::Let` handling) or from `const X = this;` inside one of that class's own methods
    /// (`emit_class_def`'s `Stmt::Let{init: ThisArg}` recognition). Reset per top-level
    /// function/class-method (like `LOCAL_REFNESS`); *not* reset when entering a nested closure —
    /// `emit_closure` already saves/restores the maps it needs to keep an outer scope visible to
    /// a nested one the same way, and this one just rides along.
    static CLASS_INSTANCE_LOCALS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// The class whose method body is currently being emitted, if any — lets `emit_member`/
    /// `emit_assign` know what a bare `this`/`ThisArg` refers to. `None` outside class-method
    /// emission, where a bare `ThisArg` is the ordinary `thisArg`/`_this` closure-parameter
    /// sentinel it already was before classes existed.
    static CURRENT_CLASS: RefCell<Option<String>> = RefCell::new(None);
    /// (class name, method name) -> the extra trailing argument tokens a call to that method
    /// needs beyond its own declared TS parameters — populated once, up front (like
    /// `CLASS_DEFS`), by checking whether the method's *own* IR body mentions a cross-file
    /// factory's name or this module's own per-tenant-cache variable (a coarse text search over
    /// the IR's `Debug` output, in the same "conservative, false-positive-only-costs-an-unused-
    /// param" spirit as `word_referenced`'s other uses — see `maybe_hoist_arg`'s doc comment).
    /// `emit_call`'s class-instance-method-call branch consults this so a call site can thread
    /// the matching extra argument without re-emitting (or duplicating the logic of) the
    /// callee's own signature-building step in `emit_class_method`.
    static CLASS_METHOD_EXTRA_ARGS: RefCell<HashMap<(String, String), Vec<TokenStream>>> = RefCell::new(HashMap::new());
    /// Whether the function/method/closure body currently being emitted returns `Result<_,
    /// TenantError>` — consulted by `emit_stmt`'s `Stmt::Return` handling to decide whether
    /// `return EXPR;` needs `Ok(...)` wrapping. Always `true` for a closure (`emit_closure`
    /// hardcodes a `Result`-returning signature) and for every `TenantExoticHandler` trap
    /// (`RETURN_COERCION::Trap` always implies a `Result`-returning method) — every top-level
    /// function has also always been throwing so far too (every one calls a fallible tenant
    /// operation somewhere), so this has never mattered before class methods: a plain,
    /// non-generator, non-throwing method (`array-buffer.ts`'s `prototypeOf`) is the first case
    /// where it's actually `false`. Defaults to `true` (every emission context but a plain class
    /// method sets it that way anyway) so a missed call site fails safe toward the historically
    /// always-correct behavior rather than silently regressing it.
    static CURRENT_FN_THROWS: RefCell<bool> = RefCell::new(true);
    /// The module-level const name recognized as a `codecs`-shaped `DataView` table (see
    /// `try_emit_data_view_codec_table`), if any — at most one per module. `None` until such a
    /// table is actually emitted.
    static CODEC_TABLE_NAME: RefCell<Option<String>> = RefCell::new(None);
    /// The string-enum type name `CODEC_TABLE_NAME`'s table is keyed by (`"TypedArrayKind"`) —
    /// set alongside it by `try_emit_data_view_codec_table`. Needed wherever a captured codec-
    /// typed local's own Rust *type* has to be spelled out (`emit_exotic_handler_literal`'s
    /// struct-field declarations) rather than just a method call resolved by name.
    static CODEC_TABLE_ENUM: RefCell<Option<String>> = RefCell::new(None);
    /// Local-variable-name (snake_case) -> the string-enum type it holds, for locals bound from
    /// `#codec_table_name[kind]` (`typed-arrays.ts`'s `const codec = codecs[kind];`) — the table
    /// lookup itself is erased (see `emit_member`'s `CODEC_TABLE_NAME`-aware computed-access
    /// case), so such a local is really just `kind` again, and its own `.bytes`/`.get(...)`/
    /// `.set(...)` need to become `.codec_bytes()`/`.codec_get(...)`/`.codec_set(...)` inherent-
    /// method calls (generated by `try_emit_data_view_codec_table`) rather than field access.
    /// Reset per top-level function/class-method, like `CLASS_INSTANCE_LOCALS`.
    static CODEC_TYPED_LOCALS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
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

fn is_property_key_typed(name: &str) -> bool {
    PROPERTY_KEY_TYPED_LOCALS.with(|set| set.borrow().contains(name))
}

fn is_buffer_kind_typed(name: &str) -> bool {
    BUFFER_KIND_TYPED_LOCALS.with(|set| set.borrow().contains(name))
}

fn is_f64_typed(name: &str) -> bool {
    F64_TYPED_LOCALS.with(|set| set.borrow().contains(name))
}

fn class_def(name: &str) -> Option<ClassDef> {
    CLASS_DEFS.with(|m| m.borrow().get(name).cloned())
}

/// Whether `class_name`'s own `method_name` throws (a non-generator method, `Result`-returning
/// in Rust) — used by call sites (`emit_call`'s class-instance-method-call branch, `expr_throws`'s
/// own same-class-method-call case) to decide whether *their* call needs a trailing `?`. Not just
/// `block_throws(&method.func.body)` directly: that body's own `this.<otherMethod>()` calls
/// (`ProxyStateImpl::target`/`::handler` calling `require_live`) resolve through
/// `class_instance_obj`'s `Expr::ThisArg` case, which reads `CURRENT_CLASS` — but `CURRENT_CLASS`
/// reflects whatever class (if any) is *actually* being emitted right now, not `class_name`, when
/// this is called from an unrelated caller's context (a top-level function like `trap`, or a
/// different class's own method). Temporarily pointing `CURRENT_CLASS` at `class_name` for the
/// duration of this check keeps `this` resolving correctly regardless of who's asking.
fn class_method_throws(class_name: &str, method_name: &str) -> bool {
    let Some(def) = class_def(class_name) else { return false };
    let Some(method) = def.methods.iter().find(|m| m.name == method_name) else { return false };
    if method.func.is_generator {
        return false;
    }
    let saved = CURRENT_CLASS.with(|c| c.borrow().clone());
    CURRENT_CLASS.with(|c| *c.borrow_mut() = Some(class_name.to_string()));
    let throws = block_throws(&method.func.body);
    CURRENT_CLASS.with(|c| *c.borrow_mut() = saved);
    throws
}

/// The class name of a `new ClassName(...)` expression — either directly, or as the right-hand
/// side of `existing ?? new ClassName(...)` (`proxy.ts`'s `existingState ?? new
/// ProxyStateImpl(target, handler)`, reusing a possibly-passed-in instance or constructing a
/// fresh one) — see `Stmt::Let`'s `CLASS_INSTANCE_LOCALS` registration, which uses this instead of
/// matching `Expr::New` directly so both shapes register their local the same way.
fn new_class_instance_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::New { callee, .. } => {
            let Expr::Ident(name) = callee.as_ref() else { return None };
            class_def(name).is_some().then_some(name.as_str())
        }
        Expr::Bin { op: BinOp::Nullish, rhs, .. } => new_class_instance_name(rhs),
        _ => None,
    }
}

/// A same-module class's own name, if `interface_name` refers to a class directly (its own name)
/// or to an interface that class `implements` — `rust_type`'s fallback for a type reference with
/// no `StructDef` of its own (see `lower.rs`'s method-shaped-interface erasure, which leans on
/// this to resolve what such a name should actually mean in Rust).
fn class_backing_interface(interface_name: &str) -> Option<String> {
    CLASS_DEFS.with(|m| {
        let m = m.borrow();
        if m.contains_key(interface_name) {
            return Some(interface_name.to_string());
        }
        m.values().find(|def| def.implements.iter().any(|i| i == interface_name)).map(|def| def.name.clone())
    })
}

fn class_field_lookup(class_name: &str, field_name: &str) -> Option<ClassField> {
    class_def(class_name).and_then(|def| def.fields.into_iter().find(|f| f.name == field_name))
}

fn member_prop_field_name(prop: &MemberProp) -> Option<&str> {
    match prop {
        MemberProp::Ident(f) | MemberProp::Private(f) => Some(f.as_str()),
        MemberProp::Computed(_) => None,
    }
}

/// If `obj` is a known class-instance-typed expression — `this`/`ThisArg` while emitting a
/// method of a known class (`CURRENT_CLASS`), or a local registered in `CLASS_INSTANCE_LOCALS`
/// — returns the class name and the Rust tokens for a receiver expression reaching its wrapper
/// value (`self`, Rust's own method receiver, for `ThisArg`; the identifier itself otherwise).
fn class_instance_obj(obj: &Expr) -> Option<(String, TokenStream)> {
    match obj {
        Expr::ThisArg => CURRENT_CLASS.with(|c| c.borrow().clone()).map(|name| (name, quote! { self })),
        Expr::Ident(name) => {
            let snake = to_snake_case(name);
            CLASS_INSTANCE_LOCALS.with(|m| m.borrow().get(&snake).cloned()).map(|class_name| {
                let ident = format_ident!("{snake}");
                (class_name, quote! { #ident })
            })
        }
        _ => None,
    }
}

/// Whether `ty` (transitively, through `Optional`/`Array`/generic type arguments) involves a
/// guest `Tenant::Value` — used to decide whether a generated struct's own `impl`/type
/// parameter list needs `T: Tenant` at all (see `emit_struct_def`/`rust_type`'s struct-reference
/// case). A reference to another locally-declared struct counts too, since every one of those is
/// itself always at least `<T: Tenant>` today (see `emit_struct_def`).
fn type_needs_tenant(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Named(n) => {
            matches!(n.as_str(), "object" | "Function" | "unknown")
                || STRUCT_DEFS.with(|d| d.borrow().contains_key(n))
                // A field typed as a method-shaped interface backed by a same-module class (e.g.
                // `#buffers: BufferPrimordial`, backed by `BufferPrimordialImpl<T, H>`) needs `T`
                // too, the same as a direct struct reference — `class_backing_interface` is
                // `rust_type`'s own resolution path for exactly this case.
                || class_backing_interface(n).is_some()
                || cross_file::lookup_class(n).is_some_and(|c| c.needs_tenant)
        }
        TypeRef::Generic { args, .. } => args.iter().any(type_needs_tenant),
        TypeRef::Optional(inner) | TypeRef::Array(inner) => type_needs_tenant(inner),
    }
}

/// Whether `ty` involves `BufferHooks`'s associated `Handle` type (`array-buffer.ts`'s
/// `BufferHandle` type alias) — the `H: BufferHooks` analogue of `type_needs_tenant`.
fn type_needs_buffer_hooks(ty: &TypeRef) -> bool {
    match ty {
        // Same reasoning as `type_needs_tenant`'s `class_backing_interface` case: a field typed
        // as an interface backed by a class that itself needs `H` (e.g. `#buffers:
        // BufferPrimordial`, backed by `BufferPrimordialImpl<T, H>`) needs `H` too.
        TypeRef::Named(n) => {
            n == "BufferHandle"
                || n == "BufferHooks"
                || class_backing_interface(n).and_then(|c| class_def(&c)).is_some_and(|def| class_needs_buffer_hooks(&def))
                || cross_file::lookup_class(n).is_some_and(|c| c.needs_buffer_hooks)
        }
        TypeRef::Generic { args, .. } => args.iter().any(type_needs_buffer_hooks),
        TypeRef::Optional(inner) | TypeRef::Array(inner) => type_needs_buffer_hooks(inner),
    }
}

/// Whether `ty` is a `Map`/`WeakMap` generic type keyed by a guest value (`object`/`Function`/
/// `unknown`) — such a field is translated to `HashMap<T::ObjectId, V>` (see `rust_type`), so a
/// `.get`/`.set`/`.has`/`.delete` call against it needs its key argument converted via
/// `Tenant::object_id` rather than used bare (`T::Value` itself is not `Hash`/`Eq`).
fn is_object_identity_map_type(ty: &TypeRef) -> bool {
    matches!(ty, TypeRef::Generic { name, args } if (name == "Map" || name == "WeakMap") && args.len() == 2
        && matches!(&args[0], TypeRef::Named(n) if n == "object" || n == "Function" || n == "unknown"))
}

/// A class's own generic parameter list (`<T: Tenant>`, `<T: Tenant, H: BufferHooks>`, or plain
/// `<T: Tenant>` when nothing needs `H`) and the matching argument list (`<T>`/`<T, H>`) for
/// naming the type — every class observed so far needs `T` (each holds at least one `T::Value`
/// or references `tenant` in a method), so unlike `emit_struct_def` this doesn't bother making
/// `T` itself conditional.
/// The generated `{value_ty}Cache<...>` per-tenant-cache struct's own generic parameter list and
/// matching argument list — `T: Tenant` always, plus `H: BufferHooks` too when `value_ty`
/// resolves to a class that itself needs `H` (`BufferPrimordialCache`, backing
/// `BufferPrimordialImpl`, is the first such cache). Consulted everywhere `{value_ty}Cache` is
/// named: its own declaration (`emit_item`'s `PerTenantCache` arm) and every function/method
/// that gains a `cache: &mut {value_ty}Cache<...>` parameter (`emit_fn_decl`/`emit_class_method`).
fn per_tenant_cache_generics(value_ty: &str) -> (TokenStream, TokenStream) {
    let needs_buffer_hooks = class_backing_interface(value_ty).and_then(|c| class_def(&c)).is_some_and(|def| class_needs_buffer_hooks(&def));
    // `+ 'static` unconditionally on `T`, matching `class_generics`: a cache that can hold a
    // class instance (`BufferPrimordialImpl<T, H>`, itself always `T: Tenant + 'static`) needs
    // its own `T` to satisfy that bound too, and it's a harmless superset requirement otherwise
    // (see `emit_fn_decl`'s matching note on why this is always fine for a real `Tenant` impl).
    if needs_buffer_hooks {
        (quote! { T: Tenant + 'static, H: BufferHooks + 'static }, quote! { T, H })
    } else {
        (quote! { T: Tenant + 'static }, quote! { T })
    }
}

fn class_needs_buffer_hooks(def: &ClassDef) -> bool {
    def.fields.iter().any(|f| type_needs_buffer_hooks(&f.ty))
        || def
            .constructor
            .iter()
            .flat_map(|c| c.params.iter())
            .any(|p| p.ty.as_ref().is_some_and(type_needs_buffer_hooks))
        || def.methods.iter().any(|m| {
            m.func.params.iter().any(|p| p.ty.as_ref().is_some_and(type_needs_buffer_hooks))
                || m.func.return_type.as_ref().is_some_and(type_needs_buffer_hooks)
        })
}

fn class_generics(def: &ClassDef) -> (TokenStream, TokenStream) {
    if class_needs_buffer_hooks(def) {
        (quote! { T: Tenant + 'static, H: BufferHooks + 'static }, quote! { T, H })
    } else {
        (quote! { T: Tenant + 'static }, quote! { T })
    }
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

/// `jade-tenant-rt::BufferHooks`'s own trait methods (hand-written, not IR-derived — see
/// `crates/jade-tenant-rt/src/buffer.rs`), and each positional argument's passing convention.
/// Distinct from `shims::ArgKind`/`tenant_method`: every byte offset/length argument is `f64` on
/// the TS side (from `toIndex`) but `usize` on this trait's side, a conversion neither of those
/// tables' kinds express.
#[derive(Clone, Copy)]
enum BufferHooksArgKind {
    /// Borrowed as-is (`&Self::Handle`, or `write`'s `&[u8]` bytes).
    Ref,
    /// Passed by value with no conversion (`BufferKind`, `Option<BufferKind>`).
    Owned,
    /// Passed by value after an `as usize` conversion.
    UsizeFromF64,
}

struct BufferHooksMethodSpec {
    ts_name: &'static str,
    rust_name: &'static str,
    params: &'static [BufferHooksArgKind],
    /// Whether the call's own result needs `.map(|v| v as f64)` before the surrounding
    /// `TenantYield`'s `?` unwraps it — `byteLength` returns `usize` (matching `BufferHooks::
    /// byte_length`), but the TS source treats the result as an ordinary JS number, feeding it
    /// into `Math.min`/other already-`f64`-typed arithmetic on the Rust side.
    returns_usize: bool,
}

const BUFFER_HOOKS_METHODS: &[BufferHooksMethodSpec] = &[
    BufferHooksMethodSpec {
        ts_name: "allocate",
        rust_name: "allocate",
        params: &[BufferHooksArgKind::Owned, BufferHooksArgKind::UsizeFromF64],
        returns_usize: false,
    },
    BufferHooksMethodSpec {
        ts_name: "isHandle",
        rust_name: "is_handle",
        params: &[BufferHooksArgKind::Ref, BufferHooksArgKind::Owned],
        returns_usize: false,
    },
    BufferHooksMethodSpec {
        ts_name: "byteLength",
        rust_name: "byte_length",
        params: &[BufferHooksArgKind::Ref],
        returns_usize: true,
    },
    BufferHooksMethodSpec {
        ts_name: "slice",
        rust_name: "slice",
        params: &[BufferHooksArgKind::Ref, BufferHooksArgKind::UsizeFromF64, BufferHooksArgKind::UsizeFromF64],
        returns_usize: false,
    },
    BufferHooksMethodSpec {
        ts_name: "read",
        rust_name: "read",
        params: &[BufferHooksArgKind::Ref, BufferHooksArgKind::UsizeFromF64, BufferHooksArgKind::UsizeFromF64],
        returns_usize: false,
    },
    BufferHooksMethodSpec {
        ts_name: "write",
        rust_name: "write",
        params: &[BufferHooksArgKind::Ref, BufferHooksArgKind::UsizeFromF64, BufferHooksArgKind::Ref],
        returns_usize: false,
    },
];

fn buffer_hooks_method(name: &str) -> Option<&'static BufferHooksMethodSpec> {
    BUFFER_HOOKS_METHODS.iter().find(|m| m.ts_name == name)
}

fn emit_buffer_hooks_arg(expr: &Expr, kind: BufferHooksArgKind) -> Result<TokenStream, IrError> {
    // `hooks.allocate("array-buffer", ...)` — a `BufferKind`-typed argument fed a string literal
    // directly, the same shape `emit_call`'s class-instance-method-call branch already handles
    // for `that.shell(tenant, "array-buffer", ...)` via `buffer_kind_literal`.
    if let Some(variant) = buffer_kind_literal(expr) {
        return Ok(variant);
    }
    let tokens = emit_expr(expr)?;
    Ok(match kind {
        BufferHooksArgKind::Owned => tokens,
        BufferHooksArgKind::UsizeFromF64 => quote! { (#tokens) as usize },
        BufferHooksArgKind::Ref => {
            let already_ref = matches!(expr, Expr::Ident(name) if is_known_ref(&to_snake_case(name)));
            if already_ref {
                tokens
            } else {
                quote! { &(#tokens) }
            }
        }
    })
}

/// Whether `expr` is `yield tenant.yieldTenant(<class field>.#hooks.<method>(...))` for a
/// `usize`-returning `BufferHooks` method (`byteLength` today) — see
/// `coerce_trap_return_value`'s use of this: such a call's Rust translation is a real host
/// number (`f64`, via `BUFFER_HOOKS_METHODS`'s `returns_usize` handling in `emit_call`), not yet
/// a guest `T::Value`, when it flows directly into an exotic-trap `return`.
fn is_buffer_hooks_usize_returning_call(expr: &Expr) -> bool {
    let Expr::TenantYield(inner) = expr else { return false };
    let Expr::Call { callee, .. } = inner.as_ref() else { return false };
    let Expr::Member { obj: field_obj, prop: MemberProp::Ident(method) } = callee.as_ref() else { return false };
    let Expr::Member { obj: recv_obj, prop: recv_prop } = field_obj.as_ref() else { return false };
    let Some(field_name) = member_prop_field_name(recv_prop) else { return false };
    let Some((class_name, _)) = class_instance_obj(recv_obj) else { return false };
    let Some(field) = class_field_lookup(&class_name, field_name) else { return false };
    if !matches!(&field.ty, TypeRef::Named(n) if n == "BufferHooks") {
        return false;
    }
    buffer_hooks_method(method).is_some_and(|spec| spec.returns_usize)
}

/// `codec.get(...)` (a call to `CODEC_TYPED_LOCALS`'s own `codec_get`, returning a native `f64` —
/// see `emit_call`'s codec-typed-local branch) returned directly from a trap (`typed-arrays.ts`'s
/// `get` trap's array-index-read branch) needs the same `Tenant::number_value` wrapping a bare
/// numeric literal does.
fn is_codec_get_call(expr: &Expr) -> bool {
    let Expr::Call { callee, .. } = expr else { return false };
    let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee.as_ref() else { return false };
    let Expr::Ident(name) = obj.as_ref() else { return false };
    method == "get" && CODEC_TYPED_LOCALS.with(|m| m.borrow().contains_key(&to_snake_case(name)))
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
            } else if matches!(expr, Expr::Cast { target: TypeRef::Optional(_), .. })
                || matches!(expr, Expr::Ident(name) if OPTION_VALUE_TYPED_LOCALS.with(|m| m.borrow().contains(&to_snake_case(name))))
            {
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
    CLASS_DEFS.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for item in &module.items {
            if let Item::ClassDef(def) = item {
                map.insert(def.name.clone(), def.clone());
            }
        }
    });
    STRING_ENUM_DEFS.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for item in &module.items {
            if let Item::StringEnumDef(def) = item {
                map.insert(def.name.clone(), def.clone());
            }
        }
    });
    PER_TENANT_CACHE.with(|cache| {
        *cache.borrow_mut() = module.items.iter().find_map(|item| match item {
            Item::PerTenantCache { name, value_ty, .. } => Some((name.clone(), value_ty.clone())),
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
    LOCAL_FN_RETURN_TYPES.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for item in &module.items {
            if let Item::FnDecl(func) = item
                && let Some(name) = &func.name
            {
                let unwrapped = match &func.return_type {
                    Some(TypeRef::Generic { name, args }) if name == "TenantGenerator" && args.len() == 1 => Some(args[0].clone()),
                    other => other.clone(),
                };
                if let Some(ty) = unwrapped {
                    map.insert(name.clone(), ty);
                }
            }
        }
    });
    CLASS_METHOD_EXTRA_ARGS.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        let cache = PER_TENANT_CACHE.with(|c| c.borrow().clone());
        for item in &module.items {
            let Item::ClassDef(def) = item else { continue };
            for method in &def.methods {
                let debug_text = format!("{:?}", method.func);
                let mut extra = Vec::new();
                if let Some((cache_var, _)) = &cache
                    && word_referenced(&debug_text, cache_var)
                {
                    extra.push(quote! { cache });
                }
                for factory in cross_file::TABLE {
                    if word_referenced(&debug_text, factory.name) {
                        let cache_ident = format_ident!("{}", cross_file::cache_param_name(factory));
                        extra.push(quote! { #cache_ident });
                    }
                }
                if !extra.is_empty() {
                    map.insert((def.name.clone(), method.name.clone()), extra);
                }
            }
        }
    });
    EXOTIC_HANDLER_COUNTER.with(|c| *c.borrow_mut() = 0);

    let mut items = Vec::new();
    items.push(quote! {
        #[allow(unused_imports)]
        use portal_solutions_jade_tenant_rt::{Tenant, PropertyKey, TenantError, TenantPropertyDescriptor, TenantInvocation, TenantExoticHandler, DynFields, ValueTag, BufferKind, BufferHooks};
    });
    // Every registered cross-file factory's struct, unconditionally — small, fixed registry, so
    // this is simpler than scanning for which ones a given file actually destructures (see
    // `cross_file.rs`) — except the one defining *this* file's own struct: that would be a
    // self-import of a name already defined right below (`object.rs` generating a `use
    // crate::object::ObjectPrimordial;` for its own `ObjectPrimordial`), a hard duplicate-
    // definition error, not merely redundant.
    for factory in cross_file::TABLE {
        // `factory.struct_name` may name either a `StructDef` (`ObjectPrimordial`) or a
        // `ClassDef` (`BufferPrimordialImpl`, method-shaped interfaces having no `StructDef` of
        // their own — see `lower.rs`) — either means this *is* that file, so skip the self-import.
        if STRUCT_DEFS.with(|d| d.borrow().contains_key(factory.struct_name))
            || CLASS_DEFS.with(|d| d.borrow().contains_key(factory.struct_name))
        {
            continue;
        }
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
        Item::ClassDef(def) => format!("class `{}`", def.name),
        Item::StringEnumDef(def) => format!("type `{}`", def.name),
    }
}

fn emit_item(item: &Item) -> Result<Option<TokenStream>, IrError> {
    match item {
        Item::TypeImport { .. } | Item::ValueImport { .. } => Ok(None),
        Item::StructDef(def) => Ok(Some(emit_struct_def(def)?)),
        Item::PerTenantCache { value_ty, .. } => {
            let cache_ident = format_ident!("{value_ty}Cache");
            // Resolves through the same fallback `rust_type` uses everywhere else: a plain
            // `StructDef`-backed interface becomes `Value<T>`, but `array-buffer.ts`'s
            // `BufferPrimordial` (method-shaped, erased at lowering — see `lower.rs`) resolves to
            // its backing class's own wrapper type (`BufferPrimordialImpl<T, H>`) instead, so
            // this can't just hand-roll `#value_ident<T>` the way it used to.
            let value_ty_tokens = rust_type(&TypeRef::Named(value_ty.clone()))?;
            let (generic_params, generic_args) = per_tenant_cache_generics(value_ty);
            Ok(Some(quote! {
                pub struct #cache_ident<#generic_params> { entry: Option<#value_ty_tokens> }
                impl<#generic_params> Default for #cache_ident<#generic_args> {
                    fn default() -> Self { Self { entry: None } }
                }
            }))
        }
        Item::ModuleConst { name, init: Expr::Object(props), .. } => {
            if let Some(tokens) = try_emit_data_view_codec_table(name, props)? {
                return Ok(Some(tokens));
            }
            Err(IrError::Unsupported {
                file: String::new(),
                construct: "module-level const emission".into(),
            })
        }
        Item::ModuleConst { .. } => Err(IrError::Unsupported {
            file: String::new(),
            construct: "module-level const emission".into(),
        }),
        Item::FnDecl(func) => Ok(Some(emit_fn_decl(func)?)),
        Item::ClassDef(def) => Ok(Some(emit_class_def(def)?)),
        // Skipped entirely if this name is already hand-shimmed (`BufferKind`, via `TYPE_SHIMS`)
        // — that table is the authority for those; regenerating would produce a redundant, unused
        // duplicate enum rather than a conflict (nothing references the bare local name), but
        // there's no reason to emit dead code.
        Item::StringEnumDef(def) if TYPE_SHIMS.iter().any(|(n, _)| *n == def.name) => Ok(None),
        Item::StringEnumDef(def) => Ok(Some(emit_string_enum_def(def))),
    }
}

/// Emits an `interface`'s generated struct plus a hand-written `Clone` impl (**not**
/// `#[derive(Clone)]`: the derive would add an overly strict `T: Clone` bound on the struct's
/// own generic parameter, when only `T::Value: Clone` — already guaranteed by the `Tenant`
/// supertrait bound — is actually needed by any field).
fn emit_struct_def(def: &StructDef) -> Result<TokenStream, IrError> {
    let struct_ident = format_ident!("{}", def.name);
    let (generic_params, generic_args) = struct_generics(def);
    let mut field_decls = Vec::new();
    let mut field_idents = Vec::new();
    for (name, ty) in &def.fields {
        let field_ident = format_ident!("{}", to_snake_case(name));
        let field_ty = rust_type(ty)?;
        field_decls.push(quote! { pub #field_ident: #field_ty });
        field_idents.push(field_ident);
    }
    Ok(quote! {
        pub struct #struct_ident<#generic_params> { #(#field_decls),* }
        impl<#generic_params> Clone for #struct_ident<#generic_args> {
            fn clone(&self) -> Self {
                Self { #(#field_idents: self.#field_idents.clone()),* }
            }
        }
    })
}

/// A generated `interface` struct's own generic parameter list and matching argument list.
/// `T: Tenant` is included whenever any field involves a guest `Tenant::Value` (the common
/// case, and every struct observed before `array-buffer.ts` needed exactly this); `H:
/// BufferHooks` is added too when a field involves `BufferHooks`'s associated `Handle` type
/// (`BufferRecord`'s `handle: BufferHandle` is the first such struct — see
/// `docs/proxy-and-buffer-primordial-gap-plan.md`). A struct needing neither gets no generic
/// parameters at all, rather than an unconditional (and, for such a struct, meaningless) `<T>`.
fn struct_generics(def: &StructDef) -> (TokenStream, TokenStream) {
    let needs_tenant = def.fields.iter().any(|(_, ty)| type_needs_tenant(ty));
    let needs_buffer_hooks = def.fields.iter().any(|(_, ty)| type_needs_buffer_hooks(ty));
    let mut params = Vec::new();
    let mut args = Vec::new();
    if needs_tenant {
        params.push(quote! { T: Tenant });
        args.push(quote! { T });
    }
    if needs_buffer_hooks {
        params.push(quote! { H: BufferHooks });
        args.push(quote! { H });
    }
    (quote! { #(#params),* }, quote! { #(#args),* })
}

/// The generic argument list to name a locally-declared struct's own type (`Foo<T>`, `Foo<T,
/// H>`, or plain `Foo` for one needing neither) — the type-reference-position counterpart to
/// `struct_generics`'s declaration-position list.
fn struct_type_args(def: &StructDef) -> TokenStream {
    let needs_tenant = def.fields.iter().any(|(_, ty)| type_needs_tenant(ty));
    let needs_buffer_hooks = def.fields.iter().any(|(_, ty)| type_needs_buffer_hooks(ty));
    let mut args = Vec::new();
    if needs_tenant {
        args.push(quote! { T });
    }
    if needs_buffer_hooks {
        args.push(quote! { H });
    }
    if args.is_empty() {
        quote! {}
    } else {
        quote! { <#(#args),*> }
    }
}

/// TS type alias name -> fully-qualified Rust path, for a type that (unlike every other
/// interface handled so far) already has a hand-written Rust counterpart rather than becoming a
/// generated struct — `array-buffer.ts`'s `BufferKind` string-literal union and the
/// `jade-tenant-rt::buffer::BufferKind` enum built for exactly this purpose are the first entry.
/// See `docs/proxy-and-buffer-primordial-gap-plan.md`'s "type-name shimming" note.
const TYPE_SHIMS: &[(&str, &str)] = &[("BufferKind", "portal_solutions_jade_tenant_rt::BufferKind")];

/// `BufferKind`'s two string-literal values, in `EXOTIC_TRAPS`-table style: the TS literal and
/// the matching Rust enum variant, consulted by `emit_eq_cmp` for `kind === "shared-array-buffer"`
/// -style comparisons against a known `BufferKind`-typed local (see `BUFFER_KIND_TYPED_LOCALS`).
const BUFFER_KIND_VARIANTS: &[(&str, &str)] = &[("array-buffer", "ArrayBuffer"), ("shared-array-buffer", "SharedArrayBuffer")];

/// If `expr` is a string literal matching one of `BUFFER_KIND_VARIANTS`, the matching
/// `BufferKind::Variant` tokens — see `emit_call`'s class-instance-method-call branch, the one
/// place a `BufferKind`-typed parameter is fed a literal directly (`that.shell(tenant,
/// "array-buffer", ...)`).
fn buffer_kind_literal(expr: &Expr) -> Option<TokenStream> {
    let Expr::Lit(Lit::Str(s)) = expr else { return None };
    let variant = BUFFER_KIND_VARIANTS.iter().find(|(lit, _)| lit == s)?.1;
    let variant_ident = format_ident!("{variant}");
    Some(quote! { portal_solutions_jade_tenant_rt::BufferKind::#variant_ident })
}

/// A cross-file factory's own cache type, fully qualified and with the right generic argument
/// list (`BufferPrimordialCache<T, H>` vs `ObjectPrimordialCache<T>` — see
/// `CrossFileFactory::cache_needs_buffer_hooks`'s doc comment for why this isn't always just
/// `<T>`). Shared by `emit_fn_decl`'s and `emit_class_method`'s identical cross-file
/// cache-parameter-threading logic.
fn cross_file_cache_type_tokens(factory: &cross_file::CrossFileFactory) -> Result<TokenStream, IrError> {
    let path: TokenStream = factory.cache_type_path.parse().map_err(|e| IrError::Unsupported {
        file: String::new(),
        construct: format!("cross-file factory `{}` cache_type_path did not parse as Rust: {e}", factory.name),
    })?;
    Ok(if factory.cache_needs_buffer_hooks { quote! { #path<T, H> } } else { quote! { #path<T> } })
}

/// PascalCases a string-literal enum variant's own source text into a valid Rust identifier
/// (`"array-buffer"` -> `"ArrayBuffer"`; `"Int8Array"` -> `"Int8Array"`, unchanged since it's
/// already one) — non-alphanumeric characters are dropped and start a new capitalized segment.
fn to_pascal_case_variant(s: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                out.extend(ch.to_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    out
}

/// Generates a real Rust `enum` for a [`StringEnumDef`], plus `AsRef<str>`/`Display` impls back to
/// each variant's original string literal — `AsRef<str>` for call sites like
/// `makeBuiltin(tenant, kind, ...)` where a TS string parameter is fed a value of this type (see
/// `make_builtin`'s own `impl AsRef<str>` parameter in `jade-primordial-rt::types_shim`);
/// `Display` for a template-literal interpolation (`` `Constructor ${kind} requires 'new'` ``).
fn emit_string_enum_def(def: &StringEnumDef) -> TokenStream {
    let enum_ident = format_ident!("{}", def.name);
    let variant_idents: Vec<_> = def.variants.iter().map(|v| format_ident!("{}", to_pascal_case_variant(v))).collect();
    let variant_strs: &Vec<&str> = &def.variants.iter().map(String::as_str).collect();
    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum #enum_ident { #(#variant_idents),* }
        impl AsRef<str> for #enum_ident {
            fn as_ref(&self) -> &str {
                match self {
                    #(#enum_ident::#variant_idents => #variant_strs),*
                }
            }
        }
        impl std::fmt::Display for #enum_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_ref())
            }
        }
    }
}

/// One entry extracted from `typed-arrays.ts`'s `codecs` table — see
/// `try_emit_data_view_codec_table`'s doc comment for the exact shape recognized.
struct DataViewCodecEntry {
    bytes: usize,
    /// The `DataView` method suffix (`"Int8"`, `"Uint16"`, ...) shared by this entry's getter and
    /// setter — note `Uint8Array`/`Uint8ClampedArray` share the *same* suffix (`"Uint8"`); the
    /// only difference between them is the setter's own value expression (a plain passthrough vs
    /// a `Math.max/min/round` clamp chain), captured separately in `set_value`.
    suffix: String,
    view_param: String,
    offset_param: String,
    value_param: String,
    /// The setter's own value expression, in terms of `value_param` — lowered through the
    /// ordinary `emit_expr` pipeline (which already handles the `Math.max/min/round` clamp chain
    /// `Uint8ClampedArray` needs, as pre-existing intrinsics), not hand-modeled here.
    set_value: Expr,
}

/// If `props` matches this exact object literal shape for some entry — `{ bytes: N, get: (view,
/// offset) => view.getSUFFIX(offset[, true]), set: (view, offset, value) =>
/// view.setSUFFIX(offset, VALUE[, true]) }` — the extracted [`DataViewCodecEntry`]. Anything else
/// returns `None`, meaning `props` isn't this shape (not an error — the caller falls through).
fn extract_data_view_codec_entry(props: &[ObjectProp]) -> Option<DataViewCodecEntry> {
    let mut bytes = None;
    let mut get_func = None;
    let mut set_func = None;
    for prop in props {
        match prop {
            ObjectProp::KeyValue { key: PropKey::Ident(k), value: Expr::Lit(Lit::Num(n)) } if k == "bytes" => {
                bytes = Some(*n as usize);
            }
            ObjectProp::KeyValue { key: PropKey::Ident(k), value: Expr::Closure(func) } if k == "get" => {
                get_func = Some(func.as_ref());
            }
            ObjectProp::KeyValue { key: PropKey::Ident(k), value: Expr::Closure(func) } if k == "set" => {
                set_func = Some(func.as_ref());
            }
            _ => return None,
        }
    }
    let (bytes, get_func, set_func) = (bytes?, get_func?, set_func?);

    let [Param { pattern: Pattern::Ident(view_param), .. }, Param { pattern: Pattern::Ident(offset_param), .. }] =
        get_func.params.as_slice()
    else {
        return None;
    };
    let [Stmt::Return(Some(Expr::Call { callee, args }))] = get_func.body.0.as_slice() else {
        return None;
    };
    let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee.as_ref() else {
        return None;
    };
    if !matches!(obj.as_ref(), Expr::Ident(n) if n == view_param) {
        return None;
    }
    let suffix = method.strip_prefix("get")?.to_string();
    match args.as_slice() {
        [CallArg::Normal(Expr::Ident(o))] if o == offset_param => {}
        [CallArg::Normal(Expr::Ident(o)), CallArg::Normal(Expr::Lit(Lit::Bool(true)))] if o == offset_param => {}
        _ => return None,
    }

    let [Param { pattern: Pattern::Ident(set_view_param), .. }, Param { pattern: Pattern::Ident(set_offset_param), .. }, Param { pattern: Pattern::Ident(value_param), .. }] =
        set_func.params.as_slice()
    else {
        return None;
    };
    if set_view_param != view_param || set_offset_param != offset_param {
        return None;
    }
    let [Stmt::Return(Some(Expr::Call { callee: set_callee, args: set_args }))] = set_func.body.0.as_slice() else {
        return None;
    };
    let Expr::Member { obj: set_obj, prop: MemberProp::Ident(set_method) } = set_callee.as_ref() else {
        return None;
    };
    if !matches!(set_obj.as_ref(), Expr::Ident(n) if n == view_param) {
        return None;
    }
    if set_method != &format!("set{suffix}") {
        return None;
    }
    let set_value = match set_args.as_slice() {
        [CallArg::Normal(Expr::Ident(o)), CallArg::Normal(value)] if o == offset_param => value.clone(),
        [CallArg::Normal(Expr::Ident(o)), CallArg::Normal(value), CallArg::Normal(Expr::Lit(Lit::Bool(true)))] if o == offset_param => {
            value.clone()
        }
        _ => return None,
    };

    Some(DataViewCodecEntry {
        bytes,
        suffix,
        view_param: view_param.clone(),
        offset_param: offset_param.clone(),
        value_param: value_param.clone(),
        set_value,
    })
}

/// The byte-level read expression for one `DataView` getter suffix, reading from `view` at
/// `offset` (both already-bound idents of the caller's own choosing).
fn data_view_get_tokens(suffix: &str, view: &proc_macro2::Ident, offset: &proc_macro2::Ident) -> Result<TokenStream, IrError> {
    Ok(match suffix {
        "Int8" => quote! { (#view[#offset] as i8) as f64 },
        "Uint8" => quote! { #view[#offset] as f64 },
        "Int16" => quote! { i16::from_le_bytes([#view[#offset], #view[#offset + 1]]) as f64 },
        "Uint16" => quote! { u16::from_le_bytes([#view[#offset], #view[#offset + 1]]) as f64 },
        "Int32" => quote! { i32::from_le_bytes([#view[#offset], #view[#offset + 1], #view[#offset + 2], #view[#offset + 3]]) as f64 },
        "Uint32" => quote! { u32::from_le_bytes([#view[#offset], #view[#offset + 1], #view[#offset + 2], #view[#offset + 3]]) as f64 },
        "Float32" => quote! { f32::from_le_bytes([#view[#offset], #view[#offset + 1], #view[#offset + 2], #view[#offset + 3]]) as f64 },
        "Float64" => quote! {
            f64::from_le_bytes([
                #view[#offset], #view[#offset + 1], #view[#offset + 2], #view[#offset + 3],
                #view[#offset + 4], #view[#offset + 5], #view[#offset + 6], #view[#offset + 7],
            ])
        },
        other => {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("`DataView` getter suffix `{other}`"),
            });
        }
    })
}

/// The byte-level write statement for one `DataView` setter suffix, writing `value_tokens`
/// (already-emitted Rust tokens for the setter's own value expression — see
/// [`DataViewCodecEntry::set_value`]) into `view` at `offset`.
fn data_view_set_tokens(
    suffix: &str,
    view: &proc_macro2::Ident,
    offset: &proc_macro2::Ident,
    value_tokens: &TokenStream,
) -> Result<TokenStream, IrError> {
    Ok(match suffix {
        "Int8" | "Uint8" => quote! { #view[#offset] = ((#value_tokens) as i64) as u8; },
        "Int16" | "Uint16" => quote! {
            let __bytes = (((#value_tokens) as i64) as u16).to_le_bytes();
            #view[#offset] = __bytes[0];
            #view[#offset + 1] = __bytes[1];
        },
        "Int32" | "Uint32" => quote! {
            let __bytes = (((#value_tokens) as i64) as u32).to_le_bytes();
            #view[#offset..#offset + 4].copy_from_slice(&__bytes);
        },
        "Float32" => quote! {
            let __bytes = ((#value_tokens) as f32).to_le_bytes();
            #view[#offset..#offset + 4].copy_from_slice(&__bytes);
        },
        "Float64" => quote! {
            let __bytes = (#value_tokens).to_le_bytes();
            #view[#offset..#offset + 8].copy_from_slice(&__bytes);
        },
        other => {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("`DataView` setter suffix `{other}`"),
            });
        }
    })
}

/// Recognizes `typed-arrays.ts`'s `codecs` table shape — an object literal keyed by every variant
/// of some known `StringEnumDef`, each value a [`DataViewCodecEntry`] — and, if matched, emits
/// three inherent methods directly on that enum (`codec_bytes`/`codec_get`/`codec_set`) rather
/// than trying to represent `codecs`/`Codec` as real Rust values: recognized as a whole unit, the
/// same philosophy as `Item::PerTenantCache`, since the getter/setter closures have no explicit
/// parameter types of their own (TS infers them from `Codec`'s declared method signatures, which
/// have no `StructDef` translation either — function-typed struct fields aren't otherwise
/// modeled) and representing "one Rust value per `TypedArrayKind`" as a real runtime lookup would
/// need a `HashMap` rebuilt on every access or closures living in a `const`, neither of which is
/// necessary when the table is this small and fixed. Returns `Ok(None)`, not an error, if `props`
/// doesn't match — callers fall through to whatever else `Item::ModuleConst` supports.
///
/// Registers `name` in `CODEC_TABLE_NAME` on success, so later statement/expression emission
/// (`try_emit_expr_codec_index`, `emit_member`/`emit_call`'s codec-typed-local handling) knows
/// `#name[kind]` is really just `kind`, and a local bound from it needs `.bytes`/`.get(...)`/
/// `.set(...)` translated into the inherent methods generated here.
fn try_emit_data_view_codec_table(name: &str, props: &[ObjectProp]) -> Result<Option<TokenStream>, IrError> {
    let mut entries: Vec<(String, DataViewCodecEntry)> = Vec::new();
    for prop in props {
        let ObjectProp::KeyValue { key: PropKey::Ident(variant), value: Expr::Object(entry_props) } = prop else {
            return Ok(None);
        };
        let Some(entry) = extract_data_view_codec_entry(entry_props) else {
            return Ok(None);
        };
        entries.push((variant.clone(), entry));
    }
    if entries.is_empty() {
        return Ok(None);
    }
    let enum_name = STRING_ENUM_DEFS.with(|m| {
        m.borrow()
            .values()
            .find(|def| def.variants.len() == entries.len() && def.variants.iter().all(|v| entries.iter().any(|(k, _)| k == v)))
            .map(|def| def.name.clone())
    });
    let Some(enum_name) = enum_name else { return Ok(None) };

    // Every entry must agree on parameter names — this table's shape assumes one shared method
    // signature per inherent method, not per-variant naming.
    let (view_param, offset_param, value_param) = {
        let first = &entries[0].1;
        (first.view_param.clone(), first.offset_param.clone(), first.value_param.clone())
    };
    if !entries
        .iter()
        .all(|(_, e)| e.view_param == view_param && e.offset_param == offset_param && e.value_param == value_param)
    {
        return Ok(None);
    }

    let enum_ident = format_ident!("{enum_name}");
    let view_ident = format_ident!("{view_param}");
    let offset_ident = format_ident!("{offset_param}");
    let value_ident = format_ident!("{value_param}");

    let mut bytes_arms = Vec::new();
    let mut get_arms = Vec::new();
    let mut set_arms = Vec::new();
    for (variant, entry) in &entries {
        let variant_ident = format_ident!("{}", to_pascal_case_variant(variant));
        // `f64`, not `usize`: every other TS `number` in this IR becomes `f64` (byte offsets used
        // in surrounding arithmetic — e.g. `source.offset + i * codec.bytes` — are `f64` too), so
        // this stays consistent rather than forcing a mixed-type expression at every call site;
        // `codec_get`/`codec_set`'s own `offset` parameter is still `usize` for byte-slice
        // indexing, cast at the one call site that needs it (`emit_call`'s codec-typed-local
        // branch).
        let bytes = entry.bytes as f64;
        bytes_arms.push(quote! { #enum_ident::#variant_ident => #bytes });
        let get_expr = data_view_get_tokens(&entry.suffix, &view_ident, &offset_ident)?;
        get_arms.push(quote! { #enum_ident::#variant_ident => #get_expr });
        let value_tokens = emit_expr(&entry.set_value)?;
        let set_stmt = data_view_set_tokens(&entry.suffix, &view_ident, &offset_ident, &value_tokens)?;
        set_arms.push(quote! { #enum_ident::#variant_ident => { #set_stmt } });
    }

    CODEC_TABLE_NAME.with(|c| *c.borrow_mut() = Some(name.to_string()));
    CODEC_TABLE_ENUM.with(|c| *c.borrow_mut() = Some(enum_name.clone()));

    Ok(Some(quote! {
        impl #enum_ident {
            pub fn codec_bytes(&self) -> f64 {
                match self { #(#bytes_arms),* }
            }
            pub fn codec_get(&self, #view_ident: &[u8], #offset_ident: usize) -> f64 {
                match self { #(#get_arms),* }
            }
            pub fn codec_set(&self, #view_ident: &mut [u8], #offset_ident: usize, #value_ident: f64) {
                match self { #(#set_arms),* }
            }
        }
    }))
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
            // `array-buffer.ts`'s `BufferHandle` type alias (`= object`, but a *host* handle,
            // never a guest `Tenant::Value`) — depends on the local `H: BufferHooks` generic
            // parameter's own name, so unlike `TYPE_SHIMS`'s fixed absolute paths this is
            // special-cased directly rather than added to that table.
            "BufferHandle" => quote! { H::Handle },
            // `proxy.ts`'s `TrapResult` (`{ found: false } | { found: true; value: unknown }`) —
            // resolves directly to `Option<T::Value>` rather than a generated enum; see the type
            // alias's own doc comment in `proxy.ts` and `emit_object_lit`/`emit_member`'s
            // `TrapResult`-shaped-construction/`.found`/`.value` special cases, which recognize
            // the TS-side shape structurally rather than needing a real IR item for it.
            "TrapResult" => quote! { Option<T::Value> },
            // A struct/class *field* typed `BufferHooks` (e.g. `BufferPrimordialImpl`'s
            // `#hooks`) is the class's own `H: BufferHooks` generic parameter, owned outright —
            // unlike a `BufferHooks`-typed *function parameter*, which `emit_param` types as
            // `&mut H` instead (a fresh per-call borrow, not a value the callee keeps).
            "BufferHooks" => quote! { H },
            other if TYPE_SHIMS.iter().any(|(n, _)| *n == other) => {
                let path = TYPE_SHIMS.iter().find(|(n, _)| *n == other).unwrap().1;
                path.parse().map_err(|e| IrError::Unsupported {
                    file: String::new(),
                    construct: format!("type shim `{other}` path did not parse as Rust: {e}"),
                })?
            }
            other => {
                // `class_backing_interface` before `STRUCT_DEFS`: an interface a same-module
                // class explicitly `implements` should always resolve to that class, even when
                // the interface's own shape happens to *also* be plain-data enough to have gotten
                // a `StructDef` of its own (`TypedArrayPrimordial { constructors: ... }` — unlike
                // `BufferPrimordial`, which is method-shaped and has no `StructDef` at all, so this
                // ordering never mattered there). An explicit `implements` is a stronger signal
                // than an incidental shape match.
                if let Some(class_name) = class_backing_interface(other) {
                    let ident = format_ident!("{class_name}");
                    let def = class_def(&class_name).unwrap();
                    let (_, generic_args) = class_generics(&def);
                    quote! { #ident<#generic_args> }
                } else if let Some(def) = STRUCT_DEFS.with(|d| d.borrow().get(other).cloned()) {
                    let ident = format_ident!("{other}");
                    let args = struct_type_args(&def);
                    quote! { #ident #args }
                } else if STRING_ENUM_DEFS.with(|m| m.borrow().contains_key(other)) {
                    let ident = format_ident!("{other}");
                    quote! { #ident }
                } else if let Some(cross_class) = cross_file::lookup_class(other) {
                    let path: TokenStream = cross_class.rust_path.parse().map_err(|e| IrError::Unsupported {
                        file: String::new(),
                        construct: format!("cross-file class `{other}` rust_path did not parse as Rust: {e}"),
                    })?;
                    let mut generic_args = Vec::new();
                    if cross_class.needs_tenant {
                        generic_args.push(quote! { T });
                    }
                    if cross_class.needs_buffer_hooks {
                        generic_args.push(quote! { H });
                    }
                    quote! { #path<#(#generic_args),*> }
                } else {
                    return Err(IrError::Unsupported {
                        file: String::new(),
                        construct: format!("type reference `{other}`"),
                    });
                }
            }
        }),
        TypeRef::Generic { name, args } if name == "Partial" && args.len() == 1 => rust_type(&args[0]),
        // `Map`/`WeakMap` — see `ir.rs`'s `Item::PerTenantCache` doc comment and the primordial-
        // IR plan's "Rust has no `WeakMap`, but doesn't need one" note. A key type of `object`/
        // `Function`/`unknown` (a guest value, not `Hash`/`Eq`-able) maps to `T::ObjectId`
        // instead — see `is_object_identity_map_type`, which callers touching the map's own
        // `.get`/`.set`/`.has`/`.delete` calls consult to convert the key argument accordingly.
        TypeRef::Generic { name, args } if (name == "Map" || name == "WeakMap" || name == "ReadonlyMap") && args.len() == 2 => {
            let key = if matches!(&args[0], TypeRef::Named(n) if n == "object" || n == "Function" || n == "unknown") {
                quote! { T::ObjectId }
            } else {
                rust_type(&args[0])?
            };
            let val = rust_type(&args[1])?;
            Ok(quote! { std::collections::HashMap<#key, #val> })
        }
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
            // A call to another method on a known same-module class instance
            // (`this.requireLive()`, `proxy.ts`'s `ProxyStateImpl.target()`/`.handler()` calling
            // their own `requireLive()`) throws if *that* method's own body does — a class
            // method's `throws`-ness isn't otherwise visible to its callers, since
            // `emit_class_method` computes it independently per method with no cross-method
            // awareness. Only one level deep (checks the callee's body directly, not further
            // transitively) — sufficient for the surveyed source, where no class method calls
            // another throwing method through more than one hop.
            let is_throwing_class_method_call = if let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee.as_ref() {
                class_instance_obj(obj).is_some_and(|(class_name, _)| class_method_throws(&class_name, method))
            } else {
                false
            };
            is_shim_call || is_local_call || is_cross_file_call || is_throwing_class_method_call || expr_throws(callee) || args.iter().any(call_arg_throws)
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
        Expr::NonNull(expr) => expr_throws(expr),
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
    for param in &func.params {
        if let Pattern::Ident(pname) = &param.pattern
            && matches!(param.ty, Some(TypeRef::Named(ref n)) if n == "BufferKind")
        {
            BUFFER_KIND_TYPED_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(pname)));
        }
    }
    // A parameter typed `X | undefined` where `X` isn't one of the names that map directly to a
    // guest `T::Value` (`object`/`Function`/`unknown`) is a *real* Rust `Option<_>` — e.g.
    // `proxy.ts`'s `existingState: ProxyState | undefined` (`Option<ProxyStateImpl<T>>`) — not a
    // guest value that might itself be the guest `null`/`undefined`. Registering it in
    // `OPTION_LOCALS` routes `existingState ?? new ProxyStateImpl(...)` through the ordinary
    // `Option::unwrap_or_else` nullish translation instead of the `Tenant::typeof_tag`-based one
    // meant for guest values.
    for param in &func.params {
        if let Pattern::Ident(pname) = &param.pattern
            && let Some(TypeRef::Optional(inner)) = &param.ty
            && !matches!(inner.as_ref(), TypeRef::Named(n) if n == "object" || n == "Function" || n == "unknown")
        {
            OPTION_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(pname)));
        }
    }
    // A parameter typed as a same-module class (by name or by an interface it `implements`) is a
    // class instance too, exactly like a `let`-bound one — needed for a top-level function that
    // takes an existing instance as an explicit parameter rather than constructing its own (e.g.
    // `proxy.ts`'s `trap(tenant, state: ProxyState, ...)`, called from `proxyExotic` with its own
    // `state` local). Cleared first, matching `emit_class_method`'s own discipline, since a stale
    // entry from whatever function/method was emitted previously would otherwise leak in (this
    // never mattered before nothing needed it, but that's no longer true).
    CLASS_INSTANCE_LOCALS.with(|m| m.borrow_mut().clear());
    for param in &func.params {
        if let Pattern::Ident(pname) = &param.pattern
            && let Some(TypeRef::Named(ty_name)) = &param.ty
            && let Some(class_name) = class_backing_interface(ty_name)
        {
            CLASS_INSTANCE_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(pname), class_name));
        }
    }

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

    let saved_fn_throws = CURRENT_FN_THROWS.with(|c| *c.borrow());
    CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = throws);
    let body_result = emit_block(&func.body);
    CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = saved_fn_throws);
    let mut body = body_result?;
    if throws && is_void {
        body = quote! { #body Ok(()) };
    }
    // See `emit_member`'s numeric-computed-member doc comment: every `args[N]` fallback reads
    // this local instead of calling `tenant.undefined_value()` inline, to avoid a second live
    // mutable borrow of `tenant` when `args[N]` appears as an argument to a call that already
    // borrows `tenant` itself. Unconditional (every function here takes `tenant`, whether or not
    // its body actually contains a numeric computed-member access) — an unused local is only a
    // warning, never a build error.
    body = quote! { let __undefined = tenant.undefined_value(); #body };

    let cache = PER_TENANT_CACHE.with(|c| c.borrow().clone());
    // Checks for the literal word `cache`, not the TS source's own per-tenant-cache variable
    // name (`cache` in most files, but `caches` in `array-buffer.ts`/`typed-arrays.ts`):
    // `try_emit_cache_lookup_pair`/`try_emit_cache_set` always emit a `cache`-named Rust local
    // regardless of what the TS source called it, so that's the name that can actually appear in
    // `body`'s own rendered text.
    if let Some((_, value_ty)) = &cache
        && word_referenced(&body.to_string(), "cache")
    {
        let cache_ty_ident = format_ident!("{value_ty}Cache");
        let (_, cache_generic_args) = per_tenant_cache_generics(value_ty);
        param_tokens.push(quote! { cache: &mut #cache_ty_ident<#cache_generic_args> });
    }
    // Any registered cross-file factory this function's body calls needs its cache threaded in
    // too, one extra parameter per distinct factory referenced — see `cross_file.rs`.
    let body_str = body.to_string();
    for factory in cross_file::TABLE {
        let cache_param = cross_file::cache_param_name(factory);
        if word_referenced(&body_str, &cache_param) {
            let cache_ident = format_ident!("{cache_param}");
            let cache_ty = cross_file_cache_type_tokens(factory)?;
            param_tokens.push(quote! { #cache_ident: &mut #cache_ty });
        }
    }

    // `BufferHooks` is an added generic trait-bound parameter, not a struct (see `emit_param`'s
    // matching case) — a function with a `hooks: BufferHooks` parameter needs `H: BufferHooks`
    // in its own generic parameter list, alongside `T: Tenant`.
    let has_buffer_hooks_param = func.params.iter().any(|p| matches!(p.ty, Some(TypeRef::Named(ref n)) if n == "BufferHooks"));
    let generics = if has_buffer_hooks_param {
        // `+ 'static` on `H` too: a function taking `hooks: BufferHooks` always either stores it
        // into (or returns/threads through) a class instance, and every such class's own
        // definition requires `H: BufferHooks + 'static` (see `class_generics`) — so anything
        // naming that class's type has to satisfy the same bound. `+ Clone`: a function that
        // both constructs a class instance directly *and* forwards `hooks` to a cross-file
        // factory (`typedArraysPrimordial` calling both `new TypedArrayPrimordialImpl(...)` and
        // `bufferPrimordial(tenant, hooks)`) needs to use `hooks` twice — TS has no ownership to
        // fight, but Rust does, so the cross-file-factory call site clones it (see `emit_call`'s
        // cross-file-factory branch) rather than moving the caller's own copy.
        quote! { <T: Tenant + 'static, H: BufferHooks + Clone + 'static> }
    } else {
        quote! { <T: Tenant + 'static> }
    };

    // `+ 'static` unconditionally: any factory that (transitively) reaches `make_builtin` needs
    // it (`ConstructFn<T>`/`impl FnMut(...) + 'static` both require it), and it's a harmless
    // superset requirement for the rest — every real `Tenant` impl in an actual embedding
    // satisfies it anyway, so this sidesteps tracking "does this specific function's call graph
    // reach a closure-taking shim" as its own analysis.
    Ok(quote! {
        pub fn #fn_ident #generics (#(#param_tokens),*) #return_ty {
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
    // `BufferHooks` is an added generic trait-bound parameter (`H: BufferHooks`, see
    // `fn_has_buffer_hooks_param`/`emit_fn_decl`), not a struct. Owned `H`, not `&mut H`: every
    // top-level function taking a `hooks: BufferHooks` parameter today (`bufferPrimordial`) only
    // ever *passes it through* — to a class constructor that stores it (`new
    // BufferPrimordialImpl(hooks)`) or to another such function — never calls a method on it
    // directly (that only ever happens through a *stored* `H` field, reached via
    // `class_instance_obj`'s machinery in `emit_call`/`emit_host_intrinsic`, which borrows the
    // field itself rather than needing the parameter borrowed).
    if matches!(ty, TypeRef::Named(n) if n == "BufferHooks") {
        return Ok(quote! { #ident: H });
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

/// Emits a `class` declaration as an `Rc<RefCell<Inner>>`-backed wrapper type — see `ir.rs`'s
/// `Item::ClassDef` doc comment for the design: every JS class instance is reference-shared, so
/// this needs no per-instance mutation/capture analysis. Every generated inherent method takes
/// `&self` (never `&mut self` — mutation happens through the shared `RefCell`), which is also
/// exactly what makes capturing an instance into a nested closure (`const that = this;`, see
/// `emit_stmt`'s `Stmt::Let` handling) just an ordinary `.clone()` — a cheap `Rc::clone` — that
/// `emit_closure`'s existing capture mechanism already does for free once the wrapper has a
/// hand-written `Clone` impl (below).
fn emit_class_def(def: &ClassDef) -> Result<TokenStream, IrError> {
    let class_ident = format_ident!("{}", def.name);
    let inner_ident = format_ident!("{}Inner", def.name);
    let (generic_params, generic_args) = class_generics(def);

    let mut inner_field_decls = Vec::new();
    for field in &def.fields {
        let field_ident = format_ident!("{}", to_snake_case(&field.name));
        let base_ty = rust_type(&field.ty)?;
        let field_ty = if field.optional || field.definite_assignment { quote! { Option<#base_ty> } } else { base_ty };
        inner_field_decls.push(quote! { #field_ident: #field_ty });
    }

    let Some(ctor) = &def.constructor else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("class `{}` has no constructor", def.name),
        });
    };
    let mut ctor_param_tokens = Vec::new();
    for param in &ctor.params {
        // Unlike an ordinary method/function parameter, a constructor parameter is *stored*
        // (into a field), not used transiently for one call — a `BufferHooks`-typed one needs
        // owned `H`, not `emit_param`'s usual `&mut H` (a fresh per-call borrow would be the
        // wrong shape to hold across the instance's whole lifetime, and wouldn't type-check
        // against the field's own declared type either — see `rust_type`'s `"BufferHooks" => H`
        // case).
        if matches!(&param.ty, Some(TypeRef::Named(n)) if n == "BufferHooks")
            && let Pattern::Ident(name) = &param.pattern
        {
            let ident = format_ident!("{}", to_snake_case(name));
            ctor_param_tokens.push(quote! { #ident: H });
            continue;
        }
        ctor_param_tokens.push(emit_param(param)?);
    }
    // A constructor body is expected to be a flat sequence of `this.#field = expr;`/`this.field =
    // expr;` assignments (the only shape observed in the surveyed source) — extracted directly
    // into a struct-literal field map rather than emitted as ordinary statements, since Rust
    // struct construction happens all at once and a field with no inline initializer (`#hooks`,
    // set only from the constructor) has no sensible placeholder value to construct incrementally
    // toward. This also sidesteps a real name collision the "obvious" translation would hit: the
    // constructor parameter and the field it initializes are conventionally the same name (`this
    // .#hooks = hooks;`), so a pre-declared `hooks` field-local would shadow the parameter.
    // Constructor param names (snake_case) that `emit_param` types as a *reference*
    // (`arg_kind_for_type`'s `Ref`/`RefKey` cases — `object`/`Function`/`unknown`/`PropertyKey`)
    // rather than owned — a field assigned straight from one of these (`this.#objectPrototype =
    // objectPrototype;`) needs `.clone()` to go from the param's borrow to the field's owned
    // storage, unlike `BufferHooks`/plain-owned params (`hooks`), where the param and field types
    // already agree.
    let ref_typed_params: std::collections::HashSet<String> = ctor
        .params
        .iter()
        .filter_map(|p| {
            let Pattern::Ident(name) = &p.pattern else { return None };
            let ty = p.ty.as_ref()?;
            if matches!(ty, TypeRef::Named(n) if n == "BufferHooks" || n == "string") {
                return None;
            }
            matches!(arg_kind_for_type(ty), shims::ArgKind::Ref | shims::ArgKind::RefKey).then(|| to_snake_case(name))
        })
        .collect();
    let mut assigned: HashMap<String, TokenStream> = HashMap::new();
    for stmt in &ctor.body.0 {
        let Stmt::Expr(Expr::Assign { target, value }) = stmt else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("class `{}` constructor has a statement other than `this.<field> = ...;`", def.name),
            });
        };
        let Expr::Member { obj, prop } = target.as_ref() else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("class `{}` constructor assigns to a non-field target", def.name),
            });
        };
        if !matches!(obj.as_ref(), Expr::ThisArg) {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("class `{}` constructor assignment target is not `this.<field>`", def.name),
            });
        }
        let Some(field_name) = member_prop_field_name(prop) else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("class `{}` constructor assigns to a computed member", def.name),
            });
        };
        let value_tokens = emit_expr(value)?;
        let value_tokens = if matches!(value.as_ref(), Expr::Ident(n) if ref_typed_params.contains(&to_snake_case(n))) {
            quote! { (#value_tokens).clone() }
        } else {
            value_tokens
        };
        assigned.insert(field_name.to_string(), value_tokens);
    }
    let mut ctor_field_inits = Vec::new();
    for field in &def.fields {
        let field_ident = format_ident!("{}", to_snake_case(&field.name));
        let value = if let Some(value_tokens) = assigned.get(&field.name) {
            if field.optional || field.definite_assignment {
                quote! { Some(#value_tokens) }
            } else {
                value_tokens.clone()
            }
        } else if field.optional || field.definite_assignment {
            quote! { None }
        } else if let Some(init) = &field.init {
            emit_expr(init)?
        } else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!(
                    "class `{}` field `{}` has no inline initializer, isn't `?`/`!`-declared, and isn't assigned in the constructor",
                    def.name, field.name
                ),
            });
        };
        ctor_field_inits.push(quote! { #field_ident: #value });
    }
    let new_fn = quote! {
        pub fn new(#(#ctor_param_tokens),*) -> Self {
            Self { inner: ::std::rc::Rc::new(::std::cell::RefCell::new(#inner_ident { #(#ctor_field_inits),* })) }
        }
    };

    let saved_class = CURRENT_CLASS.with(|c| c.borrow().clone());
    CURRENT_CLASS.with(|c| *c.borrow_mut() = Some(def.name.clone()));
    let mut method_tokens = Vec::new();
    for method in &def.methods {
        method_tokens.push(emit_class_method(method)?);
    }
    CURRENT_CLASS.with(|c| *c.borrow_mut() = saved_class);

    Ok(quote! {
        pub struct #inner_ident<#generic_params> { #(#inner_field_decls),* }

        pub struct #class_ident<#generic_params> { inner: ::std::rc::Rc<::std::cell::RefCell<#inner_ident<#generic_args>>> }

        impl<#generic_params> Clone for #class_ident<#generic_args> {
            fn clone(&self) -> Self { Self { inner: ::std::rc::Rc::clone(&self.inner) } }
        }

        impl<#generic_params> #class_ident<#generic_args> {
            #new_fn
            #(#method_tokens)*
        }
    })
}

/// Emits one class method (regular or generator — like every top-level function, a generator
/// method's TS `yield`-driving ceremony has already collapsed to a plain synchronous, fallible
/// Rust function by the time it reaches this emitter; see `ir.rs`'s `Expr::TenantYield` doc
/// comment) as an inherent `&self` method on the class's wrapper type.
fn emit_class_method(method: &ClassMethod) -> Result<TokenStream, IrError> {
    let method_ident = format_ident!("{}", to_snake_case(&method.name));
    let func = &method.func;

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
    CLASS_INSTANCE_LOCALS.with(|m| m.borrow_mut().clear());

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

    let saved_coercion = RETURN_COERCION.with(|c| *c.borrow());
    RETURN_COERCION.with(|c| *c.borrow_mut() = ReturnCoercion::Closure);
    let saved_fn_throws = CURRENT_FN_THROWS.with(|c| *c.borrow());
    CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = throws);
    let body_result = emit_block(&func.body);
    CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = saved_fn_throws);
    RETURN_COERCION.with(|c| *c.borrow_mut() = saved_coercion);
    let mut body = body_result?;
    if throws && is_void {
        body = quote! { #body Ok(()) };
    }
    // Unlike `emit_fn_decl` (every top-level factory function takes `tenant`), a class method may
    // not (`prototypeOf` doesn't) — the `__undefined` prelude is only valid, and only needed, when
    // `tenant` is actually one of this method's own parameters.
    let has_tenant_param = func.params.iter().any(|p| matches!(&p.pattern, Pattern::Ident(n) if n == "tenant"));
    if has_tenant_param {
        body = quote! { let __undefined = tenant.undefined_value(); #body };
    }

    let cache = PER_TENANT_CACHE.with(|c| c.borrow().clone());
    // Checks for the literal word `cache`, not the TS source's own per-tenant-cache variable
    // name (`cache` in most files, but `caches` in `array-buffer.ts`/`typed-arrays.ts`):
    // `try_emit_cache_lookup_pair`/`try_emit_cache_set` always emit a `cache`-named Rust local
    // regardless of what the TS source called it, so that's the name that can actually appear in
    // `body`'s own rendered text.
    if let Some((_, value_ty)) = &cache
        && word_referenced(&body.to_string(), "cache")
    {
        let cache_ty_ident = format_ident!("{value_ty}Cache");
        let (_, cache_generic_args) = per_tenant_cache_generics(value_ty);
        param_tokens.push(quote! { cache: &mut #cache_ty_ident<#cache_generic_args> });
    }
    let body_str = body.to_string();
    for factory in cross_file::TABLE {
        let cache_param = cross_file::cache_param_name(factory);
        if word_referenced(&body_str, &cache_param) {
            let cache_ident = format_ident!("{cache_param}");
            let cache_ty = cross_file_cache_type_tokens(factory)?;
            param_tokens.push(quote! { #cache_ident: &mut #cache_ty });
        }
    }

    Ok(quote! {
        pub fn #method_ident(&self, #(#param_tokens),*) #return_ty {
            #body
        }
    })
}

/// Whether `init` is a `TenantYield`-wrapped call to a `Tenant` method whose Rust return type is
/// `Option<_>` — the two the surveyed source narrows immediately afterward
/// (`getOwnPropertyDescriptor`'s ternary, `getPrototypeOf`, though only the former is exercised
/// today). Drives `OPTION_LOCALS` population in `emit_stmt`'s `Stmt::Let` handling.
fn returns_option(init: &Expr) -> bool {
    tenant_call_method_name(init).is_some_and(|m| matches!(m, "getOwnPropertyDescriptor" | "getPrototypeOf"))
        // A `Map`/`WeakMap` `.get(...)` call (`intrinsics::MAP_GET`'s own template,
        // `.get(&key).cloned()`) is `Option<V>` too — e.g. `typed-arrays.ts`'s `const source =
        // records.get(args[0] as object);`, later checked with `if (!source) throw ...;`.
        || matches!(init, Expr::HostIntrinsic { name, .. } if *name == intrinsics::MAP_GET)
        // A cross-file class method registered `returns_option: true` (`BufferPrimordial.record`)
        // — e.g. `const bufferRecord = that.#buffers.record(tenant, first);`, later checked with
        // `if (bufferRecord) {...}`.
        || is_cross_file_class_option_call(init)
        // A call to a same-module top-level generator function whose own declared return type
        // resolves `Option`-shaped (`TrapResult` -> `Option<T::Value>`) — e.g. `proxy.ts`'s
        // `const result = yield tenant.yieldTenant(trap(tenant, state, "get", [...]));`.
        || local_fn_call_returns_option(init)
}

/// Whether `expr` is `yield tenant.yieldTenant(someLocalFn(...))` where `someLocalFn` is a
/// same-module top-level function registered in `LOCAL_FN_RETURN_TYPES` with an `Option`-shaped
/// return type — see `returns_option`'s doc comment.
fn local_fn_call_returns_option(expr: &Expr) -> bool {
    let Expr::TenantYield(inner) = expr else { return false };
    let Expr::Call { callee, .. } = inner.as_ref() else { return false };
    let Expr::Ident(name) = callee.as_ref() else { return false };
    LOCAL_FN_RETURN_TYPES.with(|m| {
        m.borrow()
            .get(name)
            .is_some_and(|ty| matches!(ty, TypeRef::Optional(_)) || matches!(ty, TypeRef::Named(n) if n == "TrapResult"))
    })
}

/// Narrower than `local_fn_call_returns_option`: specifically a call to a function returning
/// `TrapResult` (`proxy.ts`'s `trap`) — see `TRAP_RESULT_TYPED_LOCALS`'s doc comment.
fn local_fn_call_returns_trap_result(expr: &Expr) -> bool {
    let Expr::TenantYield(inner) = expr else { return false };
    let Expr::Call { callee, .. } = inner.as_ref() else { return false };
    let Expr::Ident(name) = callee.as_ref() else { return false };
    LOCAL_FN_RETURN_TYPES.with(|m| m.borrow().get(name).is_some_and(|ty| matches!(ty, TypeRef::Named(n) if n == "TrapResult")))
}

/// Whether `expr` is `X.value` where `X` is a `TRAP_RESULT_TYPED_LOCALS` local — see `Expr::Un`'s
/// `UnOp::Not` special case, which needs to know this to insert `Tenant::to_boolean`.
fn is_trap_result_value_field(expr: &Expr) -> bool {
    let Expr::Member { obj, prop: MemberProp::Ident(field) } = expr else { return false };
    field == "value" && matches!(obj.as_ref(), Expr::Ident(n) if TRAP_RESULT_TYPED_LOCALS.with(|m| m.borrow().contains(&to_snake_case(n))))
}

/// Whether `expr` is a call to a cross-file class method registered `returns_option: true` (e.g.
/// `that.#buffers.record(tenant, first)`) — see `returns_option`'s doc comment.
fn is_cross_file_class_option_call(expr: &Expr) -> bool {
    let Expr::Call { callee, .. } = expr else { return false };
    let Expr::Member { obj: field_obj, prop: MemberProp::Ident(method) } = callee.as_ref() else { return false };
    let Expr::Member { obj: recv_obj, prop: recv_prop } = field_obj.as_ref() else { return false };
    let Some(field_name) = member_prop_field_name(recv_prop) else { return false };
    let Some((class_name, _)) = class_instance_obj(recv_obj) else { return false };
    let Some(field) = class_field_lookup(&class_name, field_name) else { return false };
    let TypeRef::Named(interface_name) = &field.ty else { return false };
    let Some(cross_class) = cross_file::lookup_class(interface_name) else { return false };
    cross_class.methods.iter().any(|m| m.ts_name == method && m.returns_option)
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
/// (`has`/`isExtensible`/`preventExtensions`/`defineProperty`/`setPrototypeOf`),
/// `Option<T::Value>` (`getPrototypeOf`), or `Vec<PropertyKey>` (`ownKeys`/`ownPropertyKeys`) —
/// where the *TS* source gets away with returning the bare value directly because in the current
/// TS-hosted execution model a host `boolean`/primitive/`Array` *is* already a valid
/// guest-observable value with no marshaling boundary at all (only objects/functions are
/// tenant-managed). Rust's `Tenant::Value` is opaque, so returning one of these from a closure
/// needs an explicit conversion at exactly this boundary: `bool` via `Tenant::boolean_value`,
/// `Option<T::Value>` by unwrapping to `Tenant::null_value()`, and `Vec<PropertyKey>` via
/// `Tenant::property_key_value` (per key) + `Tenant::indexed_collection` (the whole list) — see
/// `docs/array-primordial-gap-plan.md` for why those two methods, not a full guest `Array`
/// primordial, are the right-sized fix here.
fn coerce_return_value(expr: &Expr, tokens: TokenStream) -> Result<TokenStream, IrError> {
    const BOOL_RETURNING: &[&str] = &["has", "isExtensible", "preventExtensions", "defineProperty", "setPrototypeOf"];
    const OPTION_VALUE_RETURNING: &[&str] = &["getPrototypeOf"];
    const KEY_LIST_RETURNING: &[&str] = &["ownKeys", "ownPropertyKeys"];
    if let Some(method) = tenant_call_method_name(expr) {
        if BOOL_RETURNING.contains(&method) {
            // `#tokens` is itself `(tenant.<method>(...))?` — a second live mutable borrow of
            // `tenant` (the inner call's own receiver borrow) can't be inlined directly as an
            // argument to `tenant.boolean_value(...)`, which borrows `tenant` too (same class of
            // conflict as `emit_member`'s `args[N]` fallback — see its doc comment). Hoisting
            // the inner call's result into its own `let` first resolves that borrow before
            // `boolean_value`'s call starts.
            return Ok(quote! {
                {
                    let __result = #tokens;
                    tenant.boolean_value(__result)
                }
            });
        }
        if OPTION_VALUE_RETURNING.contains(&method) {
            return Ok(quote! { (#tokens).unwrap_or_else(|| tenant.null_value()) });
        }
        if KEY_LIST_RETURNING.contains(&method) {
            return Ok(quote! {
                {
                    let __keys = #tokens;
                    let __values = __keys.iter().map(|k| tenant.property_key_value(k)).collect::<Result<Vec<_>, _>>()?;
                    tenant.indexed_collection(__values)?
                }
            });
        }
    }
    // A bare `return true;`/`return false;` (`Reflect.set`/`deleteProperty`'s own always-`true`
    // result, matching the ECMAScript `Reflect` methods that report success as a boolean) needs
    // the same `T::Value` conversion as a `has`/`isExtensible`-returning call above, just with no
    // inner call to hoist first.
    if matches!(expr, Expr::Lit(Lit::Bool(_))) {
        return Ok(quote! { tenant.boolean_value(#tokens) });
    }
    // Returning a bare identifier that's already a Rust reference (a by-ref parameter, or a
    // loop/match binding — see `LOCAL_REFNESS`) needs an explicit `.clone()`: the function's own
    // declared return type is always an owned `T::Value`/struct, never a reference.
    if matches!(expr, Expr::Ident(name) if is_known_ref(&to_snake_case(name))) {
        return Ok(quote! { (#tokens).clone() });
    }
    // A private class field read directly returned (`proxy.ts`'s `ProxyStateImpl.target()`:
    // `return this.#target;`) — `emit_member`'s class-field branch deliberately produces a
    // *borrow* through `self.inner.borrow().<field>` with no `.clone()` (every other occurrence
    // is itself the receiver of a further call, which bypasses this coercion entirely via
    // `emit_call`'s own class-field-aware branches). A bare `return` of that borrow would try to
    // move out of the temporary `Ref`, which doesn't live past the statement.
    if let Expr::Member { obj, prop } = expr
        && member_prop_field_name(prop).is_some()
        && class_instance_obj(obj).is_some()
    {
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
    // The plain shape: `if (existing) return existing;`.
    if let [Stmt::Return(Some(Expr::Ident(ret_name)))] = then_branch.0.as_slice()
        && ret_name == existing_name
    {
        let ident = format_ident!("{}", to_snake_case(existing_name));
        return Ok(Some(quote! {
            if let Some(#ident) = cache.entry.clone() { return Ok(#ident); }
        }));
    }
    // The `{ identity, primordial }`-wrapped shape (`array-buffer.ts`/`typed-arrays.ts`): `if
    // (cached) { if (cached.identity !== hooks.identity) throw ...; return cached.primordial; }`.
    // The identity check is moot on the Rust side (the cache's own generic type parameter
    // already ties it to one hooks type at compile time — see `docs/proxy-and-buffer-primordial
    // -gap-plan.md`), so it's simply dropped, not translated.
    if let [
        Stmt::If { cond: Expr::Bin { op: BinOp::NotEq, .. }, then_branch: throw_branch, else_branch: None },
        Stmt::Return(Some(Expr::Member { obj: ret_obj, prop: MemberProp::Ident(field) })),
    ] = then_branch.0.as_slice()
        && field == "primordial"
        && matches!(ret_obj.as_ref(), Expr::Ident(n) if n == existing_name)
        && matches!(throw_branch.0.as_slice(), [Stmt::Throw(_)])
    {
        let ident = format_ident!("{}", to_snake_case(existing_name));
        return Ok(Some(quote! {
            if let Some(#ident) = cache.entry.clone() { return Ok(#ident); }
        }));
    }
    Ok(None)
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
    // `caches.set(tenant, { identity: hooks.identity, primordial: result })` — the same wrapped
    // shape `try_emit_cache_lookup_pair` recognizes on the read side; extract just the
    // `primordial` field's value (the `identity` field has no Rust-side meaning to preserve).
    let value_expr = match result_expr {
        Expr::Object(props) => props
            .iter()
            .find_map(|p| match p {
                ObjectProp::KeyValue { key: PropKey::Ident(k), value } if k == "primordial" => Some(value),
                _ => None,
            })
            .unwrap_or(result_expr),
        _ => result_expr,
    };
    let result_tokens = emit_expr(value_expr)?;
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

/// `if (!x) throw ...;` where `x` is a known `Option<_>`-typed local (e.g. `typed-arrays.ts`'s
/// `const source = records.get(...); if (!source) throw new TypeError(...);`) — the same
/// `let-else` narrowing `option_narrow_continue` does for a `for`-loop's `continue` guard, just
/// re-raising the same error instead of skipping an iteration. Rebinds `x` (shadowed) to the
/// unwrapped value for the rest of the enclosing scope, exactly matching what the TS source's own
/// control-flow narrowing already guarantees at every later use of `x`.
fn option_narrow_throw(cond: &Expr, then_branch: &Block) -> Option<TokenStream> {
    let Expr::Un { op: UnOp::Not, arg } = cond else { return None };
    let Expr::Ident(name) = arg.as_ref() else { return None };
    if !is_option_local(&to_snake_case(name)) {
        return None;
    }
    let [Stmt::Throw(throw_expr)] = then_branch.0.as_slice() else { return None };
    let ident = format_ident!("{}", to_snake_case(name));
    let throw_tokens = emit_throw(throw_expr).ok()?;
    Some(quote! { let Some(#ident) = #ident else { return Err(#throw_tokens); }; })
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
        // `const that = this;` inside a class method — captures the current instance for nested
        // closures to reach back into (see `ir.rs`'s `Item::ClassDef` doc comment). `self` (Rust's
        // own method receiver) is a cheap `Rc`-backed `Clone`, so this is just an ordinary clone
        // into a same-named local, exactly like any other captured value `emit_closure` handles —
        // the *only* thing needed here beyond that is registering `that` in
        // `CLASS_INSTANCE_LOCALS` so later `that.<method>(...)`/`that.#<field>` references resolve.
        Stmt::Let { pattern: Pattern::Ident(name), init: Some(Expr::ThisArg) } if CURRENT_CLASS.with(|c| c.borrow().is_some()) => {
            let class_name = CURRENT_CLASS.with(|c| c.borrow().clone()).unwrap();
            let ident = format_ident!("{}", to_snake_case(name));
            CLASS_INSTANCE_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(name), class_name));
            LOCAL_REFNESS.with(|m| m.borrow_mut().insert(to_snake_case(name), false));
            Ok(quote! { let #ident = self.clone(); })
        }
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
            // `const impl = new BufferPrimordialImpl(hooks);` (or `const state: ProxyState =
            // existingState ?? new ProxyStateImpl(target, handler);` — a possibly-passed-in
            // instance reused via nullish-coalesce, or a freshly constructed one) — registers the
            // local as a known class-instance local so later `impl.<method>(...)`/`impl.<field> =
            // ...` references resolve through `class_instance_obj` the same way a method's own
            // `that`/`this` does.
            if let Some(class_name) = init.as_ref().and_then(new_class_instance_name) {
                CLASS_INSTANCE_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(name), class_name.to_string()));
            }
            // `const codec = codecs[kind];` — `codecs[kind]` erased to bare `kind` (see
            // `emit_member`'s `CODEC_TABLE_NAME`-aware computed-access case), so `codec` itself is
            // just as codec-typed as `kind` was; register it so its own `.bytes`/`.get(...)`/
            // `.set(...)` translate the same way.
            if let Some(Expr::Member { obj, prop: MemberProp::Computed(_) }) = init
                && let Expr::Ident(table_name) = obj.as_ref()
                && CODEC_TABLE_NAME.with(|c| c.borrow().as_deref() == Some(table_name.as_str()))
                && let Some(enum_name) = CODEC_TABLE_ENUM.with(|c| c.borrow().clone())
            {
                CODEC_TYPED_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(name), enum_name));
            }
            LOCAL_REFNESS.with(|m| m.borrow_mut().insert(to_snake_case(name), false));
            if init.as_ref().is_some_and(returns_option) {
                OPTION_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(name)));
            }
            if init.as_ref().is_some_and(local_fn_call_returns_trap_result) {
                TRAP_RESULT_TYPED_LOCALS.with(|m| m.borrow_mut().insert(to_snake_case(name)));
            }
            if init.as_ref().is_some_and(is_buffer_hooks_usize_returning_call) {
                F64_TYPED_LOCALS.with(|set| set.borrow_mut().insert(to_snake_case(name)));
            }
            // Always `mut`: whether a given binding is later reassigned (a struct field write
            // through it, e.g. `next.writable = false`) isn't tracked separately, and an unused
            // `mut` is a warning, never a build error — see the module doc comment's framing.
            Ok(quote! { let mut #ident = #value; })
        }
        Stmt::Expr(expr) => {
            // A same-class throwing method call (`this.requireLive();`) already gets its own
            // trailing `?` from `emit_call`'s class-instance-method-call branch — nothing extra
            // needed here.
            let value = emit_expr(expr)?;
            Ok(quote! { #value; })
        }
        Stmt::Return(value) => {
            let throws = CURRENT_FN_THROWS.with(|c| *c.borrow());
            match value {
                Some(expr) => {
                    let value = emit_expr(expr)?;
                    let value = match RETURN_COERCION.with(|c| *c.borrow()) {
                        ReturnCoercion::Closure => coerce_return_value(expr, value)?,
                        ReturnCoercion::Trap(kind) => coerce_trap_return_value(kind, expr, value),
                    };
                    Ok(if throws { quote! { return Ok(#value); } } else { quote! { return #value; } })
                }
                None => Ok(if throws { quote! { return Ok(()); } } else { quote! { return; } }),
            }
        }
        Stmt::If { cond, then_branch, else_branch: None } if option_narrow_continue(cond, then_branch).is_some() => {
            Ok(option_narrow_continue(cond, then_branch).unwrap())
        }
        Stmt::If { cond, then_branch, else_branch: None } if option_narrow_throw(cond, then_branch).is_some() => {
            Ok(option_narrow_throw(cond, then_branch).unwrap())
        }
        // `if (x) { ... }`/`if (x) { ... } else { ... }` where `x` is a known `Option<_>`-typed
        // local (TS truthy-checks it directly rather than comparing to `undefined`) — e.g.
        // `typed-arrays.ts`'s `const bufferRecord = that.#buffers.record(...); if (bufferRecord)
        // { ...bufferRecord.handle... }`. A real `if let`, not just a `.is_some()` boolean check,
        // since the branch body goes on to use the value unwrapped. Clones rather than moves so
        // the outer binding stays available if anything after the `if` also references it (no
        // real occurrence does yet, but nothing here guarantees it never will).
        Stmt::If { cond: Expr::Ident(name), then_branch, else_branch } if is_option_local(&to_snake_case(name)) => {
            let ident = format_ident!("{}", to_snake_case(name));
            let saved = LOCAL_REFNESS.with(|m| m.borrow().clone());
            let then_tokens = emit_block(then_branch)?;
            LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved.clone());
            match else_branch {
                Some(else_branch) => {
                    let else_tokens = emit_block(else_branch)?;
                    LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved);
                    Ok(quote! { if let Some(#ident) = (#ident).clone() { #then_tokens } else { #else_tokens } })
                }
                None => Ok(quote! { if let Some(#ident) = (#ident).clone() { #then_tokens } }),
            }
        }
        Stmt::If { cond, then_branch, else_branch } => {
            // A bare arithmetic expression (`offset % codec.bytes`) is a native `f64` used in TS's
            // own "truthy number" sense (any non-zero value is truthy) — Rust has no such
            // coercion, so this needs an explicit `!= 0.0` (`typed-arrays.ts`'s `if (offset %
            // codec.bytes) throw ...;`). Every other recognized condition shape already produces
            // a real `bool` on its own (comparisons, `Tenant`/`BufferHooks` boolean-returning
            // calls, etc.), so this is scoped to exactly the arithmetic-`BinOp` case.
            let cond = if matches!(cond, Expr::Bin { op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod, .. }) {
                let cond_t = emit_expr(cond)?;
                quote! { ((#cond_t) != 0.0) }
            } else {
                emit_expr(cond)?
            };
            // `LOCAL_REFNESS` is restored after each branch: a `const`/`let` declared inside an
            // `if`/`else` block is block-scoped in JS and must not leak into whatever follows the
            // `if` in the same enclosing function — otherwise a *later*, unrelated closure that
            // happens to redeclare the same name (its own block-scoped local, not a real capture)
            // gets wrongly treated as capturing an "outer" binding that isn't actually in scope
            // there (`emit_closure`'s capture computation matches purely by name against
            // `LOCAL_REFNESS`'s snapshot — a real bug `typed-arrays.ts`'s `get` trap hit, where
            // two sibling branches both declare their own unrelated `const bytes = ...`).
            let saved = LOCAL_REFNESS.with(|m| m.borrow().clone());
            let then_branch = emit_block(then_branch)?;
            LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved.clone());
            match else_branch {
                Some(else_branch) => {
                    let else_branch = emit_block(else_branch)?;
                    LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved);
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
            // `args.slice(N)` (owned `Vec<T::Value>`, see `intrinsics.rs`'s `ARRAY_SLICE_FROM`
            // template) and `ownKeys`/`ownPropertyKeys` (also an owned `Vec<_>`) both yield
            // *owned* items when iterated by value — no recognized source produces a borrowed
            // slice/iterator here anymore, so the loop-local is always owned.
            let saved = LOCAL_REFNESS.with(|m| m.borrow().clone());
            LOCAL_REFNESS.with(|m| {
                m.borrow_mut().insert(to_snake_case(name), false);
            });
            let body_tokens = emit_block(body)?;
            LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved);
            Ok(quote! { for #ident in #iter_tokens { #body_tokens } })
        }
        // `for (let i = start; i < bound; i++) { body }` — Rust has no C-style counting `for`, so
        // this becomes a `while` loop over a plain `f64` local, consistent with every other TS
        // `number` in this IR (rather than a native `usize` range, which would force a mixed-type
        // expression wherever `i` is used in surrounding `f64` arithmetic — see
        // `typed-arrays.ts`'s `source.offset + i * codec.bytes`).
        Stmt::ForCounting { binding, start, bound, body } => {
            let ident = format_ident!("{}", to_snake_case(binding));
            let start_tokens = emit_expr(start)?;
            let bound_tokens = emit_expr(bound)?;
            let saved = LOCAL_REFNESS.with(|m| m.borrow().clone());
            LOCAL_REFNESS.with(|m| {
                m.borrow_mut().insert(to_snake_case(binding), false);
            });
            let body_tokens = emit_block(body)?;
            LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved);
            Ok(quote! {
                {
                    let mut #ident = #start_tokens;
                    while #ident < #bound_tokens {
                        #body_tokens
                        #ident += 1.0;
                    }
                }
            })
        }
        Stmt::TryCatch { .. } => Err(IrError::Unsupported {
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

/// `new ClassName(...)` — constructs one of this module's own `Item::ClassDef`s via its
/// generated `ClassName::new(...)` associated function (see `emit_class_def`), or `new
/// Map()`/`new WeakMap()` with no type arguments passed at the construction site (a class field's
/// own inline initializer, e.g. `#prototypes: Map<BufferKind, object> = new Map();` — the
/// generic key/value types live on the *field's* declared type, per `rust_type`'s `Map`/
/// `WeakMap` case, not on this expression). `new TypeError(...)`/`new RangeError(...)` never
/// reach here: those only ever appear directly under `throw`, handled by `emit_throw` instead.
fn emit_new(callee: &Expr, args: &[CallArg]) -> Result<TokenStream, IrError> {
    let Expr::Ident(name) = callee else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: "`new` of a non-identifier constructor".into(),
        });
    };
    if (name == "Map" || name == "WeakMap") && args.is_empty() {
        return Ok(quote! { ::std::collections::HashMap::new() });
    }
    // `new Uint8Array(N)` — a zero-filled, `N`-byte owned buffer (`typed-arrays.ts`'s `codecs`
    // setters allocate exactly one element's worth of scratch bytes this way before writing
    // through `DataView`, which is itself erased to the plain byte buffer at lowering time — see
    // `lower.rs`'s `ast::Expr::New` handling for `DataView`).
    if name == "Uint8Array"
        && let [CallArg::Normal(len_expr)] = args
    {
        let len_tokens = emit_expr(len_expr)?;
        return Ok(quote! { vec![0u8; (#len_tokens) as usize] });
    }
    let Some(def) = class_def(name) else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("`new {name}(...)` does not refer to a class declared in this module"),
        });
    };
    let param_tys: Vec<Option<TypeRef>> =
        def.constructor.as_ref().map(|c| c.params.iter().map(|p| p.ty.clone()).collect()).unwrap_or_default();
    if args.len() != param_tys.len() {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("`new {name}(...)` called with {} args, expected {}", args.len(), param_tys.len()),
        });
    }
    let mut arg_tokens = Vec::new();
    for (arg, ty) in args.iter().zip(param_tys.iter()) {
        let CallArg::Normal(expr) = arg else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: "spread argument to a class constructor".into(),
            });
        };
        let kind = ty.as_ref().map(arg_kind_for_type).unwrap_or(shims::ArgKind::Owned);
        arg_tokens.push(emit_call_arg(expr, kind)?);
    }
    let class_ident = format_ident!("{name}");
    Ok(quote! { #class_ident::new(#(#arg_tokens),*) })
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
        // `!result.value`/`!!result.value` (`proxy.ts`'s `TrapResult.value`, a guest `T::Value`
        // after `emit_member`'s unwrap translation) — TS's logical `!` truthy-checks a guest
        // value; Rust's own `!` only works on `bool`, so this needs `Tenant::to_boolean` first.
        // Every other `!X` in the surveyed source already produces a real `bool` on its own
        // (comparisons, `.found`'s `.is_some()`, etc.), so this is scoped to exactly this shape.
        Expr::Un { op: UnOp::Not, arg } if is_trap_result_value_field(arg) => {
            let arg = emit_expr(arg)?;
            Ok(quote! { (!tenant.to_boolean(&(#arg))) })
        }
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
        Expr::NonNull(inner) => {
            let tokens = emit_expr(inner)?;
            Ok(quote! { (#tokens).unwrap() })
        }
        Expr::HostIntrinsic { name, args } => emit_host_intrinsic(name, args),
        Expr::Closure(func) => emit_closure(func),
        Expr::New { callee, args } => emit_new(callee, args),
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
    // A field access on a known class instance (`this.#hooks`, `that.#records`, ...) — see
    // `class_instance_obj`'s doc comment. Deliberately produces a *borrow* through the wrapper's
    // `Rc<RefCell<Inner>>` (`self.inner.borrow().<field>`, no `.clone()`), not an owned value:
    // every occurrence in the surveyed source is itself the *receiver* of a further call
    // (`this.#hooks.byteLength(...)`, `this.#records.get(...)`), which needs a borrow, not a
    // clone of the whole field — see `emit_call`'s and `emit_host_intrinsic`'s class-field-aware
    // branches, which are what actually consume this. A borrow is valid Rust here because the
    // temporary `Ref`/`RefMut` `.borrow()` returns lives for the rest of the enclosing statement,
    // which is always enough for these single-statement/single-expression uses.
    if let Some(field_name) = member_prop_field_name(prop)
        && let Some((_, obj_tokens)) = class_instance_obj(obj)
    {
        let field_ident = format_ident!("{}", to_snake_case(field_name));
        return Ok(quote! { #obj_tokens.inner.borrow().#field_ident });
    }
    match prop {
        // `jade-tenant-rt::BufferHooks::supports_shared_array_buffer` is a *method*
        // (`fn(&self) -> bool`), unlike the TS `BufferHooks` interface's `readonly
        // supportsSharedArrayBuffer: boolean` field it mirrors — a fixed, narrow shape mismatch
        // between the two (every other `BufferHooks` member is already a method on both sides),
        // special-cased directly since it's the only one.
        MemberProp::Ident(field) if field == "supportsSharedArrayBuffer" => {
            let obj_tokens = emit_expr(obj)?;
            Ok(quote! { #obj_tokens.supports_shared_array_buffer() })
        }
        // `codec.bytes` where `codec` is a `CODEC_TYPED_LOCALS` local (bound from
        // `codecs[kind]`, itself now just `kind`) — `codec` is a bare enum value, not a struct,
        // so `.bytes` becomes the inherent method `try_emit_data_view_codec_table` generated.
        MemberProp::Ident(field)
            if field == "bytes"
                && let Expr::Ident(name) = obj
                && CODEC_TYPED_LOCALS.with(|m| m.borrow().contains_key(&to_snake_case(name))) =>
        {
            let obj_tokens = emit_expr(obj)?;
            Ok(quote! { #obj_tokens.codec_bytes() })
        }
        // `result.found`/`result.value` where `result` is a `TRAP_RESULT_TYPED_LOCALS` local
        // (bound from a call to `trap`, `Option<T::Value>` on the Rust side — see `rust_type`'s
        // `"TrapResult"` case). `.found` is a plain presence check; `.value` unwraps, safe in
        // practice because every real use only reads `.value` after `.found` already confirmed
        // `Some` (Rust can't see that itself from the TS-level `? :`/`if` structure, but the
        // value is never read otherwise in the surveyed source).
        MemberProp::Ident(field)
            if (field == "found" || field == "value")
                && let Expr::Ident(name) = obj
                && TRAP_RESULT_TYPED_LOCALS.with(|m| m.borrow().contains(&to_snake_case(name))) =>
        {
            let obj_tokens = emit_expr(obj)?;
            Ok(if field == "found" { quote! { #obj_tokens.is_some() } } else { quote! { (#obj_tokens.clone().unwrap()) } })
        }
        MemberProp::Ident(field) => {
            let obj_tokens = emit_expr(obj)?;
            let field_ident = format_ident!("{}", to_snake_case(field));
            Ok(quote! { #obj_tokens.#field_ident })
        }
        MemberProp::Private(_) => Err(IrError::Unsupported {
            file: String::new(),
            construct: "private field access on a receiver that isn't a recognized class instance".into(),
        }),
        // `codecs[kind]` (`CODEC_TABLE_NAME`'s own table, indexed by a bare identifier rather
        // than a numeric literal) — erased entirely to just `kind`, since the table itself never
        // becomes a real Rust value (see `try_emit_data_view_codec_table`).
        MemberProp::Computed(index)
            if matches!(obj, Expr::Ident(n) if CODEC_TABLE_NAME.with(|c| c.borrow().as_deref() == Some(n.as_str()))) =>
        {
            emit_expr(index)
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
            // The fallback deliberately reads a pre-materialized `__undefined` local (always
            // bound as the first statement of every function/closure body — see
            // `emit_fn_decl`/`emit_closure`) rather than calling `tenant.undefined_value()`
            // inline: this expression very often appears *as a call argument* to a
            // `tenant.<method>(...)`/shimmed call that also borrows `tenant` (as receiver or as
            // its own leading argument), and a fresh `&mut tenant` borrow from an inline call
            // here would conflict with that outer borrow — a real, previously-hit compile error,
            // not a hypothetical one. `__undefined.clone()` only ever needs `&__undefined`, which
            // never conflicts with anything.
            Ok(quote! { (#obj_tokens).get(#n).cloned().unwrap_or_else(|| __undefined.clone()) })
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
/// `{ found: false }` -> `None`, `{ found: true, value: X }` -> `Some(X)` — `proxy.ts`'s
/// `TrapResult` construction sites (see `rust_type`'s own `"TrapResult"` case and its doc
/// comment for why this resolves to `Option<T::Value>` rather than a generated enum). Returns
/// `None` (the outer `Option`, not `TrapResult`'s own `None`) if `props` doesn't match either
/// shape, so `emit_object_literal` falls through to its other recognized shapes.
fn try_emit_trap_result_literal(props: &[ObjectProp]) -> Option<Result<TokenStream, IrError>> {
    match props {
        [ObjectProp::KeyValue { key: PropKey::Ident(k), value: Expr::Lit(Lit::Bool(false)) }] if k == "found" => {
            Some(Ok(quote! { None }))
        }
        [
            ObjectProp::KeyValue { key: PropKey::Ident(k1), value: Expr::Lit(Lit::Bool(true)) },
            ObjectProp::KeyValue { key: PropKey::Ident(k2), value: value_expr },
        ] if k1 == "found" && k2 == "value" => Some(emit_expr(value_expr).map(|value_tokens| quote! { Some(#value_tokens) })),
        _ => None,
    }
}

fn emit_object_literal(props: &[ObjectProp]) -> Result<TokenStream, IrError> {
    if let Some(result) = try_emit_trap_result_literal(props) {
        return result;
    }
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

/// Which return-value coercion `Stmt::Return`'s emission should apply — see `RETURN_COERCION`'s
/// doc comment.
#[derive(Clone, Copy)]
enum ReturnCoercion {
    Closure,
    Trap(TrapReturn),
}

/// What a `TenantExoticHandler` trap's Rust translation returns, and therefore how to coerce the
/// trap body's own `return` values into it. `jade-tenant-rt::TenantExoticHandler`'s trait method
/// signatures are the source of truth here — see [`EXOTIC_TRAPS`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum TrapReturn {
    /// `Result<T::Value, TenantError>` (`get`) — the one shape needing a fixup: a bare `return
    /// undefined;` must become `tenant.undefined_value()`, not the generic `Lit::Undefined ->
    /// None` mapping every other context uses (see `coerce_trap_return_value`).
    Value,
    /// `Result<(), TenantError>` (`set`/`delete`/`define`/`assign`) — an empty or fall-off-the-
    /// end body needs a trailing `Ok(())` appended, the same fixup `emit_fn_decl` already does
    /// for a void generator.
    Void,
    Bool,
    KeyList,
    /// `Result<Option<T::Value>, TenantError>` (`getPrototypeOf`) — `Lit::Undefined`/`Lit::Null`
    /// already map to `None` correctly with no fixup; a real value just needs `Some(...)`
    /// wrapping, which `Stmt::Return`'s ordinary emission doesn't do on its own (see
    /// `wrap_trap_return_value`).
    OptionValue,
    /// `Result<Option<TenantPropertyDescriptor<T::Value>>, TenantError>`
    /// (`getOwnPropertyDescriptor`) — same shape as `OptionValue`, different payload type.
    OptionDescriptor,
}

struct TrapSpec {
    ts_name: &'static str,
    rust_name: &'static str,
    /// (default parameter name, type) for the trap's own parameters, in order — excludes the
    /// implicit `&mut self`/`tenant: &mut T` every trap takes. The object literal's own method
    /// may declare fewer parameter names than this (or none at all, e.g. `*ownKeys() {}`); unnamed
    /// positions fall back to this default name (matching `emit_closure`'s `SYNTHETIC_PARAM_NAMES`
    /// fallback for the same reason: the trait signature is fixed regardless of how many of its
    /// parameters a given implementation's body actually names).
    params: &'static [(&'static str, &'static str)],
    returns: TrapReturn,
}

const EXOTIC_TRAPS: &[TrapSpec] = &[
    TrapSpec { ts_name: "get", rust_name: "get", params: &[("receiver", "&T::Value"), ("key", "&PropertyKey")], returns: TrapReturn::Value },
    TrapSpec {
        ts_name: "set",
        rust_name: "set",
        params: &[("receiver", "&T::Value"), ("key", "&PropertyKey"), ("value", "T::Value")],
        returns: TrapReturn::Void,
    },
    TrapSpec { ts_name: "has", rust_name: "has", params: &[("receiver", "&T::Value"), ("key", "&PropertyKey")], returns: TrapReturn::Bool },
    TrapSpec { ts_name: "delete", rust_name: "delete", params: &[("receiver", "&T::Value"), ("key", "&PropertyKey")], returns: TrapReturn::Void },
    TrapSpec { ts_name: "ownKeys", rust_name: "own_keys", params: &[("receiver", "&T::Value")], returns: TrapReturn::KeyList },
    TrapSpec { ts_name: "ownPropertyKeys", rust_name: "own_property_keys", params: &[("receiver", "&T::Value")], returns: TrapReturn::KeyList },
    TrapSpec {
        ts_name: "getOwnPropertyDescriptor",
        rust_name: "get_own_property_descriptor",
        params: &[("receiver", "&T::Value"), ("key", "&PropertyKey")],
        returns: TrapReturn::OptionDescriptor,
    },
    TrapSpec {
        ts_name: "defineProperty",
        rust_name: "define_property",
        params: &[("receiver", "&T::Value"), ("key", "&PropertyKey"), ("descriptor", "TenantPropertyDescriptor<T::Value>")],
        returns: TrapReturn::Bool,
    },
    TrapSpec { ts_name: "getPrototypeOf", rust_name: "get_prototype_of", params: &[("receiver", "&T::Value")], returns: TrapReturn::OptionValue },
    TrapSpec {
        ts_name: "setPrototypeOf",
        rust_name: "set_prototype_of",
        params: &[("receiver", "&T::Value"), ("prototype", "Option<T::Value>")],
        returns: TrapReturn::Bool,
    },
    TrapSpec { ts_name: "isExtensible", rust_name: "is_extensible", params: &[("receiver", "&T::Value")], returns: TrapReturn::Bool },
    TrapSpec { ts_name: "preventExtensions", rust_name: "prevent_extensions", params: &[("receiver", "&T::Value")], returns: TrapReturn::Bool },
    TrapSpec { ts_name: "define", rust_name: "define", params: &[("receiver", "&T::Value"), ("descriptors", "&T::Value")], returns: TrapReturn::Void },
    TrapSpec { ts_name: "assign", rust_name: "assign", params: &[("receiver", "&T::Value"), ("source", "&T::Value")], returns: TrapReturn::Void },
];

/// Translates `tenant.makeExotic(proto, { *get(receiver, key) { ... }, *set(...) { ... }, ... })`
/// — an object literal whose values are all generator methods named after
/// `TenantExoticHandler`'s traps (`array-buffer.ts`'s buffer shell, `typed-arrays.ts`'s
/// typed-array records, `proxy.ts`'s `native` handler) — into a freshly generated struct (one
/// field per free variable the handler's methods capture) implementing
/// `jade-tenant-rt::TenantExoticHandler<T>` directly, the generated counterpart to
/// `types_shim.rs`'s hand-written `BuiltinHandler`. `makeCallableExotic`-from-object-literal
/// (`apply`/`construct` traps) is deliberately not handled here — no primordial's own object
/// literal needs it; every callable exotic in the surveyed source goes through `make_builtin`
/// instead, whose hand-written `BuiltinHandler` already covers that shape.
///
/// **Known simplification:** every captured field defaults to `T::Value` — correct for the
/// common case (`array-buffer.ts`'s `proto`), but wrong for a captured `Map`/`WeakMap` or an
/// embedder-capability parameter (`records`, `hooks`) — those need real type inference this
/// emitter doesn't have yet. A wrong field type is a loud, immediate compile error at the
/// generated struct-literal construction site, never a silent miscompile.
fn emit_exotic_handler_literal(proto_expr: &Expr, props: &[ObjectProp]) -> Result<TokenStream, IrError> {
    let mut methods = Vec::new();
    for prop in props {
        let ObjectProp::Method { key: PropKey::Ident(key), func } = prop else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: "tenant.makeExotic(...) handler literal member outside the recognized {*trap(...) {...}, ...} shape".into(),
            });
        };
        let Some(spec) = EXOTIC_TRAPS.iter().find(|t| t.ts_name == key.as_str()) else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("tenant.makeExotic(...) handler literal has an unrecognized trap `{key}`"),
            });
        };
        methods.push((spec, func));
    }

    // Only names that are *actually* bound in the enclosing scope count as captures — a name
    // referenced inside a trap's own body that's really a local the trap (or a closure nested
    // inside it) declares for itself must not be treated as an outer capture just because
    // `free_idents_in_expr` saw it referenced somewhere in the whole handler literal. Mirrors
    // `emit_closure`'s own `outer_names ∩ referenced` intersection, for the same reason.
    let outer_names: std::collections::HashSet<String> = LOCAL_REFNESS.with(|m| m.borrow().keys().cloned().collect());
    let captured: Vec<String> = {
        let mut set = std::collections::HashSet::new();
        free_idents_in_expr(&Expr::Object(props.to_vec()), &mut set);
        let mut names: Vec<String> = set.into_iter().filter(|n| n != "tenant" && outer_names.contains(n)).collect();
        names.sort();
        names
    };

    let n = EXOTIC_HANDLER_COUNTER.with(|c| {
        let mut c = c.borrow_mut();
        let n = *c;
        *c += 1;
        n
    });
    let struct_ident = format_ident!("ExoticHandler{n}");

    // A captured name that's a known class instance (`that`, from `const that = this;`) needs
    // that class's own wrapper type, not the default `T::Value` — see `class_instance_obj`'s doc
    // comment on why capturing one is just an ordinary `.clone()` like any other captured value.
    let captured_classes: HashMap<String, ClassDef> = captured
        .iter()
        .filter_map(|name| CLASS_INSTANCE_LOCALS.with(|m| m.borrow().get(name).cloned()).and_then(|c| class_def(&c)).map(|def| (name.clone(), def)))
        .collect();
    let needs_buffer_hooks = captured_classes.values().any(class_needs_buffer_hooks);
    // `+ 'static` unconditionally on `T`, matching `class_generics`/`per_tenant_cache_generics`:
    // a captured class instance is itself always `T: Tenant + 'static`, so a handler struct that
    // holds one needs the same bound on its own `T`.
    // `H: BufferHooks + 'static`, not just `BufferHooks`: `Box<dyn TenantExoticHandler<T>>`
    // requires the concrete struct (and everything reachable through its own fields, including
    // `H::Handle` transitively via a captured class instance) to be `'static` too.
    let handler_generics = if needs_buffer_hooks {
        quote! { T: Tenant + 'static, H: BufferHooks + 'static }
    } else {
        quote! { T: Tenant + 'static }
    };
    let handler_generic_args = if needs_buffer_hooks { quote! { T, H } } else { quote! { T } };

    let field_decls: Vec<TokenStream> = captured
        .iter()
        .map(|name| {
            let ident = format_ident!("{name}");
            if let Some(class_def) = captured_classes.get(name) {
                let class_ident = format_ident!("{}", class_def.name);
                let (_, generic_args) = class_generics(class_def);
                quote! { #ident: #class_ident<#generic_args> }
            } else if let Some(enum_name) = CODEC_TYPED_LOCALS.with(|m| m.borrow().get(name).cloned()) {
                // A captured codec-typed local (`codec`, `sourceCodec` — see `CODEC_TYPED_LOCALS`'s
                // doc comment) is a bare enum value, not a guest `T::Value`.
                let enum_ident = format_ident!("{enum_name}");
                quote! { #ident: #enum_ident }
            } else {
                quote! { #ident: T::Value }
            }
        })
        .collect();
    let field_inits: Vec<TokenStream> = captured
        .iter()
        .map(|name| {
            let ident = format_ident!("{name}");
            quote! { #ident: (#ident).clone() }
        })
        .collect();

    let saved_refness = LOCAL_REFNESS.with(|m| m.borrow().clone());
    let mut impl_methods = Vec::new();
    for (spec, func) in &methods {
        LOCAL_REFNESS.with(|m| {
            let mut m = m.borrow_mut();
            for name in &captured {
                m.insert(name.clone(), false);
            }
        });
        let mut param_tokens = Vec::new();
        for (i, (default_name, ty_str)) in spec.params.iter().enumerate() {
            let name = match func.params.get(i) {
                Some(Param { pattern: Pattern::Ident(name), .. }) => to_snake_case(name),
                Some(Param { pattern: Pattern::ObjectShallow(_), .. }) => {
                    return Err(IrError::Unsupported {
                        file: String::new(),
                        construct: format!("destructured parameter in `{}` trap", spec.ts_name),
                    });
                }
                None => (*default_name).to_string(),
            };
            let is_ref = ty_str.starts_with('&');
            LOCAL_REFNESS.with(|m| m.borrow_mut().insert(name.clone(), is_ref));
            if *ty_str == "&PropertyKey" {
                PROPERTY_KEY_TYPED_LOCALS.with(|set| set.borrow_mut().insert(name.clone()));
            }
            if *ty_str == "Option<T::Value>" {
                OPTION_VALUE_TYPED_LOCALS.with(|set| set.borrow_mut().insert(name.clone()));
            }
            let ident = format_ident!("{name}");
            let ty: TokenStream = ty_str.parse().unwrap();
            param_tokens.push(quote! { #ident: #ty });
        }

        let saved_coercion = RETURN_COERCION.with(|c| *c.borrow());
        RETURN_COERCION.with(|c| *c.borrow_mut() = ReturnCoercion::Trap(spec.returns));
        // Every `TenantExoticHandler` trap method always returns `Result<_, TenantError>`,
        // regardless of `CURRENT_FN_THROWS`'s value for whatever *enclosing* function/method
        // this handler literal is being constructed inside of — see its doc comment.
        let saved_fn_throws = CURRENT_FN_THROWS.with(|c| *c.borrow());
        CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = true);
        let body_result = emit_block(&func.body);
        CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = saved_fn_throws);
        RETURN_COERCION.with(|c| *c.borrow_mut() = saved_coercion);
        let mut body = body_result?;
        if spec.returns == TrapReturn::Void {
            body = quote! { #body Ok(()) };
        }
        // A captured free variable lives in `self.<field>`, but the body (lowered straight from
        // the original JS closure, which captured it lexically) references it as a bare
        // identifier — shadow it with a local clone first, the same "clone into a same-named
        // binding" idiom `emit_closure` uses for its own captures, and for the same reason (a
        // struct field access can't be substituted in after the fact without walking the whole
        // body; shadowing needs no rewriting at all).
        let field_prelude: Vec<TokenStream> = captured
            .iter()
            .map(|name| {
                let ident = format_ident!("{name}");
                quote! { let #ident = self.#ident.clone(); }
            })
            .collect();
        body = quote! { #(#field_prelude)* #body };
        let return_ty: TokenStream = match spec.returns {
            TrapReturn::Value => quote! { T::Value },
            TrapReturn::Void => quote! { () },
            TrapReturn::Bool => quote! { bool },
            TrapReturn::KeyList => quote! { Vec<PropertyKey> },
            TrapReturn::OptionValue => quote! { Option<T::Value> },
            TrapReturn::OptionDescriptor => quote! { Option<TenantPropertyDescriptor<T::Value>> },
        };
        let method_ident = format_ident!("{}", spec.rust_name);
        impl_methods.push(quote! {
            fn #method_ident(&mut self, tenant: &mut T, #(#param_tokens),*) -> Result<#return_ty, TenantError> {
                let __undefined = tenant.undefined_value();
                #body
            }
        });
    }
    LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved_refness);

    let proto_tokens = emit_proto_option(proto_expr)?;

    Ok(quote! {
        {
            struct #struct_ident<#handler_generics> { #(#field_decls),* }
            impl<#handler_generics> TenantExoticHandler<T> for #struct_ident<#handler_generic_args> {
                #(#impl_methods)*
            }
            tenant.make_exotic(#proto_tokens, Box::new(#struct_ident { #(#field_inits),* }))
        }
    })
}

/// Mirrors `coerce_return_value`'s role but for a `TenantExoticHandler` trap method body, whose
/// Rust return type is whatever `TrapReturn` says (not always `T::Value` — see
/// `RETURN_COERCION`'s doc comment for why the two can't share one coercion function).
fn coerce_trap_return_value(kind: TrapReturn, expr: &Expr, tokens: TokenStream) -> TokenStream {
    match kind {
        // A string/number/bool literal needs an explicit `Tenant` constructor the same way
        // `coerce_return_value` handles a bare closure return (`emit_expr` has no way to know a
        // bare `"foo"`/`5`/`true` literal needs `Tenant::string_value`/etc. without knowing the
        // surrounding context is `T::Value`-typed). `Lit::Null`/`Lit::Undefined` need no fixup
        // here — `emit_expr` already renders those as `tenant.null_value()`/`undefined_value()`,
        // correct for this exact position. Anything else is assumed already `T::Value`-shaped
        // (the common case: a tenant/shim call result, or a captured `T::Value` local).
        // `emit_expr` has no way to know a bare literal needs a `Tenant` constructor without
        // knowing the surrounding context is `T::Value`-typed — see `coerce_literal_to_value`.
        // `Lit::Null`/`Lit::Undefined` need no fixup here — `emit_expr` already renders those as
        // `tenant.null_value()`/`undefined_value()`, correct for this exact position. A
        // `byteLength` call needs the same numeric wrapping a bare literal does (see its own
        // case); anything else is assumed already `T::Value`-shaped (the common case: a tenant/
        // shim call result, or a captured `T::Value` local).
        TrapReturn::Value if is_buffer_hooks_usize_returning_call(expr) || is_codec_get_call(expr) => {
            quote! { tenant.number_value(#tokens) }
        }
        TrapReturn::Value => coerce_literal_to_value(expr, tokens),
        // `emit_expr`'s `Lit::Null`/`Lit::Undefined` mapping produces a real guest value
        // (`tenant.null_value()`/`undefined_value()`) now, not Rust's `None` — correct for a
        // `T::Value`-returning position (`TrapReturn::Value` above), but this position's real
        // Rust return type is `Option<_>`, so a literal `null`/`undefined` return needs `None`
        // explicitly here instead, ignoring `tokens`. Anything else is a real value needing
        // `Some(...)` wrapping, which ordinary `Stmt::Return` emission doesn't do on its own. A
        // value that's *already* `Option`-shaped (e.g. a `Cast` to `object | null`, `Tenant
        // ::nullable`'s own return shape) must not be wrapped again — recognized structurally
        // rather than re-deriving full type inference.
        TrapReturn::OptionValue | TrapReturn::OptionDescriptor => match expr {
            Expr::Lit(Lit::Undefined) | Expr::Lit(Lit::Null) => quote! { None },
            Expr::Cast { target: TypeRef::Optional(_), .. } => tokens,
            // `return yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(...))` /
            // `...getPrototypeOf(...)` — the callee's own Rust return type is already
            // `Option<_>` (see `returns_option`), so wrapping in another `Some(...)` here would
            // double-wrap it instead of forwarding the `None`/`Some(_)` it already produced.
            _ if returns_option(expr) => tokens,
            _ => quote! { Some((#tokens).clone()) },
        },
        TrapReturn::Bool | TrapReturn::KeyList | TrapReturn::Void => tokens,
    }
}

/// `proto: object | null` (or `object | null | undefined`) — `tenant.makeExotic`'s first
/// argument. `null` is the only literal ever passed directly (`proxy.ts`'s `tenant.makeExotic
/// (null, native)`); anything else is an existing `T::Value` local needing `Some((...).clone())`
/// wrapping, the same convention `emit_call_arg`'s `OptionRef`/`OptionOwned` kinds use elsewhere.
fn emit_proto_option(proto_expr: &Expr) -> Result<TokenStream, IrError> {
    if matches!(proto_expr, Expr::Lit(Lit::Null)) {
        return Ok(quote! { None });
    }
    let tokens = emit_expr(proto_expr)?;
    Ok(quote! { Some((#tokens).clone()) })
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
/// A `readonly unknown[]`-typed array-literal call argument (`proxy.ts`'s `trap(tenant, state,
/// "get", [state.target(), key, receiver])`) — each element becomes a guest `T::Value`
/// individually (a `PropertyKey`-typed element, e.g. `key`, converts via
/// `Tenant::property_key_value`; everything else is already `T::Value`), then the whole thing is
/// borrowed to match the callee's own `&[T::Value]` parameter type (`emit_param`'s translation of
/// `readonly unknown[]`).
fn emit_value_slice_array_literal(elements: &[ArrayElement]) -> Result<TokenStream, IrError> {
    let mut items = Vec::new();
    for element in elements {
        let ArrayElement::Normal(e) = element else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: "spread element in a `readonly unknown[]`-typed array literal".into(),
            });
        };
        let tokens = emit_expr(e)?;
        let snake = if let Expr::Ident(n) = e { Some(to_snake_case(n)) } else { None };
        let tokens = if snake.as_deref().is_some_and(is_property_key_typed) {
            quote! { (tenant.property_key_value(#tokens))? }
        } else if snake.as_deref().is_some_and(|n| OPTION_VALUE_TYPED_LOCALS.with(|m| m.borrow().contains(n))) {
            // A trap parameter that's already `Option<T::Value>` (`setPrototypeOf`'s
            // `prototype`) needs unwrapping to a real guest value — a `readonly unknown[]` slot
            // is always a genuine `T::Value`, never a Rust `Option`. Cloned first: `prototype`
            // is reused after this array literal in the trap's own fallback branch
            // (`tenant.setPrototypeOf(state.target(), prototype)`), and `Option::unwrap_or_else`
            // otherwise moves out of it.
            quote! { (#tokens).clone().unwrap_or_else(|| tenant.null_value()) }
        } else if snake.is_some() {
            // Any other bare local — whether a `&T::Value`-typed ref (a trap's own `receiver`/
            // `descriptors`/`source` parameter, needing an owned clone for this owned-`T::Value`
            // array slot) or already owned (`set`'s own `value: T::Value` parameter) — gets
            // cloned rather than moved: the local is very often still referenced later in the
            // same trap body (`set`'s fallback branch reuses `value` after this array literal
            // already consumed it once), and cloning a guest `T::Value` is always cheap/valid
            // here, unlike a move which would only be safe for a genuinely single-use local.
            quote! { (#tokens).clone() }
        } else {
            tokens
        };
        items.push(tokens);
    }
    Ok(quote! { &[#(#items),*] })
}

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
    // A write to a known class instance's own field (`this.#hooks = hooks;`, `impl.ArrayBuffer =
    // ...;`) — mutates through the wrapper's `Rc<RefCell<Inner>>` (`borrow_mut()`), and wraps in
    // `Some(...)` for a field whose Rust type is `Option<_>` (every `?`/`!`-declared field — see
    // `ClassField`'s doc comment; both are unset at construction, so both need the same wrapping
    // on write, even though only `?` keeps reading back as `Option` afterward).
    if let Expr::Member { obj, prop } = target
        && let Some(field_name) = member_prop_field_name(prop)
        && let Some((class_name, obj_tokens)) = class_instance_obj(obj)
    {
        let field = class_field_lookup(&class_name, field_name);
        let field_ident = format_ident!("{}", to_snake_case(field_name));
        let value_tokens = emit_expr(value)?;
        let wrapped = if field.is_some_and(|f| f.optional || f.definite_assignment) {
            quote! { Some(#value_tokens) }
        } else {
            value_tokens
        };
        return Ok(quote! { #obj_tokens.inner.borrow_mut().#field_ident = #wrapped });
    }
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
    // A ternary where one branch is a literal `undefined`/`null` and the other is itself
    // `Option`-shaped on the Rust side (a `Map`/`WeakMap` `.get(...)` lookup — see
    // `intrinsics::MAP_GET`) needs the *whole* ternary treated as `Option`-shaped too: the
    // `undefined`/`null` branch must become `None`, not the ordinary guest `undefined`/`null`
    // value `emit_expr`'s default `Lit::Undefined`/`Lit::Null` mapping produces (correct
    // everywhere else) — see e.g. `array-buffer.ts`'s `BufferPrimordialImpl::record`, whose whole
    // body is exactly this ternary.
    if let Some((option_branch, literal_is_cons)) = option_shaped_ternary(cons, alt) {
        let option_tokens = emit_expr(option_branch)?;
        let test_tokens = emit_expr(test)?;
        return Ok(if literal_is_cons {
            quote! { (if #test_tokens { None } else { #option_tokens }) }
        } else {
            quote! { (if #test_tokens { #option_tokens } else { None }) }
        });
    }
    let test_tokens = emit_expr(test)?;
    let cons_tokens = emit_expr(cons)?;
    let alt_tokens = emit_expr(alt)?;
    Ok(quote! { (if #test_tokens { #cons_tokens } else { #alt_tokens }) })
}

/// If exactly one of `cons`/`alt` is a literal `undefined`/`null` and the other is a `Map`/
/// `WeakMap` `.get(...)` lookup (`Option`-shaped on the Rust side), returns the non-literal
/// branch and whether the *literal* one was `cons` (the `? ... :` branch, not the `: ...` one) —
/// see `emit_cond`'s doc comment.
fn option_shaped_ternary<'a>(cons: &'a Expr, alt: &'a Expr) -> Option<(&'a Expr, bool)> {
    let is_undef_ish = |e: &Expr| matches!(e, Expr::Lit(Lit::Undefined) | Expr::Lit(Lit::Null));
    let is_option_shaped = |e: &Expr| matches!(e, Expr::HostIntrinsic { name, .. } if *name == intrinsics::MAP_GET);
    if is_undef_ish(cons) && is_option_shaped(alt) {
        return Some((alt, true));
    }
    if is_option_shaped(cons) && is_undef_ish(alt) {
        return Some((cons, false));
    }
    None
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

/// A string/number/bool literal needs an explicit `Tenant` constructor whenever it flows into a
/// `T::Value`-typed position — `emit_expr` has no way to know a bare `"foo"`/`5`/`true` literal
/// needs `Tenant::string_value`/etc. without knowing the surrounding context expects a guest
/// value. Shared by `coerce_trap_return_value` (a trap's own `return`) and `emit_bin`'s
/// `args[N] ?? DEFAULT` handling (the `DEFAULT` fallback).
fn coerce_literal_to_value(expr: &Expr, tokens: TokenStream) -> TokenStream {
    match expr {
        Expr::Lit(Lit::Str(_)) => quote! { tenant.string_value(&(#tokens)) },
        Expr::Lit(Lit::Num(_)) => quote! { tenant.number_value(#tokens) },
        Expr::Lit(Lit::Bool(_)) => quote! { tenant.boolean_value(#tokens) },
        // A struct field read (`record.length`) whose declared TS type is a plain `number` — a
        // native `f64`, not a guest value, returned directly from a trap (`typed-arrays.ts`'s
        // `get` trap: `if (key === "length") return record.length;`). Every other struct field
        // this codebase has needed so far has itself been guest-`T::Value`-typed
        // (`object`/`Function`/`unknown`); `Record_.length`/`.offset` are the first `number`
        // fields. Not receiver-specific (any struct's `number` field matches) — conservative but
        // safe, since a real `T::Value` field is never itself typed plain `number`.
        Expr::Member { obj, prop: MemberProp::Ident(field) } if matches!(obj.as_ref(), Expr::Ident(_)) && struct_field_is_number(field) => {
            quote! { tenant.number_value(#tokens) }
        }
        // An arithmetic expression (`record.length * codec.bytes`) always produces a native
        // `f64` in this IR's own number model, never a guest value — needs the same wrapping a
        // bare numeric literal does.
        Expr::Bin { op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod, .. } => {
            quote! { tenant.number_value(#tokens) }
        }
        // A private class field read directly returned (`proxy.ts`'s `ProxyStateImpl.target()`:
        // `return this.#target;`) — `emit_member`'s class-field branch deliberately produces a
        // *borrow* through `self.inner.borrow().<field>` with no `.clone()` (see its own doc
        // comment: every other occurrence is itself the receiver of a further call, which
        // bypasses this coercion entirely via `emit_call`'s own class-field-aware branches). A
        // bare `return` of that borrow would try to move out of the temporary `Ref`, which
        // doesn't live past the statement — needs an explicit clone here instead.
        Expr::Member { obj, prop } if member_prop_field_name(prop).is_some() && class_instance_obj(obj).is_some() => {
            quote! { (#tokens).clone() }
        }
        _ => tokens,
    }
}

fn struct_field_is_number(field_name: &str) -> bool {
    STRUCT_DEFS.with(|d| {
        d.borrow()
            .values()
            .any(|def| def.fields.iter().any(|(name, ty)| name == field_name && matches!(ty, TypeRef::Named(n) if n == "number")))
    })
}

fn emit_bin(op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<TokenStream, IrError> {
    match op {
        // `args[N] ?? DEFAULT` — `args[N]`'s own Rust translation already yields a *guest*
        // `T::Value` that defaults to `undefined` for an out-of-bounds index (see `emit_member`'s
        // doc comment on its computed-member fallback), not Rust's `None` — so the nullish check
        // has to happen at the guest-value level (`Tenant::typeof_tag`), not via `Option::
        // unwrap_or` (there is no `Option` here for that generic case to apply to).
        BinOp::Nullish
            if matches!(lhs, Expr::Member { prop: MemberProp::Computed(idx), .. } if matches!(idx.as_ref(), Expr::Lit(Lit::Num(_))))
                // A bare guest `T::Value` identifier (`first ?? 0` — `const first = args[0];`)
                // needs the same `tenant.typeof_tag`-based nullish check `args[N] ?? ...` does,
                // not the generic `Option::unwrap_or` fallback below (which is for a genuinely
                // `Option<_>`-typed `lhs`, e.g. a `getPrototypeOf` result) — excluded here by
                // checking it's neither `Option`- nor `f64`-typed, the two other recognized shapes.
                || matches!(lhs, Expr::Ident(name) if !is_option_local(&to_snake_case(name)) && !is_f64_typed(&to_snake_case(name))) =>
        {
            let lhs_t = emit_expr(lhs)?;
            let rhs_t = emit_expr(rhs)?;
            // The fallback needs to end up a guest `T::Value` either way: a literal needs the
            // usual `Tenant` constructor (`coerce_literal_to_value`), and a bare identifier that
            // is itself a bare host `f64` (`F64_TYPED_LOCALS` — a `BufferHooks::byte_length`
            // result, e.g. `args[1] ?? length`) needs the same `number_value` wrapping, just
            // applied here instead of at its own `let` site (which also needs it to stay a plain
            // `f64` for direct `Math.min`/`max` use).
            let rhs_t = if matches!(rhs, Expr::Ident(name) if is_f64_typed(&to_snake_case(name))) {
                quote! { tenant.number_value(#rhs_t) }
            } else {
                coerce_literal_to_value(rhs, rhs_t)
            };
            Ok(quote! {
                {
                    let __arg = #lhs_t;
                    if matches!(tenant.typeof_tag(&__arg), ValueTag::Undefined | ValueTag::Null) { #rhs_t } else { __arg }
                }
            })
        }
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
    // `kind === "shared-array-buffer"`-style comparisons against a known `BufferKind`-typed
    // local — see `TYPE_SHIMS`/`BUFFER_KIND_VARIANTS`.
    if let Expr::Ident(name) = lhs
        && is_buffer_kind_typed(&to_snake_case(name))
        && let Expr::Lit(Lit::Str(s)) = rhs
        && let Some((_, variant)) = BUFFER_KIND_VARIANTS.iter().find(|(lit, _)| lit == s)
    {
        let lhs_t = emit_expr(lhs)?;
        let variant_ident = format_ident!("{variant}");
        let op_t = bin_op_tokens(op)?;
        return Ok(quote! { (#lhs_t #op_t BufferKind::#variant_ident) });
    }
    // `typeof key === "string"`/`!==` where `key` is known `PropertyKey`-typed (a trap's own
    // `key: &PropertyKey` parameter, never a guest `T::Value`) — `Tenant::typeof_tag` doesn't
    // apply to a host-level `PropertyKey` at all; this is really just "is this key a string
    // variant, not a symbol", which `intrinsics::IS_STRING_KEY`'s own translation already
    // expresses. Checked before the generic `typeof X === "<tag>"` case below, which otherwise
    // assumes `X` is always a guest value.
    if let Expr::Un { op: UnOp::TypeOf, arg } = lhs
        && let Expr::Ident(name) = arg.as_ref()
        && is_property_key_typed(&to_snake_case(name))
        && let Expr::Lit(Lit::Str(tag)) = rhs
        && tag == "string"
    {
        let arg_t = emit_expr(arg)?;
        return Ok(if op == BinOp::Eq {
            quote! { matches!(#arg_t, portal_solutions_jade_tenant_rt::PropertyKey::String(_)) }
        } else {
            quote! { !matches!(#arg_t, portal_solutions_jade_tenant_rt::PropertyKey::String(_)) }
        });
    }
    if let Expr::Un { op: UnOp::TypeOf, arg } = lhs
        && let Expr::Lit(Lit::Str(tag)) = rhs
        && let Some(variant) = value_tag_variant(tag)
    {
        let arg_t = emit_call_arg(arg, shims::ArgKind::Ref)?;
        let cmp = if op == BinOp::Eq { quote! { == } } else { quote! { != } };
        return Ok(quote! { (tenant.typeof_tag(#arg_t) #cmp ValueTag::#variant) });
    }
    if is_nullish_lit(rhs) {
        // `result.value` on a `TRAP_RESULT_TYPED_LOCALS` local (`proxy.ts`'s `TrapResult.value`)
        // shares its field name with `TenantPropertyDescriptor.value` — excluded here so it goes
        // through `emit_member`'s own `TrapResult`-aware translation (an unwrap, not
        // `.is_none()`/`.is_some()` on the field directly) instead of being mistaken for one.
        let is_trap_result_field = matches!(lhs, Expr::Member { obj, .. }
            if matches!(obj.as_ref(), Expr::Ident(n) if TRAP_RESULT_TYPED_LOCALS.with(|m| m.borrow().contains(&to_snake_case(n)))));
        if let Expr::Member { obj, prop: MemberProp::Ident(field) } = lhs
            && DESCRIPTOR_FIELDS.contains(&field.as_str())
            && !is_trap_result_field
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
    // `key === "byteLength"`-style comparisons (an exotic-handler trap's own `key: &PropertyKey`
    // parameter against a string literal) — `PropertyKey` has no `PartialEq<str>` impl, so the
    // literal needs `PropertyKey::from(...)` conversion before the two sides can compare at all.
    if let Expr::Ident(name) = lhs
        && is_property_key_typed(&to_snake_case(name))
        && let Expr::Lit(Lit::Str(_)) = rhs
    {
        let lhs_t = emit_expr(lhs)?;
        let rhs_t = emit_expr(rhs)?;
        let op_t = bin_op_tokens(op)?;
        return Ok(quote! { (#lhs_t #op_t &PropertyKey::from(#rhs_t)) });
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
        // `keys as PropertyKey[]` — `keys: Vec<T::Value>` (from `guestArrayLike`) needs a
        // per-element `Tenant::to_property_key` conversion, not a bare type-level erasure.
        TypeRef::Array(inner) if matches!(inner.as_ref(), TypeRef::Named(n) if n == "PropertyKey") => {
            let e = emit_expr(expr)?;
            Ok(quote! { (#e).iter().map(|__k| tenant.to_property_key(__k)).collect::<Vec<_>>() })
        }
        _ => emit_expr(expr),
    }
}

fn emit_call(callee: &Expr, args: &[CallArg]) -> Result<TokenStream, IrError> {
    // A `.get(...)`/`.set(...)` call on a `CODEC_TYPED_LOCALS` local (`codec.get(new DataView(...),
    // 0)`, `sourceCodec.get(...)`, `codec.set(new DataView(...), 0, x)`) — `codec` is a bare enum
    // value (see `CODEC_TYPED_LOCALS`'s doc comment), and these become the inherent
    // `codec_get`/`codec_set` methods `try_emit_data_view_codec_table` generated. `DataView`'s own
    // wrapper is already erased to the underlying byte buffer at lowering time (`lower.rs`), so the
    // buffer argument just needs the right borrow; the offset argument needs an explicit `as
    // usize` cast (TS numbers are `f64` throughout this IR, but byte offsets are `usize`).
    if let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee
        && let Expr::Ident(name) = obj.as_ref()
        && CODEC_TYPED_LOCALS.with(|m| m.borrow().contains_key(&to_snake_case(name)))
        && matches!(method.as_str(), "get" | "set")
    {
        let obj_tokens = emit_expr(obj)?;
        let method_ident = format_ident!("codec_{method}");
        return match (method.as_str(), args) {
            ("get", [CallArg::Normal(view_expr), CallArg::Normal(offset_expr)]) => {
                let view_tokens = emit_expr(view_expr)?;
                let offset_tokens = emit_expr(offset_expr)?;
                Ok(quote! { #obj_tokens.#method_ident(&(#view_tokens), (#offset_tokens) as usize) })
            }
            ("set", [CallArg::Normal(view_expr), CallArg::Normal(offset_expr), CallArg::Normal(value_expr)]) => {
                let view_tokens = emit_expr(view_expr)?;
                let offset_tokens = emit_expr(offset_expr)?;
                let value_tokens = emit_expr(value_expr)?;
                Ok(quote! { #obj_tokens.#method_ident(&mut (#view_tokens), (#offset_tokens) as usize, #value_tokens) })
            }
            _ => Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("codec table `.{method}(...)` called with an unexpected argument shape"),
            }),
        };
    }

    if let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee
        && let Expr::Ident(recv) = obj.as_ref()
        && recv == "tenant"
        && method == "makeExotic"
        && let [CallArg::Normal(proto_expr), CallArg::Normal(Expr::Object(props))] = args
    {
        return emit_exotic_handler_literal(proto_expr, props);
    }

    // `Array.from({ length: N }, (elem, index) => EXPR)` — produces one computed value per index
    // `0..N`. Only the specific shape `typed-arrays.ts`'s `ownKeys`/`ownPropertyKeys` traps need
    // (`Array.from({ length: record.length }, (_, i) => String(i))`, producing string property
    // keys for a `TrapReturn::KeyList` return) is recognized; anything else calling `Array.from`
    // is a hard rejection rather than a general translation.
    if let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee
        && matches!(obj.as_ref(), Expr::Ident(n) if n == "Array")
        && method == "from"
        && let [CallArg::Normal(Expr::Object(props)), CallArg::Normal(Expr::Closure(mapper))] = args
        && let [ObjectProp::KeyValue { key: PropKey::Ident(k), value: length_expr }] = props.as_slice()
        && k == "length"
        && let [_elem_param, Param { pattern: Pattern::Ident(index_param), .. }] = mapper.params.as_slice()
        && let [Stmt::Return(Some(result_expr))] = mapper.body.0.as_slice()
    {
        let len_tokens = emit_expr(length_expr)?;
        let index_ident = format_ident!("{}", to_snake_case(index_param));
        let saved = LOCAL_REFNESS.with(|m| m.borrow().clone());
        LOCAL_REFNESS.with(|m| {
            m.borrow_mut().insert(to_snake_case(index_param), false);
        });
        let result_tokens = emit_expr(result_expr)?;
        LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved);
        return Ok(quote! {
            (0..(#len_tokens) as usize).map(|__index| {
                let #index_ident = __index as f64;
                PropertyKey::from(#result_tokens)
            }).collect::<Vec<_>>()
        });
    }

    // A method call on a known class instance (`that.shell(tenant, ...)`, `impl.makeConstructor
    // (tenant, ...)`) — resolved against that class's own `ClassMethod` list (see
    // `class_instance_obj`). Every argument is passed per the *method's own* declared parameter
    // type, the same convention `LOCAL_FN_PARAMS`'s branch below uses for top-level functions
    // (including the bare `tenant` argument itself: `arg_kind_for_type` has no special case for
    // it and falls through to `Owned`, which just emits the bare identifier — Rust's implicit
    // reborrowing of an already-`&mut T` value in argument position makes that work correctly, as
    // proven by every existing local-function-call site already doing exactly this).
    if let Expr::Member { obj, prop: MemberProp::Ident(method) } = callee
        && let Some((class_name, obj_tokens)) = class_instance_obj(obj)
        && let Some(def) = class_def(&class_name)
        && let Some(class_method) = def.methods.iter().find(|m| &m.name == method)
    {
        let method_ident = format_ident!("{}", to_snake_case(method));
        let param_tys: Vec<Option<TypeRef>> = class_method.func.params.iter().map(|p| p.ty.clone()).collect();
        if args.len() != param_tys.len() {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("`{class_name}.{method}(...)` called with {} args, expected {}", args.len(), param_tys.len()),
            });
        }
        let mut lets = Vec::new();
        let mut arg_tokens = Vec::new();
        for (i, (arg, ty)) in args.iter().zip(param_tys.iter()).enumerate() {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("spread argument to `{class_name}.{method}(...)`"),
                });
            };
            // A `BufferKind`-typed parameter fed a string literal (`that.shell(tenant, "array-
            // buffer", ...)`) needs the enum variant, not the bare string `arg_kind_for_type`/
            // `emit_call_arg` would otherwise pass through unchanged (neither knows about
            // `TYPE_SHIMS`-mapped types at all — this mirrors `emit_eq_cmp`'s own
            // `BUFFER_KIND_VARIANTS` lookup for the comparison case).
            if matches!(ty, Some(TypeRef::Named(n)) if n == "BufferKind")
                && let Some(variant) = buffer_kind_literal(expr)
            {
                arg_tokens.push(variant);
                continue;
            }
            let kind = ty.as_ref().map(arg_kind_for_type).unwrap_or(shims::ArgKind::Owned);
            let tokens = emit_call_arg(expr, kind)?;
            // Same borrow-conflict hazard `tenant.<method>(...)` calls already hit (see
            // `maybe_hoist_arg`'s own doc comment): an argument expression that itself borrows
            // `tenant` (e.g. `that.#hooks.allocate(kind, toIndex(args[0] ?? 0))`, computed while
            // `tenant` is already borrowed as this call's own leading argument) needs hoisting
            // into its own statement first.
            arg_tokens.push(maybe_hoist_arg(expr, tokens, i, &mut lets));
        }
        // Thread through whatever extra trailing parameter(s) `emit_class_method` injected into
        // this method's own signature (a per-tenant-cache or cross-file-factory-cache argument
        // the TS call site never passes) — see `CLASS_METHOD_EXTRA_ARGS`'s doc comment.
        if let Some(extra) = CLASS_METHOD_EXTRA_ARGS.with(|m| m.borrow().get(&(class_name.clone(), method.clone())).cloned()) {
            arg_tokens.extend(extra);
        }
        // A non-generator method is never wrapped in `TenantYield` at its call site (that only
        // ever wraps a generator method's own call, appending `?` there), so if it's Rust-side
        // `Result`-returning at all (`class_method.func.body` itself throws, or transitively
        // calls another throwing method — see `expr_throws`'s own class-method-call case,
        // `ProxyStateImpl::target`/`::handler` calling `require_live`), this call site is the
        // only place that can propagate it.
        let needs_question_mark = class_method_throws(&class_name, method);
        let call = quote! { #obj_tokens.#method_ident(#(#arg_tokens),*) };
        let call = if needs_question_mark { quote! { (#call)? } } else { call };
        return Ok(if lets.is_empty() { call } else { quote! { { #(#lets)* #call } } });
    }

    // A method call on a class field typed `BufferHooks` (`this.#hooks.byteLength(...)`,
    // `that.#hooks.allocate(...)`) — the `H: BufferHooks` generic parameter's own trait methods,
    // reached by borrowing the field mutably (every `BufferHooks` method takes `&mut self`).
    if let Expr::Member { obj: field_obj, prop: MemberProp::Ident(method) } = callee
        && let Expr::Member { obj: recv_obj, prop: recv_prop } = field_obj.as_ref()
        && let Some(field_name) = member_prop_field_name(recv_prop)
        && let Some((class_name, recv_tokens)) = class_instance_obj(recv_obj)
        && let Some(field) = class_field_lookup(&class_name, field_name)
        && matches!(&field.ty, TypeRef::Named(n) if n == "BufferHooks")
    {
        let Some(spec) = buffer_hooks_method(method) else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("`BufferHooks` has no recognized method `{method}`"),
            });
        };
        if args.len() != spec.params.len() {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("`BufferHooks.{method}(...)` called with {} args, expected {}", args.len(), spec.params.len()),
            });
        }
        let field_ident = format_ident!("{}", to_snake_case(field_name));
        let method_ident = format_ident!("{}", spec.rust_name);
        let mut arg_tokens = Vec::new();
        for (arg, kind) in args.iter().zip(spec.params.iter()) {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "spread argument to a `BufferHooks` method call".into(),
                });
            };
            arg_tokens.push(emit_buffer_hooks_arg(expr, *kind)?);
        }
        let call = quote! { #recv_tokens.inner.borrow_mut().#field_ident.#method_ident(#(#arg_tokens),*) };
        return Ok(if spec.returns_usize { quote! { (#call).map(|v| v as f64) } } else { call });
    }

    // A method call on a class field typed as a *cross-file* class's own interface
    // (`that.#buffers.record(...)`, `that.#buffers.shell(...)`) — `BufferPrimordialImpl` lives in
    // a different generated file, so this can't resolve through `CLASS_DEFS`/`class_def` (which
    // only knows this module's own classes) the way the local class-instance-method-call branch
    // above does. See `cross_file::CLASS_TABLE`. The field itself is a `BufferPrimordialImpl<T,
    // H>` value (already reference-shared via its own `Rc<RefCell<Inner>>`), so no clone is
    // needed just to call an inherent `&self` method through the borrow.
    if let Expr::Member { obj: field_obj, prop: MemberProp::Ident(method) } = callee
        && let Expr::Member { obj: recv_obj, prop: recv_prop } = field_obj.as_ref()
        && let Some(field_name) = member_prop_field_name(recv_prop)
        && let Some((class_name, recv_tokens)) = class_instance_obj(recv_obj)
        && let Some(field) = class_field_lookup(&class_name, field_name)
        && let TypeRef::Named(interface_name) = &field.ty
        && let Some(cross_class) = cross_file::lookup_class(interface_name)
        && let Some(cross_method) = cross_class.methods.iter().find(|m| m.ts_name == method)
    {
        if args.len() != cross_method.param_types.len() {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!(
                    "`{interface_name}.{method}(...)` called with {} args, expected {}",
                    args.len(),
                    cross_method.param_types.len()
                ),
            });
        }
        let field_ident = format_ident!("{}", to_snake_case(field_name));
        let method_ident = format_ident!("{}", cross_method.rust_name);
        let mut arg_tokens = Vec::new();
        for (arg, ty_name) in args.iter().zip(cross_method.param_types.iter()) {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("spread argument to `{interface_name}.{method}(...)`"),
                });
            };
            let ty = TypeRef::Named((*ty_name).to_string());
            if matches!(&ty, TypeRef::Named(n) if n == "BufferKind")
                && let Some(variant) = buffer_kind_literal(expr)
            {
                arg_tokens.push(variant);
                continue;
            }
            let kind = arg_kind_for_type(&ty);
            arg_tokens.push(emit_call_arg(expr, kind)?);
        }
        return Ok(quote! { #recv_tokens.inner.borrow().#field_ident.#method_ident(#(#arg_tokens),*) });
    }

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
        let mut lets = Vec::new();
        let mut arg_tokens = Vec::new();
        for (i, (arg, kind)) in args.iter().zip(arg_refs).enumerate() {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: "spread argument to a tenant method".into(),
                });
            };
            let tokens = emit_call_arg(expr, *kind)?;
            arg_tokens.push(maybe_hoist_arg(expr, tokens, i, &mut lets));
        }
        let call = quote! { tenant.#method_ident(#(#arg_tokens),*) };
        return Ok(if lets.is_empty() { call } else { quote! { { #(#lets)* #call } } });
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
        // Leading argument is always the bare `tenant` this crate expects; anything after that
        // matches `factory.extra_params` one-for-one (`bufferPrimordial(tenant, hooks)` has one).
        let Some((CallArg::Normal(Expr::Ident(tenant_arg)), rest)) = args.split_first() else {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("{name}(...) called with an argument shape other than a leading bare `tenant` this crate expects"),
            });
        };
        if tenant_arg != "tenant" {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!("{name}(...) called with a non-`tenant` leading argument"),
            });
        }
        if rest.len() != factory.extra_params.len() {
            return Err(IrError::Unsupported {
                file: String::new(),
                construct: format!(
                    "{name}(...) called with {} args, expected 1 (tenant) + {}",
                    args.len(),
                    factory.extra_params.len()
                ),
            });
        }
        let path: TokenStream = factory.rust_fn_path.parse().map_err(|e| IrError::Unsupported {
            file: String::new(),
            construct: format!("cross-file factory `{name}` rust_fn_path did not parse as Rust: {e}"),
        })?;
        let mut arg_tokens = vec![quote! { tenant }];
        for (arg, (_, ty_name)) in rest.iter().zip(factory.extra_params.iter()) {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("spread argument to cross-file factory `{name}(...)`"),
                });
            };
            // A `BufferHooks`-typed extra param (`hooks`) is cloned, not moved: the caller
            // (`typedArraysPrimordial`) needs its own copy afterward too (`new
            // TypedArrayPrimordialImpl(buffers, hooks, ...)`), and `H: Clone` is already part of
            // `emit_fn_decl`'s generated bound for any function with a `BufferHooks` parameter.
            if *ty_name == "BufferHooks" {
                let tokens = emit_expr(expr)?;
                arg_tokens.push(quote! { (#tokens).clone() });
                continue;
            }
            let kind = arg_kind_for_type(&TypeRef::Named((*ty_name).to_string()));
            arg_tokens.push(emit_call_arg(expr, kind)?);
        }
        let cache_ident = format_ident!("{}", cross_file::cache_param_name(factory));
        arg_tokens.push(quote! { #cache_ident });
        for transitive in factory.transitive_caches {
            let transitive_factory = cross_file::TABLE.iter().find(|f| f.struct_name == *transitive).ok_or_else(|| IrError::Unsupported {
                file: String::new(),
                construct: format!("cross-file factory `{name}` names an unregistered transitive cache `{transitive}`"),
            })?;
            let transitive_ident = format_ident!("{}", cross_file::cache_param_name(transitive_factory));
            arg_tokens.push(quote! { #transitive_ident });
        }
        // Never `?`-suffixed here either — same reasoning as the local-function-call and shim
        // branches: every real call site wraps this in `yield tenant.yieldTenant(...)`, which
        // already appends the `?`.
        return Ok(quote! { #path(#(#arg_tokens),*) });
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
        let mut lets = Vec::new();
        let mut arg_tokens = Vec::new();
        for (i, (arg, ty)) in args.iter().zip(param_tys.iter()).enumerate() {
            let CallArg::Normal(expr) = arg else {
                return Err(IrError::Unsupported {
                    file: String::new(),
                    construct: format!("spread argument to local function call `{name}`"),
                });
            };
            // A `readonly unknown[]`-typed array-literal argument (`trap`'s own `args` parameter
            // — `proxy.ts`'s `trap(tenant, state, "get", [state.target(), key, receiver])`) needs
            // per-element conversion (a `PropertyKey`-typed element, e.g. `key`, becomes a guest
            // value via `Tenant::property_key_value`) and an outer `&`, neither of which
            // `arg_kind_for_type`/`emit_call_arg`'s generic per-type handling covers (no
            // `TypeRef::Array` case at all, only `TypeRef::Named`).
            if let Expr::Array(elements) = expr
                && matches!(ty, Some(TypeRef::Array(inner)) if matches!(inner.as_ref(), TypeRef::Named(n) if n == "unknown"))
            {
                let tokens = emit_value_slice_array_literal(elements)?;
                arg_tokens.push(maybe_hoist_arg(expr, tokens, i, &mut lets));
                continue;
            }
            // A `ClassInterface | undefined`-typed parameter (`proxyExotic`'s own `existingState:
            // ProxyState | undefined`) resolves to `Option<ProxyStateImpl<T>>` in Rust (see
            // `emit_fn_decl`'s `OPTION_LOCALS` registration for this exact shape) — an explicit
            // `undefined`/`null` argument needs `None`, and a real class-instance argument needs
            // `Some((...).clone())`, neither of which `arg_kind_for_type`'s generic `TypeRef`
            // dispatch covers (no `TypeRef::Optional` case at all).
            if let Some(TypeRef::Optional(inner)) = ty
                && let TypeRef::Named(n) = inner.as_ref()
                && class_backing_interface(n).is_some()
            {
                let tokens = if is_nullish_lit(expr) {
                    quote! { None }
                } else {
                    let inner_tokens = emit_expr(expr)?;
                    quote! { Some((#inner_tokens).clone()) }
                };
                arg_tokens.push(maybe_hoist_arg(expr, tokens, i, &mut lets));
                continue;
            }
            let kind = ty.as_ref().map(arg_kind_for_type).unwrap_or(shims::ArgKind::Owned);
            let tokens = emit_call_arg(expr, kind)?;
            // A class-instance-typed parameter (`trap`'s own `state: ProxyState`) is passed by
            // value in Rust too (matching the TS signature, no `&`), but the caller's own local
            // almost always outlives this one call (`proxy.ts`'s traps reuse `state` in a
            // fallback branch after the `trap(...)` call returns, sometimes even later in the
            // very same argument list via `state.target()`) — clone the cheap `Rc<RefCell<...>>`
            // wrapper rather than moving it, the same convention captured class-instance fields
            // already use (see `emit_exotic_handler_literal`'s `field_inits`).
            let tokens = if matches!(kind, shims::ArgKind::Owned)
                && matches!(ty, Some(TypeRef::Named(n)) if class_backing_interface(n).is_some())
            {
                quote! { (#tokens).clone() }
            } else {
                tokens
            };
            arg_tokens.push(maybe_hoist_arg(expr, tokens, i, &mut lets));
        }
        // Never `?`-suffixed here: every local function in the surveyed source is only ever
        // called wrapped in `TenantYield` (`yield tenant.yieldTenant(installMethod(...))`),
        // which already appends the `?` — see `emit_shim_call`'s matching note.
        return Ok(quote! { {#(#lets)* #fn_ident(#(#arg_tokens),*)} });
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
/// Hoists an already-`ArgKind`-wrapped argument token into its own `let` binding right before
/// the call, if its rendered text suggests it might need a *fresh* mutable borrow of `tenant`
/// mid-evaluation (contains a `?` — some fallible sub-call — or the bare word `tenant`). Both
/// `emit_call`'s `tenant.<method>(...)` branch and `emit_shim_call` use this: a call that already
/// borrows `tenant` (as receiver, or as its own leading argument) can't also evaluate another
/// argument that needs its own live mutable borrow of `tenant` inline — a class of borrow-checker
/// rejection this crate hit repeatedly in different call shapes (`args[N]`'s `undefined`
/// fallback, `coerce_return_value`'s `boolean_value` wrapping, and now a tenant-method argument
/// that itself performs a nested `TenantYield`-wrapped call, e.g. `Reflect.defineProperty`'s
/// `tenant.defineProperty(a, b, yield tenant.yieldTenant(readGuestDescriptor(tenant, c)))`).
/// Hoisting the inner evaluation into its own statement first releases its borrow before the
/// outer call starts. A coarse text-based heuristic, deliberately conservative (a false positive
/// only costs an extra harmless `let`, never a wrong answer) rather than a precise borrow
/// analysis.
fn maybe_hoist_arg(expr: &Expr, tokens: TokenStream, index: usize, lets: &mut Vec<TokenStream>) -> TokenStream {
    let text = tokens.to_string();
    // The bare `tenant` argument itself (`&mut T`, not `Copy`) must never be hoisted: there's
    // nothing nested inside it to release a borrow from, and moving it into a temporary would
    // make every *later* use of `tenant` in this same call/scope a use-after-move. A closure
    // literal (`makeBuiltin`'s `apply`/`construct` arguments) must never be hoisted either, for a
    // different reason: constructing a closure doesn't touch `tenant` until the closure is later
    // *invoked* (a separate, deferred borrow, not a conflict here), and hoisting it into an
    // unannotated `let` loses the call site's expected-type hint that `Box::new(closure)` needs
    // to unsize-coerce to `Box<dyn FnMut(...)>` — a real regression this crate hit (the
    // intermediate binding infers the closure's own concrete type instead).
    if text == "tenant" || matches!(expr, Expr::Closure(_)) {
        return tokens;
    }
    if text.contains('?') || word_referenced(&text, "tenant") {
        let ident = format_ident!("__hoisted_arg{index}");
        lets.push(quote! { let #ident = #tokens; });
        quote! { #ident }
    } else {
        tokens
    }
}

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
    let mut lets = Vec::new();
    let mut arg_tokens = Vec::new();
    if spec.inject_tenant {
        arg_tokens.push(quote! { tenant });
    }
    for (i, arg_spec) in spec.args.iter().enumerate() {
        let (expr_opt, tokens) = match args.get(i) {
            Some(CallArg::Normal(expr)) => (Some(expr), emit_call_arg(expr, arg_spec.kind)?),
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
                let tokens = default_literal.parse::<TokenStream>().map_err(|e| IrError::Unsupported {
                    file: String::new(),
                    construct: format!("shim `{}` default literal did not parse as Rust: {e}", spec.name),
                })?;
                (None, tokens)
            }
        };
        arg_tokens.push(match expr_opt {
            Some(expr) => maybe_hoist_arg(expr, tokens, i, &mut lets),
            // A default-literal argument (`"None"`) is never a closure and never references
            // `tenant`, so it never needs hoisting — no real `Expr` exists for it to check.
            None => tokens,
        });
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
    let call = if spec.inject_tenant {
        // `toIndex`'s Rust counterpart returns `usize` (a genuine array index/count), but every
        // caller in the surveyed source immediately mixes its result into ordinary `number` (f64)
        // arithmetic (`record.offset + index * codec.bytes`, `start + source.length`) rather than
        // using it as a direct index — casting to `f64` right here, once, keeps it consistent with
        // the rest of this IR's number handling instead of leaving `usize` to leak into arithmetic
        // that expects `f64` everywhere it's eventually used. Harmless where a caller separately
        // casts back to `usize` at a real indexing boundary (`f64 as usize` is exact for the
        // integer values `toIndex` only ever produces).
        if spec.name == "toIndex" {
            quote! { ((#path(#(#arg_tokens),*))? as f64) }
        } else {
            quote! { (#path(#(#arg_tokens),*))? }
        }
    } else {
        quote! { #path(#(#arg_tokens),*) }
    };
    Ok(if lets.is_empty() { call } else { quote! { { #(#lets)* #call } } })
}

fn emit_host_intrinsic(name: &str, args: &[Expr]) -> Result<TokenStream, IrError> {
    // `Number(x)` — needs a genuinely different Rust translation depending on what `x` is,
    // neither of which the generic per-name template below can express (checked here, before
    // it, since it depends on the specific argument's own known type, not just the intrinsic
    // name). Every occurrence before `typed-arrays.ts` was `Number(key)` with `key: &PropertyKey`
    // (a host-level string/symbol key, not a guest value) — `PropertyKey` has no `.parse()` of
    // its own, so this parses its `Display` text instead (only ever reached after an
    // array-index-string guard in the surveyed source, so always the `String` variant in
    // practice). `typed-arrays.ts`'s `Number(value)` is the first case where the argument is an
    // ordinary guest `T::Value` instead, needing the real `Tenant::to_number` conversion (JS
    // `Number(x)`'s `ToNumber`).
    if name == intrinsics::TO_NUMBER
        && let [arg] = args
    {
        return Ok(if matches!(arg, Expr::Ident(n) if is_property_key_typed(&to_snake_case(n))) {
            let arg_tokens = emit_expr(arg)?;
            quote! { (#arg_tokens.to_string()).parse::<f64>().unwrap_or(f64::NAN) }
        } else {
            let arg_tokens = emit_call_arg(arg, shims::ArgKind::Ref)?;
            quote! { tenant.to_number(#arg_tokens) }
        });
    }
    // `typeof key === "string"`/`/regex/.test(key)` where `key` is known `PropertyKey`-typed (a
    // trap's own `key: &PropertyKey` parameter) — `Tenant::typeof_tag` doesn't apply to a
    // host-level `PropertyKey` at all (see `emit_eq_cmp`'s matching case for the `typeof`
    // half); `is_array_index_string` needs a real `&str`, extracted here by matching the
    // `String` variant directly (a `Symbol` never matches the array-index pattern regardless,
    // so this stays correct even without the `typeof === "string"` guard actually having run
    // first at the Rust level).
    if name == intrinsics::IS_ARRAY_INDEX_STRING
        && let [arg] = args
        && matches!(arg, Expr::Ident(n) if is_property_key_typed(&to_snake_case(n)))
    {
        let arg_tokens = emit_expr(arg)?;
        return Ok(quote! {
            matches!(#arg_tokens, portal_solutions_jade_tenant_rt::PropertyKey::String(__s) if portal_solutions_jade_tenant_rt::intrinsics::is_array_index_string(__s))
        });
    }
    let Some(spec) = intrinsics::spec(name) else {
        return Err(IrError::Unsupported {
            file: String::new(),
            construct: format!("host intrinsic `{name}` has no Rust lowering registered"),
        });
    };
    // `Map`/`WeakMap` accessor methods (`get`/`set`/`has`/`delete`) need two adjustments when
    // the receiver (`args[0]`) is a class field: the field access must be a `borrow`/`borrow_mut`
    // *through* the class wrapper rather than going through `emit_member`'s ordinary handling
    // (which, for anything else, is either not a class field at all or is deliberately borrow-
    // only already — see its own doc comment), with mutability matching whether this specific
    // method mutates; and if the field's own declared type is a guest-value-keyed `Map`/`WeakMap`
    // (`is_object_identity_map_type` — stored as `HashMap<T::ObjectId, V>`, see `rust_type`), the
    // key argument (`args[1]`) needs `Tenant::object_id` conversion rather than a bare `&key`.
    let is_map_method = [intrinsics::MAP_GET, intrinsics::MAP_SET, intrinsics::MAP_HAS, intrinsics::MAP_DELETE].contains(&name);
    let needs_mut_receiver = name == intrinsics::MAP_SET || name == intrinsics::MAP_DELETE;
    let receiver_field = args.first().and_then(|a| {
        let Expr::Member { obj, prop } = a else { return None };
        let field_name = member_prop_field_name(prop)?;
        let (class_name, obj_tokens) = class_instance_obj(obj)?;
        let field = class_field_lookup(&class_name, field_name)?;
        Some((obj_tokens, field_name.to_string(), field))
    });
    let object_identity_key = is_map_method && receiver_field.as_ref().is_some_and(|(_, _, f)| is_object_identity_map_type(&f.ty));
    let mut rendered = spec.rust_template.to_string();
    for (i, arg) in args.iter().enumerate() {
        let tokens = if is_map_method && i == 0 && let Some((obj_tokens, field_name, _)) = &receiver_field {
            let field_ident = format_ident!("{}", to_snake_case(field_name));
            let borrow = if needs_mut_receiver { quote! { borrow_mut } } else { quote! { borrow } };
            quote! { #obj_tokens.inner.#borrow().#field_ident }
        } else if object_identity_key && i == 1 {
            let ref_tokens = emit_call_arg(arg, shims::ArgKind::Ref)?;
            quote! { tenant.object_id(#ref_tokens) }
        } else {
            emit_expr(arg)?
        };
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

    // A closure's own signature is always hard-coded `Result`-returning below, regardless of
    // whatever `CURRENT_FN_THROWS` says about the *enclosing* function/method — see
    // `CURRENT_FN_THROWS`'s doc comment.
    let saved_fn_throws = CURRENT_FN_THROWS.with(|c| *c.borrow());
    CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = true);
    let body_result = emit_block(&func.body);
    CURRENT_FN_THROWS.with(|c| *c.borrow_mut() = saved_fn_throws);
    let body = body_result?;

    // A precise IR walk, *not* a text search over the emitted body: this closure's own generated
    // code routinely spells out Rust struct-literal field labels (`TenantInvocation::Apply {
    // this_arg: ..., args: ... }`) that collide, as plain text, with genuinely-outer-scope names
    // like a sibling closure's own `thisArg`/`args` parameters — a text search would treat the
    // field label as a reference to that outer name and wrongly try to capture it (and, worse,
    // wrongly try to `.clone()` a name already moved earlier in this same scope, or capture a
    // borrowed `&[T::Value]` slice tied to a shorter-lived outer closure — both real bugs this
    // crate hit before switching to this walk). `free_idents` only collects `Expr::Ident`s that
    // are genuine value reads, explicitly skipping struct/object keys and resolvable call
    // callees (`tenant.<method>`, a shim, a local function, a cross-file factory — none of which
    // are ever a captured closure variable).
    let mut referenced = std::collections::HashSet::new();
    free_idents_in_fn(func, &mut referenced);
    let captured: Vec<String> = outer_names.into_iter().filter(|name| name != "tenant" && referenced.contains(name)).collect();

    LOCAL_REFNESS.with(|m| *m.borrow_mut() = saved_refness);
    OPTION_LOCALS.with(|m| *m.borrow_mut() = saved_options);

    let clone_prelude: Vec<TokenStream> = captured
        .iter()
        .map(|name| {
            let ident = format_ident!("{name}");
            // A `&str`-typed capture (e.g. `makeConstructor`'s own `name` parameter, captured by
            // its throw-closure) needs `.to_string()`, not `.clone()`: cloning a `&str` produces
            // another `&str` with the *same* borrowed lifetime, still not `'static` — exactly the
            // shape of problem `tenant`/a per-tenant cache reference can't cross a closure
            // boundary either, just for a borrowed string instead of a borrowed struct.
            if is_string_typed(name) {
                quote! { let #ident = #ident.to_string(); }
            } else {
                quote! { let #ident = #ident.clone(); }
            }
        })
        .collect();

    // A "void" JS function (no `return` at all — `typed-arrays.ts`'s `set` builtin, whose body is
    // just a guard clause and a `for` loop) falls off the end with no value, but this closure's
    // Rust signature is always `Result<T::Value, TenantError>` (never `()`) — every closure up to
    // now happened to always end in an explicit `return`/`throw`, a real invariant of the
    // surveyed source that no longer holds once a loop can be the last statement. Appending a
    // trailing `Ok(tenant.undefined_value())` is always safe even when *not* needed (the body
    // already ends in a `return`/`throw`, i.e. a diverging statement): Rust treats it as merely
    // unreachable code, a warning, not an error.
    let trailing_ok = if matches!(func.body.0.last(), Some(Stmt::Return(_)) | Some(Stmt::Throw(_))) {
        quote! {}
    } else {
        quote! { Ok(tenant.undefined_value()) }
    };
    // See `emit_member`'s doc comment / `emit_fn_decl`'s matching prelude: every closure gets its
    // own `__undefined` local for the same reason (each closure is its own borrow scope for
    // `tenant`).
    Ok(quote! {
        {
            #(#clone_prelude)*
            move |tenant: &mut T, #(#param_tokens),*| -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                #body
                #trailing_ok
            }
        }
    })
}

/// Whether `word` (an exact identifier, not a substring) appears anywhere in `haystack` — used
/// for the per-tenant/cross-file-factory cache-parameter injection in `emit_fn_decl` (checking
/// for a specific, unambiguous synthetic name this crate itself chose, e.g.
/// `object_primordial_cache` — not the general free-variable question, which
/// `free_idents_in_fn` answers precisely instead; see its doc comment for why a text search was
/// not precise enough for that).
fn word_referenced(haystack: &str, word: &str) -> bool {
    haystack.split(|c: char| !c.is_alphanumeric() && c != '_').any(|token| token == word)
}

/// Collects every free (non-parameter) identifier `func`'s body reads as a value, snake_cased,
/// into `out` — the real free-variable walk `emit_closure` uses for capture detection. "Reads as
/// a value" deliberately excludes: object/struct-literal *keys* (`PropKey`/named-field
/// positions), member-access field names, and a call's callee when it resolves to something this
/// emitter calls by a fixed Rust path anyway (a `tenant.<method>`, a shim, a locally-defined
/// function, or a cross-file factory) rather than an actual captured closure value.
fn free_idents_in_fn(func: &FnDecl, out: &mut std::collections::HashSet<String>) {
    let mut inner = std::collections::HashSet::new();
    free_idents_in_block(&func.body, &mut inner);
    let param_names: std::collections::HashSet<String> = func
        .params
        .iter()
        .filter_map(|p| match &p.pattern {
            Pattern::Ident(name) => Some(to_snake_case(name)),
            Pattern::ObjectShallow(_) => None,
        })
        .collect();
    out.extend(inner.into_iter().filter(|name| !param_names.contains(name)));
}

fn free_idents_in_block(block: &Block, out: &mut std::collections::HashSet<String>) {
    for stmt in &block.0 {
        free_idents_in_stmt(stmt, out);
    }
}

fn free_idents_in_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<String>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                free_idents_in_expr(e, out);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => free_idents_in_expr(e, out),
        Stmt::Return(v) => {
            if let Some(e) = v {
                free_idents_in_expr(e, out);
            }
        }
        Stmt::If { cond, then_branch, else_branch } => {
            free_idents_in_expr(cond, out);
            free_idents_in_block(then_branch, out);
            if let Some(b) = else_branch {
                free_idents_in_block(b, out);
            }
        }
        Stmt::ForOf { iter, body, .. } => {
            free_idents_in_expr(iter, out);
            free_idents_in_block(body, out);
        }
        Stmt::ForCounting { start, bound, body, .. } => {
            free_idents_in_expr(start, out);
            free_idents_in_expr(bound, out);
            free_idents_in_block(body, out);
        }
        Stmt::TryCatch { try_block, catch_block, .. } => {
            free_idents_in_block(try_block, out);
            free_idents_in_block(catch_block, out);
        }
        Stmt::Continue => {}
    }
}

/// Whether `callee` resolves to a fixed Rust call target this emitter already knows how to
/// reach (`tenant.<method>`, a shim, a locally-defined function, or a cross-file factory) —
/// mirrors the recognition logic in `emit_call` itself. If so, the callee identifier is not a
/// captured closure value and must not be walked as one.
fn is_resolvable_call_callee(callee: &Expr) -> bool {
    match callee {
        Expr::Member { obj, prop: MemberProp::Ident(method) } => {
            matches!(obj.as_ref(), Expr::Ident(n) if n == "tenant") && tenant_method(method).is_some()
        }
        Expr::Ident(name) => {
            imported_source(name).is_some_and(|source| shims::lookup(&source, name).is_some() || cross_file::lookup(&source, name).is_some())
                || LOCAL_FN_PARAMS.with(|m| m.borrow().contains_key(name))
        }
        _ => false,
    }
}

fn free_idents_in_expr(expr: &Expr, out: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            out.insert(to_snake_case(name));
        }
        Expr::Lit(_) | Expr::ThisArg => {}
        Expr::TemplateLiteral(parts) => {
            for p in parts {
                if let TemplatePart::Expr(e) = p {
                    free_idents_in_expr(e, out);
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Normal(e) | ArrayElement::Spread(e) => free_idents_in_expr(e, out),
                }
            }
        }
        Expr::Object(props) => {
            for p in props {
                match p {
                    // The key itself is a struct/descriptor field *label*, not a value read —
                    // only a `Computed` key (a real runtime expression) counts.
                    ObjectProp::KeyValue { key: PropKey::Computed(e), value } => {
                        free_idents_in_expr(e, out);
                        free_idents_in_expr(value, out);
                    }
                    ObjectProp::KeyValue { key: PropKey::Ident(_), value } => free_idents_in_expr(value, out),
                    ObjectProp::Spread(e) => free_idents_in_expr(e, out),
                    ObjectProp::Method { func, .. } => free_idents_in_fn(func, out),
                }
            }
        }
        Expr::Member { obj, prop } => {
            free_idents_in_expr(obj, out);
            if let MemberProp::Computed(e) = prop {
                free_idents_in_expr(e, out);
            }
        }
        Expr::Call { callee, args } => {
            if !is_resolvable_call_callee(callee) {
                free_idents_in_expr(callee, out);
            }
            for a in args {
                match a {
                    CallArg::Normal(e) | CallArg::Spread(e) => free_idents_in_expr(e, out),
                }
            }
        }
        Expr::New { args, .. } => {
            // `new TypeError(...)`/`new RangeError(...)` (the only recognized `new` callees,
            // per `emit_throw`) are never a captured closure value.
            for a in args {
                match a {
                    CallArg::Normal(e) | CallArg::Spread(e) => free_idents_in_expr(e, out),
                }
            }
        }
        Expr::Closure(func) => free_idents_in_fn(func, out),
        Expr::TenantYield(e)
        | Expr::Un { arg: e, .. }
        | Expr::Spread(e)
        | Expr::Paren(e)
        | Expr::Cast { expr: e, .. }
        | Expr::NonNull(e) => free_idents_in_expr(e, out),
        Expr::Bin { lhs, rhs, .. } => {
            free_idents_in_expr(lhs, out);
            free_idents_in_expr(rhs, out);
        }
        Expr::Cond { test, cons, alt } => {
            free_idents_in_expr(test, out);
            free_idents_in_expr(cons, out);
            free_idents_in_expr(alt, out);
        }
        Expr::Assign { target, value } => {
            free_idents_in_expr(target, out);
            free_idents_in_expr(value, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                free_idents_in_expr(e, out);
            }
        }
        Expr::HostIntrinsic { args, .. } => {
            for a in args {
                free_idents_in_expr(a, out);
            }
        }
    }
}

fn bin_op_tokens(op: BinOp) -> Result<TokenStream, IrError> {
    Ok(match op {
        BinOp::Add => quote! { + },
        BinOp::Sub => quote! { - },
        BinOp::Mul => quote! { * },
        BinOp::Div => quote! { / },
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

#[cfg(test)]
mod tests {
    //! Regression coverage for `emit_exotic_handler_literal`'s trap-return coercion — built after
    //! actually compiling its output as real Rust caught three real bugs in one pass (a
    //! `Lit::Undefined`/`Lit::Null` mapping change elsewhere in this file that made the original
    //! "already `None`" assumption wrong, a missing `self.<field>` shadow-clone prelude for
    //! captured free variables, and a `PropertyKey`-vs-`&str` comparison with no `PartialEq`
    //! impl) — see the primordial-IR plan's "Progress" section for the full story. Asserts on
    //! the emitted *text* rather than compiling it (no downstream crate available from this
    //! crate's own test suite); `crates/jade-primordial-rt`'s own tests are what prove a real
    //! generated primordial file compiles and behaves correctly end to end.

    const SMOKE_SOURCE: &str = r#"
import type { Tenant, TenantGenerator } from "../tenants/types.ts";

export function* makeSmokeExotic(tenant: Tenant, proto: object): TenantGenerator<object> {
  const value = yield tenant.yieldTenant(tenant.makeExotic(proto, {
    *get(receiver, key) {
      if (key === "tag") return "smoke";
      return undefined;
    },
    *set() {},
    *has(_receiver, key) { return key === "tag"; },
    *delete() {},
    *ownKeys() { return []; },
    *ownPropertyKeys() { return []; },
    *getOwnPropertyDescriptor() { return undefined; },
    *defineProperty() { return false; },
    *getPrototypeOf() { return proto; },
    *setPrototypeOf() { return false; },
    *isExtensible() { return true; },
    *preventExtensions() { return true; },
    *define() {},
    *assign() {},
  }));
  return value;
}
"#;

    fn emit() -> String {
        let lowered = crate::lower::lower_module("exotic-smoke", SMOKE_SOURCE).expect("lowering should succeed");
        super::emit_module(&lowered.module, "exotic-smoke")
    }

    #[test]
    fn generates_a_real_trait_impl() {
        let rust = emit();
        // `+ 'static` on `T` here unconditionally now (not just when a captured class instance
        // needs it) — `Box<dyn TenantExoticHandler<T>>` always requires the concrete handler
        // struct to be `'static`; see `emit_exotic_handler_literal`'s `handler_generics`.
        assert!(rust.contains("impl < T : Tenant + 'static > TenantExoticHandler < T > for"), "{rust}");
        assert!(rust.contains("fn get_own_property_descriptor"), "{rust}");
    }

    #[test]
    fn a_captured_free_variable_is_shadow_cloned_from_self() {
        // The bug: `proto` (captured from the enclosing function) was emitted as a bare
        // identifier inside a trait method, which doesn't compile — a struct field is only
        // reachable via `self.proto`. Fixed by prepending a `let proto = self.proto.clone();`
        // shadow-clone, the same idiom `emit_closure` already uses for its own captures.
        let rust = emit();
        assert!(rust.contains("let proto = self . proto . clone () ;"), "{rust}");
    }

    #[test]
    fn get_own_property_descriptor_undefined_return_is_none_not_undefined_value() {
        // The bug: `emit_expr`'s `Lit::Undefined` mapping produces `tenant.undefined_value()`
        // (correct for a `T::Value`-returning trap like `get`), but `getOwnPropertyDescriptor`'s
        // real Rust return type is `Option<TenantPropertyDescriptor<T::Value>>` — a bare
        // `return undefined;` there must become `None`, not a `T::Value`.
        let rust = emit();
        assert!(rust.contains("fn get_own_property_descriptor"), "{rust}");
        let start = rust.find("fn get_own_property_descriptor").unwrap();
        let body = &rust[start..start + 400];
        assert!(body.contains("return Ok (None) ;"), "{body}");
    }

    #[test]
    fn get_prototype_of_wraps_a_real_value_in_some() {
        let rust = emit();
        let start = rust.find("fn get_prototype_of").unwrap();
        let body = &rust[start..start + 400];
        assert!(body.contains("return Ok (Some ((proto) . clone ())) ;"), "{body}");
    }

    #[test]
    fn get_wraps_a_bare_string_literal_via_tenant_string_value() {
        // The bug: a bare `return "smoke";` inside a `T::Value`-returning trap needs an explicit
        // `Tenant::string_value` constructor — `emit_expr` has no way to know a plain string
        // literal is being returned in a `T::Value`-typed position on its own.
        let rust = emit();
        let start = rust.find("fn get (").unwrap();
        let body = &rust[start..start + 400];
        assert!(body.contains("tenant . string_value"), "{body}");
    }

    #[test]
    fn key_compared_against_a_string_literal_converts_the_literal_to_a_property_key() {
        // The bug: `PropertyKey` has no `PartialEq<str>` impl, so `key == "tag"` (`key: &
        // PropertyKey`) doesn't compile without converting the literal first.
        let rust = emit();
        assert!(rust.contains("PropertyKey :: from (\"tag\")"), "{rust}");
    }
}
