//! Type mapping from IR types to C types.

use datalove_datafun_ir::IrType;
use datalove_rtdt as rtdt;

/// Size and alignment for a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
}

/// Representation for an IR type in C.
#[derive(Debug, Clone)]
pub enum CRepr {
    /// Fits in a single C scalar.
    Scalar(String),
    /// Multi-word aggregate stored in memory.
    Aggregate(TypeLayout),
}

impl CRepr {
    /// Get the layout for this representation.
    pub fn layout(&self) -> TypeLayout {
        match self {
            CRepr::Scalar(ty) => {
                let (size, align) = scalar_type_layout(ty);
                TypeLayout { size, align }
            }
            CRepr::Aggregate(layout) => *layout,
        }
    }

    /// Check if this is a scalar type.
    pub fn is_scalar(&self) -> bool {
        matches!(self, CRepr::Scalar(_))
    }
}

/// Get the size and alignment for a C scalar type.
fn scalar_type_layout(ty: &str) -> (u32, u32) {
    match ty {
        "bool_t" | "uint8_t" | "int8_t" => (1, 1),
        "uint16_t" | "int16_t" => (2, 2),
        "uint32_t" | "int32_t" | "float" => (4, 4),
        "uint64_t" | "int64_t" | "double" => (8, 8),
        "index_t" => {
            #[cfg(not(feature = "index-64"))]
            { (4, 4) }
            #[cfg(feature = "index-64")]
            { (8, 8) }
        }
        "offset_t" => {
            #[cfg(not(feature = "index-64"))]
            { (4, 4) }
            #[cfg(feature = "index-64")]
            { (8, 8) }
        }
        _ => (8, 8), // Assume pointer-sized for unknown types
    }
}

/// Align a value up to the given alignment.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Get the C representation for an IR type.
pub fn ir_type_to_crepr(ty: &IrType) -> CRepr {
    match ty {
        // Zero-sized type.
        IrType::Unit => CRepr::Aggregate(TypeLayout { size: 0, align: 1 }),

        // Scalars that fit in registers.
        IrType::Bool => CRepr::Scalar("bool_t".into()),
        IrType::U8 => CRepr::Scalar("uint8_t".into()),
        IrType::I8 => CRepr::Scalar("int8_t".into()),
        IrType::U16 => CRepr::Scalar("uint16_t".into()),
        IrType::I16 => CRepr::Scalar("int16_t".into()),
        IrType::U32 => CRepr::Scalar("uint32_t".into()),
        IrType::I32 => CRepr::Scalar("int32_t".into()),
        IrType::U64 => CRepr::Scalar("uint64_t".into()),
        IrType::I64 => CRepr::Scalar("int64_t".into()),
        IrType::Index => CRepr::Scalar("index_t".into()),
        IrType::Offset => CRepr::Scalar("offset_t".into()),
        IrType::F32 => CRepr::Scalar("float".into()),
        IrType::F64 => CRepr::Scalar("double".into()),

        // Runtime types with fixed layouts from rtdt.
        IrType::Int => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Int>() as u32,
            align: std::mem::align_of::<rtdt::Int>() as u32,
        }),
        IrType::String => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
        }),
        IrType::Data => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Data>() as u32,
            align: std::mem::align_of::<rtdt::Data>() as u32,
        }),
        IrType::Error => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Error>() as u32,
            align: std::mem::align_of::<rtdt::Error>() as u32,
        }),
        IrType::List(_) => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
        }),
        IrType::Set(_) => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
        }),
        IrType::Map(_, _) => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
        }),
        IrType::Tensor(_, _) => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Tensor>() as u32,
            align: std::mem::align_of::<rtdt::Tensor>() as u32,
        }),
        IrType::Table(_) => CRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Table>() as u32,
            align: std::mem::align_of::<rtdt::Table>() as u32,
        }),

        // Composite types with computed layouts.
        IrType::Tuple(fields) => {
            let layout = compute_tuple_layout(fields);
            CRepr::Aggregate(layout)
        }
        IrType::Struct(fields) => {
            let field_types: Vec<_> = fields.iter().map(|(_, ty)| ty.clone()).collect();
            let layout = compute_tuple_layout(&field_types);
            CRepr::Aggregate(layout)
        }
        IrType::Enum(variants) => {
            let layout = compute_enum_layout(variants);
            CRepr::Aggregate(layout)
        }
        IrType::Atom(_) => CRepr::Aggregate(TypeLayout { size: 0, align: 1 }),
        IrType::Term(_, payload) => ir_type_to_crepr(payload),
        IrType::Option(inner) => {
            let layout = compute_option_layout(inner);
            CRepr::Aggregate(layout)
        }
        IrType::Result(ok_ty) => {
            let layout = compute_result_layout(ok_ty);
            CRepr::Aggregate(layout)
        }

        // Ref is a pointer to the inner type.
        IrType::Ref(_) => CRepr::Scalar("void*".into()),
    }
}

