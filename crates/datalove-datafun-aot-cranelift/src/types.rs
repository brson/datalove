//! Type mapping from IR types to Cranelift types.
//!
//! Maps `IrType` to Cranelift's type system while preserving exact memory
//! layouts as defined in `datalove-rtdt`.

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::Type as CraneliftType;
use datalove_datafun_ir::IrType;
use datalove_rtdt as rtdt;

/// Size and alignment for a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
}

/// Cranelift representation for an IR type.
///
/// Types are either:
/// - Scalar: fits in a single Cranelift value (register)
/// - Aggregate: multi-word, stored in memory, passed by pointer
#[derive(Debug, Clone)]
pub enum CraneliftRepr {
    /// Fits in a single Cranelift value.
    Scalar(CraneliftType),
    /// Multi-word aggregate stored in memory.
    Aggregate(TypeLayout),
}

impl CraneliftRepr {
    /// Get the layout for this representation.
    pub fn layout(&self) -> TypeLayout {
        match self {
            CraneliftRepr::Scalar(ty) => TypeLayout {
                size: ty.bytes(),
                align: ty.bytes().max(1),
            },
            CraneliftRepr::Aggregate(layout) => *layout,
        }
    }

    /// Check if this is a scalar type.
    pub fn is_scalar(&self) -> bool {
        matches!(self, CraneliftRepr::Scalar(_))
    }
}

/// Align a value up to the given alignment.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Get the Cranelift representation for an IR type.
///
/// This matches the exact layouts defined in `datalove-rtdt`.
pub fn ir_type_to_cranelift(ty: &IrType) -> CraneliftRepr {
    match ty {
        // Zero-sized type.
        IrType::Unit => CraneliftRepr::Aggregate(TypeLayout { size: 0, align: 1 }),

        // Scalars that fit in registers.
        IrType::Bool => CraneliftRepr::Scalar(cl_types::I8),
        IrType::U8 => CraneliftRepr::Scalar(cl_types::I8),
        IrType::I8 => CraneliftRepr::Scalar(cl_types::I8),
        IrType::U16 => CraneliftRepr::Scalar(cl_types::I16),
        IrType::I16 => CraneliftRepr::Scalar(cl_types::I16),
        IrType::U32 => CraneliftRepr::Scalar(cl_types::I32),
        IrType::I32 => CraneliftRepr::Scalar(cl_types::I32),
        IrType::U64 => CraneliftRepr::Scalar(cl_types::I64),
        IrType::I64 => CraneliftRepr::Scalar(cl_types::I64),
        IrType::F32 => CraneliftRepr::Scalar(cl_types::F32),

        // Runtime types with fixed layouts from rtdt.
        IrType::Int => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Int>() as u32,
            align: std::mem::align_of::<rtdt::Int>() as u32,
        }),
        IrType::String => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
        }),
        IrType::Data => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Data>() as u32,
            align: std::mem::align_of::<rtdt::Data>() as u32,
        }),
        IrType::Error => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Error>() as u32,
            align: std::mem::align_of::<rtdt::Error>() as u32,
        }),
        IrType::List(_) => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
        }),
        IrType::Set(_) => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
        }),
        IrType::Map(_, _) => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
        }),
        IrType::Tensor(_, _) => CraneliftRepr::Aggregate(TypeLayout {
            size: std::mem::size_of::<rtdt::Tensor>() as u32,
            align: std::mem::align_of::<rtdt::Tensor>() as u32,
        }),

        // Composite types with computed layouts.
        IrType::Tuple(fields) => {
            let layout = compute_tuple_layout(fields);
            CraneliftRepr::Aggregate(layout)
        }
        IrType::Struct(fields) => {
            let field_types: Vec<_> = fields.iter().map(|(_, ty)| ty.clone()).collect();
            let layout = compute_tuple_layout(&field_types);
            CraneliftRepr::Aggregate(layout)
        }
        IrType::Enum(variants) => {
            let layout = compute_enum_layout(variants);
            CraneliftRepr::Aggregate(layout)
        }
        IrType::Option(inner) => {
            let layout = compute_option_layout(inner);
            CraneliftRepr::Aggregate(layout)
        }
        IrType::Result(ok_ty) => {
            let layout = compute_result_layout(ok_ty);
            CraneliftRepr::Aggregate(layout)
        }
    }
}

