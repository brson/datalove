//! Type size and alignment computation for frame analysis.

use rmx::prelude::*;
use crate::datalit::tycheck::{Type, TypeAndHeap};
use crate::tycheck::Type as DatafunType;

/// Size and alignment information for a type.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
}

/// Align a value up to the given alignment.
/// Alignment must be a power of 2.
#[inline]
fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Compute the size and alignment of a datafun type.
pub fn compute_datafun_type_layout<'db>(
    db: &'db dyn crate::Db,
    ty: crate::tycheck::TypeAndHeap<'db>,
) -> TypeLayout {
    match ty.ty(db) {
        DatafunType::Datalit(datalit_ty) => {
            compute_type_layout(db, datalit_ty)
        }
        DatafunType::Function(_) => {
            // Function types don't have runtime representation yet.
            // For now treat as zero-sized.
            TypeLayout { size: 0, align: 1 }
        }
        DatafunType::Void => {
            // Void is zero-sized.
            TypeLayout { size: 0, align: 1 }
        }
    }
}

/// Compute the size and alignment of a datalit type.
pub fn compute_type_layout<'db>(
    db: &'db dyn crate::Db,
    ty: &Type<'db>,
) -> TypeLayout {
    use std::mem::{size_of, align_of};

    match ty {
        Type::Bool => TypeLayout { size: 1, align: 1 },
        Type::U8 | Type::I8 => TypeLayout { size: 1, align: 1 },
        Type::U16 | Type::I16 => TypeLayout { size: 2, align: 2 },
        Type::U32 | Type::I32 => TypeLayout { size: 4, align: 4 },
        Type::U64 | Type::I64 => TypeLayout { size: 8, align: 8 },
        Type::F32 => TypeLayout { size: 4, align: 4 },

        // Runtime-allocated types (pointers).
        Type::Int => TypeLayout {
            size: size_of::<datalove_rtdt::Int>() as u32,
            align: align_of::<datalove_rtdt::Int>() as u32,
        },
        Type::String => TypeLayout {
            size: size_of::<datalove_rtdt::String>() as u32,
            align: align_of::<datalove_rtdt::String>() as u32,
        },
        Type::List(_) => TypeLayout {
            size: size_of::<datalove_rtdt::List>() as u32,
            align: align_of::<datalove_rtdt::List>() as u32,
        },
        Type::Map(_) => TypeLayout {
            size: size_of::<datalove_rtdt::Map>() as u32,
            align: align_of::<datalove_rtdt::Map>() as u32,
        },
        Type::Set(_) => TypeLayout {
            size: size_of::<datalove_rtdt::Set>() as u32,
            align: align_of::<datalove_rtdt::Set>() as u32,
        },
        Type::Tensor(_) => TypeLayout {
            size: size_of::<datalove_rtdt::Tensor>() as u32,
            align: align_of::<datalove_rtdt::Tensor>() as u32,
        },
        Type::Data => TypeLayout {
            size: size_of::<datalove_rtdt::Data>() as u32,
            align: align_of::<datalove_rtdt::Data>() as u32,
        },
        Type::Error => TypeLayout {
            size: size_of::<datalove_rtdt::Error>() as u32,
            align: align_of::<datalove_rtdt::Error>() as u32,
        },

        // Tuple: compute layout of all fields.
        Type::AnonTuple(tuple) => {
            compute_tuple_layout(db, &tuple.fields(db))
        }

        // Struct: same as tuple.
        Type::AnonStruct(struct_ty) => {
            let field_types: Vec<_> = struct_ty.fields(db)
                .iter()
                .map(|f| f.ty(db))
                .collect();
            compute_tuple_layout(db, &field_types)
        }

        // Enum: discriminant + max variant size.
        Type::AnonEnum(enum_ty) => {
            compute_enum_layout(db, &enum_ty.variants(db))
        }

        // Option: tag + inner type.
        Type::Option(option_ty) => {
            let inner = option_ty.inner_type(db);
            // inner is datalit::TypeAndHeap, so compute its layout directly.
            let inner_layout = compute_type_layout(db, inner.ty(db));
            compute_option_layout(inner_layout)
        }

        // Result: tag + max(ok, error).
        Type::Result(result_ty) => {
            let ok = result_ty.inner_type(db);
            // ok is datalit::TypeAndHeap, so compute its layout directly.
            let ok_layout = compute_type_layout(db, ok.ty(db));
            compute_result_layout(ok_layout)
        }
    }
}

