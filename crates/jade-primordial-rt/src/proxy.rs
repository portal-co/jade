/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/proxy.ts. */
#[allow(unused_imports)]
use crate::array_buffer::BufferPrimordialImpl;
#[allow(unused_imports)]
use crate::function::FunctionPrimordial;
#[allow(unused_imports)]
use crate::object::ObjectPrimordial;
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    BufferHooks, BufferKind, DynFields, PropertyKey, Tenant, TenantError, TenantExoticHandler,
    TenantInvocation, TenantPropertyDescriptor, ValueTag,
};
pub struct ProxyPrimordial<T: Tenant> {
    pub proxy: T::Value,
}
impl<T: Tenant> Clone for ProxyPrimordial<T> {
    fn clone(&self) -> Self {
        Self {
            proxy: self.proxy.clone(),
        }
    }
}
pub struct ProxyStateImplInner<T: Tenant + 'static> {
    target: T::Value,
    handler: T::Value,
    revoked: bool,
}
pub struct ProxyStateImpl<T: Tenant + 'static> {
    inner: ::std::rc::Rc<::std::cell::RefCell<ProxyStateImplInner<T>>>,
}
impl<T: Tenant + 'static> Clone for ProxyStateImpl<T> {
    fn clone(&self) -> Self {
        Self {
            inner: ::std::rc::Rc::clone(&self.inner),
        }
    }
}
impl<T: Tenant + 'static> ProxyStateImpl<T> {
    pub fn new(target: &T::Value, handler: &T::Value) -> Self {
        Self {
            inner: ::std::rc::Rc::new(::std::cell::RefCell::new(ProxyStateImplInner {
                target: (target).clone(),
                handler: (handler).clone(),
                revoked: false,
            })),
        }
    }
    pub fn require_live(&self) -> Result<(), TenantError> {
        if self.inner.borrow().revoked {
            return Err(TenantError::TypeError(
                ("Cannot perform operation on a revoked Proxy").to_string(),
            ));
        }
        Ok(())
    }
    pub fn target(&self) -> Result<T::Value, TenantError> {
        (self.require_live())?;
        return Ok((self.inner.borrow().target).clone());
    }
    pub fn handler(&self) -> Result<T::Value, TenantError> {
        (self.require_live())?;
        return Ok((self.inner.borrow().handler).clone());
    }
    pub fn revoke(&self) -> () {
        self.inner.borrow_mut().revoked = true;
    }
}
pub struct ProxyPrimordialCache<T: Tenant + 'static> {
    entry: Option<ProxyPrimordial<T>>,
}
impl<T: Tenant + 'static> Default for ProxyPrimordialCache<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub fn trap<T: Tenant + 'static>(
    tenant: &mut T,
    state: ProxyStateImpl<T>,
    name: &str,
    args: &[T::Value],
) -> Result<Option<T::Value>, TenantError> {
    let __undefined = tenant.undefined_value();
    let mut guest_handler = (state.handler())?;
    let mut value = (tenant.get(&guest_handler, &PropertyKey::from(name)))?;
    if (tenant.typeof_tag(&value) == ValueTag::Undefined) {
        return Ok(None);
    }
    if (tenant.typeof_tag(&value) != ValueTag::Function) {
        return Err(TenantError::TypeError(
            (format!("Proxy trap {} is not callable", name)).to_string(),
        ));
    }
    return Ok(Some(
        (tenant.invoke(
            &value,
            TenantInvocation::Apply {
                this_arg: (guest_handler).clone(),
                args: (args).to_vec(),
            },
        ))?,
    ));
}
pub fn proxy_exotic<T: Tenant + 'static>(
    tenant: &mut T,
    target: &T::Value,
    handler: &T::Value,
    existing_state: Option<ProxyStateImpl<T>>,
) -> Result<T::Value, TenantError> {
    let __undefined = tenant.undefined_value();
    let mut state = (existing_state).unwrap_or(ProxyStateImpl::new(target, handler));
    return Ok(({
        struct ExoticHandler0<T: Tenant + 'static> {
            state: ProxyStateImpl<T>,
        }
        impl<T: Tenant + 'static> TenantExoticHandler<T> for ExoticHandler0<T> {
            fn get(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
                key: &PropertyKey,
            ) -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[
                        (state.target())?,
                        (tenant.property_key_value(key))?,
                        (receiver).clone(),
                    ];
                    trap(tenant, (state).clone(), "get", __hoisted_arg3)
                })?;
                return Ok((if result.is_some() {
                    (result.clone().unwrap())
                } else {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.get(__hoisted_arg0, key)
                    })?
                }));
            }
            fn set(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
                key: &PropertyKey,
                value: T::Value,
            ) -> Result<(), TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[
                        (state.target())?,
                        (tenant.property_key_value(key))?,
                        (value).clone(),
                        (receiver).clone(),
                    ];
                    trap(tenant, (state).clone(), "set", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.set(__hoisted_arg0, key, value)
                    })?;
                } else {
                    if (!tenant.to_boolean(&(result.clone().unwrap()))) {
                        return Err(TenantError::TypeError(
                            ("Proxy set trap returned false").to_string(),
                        ));
                    }
                }
                Ok(())
            }
            fn has(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                key: &PropertyKey,
            ) -> Result<bool, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?, (tenant.property_key_value(key))?];
                    trap(tenant, (state).clone(), "has", __hoisted_arg3)
                })?;
                return Ok((if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.has(__hoisted_arg0, key)
                    })?
                } else {
                    (!(!tenant.to_boolean(&(result.clone().unwrap()))))
                }));
            }
            fn delete(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                key: &PropertyKey,
            ) -> Result<(), TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?, (tenant.property_key_value(key))?];
                    trap(tenant, (state).clone(), "deleteProperty", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.delete(__hoisted_arg0, key)
                    })?;
                } else {
                    if (!tenant.to_boolean(&(result.clone().unwrap()))) {
                        return Err(TenantError::TypeError(
                            ("Proxy deleteProperty trap returned false").to_string(),
                        ));
                    }
                }
                Ok(())
            }
            fn own_keys(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
            ) -> Result<Vec<PropertyKey>, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?];
                    trap(tenant, (state).clone(), "ownKeys", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    return Ok(({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.own_keys(__hoisted_arg0)
                    })?);
                }
                (crate::types_shim::assert_object(
                    tenant,
                    &(result.clone().unwrap()),
                    "Proxy ownKeys trap must return an object",
                ))?;
                let mut keys =
                    (crate::types_shim::guest_array_like(tenant, &(result.clone().unwrap())))?;
                return Ok((keys)
                    .iter()
                    .map(|__k| tenant.to_property_key(__k))
                    .collect::<Vec<_>>());
            }
            fn own_property_keys(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
            ) -> Result<Vec<PropertyKey>, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?];
                    trap(tenant, (state).clone(), "ownKeys", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    return Ok(({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.own_property_keys(__hoisted_arg0)
                    })?);
                }
                (crate::types_shim::assert_object(
                    tenant,
                    &(result.clone().unwrap()),
                    "Proxy ownKeys trap must return an object",
                ))?;
                let mut keys =
                    (crate::types_shim::guest_array_like(tenant, &(result.clone().unwrap())))?;
                return Ok((keys)
                    .iter()
                    .map(|__k| tenant.to_property_key(__k))
                    .collect::<Vec<_>>());
            }
            fn get_own_property_descriptor(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                key: &PropertyKey,
            ) -> Result<Option<TenantPropertyDescriptor<T::Value>>, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?, (tenant.property_key_value(key))?];
                    trap(
                        tenant,
                        (state).clone(),
                        "getOwnPropertyDescriptor",
                        __hoisted_arg3,
                    )
                })?;
                if (!result.is_some()) {
                    return Ok(({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.get_own_property_descriptor(__hoisted_arg0, key)
                    })?);
                }
                if (tenant.typeof_tag(&(result.clone().unwrap())) == ValueTag::Undefined) {
                    return Ok(None);
                }
                (crate::types_shim::assert_object(
                    tenant,
                    &(result.clone().unwrap()),
                    "Proxy descriptor trap must return an object",
                ))?;
                return Ok(Some(
                    ((crate::types_shim::read_guest_descriptor(
                        tenant,
                        &(result.clone().unwrap()),
                    ))?)
                    .clone(),
                ));
            }
            fn define_property(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                key: &PropertyKey,
                descriptor: TenantPropertyDescriptor<T::Value>,
            ) -> Result<bool, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut descriptor_arg =
                    (crate::types_shim::descriptor_object(tenant, &descriptor))?;
                let mut result = ({
                    let __hoisted_arg3 = &[
                        (state.target())?,
                        (tenant.property_key_value(key))?,
                        (descriptor_arg).clone(),
                    ];
                    trap(tenant, (state).clone(), "defineProperty", __hoisted_arg3)
                })?;
                return Ok((if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.define_property(__hoisted_arg0, key, descriptor)
                    })?
                } else {
                    (!(!tenant.to_boolean(&(result.clone().unwrap()))))
                }));
            }
            fn get_prototype_of(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
            ) -> Result<Option<T::Value>, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?];
                    trap(tenant, (state).clone(), "getPrototypeOf", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    return Ok(({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.get_prototype_of(__hoisted_arg0)
                    })?);
                }
                if (tenant.typeof_tag(&(result.clone().unwrap())) != ValueTag::Null) {
                    (crate::types_shim::assert_object(
                        tenant,
                        &(result.clone().unwrap()),
                        "Proxy getPrototypeOf trap must return object or null",
                    ))?;
                }
                return Ok(tenant.nullable(&(result.clone().unwrap())));
            }
            fn set_prototype_of(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                prototype: Option<T::Value>,
            ) -> Result<bool, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[
                        (state.target())?,
                        (prototype).clone().unwrap_or_else(|| tenant.null_value()),
                    ];
                    trap(tenant, (state).clone(), "setPrototypeOf", __hoisted_arg3)
                })?;
                return Ok((if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.set_prototype_of(__hoisted_arg0, prototype)
                    })?
                } else {
                    (!(!tenant.to_boolean(&(result.clone().unwrap()))))
                }));
            }
            fn is_extensible(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
            ) -> Result<bool, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?];
                    trap(tenant, (state).clone(), "isExtensible", __hoisted_arg3)
                })?;
                return Ok((if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.is_extensible(__hoisted_arg0)
                    })?
                } else {
                    (!(!tenant.to_boolean(&(result.clone().unwrap()))))
                }));
            }
            fn prevent_extensions(
                &mut self,
                tenant: &mut T,
                receiver: &T::Value,
            ) -> Result<bool, TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?];
                    trap(tenant, (state).clone(), "preventExtensions", __hoisted_arg3)
                })?;
                return Ok((if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.prevent_extensions(__hoisted_arg0)
                    })?
                } else {
                    (!(!tenant.to_boolean(&(result.clone().unwrap()))))
                }));
            }
            fn define(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                descriptors: &T::Value,
            ) -> Result<(), TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?, (descriptors).clone()];
                    trap(tenant, (state).clone(), "defineProperties", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.define(__hoisted_arg0, descriptors)
                    })?;
                }
                Ok(())
            }
            fn assign(
                &mut self,
                tenant: &mut T,
                _receiver: &T::Value,
                source: &T::Value,
            ) -> Result<(), TenantError> {
                let __undefined = tenant.undefined_value();
                let state = self.state.clone();
                let mut result = ({
                    let __hoisted_arg3 = &[(state.target())?, (source).clone()];
                    trap(tenant, (state).clone(), "assign", __hoisted_arg3)
                })?;
                if (!result.is_some()) {
                    ({
                        let __hoisted_arg0 = &(state.target())?;
                        tenant.assign(__hoisted_arg0, source)
                    })?;
                }
                Ok(())
            }
        }
        tenant.make_exotic(
            None,
            Box::new(ExoticHandler0 {
                state: (state).clone(),
            }),
        )
    })?);
}
pub fn proxy_primordial<T: Tenant + 'static>(
    tenant: &mut T,
    cache: &mut ProxyPrimordialCache<T>,
    object_primordial_cache: &mut crate::object::ObjectPrimordialCache<T>,
    function_primordial_cache: &mut crate::function::FunctionPrimordialCache<T>,
) -> Result<ProxyPrimordial<T>, TenantError> {
    let __undefined = tenant.undefined_value();
    if let Some(existing) = cache.entry.clone() {
        return Ok(existing);
    }
    let FunctionPrimordial {
        function_prototype: function_prototype,
        ..
    } = (crate::function::function_primordial(
        tenant,
        function_primordial_cache,
        object_primordial_cache,
    ))?;
    let mut proxy_fn = (crate::types_shim::make_builtin(
        tenant,
        "Proxy",
        {
            move |tenant: &mut T,
                  _unused_this_arg: T::Value,
                  _unused_args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                return Err(TenantError::TypeError(
                    ("Constructor Proxy requires 'new'").to_string(),
                ));
            }
        },
        Some(Box::new({
            move |tenant: &mut T,
                  _new_target: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                (crate::types_shim::assert_object(
                    tenant,
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    "Proxy target must be an object",
                ))?;
                (crate::types_shim::assert_object(
                    tenant,
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    "Proxy handler must be an object",
                ))?;
                return Ok(({
                    proxy_exotic(
                        tenant,
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        &(args)
                            .get(1usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        None,
                    )
                })?);
            }
        })),
        Some((function_prototype).clone()),
    ))?;
    let mut result = ProxyPrimordial {
        proxy: (proxy_fn).clone(),
    };
    cache.entry = Some((result).clone());
    let mut revocable = (crate::types_shim::make_builtin(
        tenant,
        "revocable",
        {
            let function_prototype = function_prototype.clone();
            move |tenant: &mut T,
                  _this: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                (crate::types_shim::assert_object(
                    tenant,
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    "Proxy target must be an object",
                ))?;
                (crate::types_shim::assert_object(
                    tenant,
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    "Proxy handler must be an object",
                ))?;
                let mut state = ProxyStateImpl::new(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                );
                let mut proxy = ({
                    proxy_exotic(
                        tenant,
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        &(args)
                            .get(1usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        Some((state).clone()),
                    )
                })?;
                let mut revoke = (crate::types_shim::make_builtin(
                    tenant,
                    "revoke",
                    {
                        let state = state.clone();
                        move |tenant: &mut T,
                              _unused_this_arg: T::Value,
                              _unused_args: &[T::Value]|
                              -> Result<T::Value, TenantError> {
                            let __undefined = tenant.undefined_value();
                            state.revoke();
                            Ok(tenant.undefined_value())
                        }
                    },
                    None,
                    Some((function_prototype).clone()),
                ))?;
                let mut record = (tenant.make(None))?;
                (crate::types_shim::define_data(
                    tenant,
                    &record,
                    &PropertyKey::from("proxy"),
                    &proxy,
                    TenantPropertyDescriptor::default(),
                ))?;
                (crate::types_shim::define_data(
                    tenant,
                    &record,
                    &PropertyKey::from("revoke"),
                    &revoke,
                    TenantPropertyDescriptor::default(),
                ))?;
                return Ok(record);
            }
        },
        None,
        Some((function_prototype).clone()),
    ))?;
    (crate::types_shim::define_data(
        tenant,
        &proxy_fn,
        &PropertyKey::from("revocable"),
        &revocable,
        TenantPropertyDescriptor::default(),
    ))?;
    return Ok(result);
}
