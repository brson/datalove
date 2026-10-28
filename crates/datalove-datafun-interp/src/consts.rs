//! Constants: written into a value's place, and kept once in a pool for a
//! static const read by reference.

use super::*;
use datalove_rtdt::{AnyMut, AnyOut, AnyOwned};

impl IrInterpreter {
    /// The address of the pool's copy of a static const of type `ty`,
    /// building it the first time.
    pub fn static_const(&mut self, value: &std::sync::Arc<ConstValue>, ty: &IrType) -> *const u8 {
        let tydesc = self.tydesc_table.get_or_create(ty);
        self.static_const_with_tydesc(value, tydesc)
    }

    pub(super) fn static_const_with_tydesc(
        &mut self,
        value: &std::sync::Arc<ConstValue>,
        tydesc: *const rtdt::TyDesc,
    ) -> *const u8 {
        let identity = std::sync::Arc::as_ptr(value) as usize;
        if let Some(&ptr) = self.static_pool.by_identity.get(&identity) {
            return ptr;
        }
        let key = (tydesc as usize, datalove_datafun_ir::SharedConst(std::sync::Arc::clone(value)));
        let ptr = match self.static_pool.by_value.get(&key) {
            Some(&ptr) => ptr,
            None => {
                let mut storage = datalove_rt::rust::AlignedBuffer::for_tydesc(unsafe { &*tydesc });
                let ptr = storage.as_mut_ptr();
                self.write_const(value, Destination { ptr, tydesc });
                self.static_pool.entries.push((storage, tydesc));
                self.static_pool.by_value.insert(key, ptr);
                ptr
            }
        };
        self.static_pool.held.push(std::sync::Arc::clone(value));
        self.static_pool.by_identity.insert(identity, ptr);
        ptr
    }

