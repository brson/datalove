//! Tests for the cursors a `for` loop walks collections with.
//!
//! Every walk goes through the exported protocol -- begin, then advance until
//! the cursor's fields say it is done -- and is compared with what the
//! collection holds, read some other way.

use rmx::prelude::*;
use datalove_rt::c::{self, LocalRtHandle, RtStatus};
use datalove_rtdt as rtdt;
use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::ptr;

struct TyDescArena {
    ptrs: RefCell<Vec<*mut rtdt::TyDesc>>,
}

impl TyDescArena {
    fn new() -> Self {
        Self { ptrs: RefCell::new(Vec::new()) }
    }

    fn alloc(&self, td: rtdt::TyDesc) -> *const rtdt::TyDesc {
        let ptr = Box::into_raw(Box::new(td));
        self.ptrs.borrow_mut().push(ptr);
        ptr
    }

    fn u32(&self) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::U32,
            flags: rtdt::TyDesc::PLAIN,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
        })
    }

    fn u64(&self) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::U64,
            flags: rtdt::TyDesc::PLAIN,
            size: 8,
            align: 8,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
        })
    }

    /// `()`, which is zero-sized.
    fn unit(&self) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            flags: rtdt::TyDesc::PLAIN,
            size: 0,
            align: 1,
            type_info: rtdt::TyInfo { tuple: rtdt::TyInfoTuple { num_fields: 0, fields: ptr::null() } },
        })
    }

    fn list(&self, element_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::List,
            flags: 0,
            size: size_of::<rtdt::List>() as u32,
            align: align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo { list: rtdt::TyInfoList { element_tydesc } },
        })
    }

    fn tensor(&self, element_tydesc: *const rtdt::TyDesc, rank: u32) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tensor,
            flags: 0,
            size: size_of::<rtdt::Tensor>() as u32,
            align: align_of::<rtdt::Tensor>() as u32,
            type_info: rtdt::TyInfo { tensor: rtdt::TyInfoTensor { element_tydesc, rank } },
        })
    }

    fn set(&self, element_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Set,
            flags: 0,
            size: size_of::<rtdt::Set>() as u32,
            align: align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo { set: unsafe { rtdt::TyInfoSet::new(element_tydesc) } },
        })
    }

    fn map(&self, key_tydesc: *const rtdt::TyDesc, value_tydesc: *const rtdt::TyDesc) -> *const rtdt::TyDesc {
        self.alloc(rtdt::TyDesc {
            type_tag: rtdt::TyTag::Map,
            flags: 0,
            size: size_of::<rtdt::Map>() as u32,
            align: align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo { map: unsafe { rtdt::TyInfoMap::new(key_tydesc, value_tydesc) } },
        })
    }
}

impl Drop for TyDescArena {
    fn drop(&mut self) {
        for &ptr in self.ptrs.borrow().iter() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }
}

/// A runtime handle, shut down at the end of the test.
struct Rt(LocalRtHandle);

impl Rt {
    fn new() -> Rt {
        let rt = c::dtlv_rti_init();
        assert!(!rt.is_null());
        Rt(rt)
    }
}

impl Drop for Rt {
    fn drop(&mut self) {
        unsafe { c::dtlv_rti_shutdown(self.0) };
    }
}

// ==================== Walks ====================

/// Move a walk on with a protocol function, checking it succeeded.
unsafe fn step<C>(rt: &Rt, f: unsafe extern "C-unwind" fn(LocalRtHandle, *mut C) -> RtStatus, cursor: *mut C) {
    assert_eq!(unsafe { f(rt.0, cursor) }, RtStatus::Ok);
}

/// Every element a list or rank-1 tensor walk visits, checking each one's
/// index is its position.
unsafe fn walk_stride(rt: &Rt, cursor: &mut rtdt::StrideCursor) -> Vec<*const u8> {
    unsafe {
        let mut seen = Vec::new();
        while cursor.index != cursor.len {
            assert_eq!(cursor.index.0 as usize, seen.len());
            seen.push(cursor.elem);
            step(rt, c::dtlv_rti_stride_cursor_advance_local, cursor);
        }
        seen
    }
}

