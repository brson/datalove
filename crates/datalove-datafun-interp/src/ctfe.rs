//! Compile-time function evaluation (CTFE) support.
//!
//! Implements the `CtfeEvaluator` trait for the interpreter, allowing
//! const expressions to be evaluated at compile time.

use rmx::prelude::*;
use std::sync::Arc;
use datalove_datafun_ir::{ConstF32, ConstF64, ConstValue, CtfeError, CtfeEvaluator, IrCodeUnit, IrType};
use crate::{IrInterpreter, ScriptEnvironment, UnitCompletion, Destination, ModuleFunctionRegistry, NativeResolver};
use datalove_rtdt::AnyOwned;

/// CTFE evaluator backed by the IR interpreter.
pub struct InterpCtfeEvaluator {
    interp: IrInterpreter,
    /// Module function registry for cross-module CTFE calls.
    module_registry: Option<Arc<ModuleFunctionRegistry>>,
}

impl InterpCtfeEvaluator {
    /// Create a new CTFE evaluator without module function support.
    pub fn new() -> Self {
        Self {
            interp: IrInterpreter::new(),
            module_registry: None,
        }
    }

    /// Create a CTFE evaluator with module function support.
    ///
    /// The registry provides access to compiled module functions for cross-module calls.
    pub fn with_module_registry(module_registry: Arc<ModuleFunctionRegistry>) -> Self {
        Self {
            interp: IrInterpreter::new(),
            module_registry: Some(module_registry),
        }
    }

    /// Let const expressions call natives, found through `resolver`.
    ///
    /// Without one, a const that reaches a native call panics, there being
    /// nothing registered to call.
    pub fn with_native_resolver(mut self, resolver: Arc<dyn NativeResolver>) -> Self {
        self.interp.native_table_mut().set_resolver(resolver);
        self
    }
}

impl Default for InterpCtfeEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl CtfeEvaluator for InterpCtfeEvaluator {
    fn evaluate(&mut self, unit: &IrCodeUnit, result_type: &IrType) -> Result<ConstValue, CtfeError> {
        // Every evaluation runs as unit 0, with the local functions of the
        // unit it is for; the last evaluation's were other functions under
        // the same identities.
        self.interp.forget_unit_bodies(0);
        // Create environment with module registry if available (for cross-module CTFE).
        let mut env = match &self.module_registry {
            Some(registry) => ScriptEnvironment::with_module_registry(registry.C()),
            None => ScriptEnvironment::new(),
        };

        // Allocate space for the result.
        let result_tydesc = self.interp.tydesc_table_mut().get_or_create(result_type);
        let mut result_buffer = datalove_rt::rust::AlignedBuffer::for_tydesc(unsafe { &*result_tydesc });
        let result_dest = Destination {
            ptr: result_buffer.as_mut_ptr(),
            tydesc: result_tydesc,
        };

        // Allocate space for early return (Result<(), Error>).
        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let mut ret_buffer = datalove_rt::rust::AlignedBuffer::for_tydesc(unsafe { &*ret_tydesc });
        let ret_dest = Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // Execute the unit.
        let completion = self.interp
            .execute_script_unit_in_env(unit, &mut env, ret_dest, Some(result_dest))
            .map_err(|e| CtfeError::InterpError(e.S()))?;

        // Clean up the environment (frames from executed units).
        // This must happen before env is dropped to free any heap allocations.
        env.destroy_live_values(self.interp.runtime_handle());

        let result = match completion {
            UnitCompletion::Normal => {
                // Extract the result value into a ConstValue.
                extract_const_value(result_buffer.as_ptr(), result_type)
            }
            UnitCompletion::EarlyReturn => {
                Err(CtfeError::EarlyReturn(
                    "const expression returned early via ! or ?".S()
                ))
            }
        };

        // Destroy whichever buffer the unit wrote, and only that one: the other
        // holds zeroes, which for a result or an option is not a valid tag.
        // The result goes after extract_const_value, which reads it; the early
        // return holds an error that checked operations (like /!) wrote.
        let (written, written_tydesc) = match completion {
            UnitCompletion::Normal => (result_buffer.as_mut_ptr(), result_tydesc),
            UnitCompletion::EarlyReturn => (ret_buffer.as_mut_ptr(), ret_tydesc),
        };
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.interp.runtime_handle(),
                AnyOwned::new(written),
                &*written_tydesc,
            );
        }

        result
    }

    fn set_module_registry(&mut self, registry: Arc<ModuleFunctionRegistry>) {
        self.module_registry = Some(registry);
        self.interp.forget_compiled_bodies();
    }
}

