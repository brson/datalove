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

/// Move a value into its erased shape.
///
/// A generic function is compiled with its type parameters replaced by `data`,
/// so a parameter written `T` becomes `data` and one written `?T` becomes
/// `?data`. This walks the two type descriptors together and converts a value
/// of the caller's type into the shape the callee was compiled for.
///
/// The value is moved. The source is dead afterwards.
///
/// # Safety
///
/// `src_in` must be an initialized value of `src_tydesc`, and `dst_tydesc` must
/// be `src_tydesc` with some positions replaced by `data`.
pub unsafe fn erase_local(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
    dst_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe { convert(rt, src_in, src_tydesc, dst_out, dst_tydesc, Direction::Erase) }
}

/// Move a value back out of its erased shape.
///
/// The inverse of [`erase_local`]. The caller supplies the type it erased from,
/// so this is a move rather than a checked downcast.
///
/// # Safety
///
/// `src_in` must be an initialized value of `src_tydesc`, and `src_tydesc` must
/// be `dst_tydesc` with some positions replaced by `data`.
pub unsafe fn reify_local(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
    dst_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe { convert(rt, src_in, src_tydesc, dst_out, dst_tydesc, Direction::Reify) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Erase,
    Reify,
}

/// Walk a value and its erased counterpart, converting at the `data` positions.
///
/// The two descriptors have the same shape except where one side is `data`.
/// Reaching such a position is the base case; anywhere else the structure is
/// the same on both sides and the payload is converted in place.
unsafe fn convert(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
    dst_tydesc: *const rtdt::TyDesc,
    dir: Direction,
) -> RtStatus {
    unsafe {
        let src_tag = (*src_tydesc).type_tag;
        let dst_tag = (*dst_tydesc).type_tag;

        // The position that was erased.
        //
        // A `data` on the erased side is that position whatever the other side
        // holds, including another `data`: a generic function calling a generic
        // function passes its own already-erased parameter, and that wraps a
        // level the return has to take back off. Deciding by whether the two
        // differ would mistake that for nothing having been erased and leave
        // the wrap in place.
        match dir {
            Direction::Erase if dst_tag == TyTag::Data => {
                return data_from_local(rt, src_in, src_tydesc, dst_out);
            }
            Direction::Reify if src_tag == TyTag::Data => {
                return data_into_local(rt, src_in, dst_out, dst_tydesc);
            }
            _ => {}
        }

        if src_tag != dst_tag {
            return RtStatus::Error;
        }

        match src_tag {
            TyTag::Option => {
                let src_ty = rtdt::TyDescRef::from_ptr(src_tydesc);
                let dst_ty = rtdt::TyDescRef::from_ptr(dst_tydesc);
                let src_layout = rtdt::layout::compute_option_layout(src_ty);
                let dst_layout = rtdt::layout::compute_option_layout(dst_ty);

                let tag = *(src_in as *const u8);
                *(dst_out as *mut u8) = tag;
                if tag == rtdt::OptionTag::None as u8 {
                    return RtStatus::Ok;
                }
                convert(
                    rt,
                    src_in.add(src_layout.payload_offset as usize),
                    src_ty.option_inner_ty().as_ptr(),
                    dst_out.add(dst_layout.payload_offset as usize),
                    dst_ty.option_inner_ty().as_ptr(),
                    dir,
                )
            }
            TyTag::Result => {
                let src_ty = rtdt::TyDescRef::from_ptr(src_tydesc);
                let dst_ty = rtdt::TyDescRef::from_ptr(dst_tydesc);
                let src_layout = rtdt::layout::compute_result_layout(src_ty);
                let dst_layout = rtdt::layout::compute_result_layout(dst_ty);

                let tag = *(src_in as *const u8);
                *(dst_out as *mut u8) = tag;
                if tag != rtdt::ResultTag::Ok as u8 {
                    // The error side is an Error value, which is the same type
                    // on both sides, so it moves across unchanged.
                    let size = std::mem::size_of::<rtdt::Error>();
                    std::ptr::copy_nonoverlapping(
                        src_in.add(src_layout.payload_offset as usize),
                        dst_out.add(dst_layout.payload_offset as usize),
                        size,
                    );
                    return RtStatus::Ok;
                }
                convert(
                    rt,
                    src_in.add(src_layout.payload_offset as usize),
                    src_ty.result_ok_ty().as_ptr(),
                    dst_out.add(dst_layout.payload_offset as usize),
                    dst_ty.result_ok_ty().as_ptr(),
                    dir,
                )
            }
            // A term is its payload under a name, laid out exactly as the
            // payload is, so converting one is converting that at the same
            // address with a different descriptor.
            TyTag::Term => {
                let src_ty = rtdt::TyDescRef::from_ptr(src_tydesc);
                let dst_ty = rtdt::TyDescRef::from_ptr(dst_tydesc);
                let (_, src_payload) = src_ty.term_info();
                let (_, dst_payload) = dst_ty.term_info();
                convert(rt, src_in, src_payload.as_ptr(), dst_out, dst_payload.as_ptr(), dir)
            }

            // An enum is a payload chosen by a discriminant, which is the
            // option arm with more than one thing it could be. The
            // discriminant crosses as it stands and the variant it names says
            // which payload to convert and where each side keeps it.
            TyTag::Enum => {
                let src_ty = rtdt::TyDescRef::from_ptr(src_tydesc);
                let dst_ty = rtdt::TyDescRef::from_ptr(dst_tydesc);
                let src_info = src_ty.enum_info();
                let dst_info = dst_ty.enum_info();

                let discriminant = *(src_in as *const u32);
                *(dst_out as *mut u32) = discriminant;

                let index = discriminant as usize;
                if index >= src_info.num_variants() as usize
                    || index >= dst_info.num_variants() as usize
                {
                    return RtStatus::Error;
                }
                let (Some(src_variant), Some(dst_variant)) =
                    (src_info.variant(index), dst_info.variant(index))
                else {
                    return RtStatus::Error;
                };
                match (src_variant.payload(), dst_variant.payload()) {
                    (Some(src_payload), Some(dst_payload)) => convert(
                        rt,
                        src_in.add(src_variant.offset() as usize),
                        src_payload.as_ptr(),
                        dst_out.add(dst_variant.offset() as usize),
                        dst_payload.as_ptr(),
                        dir,
                    ),
                    // An atom carries nothing, so the discriminant was all of
                    // it. The two sides disagreeing about that is the front
                    // end having admitted two shapes that are not the same
                    // enum.
                    (None, None) => RtStatus::Ok,
                    _ => RtStatus::Error,
                }
            }

            // A tuple and a struct hold their fields inline, packed by size, so
            // converting one means converting each field and writing it at the
            // other side's offset. How many fields there are is fixed by the
            // type rather than by the data, which is what separates these from
            // a collection: `(u32, u32)` is eight bytes where `(data, data)`
            // is thirty-two, so there is real work, but it is a walk of known
            // length rather than of the value's contents.
            //
            // A field that fails after an earlier one succeeded leaves what
            // was already moved in the destination and the rest in the source.
            // Only allocation failure can do that, and neither side is
            // destroyed here, so nothing is freed twice; what is lost is the
            // moved prefix.
            TyTag::Tuple => {
                let src_ty = rtdt::TyDescRef::from_ptr(src_tydesc);
                let dst_ty = rtdt::TyDescRef::from_ptr(dst_tydesc);
                let src_layout = rtdt::layout::compute_tuple_layout(src_ty);
                let dst_layout = rtdt::layout::compute_tuple_layout(dst_ty);
                if src_layout.field_offsets.len() != dst_layout.field_offsets.len() {
                    return RtStatus::Error;
                }
                let src_fields: Vec<_> = src_ty.iter_tuple_fields().collect();
                let dst_fields: Vec<_> = dst_ty.iter_tuple_fields().collect();
                for i in 0..src_layout.field_offsets.len() {
                    let status = convert(
                        rt,
                        src_in.add(src_layout.field_offsets[i] as usize),
                        src_fields[i].tydesc().as_ptr(),
                        dst_out.add(dst_layout.field_offsets[i] as usize),
                        dst_fields[i].tydesc().as_ptr(),
                        dir,
                    );
                    if status != RtStatus::Ok {
                        return status;
                    }
                }
                RtStatus::Ok
            }
            TyTag::Struct => {
                let src_ty = rtdt::TyDescRef::from_ptr(src_tydesc);
                let dst_ty = rtdt::TyDescRef::from_ptr(dst_tydesc);
                let src_layout = rtdt::layout::compute_struct_layout(src_ty);
                let dst_layout = rtdt::layout::compute_struct_layout(dst_ty);
                if src_layout.field_offsets.len() != dst_layout.field_offsets.len() {
                    return RtStatus::Error;
                }
                let src_fields: Vec<_> = src_ty.iter_struct_fields().collect();
                let dst_fields: Vec<_> = dst_ty.iter_struct_fields().collect();
                for i in 0..src_layout.field_offsets.len() {
                    let status = convert(
                        rt,
                        src_in.add(src_layout.field_offsets[i] as usize),
                        src_fields[i].tydesc().as_ptr(),
                        dst_out.add(dst_layout.field_offsets[i] as usize),
                        dst_fields[i].tydesc().as_ptr(),
                        dir,
                    );
                    if status != RtStatus::Ok {
                        return status;
                    }
                }
                RtStatus::Ok
            }
            // Nothing was erased here, so the value moves across as it is.
            //
            // That holds only while the two descriptors say the same thing,
            // which is the front end's to guarantee: it admits a type
            // parameter alone or under `?` and `!`, and those are handled
            // above. A shape it starts admitting without a case here would
            // arrive as two descriptors that differ, and copying one size
            // over the other is how that becomes silent corruption rather
            // than a refusal -- a `(u32, u32)` is eight bytes where a
            // `(data, data)` wants thirty-two.
            //
            // Sizes agreeing does not prove the two are the same type, only
            // that this copy is not the obviously wrong length. A container
            // holds its element type in its descriptor rather than its
            // layout, so `[u32]` and `[data]` are the same size and moving
            // one as the other moves the container correctly and says nothing
            // about the elements. Admitting those means deciding who carries
            // the element descriptor, not writing a case here.
            _ => {
                let src_size = (*src_tydesc).size;
                if src_size != (*dst_tydesc).size {
                    return RtStatus::Error;
                }
                std::ptr::copy_nonoverlapping(src_in, dst_out, src_size as usize);
                RtStatus::Ok
            }
        }
    }
}

/// Clone a value into a destination that may be the erased shape.
///
/// A `@` inside a generic clones something whose type only the descriptor
/// knows, into a slot the function was compiled to hold a `data` in. Deciding
/// between cloning and cloning-then-wrapping needs both descriptors, and the
/// call site has only one of them statically, so the decision is here.
///
/// # Safety
///
/// `src_in` must be an initialized value of `src_tydesc`, and `dst_out` must
/// have room for a value of `dst_tydesc`.
pub unsafe fn clone_erased_local(
    rt: LocalRtHandle,
    src_in: *const u8,
    src_tydesc: *const rtdt::TyDesc,
    dst_out: *mut u8,
    dst_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let wraps = (*dst_tydesc).type_tag == TyTag::Data
            && (*src_tydesc).type_tag != TyTag::Data;
        if wraps {
            data_clone_from_local(rt, src_in, src_tydesc, dst_out)
        } else {
            crate::impls::clone::clone_value(rt, src_in, src_tydesc, dst_out)
        }
    }
}