unsafe fn walk_list_u32(rt: &Rt, list: &rtdt::List, list_td: *const rtdt::TyDesc) -> Vec<u32> {
    unsafe {
        let mut cursor = MaybeUninit::uninit();
        let status = c::dtlv_rti_list_cursor_begin_local(rt.0, list as *const rtdt::List as *const u8, &*list_td, cursor.as_mut_ptr());
        assert_eq!(status, RtStatus::Ok);
        walk_stride(rt, cursor.assume_init_mut()).into_iter().map(|p| *(p as *const u32)).collect()
    }
}

unsafe fn walk_tensor_u32(rt: &Rt, tensor: &rtdt::Tensor, tensor_td: *const rtdt::TyDesc) -> Vec<u32> {
    unsafe {
        let mut cursor = MaybeUninit::uninit();
        let status = c::dtlv_rti_tensor_cursor_begin_local(rt.0, tensor as *const rtdt::Tensor as *const u8, &*tensor_td, cursor.as_mut_ptr());
        assert_eq!(status, RtStatus::Ok);
        walk_stride(rt, cursor.assume_init_mut()).into_iter().map(|p| *(p as *const u32)).collect()
    }
}

/// A tensor of any rank, read out in order by walking rows down to rank 1.
///
/// Each row is walked while its cursor is current, as an inner `for` would:
/// the view it points at lives in the row cursor.
unsafe fn walk_tensor_nested(
    rt: &Rt,
    tensor: *const rtdt::Tensor,
    rank: u32,
    tds: &[*const rtdt::TyDesc],
) -> Vec<u32> {
    unsafe {
        if rank == 1 {
            return walk_tensor_u32(rt, &*tensor, tds[0]);
        }
        let mut cursor = MaybeUninit::uninit();
        let status = c::dtlv_rti_tensor_row_cursor_begin_local(rt.0, tensor as *const u8, &*(tds[rank as usize - 1]), cursor.as_mut_ptr());
        assert_eq!(status, RtStatus::Ok);
        let cursor = cursor.assume_init_mut();
        let mut all = Vec::new();
        let mut rows = 0;
        while cursor.index != cursor.len {
            assert_eq!(cursor.index.0 as usize, rows);
            rows += 1;
            let row = &raw const cursor.view;
            assert_eq!((*row).capacity_elems, rtdt::Index::ZERO, "a row is a view");
            all.extend(walk_tensor_nested(rt, row, rank - 1, tds));
            step(rt, c::dtlv_rti_row_cursor_advance_local, cursor);
        }
        all
    }
}

/// Every element or entry a set or map walk visits, as key and value
/// pointers.
///
/// Before each step it checks the cursor's bookkeeping against the leaf it is
/// on, since compiled code advances on that bookkeeping alone: the current
/// leaf's length, and the key and value at the slot's offset into it.
unsafe fn walk_tree(rt: &Rt, cursor: &mut rtdt::TreeCursor) -> Vec<(*const u8, *const u8)> {
    unsafe {
        let is_map = !cursor.value.is_null() || cursor.values_offset != 0;
        let mut seen = Vec::new();
        while !cursor.leaf.is_null() {
            let leaf_len = *(cursor.leaf.add(rtdt::TREE_NODE_LEN_OFFSET) as *const u32);
            assert_eq!(cursor.leaf_len, leaf_len, "the cursor's leaf length is the leaf's");
            assert!(cursor.slot < cursor.leaf_len);
            let slot = cursor.slot as usize;
            assert_eq!(cursor.elem, cursor.leaf.add(cursor.keys_offset as usize + slot * cursor.elem_stride));
            if is_map {
                assert_eq!(cursor.value, cursor.leaf.add(cursor.values_offset as usize + slot * cursor.value_stride));
            } else {
                assert!(cursor.value.is_null(), "a set's walk has no values");
            }
            seen.push((cursor.elem, cursor.value));
            step(rt, c::dtlv_rti_tree_cursor_advance_local, cursor);
        }
        assert!(cursor.elem.is_null(), "a done cursor points at nothing");
        assert!(cursor.value.is_null(), "a done cursor points at nothing");
        seen
    }
}

