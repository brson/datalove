//! Set operations for the Datalove runtime.

use datalove_rtdt as rtdt;
use datalove_rtdt::{TyDesc, Set, SetNode, SetNodeTag, SET_NODE_CAPACITY};
use crate::impls::rt_local::RtLocal;
use crate::c::RtStatus;
use crate::rust::AlignedBuffer;

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

/// Writes the tag to a set node.
unsafe fn write_node_tag(node: *mut SetNode, tag: SetNodeTag) {
    unsafe {
        let tag_ptr = node as *mut u8;
        *tag_ptr = tag as u8;
    }
}

/// Reads the length from a set node.
unsafe fn read_node_len(node: *const SetNode) -> u32 {
    unsafe { (*node).len }
}

/// Writes the length to a set node.
unsafe fn write_node_len(node: *mut SetNode, len: u32) {
    unsafe {
        (*node).len = len;
    }
}

/// Gets pointer to keys array in an internal node.
unsafe fn internal_keys_ptr(node: *mut SetNode, key_tydesc: *const TyDesc) -> *mut u8 {
    unsafe {
        let tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let layout = rtdt::layout::compute_set_internal_node_layout(tydesc_ref);
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Gets pointer to child pointers array in an internal node.
unsafe fn internal_child_ptrs_ptr(node: *mut SetNode, key_tydesc: *const TyDesc) -> *mut *mut SetNode {
    unsafe {
        let layout = rtdt::layout::compute_set_internal_node_layout(rtdt::TyDescRef::from_ptr(key_tydesc));
        (node as *mut u8).add(layout.child_ptrs_offset as usize) as *mut *mut SetNode
    }
}

/// Gets pointer to keys array in a leaf node.
unsafe fn leaf_keys_ptr(node: *mut SetNode, key_tydesc: *const TyDesc) -> *mut u8 {
    unsafe {
        let layout = rtdt::layout::compute_set_leaf_node_layout(rtdt::TyDescRef::from_ptr(key_tydesc));
        (node as *mut u8).add(layout.keys_offset as usize)
    }
}

/// Allocate and initialize a new internal node.
unsafe fn alloc_internal_node(
    rt: &mut RtLocal,
    element_tydesc: *const TyDesc,
) -> *mut SetNode {
    unsafe {
        let layout = rtdt::layout::compute_set_internal_node_layout(rtdt::TyDescRef::from_ptr(element_tydesc));

        let ptr = rt.alloc.alloc(layout.size, layout.align, 1);
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let node = ptr as *mut SetNode;
        write_node_tag(node, SetNodeTag::Internal);
        write_node_len(node, 0);

        node
    }
}

/// Allocate and initialize a new leaf node.
unsafe fn alloc_leaf_node(
    rt: &mut RtLocal,
    element_tydesc: *const TyDesc,
) -> *mut SetNode {
    unsafe {
        let layout = rtdt::layout::compute_set_leaf_node_layout(rtdt::TyDescRef::from_ptr(element_tydesc));

        let ptr = rt.alloc.alloc(layout.size, layout.align, 1);
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let node = ptr as *mut SetNode;
        write_node_tag(node, SetNodeTag::Leaf);
        write_node_len(node, 0);

        node
    }
}

/// Frees a set node.
unsafe fn free_node(rt: &mut RtLocal, node: *mut SetNode, key_tydesc: *const TyDesc) {
    unsafe {
        let tag = read_node_tag(node);
        let layout = match tag {
            SetNodeTag::Internal => {
                let l = rtdt::layout::compute_set_internal_node_layout(rtdt::TyDescRef::from_ptr(key_tydesc));
                (l.size, l.align)
            }
            SetNodeTag::Leaf => {
                let l = rtdt::layout::compute_set_leaf_node_layout(rtdt::TyDescRef::from_ptr(key_tydesc));
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

        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let tag = read_node_tag(node);

        match tag {
            SetNodeTag::Internal => {
                let len = read_node_len(node);

                // Destroy all keys in the internal node.
                let keys_ptr = internal_keys_ptr(node, key_tydesc);
                let key_size = key_tydesc_ref.size() as usize;
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
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
                let key_size = key_tydesc_ref.size() as usize;
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

                // Destroy all keys in the leaf node.
                for i in 0..len as usize {
                    let key_slot = keys_ptr.add(i * key_size);
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                }
            }
        }

        // Free the node itself.
        free_node(rt, node, key_tydesc);
    }
}

/// Recursively clone a set subtree, collecting leaf nodes in order.
unsafe fn clone_tree_recursive(
    rt: &mut RtLocal,
    node: *const SetNode,
    key_tydesc: *const TyDesc,
    leaves: &mut Vec<*mut SetNode>,
) -> *mut SetNode {
    unsafe {
        if node.is_null() {
            return std::ptr::null_mut();
        }

        let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
        let tag = read_node_tag(node);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        match tag {
            SetNodeTag::Internal => {
                // Allocate a new internal node.
                let layout = rtdt::layout::compute_set_internal_node_layout(key_tydesc_ref);
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
                let key_size = key_tydesc_ref.size() as usize;

                // Clone all keys.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::impls::clone::clone_value(rt_handle, key_src, key_tydesc, key_dst);
                    if status != RtStatus::Ok {
                        // Destroy already-cloned keys.
                        for j in 0..i {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                        }
                        free_node(rt, new_node, key_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                // Recursively clone all children.
                let children_ptr = internal_child_ptrs_ptr(node as *mut SetNode, key_tydesc);
                let new_children_ptr = internal_child_ptrs_ptr(new_node, key_tydesc);

                // Initialize all child pointers to null for safe cleanup.
                for i in 0..=(len as usize) {
                    *new_children_ptr.add(i) = std::ptr::null_mut();
                }

                for i in 0..=(len as usize) {
                    let child = *children_ptr.add(i);
                    let new_child = clone_tree_recursive(rt, child, key_tydesc, leaves);
                    // Store the cloned child pointer immediately so error handling can access it.
                    *new_children_ptr.add(i) = new_child;
                    if new_child.is_null() && !child.is_null() {
                        // Destroy all keys.
                        for j in 0..len as usize {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                        }
                        // Destroy already-cloned children (including current one which is null).
                        for j in 0..=i {
                            let cloned_child = *new_children_ptr.add(j);
                            destroy_tree_recursive(rt, cloned_child, key_tydesc);
                        }
                        free_node(rt, new_node, key_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                new_node
            }
            SetNodeTag::Leaf => {
                // Allocate a new leaf node.
                let layout = rtdt::layout::compute_set_leaf_node_layout(key_tydesc_ref);
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
                let key_size = key_tydesc_ref.size() as usize;

                // Clone all keys.
                for i in 0..len as usize {
                    let key_src = keys_ptr.add(i * key_size);
                    let key_dst = new_keys_ptr.add(i * key_size);
                    let status = crate::impls::clone::clone_value(rt_handle, key_src, key_tydesc, key_dst);
                    if status != RtStatus::Ok {
                        // Destroy already-cloned keys.
                        for j in 0..i {
                            let key_slot = new_keys_ptr.add(j * key_size);
                            let _ = crate::impls::destroy::any_destroy_local(rt_handle, key_slot, key_tydesc);
                        }
                        free_node(rt, new_leaf, key_tydesc);
                        return std::ptr::null_mut();
                    }
                }

                // Initialize next_leaf to null; will be linked after tree is cloned.
                let next_leaf_ptr = (new_leaf as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut SetNode;
                *next_leaf_ptr = std::ptr::null_mut();

                // Collect this leaf in order (left-to-right traversal).
                leaves.push(new_leaf);

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
        let mut leaves = Vec::new();
        let new_root = clone_tree_recursive(rt, root, key_tydesc, &mut leaves);

        // Link leaf nodes via next_leaf pointers.
        if !leaves.is_empty() {
            let key_tydesc_ref = rtdt::TyDescRef::from_ptr(key_tydesc);
            let layout = rtdt::layout::compute_set_leaf_node_layout(key_tydesc_ref);

            for i in 0..leaves.len() - 1 {
                let next_leaf_ptr = (leaves[i] as *mut u8).add(layout.next_leaf_offset as usize) as *mut *mut SetNode;
                *next_leaf_ptr = leaves[i + 1];
            }
        }

        new_root
    }
}

/// Build a Set B-tree from a sorted slice of already-instantiated elements.
///
/// Takes ownership of the elements by moving them from the input buffer into
/// the tree structure. The input buffer should not be used after this call.
/// Elements must already be sorted.
pub unsafe fn btreeset_build_from_sorted_slice(
    rt: &mut RtLocal,
    set_out: *mut Set,
    element_tydesc: *const TyDesc,
    elements_ptr: *mut u8,
    num_elements: rtdt::UsizeRepr,
) -> RtStatus {
    unsafe {
        if set_out.is_null() || element_tydesc.is_null() {
            return RtStatus::Error;
        }

        // Handle empty case.
        if num_elements == 0 || elements_ptr.is_null() {
            (*set_out).root = std::ptr::null();
            (*set_out).len = rtdt::Usize::ZERO;
            return RtStatus::Ok;
        }

        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let element_size = element_tydesc_ref.size() as usize;
        let leaf_layout = rtdt::layout::compute_set_leaf_node_layout(element_tydesc_ref);

        // Step 1: Build all leaf nodes.
        let num_elements_usize = num_elements as usize;
        let num_leaves = (num_elements_usize + SET_NODE_CAPACITY as usize - 1) / SET_NODE_CAPACITY as usize;
        let mut leaves: Vec<*mut SetNode> = Vec::with_capacity(num_leaves);

        let elements_per_leaf = (num_elements_usize + num_leaves - 1) / num_leaves;
        let mut element_idx = 0usize;

        for _ in 0..num_leaves {
            let leaf_elements = elements_per_leaf.min(num_elements_usize - element_idx);

            // Allocate leaf node.
            let leaf_node = rt.alloc.alloc(leaf_layout.size, leaf_layout.align, 1);
            if leaf_node.is_null() {
                // Cleanup already-created leaves.
                cleanup_set_leaves_internal(rt, &leaves, element_tydesc, &leaf_layout);
                return RtStatus::Error;
            }

            // Initialize node header.
            *leaf_node = SetNodeTag::Leaf as u8;
            *(leaf_node.add(4) as *mut u32) = leaf_elements as u32;

            // Initialize next_leaf pointer to null (will be linked later).
            let next_leaf_ptr = leaf_node.add(leaf_layout.next_leaf_offset as usize) as *mut *mut SetNode;
            *next_leaf_ptr = std::ptr::null_mut();

            // Move elements from input buffer to leaf.
            let keys_array = leaf_node.add(leaf_layout.keys_offset as usize);
            for i in 0..leaf_elements {
                let src = elements_ptr.add((element_idx + i) * element_size);
                let dst = keys_array.add(i * element_size);
                std::ptr::copy_nonoverlapping(src, dst, element_size);
            }

            leaves.push(leaf_node as *mut SetNode);
            element_idx += leaf_elements;
        }

        // Link leaf nodes via next_leaf pointers.
        for i in 0..leaves.len() - 1 {
            let next_leaf_ptr = (leaves[i] as *mut u8).add(leaf_layout.next_leaf_offset as usize) as *mut *mut SetNode;
            *next_leaf_ptr = leaves[i + 1];
        }

        // If only one leaf, it's the root.
        if leaves.len() == 1 {
            (*set_out).root = leaves[0] as *const SetNode;
            (*set_out).len = rtdt::Usize(num_elements);
            return RtStatus::Ok;
        }

        // Step 2: Build internal levels bottom-up.
        let internal_layout = rtdt::layout::compute_set_internal_node_layout(element_tydesc_ref);
        let mut current_level = leaves;

        loop {
            let num_nodes = current_level.len();
            if num_nodes == 1 {
                (*set_out).root = current_level[0] as *const SetNode;
                (*set_out).len = rtdt::Usize(num_elements);
                return RtStatus::Ok;
            }

            // Build next level of internal nodes.
            let capacity_plus_one = (SET_NODE_CAPACITY + 1) as usize;
            let num_parents = (num_nodes + capacity_plus_one - 1) / capacity_plus_one;
            let mut parents: Vec<*mut SetNode> = Vec::with_capacity(num_parents);

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
                *internal_node = SetNodeTag::Internal as u8;
                *(internal_node.add(4) as *mut u32) = (num_children - 1) as u32;

                // Get pointers to keys and child_ptrs arrays.
                let keys_array = internal_node.add(internal_layout.keys_offset as usize);
                let child_ptrs_array = internal_node.add(internal_layout.child_ptrs_offset as usize) as *mut *mut SetNode;

                // Set child pointers.
                for i in 0..num_children {
                    *child_ptrs_array.add(i) = current_level[child_idx + i];
                }

                // Extract separator keys (first key from each child except the first).
                let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                for i in 1..num_children {
                    let child_node = current_level[child_idx + i];
                    let child_is_leaf = *(child_node as *const u8) == SetNodeTag::Leaf as u8;

                    let first_key_src = if child_is_leaf {
                        (child_node as *const u8).add(leaf_layout.keys_offset as usize)
                    } else {
                        (child_node as *const u8).add(internal_layout.keys_offset as usize)
                    };

                    let key_dest = keys_array.add((i - 1) * element_size);
                    // Clone the key for the internal node.
                    let status = crate::impls::clone::clone_value(
                        rt_handle,
                        first_key_src,
                        element_tydesc,
                        key_dest,
                    );
                    if status != RtStatus::Ok {
                        // TODO: proper cleanup of partial tree
                        return RtStatus::Error;
                    }
                }

                parents.push(internal_node as *mut SetNode);
                child_idx += num_children;
            }

            current_level = parents;
        }
    }
}

/// Helper to clean up partially constructed set leaves.
unsafe fn cleanup_set_leaves_internal(
    rt: &mut RtLocal,
    leaves: &[*mut SetNode],
    element_tydesc: *const TyDesc,
    leaf_layout: &rtdt::SetNodeLeafLayout,
) {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let element_size = element_tydesc_ref.size() as usize;
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        for &leaf_node in leaves {
            let len = (*(leaf_node as *const SetNode)).len as usize;
            let keys_array = (leaf_node as *mut u8).add(leaf_layout.keys_offset as usize);

            // Destroy all elements in this leaf.
            for i in 0..len {
                let elem_to_destroy = keys_array.add(i * element_size);
                crate::impls::destroy::any_destroy_local(rt_handle, elem_to_destroy, element_tydesc);
            }

            // Free the leaf node.
            rt.alloc.free(leaf_layout.size, leaf_layout.align, 1, leaf_node as *mut u8);
        }
    }
}

/// Creates an empty BTreeSet.
pub unsafe fn btreeset_create_impl(
    _rt: &mut RtLocal,
    value_out: *mut u8,
    _tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if value_out.is_null() {
            return RtStatus::Error;
        }

        // Create an empty set (null root, zero length).
        let set_ptr = value_out as *mut Set;
        (*set_ptr).root = std::ptr::null_mut();
        (*set_ptr).len = rtdt::Usize::ZERO;

        RtStatus::Ok
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

        let ty = rtdt::TyDescRef::from_ptr(tydesc);
        let element_ty = ty.set_element_ty();
        let element_tydesc = element_ty.as_ptr();

        let set_ptr = value_in as *mut Set;
        let root = (*set_ptr).root as *mut SetNode;

        if !root.is_null() {
            destroy_tree_recursive(rt, root, element_tydesc);
        }

        // Clear the set struct.
        (*set_ptr).root = std::ptr::null_mut();
        (*set_ptr).len = rtdt::Usize::ZERO;

        RtStatus::Ok
    }
}

/// Clears all elements from a set, leaving it empty.
pub unsafe fn btreeset_clear_impl(
    rt: &mut RtLocal,
    value_mut: *mut u8,
    tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        // Destroy the existing tree.
        let status = set_destroy_impl(rt, value_mut, tydesc);
        if status != RtStatus::Ok {
            return status;
        }

        // Reinitialize as empty.
        btreeset_create_impl(rt, value_mut, tydesc)
    }
}

/// Find the leaf node where an element should be.
unsafe fn find_leaf_for_element(
    mut node: *mut SetNode,
    element: *const u8,
    element_tydesc: *const TyDesc,
) -> *mut SetNode {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        loop {
            let tag = read_node_tag(node);
            match tag {
                SetNodeTag::Leaf => return node,
                SetNodeTag::Internal => {
                    let len = read_node_len(node);
                    let keys_ptr = internal_keys_ptr(node, element_tydesc);
                    let children_ptr = internal_child_ptrs_ptr(node, element_tydesc);
                    let element_size = element_tydesc_ref.size() as usize;

                    // Find the child to descend into.
                    let mut child_idx = 0;
                    for i in 0..len as usize {
                        let node_key = keys_ptr.add(i * element_size);
                        let cmp_result = super::cmp::cmp_total(
                            element,
                            element_tydesc,
                            node_key,
                            element_tydesc,
                        );
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

                    node = *children_ptr.add(child_idx);
                }
            }
        }
    }
}

/// Result of attempting to insert into a leaf.
#[derive(Debug, PartialEq, Eq)]
enum LeafInsertResult {
    /// Element was newly inserted.
    Inserted,
    /// Element already existed (no-op for sets).
    AlreadyExists,
    /// Leaf is full, needs to be split.
    NeedsSplit,
}

/// Information about a node split.
struct SplitInfo {
    /// The separator key to insert into parent.
    separator_key_buf: AlignedBuffer,
    /// The new right sibling node created by the split.
    new_node: *mut SetNode,
    /// The result of inserting the pending element during the split.
    insert_result: LeafInsertResult,
}

impl SplitInfo {
    /// Destroy the separator key and clean up.
    unsafe fn destroy(mut self, rt: &mut RtLocal, element_tydesc: *const TyDesc) {
        unsafe {
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let _ = crate::impls::destroy::any_destroy_local(
                rt_handle,
                self.separator_key_buf.as_mut_ptr(),
                element_tydesc,
            );
        }
    }
}

/// Try to insert an element in a leaf node.
unsafe fn leaf_insert_element(
    rt: &mut RtLocal,
    leaf: *mut SetNode,
    element: *const u8,
    element_tydesc: *const TyDesc,
) -> LeafInsertResult {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let len = read_node_len(leaf);
        let capacity = SET_NODE_CAPACITY;

        if len >= capacity {
            return LeafInsertResult::NeedsSplit;
        }

        let keys_ptr = leaf_keys_ptr(leaf, element_tydesc);
        let element_size = element_tydesc_ref.size() as usize;

        // Find the insertion position.
        let mut insert_pos = len as usize;
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * element_size);
            let cmp_result = super::cmp::cmp_total(
                element,
                element_tydesc,
                node_key,
                element_tydesc,
            );
            match cmp_result {
                crate::c::RtOrdering::Less => {
                    insert_pos = i;
                    break;
                }
                crate::c::RtOrdering::Equal => {
                    // Element already exists, destroy the input and return.
                    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, element as *mut u8, element_tydesc);
                    return LeafInsertResult::AlreadyExists;
                }
                crate::c::RtOrdering::Greater => continue,
                crate::c::RtOrdering::Error => {
                    insert_pos = i;
                    break;
                }
            }
        }

        // Shift elements to make room.
        if insert_pos < len as usize {
            let shift_count = len as usize - insert_pos;
            let src_key = keys_ptr.add(insert_pos * element_size);
            let dst_key = keys_ptr.add((insert_pos + 1) * element_size);
            std::ptr::copy(src_key, dst_key, shift_count * element_size);
        }

        // Move the new element into the node (by-move semantics).
        let key_slot = keys_ptr.add(insert_pos * element_size);
        std::ptr::copy_nonoverlapping(element, key_slot, element_size);

        write_node_len(leaf, len + 1);
        LeafInsertResult::Inserted
    }
}

/// Split a full leaf node and insert the pending element.
unsafe fn split_leaf(
    rt: &mut RtLocal,
    leaf: *mut SetNode,
    element: *const u8,
    element_tydesc: *const TyDesc,
) -> core::result::Result<SplitInfo, RtStatus> {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        // Allocate a new sibling leaf.
        let new_leaf = alloc_leaf_node(rt, element_tydesc);
        if new_leaf.is_null() {
            return Err(RtStatus::Error);
        }

        let capacity = SET_NODE_CAPACITY;
        let split_point = (capacity / 2) as usize;

        let keys_ptr = leaf_keys_ptr(leaf, element_tydesc);
        let new_keys_ptr = leaf_keys_ptr(new_leaf, element_tydesc);
        let element_size = element_tydesc_ref.size() as usize;

        // Move upper half to new leaf.
        let move_count = capacity as usize - split_point;
        std::ptr::copy_nonoverlapping(
            keys_ptr.add(split_point * element_size),
            new_keys_ptr,
            move_count * element_size,
        );

        write_node_len(leaf, split_point as u32);
        write_node_len(new_leaf, move_count as u32);

        // Clone the separator key (first key of new_leaf).
        let mut separator_key_buf = AlignedBuffer::with_align(element_size, element_tydesc_ref.align() as usize);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            new_keys_ptr,
            element_tydesc,
            separator_key_buf.as_mut_ptr(),
        );
        if status != RtStatus::Ok {
            return Err(status);
        }

        // Determine which leaf should receive the new element.
        let cmp_result = super::cmp::cmp_total(element, element_tydesc, separator_key_buf.as_ptr(), element_tydesc);
        let insert_result = match cmp_result {
            crate::c::RtOrdering::Less => {
                leaf_insert_element(rt, leaf, element, element_tydesc)
            }
            crate::c::RtOrdering::Equal | crate::c::RtOrdering::Greater => {
                leaf_insert_element(rt, new_leaf, element, element_tydesc)
            }
            crate::c::RtOrdering::Error => {
                return Err(RtStatus::Error);
            }
        };

        if insert_result == LeafInsertResult::NeedsSplit {
            return Err(RtStatus::Error);
        }

        Ok(SplitInfo {
            separator_key_buf,
            new_node: new_leaf,
            insert_result,
        })
    }
}

/// Find the leaf node where an element should be inserted, tracking the path.
unsafe fn find_leaf_with_path(
    mut node: *mut SetNode,
    element: *const u8,
    element_tydesc: *const TyDesc,
    path: &mut Vec<*mut SetNode>,
) -> *mut SetNode {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        loop {
            let tag = read_node_tag(node);
            match tag {
                SetNodeTag::Leaf => return node,
                SetNodeTag::Internal => {
                    path.push(node);

                    let len = read_node_len(node);
                    let keys_ptr = internal_keys_ptr(node, element_tydesc);
                    let children_ptr = internal_child_ptrs_ptr(node, element_tydesc);
                    let element_size = element_tydesc_ref.size() as usize;

                    let mut child_idx = 0;
                    for i in 0..len as usize {
                        let node_key = keys_ptr.add(i * element_size);
                        let cmp_result = super::cmp::cmp_total(
                            element,
                            element_tydesc,
                            node_key,
                            element_tydesc,
                        );
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

                    node = *children_ptr.add(child_idx);
                }
            }
        }
    }
}

/// Insert a separator key and child pointer into an internal node.
unsafe fn insert_into_internal(
    rt: &mut RtLocal,
    node: *mut SetNode,
    separator_key: &[u8],
    right_child: *mut SetNode,
    element_tydesc: *const TyDesc,
) -> core::result::Result<(), SplitInfo> {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let len = read_node_len(node);
        let capacity = SET_NODE_CAPACITY;

        if len >= capacity {
            return Err(split_internal_node(rt, node, separator_key, right_child, element_tydesc));
        }

        let keys_ptr = internal_keys_ptr(node, element_tydesc);
        let children_ptr = internal_child_ptrs_ptr(node, element_tydesc);
        let element_size = element_tydesc_ref.size() as usize;

        // Find insertion position.
        let mut insert_pos = len as usize;
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * element_size);
            let cmp_result = super::cmp::cmp_total(
                separator_key.as_ptr(),
                element_tydesc,
                node_key,
                element_tydesc,
            );
            match cmp_result {
                crate::c::RtOrdering::Less => {
                    insert_pos = i;
                    break;
                }
                crate::c::RtOrdering::Equal => {
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
            let src_key = keys_ptr.add(insert_pos * element_size);
            let dst_key = keys_ptr.add((insert_pos + 1) * element_size);
            std::ptr::copy(src_key, dst_key, shift_count * element_size);

            let src_child = children_ptr.add(insert_pos + 1);
            let dst_child = children_ptr.add(insert_pos + 2);
            std::ptr::copy(src_child, dst_child, shift_count);
        }

        // Clone the separator key into the node.
        let key_slot = keys_ptr.add(insert_pos * element_size);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            separator_key.as_ptr(),
            element_tydesc,
            key_slot,
        );
        if status != RtStatus::Ok {
            return Err(SplitInfo {
                separator_key_buf: AlignedBuffer::new(0),
                new_node: std::ptr::null_mut(),
                insert_result: LeafInsertResult::NeedsSplit,
            });
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
    node: *mut SetNode,
    pending_key: &[u8],
    pending_child: *mut SetNode,
    element_tydesc: *const TyDesc,
) -> SplitInfo {
    unsafe {
        let element_tydesc_ref = rtdt::TyDescRef::from_ptr(element_tydesc);
        let new_internal = alloc_internal_node(rt, element_tydesc);
        if new_internal.is_null() {
            return SplitInfo {
                separator_key_buf: AlignedBuffer::new(0),
                new_node: std::ptr::null_mut(),
                insert_result: LeafInsertResult::NeedsSplit,
            };
        }

        let capacity = SET_NODE_CAPACITY;
        let split_point = (capacity / 2) as usize;

        let keys_ptr = internal_keys_ptr(node, element_tydesc);
        let children_ptr = internal_child_ptrs_ptr(node, element_tydesc);
        let new_keys_ptr = internal_keys_ptr(new_internal, element_tydesc);
        let new_children_ptr = internal_child_ptrs_ptr(new_internal, element_tydesc);
        let element_size = element_tydesc_ref.size() as usize;

        // Clone the middle key as the separator to push up.
        let mut separator_key_buf = AlignedBuffer::with_align(element_size, element_tydesc_ref.align() as usize);
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            keys_ptr.add(split_point * element_size),
            element_tydesc,
            separator_key_buf.as_mut_ptr(),
        );
        if status != RtStatus::Ok {
            return SplitInfo {
                separator_key_buf: AlignedBuffer::new(0),
                new_node: std::ptr::null_mut(),
                insert_result: LeafInsertResult::NeedsSplit,
            };
        }

        // Destroy the separator key in the old node since we've cloned it out.
        let _ = crate::impls::destroy::any_destroy_local(
            rt_handle,
            keys_ptr.add(split_point * element_size),
            element_tydesc,
        );

        // Move keys after split_point (excluding the separator) to new node.
        let keys_to_move = capacity as usize - split_point - 1;
        if keys_to_move > 0 {
            std::ptr::copy_nonoverlapping(
                keys_ptr.add((split_point + 1) * element_size),
                new_keys_ptr,
                keys_to_move * element_size,
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
        write_node_len(new_internal, keys_to_move as u32);

        // Now insert the pending key/child into the appropriate node.
        let cmp_result = super::cmp::cmp_total(
            pending_key.as_ptr(),
            element_tydesc,
            separator_key_buf.as_ptr(),
            element_tydesc,
        );

        let insert_result = match cmp_result {
            crate::c::RtOrdering::Less => {
                insert_into_internal(rt, node, pending_key, pending_child, element_tydesc)
            }
            _ => {
                insert_into_internal(rt, new_internal, pending_key, pending_child, element_tydesc)
            }
        };

        // After split, there should be room - if not, something is very wrong.
        assert!(insert_result.is_ok(), "Split didn't make room for insertion");

        SplitInfo {
            separator_key_buf,
            new_node: new_internal,
            insert_result: LeafInsertResult::Inserted,
        }
    }
}

/// Propagate a split up the tree.
unsafe fn propagate_split_up(
    rt: &mut RtLocal,
    root_ptr: &mut *const SetNode,
    mut _child: *mut SetNode,
    mut split_info: SplitInfo,
    path: &[*mut SetNode],
    element_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        // If there's no parent, child must be the root.
        if path.is_empty() {
            // Create new internal root.
            let new_root = alloc_internal_node(rt, element_tydesc);
            if new_root.is_null() {
                split_info.destroy(rt, element_tydesc);
                return RtStatus::Error;
            }

            let root_keys_ptr = internal_keys_ptr(new_root, element_tydesc);
            let root_children_ptr = internal_child_ptrs_ptr(new_root, element_tydesc);

            // Clone separator into new root.
            let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
            let status = crate::impls::clone::clone_value(
                rt_handle,
                split_info.separator_key_buf.as_ptr(),
                element_tydesc,
                root_keys_ptr,
            );
            if status != RtStatus::Ok {
                split_info.destroy(rt, element_tydesc);
                return status;
            }

            // Set children: old root and new sibling.
            *root_children_ptr = *root_ptr as *mut SetNode;
            *root_children_ptr.add(1) = split_info.new_node;
            write_node_len(new_root, 1);

            *root_ptr = new_root;
            split_info.destroy(rt, element_tydesc);
            return RtStatus::Ok;
        }

        // Walk up the tree, inserting split info into parents.
        for i in (0..path.len()).rev() {
            let parent = path[i];
            let result = insert_into_internal(
                rt,
                parent,
                split_info.separator_key_buf.as_slice(),
                split_info.new_node,
                element_tydesc,
            );

            match result {
                Ok(()) => {
                    split_info.destroy(rt, element_tydesc);
                    return RtStatus::Ok;
                }
                Err(new_split_info) => {
                    split_info.destroy(rt, element_tydesc);
                    split_info = new_split_info;

                    if i == 0 {
                        // Parent is root and split, create new root.
                        let new_root = alloc_internal_node(rt, element_tydesc);
                        if new_root.is_null() {
                            split_info.destroy(rt, element_tydesc);
                            return RtStatus::Error;
                        }

                        let root_keys_ptr = internal_keys_ptr(new_root, element_tydesc);
                        let root_children_ptr = internal_child_ptrs_ptr(new_root, element_tydesc);

                        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                        let status = crate::impls::clone::clone_value(
                            rt_handle,
                            split_info.separator_key_buf.as_ptr(),
                            element_tydesc,
                            root_keys_ptr,
                        );
                        if status != RtStatus::Ok {
                            split_info.destroy(rt, element_tydesc);
                            return status;
                        }

                        *root_children_ptr = *root_ptr as *mut SetNode;
                        *root_children_ptr.add(1) = split_info.new_node;
                        write_node_len(new_root, 1);

                        *root_ptr = new_root;
                        split_info.destroy(rt, element_tydesc);
                        return RtStatus::Ok;
                    }
                }
            }
        }

        split_info.destroy(rt, element_tydesc);
        RtStatus::Ok
    }
}

/// Inserts an element into a set.
pub unsafe fn btreeset_insert_impl(
    rt: &mut RtLocal,
    btreeset_value_ref: *mut u8,
    btreeset_tydesc: *const TyDesc,
    element_ptr: *mut u8,
    _element_tydesc: *const TyDesc,
    bool_out: *mut u8,
) -> RtStatus {
    unsafe {
        if btreeset_value_ref.is_null() || element_ptr.is_null() || bool_out.is_null() {
            return RtStatus::Error;
        }

        let ty = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        let set_element_ty = ty.set_element_ty();
        let set_element_tydesc = set_element_ty.as_ptr();
        let set_element_tydesc_ref = rtdt::TyDescRef::from_ptr(set_element_tydesc);

        let set_ptr = btreeset_value_ref as *mut Set;
        let root = (*set_ptr).root as *mut SetNode;

        // If set is empty, create the first leaf.
        if root.is_null() {
            let leaf = alloc_leaf_node(rt, set_element_tydesc);
            if leaf.is_null() {
                return RtStatus::Error;
            }

            let keys_ptr = leaf_keys_ptr(leaf, set_element_tydesc);
            let element_size = set_element_tydesc_ref.size() as usize;
            std::ptr::copy_nonoverlapping(element_ptr, keys_ptr, element_size);

            write_node_len(leaf, 1);
            (*set_ptr).root = leaf as *const SetNode;
            (*set_ptr).len = rtdt::Usize::ONE;

            *bool_out = 1;
            return RtStatus::Ok;
        }

        // Find the leaf where the element should be inserted.
        let mut path: Vec<*mut SetNode> = Vec::new();
        let leaf = find_leaf_with_path(root, element_ptr, set_element_tydesc, &mut path);

        // Try to insert into the leaf.
        let result = leaf_insert_element(rt, leaf, element_ptr, set_element_tydesc);

        match result {
            LeafInsertResult::AlreadyExists => {
                *bool_out = 0;
                RtStatus::Ok
            }
            LeafInsertResult::Inserted => {
                (*set_ptr).len += rtdt::Usize::ONE;
                *bool_out = 1;
                RtStatus::Ok
            }
            LeafInsertResult::NeedsSplit => {
                let split_info = match split_leaf(rt, leaf, element_ptr, set_element_tydesc) {
                    Ok(info) => info,
                    Err(status) => return status,
                };

                let was_inserted = split_info.insert_result == LeafInsertResult::Inserted;

                let status = propagate_split_up(
                    rt,
                    &mut (*set_ptr).root,
                    leaf,
                    split_info,
                    &path,
                    set_element_tydesc,
                );

                if status == RtStatus::Ok && was_inserted {
                    (*set_ptr).len += rtdt::Usize::ONE;
                    *bool_out = 1;
                } else {
                    *bool_out = 0;
                }
                status
            }
        }
    }
}

/// Creates a BTreeSet from a slice of elements.
pub unsafe fn btreeset_clone_from_slice_impl(
    rt: &mut RtLocal,
    slice_ptr_ref: *const u8,
    slice_ptr_len: rtdt::UsizeRepr,
    slice_element_tydesc: *const TyDesc,
    btreeset_value_out: *mut u8,
    btreeset_tydesc: *const TyDesc,
) -> RtStatus {
    unsafe {
        if btreeset_value_out.is_null() || slice_element_tydesc.is_null() {
            return RtStatus::Error;
        }

        // Create an empty set.
        let status = btreeset_create_impl(rt, btreeset_value_out, btreeset_tydesc);
        if status != RtStatus::Ok {
            return status;
        }

        // If the slice is empty, we're done.
        if slice_ptr_len == 0 || slice_ptr_ref.is_null() {
            return RtStatus::Ok;
        }

        let ty = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        let set_element_ty = ty.set_element_ty();
        let set_element_tydesc = set_element_ty.as_ptr();

        let slice_element_tydesc_ref = rtdt::TyDescRef::from_ptr(slice_element_tydesc);
        let element_size = slice_element_tydesc_ref.size() as usize;
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        // Iterate through each element in the slice.
        for i in 0..slice_ptr_len {
            let element_ptr = slice_ptr_ref.add(i as usize * element_size);

            // Clone the element into a temporary buffer.
            let mut element_buf = AlignedBuffer::with_align(element_size, slice_element_tydesc_ref.align() as usize);
            let status = crate::impls::clone::clone_value(
                rt_handle,
                element_ptr,
                slice_element_tydesc,
                element_buf.as_mut_ptr(),
            );
            if status != RtStatus::Ok {
                return status;
            }

            // Insert the cloned element into the set.
            let mut was_inserted = 0u8;
            let status = btreeset_insert_impl(
                rt,
                btreeset_value_out,
                btreeset_tydesc,
                element_buf.as_mut_ptr(),
                set_element_tydesc,
                &mut was_inserted,
            );

            // btreeset_insert_impl takes ownership of the element (by-move semantics).
            // The Vec will be dropped automatically.

            if status != RtStatus::Ok {
                return status;
            }
        }

        RtStatus::Ok
    }
}

/// Removes an element from a set.
pub unsafe fn btreeset_remove_impl(
    rt: &mut RtLocal,
    btreeset_value_mut: *mut u8,
    btreeset_tydesc: *const TyDesc,
    element_ref: *const u8,
    _element_tydesc: *const TyDesc,
    bool_out: *mut u8,
) -> RtStatus {
    unsafe {
        if btreeset_value_mut.is_null() || element_ref.is_null() || bool_out.is_null() {
            return RtStatus::Error;
        }

        let ty = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        let set_element_ty = ty.set_element_ty();
        let set_element_tydesc = set_element_ty.as_ptr();
        let set_element_tydesc_ref = rtdt::TyDescRef::from_ptr(set_element_tydesc);

        let set_ptr = btreeset_value_mut as *mut Set;
        let root = (*set_ptr).root as *mut SetNode;

        // If set is empty, nothing to remove.
        if root.is_null() {
            *bool_out = 0;
            return RtStatus::Ok;
        }

        // Find the leaf containing the element.
        let leaf = find_leaf_for_element(root, element_ref, set_element_tydesc);

        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, set_element_tydesc);
        let element_size = set_element_tydesc_ref.size() as usize;

        // Find and remove the element from the leaf.
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * element_size);
            let cmp_result = super::cmp::cmp_total(
                element_ref,
                set_element_tydesc,
                node_key,
                set_element_tydesc,
            );

            match cmp_result {
                crate::c::RtOrdering::Equal => {
                    // Found the element, destroy it.
                    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
                    let _ = crate::impls::destroy::any_destroy_local(rt_handle, node_key as *mut u8, set_element_tydesc);

                    // Shift remaining elements left.
                    if i < (len - 1) as usize {
                        let shift_count = (len - 1) as usize - i;
                        let src_key = keys_ptr.add((i + 1) * element_size);
                        let dst_key = keys_ptr.add(i * element_size);
                        std::ptr::copy(src_key, dst_key, shift_count * element_size);
                    }

                    write_node_len(leaf, len - 1);
                    (*set_ptr).len -= rtdt::Usize::ONE;

                    // Handle empty root case.
                    if (*set_ptr).len == rtdt::Usize::ZERO {
                        destroy_tree_recursive(rt, root, set_element_tydesc);
                        (*set_ptr).root = std::ptr::null();
                    }

                    *bool_out = 1;
                    return RtStatus::Ok;
                }
                crate::c::RtOrdering::Greater => continue,
                crate::c::RtOrdering::Less | crate::c::RtOrdering::Error => break,
            }
        }

        // Element not found.
        *bool_out = 0;
        RtStatus::Ok
    }
}

