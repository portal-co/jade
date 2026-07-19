//! Tier-0 Jade JIT execution in a WSDOM-connected browser.
//!
//! The browser evaluates trusted source produced by `jade-vm-jit`; Jade bytecode
//! is never interpreted operation-by-operation over the WSDOM transport. The
//! caller supplies browser-local tenant, `nt`, state, and global handles. Every
//! supplied handle is checked to belong to this runtime's [`Browser`].

use core::{fmt, future::IntoFuture};

use serde::{Deserialize, Serialize};

use portal_jit_host_names::{CanonicalHostMethodNames, HostMethodNames};
use portal_solutions_jade_vm_jit::{Config, JadeTenantMethod, VecRegistry, compile_from_variant};
use px_wsdom::Browser;
use px_wsdom::callback::{self, Callback};
use px_wsdom_core::{
    JsCast, UseInJsCode,
    js_types::{JsBoolean, JsObject, JsValue},
};

/// Wraps a WSDOM serializable value so it can be inserted into a trusted,
/// compiler-produced `format_args!` expression. WSDOM intentionally keeps its
/// raw source writer private; this adapter is the same narrow pattern used by
/// `vane-wsdom`.
struct AsJs<'a>(&'a dyn UseInJsCode);

impl fmt::Display for AsJs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.serialize_to(f)
    }
}

/// Local setup errors. Browser JavaScript exceptions remain WSDOM remote
/// values: WSDOM exposes them when the caller retrieves or awaits the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsdomJadeError {
    Compile(String),
    TierUnavailable(ExecutionTier),
    CrossConnectionValue(&'static str),
    ServerTenantNeedsAsync,
    ServerTenantBootstrapUnavailable,
}

impl fmt::Display for WsdomJadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(err) => write!(f, "Jade JIT compilation failed: {err}"),
            Self::TierUnavailable(tier) => write!(
                f,
                "Jade execution tier {tier:?} is not enabled in jade-vm-wsdom"
            ),
            Self::CrossConnectionValue(name) => {
                write!(f, "{name} belongs to a different WSDOM Browser connection")
            }
            Self::ServerTenantNeedsAsync => write!(
                f,
                "a callback-driven server tenant requires the async Jade execution variant"
            ),
            Self::ServerTenantBootstrapUnavailable => write!(
                f,
                "the imported Jade server-tenant runtime has not been wired yet"
            ),
        }
    }
}

impl core::error::Error for WsdomJadeError {}

pub type ServerValueId = u64;

/// A client-visible reference to a value owned by a particular server tenant.
/// It is a descriptor only: browser code must use the imported Jade runtime's
/// private `WeakMap` shim, not manufacture this shape itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerValueDescriptor {
    pub capability: String,
    pub id: ServerValueId,
}

/// Values crossing a callback tenant request. Browser values stay as WSDOM
/// handles; server values use their separate `ServerValueId` namespace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TenantValueWire {
    Server(ServerValueDescriptor),
    Primitive(serde_json::Value),
}

/// A server value store is deliberately separate from WSDOM's browser `_w`
/// heap. It owns tenant values until explicit release or disconnect.
#[derive(Debug)]
pub struct ServerValueStore<V> {
    next_id: ServerValueId,
    values: std::collections::BTreeMap<ServerValueId, V>,
}

impl<V> Default for ServerValueStore<V> {
    fn default() -> Self {
        Self {
            next_id: 1,
            values: std::collections::BTreeMap::new(),
        }
    }
}

impl<V> ServerValueStore<V> {
    pub fn insert(&mut self, value: V) -> ServerValueId {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("server value IDs exhausted");
        assert!(self.values.insert(id, value).is_none());
        id
    }

    pub fn get(&self, id: ServerValueId) -> Option<&V> {
        self.values.get(&id)
    }

    pub fn get_mut(&mut self, id: ServerValueId) -> Option<&mut V> {
        self.values.get_mut(&id)
    }