unsafe fn begin_set(rt: &Rt, set: &rtdt::Set, set_td: *const rtdt::TyDesc) -> rtdt::TreeCursor {
    unsafe {
        let mut cursor = MaybeUninit::uninit();
        let status = c::dtlv_rti_btreeset_cursor_begin_local(rt.0, set as *const rtdt::Set as *const u8, &*set_td, cursor.as_mut_ptr());
        assert_eq!(status, RtStatus::Ok);
        cursor.assume_init()
    }
}

unsafe fn begin_map(rt: &Rt, map: &rtdt::Map, map_td: *const rtdt::TyDesc) -> rtdt::TreeCursor {
    unsafe {
        let mut cursor = MaybeUninit::uninit();
        let status = c::dtlv_rti_btreemap_cursor_begin_local(rt.0, map as *const rtdt::Map as *const u8, &*map_td, cursor.as_mut_ptr());
        assert_eq!(status, RtStatus::Ok);
        cursor.assume_init()
    }
}

unsafe fn walk_set_u32(rt: &Rt, set: &rtdt::Set, set_td: *const rtdt::TyDesc) -> Vec<u32> {
    unsafe {
        let mut cursor = begin_set(rt, set, set_td);
        walk_tree(rt, &mut cursor).into_iter().map(|(k, _)| *(k as *const u32)).collect()
    }
}

unsafe fn walk_map_u32(rt: &Rt, map: &rtdt::Map, map_td: *const rtdt::TyDesc) -> Vec<(u32, u32)> {
    unsafe {
        let mut cursor = begin_map(rt, map, map_td);
        walk_tree(rt, &mut cursor).into_iter().map(|(k, v)| (*(k as *const u32), *(v as *const u32))).collect()
    }
}

// ==================== Builders ====================

unsafe fn list_of<T: Copy>(rt: &Rt, values: &[T], list_td: *const rtdt::TyDesc, elem_td: *const rtdt::TyDesc) -> rtdt::List {
    unsafe {
        let mut list = MaybeUninit::<rtdt::List>::uninit();
        assert_eq!(c::dtlv_rti_list_create_local(rt.0, list.as_mut_ptr() as *mut u8, &*list_td), RtStatus::Ok);
        let mut list = list.assume_init();
        for &value in values {
            let mut v = value;
            let status = c::dtlv_rti_list_push_local(rt.0, &mut list as *mut rtdt::List as *mut u8, &*list_td, &mut v as *mut T as *mut u8, &*elem_td);
            assert_eq!(status, RtStatus::Ok);
        }
        list
    }
}

unsafe fn destroy_list(rt: &Rt, list: &mut rtdt::List, list_td: *const rtdt::TyDesc) {
    unsafe { assert_eq!(c::dtlv_rti_list_destroy_local(rt.0, list as *mut rtdt::List as *mut u8, &*list_td), RtStatus::Ok) };
}

fn empty_tensor() -> rtdt::Tensor {
    rtdt::Tensor {
        ptr_base: ptr::null_mut(),
        offset_elems: rtdt::Index::ZERO,
        capacity_elems: rtdt::Index::ZERO,
        shape: ptr::null(),
        strides: ptr::null(),
        layout: rtdt::TensorLayout::RowMajor,
    }
}