/// Alignment of an IR type.
fn align_of_ir_type(ir_type: &IrType) -> u32 {
    datalove_datafun_ir::layout::layout_of(ir_type).align
}

/// Size of an IR type.
fn size_of_ir_type(ir_type: &IrType) -> u32 {
    datalove_datafun_ir::layout::layout_of(ir_type).size
}

/// Compute field offsets for a tuple type.
fn compute_tuple_field_offsets(field_types: &[IrType]) -> Vec<u32> {
    datalove_datafun_ir::layout::aggregate_field_offsets(field_types)
}

/// Extract a ConstValue from raw memory.
///
/// Reads bytes from the given pointer and converts to a ConstValue based on type.
fn extract_const_value(ptr: *const u8, ir_type: &IrType) -> Result<ConstValue, CtfeError> {
    unsafe {
        match ir_type {
            IrType::Unit => Ok(ConstValue::Unit),
            IrType::Bool => {
                let val = *(ptr as *const bool);
                Ok(ConstValue::Bool(val))
            }
            IrType::U8 => {
                let val = *(ptr as *const u8);
                Ok(ConstValue::U8(val))
            }
            IrType::U16 => {
                let val = *(ptr as *const u16);
                Ok(ConstValue::U16(val))
            }
            IrType::U32 => {
                let val = *(ptr as *const u32);
                Ok(ConstValue::U32(val))
            }
            IrType::U64 => {
                let val = *(ptr as *const u64);
                Ok(ConstValue::U64(val))
            }
            IrType::I8 => {
                let val = *(ptr as *const i8);
                Ok(ConstValue::I8(val))
            }
            IrType::I16 => {
                let val = *(ptr as *const i16);
                Ok(ConstValue::I16(val))
            }
            IrType::I32 => {
                let val = *(ptr as *const i32);
                Ok(ConstValue::I32(val))
            }
            IrType::I64 => {
                let val = *(ptr as *const i64);
                Ok(ConstValue::I64(val))
            }
            IrType::Index => {
                let val = *(ptr as *const datalove_rtdt::IndexRepr);
                Ok(ConstValue::Index(val))
            }
            IrType::Offset => {
                let val = *(ptr as *const datalove_rtdt::OffsetRepr);
                Ok(ConstValue::Offset(val))
            }
            IrType::F32 => {
                let val = *(ptr as *const f32);
                Ok(ConstValue::F32(ConstF32(val)))
            }
            IrType::F64 => {
                let val = *(ptr as *const f64);
                Ok(ConstValue::F64(ConstF64(val)))
            }
            IrType::Int => {
                // Bigint is stored as: data pointer, size_and_sign, capacity.
                let int_ptr = ptr as *const datalove_rtdt::Int;
                let int_val = &*int_ptr;

                let num_limbs = int_val.size_and_sign.unsigned_abs() as usize;
                let negative = int_val.size_and_sign < 0;

                let limbs = if num_limbs == 0 {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(int_val.data, num_limbs).to_vec()
                };

                Ok(ConstValue::Int { limbs, negative })
            }
            IrType::Option(inner) => {
                // Option layout: tag (u8) at offset 0, payload at aligned offset.
                let tag = *(ptr as *const u8);
                match tag {
                    1 => Ok(ConstValue::OptionNone),
                    2 => {
                        // Compute payload offset based on inner type alignment.
                        let inner_align = align_of_ir_type(inner);
                        let payload_offset = datalove_rtdt::layout::option_payload_offset(inner_align);
                        let payload_ptr = ptr.add(payload_offset as usize);
                        let inner_value = extract_const_value(payload_ptr, inner)?;
                        Ok(ConstValue::OptionSome(Box::new(inner_value)))
                    }
                    _ => Err(CtfeError::InterpError(format!(
                        "invalid option tag: {}", tag
                    ))),
                }
            }
            IrType::Tuple(field_types) => {
                // Compute field offsets using standard struct layout algorithm.
                let field_offsets = compute_tuple_field_offsets(field_types);

                // Extract each field value.
                let mut values = Vec::with_capacity(field_types.len());
                for (i, field_type) in field_types.iter().enumerate() {
                    let field_ptr = ptr.add(field_offsets[i] as usize);
                    let field_value = extract_const_value(field_ptr, field_type)?;
                    values.push(field_value);
                }

                Ok(ConstValue::Tuple(values))
            }
            IrType::Struct(fields) => {
                // Struct layout is the same as tuple layout, just with named fields.
                let field_types: Vec<_> = fields.iter().map(|(_, ty)| ty.C()).collect();
                let field_offsets = compute_tuple_field_offsets(&field_types);

                // Extract each named field value.
                let mut values = Vec::with_capacity(fields.len());
                for (i, (name, field_type)) in fields.iter().enumerate() {
                    let field_ptr = ptr.add(field_offsets[i] as usize);
                    let field_value = extract_const_value(field_ptr, field_type)?;
                    values.push((name.C(), field_value));
                }

                Ok(ConstValue::Struct(values))
            }
            IrType::Enum(variants) => {
                // Enum layout: discriminant (u32) at offset 0, payload at aligned offset.
                let discriminant = *(ptr as *const u32);
                let variant_index = discriminant as usize;

                if variant_index >= variants.len() {
                    return Err(CtfeError::InterpError(format!(
                        "invalid enum discriminant: {} (max {})",
                        discriminant,
                        variants.len() - 1
                    )));
                }

                let (variant_name, payload_type) = &variants[variant_index];

                if let Some(payload_ty) = payload_type {
                    let payload_offset = datalove_datafun_ir::layout::enum_payload_offset(payload_ty);
                    let payload_ptr = ptr.add(payload_offset as usize);
                    let payload_value = extract_const_value(payload_ptr, payload_ty)?;
                    Ok(ConstValue::Enum {
                        variant: variant_name.C(),
                        payload: Some(Box::new(payload_value)),
                    })
                } else {
                    Ok(ConstValue::Enum {
                        variant: variant_name.C(),
                        payload: None,
                    })
                }
            }
            IrType::Atom(name) => {
                // Atom is zero-sized.
                Ok(ConstValue::Enum {
                    variant: name.C(),
                    payload: None,
                })
            }
            IrType::Term(name, payload_ty) => {
                // Term has same layout as payload.
                let payload_value = extract_const_value(ptr, payload_ty)?;
                Ok(ConstValue::Enum {
                    variant: name.C(),
                    payload: Some(Box::new(payload_value)),
                })
            }
            IrType::String => {
                // String layout: data ptr, size, capacity.
                Ok(ConstValue::String((*(ptr as *const datalove_rtdt::String)).as_str().S()))
            }
            IrType::Result(inner) => {
                // Result layout: tag (u8) at offset 0, payload at aligned offset.
                let tag = *(ptr as *const u8);
                let payload_offset = datalove_datafun_ir::layout::result_payload_offset(inner);
                let payload_ptr = ptr.add(payload_offset as usize);

                match tag {
                    1 => {
                        // Ok variant - extract inner value.
                        let inner_value = extract_const_value(payload_ptr, inner)?;
                        Ok(ConstValue::ResultOk(Box::new(inner_value)))
                    }
                    2 => {
                        // Err variant - extract Error.
                        let error_ptr = payload_ptr as *const datalove_rtdt::Error;
                        let error_val = &*error_ptr;

                        // Error contains a boxed value with its own tydesc.
                        let inner_tydesc_ptr = error_val.tydesc();
                        if inner_tydesc_ptr.is_null() {
                            // Null error - return Unit as placeholder.
                            Ok(ConstValue::ResultErr(Box::new(ConstValue::Error {
                                payload_type: Box::new(IrType::Unit),
                                value: Box::new(ConstValue::Unit),
                            })))
                        } else {
                            let inner_tydesc = datalove_rtdt::TyDescRef::from_ptr(inner_tydesc_ptr);
                            let inner_value_ptr = error_val.value_ptr();

                            // Extract the inner error value based on its actual type.
                            let inner_ir_type = ir_type_from_tydesc(inner_tydesc)?;
                            let inner_value = extract_const_value(inner_value_ptr, &inner_ir_type)?;
                            Ok(ConstValue::ResultErr(Box::new(ConstValue::Error {
                                payload_type: Box::new(inner_ir_type),
                                value: Box::new(inner_value),
                            })))
                        }
                    }
                    _ => Err(CtfeError::InterpError(format!(
                        "invalid result tag: {}", tag
                    ))),
                }
            }
            IrType::List(element_type) => {
                // List layout: data ptr, size, capacity.
                let list_ptr = ptr as *const datalove_rtdt::List;
                let list_val = &*list_ptr;

                let size = list_val.size.0 as usize;
                if size == 0 || list_val.data.is_null() {
                    Ok(ConstValue::List(Vec::new()))
                } else {
                    // The elements sit one element's size apart, as the
                    // runtime stores them; one of no size takes no room.
                    let stride = size_of_ir_type(element_type) as usize;

                    let mut elements = Vec::with_capacity(size);
                    for i in 0..size {
                        let element_ptr = list_val.data.add(i * stride);
                        let element_value = extract_const_value(element_ptr, element_type)?;
                        elements.push(element_value);
                    }
                    Ok(ConstValue::List(elements))
                }
            }
            IrType::Set(element_type) => {
                // Set layout: root (*const SetNode), len.
                let set_ptr = ptr as *const datalove_rtdt::Set;
                let set_val = &*set_ptr;

                let len = set_val.len.0 as usize;
                if len == 0 || set_val.root.is_null() {
                    Ok(ConstValue::Set(Vec::new()))
                } else {
                    // Extract elements by walking the B-tree leaf nodes.
                    let mut elements = Vec::with_capacity(len);
                    extract_set_elements(set_val.root, element_type, &mut elements)?;
                    Ok(ConstValue::Set(elements))
                }
            }
            IrType::Map(key_type, value_type) => {
                // Map layout: root (*const MapNode), len.
                let map_ptr = ptr as *const datalove_rtdt::Map;
                let map_val = &*map_ptr;

                let len = map_val.len.0 as usize;
                if len == 0 || map_val.root.is_null() {
                    Ok(ConstValue::Map(Vec::new()))
                } else {
                    // Extract key-value pairs by walking the B-tree leaf nodes.
                    let mut entries = Vec::with_capacity(len);
                    extract_map_entries(map_val.root, key_type, value_type, &mut entries)?;
                    Ok(ConstValue::Map(entries))
                }
            }
            IrType::Table(columns) => {
                // Table layout: len, capacity, data.
                let table_ptr = ptr as *const datalove_rtdt::Table;
                let table_val = &*table_ptr;

                let num_rows = table_val.len.0 as usize;
                if num_rows == 0 || table_val.data.is_null() {
                    let column_names: Vec<String> = columns.iter().map(|(name, _)| name.C()).collect();
                    Ok(ConstValue::Table { columns: column_names, rows: Vec::new() })
                } else {
                    // Extract rows from columnar storage.
                    let column_names: Vec<String> = columns.iter().map(|(name, _)| name.C()).collect();
                    let column_types: Vec<&IrType> = columns.iter().map(|(_, ty)| ty.as_ref()).collect();
                    let mut rows = Vec::with_capacity(num_rows);

                    // Each column a run of its elements, where the runtime's
                    // layout puts it.
                    let sized: Vec<(u32, u32)> = column_types.iter()
                        .map(|ty| (size_of_ir_type(ty), align_of_ir_type(ty)))
                        .collect();
                    let capacity = table_val.capacity.0;
                    let col_offsets: Vec<usize> = (0..sized.len())
                        .map(|col| datalove_rtdt::layout::table_column_offset_of(&sized, col, capacity))
                        .collect();

                    for row_idx in 0..num_rows {
                        let mut row_values = Vec::with_capacity(columns.len());
                        for (col_idx, col_ty) in column_types.iter().enumerate() {
                            let col_offset = col_offsets[col_idx];
                            let elem_ptr = table_val.data.add(col_offset + row_idx * sized[col_idx].0 as usize);
                            let elem_value = extract_const_value(elem_ptr, col_ty)?;
                            row_values.push(elem_value);
                        }
                        rows.push(row_values);
                    }

                    Ok(ConstValue::Table { columns: column_names, rows })
                }
            }
            IrType::Tensor(element_type, rank) => {
                // A tensor keeps its elements in one run, with a shape beside
                // them saying how they are grouped. Both are read, because the
                // elements alone do not say the shape and the rank alone does
                // not say the extents.
                let tensor = &*(ptr as *const datalove_rtdt::Tensor);
                let rank = *rank as usize;
                let extents: Vec<usize> = (0..rank)
                    .map(|i| (*tensor.shape.add(i)).as_usize())
                    .collect();
                let shape: Vec<u32> = extents.iter().map(|extent| *extent as u32).collect();

                let count = datalove_rtdt::tensor_element_count(&extents);
                let mut elements = Vec::with_capacity(count);
                if count > 0 {
                    let element_size = size_of_ir_type(element_type) as usize;
                    let start = tensor.offset_elems.0 as usize;
                    for i in 0..count {
                        let element_ptr = tensor.ptr_base.add((start + i) * element_size);
                        elements.push(extract_const_value(element_ptr, element_type)?);
                    }
                }
                Ok(ConstValue::Tensor { shape, elements })
            }

            // A `data` or an `error` is a value under a descriptor it carries
            // itself, so what is read back is decided by that rather than by
            // the type written down. Borrowed rather than unpacked, because
            // what the const evaluator is looking at still belongs to the
            // frame it was computed in and is destroyed with it.
            IrType::Data => {
                let (value_ptr, tydesc) = borrow_packed(ptr)?;
                let inner_ir_type = ir_type_from_tydesc(tydesc)?;
                let inner = extract_const_value(value_ptr, &inner_ir_type)?;
                Ok(ConstValue::Data {
                    payload_type: Box::new(inner_ir_type),
                    value: Box::new(inner),
                })
            }
            IrType::Error => {
                let error_val = &*(ptr as *const datalove_rtdt::Error);
                let tydesc_ptr = error_val.tydesc();
                if tydesc_ptr.is_null() {
                    return Ok(ConstValue::Error {
                        payload_type: Box::new(IrType::Unit),
                        value: Box::new(ConstValue::Unit),
                    });
                }
                let tydesc = datalove_rtdt::TyDescRef::from_ptr(tydesc_ptr);
                let inner_ir_type = ir_type_from_tydesc(tydesc)?;
                let inner = extract_const_value(error_val.value_ptr(), &inner_ir_type)?;
                Ok(ConstValue::Error {
                    payload_type: Box::new(inner_ir_type),
                    value: Box::new(inner),
                })
            }

            _ => Err(CtfeError::UnsupportedType(format!("{}", ir_type))),
        }
    }
}