    pub fn release(&mut self, id: ServerValueId) -> Option<V> {
        self.values.remove(&id)
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}

/// A decoded callback argument. Browser values remain WSDOM handles and must
/// never be JSON-copied into the server-value namespace.
#[derive(Debug)]
pub enum TenantArgument {
    Server(ServerValueDescriptor),
    Primitive(serde_json::Value),
    Browser(JsValue),
}

/// One Jade tenant operation delivered by the imported runtime's request
/// callback. The concrete callback dispatcher decodes primitive/server-shim
/// descriptors and preserves browser objects in [`TenantArgument::Browser`].
#[derive(Debug)]
pub struct TenantRequest {
    pub capability: String,
    pub request_id: u64,
    pub operation: String,
    pub args: Vec<TenantArgument>,
}

/// Typed boundary for the callback-driven server tenant. The concrete
/// callback/import installation follows the fork's generated binding surface;
/// this trait and `ServerValueStore` make its value ownership explicit now.
pub trait AsyncTenant {
    type Value;
    type Error;

    fn dispatch(
        &self,
        request: TenantRequest,
        values: &mut ServerValueStore<Self::Value>,
    ) -> impl core::future::Future<Output = Result<TenantValueWire, Self::Error>> + Send;
}

/// The WSDOM import name under which the generated client must expose Jade's
/// shared server-tenant runtime.
pub const WSDOM_TENANT_RUNTIME_IMPORT: &str = "@portal-solutions/jade-js/wsdom-tenant-runtime";

/// The two browser-to-server callback streams kept alive for an installed
/// server tenant. Each request carries its own resolve/reject function handles;
/// a dispatcher consumes `request` and calls one of those handles exactly once.
pub struct ServerTenantInstallation {
    runtime: WsdomRuntime,
    pub request: Callback<JsValue>,
    pub release: Callback<JsValue>,
}

impl ServerTenantInstallation {
    pub fn runtime(&self) -> &WsdomRuntime {
        &self.runtime
    }

