//! Layout computation for datalit runtime types.

use crate::*;

/// Align a value up to the given alignment.
/// Alignment must be a power of 2.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Size and alignment of a value, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
}

/// The layout of a sum: its size and alignment, and where its payload starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SumLayout {
    pub layout: TypeLayout,
    pub payload_offset: u32,
}

/// The size of an enum's discriminant, a `u32`, which is also its alignment.
pub const ENUM_DISCRIMINANT_SIZE: u32 = 4;

/// The size of the tag of a `?T` or `!T`, a `u8`.
pub const TAG_SIZE: u32 = 1;

/// The layout of the error a `!T` carries in its `Err` arm.
pub const ERROR_LAYOUT: TypeLayout = TypeLayout {
    size: std::mem::size_of::<Error>() as u32,
    align: std::mem::align_of::<Error>() as u32,
};

/// Compute the payload offset for an Option<T> type.
///
/// The payload starts after the tag (u8), aligned to the inner type's alignment.
#[inline]
pub fn option_payload_offset(inner_align: u32) -> u32 {
    align_up(TAG_SIZE, inner_align)
}

/// Compute the payload offset for an enum variant, given the payload's
/// alignment.
///
/// The payload starts after the u32 discriminant, aligned to the payload.
#[inline]
pub fn enum_payload_offset(payload_align: u32) -> u32 {
    align_up(ENUM_DISCRIMINANT_SIZE, payload_align)
}

/// Compute the payload offset for a Result<T> type.
///
/// The payload starts after the tag (u8), aligned to the maximum of
/// the ok type's alignment and the error alignment.
#[inline]
pub fn result_payload_offset(ok_align: u32) -> u32 {
    align_up(TAG_SIZE, ok_align.max(ERROR_LAYOUT.align))
}

/// Lay out parts in order, each at the first offset past the last that its
/// alignment allows.
///
/// This is the layout of a tuple or struct, and the C rule: `place` is given
/// each part's offset in turn, and the whole is aligned to its most aligned
/// part, at least 1, and sized to end on that alignment.
pub fn sequence_layout(parts: impl IntoIterator<Item = TypeLayout>, mut place: impl FnMut(u32)) -> TypeLayout {
    let mut offset = 0u32;
    let mut align = 1u32;
    for part in parts {
        offset = align_up(offset, part.align);
        place(offset);
        offset += part.size;
        align = align.max(part.align);
    }
    TypeLayout { size: align_up(offset, align), align }
}

/// Lay out an enum: a `u32` discriminant, then room for its largest payload.
///
/// The shared payload offset is the one the most aligned payload needs; a
/// variant's own payload sits at `enum_payload_offset` of its alignment,
/// which is no later.
pub fn enum_layout_of(payloads: impl IntoIterator<Item = TypeLayout>) -> SumLayout {
    let mut size = 0u32;
    let mut align = ENUM_DISCRIMINANT_SIZE;
    for payload in payloads {
        size = size.max(payload.size);
        align = align.max(payload.align);
    }
    tagged_layout(ENUM_DISCRIMINANT_SIZE, TypeLayout { size, align })
}

/// Lay out a `?T`: a tag byte, then the payload.
pub fn option_layout_of(inner: TypeLayout) -> SumLayout {
    tagged_layout(TAG_SIZE, inner)
}

/// Lay out a `!T`: a tag byte, then room for the larger of the ok value and
/// the error, at the alignment of the more aligned.
pub fn result_layout_of(ok: TypeLayout) -> SumLayout {
    tagged_layout(TAG_SIZE, TypeLayout {
        size: ok.size.max(ERROR_LAYOUT.size),
        align: ok.align.max(ERROR_LAYOUT.align),
    })
}

/// A tag of `tag_size` bytes followed by a payload, as one sequence.
fn tagged_layout(tag_size: u32, payload: TypeLayout) -> SumLayout {
    let tag = TypeLayout { size: tag_size, align: 1 };
    let mut payload_offset = 0;
    let layout = sequence_layout([tag, payload], |offset| payload_offset = offset);
    SumLayout { layout, payload_offset }
}

/// The size and alignment a descriptor gives a value.
fn layout_of_tydesc(tydesc: TyDescRef) -> TypeLayout {
    TypeLayout { size: tydesc.size(), align: tydesc.align() }
}

