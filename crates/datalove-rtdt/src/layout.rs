//! Layout computation for datalit runtime types.

use crate::*;

/// Align a value up to the given alignment.
/// Alignment must be a power of 2.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Compute the payload offset for an Option<T> type.
///
/// The payload starts after the tag (u8), aligned to the inner type's alignment.
#[inline]
pub fn option_payload_offset(inner_align: u32) -> u32 {
    align_up(1, inner_align)
}

/// Compute the payload offset for an enum variant, given the payload's
/// alignment.
///
/// The payload starts after the u32 discriminant, aligned to the payload.
#[inline]
pub fn enum_payload_offset(payload_align: u32) -> u32 {
    align_up(4, payload_align)
}

/// Compute the payload offset for a Result<T> type.
///
/// The payload starts after the tag (u8), aligned to the maximum of
/// the ok type's alignment and the error alignment.
#[inline]
pub fn result_payload_offset(ok_align: u32) -> u32 {
    let error_align = std::mem::align_of::<usize>().max(std::mem::align_of::<*const TyDesc>()) as u32;
    let max_payload_align = ok_align.max(error_align);
    align_up(1, max_payload_align)
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
pub fn compute_tuple_layout(tydesc: TyDescRef) -> TupleLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Tuple);

    let tuple_info = tydesc.tuple_info();
    let num_fields = tuple_info.num_fields();

    let mut offset = 0u32;
    let mut max_align = 1u32;
    let mut field_offsets = Vec::with_capacity(num_fields as usize);

    for field in tydesc.iter_tuple_fields() {
        let field_tydesc = field.tydesc();
        let field_size = field_tydesc.size();
        let field_align = field_tydesc.align();

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

/// Compute the memory layout for a struct type.
///
/// Struct layout is identical to tuple layout - fields are laid out
/// sequentially with proper alignment, and the struct is padded to
/// its natural alignment.
pub fn compute_struct_layout(tydesc: TyDescRef) -> StructLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Struct);

    let struct_info = tydesc.struct_info();
    let num_fields = struct_info.num_fields();

    let mut offset = 0u32;
    let mut max_align = 1u32;
    let mut field_offsets = Vec::with_capacity(num_fields as usize);

    for field in tydesc.iter_struct_fields() {
        let field_tydesc = field.tydesc();
        let field_size = field_tydesc.size();
        let field_align = field_tydesc.align();

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

/// Compute the memory layout for an enum type.
///
/// Enum layout consists of:
/// 1. A discriminant (u32) indicating which variant is active
/// 2. Padding to align the payload
/// 3. Space for the largest variant payload
///
/// All variants share the same payload space, with each variant's
/// payload starting at its specific offset.
pub fn compute_enum_layout(tydesc: TyDescRef) -> EnumLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Enum);

    let enum_info = tydesc.enum_info();
    let num_variants = enum_info.num_variants();

    let discriminant_size = 4u32; // u32
    let discriminant_align = 4u32;

    let mut max_variant_size = 0u32;
    let mut max_variant_align = discriminant_align;
    let mut variant_offsets = Vec::with_capacity(num_variants as usize);

    // Find the largest variant to determine payload space needed.
    for variant in tydesc.iter_enum_variants() {
        if let Some(payload_tydesc) = variant.payload() {
            let payload_size = payload_tydesc.size();
            let payload_align = payload_tydesc.align();

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

/// Compute the memory layout for an Option<T> type.
///
/// Option layout consists of:
/// 1. A tag (u8) indicating None or Some
/// 2. Padding to align the payload
/// 3. Space for the inner type T (if Some)
pub fn compute_option_layout(tydesc: TyDescRef) -> OptionLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Option);

    let inner_tydesc = tydesc.option_inner_ty();

    let tag_size = 1u32; // u8
    let tag_align = 1u32;

    let inner_size = inner_tydesc.size();
    let inner_align = inner_tydesc.align();

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

/// Compute the memory layout for a Result<T> type.
///
/// Result layout consists of:
/// 1. A tag (u8) indicating Ok or Err
/// 2. Padding to align the payload
/// 3. Space for max(T, Error) - whichever is larger
///
/// The Ok variant contains T, the Err variant contains Error.
pub fn compute_result_layout(tydesc: TyDescRef) -> ResultLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Result);

    let ok_tydesc = tydesc.result_ok_ty();

    let tag_size = 1u32; // u8
    let tag_align = 1u32;

    let ok_size = ok_tydesc.size();
    let ok_align = ok_tydesc.align();

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