/// A row-major tensor of `0..` with the given shape.
unsafe fn tensor_of(rt: &Rt, arena: &TyDescArena, shape: &[u32], tensor_td: *const rtdt::TyDesc) -> rtdt::Tensor {
    unsafe {
        let u32_td = arena.u32();
        let list_td = arena.list(u32_td);
        let count: u32 = shape.iter().product();
        let data: Vec<u32> = (0..count).collect();
        let mut shape_list = list_of(rt, shape, list_td, u32_td);
        let mut tensor = empty_tensor();
        let status = c::dtlv_rti_tensor_create_from_slice_local(
            rt.0, data.as_ptr() as *const u8, count.into(), &*u32_td,
            &mut shape_list as *mut rtdt::List as *mut u8, &*list_td,
            rtdt::TensorLayout::RowMajor as u8,
            &mut tensor as *mut rtdt::Tensor as *mut u8, &*tensor_td,
        );
        assert_eq!(status, RtStatus::Ok);
        tensor
    }
}

unsafe fn destroy_tensor(rt: &Rt, tensor: &mut rtdt::Tensor, tensor_td: *const rtdt::TyDesc) {
    unsafe { assert_eq!(c::dtlv_rti_tensor_destroy_local(rt.0, tensor as *mut rtdt::Tensor as *mut u8, &*tensor_td), RtStatus::Ok) };
}

unsafe fn set_by_inserting(rt: &Rt, values: impl IntoIterator<Item = u32>, set_td: *const rtdt::TyDesc, elem_td: *const rtdt::TyDesc) -> rtdt::Set {
    unsafe {
        let mut set = rtdt::Set { root: ptr::null(), len: rtdt::Index::ZERO };
        assert_eq!(c::dtlv_rti_btreeset_create_local(rt.0, &mut set as *mut rtdt::Set as *mut u8, &*set_td), RtStatus::Ok);
        for x in values {
            let mut x = x;
            let mut inserted = 0u8;
            let status = c::dtlv_rti_btreeset_insert_local(rt.0, &mut set as *mut rtdt::Set as *mut u8, &*set_td, &mut x as *mut u32 as *mut u8, &*elem_td, &mut inserted);
            assert_eq!(status, RtStatus::Ok);
        }
        set
    }
}

unsafe fn destroy_set(rt: &Rt, set: &mut rtdt::Set, set_td: *const rtdt::TyDesc) {
    unsafe { assert_eq!(c::dtlv_rti_btreeset_destroy_local(rt.0, set as *mut rtdt::Set as *mut u8, &*set_td), RtStatus::Ok) };
}

unsafe fn map_by_inserting(rt: &Rt, keys: impl IntoIterator<Item = u32>, map_td: *const rtdt::TyDesc, u32_td: *const rtdt::TyDesc) -> rtdt::Map {
    unsafe {
        let mut map = rtdt::Map { root: ptr::null(), len: rtdt::Index::ZERO };
        assert_eq!(c::dtlv_rti_btreemap_create_local(rt.0, &mut map as *mut rtdt::Map as *mut u8, &*map_td), RtStatus::Ok);
        for k in keys {
            let (mut k, mut v) = (k, k * 10);
            let status = c::dtlv_rti_btreemap_insert_local(
                rt.0, &mut map as *mut rtdt::Map as *mut u8, &*map_td,
                &mut k as *mut u32 as *mut u8, &*u32_td, &mut v as *mut u32 as *mut u8, &*u32_td,
            );
            assert_eq!(status, RtStatus::Ok);
        }
        map
    }
}