/// Read what a `data` holds, without taking it.
///
/// The three ways one is packed -- on the heap, inline with a descriptor, and
/// inline with only a tag -- are all read the same way here, which is what
/// `data_borrow` is for. The scratch is where a value packed into the words is
/// written so that there is something to point at; it outlives the read
/// because the caller extracts before returning.
unsafe fn borrow_packed(
    ptr: *const u8,
) -> Result<(*const u8, datalove_rtdt::TyDescRef<'static>), CtfeError> {
    unsafe {
        let mut scratch = datalove_rt::rust::DataScratch::default();
        let mut value_ptr: *const u8 = std::ptr::null();
        let mut tydesc_ptr: *const datalove_rtdt::TyDesc = std::ptr::null();
        let status = datalove_rt::c::dtlv_rti_data_borrow(
            ptr,
            scratch.as_mut_ptr(),
            &mut value_ptr as *mut *const u8,
            &mut tydesc_ptr as *mut *const datalove_rtdt::TyDesc,
        );
        if status != datalove_rt::c::RtStatus::Ok || tydesc_ptr.is_null() {
            return Err(CtfeError::UnsupportedType(
                "a `data` with nothing in it".S()));
        }
        // The value may point into `scratch`, which is this frame's. Reading it
        // out here keeps it alive for exactly as long as the caller needs.
        let tydesc = datalove_rtdt::TyDescRef::from_ptr(tydesc_ptr);
        let size = tydesc.size() as usize;
        if value_ptr == scratch.as_ptr() {
            // Packed into the words: copy to a leaked buffer, since the value
            // has to outlive this scratch. A const is evaluated once.
            let mut owned = datalove_rt::rust::AlignedBuffer::for_tydesc(tydesc.as_ref());
            std::ptr::copy_nonoverlapping(value_ptr, owned.as_mut_ptr(), size);
            let leaked = owned.as_ptr();
            std::mem::forget(owned);
            return Ok((leaked, tydesc));
        }
        Ok((value_ptr, tydesc))
    }
}

