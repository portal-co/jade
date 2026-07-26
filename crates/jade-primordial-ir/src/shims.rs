//! The shim registry: primordial source modules that are hand-ported to Rust rather than run
//! through `lower.rs`/`emit_rust.rs`, and the exact Rust path each of their exports resolves to.
//!
//! `types.ts` is the first and so far only entry — see `jade-primordial-rt::types_shim`'s module
//! doc comment for why (its `makeBuiltin` closes over `this` inside an exotic-handler object
//! literal, and `readGuestDescriptor`/`descriptorObject` do dynamic by-name field access on what
//! becomes a fixed Rust struct — both natural once hand-written, both requiring real
//! reflection/dynamic-dispatch machinery in the IR to derive automatically). Any other primordial
//! file's own source is still fully IR-lowered; only *calls into a shimmed module* are special:
//!
//! - **TypeScript emission** needs no special handling at all — `Item::ValueImport` is already
//!   re-emitted verbatim (still `import { defineData, ... } from "./types.ts";`), and a call to
//!   an imported identifier is already emitted as an ordinary `Expr::Call` (see `emit_ts.rs`).
//!   The regenerated file keeps calling the real, hand-written `types.ts` — nothing about
//!   `types.ts` itself needs to round-trip through the IR for this to work.
//! - **Rust emission** resolves the same call to this table's registered Rust path instead of
//!   attempting to inline or re-derive the shimmed function's body.

/// How one positional argument to a call should be passed — shared between the shim registry
/// here and `emit_rust.rs`'s `tenant_method` table, since both need the same three cases. A
/// bare-identifier argument (almost always the case for `tenant`/an already-typed parameter) is
/// passed through untouched regardless of this value; only a freshly constructed expression
/// actually gets wrapped, per variant:
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// Pass through untouched — used for a bare `tenant` argument (`&mut T`, never re-borrowed
    /// with a leading `&`, which would produce `&&mut T`) and for owned/by-value arguments.
    Owned,
    /// `&`-wrap a non-identifier argument (an object/descriptor/invocation-value parameter,
    /// matching the target's own `&T::Value`/`&TenantPropertyDescriptor<T::Value>` parameter
    /// type).
    Ref,
    /// A `&PropertyKey` parameter specifically. A raw TS string-literal key (`"name"`, extremely
    /// common: `tenant.defineProperty(target, "name", ...)`) needs `PropertyKey::from(...)`
    /// conversion, not just a borrow — a plain `Ref` would produce `&&str`, not `&PropertyKey`.
    /// An already-`PropertyKey`-typed identifier still passes through untouched.
    RefKey,
    /// `Some((expr).clone())` — for a provided-but-optional owned `T::Value` positional argument
    /// (`makeBuiltin`'s `proto` parameter, `Option<T::Value>`) where the TS call site passes a
    /// plain, already-owned `T::Value` local (never a literal), so the wrap always needs a clone
    /// (the source local usually stays alive and in use after the call).
    OptionRef,
    /// `Some(expr)` — for a provided-but-optional argument whose TS call-site value is always a
    /// freshly constructed closure/value (`makeBuiltin`'s `construct` parameter), never an
    /// existing owned variable, so no clone is needed or correct (closures generally aren't
    /// `Clone`).
    OptionOwned,
    /// For a *required* `Option<T::Value>` parameter (`Tenant::make`'s `proto`,
    /// `Tenant::set_prototype_of`'s `prototype`) whose TS declared type is `object | null` but
    /// whose call-site argument is an ordinary, unwrapped expression (`tenant.make(null)`,
    /// `tenant.make(ObjectPrototype)`) — TS callers never wrap it themselves the way an optional
    /// trailing parameter's caller might. Three cases, checked in order by `emit_call_arg`: a
    /// literal `null`/`undefined` argument is already `None` (see the module doc comment on why
    /// those literals mean the real guest value by default and `None` is the exception here);
    /// an `EXPR as object | null` cast argument already produces `Option<T::Value>` on its own
    /// (`Tenant::nullable`); anything else gets `Some((expr).clone())`, the same as `OptionRef`.
    OptionalValue,
}

pub struct ShimArg {
    pub kind: ArgKind,
    /// A Rust literal substituted when the TS call site omits this (trailing) argument — e.g.
    /// `assertObject(value)` without its optional `message`. `None` means the argument is
    /// required at the TS call site too.
    pub default_literal: Option<&'static str>,
}

const fn required(kind: ArgKind) -> ShimArg {
    ShimArg { kind, default_literal: None }
}

pub struct ShimSpec {
    /// The import source exactly as written in a primordial file's own `import` statement
    /// (e.g. `"./types.ts"`).
    pub module: &'static str,
    /// The imported/exported identifier (e.g. `"defineData"`).
    pub name: &'static str,
    /// Fully-qualified Rust path to call instead.
    pub rust_path: &'static str,
    /// Whether the Rust target needs `tenant` injected as a leading argument the TS call site
    /// doesn't itself pass — true for `assertObject`/`toIndex`, whose TS versions close over
    /// `tenant` lexically instead of taking it as a declared parameter; false for the rest of
    /// `types.ts`'s exports, which all declare `tenant` as their own first TS parameter already.
    pub inject_tenant: bool,
    /// Wrapping convention for each argument as it appears at the TS call site.
    pub args: &'static [ShimArg],
}

pub const TABLE: &[ShimSpec] = &[
    ShimSpec {
        module: "./types.ts",
        name: "defineData",
        rust_path: "crate::types_shim::define_data",
        inject_tenant: false,
        args: &[required(ArgKind::Owned), required(ArgKind::Ref), required(ArgKind::RefKey), required(ArgKind::Ref), required(ArgKind::Owned)],
    },
    ShimSpec {
        module: "./types.ts",
        name: "readGuestDescriptor",
        rust_path: "crate::types_shim::read_guest_descriptor",
        inject_tenant: false,
        args: &[required(ArgKind::Owned), required(ArgKind::Ref)],
    },
    ShimSpec {
        module: "./types.ts",
        name: "descriptorObject",
        rust_path: "crate::types_shim::descriptor_object",
        inject_tenant: false,
        args: &[required(ArgKind::Owned), required(ArgKind::Ref)],
    },
    ShimSpec {
        module: "./types.ts",
        name: "guestArrayLike",
        rust_path: "crate::types_shim::guest_array_like",
        inject_tenant: false,
        args: &[required(ArgKind::Owned), required(ArgKind::Ref)],
    },
    ShimSpec {
        module: "./types.ts",
        name: "assertObject",
        rust_path: "crate::types_shim::assert_object",
        inject_tenant: true,
        args: &[
            required(ArgKind::Ref),
            ShimArg { kind: ArgKind::Owned, default_literal: Some("\"expected an object\"") },
        ],
    },
    ShimSpec {
        module: "./types.ts",
        name: "toIndex",
        rust_path: "crate::types_shim::to_index",
        inject_tenant: true,
        args: &[required(ArgKind::Ref)],
    },
    ShimSpec {
        module: "./types.ts",
        name: "makeBuiltin",
        rust_path: "crate::types_shim::make_builtin",
        inject_tenant: false,
        args: &[
            required(ArgKind::Owned),
            required(ArgKind::Owned),
            required(ArgKind::Owned),
            ShimArg { kind: ArgKind::OptionOwned, default_literal: Some("None") },
            ShimArg { kind: ArgKind::OptionRef, default_literal: Some("None") },
        ],
    },
];

pub fn lookup(module: &str, name: &str) -> Option<&'static ShimSpec> {
    TABLE.iter().find(|entry| entry.module == module && entry.name == name)
}