/// Compute the memory layout for a Map internal node.
///
/// Internal node layout:
/// 1. tag (u8) - MapNodeTag::Internal
/// 2. padding to align len
/// 3. len (u32) - actual keys currently stored (0 <= len <= CAPACITY)
/// 4. padding to align keys array
/// 5. keys[CAPACITY] array - space for MAP_NODE_CAPACITY = 11 keys
/// 6. padding to align child_ptrs array
/// 7. child_ptrs[CAPACITY+1] array - 12 child pointers
///
/// # Parameters
/// - `key_tydesc`: Type descriptor for the key type
pub fn compute_map_internal_node_layout(
    key_tydesc: TyDescRef,
) -> MapNodeInternalLayout {
    let key_size = key_tydesc.size();
    let key_align = key_tydesc.align();

    let tag_size = 1u32; // u8
    let ptr_size = std::mem::size_of::<*const MapNode>() as u32;
    let ptr_align = std::mem::align_of::<*const MapNode>() as u32;

    // Start after tag.
    let mut offset = tag_size;

    // len (u32) at 4-byte aligned offset.
    offset = align_up(offset, 4);
    offset += 4; // sizeof(u32)

    // keys array at key-aligned offset.
    offset = align_up(offset, key_align);
    let keys_offset = offset;
    offset += key_size * MAP_NODE_CAPACITY;

    // child_ptrs array at pointer-aligned offset.
    // Internal nodes have CAPACITY+1 child pointers.
    offset = align_up(offset, ptr_align);
    let child_ptrs_offset = offset;
    offset += ptr_size * (MAP_NODE_CAPACITY + 1);

    // Overall alignment is max of all components.
    let overall_align = 1u32.max(4).max(key_align).max(ptr_align);

    // Total size aligned to overall alignment.
    let total_size = align_up(offset, overall_align);

    MapNodeInternalLayout {
        size: total_size,
        align: overall_align,
        keys_offset,
        child_ptrs_offset,
    }
}

/// Compute the memory layout for a Map leaf node.
///
/// Leaf node layout:
/// 1. tag (u8) - MapNodeTag::Leaf
/// 2. padding to align len
/// 3. len (u32) - actual key-value pairs currently stored (0 <= len <= CAPACITY)
/// 4. next_leaf (*const MapNode) - pointer to next leaf in chain
/// 5. padding to align keys array
/// 6. keys[CAPACITY] array - space for MAP_NODE_CAPACITY = 11 keys
/// 7. padding to align values array
/// 8. values[CAPACITY] array - space for MAP_NODE_CAPACITY = 11 values
///
/// # Parameters
/// - `key_tydesc`: Type descriptor for the key type
/// - `value_tydesc`: Type descriptor for the value type
pub fn compute_map_leaf_node_layout(
    key_tydesc: TyDescRef,
    value_tydesc: TyDescRef,
) -> MapNodeLeafLayout {
    let key_size = key_tydesc.size();
    let key_align = key_tydesc.align();
    let value_size = value_tydesc.size();
    let value_align = value_tydesc.align();

    let tag_size = 1u32; // u8
    let ptr_size = std::mem::size_of::<*const MapNode>() as u32;
    let ptr_align = std::mem::align_of::<*const MapNode>() as u32;

    // Start after tag.
    let mut offset = tag_size;

    // len (u32) at 4-byte aligned offset.
    offset = align_up(offset, 4);
    offset += 4; // sizeof(u32)

    // next_leaf pointer at pointer-aligned offset.
    offset = align_up(offset, ptr_align);
    let next_leaf_offset = offset;
    offset += ptr_size;

    // keys array at key-aligned offset.
    offset = align_up(offset, key_align);
    let keys_offset = offset;
    offset += key_size * MAP_NODE_CAPACITY;

    // values array at value-aligned offset.
    offset = align_up(offset, value_align);
    let values_offset = offset;
    offset += value_size * MAP_NODE_CAPACITY;

    // Overall alignment is max of all components.
    let overall_align = 1u32.max(4).max(ptr_align).max(key_align).max(value_align);

    // Total size aligned to overall alignment.
    let total_size = align_up(offset, overall_align);

    MapNodeLeafLayout {
        size: total_size,
        align: overall_align,
        next_leaf_offset,
        keys_offset,
        values_offset,
    }
}

