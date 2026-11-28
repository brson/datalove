//! Reading the binary encoding into a runtime value.
//!
//! Decoding reads the input twice. The first pass checks everything, and
//! allocates nothing on the runtime heap, so a rejected input leaves nothing
//! to clean up. It also counts every collection's elements, in the order it
//! meets them, so that the second pass, which builds the value and trusts the
//! input, allocates each at its final size.

use datalove_rt::c::{LocalRtHandle, RtStatus};
use datalove_rtdt as rtdt;
use crate::read::{
    DecodeError, DecodeErrorKind, Reader, read_escaped, read_string, read_varint,
};
use crate::tybuild::TyArena;
use datalove_rtdt::{TyDescRef, TyTag};

/// How deeply a value may nest before the decoder refuses it.
pub const MAX_VALUE_DEPTH: usize = 512;

/// Decode a value of the type `td` describes from `bytes`, which hold it and
/// nothing else, writing it into `out`.
///
/// On success the value is the caller's, to destroy through `rt`. The
/// descriptors of any `data` in it are made in `arena`, which must outlive
/// it. On failure nothing has been written or allocated.
///
/// # Safety
///
/// `out` is writable space of the type's size and alignment, `rt` is a live
/// runtime of this thread, and `td` and every descriptor it reaches are live.
pub unsafe fn decode(
    rt: LocalRtHandle,
    arena: &mut TyArena,
    bytes: &[u8],
    td: TyDescRef<'_>,
    out: rtdt::AnyOut<'_>,
) -> Result<(), DecodeError> {
    let mut check = Pass { r: Reader::new(bytes), arena: &mut *arena, build: None, counts: Vec::new(), next_count: 0 };
    unsafe { check.value(td, std::ptr::null_mut(), 0)? };
    check.r.finish()?;
    let counts = check.counts;

    let mut build = Pass { r: Reader::new(bytes), arena, build: Some(rt), counts, next_count: 0 };
    unsafe { build.value(td, out.as_ptr(), 0) }
        .expect("the second pass reads what the first accepted");
    Ok(())
}

struct Pass<'a, 'b> {
    r: Reader<'a>,
    arena: &'b mut TyArena,
    /// The runtime to build with, on the second pass.
    build: Option<LocalRtHandle>,
    /// Each collection's length, in the order the first pass met them.
    counts: Vec<usize>,
    next_count: usize,
}

/// Whether a type's values encode as no bytes at all, which makes them all
/// one value and of no size.
fn encodes_empty(td: TyDescRef<'_>) -> bool {
    match td.type_tag() {
        TyTag::Atom => true,
        TyTag::Tuple => td.iter_tuple_fields().all(|f| encodes_empty(f.tydesc())),
        TyTag::Struct => td.iter_struct_fields().all(|f| encodes_empty(f.tydesc())),
        TyTag::Term => encodes_empty(td.term_info().1),
        _ => false,
    }
}

impl Pass<'_, '_> {
    fn err(&self, at: usize, kind: DecodeErrorKind) -> DecodeError {
        DecodeError { at, kind }
    }

    /// Begin a collection: on the first pass, a slot for its length, filled
    /// by [`Pass::end_count`]; on the second, the length.
    fn begin_count(&mut self) -> usize {
        if self.build.is_some() {
            let n = self.counts[self.next_count];
            self.next_count += 1;
            n
        } else {
            self.counts.push(0);
            self.counts.len() - 1
        }
    }

    fn end_count(&mut self, slot: usize, n: usize) {
        if self.build.is_none() {
            self.counts[slot] = n;
        }
    }

    /// Read whether another element follows: `01` before each, `00` after
    /// the last.
    fn more(&mut self) -> Result<bool, DecodeError> {
        self.r.flag()
    }

