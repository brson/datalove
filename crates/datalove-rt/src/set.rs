//! Set operations for the Datalove runtime.

use rmx::prelude::*;
use crate::rtdt::{self, TyDesc, Set, SetNode, SetNodeTag, SET_NODE_CAPACITY};
use crate::rt_local::RtLocal;
use crate::RtStatus;

/// Reads the tag from a set node.
unsafe fn read_node_tag(node: *const SetNode) -> SetNodeTag {
    unsafe {
        let tag_byte = *(node as *const u8);
        match tag_byte {
            1 => SetNodeTag::Internal,
            2 => SetNodeTag::Leaf,
            _ => panic!("Invalid SetNodeTag: {}", tag_byte),
        }
    }
}

/// Reads the length from a set node.
unsafe fn read_node_len(node: *const SetNode) -> u32 {
    unsafe { (*node).len }
}

/// Gets pointer to keys array in an internal node.
unsafe fn internal_keys_ptr(node: *mut SetNode, key_tydesc: *const TyDesc) -> *mut u8 {
    unsafe {
        let layout = rtdt::layout::compute_set_internal_node_layout(key_tydesc);
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Gets pointer to child pointers array in an internal node.
unsafe fn internal_child_ptrs_ptr(node: *mut SetNode, key_tydesc: *const TyDesc) -> *mut *mut SetNode {
    unsafe {
        let layout = rtdt::layout::compute_set_internal_node_layout(key_tydesc);
        (node as *mut u8).add(layout.child_ptrs_offset as usize) as *mut *mut SetNode
    }
}

/// Gets pointer to keys array in a leaf node.
unsafe fn leaf_keys_ptr(node: *mut SetNode, key_tydesc: *const TyDesc) -> *mut u8 {
    unsafe {
        let layout = rtdt::layout::compute_set_leaf_node_layout(key_tydesc);
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Frees a set node.
unsafe fn free_node(rt: &mut RtLocal, node: *mut SetNode, key_tydesc: *const TyDesc) {
    unsafe {
        let tag = read_node_tag(node);
        let layout = match tag {
            SetNodeTag::Internal => {
                let l = rtdt::layout::compute_set_internal_node_layout(key_tydesc);
                (l.size, l.align)
            }
            SetNodeTag::Leaf => {
                let l = rtdt::layout::compute_set_leaf_node_layout(key_tydesc);
                (l.size, l.align)
            }
        };

        rt.alloc.free(layout.0, layout.1, 1, node as *mut u8);
    }
}

/// Recursively destroys a set subtree.
unsafe fn destroy_tree_recursive(
    rt: &mut RtLocal,
    node: *mut SetNode,
    key_tydesc: *const TyDesc,
) {
    unsafe {
        if node.is_null() {
            return;
        }

        let tag = read_node_tag(node);

        match tag {
            SetNodeTag::Internal => {
                let len = read_node_len(node);

                // Destroy all keys in the internal node.
                let keys_ptr = internal_keys_ptr(node, key_tydesc);
                let key_size = (*key_tydesc).size as usize;
                let rt_handle = rt as *mut RtLocal as crate::LocalRtHandle;
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let _ = crate::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                }

                // Recursively destroy children.
                let children_ptr = internal_child_ptrs_ptr(node, key_tydesc);
                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    destroy_tree_recursive(rt, child, key_tydesc);
                }
            }
            SetNodeTag::Leaf => {
                let len = read_node_len(node);
                let keys_ptr = leaf_keys_ptr(node, key_tydesc);
                let key_size = (*key_tydesc).size as usize;
                let rt_handle = rt as *mut RtLocal as crate::LocalRtHandle;

                // Destroy all keys in the leaf node.
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let _ = crate::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                }
            }
        }

        // Free the node itself.
        free_node(rt, node, key_tydesc);
    }
}

/// Recursively clone a set subtree.
unsafe fn clone_tree_recursive(
    rt: &mut RtLocal,
    node: *const SetNode,
    key_tydesc: *const TyDesc,
) -> *mut SetNode {
    unsafe {
        if node.is_null() {
            return std::ptr::null_mut();
        }

        let tag = read_node_tag(node);
        let rt_handle = rt as *mut RtLocal as crate::LocalRtHandle;

        match tag {
            SetNodeTag::Internal => {
                // Allocate a new internal node.
                let layout = rtdt::layout::compute_set_internal_node_layout(key_tydesc);
                let new_node_ptr = rt.alloc.alloc(layout.size, layout.align, 1);
                if new_node_ptr.is_null() {
                    return std::ptr::null_mut();
                }
                let new_node = new_node_ptr as *mut SetNode;

                let len = read_node_len(node);
                (*new_node).tag = SetNodeTag::Internal;
                (*new_node).len = len;

                let keys_ptr = internal_keys_ptr(node as *mut SetNode, key_tydesc);
                let new_keys_ptr = internal_keys_ptr(new_node, key_tydesc);
                let key_size = (*key_tydesc).size as usize;

                // Clone all keys.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::clone::clone_value(rt_handle, key_src, key_tydesc, key_dst);
                    if status != RtStatus::Ok {
                        free_node(rt, new_node, key_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                // Recursively clone all children.
                let children_ptr = internal_child_ptrs_ptr(node as *mut SetNode, key_tydesc);
                let new_children_ptr = internal_child_ptrs_ptr(new_node, key_tydesc);

                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    let new_child = clone_tree_recursive(rt, child, key_tydesc);
                    if new_child.is_null() && !child.is_null() {
                        free_node(rt, new_node, key_tydesc);
                        return std::ptr::null_mut();
                    }
                    *new_children_ptr.add(i) = new_child;
                }

                new_node
            }
            SetNodeTag::Leaf => {
                // Allocate a new leaf node.
                let layout = rtdt::layout::compute_set_leaf_node_layout(key_tydesc);
                let new_leaf_ptr = rt.alloc.alloc(layout.size, layout.align, 1);
                if new_leaf_ptr.is_null() {
                    return std::ptr::null_mut();
                }
                let new_leaf = new_leaf_ptr as *mut SetNode;

                let len = read_node_len(node);
                (*new_leaf).tag = SetNodeTag::Leaf;
                (*new_leaf).len = len;

                let keys_ptr = leaf_keys_ptr(node as *mut SetNode, key_tydesc);
                let new_keys_ptr = leaf_keys_ptr(new_leaf, key_tydesc);
                let key_size = (*key_tydesc).size as usize;

                // Clone all keys.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::clone::clone_value(rt_handle, key_src, key_tydesc, key_dst);
                    if status != RtStatus::Ok {
                        free_node(rt, new_leaf, key_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                // Note: We don't clone next_leaf pointers here.
                // The cloned tree will have its own leaf chain that needs to be rebuilt.

                new_leaf
            }
        }
    }
}

/// Clone a set tree.
pub unsafe fn set_clone_tree(
    rt: &mut RtLocal,
    root: *const SetNode,
    key_tydesc: *const TyDesc,
) -> *mut SetNode {
    unsafe {
        clone_tree_recursive(rt, root, key_tydesc)
    }
}

/// Destroys a set, freeing all allocations.
pub unsafe fn set_destroy_impl(
    rt: &mut RtLocal,
    value_in: *mut u8,
    tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if value_in.is_null() || tydesc.is_null() {
            return RtStatus::Error;
        }

        let set_info = (*tydesc).type_info.set;
        let element_tydesc = set_info.element_tydesc;

        let set_ptr = value_in as *mut Set;
        let root = (*set_ptr).root as *mut SetNode;

        if !root.is_null() {
            destroy_tree_recursive(rt, root, element_tydesc);
        }

        // Clear the set struct.
        (*set_ptr).root = std::ptr::null_mut();
        (*set_ptr).len = 0;

        RtStatus::Ok
    }
}