    pub fn into_runtime(self) -> WsdomRuntime {
        self.runtime
    }
}

/// Import the shared Jade browser runtime and create a callback-driven server
/// tenant facade for this WSDOM connection.
///
/// The embedding WSDOM client must be generated with the Jade runtime module
/// under [`WSDOM_TENANT_RUNTIME_IMPORT`]. This function deliberately leaves
/// tenant dispatch to the caller: consume [`ServerTenantInstallation::request`]
/// and settle the resolve/reject callback handles included in each request.
pub fn install_server_tenant(
    browser: Browser,
    nt: JsValue,
    global_this: JsValue,
) -> Result<ServerTenantInstallation, WsdomJadeError> {
    validate_value(&browser, &nt, "nt")?;
    validate_value(&browser, &global_this, "global_this")?;

    let (request, request_callback) = callback::new_callback::<JsValue>(&browser);
    let (release, release_callback) = callback::new_callback::<JsValue>(&browser);
    let imported = browser.import(WSDOM_TENANT_RUNTIME_IMPORT);
    let imported: &JsObject = imported.unchecked_ref();
    let install = imported.js_get_field(&"installServerTenantRuntime");

    // The capability is generated locally and is only ever interpolated through
    // serde_json's string encoder. The runtime checks it on every request/shim.
    let capability = browser.value_from_raw_code(format_args!(
        "{}",
        serde_json::to_string(&format!(
            "jade-wsdom-{}",
            NEXT_CAPABILITY.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
        .expect("string serialization cannot fail")
    ));
    let install: &JsObject = install.unchecked_ref();
    let tenant = install.js_call_self(
        [
            &request_callback as &dyn UseInJsCode,
            &release_callback as &dyn UseInJsCode,
            &capability as &dyn UseInJsCode,
        ],
        false,
    );
    let runtime = WsdomRuntime::new(browser, tenant, nt, global_this)?.with_server_tenant();
    Ok(ServerTenantInstallation {
        runtime,
        request,
        release,
    })
}

static NEXT_CAPABILITY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Selects the Jade JIT source-generation tier used for a WSDOM invocation.
/// Tier 0 is the conservative default; Tier 1 and Tier 2 retain the same
/// wrapper, value ownership, and tenant-name contracts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionTier {
    #[default]
    Tier0,
    Tier1,
    Tier2,
}

/// Extra Tier-0 compilation options for one WSDOM invocation.
pub struct RunOptions<N = CanonicalHostMethodNames> {
    pub ip: usize,
    pub add_async: bool,
    pub add_gen: bool,
    pub names: N,
    pub tier: ExecutionTier,
}

impl Default for RunOptions<CanonicalHostMethodNames> {
    fn default() -> Self {
        Self {
            ip: 0,
            add_async: false,
            add_gen: false,
            names: CanonicalHostMethodNames,
            tier: ExecutionTier::Tier0,
        }
    }
}

impl<N> RunOptions<N> {
    pub fn with_names(names: N) -> Self {
        Self {
            ip: 0,
            add_async: false,
            add_gen: false,
            names,
            tier: ExecutionTier::Tier0,
        }
    }
}

/// The remote result of a Jade async invocation.
#[repr(transparent)]
pub struct RemotePromise(pub JsValue);

/// The remote result of a Jade synchronous generator invocation.
#[repr(transparent)]
pub struct RemoteGenerator(pub JsValue);

/// The remote result of a Jade async-generator invocation.
#[repr(transparent)]
pub struct RemoteAsyncGenerator(pub JsValue);

impl RemotePromise {
    pub fn into_inner(self) -> JsValue {
        self.0
    }
}
impl IntoFuture for RemotePromise {
    type Output = JsValue;
    type IntoFuture = <JsValue as IntoFuture>::IntoFuture;

    /// Await the browser promise using WSDOM's established promise bridge.
    fn into_future(self) -> Self::IntoFuture {
        self.0.into_future()
    }
}

pub struct RemoteIterResult {
    pub value: JsValue,
    pub done: bool,
}

impl RemoteGenerator {
    pub fn into_inner(self) -> JsValue {
        self.0
    }

    pub fn next_raw(&self, sent: Option<&dyn UseInJsCode>) -> JsValue {
        iterator_call(&self.0, "next", sent)
    }

    pub fn return_raw(&self, value: Option<&dyn UseInJsCode>) -> JsValue {
        iterator_call(&self.0, "return", value)
    }

    pub fn throw_raw(&self, value: &dyn UseInJsCode) -> JsValue {
        iterator_call(&self.0, "throw", Some(value))
    }

    pub async fn next(&self, sent: Option<&dyn UseInJsCode>) -> Result<RemoteIterResult, JsValue> {
        remote_iter_result(self.next_raw(sent)).await
    }
}
impl RemoteAsyncGenerator {
    pub fn into_inner(self) -> JsValue {
        self.0
    }

    pub fn next_raw(&self, sent: Option<&dyn UseInJsCode>) -> JsValue {
        iterator_call(&self.0, "next", sent)
    }

    pub fn return_raw(&self, value: Option<&dyn UseInJsCode>) -> JsValue {
        iterator_call(&self.0, "return", value)
    }

    pub fn throw_raw(&self, value: &dyn UseInJsCode) -> JsValue {
        iterator_call(&self.0, "throw", Some(value))
    }

    pub async fn next(&self, sent: Option<&dyn UseInJsCode>) -> Result<RemoteIterResult, JsValue> {
        let result = self.next_raw(sent).await;
        remote_iter_result(result).await
    }
}

/// A browser-local Jade execution context.
pub struct WsdomRuntime {
    browser: Browser,
    tenant: JsValue,
    nt: JsValue,
    global_this: JsValue,
    server_tenant: bool,
}

impl WsdomRuntime {
    /// Construct a context using a tenant that already resides in the browser.
    pub fn new(
        browser: Browser,
        tenant: JsValue,
        nt: JsValue,
        global_this: JsValue,
    ) -> Result<Self, WsdomJadeError> {
        validate_value(&browser, &tenant, "tenant")?;
        validate_value(&browser, &nt, "nt")?;
        validate_value(&browser, &global_this, "global_this")?;
        Ok(Self {
            browser,
            tenant,
            nt,
            global_this,
            server_tenant: false,
        })
    }

    /// Mark this context as callback-driven server-tenant execution.
    ///
    /// The facade itself must already have been installed by the browser's
    /// imported Jade runtime. The concrete import/callback adapter is added in
    /// the next server-tenant phase; keeping this flag here ensures the public
    /// execution path rejects impossible synchronous use today.
    pub fn with_server_tenant(mut self) -> Self {
        self.server_tenant = true;
        self
    }

    pub fn browser(&self) -> &Browser {
        &self.browser
    }

    /// Allocate an empty state object in the connected browser.
    pub fn default_state(&self) -> JsValue {
        self.browser
            .value_from_raw_code(format_args!("Object.create(null)"))
    }

    pub fn run_virtualized<N>(
        &self,
        code: &[u8],
        state: &JsValue,
        options: RunOptions<N>,
    ) -> Result<JsValue, WsdomJadeError>
    where
        N: HostMethodNames<JadeTenantMethod>,
    {
        if self.server_tenant {
            return Err(WsdomJadeError::ServerTenantNeedsAsync);
        }
        self.run(code, state, options, WrapperKind::Sync)
    }

    pub fn run_virtualized_a<N>(
        &self,
        code: &[u8],
        state: &JsValue,
        options: RunOptions<N>,
    ) -> Result<RemotePromise, WsdomJadeError>
    where
        N: HostMethodNames<JadeTenantMethod>,
    {
        Ok(RemotePromise(self.run(
            code,
            state,
            options,
            WrapperKind::Async,
        )?))
    }

    pub fn run_virtualized_g<N>(
        &self,
        code: &[u8],
        state: &JsValue,
        options: RunOptions<N>,
    ) -> Result<RemoteGenerator, WsdomJadeError>
    where
        N: HostMethodNames<JadeTenantMethod>,
    {
        if self.server_tenant {
            return Err(WsdomJadeError::ServerTenantNeedsAsync);
        }
        Ok(RemoteGenerator(self.run(
            code,
            state,
            options,
            WrapperKind::Generator,
        )?))
    }

    pub fn run_virtualized_ag<N>(
        &self,
        code: &[u8],
        state: &JsValue,
        options: RunOptions<N>,
    ) -> Result<RemoteAsyncGenerator, WsdomJadeError>
    where
        N: HostMethodNames<JadeTenantMethod>,
    {
        Ok(RemoteAsyncGenerator(self.run(
            code,
            state,
            options,
            WrapperKind::AsyncGenerator,
        )?))
    }

    fn run<N>(
        &self,
        code: &[u8],
        state: &JsValue,
        options: RunOptions<N>,
        wrapper: WrapperKind,
    ) -> Result<JsValue, WsdomJadeError>
    where
        N: HostMethodNames<JadeTenantMethod>,
    {
        validate_value(&self.browser, state, "state")?;
        let mut config = Config::with_names(options.names);
        config.add_async = options.add_async
            || matches!(wrapper, WrapperKind::Async | WrapperKind::AsyncGenerator);
        config.add_gen = options.add_gen
            || matches!(
                wrapper,
                WrapperKind::Generator | WrapperKind::AsyncGenerator
            );
        let (body, registry) = compile_for_tier(
            options.tier,
            code,
            options.ip,
            config,
            matches!(
                wrapper,
                WrapperKind::Generator | WrapperKind::AsyncGenerator
            ),
            matches!(wrapper, WrapperKind::Async | WrapperKind::AsyncGenerator),
        )?;
        let prelude = registry.prelude();
        Ok(self.browser.value_from_raw_code(format_args!(
            "({}(tenant,nt,state,globalThis){{{prelude}\n{body}}})({},{},{},{})",
            wrapper.keyword(),
            AsJs(&self.tenant),
            AsJs(&self.nt),
            AsJs(state),
            AsJs(&self.global_this),
        )))
    }
}

fn compile_for_tier<N>(
    tier: ExecutionTier,
    code: &[u8],
    start_ip: usize,
    config: Config<N>,
    is_gen: bool,
    is_async: bool,
) -> Result<(String, VecRegistry), WsdomJadeError>
where
    N: HostMethodNames<JadeTenantMethod>,
{
    portal_solutions_jade_vm_jit::validate_host_method_names(&config.names)
        .map_err(WsdomJadeError::Compile)?;
    match tier {
        ExecutionTier::Tier0 => {
            compile_from_variant(code, start_ip, VecRegistry::new(), config, is_gen, is_async)
                .map_err(WsdomJadeError::Compile)
        }
        ExecutionTier::Tier1 => portal_solutions_jade_vm_jit::compile_reloop_from_variant(
            code,
            start_ip,
            VecRegistry::new(),
            config,
            is_gen,
            is_async,
        )
        .map_err(WsdomJadeError::Compile),
        ExecutionTier::Tier2 => compile_tier2(code, start_ip, config, is_gen, is_async),
    }
}

#[cfg(feature = "tier2")]
fn compile_tier2<N>(
    code: &[u8],
    start_ip: usize,
    config: Config<N>,
    is_gen: bool,
    is_async: bool,
) -> Result<(String, VecRegistry), WsdomJadeError>
where
    N: HostMethodNames<JadeTenantMethod>,
{
    portal_solutions_jade_vm_jit_swc::compile_from_variant(code, start_ip, config, is_gen, is_async)
        .map_err(WsdomJadeError::Compile)
}

#[cfg(not(feature = "tier2"))]
fn compile_tier2<N>(
    _code: &[u8],
    _start_ip: usize,
    _config: Config<N>,
    _is_gen: bool,
    _is_async: bool,
) -> Result<(String, VecRegistry), WsdomJadeError>
where
    N: HostMethodNames<JadeTenantMethod>,
{
    Err(WsdomJadeError::TierUnavailable(ExecutionTier::Tier2))
}

fn validate_value(
    browser: &Browser,
    value: &JsValue,
    name: &'static str,
) -> Result<(), WsdomJadeError> {
    if browser.same_connection(value.browser()) {
        Ok(())
    } else {
        Err(WsdomJadeError::CrossConnectionValue(name))
    }
}

fn iterator_call(iterator: &JsValue, method: &str, value: Option<&dyn UseInJsCode>) -> JsValue {
    let browser = iterator.browser();
    match value {
        Some(value) => browser.value_from_raw_code(format_args!(
            "({})[{method}]({})",
            AsJs(iterator),
            AsJs(value),
        )),
        None => browser.value_from_raw_code(format_args!("({})[{method}]()", AsJs(iterator))),
    }
}

async fn remote_iter_result(result: JsValue) -> Result<RemoteIterResult, JsValue> {
    let object: &JsObject = result.unchecked_ref();
    let done: JsBoolean = JsCast::unchecked_from_js(object.js_get_field(&"done"));
    let value = object.js_get_field(&"value");
    match done.retrieve().await {
        Ok(done) => Ok(RemoteIterResult { value, done }),
        Err(error) => Err(error),
    }
}

#[derive(Copy, Clone)]
enum WrapperKind {
    Sync,
    Async,
    Generator,
    AsyncGenerator,
}

impl WrapperKind {
    fn keyword(self) -> &'static str {
        match self {
            Self::Sync => "function",
            Self::Async => "async function",
            Self::Generator => "function*",
            Self::AsyncGenerator => "async function*",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm::{Operand, Operation};

    fn sample_code() -> Vec<u8> {
        let op = Operation::Ret(Operand::Literal(7));
        op.emit().collect()
    }

    #[test]
    fn wrapper_kinds_match_jade_variants() {
        assert_eq!(WrapperKind::Sync.keyword(), "function");
        assert_eq!(WrapperKind::Async.keyword(), "async function");
        assert_eq!(WrapperKind::Generator.keyword(), "function*");
        assert_eq!(WrapperKind::AsyncGenerator.keyword(), "async function*");
    }

    #[test]
    fn cross_connection_inputs_are_rejected_locally() {
        let first = Browser::new();
        let second = Browser::new();
        let tenant = first.value_from_raw_code(format_args!("({{}})"));
        let nt = first.value_from_raw_code(format_args!("undefined"));
        let global = first.value_from_raw_code(format_args!("globalThis"));
        let runtime = WsdomRuntime::new(first.clone(), tenant, nt, global).unwrap();
        let foreign_state = second.value_from_raw_code(format_args!("({{}})"));
        assert_eq!(
            runtime
                .run_virtualized(&sample_code(), &foreign_state, RunOptions::default())
                .unwrap_err(),
            WsdomJadeError::CrossConnectionValue("state")
        );
    }

    #[test]
    fn server_values_are_separate_and_released() {
        let mut values = ServerValueStore::default();
        let first = values.insert("first");
        let second = values.insert("second");
        assert_ne!(first, second);
        assert_eq!(values.get(first), Some(&"first"));
        assert_eq!(values.release(first), Some("first"));
        assert_eq!(values.get(first), None);
        assert_eq!(values.get(second), Some(&"second"));
    }

    #[test]
    fn server_tenant_rejects_sync_shapes() {
        let browser = Browser::new();
        let tenant = browser.value_from_raw_code(format_args!("({{}})"));
        let nt = browser.value_from_raw_code(format_args!("undefined"));
        let global = browser.value_from_raw_code(format_args!("globalThis"));
        let state = browser.value_from_raw_code(format_args!("({{}})"));
        let runtime = WsdomRuntime::new(browser, tenant, nt, global)
            .unwrap()
            .with_server_tenant();
        assert_eq!(
            runtime
                .run_virtualized(&sample_code(), &state, RunOptions::default())
                .unwrap_err(),
            WsdomJadeError::ServerTenantNeedsAsync
        );
    }
}
