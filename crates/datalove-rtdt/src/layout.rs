//! Layout computation for datalit runtime types.

use crate::*;

/// Align a value up to the given alignment.
/// Alignment must be a power of 2.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Compute the memory layout for a tuple type.
///
/// This function implements the standard struct layout algorithm:
/// 1. Start at offset 0 with alignment 1
/// 2. For each field in order:
///    - Align current offset to field's alignment requirement
///    - Record this as the field's offset
///    - Advance offset by field's size
///    - Track maximum alignment seen
/// 3. Final size is current offset aligned up to maximum alignment
/// 4. Overall alignment is the maximum alignment
///
/// # Safety
/// The tydesc must point to a valid TyDesc with type_tag = TyTag::Tuple.
pub unsafe fn compute_tuple_layout(tydesc: *const TyDesc) -> TupleLayout {
    unsafe {
        debug_assert_eq!((*tydesc).type_tag, TyTag::Tuple);

        let tuple_info = (*tydesc).type_info.tuple;
        let num_fields = tuple_info.num_fields;
        let fields = tuple_info.fields;

        let mut offset = 0u32;
        let mut max_align = 1u32;
        let mut field_offsets = Vec::with_capacity(num_fields as usize);

        for i in 0..num_fields {
            let field = &*fields.add(i as usize);
            let field_tydesc = &*field.tydesc;
            let field_size = field_tydesc.size;
            let field_align = field_tydesc.align;

            // Align offset to field's alignment requirement.
            offset = align_up(offset, field_align);
            field_offsets.push(offset);

            // Advance offset by field size.
            offset += field_size;

            // Track maximum alignment.
            max_align = max_align.max(field_align);
        }

        // Total size must be aligned to the maximum alignment.
        let total_size = align_up(offset, max_align);

        TupleLayout {
            size: total_size,
            align: max_align,
            field_offsets,
        }
    }
}

/// Compute the memory layout for a struct type.
///
/// Struct layout is identical to tuple layout - fields are laid out
/// sequentially with proper alignment, and the struct is padded to
/// its natural alignment.
///
/// # Safety
/// The tydesc must point to a valid TyDesc with type_tag = TyTag::Struct.
pub unsafe fn compute_struct_layout(tydesc: *const TyDesc) -> StructLayout {
    unsafe {
        debug_assert_eq!((*tydesc).type_tag, TyTag::Struct);

        let struct_info = (*tydesc).type_info.struct_;
        let num_fields = struct_info.num_fields;
        let fields = struct_info.fields;

        let mut offset = 0u32;
        let mut max_align = 1u32;
        let mut field_offsets = Vec::with_capacity(num_fields as usize);

        for i in 0..num_fields {
            let field = &*fields.add(i as usize);
            let field_tydesc = &*field.tydesc;
            let field_size = field_tydesc.size;
            let field_align = field_tydesc.align;

            // Align offset to field's alignment requirement.
            offset = align_up(offset, field_align);
            field_offsets.push(offset);

            // Advance offset by field size.
            offset += field_size;

            // Track maximum alignment.
            max_align = max_align.max(field_align);
        }

        // Total size must be aligned to the maximum alignment.
        let total_size = align_up(offset, max_align);

        StructLayout {
            size: total_size,
            align: max_align,
            field_offsets,
        }
    }
}

