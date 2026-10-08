//! Runtime implementation of BTreeMap (ordered map).
//!
//! Uses a B+tree structure with fixed-capacity nodes aligned to allocator size classes.

use crate::impls::rt_local::RtLocal;
use datalove_rtdt as rtdt;
use datalove_rtdt::*;
use crate::c::RtStatus;
use crate::rust::AlignedBuffer;

// B-tree constants.
const MAP_NODE_B: u32 = rtdt::MAP_NODE_B;

// Offset constants for MapNode header fields.
const TAG_OFFSET: u32 = 0;
const LEN_OFFSET: u32 = 4;  // Aligned to u32.

/// A map's key and value types and node layouts.
///
/// Read out of the map's descriptor once per operation and handed down, where
/// the layouts used to be worked out again from the key and value types at
/// every node visited.
#[derive(Copy, Clone)]
pub(crate) struct MapTy<'a> {
    pub key: rtdt::TyDescRef<'a>,
    pub value: rtdt::TyDescRef<'a>,
    pub leaf: &'a MapNodeLeafLayout,
    pub internal: &'a MapNodeInternalLayout,
    pub ord: super::cmp::KeyOrd<'a>,
}

impl<'a> MapTy<'a> {
    pub fn of(map_tydesc: rtdt::TyDescRef<'a>) -> MapTy<'a> {
        MapTy {
            key: map_tydesc.map_key_ty(),
            value: map_tydesc.map_value_ty(),
            leaf: map_tydesc.map_leaf_layout(),
            internal: map_tydesc.map_internal_layout(),
            ord: super::cmp::KeyOrd::of(map_tydesc.map_key_ty()),
        }
    }
}

/// The nodes from a tree's root down to a leaf, for an insert to split back up.
///
/// On the stack: it was a `Vec`, allocated by every insert. A non-root node has
/// at least `MAP_NODE_B` children, and a set's the same, so a tree of as many
/// entries as an index can count is well under this deep.
pub(crate) struct NodePath<N> {
    nodes: [*mut N; 32],
    len: usize,
}

impl<N> NodePath<N> {
    pub(crate) fn new() -> Self {
        NodePath { nodes: [std::ptr::null_mut(); 32], len: 0 }
    }

    pub(crate) fn push(&mut self, node: *mut N) {
        assert!(self.len < self.nodes.len(), "a tree deeper than any index can fill");
        self.nodes[self.len] = node;
        self.len += 1;
    }
}

impl<N> std::ops::Deref for NodePath<N> {
    type Target = [*mut N];
    fn deref(&self) -> &[*mut N] {
        &self.nodes[..self.len]
    }
}

/// Somewhere to unpack a key, value or element out of a `data` on its way into
/// a tree, which then moves it out again.
///
/// On the stack when it fits, as nearly every one does; it was taken from the
/// allocator and given back on every insert.
pub(crate) struct UnpackSlot {
    inline: InlineSlot,
    heap: *mut u8,
    size: u32,
    align: u32,
}

#[repr(C, align(16))]
struct InlineSlot([u8; 64]);

impl UnpackSlot {
    /// A slot for a value of `ty`, or none if the allocator had nothing.
    pub(crate) unsafe fn new(rt: &mut RtLocal, ty: rtdt::TyDescRef) -> std::option::Option<UnpackSlot> {
        let (size, align) = (ty.size(), ty.align());
        let heap = if size as usize <= size_of::<InlineSlot>() && align as usize <= align_of::<InlineSlot>() {
            std::ptr::null_mut()
        } else {
            let ptr = unsafe { rt.alloc.alloc(size, align, 1) };
            if ptr.is_null() {
                return None;
            }
            ptr
        };
        Some(UnpackSlot { inline: InlineSlot([0; 64]), heap, size, align })
    }

    /// Where the value goes. The slot must not move while this is in use.
    pub(crate) fn ptr(&mut self) -> *mut u8 {
        if self.heap.is_null() { self.inline.0.as_mut_ptr() } else { self.heap }
    }

    /// Give the storage back, whatever it held having been moved out.
    pub(crate) unsafe fn release(self, rt: &mut RtLocal) {
        if !self.heap.is_null() {
            unsafe { rt.alloc.free(self.size, self.align, 1, self.heap) };
        }
    }
}

/// Where `key` falls among the first `len` keys at `keys`: `Ok` with the
/// position of the one equal to it, or `Err` with the position it would go at.
///
/// In order, not by bisection. Bisecting compared fewer keys -- a node holds up
/// to eleven -- but its branches go either way at random where a scan's go one
/// way until they stop, and on the store demo and `benchvs/wordfreq` the
/// mispredictions cost more cycles than the comparisons saved.
#[inline]
unsafe fn search_keys(keys: *const u8, len: usize, key: *const u8, ty: MapTy) -> std::result::Result<usize, usize> {
    unsafe {
        let key_size = ty.key.size() as usize;
        for i in 0..len {
            match ty.ord.cmp(key, keys.add(i * key_size)) {
                crate::c::RtOrdering::Less => return Err(i),
                crate::c::RtOrdering::Greater => {}
                crate::c::RtOrdering::Equal => return Ok(i),
                crate::c::RtOrdering::Error => unreachable!("two values of one type always order"),
            }
        }
        Err(len)
    }
}

/// The child of an internal node to descend into for `key`.
///
/// A separator is the least key in the subtree to its right, so a key equal to
/// one goes right.
#[inline]
unsafe fn child_index(keys: *const u8, len: usize, key: *const u8, ty: MapTy) -> usize {
    unsafe {
        match search_keys(keys, len, key, ty) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

/// Allocate and initialize a new internal node.
unsafe fn alloc_internal_node(
    rt: &mut RtLocal,
    ty: MapTy,
) -> *mut MapNode {
    unsafe {
        let layout = *ty.internal;

        // Allocate the node.
        let ptr = rt.alloc.alloc(layout.size, layout.align, 1);
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let node = ptr as *mut MapNode;

        // Initialize header.
        write_node_tag(node, MapNodeTag::Internal);
        write_node_len(node, 0);

        node
    }
}

/// Allocate and initialize a new leaf node.
unsafe fn alloc_leaf_node(
    rt: &mut RtLocal,
    ty: MapTy,
) -> *mut MapNode {
    unsafe {
        let layout = *ty.leaf;

        // Allocate the node.
        let ptr = rt.alloc.alloc(layout.size, layout.align, 1);
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let node = ptr as *mut MapNode;

        // Initialize header.
        write_node_tag(node, MapNodeTag::Leaf);
        write_node_len(node, 0);

        // Initialize next_leaf pointer to null.
        let next_ptr = leaf_next_ptr_mut(node, ty);
        *next_ptr = std::ptr::null_mut();

        node
    }
}

/// Free a node and its contents.
unsafe fn free_node(
    rt: &mut RtLocal,
    node: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        if node.is_null() {
            return;
        }

        let tag = read_node_tag(node);

        match tag {
            MapNodeTag::Internal => {
                let layout = *ty.internal;
                rt.alloc.free(layout.size, layout.align, 1, node as *mut u8);
            }
            MapNodeTag::Leaf => {
                let layout = *ty.leaf;
                rt.alloc.free(layout.size, layout.align, 1, node as *mut u8);
            }
        };
    }
}

// Node header accessors.

#[inline]
unsafe fn read_node_tag(node: *const MapNode) -> MapNodeTag {
    unsafe {
        let tag_ptr = (node as *const u8).add(TAG_OFFSET as usize);
        let tag_val = *tag_ptr;
        match tag_val {
            1 => MapNodeTag::Internal,
            2 => MapNodeTag::Leaf,
            _ => panic!("Invalid MapNodeTag: {}", tag_val),
        }
    }
}

#[inline]
unsafe fn write_node_tag(node: *mut MapNode, tag: MapNodeTag) {
    unsafe {
        let tag_ptr = (node as *mut u8).add(TAG_OFFSET as usize);
        *tag_ptr = tag as u8;
    }
}

#[inline]
unsafe fn read_node_len(node: *const MapNode) -> u32 {
    unsafe {
        let len_ptr = (node as *const u8).add(LEN_OFFSET as usize) as *const u32;
        *len_ptr
    }
}

#[inline]
unsafe fn write_node_len(node: *mut MapNode, len: u32) {
    unsafe {
        let len_ptr = (node as *mut u8).add(LEN_OFFSET as usize) as *mut u32;
        *len_ptr = len;
    }
}

// Internal node accessors.

/// Get pointer to keys array in an internal node.
#[inline]
unsafe fn internal_keys_ptr(
    node: *mut MapNode,
    ty: MapTy,
) -> *mut u8 {
    unsafe {
        let layout = *ty.internal;
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Get pointer to child pointers array in an internal node.
#[inline]
unsafe fn internal_child_ptrs_ptr(
    node: *mut MapNode,
    ty: MapTy,
) -> *mut *mut MapNode {
    unsafe {
        let layout = *ty.internal;
        (node as *mut u8).add(layout.child_ptrs_offset as usize) as *mut *mut MapNode
    }
}

/// The first leaf of a tree, reached down the left spine.
///
/// See `set::leftmost_leaf`: every walk of a map's entries in order starts
/// here, and a root is only a leaf while the map fits in one node.
pub(crate) unsafe fn leftmost_leaf(root: *mut MapNode, ty: MapTy) -> *mut MapNode {
    unsafe {
        let mut node = root;
        while matches!(read_node_tag(node), MapNodeTag::Internal) {
            node = *internal_child_ptrs_ptr(node, ty);
        }
        node
    }
}

// Leaf node accessors.

/// Get pointer to next_leaf field in a leaf node.
#[inline]
unsafe fn leaf_next_ptr_mut(
    node: *mut MapNode,
    ty: MapTy,
) -> *mut *mut MapNode {
    unsafe {
        let layout = *ty.leaf;
        (node as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut MapNode
    }
}

/// Get pointer to keys array in a leaf node.
#[inline]
unsafe fn leaf_keys_ptr(
    node: *mut MapNode,
    ty: MapTy,
) -> *mut u8 {
    unsafe {
        let layout = *ty.leaf;
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Get pointer to values array in a leaf node.
#[inline]
unsafe fn leaf_values_ptr(
    node: *mut MapNode,
    ty: MapTy,
) -> *mut u8 {
    unsafe {
        let layout = *ty.leaf;
        (node as *mut u8).add(layout.values_offset as usize)
    }
}

/// Create an empty BTreeMap.
pub unsafe fn btreemap_create_impl(
    _rt: &mut RtLocal,
    value_out: *mut u8,
    _tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if value_out.is_null() {
            return RtStatus::Error;
        }

        // Create an empty map (null root, zero length).
        let map_ptr = value_out as *mut Map;
        (*map_ptr).root = std::ptr::null_mut();
        (*map_ptr).len = rtdt::Index::ZERO;

        RtStatus::Ok
    }
}

/// Destroy a BTreeMap and free all nodes.
pub unsafe fn btreemap_destroy_impl(
    rt: &mut RtLocal,
    value_in: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(tydesc);
        if value_in.is_null() {
            return RtStatus::Error;
        }


        let map_ptr = value_in as *mut Map;
        let root = (*map_ptr).root as *mut MapNode;

        if !root.is_null() {
            destroy_tree_recursive(rt, root, ty);
        }

        // Clear the map struct.
        (*map_ptr).root = std::ptr::null_mut();
        (*map_ptr).len = rtdt::Index::ZERO;

        RtStatus::Ok
    }
}

/// Recursively destroy a subtree.
unsafe fn destroy_tree_recursive(
    rt: &mut RtLocal,
    node: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        if node.is_null() {
            return;
        }

        let tag = read_node_tag(node);

        match tag {
            MapNodeTag::Internal => {
                let len = read_node_len(node);

                // Destroy all keys in the internal node.
                let keys_ptr = internal_keys_ptr(node, ty);
                let key_size = ty.key.size() as usize;
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());
                }

                // Recursively destroy children.
                let children_ptr = internal_child_ptrs_ptr(node, ty);
                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    destroy_tree_recursive(rt, child, ty);
                }
            }
            MapNodeTag::Leaf => {
                let len = read_node_len(node);
                let keys_ptr = leaf_keys_ptr(node, ty);
                let values_ptr = leaf_values_ptr(node, ty);
                let key_size = ty.key.size() as usize;
                let value_size = ty.value.size() as usize;
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

                // Destroy all keys and values in the leaf node.
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let value_slot = values_ptr.add(i * value_size);
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, value_slot, ty.value.as_ptr());
                }
            }
        }

