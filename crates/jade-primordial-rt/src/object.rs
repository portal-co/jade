/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/object.ts. */
// Generator-induced warning clutter is suppressed module-locally; hand-written
// code keeps full lints.
#![allow(
    unused_mut,
    unused_variables,
    unused_parens,
    unused_must_use,
    non_shorthand_field_patterns,
    unreachable_patterns,
    unused_labels,
    unreachable_code
)]
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    BufferHooks, BufferKind, DynFields, PropertyKey, Tenant, TenantError, TenantExoticHandler,
    TenantInvocation, TenantPropertyDescriptor, ValueTag,
};
pub struct ObjectPrimordial<T: Tenant> {
    pub object: T::Value,
    pub object_prototype: T::Value,
}
impl<T: Tenant> Clone for ObjectPrimordial<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            object_prototype: self.object_prototype.clone(),
        }
    }
}
pub struct ObjectPrimordialCache<T: Tenant> {
    entry: Option<ObjectPrimordial<T>>,
}
impl<T: Tenant> Default for ObjectPrimordialCache<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub fn install_method<T: Tenant + 'static>(
    tenant: &mut T,
    target: &T::Value,
    name: &str,
    apply: impl FnMut(&mut T, T::Value, &[T::Value]) -> Result<T::Value, TenantError> + 'static,
) -> Result<T::Value, TenantError> {
    let __undefined = tenant.undefined_value();
    let mut fn_ = (crate::types_shim::make_builtin(tenant, name, apply, None, None))?;
    (crate::types_shim::define_data(
        tenant,
        target,
        &PropertyKey::from(name),
        &fn_,
        TenantPropertyDescriptor {
            value: None,
            writable: Some(true),
            get: None,
            set: None,
            enumerable: None,
            configurable: Some(true),
        },
    ))?;
    return Ok(fn_);
}
pub fn lock<T: Tenant + 'static>(
    tenant: &mut T,
    object: &T::Value,
    freeze: bool,
) -> Result<T::Value, TenantError> {
    let __undefined = tenant.undefined_value();
    for key in (tenant.own_property_keys(object))? {
        let mut descriptor = (tenant.get_own_property_descriptor(object, &key))?;
        let Some(descriptor) = descriptor else {
            continue;
        };
        let mut next = TenantPropertyDescriptor {
            configurable: Some(false),
            ..(descriptor).clone()
        };
        if (freeze && ((&next).has_field("value") || (next.writable.is_some()))) {
            next.writable = Some(false);
        }
        if (!((tenant.define_property(object, &key, next))?)) {
            return Err(TenantError::TypeError(
                ("cannot update object descriptor").to_string(),
            ));
        }
    }
    if (!((tenant.prevent_extensions(object))?)) {
        return Err(TenantError::TypeError(
            ("cannot prevent extensions").to_string(),
        ));
    }
    return Ok((object).clone());
}
pub fn object_primordial<T: Tenant + 'static>(
    tenant: &mut T,
    cache: &mut ObjectPrimordialCache<T>,
) -> Result<ObjectPrimordial<T>, TenantError> {
    let __undefined = tenant.undefined_value();
    if let Some(existing) = cache.entry.clone() {
        return Ok(existing);
    }
    let mut object_prototype = (tenant.make(None))?;
    let mut object_fn = (crate::types_shim::make_builtin(
        tenant,
        "Object",
        {
            let object_prototype = object_prototype.clone();
            move |tenant: &mut T,
                  _this_arg: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                let mut value = (args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone());
                if ((tenant.typeof_tag(&value) == ValueTag::Null)
                    || (tenant.typeof_tag(&value) == ValueTag::Undefined))
                {
                    return Ok((tenant.make(Some((object_prototype).clone())))?);
                }
                if ((tenant.typeof_tag(&value) == ValueTag::Object)
                    || (tenant.typeof_tag(&value) == ValueTag::Function))
                {
                    return Ok(value);
                }
                return Err(TenantError::TypeError(
                    ("primitive Object boxing is not available in this Jade realm").to_string(),
                ));
            }
        },
        Some(Box::new({
            let object_prototype = object_prototype.clone();
            move |tenant: &mut T,
                  _new_target: T::Value,
                  args: &[T::Value]|
                  -> Result<T::Value, TenantError> {
                let __undefined = tenant.undefined_value();
                let mut value = (args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone());
                if ((tenant.typeof_tag(&value) == ValueTag::Null)
                    || (tenant.typeof_tag(&value) == ValueTag::Undefined))
                {
                    return Ok((tenant.make(Some((object_prototype).clone())))?);
                }
                if ((tenant.typeof_tag(&value) == ValueTag::Object)
                    || (tenant.typeof_tag(&value) == ValueTag::Function))
                {
                    return Ok(value);
                }
                return Err(TenantError::TypeError(
                    ("primitive Object boxing is not available in this Jade realm").to_string(),
                ));
            }
        })),
        None,
    ))?;
    let mut result = ObjectPrimordial {
        object: (object_fn).clone(),
        object_prototype: (object_prototype).clone(),
    };
    cache.entry = Some((result).clone());
    (crate::types_shim::define_data(
        tenant,
        &object_fn,
        &PropertyKey::from("prototype"),
        &object_prototype,
        TenantPropertyDescriptor {
            value: None,
            writable: Some(false),
            get: None,
            set: None,
            enumerable: None,
            configurable: Some(false),
        },
    ))?;
    (tenant.set_prototype_of(&object_fn, Some((object_prototype).clone())))?;
    (install_method(tenant, &object_fn, "create", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            let mut proto = (args)
                .get(0usize)
                .cloned()
                .unwrap_or_else(|| __undefined.clone());
            if (tenant.typeof_tag(&proto) != ValueTag::Null) {
                (crate::types_shim::assert_object(
                    tenant,
                    &proto,
                    "Object.create prototype must be object or null",
                ))?;
            }
            return Ok(({
                let __hoisted_arg0 = tenant.nullable(&proto);
                tenant.make(__hoisted_arg0)
            })?);
        }
    }))?;
    (install_method(tenant, &object_fn, "keys", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
                let __keys = (tenant.own_keys(
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
    (install_method(tenant, &object_fn, "hasOwn", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
    (install_method(tenant, &object_fn, "assign", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            for source in (args)[(1f64) as usize..].to_vec() {
                if ((tenant.typeof_tag(&source) != ValueTag::Null)
                    && (tenant.typeof_tag(&source) != ValueTag::Undefined))
                {
                    (crate::types_shim::assert_object(tenant, &source, "expected an object"))?;
                    (tenant.assign(
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                        &source,
                    ))?;
                }
            }
            return Ok((args)
                .get(0usize)
                .cloned()
                .unwrap_or_else(|| __undefined.clone()));
        }
    }))?;
    (install_method(tenant, &object_fn, "defineProperty", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
                "property descriptor must be an object",
            ))?;
            let mut descriptor = (crate::types_shim::read_guest_descriptor(
                tenant,
                &(args)
                    .get(2usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ))?;
            if (!(({
                let __hoisted_arg1 = &tenant.to_property_key(
                    &(args)
                        .get(1usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                );
                tenant.define_property(
                    &(args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone()),
                    __hoisted_arg1,
                    descriptor,
                )
            })?)) {
                return Err(TenantError::TypeError(
                    ("cannot define property").to_string(),
                ));
            }
            return Ok((args)
                .get(0usize)
                .cloned()
                .unwrap_or_else(|| __undefined.clone()));
        }
    }))?;
    (install_method(tenant, &object_fn, "defineProperties", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
                    .get(1usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "descriptors must be an object",
            ))?;
            (tenant.define(
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                &(args)
                    .get(1usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ))?;
            return Ok((args)
                .get(0usize)
                .cloned()
                .unwrap_or_else(|| __undefined.clone()));
        }
    }))?;
    (install_method(tenant, &object_fn, "getOwnPropertyDescriptor", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
    (install_method(tenant, &object_fn, "getPrototypeOf", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
    (install_method(tenant, &object_fn, "setPrototypeOf", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
                    "prototype must be object or null",
                ))?;
            }
            if (!(({
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
            })?)) {
                return Err(TenantError::TypeError(("cannot set prototype").to_string()));
            }
            return Ok((args)
                .get(0usize)
                .cloned()
                .unwrap_or_else(|| __undefined.clone()));
        }
    }))?;
    (install_method(tenant, &object_fn, "isExtensible", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
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
    (install_method(tenant, &object_fn, "preventExtensions", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            if (!((tenant.prevent_extensions(
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
            ))?)) {
                return Err(TenantError::TypeError(
                    ("cannot prevent extensions").to_string(),
                ));
            }
            return Ok((args)
                .get(0usize)
                .cloned()
                .unwrap_or_else(|| __undefined.clone()));
        }
    }))?;
    (install_method(tenant, &object_fn, "seal", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok((lock(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                false,
            ))?);
        }
    }))?;
    (install_method(tenant, &object_fn, "freeze", {
        move |tenant: &mut T,
              _this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                "expected an object",
            ))?;
            return Ok((lock(
                tenant,
                &(args)
                    .get(0usize)
                    .cloned()
                    .unwrap_or_else(|| __undefined.clone()),
                true,
            ))?);
        }
    }))?;
    (install_method(tenant, &object_prototype, "hasOwnProperty", {
        move |tenant: &mut T,
              this_arg: T::Value,
              args: &[T::Value]|
              -> Result<T::Value, TenantError> {
            let __undefined = tenant.undefined_value();
            (crate::types_shim::assert_object(
                tenant,
                &this_arg,
                "Object.prototype.hasOwnProperty called on non-object",
            ))?;
            return Ok({
                let __result = ({
                    let __hoisted_arg1 = &tenant.to_property_key(
                        &(args)
                            .get(0usize)
                            .cloned()
                            .unwrap_or_else(|| __undefined.clone()),
                    );
                    tenant.has(&this_arg, __hoisted_arg1)
                })?;
                tenant.boolean_value(__result)
            });
        }
    }))?;
    return Ok(result);
}
