//! Hand-authored Rust mirror of `packages/jade-js/tenants/types.ts`'s `Tenant` interface and
//! `packages/jade-js/async-host.ts`'s `HostAsyncCapability` interface.
//!
//! See `docs/primordial-ir-plan.md` (mirrors the approved planning doc) for the full design.
//! This crate is **not generated** — `jade-primordial-rt` (generated from
//! `packages/jade-js/primordials/*.ts` by `jade-primordial-ir`) depends on it and calls into
//! these traits.
//!
//! Deliberate simplifications versus the TS `Tenant` interface: every operation here is a
//! plain, synchronous, fallible call (`Result<_, TenantError>`) rather than a generator
//! composed via `yield tenant.yieldTenant(...)`. TS primordials are generators solely to
//! support the *optional* `addAsync`/`addGen` ambient-upgrade path through `driveTenant`; a
//! Rust embedding's tenant operations are already synchronous (matching
//! `jade-vm-core::dispatch::Ops`'s existing `Result<Value, Error>` convention), so there is no
//! cooperative scheduler to model at this layer. The TS-only ABI/driver plumbing methods
//! (`markGuestFn`, `createGuestGen`, `unpackGuestGen`, `yieldHostTask`, `yieldTenant`,
//! `driveTenant`) have no Rust equivalent and are intentionally absent.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub mod intrinsics;

/// Opaque per-realm symbol identity. The embedder is responsible for allocating distinct ids;
/// this crate never inspects a symbol's description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u64);

/// Mirrors TS `PropertyKey` (`string | symbol` at the Tenant boundary — Jade's tenant surface
/// never accepts a bare `number` key; numeric-looking keys are strings, exactly as in real
/// JavaScript).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PropertyKey {
    String(String),
    Symbol(SymbolId),
}

impl From<&str> for PropertyKey {
    fn from(value: &str) -> Self {
        PropertyKey::String(value.to_string())
    }
}

impl From<String> for PropertyKey {
    fn from(value: String) -> Self {
        PropertyKey::String(value)
    }
}

impl std::fmt::Display for PropertyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyKey::String(s) => write!(f, "{s}"),
            PropertyKey::Symbol(id) => write!(f, "Symbol({})", id.0),
        }
    }
}

/// Mirrors the two error constructors actually thrown by the primordials
/// (`throw new TypeError(...)` / `throw new RangeError(...)`) — no other guest-visible error
/// constructor appears in `packages/jade-js/primordials/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantError {
    TypeError(String),
    RangeError(String),
}

impl std::fmt::Display for TenantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TenantError::TypeError(message) => write!(f, "TypeError: {message}"),
            TenantError::RangeError(message) => write!(f, "RangeError: {message}"),
        }
    }
}

impl std::error::Error for TenantError {}

impl TenantError {
    pub fn type_error(message: impl Into<String>) -> Self {
        TenantError::TypeError(message.into())
    }

    pub fn range_error(message: impl Into<String>) -> Self {
        TenantError::RangeError(message.into())
    }
}