        // Free the node itself.
        free_node(rt, node, ty);
    }
}

/// Clear a BTreeMap (destroy and recreate empty).
/// The number of entries in a map.
///
/// A generic function has no static type for the keys or values, but the count
/// does not need one.
pub unsafe fn btreemap_len_impl(
    map_value_ref: *const u8,
    len_out: *mut u8,
) -> RtStatus {
    unsafe {
        let map = &*(map_value_ref as *const rtdt::Map);
        std::ptr::write(len_out as *mut rtdt::Index, map.len);
    }
    RtStatus::Ok
}

pub unsafe fn btreemap_clear_impl(
    rt: &mut RtLocal,
    value_mut: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        // Destroy the existing tree.
        let status = btreemap_destroy_impl(rt, value_mut, tydesc);
        if status != RtStatus::Ok {
            return status;
        }

        // Reinitialize as empty.
        btreemap_create_impl(rt, value_mut, tydesc)
    }
}

// Insert helper types and functions.

/// Result of attempting to insert into a leaf.
#[derive(Debug, PartialEq, Eq)]
enum LeafInsertResult {
    /// Key was newly inserted.
    Inserted,
    /// Key already existed, value was updated.
    Updated,
    /// Leaf is full, needs to be split.
    NeedsSplit,
}

/// Information about a node split.
struct SplitInfo {
    /// The separator key to insert into parent.
    separator_key_buf: AlignedBuffer,
    /// The new right sibling node created by the split.
    new_node: *mut MapNode,
    /// The result of inserting the pending key during the split.
    insert_result: LeafInsertResult,
}

impl SplitInfo {
    /// Destroy the separator key and clean up.
    unsafe fn destroy(mut self, rt: &mut RtLocal, key_tydesc: rtdt::TyDescRef) {
        unsafe {
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let _ = crate::impls::destroy::any_destroy_local(
                rt_handle,
                self.separator_key_buf.as_mut_ptr(),
                key_tydesc.as_ptr(),
            );
        }
    }
}

/// Find the leaf node where a key should be inserted.
unsafe fn find_leaf_for_key(
    mut node: *mut MapNode,
    key: *const u8,
    ty: MapTy,
) -> *mut MapNode {
    unsafe {
        loop {
            let tag = read_node_tag(node);
            match tag {
                MapNodeTag::Leaf => return node,
                MapNodeTag::Internal => {
                    let len = read_node_len(node);
                    let keys_ptr = internal_keys_ptr(node, ty);
                    let children_ptr = internal_child_ptrs_ptr(node, ty);

                    // Find the child to descend into.
                    // In a B+tree, separators represent the minimum key in the right subtree,
                    // so Equal should go right.
                    let child_idx = child_index(keys_ptr, len as usize, key, ty);

                    node = *children_ptr.add(child_idx);
                }
            }
        }
    }
}

/// Try to insert or update a key-value pair in a leaf node.
unsafe fn leaf_insert_or_update(
    rt: &mut RtLocal,
    leaf: *mut MapNode,
    key: *const u8,
    value: *const u8,
    ty: MapTy,
) -> LeafInsertResult {
    unsafe {
        let len = read_node_len(leaf);
        let capacity = rtdt::MAP_NODE_CAPACITY;

        if len >= capacity {
            return LeafInsertResult::NeedsSplit;
        }

        let keys_ptr = leaf_keys_ptr(leaf, ty);
        let values_ptr = leaf_values_ptr(leaf, ty);
        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;

        let insert_pos = match search_keys(keys_ptr, len as usize, key, ty) {
            Ok(i) => {
                // Key already exists, update the value.
                // Destroy the old value before overwriting.
                let value_slot = values_ptr.add(i * value_size);
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                let _ = crate::impls::destroy::any_destroy_local(rt_handle, value_slot, ty.value.as_ptr());
                // Move the new value (by-move semantics).
                std::ptr::copy_nonoverlapping(value, value_slot, value_size);
                // Destroy the input key since we're not using it (key already exists in tree).
                let _ = crate::impls::destroy::any_destroy_local(rt_handle, key as *mut u8, ty.key.as_ptr());
                return LeafInsertResult::Updated;
            }
            Err(pos) => pos,
        };

        // Shift elements to make room.
        if insert_pos < len as usize {
            let shift_count = len as usize - insert_pos;
            let src_key = keys_ptr.add(insert_pos * key_size);
            let dst_key = keys_ptr.add((insert_pos + 1) * key_size);
            std::ptr::copy(src_key, dst_key, shift_count * key_size);

            let src_val = values_ptr.add(insert_pos * value_size);
            let dst_val = values_ptr.add((insert_pos + 1) * value_size);
            std::ptr::copy(src_val, dst_val, shift_count * value_size);
        }

        // Move the new key-value pair into the node (by-move semantics).
        let key_slot = keys_ptr.add(insert_pos * key_size);
        let value_slot = values_ptr.add(insert_pos * value_size);
        std::ptr::copy_nonoverlapping(key, key_slot, key_size);
        std::ptr::copy_nonoverlapping(value, value_slot, value_size);

        write_node_len(leaf, len + 1);
        LeafInsertResult::Inserted
    }
}

/// Split a full leaf node and insert the pending key-value pair.
///
/// Returns split info for parent update.
unsafe fn split_leaf(
    rt: &mut RtLocal,
    leaf: *mut MapNode,
    key: *const u8,
    value: *const u8,
    ty: MapTy,
) -> core::result::Result<SplitInfo, RtStatus> {
    unsafe {
        // Allocate a new sibling leaf.
        let new_leaf = alloc_leaf_node(rt, ty);
        if new_leaf.is_null() {
            return Err(RtStatus::Error);
        }

        let capacity = rtdt::MAP_NODE_CAPACITY;
        let split_point = (capacity / 2) as usize;

        let keys_ptr = leaf_keys_ptr(leaf, ty);
        let values_ptr = leaf_values_ptr(leaf, ty);
        let new_keys_ptr = leaf_keys_ptr(new_leaf, ty);
        let new_values_ptr = leaf_values_ptr(new_leaf, ty);

        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;

        // Move upper half to new leaf.
        let move_count = capacity as usize - split_point;
        std::ptr::copy_nonoverlapping(
            keys_ptr.add(split_point * key_size),
            new_keys_ptr,
            move_count * key_size,
        );
        std::ptr::copy_nonoverlapping(
            values_ptr.add(split_point * value_size),
            new_values_ptr,
            move_count * value_size,
        );

        write_node_len(leaf, split_point as u32);
        write_node_len(new_leaf, move_count as u32);

        // Update next_leaf pointers.
        let old_next = *leaf_next_ptr_mut(leaf, ty);
        *leaf_next_ptr_mut(leaf, ty) = new_leaf;
        *leaf_next_ptr_mut(new_leaf, ty) = old_next;

        // Clone the separator key (first key of new_leaf) into a buffer.
        // In a B+tree, the separator stays in the leaf, but internal nodes need their own copy.
        let mut separator_key_buf = AlignedBuffer::with_align(key_size, ty.key.align() as usize);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            new_keys_ptr,
            ty.key.as_ptr(),
            separator_key_buf.as_mut_ptr(),
        );
        if status != RtStatus::Ok {
            return Err(status);
        }

        // Determine which leaf should receive the new key.
        let cmp_result = ty.ord.cmp(key, separator_key_buf.as_ptr());
        let insert_result = match cmp_result {
            crate::c::RtOrdering::Less => {
                // Key goes in left leaf.
                leaf_insert_or_update(rt, leaf, key, value, ty)
            }
            crate::c::RtOrdering::Equal | crate::c::RtOrdering::Greater => {
                // Key goes in right leaf (new_leaf).
                // Equal goes right because separator represents min key of right subtree.
                leaf_insert_or_update(rt, new_leaf, key, value, ty)
            }
            crate::c::RtOrdering::Error => {
                // Should not happen.
                return Err(RtStatus::Error);
            }
        };

        if insert_result == LeafInsertResult::NeedsSplit {
            // Still needs split - this shouldn't happen with proper split point.
            return Err(RtStatus::Error);
        }

        Ok(SplitInfo {
            separator_key_buf,
            new_node: new_leaf,
            insert_result,
        })
    }
}

