/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/function.ts. */
#[allow(unused_imports)]
use crate::object::ObjectPrimordial;
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    BufferHooks, BufferKind, DynFields, PropertyKey, Tenant, TenantError, TenantExoticHandler,
    TenantInvocation, TenantPropertyDescriptor, ValueTag,
};
pub struct FunctionPrimordial<T: Tenant> {
    pub function: T::Value,
    pub function_prototype: T::Value,
}
impl<T: Tenant> Clone for FunctionPrimordial<T> {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
            function_prototype: self.function_prototype.clone(),
        }
    }
}
pub struct FunctionPrimordialCache<T: Tenant> {
    entry: Option<FunctionPrimordial<T>>,
}
impl<T: Tenant> Default for FunctionPrimordialCache<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub fn function_primordial<T: Tenant + 'static>(
    tenant: &mut T,
    cache: &mut FunctionPrimordialCache<T>,
    object_primordial_cache: &mut crate::object::ObjectPrimordialCache<T>,
) -> Result<FunctionPrimordial<T>, TenantError> {
    let __undefined = tenant.undefined_value();
    if let Some(existing) = cache.entry.clone() {
        return Ok(existing);
    }
    let ObjectPrimordial {
        object_prototype: object_prototype,
        ..
    } = (crate::object::object_primordial(tenant, object_primordial_cache))?;
    let mut function_prototype = (tenant.make(Some((object_prototype).clone())))?;
    let mut function_fn = (crate::types_shim::make_builtin(
        tenant,
        "Function",
        {
            move |tenant: &mut T,
                  _unused_this_arg: T::Value,
                  _unused_args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                return Err(TenantError::TypeError(
                    ("dynamic Function construction is not available in this Jade realm")
                        .to_string(),
                ));
            }
        },
        Some(Box::new({
            move |tenant: &mut T,
                  _unused_this_arg: T::Value,
                  _unused_args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                return Err(TenantError::TypeError(
                    ("dynamic Function construction is not available in this Jade realm")
                        .to_string(),
                ));
            }
        })),
        Some((function_prototype).clone()),
    ))?;
    let mut result = FunctionPrimordial {
        function: (function_fn).clone(),
        function_prototype: (function_prototype).clone(),
    };
    cache.entry = Some((result).clone());
    (tenant.set_prototype_of(&function_fn, Some((function_prototype).clone())))?;
    (crate::types_shim::define_data(
        tenant,
        &function_fn,
        &PropertyKey::from("prototype"),
        &function_prototype,
        TenantPropertyDescriptor {
            value: None,
            writable: Some(false),
            get: None,
            set: None,
            enumerable: None,
            configurable: Some(false),
        },
    ))?;
    let mut call = (crate::types_shim::make_builtin(
        tenant,
        "call",
        {
            move |tenant: &mut T,
                  this_arg: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                if (tenant.typeof_tag(&this_arg) != ValueTag::Function) {
                    return Err(TenantError::TypeError(
                        ("Function.prototype.call called on non-function").to_string(),
                    ));
                }
                return Ok((tenant.invoke(
                    &this_arg,
                    TenantInvocation::Apply {
                        this_arg: ((args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()))
                        .clone(),
                        args: ((args)[(1f64) as usize..].to_vec()).to_vec(),
                    },
                ))?);
            }
        },
        None,
        Some((function_prototype).clone()),
    ))?;
    let mut apply = (crate::types_shim::make_builtin(
        tenant,
        "apply",
        {
            move |tenant: &mut T,
                  this_arg: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                if (tenant.typeof_tag(&this_arg) != ValueTag::Function) {
                    return Err(TenantError::TypeError(
                        ("Function.prototype.apply called on non-function").to_string(),
                    ));
                }
                let mut list = (args)
                    .get(1usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone());
                let mut values = (if ((tenant.typeof_tag(&list) == ValueTag::Null)
                    || (tenant.typeof_tag(&list) == ValueTag::Undefined))
                {
                    vec![]
                } else {
                    ({
                        (crate::types_shim::assert_object(
                            tenant,
                            &list,
                            "Function.prototype.apply arguments must be an object",
                        ))?;
                        (crate::types_shim::guest_array_like(tenant, &list))?
                    })
                });
                return Ok((tenant.invoke(
                    &this_arg,
                    TenantInvocation::Apply {
                        this_arg: ((args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()))
                        .clone(),
                        args: (values).to_vec(),
                    },
                ))?);
            }
        },
        None,
        Some((function_prototype).clone()),
    ))?;
    let mut bind = (crate::types_shim::make_builtin(
        tenant,
        "bind",
        {
            let function_prototype = function_prototype.clone();
            move |tenant: &mut T,
                  this_arg: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                if (tenant.typeof_tag(&this_arg) != ValueTag::Function) {
                    return Err(TenantError::TypeError(
                        ("Function.prototype.bind called on non-function").to_string(),
                    ));
                }
                let mut target = this_arg;
                let mut bound_this = (args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone());
                let mut prefix = (args)[(1f64) as usize..].to_vec();
                return Ok((crate::types_shim::make_builtin(
                    tenant,
                    "bound",
                    {
                        let bound_this = bound_this.clone();
                        let prefix = prefix.clone();
                        let target = target.clone();
                        move |tenant: &mut T,
                              _ignored: T::Value,
                              call_args: &[T::Value]|
                              -> Result<T::Value, TenantError> {
                            let __undefined = tenant.undefined_value();
                            return Ok((tenant.invoke(
                                &target,
                                TenantInvocation::Apply {
                                    this_arg: (bound_this).clone(),
                                    args: ((((prefix).iter().cloned())
                                        .chain((call_args).iter().cloned()))
                                    .collect::<Vec<_>>())
                                    .to_vec(),
                                },
                            ))?);
                        }
                    },
                    Some(Box::new({
                        let prefix = prefix.clone();
                        let target = target.clone();
                        move |tenant: &mut T,
                              new_target: T::Value,
                              call_args: &[T::Value]|
                              -> Result<T::Value, TenantError> {
                            let __undefined = tenant.undefined_value();
                            return Ok((tenant.invoke(
                                &target,
                                TenantInvocation::Construct {
                                    args: ((((prefix).iter().cloned())
                                        .chain((call_args).iter().cloned()))
                                    .collect::<Vec<_>>())
                                    .to_vec(),
                                    new_target: (new_target).clone(),
                                },
                            ))?);
                        }
                    })),
                    Some((function_prototype).clone()),
                ))?);
            }
        },
        None,
        Some((function_prototype).clone()),
    ))?;
    (crate::types_shim::define_data(
        tenant,
        &function_prototype,
        &PropertyKey::from("call"),
        &call,
        TenantPropertyDescriptor::default(),
    ))?;
    (crate::types_shim::define_data(
        tenant,
        &function_prototype,
        &PropertyKey::from("apply"),
        &apply,
        TenantPropertyDescriptor::default(),
    ))?;
    (crate::types_shim::define_data(
        tenant,
        &function_prototype,
        &PropertyKey::from("bind"),
        &bind,
        TenantPropertyDescriptor::default(),
    ))?;
    return Ok(result);
}
