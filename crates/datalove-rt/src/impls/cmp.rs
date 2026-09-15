use datalove_rtdt as rtdt;

/// Float equality policy for comparison operations.
#[derive(Copy, Clone)]
enum FloatEqPolicy {
    /// IEEE equality with zero coalescing: NaN != NaN; +0.0 == -0.0.
    Ieee,
    /// Bitwise equality: all bit patterns distinct.
    Bitwise,
}

/// Read what a `data` holds, unpacking it into `scratch` when it has no
/// address of its own.
///
/// A packed value is not at a byte address and the narrowest carry no
/// descriptor, so both come from here. See `boxing::data_borrow`.
///
/// # Safety
///
/// `data` must be initialized and `scratch` writable for eight bytes.
unsafe fn data_inner(
    data: *const rtdt::Data,
    scratch: &mut u64,
) -> std::option::Option<(*const u8, *const rtdt::TyDesc)> {
    let mut value: *const u8 = std::ptr::null();
    let mut tydesc: *const rtdt::TyDesc = std::ptr::null();
    let status = unsafe {
        super::boxing::data_borrow(
            data as *const u8,
            scratch as *mut u64 as *mut u8,
            &mut value,
            &mut tydesc,
        )
    };
    if status != crate::c::RtStatus::Ok {
        return std::option::Option::None;
    }
    std::option::Option::Some((value, tydesc))
}

/// Whether two packed values are equal.
///
/// A `data` and an `error` are the same three encodings under two type tags,
/// and an `error` is how the error side of a result is held, so all three read
/// through here. Each of them used to compare for itself: the `error` arms
/// read the two words raw, which puts every negative integer above every
/// positive one, and the result's arm reached straight for the value pointer,
/// which only a heap-packed value has. A set of results whose errors held a
/// `bool` could not be built at all -- inserting the second one compared it
/// against the first and the runtime stopped on "value not stored as pointer".
///
/// # Safety
///
/// Both pointers must be initialized packed values.
unsafe fn eq_packed(
    a: *const rtdt::Data,
    b: *const rtdt::Data,
    float_policy: FloatEqPolicy,
) -> bool {
    unsafe {
        if (*a).tytag() != (*b).tytag() {
            return false;
        }
        if std::ptr::eq(a, b) {
            return true;
        }
        // Unpacked and compared as what it is, rather than as the words it
        // lies in: the two float zeros have different bits and are equal, and
        // a NaN has the same bits as itself and is not.
        let mut scratch_a = 0u64;
        let mut scratch_b = 0u64;
        let (std::option::Option::Some((inner_a, inner_td_a)),
             std::option::Option::Some((inner_b, inner_td_b))) =
            (data_inner(a, &mut scratch_a), data_inner(b, &mut scratch_b))
        else {
            return false;
        };
        // A tag is not a type. Two packed values can both be tagged `Set` and
        // hold a set of different elements, and reading one through the
        // other's descriptor reads its nodes at the wrong stride.
        let td_a = rtdt::TyDescRef::from_ptr(inner_td_a);
        let td_b = rtdt::TyDescRef::from_ptr(inner_td_b);
        if !eq_tydesc(td_a, td_b) {
            return false;
        }
        eq_value(inner_a, inner_b, td_a, float_policy)
    }
}