    #[inline(always)]
    pub(super) fn write_const(&mut self, value: &ConstValue, dest: Destination) {
        unsafe {
            match value {
                ConstValue::Unit => {}
                ConstValue::Bool(b) => {
                    *(dest.ptr as *mut bool) = *b;
                }
                ConstValue::U8(n) => {
                    *(dest.ptr as *mut u8) = *n;
                }
                ConstValue::U16(n) => {
                    *(dest.ptr as *mut u16) = *n;
                }
                ConstValue::U32(n) => {
                    *(dest.ptr as *mut u32) = *n;
                }
                ConstValue::U64(n) => {
                    *(dest.ptr as *mut u64) = *n;
                }
                ConstValue::I8(n) => {
                    *(dest.ptr as *mut i8) = *n;
                }
                ConstValue::I16(n) => {
                    *(dest.ptr as *mut i16) = *n;
                }
                ConstValue::I32(n) => {
                    *(dest.ptr as *mut i32) = *n;
                }
                ConstValue::I64(n) => {
                    *(dest.ptr as *mut i64) = *n;
                }
                ConstValue::Index(n) => {
                    *(dest.ptr as *mut rtdt::IndexRepr) = *n;
                }
                ConstValue::Offset(n) => {
                    *(dest.ptr as *mut rtdt::OffsetRepr) = *n;
                }
                ConstValue::Int { limbs, negative } => {
                    datalove_rt::c::dtlv_rti_int_from_limbs(
                        self.runtime.handle(),
                        if limbs.is_empty() { std::ptr::null() } else { limbs.as_ptr() },
                        limbs.len() as u32,
                        *negative,
                        AnyOut::new(dest.ptr),
                        &*dest.tydesc,
                    );
                }
                ConstValue::F32(n) => {
                    *(dest.ptr as *mut f32) = n.0;
                }
                ConstValue::F64(n) => {
                    *(dest.ptr as *mut f64) = n.0;
                }
                ConstValue::String(s) => {
                    let bytes_ptr = if s.is_empty() { std::ptr::null() } else { s.as_ptr() };
                    datalove_rt::c::dtlv_rti_string_from_bytes(
                        self.runtime.handle(),
                        bytes_ptr,
                        s.len() as rtdt::IndexRepr,
                        AnyOut::new(dest.ptr),
                        &*dest.tydesc,
                    );
                }
                ConstValue::OptionNone => {
                    // Option layout: tag at offset 0.
                    // None tag = 1.
                    *(dest.ptr as *mut u8) = 1;
                }
                ConstValue::OptionSome(inner) => {
                    // Option layout: tag at offset 0, payload at aligned offset.
                    // Some tag = 2.
                    *(dest.ptr as *mut u8) = 2;

                    // Get inner type from tydesc.
                    let inner_tydesc = (*dest.tydesc).type_info.option.inner_tydesc;
                    let inner_align = (*inner_tydesc).align;
                    let payload_offset = rtdt::layout::option_payload_offset(inner_align);
                    let payload_ptr = dest.ptr.add(payload_offset as usize);
                    let payload_dest = Destination {
                        ptr: payload_ptr,
                        tydesc: inner_tydesc,
                    };
                    self.write_const(inner, payload_dest);
                }
                ConstValue::Tuple(fields) => {
                    // Tuple layout: fields at computed offsets.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let layout = rtdt::layout::compute_tuple_layout(tydesc_ref);
                    let tuple_info = tydesc_ref.tuple_info();
                    for (i, field_value) in fields.iter().enumerate() {
                        let field_offset = layout.field_offsets[i];
                        let field_ref = tuple_info.field(i).expect("tuple field out of bounds");
                        let field_tydesc = field_ref.tydesc().as_ptr();
                        let field_ptr = dest.ptr.add(field_offset as usize);
                        let field_dest = Destination {
                            ptr: field_ptr,
                            tydesc: field_tydesc,
                        };
                        self.write_const(field_value, field_dest);
                    }
                }
                ConstValue::Struct(fields) => {
                    // Struct layout: fields at computed offsets (same as tuple).
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let layout = rtdt::layout::compute_struct_layout(tydesc_ref);
                    let struct_info = tydesc_ref.struct_info();
                    for (i, (_, field_value)) in fields.iter().enumerate() {
                        let field_offset = layout.field_offsets[i];
                        let field_ref = struct_info.field(i).expect("struct field out of bounds");
                        let field_tydesc = field_ref.tydesc().as_ptr();
                        let field_ptr = dest.ptr.add(field_offset as usize);
                        let field_dest = Destination {
                            ptr: field_ptr,
                            tydesc: field_tydesc,
                        };
                        self.write_const(field_value, field_dest);
                    }
                }
                ConstValue::Enum { variant, payload } => {
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    match tydesc_ref.type_tag() {
                        rtdt::TyTag::Atom => {
                            // Atom is zero-sized. Nothing to write.
                        }
                        rtdt::TyTag::Term => {
                            // Term has same layout as payload.
                            if let Some(payload_value) = payload {
                                let (_, payload_tydesc) = tydesc_ref.term_info();
                                let payload_dest = Destination {
                                    ptr: dest.ptr,
                                    tydesc: payload_tydesc.as_ptr(),
                                };
                                self.write_const(payload_value, payload_dest);
                            }
                        }
                        _ => {
                            // Enum layout: discriminant (u32) at offset 0, payload at variant offset.
                            let enum_info = tydesc_ref.enum_info();

                            // Find the variant index by name.
                            let mut variant_index = None;
                            for (i, v) in tydesc_ref.iter_enum_variants().enumerate() {
                                if v.name() == variant {
                                    variant_index = Some(i);
                                    break;
                                }
                            }
                            let variant_idx = variant_index.expect("enum variant not found");

                            // Write discriminant.
                            *(dest.ptr as *mut u32) = variant_idx as u32;

                            // Write payload if present.
                            if let Some(payload_value) = payload {
                                let variant_ref = enum_info.variant(variant_idx).expect("variant out of bounds");
                                let payload_tydesc = variant_ref.payload().expect("variant has no payload");
                                let payload_offset = variant_ref.offset();
                                let payload_ptr = dest.ptr.add(payload_offset as usize);
                                let payload_dest = Destination {
                                    ptr: payload_ptr,
                                    tydesc: payload_tydesc.as_ptr(),
                                };
                                self.write_const(payload_value, payload_dest);
                            }
                        }
                    }
                }
                ConstValue::ResultOk(inner) => {
                    // Result layout: tag (u8) at offset 0, payload at aligned offset.
                    // Ok tag = 1.
                    *(dest.ptr as *mut u8) = 1;

                    // Get inner type from tydesc.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let ok_tydesc = tydesc_ref.result_ok_ty();
                    let payload_offset = rtdt::layout::result_payload_offset(ok_tydesc.align());
                    let payload_ptr = dest.ptr.add(payload_offset as usize);
                    let payload_dest = Destination {
                        ptr: payload_ptr,
                        tydesc: ok_tydesc.as_ptr(),
                    };
                    self.write_const(inner, payload_dest);
                }
                ConstValue::List(elements) => {
                    // Build list from slice of elements.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let element_tydesc = tydesc_ref.list_element_ty().as_ptr();
                    let stride = rtdt::layout::align_up((*element_tydesc).size, (*element_tydesc).align) as usize;

                    // Allocate buffer for ALL elements.
                    let num_elements = elements.len();
                    let mut elements_buffer = datalove_rt::rust::AlignedBuffer::for_elements(&*element_tydesc, num_elements);

                    // Write each element at its offset in the buffer.
                    for (i, element_value) in elements.iter().enumerate() {
                        let element_dest = Destination {
                            ptr: elements_buffer.as_mut_ptr().add(i * stride),
                            tydesc: element_tydesc,
                        };
                        self.write_const(element_value, element_dest);
                    }

                    // Build list from slice.
                    datalove_rt::c::dtlv_rti_list_build_from_slice_local(
                        self.runtime.handle(),
                        AnyOut::new(dest.ptr),
                        &*element_tydesc,
                        elements_buffer.as_mut_ptr(),
                        num_elements as rtdt::IndexRepr,
                    );
                }
                ConstValue::Tensor { shape, elements } => {
                    // The elements go into one run and the runtime takes them
                    // from there, along with the shape saying how they group.
                    // The same way a tensor literal is built.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let element_tydesc = tydesc_ref.tensor_element_ty().as_ptr();
                    let stride = rtdt::layout::align_up((*element_tydesc).size, (*element_tydesc).align) as usize;

                    let mut elements_buffer = datalove_rt::rust::AlignedBuffer::for_elements(&*element_tydesc, elements.len());
                    for (i, element_value) in elements.iter().enumerate() {
                        let element_dest = Destination {
                            ptr: elements_buffer.as_mut_ptr().add(i * stride),
                            tydesc: element_tydesc,
                        };
                        self.write_const(element_value, element_dest);
                    }

                    let shape_values: Vec<u32> = shape.clone();
                    datalove_rt::c::dtlv_rti_tensor_init_local(
                        self.runtime.handle(),
                        elements_buffer.as_mut_ptr(),
                        elements.len() as rtdt::IndexRepr,
                        &*element_tydesc,
                        shape_values.as_ptr(),
                        shape_values.len() as u32,
                        AnyOut::new(dest.ptr),
                        &*dest.tydesc,
                    );
                }
                // Inserted one at a time rather than built in bulk, for the
                // reason `execute_set_new` gives: a constant's elements need
                // not arrive sorted, or once each, and the insert is what
                // puts them in order and lets go of a duplicate.
                ConstValue::Set(elements) => {
                    let element_tydesc = rtdt::TyDescRef::from_ptr(dest.tydesc).set_element_ty().as_ptr();
                    let rt = self.runtime.handle();
                    datalove_rt::c::dtlv_rti_btreeset_create_local(rt, AnyOut::new(dest.ptr), &*dest.tydesc);
                    let mut scratch = datalove_rt::rust::AlignedBuffer::for_tydesc(&*element_tydesc);
                    for element_value in elements {
                        let element = Destination { ptr: scratch.as_mut_ptr(), tydesc: element_tydesc };
                        self.write_const(element_value, element);
                        let mut added = false;
                        datalove_rt::c::dtlv_rti_btreeset_insert_local(
                            rt, AnyMut::new(dest.ptr), &*dest.tydesc, AnyOwned::new(element.ptr), &*element.tydesc,
                            AnyOut::new(&mut added as *mut bool as *mut u8));
                    }
                }
                ConstValue::Map(entries) => {
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let key_tydesc = tydesc_ref.map_key_ty().as_ptr();
                    let value_tydesc = tydesc_ref.map_value_ty().as_ptr();
                    let rt = self.runtime.handle();
                    datalove_rt::c::dtlv_rti_btreemap_create_local(rt, AnyOut::new(dest.ptr), &*dest.tydesc);
                    let mut key_scratch = datalove_rt::rust::AlignedBuffer::for_tydesc(&*key_tydesc);
                    let mut value_scratch = datalove_rt::rust::AlignedBuffer::for_tydesc(&*value_tydesc);
                    for (key_value, val_value) in entries {
                        let key = Destination { ptr: key_scratch.as_mut_ptr(), tydesc: key_tydesc };
                        let val = Destination { ptr: value_scratch.as_mut_ptr(), tydesc: value_tydesc };
                        self.write_const(key_value, key);
                        self.write_const(val_value, val);
                        datalove_rt::c::dtlv_rti_btreemap_insert_local(
                            rt, AnyMut::new(dest.ptr), &*dest.tydesc, AnyOwned::new(key.ptr), &*key.tydesc, AnyOwned::new(val.ptr), &*val.tydesc);
                    }
                }
                ConstValue::Table { columns: _, rows } => {
                    // Build table from rows.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);

                    // Collect column type descriptors.
                    let col_tydescs: Vec<*const rtdt::TyDesc> = tydesc_ref
                        .table_column_tydescs()
                        .map(|col| col.tydesc().as_ptr())
                        .collect();
                    let num_cols = col_tydescs.len();

                    // Compute row tuple layout manually.
                    let mut row_offset = 0u32;
                    let mut row_max_align = 1u32;
                    let mut field_offsets = Vec::with_capacity(num_cols);
                    for &col_tydesc in col_tydescs.iter() {
                        let field_align = (*col_tydesc).align;
                        let field_size = (*col_tydesc).size;
                        row_max_align = row_max_align.max(field_align);
                        row_offset = rtdt::layout::align_up(row_offset, field_align);
                        field_offsets.push(row_offset);
                        row_offset += field_size;
                    }
                    let row_size = rtdt::layout::align_up(row_offset, row_max_align);

                    // Create row tuple type descriptor.
                    let mut row_tuple_fields: Vec<rtdt::TyInfoTupleField> = Vec::with_capacity(num_cols);
                    for (i, &col_tydesc) in col_tydescs.iter().enumerate() {
                        row_tuple_fields.push(rtdt::TyInfoTupleField {
                            offset: field_offsets[i],
                            tydesc: col_tydesc,
                        });
                    }
                    let row_tydesc = rtdt::TyDesc {
                        type_tag: rtdt::TyTag::Tuple,
                        flags: 0,
                        size: row_size,
                        align: row_max_align,
                        type_info: rtdt::TyInfo {
                            tuple: rtdt::TyInfoTuple {
                                num_fields: num_cols as u32,
                                fields: row_tuple_fields.as_ptr(),
                            },
                        },
                    };

                    // Allocate buffer for ALL rows.
                    let num_rows = rows.len();
                    let mut rows_buffer = datalove_rt::rust::AlignedBuffer::for_elements(&row_tydesc, num_rows);

                    // Write each row at its offset in the buffer.
                    for (row_idx, row_values) in rows.iter().enumerate() {
                        let row_base_offset = row_idx * row_size as usize;
                        for (col_idx, col_value) in row_values.iter().enumerate() {
                            let col_offset = row_base_offset + field_offsets[col_idx] as usize;
                            let col_dest = Destination {
                                ptr: rows_buffer.as_mut_ptr().add(col_offset),
                                tydesc: col_tydescs[col_idx],
                            };
                            self.write_const(col_value, col_dest);
                        }
                    }

                    // Build table from rows.
                    datalove_rt::c::dtlv_rti_table_build_from_rows_local(
                        self.runtime.handle(),
                        AnyOut::new(dest.ptr),
                        &*dest.tydesc,
                        rows_buffer.as_mut_ptr(),
                        &row_tydesc,
                        num_rows as rtdt::IndexRepr,
                    );
                }
                ConstValue::ResultErr(inner) => {
                    // Result::Err layout: tag (u8) at offset 0, Error payload at aligned offset.
                    // Err tag = 2.
                    *(dest.ptr as *mut u8) = 2;

                    // Get the Ok type from the Result tydesc to compute payload offset.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let ok_tydesc = tydesc_ref.result_ok_ty();
                    let payload_offset = rtdt::layout::result_payload_offset(ok_tydesc.align());
                    let payload_ptr = dest.ptr.add(payload_offset as usize);

                    // The payload is an Error value. Create the Error tydesc and destination.
                    let error_tydesc = self.tydesc_table.get_or_create(&IrType::Error);
                    let payload_dest = Destination {
                        ptr: payload_ptr,
                        tydesc: error_tydesc,
                    };
                    self.write_const(inner, payload_dest);
                }
                ConstValue::Error { payload_type, value: inner } => {
                    // Error is a boxed wrapper around any value.
                    // The type the value was read back as, rather than one
                    // worked out from the value: an empty collection cannot say
                    // what it holds.
                    let inner_ir_type = (**payload_type).clone();
                    let inner_tydesc = self.tydesc_table.get_or_create(&inner_ir_type);
                    let inner_size = (*inner_tydesc).size as usize;
                    let inner_align = (*inner_tydesc).align as usize;

                    // Allocate temp buffer with proper alignment.
                    let layout = std::alloc::Layout::from_size_align(inner_size.max(1), inner_align.max(1))
                        .expect("invalid layout for Error inner");
                    let inner_buffer = std::alloc::alloc_zeroed(layout);

                    // Write the inner value to the temp buffer.
                    let inner_dest = Destination {
                        ptr: inner_buffer,
                        tydesc: inner_tydesc,
                    };
                    self.write_const(inner, inner_dest);

                    // Now box the inner value into an Error at the destination.
                    datalove_rt::c::dtlv_rti_error_from_local(
                        self.runtime.handle(),
                        AnyOwned::new(inner_buffer),
                        &*inner_tydesc,
                        AnyOut::new(dest.ptr),
                    );

                    // Deallocate temp buffer.
                    std::alloc::dealloc(inner_buffer, layout);
                }
                ConstValue::Data { payload_type, value: inner } => {
                    // Data is a boxed wrapper around any value.
                    // The type the value was read back as, rather than one
                    // worked out from the value: an empty collection cannot say
                    // what it holds.
                    let inner_ir_type = (**payload_type).clone();
                    let inner_tydesc = self.tydesc_table.get_or_create(&inner_ir_type);
                    let inner_size = (*inner_tydesc).size as usize;
                    let inner_align = (*inner_tydesc).align as usize;

                    // Allocate temp buffer with proper alignment.
                    let layout = std::alloc::Layout::from_size_align(inner_size.max(1), inner_align.max(1))
                        .expect("invalid layout for Data inner");
                    let inner_buffer = std::alloc::alloc_zeroed(layout);

                    // Write the inner value to the temp buffer.
                    let inner_dest = Destination {
                        ptr: inner_buffer,
                        tydesc: inner_tydesc,
                    };
                    self.write_const(inner, inner_dest);

                    // Now box the inner value into a Data at the destination.
                    datalove_rt::c::dtlv_rti_data_from_local(
                        self.runtime.handle(),
                        AnyOwned::new(inner_buffer),
                        &*inner_tydesc,
                        AnyOut::new(dest.ptr),
                    );

                    // Deallocate temp buffer.
                    std::alloc::dealloc(inner_buffer, layout);
                }
            }
        }
    }
}
