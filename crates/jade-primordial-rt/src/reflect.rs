/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/reflect.ts. */
#[allow(unused_imports)]
use crate::object::ObjectPrimordial;
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    DynFields, PropertyKey, Tenant, TenantError, TenantInvocation, TenantPropertyDescriptor,
    ValueTag,
};
pub struct ReflectPrimordial<T: Tenant> {
    pub reflect: T::Value,
}
impl<T: Tenant> Clone for ReflectPrimordial<T> {
    fn clone(&self) -> Self {
        Self {
            reflect: self.reflect.clone(),
        }
    }
}
pub struct ReflectPrimordialCache<T: Tenant> {
    entry: Option<ReflectPrimordial<T>>,
}
impl<T: Tenant> Default for ReflectPrimordialCache<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub fn method<T: Tenant + 'static>(
    tenant: &mut T,
    target: &T::Value,
    name: &str,
    apply: impl FnMut(&mut T, T::Value, &[T::Value]) -> Result<T::Value, TenantError> + 'static,
) -> Result<(), TenantError> {
    let __undefined = tenant.undefined_value();
    ({
        let __hoisted_arg3 = &(crate::types_shim::make_builtin(tenant, name, apply, None, None))?;
        crate::types_shim::define_data(
            tenant,
            target,
            &PropertyKey::from(name),
            __hoisted_arg3,
            TenantPropertyDescriptor::default(),
        )
    })?;
    Ok(())
}
pub fn reflect_primordial<T: Tenant + 'static>(
    tenant: &mut T,
    cache: &mut ReflectPrimordialCache<T>,
    object_primordial_cache: &mut crate::object::ObjectPrimordialCache<T>,
) -> Result<ReflectPrimordial<T>, TenantError> {
    let __undefined = tenant.undefined_value();
    if let Some(existing) = cache.entry.clone() {
        return Ok(existing);
    }
    let ObjectPrimordial {
        object_prototype: object_prototype,
        ..
    } = (crate::object::object_primordial(tenant, object_primordial_cache))?;
    let mut reflect_object = (tenant.make(Some((object_prototype).clone())))?;
    let mut result = ReflectPrimordial {
        reflect: (reflect_object).clone(),
    };
    cache.entry = Some((result).clone());
    (method(tenant, &reflect_object, "get", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok(({
                let __hoisted_arg1 = &tenant.to_property_key(
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                );
                tenant.get(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                )
            })?);
        }
    }))?;
    (method(tenant, &reflect_object, "set", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            ({
                let __hoisted_arg1 = &tenant.to_property_key(
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                );
                tenant.set(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                    (args)
                        .get(2usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                )
            })?;
            return Ok(tenant.boolean_value(true));
        }
    }))?;
    (method(tenant, &reflect_object, "has", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok({
                let __result = ({
                    let __hoisted_arg1 = &tenant.to_property_key(
                        &(args)
                            .get(1usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    );
                    tenant.has(
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        __hoisted_arg1,
                    )
                })?;
                tenant.boolean_value(__result)
            });
        }
    }))?;
    (method(tenant, &reflect_object, "deleteProperty", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            ({
                let __hoisted_arg1 = &tenant.to_property_key(
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                );
                tenant.delete(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                )
            })?;
            return Ok(tenant.boolean_value(true));
        }
    }))?;
    (method(tenant, &reflect_object, "ownKeys", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok({
                let __keys = (tenant.own_property_keys(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                ))?;
                let __values = __keys
                    .iter()
                    .map(|k| tenant.property_key_value(k))
                    .collect::<Result<Vec<_>, _>>()?;
                tenant.indexed_collection(__values)?
            });
        }
    }))?;
    (method(tenant, &reflect_object, "getOwnPropertyDescriptor", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            let mut descriptor = ({
                let __hoisted_arg1 = &tenant.to_property_key(
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                );
                tenant.get_own_property_descriptor(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                )
            })?;
            return Ok(match descriptor {
                None => tenant.undefined_value(),
                Some(ref descriptor) => {
                    (crate::types_shim::descriptor_object(tenant, &descriptor))?
                }
            });
        }
    }))?;
    (method(tenant, &reflect_object, "defineProperty", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(2usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "descriptor must be an object",
            ))?;
            return Ok({
                let __result = ({
                    let __hoisted_arg1 = &tenant.to_property_key(
                        &(args)
                            .get(1usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    );
                    let __hoisted_arg2 = (crate::types_shim::read_guest_descriptor(
                        tenant,
                        &(args)
                            .get(2usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    ))?;
                    tenant.define_property(
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        __hoisted_arg1,
                        __hoisted_arg2,
                    )
                })?;
                tenant.boolean_value(__result)
            });
        }
    }))?;
    (method(tenant, &reflect_object, "getPrototypeOf", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok(((tenant.get_prototype_of(
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ))?)
            .unwrap_or_else(|| tenant.null_value()));
        }
    }))?;
    (method(tenant, &reflect_object, "setPrototypeOf", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            if (tenant.typeof_tag(
                &(args)
                    .get(1usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ) != ValueTag::Null)
            {
                (crate::types_shim::assert_object(
                    tenant,
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    "expected an object",
                ))?;
            }
            return Ok({
                let __result = ({
                    let __hoisted_arg1 = tenant.nullable(
                        &(args)
                            .get(1usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    );
                    tenant.set_prototype_of(
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        __hoisted_arg1,
                    )
                })?;
                tenant.boolean_value(__result)
            });
        }
    }))?;
    (method(tenant, &reflect_object, "isExtensible", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok({
                let __result = (tenant.is_extensible(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                ))?;
                tenant.boolean_value(__result)
            });
        }
    }))?;
    (method(tenant, &reflect_object, "preventExtensions", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok({
                let __result = (tenant.prevent_extensions(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                ))?;
                tenant.boolean_value(__result)
            });
        }
    }))?;
    (method(tenant, &reflect_object, "apply", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            if (tenant.typeof_tag(
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ) != ValueTag::Function)
            {
                return Err(TenantError::TypeError(
                    ("Reflect.apply target must be a function").to_string(),
                ));
            }
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(2usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "Reflect.apply arguments must be an object",
            ))?;
            return Ok(({
                let __hoisted_arg1 = TenantInvocation::Apply {
                    this_arg: ((args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()))
                    .clone(),
                    args: ((crate::types_shim::guest_array_like(
                        tenant,
                        &(args)
                            .get(2usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    ))?)
                    .to_vec(),
                };
                tenant.invoke(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                )
            })?);
        }
    }))?;
    (method(tenant, &reflect_object, "construct", {
        move |tenant: &mut T, _this: T::Value, args: &[T::Value]| -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            if (tenant.typeof_tag(
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ) != ValueTag::Function)
            {
                return Err(TenantError::TypeError(
                    ("Reflect.construct target must be a function").to_string(),
                ));
            }
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(1usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "Reflect.construct arguments must be an object",
            ))?;
            let mut new_target = (if (tenant.typeof_tag(
                &(args)
                    .get(2usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ) == ValueTag::Undefined)
            {
                (args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone())
            } else {
                (args)
                    .get(2usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone())
            });
            if (tenant.typeof_tag(&new_target) != ValueTag::Function) {
                return Err(TenantError::TypeError(
                    ("Reflect.construct newTarget must be a function").to_string(),
                ));
            }
            return Ok(({
                let __hoisted_arg1 = TenantInvocation::Construct {
                    args: ((crate::types_shim::guest_array_like(
                        tenant,
                        &(args)
                            .get(1usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    ))?)
                    .to_vec(),
                    new_target: (new_target).clone(),
                };
                tenant.invoke(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                )
            })?);
        }
    }))?;
    return Ok(result);
}