/// Checks if a set contains an element.
pub unsafe fn btreeset_contains_impl(
    _rt: &mut RtLocal,
    btreeset_value_ref: *const u8,
    btreeset_tydesc: *const TyDesc,
    element_ref: *const u8,
    _element_tydesc: *const TyDesc,
    bool_out: *mut u8,
) -> RtStatus {
    unsafe {
        if btreeset_value_ref.is_null() || element_ref.is_null() || bool_out.is_null() {
            return RtStatus::Error;
        }

        let ty = rtdt::TyDescRef::from_ptr(btreeset_tydesc);
        let set_element_ty = ty.set_element_ty();
        let set_element_tydesc = set_element_ty.as_ptr();
        let set_element_tydesc_ref = rtdt::TyDescRef::from_ptr(set_element_tydesc);

        let set_ptr = btreeset_value_ref as *const Set;
        let root = (*set_ptr).root as *mut SetNode;

        // If set is empty, return false.
        if root.is_null() {
            *bool_out = 0;
            return RtStatus::Ok;
        }

        // Find the leaf node where the element would be.
        let leaf = find_leaf_for_element(root, element_ref, set_element_tydesc);

        let len = read_node_len(leaf);
        let keys_ptr = leaf_keys_ptr(leaf, set_element_tydesc);
        let element_size = set_element_tydesc_ref.size() as usize;

        // Search for the element in the leaf.
        for i in 0..len as usize {
            let node_key = keys_ptr.add(i * element_size);
            let cmp_result = super::cmp::cmp_total(
                element_ref,
                set_element_tydesc,
                node_key,
                set_element_tydesc,
            );

            match cmp_result {
                crate::c::RtOrdering::Equal => {
                    // Element found!
                    *bool_out = 1;
                    return RtStatus::Ok;
                }
                crate::c::RtOrdering::Greater => {
                    // Continue searching.
                    continue;
                }
                crate::c::RtOrdering::Less | crate::c::RtOrdering::Error => {
                    // Element not found.
                    break;
                }
            }
        }

        // Element not found.
        *bool_out = 0;
        RtStatus::Ok
    }
}