/// Compute the memory layout for an enum type.
///
/// Enum layout consists of:
/// 1. A discriminant (u32) indicating which variant is active
/// 2. Padding to align the payload
/// 3. Space for the largest variant payload
///
/// All variants share the same payload space, with each variant's
/// payload starting at its specific offset.
///
/// # Safety
/// The tydesc must point to a valid TyDesc with type_tag = TyTag::Enum.
pub unsafe fn compute_enum_layout(tydesc: *const TyDesc) -> EnumLayout {
    unsafe {
        debug_assert_eq!((*tydesc).type_tag, TyTag::Enum);

        let enum_info = (*tydesc).type_info.enum_;
        let num_variants = enum_info.num_variants;
        let variants = enum_info.variants;

        let discriminant_size = 4u32; // u32
        let discriminant_align = 4u32;

        let mut max_variant_size = 0u32;
        let mut max_variant_align = discriminant_align;
        let mut variant_offsets = Vec::with_capacity(num_variants as usize);

        // Find the largest variant to determine payload space needed.
        for i in 0..num_variants {
            let variant = &*variants.add(i as usize);

            if let Some(payload_tydesc) = variant.payload.as_ref() {
                let payload_size = payload_tydesc.size;
                let payload_align = payload_tydesc.align;

                max_variant_size = max_variant_size.max(payload_size);
                max_variant_align = max_variant_align.max(payload_align);

                // Each variant's payload starts at the aligned offset after discriminant.
                let payload_offset = align_up(discriminant_size, payload_align);
                variant_offsets.push(payload_offset);
            } else {
                // No payload - variant has no offset.
                variant_offsets.push(0);
            }
        }

        // Payload starts after discriminant, aligned to max payload alignment.
        let payload_offset = align_up(discriminant_size, max_variant_align);

        // Total size is payload offset + max payload size, aligned to max alignment.
        let total_size = align_up(payload_offset + max_variant_size, max_variant_align);

        EnumLayout {
            size: total_size,
            align: max_variant_align,
            discriminant_size,
            payload_offset,
            variant_offsets,
        }
    }
}

/// Compute the memory layout for an Option<T> type.
///
/// Option layout consists of:
/// 1. A tag (u8) indicating None or Some
/// 2. Padding to align the payload
/// 3. Space for the inner type T (if Some)
///
/// # Safety
/// The tydesc must point to a valid TyDesc with type_tag = TyTag::Option.
pub unsafe fn compute_option_layout(tydesc: *const TyDesc) -> OptionLayout {
    unsafe {
        debug_assert_eq!((*tydesc).type_tag, TyTag::Option);

        let option_info = (*tydesc).type_info.option;
        let inner_tydesc = &*option_info.inner_tydesc;

        let tag_size = 1u32; // u8
        let tag_align = 1u32;

        let inner_size = inner_tydesc.size;
        let inner_align = inner_tydesc.align;

        // Payload starts after tag, aligned to inner type's alignment.
        let payload_offset = align_up(tag_size, inner_align);

        // Overall alignment is max of tag and inner alignment.
        let overall_align = tag_align.max(inner_align);

        // Total size is payload offset + inner size, aligned to overall alignment.
        let total_size = align_up(payload_offset + inner_size, overall_align);

        OptionLayout {
            size: total_size,
            align: overall_align,
            tag_size,
            payload_offset,
        }
    }
}

/// Compute the memory layout for a Result<T> type.
///
/// Result layout consists of:
/// 1. A tag (u8) indicating Ok or Err
/// 2. Padding to align the payload
/// 3. Space for max(T, Error) - whichever is larger
///
/// The Ok variant contains T, the Err variant contains Error.
///
/// # Safety
/// The tydesc must point to a valid TyDesc with type_tag = TyTag::Result.
pub unsafe fn compute_result_layout(tydesc: *const TyDesc) -> ResultLayout {
    unsafe {
        debug_assert_eq!((*tydesc).type_tag, TyTag::Result);

        let result_info = (*tydesc).type_info.result;
        let ok_tydesc = &*result_info.ok_tydesc;

        let tag_size = 1u32; // u8
        let tag_align = 1u32;

        let ok_size = ok_tydesc.size;
        let ok_align = ok_tydesc.align;

        // Error type layout: usize + pointer
        let error_size = (std::mem::size_of::<usize>() + std::mem::size_of::<*const TyDesc>()) as u32;
        let error_align = std::mem::align_of::<usize>().max(std::mem::align_of::<*const TyDesc>()) as u32;

        // Payload must accommodate the larger of Ok and Err variants.
        let max_payload_size = ok_size.max(error_size);
        let max_payload_align = ok_align.max(error_align);

        // Payload starts after tag, aligned to maximum payload alignment.
        let payload_offset = align_up(tag_size, max_payload_align);

        // Overall alignment is max of tag and payload alignment.
        let overall_align = tag_align.max(max_payload_align);

        // Total size is payload offset + max payload size, aligned to overall alignment.
        let total_size = align_up(payload_offset + max_payload_size, overall_align);

        ResultLayout {
            size: total_size,
            align: overall_align,
            tag_size,
            payload_offset,
        }
    }
}

