# JIT host-method name mappings

Jade's Rust JIT emits calls against a tenant object. A host that property-mangles
that object must use the same method names in every JIT body it compiles.
`portal-solutions-jade-vm-jit` supports this without putting a lookup table or
wrapper call in emitted guest-operation paths.

## ABI inventory

`JadeTenantMethod` is owned by `jade-vm-jit`; its canonical spellings are:

- `make`, `get`, `set`, `define`, and `assign` for tenant object operations;
- `driveTenant` for driving a tenant operation under the ambient async/generator
  flags;
- `createGuestGen` for guest generator construction.

The shared `portal-jit-host-names` crate does not import this enum. It only
uses its `Display` implementation, so Jade remains the owner of the tenant
ABI.

## Default behavior

`Config` defaults to the zero-sized `CanonicalHostMethodNames` resolver:

```rust
use portal_solutions_jade_vm_jit::{compile, Config, VecRegistry};

let (body, registry) = compile(&bytecode, VecRegistry::new(), Config::default())?;
```

This keeps canonical direct accesses such as
`tenant.driveTenant(tenant.get(...), false, false)` and has no runtime naming
resolver in emitted JavaScript.

## Custom mappings

Use `Config::with_names` with a `HostMethodNames<JadeTenantMethod>` resolver.
`MappedHostMethodNames` is the supplied map-backed implementation:

```rust
use portal_jit_host_names::MappedHostMethodNames;
use portal_solutions_jade_vm_jit::{compile, Config, VecRegistry};

let names = MappedHostMethodNames::new([
    ("make".into(), "a".into()),
    ("get".into(), "b".into()),
    ("set".into(), "c".into()),
    ("define".into(), "d".into()),
    ("assign".into(), "e".into()),
    ("driveTenant".into(), "f".into()),
    ("createGuestGen".into(), "g".into()),
]);
let (body, registry) = compile(&bytecode, VecRegistry::new(), Config::with_names(names))?;
# Ok::<(), String>(())
```

Compilation validates the full inventory before emitting source. An incomplete
mapping fails rather than silently falling back to canonical names. Mapped
names may be arbitrary property keys: a value such as `"not-a-name"` is emitted
as `tenant["not-a-name"]`, never interpolated as raw JavaScript.

## Tier consistency

The resolver flows through Tier 0, the optional Tier 1 reloop backend, Tier 2
SWC compilation (including nested closures), and the Tier 2 WASM entry point.
All JIT tiers and the tenant host must therefore be given one complete,
consistent scheme for a compilation/run. Tenant-method body inlining is
separate: it can remove an eligible call entirely, but never changes the
required host ABI for calls that remain.