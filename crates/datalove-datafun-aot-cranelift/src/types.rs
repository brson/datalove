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

/// Tests that verify AOT layout calculations match rtdt exactly.
///
/// These tests ensure ABI compatibility between compiled code and the runtime.
#[cfg(test)]
mod rtdt_compat_tests {
    use super::*;

    // Verify our layouts match Rust's std::mem for rtdt types.

    #[test]
    fn test_int_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::Int).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Int>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Int>() as u32);
    }

    #[test]
    fn test_string_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::String).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::String>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::String>() as u32);
    }

    #[test]
    fn test_list_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::List(Box::new(IrType::U32))).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::List>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::List>() as u32);
    }

    #[test]
    fn test_set_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::Set(Box::new(IrType::U32))).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Set>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Set>() as u32);
    }

    #[test]
    fn test_map_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::Map(
            Box::new(IrType::String),
            Box::new(IrType::U32),
        )).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Map>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Map>() as u32);
    }

    #[test]
    fn test_tensor_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::Tensor(Box::new(IrType::F32), 2)).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Tensor>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Tensor>() as u32);
    }

    #[test]
    fn test_data_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::Data).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Data>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Data>() as u32);
    }

    #[test]
    fn test_error_matches_rtdt() {
        let our_layout = ir_type_to_cranelift(&IrType::Error).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Error>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Error>() as u32);
    }

    // Verify scalar types match their Rust equivalents.

    #[test]
    fn test_bool_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::Bool).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::Bool>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::Bool>() as u32);
    }

    #[test]
    fn test_u8_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::U8).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::U8>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::U8>() as u32);
    }

    #[test]
    fn test_u16_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::U16).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::U16>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::U16>() as u32);
    }

    #[test]
    fn test_u32_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::U32).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::U32>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::U32>() as u32);
    }

    #[test]
    fn test_u64_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::U64).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::U64>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::U64>() as u32);
    }

    #[test]
    fn test_i8_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::I8).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::I8>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::I8>() as u32);
    }

    #[test]
    fn test_i16_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::I16).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::I16>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::I16>() as u32);
    }

    #[test]
    fn test_i32_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::I32).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::I32>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::I32>() as u32);
    }

    #[test]
    fn test_i64_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::I64).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::I64>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::I64>() as u32);
    }

    #[test]
    fn test_f32_matches_rust() {
        let our_layout = ir_type_to_cranelift(&IrType::F32).layout();
        assert_eq!(our_layout.size, std::mem::size_of::<rtdt::F32>() as u32);
        assert_eq!(our_layout.align, std::mem::align_of::<rtdt::F32>() as u32);
    }

    // Verify Option layout matches rtdt::layout computation.

    #[test]
    fn test_option_u32_matches_rtdt_layout() {
        let our_layout = ir_type_to_cranelift(&IrType::Option(Box::new(IrType::U32))).layout();

        // Manually compute what rtdt::layout::compute_option_layout would give.
        // Option<u32>: tag (u8) at 0, payload at align_up(1, 4) = 4, size = 4
        // Total: 4 + 4 = 8, align = 4
        let inner_size = 4u32;
        let inner_align = 4u32;
        let payload_offset = align_up(1, inner_align);
        let expected_size = align_up(payload_offset + inner_size, inner_align);

        assert_eq!(our_layout.size, expected_size);
        assert_eq!(our_layout.align, inner_align);
    }

    #[test]
    fn test_option_u64_matches_rtdt_layout() {
        let our_layout = ir_type_to_cranelift(&IrType::Option(Box::new(IrType::U64))).layout();

        // Option<u64>: tag (u8) at 0, payload at align_up(1, 8) = 8, size = 8
        // Total: 8 + 8 = 16, align = 8
        let inner_size = 8u32;
        let inner_align = 8u32;
        let payload_offset = align_up(1, inner_align);
        let expected_size = align_up(payload_offset + inner_size, inner_align);

        assert_eq!(our_layout.size, expected_size);
        assert_eq!(our_layout.align, inner_align);
    }

    #[test]
    fn test_option_string_matches_rtdt_layout() {
        let our_layout = ir_type_to_cranelift(&IrType::Option(Box::new(IrType::String))).layout();

        // Option<String>: tag (u8) at 0, payload at align_up(1, 8) = 8
        // String is 16 bytes, so total = 8 + 16 = 24, align = 8
        let inner_size = std::mem::size_of::<rtdt::String>() as u32;
        let inner_align = std::mem::align_of::<rtdt::String>() as u32;
        let payload_offset = align_up(1, inner_align);
        let expected_size = align_up(payload_offset + inner_size, inner_align);

        assert_eq!(our_layout.size, expected_size);
        assert_eq!(our_layout.align, inner_align);
    }

    // Verify Result layout matches rtdt::layout computation.

    #[test]
    fn test_result_u32_matches_rtdt_layout() {
        let our_layout = ir_type_to_cranelift(&IrType::Result(Box::new(IrType::U32))).layout();

        // Result<u32>: tag (u8), payload = max(u32, Error)
        // Error is 16 bytes with 8-byte align
        let ok_size = 4u32;
        let ok_align = 4u32;
        let error_size = std::mem::size_of::<rtdt::Error>() as u32;
        let error_align = std::mem::align_of::<rtdt::Error>() as u32;

        let max_payload_size = ok_size.max(error_size);
        let max_payload_align = ok_align.max(error_align);
        let payload_offset = align_up(1, max_payload_align);
        let expected_size = align_up(payload_offset + max_payload_size, max_payload_align);

        assert_eq!(our_layout.size, expected_size);
        assert_eq!(our_layout.align, max_payload_align);
    }

    #[test]
    fn test_result_string_matches_rtdt_layout() {
        let our_layout = ir_type_to_cranelift(&IrType::Result(Box::new(IrType::String))).layout();

        let ok_size = std::mem::size_of::<rtdt::String>() as u32;
        let ok_align = std::mem::align_of::<rtdt::String>() as u32;
        let error_size = std::mem::size_of::<rtdt::Error>() as u32;
        let error_align = std::mem::align_of::<rtdt::Error>() as u32;

        let max_payload_size = ok_size.max(error_size);
        let max_payload_align = ok_align.max(error_align);
        let payload_offset = align_up(1, max_payload_align);
        let expected_size = align_up(payload_offset + max_payload_size, max_payload_align);

        assert_eq!(our_layout.size, expected_size);
        assert_eq!(our_layout.align, max_payload_align);
    }

    // Verify tuple layout algorithm.

    #[test]
    fn test_tuple_empty() {
        let our_layout = ir_type_to_cranelift(&IrType::Tuple(vec![])).layout();
        assert_eq!(our_layout.size, 0);
        assert_eq!(our_layout.align, 1);
    }

    #[test]
    fn test_tuple_single_field() {
        let our_layout = ir_type_to_cranelift(&IrType::Tuple(vec![IrType::U64])).layout();
        assert_eq!(our_layout.size, 8);
        assert_eq!(our_layout.align, 8);
    }

    #[test]
    fn test_tuple_mixed_alignment() {
        // (u8, u32, u8, u64): layout should be
        // u8 at 0, u32 at 4, u8 at 8, u64 at 16, total 24
        let our_layout = ir_type_to_cranelift(&IrType::Tuple(vec![
            IrType::U8,
            IrType::U32,
            IrType::U8,
            IrType::U64,
        ])).layout();

        assert_eq!(our_layout.align, 8);
        // 0: u8 (1), pad to 4, 4: u32 (4), 8: u8 (1), pad to 16, 16: u64 (8) = 24
        assert_eq!(our_layout.size, 24);
    }

    #[test]
    fn test_tuple_field_offsets() {
        let fields = vec![IrType::U8, IrType::U32, IrType::U8, IrType::U64];
        let offsets = compute_tuple_field_offsets(&fields);

        assert_eq!(offsets[0], 0);  // u8 at 0
        assert_eq!(offsets[1], 4);  // u32 at 4 (aligned)
        assert_eq!(offsets[2], 8);  // u8 at 8
        assert_eq!(offsets[3], 16); // u64 at 16 (aligned)
    }

    // Verify enum layout algorithm.

    #[test]
    fn test_enum_no_payloads() {
        let our_layout = ir_type_to_cranelift(&IrType::Enum(vec![
            ("A".into(), None),
            ("B".into(), None),
            ("C".into(), None),
        ])).layout();

        // Just discriminant (u32), no payload
        assert_eq!(our_layout.size, 4);
        assert_eq!(our_layout.align, 4);
    }

    #[test]
    fn test_enum_with_payloads() {
        let our_layout = ir_type_to_cranelift(&IrType::Enum(vec![
            ("None".into(), None),
            ("Some".into(), Some(IrType::U64)),
        ])).layout();

        // discriminant (u32) + padding + u64 payload
        // discriminant at 0 (4 bytes), payload at align_up(4, 8) = 8
        // Total: 8 + 8 = 16, align = 8
        assert_eq!(our_layout.size, 16);
        assert_eq!(our_layout.align, 8);
    }

    #[test]
    fn test_enum_variant_offsets() {
        let variants = vec![
            ("None".into(), None),
            ("SomeU32".into(), Some(IrType::U32)),
            ("SomeU64".into(), Some(IrType::U64)),
        ];
        let offsets = compute_enum_variant_offsets(&variants);

        assert_eq!(offsets[0], 0); // No payload
        assert_eq!(offsets[1], 4); // u32 payload at align_up(4, 4) = 4
        assert_eq!(offsets[2], 8); // u64 payload at align_up(4, 8) = 8
    }

    // Verify nested composite types.

    #[test]
    fn test_nested_option_tuple() {
        // Option<(u32, u64)>
        let tuple_layout = ir_type_to_cranelift(&IrType::Tuple(vec![IrType::U32, IrType::U64])).layout();
        let option_layout = ir_type_to_cranelift(&IrType::Option(Box::new(
            IrType::Tuple(vec![IrType::U32, IrType::U64])
        ))).layout();

        // Tuple is 16 bytes with 8-byte align
        assert_eq!(tuple_layout.size, 16);
        assert_eq!(tuple_layout.align, 8);

        // Option: tag (1) + pad to 8 + tuple (16) = 24
        let payload_offset = align_up(1, tuple_layout.align);
        let expected_size = align_up(payload_offset + tuple_layout.size, tuple_layout.align);
        assert_eq!(option_layout.size, expected_size);
        assert_eq!(option_layout.align, tuple_layout.align);
    }

    #[test]
    fn test_tuple_with_string() {
        // (String, u32)
        let our_layout = ir_type_to_cranelift(&IrType::Tuple(vec![
            IrType::String,
            IrType::U32,
        ])).layout();

        // String at 0 (16 bytes), u32 at 16 (4 bytes), total 20 -> aligned to 24
        assert_eq!(our_layout.size, 24);
        assert_eq!(our_layout.align, 8);
    }
}
