//! Registry of *other generated files'* per-tenant primordial factories — `objectPrimordial`
//! (from `object.ts`) is the first entry, called by `function.ts`/`reflect.ts`/`proxy.ts` for
//! `ObjectPrototype`. This is the generated-code counterpart to `shims.rs`'s hand-written-module
//! registry: same shape of problem (a call resolves to a Rust path instead of being IR-lowered
//! itself), but the target is *generated* Rust in a sibling file, and the TS call site never
//! passes the extra per-tenant-cache argument the Rust signature needs (see
//! `emit_rust.rs`'s `PerTenantCache` handling for the *local* file's own cache — this is the
//! same idea, just for a callee that lives elsewhere).
//!
//! `gen-primordials` currently lowers one file at a time (`--file <path>`), so this table is
//! hand-maintained rather than discovered by scanning sibling files — acceptable while the
//! registry is this small; a real multi-file sweep (the plan's Phase 8) would derive it instead.

pub struct CrossFileFactory {
    /// The import source exactly as written (`"./object.ts"`).
    pub module: &'static str,
    /// The imported/exported identifier (`"objectPrimordial"`).
    pub name: &'static str,
    /// Fully-qualified Rust path to the generated factory function.
    pub rust_fn_path: &'static str,
    /// The generated struct name (`"ObjectPrimordial"`) — used both to know which `use` to add
    /// for destructuring (`emit_rust.rs`'s `Stmt::Let` handling) and to derive the cache type's
    /// name (`"{struct_name}Cache"`, the same convention `emit_item`'s `PerTenantCache` handling
    /// uses for the *local* per-tenant cache struct).
    pub struct_name: &'static str,
    /// Fully-qualified path to the struct, for the prelude `use` (`"crate::object::
    /// ObjectPrimordial"`).
    pub struct_path: &'static str,
    /// Fully-qualified path to the module the cache type lives in (`"crate::object"`) — the
    /// cache type itself is referenced fully-qualified at its one use site (the extra parameter
    /// this factory's caller gains), so it doesn't need its own prelude `use`.
    pub module_path: &'static str,
    /// Fully-qualified path to the cache type itself (`"crate::object::ObjectPrimordialCache"`).
    /// *Not* always `struct_path` + `"Cache"`: `Item::PerTenantCache`'s own naming convention
    /// (`{value_ty}Cache`, see `emit_rust.rs`'s `emit_item`) names the cache after the *interface*
    /// a factory returns, which is the same as `struct_name` when there's no backing class
    /// (`objectPrimordial` -> `ObjectPrimordial`/`ObjectPrimordialCache`) but different when there
    /// is one (`bufferPrimordial` returns `BufferPrimordial`, backed by the *class*
    /// `BufferPrimordialImpl` — `struct_name`/`struct_path` name the class for the self-import
    /// and prelude-`use` machinery, but the cache type stays `BufferPrimordialCache`, never
    /// `BufferPrimordialImplCache`).
    pub cache_type_path: &'static str,
    /// Whether the cache type's own generic parameter list includes `H: BufferHooks` alongside
    /// `T: Tenant` (`BufferPrimordialCache<T, H>`) — `false` for `ObjectPrimordialCache<T>`.
    pub cache_needs_buffer_hooks: bool,
    /// Positional parameters this factory takes beyond the leading bare `tenant` — each a
    /// (TS-side param name, param type name) pair, in call order, resolved through the ordinary
    /// `arg_kind_for_type`/`emit_call_arg` machinery the same way a class method's own parameters
    /// are. Empty for `objectPrimordial` (`tenant` only); `bufferPrimordial(tenant, hooks)` has
    /// one entry (`hooks`, `"BufferHooks"`).
    pub extra_params: &'static [(&'static str, &'static str)],
    /// Other entries in this same table (by their own `struct_name`) whose cache parameter this
    /// factory's *generated Rust function* also requires, because it calls that other factory
    /// internally — e.g. `buffer_primordial` calls `objectPrimordial` for `ObjectPrototype`, so a
    /// caller of `bufferPrimordial` must also supply `object_primordial_cache`.
    /// `objectPrimordial` has none (no further cross-file dependencies of its own).
    pub transitive_caches: &'static [&'static str],
}