/// A map from each key to a value of any plain type.
unsafe fn map_of<V: Copy>(
    rt: &Rt,
    entries: impl IntoIterator<Item = (u32, V)>,
    map_td: *const rtdt::TyDesc,
    u32_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> rtdt::Map {
    unsafe {
        let mut map = rtdt::Map { root: ptr::null(), len: rtdt::Index::ZERO };
        assert_eq!(c::dtlv_rti_btreemap_create_local(rt.0, &mut map as *mut rtdt::Map as *mut u8, &*map_td), RtStatus::Ok);
        for (k, v) in entries {
            let (mut k, mut v) = (k, v);
            let status = c::dtlv_rti_btreemap_insert_local(
                rt.0, &mut map as *mut rtdt::Map as *mut u8, &*map_td,
                &mut k as *mut u32 as *mut u8, &*u32_td, &mut v as *mut V as *mut u8, &*value_td,
            );
            assert_eq!(status, RtStatus::Ok);
        }
        map
    }
}

unsafe fn destroy_map(rt: &Rt, map: &mut rtdt::Map, map_td: *const rtdt::TyDesc) {
    unsafe { assert_eq!(c::dtlv_rti_btreemap_destroy_local(rt.0, map as *mut rtdt::Map as *mut u8, &*map_td), RtStatus::Ok) };
}

/// A fixed shuffle of `0..n`, so a failure reproduces.
fn shuffled(n: u32, seed: u64) -> Vec<u32> {
    let mut xs: Vec<u32> = (0..n).collect();
    let mut state = seed;
    for i in (1..xs.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        xs.swap(i, j);
    }
    xs
}

// ==================== Lists ====================

#[test]
fn test_list_cursor_walks_in_order() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let list_td = arena.list(u32_td);
    for n in datalove_rt_tests::sizes([0u32, 1, 2, 1000]) {
        let values: Vec<u32> = (0..n).map(|x| x * 3 + 1).collect();
        let mut list = unsafe { list_of(&rt, &values, list_td, u32_td) };
        assert_eq!(unsafe { walk_list_u32(&rt, &list, list_td) }, values);
        unsafe { destroy_list(&rt, &mut list, list_td) };
    }
    Ok(())
}

/// A list of `()` has every element at one address. Counting down, rather
/// than comparing with an end pointer, is what walks it.
#[test]
fn test_list_cursor_walks_zero_sized_elements() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let unit_td = arena.unit();
    let list_td = arena.list(unit_td);
    let mut list = unsafe { list_of(&rt, &[(), (), ()], list_td, unit_td) };
    let mut cursor = MaybeUninit::uninit();
    let seen = unsafe {
        let status = c::dtlv_rti_list_cursor_begin_local(rt.0, &list as *const rtdt::List as *const u8, &*list_td, cursor.as_mut_ptr());
        assert_eq!(status, RtStatus::Ok);
        walk_stride(&rt, cursor.assume_init_mut())
    };
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|&p| p == seen[0]));
    unsafe { destroy_list(&rt, &mut list, list_td) };
    Ok(())
}

// ==================== Tensors ====================

#[test]
fn test_tensor_cursor_walks_rank_1() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let td = arena.tensor(arena.u32(), 1);
    let mut tensor = unsafe { tensor_of(&rt, &arena, &[7], td) };
    assert_eq!(unsafe { walk_tensor_u32(&rt, &tensor, td) }, (0..7).collect::<Vec<_>>());
    unsafe { destroy_tensor(&rt, &mut tensor, td) };
    Ok(())
}

#[test]
fn test_row_cursor_walks_rank_2_and_3() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let tds = [arena.tensor(u32_td, 1), arena.tensor(u32_td, 2), arena.tensor(u32_td, 3)];

    let mut t2 = unsafe { tensor_of(&rt, &arena, &[3, 4], tds[1]) };
    assert_eq!(unsafe { walk_tensor_nested(&rt, &t2, 2, &tds) }, (0..12).collect::<Vec<_>>());
    unsafe { destroy_tensor(&rt, &mut t2, tds[1]) };

    let mut t3 = unsafe { tensor_of(&rt, &arena, &[2, 3, 4], tds[2]) };
    assert_eq!(unsafe { walk_tensor_nested(&rt, &t3, 3, &tds) }, (0..24).collect::<Vec<_>>());
    unsafe { destroy_tensor(&rt, &mut t3, tds[2]) };
    Ok(())
}

