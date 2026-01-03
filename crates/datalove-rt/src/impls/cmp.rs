use rmx::prelude::*;

use datalove_rtdt as rtdt;

/// Float equality policy for comparison operations.
#[derive(Copy, Clone)]
enum FloatEqPolicy {
    /// IEEE equality with zero coalescing: NaN != NaN; +0.0 == -0.0.
    Ieee,
    /// Bitwise equality: all bit patterns distinct.
    Bitwise,
}

/// Float ordering policy for comparison operations.
#[derive(Copy, Clone)]
enum FloatOrdPolicy {
    /// Datalove ordering: NaN total order with zero coalescing.
    /// -NaN < -Infinity < -numbers < -0.0 == +0.0 < +numbers < +Infinity < +NaN
    Datalove,
    /// IEEE 754-2008 total order: distinguishes -0.0 from +0.0.
    /// -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN
    Total,
}

pub unsafe fn eq(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::c::RtEq {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        let td_a = rtdt::TyDescRef::from_ptr(tydesc_a);
        let td_b = rtdt::TyDescRef::from_ptr(tydesc_b);
        if !eq_tydesc(td_a, td_b) {
            return crate::c::RtEq::Error;
        }
        if eq_value(value_a, value_b, td_a, FloatEqPolicy::Ieee) {
            crate::c::RtEq::Equals
        } else {
            crate::c::RtEq::NotEquals
        }
    }
}

pub unsafe fn eq_unique(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::c::RtEq {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        let td_a = rtdt::TyDescRef::from_ptr(tydesc_a);
        let td_b = rtdt::TyDescRef::from_ptr(tydesc_b);
        if !eq_tydesc(td_a, td_b) {
            return crate::c::RtEq::Error;
        }
        if eq_value(value_a, value_b, td_a, FloatEqPolicy::Bitwise) {
            crate::c::RtEq::Equals
        } else {
            crate::c::RtEq::NotEquals
        }
    }
}

pub unsafe fn cmp(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::c::RtOrdering {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        let td_a = rtdt::TyDescRef::from_ptr(tydesc_a);
        let td_b = rtdt::TyDescRef::from_ptr(tydesc_b);
        if !eq_tydesc(td_a, td_b) {
            return crate::c::RtOrdering::Error;
        }
        cmp_value(value_a, value_b, td_a, FloatOrdPolicy::Datalove)
    }
}

pub unsafe fn cmp_total(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::c::RtOrdering {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        let td_a = rtdt::TyDescRef::from_ptr(tydesc_a);
        let td_b = rtdt::TyDescRef::from_ptr(tydesc_b);
        if !eq_tydesc(td_a, td_b) {
            return crate::c::RtOrdering::Error;
        }
        cmp_value(value_a, value_b, td_a, FloatOrdPolicy::Total)
    }
}