/// Insert a separator key and child pointer into an internal node.
///
/// Returns Ok if successful, Err(SplitInfo) if the node was full and had to split.
unsafe fn insert_into_internal(
    rt: &mut RtLocal,
    node: *mut MapNode,
    separator_key: &[u8],
    right_child: *mut MapNode,
    ty: MapTy,
) -> core::result::Result<(), SplitInfo> {
    unsafe {
        let len = read_node_len(node);
        let capacity = rtdt::MAP_NODE_CAPACITY;

        if len >= capacity {
            // Node is full, need to split.
            return Err(split_internal_node(rt, node, separator_key, right_child, ty));
        }

        let keys_ptr = internal_keys_ptr(node, ty);
        let children_ptr = internal_child_ptrs_ptr(node, ty);
        let key_size = ty.key.size() as usize;

        // Find insertion position.
        let mut insert_pos = len as usize;
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * key_size);
            let cmp_result = ty.ord.cmp(separator_key.as_ptr(), node_key);
            match cmp_result {
                crate::c::RtOrdering::Less => {
                    insert_pos = i;
                    break;
                }
                crate::c::RtOrdering::Equal => {
                    // Duplicate separator key shouldn't happen.
                    insert_pos = i;
                    break;
                }
                crate::c::RtOrdering::Greater => continue,
                crate::c::RtOrdering::Error => {
                    insert_pos = i;
                    break;
                }
            }
        }

        // Shift keys and child pointers to make room.
        if insert_pos < len as usize {
            let shift_count = len as usize - insert_pos;
            let src_key = keys_ptr.add(insert_pos * key_size);
            let dst_key = keys_ptr.add((insert_pos + 1) * key_size);
            std::ptr::copy(src_key, dst_key, shift_count * key_size);

            let src_child = children_ptr.add(insert_pos + 1);
            let dst_child = children_ptr.add(insert_pos + 2);
            std::ptr::copy(src_child, dst_child, shift_count);
        }

        // Clone the separator key into the node.
        let key_slot = keys_ptr.add(insert_pos * key_size);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            separator_key.as_ptr(),
            ty.key.as_ptr(),
            key_slot,
        );
        if status != RtStatus::Ok {
            panic!("Failed to clone separator key during insert_into_internal");
        }

        // Insert the right child pointer.
        *children_ptr.add(insert_pos + 1) = right_child;

        write_node_len(node, len + 1);
        Ok(())
    }
}

/// Split a full internal node.
unsafe fn split_internal_node(
    rt: &mut RtLocal,
    node: *mut MapNode,
    pending_key: &[u8],
    pending_child: *mut MapNode,
    ty: MapTy,
) -> SplitInfo {
    unsafe {
        // Allocate a new sibling internal node.
        let new_node = alloc_internal_node(rt, ty);
        assert!(!new_node.is_null(), "Failed to allocate internal node");

        let capacity = rtdt::MAP_NODE_CAPACITY;
        let split_point = (capacity / 2) as usize;

        let keys_ptr = internal_keys_ptr(node, ty);
        let children_ptr = internal_child_ptrs_ptr(node, ty);
        let new_keys_ptr = internal_keys_ptr(new_node, ty);
        let new_children_ptr = internal_child_ptrs_ptr(new_node, ty);

        let key_size = ty.key.size() as usize;

        // Clone the middle key as the separator to push up.
        let mut separator_key_buf = AlignedBuffer::with_align(key_size, ty.key.align() as usize);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            keys_ptr.add(split_point * key_size),
            ty.key.as_ptr(),
            separator_key_buf.as_mut_ptr(),
        );
        if status != RtStatus::Ok {
            panic!("Failed to clone separator key during internal node split");
        }

        // Destroy the separator key in the old node since we've cloned it out.
        let _ = crate::impls::destroy::any_destroy_local(
            rt_handle,
            keys_ptr.add(split_point * key_size),
            ty.key.as_ptr(),
        );

        // Move keys after split_point (excluding the separator) to new node.
        let keys_to_move = capacity as usize - split_point - 1;
        if keys_to_move > 0 {
            std::ptr::copy_nonoverlapping(
                keys_ptr.add((split_point + 1) * key_size),
                new_keys_ptr,
                keys_to_move * key_size,
            );
        }

        // Move child pointers (split_point+1 onwards) to new node.
        let children_to_move = capacity as usize - split_point;
        std::ptr::copy_nonoverlapping(
            children_ptr.add(split_point + 1),
            new_children_ptr,
            children_to_move,
        );

        write_node_len(node, split_point as u32);
        write_node_len(new_node, keys_to_move as u32);

        // Now insert the pending key/child into the appropriate node.
        let cmp_result = ty.ord.cmp(pending_key.as_ptr(), separator_key_buf.as_ptr());

        let insert_result = match cmp_result {
            crate::c::RtOrdering::Less => {
                insert_into_internal(rt, node, pending_key, pending_child, ty)
            }
            _ => {
                insert_into_internal(rt, new_node, pending_key, pending_child, ty)
            }
        };

        // After split, there should be room - if not, something is very wrong.
        assert!(insert_result.is_ok(), "Split didn't make room for insertion");

        SplitInfo {
            separator_key_buf,
            new_node,
            // Internal node splits always represent new separator insertions.
            insert_result: LeafInsertResult::Inserted,
        }
    }
}

/// Insert a key-value pair into the BTreeMap.
pub unsafe fn btreemap_insert_impl(
    rt: &mut RtLocal,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_in: *mut u8,
    _key_tydesc: rtdt::TyDescRef,
    value_in: *mut u8,
    _value_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_mut.is_null()
            || key_in.is_null()
            || value_in.is_null() {
            return RtStatus::Error;
        }

        // Get key and value type descriptors from map type.
        let map_key_ty = btreemap_tydesc.map_key_ty();
        let map_value_ty = btreemap_tydesc.map_value_ty();

        let key_ptr = key_in;
        let val_ptr = value_in;

        let map_ptr = btreemap_value_mut as *mut Map;
        let root = (*map_ptr).root as *mut MapNode;

        // If map is empty, create the first leaf.
        if root.is_null() {
            let leaf = alloc_leaf_node(rt, ty);
            if leaf.is_null() {
                return RtStatus::Error;
            }

            // Insert the key-value pair into the empty leaf.
            let keys_ptr = leaf_keys_ptr(leaf, ty);
            let values_ptr = leaf_values_ptr(leaf, ty);

            // Move key and value from the input pointers (by-move semantics).
            let key_size = map_key_ty.size() as usize;
            let value_size = map_value_ty.size() as usize;
            std::ptr::copy_nonoverlapping(key_ptr, keys_ptr, key_size);
            std::ptr::copy_nonoverlapping(val_ptr, values_ptr, value_size);

            write_node_len(leaf, 1);
            (*map_ptr).root = leaf as *const MapNode;
            (*map_ptr).len = rtdt::Index::ONE;

            return RtStatus::Ok;
        }

        // Find the leaf where the key should be inserted, keeping track of the path.
        let mut path = NodePath::new();
        let leaf = find_leaf_with_path(root, key_ptr, ty, &mut path);

        // Try to insert into the leaf.
        let result = leaf_insert_or_update(
            rt,
            leaf,
            key_ptr,
            val_ptr,
            ty,
        );

        match result {
            LeafInsertResult::Updated => {
                // Key already existed, value was updated, len unchanged.
                RtStatus::Ok
            }
            LeafInsertResult::Inserted => {
                // Key was inserted, increment len.
                (*map_ptr).len += rtdt::Index::ONE;
                RtStatus::Ok
            }
            LeafInsertResult::NeedsSplit => {
                // Leaf is full, need to split and propagate up.
                let split_info = match split_leaf(rt, leaf, key_ptr, val_ptr, ty) {
                    Ok(info) => info,
                    Err(status) => return status,
                };

                let was_inserted = split_info.insert_result == LeafInsertResult::Inserted;

                // Propagate split up the tree.
                let status = propagate_split_up(
                    rt,
                    &mut (*map_ptr).root,
                    leaf,
                    split_info,
                    &path,
                    ty,
                );

                if status == RtStatus::Ok && was_inserted {
                    (*map_ptr).len += rtdt::Index::ONE;
                }
                status
            }
        }
    }
}

/// Find the leaf node where a key should be inserted, tracking the path.
unsafe fn find_leaf_with_path(
    mut node: *mut MapNode,
    key: *const u8,
    ty: MapTy,
    path: &mut NodePath<MapNode>,
) -> *mut MapNode {
    unsafe {
        loop {
            let tag = read_node_tag(node);
            match tag {
                MapNodeTag::Leaf => return node,
                MapNodeTag::Internal => {
                    path.push(node);

                    let len = read_node_len(node);
                    let keys_ptr = internal_keys_ptr(node, ty);
                    let children_ptr = internal_child_ptrs_ptr(node, ty);

                    // Find the child to descend into.
                    // In a B+tree, separators represent the minimum key in the right subtree,
                    // so Equal should go right.
                    let child_idx = child_index(keys_ptr, len as usize, key, ty);

                    node = *children_ptr.add(child_idx);
                }
            }
        }
    }
}

/// Propagate a split up the tree.
unsafe fn propagate_split_up(
    rt: &mut RtLocal,
    root_ptr: &mut *const MapNode,
    mut child: *mut MapNode,
    mut split_info: SplitInfo,
    path: &[*mut MapNode],
    ty: MapTy,
) -> RtStatus {
    unsafe {
        // If there's no parent, child must be the root.
        if path.is_empty() {
            // Create new internal root.
            let new_root = alloc_internal_node(rt, ty);
            if new_root.is_null() {
                return RtStatus::Error;
            }

            let root_keys_ptr = internal_keys_ptr(new_root, ty);
            let root_children_ptr = internal_child_ptrs_ptr(new_root, ty);

            // Clone separator key into new root.
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let status = crate::impls::clone::clone_value(
                rt_handle,
                split_info.separator_key_buf.as_ptr(),
                ty.key.as_ptr(),
                root_keys_ptr,
            );
            if status != RtStatus::Ok {
                return RtStatus::Error;
            }

            // Set children.
            *root_children_ptr.add(0) = child;
            *root_children_ptr.add(1) = split_info.new_node;

            write_node_len(new_root, 1);
            *root_ptr = new_root;

            split_info.destroy(rt, ty.key);
            return RtStatus::Ok;
        }

        // Walk up the path, inserting separators into parents.
        for &parent in path.iter().rev() {
            match insert_into_internal(
                rt,
                parent,
                split_info.separator_key_buf.as_slice(),
                split_info.new_node,
                ty,
            ) {
                Ok(()) => {
                    // Successfully inserted into parent, done!
                    split_info.destroy(rt, ty.key);
                    return RtStatus::Ok;
                }
                Err(new_split_info) => {
                    // Parent split, continue propagating up.
                    // Destroy the old split_info before replacing it.
                    split_info.destroy(rt, ty.key);
                    child = parent;
                    split_info = new_split_info;
                }
            }
        }

        // If we get here, the root split.
        let new_root = alloc_internal_node(rt, ty);
        if new_root.is_null() {
            return RtStatus::Error;
        }

        let root_keys_ptr = internal_keys_ptr(new_root, ty);
        let root_children_ptr = internal_child_ptrs_ptr(new_root, ty);

        // Clone separator key into new root.
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            split_info.separator_key_buf.as_ptr(),
            ty.key.as_ptr(),
            root_keys_ptr,
        );
        if status != RtStatus::Ok {
            return RtStatus::Error;
        }

        *root_children_ptr.add(0) = child;
        *root_children_ptr.add(1) = split_info.new_node;

        write_node_len(new_root, 1);
        *root_ptr = new_root;

        split_info.destroy(rt, ty.key);
        RtStatus::Ok
    }
}