/// A transposed tensor walks in its own order, not its buffer's.
#[test]
fn test_row_cursor_walks_a_transposed_tensor() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let tds = [arena.tensor(u32_td, 1), arena.tensor(u32_td, 2)];

    // [[0, 1, 2], [3, 4, 5]] transposed is [[0, 3], [1, 4], [2, 5]].
    let mut tensor = unsafe { tensor_of(&rt, &arena, &[2, 3], tds[1]) };
    let mut transposed = empty_tensor();
    let perm = [1u32, 0];
    let status = unsafe {
        c::dtlv_rti_tensor_transpose_local(
            rt.0, &mut tensor as *mut rtdt::Tensor as *mut u8, &*(tds[1]), perm.as_ptr(),
            &mut transposed as *mut rtdt::Tensor as *mut u8, &*(tds[1]),
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(unsafe { walk_tensor_nested(&rt, &transposed, 2, &tds) }, vec![0, 3, 1, 4, 2, 5]);
    unsafe { destroy_tensor(&rt, &mut transposed, tds[1]) };
    Ok(())
}

/// A sliced tensor starts partway into its buffer and strides past what it
/// left out.
#[test]
fn test_row_cursor_walks_a_sliced_tensor() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let tds = [arena.tensor(u32_td, 1), arena.tensor(u32_td, 2)];

    // Rows 1..3 and columns 1..3 of a 4x4 of 0..16.
    let mut tensor = unsafe { tensor_of(&rt, &arena, &[4, 4], tds[1]) };
    let ranges = [rtdt::SliceRange { start: 1, end: 3 }, rtdt::SliceRange { start: 1, end: 3 }];

    // Slicing gives a `!` of the tensor.
    let payload_offset = rtdt::layout::result_payload_offset(align_of::<rtdt::Tensor>() as u32) as usize;
    let result_td = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Result,
        flags: 0,
        size: (payload_offset + size_of::<rtdt::Tensor>().max(size_of::<rtdt::Error>())).next_multiple_of(8) as u32,
        align: 8,
        type_info: rtdt::TyInfo { result: rtdt::TyInfoResult { ok_tydesc: tds[1] } },
    });
    let mut result = [0u64; 16];
    let status = unsafe {
        c::dtlv_rti_tensor_slice_local(
            rt.0, &mut tensor as *mut rtdt::Tensor as *mut u8, &*(tds[1]), ranges.as_ptr(),
            result.as_mut_ptr() as *mut u8, &*result_td,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(unsafe { *(result.as_ptr() as *const u8) }, rtdt::ResultTag::Ok as u8);
    let mut sliced = unsafe { ((result.as_ptr() as *const u8).add(payload_offset) as *const rtdt::Tensor).read() };
    assert_eq!(unsafe { walk_tensor_nested(&rt, &sliced, 2, &tds) }, vec![5, 6, 9, 10]);
    unsafe { destroy_tensor(&rt, &mut sliced, tds[1]) };
    Ok(())
}

// ==================== Sets and maps ====================

#[test]
fn test_tree_cursor_walks_an_empty_set_and_map() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let set_td = arena.set(u32_td);
    let map_td = arena.map(u32_td, u32_td);
    let mut set = unsafe { set_by_inserting(&rt, [], set_td, u32_td) };
    let mut map = unsafe { map_by_inserting(&rt, [], map_td, u32_td) };
    assert!(unsafe { walk_set_u32(&rt, &set, set_td) }.is_empty());
    assert!(unsafe { walk_map_u32(&rt, &map, map_td) }.is_empty());
    unsafe { destroy_set(&rt, &mut set, set_td) };
    unsafe { destroy_map(&rt, &mut map, map_td) };
    Ok(())
}

