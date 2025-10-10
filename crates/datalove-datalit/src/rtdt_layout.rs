//! Layout computation for datalit runtime types.

use super::rtdt::*;

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

/// Compute the memory layout for a struct type.
///
/// Struct layout is identical to tuple layout - fields are laid out
/// sequentially with proper alignment, and the struct is padded to
/// its natural alignment.
///
/// # Safety
/// The tydesc must point to a valid TyDesc with type_tag = TyTag::Struct.
pub unsafe fn compute_struct_layout(tydesc: *const TyDesc) -> StructLayout {
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