/// Get a value from the BTreeMap by key.
///
/// Returns the value as an Option<V>:
/// - If the key is found, sets the option to Some and clones the value.
/// - If the key is not found, sets the option to None.
/// Insert an entry whose key and value arrive packed into `data`.
///
/// Each is moved out into a value of the map's own key or value type, which
/// the map's descriptor names, and the typed insert takes it from there. Both
/// datas are consumed.
pub unsafe fn btreemap_insert_data_impl(
    rt: &mut RtLocal,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_data_in: *const u8,
    value_data_in: *const u8,
) -> RtStatus {
    unsafe {
        btreemap_insert_sides_impl(
            rt, btreemap_value_mut, btreemap_tydesc,
            key_data_in as *mut u8, true, value_data_in as *mut u8, true,
        )
    }
}

/// Insert an entry where either side may have arrived packed into a `data`.
///
/// A side that arrived packed is moved out into a value of the map's own key
/// or value type on the way in, consuming the `data`; a side that did not is
/// moved in as it stands. The two are decided apart, because a map built
/// inside a generic can be erased on one side and not the other: `%{k = v}`
/// written in a generic over `K` and `V`, called at `K := data`, hands over a
/// key that is already what the map holds and a value that is not.
pub unsafe fn btreemap_insert_sides_impl(
    rt: &mut RtLocal,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: rtdt::TyDescRef,
    // Value is moved.
    key_in: *mut u8,
    key_is_data: bool,
    // Value is moved.
    value_in: *mut u8,
    value_is_data: bool,
) -> RtStatus {
    unsafe {
        let key_ty = btreemap_tydesc.map_key_ty();
        let value_ty = btreemap_tydesc.map_value_ty();
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        // A side that arrived packed needs somewhere of the real type to be
        // unpacked into. One that did not is already there.
        let mut key_unpacked = None;
        let mut value_unpacked = None;
        let status = 'unpack: {
            let key_slot = if key_is_data {
                let Some(slot) = UnpackSlot::new(rt, key_ty) else { break 'unpack RtStatus::Error };
                let slot = key_unpacked.insert(slot).ptr();
                let status = crate::impls::boxing::data_into_local(
                    rt_handle, key_in, slot, key_ty.as_ptr(),
                );
                if status != RtStatus::Ok {
                    break 'unpack RtStatus::Error;
                }
                slot
            } else {
                key_in
            };

            let value_slot = if value_is_data {
                let Some(slot) = UnpackSlot::new(rt, value_ty) else { break 'unpack RtStatus::Error };
                let slot = value_unpacked.insert(slot).ptr();
                let status = crate::impls::boxing::data_into_local(
                    rt_handle, value_in, slot, value_ty.as_ptr(),
                );
                if status != RtStatus::Ok {
                    break 'unpack RtStatus::Error;
                }
                slot
            } else {
                value_in
            };

            btreemap_insert_impl(
                rt, btreemap_value_mut, btreemap_tydesc,
                key_slot, key_ty, value_slot, value_ty,
            )
        };
        // A slot held its value only on the way in; the map has it now, so the
        // storage goes back and what it held does not.
        for slot in [key_unpacked, value_unpacked].into_iter().flatten() {
            slot.release(rt);
        }
        status
    }
}

pub unsafe fn btreemap_get_impl(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    key_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        btreemap_get_inner(
            rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc,
            option_value_out, option_tydesc, false,
        )
    }
}

/// Look a key up, giving the value back packed into a `data`.
///
/// What a generic function asks for: it has no static type for the values, so
/// its option is an option of `data`, and the value is packed to reach it.
pub unsafe fn btreemap_get_as_data_impl(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    key_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        btreemap_get_inner(
            rt, btreemap_value_ref, btreemap_tydesc, key_ref, key_tydesc,
            option_value_out, option_tydesc, true,
        )
    }
}

unsafe fn btreemap_get_inner(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    key_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
    as_data: bool,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_ref.is_null()
            || key_ref.is_null()
            || option_value_out.is_null() {
            return RtStatus::Error;
        }

        // Get key and value type descriptors from map type.
        let map_value_ty = btreemap_tydesc.map_value_ty();

        let map_ptr = btreemap_value_ref as *const Map;
        let root = (*map_ptr).root as *mut MapNode;

        // Compute option layout.
        let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
        let option_tag_ptr = option_value_out as *mut u8;
        let option_payload_ptr = option_value_out.add(option_layout.payload_offset as usize);

        // If map is empty, return None.
        // The key's descriptor is checked once here, where it used to be
        // checked at every comparison; a key of another type is in no map.
        if root.is_null() || !super::cmp::eq_tydesc(key_tydesc, ty.key) {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }

        // Find the leaf node where the key would be.
        let leaf = find_leaf_for_key(root, key_ref, ty);

        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, ty);
        let values_ptr = leaf_values_ptr(leaf, ty);
        let value_size = map_value_ty.size() as usize;

        // Search for the key in the leaf.
        if let Ok(i) = search_keys(keys_ptr, len as usize, key_ref, ty) {
            // Key found! Clone the value into the option payload. A
            // caller with no static type for the value asks for it
            // packed into a `data`, which is the shape it does have.
            let value_slot = values_ptr.add(i * value_size);
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let status = if as_data {
                crate::impls::boxing::data_clone_from_local(
                    rt_handle,
                    value_slot,
                    map_value_ty.as_ptr(),
                    option_payload_ptr,
                )
            } else {
                crate::impls::clone::clone_value(
                    rt_handle,
                    value_slot,
                    map_value_ty.as_ptr(),
                    option_payload_ptr,
                )
            };

            if status != RtStatus::Ok {
                return status;
            }

            // Set option tag to Some.
            *option_tag_ptr = rtdt::OptionTag::Some as u8;
            return RtStatus::Ok;
        }

        // Key not found, return None.
        *option_tag_ptr = rtdt::OptionTag::None as u8;
        RtStatus::Ok
    }
}

/// The key of the entry at `index` in sort order, or none past the end.
///
/// A map has no positional access of its own -- it is a tree, and its order is
/// its keys' -- but a list has one, and everything written over a collection
/// here is written as a loop over `len` and an index. This is what gives a map
/// the same reach.
///
/// The leaves hold every entry in order and are chained, so this walks to the
/// leftmost leaf and follows the chain. That is O(index) per call, so a loop
/// over the whole map is quadratic in its length. A cursor would be linear, and
/// wants a kind of value the language does not have yet.
pub unsafe fn btreemap_key_at_impl(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
    as_data: bool,
) -> RtStatus {
    unsafe { btreemap_at_inner(rt, btreemap_value_ref, btreemap_tydesc, index,
                               option_value_out, option_tydesc, as_data, true) }
}

/// The value of the entry at `index` in sort order. See `btreemap_key_at_impl`.
pub unsafe fn btreemap_value_at_impl(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
    as_data: bool,
) -> RtStatus {
    unsafe { btreemap_at_inner(rt, btreemap_value_ref, btreemap_tydesc, index,
                               option_value_out, option_tydesc, as_data, false) }
}

#[allow(clippy::too_many_arguments)]
unsafe fn btreemap_at_inner(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
    as_data: bool,
    want_key: bool,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_ref.is_null() || option_value_out.is_null() {
            return RtStatus::Error;
        }

        let map_key_ty = btreemap_tydesc.map_key_ty();
        let map_value_ty = btreemap_tydesc.map_value_ty();
        let map_ptr = btreemap_value_ref as *const Map;

        let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
        let option_tag_ptr = option_value_out as *mut u8;
        let option_payload_ptr = option_value_out.add(option_layout.payload_offset as usize);

        if index >= (*map_ptr).len.0 {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }

        // Down the left spine to the first leaf, then along the chain.
        let mut node = (*map_ptr).root as *mut MapNode;
        if node.is_null() {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }
        while matches!(read_node_tag(node), MapNodeTag::Internal) {
            node = *internal_child_ptrs_ptr(node, ty);
        }

        let mut remaining = index;
        loop {
            let len = read_node_len(node) as rtdt::IndexRepr;
            if remaining < len {
                break;
            }
            remaining -= len;
            let next = *leaf_next_ptr_mut(node, ty);
            if next.is_null() {
                // The length said this entry was there, so the chain and the
                // length disagree.
                *option_tag_ptr = rtdt::OptionTag::None as u8;
                return RtStatus::Ok;
            }
            node = next;
        }

        let (slot, slot_ty) = if want_key {
            let keys_ptr = leaf_keys_ptr(node, ty);
            (keys_ptr.add(remaining as usize * map_key_ty.size() as usize), map_key_ty)
        } else {
            let values_ptr = leaf_values_ptr(node, ty);
            (values_ptr.add(remaining as usize * map_value_ty.size() as usize), map_value_ty)
        };

        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = if as_data {
            crate::impls::boxing::data_clone_from_local(
                rt_handle, slot, slot_ty.as_ptr(), option_payload_ptr)
        } else {
            crate::impls::clone::clone_value(
                rt_handle, slot, slot_ty.as_ptr(), option_payload_ptr)
        };
        if status != RtStatus::Ok {
            return status;
        }
        *option_tag_ptr = rtdt::OptionTag::Some as u8;
        RtStatus::Ok
    }
}

/// What `btreemap_collect_impl` takes out of each entry.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MapPart {
    Keys,
    Values,
    /// The key and the value, as the two fields of a tuple.
    Entries,
}