/// Compare two type descriptors for structural equality.
///
/// Fixme we can probably use pointer equality, but need
/// to make sure tydescs are fully deduplicated.
fn eq_tydesc(
    td_a: rtdt::TyDescRef,
    td_b: rtdt::TyDescRef,
) -> bool {
    // Type tags must match.
    if td_a.type_tag() != td_b.type_tag() {
        return false;
    }

    // Size and alignment should match for same type.
    if td_a.size() != td_b.size() || td_a.align() != td_b.align() {
        return false;
    }

    // For composite types, we need to compare the structure recursively.
    match td_a.type_tag() {
            rtdt::TyTag::Bool | rtdt::TyTag::U8 | rtdt::TyTag::I8 |
            rtdt::TyTag::U16 | rtdt::TyTag::I16 | rtdt::TyTag::U32 | rtdt::TyTag::I32 |
            rtdt::TyTag::F32 | rtdt::TyTag::U64 | rtdt::TyTag::I64 | rtdt::TyTag::F64 |
            rtdt::TyTag::Int | rtdt::TyTag::String | rtdt::TyTag::Data | rtdt::TyTag::Error => {
                true
            }
            rtdt::TyTag::Tuple => {
                let info_a = td_a.tuple_info();
                let info_b = td_b.tuple_info();

                if info_a.num_fields() != info_b.num_fields() {
                    return false;
                }

                for i in 0..info_a.num_fields() as usize {
                    let field_a = info_a.field(i).unwrap();
                    let field_b = info_b.field(i).unwrap();

                    if field_a.offset() != field_b.offset() {
                        return false;
                    }
                    if !eq_tydesc(field_a.tydesc(), field_b.tydesc()) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Struct => {
                let info_a = td_a.struct_info();
                let info_b = td_b.struct_info();

                if info_a.num_fields() != info_b.num_fields() {
                    return false;
                }

                for i in 0..info_a.num_fields() as usize {
                    let field_a = info_a.field(i).unwrap();
                    let field_b = info_b.field(i).unwrap();

                    // Compare field names.
                    if field_a.name() != field_b.name() {
                        return false;
                    }

                    if field_a.offset() != field_b.offset() {
                        return false;
                    }
                    if !eq_tydesc(field_a.tydesc(), field_b.tydesc()) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Enum => {
                let info_a = td_a.enum_info();
                let info_b = td_b.enum_info();

                if info_a.num_variants() != info_b.num_variants() {
                    return false;
                }

                for i in 0..info_a.num_variants() as usize {
                    let variant_a = info_a.variant(i).unwrap();
                    let variant_b = info_b.variant(i).unwrap();

                    // Compare variant names.
                    if variant_a.name() != variant_b.name() {
                        return false;
                    }

                    if variant_a.offset() != variant_b.offset() {
                        return false;
                    }

                    // Compare payload types.
                    match (variant_a.payload(), variant_b.payload()) {
                        (core::option::Option::None, core::option::Option::None) => {}
                        (core::option::Option::Some(payload_ty_a), core::option::Option::Some(payload_ty_b)) => {
                            if !eq_tydesc(payload_ty_a, payload_ty_b) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                true
            }
            rtdt::TyTag::List => {
                let elem_ty_a = td_a.list_element_ty();
                let elem_ty_b = td_b.list_element_ty();
                eq_tydesc(elem_ty_a, elem_ty_b)
            }
            rtdt::TyTag::Map => {
                let key_ty_a = td_a.map_key_ty();
                let key_ty_b = td_b.map_key_ty();
                let value_ty_a = td_a.map_value_ty();
                let value_ty_b = td_b.map_value_ty();
                eq_tydesc(key_ty_a, key_ty_b) &&
                    eq_tydesc(value_ty_a, value_ty_b)
            }
            rtdt::TyTag::Set => {
                let elem_ty_a = td_a.set_element_ty();
                let elem_ty_b = td_b.set_element_ty();
                eq_tydesc(elem_ty_a, elem_ty_b)
            }
            rtdt::TyTag::Tensor => {
                let elem_ty_a = td_a.tensor_element_ty();
                let elem_ty_b = td_b.tensor_element_ty();
                let rank_a = td_a.tensor_rank();
                let rank_b = td_b.tensor_rank();

                rank_a == rank_b && eq_tydesc(elem_ty_a, elem_ty_b)
            }
            rtdt::TyTag::Option => {
                let inner_ty_a = td_a.option_inner_ty();
                let inner_ty_b = td_b.option_inner_ty();
                eq_tydesc(inner_ty_a, inner_ty_b)
            }
        rtdt::TyTag::Result => {
            let ok_ty_a = td_a.result_ok_ty();
            let ok_ty_b = td_b.result_ok_ty();
            eq_tydesc(ok_ty_a, ok_ty_b)
        }
    }
}

/// Compare two values for equality given a shared type descriptor.
///
/// Assumes both values have the same type (tydesc has already been checked).
unsafe fn eq_value(
    value_a: *const u8,
    value_b: *const u8,
    tydesc: rtdt::TyDescRef,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        let td = tydesc;

        match td.type_tag() {
            rtdt::TyTag::Bool => {
                let a = *value_a;
                let b = *value_b;
                a == b
            }
            rtdt::TyTag::U32 => {
                let a = *(value_a as *const u32);
                let b = *(value_b as *const u32);
                a == b
            }
            rtdt::TyTag::U8 => {
                let a = *value_a;
                let b = *value_b;
                a == b
            }
            rtdt::TyTag::I8 => {
                let a = *(value_a as *const i8);
                let b = *(value_b as *const i8);
                a == b
            }
            rtdt::TyTag::U16 => {
                let a = *(value_a as *const u16);
                let b = *(value_b as *const u16);
                a == b
            }
            rtdt::TyTag::I16 => {
                let a = *(value_a as *const i16);
                let b = *(value_b as *const i16);
                a == b
            }
            rtdt::TyTag::I32 => {
                let a = *(value_a as *const i32);
                let b = *(value_b as *const i32);
                a == b
            }
            rtdt::TyTag::U64 => {
                let a = *(value_a as *const u64);
                let b = *(value_b as *const u64);
                a == b
            }
            rtdt::TyTag::I64 => {
                let a = *(value_a as *const i64);
                let b = *(value_b as *const i64);
                a == b
            }
            rtdt::TyTag::F32 => {
                let a = *(value_a as *const f32);
                let b = *(value_b as *const f32);
                match float_policy {
                    FloatEqPolicy::Ieee => {
                        // IEEE equality: NaN != NaN; +0.0 == -0.0
                        // Same as Rust.
                        a == b
                    }
                    FloatEqPolicy::Bitwise => {
                        // Bitwise equality: all float bit patterns are distinct.
                        a.to_bits() == b.to_bits()
                    }
                }
            }
            rtdt::TyTag::F64 => {
                let a = *(value_a as *const f64);
                let b = *(value_b as *const f64);
                match float_policy {
                    FloatEqPolicy::Ieee => {
                        // IEEE equality: NaN != NaN; +0.0 == -0.0
                        a == b
                    }
                    FloatEqPolicy::Bitwise => {
                        // Bitwise equality: all float bit patterns are distinct.
                        a.to_bits() == b.to_bits()
                    }
                }
            }
            rtdt::TyTag::Int => {
                let int_a = &*(value_a as *const rtdt::Int);
                let int_b = &*(value_b as *const rtdt::Int);

                // Compare size and sign.
                if int_a.size_and_sign != int_b.size_and_sign {
                    return false;
                }

                // Handle zero case (no limbs, data can be null).
                let num_limbs = int_a.size_and_sign.abs() as usize;
                if num_limbs == 0 {
                    return true;
                }

                // Compare limbs.
                let limbs_a = std::slice::from_raw_parts(int_a.data, num_limbs);
                let limbs_b = std::slice::from_raw_parts(int_b.data, num_limbs);
                limbs_a == limbs_b
            }
            rtdt::TyTag::String => {
                let str_a = &*(value_a as *const rtdt::String);
                let str_b = &*(value_b as *const rtdt::String);

                // Handle empty strings (size 0, data can be null).
                if str_a.size == 0 && str_b.size == 0 {
                    return true;
                }
                if str_a.size != str_b.size {
                    return false;
                }

                let bytes_a = std::slice::from_raw_parts(str_a.data, str_a.size as usize);
                let bytes_b = std::slice::from_raw_parts(str_b.data, str_b.size as usize);
                bytes_a == bytes_b
            }
            rtdt::TyTag::Tuple => {
                for field in td.iter_tuple_fields() {
                    let field_a = value_a.add(field.offset() as usize);
                    let field_b = value_b.add(field.offset() as usize);
                    if !eq_value(field_a, field_b, field.tydesc(), float_policy) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Struct => {
                for field in td.iter_struct_fields() {
                    let field_a = value_a.add(field.offset() as usize);
                    let field_b = value_b.add(field.offset() as usize);
                    if !eq_value(field_a, field_b, field.tydesc(), float_policy) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Enum => {
                let enum_info = td.enum_info();

                // Compare discriminants.
                let disc_a = *(value_a as *const u32);
                let disc_b = *(value_b as *const u32);
                if disc_a != disc_b {
                    return false;
                }

                // Compare payload if present.
                if disc_a < enum_info.num_variants() {
                    if let core::option::Option::Some(variant) = enum_info.variant(disc_a as usize) {
                        if let core::option::Option::Some(payload_ty) = variant.payload() {
                            let payload_a = value_a.add(variant.offset() as usize);
                            let payload_b = value_b.add(variant.offset() as usize);
                            return eq_value(payload_a, payload_b, payload_ty, float_policy);
                        }
                    }
                }
                true
            }
            rtdt::TyTag::List => {
                let list_a = &*(value_a as *const rtdt::List);
                let list_b = &*(value_b as *const rtdt::List);

                // Compare sizes.
                if list_a.size != list_b.size {
                    return false;
                }

                // Compare elements.
                let element_ty = td.list_element_ty();
                let element_size = element_ty.size() as usize;

                for i in 0..list_a.size as usize {
                    let elem_a = list_a.data.add(i * element_size);
                    let elem_b = list_b.data.add(i * element_size);
                    if !eq_value(elem_a, elem_b, element_ty, float_policy) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Option => {
                let option_a = &*(value_a as *const rtdt::Option);
                let option_b = &*(value_b as *const rtdt::Option);

                // Compare tags first.
                if option_a.tag != option_b.tag {
                    return false;
                }

                // If both are Some, compare inner values.
                if option_a.tag == rtdt::OptionTag::Some {
                    let inner_ty = td.option_inner_ty();
                    let layout = rtdt::layout::compute_option_layout(tydesc);
                    let payload_a = value_a.add(layout.payload_offset as usize);
                    let payload_b = value_b.add(layout.payload_offset as usize);
                    return eq_value(payload_a, payload_b, inner_ty, float_policy);
                }

                true
            }
            rtdt::TyTag::Result => {
                let result_a = &*(value_a as *const rtdt::Result);
                let result_b = &*(value_b as *const rtdt::Result);

                // Compare tags first.
                if result_a.tag != result_b.tag {
                    return false;
                }

                let ok_ty = td.result_ok_ty();
                let layout = rtdt::layout::compute_result_layout(tydesc);
                let payload_a = value_a.add(layout.payload_offset as usize);
                let payload_b = value_b.add(layout.payload_offset as usize);

                match result_a.tag {
                    rtdt::ResultTag::Ok => {
                        eq_value(payload_a, payload_b, ok_ty, float_policy)
                    }
                    rtdt::ResultTag::Err => {
                        // Compare Error values.
                        // Error has same structure as Data: (tydesc, value_ptr).
                        let err_a = &*(payload_a as *const rtdt::Error);
                        let err_b = &*(payload_b as *const rtdt::Error);
                        let tydesc_a = err_a.tydesc();
                        let tydesc_b = err_b.tydesc();
                        if tydesc_a != tydesc_b {
                            return false;
                        }
                        let value_a = err_a.value_ptr();
                        let value_b = err_b.value_ptr();
                        eq_value(value_a, value_b, rtdt::TyDescRef::from_ptr(tydesc_a), float_policy)
                    }
                }
            }
            rtdt::TyTag::Map => {
                let map_a = &*(value_a as *const rtdt::Map);
                let map_b = &*(value_b as *const rtdt::Map);

                // Compare lengths first (fast path).
                if map_a.len != map_b.len {
                    return false;
                }

                // Both empty.
                if map_a.len == 0 {
                    return true;
                }

                let key_ty = td.map_key_ty();
                let value_ty = td.map_value_ty();

                // Walk both trees in sorted order using leaf chains.
                eq_map_trees(
                    map_a.root as *mut rtdt::MapNode,
                    map_b.root as *mut rtdt::MapNode,
                    key_ty,
                    value_ty,
                    float_policy,
                )
            }
            rtdt::TyTag::Set => {
                let set_a = &*(value_a as *const rtdt::Set);
                let set_b = &*(value_b as *const rtdt::Set);

                // Compare lengths first (fast path).
                if set_a.len != set_b.len {
                    return false;
                }

                // Both empty.
                if set_a.len == 0 {
                    return true;
                }

                let element_ty = td.set_element_ty();

                // Walk both trees in sorted order using leaf chains.
                eq_set_trees(
                    set_a.root as *mut rtdt::SetNode,
                    set_b.root as *mut rtdt::SetNode,
                    element_ty,
                    float_policy,
                )
            }
            rtdt::TyTag::Tensor => {
                let tensor_a = &*(value_a as *const rtdt::Tensor);
                let tensor_b = &*(value_b as *const rtdt::Tensor);

                let rank = td.tensor_rank();

                // Compare shapes.
                if rank > 0 {
                    let shape_a = std::slice::from_raw_parts(tensor_a.shape, rank as usize);
                    let shape_b = std::slice::from_raw_parts(tensor_b.shape, rank as usize);

                    if shape_a != shape_b {
                        return false;
                    }

                    // Compute total number of elements.
                    let total_elems = shape_a.iter().product::<u32>();

                    if total_elems == 0 {
                        return true;  // Empty tensors with matching shapes are equal.
                    }

                    let element_ty = td.tensor_element_ty();
                    let element_size = element_ty.size() as usize;
                    let strides_a = std::slice::from_raw_parts(tensor_a.strides, rank as usize);
                    let strides_b = std::slice::from_raw_parts(tensor_b.strides, rank as usize);

                    // Iterate through all multi-dimensional indices.
                    let mut indices = vec![0u32; rank as usize];
                    for _ in 0..total_elems {
                        // Compute linear offset for tensor_a.
                        let mut offset_a = tensor_a.offset_elems;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_a += idx * strides_a[i];
                        }
                        let elem_a = tensor_a.ptr_base.add((offset_a as usize) * element_size);

                        // Compute linear offset for tensor_b.
                        let mut offset_b = tensor_b.offset_elems;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_b += idx * strides_b[i];
                        }
                        let elem_b = tensor_b.ptr_base.add((offset_b as usize) * element_size);

                        // Compare elements.
                        if !eq_value(elem_a, elem_b, element_ty, float_policy) {
                            return false;
                        }

                        // Increment indices (like odometer).
                        let mut carry = 1;
                        for i in (0..rank as usize).rev() {
                            if carry == 0 {
                                break;
                            }
                            indices[i] += carry;
                            if indices[i] >= shape_a[i] {
                                indices[i] = 0;
                                carry = 1;
                            } else {
                                carry = 0;
                            }
                        }
                    }

                    true
                } else {
                    // Rank 0 tensor (scalar).
                    let element_ty = td.tensor_element_ty();
                    let element_size = element_ty.size() as usize;

                    let elem_a = tensor_a.ptr_base.add((tensor_a.offset_elems as usize) * element_size);
                    let elem_b = tensor_b.ptr_base.add((tensor_b.offset_elems as usize) * element_size);

                    eq_value(elem_a, elem_b, element_ty, float_policy)
                }
            }
            rtdt::TyTag::Data => {
                // Compare Data values.
                // Data uses a tagged encoding that can store values in three ways.
                let data_a = &*(value_a as *const rtdt::Data);
                let data_b = &*(value_b as *const rtdt::Data);

                // First check if types match.
                let tytag_a = data_a.tytag();
                let tytag_b = data_b.tytag();
                if tytag_a != tytag_b {
                    return false;
                }

                // Same type - compare based on encoding.
                match data_a.tag() {
                    rtdt::anypack::Tag::TwoPointers => {
                        // Heap-allocated values (Int, String, List, etc.).
                        // Recursively compare the inner values.
                        let tydesc_a = data_a.tydesc();
                        let tydesc_b = data_b.tydesc();
                        if tydesc_a != tydesc_b {
                            return false;
                        }
                        let inner_value_a = data_a.value_ptr();
                        let inner_value_b = data_b.value_ptr();
                        eq_value(inner_value_a, inner_value_b, rtdt::TyDescRef::from_ptr(tydesc_a), float_policy)
                    }
                    rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                        // For same type, compare the Data structures field-wise.
                        // This works because each type has a consistent encoding.
                        // Data is repr(C) with two pointer fields: (primary, secondary).
                        if std::ptr::eq(data_a, data_b) {
                            true
                        } else {
                            // Read Data as two usize values and compare.
                            let data_a_bytes = std::ptr::read(data_a);
                            let data_b_bytes = std::ptr::read(data_b);

                            // Compare using transmute to [usize; 2] for consistent equality.
                            let a_words: [usize; 2] = std::mem::transmute(data_a_bytes);
                            let b_words: [usize; 2] = std::mem::transmute(data_b_bytes);

                            a_words == b_words
                        }
                    }
                    _ => {
                        panic!("invalid Data tag: {:?}", data_a.tag());
                    }
                }
            }
            rtdt::TyTag::Error => {
                // Compare Error values.
                // Error uses same encoding as Data.
                let err_a = &*(value_a as *const rtdt::Error);
                let err_b = &*(value_b as *const rtdt::Error);
                let as_data_a = &*(value_a as *const rtdt::Data);
                let as_data_b = &*(value_b as *const rtdt::Data);

                // First check if types match.
                let tytag_a = as_data_a.tytag();
                let tytag_b = as_data_b.tytag();
                if tytag_a != tytag_b {
                    return false;
                }

                // Same type - compare based on encoding.
                match as_data_a.tag() {
                    rtdt::anypack::Tag::TwoPointers => {
                        // Heap-allocated error values.
                        // Recursively compare the inner values.
                        let tydesc_a = err_a.tydesc();
                        let tydesc_b = err_b.tydesc();
                        if tydesc_a != tydesc_b {
                            return false;
                        }
                        let inner_value_a = err_a.value_ptr();
                        let inner_value_b = err_b.value_ptr();
                        eq_value(inner_value_a, inner_value_b, rtdt::TyDescRef::from_ptr(tydesc_a), float_policy)
                    }
                    rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                        // For same type, compare the Error structures field-wise.
                        if std::ptr::eq(err_a, err_b) {
                            true
                        } else {
                            // Read Error as two usize values and compare.
                            let err_a_bytes = std::ptr::read(as_data_a);
                            let err_b_bytes = std::ptr::read(as_data_b);

                            // Compare using transmute to [usize; 2] for consistent equality.
                            let a_words: [usize; 2] = std::mem::transmute(err_a_bytes);
                            let b_words: [usize; 2] = std::mem::transmute(err_b_bytes);

                            a_words == b_words
                        }
                    }
                    _ => {
                        panic!("invalid Error tag: {:?}", as_data_a.tag());
                    }
                }
            }
        }
    }
}

/// Compare two values for ordering given a shared type descriptor.
///
/// Assumes both values have the same type (tydesc has already been checked).
unsafe fn cmp_value(
    value_a: *const u8,
    value_b: *const u8,
    tydesc: rtdt::TyDescRef,
    float_policy: FloatOrdPolicy,
) -> crate::c::RtOrdering {
    unsafe {
        let td = tydesc;

        match td.type_tag() {
            rtdt::TyTag::Bool => {
                let a = *value_a;
                let b = *value_b;
                // false < true
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U32 => {
                let a = *(value_a as *const u32);
                let b = *(value_b as *const u32);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U8 => {
                let a = *value_a;
                let b = *value_b;
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I8 => {
                let a = *(value_a as *const i8);
                let b = *(value_b as *const i8);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U16 => {
                let a = *(value_a as *const u16);
                let b = *(value_b as *const u16);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I16 => {
                let a = *(value_a as *const i16);
                let b = *(value_b as *const i16);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I32 => {
                let a = *(value_a as *const i32);
                let b = *(value_b as *const i32);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U64 => {
                let a = *(value_a as *const u64);
                let b = *(value_b as *const u64);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I64 => {
                let a = *(value_a as *const i64);
                let b = *(value_b as *const i64);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::F32 => {
                let a = *(value_a as *const f32);
                let b = *(value_b as *const f32);
                match float_policy {
                    FloatOrdPolicy::Datalove => {
                        // Datalove ordering: NaN total order; +0.0 == -0.0
                        // -NaN < -Infinity < -numbers < -0.0 == +0.0 < +numbers < +Infinity < +NaN

                        // Rust's partial_cmp gives us -0.0 ==+.0.0, rejects NaN
                        match a.partial_cmp(&b) {
                            Some(std::cmp::Ordering::Less) => crate::c::RtOrdering::Less,
                            Some(std::cmp::Ordering::Greater) => crate::c::RtOrdering::Greater,
                            Some(std::cmp::Ordering::Equal) => crate::c::RtOrdering::Equal,
                            None => {
                                debug_assert!(a.is_nan() || b.is_nan());
                                // Rust's total_cmp gives us the correct NaN ordering.
                                match a.total_cmp(&b) {
                                    std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                    std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                    std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                                }
                            }
                        }
                    }
                    FloatOrdPolicy::Total => {
                        // IEEE 754-2008 total order: distinguishes -0.0 from +0.0.
                        // -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN
                        match a.total_cmp(&b) {
                            std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                            std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                            std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                        }
                    }
                }
            }
            rtdt::TyTag::F64 => {
                let a = *(value_a as *const f64);
                let b = *(value_b as *const f64);
                match float_policy {
                    FloatOrdPolicy::Datalove => {
                        // Datalove ordering: NaN total order; +0.0 == -0.0
                        match a.partial_cmp(&b) {
                            Some(std::cmp::Ordering::Less) => crate::c::RtOrdering::Less,
                            Some(std::cmp::Ordering::Greater) => crate::c::RtOrdering::Greater,
                            Some(std::cmp::Ordering::Equal) => crate::c::RtOrdering::Equal,
                            None => {
                                debug_assert!(a.is_nan() || b.is_nan());
                                match a.total_cmp(&b) {
                                    std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                    std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                    std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                                }
                            }
                        }
                    }
                    FloatOrdPolicy::Total => {
                        // IEEE 754-2008 total order: distinguishes -0.0 from +0.0.
                        match a.total_cmp(&b) {
                            std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                            std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                            std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                        }
                    }
                }
            }
            rtdt::TyTag::Int => {
                match super::int_math::int_cmp_impl(value_a, value_b) {
                    c if c < 0 => crate::c::RtOrdering::Less,
                    c if c > 0 => crate::c::RtOrdering::Greater,
                    _ => crate::c::RtOrdering::Equal,
                }
            }
            rtdt::TyTag::String => {
                let str_a = &*(value_a as *const rtdt::String);
                let str_b = &*(value_b as *const rtdt::String);

                // Handle empty strings (size 0, data can be null).
                if str_a.size == 0 && str_b.size == 0 {
                    return crate::c::RtOrdering::Equal;
                }
                if str_a.size == 0 {
                    return crate::c::RtOrdering::Less;
                }
                if str_b.size == 0 {
                    return crate::c::RtOrdering::Greater;
                }

                let bytes_a = std::slice::from_raw_parts(str_a.data, str_a.size as usize);
                let bytes_b = std::slice::from_raw_parts(str_b.data, str_b.size as usize);

                // Lexicographic comparison.
                match bytes_a.cmp(bytes_b) {
                    std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                    std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                    std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                }
            }
            rtdt::TyTag::Tuple => {
                // Lexicographic ordering by fields.
                for field in td.iter_tuple_fields() {
                    let field_a = value_a.add(field.offset() as usize);
                    let field_b = value_b.add(field.offset() as usize);
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc(), float_policy);
                    match field_cmp {
                        crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                        crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                        crate::c::RtOrdering::Equal => continue,
                        crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                    }
                }
                crate::c::RtOrdering::Equal
            }
            rtdt::TyTag::Struct => {
                // Lexicographic ordering by fields.
                for field in td.iter_struct_fields() {
                    let field_a = value_a.add(field.offset() as usize);
                    let field_b = value_b.add(field.offset() as usize);
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc(), float_policy);
                    match field_cmp {
                        crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                        crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                        crate::c::RtOrdering::Equal => continue,
                        crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                    }
                }
                crate::c::RtOrdering::Equal
            }
            rtdt::TyTag::Enum => {
                let enum_info = td.enum_info();

                // Compare discriminants first.
                let disc_a = *(value_a as *const u32);
                let disc_b = *(value_b as *const u32);

                if disc_a != disc_b {
                    return if disc_a < disc_b {
                        crate::c::RtOrdering::Less
                    } else {
                        crate::c::RtOrdering::Greater
                    };
                }

                // Same variant, compare payload if present.
                if disc_a < enum_info.num_variants() {
                    if let core::option::Option::Some(variant) = enum_info.variant(disc_a as usize) {
                        if let core::option::Option::Some(payload_ty) = variant.payload() {
                            let payload_a = value_a.add(variant.offset() as usize);
                            let payload_b = value_b.add(variant.offset() as usize);
                            return cmp_value(payload_a, payload_b, payload_ty, float_policy);
                        }
                    }
                }
                crate::c::RtOrdering::Equal
            }
            rtdt::TyTag::List => {
                let list_a = &*(value_a as *const rtdt::List);
                let list_b = &*(value_b as *const rtdt::List);

                let element_ty = td.list_element_ty();
                let element_size = element_ty.size() as usize;

                // Lexicographic comparison.
                let min_size = list_a.size.min(list_b.size) as usize;
                for i in 0..min_size {
                    let elem_a = list_a.data.add(i * element_size);
                    let elem_b = list_b.data.add(i * element_size);
                    let elem_cmp = cmp_value(elem_a, elem_b, element_ty, float_policy);
                    match elem_cmp {
                        crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                        crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                        crate::c::RtOrdering::Equal => continue,
                        crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                    }
                }

                // All compared elements equal, compare by length.
                if list_a.size < list_b.size {
                    crate::c::RtOrdering::Less
                } else if list_a.size > list_b.size {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::Option => {
                let option_a = &*(value_a as *const rtdt::Option);
                let option_b = &*(value_b as *const rtdt::Option);

                // None < Some.
                match (option_a.tag, option_b.tag) {
                    (rtdt::OptionTag::None, rtdt::OptionTag::None) => crate::c::RtOrdering::Equal,
                    (rtdt::OptionTag::None, rtdt::OptionTag::Some) => crate::c::RtOrdering::Less,
                    (rtdt::OptionTag::Some, rtdt::OptionTag::None) => crate::c::RtOrdering::Greater,
                    (rtdt::OptionTag::Some, rtdt::OptionTag::Some) => {
                        let inner_ty = td.option_inner_ty();
                        let layout = rtdt::layout::compute_option_layout(tydesc);
                        let payload_a = value_a.add(layout.payload_offset as usize);
                        let payload_b = value_b.add(layout.payload_offset as usize);
                        cmp_value(payload_a, payload_b, inner_ty, float_policy)
                    }
                }
            }
            rtdt::TyTag::Result => {
                let result_a = &*(value_a as *const rtdt::Result);
                let result_b = &*(value_b as *const rtdt::Result);

                // Err < Ok (conventional).
                match (result_a.tag, result_b.tag) {
                    (rtdt::ResultTag::Err, rtdt::ResultTag::Ok) => crate::c::RtOrdering::Less,
                    (rtdt::ResultTag::Ok, rtdt::ResultTag::Err) => crate::c::RtOrdering::Greater,
                    _ => {
                        let ok_ty = td.result_ok_ty();
                        let layout = rtdt::layout::compute_result_layout(tydesc);
                        let payload_a = value_a.add(layout.payload_offset as usize);
                        let payload_b = value_b.add(layout.payload_offset as usize);

                        match result_a.tag {
                            rtdt::ResultTag::Ok => {
                                cmp_value(payload_a, payload_b, ok_ty, float_policy)
                            }
                            rtdt::ResultTag::Err => {
                                // Compare Error values.
                                // Error has same structure as Data: (tydesc, value_ptr).
                                let err_a = &*(payload_a as *const rtdt::Error);
                                let err_b = &*(payload_b as *const rtdt::Error);
                                let tydesc_a = err_a.tydesc();
                                let tydesc_b = err_b.tydesc();
                                if tydesc_a != tydesc_b {
                                    // Different error types - compare tydesc pointers.
                                    return match (tydesc_a as usize).cmp(&(tydesc_b as usize)) {
                                        std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                        std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                        std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                                    };
                                }
                                let value_a = err_a.value_ptr();
                                let value_b = err_b.value_ptr();
                                cmp_value(value_a, value_b, rtdt::TyDescRef::from_ptr(tydesc_a), float_policy)
                            }
                        }
                    }
                }
            }
            rtdt::TyTag::Map => {
                let map_a = &*(value_a as *const rtdt::Map);
                let map_b = &*(value_b as *const rtdt::Map);

                let key_ty = td.map_key_ty();
                let value_ty = td.map_value_ty();

                // Lexicographic comparison by sorted key-value pairs.
                cmp_map_trees(
                    map_a.root as *mut rtdt::MapNode,
                    map_b.root as *mut rtdt::MapNode,
                    key_ty,
                    value_ty,
                    float_policy,
                )
            }
            rtdt::TyTag::Set => {
                let set_a = &*(value_a as *const rtdt::Set);
                let set_b = &*(value_b as *const rtdt::Set);

                let element_ty = td.set_element_ty();

                // Lexicographic comparison by sorted elements.
                cmp_set_trees(
                    set_a.root as *mut rtdt::SetNode,
                    set_b.root as *mut rtdt::SetNode,
                    element_ty,
                    float_policy,
                )
            }
            rtdt::TyTag::Tensor => {
                let tensor_a = &*(value_a as *const rtdt::Tensor);
                let tensor_b = &*(value_b as *const rtdt::Tensor);

                let rank = td.tensor_rank();

                // Compare shapes lexicographically.
                if rank > 0 {
                    let shape_a = std::slice::from_raw_parts(tensor_a.shape, rank as usize);
                    let shape_b = std::slice::from_raw_parts(tensor_b.shape, rank as usize);

                    // Compare shapes dimension by dimension.
                    for i in 0..rank as usize {
                        if shape_a[i] < shape_b[i] {
                            return crate::c::RtOrdering::Less;
                        } else if shape_a[i] > shape_b[i] {
                            return crate::c::RtOrdering::Greater;
                        }
                    }

                    // Shapes are equal, compare elements.
                    let total_elems = shape_a.iter().product::<u32>();

                    if total_elems == 0 {
                        return crate::c::RtOrdering::Equal;  // Empty tensors with matching shapes are equal.
                    }

                    let element_ty = td.tensor_element_ty();
                    let element_size = element_ty.size() as usize;
                    let strides_a = std::slice::from_raw_parts(tensor_a.strides, rank as usize);
                    let strides_b = std::slice::from_raw_parts(tensor_b.strides, rank as usize);

                    // Iterate through all multi-dimensional indices lexicographically.
                    let mut indices = vec![0u32; rank as usize];
                    for _ in 0..total_elems {
                        // Compute linear offset for tensor_a.
                        let mut offset_a = tensor_a.offset_elems;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_a += idx * strides_a[i];
                        }
                        let elem_a = tensor_a.ptr_base.add((offset_a as usize) * element_size);

                        // Compute linear offset for tensor_b.
                        let mut offset_b = tensor_b.offset_elems;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_b += idx * strides_b[i];
                        }
                        let elem_b = tensor_b.ptr_base.add((offset_b as usize) * element_size);

                        // Compare elements.
                        let elem_cmp = cmp_value(elem_a, elem_b, element_ty, float_policy);
                        match elem_cmp {
                            crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                            crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                            crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                            crate::c::RtOrdering::Equal => {
                                // Continue to next element.
                            }
                        }

                        // Increment indices (like odometer).
                        let mut carry = 1;
                        for i in (0..rank as usize).rev() {
                            if carry == 0 {
                                break;
                            }
                            indices[i] += carry;
                            if indices[i] >= shape_a[i] {
                                indices[i] = 0;
                                carry = 1;
                            } else {
                                carry = 0;
                            }
                        }
                    }

                    crate::c::RtOrdering::Equal
                } else {
                    // Rank 0 tensor (scalar).
                    let element_ty = td.tensor_element_ty();
                    let element_size = element_ty.size() as usize;

                    let elem_a = tensor_a.ptr_base.add((tensor_a.offset_elems as usize) * element_size);
                    let elem_b = tensor_b.ptr_base.add((tensor_b.offset_elems as usize) * element_size);

                    cmp_value(elem_a, elem_b, element_ty, float_policy)
                }
            }
            rtdt::TyTag::Data => {
                // Compare Data values.
                // Data uses a tagged encoding that can store values in three ways.
                let data_a = &*(value_a as *const rtdt::Data);
                let data_b = &*(value_b as *const rtdt::Data);

                // First check if types match.
                let tytag_a = data_a.tytag();
                let tytag_b = data_b.tytag();
                if tytag_a != tytag_b {
                    // Different types - order by tytag.
                    return match (tytag_a as u8).cmp(&(tytag_b as u8)) {
                        std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                        std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                        std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                    };
                }

                // Same type - compare based on encoding.
                match data_a.tag() {
                    rtdt::anypack::Tag::TwoPointers => {
                        // Heap-allocated values (Int, String, List, etc.).
                        // Recursively compare the inner values.
                        let tydesc_a = data_a.tydesc();
                        let tydesc_b = data_b.tydesc();
                        if tydesc_a != tydesc_b {
                            // Different tydescs - order by pointer.
                            return match (tydesc_a as usize).cmp(&(tydesc_b as usize)) {
                                std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                            };
                        }
                        let inner_value_a = data_a.value_ptr();
                        let inner_value_b = data_b.value_ptr();
                        cmp_value(inner_value_a, inner_value_b, rtdt::TyDescRef::from_ptr(tydesc_a), float_policy)
                    }
                    rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                        // For same type, compare the Data structures field-wise.
                        // This works because each type has a consistent encoding.
                        // Data is repr(C) with two pointer fields: (primary, secondary).
                        if std::ptr::eq(data_a, data_b) {
                            crate::c::RtOrdering::Equal
                        } else {
                            // Read Data as two usize values and compare.
                            let data_a_bytes = std::ptr::read(data_a);
                            let data_b_bytes = std::ptr::read(data_b);

                            // Compare using transmute to [usize; 2] for consistent ordering.
                            let a_words: [usize; 2] = std::mem::transmute(data_a_bytes);
                            let b_words: [usize; 2] = std::mem::transmute(data_b_bytes);

                            match a_words.cmp(&b_words) {
                                std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                            }
                        }
                    }
                    _ => {
                        panic!("invalid Data tag: {:?}", data_a.tag());
                    }
                }
            }
            rtdt::TyTag::Error => {
                // Compare Error values.
                // Error uses same encoding as Data.
                let err_a = &*(value_a as *const rtdt::Error);
                let err_b = &*(value_b as *const rtdt::Error);
                let as_data_a = &*(value_a as *const rtdt::Data);
                let as_data_b = &*(value_b as *const rtdt::Data);

                // First check if types match.
                let tytag_a = as_data_a.tytag();
                let tytag_b = as_data_b.tytag();
                if tytag_a != tytag_b {
                    // Different types - order by tytag.
                    return match (tytag_a as u8).cmp(&(tytag_b as u8)) {
                        std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                        std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                        std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                    };
                }

                // Same type - compare based on encoding.
                match as_data_a.tag() {
                    rtdt::anypack::Tag::TwoPointers => {
                        // Heap-allocated error values.
                        // Recursively compare the inner values.
                        let tydesc_a = err_a.tydesc();
                        let tydesc_b = err_b.tydesc();
                        if tydesc_a != tydesc_b {
                            // Different tydescs - order by pointer.
                            return match (tydesc_a as usize).cmp(&(tydesc_b as usize)) {
                                std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                            };
                        }
                        let inner_value_a = err_a.value_ptr();
                        let inner_value_b = err_b.value_ptr();
                        cmp_value(inner_value_a, inner_value_b, rtdt::TyDescRef::from_ptr(tydesc_a), float_policy)
                    }
                    rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                        // For same type, compare the Error structures field-wise.
                        // This works because each type has a consistent encoding.
                        // Error has same repr as Data: two pointer fields.
                        if std::ptr::eq(err_a, err_b) {
                            crate::c::RtOrdering::Equal
                        } else {
                            // Read Error as two usize values and compare.
                            let err_a_bytes = std::ptr::read(err_a);
                            let err_b_bytes = std::ptr::read(err_b);

                            // Compare using transmute to [usize; 2] for consistent ordering.
                            let a_words: [usize; 2] = std::mem::transmute(err_a_bytes);
                            let b_words: [usize; 2] = std::mem::transmute(err_b_bytes);

                            match a_words.cmp(&b_words) {
                                std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                                std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                                std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                            }
                        }
                    }
                    _ => {
                        panic!("invalid Error tag: {:?}", as_data_a.tag());
                    }
                }
            }
        }
    }
}

/// Helper to find the leftmost leaf in a map tree.
unsafe fn find_leftmost_map_leaf(mut node: *mut rtdt::MapNode, key_tydesc: rtdt::TyDescRef) -> *mut rtdt::MapNode {
    unsafe {
        loop {
            let tag = read_map_node_tag(node);
            match tag {
                rtdt::MapNodeTag::Leaf => return node,
                rtdt::MapNodeTag::Internal => {
                    let layout = rtdt::layout::compute_map_internal_node_layout(key_tydesc);
                    let children_ptr = (node as *mut u8).add(layout.child_ptrs_offset as usize) as *mut *mut rtdt::MapNode;
                    node = *children_ptr;
                }
            }
        }
    }
}

/// Helper to find the leftmost leaf in a set tree.
unsafe fn find_leftmost_set_leaf(mut node: *mut rtdt::SetNode, key_tydesc: rtdt::TyDescRef) -> *mut rtdt::SetNode {
    unsafe {
        loop {
            let tag = read_set_node_tag(node);
            match tag {
                rtdt::SetNodeTag::Leaf => return node,
                rtdt::SetNodeTag::Internal => {
                    let layout = rtdt::layout::compute_set_internal_node_layout(key_tydesc);
                    let children_ptr = (node as *mut u8).add(layout.child_ptrs_offset as usize) as *mut *mut rtdt::SetNode;
                    node = *children_ptr;
                }
            }
        }
    }
}

/// Read map node tag.
unsafe fn read_map_node_tag(node: *const rtdt::MapNode) -> rtdt::MapNodeTag {
    unsafe {
        let tag_byte = *(node as *const u8);
        match tag_byte {
            1 => rtdt::MapNodeTag::Internal,
            2 => rtdt::MapNodeTag::Leaf,
            _ => panic!("Invalid MapNodeTag: {}", tag_byte),
        }
    }
}

/// Read map node length.
unsafe fn read_map_node_len(node: *const rtdt::MapNode) -> u32 {
    unsafe {
        let len_ptr = (node as *const u8).add(4) as *const u32;
        *len_ptr
    }
}

/// Read set node tag.
unsafe fn read_set_node_tag(node: *const rtdt::SetNode) -> rtdt::SetNodeTag {
    unsafe {
        let tag_byte = *(node as *const u8);
        match tag_byte {
            1 => rtdt::SetNodeTag::Internal,
            2 => rtdt::SetNodeTag::Leaf,
            _ => panic!("Invalid SetNodeTag: {}", tag_byte),
        }
    }
}

/// Read set node length.
unsafe fn read_set_node_len(node: *const rtdt::SetNode) -> u32 {
    unsafe {
        let len_ptr = (node as *const u8).add(4) as *const u32;
        *len_ptr
    }
}

/// Compare two map trees for equality by walking leaf chains.
unsafe fn eq_map_trees(
    root_a: *mut rtdt::MapNode,
    root_b: *mut rtdt::MapNode,
    key_tydesc: rtdt::TyDescRef,
    value_tydesc: rtdt::TyDescRef,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_map_leaf(root_a, key_tydesc);
        let mut leaf_b = find_leftmost_map_leaf(root_b, key_tydesc);

        let key_size = key_tydesc.size() as usize;
        let value_size = value_tydesc.size() as usize;

        let mut idx_a = 0u32;
        let mut idx_b = 0u32;
        let mut len_a = read_map_node_len(leaf_a);
        let mut len_b = read_map_node_len(leaf_b);

        loop {
            // Check if we've exhausted leaves.
            let exhausted_a = idx_a >= len_a && {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                (*next_ptr).is_null()
            };

            let exhausted_b = idx_b >= len_b && {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                (*next_ptr).is_null()
            };

            if exhausted_a && exhausted_b {
                return true;
            }
            if exhausted_a || exhausted_b {
                return false;
            }

            // Move to next leaf if needed.
            if idx_a >= len_a {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                leaf_a = *next_ptr;
                idx_a = 0;
                len_a = read_map_node_len(leaf_a);
            }

            if idx_b >= len_b {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                leaf_b = *next_ptr;
                idx_b = 0;
                len_b = read_map_node_len(leaf_b);
            }

            // Get key and value pointers.
            let layout_a = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
            let keys_a = (leaf_a as *mut u8).add(layout_a.keys_offset as usize);
            let values_a = (leaf_a as *mut u8).add(layout_a.values_offset as usize);
            let key_a = keys_a.add((idx_a as usize) * key_size);
            let value_a = values_a.add((idx_a as usize) * value_size);

            let layout_b = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
            let keys_b = (leaf_b as *mut u8).add(layout_b.keys_offset as usize);
            let values_b = (leaf_b as *mut u8).add(layout_b.values_offset as usize);
            let key_b = keys_b.add((idx_b as usize) * key_size);
            let value_b = values_b.add((idx_b as usize) * value_size);

            // Compare keys.
            if !eq_value(key_a, key_b, key_tydesc, float_policy) {
                return false;
            }

            // Compare values.
            if !eq_value(value_a, value_b, value_tydesc, float_policy) {
                return false;
            }

            idx_a += 1;
            idx_b += 1;
        }
    }
}

/// Compare two set trees for equality by walking leaf chains.
unsafe fn eq_set_trees(
    root_a: *mut rtdt::SetNode,
    root_b: *mut rtdt::SetNode,
    element_tydesc: rtdt::TyDescRef,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_set_leaf(root_a, element_tydesc);
        let mut leaf_b = find_leftmost_set_leaf(root_b, element_tydesc);

        let element_size = element_tydesc.size() as usize;

        let mut idx_a = 0u32;
        let mut idx_b = 0u32;
        let mut len_a = read_set_node_len(leaf_a);
        let mut len_b = read_set_node_len(leaf_b);

        loop {
            // Check if we've exhausted leaves.
            let exhausted_a = idx_a >= len_a && {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                (*next_ptr).is_null()
            };

            let exhausted_b = idx_b >= len_b && {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                (*next_ptr).is_null()
            };

            if exhausted_a && exhausted_b {
                return true;
            }
            if exhausted_a || exhausted_b {
                return false;
            }

            // Move to next leaf if needed.
            if idx_a >= len_a {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                leaf_a = *next_ptr;
                idx_a = 0;
                len_a = read_set_node_len(leaf_a);
            }

            if idx_b >= len_b {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                leaf_b = *next_ptr;
                idx_b = 0;
                len_b = read_set_node_len(leaf_b);
            }

            // Get element pointers.
            let layout_a = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
            let elements_a = (leaf_a as *mut u8).add(layout_a.keys_offset as usize);
            let element_a = elements_a.add((idx_a as usize) * element_size);

            let layout_b = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
            let elements_b = (leaf_b as *mut u8).add(layout_b.keys_offset as usize);
            let element_b = elements_b.add((idx_b as usize) * element_size);

            // Compare elements.
            if !eq_value(element_a, element_b, element_tydesc, float_policy) {
                return false;
            }

            idx_a += 1;
            idx_b += 1;
        }
    }
}

/// Compare two map trees lexicographically by walking leaf chains.
unsafe fn cmp_map_trees(
    root_a: *mut rtdt::MapNode,
    root_b: *mut rtdt::MapNode,
    key_tydesc: rtdt::TyDescRef,
    value_tydesc: rtdt::TyDescRef,
    float_policy: FloatOrdPolicy,
) -> crate::c::RtOrdering {
    unsafe {
        // Handle null roots.
        if root_a.is_null() && root_b.is_null() {
            return crate::c::RtOrdering::Equal;
        }
        if root_a.is_null() {
            return crate::c::RtOrdering::Less;
        }
        if root_b.is_null() {
            return crate::c::RtOrdering::Greater;
        }

        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_map_leaf(root_a, key_tydesc);
        let mut leaf_b = find_leftmost_map_leaf(root_b, key_tydesc);

        let key_size = key_tydesc.size() as usize;
        let value_size = value_tydesc.size() as usize;

        let mut idx_a = 0u32;
        let mut idx_b = 0u32;
        let mut len_a = read_map_node_len(leaf_a);
        let mut len_b = read_map_node_len(leaf_b);

        loop {
            // Check if we've exhausted leaves.
            let exhausted_a = idx_a >= len_a && {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                (*next_ptr).is_null()
            };

            let exhausted_b = idx_b >= len_b && {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                (*next_ptr).is_null()
            };

            if exhausted_a && exhausted_b {
                return crate::c::RtOrdering::Equal;
            }
            if exhausted_a {
                return crate::c::RtOrdering::Less;
            }
            if exhausted_b {
                return crate::c::RtOrdering::Greater;
            }

            // Move to next leaf if needed.
            if idx_a >= len_a {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                leaf_a = *next_ptr;
                idx_a = 0;
                len_a = read_map_node_len(leaf_a);
            }

            if idx_b >= len_b {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                leaf_b = *next_ptr;
                idx_b = 0;
                len_b = read_map_node_len(leaf_b);
            }

            // Get key and value pointers.
            let layout_a = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
            let keys_a = (leaf_a as *mut u8).add(layout_a.keys_offset as usize);
            let values_a = (leaf_a as *mut u8).add(layout_a.values_offset as usize);
            let key_a = keys_a.add((idx_a as usize) * key_size);
            let value_a = values_a.add((idx_a as usize) * value_size);

            let layout_b = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
            let keys_b = (leaf_b as *mut u8).add(layout_b.keys_offset as usize);
            let values_b = (leaf_b as *mut u8).add(layout_b.values_offset as usize);
            let key_b = keys_b.add((idx_b as usize) * key_size);
            let value_b = values_b.add((idx_b as usize) * value_size);

            // Compare keys first.
            let key_cmp = cmp_value(key_a, key_b, key_tydesc, float_policy);
            match key_cmp {
                crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                crate::c::RtOrdering::Equal => {
                    // Keys equal, compare values.
                    let value_cmp = cmp_value(value_a, value_b, value_tydesc, float_policy);
                    match value_cmp {
                        crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                        crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                        crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                        crate::c::RtOrdering::Equal => {
                            // This pair equal, continue to next.
                            idx_a += 1;
                            idx_b += 1;
                        }
                    }
                }
            }
        }
    }
}

/// Compare two set trees lexicographically by walking leaf chains.
unsafe fn cmp_set_trees(
    root_a: *mut rtdt::SetNode,
    root_b: *mut rtdt::SetNode,
    element_tydesc: rtdt::TyDescRef,
    float_policy: FloatOrdPolicy,
) -> crate::c::RtOrdering {
    unsafe {
        // Handle null roots.
        if root_a.is_null() && root_b.is_null() {
            return crate::c::RtOrdering::Equal;
        }
        if root_a.is_null() {
            return crate::c::RtOrdering::Less;
        }
        if root_b.is_null() {
            return crate::c::RtOrdering::Greater;
        }

        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_set_leaf(root_a, element_tydesc);
        let mut leaf_b = find_leftmost_set_leaf(root_b, element_tydesc);

        let element_size = element_tydesc.size() as usize;

        let mut idx_a = 0u32;
        let mut idx_b = 0u32;
        let mut len_a = read_set_node_len(leaf_a);
        let mut len_b = read_set_node_len(leaf_b);

        loop {
            // Check if we've exhausted leaves.
            let exhausted_a = idx_a >= len_a && {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                (*next_ptr).is_null()
            };

            let exhausted_b = idx_b >= len_b && {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                (*next_ptr).is_null()
            };

            if exhausted_a && exhausted_b {
                return crate::c::RtOrdering::Equal;
            }
            if exhausted_a {
                return crate::c::RtOrdering::Less;
            }
            if exhausted_b {
                return crate::c::RtOrdering::Greater;
            }

            // Move to next leaf if needed.
            if idx_a >= len_a {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                leaf_a = *next_ptr;
                idx_a = 0;
                len_a = read_set_node_len(leaf_a);
            }

            if idx_b >= len_b {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                leaf_b = *next_ptr;
                idx_b = 0;
                len_b = read_set_node_len(leaf_b);
            }

            // Get element pointers.
            let layout_a = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
            let elements_a = (leaf_a as *mut u8).add(layout_a.keys_offset as usize);
            let element_a = elements_a.add((idx_a as usize) * element_size);

            let layout_b = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
            let elements_b = (leaf_b as *mut u8).add(layout_b.keys_offset as usize);
            let element_b = elements_b.add((idx_b as usize) * element_size);

            // Compare elements.
            let elem_cmp = cmp_value(element_a, element_b, element_tydesc, float_policy);
            match elem_cmp {
                crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                crate::c::RtOrdering::Equal => {
                    // This element equal, continue to next.
                    idx_a += 1;
                    idx_b += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create TyDesc instances for testing.
    fn make_tydesc(type_tag: rtdt::TyTag) -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag,
            size: match type_tag {
                rtdt::TyTag::U8 | rtdt::TyTag::I8 => 1,
                rtdt::TyTag::U16 | rtdt::TyTag::I16 => 2,
                rtdt::TyTag::U32 | rtdt::TyTag::I32 | rtdt::TyTag::F32 => 4,
                rtdt::TyTag::U64 | rtdt::TyTag::I64 | rtdt::TyTag::F64 => 8,
                _ => 0,
            },
            align: match type_tag {
                rtdt::TyTag::U8 | rtdt::TyTag::I8 => 1,
                rtdt::TyTag::U16 | rtdt::TyTag::I16 => 2,
                rtdt::TyTag::U32 | rtdt::TyTag::I32 | rtdt::TyTag::F32 => 4,
                rtdt::TyTag::U64 | rtdt::TyTag::I64 | rtdt::TyTag::F64 => 8,
                _ => 0,
            },
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    #[test]
    fn test_u8_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::U8);
        let a: u8 = 42;
        let b: u8 = 42;
        let c: u8 = 99;

        unsafe {
            let result = eq(
                &a as *const u8,
                &tydesc,
                &b as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const u8,
                &tydesc,
                &c as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_i8_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::I8);
        let a: i8 = -42;
        let b: i8 = -42;
        let c: i8 = 42;

        unsafe {
            let result = eq(
                &a as *const i8 as *const u8,
                &tydesc,
                &b as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const i8 as *const u8,
                &tydesc,
                &c as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_u16_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::U16);
        let a: u16 = 1000;
        let b: u16 = 1000;
        let c: u16 = 2000;

        unsafe {
            let result = eq(
                &a as *const u16 as *const u8,
                &tydesc,
                &b as *const u16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const u16 as *const u8,
                &tydesc,
                &c as *const u16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_i16_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::I16);
        let a: i16 = -1000;
        let b: i16 = -1000;
        let c: i16 = 1000;

        unsafe {
            let result = eq(
                &a as *const i16 as *const u8,
                &tydesc,
                &b as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const i16 as *const u8,
                &tydesc,
                &c as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_i32_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::I32);
        let a: i32 = -1000000;
        let b: i32 = -1000000;
        let c: i32 = 1000000;

        unsafe {
            let result = eq(
                &a as *const i32 as *const u8,
                &tydesc,
                &b as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const i32 as *const u8,
                &tydesc,
                &c as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_u64_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::U64);
        let a: u64 = 123456789012345;
        let b: u64 = 123456789012345;
        let c: u64 = 987654321098765;

        unsafe {
            let result = eq(
                &a as *const u64 as *const u8,
                &tydesc,
                &b as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const u64 as *const u8,
                &tydesc,
                &c as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_i64_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::I64);
        let a: i64 = -123456789012345;
        let b: i64 = -123456789012345;
        let c: i64 = 123456789012345;

        unsafe {
            let result = eq(
                &a as *const i64 as *const u8,
                &tydesc,
                &b as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const i64 as *const u8,
                &tydesc,
                &c as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_f64_eq() {
        let tydesc = make_tydesc(rtdt::TyTag::F64);
        let a: f64 = 3.14159265358979;
        let b: f64 = 3.14159265358979;
        let c: f64 = 2.71828182845905;

        unsafe {
            let result = eq(
                &a as *const f64 as *const u8,
                &tydesc,
                &b as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::Equals);

            let result = eq(
                &a as *const f64 as *const u8,
                &tydesc,
                &c as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtEq::NotEquals);
        }
    }

    #[test]
    fn test_u8_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::U8);
        let a: u8 = 10;
        let b: u8 = 20;
        let c: u8 = 10;

        unsafe {
            let result = cmp(
                &a as *const u8,
                &tydesc,
                &b as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const u8,
                &tydesc,
                &a as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const u8,
                &tydesc,
                &c as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_i8_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::I8);
        let a: i8 = -50;
        let b: i8 = 50;
        let c: i8 = -50;

        unsafe {
            let result = cmp(
                &a as *const i8 as *const u8,
                &tydesc,
                &b as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const i8 as *const u8,
                &tydesc,
                &a as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const i8 as *const u8,
                &tydesc,
                &c as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_i16_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::I16);
        let a: i16 = -1000;
        let b: i16 = 1000;
        let c: i16 = -1000;

        unsafe {
            let result = cmp(
                &a as *const i16 as *const u8,
                &tydesc,
                &b as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const i16 as *const u8,
                &tydesc,
                &a as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const i16 as *const u8,
                &tydesc,
                &c as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_i32_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::I32);
        let a: i32 = -1000000;
        let b: i32 = 1000000;
        let c: i32 = -1000000;

        unsafe {
            let result = cmp(
                &a as *const i32 as *const u8,
                &tydesc,
                &b as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const i32 as *const u8,
                &tydesc,
                &a as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const i32 as *const u8,
                &tydesc,
                &c as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_i64_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::I64);
        let a: i64 = -123456789012345;
        let b: i64 = 123456789012345;
        let c: i64 = -123456789012345;

        unsafe {
            let result = cmp(
                &a as *const i64 as *const u8,
                &tydesc,
                &b as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const i64 as *const u8,
                &tydesc,
                &a as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const i64 as *const u8,
                &tydesc,
                &c as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_u64_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::U64);
        let a: u64 = 100;
        let b: u64 = 200;
        let c: u64 = 100;

        unsafe {
            let result = cmp(
                &a as *const u64 as *const u8,
                &tydesc,
                &b as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const u64 as *const u8,
                &tydesc,
                &a as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const u64 as *const u8,
                &tydesc,
                &c as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_f64_cmp() {
        let tydesc = make_tydesc(rtdt::TyTag::F64);
        let a: f64 = 1.5;
        let b: f64 = 2.5;
        let c: f64 = 1.5;

        unsafe {
            let result = cmp(
                &a as *const f64 as *const u8,
                &tydesc,
                &b as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &b as *const f64 as *const u8,
                &tydesc,
                &a as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &a as *const f64 as *const u8,
                &tydesc,
                &c as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Equal);
        }
    }

    #[test]
    fn test_i8_boundaries() {
        let tydesc = make_tydesc(rtdt::TyTag::I8);
        let min: i8 = i8::MIN;
        let max: i8 = i8::MAX;
        let zero: i8 = 0;

        unsafe {
            let result = cmp(
                &min as *const i8 as *const u8,
                &tydesc,
                &zero as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &max as *const i8 as *const u8,
                &tydesc,
                &zero as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &min as *const i8 as *const u8,
                &tydesc,
                &max as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);
        }
    }

    #[test]
    fn test_i32_boundaries() {
        let tydesc = make_tydesc(rtdt::TyTag::I32);
        let min: i32 = i32::MIN;
        let max: i32 = i32::MAX;
        let zero: i32 = 0;

        unsafe {
            let result = cmp(
                &min as *const i32 as *const u8,
                &tydesc,
                &zero as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &max as *const i32 as *const u8,
                &tydesc,
                &zero as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &min as *const i32 as *const u8,
                &tydesc,
                &max as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);
        }
    }

    #[test]
    fn test_i64_boundaries() {
        let tydesc = make_tydesc(rtdt::TyTag::I64);
        let min: i64 = i64::MIN;
        let max: i64 = i64::MAX;
        let zero: i64 = 0;

        unsafe {
            let result = cmp(
                &min as *const i64 as *const u8,
                &tydesc,
                &zero as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp(
                &max as *const i64 as *const u8,
                &tydesc,
                &zero as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp(
                &min as *const i64 as *const u8,
                &tydesc,
                &max as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);
        }
    }
}

