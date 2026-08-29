//! Boxing operations for Data and Error types.

use crate::c::{LocalRtHandle, RtStatus};
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
    tydesc: *const rtdt::TyDesc,
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

/// Move the value back out of a Data.
///
/// The inverse of [`data_from_local`]. The caller knows what type it put in and
/// passes the tydesc for it; nothing here decides that, so this is not a
/// checked downcast. A generic call site is the caller that knows.
///
/// The value is moved, not copied: the payload is written to `dest_out` and the
/// box, if there was one, is freed without destroying what it held. The `Data`
/// is dead afterwards and must not be dropped.
///
/// # Safety
///
/// `data_in` must be an initialized `Data` holding a value of the type
/// `dest_tydesc` describes.
pub unsafe fn data_into_local(
    rt: LocalRtHandle,
    data_in: *const u8,
    dest_out: *mut u8,
    dest_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let data = &*(data_in as *const rtdt::Data);
        let size = (*dest_tydesc).size as usize;

        match data.tag() {
            rtdt::anypack::Tag::TwoPointers => {
                let value_ptr = data.value_ptr();
                let inner_tydesc = data.tydesc();
                if value_ptr.is_null() || inner_tydesc.is_null() {
                    return RtStatus::Error;
                }
                // Move the payload out, then release the box it sat in. The
                // payload is not destroyed: it now belongs to the destination.
                std::ptr::copy_nonoverlapping(value_ptr, dest_out, size);
                let inner_ty = rtdt::TyDescRef::from_ptr(inner_tydesc);
                let rt_ref = &mut *(rt as *mut crate::impls::rt_local::RtLocal);
                rt_ref.alloc.free(
                    inner_ty.size(),
                    inner_ty.align(),
                    1,
                    value_ptr as *mut u8,
                );
                RtStatus::Ok
            }
            rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                // Packed in the two words, so there is nothing to free and the
                // accessor for the tag reads it back.
                unpack_scalar(data, dest_out)
            }
            _ => RtStatus::Error,
        }
    }
}

/// Write a packed scalar to `dest_out`.
///
/// # Safety
///
/// `dest_out` must have room for a value of the type `data`'s tag names.
unsafe fn unpack_scalar(data: &rtdt::Data, dest_out: *mut u8) -> RtStatus {
    unsafe {
        macro_rules! write_as {
            ($accessor:ident, $ty:ty) => {{
                match data.$accessor() {
                    Some(v) => {
                        std::ptr::write(dest_out as *mut $ty, v);
                        RtStatus::Ok
                    }
                    None => RtStatus::Error,
                }
            }};
        }
        match data.tytag() {
            TyTag::Bool => write_as!(as_bool, bool),
            TyTag::U8 => write_as!(as_u8, u8),
            TyTag::I8 => write_as!(as_i8, i8),
            TyTag::U16 => write_as!(as_u16, u16),
            TyTag::I16 => write_as!(as_i16, i16),
            TyTag::U32 => write_as!(as_u32, u32),
            TyTag::I32 => write_as!(as_i32, i32),
            TyTag::U64 => write_as!(as_u64, u64),
            TyTag::I64 => write_as!(as_i64, i64),
            TyTag::F32 => write_as!(as_f32, f32),
            TyTag::F64 => write_as!(as_f64, f64),
            _ => RtStatus::Error,
        }
    }
}

/// Create an Error from any value (moves the value to heap).
///
/// Allocates heap storage, copies the inner value, and creates an Error
/// using anypack tagged pointer encoding.
pub unsafe fn error_from_local(
    rt: LocalRtHandle,
    inner_in: *const u8,
    inner_tydesc: *const rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    // Error uses the same encoding as Data, so a scalar packs the same way.
    let tytag = unsafe { (*inner_tydesc).type_tag };
    if tytag.can_inline() {
        unsafe {
            let data = inline_data(inner_in, inner_tydesc, tytag);
            std::ptr::write(dest_out as *mut rtdt::Error, std::mem::transmute(data));
        }
        return RtStatus::Ok;
    }

    let inner_size = unsafe { (*inner_tydesc).size as usize };

    // Allocate heap storage for the inner value.
    let moved_ptr = unsafe {
        crate::c::dtlv_rti_mem_alloc_local(rt, inner_tydesc, 1)
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
    rt: LocalRtHandle,
    inner_in: *const u8,
    inner_tydesc: *const rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    // A scalar goes in the two words directly rather than onto the heap.
    let tytag = unsafe { (*inner_tydesc).type_tag };
    if tytag.can_inline() {
        unsafe {
            let data = inline_data(inner_in, inner_tydesc, tytag);
            std::ptr::write(dest_out as *mut rtdt::Data, data);
        }
        return RtStatus::Ok;
    }

    let inner_size = unsafe { (*inner_tydesc).size as usize };

    // Allocate heap storage for the inner value.
    let moved_ptr = unsafe {
        crate::c::dtlv_rti_mem_alloc_local(rt, inner_tydesc, 1)
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