/// Compute layout for a tuple type.
pub fn compute_tuple_layout(fields: &[IrType]) -> TypeLayout {
    let mut offset = 0u32;
    let mut max_align = 1u32;

    for field in fields {
        let field_layout = ir_type_to_crepr(field).layout();
        offset = align_up(offset, field_layout.align);
        offset += field_layout.size;
        max_align = max_align.max(field_layout.align);
    }

    TypeLayout {
        size: align_up(offset, max_align),
        align: max_align,
    }
}

/// Compute layout for an enum type.
fn compute_enum_layout(variants: &[(String, Option<IrType>)]) -> TypeLayout {
    let discriminant_size = 4u32;
    let discriminant_align = 4u32;

    let mut max_payload_size = 0u32;
    let mut max_payload_align = discriminant_align;

    for (_, payload) in variants {
        if let Some(payload_ty) = payload {
            let payload_layout = ir_type_to_crepr(payload_ty).layout();
            max_payload_size = max_payload_size.max(payload_layout.size);
            max_payload_align = max_payload_align.max(payload_layout.align);
        }
    }

    let payload_offset = align_up(discriminant_size, max_payload_align);
    let total_size = align_up(payload_offset + max_payload_size, max_payload_align);

    TypeLayout {
        size: total_size,
        align: max_payload_align,
    }
}

/// Compute layout for an Option type.
pub fn compute_option_layout(inner: &IrType) -> TypeLayout {
    let tag_size = 1u32;
    let inner_layout = ir_type_to_crepr(inner).layout();

    let payload_offset = align_up(tag_size, inner_layout.align);
    let overall_align = inner_layout.align.max(1);
    let total_size = align_up(payload_offset + inner_layout.size, overall_align);

    TypeLayout {
        size: total_size,
        align: overall_align,
    }
}

/// Compute layout for a Result type.
pub fn compute_result_layout(ok_ty: &IrType) -> TypeLayout {
    let tag_size = 1u32;
    let ok_layout = ir_type_to_crepr(ok_ty).layout();

    // Error type layout.
    let error_size = std::mem::size_of::<rtdt::Error>() as u32;
    let error_align = std::mem::align_of::<rtdt::Error>() as u32;

    let max_payload_size = ok_layout.size.max(error_size);
    let max_payload_align = ok_layout.align.max(error_align);

    let payload_offset = align_up(tag_size, max_payload_align);
    let overall_align = max_payload_align.max(1);
    let total_size = align_up(payload_offset + max_payload_size, overall_align);

    TypeLayout {
        size: total_size,
        align: overall_align,
    }
}

/// Get tuple field offsets for an IR tuple type.
pub fn compute_tuple_field_offsets(fields: &[IrType]) -> Vec<u32> {
    let mut offsets = Vec::with_capacity(fields.len());
    let mut offset = 0u32;

    for field in fields {
        let field_layout = ir_type_to_crepr(field).layout();
        offset = align_up(offset, field_layout.align);
        offsets.push(offset);
        offset += field_layout.size;
    }

    offsets
}

/// Get the payload offset of each enum variant.
///
/// Each variant's payload sits at the first offset past the discriminant that
/// satisfies that variant's own alignment, so variants with differing alignment
/// have differing offsets. Variants without a payload get 0. This must agree
/// with `rtdt::layout::compute_enum_layout`, which the runtime uses to find
/// payloads, and with the offsets `emit_enum_variant` writes.
pub fn compute_enum_variant_offsets(variants: &[(String, Option<IrType>)]) -> Vec<u32> {
    let discriminant_size = 4u32;

    variants.iter().map(|(_, payload)| {
        match payload {
            Some(payload_ty) => {
                let payload_layout = ir_type_to_crepr(payload_ty).layout();
                align_up(discriminant_size, payload_layout.align)
            }
            None => 0,
        }
    }).collect()
}

/// Get the C type name for an IR type.
pub fn ir_type_to_c(ty: &IrType) -> String {
    match ir_type_to_crepr(ty) {
        CRepr::Scalar(name) => name,
        CRepr::Aggregate(_) => "void".into(), // Aggregates are passed by pointer
    }
}

/// Check if a type uses sret (structure return) convention.
pub fn uses_sret(ty: &IrType) -> bool {
    match ty {
        IrType::Unit => false,
        _ => matches!(ir_type_to_crepr(ty), CRepr::Aggregate(_)),
    }
}

/// Pointer size in bytes.
pub const PTR_SIZE: u32 = 8;

/// Pointer alignment in bytes.
pub const PTR_ALIGN: u32 = 8;