/// Compute layout for a tuple type.
fn compute_tuple_layout(fields: &[IrType]) -> TypeLayout {
    let mut offset = 0u32;
    let mut max_align = 1u32;

    for field in fields {
        let field_layout = ir_type_to_cranelift(field).layout();
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
            let payload_layout = ir_type_to_cranelift(payload_ty).layout();
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
fn compute_option_layout(inner: &IrType) -> TypeLayout {
    let tag_size = 1u32;
    let inner_layout = ir_type_to_cranelift(inner).layout();

    let payload_offset = align_up(tag_size, inner_layout.align);
    let overall_align = inner_layout.align.max(1);
    let total_size = align_up(payload_offset + inner_layout.size, overall_align);

    TypeLayout {
        size: total_size,
        align: overall_align,
    }
}

/// Compute layout for a Result type.
fn compute_result_layout(ok_ty: &IrType) -> TypeLayout {
    let tag_size = 1u32;
    let ok_layout = ir_type_to_cranelift(ok_ty).layout();

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
        let field_layout = ir_type_to_cranelift(field).layout();
        offset = align_up(offset, field_layout.align);
        offsets.push(offset);
        offset += field_layout.size;
    }

    offsets
}

/// Get enum variant payload offsets.
pub fn compute_enum_variant_offsets(variants: &[(String, Option<IrType>)]) -> Vec<u32> {
    let discriminant_size = 4u32;

    variants.iter().map(|(_, payload)| {
        if let Some(payload_ty) = payload {
            let payload_layout = ir_type_to_cranelift(payload_ty).layout();
            align_up(discriminant_size, payload_layout.align)
        } else {
            0
        }
    }).collect()
}

/// Pointer type for the target (64-bit).
pub const PTR_TYPE: CraneliftType = cl_types::I64;

/// Pointer size in bytes.
pub const PTR_SIZE: u32 = 8;

/// Pointer alignment in bytes.
pub const PTR_ALIGN: u32 = 8;

/// Get the size of an IR type in bytes.
pub fn ir_type_size(ty: &IrType) -> u32 {
    ir_type_to_cranelift(ty).layout().size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_types() {
        assert!(ir_type_to_cranelift(&IrType::Bool).is_scalar());
        assert!(ir_type_to_cranelift(&IrType::U32).is_scalar());
        assert!(ir_type_to_cranelift(&IrType::I64).is_scalar());
        assert!(ir_type_to_cranelift(&IrType::F32).is_scalar());
    }

    #[test]
    fn test_aggregate_types() {
        assert!(!ir_type_to_cranelift(&IrType::Int).is_scalar());
        assert!(!ir_type_to_cranelift(&IrType::String).is_scalar());
        assert!(!ir_type_to_cranelift(&IrType::List(Box::new(IrType::U32))).is_scalar());
    }

    #[test]
    fn test_int_layout() {
        let layout = ir_type_to_cranelift(&IrType::Int).layout();
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_string_layout() {
        let layout = ir_type_to_cranelift(&IrType::String).layout();
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_list_layout() {
        let layout = ir_type_to_cranelift(&IrType::List(Box::new(IrType::U32))).layout();
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_map_layout() {
        let layout = ir_type_to_cranelift(&IrType::Map(
            Box::new(IrType::String),
            Box::new(IrType::U32),
        )).layout();
        // Map is { root: *MapNode, len: u32 } = 12 bytes, but 8-byte aligned
        assert_eq!(layout.size, 16); // Padded to alignment
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_tuple_layout() {
        // (u32, u64) should be: u32 at 0, padding, u64 at 8, total 16
        let layout = ir_type_to_cranelift(&IrType::Tuple(vec![IrType::U32, IrType::U64])).layout();
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn test_option_layout() {
        // Option<u32>: tag (1) + padding (3) + u32 (4) = 8
        let layout = ir_type_to_cranelift(&IrType::Option(Box::new(IrType::U32))).layout();
        assert_eq!(layout.size, 8);
        assert_eq!(layout.align, 4);
    }
}