/// Mirrors `TenantPropertyDescriptor` in `packages/jade-js/tenants/types.ts`. Every field is
/// independently optional, exactly as the TS control record is: a partial descriptor merges
/// with whatever the target's existing state already is at the `Tenant::define_property` call
/// site, never implicitly defaulting a field that wasn't supplied.
#[derive(Debug, Clone)]
pub struct TenantPropertyDescriptor<V> {
    pub value: Option<V>,
    pub writable: Option<bool>,
    pub get: Option<V>,
    pub set: Option<V>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

impl<V> Default for TenantPropertyDescriptor<V> {
    fn default() -> Self {
        TenantPropertyDescriptor {
            value: None,
            writable: None,
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        }
    }
}

/// A dynamic-shaped bag over a fixed, small set of named fields — the Rust counterpart of a
/// plain-object-in-practice JS value that is only ever probed with `key in value` /
/// `value[key]` over a known key set. `types.ts`'s `readGuestDescriptor`/`descriptorObject`
/// helpers loop `for (const key of ["value","writable",...] as const) { if (key in x) ... }`;
/// generated Rust for that exact pattern calls [`DynFields::has_field`] instead of modeling a
/// general dynamic `in` operator, which Rust structs don't have.
pub trait DynFields {
    fn has_field(&self, key: &str) -> bool;
}

impl<V: Clone> DynFields for TenantPropertyDescriptor<V> {
    fn has_field(&self, key: &str) -> bool {
        match key {
            "value" => self.value.is_some(),
            "writable" => self.writable.is_some(),
            "get" => self.get.is_some(),
            "set" => self.set.is_some(),
            "enumerable" => self.enumerable.is_some(),
            "configurable" => self.configurable.is_some(),
            _ => false,
        }
    }
}

/// Mirrors `TenantInvocation` in `packages/jade-js/tenants/types.ts`.
#[derive(Debug, Clone)]
pub enum TenantInvocation<V> {
    Apply { this_arg: V, args: Vec<V> },
    Construct { args: Vec<V>, new_target: V },
}

/// Mirrors `TenantExoticHandler` in `packages/jade-js/tenants/types.ts`. Every trap has a
/// default implementation that fails closed with a `TypeError`, matching the TS rule that a
/// **missing native exotic trap** always fails closed (never silently falls back to target
/// behavior — that fallback is only ever a *guest* `Proxy` handler concern, one layer above
/// this trait; see `proxy.ts`'s two-layer trap design).
pub trait TenantExoticHandler<V> {
    fn get(&mut self, _receiver: &V, _key: &PropertyKey) -> Result<V, TenantError> {
        Err(TenantError::type_error("exotic trap 'get' is not implemented"))
    }
    fn set(&mut self, _receiver: &V, _key: &PropertyKey, _value: V) -> Result<(), TenantError> {
        Err(TenantError::type_error("exotic trap 'set' is not implemented"))
    }
    fn has(&mut self, _receiver: &V, _key: &PropertyKey) -> Result<bool, TenantError> {
        Err(TenantError::type_error("exotic trap 'has' is not implemented"))
    }
    fn delete(&mut self, _receiver: &V, _key: &PropertyKey) -> Result<(), TenantError> {
        Err(TenantError::type_error("exotic trap 'delete' is not implemented"))
    }
    fn own_keys(&mut self, _receiver: &V) -> Result<Vec<PropertyKey>, TenantError> {
        Err(TenantError::type_error("exotic trap 'ownKeys' is not implemented"))
    }
    fn own_property_keys(&mut self, _receiver: &V) -> Result<Vec<PropertyKey>, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'ownPropertyKeys' is not implemented",
        ))
    }
    fn get_own_property_descriptor(
        &mut self,
        _receiver: &V,
        _key: &PropertyKey,
    ) -> Result<Option<TenantPropertyDescriptor<V>>, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'getOwnPropertyDescriptor' is not implemented",
        ))
    }
    fn define_property(
        &mut self,
        _receiver: &V,
        _key: &PropertyKey,
        _descriptor: TenantPropertyDescriptor<V>,
    ) -> Result<bool, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'defineProperty' is not implemented",
        ))
    }
    fn get_prototype_of(&mut self, _receiver: &V) -> Result<Option<V>, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'getPrototypeOf' is not implemented",
        ))
    }
    fn set_prototype_of(&mut self, _receiver: &V, _prototype: Option<V>) -> Result<bool, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'setPrototypeOf' is not implemented",
        ))
    }
    fn is_extensible(&mut self, _receiver: &V) -> Result<bool, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'isExtensible' is not implemented",
        ))
    }
    fn prevent_extensions(&mut self, _receiver: &V) -> Result<bool, TenantError> {
        Err(TenantError::type_error(
            "exotic trap 'preventExtensions' is not implemented",
        ))
    }
    fn define(&mut self, _receiver: &V, _descriptors: &V) -> Result<(), TenantError> {
        Err(TenantError::type_error("exotic trap 'define' is not implemented"))
    }
    fn assign(&mut self, _receiver: &V, _source: &V) -> Result<(), TenantError> {
        Err(TenantError::type_error("exotic trap 'assign' is not implemented"))
    }
}

/// Mirrors `TenantCallableExoticHandler` in `packages/jade-js/tenants/types.ts`.
pub trait TenantCallableExoticHandler<V>: TenantExoticHandler<V> {
    fn apply(&mut self, receiver: &V, this_arg: &V, args: &[V]) -> Result<V, TenantError>;
    fn construct(&mut self, receiver: &V, new_target: &V, args: &[V]) -> Result<V, TenantError>;
}

/// Mirrors the object-model surface of `Tenant` in `packages/jade-js/tenants/types.ts`. See the
/// module doc comment for what was deliberately dropped (the TS-only generator/ABI-driver
/// plumbing) and why.
pub trait Tenant {
    type Value: Clone;
    /// Stable per-object identity, used as the key for non-tenant-keyed caches (buffer/typed
    /// array/descriptor records). TS has no equivalent concept because a `WeakMap<object, V>`
    /// keys directly off object identity; Rust needs an explicit, hashable/comparable stand-in.
    type ObjectId: Eq + Hash + Clone;