/// How two packed values order. See `eq_packed`.
///
/// Two of different types order by their type tags, which is arbitrary but
/// total, and is what a collection sorted on them needs.
///
/// # Safety
///
/// Both pointers must be initialized packed values.
unsafe fn cmp_packed(
    a: *const rtdt::Data,
    b: *const rtdt::Data,
) -> crate::c::RtOrdering {
    unsafe {
        let tytag_a = (*a).tytag();
        let tytag_b = (*b).tytag();
        if tytag_a != tytag_b {
            return ordering_of((tytag_a as u8).cmp(&(tytag_b as u8)));
        }
        if std::ptr::eq(a, b) {
            return crate::c::RtOrdering::Equal;
        }
        let mut scratch_a = 0u64;
        let mut scratch_b = 0u64;
        let (std::option::Option::Some((inner_a, inner_td_a)),
             std::option::Option::Some((inner_b, inner_td_b))) =
            (data_inner(a, &mut scratch_a), data_inner(b, &mut scratch_b))
        else {
            return crate::c::RtOrdering::Error;
        };
        // A tag is not a type: two packed values can share one and still hold
        // different types, a set of `u32` against a set of `string`. Only
        // where they are the one type may their values be read through the one
        // descriptor; where they are not, the types themselves say the order.
        let td_a = rtdt::TyDescRef::from_ptr(inner_td_a);
        let td_b = rtdt::TyDescRef::from_ptr(inner_td_b);
        match cmp_tydesc(td_a, td_b) {
            std::cmp::Ordering::Equal => cmp_value(inner_a, inner_b, td_a),
            ord => ordering_of(ord),
        }
    }
}

/// Carry an ordering across to the one the runtime speaks.
fn ordering_of(ordering: std::cmp::Ordering) -> crate::c::RtOrdering {
    match ordering {
        std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
        std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
        std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
    }
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
        cmp_value(value_a, value_b, td_a)
    }
}