/// Convert a runtime type descriptor back to an IrType.
///
/// This is used for extracting dynamically-typed values like Error contents.
fn ir_type_from_tydesc(tydesc: datalove_rtdt::TyDescRef) -> Result<IrType, CtfeError> {
    use datalove_rtdt::TyTag;

    match tydesc.type_tag() {
        TyTag::Bool => Ok(IrType::Bool),
        TyTag::U8 => Ok(IrType::U8),
        TyTag::I8 => Ok(IrType::I8),
        TyTag::U16 => Ok(IrType::U16),
        TyTag::I16 => Ok(IrType::I16),
        TyTag::U32 => Ok(IrType::U32),
        TyTag::I32 => Ok(IrType::I32),
        TyTag::U64 => Ok(IrType::U64),
        TyTag::I64 => Ok(IrType::I64),
        TyTag::Index => Ok(IrType::Index),
        TyTag::Offset => Ok(IrType::Offset),
        TyTag::F32 => Ok(IrType::F32),
        TyTag::F64 => Ok(IrType::F64),
        TyTag::Int => Ok(IrType::Int),
        TyTag::String => Ok(IrType::String),
        TyTag::Data => Ok(IrType::Data),
        TyTag::Error => Ok(IrType::Error),
        TyTag::Param => Ok(IrType::Param(unsafe { tydesc.as_ref().type_info.param.index })),

        // A descriptor carries the whole of a type, so a structured one is
        // read back by walking it. This used to stop at the scalars, under a
        // note that a complex type would want recursion; it is wanted, because
        // this is how the payload of an `error` is read and an error carries
        // whatever it was built from. A const of a result whose error held a
        // tuple could not be evaluated at all.
        TyTag::Option => Ok(IrType::Option(Box::new(
            ir_type_from_tydesc(tydesc.option_inner_ty())?))),
        TyTag::Result => Ok(IrType::Result(Box::new(
            ir_type_from_tydesc(tydesc.result_ok_ty())?))),
        TyTag::List => Ok(IrType::List(Box::new(
            ir_type_from_tydesc(tydesc.list_element_ty())?))),
        TyTag::Set => Ok(IrType::Set(Box::new(
            ir_type_from_tydesc(tydesc.set_element_ty())?))),
        TyTag::Map => Ok(IrType::Map(
            Box::new(ir_type_from_tydesc(tydesc.map_key_ty())?),
            Box::new(ir_type_from_tydesc(tydesc.map_value_ty())?),
        )),
        TyTag::Tensor => Ok(IrType::Tensor(
            Box::new(ir_type_from_tydesc(tydesc.tensor_element_ty())?),
            tydesc.tensor_rank(),
        )),
        TyTag::Tuple => {
            let mut fields = Vec::new();
            for field in tydesc.iter_tuple_fields() {
                fields.push(ir_type_from_tydesc(field.tydesc())?);
            }
            Ok(IrType::Tuple(fields))
        }
        TyTag::Struct => {
            let mut fields = Vec::new();
            for field in tydesc.iter_struct_fields() {
                fields.push((field.name().S(), ir_type_from_tydesc(field.tydesc())?));
            }
            Ok(IrType::Struct(fields))
        }
        TyTag::Table => {
            let mut columns = Vec::new();
            for column in tydesc.table_column_tydescs() {
                columns.push((
                    column.name().S(),
                    Box::new(ir_type_from_tydesc(column.tydesc())?),
                ));
            }
            Ok(IrType::Table(columns))
        }
        TyTag::Atom => Ok(IrType::Atom(tydesc.atom_info().0.S())),
        TyTag::Term => {
            let (name, payload) = tydesc.term_info();
            Ok(IrType::Term(name.S(), Box::new(ir_type_from_tydesc(payload)?)))
        }
        TyTag::Enum => {
            let info = tydesc.enum_info();
            let mut variants = Vec::new();
            for i in 0..info.num_variants() as usize {
                let variant = info.variant(i).ok_or_else(|| CtfeError::UnsupportedType(
                    "enum variant out of bounds".S()))?;
                let payload = match variant.payload() {
                    core::option::Option::Some(ty) => Some(ir_type_from_tydesc(ty)?),
                    core::option::Option::None => None,
                };
                variants.push((variant.name().S(), payload));
            }
            Ok(IrType::Enum(variants))
        }
    }
}

