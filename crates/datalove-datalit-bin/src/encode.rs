//! Writing a runtime value in the binary encoding.

use datalove_rt::c::{LocalRtHandle, RtStatus};
use datalove_rtdt as rtdt;
use datalove_rtdt::ordered::{write_escaped, write_str, write_varint};
use datalove_rtdt::tyenc::encode_type;
use datalove_rtdt::{TyDescRef, TyTag};

/// Append the encoding of a value of the type `td` describes.
///
/// # Safety
///
/// `value` points to an initialized value of that type, made by the runtime
/// `rt` is a handle to, and every descriptor it reaches is live.
pub unsafe fn encode(rt: LocalRtHandle, td: TyDescRef<'_>, value: *const u8, out: &mut Vec<u8>) {
    unsafe { value_into(rt, td, value, out) }
}

/// Append the self-describing encoding of a value: its type's, then its own.
///
/// This is the encoding of the value as a `data` holding it.
///
/// # Safety
///
/// As for [`encode`].
pub unsafe fn encode_data(rt: LocalRtHandle, td: TyDescRef<'_>, value: *const u8, out: &mut Vec<u8>) {
    encode_type(td, out);
    unsafe { value_into(rt, td, value, out) }
}

unsafe fn read<T: Copy>(p: *const u8) -> T {
    unsafe { std::ptr::read(p as *const T) }
}

