//! Boxing operations for Data and Error types.

use crate::c::RtStatus;
use crate::c::impls::rt_local::RtLocal;
use datalove_rtdt as rtdt;
use rtdt::TyTag;

/// Pack a scalar into the anypack representation without allocating.
///
/// The types `TyTag::can_inline` admits either fit in the 61 bits the
/// small-immediate tag leaves, or in the whole secondary word alongside a
/// tydesc. Reading them costs one word either way, where the heap form costs an
/// allocation and a dereference.
///
/// # Safety
///
/// `inner_in` must point to an initialized value of the type `tydesc`
/// describes, and `tytag` must be that type's tag.
unsafe fn inline_data(
    inner_in: *const u8,
    tydesc: &rtdt::TyDesc,
    tytag: TyTag,
) -> rtdt::Data {
    unsafe {
        match tytag {
            // Small immediate: the value rides in the primary word, the tag in
            // the secondary, and no tydesc is needed to read it back.
            TyTag::Bool => rtdt::Data::from_bool(*(inner_in as *const bool)),
            TyTag::U8 => rtdt::Data::from_u8(*(inner_in as *const u8)),
            TyTag::I8 => rtdt::Data::from_i8(*(inner_in as *const i8)),
            TyTag::U16 => rtdt::Data::from_u16(*(inner_in as *const u16)),
            TyTag::I16 => rtdt::Data::from_i16(*(inner_in as *const i16)),
            TyTag::U32 => rtdt::Data::from_u32(*(inner_in as *const u32)),
            TyTag::I32 => rtdt::Data::from_i32(*(inner_in as *const i32)),
            // Inline with tydesc: the value takes the whole secondary word, so
            // the type has to come from the tydesc in the primary.
            TyTag::F32 => rtdt::Data::from_f32(*(inner_in as *const f32), tydesc),
            TyTag::U64 => rtdt::Data::from_u64(*(inner_in as *const u64), tydesc),
            TyTag::I64 => rtdt::Data::from_i64(*(inner_in as *const i64), tydesc),
            TyTag::F64 => rtdt::Data::from_f64(*(inner_in as *const f64), tydesc),
            _ => unreachable!("can_inline admitted {:?} with no way to pack it", tytag),
        }
    }
}

/// Borrow what a Data holds, writing a packed value into `scratch` first.
///
/// A `data` holding something on the heap can lend the address it already has.
/// One holding a narrow scalar in its own words cannot: the value is not at a
/// byte address of its own, and for the narrowest it has no descriptor beside
/// it either. So the value is unpacked into eight bytes the caller lends, and
/// the descriptor comes from `packed_tydesc`, which is a constant per type
/// rather than anything made up here.
///
/// The borrow lasts as long as `scratch` does, so a caller keeps it alive
/// across whatever it was borrowing for.
///
/// # Safety
///
/// `data_in` must be an initialized `Data`.
pub unsafe fn data_borrow(
    data_in: *const u8,
    scratch: &mut crate::rust::DataScratch,
) -> (*const u8, *const rtdt::TyDesc) {
    unsafe {
        let data = &*(data_in as *const rtdt::Data);
        if let Some((tydesc, value)) = data.pointee() {
            return (value, tydesc.as_ptr());
        }

        let tytag = data.tytag();
        let bits = data.inline_bits();
        let scratch = scratch.as_mut_ptr();
        // Written through a pointer of the right width rather than copied from
        // the low bytes, so that the byte order is the machine's own.
        match tytag {
            TyTag::Bool | TyTag::U8 | TyTag::I8 => *scratch = bits as u8,
            TyTag::U16 | TyTag::I16 => *(scratch as *mut u16) = bits as u16,
            TyTag::U32 | TyTag::I32 | TyTag::F32 => *(scratch as *mut u32) = bits as u32,
            _ => *(scratch as *mut u64) = bits,
        }

        // The wider inline forms keep their descriptor; the narrow ones have
        // only a tag, and the descriptor for a tag is a constant.
        let tydesc = match rtdt::packed_tydesc(tytag) {
            Some(td) => td,
            None => data.tydesc(),
        };
        assert!(!tydesc.is_null(), "a packed {tytag:?} has no descriptor");
        (scratch as *const u8, tydesc)
    }
}

/// Create an Error from any value (moves the value to heap).
///
/// Allocates heap storage, copies the inner value, and creates an Error
/// using anypack tagged pointer encoding.
pub unsafe fn error_from_local(
    rt: &mut RtLocal,
    inner_in: *const u8,
    inner_tydesc: &rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    // Error uses the same encoding as Data, so a scalar packs the same way.
    let tytag = inner_tydesc.type_tag;
    if tytag.can_inline() {
        unsafe {
            let data = inline_data(inner_in, inner_tydesc, tytag);
            std::ptr::write(dest_out as *mut rtdt::Error, std::mem::transmute(data));
        }
        return RtStatus::Ok;
    }

    let inner_size = inner_tydesc.size as usize;

    // Allocate heap storage for the inner value.
    let moved_ptr = unsafe {
        rt.alloc.alloc(inner_tydesc.size, inner_tydesc.align, 1)
    };
    if moved_ptr.is_null() {
        return RtStatus::Error;
    }

    // Move inner value to heap storage (bitwise copy).
    unsafe {
        std::ptr::copy_nonoverlapping(inner_in, moved_ptr, inner_size);
    }

    // Write Error struct to destination.
    // Error has same layout as Data, so we use Data::from_pointers and transmute.
    unsafe {
        let data = rtdt::Data::from_pointers(inner_tydesc, moved_ptr);
        std::ptr::write(dest_out as *mut rtdt::Error, std::mem::transmute(data));
    }

    RtStatus::Ok
}

/// Create a Data from any value (moves the value to heap).
///
/// Allocates heap storage, copies the inner value, and creates a Data
/// using anypack tagged pointer encoding.
pub unsafe fn data_from_local(
    rt: &mut RtLocal,
    inner_in: *const u8,
    inner_tydesc: &rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    let tytag = inner_tydesc.type_tag;
    // A scalar goes in the two words directly rather than onto the heap.
    if tytag.can_inline() {
        unsafe {
            let data = inline_data(inner_in, inner_tydesc, tytag);
            std::ptr::write(dest_out as *mut rtdt::Data, data);
        }
        return RtStatus::Ok;
    }

    let inner_size = inner_tydesc.size as usize;

    // Allocate heap storage for the inner value.
    let moved_ptr = unsafe {
        rt.alloc.alloc(inner_tydesc.size, inner_tydesc.align, 1)
    };
    if moved_ptr.is_null() {
        return RtStatus::Error;
    }

    // Move inner value to heap storage (bitwise copy).
    unsafe {
        std::ptr::copy_nonoverlapping(inner_in, moved_ptr, inner_size);
    }

    // Write Data struct to destination.
    unsafe {
        let data = rtdt::Data::from_pointers(inner_tydesc, moved_ptr);
        std::ptr::write(dest_out as *mut rtdt::Data, data);
    }

    RtStatus::Ok
}