/// Compute the memory layout for a Set internal node.
///
/// Internal node layout:
/// 1. tag (u8) - SetNodeTag::Internal
/// 2. padding to align len
/// 3. len (u32) - actual keys currently stored (0 <= len <= CAPACITY)
/// 4. padding to align keys array
/// 5. keys[CAPACITY] array - space for SET_NODE_CAPACITY = 11 keys
/// 6. padding to align child_ptrs array
/// 7. child_ptrs[CAPACITY+1] array - 12 child pointers
///
/// # Parameters
/// - `key_tydesc`: Type descriptor for the element type
pub fn compute_set_internal_node_layout(
    key_tydesc: TyDescRef,
) -> SetNodeInternalLayout {
    let key_size = key_tydesc.size();
    let key_align = key_tydesc.align();

    let tag_size = 1u32; // u8
    let ptr_size = std::mem::size_of::<*const SetNode>() as u32;
    let ptr_align = std::mem::align_of::<*const SetNode>() as u32;

    // Start after tag.
    let mut offset = tag_size;

    // len (u32) at 4-byte aligned offset.
    offset = align_up(offset, 4);
    offset += 4; // sizeof(u32)

    // keys array at key-aligned offset.
    offset = align_up(offset, key_align);
    let keys_offset = offset;
    offset += key_size * SET_NODE_CAPACITY;

    // child_ptrs array at pointer-aligned offset.
    // Internal nodes have CAPACITY+1 child pointers.
    offset = align_up(offset, ptr_align);
    let child_ptrs_offset = offset;
    offset += ptr_size * (SET_NODE_CAPACITY + 1);

    // Overall alignment is max of all components.
    let overall_align = 1u32.max(4).max(key_align).max(ptr_align);

    // Total size aligned to overall alignment.
    let total_size = align_up(offset, overall_align);

    SetNodeInternalLayout {
        size: total_size,
        align: overall_align,
        keys_offset,
        child_ptrs_offset,
    }
}

/// Compute the memory layout for a Set leaf node.
///
/// Leaf node layout:
/// 1. tag (u8) - SetNodeTag::Leaf
/// 2. padding to align len
/// 3. len (u32) - actual elements currently stored (0 <= len <= CAPACITY)
/// 4. next_leaf (*const SetNode) - pointer to next leaf in chain
/// 5. padding to align keys array
/// 6. keys[CAPACITY] array - space for SET_NODE_CAPACITY = 11 keys (set elements)
///
/// # Parameters
/// - `key_tydesc`: Type descriptor for the element type
pub fn compute_set_leaf_node_layout(
    key_tydesc: TyDescRef,
) -> SetNodeLeafLayout {
    let key_size = key_tydesc.size();
    let key_align = key_tydesc.align();

    let tag_size = 1u32; // u8
    let ptr_size = std::mem::size_of::<*const SetNode>() as u32;
    let ptr_align = std::mem::align_of::<*const SetNode>() as u32;

    // Start after tag.
    let mut offset = tag_size;

    // len (u32) at 4-byte aligned offset.
    offset = align_up(offset, 4);
    offset += 4; // sizeof(u32)

    // next_leaf pointer at pointer-aligned offset.
    offset = align_up(offset, ptr_align);
    let next_leaf_offset = offset;
    offset += ptr_size;

    // keys array at key-aligned offset.
    offset = align_up(offset, key_align);
    let keys_offset = offset;
    offset += key_size * SET_NODE_CAPACITY;

    // Overall alignment is max of all components.
    let overall_align = 1u32.max(4).max(ptr_align).max(key_align);

    // Total size aligned to overall alignment.
    let total_size = align_up(offset, overall_align);

    SetNodeLeafLayout {
        size: total_size,
        align: overall_align,
        next_leaf_offset,
        keys_offset,
    }
}