/// Extract all elements from a Set B-tree by walking leaf nodes.
///
/// The B+tree stores elements in leaf nodes, linked via next_leaf pointers.
unsafe fn extract_set_elements(
    root: *const datalove_rtdt::SetNode,
    element_type: &IrType,
    out: &mut Vec<ConstValue>,
) -> Result<(), CtfeError> {
    if root.is_null() {
        return Ok(());
    }

    // The node layouts are the runtime's own, as a map's are.
    let element_size = size_of_ir_type(element_type);
    let element_align = align_of_ir_type(element_type);
    let internal = datalove_rtdt::layout::set_internal_node_layout(element_size, element_align);
    let leaf = datalove_rtdt::layout::set_leaf_node_layout(element_size, element_align);

    // Find the leftmost leaf by descending through internal nodes.
    let mut node = root;
    loop {
        let tag = unsafe { *(node as *const u8) };
        match tag {
            1 => {
                // Internal node - descend to first child.
                let child_ptr = unsafe {
                    let ptr = (node as *const u8).add(internal.child_ptrs_offset as usize) as *const *const datalove_rtdt::SetNode;
                    *ptr
                };
                node = child_ptr;
            }
            2 => {
                // Leaf node - we've found the leftmost leaf.
                break;
            }
            _ => {
                return Err(CtfeError::InterpError(format!("invalid SetNodeTag: {}", tag)));
            }
        }
    }

    // Now walk through all leaf nodes via next_leaf pointers.
    loop {
        let tag = unsafe { *(node as *const u8) };
        if tag != 2 {
            return Err(CtfeError::InterpError("expected leaf node".S()));
        }

        let len = unsafe { *((node as *const u8).add(4) as *const u32) };

        let next_leaf_offset = leaf.next_leaf_offset as usize;
        for i in 0..len as usize {
            let elem_ptr = unsafe { (node as *const u8).add(leaf.keys_offset as usize + i * element_size as usize) };
            let elem_value = extract_const_value(elem_ptr, element_type)?;
            out.push(elem_value);
        }

        // Move to next leaf.
        let next_leaf = unsafe {
            let ptr = (node as *const u8).add(next_leaf_offset) as *const *const datalove_rtdt::SetNode;
            *ptr
        };
        if next_leaf.is_null() {
            break;
        }
        node = next_leaf;
    }

    Ok(())
}

