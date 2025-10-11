use rmx::prelude::*;

use datalove_rtdt as rtdt;

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
        if eq_value(value_a, value_b, tydesc_a) {
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
        cmp_value(value_a, value_b, tydesc_a)
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
            rtdt::TyTag::Bool | rtdt::TyTag::U32 | rtdt::TyTag::F32 | rtdt::TyTag::Int |
            rtdt::TyTag::String | rtdt::TyTag::Error => {
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
            rtdt::TyTag::F32 => {
                let a = *(value_a as *const f32);
                let b = *(value_b as *const f32);
                // For floats, use bitwise equality to handle NaN correctly.
                a.to_bits() == b.to_bits()
            }
            rtdt::TyTag::Int => {
                let int_a = &*(value_a as *const rtdt::Int);
                let int_b = &*(value_b as *const rtdt::Int);

                // Compare size and sign..
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
                    if !eq_value(field_a, field_b, field.tydesc) {
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
                    if !eq_value(field_a, field_b, field.tydesc) {
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
                        return eq_value(payload_a, payload_b, variant.payload);
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
                    if !eq_value(elem_a, elem_b, element_tydesc) {
                        return false;
                    }
                }
                true
            }
            rtdt::TyTag::Map | rtdt::TyTag::Set | rtdt::TyTag::Option | rtdt::TyTag::Result | rtdt::TyTag::Error => {
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
            rtdt::TyTag::F32 => {
                let a = *(value_a as *const f32);
                let b = *(value_b as *const f32);
                // Use total ordering: NaN == NaN, -NaN < numbers < NaN
                if a.is_nan() && b.is_nan() {
                    // Both NaN - consider equal, but distinguish by sign bit.
                    let a_bits = a.to_bits();
                    let b_bits = b.to_bits();
                    if a_bits < b_bits {
                        crate::RtOrdering::Less
                    } else if a_bits > b_bits {
                        crate::RtOrdering::Greater
                    } else {
                        crate::RtOrdering::Equal
                    }
                } else if a.is_nan() {
                    // NaN is greater than non-NaN.
                    crate::RtOrdering::Greater
                } else if b.is_nan() {
                    // Non-NaN is less than NaN.
                    crate::RtOrdering::Less
                } else if a < b {
                    crate::RtOrdering::Less
                } else if a > b {
                    crate::RtOrdering::Greater
                } else {
                    crate::RtOrdering::Equal
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
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc);
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
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc);
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
                        return cmp_value(payload_a, payload_b, variant.payload);
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
                    let elem_cmp = cmp_value(elem_a, elem_b, element_tydesc);
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
            rtdt::TyTag::Map | rtdt::TyTag::Set | rtdt::TyTag::Option | rtdt::TyTag::Result | rtdt::TyTag::Error => {
                // Not yet implemented.
                unimplemented!("cmp_value for {:?}", td.type_tag)
            }
        }
    }
}