pub const TABLE: &[CrossFileFactory] = &[
    CrossFileFactory {
        module: "./object.ts",
        name: "objectPrimordial",
        rust_fn_path: "crate::object::object_primordial",
        struct_name: "ObjectPrimordial",
        struct_path: "crate::object::ObjectPrimordial",
        module_path: "crate::object",
        cache_type_path: "crate::object::ObjectPrimordialCache",
        cache_needs_buffer_hooks: false,
        extra_params: &[],
        transitive_caches: &[],
    },
    CrossFileFactory {
        module: "./function.ts",
        name: "functionPrimordial",
        rust_fn_path: "crate::function::function_primordial",
        struct_name: "FunctionPrimordial",
        struct_path: "crate::function::FunctionPrimordial",
        module_path: "crate::function",
        cache_type_path: "crate::function::FunctionPrimordialCache",
        cache_needs_buffer_hooks: false,
        extra_params: &[],
        transitive_caches: &["ObjectPrimordial"],
    },
    CrossFileFactory {
        module: "./array-buffer.ts",
        name: "bufferPrimordial",
        rust_fn_path: "crate::array_buffer::buffer_primordial",
        struct_name: "BufferPrimordialImpl",
        struct_path: "crate::array_buffer::BufferPrimordialImpl",
        module_path: "crate::array_buffer",
        cache_type_path: "crate::array_buffer::BufferPrimordialCache",
        cache_needs_buffer_hooks: true,
        extra_params: &[("hooks", "BufferHooks")],
        transitive_caches: &["ObjectPrimordial"],
    },
];

pub fn lookup(module: &str, name: &str) -> Option<&'static CrossFileFactory> {
    TABLE.iter().find(|entry| entry.module == module && entry.name == name)
}

/// A cross-file *class* referenced only as a field's interface type (e.g. `TypedArrayPrimordialImpl`'s
/// `#buffers: BufferPrimordial`, backed by `array-buffer.ts`'s `BufferPrimordialImpl`) — the
/// class-analogue of `CrossFileFactory`, needed because `gen-primordials` lowers/emits one file
/// at a time, so `CLASS_DEFS` (populated fresh per invocation) never sees another file's classes.
pub struct CrossFileClassMethod {
    /// The method name as called from TS (`"record"`, `"shell"`).
    pub ts_name: &'static str,
    /// The Rust inherent method name (currently always identical after `to_snake_case`, but kept
    /// distinct in case that ever isn't true).
    pub rust_name: &'static str,
    /// Parameter type names in declared order, exactly as they'd appear in `TypeRef::Named` —
    /// resolved through the same `arg_kind_for_type` table any other typed parameter list is.
    pub param_types: &'static [&'static str],
    /// Whether this method's Rust return type is `Option<_>` (`record`'s `BufferRecord | undefined`)
    /// rather than a plain/`Result`-wrapped value (`shell`'s `TenantGenerator<object>`) — consulted
    /// by `emit_rust.rs`'s `returns_option` so a local bound from a call to this method (`const
    /// bufferRecord = that.#buffers.record(...);`) gets truthy/`!`/narrowing checks translated
    /// correctly, the same as any other `Option`-typed local.
    pub returns_option: bool,
}

pub struct CrossFileClass {
    /// The import source exactly as written (`"./array-buffer.ts"`).
    pub module: &'static str,
    /// The interface name a field/param type names (`"BufferPrimordial"`) — what callers of
    /// `lookup_class` search by, since that's the only name visible outside the defining file
    /// (the class itself, `BufferPrimordialImpl`, is erased from the interface it implements —
    /// see `lower.rs`'s method-shaped-interface erasure).
    pub interface_name: &'static str,
    /// Fully-qualified path to the class's own generated wrapper type
    /// (`"crate::array_buffer::BufferPrimordialImpl"`).
    pub rust_path: &'static str,
    /// Whether the class's own generic parameter list includes `T: Tenant`/`H: BufferHooks` —
    /// used to build the right `<...>` argument list at each reference (mirrors `class_generics`
    /// for a same-module class).
    pub needs_tenant: bool,
    pub needs_buffer_hooks: bool,
    pub methods: &'static [CrossFileClassMethod],
}

pub const CLASS_TABLE: &[CrossFileClass] = &[CrossFileClass {
    module: "./array-buffer.ts",
    interface_name: "BufferPrimordial",
    rust_path: "crate::array_buffer::BufferPrimordialImpl",
    needs_tenant: true,
    needs_buffer_hooks: true,
    methods: &[
        CrossFileClassMethod {
            ts_name: "record",
            rust_name: "record",
            param_types: &["Tenant", "unknown"],
            returns_option: true,
        },
        CrossFileClassMethod {
            ts_name: "shell",
            rust_name: "shell",
            param_types: &["Tenant", "BufferKind", "BufferHandle", "object"],
            returns_option: false,
        },
    ],
}];

pub fn lookup_class(interface_name: &str) -> Option<&'static CrossFileClass> {
    CLASS_TABLE.iter().find(|entry| entry.interface_name == interface_name)
}

/// The extra parameter name a caller gains when it uses this factory — `object_primordial_cache`
/// for `ObjectPrimordial`, derived mechanically from the struct name rather than hand-picked per
/// entry, so it can never drift from `emit_item`'s own `{struct_name}Cache` naming convention.
pub fn cache_param_name(factory: &CrossFileFactory) -> String {
    format!("{}_cache", to_snake_case(factory.struct_name))
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
