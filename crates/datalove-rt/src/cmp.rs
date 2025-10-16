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
) -> crate::RtEq {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        if !eq_tydesc(tydesc_a, tydesc_b) {
            return crate::RtEq::Error;
        }
        if eq_value(value_a, value_b, tydesc_a, FloatEqPolicy::Ieee) {
            crate::RtEq::Equals
        } else {
            crate::RtEq::NotEquals
        }
    }
}

pub unsafe fn eq_unique(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::RtEq {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        if !eq_tydesc(tydesc_a, tydesc_b) {
            return crate::RtEq::Error;
        }
        if eq_value(value_a, value_b, tydesc_a, FloatEqPolicy::Bitwise) {
            crate::RtEq::Equals
        } else {
            crate::RtEq::NotEquals
        }
    }
}

pub unsafe fn cmp(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::RtOrdering {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        if !eq_tydesc(tydesc_a, tydesc_b) {
            return crate::RtOrdering::Error;
        }
        cmp_value(value_a, value_b, tydesc_a, FloatOrdPolicy::Datalove)
    }
}

pub unsafe fn cmp_total(
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> crate::RtOrdering {
    assert!(!(value_a.is_null() || value_b.is_null()));
    assert!(!(tydesc_a.is_null() || tydesc_b.is_null()));

    unsafe {
        if !eq_tydesc(tydesc_a, tydesc_b) {
            return crate::RtOrdering::Error;
        }
        cmp_value(value_a, value_b, tydesc_a, FloatOrdPolicy::Total)
    }
}

/// Compare two type descriptors for structural equality.
///
/// Fixme we can probably use pointer equality, but need
/// to make sure tydescs are fully deduplicated.
unsafe fn eq_tydesc(
    tydesc_a: *const rtdt::TyDesc,
    tydesc_b: *const rtdt::TyDesc,
) -> bool {
    unsafe {
        let td_a = &*tydesc_a;
        let td_b = &*tydesc_b;

        // Type tags must match.
        if td_a.type_tag != td_b.type_tag {
            return false;
        }

        // Size and alignment should match for same type.
        if td_a.size != td_b.size || td_a.align != td_b.align {
            return false;
        }

        // For composite types, we need to compare the structure recursively.
        match td_a.type_tag {
            rtdt::TyTag::Bool | rtdt::TyTag::U8 | rtdt::TyTag::I8 |
            rtdt::TyTag::U16 | rtdt::TyTag::I16 | rtdt::TyTag::U32 | rtdt::TyTag::I32 |
            rtdt::TyTag::F32 | rtdt::TyTag::U64 | rtdt::TyTag::I64 | rtdt::TyTag::F64 |
            rtdt::TyTag::Int | rtdt::TyTag::String | rtdt::TyTag::Data | rtdt::TyTag::Error => {
                true
            }
            rtdt::TyTag::Tuple => {
                let info_a = &td_a.type_info.tuple;
                let info_b = &td_b.type_info.tuple;

                if info_a.num_fields != info_b.num_fields {
                    return false;
                }

                let fields_a = std::slice::from_raw_parts(info_a.fields, info_a.num_fields as usize);
                let fields_b = std::slice::from_raw_parts(info_b.fields, info_b.num_fields as usize);

                for i in 0..info_a.num_fields as usize {
                    if fields_a[i].offset != fields_b[i].offset {
                        return false;
                    }
                    if !eq_tydesc(fields_a[i].tydesc, fields_b[i].tydesc) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Struct => {
                let info_a = &td_a.type_info.struct_;
                let info_b = &td_b.type_info.struct_;

                if info_a.num_fields != info_b.num_fields {
                    return false;
                }

                let fields_a = std::slice::from_raw_parts(info_a.fields, info_a.num_fields as usize);
                let fields_b = std::slice::from_raw_parts(info_b.fields, info_b.num_fields as usize);

                for i in 0..info_a.num_fields as usize {
                    // Compare field names.
                    let name_a = std::slice::from_raw_parts(fields_a[i].name, fields_a[i].name_len as usize);
                    let name_b = std::slice::from_raw_parts(fields_b[i].name, fields_b[i].name_len as usize);
                    if name_a != name_b {
                        return false;
                    }

                    if fields_a[i].offset != fields_b[i].offset {
                        return false;
                    }
                    if !eq_tydesc(fields_a[i].tydesc, fields_b[i].tydesc) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Enum => {
                let info_a = &td_a.type_info.enum_;
                let info_b = &td_b.type_info.enum_;

                if info_a.num_variants != info_b.num_variants {
                    return false;
                }

                let variants_a = std::slice::from_raw_parts(info_a.variants, info_a.num_variants as usize);
                let variants_b = std::slice::from_raw_parts(info_b.variants, info_b.num_variants as usize);

                for i in 0..info_a.num_variants as usize {
                    // Compare variant names.
                    let name_a = std::slice::from_raw_parts(variants_a[i].name, variants_a[i].name_len as usize);
                    let name_b = std::slice::from_raw_parts(variants_b[i].name, variants_b[i].name_len as usize);
                    if name_a != name_b {
                        return false;
                    }

                    if variants_a[i].offset != variants_b[i].offset {
                        return false;
                    }

                    // Compare payload types.
                    let payload_a = variants_a[i].payload;
                    let payload_b = variants_b[i].payload;
                    if payload_a.is_null() != payload_b.is_null() {
                        return false;
                    }
                    if !payload_a.is_null() && !eq_tydesc(payload_a, payload_b) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::List => {
                let info_a = &td_a.type_info.list;
                let info_b = &td_b.type_info.list;
                eq_tydesc(info_a.element_tydesc, info_b.element_tydesc)
            }
            rtdt::TyTag::Map => {
                let info_a = &td_a.type_info.map;
                let info_b = &td_b.type_info.map;
                eq_tydesc(info_a.key_tydesc, info_b.key_tydesc) &&
                    eq_tydesc(info_a.value_tydesc, info_b.value_tydesc)
            }
            rtdt::TyTag::Set => {
                let info_a = &td_a.type_info.set;
                let info_b = &td_b.type_info.set;
                eq_tydesc(info_a.element_tydesc, info_b.element_tydesc)
            }
            rtdt::TyTag::Option => {
                let info_a = &td_a.type_info.option;
                let info_b = &td_b.type_info.option;
                eq_tydesc(info_a.inner_tydesc, info_b.inner_tydesc)
            }
            rtdt::TyTag::Result => {
                let info_a = &td_a.type_info.result;
                let info_b = &td_b.type_info.result;
                eq_tydesc(info_a.ok_tydesc, info_b.ok_tydesc)
            }
        }
    }
}

/// Compare two values for equality given a shared type descriptor.
///
/// Assumes both values have the same type (tydesc has already been checked).
unsafe fn eq_value(
    value_a: *const u8,
    value_b: *const u8,
    tydesc: *const rtdt::TyDesc,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        let td = &*tydesc;

        match td.type_tag {
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

                // Compare limbs.
                let num_limbs = int_a.size_and_sign.abs() as usize;
                let limbs_a = std::slice::from_raw_parts(int_a.data, num_limbs);
                let limbs_b = std::slice::from_raw_parts(int_b.data, num_limbs);
                limbs_a == limbs_b
            }
            rtdt::TyTag::String => {
                let str_a = &*(value_a as *const rtdt::String);
                let str_b = &*(value_b as *const rtdt::String);

                let bytes_a = std::slice::from_raw_parts(str_a.data, str_a.size as usize);
                let bytes_b = std::slice::from_raw_parts(str_b.data, str_b.size as usize);
                bytes_a == bytes_b
            }
            rtdt::TyTag::Tuple => {
                let tuple_info = &td.type_info.tuple;
                let fields = std::slice::from_raw_parts(tuple_info.fields, tuple_info.num_fields as usize);

                for field in fields {
                    let field_a = value_a.add(field.offset as usize);
                    let field_b = value_b.add(field.offset as usize);
                    if !eq_value(field_a, field_b, field.tydesc, float_policy) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Struct => {
                let struct_info = &td.type_info.struct_;
                let fields = std::slice::from_raw_parts(struct_info.fields, struct_info.num_fields as usize);

                for field in fields {
                    let field_a = value_a.add(field.offset as usize);
                    let field_b = value_b.add(field.offset as usize);
                    if !eq_value(field_a, field_b, field.tydesc, float_policy) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Enum => {
                let enum_info = &td.type_info.enum_;
                let variants = std::slice::from_raw_parts(enum_info.variants, enum_info.num_variants as usize);

                // Compare discriminants.
                let disc_a = *(value_a as *const u32);
                let disc_b = *(value_b as *const u32);
                if disc_a != disc_b {
                    return false;
                }

                // Compare payload if present.
                if disc_a < enum_info.num_variants {
                    let variant = &variants[disc_a as usize];
                    if !variant.payload.is_null() {
                        let payload_a = value_a.add(variant.offset as usize);
                        let payload_b = value_b.add(variant.offset as usize);
                        return eq_value(payload_a, payload_b, variant.payload, float_policy);
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
                let list_info = &td.type_info.list;
                let element_tydesc = list_info.element_tydesc;
                let element_size = (*element_tydesc).size as usize;

                for i in 0..list_a.size as usize {
                    let elem_a = list_a.data.add(i * element_size);
                    let elem_b = list_b.data.add(i * element_size);
                    if !eq_value(elem_a, elem_b, element_tydesc, float_policy) {
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
                    let option_info = &td.type_info.option;
                    let layout = rtdt::layout::compute_option_layout(tydesc);
                    let payload_a = value_a.add(layout.payload_offset as usize);
                    let payload_b = value_b.add(layout.payload_offset as usize);
                    return eq_value(payload_a, payload_b, option_info.inner_tydesc, float_policy);
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

                let result_info = &td.type_info.result;
                let layout = rtdt::layout::compute_result_layout(tydesc);
                let payload_a = value_a.add(layout.payload_offset as usize);
                let payload_b = value_b.add(layout.payload_offset as usize);

                match result_a.tag {
                    rtdt::ResultTag::Ok => {
                        eq_value(payload_a, payload_b, result_info.ok_tydesc, float_policy)
                    }
                    rtdt::ResultTag::Err => {
                        // Compare Error values.
                        // Error is a dynamic type with primary and secondary fields.
                        // For now, use bitwise comparison of Error struct.
                        let err_a = std::ptr::read(payload_a as *const (u64, u64));
                        let err_b = std::ptr::read(payload_b as *const (u64, u64));
                        err_a == err_b
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

                let map_info = &td.type_info.map;
                let key_tydesc = map_info.key_tydesc;
                let value_tydesc = map_info.value_tydesc;

                // Walk both trees in sorted order using leaf chains.
                eq_map_trees(
                    map_a.root as *mut rtdt::MapNode,
                    map_b.root as *mut rtdt::MapNode,
                    key_tydesc,
                    value_tydesc,
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

                let set_info = &td.type_info.set;
                let element_tydesc = set_info.element_tydesc;

                // Walk both trees in sorted order using leaf chains.
                eq_set_trees(
                    set_a.root as *mut rtdt::SetNode,
                    set_b.root as *mut rtdt::SetNode,
                    element_tydesc,
                    float_policy,
                )
            }
            rtdt::TyTag::Data | rtdt::TyTag::Error => {
                // Not yet implemented.
                unimplemented!("eq_value for {:?}", td.type_tag)
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
    tydesc: *const rtdt::TyDesc,
    float_policy: FloatOrdPolicy,
) -> crate::RtOrdering {
    unsafe {
        let td = &*tydesc;

        match td.type_tag {
            rtdt::TyTag::Bool => {
                let a = *value_a;
                let b = *value_b;
                // false < true
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U32 => {
                let a = *(value_a as *const u32);
                let b = *(value_b as *const u32);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U8 => {
                let a = *value_a;
                let b = *value_b;
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I8 => {
                let a = *(value_a as *const i8);
                let b = *(value_b as *const i8);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U16 => {
                let a = *(value_a as *const u16);
                let b = *(value_b as *const u16);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I16 => {
                let a = *(value_a as *const i16);
                let b = *(value_b as *const i16);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I32 => {
                let a = *(value_a as *const i32);
                let b = *(value_b as *const i32);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::U64 => {
                let a = *(value_a as *const u64);
                let b = *(value_b as *const u64);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::I64 => {
                let a = *(value_a as *const i64);
                let b = *(value_b as *const i64);
                if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
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
                            Some(std::cmp::Ordering::Less) => crate::RtOrdering::Less,
                            Some(std::cmp::Ordering::Greater) => crate::RtOrdering::Greater,
                            Some(std::cmp::Ordering::Equal) => crate::RtOrdering::Equal,
                            None => {
                                debug_assert!(a.is_nan() || b.is_nan());
                                // Rust's total_cmp gives us the correct NaN ordering.
                                match a.total_cmp(&b) {
                                    std::cmp::Ordering::Less => crate::RtOrdering::Less,
                                    std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                                    std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                                }
                            }
                        }
                    }
                    FloatOrdPolicy::Total => {
                        // IEEE 754-2008 total order: distinguishes -0.0 from +0.0.
                        // -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN
                        match a.total_cmp(&b) {
                            std::cmp::Ordering::Less => crate::RtOrdering::Less,
                            std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                            std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
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
                            Some(std::cmp::Ordering::Less) => crate::RtOrdering::Less,
                            Some(std::cmp::Ordering::Greater) => crate::RtOrdering::Greater,
                            Some(std::cmp::Ordering::Equal) => crate::RtOrdering::Equal,
                            None => {
                                debug_assert!(a.is_nan() || b.is_nan());
                                match a.total_cmp(&b) {
                                    std::cmp::Ordering::Less => crate::RtOrdering::Less,
                                    std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                                    std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                                }
                            }
                        }
                    }
                    FloatOrdPolicy::Total => {
                        // IEEE 754-2008 total order: distinguishes -0.0 from +0.0.
                        match a.total_cmp(&b) {
                            std::cmp::Ordering::Less => crate::RtOrdering::Less,
                            std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                            std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                        }
                    }
                }
            }
            rtdt::TyTag::Int => {
                let int_a = &*(value_a as *const rtdt::Int);
                let int_b = &*(value_b as *const rtdt::Int);

                // Compare by sign first.
                let sign_a = int_a.size_and_sign.signum();
                let sign_b = int_b.size_and_sign.signum();

                if sign_a != sign_b {
                    // Different signs: negative < positive.
                    return if sign_a < sign_b {
                        crate::RtOrdering::Less
                    } else {
                        crate::RtOrdering::Greater
                    };
                }

                // Same sign, compare magnitudes.
                let num_limbs_a = int_a.size_and_sign.abs() as usize;
                let num_limbs_b = int_b.size_and_sign.abs() as usize;

                if num_limbs_a != num_limbs_b {
                    // Different number of limbs.
                    let mag_cmp = num_limbs_a.cmp(&num_limbs_b);
                    return if sign_a >= 0 {
                        // Positive: more limbs = greater.
                        match mag_cmp {
                            std::cmp::Ordering::Less => crate::RtOrdering::Less,
                            std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                            std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                        }
                    } else {
                        // Negative: more limbs = smaller (more negative).
                        match mag_cmp {
                            std::cmp::Ordering::Less => crate::RtOrdering::Greater,
                            std::cmp::Ordering::Greater => crate::RtOrdering::Less,
                            std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                        }
                    };
                }

                // Same number of limbs, compare limb by limb from most significant.
                let limbs_a = std::slice::from_raw_parts(int_a.data, num_limbs_a);
                let limbs_b = std::slice::from_raw_parts(int_b.data, num_limbs_b);

                for i in (0..num_limbs_a).rev() {
                    if limbs_a[i] != limbs_b[i] {
                        let limb_cmp = limbs_a[i].cmp(&limbs_b[i]);
                        return if sign_a >= 0 {
                            // Positive numbers.
                            match limb_cmp {
                                std::cmp::Ordering::Less => crate::RtOrdering::Less,
                                std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                                std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                            }
                        } else {
                            // Negative numbers: invert comparison.
                            match limb_cmp {
                                std::cmp::Ordering::Less => crate::RtOrdering::Greater,
                                std::cmp::Ordering::Greater => crate::RtOrdering::Less,
                                std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                            }
                        };
                    }
                }

                crate::RtOrdering::Equal
            }
            rtdt::TyTag::String => {
                let str_a = &*(value_a as *const rtdt::String);
                let str_b = &*(value_b as *const rtdt::String);

                let bytes_a = std::slice::from_raw_parts(str_a.data, str_a.size as usize);
                let bytes_b = std::slice::from_raw_parts(str_b.data, str_b.size as usize);

                // Lexicographic comparison.
                match bytes_a.cmp(bytes_b) {
                    std::cmp::Ordering::Less => crate::RtOrdering::Less,
                    std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                    std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                }
            }
            rtdt::TyTag::Tuple => {
                let tuple_info = &td.type_info.tuple;
                let fields = std::slice::from_raw_parts(tuple_info.fields, tuple_info.num_fields as usize);

                // Lexicographic ordering by fields.
                for field in fields {
                    let field_a = value_a.add(field.offset as usize);
                    let field_b = value_b.add(field.offset as usize);
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc, float_policy);
                    match field_cmp {
                        crate::RtOrdering::Less => return crate::RtOrdering::Less,
                        crate::RtOrdering::Greater => return crate::RtOrdering::Greater,
                        crate::RtOrdering::Equal => continue,
                        crate::RtOrdering::Error => return crate::RtOrdering::Error,
                    }
                }
                crate::RtOrdering::Equal
            }
            rtdt::TyTag::Struct => {
                let struct_info = &td.type_info.struct_;
                let fields = std::slice::from_raw_parts(struct_info.fields, struct_info.num_fields as usize);

                // Lexicographic ordering by fields.
                for field in fields {
                    let field_a = value_a.add(field.offset as usize);
                    let field_b = value_b.add(field.offset as usize);
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc, float_policy);
                    match field_cmp {
                        crate::RtOrdering::Less => return crate::RtOrdering::Less,
                        crate::RtOrdering::Greater => return crate::RtOrdering::Greater,
                        crate::RtOrdering::Equal => continue,
                        crate::RtOrdering::Error => return crate::RtOrdering::Error,
                    }
                }
                crate::RtOrdering::Equal
            }
            rtdt::TyTag::Enum => {
                let enum_info = &td.type_info.enum_;
                let variants = std::slice::from_raw_parts(enum_info.variants, enum_info.num_variants as usize);

                // Compare discriminants first.
                let disc_a = *(value_a as *const u32);
                let disc_b = *(value_b as *const u32);

                if disc_a != disc_b {
                    return if disc_a < disc_b {
                        crate::RtOrdering::Less
                    } else {
                        crate::RtOrdering::Greater
                    };
                }

                // Same variant, compare payload if present.
                if disc_a < enum_info.num_variants {
                    let variant = &variants[disc_a as usize];
                    if !variant.payload.is_null() {
                        let payload_a = value_a.add(variant.offset as usize);
                        let payload_b = value_b.add(variant.offset as usize);
                        return cmp_value(payload_a, payload_b, variant.payload, float_policy);
                    }
                }
                crate::RtOrdering::Equal
            }
            rtdt::TyTag::List => {
                let list_a = &*(value_a as *const rtdt::List);
                let list_b = &*(value_b as *const rtdt::List);

                let list_info = &td.type_info.list;
                let element_tydesc = list_info.element_tydesc;
                let element_size = (*element_tydesc).size as usize;

                // Lexicographic comparison.
                let min_size = list_a.size.min(list_b.size) as usize;
                for i in 0..min_size {
                    let elem_a = list_a.data.add(i * element_size);
                    let elem_b = list_b.data.add(i * element_size);
                    let elem_cmp = cmp_value(elem_a, elem_b, element_tydesc, float_policy);
                    match elem_cmp {
                        crate::RtOrdering::Less => return crate::RtOrdering::Less,
                        crate::RtOrdering::Greater => return crate::RtOrdering::Greater,
                        crate::RtOrdering::Equal => continue,
                        crate::RtOrdering::Error => return crate::RtOrdering::Error,
                    }
                }

                // All compared elements equal, compare by length.
                if list_a.size < list_b.size {
                    crate::RtOrdering::Less
                } else if list_a.size > list_b.size {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
                }
            }
            rtdt::TyTag::Option => {
                let option_a = &*(value_a as *const rtdt::Option);
                let option_b = &*(value_b as *const rtdt::Option);

                // None < Some.
                match (option_a.tag, option_b.tag) {
                    (rtdt::OptionTag::None, rtdt::OptionTag::None) => crate::RtOrdering::Equal,
                    (rtdt::OptionTag::None, rtdt::OptionTag::Some) => crate::RtOrdering::Less,
                    (rtdt::OptionTag::Some, rtdt::OptionTag::None) => crate::RtOrdering::Greater,
                    (rtdt::OptionTag::Some, rtdt::OptionTag::Some) => {
                        let option_info = &td.type_info.option;
                        let layout = rtdt::layout::compute_option_layout(tydesc);
                        let payload_a = value_a.add(layout.payload_offset as usize);
                        let payload_b = value_b.add(layout.payload_offset as usize);
                        cmp_value(payload_a, payload_b, option_info.inner_tydesc, float_policy)
                    }
                }
            }
            rtdt::TyTag::Result => {
                let result_a = &*(value_a as *const rtdt::Result);
                let result_b = &*(value_b as *const rtdt::Result);

                // Err < Ok (conventional).
                match (result_a.tag, result_b.tag) {
                    (rtdt::ResultTag::Err, rtdt::ResultTag::Ok) => crate::RtOrdering::Less,
                    (rtdt::ResultTag::Ok, rtdt::ResultTag::Err) => crate::RtOrdering::Greater,
                    _ => {
                        let result_info = &td.type_info.result;
                        let layout = rtdt::layout::compute_result_layout(tydesc);
                        let payload_a = value_a.add(layout.payload_offset as usize);
                        let payload_b = value_b.add(layout.payload_offset as usize);

                        match result_a.tag {
                            rtdt::ResultTag::Ok => {
                                cmp_value(payload_a, payload_b, result_info.ok_tydesc, float_policy)
                            }
                            rtdt::ResultTag::Err => {
                                // Compare Error values.
                                // For now, use bitwise comparison.
                                let err_a = std::ptr::read(payload_a as *const (u64, u64));
                                let err_b = std::ptr::read(payload_b as *const (u64, u64));
                                match err_a.cmp(&err_b) {
                                    std::cmp::Ordering::Less => crate::RtOrdering::Less,
                                    std::cmp::Ordering::Greater => crate::RtOrdering::Greater,
                                    std::cmp::Ordering::Equal => crate::RtOrdering::Equal,
                                }
                            }
                        }
                    }
                }
            }
            rtdt::TyTag::Map => {
                let map_a = &*(value_a as *const rtdt::Map);
                let map_b = &*(value_b as *const rtdt::Map);

                let map_info = &td.type_info.map;
                let key_tydesc = map_info.key_tydesc;
                let value_tydesc = map_info.value_tydesc;

                // Lexicographic comparison by sorted key-value pairs.
                cmp_map_trees(
                    map_a.root as *mut rtdt::MapNode,
                    map_b.root as *mut rtdt::MapNode,
                    key_tydesc,
                    value_tydesc,
                    float_policy,
                )
            }
            rtdt::TyTag::Set => {
                let set_a = &*(value_a as *const rtdt::Set);
                let set_b = &*(value_b as *const rtdt::Set);

                let set_info = &td.type_info.set;
                let element_tydesc = set_info.element_tydesc;

                // Lexicographic comparison by sorted elements.
                cmp_set_trees(
                    set_a.root as *mut rtdt::SetNode,
                    set_b.root as *mut rtdt::SetNode,
                    element_tydesc,
                    float_policy,
                )
            }
            rtdt::TyTag::Data | rtdt::TyTag::Error => {
                // Not yet implemented.
                unimplemented!("cmp_value for {:?}", td.type_tag)
            }
        }
    }
}

/// Helper to find the leftmost leaf in a map tree.
unsafe fn find_leftmost_map_leaf(mut node: *mut rtdt::MapNode, key_tydesc: *const rtdt::TyDesc) -> *mut rtdt::MapNode {
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
unsafe fn find_leftmost_set_leaf(mut node: *mut rtdt::SetNode, key_tydesc: *const rtdt::TyDesc) -> *mut rtdt::SetNode {
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
    key_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_map_leaf(root_a, key_tydesc);
        let mut leaf_b = find_leftmost_map_leaf(root_b, key_tydesc);

        let key_size = (*key_tydesc).size as usize;
        let value_size = (*value_tydesc).size as usize;

        let mut idx_a = 0u32;
        let mut idx_b = 0u32;
        let mut len_a = read_map_node_len(leaf_a);
        let mut len_b = read_map_node_len(leaf_b);

        loop {
            // If both exhausted their current leaves, move to next.
            if idx_a >= len_a {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                leaf_a = *next_ptr;
                if leaf_a.is_null() {
                    break;
                }
                idx_a = 0;
                len_a = read_map_node_len(leaf_a);
            }

            if idx_b >= len_b {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::MapNode;
                leaf_b = *next_ptr;
                if leaf_b.is_null() {
                    break;
                }
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

        // Both should be exhausted at the same time.
        leaf_a.is_null() && leaf_b.is_null()
    }
}

/// Compare two set trees for equality by walking leaf chains.
unsafe fn eq_set_trees(
    root_a: *mut rtdt::SetNode,
    root_b: *mut rtdt::SetNode,
    element_tydesc: *const rtdt::TyDesc,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_set_leaf(root_a, element_tydesc);
        let mut leaf_b = find_leftmost_set_leaf(root_b, element_tydesc);

        let element_size = (*element_tydesc).size as usize;

        let mut idx_a = 0u32;
        let mut idx_b = 0u32;
        let mut len_a = read_set_node_len(leaf_a);
        let mut len_b = read_set_node_len(leaf_b);

        loop {
            // If both exhausted their current leaves, move to next.
            if idx_a >= len_a {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_a as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                leaf_a = *next_ptr;
                if leaf_a.is_null() {
                    break;
                }
                idx_a = 0;
                len_a = read_set_node_len(leaf_a);
            }

            if idx_b >= len_b {
                let layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc);
                let next_ptr = (leaf_b as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut rtdt::SetNode;
                leaf_b = *next_ptr;
                if leaf_b.is_null() {
                    break;
                }
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

        // Both should be exhausted at the same time.
        leaf_a.is_null() && leaf_b.is_null()
    }
}

/// Compare two map trees lexicographically by walking leaf chains.
unsafe fn cmp_map_trees(
    root_a: *mut rtdt::MapNode,
    root_b: *mut rtdt::MapNode,
    key_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
    float_policy: FloatOrdPolicy,
) -> crate::RtOrdering {
    unsafe {
        // Handle null roots.
        if root_a.is_null() && root_b.is_null() {
            return crate::RtOrdering::Equal;
        }
        if root_a.is_null() {
            return crate::RtOrdering::Less;
        }
        if root_b.is_null() {
            return crate::RtOrdering::Greater;
        }

        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_map_leaf(root_a, key_tydesc);
        let mut leaf_b = find_leftmost_map_leaf(root_b, key_tydesc);

        let key_size = (*key_tydesc).size as usize;
        let value_size = (*value_tydesc).size as usize;

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
                return crate::RtOrdering::Equal;
            }
            if exhausted_a {
                return crate::RtOrdering::Less;
            }
            if exhausted_b {
                return crate::RtOrdering::Greater;
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
                crate::RtOrdering::Less => return crate::RtOrdering::Less,
                crate::RtOrdering::Greater => return crate::RtOrdering::Greater,
                crate::RtOrdering::Error => return crate::RtOrdering::Error,
                crate::RtOrdering::Equal => {
                    // Keys equal, compare values.
                    let value_cmp = cmp_value(value_a, value_b, value_tydesc, float_policy);
                    match value_cmp {
                        crate::RtOrdering::Less => return crate::RtOrdering::Less,
                        crate::RtOrdering::Greater => return crate::RtOrdering::Greater,
                        crate::RtOrdering::Error => return crate::RtOrdering::Error,
                        crate::RtOrdering::Equal => {
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
    element_tydesc: *const rtdt::TyDesc,
    float_policy: FloatOrdPolicy,
) -> crate::RtOrdering {
    unsafe {
        // Handle null roots.
        if root_a.is_null() && root_b.is_null() {
            return crate::RtOrdering::Equal;
        }
        if root_a.is_null() {
            return crate::RtOrdering::Less;
        }
        if root_b.is_null() {
            return crate::RtOrdering::Greater;
        }

        // Find leftmost leaves.
        let mut leaf_a = find_leftmost_set_leaf(root_a, element_tydesc);
        let mut leaf_b = find_leftmost_set_leaf(root_b, element_tydesc);

        let element_size = (*element_tydesc).size as usize;

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
                return crate::RtOrdering::Equal;
            }
            if exhausted_a {
                return crate::RtOrdering::Less;
            }
            if exhausted_b {
                return crate::RtOrdering::Greater;
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
                crate::RtOrdering::Less => return crate::RtOrdering::Less,
                crate::RtOrdering::Greater => return crate::RtOrdering::Greater,
                crate::RtOrdering::Error => return crate::RtOrdering::Error,
                crate::RtOrdering::Equal => {
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const u8,
                &tydesc,
                &c as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const i8 as *const u8,
                &tydesc,
                &c as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const u16 as *const u8,
                &tydesc,
                &c as *const u16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const i16 as *const u8,
                &tydesc,
                &c as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const i32 as *const u8,
                &tydesc,
                &c as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const u64 as *const u8,
                &tydesc,
                &c as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const i64 as *const u8,
                &tydesc,
                &c as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtEq::Equals);

            let result = eq(
                &a as *const f64 as *const u8,
                &tydesc,
                &c as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtEq::NotEquals);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const u8,
                &tydesc,
                &a as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const u8,
                &tydesc,
                &c as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const i8 as *const u8,
                &tydesc,
                &a as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const i8 as *const u8,
                &tydesc,
                &c as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const i16 as *const u8,
                &tydesc,
                &a as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const i16 as *const u8,
                &tydesc,
                &c as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const i32 as *const u8,
                &tydesc,
                &a as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const i32 as *const u8,
                &tydesc,
                &c as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const i64 as *const u8,
                &tydesc,
                &a as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const i64 as *const u8,
                &tydesc,
                &c as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const u64 as *const u8,
                &tydesc,
                &a as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const u64 as *const u8,
                &tydesc,
                &c as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &b as *const f64 as *const u8,
                &tydesc,
                &a as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &a as *const f64 as *const u8,
                &tydesc,
                &c as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Equal);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &max as *const i8 as *const u8,
                &tydesc,
                &zero as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &min as *const i8 as *const u8,
                &tydesc,
                &max as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Less);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &max as *const i32 as *const u8,
                &tydesc,
                &zero as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &min as *const i32 as *const u8,
                &tydesc,
                &max as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Less);
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
            assert_eq!(result, crate::RtOrdering::Less);

            let result = cmp(
                &max as *const i64 as *const u8,
                &tydesc,
                &zero as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Greater);

            let result = cmp(
                &min as *const i64 as *const u8,
                &tydesc,
                &max as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::RtOrdering::Less);
        }
    }
}

