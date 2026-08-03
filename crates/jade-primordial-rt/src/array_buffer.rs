/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/array-buffer.ts. */
#[allow(unused_imports)]
use crate::function::FunctionPrimordial;
#[allow(unused_imports)]
use crate::object::ObjectPrimordial;
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    BufferHooks, BufferKind, DynFields, PropertyKey, Tenant, TenantError, TenantExoticHandler,
    TenantInvocation, TenantPropertyDescriptor, ValueTag,
};
pub struct BufferRecord<H: BufferHooks> {
    pub kind: portal_solutions_jade_tenant_rt::BufferKind,
    pub handle: H::Handle,
}
impl<H: BufferHooks> Clone for BufferRecord<H> {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            handle: self.handle.clone(),
        }
    }
}
pub struct BufferPrimordialCache<T: Tenant + 'static, H: BufferHooks + 'static> {
    entry: Option<BufferPrimordialImpl<T, H>>,
}
impl<T: Tenant + 'static, H: BufferHooks + 'static> Default for BufferPrimordialCache<T, H> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub struct BufferPrimordialImplInner<T: Tenant + 'static, H: BufferHooks + 'static> {
    hooks: H,
    records: std::collections::HashMap<T::ObjectId, BufferRecord<H>>,
    prototypes: std::collections::HashMap<portal_solutions_jade_tenant_rt::BufferKind, T::Value>,
    array_buffer: Option<T::Value>,
    array_buffer_prototype: Option<T::Value>,
    shared_array_buffer: Option<T::Value>,
    shared_array_buffer_prototype: Option<T::Value>,
}
pub struct BufferPrimordialImpl<T: Tenant + 'static, H: BufferHooks + 'static> {
    inner: ::std::rc::Rc<::std::cell::RefCell<BufferPrimordialImplInner<T, H>>>,
}
impl<T: Tenant + 'static, H: BufferHooks + 'static> Clone for BufferPrimordialImpl<T, H> {
    fn clone(&self) -> Self {
        Self {
            inner: ::std::rc::Rc::clone(&self.inner),
        }
    }
}
impl<T: Tenant + 'static, H: BufferHooks + 'static> BufferPrimordialImpl<T, H> {
    pub fn new(hooks: H) -> Self {
        Self {
            inner: ::std::rc::Rc::new(::std::cell::RefCell::new(BufferPrimordialImplInner {
                hooks: hooks,
                records: ::std::collections::HashMap::new(),
                prototypes: ::std::collections::HashMap::new(),
                array_buffer: None,
                array_buffer_prototype: None,
                shared_array_buffer: None,
                shared_array_buffer_prototype: None,
            })),
        }
    }
    pub fn record(&self, tenant: &mut T, value: &T::Value) -> Option<BufferRecord<H>> {
        let __undefined = tenant.undefined_value();
        return (if ((tenant.typeof_tag(value) != ValueTag::Null)
            && ((tenant.typeof_tag(value) == ValueTag::Object)
                || (tenant.typeof_tag(value) == ValueTag::Function)))
        {
            (self.inner.borrow().records)
                .get(&tenant.object_id(&value))
                .cloned()
        } else {
            None
        });
    }
    pub fn prototype_of(&self, kind: portal_solutions_jade_tenant_rt::BufferKind) -> T::Value {
        return ((self.inner.borrow().prototypes).get(&kind).cloned()).unwrap();
    }
    pub fn shell(
        &self,
        tenant: &mut T,
        kind: portal_solutions_jade_tenant_rt::BufferKind,
        handle: H::Handle,
        object_prototype: &T::Value,
    ) -> Result<T::Value, TenantError> {
        let __undefined = tenant.undefined_value();
        let that = self.clone();
        let mut proto = ((self.inner.borrow().prototypes).get(&kind).cloned()).unwrap();
        let mut value = ({
            struct ExoticHandler0<T: Tenant + 'static, H: BufferHooks + 'static> {
                object_prototype: T::Value,
                proto: T::Value,
                that: BufferPrimordialImpl<T, H>,
            }
            impl<T: Tenant + 'static, H: BufferHooks + 'static> TenantExoticHandler<T>
                for ExoticHandler0<T, H>
            {
                fn get(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<T::Value, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    let mut record = (that.record(tenant, receiver)).unwrap();
                    if (key == &PropertyKey::from("byteLength")) {
                        return Ok(tenant.number_value(
                            ((that.inner.borrow_mut().hooks.byte_length(&(record.handle)))
                                .map(|v| v as f64))?,
                        ));
                    }
                    if (key == &PropertyKey::from("slice")) {
                        return Ok((crate::types_shim::make_builtin(
                            tenant,
                            "slice",
                            {
                                let object_prototype = object_prototype.clone();
                                let that = that.clone();
                                let record = record.clone();
                                move | tenant : & mut T , _this : T :: Value , args : & [T :: Value] | -> Result < T :: Value , TenantError > { let __undefined = tenant . undefined_value () ; let mut length = ((that . inner . borrow_mut () . hooks . byte_length (& (record . handle))) . map (| v | v as f64)) ? ; let mut begin = f64 :: min (({ let __hoisted_arg0 = & { let __arg = (args) . get (0usize) . cloned () . unwrap_or_else (|| __undefined . clone ()) ; if matches ! (tenant . typeof_tag (& __arg) , ValueTag :: Undefined | ValueTag :: Null) { tenant . number_value (0f64) } else { __arg } } ; ((crate :: types_shim :: to_index (tenant , __hoisted_arg0)) ? as f64) }) as f64 , (length) as f64) ; let mut end = f64 :: min (({ let __hoisted_arg0 = & { let __arg = (args) . get (1usize) . cloned () . unwrap_or_else (|| __undefined . clone ()) ; if matches ! (tenant . typeof_tag (& __arg) , ValueTag :: Undefined | ValueTag :: Null) { tenant . number_value (length) } else { __arg } } ; ((crate :: types_shim :: to_index (tenant , __hoisted_arg0)) ? as f64) }) as f64 , (length) as f64) ; return Ok (({ let __hoisted_arg2 = (that . inner . borrow_mut () . hooks . slice (& (record . handle) , (begin) as usize , (end) as usize)) ? ; that . shell (tenant , record . kind , __hoisted_arg2 , & object_prototype) }) ?) ; }
                            },
                            None,
                            Some((object_prototype).clone()),
                        ))?);
                    }
                    return Ok(tenant.undefined_value());
                }
                fn set(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                    value: T::Value,
                ) -> Result<(), TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    Ok(())
                }
                fn has(
                    &mut self,
                    tenant: &mut T,
                    _receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(((key == &PropertyKey::from("byteLength"))
                        || (key == &PropertyKey::from("slice"))));
                }
                fn delete(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<(), TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    Ok(())
                }
                fn own_keys(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<Vec<PropertyKey>, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(vec![]);
                }
                fn own_property_keys(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<Vec<PropertyKey>, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(vec![]);
                }
                fn get_own_property_descriptor(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<Option<TenantPropertyDescriptor<T::Value>>, TenantError>
                {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(None);
                }
                fn define_property(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                    descriptor: TenantPropertyDescriptor<T::Value>,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(false);
                }
                fn get_prototype_of(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<Option<T::Value>, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(Some((proto).clone()));
                }
                fn set_prototype_of(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    prototype: Option<T::Value>,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(false);
                }
                fn is_extensible(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(true);
                }
                fn prevent_extensions(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    return Ok(true);
                }
                fn define(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    descriptors: &T::Value,
                ) -> Result<(), TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    Ok(())
                }
                fn assign(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    source: &T::Value,
                ) -> Result<(), TenantError> {
                    let __undefined = tenant.undefined_value();
                    let object_prototype = self.object_prototype.clone();
                    let proto = self.proto.clone();
                    let that = self.that.clone();
                    Ok(())
                }
            }
            tenant.make_exotic(
                Some((proto).clone()),
                Box::new(ExoticHandler0 {
                    object_prototype: (object_prototype).clone(),
                    proto: (proto).clone(),
                    that: (that).clone(),
                }),
            )
        })?;
        {
            (self.inner.borrow_mut().records).insert(
                tenant.object_id(&value),
                (BufferRecord {
                    kind: (kind).clone(),
                    handle: (handle).clone(),
                })
                .clone(),
            );
        };
        return Ok(value);
    }
    pub fn make_constructor(
        &self,
        tenant: &mut T,
        kind: portal_solutions_jade_tenant_rt::BufferKind,
        name: &str,
        object_primordial_cache: &mut crate::object::ObjectPrimordialCache<T>,
    ) -> Result<T::Value, TenantError> {
        let __undefined = tenant.undefined_value();
        let that = self.clone();
        let ObjectPrimordial {
            object_prototype: object_prototype,
            ..
        } = (crate::object::object_primordial(tenant, object_primordial_cache))?;
        let mut proto = (tenant.make(Some((object_prototype).clone())))?;
        {
            (self.inner.borrow_mut().prototypes).insert(kind, (proto).clone());
        };
        let mut ctor = (crate::types_shim::make_builtin(
            tenant,
            name,
            {
                let name = name.to_string();
                move |tenant: &mut T,
                      _unused_this_arg: T::Value,
                      _unused_args: &[T::Value]|
                      -> Result<T::Value, TenantError> {
                    let __undefined = tenant.undefined_value();
                    return Err(TenantError::TypeError(
                        (format!("Constructor {} requires 'new'", name)).to_string(),
                    ));
                }
            },
            Some(Box::new({
                let that = that.clone();
                let kind = kind.clone();
                let object_prototype = object_prototype.clone();
                move |tenant: &mut T,
                      _target: T::Value,
                      args: &[T::Value]|
                      -> Result<T::Value, TenantError> {
                    let __undefined = tenant.undefined_value();
                    return Ok(({
                        let __hoisted_arg2 = (that.inner.borrow_mut().hooks.allocate(
                            kind,
                            ({
                                let __hoisted_arg0 = &{
                                    let __arg = (args)
                                        .get(0usize)
                                        .cloned()
                                        .unwrap_or_else(|| __undefined.clone());
                                    if matches!(
                                        tenant.typeof_tag(&__arg),
                                        ValueTag::Undefined | ValueTag::Null
                                    ) {
                                        tenant.number_value(0f64)
                                    } else {
                                        __arg
                                    }
                                };
                                ((crate::types_shim::to_index(tenant, __hoisted_arg0))? as f64)
                            }) as usize,
                        ))?;
                        that.shell(tenant, kind, __hoisted_arg2, &object_prototype)
                    })?);
                }
            })),
            Some((object_prototype).clone()),
        ))?;
        (crate::types_shim::define_data(
            tenant,
            &ctor,
            &PropertyKey::from("prototype"),
            &proto,
            TenantPropertyDescriptor {
                value: None,
                writable: Some(false),
                get: None,
                set: None,
                enumerable: None,
                configurable: Some(false),
            },
        ))?;
        return Ok(ctor);
    }
}
pub fn buffer_primordial<T: Tenant + 'static, H: BufferHooks + Clone + 'static>(
    tenant: &mut T,
    hooks: H,
    cache: &mut BufferPrimordialCache<T, H>,
    object_primordial_cache: &mut crate::object::ObjectPrimordialCache<T>,
) -> Result<BufferPrimordialImpl<T, H>, TenantError> {
    let __undefined = tenant.undefined_value();
    if let Some(cached) = cache.entry.clone() {
        return Ok(cached);
    }
    let mut supports_shared_array_buffer = hooks.supports_shared_array_buffer();
    let mut impl_ = BufferPrimordialImpl::new(hooks);
    impl_.inner.borrow_mut().array_buffer = Some(
        (impl_.make_constructor(
            tenant,
            portal_solutions_jade_tenant_rt::BufferKind::ArrayBuffer,
            "ArrayBuffer",
            object_primordial_cache,
        ))?,
    );
    impl_.inner.borrow_mut().array_buffer_prototype =
        Some(impl_.prototype_of(portal_solutions_jade_tenant_rt::BufferKind::ArrayBuffer));
    if supports_shared_array_buffer {
        impl_.inner.borrow_mut().shared_array_buffer = Some(
            (impl_.make_constructor(
                tenant,
                portal_solutions_jade_tenant_rt::BufferKind::SharedArrayBuffer,
                "SharedArrayBuffer",
                object_primordial_cache,
            ))?,
        );
        impl_.inner.borrow_mut().shared_array_buffer_prototype = Some(
            impl_.prototype_of(portal_solutions_jade_tenant_rt::BufferKind::SharedArrayBuffer),
        );
    }
    let mut result = impl_;
    cache.entry = Some((result).clone());
    return Ok(result);
}