/// Compute the memory layout for a Map internal node.
///
/// Internal node layout:
/// 1. tag (u8) - MapNodeTag::Internal
/// 2. padding to align num_keys
/// 3. num_keys (u32)
/// 4. padding to align keys array
/// 5. keys[num_keys] array
/// 6. padding to align child_ptrs array
/// 7. child_ptrs[num_keys+1] array (one more child than keys)
///
/// # Parameters
/// - `num_keys`: Number of keys in this node
/// - `key_tydesc`: Type descriptor for the key type
///
/// # Safety
/// The key_tydesc must point to a valid TyDesc.
pub unsafe fn compute_map_internal_node_layout(
    num_keys: u32,
    key_tydesc: *const TyDesc,
) -> MapNodeInternalLayout {
    unsafe {
        let key_size = (*key_tydesc).size;
        let key_align = (*key_tydesc).align;

        let tag_size = 1u32; // u8
        let ptr_size = std::mem::size_of::<*const MapNode>() as u32;
        let ptr_align = std::mem::align_of::<*const MapNode>() as u32;

        // Start after tag.
        let mut offset = tag_size;

        // num_keys (u32) at 4-byte aligned offset.
        offset = align_up(offset, 4);
        let num_keys_offset = offset;
        offset += 4; // sizeof(u32)

        // keys array at key-aligned offset.
        offset = align_up(offset, key_align);
        let keys_offset = offset;
        offset += key_size * num_keys;

        // child_ptrs array at pointer-aligned offset.
        // Internal nodes have num_keys+1 child pointers.
        offset = align_up(offset, ptr_align);
        let child_ptrs_offset = offset;
        offset += ptr_size * (num_keys + 1);

        // Overall alignment is max of all components.
        let overall_align = 1u32.max(4).max(key_align).max(ptr_align);

        // Total size aligned to overall alignment.
        let total_size = align_up(offset, overall_align);

        MapNodeInternalLayout {
            size: total_size,
            align: overall_align,
            num_keys_offset,
            keys_offset,
            child_ptrs_offset,
        }
    }
}

/// Compute the memory layout for a Map leaf node.
///
/// Leaf node layout:
/// 1. tag (u8) - MapNodeTag::Leaf
/// 2. padding to align num_keys
/// 3. num_keys (u32)
/// 4. next_leaf (*const MapNode) - pointer to next leaf in chain
/// 5. padding to align keys array
/// 6. keys[num_keys] array
/// 7. padding to align values array
/// 8. values[num_keys] array (matching key-value pairs)
///
/// # Parameters
/// - `num_keys`: Number of key-value pairs in this node
/// - `key_tydesc`: Type descriptor for the key type
/// - `value_tydesc`: Type descriptor for the value type
///
/// # Safety
/// The key_tydesc and value_tydesc must point to valid TyDesc.
pub unsafe fn compute_map_leaf_node_layout(
    num_keys: u32,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
) -> MapNodeLeafLayout {
    unsafe {
        let key_size = (*key_tydesc).size;
        let key_align = (*key_tydesc).align;
        let value_size = (*value_tydesc).size;
        let value_align = (*value_tydesc).align;

        let tag_size = 1u32; // u8
        let ptr_size = std::mem::size_of::<*const MapNode>() as u32;
        let ptr_align = std::mem::align_of::<*const MapNode>() as u32;

        // Start after tag.
        let mut offset = tag_size;

        // num_keys (u32) at 4-byte aligned offset.
        offset = align_up(offset, 4);
        let num_keys_offset = offset;
        offset += 4; // sizeof(u32)

        // next_leaf pointer at pointer-aligned offset.
        offset = align_up(offset, ptr_align);
        let next_leaf_offset = offset;
        offset += ptr_size;

        // keys array at key-aligned offset.
        offset = align_up(offset, key_align);
        let keys_offset = offset;
        offset += key_size * num_keys;

        // values array at value-aligned offset.
        offset = align_up(offset, value_align);
        let values_offset = offset;
        offset += value_size * num_keys;

        // Overall alignment is max of all components.
        let overall_align = 1u32.max(4).max(ptr_align).max(key_align).max(value_align);

        // Total size aligned to overall alignment.
        let total_size = align_up(offset, overall_align);

        MapNodeLeafLayout {
            size: total_size,
            align: overall_align,
            num_keys_offset,
            next_leaf_offset,
            keys_offset,
            values_offset,
        }
    }
}