/// Every key, value or entry of a map, in sort order, cloned onto the end of a
/// list.
///
/// One walk along the leaf chain. Reaching each entry by its position instead
/// (`btreemap_key_at_impl`) walks the chain from its start every time, which
/// made reading out a whole map quadratic in its length.
pub unsafe fn btreemap_collect_impl(
    rt: &mut RtLocal,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    part: MapPart,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        let key_ty = btreemap_tydesc.map_key_ty();
        let value_ty = btreemap_tydesc.map_value_ty();
        let element_ty = list_tydesc.list_element_ty();
        let map_ptr = btreemap_value_ref as *const Map;

        let status = crate::impls::list::list_reserve_impl(
            rt, list_value_mut, list_tydesc, (*map_ptr).len.0);
        if status != RtStatus::Ok {
            return status;
        }
        let root = (*map_ptr).root as *mut MapNode;
        if root.is_null() {
            return RtStatus::Ok;
        }

        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let clone = crate::impls::clone::clone_value;
        let mut node = leftmost_leaf(root, ty);
        while !node.is_null() {
            let keys = leaf_keys_ptr(node, ty);
            let values = leaf_values_ptr(node, ty);
            for i in 0..read_node_len(node) as usize {
                let key = keys.add(i * key_ty.size() as usize);
                let value = values.add(i * value_ty.size() as usize);
                let slot = crate::impls::list::list_end_slot(list_value_mut, element_ty);
                let status = match part {
                    MapPart::Keys => clone(rt_handle, key, key_ty.as_ptr(), slot),
                    MapPart::Values => clone(rt_handle, value, value_ty.as_ptr(), slot),
                    MapPart::Entries => {
                        let pair = element_ty.tuple_info();
                        let (Some(first), Some(second)) = (pair.field(0), pair.field(1)) else {
                            return RtStatus::Error;
                        };
                        let first_slot = slot.add(first.offset() as usize);
                        let status = clone(rt_handle, key, key_ty.as_ptr(), first_slot);
                        if status != RtStatus::Ok {
                            return status;
                        }
                        let status = clone(rt_handle, value, value_ty.as_ptr(),
                            slot.add(second.offset() as usize));
                        if status != RtStatus::Ok {
                            crate::impls::destroy::any_destroy_local(
                                rt_handle, first_slot, first.tydesc().as_ptr());
                        }
                        status
                    }
                };
                if status != RtStatus::Ok {
                    return status;
                }
                crate::impls::list::list_count_last(list_value_mut, element_ty);
            }
            node = *leaf_next_ptr_mut(node, ty);
        }
        RtStatus::Ok
    }
}

/// Check if a map contains a given key.
///
/// Writes `true` to `result_out` if the key is found, `false` otherwise.
pub unsafe fn btreemap_contains_key_impl(
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    key_tydesc: rtdt::TyDescRef,
    result_out: *mut bool,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_ref.is_null()
            || key_ref.is_null()
            || result_out.is_null() {
            return RtStatus::Error;
        }


        let map_ptr = btreemap_value_ref as *const Map;
        let root = (*map_ptr).root as *mut MapNode;

        // The key's descriptor is checked once here, where it used to be
        // checked at every comparison; a key of another type is in no map.
        if root.is_null() || !super::cmp::eq_tydesc(key_tydesc, ty.key) {
            *result_out = false;
            return RtStatus::Ok;
        }

        let leaf = find_leaf_for_key(root, key_ref, ty);
        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, ty);

        if search_keys(keys_ptr, len as usize, key_ref, ty).is_ok() {
            *result_out = true;
            return RtStatus::Ok;
        }

        *result_out = false;
        RtStatus::Ok
    }
}

/// Get a pointer to a value in a map by key.
///
/// Writes the pointer to `value_ptr_out`. If the key is not found,
/// writes null.
pub unsafe fn btreemap_get_value_ref_impl(
    btreemap_value_ref: *const u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    key_tydesc: rtdt::TyDescRef,
    value_ptr_out: *mut *mut u8,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_ref.is_null()
            || key_ref.is_null()
            || value_ptr_out.is_null() {
            return RtStatus::Error;
        }

        let map_value_ty = btreemap_tydesc.map_value_ty();

        let map_ptr = btreemap_value_ref as *const Map;
        let root = (*map_ptr).root as *mut MapNode;

        // The key's descriptor is checked once here, where it used to be
        // checked at every comparison; a key of another type is in no map.
        if root.is_null() || !super::cmp::eq_tydesc(key_tydesc, ty.key) {
            *value_ptr_out = std::ptr::null_mut();
            return RtStatus::Ok;
        }

        let leaf = find_leaf_for_key(root, key_ref, ty);
        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, ty);
        let values_ptr = leaf_values_ptr(leaf, ty);
        let value_size = map_value_ty.size() as usize;

        if let Ok(i) = search_keys(keys_ptr, len as usize, key_ref, ty) {
            *value_ptr_out = values_ptr.add(i * value_size);
            return RtStatus::Ok;
        }

        *value_ptr_out = std::ptr::null_mut();
        RtStatus::Ok
    }
}

/// Set the value for an existing key in a map.
///
/// Finds the key, destroys the old value, and stores the new value.
/// Returns `Error` if the key is not found.
pub unsafe fn btreemap_set_value_impl(
    rt: &mut RtLocal,
    btreemap_value_ref: *mut u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    key_tydesc: rtdt::TyDescRef,
    value_in: *const u8,
    value_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_ref.is_null()
            || key_ref.is_null()
            || value_in.is_null() {
            return RtStatus::Error;
        }

        let map_value_ty = btreemap_tydesc.map_value_ty();

        let map_ptr = btreemap_value_ref as *const Map;
        let root = (*map_ptr).root as *mut MapNode;

        // The key's descriptor is checked once here, where it used to be
        // checked at every comparison; a key of another type is in no map.
        if root.is_null() || !super::cmp::eq_tydesc(key_tydesc, ty.key) {
            return RtStatus::Error;
        }

        let leaf = find_leaf_for_key(root, key_ref, ty);
        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, ty);
        let values_ptr = leaf_values_ptr(leaf, ty);
        let value_size = map_value_ty.size() as usize;

        if let Ok(i) = search_keys(keys_ptr, len as usize, key_ref, ty) {
            let value_slot = values_ptr.add(i * value_size);

            // Destroy old value.
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let status = crate::impls::destroy::any_destroy_local(
                rt_handle,
                value_slot,
                value_tydesc.as_ptr(),
            );
            if status != RtStatus::Ok {
                return status;
            }

            // Copy new value in.
            std::ptr::copy_nonoverlapping(
                value_in,
                value_slot,
                value_size,
            );

            return RtStatus::Ok;
        }

        RtStatus::Error
    }
}

/// Minimum number of keys a non-root node must have.
const MIN_KEYS: u32 = MAP_NODE_B - 1;

/// Result of removing from a node.
#[derive(Debug, PartialEq, Eq)]
enum RemoveResult {
    /// Key was successfully removed.
    Removed,
    /// Key was not found.
    NotFound,
    /// Node underflowed after removal.
    Underflow,
}

/// Remove a key from a leaf node.
unsafe fn leaf_remove(
    rt: &mut RtLocal,
    leaf: *mut MapNode,
    key: *const u8,
    ty: MapTy,
) -> RemoveResult {
    unsafe {
        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, ty);
        let values_ptr = leaf_values_ptr(leaf, ty);
        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;

        // Find the key in the leaf.
        if let Ok(i) = search_keys(keys_ptr, len as usize, key, ty) {
            // Found the key, destroy it and the value.
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let node_key = keys_ptr.add(i * key_size);
            let value_slot = values_ptr.add(i * value_size);
            let _ = crate::impls::destroy::any_destroy_local(rt_handle, node_key as *mut u8, ty.key.as_ptr());
            let _ = crate::impls::destroy::any_destroy_local(rt_handle, value_slot, ty.value.as_ptr());

            // Shift remaining elements left.
            if i < (len - 1) as usize {
                let shift_count = (len - 1) as usize - i;
                let src_key = keys_ptr.add((i + 1) * key_size);
                let dst_key = keys_ptr.add(i * key_size);
                std::ptr::copy(src_key, dst_key, shift_count * key_size);

                let src_val = values_ptr.add((i + 1) * value_size);
                let dst_val = values_ptr.add(i * value_size);
                std::ptr::copy(src_val, dst_val, shift_count * value_size);
            }

            write_node_len(leaf, len - 1);

            // Check if the leaf underflows.
            if len - 1 < MIN_KEYS {
                return RemoveResult::Underflow;
            } else {
                return RemoveResult::Removed;
            }
        }

        RemoveResult::NotFound
    }
}

/// Borrow a key from the left sibling.
unsafe fn borrow_from_left_leaf(
    rt: &mut RtLocal,
    parent: *mut MapNode,
    parent_key_idx: usize,
    left: *mut MapNode,
    node: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        let left_len = read_node_len(left);
        let node_len = read_node_len(node);

        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;

        let left_keys_ptr = leaf_keys_ptr(left, ty);
        let left_values_ptr = leaf_values_ptr(left, ty);
        let node_keys_ptr = leaf_keys_ptr(node, ty);
        let node_values_ptr = leaf_values_ptr(node, ty);

        // Shift node's elements right to make room.
        if node_len > 0 {
            std::ptr::copy(
                node_keys_ptr,
                node_keys_ptr.add(key_size),
                node_len as usize * key_size,
            );
            std::ptr::copy(
                node_values_ptr,
                node_values_ptr.add(value_size),
                node_len as usize * value_size,
            );
        }

        // Move the last element from left to node.
        let borrow_idx = (left_len - 1) as usize;
        std::ptr::copy_nonoverlapping(
            left_keys_ptr.add(borrow_idx * key_size),
            node_keys_ptr,
            key_size,
        );
        std::ptr::copy_nonoverlapping(
            left_values_ptr.add(borrow_idx * value_size),
            node_values_ptr,
            value_size,
        );

        write_node_len(left, left_len - 1);
        write_node_len(node, node_len + 1);

        // Update parent separator.
        let parent_keys_ptr = internal_keys_ptr(parent, ty);
        let parent_key = parent_keys_ptr.add(parent_key_idx * key_size);

        // Destroy old separator.
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let _ = crate::impls::destroy::any_destroy_local(rt_handle, parent_key, ty.key.as_ptr());

        // Clone new separator (first key of node).
        let _ = crate::impls::clone::clone_value(rt_handle, node_keys_ptr, ty.key.as_ptr(), parent_key);
    }
}

