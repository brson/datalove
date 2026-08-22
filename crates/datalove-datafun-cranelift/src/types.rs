//! Type mapping from IR types to Cranelift types.
//!
//! Maps `IrType` to Cranelift's type system while preserving exact memory
//! layouts as defined in `datalove-rtdt`.

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::Type as CraneliftType;
use datalove_datafun_ir::IrType;
use datalove_datafun_ir::layout as ir_layout;

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
/// Round `value` up to a multiple of `align`.
pub fn align_up(value: u32, align: u32) -> u32 {
    ir_layout::align_up(value, align)
}

/// Get the Cranelift representation for an IR type.
///
/// This matches the exact layouts defined in `datalove-rtdt`.
pub fn ir_type_to_cranelift(ty: &IrType) -> CraneliftRepr {
    match ty {
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
        #[cfg(not(feature = "index-64"))]
        IrType::Index => CraneliftRepr::Scalar(cl_types::I32),
        #[cfg(feature = "index-64")]
        IrType::Index => CraneliftRepr::Scalar(cl_types::I64),
        #[cfg(not(feature = "index-64"))]
        IrType::Offset => CraneliftRepr::Scalar(cl_types::I32),
        #[cfg(feature = "index-64")]
        IrType::Offset => CraneliftRepr::Scalar(cl_types::I64),
        IrType::F32 => CraneliftRepr::Scalar(cl_types::F32),
        IrType::F64 => CraneliftRepr::Scalar(cl_types::F64),

        // A term is transparent over its payload, so it keeps the payload's
        // representation rather than becoming an aggregate.
        IrType::Term(_, payload) => ir_type_to_cranelift(payload),

        // Ref is a pointer to the inner type.
        IrType::Ref(_) => CraneliftRepr::Scalar(PTR_TYPE),

        // Everything else is held in memory, laid out by the shared authority.
        _ => CraneliftRepr::Aggregate(to_layout(ir_layout::layout_of(ty))),
    }
}

/// Byte offset of each field of a tuple or struct.
pub fn compute_tuple_field_offsets(fields: &[IrType]) -> Vec<u32> {
    ir_layout::aggregate_field_offsets(fields)
}

/// Byte offset of each enum variant payload.
pub fn compute_enum_variant_offsets(variants: &[(String, Option<IrType>)]) -> Vec<u32> {
    ir_layout::enum_variant_offsets(variants)
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
        use datalove_rtdt::Int;
        let layout = ir_type_to_cranelift(&IrType::Int).layout();
        assert_eq!(layout.size, std::mem::size_of::<Int>() as u32);
        assert_eq!(layout.align, std::mem::align_of::<Int>() as u32);
    }

    #[test]
    fn test_string_layout() {
        use datalove_rtdt::String;
        let layout = ir_type_to_cranelift(&IrType::String).layout();
        assert_eq!(layout.size, std::mem::size_of::<String>() as u32);
        assert_eq!(layout.align, std::mem::align_of::<String>() as u32);
    }

    #[test]
    fn test_list_layout() {
        use datalove_rtdt::List;
        let layout = ir_type_to_cranelift(&IrType::List(Box::new(IrType::U32))).layout();
        assert_eq!(layout.size, std::mem::size_of::<List>() as u32);
        assert_eq!(layout.align, std::mem::align_of::<List>() as u32);
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

/// Bridge the shared layout type into this backend's own.
fn to_layout(l: ir_layout::TypeLayout) -> TypeLayout {
    TypeLayout { size: l.size, align: l.align }
}