/// Align a usize value up to the given alignment.
#[inline]
fn align_up_usize(value: usize, align: u32) -> usize {
    let a = align as usize;
    (value + a - 1) & !(a - 1)
}

/// Compute byte offset of a column within a table's data allocation.
///
/// Iterates through prior columns, accumulating their aligned sizes.
#[inline]
pub fn table_column_offset(
    column_tydescs: &[&TyDesc],
    column_index: usize,
    capacity: IndexRepr,
) -> usize {
    let mut offset = 0usize;
    for i in 0..column_index {
        offset = align_up_usize(offset, column_tydescs[i].align);
        offset += (column_tydescs[i].size as usize) * (capacity as usize);
    }
    align_up_usize(offset, column_tydescs[column_index].align)
}

/// Compute total allocation size for a table's data.
#[inline]
pub fn table_data_allocation_size(
    column_tydescs: &[&TyDesc],
    capacity: IndexRepr,
) -> u32 {
    if column_tydescs.is_empty() || capacity == 0 {
        return 0;
    }
    let mut offset = 0usize;
    let mut max_align = 1u32;
    for tydesc in column_tydescs {
        offset = align_up_usize(offset, tydesc.align);
        offset += (tydesc.size as usize) * (capacity as usize);
        max_align = max_align.max(tydesc.align);
    }
    align_up_usize(offset, max_align) as u32
}