/// Borrow a key from the left sibling (internal node).
unsafe fn borrow_from_left_internal(
    _rt: &mut RtLocal,
    parent: *mut MapNode,
    parent_key_idx: usize,
    left: *mut MapNode,
    node: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        let left_len = read_node_len(left);
        let node_len = read_node_len(node);
        let key_size = ty.key.size() as usize;

        let left_keys_ptr = internal_keys_ptr(left, ty);
        let left_children_ptr = internal_child_ptrs_ptr(left, ty);
        let node_keys_ptr = internal_keys_ptr(node, ty);
        let node_children_ptr = internal_child_ptrs_ptr(node, ty);
        let parent_keys_ptr = internal_keys_ptr(parent, ty);

        // Shift node's keys and children right to make room.
        if node_len > 0 {
            std::ptr::copy(
                node_keys_ptr,
                node_keys_ptr.add(key_size),
                node_len as usize * key_size,
            );
            std::ptr::copy(
                node_children_ptr,
                node_children_ptr.add(1),
                node_len as usize + 1,
            );
        }

        // Move parent separator down to node.
        let parent_key = parent_keys_ptr.add(parent_key_idx * key_size);
        std::ptr::copy_nonoverlapping(parent_key, node_keys_ptr, key_size);

        // Move last key from left to parent.
        let left_last_key = left_keys_ptr.add((left_len - 1) as usize * key_size);
        std::ptr::copy_nonoverlapping(left_last_key, parent_key, key_size);

        // Move last child pointer from left to node.
        let left_last_child = *left_children_ptr.add(left_len as usize);
        *node_children_ptr.add(0) = left_last_child;

        write_node_len(left, left_len - 1);
        write_node_len(node, node_len + 1);
    }
}

/// Borrow a key from the right sibling (internal node).
unsafe fn borrow_from_right_internal(
    _rt: &mut RtLocal,
    parent: *mut MapNode,
    parent_key_idx: usize,
    node: *mut MapNode,
    right: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        let node_len = read_node_len(node);
        let right_len = read_node_len(right);
        let key_size = ty.key.size() as usize;

        let node_keys_ptr = internal_keys_ptr(node, ty);
        let node_children_ptr = internal_child_ptrs_ptr(node, ty);
        let right_keys_ptr = internal_keys_ptr(right, ty);
        let right_children_ptr = internal_child_ptrs_ptr(right, ty);
        let parent_keys_ptr = internal_keys_ptr(parent, ty);

        // Move parent separator down to node.
        let parent_key = parent_keys_ptr.add(parent_key_idx * key_size);
        std::ptr::copy_nonoverlapping(parent_key, node_keys_ptr.add(node_len as usize * key_size), key_size);

        // Move first child pointer from right to node.
        let right_first_child = *right_children_ptr.add(0);
        *node_children_ptr.add(node_len as usize + 1) = right_first_child;

        // Move first key from right to parent.
        std::ptr::copy_nonoverlapping(right_keys_ptr, parent_key, key_size);

        // Shift right's keys and children left.
        if right_len > 1 {
            std::ptr::copy(
                right_keys_ptr.add(key_size),
                right_keys_ptr,
                (right_len - 1) as usize * key_size,
            );
            std::ptr::copy(
                right_children_ptr.add(1),
                right_children_ptr,
                right_len as usize,
            );
        }

        write_node_len(node, node_len + 1);
        write_node_len(right, right_len - 1);
    }
}

/// Borrow a key from the right sibling.
unsafe fn borrow_from_right_leaf(
    rt: &mut RtLocal,
    parent: *mut MapNode,
    parent_key_idx: usize,
    node: *mut MapNode,
    right: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        let node_len = read_node_len(node);
        let right_len = read_node_len(right);

        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;

        let node_keys_ptr = leaf_keys_ptr(node, ty);
        let node_values_ptr = leaf_values_ptr(node, ty);
        let right_keys_ptr = leaf_keys_ptr(right, ty);
        let right_values_ptr = leaf_values_ptr(right, ty);

        // Move first element from right to node.
        std::ptr::copy_nonoverlapping(
            right_keys_ptr,
            node_keys_ptr.add(node_len as usize * key_size),
            key_size,
        );
        std::ptr::copy_nonoverlapping(
            right_values_ptr,
            node_values_ptr.add(node_len as usize * value_size),
            value_size,
        );

        // Shift right's elements left.
        if right_len > 1 {
            std::ptr::copy(
                right_keys_ptr.add(key_size),
                right_keys_ptr,
                (right_len - 1) as usize * key_size,
            );
            std::ptr::copy(
                right_values_ptr.add(value_size),
                right_values_ptr,
                (right_len - 1) as usize * value_size,
            );
        }

        write_node_len(node, node_len + 1);
        write_node_len(right, right_len - 1);

        // Update parent separator.
        let parent_keys_ptr = internal_keys_ptr(parent, ty);
        let parent_key = parent_keys_ptr.add(parent_key_idx * key_size);

        // Destroy old separator.
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let _ = crate::impls::destroy::any_destroy_local(rt_handle, parent_key, ty.key.as_ptr());

        // Clone new separator (first key of right).
        let _ = crate::impls::clone::clone_value(rt_handle, right_keys_ptr, ty.key.as_ptr(), parent_key);
    }
}

/// Merge node with its left sibling (internal node).
unsafe fn merge_with_left_internal(
    rt: &mut RtLocal,
    parent: *mut MapNode,
    parent_key_idx: usize,
    left: *mut MapNode,
    node: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        let left_len = read_node_len(left);
        let node_len = read_node_len(node);
        let key_size = ty.key.size() as usize;

        let left_keys_ptr = internal_keys_ptr(left, ty);
        let left_children_ptr = internal_child_ptrs_ptr(left, ty);
        let node_keys_ptr = internal_keys_ptr(node, ty);
        let node_children_ptr = internal_child_ptrs_ptr(node, ty);
        let parent_keys_ptr = internal_keys_ptr(parent, ty);

        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        // Clone parent separator down to left.
        let parent_key = parent_keys_ptr.add(parent_key_idx * key_size);
        let left_separator_slot = left_keys_ptr.add(left_len as usize * key_size);
        let _ = crate::impls::clone::clone_value(rt_handle, parent_key, ty.key.as_ptr(), left_separator_slot);

        // Copy all keys from node to left.
        if node_len > 0 {
            std::ptr::copy_nonoverlapping(
                node_keys_ptr,
                left_keys_ptr.add((left_len + 1) as usize * key_size),
                node_len as usize * key_size,
            );
        }

        // Copy all child pointers from node to left.
        std::ptr::copy_nonoverlapping(
            node_children_ptr,
            left_children_ptr.add((left_len + 1) as usize),
            (node_len + 1) as usize,
        );

        write_node_len(left, left_len + 1 + node_len);

        // Free the merged node (but don't destroy keys/values, they're now in left).
        free_node(rt, node, ty);
    }
}

/// Merge node with its left sibling.
unsafe fn merge_with_left_leaf(
    rt: &mut RtLocal,
    left: *mut MapNode,
    node: *mut MapNode,
    ty: MapTy,
) {
    unsafe {
        let left_len = read_node_len(left);
        let node_len = read_node_len(node);

        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;

        let left_keys_ptr = leaf_keys_ptr(left, ty);
        let left_values_ptr = leaf_values_ptr(left, ty);
        let node_keys_ptr = leaf_keys_ptr(node, ty);
        let node_values_ptr = leaf_values_ptr(node, ty);

        // Copy all elements from node to left.
        std::ptr::copy_nonoverlapping(
            node_keys_ptr,
            left_keys_ptr.add(left_len as usize * key_size),
            node_len as usize * key_size,
        );
        std::ptr::copy_nonoverlapping(
            node_values_ptr,
            left_values_ptr.add(left_len as usize * value_size),
            node_len as usize * value_size,
        );

        write_node_len(left, left_len + node_len);

        // Update next_leaf pointer.
        let node_next = *leaf_next_ptr_mut(node, ty);
        *leaf_next_ptr_mut(left, ty) = node_next;

        // Free the merged node.
        free_node(rt, node, ty);
    }
}

/// Remove a key from an internal node.
unsafe fn internal_remove_key(
    rt: &mut RtLocal,
    node: *mut MapNode,
    key_idx: usize,
    ty: MapTy,
) {
    unsafe {
        let len = read_node_len(node);
        let keys_ptr = internal_keys_ptr(node, ty);
        let children_ptr = internal_child_ptrs_ptr(node, ty);
        let key_size = ty.key.size() as usize;

        // Destroy the key.
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let key_slot = keys_ptr.add(key_idx * key_size);
        let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());

        // Shift keys left.
        if key_idx < (len - 1) as usize {
            std::ptr::copy(
                keys_ptr.add((key_idx + 1) * key_size),
                keys_ptr.add(key_idx * key_size),
                (len - 1 - key_idx as u32) as usize * key_size,
            );
        }

        // Shift child pointers left.
        if key_idx + 1 < len as usize {
            std::ptr::copy(
                children_ptr.add(key_idx + 2),
                children_ptr.add(key_idx + 1),
                (len - 1 - key_idx as u32) as usize,
            );
        }

        write_node_len(node, len - 1);
    }
}