/// Compute the memory layout for a tuple type.
pub fn compute_tuple_layout(tydesc: TyDescRef) -> TupleLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Tuple);
    let mut field_offsets = Vec::with_capacity(tydesc.tuple_info().num_fields() as usize);
    let fields = tydesc.iter_tuple_fields().map(|field| layout_of_tydesc(field.tydesc()));
    let TypeLayout { size, align } = sequence_layout(fields, |offset| field_offsets.push(offset));
    TupleLayout { size, align, field_offsets }
}

/// Compute the memory layout for a struct type, which is a tuple's.
pub fn compute_struct_layout(tydesc: TyDescRef) -> StructLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Struct);
    let mut field_offsets = Vec::with_capacity(tydesc.struct_info().num_fields() as usize);
    let fields = tydesc.iter_struct_fields().map(|field| layout_of_tydesc(field.tydesc()));
    let TypeLayout { size, align } = sequence_layout(fields, |offset| field_offsets.push(offset));
    StructLayout { size, align, field_offsets }
}

/// Compute the memory layout for an enum type.
///
/// A variant without a payload is given an offset of 0.
pub fn compute_enum_layout(tydesc: TyDescRef) -> EnumLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Enum);
    let payloads = || tydesc.iter_enum_variants().filter_map(|variant| variant.payload()).map(layout_of_tydesc);
    let SumLayout { layout, payload_offset } = enum_layout_of(payloads());
    let variant_offsets = tydesc.iter_enum_variants().map(|variant| {
        variant.payload().map_or(0, |payload| enum_payload_offset(payload.align()))
    }).collect();
    EnumLayout {
        size: layout.size,
        align: layout.align,
        discriminant_size: ENUM_DISCRIMINANT_SIZE,
        payload_offset,
        variant_offsets,
    }
}

/// Compute the memory layout for an Option<T> type.
#[inline]
pub fn compute_option_layout(tydesc: TyDescRef) -> OptionLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Option);
    let SumLayout { layout, payload_offset } = option_layout_of(layout_of_tydesc(tydesc.option_inner_ty()));
    OptionLayout { size: layout.size, align: layout.align, tag_size: TAG_SIZE, payload_offset }
}

/// Compute the memory layout for a Result<T> type.
#[inline]
pub fn compute_result_layout(tydesc: TyDescRef) -> ResultLayout {
    debug_assert_eq!(tydesc.type_tag(), TyTag::Result);
    let SumLayout { layout, payload_offset } = result_layout_of(layout_of_tydesc(tydesc.result_ok_ty()));
    ResultLayout { size: layout.size, align: layout.align, tag_size: TAG_SIZE, payload_offset }
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
    map_internal_node_layout(key_tydesc.size(), key_tydesc.align())
}

/// The same, from the key's size and alignment, for a producer that writes
/// descriptors out rather than holding them.
pub fn map_internal_node_layout(key_size: u32, key_align: u32) -> MapNodeInternalLayout {

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
    map_leaf_node_layout(key_tydesc.size(), key_tydesc.align(), value_tydesc.size(), value_tydesc.align())
}

/// The same, from sizes and alignments.
pub fn map_leaf_node_layout(key_size: u32, key_align: u32, value_size: u32, value_align: u32) -> MapNodeLeafLayout {

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
    set_internal_node_layout(key_tydesc.size(), key_tydesc.align())
}

/// The same, from the element's size and alignment.
pub fn set_internal_node_layout(key_size: u32, key_align: u32) -> SetNodeInternalLayout {

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
    set_leaf_node_layout(key_tydesc.size(), key_tydesc.align())
}

/// The same, from the element's size and alignment.
pub fn set_leaf_node_layout(key_size: u32, key_align: u32) -> SetNodeLeafLayout {

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
    let columns: Vec<(u32, u32)> = column_tydescs.iter().map(|tydesc| (tydesc.size, tydesc.align)).collect();
    table_column_offset_of(&columns, column_index, capacity)
}

/// The same, from each column's element size and alignment, for a reader
/// that knows the column types and holds no descriptors.
#[inline]
pub fn table_column_offset_of(
    columns: &[(u32, u32)],
    column_index: usize,
    capacity: IndexRepr,
) -> usize {
    let mut offset = 0usize;
    for (size, align) in &columns[..column_index] {
        offset = align_up_usize(offset, *align);
        offset += (*size as usize) * (capacity as usize);
    }
    align_up_usize(offset, columns[column_index].1)
}

