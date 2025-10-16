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
                // Recursively destroy children.
                let len = read_node_len(node);
                let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    destroy_tree_recursive(rt, child, key_tydesc, value_tydesc);
                }
            }
            MapNodeTag::Leaf => {
                // Leaf nodes have no children to recurse on.
                // TODO: Need to properly destroy key and value data if they contain allocated resources.
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
                            crate::RtOrdering::Equal => break,
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
                    let value_slot = values_ptr.add(i * value_size);
                    std::ptr::copy_nonoverlapping(value, value_slot, value_size);
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

        // Insert the new key-value pair.
        let key_slot = keys_ptr.add(insert_pos * key_size);
        let value_slot = values_ptr.add(insert_pos * value_size);
        std::ptr::copy_nonoverlapping(key, key_slot, key_size);
        std::ptr::copy_nonoverlapping(value, value_slot, value_size);

        write_node_len(leaf, len + 1);
        LeafInsertResult::Inserted
    }
}

/// Split a full leaf and propagate the split up the tree.
unsafe fn split_leaf_and_propagate(
    rt: &mut LocalRt,
    root_ptr: &mut *const MapNode,
    leaf: *mut MapNode,
    key: *const u8,
    value: *const u8,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        // Allocate a new sibling leaf.
        let new_leaf = alloc_leaf_node(rt, key_tydesc, value_tydesc);
        if new_leaf.is_null() {
            return RtStatus::Error;
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

        // Determine which leaf should receive the new key.
        let first_key_of_new = new_keys_ptr;
        let cmp_result = crate::cmp::cmp_total(key, key_tydesc, first_key_of_new, key_tydesc);

        let insert_result = match cmp_result {
            crate::RtOrdering::Less => {
                leaf_insert_or_update(leaf, key, value, key_tydesc, value_tydesc)
            }
            _ => {
                leaf_insert_or_update(new_leaf, key, value, key_tydesc, value_tydesc)
            }
        };

        if insert_result == LeafInsertResult::NeedsSplit {
            // Still needs split - this shouldn't happen with proper split point.
            return RtStatus::Error;
        }

        // Get the separator key (first key of new_leaf).
        let separator_key = new_keys_ptr;

        // If root is a leaf, create new internal root.
        if *root_ptr == leaf as *const MapNode {
            let new_root = alloc_internal_node(rt, key_tydesc);
            if new_root.is_null() {
                return RtStatus::Error;
            }

            let root_keys_ptr = internal_keys_ptr(new_root, key_tydesc);
            let root_children_ptr = internal_child_ptrs_ptr(new_root, key_tydesc);

            // Copy separator key.
            std::ptr::copy_nonoverlapping(separator_key, root_keys_ptr, key_size);

            // Set children.
            *root_children_ptr.add(0) = leaf;
            *root_children_ptr.add(1) = new_leaf;

            write_node_len(new_root, 1);
            *root_ptr = new_root;

            return RtStatus::Ok;
        }

        // TODO: Handle propagating split to internal nodes.
        // For now, this only handles single-level trees.
        RtStatus::Ok
    }
}

/// Insert a key-value pair into the BTreeMap.
///
/// The `value_in` parameter should point to a tuple (K, V) containing the key-value pair.
pub unsafe fn btreemap_insert_impl(
    rt: &mut LocalRt,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const TyDesc,
    value_in: *mut u8,
    value_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if btreemap_value_mut.is_null() || btreemap_tydesc.is_null()
            || value_in.is_null() || value_tydesc.is_null() {
            return RtStatus::Error;
        }

        // Get key and value type descriptors from map type.
        let map_info = (*btreemap_tydesc).type_info.map;
        let map_key_tydesc = map_info.key_tydesc;
        let map_value_tydesc = map_info.value_tydesc;

        // Extract key and value from tuple.
        // The value_in should be a tuple (K, V).
        let tuple_info = (*value_tydesc).type_info.tuple;
        if tuple_info.num_fields != 2 {
            return RtStatus::Error;
        }

        let fields = std::slice::from_raw_parts(tuple_info.fields, 2);
        let key_ptr = value_in.add(fields[0].offset as usize);
        let val_ptr = value_in.add(fields[1].offset as usize);

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

            // Copy key and value.
            std::ptr::copy_nonoverlapping(key_ptr, keys_ptr, (*map_key_tydesc).size as usize);
            std::ptr::copy_nonoverlapping(val_ptr, values_ptr, (*map_value_tydesc).size as usize);

            write_node_len(leaf, 1);
            (*map_ptr).root = leaf as *const MapNode;
            (*map_ptr).len = 1;

            return RtStatus::Ok;
        }

        // Find the leaf where the key should be inserted.
        let leaf = find_leaf_for_key(root, key_ptr, map_key_tydesc, map_value_tydesc);

        // Try to insert into the leaf.
        let result = leaf_insert_or_update(
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
                // Leaf is full, need to split.
                let split_result = split_leaf_and_propagate(
                    rt,
                    &mut (*map_ptr).root,
                    leaf,
                    key_ptr,
                    val_ptr,
                    map_key_tydesc,
                    map_value_tydesc,
                );

                if split_result == RtStatus::Ok {
                    (*map_ptr).len += 1;
                }
                split_result
            }
        }
    }
}

/// Remove a key from the BTreeMap.
pub unsafe fn btreemap_remove_impl(
    _rt: &mut LocalRt,
    _btreemap_value_mut: *mut u8,
    _btreemap_tydesc: *const TyDesc,
    _value_ref: *const u8,
    _value_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        todo!("btreemap_remove_impl")
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