/// Fix underflow in a child of an internal node.
unsafe fn fix_underflow(
    rt: &mut RtLocal,
    parent: *mut MapNode,
    child_idx: usize,
    ty: MapTy,
) -> RemoveResult {
    unsafe {
        let parent_len = read_node_len(parent);
        let children_ptr = internal_child_ptrs_ptr(parent, ty);
        let child = *children_ptr.add(child_idx);
        let child_tag = read_node_tag(child);

        // Try to borrow from left sibling.
        if child_idx > 0 {
            let left = *children_ptr.add(child_idx - 1);
            let left_len = read_node_len(left);

            if left_len > MIN_KEYS {
                match child_tag {
                    MapNodeTag::Leaf => {
                        borrow_from_left_leaf(
                            rt,
                            parent,
                            child_idx - 1,
                            left,
                            child,
                            ty,
                        );
                    }
                    MapNodeTag::Internal => {
                        borrow_from_left_internal(
                            rt,
                            parent,
                            child_idx - 1,
                            left,
                            child,
                            ty,
                        );
                    }
                }
                return RemoveResult::Removed;
            }
        }

        // Try to borrow from right sibling.
        if child_idx < parent_len as usize {
            let right = *children_ptr.add(child_idx + 1);
            let right_len = read_node_len(right);

            if right_len > MIN_KEYS {
                match child_tag {
                    MapNodeTag::Leaf => {
                        borrow_from_right_leaf(
                            rt,
                            parent,
                            child_idx,
                            child,
                            right,
                            ty,
                        );
                    }
                    MapNodeTag::Internal => {
                        borrow_from_right_internal(
                            rt,
                            parent,
                            child_idx,
                            child,
                            right,
                            ty,
                        );
                    }
                }
                return RemoveResult::Removed;
            }
        }

        // Must merge.
        if child_idx > 0 {
            // Merge with left sibling.
            let left = *children_ptr.add(child_idx - 1);
            match child_tag {
                MapNodeTag::Leaf => {
                    merge_with_left_leaf(rt, left, child, ty);
                }
                MapNodeTag::Internal => {
                    merge_with_left_internal(rt, parent, child_idx - 1, left, child, ty);
                }
            }
            internal_remove_key(rt, parent, child_idx - 1, ty);
        } else {
            // Merge with right sibling.
            let right = *children_ptr.add(child_idx + 1);
            match child_tag {
                MapNodeTag::Leaf => {
                    merge_with_left_leaf(rt, child, right, ty);
                }
                MapNodeTag::Internal => {
                    merge_with_left_internal(rt, parent, child_idx, child, right, ty);
                }
            }
            internal_remove_key(rt, parent, child_idx, ty);
        }

        // Check if parent underflows.
        let new_parent_len = read_node_len(parent);
        if new_parent_len < MIN_KEYS {
            RemoveResult::Underflow
        } else {
            RemoveResult::Removed
        }
    }
}

/// Remove a key from the subtree rooted at node.
unsafe fn remove_recursive(
    rt: &mut RtLocal,
    node: *mut MapNode,
    key: *const u8,
    ty: MapTy,
) -> RemoveResult {
    unsafe {
        let tag = read_node_tag(node);
        match tag {
            MapNodeTag::Leaf => leaf_remove(rt, node, key, ty),
            MapNodeTag::Internal => {
                let len = read_node_len(node);
                let keys_ptr = internal_keys_ptr(node, ty);
                let children_ptr = internal_child_ptrs_ptr(node, ty);
                let key_size = ty.key.size() as usize;

                // Find the child to descend into.
                let mut child_idx = 0;
                for i in 0..len as usize {
                    let node_key = keys_ptr.add(i * key_size);
                    let cmp_result =
                        ty.ord.cmp(key, node_key);
                    match cmp_result {
                        crate::c::RtOrdering::Less => break,
                        crate::c::RtOrdering::Equal => {
                            child_idx = i + 1;
                            break;
                        }
                        crate::c::RtOrdering::Greater => {
                            child_idx = i + 1;
                        }
                        crate::c::RtOrdering::Error => break,
                    }
                }

                let child = *children_ptr.add(child_idx);
                let result = remove_recursive(rt, child, key, ty);

                match result {
                    RemoveResult::Underflow => {
                        fix_underflow(rt, node, child_idx, ty)
                    }
                    other => other,
                }
            }
        }
    }
}

/// Remove a key from the BTreeMap.
pub unsafe fn btreemap_remove_impl(
    rt: &mut RtLocal,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: rtdt::TyDescRef,
    key_ref: *const u8,
    _key_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        let ty = MapTy::of(btreemap_tydesc);
        if btreemap_value_mut.is_null()
            || key_ref.is_null()
        {
            return RtStatus::Error;
        }


        let map_ptr = btreemap_value_mut as *mut Map;
        let root = (*map_ptr).root as *mut MapNode;

        // If map is empty, nothing to remove.
        if root.is_null() {
            return RtStatus::Ok;
        }

        let result = remove_recursive(rt, root, key_ref, ty);

        match result {
            RemoveResult::NotFound => {
                // Key wasn't found, but that's not an error.
                RtStatus::Ok
            }
            RemoveResult::Removed => {
                (*map_ptr).len -= rtdt::Index::ONE;
                RtStatus::Ok
            }
            RemoveResult::Underflow => {
                // Root underflowed.
                (*map_ptr).len -= rtdt::Index::ONE;

                let root_len = read_node_len(root);
                if root_len == 0 {
                    // Root is empty.
                    let root_tag = read_node_tag(root);
                    match root_tag {
                        MapNodeTag::Internal => {
                            // Root is internal with 0 keys, so it has 1 child.
                            // Make that child the new root.
                            let children_ptr = internal_child_ptrs_ptr(root, ty);
                            let new_root = *children_ptr.add(0);
                            free_node(rt, root, ty);
                            (*map_ptr).root = new_root;
                        }
                        MapNodeTag::Leaf => {
                            // Root is leaf with 0 keys, map is now empty.
                            free_node(rt, root, ty);
                            (*map_ptr).root = std::ptr::null();
                        }
                    }
                }

                RtStatus::Ok
            }
        }
    }
}

/// Recursively clone a map subtree, collecting leaf nodes in order.
unsafe fn clone_tree_recursive(
    rt: &mut RtLocal,
    node: *const MapNode,
    ty: MapTy,
    leaves: &mut Vec<*mut MapNode>,
) -> *mut MapNode {
    unsafe {
        if node.is_null() {
            return std::ptr::null_mut();
        }

        let tag = read_node_tag(node);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        match tag {
            MapNodeTag::Internal => {
                // Allocate a new internal node.
                let new_node = alloc_internal_node(rt, ty);
                if new_node.is_null() {
                    return std::ptr::null_mut();
                }

                let len = read_node_len(node);
                write_node_len(new_node, len);

                let keys_ptr = internal_keys_ptr(node as *mut MapNode, ty);
                let new_keys_ptr = internal_keys_ptr(new_node, ty);
                let key_size = ty.key.size() as usize;

                // Clone all keys.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::impls::clone::clone_value(rt_handle, key_src, ty.key.as_ptr(), key_dst);
                    if status != RtStatus::Ok {
                        // Destroy already-cloned keys.
                        for j in 0..i {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());
                        }
                        free_node(rt, new_node, ty);
                        return std::ptr::null_mut();
                    }
                }

                // Recursively clone all children.
                let children_ptr = internal_child_ptrs_ptr(node as *mut MapNode, ty);
                let new_children_ptr = internal_child_ptrs_ptr(new_node, ty);

                // Initialize all child pointers to null for safe cleanup.
                for i in 0..=(len as usize) {
                    *new_children_ptr.add(i) = std::ptr::null_mut();
                }

                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    let new_child = clone_tree_recursive(rt, child, ty, leaves);
                    // Store the cloned child pointer immediately so error handling can access it.
                    *new_children_ptr.add(i) = new_child;
                    if new_child.is_null() && !child.is_null() {
                        // Destroy all keys.
                        for j in 0..len as usize {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());
                        }
                        // Destroy already-cloned children (including current one which is null).
                        for j in 0..=i {
                            let cloned_child = *new_children_ptr.add(j);
                            destroy_tree_recursive(rt, cloned_child, ty);
                        }
                        free_node(rt, new_node, ty);
                        return std::ptr::null_mut();
                    }
                }

                new_node
            }
            MapNodeTag::Leaf => {
                // Allocate a new leaf node.
                let new_leaf = alloc_leaf_node(rt, ty);
                if new_leaf.is_null() {
                    return std::ptr::null_mut();
                }

                let len = read_node_len(node);
                write_node_len(new_leaf, len);

                let keys_ptr = leaf_keys_ptr(node as *mut MapNode, ty);
                let values_ptr = leaf_values_ptr(node as *mut MapNode, ty);
                let new_keys_ptr = leaf_keys_ptr(new_leaf, ty);
                let new_values_ptr = leaf_values_ptr(new_leaf, ty);

                let key_size = ty.key.size() as usize;
                let value_size = ty.value.size() as usize;

                // Clone all keys first.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::impls::clone::clone_value(rt_handle, key_src, ty.key.as_ptr(), key_dst);
                    if status != RtStatus::Ok {
                        // Destroy already-cloned keys.
                        for j in 0..i {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());
                        }
                        free_node(rt, new_leaf, ty);
                        return std::ptr::null_mut();
                    }
                }

                // Clone all values.
                for i in 0..len as usize {
                    let value_src = values_ptr.add(i * value_size);
                    let value_dst = new_values_ptr.add(i * value_size);
                    let status = crate::impls::clone::clone_value(rt_handle, value_src, ty.value.as_ptr(), value_dst);
                    if status != RtStatus::Ok {
                        // Destroy all cloned keys.
                        for j in 0..len as usize {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, ty.key.as_ptr());
                        }
                        // Destroy already-cloned values (not including current one, which failed).
                        for j in 0..i {
                            let value_slot = new_values_ptr.add(j * value_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, value_slot, ty.value.as_ptr());
                        }
                        free_node(rt, new_leaf, ty);
                        return std::ptr::null_mut();
                    }
                }

                // Collect this leaf in order (left-to-right traversal).
                leaves.push(new_leaf);

                new_leaf
            }
        }
    }
}