/// Compute the memory layout for a Set internal node.
///
/// Internal node layout:
/// 1. tag (u8) - SetNodeTag::Internal
/// 2. padding to align num_keys
/// 3. num_keys (u32)
/// 4. padding to align keys array
/// 5. keys[num_keys] array
/// 6. padding to align child_ptrs array
/// 7. child_ptrs[num_keys+1] array (one more child than keys)
///
/// # Parameters
/// - `num_keys`: Number of keys in this node
/// - `key_tydesc`: Type descriptor for the element type
///
/// # Safety
/// The key_tydesc must point to a valid TyDesc.
pub unsafe fn compute_set_internal_node_layout(
    num_keys: u32,
    key_tydesc: *const TyDesc,
) -> SetNodeInternalLayout {
    unsafe {
        let key_size = (*key_tydesc).size;
        let key_align = (*key_tydesc).align;

        let tag_size = 1u32; // u8
        let ptr_size = std::mem::size_of::<*const SetNode>() as u32;
        let ptr_align = std::mem::align_of::<*const SetNode>() as u32;

        // Start after tag.
        let mut offset = tag_size;

        // num_keys (u32) at 4-byte aligned offset.
        offset = align_up(offset, 4);
        let num_keys_offset = offset;
        offset += 4; // sizeof(u32)

        // keys array at key-aligned offset.
        offset = align_up(offset, key_align);
        let keys_offset = offset;
        offset += key_size * num_keys;

        // child_ptrs array at pointer-aligned offset.
        // Internal nodes have num_keys+1 child pointers.
        offset = align_up(offset, ptr_align);
        let child_ptrs_offset = offset;
        offset += ptr_size * (num_keys + 1);

        // Overall alignment is max of all components.
        let overall_align = 1u32.max(4).max(key_align).max(ptr_align);

        // Total size aligned to overall alignment.
        let total_size = align_up(offset, overall_align);

        SetNodeInternalLayout {
            size: total_size,
            align: overall_align,
            num_keys_offset,
            keys_offset,
            child_ptrs_offset,
        }
    }
}

/// Compute the memory layout for a Set leaf node.
///
/// Leaf node layout:
/// 1. tag (u8) - SetNodeTag::Leaf
/// 2. padding to align num_keys
/// 3. num_keys (u32)
/// 4. next_leaf (*const SetNode) - pointer to next leaf in chain
/// 5. padding to align keys array
/// 6. keys[num_keys] array (set elements stored as keys)
///
/// # Parameters
/// - `num_keys`: Number of elements in this node
/// - `key_tydesc`: Type descriptor for the element type
///
/// # Safety
/// The key_tydesc must point to a valid TyDesc.
pub unsafe fn compute_set_leaf_node_layout(
    num_keys: u32,
    key_tydesc: *const TyDesc,
) -> SetNodeLeafLayout {
    unsafe {
        let key_size = (*key_tydesc).size;
        let key_align = (*key_tydesc).align;

        let tag_size = 1u32; // u8
        let ptr_size = std::mem::size_of::<*const SetNode>() as u32;
        let ptr_align = std::mem::align_of::<*const SetNode>() as u32;

        // Start after tag.
        let mut offset = tag_size;

        // num_keys (u32) at 4-byte aligned offset.
        offset = align_up(offset, 4);
        let num_keys_offset = offset;
        offset += 4; // sizeof(u32)

        // next_leaf pointer at pointer-aligned offset.
        offset = align_up(offset, ptr_align);
        let next_leaf_offset = offset;
        offset += ptr_size;

        // keys array at key-aligned offset.
        offset = align_up(offset, key_align);
        let keys_offset = offset;
        offset += key_size * num_keys;

        // Overall alignment is max of all components.
        let overall_align = 1u32.max(4).max(ptr_align).max(key_align);

        // Total size aligned to overall alignment.
        let total_size = align_up(offset, overall_align);

        SetNodeLeafLayout {
            size: total_size,
            align: overall_align,
            num_keys_offset,
            next_leaf_offset,
            keys_offset,
        }
    }
}
