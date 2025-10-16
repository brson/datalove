//! Runtime implementation of BTreeMap (ordered map).
//!
//! Uses a B+tree structure with fixed-capacity nodes aligned to allocator size classes.

use rmx::prelude::*;
use crate::alloc::LocalRt;
use crate::rtdt::{self, *};
use crate::RtStatus;

// Offset constants for MapNode header fields.
const TAG_OFFSET: u32 = 0;
const LEN_OFFSET: u32 = 4;  // Aligned to u32.
const HEADER_SIZE: u32 = 8;  // tag (u8) + padding + len (u32).

/// Allocate and initialize a new internal node.
unsafe fn alloc_internal_node(
    rt: &mut LocalRt,
    key_tydesc: *const TyDesc,
) -> *mut MapNode {
    unsafe {
        let layout = rtdt::layout::compute_map_internal_node_layout(key_tydesc);

        // Allocate the node.
        let ptr = rt.alloc(layout.size, layout.align, 1);
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
    rt: &mut LocalRt,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut MapNode {
    unsafe {
        let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);

        // Allocate the node.
        let ptr = rt.alloc(layout.size, layout.align, 1);
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let node = ptr as *mut MapNode;

        // Initialize header.
        write_node_tag(node, MapNodeTag::Leaf);
        write_node_len(node, 0);

        // Initialize next_leaf pointer to null.
        let next_ptr = leaf_next_ptr_mut(node, key_tydesc, value_tydesc);
        *next_ptr = std::ptr::null_mut();

        node
    }
}

/// Free a node and its contents.
unsafe fn free_node(
    rt: &mut LocalRt,
    node: *mut MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) {
    unsafe {
        if node.is_null() {
            return;
        }

        let tag = read_node_tag(node);

        match tag {
            MapNodeTag::Internal => {
                let layout = rtdt::layout::compute_map_internal_node_layout(key_tydesc);
                rt.free(layout.size, layout.align, 1, node as *mut u8);
            }
            MapNodeTag::Leaf => {
                let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
                rt.free(layout.size, layout.align, 1, node as *mut u8);
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
    key_tydesc: *const TyDesc,
) -> *mut u8 {
    unsafe {
        let layout = rtdt::layout::compute_map_internal_node_layout(key_tydesc);
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Get pointer to child pointers array in an internal node.
#[inline]
unsafe fn internal_child_ptrs_ptr(
    node: *mut MapNode,
    key_tydesc: *const TyDesc,
) -> *mut *mut MapNode {
    unsafe {
        let layout = rtdt::layout::compute_map_internal_node_layout(key_tydesc);
        (node as *mut u8).add(layout.child_ptrs_offset as usize) as *mut *mut MapNode
    }
}

// Leaf node accessors.

/// Get pointer to next_leaf field in a leaf node.
#[inline]
unsafe fn leaf_next_ptr_mut(
    node: *mut MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut *mut MapNode {
    unsafe {
        let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
        (node as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut MapNode
    }
}

/// Get pointer to keys array in a leaf node.
#[inline]
unsafe fn leaf_keys_ptr(
    node: *mut MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut u8 {
    unsafe {
        let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Get pointer to values array in a leaf node.
#[inline]
unsafe fn leaf_values_ptr(
    node: *mut MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut u8 {
    unsafe {
        let layout = rtdt::layout::compute_map_leaf_node_layout(key_tydesc, value_tydesc);
        (node as *mut u8).add(layout.values_offset as usize)
    }
}

/// Create an empty BTreeMap.
pub unsafe fn btreemap_create_impl(
    rt: &mut LocalRt,
    value_out: *mut u8,
    tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if value_out.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

        // Get key and value type descriptors.
        let map_info = (*tydesc).type_info.map;
        let key_tydesc = map_info.key_tydesc;
        let value_tydesc = map_info.value_tydesc;

        // Create an empty map (null root, zero length).
        let map_ptr = value_out as *mut Map;
        (*map_ptr).root = std::ptr::null_mut();
        (*map_ptr).len = 0;

        RtStatus::Ok
    }
}

/// Destroy a BTreeMap and free all nodes.
pub unsafe fn btreemap_destroy_impl(
    rt: &mut LocalRt,
    value_in: *mut u8,
    tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if value_in.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

        let map_info = (*tydesc).type_info.map;
        let key_tydesc = map_info.key_tydesc;
        let value_tydesc = map_info.value_tydesc;

        let map_ptr = value_in as *mut Map;
        let root = (*map_ptr).root as *mut MapNode;

        if !root.is_null() {
            destroy_tree_recursive(rt, root, key_tydesc, value_tydesc);
        }

        // Clear the map struct.
        (*map_ptr).root = std::ptr::null_mut();
        (*map_ptr).len = 0;

        RtStatus::Ok
    }
}

/// Recursively destroy a subtree.
unsafe fn destroy_tree_recursive(
    rt: &mut LocalRt,
    node: *mut MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
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
                let keys_ptr = internal_keys_ptr(node, key_tydesc);
                let key_size = (*key_tydesc).size as usize;
                let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let _ = crate::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                }

                // Recursively destroy children.
                let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    destroy_tree_recursive(rt, child, key_tydesc, value_tydesc);
                }
            }
            MapNodeTag::Leaf => {
                let len = read_node_len(node);
                let keys_ptr = leaf_keys_ptr(node, key_tydesc, value_tydesc);
                let values_ptr = leaf_values_ptr(node, key_tydesc, value_tydesc);
                let key_size = (*key_tydesc).size as usize;
                let value_size = (*value_tydesc).size as usize;
                let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;

                // Destroy all keys and values in the leaf node.
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let value_slot = values_ptr.add(i * value_size);
                    let _ = crate::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                    let _ = crate::destroy::any_destroy_local(rt_handle, value_slot, value_tydesc);
                }
            }
        }

        // Free the node itself.
        free_node(rt, node, key_tydesc, value_tydesc);
    }
}

/// Clear a BTreeMap (destroy and recreate empty).
pub unsafe fn btreemap_clear_impl(
    rt: &mut LocalRt,
    value_mut: *mut u8,
    tydesc: *const TyDesc,
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
    separator_key_buf: Vec<u8>,
    /// The new right sibling node created by the split.
    new_node: *mut MapNode,
    /// The result of inserting the pending key during the split.
    insert_result: LeafInsertResult,
}

impl SplitInfo {
    /// Destroy the separator key and clean up.
    unsafe fn destroy(mut self, rt: &mut LocalRt, key_tydesc: *const TyDesc) {
        unsafe {
            let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
            let _ = crate::destroy::any_destroy_local(
                rt_handle,
                self.separator_key_buf.as_mut_ptr(),
                key_tydesc,
            );
            // Vec will be dropped automatically after key is destroyed.
        }
    }
}

/// Find the leaf node where a key should be inserted.
unsafe fn find_leaf_for_key(
    mut node: *mut MapNode,
    key: *const u8,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut MapNode {
    unsafe {
        loop {
            let tag = read_node_tag(node);
            match tag {
                MapNodeTag::Leaf => return node,
                MapNodeTag::Internal => {
                    let len = read_node_len(node);
                    let keys_ptr = internal_keys_ptr(node, key_tydesc);
                    let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
                    let key_size = (*key_tydesc).size as usize;

                    // Find the child to descend into.
                    // In a B+tree, separators represent the minimum key in the right subtree,
                    // so Equal should go right.
                    let mut child_idx = 0;
                    for i in 0..len as usize {
                        let node_key = keys_ptr.add(i * key_size);
                        let cmp_result = crate::cmp::cmp_total(
                            key,
                            key_tydesc,
                            node_key,
                            key_tydesc,
                        );
                        match cmp_result {
                            crate::RtOrdering::Less => break,
                            crate::RtOrdering::Equal => {
                                // Go to right child (separator is min of right subtree).
                                child_idx = i + 1;
                                break;
                            }
                            crate::RtOrdering::Greater => {
                                child_idx = i + 1;
                            }
                            crate::RtOrdering::Error => {
                                // Should not happen if types match.
                                break;
                            }
                        }
                    }

                    node = *children_ptr.add(child_idx);
                }
            }
        }
    }
}

/// Try to insert or update a key-value pair in a leaf node.
unsafe fn leaf_insert_or_update(
    rt: &mut LocalRt,
    leaf: *mut MapNode,
    key: *const u8,
    value: *const u8,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> LeafInsertResult {
    unsafe {
        let len = read_node_len(leaf);
        let capacity = rtdt::MAP_NODE_CAPACITY;

        if len >= capacity {
            return LeafInsertResult::NeedsSplit;
        }

        let keys_ptr = leaf_keys_ptr(leaf, key_tydesc, value_tydesc);
        let values_ptr = leaf_values_ptr(leaf, key_tydesc, value_tydesc);
        let key_size = (*key_tydesc).size as usize;
        let value_size = (*value_tydesc).size as usize;

        // Find the insertion position using binary search.
        let mut insert_pos = len as usize;
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * key_size);
            let cmp_result = crate::cmp::cmp_total(
                key,
                key_tydesc,
                node_key,
                key_tydesc,
            );
            match cmp_result {
                crate::RtOrdering::Less => {
                    insert_pos = i;
                    break;
                }
                crate::RtOrdering::Equal => {
                    // Key already exists, update the value.
                    // Destroy the old value before overwriting.
                    let value_slot = values_ptr.add(i * value_size);
                    let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
                    let _ = crate::destroy::any_destroy_local(rt_handle, value_slot, value_tydesc);
                    // Move the new value (by-move semantics).
                    std::ptr::copy_nonoverlapping(value, value_slot, value_size);
                    // Destroy the input key since we're not using it (key already exists in tree).
                    let _ = crate::destroy::any_destroy_local(rt_handle, key as *mut u8, key_tydesc);
                    return LeafInsertResult::Updated;
                }
                crate::RtOrdering::Greater => continue,
                crate::RtOrdering::Error => {
                    // Should not happen.
                    insert_pos = i;
                    break;
                }
            }
        }

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
    rt: &mut LocalRt,
    leaf: *mut MapNode,
    key: *const u8,
    value: *const u8,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> core::result::Result<SplitInfo, RtStatus> {
    unsafe {
        // Allocate a new sibling leaf.
        let new_leaf = alloc_leaf_node(rt, key_tydesc, value_tydesc);
        if new_leaf.is_null() {
            return Err(RtStatus::Error);
        }

        let capacity = rtdt::MAP_NODE_CAPACITY;
        let split_point = (capacity / 2) as usize;

        let keys_ptr = leaf_keys_ptr(leaf, key_tydesc, value_tydesc);
        let values_ptr = leaf_values_ptr(leaf, key_tydesc, value_tydesc);
        let new_keys_ptr = leaf_keys_ptr(new_leaf, key_tydesc, value_tydesc);
        let new_values_ptr = leaf_values_ptr(new_leaf, key_tydesc, value_tydesc);

        let key_size = (*key_tydesc).size as usize;
        let value_size = (*value_tydesc).size as usize;

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
        let old_next = *leaf_next_ptr_mut(leaf, key_tydesc, value_tydesc);
        *leaf_next_ptr_mut(leaf, key_tydesc, value_tydesc) = new_leaf;
        *leaf_next_ptr_mut(new_leaf, key_tydesc, value_tydesc) = old_next;

        // Clone the separator key (first key of new_leaf) into a buffer.
        // In a B+tree, the separator stays in the leaf, but internal nodes need their own copy.
        let mut separator_key_buf = vec![0u8; key_size];
        let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
        let status = crate::clone::clone_value(
            rt_handle,
            new_keys_ptr,
            key_tydesc,
            separator_key_buf.as_mut_ptr(),
        );
        if status != RtStatus::Ok {
            return Err(status);
        }

        // Determine which leaf should receive the new key.
        let cmp_result = crate::cmp::cmp_total(key, key_tydesc, separator_key_buf.as_ptr(), key_tydesc);
        let mut insert_result = match cmp_result {
            crate::RtOrdering::Less => {
                // Key goes in left leaf.
                leaf_insert_or_update(rt, leaf, key, value, key_tydesc, value_tydesc)
            }
            crate::RtOrdering::Equal | crate::RtOrdering::Greater => {
                // Key goes in right leaf (new_leaf).
                // Equal goes right because separator represents min key of right subtree.
                leaf_insert_or_update(rt, new_leaf, key, value, key_tydesc, value_tydesc)
            }
            crate::RtOrdering::Error => {
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
    rt: &mut LocalRt,
    node: *mut MapNode,
    separator_key: &[u8],
    right_child: *mut MapNode,
    key_tydesc: *const TyDesc,
) -> core::result::Result<(), SplitInfo> {
    unsafe {
        let len = read_node_len(node);
        let capacity = rtdt::MAP_NODE_CAPACITY;

        if len >= capacity {
            // Node is full, need to split.
            return Err(split_internal_node(rt, node, separator_key, right_child, key_tydesc));
        }

        let keys_ptr = internal_keys_ptr(node, key_tydesc);
        let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
        let key_size = (*key_tydesc).size as usize;

        // Find insertion position.
        let mut insert_pos = len as usize;
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * key_size);
            let cmp_result = crate::cmp::cmp_total(
                separator_key.as_ptr(),
                key_tydesc,
                node_key,
                key_tydesc,
            );
            match cmp_result {
                crate::RtOrdering::Less => {
                    insert_pos = i;
                    break;
                }
                crate::RtOrdering::Equal => {
                    // Duplicate separator key shouldn't happen.
                    insert_pos = i;
                    break;
                }
                crate::RtOrdering::Greater => continue,
                crate::RtOrdering::Error => {
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
        let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
        let status = crate::clone::clone_value(
            rt_handle,
            separator_key.as_ptr(),
            key_tydesc,
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
    rt: &mut LocalRt,
    node: *mut MapNode,
    pending_key: &[u8],
    pending_child: *mut MapNode,
    key_tydesc: *const TyDesc,
) -> SplitInfo {
    unsafe {
        // Allocate a new sibling internal node.
        let new_node = alloc_internal_node(rt, key_tydesc);
        assert!(!new_node.is_null(), "Failed to allocate internal node");

        let capacity = rtdt::MAP_NODE_CAPACITY;
        let split_point = (capacity / 2) as usize;

        let keys_ptr = internal_keys_ptr(node, key_tydesc);
        let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
        let new_keys_ptr = internal_keys_ptr(new_node, key_tydesc);
        let new_children_ptr = internal_child_ptrs_ptr(new_node, key_tydesc);

        let key_size = (*key_tydesc).size as usize;

        // Clone the middle key as the separator to push up.
        let mut separator_key_buf = vec![0u8; key_size];
        let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
        let status = crate::clone::clone_value(
            rt_handle,
            keys_ptr.add(split_point * key_size),
            key_tydesc,
            separator_key_buf.as_mut_ptr(),
        );
        if status != RtStatus::Ok {
            panic!("Failed to clone separator key during internal node split");
        }

        // Destroy the separator key in the old node since we've cloned it out.
        let _ = crate::destroy::any_destroy_local(
            rt_handle,
            keys_ptr.add(split_point * key_size),
            key_tydesc,
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
        let cmp_result = crate::cmp::cmp_total(
            pending_key.as_ptr(),
            key_tydesc,
            separator_key_buf.as_ptr(),
            key_tydesc,
        );

        let insert_result = match cmp_result {
            crate::RtOrdering::Less => {
                insert_into_internal(rt, node, pending_key, pending_child, key_tydesc)
            }
            _ => {
                insert_into_internal(rt, new_node, pending_key, pending_child, key_tydesc)
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
    rt: &mut LocalRt,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const TyDesc,
    key_in: *mut u8,
    key_tydesc: *const TyDesc,
    value_in: *mut u8,
    value_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if btreemap_value_mut.is_null() || btreemap_tydesc.is_null()
            || key_in.is_null() || key_tydesc.is_null()
            || value_in.is_null() || value_tydesc.is_null() {
            return RtStatus::Error;
        }

        // Get key and value type descriptors from map type.
        let map_info = (*btreemap_tydesc).type_info.map;
        let map_key_tydesc = map_info.key_tydesc;
        let map_value_tydesc = map_info.value_tydesc;

        let key_ptr = key_in;
        let val_ptr = value_in;

        let map_ptr = btreemap_value_mut as *mut Map;
        let root = (*map_ptr).root as *mut MapNode;

        // If map is empty, create the first leaf.
        if root.is_null() {
            let leaf = alloc_leaf_node(rt, map_key_tydesc, map_value_tydesc);
            if leaf.is_null() {
                return RtStatus::Error;
            }

            // Insert the key-value pair into the empty leaf.
            let keys_ptr = leaf_keys_ptr(leaf, map_key_tydesc, map_value_tydesc);
            let values_ptr = leaf_values_ptr(leaf, map_key_tydesc, map_value_tydesc);

            // Move key and value from the input pointers (by-move semantics).
            let key_size = (*map_key_tydesc).size as usize;
            let value_size = (*map_value_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(key_ptr, keys_ptr, key_size);
            std::ptr::copy_nonoverlapping(val_ptr, values_ptr, value_size);

            write_node_len(leaf, 1);
            (*map_ptr).root = leaf as *const MapNode;
            (*map_ptr).len = 1;

            return RtStatus::Ok;
        }

        // Find the leaf where the key should be inserted, keeping track of the path.
        let mut path: Vec<*mut MapNode> = Vec::new();
        let leaf = find_leaf_with_path(root, key_ptr, map_key_tydesc, map_value_tydesc, &mut path);

        // Try to insert into the leaf.
        let result = leaf_insert_or_update(
            rt,
            leaf,
            key_ptr,
            val_ptr,
            map_key_tydesc,
            map_value_tydesc,
        );

        match result {
            LeafInsertResult::Updated => {
                // Key already existed, value was updated, len unchanged.
                RtStatus::Ok
            }
            LeafInsertResult::Inserted => {
                // Key was inserted, increment len.
                (*map_ptr).len += 1;
                RtStatus::Ok
            }
            LeafInsertResult::NeedsSplit => {
                // Leaf is full, need to split and propagate up.
                let split_info = match split_leaf(rt, leaf, key_ptr, val_ptr, map_key_tydesc, map_value_tydesc) {
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
                    map_key_tydesc,
                );

                if status == RtStatus::Ok && was_inserted {
                    (*map_ptr).len += 1;
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
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
    path: &mut Vec<*mut MapNode>,
) -> *mut MapNode {
    unsafe {
        loop {
            let tag = read_node_tag(node);
            match tag {
                MapNodeTag::Leaf => return node,
                MapNodeTag::Internal => {
                    path.push(node);

                    let len = read_node_len(node);
                    let keys_ptr = internal_keys_ptr(node, key_tydesc);
                    let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
                    let key_size = (*key_tydesc).size as usize;

                    // Find the child to descend into.
                    // In a B+tree, separators represent the minimum key in the right subtree,
                    // so Equal should go right.
                    let mut child_idx = 0;
                    for i in 0..len as usize {
                        let node_key = keys_ptr.add(i * key_size);
                        let cmp_result = crate::cmp::cmp_total(
                            key,
                            key_tydesc,
                            node_key,
                            key_tydesc,
                        );
                        match cmp_result {
                            crate::RtOrdering::Less => break,
                            crate::RtOrdering::Equal => {
                                // Go to right child (separator is min of right subtree).
                                child_idx = i + 1;
                                break;
                            }
                            crate::RtOrdering::Greater => {
                                child_idx = i + 1;
                            }
                            crate::RtOrdering::Error => break,
                        }
                    }

                    node = *children_ptr.add(child_idx);
                }
            }
        }
    }
}

/// Propagate a split up the tree.
unsafe fn propagate_split_up(
    rt: &mut LocalRt,
    root_ptr: &mut *const MapNode,
    mut child: *mut MapNode,
    mut split_info: SplitInfo,
    path: &[*mut MapNode],
    key_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        // If there's no parent, child must be the root.
        if path.is_empty() {
            // Create new internal root.
            let new_root = alloc_internal_node(rt, key_tydesc);
            if new_root.is_null() {
                return RtStatus::Error;
            }

            let root_keys_ptr = internal_keys_ptr(new_root, key_tydesc);
            let root_children_ptr = internal_child_ptrs_ptr(new_root, key_tydesc);

            // Clone separator key into new root.
            let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
            let status = crate::clone::clone_value(
                rt_handle,
                split_info.separator_key_buf.as_ptr(),
                key_tydesc,
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

            split_info.destroy(rt, key_tydesc);
            return RtStatus::Ok;
        }

        // Walk up the path, inserting separators into parents.
        for &parent in path.iter().rev() {
            match insert_into_internal(
                rt,
                parent,
                &split_info.separator_key_buf,
                split_info.new_node,
                key_tydesc,
            ) {
                Ok(()) => {
                    // Successfully inserted into parent, done!
                    split_info.destroy(rt, key_tydesc);
                    return RtStatus::Ok;
                }
                Err(new_split_info) => {
                    // Parent split, continue propagating up.
                    // Destroy the old split_info before replacing it.
                    split_info.destroy(rt, key_tydesc);
                    child = parent;
                    split_info = new_split_info;
                }
            }
        }

        // If we get here, the root split.
        let new_root = alloc_internal_node(rt, key_tydesc);
        if new_root.is_null() {
            return RtStatus::Error;
        }

        let root_keys_ptr = internal_keys_ptr(new_root, key_tydesc);
        let root_children_ptr = internal_child_ptrs_ptr(new_root, key_tydesc);

        // Clone separator key into new root.
        let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
        let status = crate::clone::clone_value(
            rt_handle,
            split_info.separator_key_buf.as_ptr(),
            key_tydesc,
            root_keys_ptr,
        );
        if status != RtStatus::Ok {
            return RtStatus::Error;
        }

        *root_children_ptr.add(0) = child;
        *root_children_ptr.add(1) = split_info.new_node;

        write_node_len(new_root, 1);
        *root_ptr = new_root;

        split_info.destroy(rt, key_tydesc);
        RtStatus::Ok
    }
}

/// Get a value from the BTreeMap by key.
///
/// Returns the value as an Option<V>:
/// - If the key is found, sets the option to Some and clones the value.
/// - If the key is not found, sets the option to None.
pub unsafe fn btreemap_get_impl(
    rt: &mut LocalRt,
    btreemap_value_ref: *const u8,
    btreemap_tydesc: *const TyDesc,
    key_ref: *const u8,
    key_tydesc: *const TyDesc,
    option_value_out: *mut u8,
    option_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if btreemap_value_ref.is_null() || btreemap_tydesc.is_null()
            || key_ref.is_null() || key_tydesc.is_null()
            || option_value_out.is_null() || option_tydesc.is_null() {
            return RtStatus::Error;
        }

        // Get key and value type descriptors from map type.
        let map_info = (*btreemap_tydesc).type_info.map;
        let map_key_tydesc = map_info.key_tydesc;
        let map_value_tydesc = map_info.value_tydesc;

        // Get the option inner type (which should be V).
        let option_info = (*option_tydesc).type_info.option;
        let inner_value_tydesc = option_info.inner_tydesc;

        let map_ptr = btreemap_value_ref as *const Map;
        let root = (*map_ptr).root as *mut MapNode;

        // Compute option layout.
        let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
        let option_tag_ptr = option_value_out as *mut u8;
        let option_payload_ptr = option_value_out.add(option_layout.payload_offset as usize);

        // If map is empty, return None.
        if root.is_null() {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }

        // Find the leaf node where the key would be.
        let leaf = find_leaf_for_key(root, key_ref, map_key_tydesc, map_value_tydesc);

        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, map_key_tydesc, map_value_tydesc);
        let values_ptr = leaf_values_ptr(leaf, map_key_tydesc, map_value_tydesc);
        let key_size = (*map_key_tydesc).size as usize;
        let value_size = (*map_value_tydesc).size as usize;

        // Search for the key in the leaf.
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * key_size);
            let cmp_result = crate::cmp::cmp_total(
                key_ref,
                key_tydesc,
                node_key,
                map_key_tydesc,
            );

            match cmp_result {
                crate::RtOrdering::Equal => {
                    // Key found! Clone the value into the option payload.
                    let value_slot = values_ptr.add(i * value_size);
                    let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;
                    let status = crate::clone::clone_value(
                        rt_handle,
                        value_slot,
                        map_value_tydesc,
                        option_payload_ptr,
                    );

                    if status != RtStatus::Ok {
                        return status;
                    }

                    // Set option tag to Some.
                    *option_tag_ptr = rtdt::OptionTag::Some as u8;
                    return RtStatus::Ok;
                }
                crate::RtOrdering::Greater => {
                    // Continue searching.
                    continue;
                }
                crate::RtOrdering::Less | crate::RtOrdering::Error => {
                    // Key not found (keys are sorted, so we've passed where it would be).
                    break;
                }
            }
        }

        // Key not found, return None.
        *option_tag_ptr = rtdt::OptionTag::None as u8;
        RtStatus::Ok
    }
}

/// Remove a key from the BTreeMap.
pub unsafe fn btreemap_remove_impl(
    _rt: &mut LocalRt,
    _btreemap_value_mut: *mut u8,
    _btreemap_tydesc: *const TyDesc,
    _key_ref: *const u8,
    _key_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        todo!("btreemap_remove_impl")
    }
}

/// Recursively clone a map subtree.
unsafe fn clone_tree_recursive(
    rt: &mut LocalRt,
    node: *const MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut MapNode {
    unsafe {
        if node.is_null() {
            return std::ptr::null_mut();
        }

        let tag = read_node_tag(node);
        let rt_handle = rt as *mut LocalRt as crate::LocalRtHandle;

        match tag {
            MapNodeTag::Internal => {
                // Allocate a new internal node.
                let new_node = alloc_internal_node(rt, key_tydesc);
                if new_node.is_null() {
                    return std::ptr::null_mut();
                }

                let len = read_node_len(node);
                write_node_len(new_node, len);

                let keys_ptr = internal_keys_ptr(node as *mut MapNode, key_tydesc);
                let new_keys_ptr = internal_keys_ptr(new_node, key_tydesc);
                let key_size = (*key_tydesc).size as usize;

                // Clone all keys.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::clone::clone_value(rt_handle, key_src, key_tydesc, key_dst);
                    if status != RtStatus::Ok {
                        // Cleanup and return null on error.
                        free_node(rt, new_node, key_tydesc, value_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                // Recursively clone all children.
                let children_ptr = internal_child_ptrs_ptr(node as *mut MapNode, key_tydesc);
                let new_children_ptr = internal_child_ptrs_ptr(new_node, key_tydesc);

                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    let new_child = clone_tree_recursive(rt, child, key_tydesc, value_tydesc);
                    if new_child.is_null() && !child.is_null() {
                        // Cleanup on error.
                        free_node(rt, new_node, key_tydesc, value_tydesc);
                        return std::ptr::null_mut();
                    }
                    *new_children_ptr.add(i) = new_child;
                }

                new_node
            }
            MapNodeTag::Leaf => {
                // Allocate a new leaf node.
                let new_leaf = alloc_leaf_node(rt, key_tydesc, value_tydesc);
                if new_leaf.is_null() {
                    return std::ptr::null_mut();
                }

                let len = read_node_len(node);
                write_node_len(new_leaf, len);

                let keys_ptr = leaf_keys_ptr(node as *mut MapNode, key_tydesc, value_tydesc);
                let values_ptr = leaf_values_ptr(node as *mut MapNode, key_tydesc, value_tydesc);
                let new_keys_ptr = leaf_keys_ptr(new_leaf, key_tydesc, value_tydesc);
                let new_values_ptr = leaf_values_ptr(new_leaf, key_tydesc, value_tydesc);

                let key_size = (*key_tydesc).size as usize;
                let value_size = (*value_tydesc).size as usize;

                // Clone all keys and values.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::clone::clone_value(rt_handle, key_src, key_tydesc, key_dst);
                    if status != RtStatus::Ok {
                        free_node(rt, new_leaf, key_tydesc, value_tydesc);
                        return std::ptr::null_mut();
                    }

                    let value_src = values_ptr.add(i * value_size);
                    let value_dst = new_values_ptr.add(i * value_size);
                    let status = crate::clone::clone_value(rt_handle, value_src, value_tydesc, value_dst);
                    if status != RtStatus::Ok {
                        free_node(rt, new_leaf, key_tydesc, value_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                // Note: We don't clone next_leaf pointers here.
                // The cloned tree will have its own leaf chain that needs to be rebuilt.
                // For now, leave next_leaf as null (initialized by alloc_leaf_node).

                new_leaf
            }
        }
    }
}

/// Clone a map tree.
pub unsafe fn btreemap_clone_tree(
    rt: &mut LocalRt,
    root: *const MapNode,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> *mut MapNode {
    unsafe {
        clone_tree_recursive(rt, root, key_tydesc, value_tydesc)
    }
}

/// Create a BTreeMap from a slice of key-value pairs.
pub unsafe fn btreemap_clone_from_slice_impl(
    _rt: &mut LocalRt,
    _btreemap_value_out: *mut u8,
    _btreemap_tydesc: *const TyDesc,
    _slice_ptr_ref: *const u8,
    _slice_ptr_len: u32,
    _slice_element_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        todo!("btreemap_clone_from_slice_impl")
    }
}
