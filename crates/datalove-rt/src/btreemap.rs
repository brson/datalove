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

/// Insert a key-value pair into the BTreeMap.
pub unsafe fn btreemap_insert_impl(
    _rt: &mut LocalRt,
    _btreemap_value_mut: *mut u8,
    _btreemap_tydesc: *const TyDesc,
    _value_in: *mut u8,
    _value_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        todo!("btreemap_insert_impl")
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