    /// Read or build a value. `dest` is null on the first pass.
    unsafe fn value(&mut self, td: TyDescRef<'_>, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        if depth > MAX_VALUE_DEPTH {
            return Err(self.r.error(DecodeErrorKind::TooDeep));
        }
        let building = self.build.is_some();
        macro_rules! fixed {
            ($t:ty, $flip:expr) => {{
                let bits = <$t>::from_be_bytes(self.r.array()?) ^ $flip;
                if building {
                    std::ptr::write(dest as *mut $t, bits);
                }
            }};
        }
        unsafe {
            match td.type_tag() {
                TyTag::Bool => {
                    let at = self.r.pos();
                    let b = self.r.byte()?;
                    if b > 1 {
                        return Err(self.err(at, DecodeErrorKind::BadByte(b)));
                    }
                    if building {
                        *dest = b;
                    }
                }
                TyTag::U8 => fixed!(u8, 0),
                TyTag::I8 => fixed!(u8, 0x80),
                TyTag::U16 => fixed!(u16, 0),
                TyTag::I16 => fixed!(u16, 1 << 15),
                TyTag::U32 => fixed!(u32, 0),
                TyTag::I32 => fixed!(u32, 1 << 31),
                TyTag::U64 => fixed!(u64, 0),
                TyTag::I64 => fixed!(u64, 1 << 63),
                TyTag::Index => fixed!(rtdt::IndexRepr, 0),
                TyTag::Offset => fixed!(rtdt::IndexRepr, 1 << (rtdt::IndexRepr::BITS - 1)),
                TyTag::F32 => {
                    let ordered = u32::from_be_bytes(self.r.array()?);
                    let bits = if ordered >> 31 == 1 { ordered ^ 1 << 31 } else { !ordered };
                    if building {
                        std::ptr::write(dest as *mut u32, bits);
                    }
                }
                TyTag::F64 => {
                    let ordered = u64::from_be_bytes(self.r.array()?);
                    let bits = if ordered >> 63 == 1 { ordered ^ 1 << 63 } else { !ordered };
                    if building {
                        std::ptr::write(dest as *mut u64, bits);
                    }
                }
                TyTag::Int => {
                    let (negative, limbs) = crate::read_int(&mut self.r)?;
                    if let Some(rt) = self.build {
                        let status = datalove_rt::c::dtlv_rti_int_from_limbs(
                            rt, limbs.as_ptr(), limbs.len() as u32, negative, rtdt::AnyOut::new(dest), td.as_ref());
                        assert_eq!(status, RtStatus::Ok, "an int is made");
                    }
                }
                TyTag::String => {
                    let s = read_string(&mut self.r)?;
                    if let Some(rt) = self.build {
                        let status = datalove_rt::c::dtlv_rti_string_from_bytes(
                            rt, s.as_ptr(), s.len() as rtdt::IndexRepr, rtdt::AnyOut::new(dest), td.as_ref());
                        assert_eq!(status, RtStatus::Ok, "a string is made");
                    }
                }
                TyTag::Tuple => {
                    for field in td.iter_tuple_fields() {
                        self.value(field.tydesc(), offset(dest, field.offset()), depth + 1)?;
                    }
                }
                TyTag::Struct => {
                    for field in td.iter_struct_fields() {
                        self.value(field.tydesc(), offset(dest, field.offset()), depth + 1)?;
                    }
                }
                TyTag::Atom => {}
                TyTag::Term => self.value(td.term_info().1, dest, depth + 1)?,
                TyTag::Enum => {
                    let at = self.r.pos();
                    let name = read_string(&mut self.r)?;
                    let info = td.enum_info();
                    let Some(index) = (0..info.num_variants() as usize)
                        .find(|&i| info.variant(i).expect("in range").name() == name)
                    else {
                        return Err(self.err(at, DecodeErrorKind::NoSuchVariant));
                    };
                    let variant = info.variant(index).expect("in range");
                    if let Some(payload) = variant.payload() {
                        self.value(payload, offset(dest, variant.offset()), depth + 1)?;
                    }
                    if building {
                        std::ptr::write(dest as *mut u32, index as u32);
                    }
                }
                TyTag::Option => {
                    if self.r.flag()? {
                        let layout = rtdt::layout::compute_option_layout(td);
                        self.value(td.option_inner_ty(), offset(dest, layout.payload_offset), depth + 1)?;
                        if building {
                            std::ptr::write(dest as *mut rtdt::OptionTag, rtdt::OptionTag::Some);
                        }
                    } else if building {
                        std::ptr::write(dest as *mut rtdt::OptionTag, rtdt::OptionTag::None);
                    }
                }
                TyTag::Result => {
                    let layout = rtdt::layout::compute_result_layout(td);
                    let payload = offset(dest, layout.payload_offset);
                    if self.r.flag()? {
                        self.value(td.result_ok_ty(), payload, depth + 1)?;
                        if building {
                            std::ptr::write(dest as *mut rtdt::ResultTag, rtdt::ResultTag::Ok);
                        }
                    } else {
                        self.packed(TyTag::Error, payload, depth)?;
                        if building {
                            std::ptr::write(dest as *mut rtdt::ResultTag, rtdt::ResultTag::Err);
                        }
                    }
                }
                TyTag::List => self.list(td, dest, depth)?,
                TyTag::Set => self.set(td, dest, depth)?,
                TyTag::Map => self.map(td, dest, depth)?,
                TyTag::Table => self.table(td, dest, depth)?,
                TyTag::Tensor => self.tensor(td, dest, depth)?,
                TyTag::Data | TyTag::Error => self.packed(td.type_tag(), dest, depth)?,
                TyTag::Param => unreachable!("a template's parameter is not the type of a value"),
            }
        }
        Ok(())
    }

    /// A `data` or `error`: a type, then a value of it, packed as the runtime
    /// packs one.
    unsafe fn packed(&mut self, tag: TyTag, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        let inner = self.arena.read_type(&mut self.r)?;
        // SAFETY: made by the arena, which outlives this.
        let inner = unsafe { TyDescRef::from_ptr(inner) };
        let Some(rt) = self.build else {
            return unsafe { self.value(inner, std::ptr::null_mut(), depth + 1) };
        };
        unsafe {
            let temp = datalove_rt::c::dtlv_rti_mem_alloc_local(rt, inner.as_ref(), 1);
            self.value(inner, temp, depth + 1)?;
            let status = if tag == TyTag::Data {
                datalove_rt::c::dtlv_rti_data_from_local(rt, rtdt::AnyOwned::new(temp), inner.as_ref(), rtdt::AnyOut::new(dest))
            } else {
                datalove_rt::c::dtlv_rti_error_from_local(rt, rtdt::AnyOwned::new(temp), inner.as_ref(), rtdt::AnyOut::new(dest))
            };
            assert_eq!(status, RtStatus::Ok, "a data is made");
            // Moved out of, so freed without being destroyed.
            datalove_rt::c::dtlv_rti_mem_free_local(rt, inner.as_ref(), 1, temp);
        }
        Ok(())
    }

    unsafe fn list(&mut self, td: TyDescRef<'_>, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        let element = td.list_element_ty();
        let size = element.size() as usize;
        if element.type_tag() == TyTag::U8 {
            let mut bytes = Vec::new();
            read_escaped(&mut self.r, &mut bytes)?;
            if let Some(rt) = self.build {
                unsafe {
                    let data = alloc(rt, element, bytes.len());
                    if !bytes.is_empty() {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
                    }
                    write_list(dest, data, bytes.len());
                }
            }
            return Ok(());
        }
        let slot = self.begin_count();
        let (data, len) = match self.build {
            Some(rt) => (unsafe { alloc(rt, element, slot) }, slot),
            None => (std::ptr::null_mut(), 0),
        };
        let mut n = 0;
        while self.more()? {
            unsafe { self.value(element, offset_n(data, n, size), depth + 1)? };
            n += 1;
        }
        self.end_count(slot, n);
        if self.build.is_some() {
            debug_assert_eq!(n, len);
            unsafe { write_list(dest, data, len) };
        }
        Ok(())
    }

    /// Read elements of a set, or entries of a map, each strictly above the
    /// one before, into staging buffers on the second pass.
    unsafe fn tree_entries(
        &mut self,
        key: TyDescRef<'_>,
        value: Option<TyDescRef<'_>>,
        depth: usize,
    ) -> Result<(usize, *mut u8, *mut u8), DecodeError> {
        let slot = self.begin_count();
        let (keys, values) = match self.build {
            Some(rt) => unsafe { (alloc(rt, key, slot), value.map_or(std::ptr::null_mut(), |v| alloc(rt, v, slot))) },
            None => (std::ptr::null_mut(), std::ptr::null_mut()),
        };
        let mut n = 0;
        let mut previous: Option<(usize, usize)> = None;
        while self.more()? {
            let start = self.r.pos();
            unsafe { self.value(key, offset_n(keys, n, key.size() as usize), depth + 1)? };
            let end = self.r.pos();
            if self.build.is_none()
                && let Some((ps, pe)) = previous
            {
                let bytes = self.r.since(0);
                if bytes[ps..pe] >= bytes[start..end] {
                    return Err(self.err(start, DecodeErrorKind::NotAscending));
                }
            }
            previous = Some((start, end));
            if let Some(value) = value {
                unsafe { self.value(value, offset_n(values, n, value.size() as usize), depth + 1)? };
            }
            n += 1;
        }
        self.end_count(slot, n);
        Ok((n, keys, values))
    }

    unsafe fn set(&mut self, td: TyDescRef<'_>, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        let element = td.set_element_ty();
        let (n, elements, _) = unsafe { self.tree_entries(element, None, depth)? };
        if let Some(rt) = self.build {
            unsafe {
                let status = datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
                    rt, rtdt::AnyOut::new(dest), element.as_ref(), elements, n as rtdt::IndexRepr);
                assert_eq!(status, RtStatus::Ok, "a set is built");
                free(rt, element, n, elements);
            }
        }
        Ok(())
    }

    unsafe fn map(&mut self, td: TyDescRef<'_>, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        let (key, value) = (td.map_key_ty(), td.map_value_ty());
        let (n, keys, values) = unsafe { self.tree_entries(key, Some(value), depth)? };
        if let Some(rt) = self.build {
            unsafe {
                let status = datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
                    rt, rtdt::AnyOut::new(dest), key.as_ref(), value.as_ref(), keys, values, n as rtdt::IndexRepr);
                assert_eq!(status, RtStatus::Ok, "a map is built");
                free(rt, key, n, keys);
                free(rt, value, n, values);
            }
        }
        Ok(())
    }

    unsafe fn table(&mut self, td: TyDescRef<'_>, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        let columns: Vec<TyDescRef<'_>> = td.table_column_tydescs().map(|c| c.tydesc()).collect();
        let row_td = {
            let ptrs: Vec<*const rtdt::TyDesc> = columns.iter().map(|c| c.as_ptr()).collect();
            // SAFETY: made by the arena, which outlives this.
            unsafe { TyDescRef::from_ptr(self.arena.tuple(&ptrs)) }
        };
        let slot = self.begin_count();
        let rows = match self.build {
            Some(rt) => unsafe { alloc(rt, row_td, slot) },
            None => std::ptr::null_mut(),
        };
        let mut n = 0;
        while self.more()? {
            let row = offset_n(rows, n, row_td.size() as usize);
            for field in row_td.iter_tuple_fields() {
                unsafe { self.value(field.tydesc(), offset(row, field.offset()), depth + 1)? };
            }
            n += 1;
        }
        self.end_count(slot, n);
        if let Some(rt) = self.build {
            unsafe {
                if columns.is_empty() {
                    // The runtime's builder makes a table of no columns empty,
                    // whatever the rows; this one keeps how many there were.
                    std::ptr::write(dest as *mut rtdt::Table, rtdt::Table {
                        len: rtdt::Index(n as rtdt::IndexRepr),
                        capacity: rtdt::Index::ZERO,
                        data: std::ptr::null(),
                    });
                } else {
                    let status = datalove_rt::c::dtlv_rti_table_build_from_rows_local(
                        rt, rtdt::AnyOut::new(dest), td.as_ref(), rows, row_td.as_ref(), n as rtdt::IndexRepr);
                    assert_eq!(status, RtStatus::Ok, "a table is built");
                }
                free(rt, row_td, n, rows);
            }
        }
        Ok(())
    }

    unsafe fn tensor(&mut self, td: TyDescRef<'_>, dest: *mut u8, depth: usize) -> Result<(), DecodeError> {
        let element = td.tensor_element_ty();
        let rank = td.tensor_rank() as usize;
        let at = self.r.pos();
        let mut shape = Vec::with_capacity(rank);
        let mut total: rtdt::IndexRepr = 1;
        for _ in 0..rank {
            let extent_at = self.r.pos();
            let extent = read_varint(&mut self.r)?;
            let extent = rtdt::IndexRepr::try_from(extent)
                .map_err(|_| self.err(extent_at, DecodeErrorKind::OutOfRange))?;
            total = total.checked_mul(extent).ok_or(self.err(extent_at, DecodeErrorKind::OutOfRange))?;
            shape.push(extent);
        }
        let empty = encodes_empty(element);
        // Each element takes at least a byte, unless none does, so a count
        // past what is left is refused before anything is allocated for it.
        if !empty && total as u64 > self.r.remaining() as u64 {
            return Err(self.err(at, DecodeErrorKind::Truncated));
        }
        let count = total as usize;
        let data = match self.build {
            Some(rt) if count > 0 => unsafe { alloc(rt, element, count) },
            _ => std::ptr::null_mut(),
        };
        if !empty {
            let size = element.size() as usize;
            for i in 0..count {
                unsafe { self.value(element, offset_n(data, i, size), depth + 1)? };
            }
        }
        if let Some(rt) = self.build {
            unsafe {
                let index_td = self.arena.scalar(TyTag::Index);
                let shape_ptr = alloc(rt, TyDescRef::from_ptr(index_td), rank) as *mut rtdt::IndexRepr;
                let strides_ptr = alloc(rt, TyDescRef::from_ptr(index_td), rank) as *mut rtdt::IndexRepr;
                for (i, &extent) in shape.iter().enumerate() {
                    *shape_ptr.add(i) = extent;
                    // Row-major, as a tensor literal is made.
                    let stride: rtdt::IndexRepr = shape[i + 1..].iter().product();
                    *strides_ptr.add(i) = if stride == 0 { 1 } else { stride };
                }
                std::ptr::write(dest as *mut rtdt::Tensor, rtdt::Tensor {
                    ptr_base: data,
                    capacity_elems: rtdt::Index(total),
                    offset_elems: rtdt::Index::ZERO,
                    shape: shape_ptr as *const rtdt::Index,
                    strides: strides_ptr as *const rtdt::Index,
                    layout: rtdt::TensorLayout::RowMajor,
                });
            }
        }
        Ok(())
    }
}