/// Compute required alignment for a table's data allocation.
#[inline]
pub fn table_data_alignment(column_tydescs: &[&TyDesc]) -> u32 {
    column_tydescs.iter().map(|td| td.align).max().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tydesc(size: u32, align: u32) -> TyDesc {
        TyDesc {
            type_tag: TyTag::U8,
            size,
            align,
            type_info: TyInfo {
                nothing: TyInfoNothing,
            },
        }
    }

    // table_column_offset tests

    #[test]
    fn test_table_column_offset_first_column() {
        let td = make_tydesc(4, 4);
        let tydescs: Vec<&TyDesc> = vec![&td];
        assert_eq!(table_column_offset(&tydescs, 0, 10), 0);
    }

    #[test]
    fn test_table_column_offset_second_column_same_align() {
        let td1 = make_tydesc(4, 4);
        let td2 = make_tydesc(4, 4);
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2];
        // Column 0: 10 elements * 4 bytes = 40 bytes at offset 0.
        // Column 1: starts at offset 40 (already aligned).
        assert_eq!(table_column_offset(&tydescs, 1, 10), 40);
    }

    #[test]
    fn test_table_column_offset_alignment_padding() {
        let td1 = make_tydesc(1, 1); // u8
        let td2 = make_tydesc(8, 8); // u64
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2];
        // Column 0: 10 elements * 1 byte = 10 bytes at offset 0.
        // Column 1: needs 8-byte alignment, so aligns 10 -> 16.
        assert_eq!(table_column_offset(&tydescs, 1, 10), 16);
    }

    #[test]
    fn test_table_column_offset_three_columns() {
        let td1 = make_tydesc(1, 1); // u8
        let td2 = make_tydesc(4, 4); // u32
        let td3 = make_tydesc(8, 8); // u64
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2, &td3];
        // Column 0: 5 elements * 1 byte = 5 bytes at offset 0.
        // Column 1: needs 4-byte alignment, aligns 5 -> 8, then 5*4=20 bytes.
        // Column 2: offset after col1 = 8 + 20 = 28, needs 8-byte alignment -> 32.
        assert_eq!(table_column_offset(&tydescs, 0, 5), 0);
        assert_eq!(table_column_offset(&tydescs, 1, 5), 8);
        assert_eq!(table_column_offset(&tydescs, 2, 5), 32);
    }

    // table_data_allocation_size tests

    #[test]
    fn test_table_data_allocation_size_empty() {
        let tydescs: Vec<&TyDesc> = vec![];
        assert_eq!(table_data_allocation_size(&tydescs, 10), 0);
    }

    #[test]
    fn test_table_data_allocation_size_zero_capacity() {
        let td = make_tydesc(4, 4);
        let tydescs: Vec<&TyDesc> = vec![&td];
        assert_eq!(table_data_allocation_size(&tydescs, 0), 0);
    }

    #[test]
    fn test_table_data_allocation_size_single_column() {
        let td = make_tydesc(4, 4);
        let tydescs: Vec<&TyDesc> = vec![&td];
        // 10 elements * 4 bytes = 40 bytes, already aligned to 4.
        assert_eq!(table_data_allocation_size(&tydescs, 10), 40);
    }

    #[test]
    fn test_table_data_allocation_size_multiple_columns() {
        let td1 = make_tydesc(1, 1); // u8
        let td2 = make_tydesc(4, 4); // u32
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2];
        // Column 0: 5 * 1 = 5 bytes at offset 0.
        // Column 1: align 5 -> 8, then 5 * 4 = 20 bytes.
        // Total: 8 + 20 = 28, align to max_align=4 -> 28.
        assert_eq!(table_data_allocation_size(&tydescs, 5), 28);
    }

    #[test]
    fn test_table_data_allocation_size_final_alignment() {
        let td1 = make_tydesc(1, 1); // u8
        let td2 = make_tydesc(8, 8); // u64
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2];
        // Column 0: 3 * 1 = 3 bytes at offset 0.
        // Column 1: align 3 -> 8, then 3 * 8 = 24 bytes.
        // Total: 8 + 24 = 32, already aligned to max_align=8.
        assert_eq!(table_data_allocation_size(&tydescs, 3), 32);
    }

    #[test]
    fn test_table_data_allocation_size_needs_final_padding() {
        let td1 = make_tydesc(8, 8); // u64
        let td2 = make_tydesc(1, 1); // u8
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2];
        // Column 0: 3 * 8 = 24 bytes at offset 0.
        // Column 1: 3 * 1 = 3 bytes at offset 24.
        // Total: 24 + 3 = 27, align to max_align=8 -> 32.
        assert_eq!(table_data_allocation_size(&tydescs, 3), 32);
    }

    // table_data_alignment tests

    #[test]
    fn test_table_data_alignment_empty() {
        let tydescs: Vec<&TyDesc> = vec![];
        assert_eq!(table_data_alignment(&tydescs), 1);
    }

    #[test]
    fn test_table_data_alignment_single_column() {
        let td = make_tydesc(4, 4);
        let tydescs: Vec<&TyDesc> = vec![&td];
        assert_eq!(table_data_alignment(&tydescs), 4);
    }

    #[test]
    fn test_table_data_alignment_max_of_columns() {
        let td1 = make_tydesc(1, 1); // u8
        let td2 = make_tydesc(8, 8); // u64
        let td3 = make_tydesc(4, 4); // u32
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2, &td3];
        assert_eq!(table_data_alignment(&tydescs), 8);
    }

    #[test]
    fn test_table_data_alignment_all_same() {
        let td1 = make_tydesc(4, 4);
        let td2 = make_tydesc(4, 4);
        let tydescs: Vec<&TyDesc> = vec![&td1, &td2];
        assert_eq!(table_data_alignment(&tydescs), 4);
    }
}
