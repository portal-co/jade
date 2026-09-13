/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/typed-arrays.ts. */
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypedArrayKind {
    Int8Array,
    Uint8Array,
    Uint8ClampedArray,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Float32Array,
    Float64Array,
}
impl AsRef<str> for TypedArrayKind {
    fn as_ref(&self) -> &str {
        match self {
            TypedArrayKind::Int8Array => "Int8Array",
            TypedArrayKind::Uint8Array => "Uint8Array",
            TypedArrayKind::Uint8ClampedArray => "Uint8ClampedArray",
            TypedArrayKind::Int16Array => "Int16Array",
            TypedArrayKind::Uint16Array => "Uint16Array",
            TypedArrayKind::Int32Array => "Int32Array",
            TypedArrayKind::Uint32Array => "Uint32Array",
            TypedArrayKind::Float32Array => "Float32Array",
            TypedArrayKind::Float64Array => "Float64Array",
        }
    }
}
impl std::fmt::Display for TypedArrayKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}
pub struct Record_<T: Tenant> {
    pub kind: TypedArrayKind,
    pub buffer: T::Value,
    pub offset: f64,
    pub length: f64,
}
impl<T: Tenant> Clone for Record_<T> {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            buffer: self.buffer.clone(),
            offset: self.offset.clone(),
            length: self.length.clone(),
        }
    }
}
impl TypedArrayKind {
    pub fn codec_bytes(&self) -> f64 {
        match self {
            TypedArrayKind::Int8Array => 1f64,
            TypedArrayKind::Uint8Array => 1f64,
            TypedArrayKind::Uint8ClampedArray => 1f64,
            TypedArrayKind::Int16Array => 2f64,
            TypedArrayKind::Uint16Array => 2f64,
            TypedArrayKind::Int32Array => 4f64,
            TypedArrayKind::Uint32Array => 4f64,
            TypedArrayKind::Float32Array => 4f64,
            TypedArrayKind::Float64Array => 8f64,
        }
    }
    pub fn codec_get(&self, v: &[u8], i: usize) -> f64 {
        match self {
            TypedArrayKind::Int8Array => (v[i] as i8) as f64,
            TypedArrayKind::Uint8Array => v[i] as f64,
            TypedArrayKind::Uint8ClampedArray => v[i] as f64,
            TypedArrayKind::Int16Array => i16::from_le_bytes([v[i], v[i + 1]]) as f64,
            TypedArrayKind::Uint16Array => u16::from_le_bytes([v[i], v[i + 1]]) as f64,
            TypedArrayKind::Int32Array => {
                i32::from_le_bytes([v[i], v[i + 1], v[i + 2], v[i + 3]]) as f64
            }
            TypedArrayKind::Uint32Array => {
                u32::from_le_bytes([v[i], v[i + 1], v[i + 2], v[i + 3]]) as f64
            }
            TypedArrayKind::Float32Array => {
                f32::from_le_bytes([v[i], v[i + 1], v[i + 2], v[i + 3]]) as f64
            }
            TypedArrayKind::Float64Array => f64::from_le_bytes([
                v[i],
                v[i + 1],
                v[i + 2],
                v[i + 3],
                v[i + 4],
                v[i + 5],
                v[i + 6],
                v[i + 7],
            ]),
        }
    }
    pub fn codec_set(&self, v: &mut [u8], i: usize, x: f64) {
        match self {
            TypedArrayKind::Int8Array => {
                v[i] = ((x) as i64) as u8;
            }
            TypedArrayKind::Uint8Array => {
                v[i] = ((x) as i64) as u8;
            }
            TypedArrayKind::Uint8ClampedArray => {
                v[i] = ((f64::max(
                    (0f64) as f64,
                    (f64::min((255f64) as f64, (f64::round(x)) as f64)) as f64,
                )) as i64) as u8;
            }
            TypedArrayKind::Int16Array => {
                let __bytes = (((x) as i64) as u16).to_le_bytes();
                v[i] = __bytes[0];
                v[i + 1] = __bytes[1];
            }
            TypedArrayKind::Uint16Array => {
                let __bytes = (((x) as i64) as u16).to_le_bytes();
                v[i] = __bytes[0];
                v[i + 1] = __bytes[1];
            }
            TypedArrayKind::Int32Array => {
                let __bytes = (((x) as i64) as u32).to_le_bytes();
                v[i..i + 4].copy_from_slice(&__bytes);
            }
            TypedArrayKind::Uint32Array => {
                let __bytes = (((x) as i64) as u32).to_le_bytes();
                v[i..i + 4].copy_from_slice(&__bytes);
            }
            TypedArrayKind::Float32Array => {
                let __bytes = ((x) as f32).to_le_bytes();
                v[i..i + 4].copy_from_slice(&__bytes);
            }
            TypedArrayKind::Float64Array => {
                let __bytes = (x).to_le_bytes();
                v[i..i + 8].copy_from_slice(&__bytes);
            }
        }
    }
}
pub struct TypedArrayPrimordial<T: Tenant> {
    pub constructors: std::collections::HashMap<TypedArrayKind, T::Value>,
}
impl<T: Tenant> Clone for TypedArrayPrimordial<T> {
    fn clone(&self) -> Self {
        Self {
            constructors: self.constructors.clone(),
        }
    }
}
pub struct TypedArrayPrimordialCache<T: Tenant + 'static, H: BufferHooks + 'static> {
    entry: Option<TypedArrayPrimordialImpl<T, H>>,
}
impl<T: Tenant + 'static, H: BufferHooks + 'static> Default for TypedArrayPrimordialCache<T, H> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub struct TypedArrayPrimordialImplInner<T: Tenant + 'static, H: BufferHooks + 'static> {
    buffers: crate::array_buffer::BufferPrimordialImpl<T, H>,
    hooks: H,
    object_prototype: T::Value,
    records: std::collections::HashMap<T::ObjectId, Record_<T>>,
    prototypes: std::collections::HashMap<TypedArrayKind, T::Value>,
    constructors: std::collections::HashMap<TypedArrayKind, T::Value>,
}
pub struct TypedArrayPrimordialImpl<T: Tenant + 'static, H: BufferHooks + 'static> {
    inner: ::std::rc::Rc<::std::cell::RefCell<TypedArrayPrimordialImplInner<T, H>>>,
}
impl<T: Tenant + 'static, H: BufferHooks + 'static> Clone for TypedArrayPrimordialImpl<T, H> {
    fn clone(&self) -> Self {
        Self {
            inner: ::std::rc::Rc::clone(&self.inner),
        }
    }
}
impl<T: Tenant + 'static, H: BufferHooks + 'static> TypedArrayPrimordialImpl<T, H> {
    pub fn new(
        buffers: crate::array_buffer::BufferPrimordialImpl<T, H>,
        hooks: H,
        object_prototype: &T::Value,
    ) -> Self {
        Self {
            inner: ::std::rc::Rc::new(::std::cell::RefCell::new(TypedArrayPrimordialImplInner {
                buffers: buffers,
                hooks: hooks,
                object_prototype: (object_prototype).clone(),
                records: ::std::collections::HashMap::new(),
                prototypes: ::std::collections::HashMap::new(),
                constructors: ::std::collections::HashMap::new(),
            })),
        }
    }
    pub fn create(
        &self,
        tenant: &mut T,
        kind: TypedArrayKind,
        buffer: &T::Value,
        offset: f64,
        length: f64,
    ) -> Result<T::Value, TenantError> {
        let __undefined = tenant.undefined_value();
        let that = self.clone();
        let mut codec = kind;
        let mut prototype = ((self.inner.borrow().prototypes).get(&kind).cloned()).unwrap();
        let mut value = ({
            struct ExoticHandler0<T: Tenant + 'static, H: BufferHooks + 'static> {
                codec: TypedArrayKind,
                prototype: T::Value,
                that: TypedArrayPrimordialImpl<T, H>,
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
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    let mut record = ((that.inner.borrow().records)
                        .get(&tenant.object_id(receiver))
                        .cloned())
                    .unwrap();
                    if (key == &PropertyKey::from("length")) {
                        return Ok(tenant.number_value(record.length));
                    }
                    if (key == &PropertyKey::from("byteLength")) {
                        return Ok(tenant.number_value((record.length * codec.codec_bytes())));
                    }
                    if (key == &PropertyKey::from("byteOffset")) {
                        return Ok(tenant.number_value(record.offset));
                    }
                    if (key == &PropertyKey::from("buffer")) {
                        return Ok(record.buffer);
                    }
                    if (matches!(key, portal_solutions_jade_tenant_rt::PropertyKey::String(_))
                        && matches ! (key , portal_solutions_jade_tenant_rt :: PropertyKey :: String (__s) if portal_solutions_jade_tenant_rt :: intrinsics :: is_array_index_string (__s)))
                    {
                        let mut index = (key.to_string()).parse::<f64>().unwrap_or(f64::NAN);
                        if (index >= record.length) {
                            return Ok(tenant.undefined_value());
                        }
                        let mut handle =
                            (that.inner.borrow().buffers.record(tenant, &record.buffer))
                                .unwrap()
                                .handle;
                        let mut bytes = (that.inner.borrow_mut().hooks.read(
                            &(handle),
                            (record.offset + (index * codec.codec_bytes())) as usize,
                            (codec.codec_bytes()) as usize,
                        ))?;
                        return Ok(tenant.number_value(codec.codec_get(&(bytes), (0f64) as usize)));
                    }
                    if (key == &PropertyKey::from("subarray")) {
                        return Ok((crate::types_shim::make_builtin(
                            tenant,
                            "subarray",
                            {
                                let record = record.clone();
                                let that = that.clone();
                                let codec = codec.clone();
                                move | tenant : & mut T , _this : T :: Value , args : & [T :: Value] | -> Result < T :: Value , TenantError > { let __undefined = tenant . undefined_value () ; let mut begin = f64 :: min (({ let __hoisted_arg0 = & { let __arg = (args) . get (0usize) . cloned () . unwrap_or_else (|| __undefined . clone ()) ; if matches ! (tenant . typeof_tag (& __arg) , ValueTag :: Undefined | ValueTag :: Null) { tenant . number_value (0f64) } else { __arg } } ; ((crate :: types_shim :: to_index (tenant , __hoisted_arg0)) ? as f64) }) as f64 , (record . length) as f64) ; let mut end = f64 :: min (({ let __hoisted_arg0 = & { let __arg = (args) . get (1usize) . cloned () . unwrap_or_else (|| __undefined . clone ()) ; if matches ! (tenant . typeof_tag (& __arg) , ValueTag :: Undefined | ValueTag :: Null) { tenant . number_value (record . length) } else { __arg } } ; ((crate :: types_shim :: to_index (tenant , __hoisted_arg0)) ? as f64) }) as f64 , (record . length) as f64) ; return Ok ((that . create (tenant , record . kind , & record . buffer , (record . offset + (begin * codec . codec_bytes ())) , f64 :: max ((0f64) as f64 , ((end - begin)) as f64))) ?) ; }
                            },
                            None,
                            Some((that.inner.borrow().object_prototype).clone()),
                        ))?);
                    }
                    if (key == &PropertyKey::from("set")) {
                        return Ok((crate::types_shim::make_builtin(
                            tenant,
                            "set",
                            {
                                let record = record.clone();
                                let that = that.clone();
                                let codec = codec.clone();
                                move | tenant : & mut T , _this : T :: Value , args : & [T :: Value] | -> Result < T :: Value , TenantError > { let __undefined = tenant . undefined_value () ; let mut source = (that . inner . borrow () . records) . get (& tenant . object_id (& (args) . get (0usize) . cloned () . unwrap_or_else (|| __undefined . clone ()))) . cloned () ; let Some (source) = source else { return Err (TenantError :: TypeError (("TypedArray.set requires a Jade typed array") . to_string ())) ; } ; let mut start = { let __hoisted_arg0 = & { let __arg = (args) . get (1usize) . cloned () . unwrap_or_else (|| __undefined . clone ()) ; if matches ! (tenant . typeof_tag (& __arg) , ValueTag :: Undefined | ValueTag :: Null) { tenant . number_value (0f64) } else { __arg } } ; ((crate :: types_shim :: to_index (tenant , __hoisted_arg0)) ? as f64) } ; if ((start + source . length) > record . length) { return Err (TenantError :: RangeError (("source is too large") . to_string ())) ; } { let mut i = 0f64 ; while i < source . length { let mut source_codec = source . kind ; let mut source_handle = (that . inner . borrow () . buffers . record (tenant , & source . buffer)) . unwrap () . handle ; let mut bytes = (that . inner . borrow_mut () . hooks . read (& (source_handle) , ((source . offset + (i * source_codec . codec_bytes ()))) as usize , (source_codec . codec_bytes ()) as usize)) ? ; let mut number = source_codec . codec_get (& (bytes) , (0f64) as usize) ; let mut target = vec ! [0u8 ; (codec . codec_bytes ()) as usize] ; codec . codec_set (& mut (target) , (0f64) as usize , number) ; (that . inner . borrow_mut () . hooks . write (& ((that . inner . borrow () . buffers . record (tenant , & record . buffer)) . unwrap () . handle) , ((record . offset + (((start + i)) * codec . codec_bytes ()))) as usize , & (target))) ? ; i += 1.0 ; } } Ok (tenant . undefined_value ()) }
                            },
                            None,
                            Some((that.inner.borrow().object_prototype).clone()),
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
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    if (!matches!(key, portal_solutions_jade_tenant_rt::PropertyKey::String(_))
                        || (!matches ! (key , portal_solutions_jade_tenant_rt :: PropertyKey :: String (__s) if portal_solutions_jade_tenant_rt :: intrinsics :: is_array_index_string (__s))))
                    {
                        return Ok(());
                    }
                    let mut record = ((that.inner.borrow().records)
                        .get(&tenant.object_id(receiver))
                        .cloned())
                    .unwrap();
                    let mut index = (key.to_string()).parse::<f64>().unwrap_or(f64::NAN);
                    if (index >= record.length) {
                        return Ok(());
                    }
                    let mut bytes = vec![0u8; (codec.codec_bytes()) as usize];
                    codec.codec_set(&mut (bytes), (0f64) as usize, tenant.to_number(&value));
                    (that.inner.borrow_mut().hooks.write(
                        &((that.inner.borrow().buffers.record(tenant, &record.buffer))
                            .unwrap()
                            .handle),
                        (record.offset + (index * codec.codec_bytes())) as usize,
                        &(bytes),
                    ))?;
                    Ok(())
                }
                fn has(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    let mut record = ((that.inner.borrow().records)
                        .get(&tenant.object_id(receiver))
                        .cloned())
                    .unwrap();
                    return Ok(((matches!(
                        key,
                        portal_solutions_jade_tenant_rt::PropertyKey::String(_)
                    ) && matches ! (key , portal_solutions_jade_tenant_rt :: PropertyKey :: String (__s) if portal_solutions_jade_tenant_rt :: intrinsics :: is_array_index_string (__s)))
                        && ((key.to_string()).parse::<f64>().unwrap_or(f64::NAN)
                            < record.length)));
                }
                fn delete(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<(), TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    Ok(())
                }
                fn own_keys(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<Vec<PropertyKey>, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    let mut record = ((that.inner.borrow().records)
                        .get(&tenant.object_id(receiver))
                        .cloned())
                    .unwrap();
                    return Ok((0..(record.length) as usize)
                        .map(|__index| {
                            let i = __index as f64;
                            PropertyKey::from((i).to_string())
                        })
                        .collect::<Vec<_>>());
                }
                fn own_property_keys(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<Vec<PropertyKey>, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    let mut record = ((that.inner.borrow().records)
                        .get(&tenant.object_id(receiver))
                        .cloned())
                    .unwrap();
                    return Ok((0..(record.length) as usize)
                        .map(|__index| {
                            let i = __index as f64;
                            PropertyKey::from((i).to_string())
                        })
                        .collect::<Vec<_>>());
                }
                fn get_own_property_descriptor(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    key: &PropertyKey,
                ) -> Result<Option<TenantPropertyDescriptor<T::Value>>, TenantError>
                {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
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
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    return Ok(false);
                }
                fn get_prototype_of(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<Option<T::Value>, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    return Ok(Some((prototype).clone()));
                }
                fn set_prototype_of(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                    prototype: Option<T::Value>,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    return Ok(false);
                }
                fn is_extensible(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    return Ok(true);
                }
                fn prevent_extensions(
                    &mut self,
                    tenant: &mut T,
                    receiver: &T::Value,
                ) -> Result<bool, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
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
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
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
                    let codec = self.codec.clone();
                    let prototype = self.prototype.clone();
                    let that = self.that.clone();
                    Ok(())
                }
            }
            tenant.make_exotic(
                Some((prototype).clone()),
                Box::new(ExoticHandler0 {
                    codec: (codec).clone(),
                    prototype: (prototype).clone(),
                    that: (that).clone(),
                }),
            )
        })?;
        {
            (self.inner.borrow_mut().records).insert(
                tenant.object_id(&value),
                (Record_ {
                    kind: (kind).clone(),
                    buffer: (buffer).clone(),
                    offset: (offset).clone(),
                    length: (length).clone(),
                })
                .clone(),
            );
        };
        return Ok(value);
    }
    pub fn make_constructor(
        &self,
        tenant: &mut T,
        kind: TypedArrayKind,
    ) -> Result<T::Value, TenantError> {
        let __undefined = tenant.undefined_value();
        let that = self.clone();
        let mut codec = kind;
        let mut prototype = (tenant.make(Some((self.inner.borrow().object_prototype).clone())))?;
        {
            (self.inner.borrow_mut().prototypes).insert(kind, (prototype).clone());
        };
        let mut ctor = (crate::types_shim::make_builtin(
            tenant,
            kind,
            {
                let kind = kind.clone();
                move |tenant: &mut T,
                      _unused_this_arg: T::Value,
                      _unused_args: &[T::Value]|
                      -> Result<T::Value, TenantError> {
                    let __undefined = tenant.undefined_value();
                    return Err(TenantError::TypeError(
                        (format!("Constructor {} requires 'new'", kind)).to_string(),
                    ));
                }
            },
            Some(Box::new({
                let kind = kind.clone();
                let that = that.clone();
                let codec = codec.clone();
                move |tenant: &mut T,
                      _target: T::Value,
                      args: &[T::Value]|
                      -> Result<T::Value, TenantError> {
                    let __undefined = tenant.undefined_value();
                    let mut first = (args)
                        .get(0usize)
                        .cloned()
                        .unwrap_or_else(|| __undefined.clone());
                    let mut buffer_record = that.inner.borrow().buffers.record(tenant, &first);
                    if let Some(buffer_record) = (buffer_record).clone() {
                        let mut offset = {
                            let __hoisted_arg0 = &{
                                let __arg = (args)
                                    .get(1usize)
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
                        };
                        if ((offset % codec.codec_bytes()) != 0.0) {
                            return Err(TenantError::RangeError(
                                ("unaligned byteOffset").to_string(),
                            ));
                        }
                        let mut total = ((that
                            .inner
                            .borrow_mut()
                            .hooks
                            .byte_length(&(buffer_record.handle)))
                        .map(|v| v as f64))?;
                        let mut length = (if (tenant.typeof_tag(
                            &(args)
                                .get(2usize)
                                .cloned()
                                .unwrap_or_else(|| __undefined.clone()),
                        ) == ValueTag::Undefined)
                        {
                            ((total - offset) / codec.codec_bytes())
                        } else {
                            ((crate::types_shim::to_index(
                                tenant,
                                &(args)
                                    .get(2usize)
                                    .cloned()
                                    .unwrap_or_else(|| __undefined.clone()),
                            ))? as f64)
                        });
                        if ((!portal_solutions_jade_tenant_rt::intrinsics::is_integer(length))
                            || ((offset + (length * codec.codec_bytes())) > total))
                        {
                            return Err(TenantError::RangeError(
                                ("typed array is out of bounds").to_string(),
                            ));
                        }
                        return Ok((that.create(tenant, kind, &first, offset, length))?);
                    }
                    let mut length = {
                        let __hoisted_arg0 = &{
                            let __arg = first;
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
                    };
                    let mut handle = (that.inner.borrow_mut().hooks.allocate(
                        portal_solutions_jade_tenant_rt::BufferKind::ArrayBuffer,
                        (length * codec.codec_bytes()) as usize,
                    ))?;
                    return Ok(({
                        let __hoisted_arg2 = &(that.inner.borrow().buffers.shell(
                            tenant,
                            portal_solutions_jade_tenant_rt::BufferKind::ArrayBuffer,
                            handle,
                            &that.inner.borrow().object_prototype,
                        ))?;
                        that.create(tenant, kind, __hoisted_arg2, 0f64, length)
                    })?);
                }
            })),
            Some((self.inner.borrow().object_prototype).clone()),
        ))?;
        (crate::types_shim::define_data(
            tenant,
            &ctor,
            &PropertyKey::from("prototype"),
            &prototype,
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
pub fn typed_arrays_primordial<T: Tenant + 'static, H: BufferHooks + Clone + 'static>(
    tenant: &mut T,
    hooks: H,
    kinds: Vec<TypedArrayKind>,
    cache: &mut TypedArrayPrimordialCache<T, H>,
    object_primordial_cache: &mut crate::object::ObjectPrimordialCache<T>,
    buffer_primordial_impl_cache: &mut crate::array_buffer::BufferPrimordialCache<T, H>,
) -> Result<TypedArrayPrimordialImpl<T, H>, TenantError> {
    let __undefined = tenant.undefined_value();
    if let Some(cached) = cache.entry.clone() {
        return Ok(cached);
    }
    let mut buffers = (crate::array_buffer::buffer_primordial(
        tenant,
        (hooks).clone(),
        buffer_primordial_impl_cache,
        object_primordial_cache,
    ))?;
    let ObjectPrimordial {
        object_prototype: object_prototype,
        ..
    } = (crate::object::object_primordial(tenant, object_primordial_cache))?;
    let mut impl_ = TypedArrayPrimordialImpl::new(buffers, hooks, &object_prototype);
    for kind in kinds {
        {
            (impl_.inner.borrow_mut().constructors)
                .insert(kind, ((impl_.make_constructor(tenant, kind))?).clone());
        };
    }
    let mut primordial = impl_;
    cache.entry = Some((primordial).clone());
    return Ok(primordial);
}