    fn make(&mut self, proto: Option<Self::Value>) -> Result<Self::Value, TenantError>;
    fn get(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<Self::Value, TenantError>;
    fn set(&mut self, obj: &Self::Value, key: &PropertyKey, value: Self::Value) -> Result<(), TenantError>;
    fn has(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<bool, TenantError>;
    fn delete(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<(), TenantError>;
    /// Own enumerable keys.
    fn own_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError>;
    /// All own keys, including non-enumerable keys.
    fn own_property_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError>;
    fn get_own_property_descriptor(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
    ) -> Result<Option<TenantPropertyDescriptor<Self::Value>>, TenantError>;
    fn define_property(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
        descriptor: TenantPropertyDescriptor<Self::Value>,
    ) -> Result<bool, TenantError>;
    fn get_prototype_of(&mut self, obj: &Self::Value) -> Result<Option<Self::Value>, TenantError>;
    fn set_prototype_of(&mut self, obj: &Self::Value, prototype: Option<Self::Value>) -> Result<bool, TenantError>;
    fn is_extensible(&mut self, obj: &Self::Value) -> Result<bool, TenantError>;
    fn prevent_extensions(&mut self, obj: &Self::Value) -> Result<bool, TenantError>;
    /// Bulk-apply property descriptors from `descriptors` (itself a tenant-managed object
    /// mapping keys to descriptor objects) to `target`.
    fn define(&mut self, target: &Self::Value, descriptors: &Self::Value) -> Result<(), TenantError>;
    /// Copy own enumerable properties from `src` into `dst` (object spread).
    fn assign(&mut self, dst: &Self::Value, src: &Self::Value) -> Result<(), TenantError>;
    /// Create a fail-closed exotic object (see `TenantExoticHandler`'s doc comment).
    fn make_exotic(
        &mut self,
        proto: Option<Self::Value>,
        handler: Box<dyn TenantExoticHandler<Self::Value>>,
    ) -> Result<Self::Value, TenantError>;
    /// Create a callable exotic represented by a constructible native function.
    fn make_callable_exotic(
        &mut self,
        proto: Option<Self::Value>,
        handler: Box<dyn TenantCallableExoticHandler<Self::Value>>,
    ) -> Result<Self::Value, TenantError>;
    /// Route normal apply or construction through the tenant callable ABI.
    fn invoke(&mut self, callee: &Self::Value, invocation: TenantInvocation<Self::Value>) -> Result<Self::Value, TenantError>;
    /// Invoke a property-descriptor getter/setter ("trap").
    fn invoke_trap(&mut self, f: &Self::Value, receiver: &Self::Value, args: &[Self::Value]) -> Result<Self::Value, TenantError>;
    /// Stable identity for `obj`, used as a map key by non-tenant-keyed caches. Two calls with
    /// values that are the *same guest object* must return equal ids; two calls with distinct
    /// guest objects should return distinct ids (a colliding implementation is memory-safe but
    /// would misbehave, exactly as a real `WeakMap` misbehaves if handed an unstable key).
    fn object_id(&self, value: &Self::Value) -> Self::ObjectId;
}

/// Opaque handle for a host-owned asynchronous task. Mirrors `HostTask<T>` in
/// `packages/jade-js/async-host.ts` — deliberately not a thenable/raw `Future`, just an
/// identity the owning `HostAsyncCapability` recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostTaskId(pub u64);

/// Mirrors `HostAsyncCapability` in `packages/jade-js/async-host.ts`.
///
/// Simplification versus the TS interface, called out as an open question in the plan: TS's
/// `HostTask<T>` is generic per-observation; this trait instead settles every task with a
/// value of the tenant's own `Value` type, since every real observer in the primordials
/// (`promise.ts`'s reaction queue) ultimately produces a guest-visible value anyway. Revisit if
/// a concrete Rust embedding needs a genuinely typed task result before that value reaches the
/// guest boundary.
pub trait HostAsyncCapability {
    type Value: Clone;

    fn enqueue_microtask(&mut self, job: Box<dyn FnOnce(&mut Self)>);
    fn observe(
        &mut self,
        task: HostTaskId,
        on_fulfilled: Box<dyn FnOnce(&mut Self, Self::Value)>,
        on_rejected: Box<dyn FnOnce(&mut Self, Self::Value)>,
    );
    fn on_unhandled_rejection(&mut self, reason: Self::Value, promise: Self::Value);
    fn on_rejection_handled(&mut self, _promise: Self::Value) {}
}

/// Hashes `value` with the default `Hasher` — a convenience for `Tenant::ObjectId`
/// implementations backed by pointer/arena-index identity rather than a derived `Hash`.
pub fn hash_object_id<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