/// One leaf, a few levels, and many, in insertion order shuffled.
#[test]
fn test_tree_cursor_walks_in_order() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let set_td = arena.set(u32_td);
    let map_td = arena.map(u32_td, u32_td);
    for n in datalove_rt_tests::sizes([1u32, 2, 11, 12, 22, 23, 200, 5000]) {
        let mut set = unsafe { set_by_inserting(&rt, shuffled(n, n as u64), set_td, u32_td) };
        assert_eq!(unsafe { walk_set_u32(&rt, &set, set_td) }, (0..n).collect::<Vec<_>>());
        unsafe { destroy_set(&rt, &mut set, set_td) };

        let mut map = unsafe { map_by_inserting(&rt, shuffled(n, n as u64 + 1), map_td, u32_td) };
        assert_eq!(unsafe { walk_map_u32(&rt, &map, map_td) }, (0..n).map(|k| (k, k * 10)).collect::<Vec<_>>());
        unsafe { destroy_map(&rt, &mut map, map_td) };
    }
    Ok(())
}

/// After removals have merged and borrowed between leaves, the chain still
/// visits exactly what is left.
#[test]
fn test_tree_cursor_walks_after_removals() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let set_td = arena.set(u32_td);
    let map_td = arena.map(u32_td, u32_td);
    let mut set = unsafe { set_by_inserting(&rt, 0..datalove_rt_tests::size(2000), set_td, u32_td) };
    let mut map = unsafe { map_by_inserting(&rt, 0..datalove_rt_tests::size(2000), map_td, u32_td) };
    let mut left: std::collections::BTreeSet<u32> = (0..datalove_rt_tests::size(2000)).collect();
    for (i, x) in shuffled(datalove_rt_tests::size(2000), 9).into_iter().enumerate() {
        if x % 5 == 0 {
            continue;
        }
        let mut removed = 0u8;
        unsafe {
            c::dtlv_rti_btreeset_remove_local(rt.0, &mut set as *mut rtdt::Set as *mut u8, &*set_td, &x as *const u32 as *const u8, &*u32_td, &mut removed);
            c::dtlv_rti_btreemap_remove_local(rt.0, &mut map as *mut rtdt::Map as *mut u8, &*map_td, &x as *const u32 as *const u8, &*u32_td);
        }
        left.remove(&x);
        if i % 97 == 0 {
            let want: Vec<u32> = left.iter().copied().collect();
            assert_eq!(unsafe { walk_set_u32(&rt, &set, set_td) }, want);
            assert_eq!(unsafe { walk_map_u32(&rt, &map, map_td) }, want.iter().map(|&k| (k, k * 10)).collect::<Vec<_>>());
        }
    }
    let want: Vec<u32> = left.iter().copied().collect();
    assert_eq!(unsafe { walk_set_u32(&rt, &set, set_td) }, want);
    assert_eq!(unsafe { walk_map_u32(&rt, &map, map_td) }, want.iter().map(|&k| (k, k * 10)).collect::<Vec<_>>());
    unsafe { destroy_set(&rt, &mut set, set_td) };
    unsafe { destroy_map(&rt, &mut map, map_td) };
    Ok(())
}

/// Trees built in bulk, whose nodes may start below the minimum.
#[test]
fn test_tree_cursor_walks_bulk_built_trees() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let set_td = arena.set(u32_td);
    let map_td = arena.map(u32_td, u32_td);
    for n in datalove_rt_tests::sizes([1u32, 12, 133, 1000]) {
        let mut elems: Vec<u32> = (0..n).collect();
        let mut set = rtdt::Set { root: ptr::null(), len: rtdt::Index::ZERO };
        let status = unsafe {
            c::dtlv_rti_btreeset_build_from_sorted_slice_local(rt.0, &mut set as *mut rtdt::Set as *mut u8, &*u32_td, elems.as_mut_ptr() as *mut u8, n.into())
        };
        assert_eq!(status, RtStatus::Ok);
        assert_eq!(unsafe { walk_set_u32(&rt, &set, set_td) }, (0..n).collect::<Vec<_>>());
        unsafe { destroy_set(&rt, &mut set, set_td) };

        let mut keys: Vec<u32> = (0..n).collect();
        let mut values: Vec<u32> = (0..n).map(|k| k * 10).collect();
        let mut map = rtdt::Map { root: ptr::null(), len: rtdt::Index::ZERO };
        let status = unsafe {
            c::dtlv_rti_btreemap_build_from_sorted_slices_local(
                rt.0, &mut map as *mut rtdt::Map as *mut u8, &*u32_td, &*u32_td,
                keys.as_mut_ptr() as *mut u8, values.as_mut_ptr() as *mut u8, n.into(),
            )
        };
        assert_eq!(status, RtStatus::Ok);
        assert_eq!(unsafe { walk_map_u32(&rt, &map, map_td) }, (0..n).map(|k| (k, k * 10)).collect::<Vec<_>>());
        unsafe { destroy_map(&rt, &mut map, map_td) };
    }
    Ok(())
}