/// How two type descriptors order.
///
/// Arbitrary but total, and `Equal` exactly where `eq_tydesc` is true. That
/// correspondence is the point: `cmp_packed` may only compare two packed
/// values against a single descriptor where they are the one type, and needs
/// something to order them by where they are not.
///
/// Ordered by tag first, then size and alignment, then structure, mirroring
/// the walk `eq_tydesc` takes -- each of those a total order, so their
/// composition is one too.
pub(crate) fn cmp_tydesc(
    td_a: rtdt::TyDescRef,
    td_b: rtdt::TyDescRef,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    if std::ptr::eq(td_a.as_ptr(), td_b.as_ptr()) {
        return Ordering::Equal;
    }

    let tag_a = td_a.type_tag();
    let tag_b = td_b.type_tag();
    match (tag_a as u8).cmp(&(tag_b as u8)) {
        Ordering::Equal => {}
        ord => return ord,
    }
    match td_a.size().cmp(&td_b.size()) {
        Ordering::Equal => {}
        ord => return ord,
    }
    match td_a.align().cmp(&td_b.align()) {
        Ordering::Equal => {}
        ord => return ord,
    }

    match tag_a {
        rtdt::TyTag::Bool | rtdt::TyTag::U8 | rtdt::TyTag::I8 |
        rtdt::TyTag::U16 | rtdt::TyTag::I16 | rtdt::TyTag::U32 | rtdt::TyTag::I32 |
        rtdt::TyTag::U64 | rtdt::TyTag::I64 | rtdt::TyTag::Index | rtdt::TyTag::Offset |
        rtdt::TyTag::F32 | rtdt::TyTag::F64 |
        rtdt::TyTag::Int | rtdt::TyTag::String | rtdt::TyTag::Data |
        rtdt::TyTag::Error => Ordering::Equal,

        // An atom's name is the whole of its identity. `atom Red` and
        // `atom Blue` are two types and nothing else in their descriptors
        // tells them apart, both being zero-sized and carrying nothing.
        rtdt::TyTag::Atom => td_a.atom_info().0.cmp(td_b.atom_info().0),

        rtdt::TyTag::Tuple => {
            let info_a = td_a.tuple_info();
            let info_b = td_b.tuple_info();
            match info_a.num_fields().cmp(&info_b.num_fields()) {
                Ordering::Equal => {}
                ord => return ord,
            }
            for i in 0..info_a.num_fields() as usize {
                let field_a = info_a.field(i).unwrap();
                let field_b = info_b.field(i).unwrap();
                match field_a.offset().cmp(&field_b.offset()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match cmp_tydesc(field_a.tydesc(), field_b.tydesc()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
            }
            Ordering::Equal
        }

        rtdt::TyTag::Struct => {
            let info_a = td_a.struct_info();
            let info_b = td_b.struct_info();
            match info_a.num_fields().cmp(&info_b.num_fields()) {
                Ordering::Equal => {}
                ord => return ord,
            }
            for i in 0..info_a.num_fields() as usize {
                let field_a = info_a.field(i).unwrap();
                let field_b = info_b.field(i).unwrap();
                match field_a.name().cmp(field_b.name()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match field_a.offset().cmp(&field_b.offset()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match cmp_tydesc(field_a.tydesc(), field_b.tydesc()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
            }
            Ordering::Equal
        }

        rtdt::TyTag::Term => {
            let (name_a, payload_a) = td_a.term_info();
            let (name_b, payload_b) = td_b.term_info();
            match name_a.cmp(name_b) {
                Ordering::Equal => cmp_tydesc(payload_a, payload_b),
                ord => ord,
            }
        }

        rtdt::TyTag::Enum => {
            let info_a = td_a.enum_info();
            let info_b = td_b.enum_info();
            match info_a.num_variants().cmp(&info_b.num_variants()) {
                Ordering::Equal => {}
                ord => return ord,
            }
            for i in 0..info_a.num_variants() as usize {
                let variant_a = info_a.variant(i).unwrap();
                let variant_b = info_b.variant(i).unwrap();
                match variant_a.name().cmp(variant_b.name()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match variant_a.offset().cmp(&variant_b.offset()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                // A variant carrying nothing comes before one that does.
                match (variant_a.payload(), variant_b.payload()) {
                    (core::option::Option::None, core::option::Option::None) => {}
                    (core::option::Option::None, core::option::Option::Some(_)) => return Ordering::Less,
                    (core::option::Option::Some(_), core::option::Option::None) => return Ordering::Greater,
                    (core::option::Option::Some(pa), core::option::Option::Some(pb)) => {
                        match cmp_tydesc(pa, pb) {
                            Ordering::Equal => {}
                            ord => return ord,
                        }
                    }
                }
            }
            Ordering::Equal
        }

        rtdt::TyTag::List => cmp_tydesc(td_a.list_element_ty(), td_b.list_element_ty()),
        rtdt::TyTag::Set => cmp_tydesc(td_a.set_element_ty(), td_b.set_element_ty()),

        rtdt::TyTag::Map => {
            match cmp_tydesc(td_a.map_key_ty(), td_b.map_key_ty()) {
                Ordering::Equal => cmp_tydesc(td_a.map_value_ty(), td_b.map_value_ty()),
                ord => ord,
            }
        }

        rtdt::TyTag::Tensor => {
            match td_a.tensor_rank().cmp(&td_b.tensor_rank()) {
                Ordering::Equal => cmp_tydesc(td_a.tensor_element_ty(), td_b.tensor_element_ty()),
                ord => ord,
            }
        }

        rtdt::TyTag::Table => {
            match td_a.table_num_columns().cmp(&td_b.table_num_columns()) {
                Ordering::Equal => {}
                ord => return ord,
            }
            for (col_a, col_b) in td_a.table_column_tydescs().zip(td_b.table_column_tydescs()) {
                match col_a.name().cmp(col_b.name()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                match cmp_tydesc(col_a.tydesc(), col_b.tydesc()) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
            }
            Ordering::Equal
        }

        rtdt::TyTag::Option => cmp_tydesc(td_a.option_inner_ty(), td_b.option_inner_ty()),
        rtdt::TyTag::Result => cmp_tydesc(td_a.result_ok_ty(), td_b.result_ok_ty()),
    }
}

/// Compare two type descriptors for structural equality.
///
/// Tydescs are not fully deduplicated, so two descriptions of one type may sit
/// at different addresses and still have to compare equal. The walk below
/// handles that. One descriptor compared against itself is the common case
/// though: the collections pass the same pointer for both sides on every key
/// comparison, so the identity check below carries the B-tree hot path.
pub(crate) fn eq_tydesc(
    td_a: rtdt::TyDescRef,
    td_b: rtdt::TyDescRef,
) -> bool {
    if std::ptr::eq(td_a.as_ptr(), td_b.as_ptr()) {
        return true;
    }

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
            rtdt::TyTag::U64 | rtdt::TyTag::I64 | rtdt::TyTag::Index | rtdt::TyTag::Offset |
            rtdt::TyTag::F32 | rtdt::TyTag::F64 |
            rtdt::TyTag::Int | rtdt::TyTag::String | rtdt::TyTag::Data |
            rtdt::TyTag::Error => {
                true
            }
            // See `cmp_tydesc`: the name is the identity.
            rtdt::TyTag::Atom => {
                td_a.atom_info().0 == td_b.atom_info().0
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
            rtdt::TyTag::Term => {
                let (name_a, payload_a) = td_a.term_info();
                let (name_b, payload_b) = td_b.term_info();
                name_a == name_b && eq_tydesc(payload_a, payload_b)
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
            rtdt::TyTag::Table => {
                let num_cols_a = td_a.table_num_columns();
                let num_cols_b = td_b.table_num_columns();
                if num_cols_a != num_cols_b {
                    return false;
                }
                for (col_a, col_b) in td_a.table_column_tydescs().zip(td_b.table_column_tydescs()) {
                    if col_a.name() != col_b.name() || !eq_tydesc(col_a.tydesc(), col_b.tydesc()) {
                        return false;
                    }
                }
                true
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
            rtdt::TyTag::Index => {
                let a = *(value_a as *const rtdt::IndexRepr);
                let b = *(value_b as *const rtdt::IndexRepr);
                a == b
            }
            rtdt::TyTag::Offset => {
                let a = *(value_a as *const rtdt::OffsetRepr);
                let b = *(value_b as *const rtdt::OffsetRepr);
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
                if str_a.size == rtdt::Index::ZERO && str_b.size == rtdt::Index::ZERO {
                    return true;
                }
                if str_a.size != str_b.size {
                    return false;
                }

                let bytes_a = std::slice::from_raw_parts(str_a.data, str_a.size.as_usize());
                let bytes_b = std::slice::from_raw_parts(str_b.data, str_b.size.as_usize());
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
            rtdt::TyTag::Atom => true,
            rtdt::TyTag::Term => {
                let (_, payload_ty) = td.term_info();
                eq_value(value_a, value_b, payload_ty, float_policy)
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

                for i in 0..list_a.size.as_usize() {
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
                        eq_packed(payload_a as *const rtdt::Data,
                            payload_b as *const rtdt::Data, float_policy)
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
                if map_a.len == rtdt::Index::ZERO {
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
                if set_a.len == rtdt::Index::ZERO {
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
                    let total_elems: rtdt::IndexRepr = shape_a.iter().map(|u| u.0).product();

                    if total_elems == 0 {
                        return true;  // Empty tensors with matching shapes are equal.
                    }

                    let element_ty = td.tensor_element_ty();
                    let element_size = element_ty.size() as usize;
                    let strides_a = std::slice::from_raw_parts(tensor_a.strides, rank as usize);
                    let strides_b = std::slice::from_raw_parts(tensor_b.strides, rank as usize);

                    // Iterate through all multi-dimensional indices.
                    let mut indices: Vec<rtdt::IndexRepr> = vec![0; rank as usize];
                    for _ in 0..total_elems {
                        // Compute linear offset for tensor_a.
                        let mut offset_a = tensor_a.offset_elems.0;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_a += idx * strides_a[i].0;
                        }
                        let elem_a = tensor_a.ptr_base.add((offset_a as usize) * element_size);

                        // Compute linear offset for tensor_b.
                        let mut offset_b = tensor_b.offset_elems.0;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_b += idx * strides_b[i].0;
                        }
                        let elem_b = tensor_b.ptr_base.add((offset_b as usize) * element_size);

                        // Compare elements.
                        if !eq_value(elem_a, elem_b, element_ty, float_policy) {
                            return false;
                        }

                        // Increment indices (like odometer).
                        let mut carry: rtdt::IndexRepr = 1;
                        for i in (0..rank as usize).rev() {
                            if carry == 0 {
                                break;
                            }
                            indices[i] += carry;
                            if indices[i] >= shape_a[i].0 {
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

                    let elem_a = tensor_a.ptr_base.add(tensor_a.offset_elems.as_usize() * element_size);
                    let elem_b = tensor_b.ptr_base.add(tensor_b.offset_elems.as_usize() * element_size);

                    eq_value(elem_a, elem_b, element_ty, float_policy)
                }
            }
            rtdt::TyTag::Table => {
                let table_a = &*(value_a as *const rtdt::Table);
                let table_b = &*(value_b as *const rtdt::Table);

                // Compare lengths first.
                if table_a.len != table_b.len {
                    return false;
                }

                // Empty tables are equal.
                if table_a.len == rtdt::Index::ZERO {
                    return true;
                }

                let column_tydescs = crate::impls::table::collect_column_tydescs(td);

                // Compare element-by-element, row-major order.
                for row in 0..table_a.len.0 {
                    for (col, col_info) in td.table_column_tydescs().enumerate() {
                        let elem_a = crate::impls::table::element_ptr(
                            table_a.data,
                            &column_tydescs,
                            row,
                            col,
                            table_a.capacity.0,
                        );
                        let elem_b = crate::impls::table::element_ptr(
                            table_b.data,
                            &column_tydescs,
                            row,
                            col,
                            table_b.capacity.0,
                        );
                        if !eq_value(elem_a, elem_b, col_info.tydesc(), float_policy) {
                            return false;
                        }
                    }
                }
                true
            }
            rtdt::TyTag::Data | rtdt::TyTag::Error => {
                eq_packed(value_a as *const rtdt::Data,
                    value_b as *const rtdt::Data, float_policy)
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
            rtdt::TyTag::Index => {
                let a = *(value_a as *const rtdt::IndexRepr);
                let b = *(value_b as *const rtdt::IndexRepr);
                if a < b {
                    crate::c::RtOrdering::Less
                } else if a > b {
                    crate::c::RtOrdering::Greater
                } else {
                    crate::c::RtOrdering::Equal
                }
            }
            rtdt::TyTag::Offset => {
                let a = *(value_a as *const rtdt::OffsetRepr);
                let b = *(value_b as *const rtdt::OffsetRepr);
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
                // IEEE 754-2008 total order, which is the one the language
                // has: NaN takes a place rather than comparing false, and the
                // two zeros are told apart.
                // -NaN < -Inf < -numbers < -0.0 < +0.0 < +numbers < +Inf < +NaN
                match a.total_cmp(&b) {
                    std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                    std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                    std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                }
            }
            rtdt::TyTag::F64 => {
                let a = *(value_a as *const f64);
                let b = *(value_b as *const f64);
                // IEEE 754-2008 total order, which is the one the language
                // has: NaN takes a place rather than comparing false, and the
                // two zeros are told apart.
                // -NaN < -Inf < -numbers < -0.0 < +0.0 < +numbers < +Inf < +NaN
                match a.total_cmp(&b) {
                    std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                    std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                    std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
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
                if str_a.size == rtdt::Index::ZERO && str_b.size == rtdt::Index::ZERO {
                    return crate::c::RtOrdering::Equal;
                }
                if str_a.size == rtdt::Index::ZERO {
                    return crate::c::RtOrdering::Less;
                }
                if str_b.size == rtdt::Index::ZERO {
                    return crate::c::RtOrdering::Greater;
                }

                let bytes_a = std::slice::from_raw_parts(str_a.data, str_a.size.as_usize());
                let bytes_b = std::slice::from_raw_parts(str_b.data, str_b.size.as_usize());

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
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc());
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
                    let field_cmp = cmp_value(field_a, field_b, field.tydesc());
                    match field_cmp {
                        crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                        crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                        crate::c::RtOrdering::Equal => continue,
                        crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                    }
                }
                crate::c::RtOrdering::Equal
            }
            rtdt::TyTag::Atom => crate::c::RtOrdering::Equal,
            rtdt::TyTag::Term => {
                let (_, payload_ty) = td.term_info();
                cmp_value(value_a, value_b, payload_ty)
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
                            return cmp_value(payload_a, payload_b, payload_ty);
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
                let min_size = list_a.size.min(list_b.size).as_usize();
                for i in 0..min_size {
                    let elem_a = list_a.data.add(i * element_size);
                    let elem_b = list_b.data.add(i * element_size);
                    let elem_cmp = cmp_value(elem_a, elem_b, element_ty);
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
                        cmp_value(payload_a, payload_b, inner_ty)
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
                                cmp_value(payload_a, payload_b, ok_ty)
                            }
                            rtdt::ResultTag::Err => {
                                cmp_packed(payload_a as *const rtdt::Data,
                                    payload_b as *const rtdt::Data)
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
                    let total_elems: rtdt::IndexRepr = shape_a.iter().map(|u| u.0).product();

                    if total_elems == 0 {
                        return crate::c::RtOrdering::Equal;  // Empty tensors with matching shapes are equal.
                    }

                    let element_ty = td.tensor_element_ty();
                    let element_size = element_ty.size() as usize;
                    let strides_a = std::slice::from_raw_parts(tensor_a.strides, rank as usize);
                    let strides_b = std::slice::from_raw_parts(tensor_b.strides, rank as usize);

                    // Iterate through all multi-dimensional indices lexicographically.
                    let mut indices: Vec<rtdt::IndexRepr> = vec![0; rank as usize];
                    for _ in 0..total_elems {
                        // Compute linear offset for tensor_a.
                        let mut offset_a = tensor_a.offset_elems.0;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_a += idx * strides_a[i].0;
                        }
                        let elem_a = tensor_a.ptr_base.add((offset_a as usize) * element_size);

                        // Compute linear offset for tensor_b.
                        let mut offset_b = tensor_b.offset_elems.0;
                        for (i, &idx) in indices.iter().enumerate() {
                            offset_b += idx * strides_b[i].0;
                        }
                        let elem_b = tensor_b.ptr_base.add((offset_b as usize) * element_size);

                        // Compare elements.
                        let elem_cmp = cmp_value(elem_a, elem_b, element_ty);
                        match elem_cmp {
                            crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                            crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                            crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                            crate::c::RtOrdering::Equal => {
                                // Continue to next element.
                            }
                        }

                        // Increment indices (like odometer).
                        let mut carry: rtdt::IndexRepr = 1;
                        for i in (0..rank as usize).rev() {
                            if carry == 0 {
                                break;
                            }
                            indices[i] += carry;
                            if indices[i] >= shape_a[i].0 {
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

                    let elem_a = tensor_a.ptr_base.add(tensor_a.offset_elems.as_usize() * element_size);
                    let elem_b = tensor_b.ptr_base.add(tensor_b.offset_elems.as_usize() * element_size);

                    cmp_value(elem_a, elem_b, element_ty)
                }
            }
            rtdt::TyTag::Table => {
                let table_a = &*(value_a as *const rtdt::Table);
                let table_b = &*(value_b as *const rtdt::Table);

                let column_tydescs = crate::impls::table::collect_column_tydescs(td);
                let min_len = table_a.len.min(table_b.len).0;

                // Compare element-by-element, row-major order (lexicographic).
                for row in 0..min_len {
                    for (col, col_info) in td.table_column_tydescs().enumerate() {
                        let elem_a = crate::impls::table::element_ptr(
                            table_a.data,
                            &column_tydescs,
                            row,
                            col,
                            table_a.capacity.0,
                        );
                        let elem_b = crate::impls::table::element_ptr(
                            table_b.data,
                            &column_tydescs,
                            row,
                            col,
                            table_b.capacity.0,
                        );
                        let ord = cmp_value(elem_a, elem_b, col_info.tydesc());
                        if ord != crate::c::RtOrdering::Equal {
                            return ord;
                        }
                    }
                }

                // All compared elements equal - shorter table is less.
                match table_a.len.0.cmp(&table_b.len.0) {
                    std::cmp::Ordering::Less => crate::c::RtOrdering::Less,
                    std::cmp::Ordering::Greater => crate::c::RtOrdering::Greater,
                    std::cmp::Ordering::Equal => crate::c::RtOrdering::Equal,
                }
            }
            rtdt::TyTag::Data | rtdt::TyTag::Error => {
                cmp_packed(value_a as *const rtdt::Data, value_b as *const rtdt::Data)
            }
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
        let mut leaf_a = crate::impls::btreemap::leftmost_leaf(root_a, key_tydesc);
        let mut leaf_b = crate::impls::btreemap::leftmost_leaf(root_b, key_tydesc);

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
        let mut leaf_a = crate::impls::set::leftmost_leaf(root_a, element_tydesc);
        let mut leaf_b = crate::impls::set::leftmost_leaf(root_b, element_tydesc);

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
        let mut leaf_a = crate::impls::btreemap::leftmost_leaf(root_a, key_tydesc);
        let mut leaf_b = crate::impls::btreemap::leftmost_leaf(root_b, key_tydesc);

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
            let key_cmp = cmp_value(key_a, key_b, key_tydesc);
            match key_cmp {
                crate::c::RtOrdering::Less => return crate::c::RtOrdering::Less,
                crate::c::RtOrdering::Greater => return crate::c::RtOrdering::Greater,
                crate::c::RtOrdering::Error => return crate::c::RtOrdering::Error,
                crate::c::RtOrdering::Equal => {
                    // Keys equal, compare values.
                    let value_cmp = cmp_value(value_a, value_b, value_tydesc);
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
        let mut leaf_a = crate::impls::set::leftmost_leaf(root_a, element_tydesc);
        let mut leaf_b = crate::impls::set::leftmost_leaf(root_b, element_tydesc);

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
            let elem_cmp = cmp_value(element_a, element_b, element_tydesc);
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
            let result = cmp_total(
                &a as *const u8,
                &tydesc,
                &b as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const u8,
                &tydesc,
                &a as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &a as *const i8 as *const u8,
                &tydesc,
                &b as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const i8 as *const u8,
                &tydesc,
                &a as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &a as *const i16 as *const u8,
                &tydesc,
                &b as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const i16 as *const u8,
                &tydesc,
                &a as *const i16 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &a as *const i32 as *const u8,
                &tydesc,
                &b as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const i32 as *const u8,
                &tydesc,
                &a as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &a as *const i64 as *const u8,
                &tydesc,
                &b as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const i64 as *const u8,
                &tydesc,
                &a as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &a as *const u64 as *const u8,
                &tydesc,
                &b as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const u64 as *const u8,
                &tydesc,
                &a as *const u64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &a as *const f64 as *const u8,
                &tydesc,
                &b as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &b as *const f64 as *const u8,
                &tydesc,
                &a as *const f64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &min as *const i8 as *const u8,
                &tydesc,
                &zero as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &max as *const i8 as *const u8,
                &tydesc,
                &zero as *const i8 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &min as *const i32 as *const u8,
                &tydesc,
                &zero as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &max as *const i32 as *const u8,
                &tydesc,
                &zero as *const i32 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
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
            let result = cmp_total(
                &min as *const i64 as *const u8,
                &tydesc,
                &zero as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);

            let result = cmp_total(
                &max as *const i64 as *const u8,
                &tydesc,
                &zero as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Greater);

            let result = cmp_total(
                &min as *const i64 as *const u8,
                &tydesc,
                &max as *const i64 as *const u8,
                &tydesc,
            );
            assert_eq!(result, crate::c::RtOrdering::Less);
        }
    }
}