/// Compute layout for a tuple (or struct).
fn compute_tuple_layout<'db>(
    db: &'db dyn crate::Db,
    fields: &[TypeAndHeap<'db>],
) -> TypeLayout {
    let mut offset = 0u32;
    let mut max_align = 1u32;

    for field in fields {
        // field is datalit::TypeAndHeap, so compute its layout directly.
        let field_layout = compute_type_layout(db, field.ty(db));

        // Align offset to field's alignment.
        offset = align_up(offset, field_layout.align);

        // Advance by field size.
        offset += field_layout.size;

        // Track max alignment.
        max_align = max_align.max(field_layout.align);
    }

    // Total size must be aligned to max alignment.
    let size = align_up(offset, max_align);

    TypeLayout { size, align: max_align }
}

/// Compute layout for an enum.
fn compute_enum_layout<'db>(
    db: &'db dyn crate::Db,
    variants: &[crate::datalit::tycheck::TypeEnumVariant<'db>],
) -> TypeLayout {
    let discriminant_size = 4u32; // u32
    let discriminant_align = 4u32;

    let mut max_variant_size = 0u32;
    let mut max_variant_align = discriminant_align;

    for variant in variants {
        if let Some(payload) = variant.payload(db) {
            // payload is datalit::TypeAndHeap, so compute its layout directly.
            let payload_layout = compute_type_layout(db, payload.ty(db));
            max_variant_size = max_variant_size.max(payload_layout.size);
            max_variant_align = max_variant_align.max(payload_layout.align);
        }
    }

    // Payload starts after discriminant, aligned to max variant alignment.
    let payload_offset = align_up(discriminant_size, max_variant_align);

    // Total size is payload offset + max variant size, aligned to max alignment.
    let size = align_up(payload_offset + max_variant_size, max_variant_align);

    TypeLayout { size, align: max_variant_align }
}

/// Compute layout for Option<T>.
fn compute_option_layout(inner_layout: TypeLayout) -> TypeLayout {
    let tag_size = 1u32; // u8
    let tag_align = 1u32;

    // Payload starts after tag, aligned to inner alignment.
    let payload_offset = align_up(tag_size, inner_layout.align);

    // Overall alignment is max of tag and inner alignment.
    let overall_align = tag_align.max(inner_layout.align);

    // Total size is payload offset + inner size, aligned to overall alignment.
    let size = align_up(payload_offset + inner_layout.size, overall_align);

    TypeLayout { size, align: overall_align }
}

/// Compute layout for Result<T>.
fn compute_result_layout(ok_layout: TypeLayout) -> TypeLayout {
    use std::mem::{size_of, align_of};

    let tag_size = 1u32; // u8
    let tag_align = 1u32;

    // Error type layout: usize + pointer.
    let error_size = (size_of::<usize>() + size_of::<*const datalove_rtdt::TyDesc>()) as u32;
    let error_align = align_of::<usize>().max(align_of::<*const datalove_rtdt::TyDesc>()) as u32;

    // Payload must accommodate the larger of Ok and Err variants.
    let max_payload_size = ok_layout.size.max(error_size);
    let max_payload_align = ok_layout.align.max(error_align);

    // Payload starts after tag, aligned to max payload alignment.
    let payload_offset = align_up(tag_size, max_payload_align);

    // Overall alignment is max of tag and payload alignment.
    let overall_align = tag_align.max(max_payload_align);

    // Total size is payload offset + max payload size, aligned to overall alignment.
    let size = align_up(payload_offset + max_payload_size, overall_align);

    TypeLayout { size, align: overall_align }
}