/// Clone a map tree.
pub unsafe fn btreemap_clone_tree(
    rt: &mut RtLocal,
    root: *const MapNode,
    ty: MapTy,
) -> *mut MapNode {
    unsafe {
        let mut leaves = Vec::new();
        let new_root = clone_tree_recursive(rt, root, ty, &mut leaves);

        // Link leaf nodes via next_leaf pointers.
        if !leaves.is_empty() {
            let layout = *ty.leaf;

            for i in 0..leaves.len() - 1 {
                let next_leaf_ptr = (leaves[i] as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut MapNode;
                *next_leaf_ptr = leaves[i + 1];
            }
        }

        new_root
    }
}

/// Build a Map B-tree from sorted slices of already-instantiated keys and values.
///
/// Takes ownership of the keys and values by moving them from the input buffers
/// into the tree structure. The input buffers should not be used after this call.
/// Keys must already be sorted.
pub unsafe fn btreemap_build_from_sorted_slices(
    rt: &mut RtLocal,
    map_out: *mut Map,
    key_tydesc: rtdt::TyDescRef,
    value_tydesc: rtdt::TyDescRef,
    keys_ptr: *mut u8,
    values_ptr: *mut u8,
    num_entries: rtdt::IndexRepr,
) -> RtStatus {
    unsafe {
        if map_out.is_null() {
            return RtStatus::Error;
        }

        // Called with the key and value types alone, so the layouts are worked
        // out here, once for the whole build.
        let leaf = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
        let internal = rtdt::layout::compute_map_internal_node_layout(key_tydesc);
        let ty = MapTy {
            key: key_tydesc, value: value_tydesc, leaf: &leaf, internal: &internal,
            ord: super::cmp::KeyOrd::of(key_tydesc),
        };

        // Handle empty case.
        if num_entries == 0 || keys_ptr.is_null() || values_ptr.is_null() {
            (*map_out).root = std::ptr::null();
            (*map_out).len = rtdt::Index::ZERO;
            return RtStatus::Ok;
        }

        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;
        let leaf_layout = *ty.leaf;

        // Step 1: Build all leaf nodes.
        let num_entries_usize = num_entries as usize;
        let num_leaves = (num_entries_usize + MAP_NODE_CAPACITY as usize - 1) / MAP_NODE_CAPACITY as usize;
        let mut leaves: Vec<*mut MapNode> = Vec::with_capacity(num_leaves);

        let entries_per_leaf = (num_entries_usize + num_leaves - 1) / num_leaves;
        let mut entry_idx = 0usize;

        for _ in 0..num_leaves {
            let leaf_entries = entries_per_leaf.min(num_entries_usize - entry_idx);

            // Allocate leaf node.
            let leaf_node = rt.alloc.alloc(leaf_layout.size, leaf_layout.align, 1);
            if leaf_node.is_null() {
                // Cleanup already-created leaves.
                cleanup_map_leaves_internal(rt, &leaves, ty, &leaf_layout);
                return RtStatus::Error;
            }

            // Initialize node header.
            *leaf_node = MapNodeTag::Leaf as u8;
            *(leaf_node.add(4) as *mut u32) = leaf_entries as u32;

            // Initialize next_leaf pointer to null (will be linked later).
            let next_leaf_ptr = leaf_node.add(leaf_layout.next_leaf_offset as usize) as *mut *mut MapNode;
            *next_leaf_ptr = std::ptr::null_mut();

            // Move keys and values from input buffers to leaf.
            let keys_array = leaf_node.add(leaf_layout.keys_offset as usize);
            let values_array = leaf_node.add(leaf_layout.values_offset as usize);
            for i in 0..leaf_entries {
                let key_src = keys_ptr.add((entry_idx + i) * key_size);
                let key_dst = keys_array.add(i * key_size);
                std::ptr::copy_nonoverlapping(key_src, key_dst, key_size);

                let value_src = values_ptr.add((entry_idx + i) * value_size);
                let value_dst = values_array.add(i * value_size);
                std::ptr::copy_nonoverlapping(value_src, value_dst, value_size);
            }

            leaves.push(leaf_node as *mut MapNode);
            entry_idx += leaf_entries;
        }

        // Link leaf nodes via next_leaf pointers.
        for i in 0..leaves.len() - 1 {
            let next_leaf_ptr = (leaves[i] as *mut u8).add(leaf_layout.next_leaf_offset as usize) as *mut *mut MapNode;
            *next_leaf_ptr = leaves[i + 1];
        }

        // If only one leaf, it's the root.
        if leaves.len() == 1 {
            (*map_out).root = leaves[0] as *const MapNode;
            (*map_out).len = rtdt::Index(num_entries);
            return RtStatus::Ok;
        }

        // Step 2: Build internal levels bottom-up.
        let internal_layout = *ty.internal;
        let mut current_level = leaves;

        loop {
            let num_nodes = current_level.len();
            if num_nodes == 1 {
                (*map_out).root = current_level[0] as *const MapNode;
                (*map_out).len = rtdt::Index(num_entries);
                return RtStatus::Ok;
            }

            // Build next level of internal nodes.
            let capacity_plus_one = (MAP_NODE_CAPACITY + 1) as usize;
            let num_parents = (num_nodes + capacity_plus_one - 1) / capacity_plus_one;
            let mut parents: Vec<*mut MapNode> = Vec::with_capacity(num_parents);

            let children_per_parent = (num_nodes + num_parents - 1) / num_parents;
            let mut child_idx = 0usize;

            for _ in 0..num_parents {
                let num_children = children_per_parent.min(num_nodes - child_idx);

                // Allocate internal node.
                let internal_node = rt.alloc.alloc(internal_layout.size, internal_layout.align, 1);
                if internal_node.is_null() {
                    // TODO: proper cleanup of partial tree
                    return RtStatus::Error;
                }

                // Initialize node header: tag = Internal, len = num_children - 1 (number of keys).
                *internal_node = MapNodeTag::Internal as u8;
                *(internal_node.add(4) as *mut u32) = (num_children - 1) as u32;

                // Get pointers to keys and child_ptrs arrays.
                let keys_array = internal_node.add(internal_layout.keys_offset as usize);
                let child_ptrs_array = internal_node.add(internal_layout.child_ptrs_offset as usize) as *mut *mut MapNode;

                // Set child pointers.
                for i in 0..num_children {
                    *child_ptrs_array.add(i) = current_level[child_idx + i];
                }

                // Extract separator keys (first key from each child except the first).
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                for i in 1..num_children {
                    let child_node = current_level[child_idx + i];
                    let child_is_leaf = *(child_node as *const u8) == MapNodeTag::Leaf as u8;

                    let first_key_src = if child_is_leaf {
                        (child_node as *const u8).add(leaf_layout.keys_offset as usize)
                    } else {
                        (child_node as *const u8).add(internal_layout.keys_offset as usize)
                    };

                    let key_dest = keys_array.add((i - 1) * key_size);
                    // Clone the key for the internal node.
                    let status = crate::impls::clone::clone_value(
                        rt_handle,
                        first_key_src,
                        ty.key.as_ptr(),
                        key_dest,
                    );
                    if status != RtStatus::Ok {
                        // TODO: proper cleanup of partial tree
                        return RtStatus::Error;
                    }
                }

                parents.push(internal_node as *mut MapNode);
                child_idx += num_children;
            }

            current_level = parents;
        }
    }
}

/// Helper to clean up partially constructed map leaves.
unsafe fn cleanup_map_leaves_internal(
    rt: &mut RtLocal,
    leaves: &[*mut MapNode],
    ty: MapTy,
    leaf_layout: &rtdt::MapNodeLeafLayout,
) {
    unsafe {
        let key_size = ty.key.size() as usize;
        let value_size = ty.value.size() as usize;
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        for &leaf_node in leaves {
            let len = (*(leaf_node as *const MapNode)).len as usize;
            let keys_array = (leaf_node as *mut u8).add(leaf_layout.keys_offset as usize);
            let values_array = (leaf_node as *mut u8).add(leaf_layout.values_offset as usize);

            // Destroy all entries in this leaf.
            for i in 0..len {
                let key_to_destroy = keys_array.add(i * key_size);
                let value_to_destroy = values_array.add(i * value_size);
                crate::impls::destroy::any_destroy_local(rt_handle, key_to_destroy, ty.key.as_ptr());
                crate::impls::destroy::any_destroy_local(rt_handle, value_to_destroy, ty.value.as_ptr());
            }

            // Free the leaf node.
            rt.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node as *mut u8);
        }
    }
}

/// Create a BTreeMap from a slice of key-value pairs.
///
/// Takes a slice of (K, V) tuples and creates a map by cloning and inserting each pair.
pub unsafe fn btreemap_clone_from_slice_impl(
    rt: &mut RtLocal,
    slice_ptr_ref: *const u8,
    slice_ptr_len: rtdt::IndexRepr,
    slice_element_tydesc: *const TyDesc,
    btreemap_value_out: *mut u8,
    btreemap_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if btreemap_value_out.is_null() || slice_element_tydesc.is_null() {
            return RtStatus::Error;
        }

        // Create an empty map.
        let status = btreemap_create_impl(rt, btreemap_value_out, btreemap_tydesc);
        if status != RtStatus::Ok {
            return status;
        }

        // If the slice is empty, we're done.
        if slice_ptr_len == 0 || slice_ptr_ref.is_null() {
            return RtStatus::Ok;
        }

        // Get map key and value type descriptors.
        let map_key_ty = btreemap_tydesc.map_key_ty();
        let map_value_ty = btreemap_tydesc.map_value_ty();

        // The slice element should be a tuple (K, V).
        let tuple_ty = rtdt::TyDescRef::from_ptr(slice_element_tydesc);
        if tuple_ty.tuple_info().num_fields() != 2 {
            return RtStatus::Error;
        }

        let mut fields = tuple_ty.iter_tuple_fields();
        let key_field = fields.next().unwrap();
        let value_field = fields.next().unwrap();
        let element_size = tuple_ty.size() as usize;

        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        // Iterate through each element in the slice.
        for i in 0..slice_ptr_len {
            let element_ptr = slice_ptr_ref.add(i as usize * element_size);

            // Get pointers to key and value within the tuple.
            let key_ptr = element_ptr.add(key_field.offset() as usize);
            let value_ptr = element_ptr.add(value_field.offset() as usize);

            // Clone the key and value into temporary buffers.
            let key_size = map_key_ty.size() as usize;
            let value_size = map_value_ty.size() as usize;

            let mut key_buf = AlignedBuffer::with_align(key_size, map_key_ty.align() as usize);
            let mut value_buf = AlignedBuffer::with_align(value_size, map_value_ty.align() as usize);

            let status = crate::impls::clone::clone_value(
                rt_handle,
                key_ptr,
                key_field.tydesc().as_ptr(),
                key_buf.as_mut_ptr(),
            );
            if status != RtStatus::Ok {
                return status;
            }

            let status = crate::impls::clone::clone_value(
                rt_handle,
                value_ptr,
                value_field.tydesc().as_ptr(),
                value_buf.as_mut_ptr(),
            );
            if status != RtStatus::Ok {
                // Clean up the cloned key before returning.
                let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_buf.as_mut_ptr(), map_key_ty.as_ptr());
                return status;
            }

            // Insert the cloned key-value pair into the map.
            let status = btreemap_insert_impl(
                rt,
                btreemap_value_out,
                btreemap_tydesc,
                key_buf.as_mut_ptr(),
                map_key_ty,
                value_buf.as_mut_ptr(),
                map_value_ty,
            );

            // Note: btreemap_insert_impl takes ownership of key and value (by-move semantics),
            // so we don't need to destroy them here. The Vec will be dropped automatically.

            if status != RtStatus::Ok {
                return status;
            }
        }

        RtStatus::Ok
    }
}
