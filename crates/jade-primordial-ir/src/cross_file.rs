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
}

pub const TABLE: &[CrossFileFactory] = &[CrossFileFactory {
    module: "./object.ts",
    name: "objectPrimordial",
    rust_fn_path: "crate::object::object_primordial",
    struct_name: "ObjectPrimordial",
    struct_path: "crate::object::ObjectPrimordial",
    module_path: "crate::object",
}];

pub fn lookup(module: &str, name: &str) -> Option<&'static CrossFileFactory> {
    TABLE.iter().find(|entry| entry.module == module && entry.name == name)
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