/// The value a wrapped one holds, and the descriptor saying what it is.
///
/// Borrowing rather than moving: the `Data` keeps what it holds, and both of
/// these point into it. A container of a type parameter crosses an owned
/// boundary wrapped, and anything wanting to read it as the container it is
/// needs the pair the rest of the runtime speaks in.
///
/// Only a wrapper holding its value on the heap has a value to point at, which
/// is every wrapper a container makes: `can_inline` admits no container, so one
/// is never packed into the two words.
///
/// # Safety
///
/// `data_in` must be an initialized `Data`.
pub unsafe fn data_parts(
    data_in: *const u8,
    value_out: *mut *const u8,
    tydesc_out: *mut *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let data = &*(data_in as *const rtdt::Data);
        if data.tag() != rtdt::anypack::Tag::TwoPointers {
            // A value packed into the words has no address to lend, and
            // nothing that reaches here should be packed.
            return RtStatus::Error;
        }
        let value_ptr = data.value_ptr();
        let tydesc = data.tydesc();
        if value_ptr.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }
        std::ptr::write(value_out, value_ptr);
        std::ptr::write(tydesc_out, tydesc);
        RtStatus::Ok
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

/// Create a Data holding a clone of a value the caller keeps.
///
/// The moving form, [`data_from_local`], is what a generic call site wants: it
/// is handing the value over. Reading an element out of a container the caller
/// still owns is the other case, and it has to leave the original intact, so
/// the value is cloned rather than copied.
///
/// A scalar goes in the two words as it does for the moving form, because
/// cloning one is copying it.
///
/// # Safety
///
/// `inner_in` must be an initialized value of the type `inner_tydesc`
/// describes, and `dest_out` must have room for a `Data`.
pub unsafe fn data_clone_from_local(
    rt: LocalRtHandle,
    inner_in: *const u8,
    inner_tydesc: *const rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    let tytag = unsafe { (*inner_tydesc).type_tag };
    if tytag.can_inline() {
        unsafe {
            let data = inline_data(inner_in, inner_tydesc, tytag);
            std::ptr::write(dest_out as *mut rtdt::Data, data);
        }
        return RtStatus::Ok;
    }

    // Clone straight into the box the data will own, rather than cloning to a
    // temporary and moving that in.
    let box_ptr = unsafe {
        crate::c::dtlv_rti_mem_alloc_local(rt, inner_tydesc, 1)
    };
    if box_ptr.is_null() {
        return RtStatus::Error;
    }

    let status = unsafe {
        crate::impls::clone::clone_value(rt, inner_in, inner_tydesc, box_ptr)
    };
    if status != RtStatus::Ok {
        return status;
    }

    unsafe {
        let data = rtdt::Data::from_pointers(inner_tydesc, box_ptr);
        std::ptr::write(dest_out as *mut rtdt::Data, data);
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
    let tytag = unsafe { (*inner_tydesc).type_tag };
    unsafe { data_from_local_tagged(rt, inner_in, tytag, inner_tydesc, dest_out) }
}

/// Create a Data from a value whose tag is already in hand.
///
/// The narrow scalars pack into the words with no descriptor at all, so a
/// value taken back out of one has none to give. Passing the tag separately
/// lets those be re-wrapped from what is known, rather than making up a
/// descriptor to hold a tag that is already here.
///
/// `inner_tydesc` may be null exactly when the tag is one of those.
pub unsafe fn data_from_local_tagged(
    rt: LocalRtHandle,
    inner_in: *const u8,
    tytag: TyTag,
    inner_tydesc: *const rtdt::TyDesc,
    dest_out: *mut u8,
) -> RtStatus {
    // A scalar goes in the two words directly rather than onto the heap.
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