fn offset(p: *mut u8, by: u32) -> *mut u8 {
    p.wrapping_add(by as usize)
}

fn offset_n(p: *mut u8, n: usize, size: usize) -> *mut u8 {
    p.wrapping_add(n * size)
}

/// Room for `n` values of a type on the runtime heap, or null for none.
unsafe fn alloc(rt: LocalRtHandle, td: TyDescRef<'_>, n: usize) -> *mut u8 {
    if n == 0 {
        return std::ptr::null_mut();
    }
    let p = unsafe { datalove_rt::c::dtlv_rti_mem_alloc_local(rt, td.as_ref(), n as rtdt::IndexRepr) };
    assert!(!p.is_null(), "the runtime allocates");
    p
}

unsafe fn free(rt: LocalRtHandle, td: TyDescRef<'_>, n: usize, p: *mut u8) {
    if n > 0 {
        unsafe { datalove_rt::c::dtlv_rti_mem_free_local(rt, td.as_ref(), n as rtdt::IndexRepr, p) };
    }
}

unsafe fn write_list(dest: *mut u8, data: *mut u8, len: usize) {
    let len = rtdt::Index(len as rtdt::IndexRepr);
    unsafe { std::ptr::write(dest as *mut rtdt::List, rtdt::List { data, size: len, capacity: len }) };
}