/// Compute total allocation size for a table's data.
///
/// Panics if the total exceeds `u32::MAX`. The allocator takes sizes as `u32`,
/// so a larger table cannot be allocated at all; truncating here would instead
/// allocate a short buffer and let the caller write past it.
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
        let column_bytes = (tydesc.size as usize)
            .checked_mul(capacity as usize)
            .expect("table column size overflow");
        offset = offset.checked_add(column_bytes)
            .expect("table data size overflow");
        max_align = max_align.max(tydesc.align);
    }
    let total = align_up_usize(offset, max_align);
    u32::try_from(total).unwrap_or_else(|_| {
        panic!("table data of {} bytes exceeds the {} byte maximum", total, u32::MAX)
    })
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
            flags: 0,
            size,
            align,
            type_info: TyInfo {
                nothing: TyInfoNothing { unused: 0 },
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
    #[should_panic(expected = "exceeds the")]
    fn test_table_data_allocation_size_rejects_oversized() {
        // 16 bytes per row times 2^28 rows is exactly 2^32, one past what the
        // allocator can express. This used to wrap to 0.
        let td = make_tydesc(16, 8);
        let tydescs: Vec<&TyDesc> = vec![&td];
        let _ = table_data_allocation_size(&tydescs, 1 << 28 as IndexRepr);
    }

    #[test]
    fn test_table_data_allocation_size_allows_largest_valid() {
        // One byte short of the limit still computes.
        let td = make_tydesc(1, 1);
        let tydescs: Vec<&TyDesc> = vec![&td];
        assert_eq!(table_data_allocation_size(&tydescs, u32::MAX as IndexRepr), u32::MAX);
    }

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

#[cfg(kani)]
mod kani_proofs {
    //! Bounded proofs of the layout arithmetic, for every size and alignment
    //! a descriptor can carry; see botdocs/kani.md.

    use super::*;

    /// The pointer size and alignment the node layouts place children by.
    const PTR: u32 = std::mem::size_of::<*const u8>() as u32;

    /// A power of two no larger than a page, which is every alignment a
    /// descriptor can carry.
    fn any_align() -> u32 {
        let align: u32 = kani::any();
        kani::assume(align.is_power_of_two() && align <= 4096);
        align
    }

    /// A size a descriptor can give a value of alignment `align`: a multiple
    /// of it, and small enough that a node of eleven cannot overflow.
    fn any_size(align: u32) -> u32 {
        let size: u32 = kani::any();
        kani::assume(size <= 1 << 20 && size & (align - 1) == 0);
        size
    }

    /// That a part at `offset` follows one ending at `end`, aligned to
    /// `align`, with no more padding between than the alignment needs.
    ///
    /// With every part checked this way, the layout is the unique one the
    /// alignments allow, which is what the runtime, the backends and a
    /// writer of descriptors each compute on their own.
    fn follows(end: u32, offset: u32, align: u32) {
        assert!(offset >= end);
        assert!(offset & (align - 1) == 0);
        assert!(offset - end < align);
    }

    /// `align_up` rounds up to the nearest multiple, and no further.
    ///
    /// Bounded away from the top of `u32`, where `value + align - 1`
    /// overflows; the harness checks that too, by its absence.
    #[kani::proof]
    fn align_up_rounds_up() {
        let align = any_align();
        let value: u32 = kani::any();
        kani::assume(value <= u32::MAX - align);
        follows(value, align_up(value, align), align);
    }

    /// Each payload follows its tag, aligned to the payload.
    #[kani::proof]
    fn payload_offsets_follow_their_tags() {
        let align = any_align();
        follows(1, option_payload_offset(align), align);
        follows(4, enum_payload_offset(align), align);
        let result_align = align.max(PTR);
        follows(1, result_payload_offset(align), result_align);
    }

    /// A size and alignment a descriptor can give a value: the size a
    /// multiple of the alignment, and under a mebibyte.
    fn any_layout() -> TypeLayout {
        let align = any_align();
        TypeLayout { size: any_size(align), align }
    }

    /// Up to four parts are laid out each after the last, aligned, with
    /// only the padding alignment needs; the whole is aligned to the most
    /// aligned part and padded only to that.
    ///
    /// Four is past where a mistake in carrying the offset from one part to
    /// the next would first show. The loops run at most four times.
    #[kani::proof]
    #[kani::unwind(5)]
    fn sequence_pads_minimally() {
        let parts = [any_layout(), any_layout(), any_layout(), any_layout()];
        let len: usize = kani::any();
        kani::assume(len <= parts.len());
        let parts = &parts[..len];

        let mut offsets = [0u32; 4];
        let mut placed = 0;
        let whole = sequence_layout(parts.iter().copied(), |offset| {
            offsets[placed] = offset;
            placed += 1;
        });
        assert!(placed == len);

        let mut end = 0;
        let mut align = 1;
        for (part, offset) in parts.iter().zip(offsets) {
            follows(end, offset, part.align);
            end = offset + part.size;
            align = align.max(part.align);
        }
        assert!(whole.align == align);
        follows(end, whole.size, whole.align);
    }

    /// That a sum's payload follows its tag of `tag_size` bytes at the
    /// payload's alignment, and the whole is padded only to that.
    fn sum_follows(sum: SumLayout, tag_size: u32, payload: TypeLayout) {
        follows(tag_size, sum.payload_offset, payload.align);
        assert!(sum.layout.align == payload.align);
        follows(sum.payload_offset + payload.size, sum.layout.size, sum.layout.align);
    }

    /// An enum of up to three payloads holds its largest after the
    /// discriminant, and each variant's own payload, at the offset its
    /// alignment gives, fits inside it.
    #[kani::proof]
    #[kani::unwind(4)]
    fn enum_pads_minimally() {
        let payloads = [any_layout(), any_layout(), any_layout()];
        let len: usize = kani::any();
        kani::assume(len <= payloads.len());
        let payloads = &payloads[..len];

        let sum = enum_layout_of(payloads.iter().copied());
        let mut largest = TypeLayout { size: 0, align: ENUM_DISCRIMINANT_SIZE };
        for payload in payloads {
            largest.size = largest.size.max(payload.size);
            largest.align = largest.align.max(payload.align);
            let offset = enum_payload_offset(payload.align);
            assert!(offset <= sum.payload_offset);
            assert!(offset + payload.size <= sum.layout.size);
        }
        sum_follows(sum, ENUM_DISCRIMINANT_SIZE, largest);
    }

    /// A `?T` holds its payload after the tag, at the offset the payload
    /// offset function gives.
    #[kani::proof]
    fn option_pads_minimally() {
        let inner = any_layout();
        let sum = option_layout_of(inner);
        assert!(sum.payload_offset == option_payload_offset(inner.align));
        sum_follows(sum, TAG_SIZE, inner);
    }

    /// A `!T` holds the ok value and the error both, each aligned, after the
    /// tag, at the offset the payload offset function gives, and is padded
    /// only as the more aligned of the two needs.
    #[kani::proof]
    fn result_pads_minimally() {
        let ok = any_layout();
        let sum = result_layout_of(ok);
        assert!(sum.payload_offset == result_payload_offset(ok.align));
        let align = ok.align.max(ERROR_LAYOUT.align);
        assert!(sum.layout.align == align);
        follows(TAG_SIZE, sum.payload_offset, align);
        for arm in [ok, ERROR_LAYOUT] {
            assert!(sum.payload_offset & (arm.align - 1) == 0);
            assert!(sum.payload_offset + arm.size <= sum.layout.size);
        }
        let end = sum.payload_offset + ok.size.max(ERROR_LAYOUT.size);
        follows(end, sum.layout.size, align);
    }

    /// The map's internal node: tag, length, keys, then children, each part
    /// after the last and the whole sized to its alignment.
    #[kani::proof]
    fn map_internal_node_pads_minimally() {
        let key_align = any_align();
        let key_size = any_size(key_align);
        let l = map_internal_node_layout(key_size, key_align);
        follows(8, l.keys_offset, key_align);
        let keys_end = l.keys_offset + key_size * MAP_NODE_CAPACITY;
        follows(keys_end, l.child_ptrs_offset, PTR);
        let children_end = l.child_ptrs_offset + PTR * (MAP_NODE_CAPACITY + 1);
        assert!(l.align == key_align.max(PTR));
        follows(children_end, l.size, l.align);
    }

    /// The map's leaf node: tag, length, next leaf, keys, then values.
    #[kani::proof]
    fn map_leaf_node_pads_minimally() {
        let key_align = any_align();
        let key_size = any_size(key_align);
        let value_align = any_align();
        let value_size = any_size(value_align);
        let l = map_leaf_node_layout(key_size, key_align, value_size, value_align);
        follows(8, l.next_leaf_offset, PTR);
        follows(l.next_leaf_offset + PTR, l.keys_offset, key_align);
        let keys_end = l.keys_offset + key_size * MAP_NODE_CAPACITY;
        follows(keys_end, l.values_offset, value_align);
        let values_end = l.values_offset + value_size * MAP_NODE_CAPACITY;
        assert!(l.align == key_align.max(value_align).max(PTR));
        follows(values_end, l.size, l.align);
    }

    /// The set's internal node, laid out as the map's.
    #[kani::proof]
    fn set_internal_node_pads_minimally() {
        let key_align = any_align();
        let key_size = any_size(key_align);
        let l = set_internal_node_layout(key_size, key_align);
        follows(8, l.keys_offset, key_align);
        let keys_end = l.keys_offset + key_size * SET_NODE_CAPACITY;
        follows(keys_end, l.child_ptrs_offset, PTR);
        let children_end = l.child_ptrs_offset + PTR * (SET_NODE_CAPACITY + 1);
        assert!(l.align == key_align.max(PTR));
        follows(children_end, l.size, l.align);
    }

    /// The set's leaf node: tag, length, next leaf, then keys.
    #[kani::proof]
    fn set_leaf_node_pads_minimally() {
        let key_align = any_align();
        let key_size = any_size(key_align);
        let l = set_leaf_node_layout(key_size, key_align);
        follows(8, l.next_leaf_offset, PTR);
        follows(l.next_leaf_offset + PTR, l.keys_offset, key_align);
        let keys_end = l.keys_offset + key_size * SET_NODE_CAPACITY;
        assert!(l.align == key_align.max(PTR));
        follows(keys_end, l.size, l.align);
    }

    /// A table's columns follow one another in its data allocation, each
    /// aligned, and the allocation holds the last and is sized to the whole.
    ///
    /// Three columns of up to a kilobyte each, at a capacity up to a thousand
    /// rows. The alignment checks are masks rather than remainders: a 64-bit
    /// remainder by a symbolic divisor is a circuit the solver runs out of
    /// memory on, three times over. The loop bounds are the column count.
    #[kani::proof]
    #[kani::unwind(4)]
    fn table_columns_follow_one_another() {
        let mut columns = [(0u32, 0u32); 3];
        for column in columns.iter_mut() {
            let align = any_align();
            let size: u32 = kani::any();
            kani::assume(size <= 1024 && size & (align - 1) == 0);
            *column = (size, align);
        }
        let tydescs = columns.map(|(size, align)| tydesc(size, align));
        let tydescs: [&TyDesc; 3] = [&tydescs[0], &tydescs[1], &tydescs[2]];
        let capacity: IndexRepr = kani::any();
        kani::assume(capacity <= 1000);

        let total = table_data_allocation_size(&tydescs, capacity) as usize;
        let align = table_data_alignment(&tydescs) as usize;
        let mut end = 0usize;
        for (i, (size, align)) in columns.iter().enumerate() {
            let offset = table_column_offset_of(&columns, i, capacity);
            let align = *align as usize;
            assert!(offset >= end);
            assert!(offset & (align - 1) == 0);
            assert!(offset - end < align);
            end = offset + *size as usize * capacity as usize;
        }
        if capacity == 0 {
            assert!(total == 0);
        } else {
            assert!(total >= end);
            assert!(total & (align - 1) == 0);
            assert!(total - end < align);
        }
    }

    /// A descriptor of a scalar shape with the given size and alignment.
    fn tydesc(size: u32, align: u32) -> TyDesc {
        TyDesc {
            type_tag: TyTag::U8,
            flags: 0,
            size,
            align,
            type_info: TyInfo { nothing: TyInfoNothing { unused: 0 } },
        }
    }
}