/// Zero-sized values step by nothing within a leaf, and still move to each
/// new leaf's values, so the value pointer stays on the leaf it belongs to.
#[test]
fn test_tree_cursor_walks_zero_sized_values() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let unit_td = arena.unit();
    let map_td = arena.map(u32_td, unit_td);
    for n in datalove_rt_tests::sizes([1u32, 11, 12, 23, 500]) {
        let mut map = unsafe { map_of(&rt, shuffled(n, 3).into_iter().map(|k| (k, ())), map_td, u32_td, unit_td) };
        let mut cursor = unsafe { begin_map(&rt, &map, map_td) };
        assert_eq!(cursor.value_stride, 0);
        let keys: Vec<u32> = unsafe { walk_tree(&rt, &mut cursor) }.into_iter().map(|(k, _)| unsafe { *(k as *const u32) }).collect();
        assert_eq!(keys, (0..n).collect::<Vec<_>>());
        unsafe { destroy_map(&rt, &mut map, map_td) };
    }
    Ok(())
}

/// Values more aligned than the keys sit after padding in the leaf, which the
/// cursor's `values_offset` accounts for.
#[test]
fn test_tree_cursor_walks_over_aligned_values() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let u64_td = arena.u64();
    let map_td = arena.map(u32_td, u64_td);
    for n in datalove_rt_tests::sizes([1u32, 12, 23, 500]) {
        let mut map = unsafe { map_of(&rt, shuffled(n, 4).into_iter().map(|k| (k, k as u64 * 1_000_000_007)), map_td, u32_td, u64_td) };
        let mut cursor = unsafe { begin_map(&rt, &map, map_td) };
        assert_eq!(cursor.value_stride, 8);
        let entries: Vec<(u32, u64)> = unsafe { walk_tree(&rt, &mut cursor) }
            .into_iter()
            .map(|(k, v)| unsafe { (*(k as *const u32), *(v as *const u64)) })
            .collect();
        assert_eq!(entries, (0..n).map(|k| (k, k as u64 * 1_000_000_007)).collect::<Vec<_>>());
        unsafe { destroy_map(&rt, &mut map, map_td) };
    }
    Ok(())
}

/// A cursor is plain data: a copy taken mid-walk carries on from where the
/// original was, with no descriptor, as compiled code does with a walk the
/// interpreter began.
#[test]
fn test_tree_cursor_copied_mid_walk_carries_on() -> AnyResult<()> {
    let rt = Rt::new();
    let arena = TyDescArena::new();
    let u32_td = arena.u32();
    let set_td = arena.set(u32_td);
    let mut set = unsafe { set_by_inserting(&rt, shuffled(100, 5), set_td, u32_td) };
    for split in [0usize, 10, 11, 12, 50, 99] {
        let mut cursor = unsafe { begin_set(&rt, &set, set_td) };
        for _ in 0..split {
            unsafe { step(&rt, c::dtlv_rti_tree_cursor_advance_local, &mut cursor) };
        }
        let mut copy = cursor;
        let rest: Vec<u32> = unsafe { walk_tree(&rt, &mut copy) }.into_iter().map(|(k, _)| unsafe { *(k as *const u32) }).collect();
        assert_eq!(rest, (split as u32..100).collect::<Vec<_>>());
    }
    unsafe { destroy_set(&rt, &mut set, set_td) };
    Ok(())
}
