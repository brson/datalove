//! Type mapping from IR types to C types.

use datalove_datafun_ir::IrType;
use datalove_datafun_ir::layout as ir_layout;

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

/// Get the C representation for an IR type.
pub fn ir_type_to_crepr(ty: &IrType) -> CRepr {
    match ty {
        // Zero-sized type.

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

        // A term is transparent over its payload, so it keeps the payload's
        // representation rather than becoming an aggregate.
        IrType::Term(_, payload) => ir_type_to_crepr(payload),

        IrType::Ref(_) => CRepr::Scalar("void*".into()),

        // Everything else is held in memory, laid out by the shared authority.
        _ => CRepr::Aggregate(to_layout(ir_layout::layout_of(ty))),
    }
}

/// Layout of an option type.
pub fn compute_option_layout(inner: &IrType) -> TypeLayout {
    to_layout(ir_layout::option_layout(inner))
}

/// Layout of a result type.
pub fn compute_result_layout(ok_ty: &IrType) -> TypeLayout {
    to_layout(ir_layout::result_layout(ok_ty))
}

/// Byte offset of each field of a tuple or struct.
pub fn compute_tuple_field_offsets(fields: &[IrType]) -> Vec<u32> {
    ir_layout::aggregate_field_offsets(fields)
}

/// Byte offset of each enum variant payload.
pub fn compute_enum_variant_offsets(variants: &[(String, Option<IrType>)]) -> Vec<u32> {
    ir_layout::enum_variant_offsets(variants)
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

/// Bridge the shared layout type into this backend's own.
fn to_layout(l: ir_layout::TypeLayout) -> TypeLayout {
    TypeLayout { size: l.size, align: l.align }
}