unsafe fn value_into(rt: LocalRtHandle, td: TyDescRef<'_>, p: *const u8, out: &mut Vec<u8>) {
    unsafe {
        match td.type_tag() {
            TyTag::Bool => out.push(read::<u8>(p)),
            TyTag::U8 => out.push(read::<u8>(p)),
            TyTag::I8 => out.push(read::<u8>(p) ^ 0x80),
            TyTag::U16 => out.extend_from_slice(&read::<u16>(p).to_be_bytes()),
            TyTag::I16 => out.extend_from_slice(&(read::<u16>(p) ^ 1 << 15).to_be_bytes()),
            TyTag::U32 => out.extend_from_slice(&read::<u32>(p).to_be_bytes()),
            TyTag::I32 => out.extend_from_slice(&(read::<u32>(p) ^ 1 << 31).to_be_bytes()),
            TyTag::U64 => out.extend_from_slice(&read::<u64>(p).to_be_bytes()),
            TyTag::I64 => out.extend_from_slice(&(read::<u64>(p) ^ 1 << 63).to_be_bytes()),
            TyTag::Index => out.extend_from_slice(&read::<rtdt::IndexRepr>(p).to_be_bytes()),
            TyTag::Offset => {
                let bits = read::<rtdt::OffsetRepr>(p) as rtdt::IndexRepr;
                out.extend_from_slice(&(bits ^ 1 << (rtdt::IndexRepr::BITS - 1)).to_be_bytes());
            }
            TyTag::F32 => {
                let bits = read::<u32>(p);
                let ordered = if bits >> 31 == 1 { !bits } else { bits ^ 1 << 31 };
                out.extend_from_slice(&ordered.to_be_bytes());
            }
            TyTag::F64 => {
                let bits = read::<u64>(p);
                let ordered = if bits >> 63 == 1 { !bits } else { bits ^ 1 << 63 };
                out.extend_from_slice(&ordered.to_be_bytes());
            }
            TyTag::Int => {
                let int = &*(p as *const rtdt::Int);
                let len = int.size_and_sign.unsigned_abs() as usize;
                let limbs = if len == 0 { &[][..] } else { std::slice::from_raw_parts(int.data, len) };
                crate::write_int(out, int.size_and_sign < 0, limbs);
            }
            TyTag::String => write_escaped(out, (*(p as *const rtdt::String)).as_bytes()),
            TyTag::Tuple => {
                for field in td.iter_tuple_fields() {
                    value_into(rt, field.tydesc(), p.add(field.offset() as usize), out);
                }
            }
            TyTag::Struct => {
                for field in td.iter_struct_fields() {
                    value_into(rt, field.tydesc(), p.add(field.offset() as usize), out);
                }
            }
            TyTag::Atom => {}
            TyTag::Term => value_into(rt, td.term_info().1, p, out),
            TyTag::Enum => {
                let discriminant = read::<u32>(p);
                let variant = td.enum_info().variant(discriminant as usize).expect("a discriminant names a variant");
                write_str(out, variant.name());
                if let Some(payload) = variant.payload() {
                    value_into(rt, payload, p.add(variant.offset() as usize), out);
                }
            }
            TyTag::Option => match read::<rtdt::OptionTag>(p) {
                rtdt::OptionTag::None => out.push(0),
                rtdt::OptionTag::Some => {
                    out.push(1);
                    let layout = rtdt::layout::compute_option_layout(td);
                    value_into(rt, td.option_inner_ty(), p.add(layout.payload_offset as usize), out);
                }
            },
            TyTag::Result => {
                let layout = rtdt::layout::compute_result_layout(td);
                let payload = p.add(layout.payload_offset as usize);
                match read::<rtdt::ResultTag>(p) {
                    rtdt::ResultTag::Err => {
                        out.push(0);
                        packed_into(rt, payload as *const rtdt::Data, out);
                    }
                    rtdt::ResultTag::Ok => {
                        out.push(1);
                        value_into(rt, td.result_ok_ty(), payload, out);
                    }
                }
            }
            TyTag::List => {
                let list = &*(p as *const rtdt::List);
                let element = td.list_element_ty();
                let len = list.size.as_usize();
                if element.type_tag() == TyTag::U8 {
                    let bytes = if len == 0 { &[][..] } else { std::slice::from_raw_parts(list.data, len) };
                    write_escaped(out, bytes);
                } else {
                    let size = element.size() as usize;
                    for i in 0..len {
                        out.push(1);
                        value_into(rt, element, list.data.add(i * size), out);
                    }
                    out.push(0);
                }
            }
            TyTag::Set => {
                let element = td.set_element_ty();
                let mut cursor = std::mem::MaybeUninit::<rtdt::TreeCursor>::uninit();
                let status = datalove_rt::c::dtlv_rti_btreeset_cursor_begin_local(
                    rt, rtdt::AnyRef::new(p), td.as_ref(), cursor.as_mut_ptr());
                assert_eq!(status, RtStatus::Ok, "a set's walk begins");
                let mut cursor = cursor.assume_init();
                while !cursor.leaf.is_null() {
                    out.push(1);
                    value_into(rt, element, cursor.elem, out);
                    datalove_rt::c::dtlv_rti_tree_cursor_advance_local(rt, &mut cursor);
                }
                out.push(0);
            }
            TyTag::Map => {
                let (key, value) = (td.map_key_ty(), td.map_value_ty());
                let mut cursor = std::mem::MaybeUninit::<rtdt::TreeCursor>::uninit();
                let status = datalove_rt::c::dtlv_rti_btreemap_cursor_begin_local(
                    rt, rtdt::AnyRef::new(p), td.as_ref(), cursor.as_mut_ptr());
                assert_eq!(status, RtStatus::Ok, "a map's walk begins");
                let mut cursor = cursor.assume_init();
                while !cursor.leaf.is_null() {
                    out.push(1);
                    value_into(rt, key, cursor.elem, out);
                    value_into(rt, value, cursor.value, out);
                    datalove_rt::c::dtlv_rti_tree_cursor_advance_local(rt, &mut cursor);
                }
                out.push(0);
            }
            TyTag::Table => {
                let table = &*(p as *const rtdt::Table);
                let columns: Vec<TyDescRef<'_>> = td.table_column_tydescs().map(|c| c.tydesc()).collect();
                let refs: Vec<&rtdt::TyDesc> = columns.iter().map(|c| c.as_ref()).collect();
                let offsets: Vec<usize> = (0..columns.len())
                    .map(|c| rtdt::layout::table_column_offset(&refs, c, table.capacity.0))
                    .collect();
                for row in 0..table.len.as_usize() {
                    out.push(1);
                    for (column, offset) in columns.iter().zip(&offsets) {
                        let cell = table.data.add(offset + row * column.size() as usize);
                        value_into(rt, *column, cell, out);
                    }
                }
                out.push(0);
            }
            TyTag::Tensor => {
                let tensor = &*(p as *const rtdt::Tensor);
                let rank = td.tensor_rank() as usize;
                let element = td.tensor_element_ty();
                let shape = std::slice::from_raw_parts(tensor.shape, rank);
                let strides = std::slice::from_raw_parts(tensor.strides, rank);
                for extent in shape {
                    write_varint(out, extent.0 as u64);
                }
                if shape.iter().any(|e| e.0 == 0) {
                    return;
                }
                // Row-major: the last index moves fastest. A stride may stand
                // for a negative one, so offsets are added with wrapping.
                let mut index = vec![0 as rtdt::IndexRepr; rank];
                'elements: loop {
                    let mut offset = tensor.offset_elems.0;
                    for (i, stride) in index.iter().zip(strides) {
                        offset = offset.wrapping_add(i.wrapping_mul(stride.0));
                    }
                    let at = tensor.ptr_base.add(offset as usize * element.size() as usize);
                    value_into(rt, element, at, out);
                    for axis in (0..rank).rev() {
                        index[axis] += 1;
                        if index[axis] < shape[axis].0 {
                            continue 'elements;
                        }
                        index[axis] = 0;
                    }
                    break;
                }
            }
            TyTag::Data | TyTag::Error => packed_into(rt, p as *const rtdt::Data, out),
            TyTag::Param => unreachable!("a template's parameter is not the type of a value"),
        }
    }
}

/// Write what a `data` or `error` holds: its type, then its value.
unsafe fn packed_into(rt: LocalRtHandle, data: *const rtdt::Data, out: &mut Vec<u8>) {
    let mut scratch = datalove_rt::rust::DataScratch::default();
    let mut value: *const u8 = std::ptr::null();
    let mut tydesc: *const rtdt::TyDesc = std::ptr::null();
    unsafe {
        let status = datalove_rt::c::dtlv_rti_data_borrow(
            data as *const u8, scratch.as_mut_ptr(), &mut value, &mut tydesc);
        assert_eq!(status, RtStatus::Ok, "a data lends what it holds");
        let td = TyDescRef::from_ptr(tydesc);
        encode_type(td, out);
        value_into(rt, td, value, out);
    }
}