/// Extract all key-value pairs from a Map B-tree by walking leaf nodes.
///
/// The B+tree stores key-value pairs in leaf nodes, linked via next_leaf pointers.
unsafe fn extract_map_entries(
    root: *const datalove_rtdt::MapNode,
    key_type: &IrType,
    value_type: &IrType,
    out: &mut Vec<(ConstValue, ConstValue)>,
) -> Result<(), CtfeError> {
    if root.is_null() {
        return Ok(());
    }

    // The node layouts are the runtime's own, so the walk reads what the
    // runtime wrote: a key of no size, `{}`, takes no room, and the values
    // follow the keys' offset directly.
    let key_size = size_of_ir_type(key_type);
    let key_align = align_of_ir_type(key_type);
    let value_size = size_of_ir_type(value_type);
    let value_align = align_of_ir_type(value_type);
    let internal = datalove_rtdt::layout::map_internal_node_layout(key_size, key_align);
    let leaf = datalove_rtdt::layout::map_leaf_node_layout(key_size, key_align, value_size, value_align);

    // Find the leftmost leaf by descending through internal nodes.
    let mut node = root;
    loop {
        let tag = unsafe { *(node as *const u8) };
        match tag {
            1 => {
                // Internal node - descend to first child.
                let child_ptr = unsafe {
                    let ptr = (node as *const u8).add(internal.child_ptrs_offset as usize) as *const *const datalove_rtdt::MapNode;
                    *ptr
                };
                node = child_ptr;
            }
            2 => {
                // Leaf node - we've found the leftmost leaf.
                break;
            }
            _ => {
                return Err(CtfeError::InterpError(format!("invalid MapNodeTag: {}", tag)));
            }
        }
    }

    // Now walk through all leaf nodes via next_leaf pointers.
    loop {
        let tag = unsafe { *(node as *const u8) };
        if tag != 2 {
            return Err(CtfeError::InterpError("expected leaf node".S()));
        }

        let len = unsafe { *((node as *const u8).add(4) as *const u32) };

        let next_leaf_offset = leaf.next_leaf_offset as usize;
        for i in 0..len as usize {
            let key_ptr = unsafe { (node as *const u8).add(leaf.keys_offset as usize + i * key_size as usize) };
            let value_ptr = unsafe { (node as *const u8).add(leaf.values_offset as usize + i * value_size as usize) };
            let key_value = extract_const_value(key_ptr, key_type)?;
            let value_value = extract_const_value(value_ptr, value_type)?;
            out.push((key_value, value_value));
        }

        // Move to next leaf.
        let next_leaf = unsafe {
            let ptr = (node as *const u8).add(next_leaf_offset) as *const *const datalove_rtdt::MapNode;
            *ptr
        };
        if next_leaf.is_null() {
            break;
        }
        node = next_leaf;
    }

    Ok(())
}
