//! Intrinsic function definitions for Datalove.
//!
//! Intrinsics are low-level operations that compile directly to machine instructions
//! without function call overhead. They are invoked via `icall name(args)` syntax.

use serde::{Deserialize, Serialize};

/// Unique identifier for an intrinsic function.
///
/// Discriminant values are stable across versions for serialization.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum IntrinsicId {
    // Bitwise operations (0-9).
    BitnotU32 = 0,
    BitandU32 = 1,
    BitorU32 = 2,
    BitxorU32 = 3,

    // Shift operations (10-19).
    ShlU32 = 10,
    ShrU32 = 11,

    // Bit counting operations (20-29).
    PopcountU32 = 20,
    ClzU32 = 21,
    CtzU32 = 22,

    // Byte/bit manipulation (30-39).
    SwapBytesU32 = 30,
    ReverseBitsU32 = 31,

    // Wrapping arithmetic (40-49).
    AddWrappingU32 = 40,
    SubWrappingU32 = 41,
    MulWrappingU32 = 42,
    RemU32 = 43,

    // Type conversion (50-59).
    U32ToI32 = 50,
    I32ToU32 = 51,
    NegWrappingI32 = 52,

    // Platform queries (60-69).
    IsBigEndian = 60,

    // Signed i32 operations (70-79).
    SshrI32 = 70,
    SremI32 = 71,

    // F32 classification intrinsics (80-89).
    IsNanF32 = 80,
    IsInfiniteF32 = 81,

    // F32 bit conversion (90-99).
    F32ToBits = 90,
    BitsToF32 = 91,

    // F32 math intrinsics (100-109).
    AbsF32 = 100,
    SqrtF32 = 101,
    FloorF32 = 102,
    CeilF32 = 103,
    RoundF32 = 104,
    TruncF32 = 105,
    CopysignF32 = 106,
    MinF32 = 107,
    MaxF32 = 108,

    // F64 classification intrinsics (110-119).
    IsNanF64 = 110,
    IsInfiniteF64 = 111,

    // F64 bit conversion (120-129).
    F64ToBits = 120,
    BitsToF64 = 121,

    // F64 math intrinsics (130-139).
    AbsF64 = 130,
    SqrtF64 = 131,
    FloorF64 = 132,
    CeilF64 = 133,
    RoundF64 = 134,
    TruncF64 = 135,
    CopysignF64 = 136,
    MinF64 = 137,
    MaxF64 = 138,

    // U64 bitwise operations (140-149).
    BitandU64 = 140,
    BitxorU64 = 141,

    // U64/I64 type conversion (150-159).
    U64ToI64 = 150,
    I64ToU64 = 151,
}

/// Simplified type for intrinsic parameters and return values.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum IntrinsicType {
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    Bool,
}

/// Definition of an intrinsic function.
#[derive(Clone, Debug)]
pub struct IntrinsicDef {
    /// Unique identifier.
    pub id: IntrinsicId,
    /// Name as it appears in source code (e.g., "bitnot_u32").
    pub name: &'static str,
    /// Parameter types.
    pub params: &'static [IntrinsicType],
    /// Return type.
    pub ret: IntrinsicType,
}

/// Static table of all intrinsic definitions.
pub static INTRINSICS: &[IntrinsicDef] = &[
    // Bitwise operations.
    IntrinsicDef {
        id: IntrinsicId::BitnotU32,
        name: "bitnot_u32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::BitandU32,
        name: "bitand_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::BitorU32,
        name: "bitor_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::BitxorU32,
        name: "bitxor_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },

    // Shift operations.
    IntrinsicDef {
        id: IntrinsicId::ShlU32,
        name: "shl_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::ShrU32,
        name: "shr_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },

    // Bit counting operations.
    IntrinsicDef {
        id: IntrinsicId::PopcountU32,
        name: "popcount_u32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::ClzU32,
        name: "clz_u32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::CtzU32,
        name: "ctz_u32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },

    // Byte/bit manipulation.
    IntrinsicDef {
        id: IntrinsicId::SwapBytesU32,
        name: "swap_bytes_u32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::ReverseBitsU32,
        name: "reverse_bits_u32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },

    // Wrapping arithmetic.
    IntrinsicDef {
        id: IntrinsicId::AddWrappingU32,
        name: "add_wrapping_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::SubWrappingU32,
        name: "sub_wrapping_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::MulWrappingU32,
        name: "mul_wrapping_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::RemU32,
        name: "rem_u32",
        params: &[IntrinsicType::U32, IntrinsicType::U32],
        ret: IntrinsicType::U32,
    },

    // Type conversion.
    IntrinsicDef {
        id: IntrinsicId::U32ToI32,
        name: "u32_to_i32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::I32,
    },
    IntrinsicDef {
        id: IntrinsicId::I32ToU32,
        name: "i32_to_u32",
        params: &[IntrinsicType::I32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::NegWrappingI32,
        name: "neg_wrapping_i32",
        params: &[IntrinsicType::I32],
        ret: IntrinsicType::I32,
    },

    // Platform queries.
    IntrinsicDef {
        id: IntrinsicId::IsBigEndian,
        name: "is_big_endian",
        params: &[],
        ret: IntrinsicType::Bool,
    },

    // Signed i32 operations.
    IntrinsicDef {
        id: IntrinsicId::SshrI32,
        name: "sshr_i32",
        params: &[IntrinsicType::I32, IntrinsicType::U32],
        ret: IntrinsicType::I32,
    },
    IntrinsicDef {
        id: IntrinsicId::SremI32,
        name: "srem_i32",
        params: &[IntrinsicType::I32, IntrinsicType::I32],
        ret: IntrinsicType::I32,
    },

    // F32 classification intrinsics.
    IntrinsicDef {
        id: IntrinsicId::IsNanF32,
        name: "is_nan_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::Bool,
    },
    IntrinsicDef {
        id: IntrinsicId::IsInfiniteF32,
        name: "is_infinite_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::Bool,
    },

    // F32 bit conversion.
    IntrinsicDef {
        id: IntrinsicId::F32ToBits,
        name: "f32_to_bits",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::BitsToF32,
        name: "bits_to_f32",
        params: &[IntrinsicType::U32],
        ret: IntrinsicType::F32,
    },

    // F32 math intrinsics.
    IntrinsicDef {
        id: IntrinsicId::AbsF32,
        name: "abs_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::SqrtF32,
        name: "sqrt_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::FloorF32,
        name: "floor_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::CeilF32,
        name: "ceil_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::RoundF32,
        name: "round_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::TruncF32,
        name: "trunc_f32",
        params: &[IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::CopysignF32,
        name: "copysign_f32",
        params: &[IntrinsicType::F32, IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::MinF32,
        name: "min_f32",
        params: &[IntrinsicType::F32, IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },
    IntrinsicDef {
        id: IntrinsicId::MaxF32,
        name: "max_f32",
        params: &[IntrinsicType::F32, IntrinsicType::F32],
        ret: IntrinsicType::F32,
    },

    // F64 classification intrinsics.
    IntrinsicDef {
        id: IntrinsicId::IsNanF64,
        name: "is_nan_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::Bool,
    },
    IntrinsicDef {
        id: IntrinsicId::IsInfiniteF64,
        name: "is_infinite_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::Bool,
    },

    // F64 bit conversion.
    IntrinsicDef {
        id: IntrinsicId::F64ToBits,
        name: "f64_to_bits",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::BitsToF64,
        name: "bits_to_f64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::F64,
    },

    // F64 math intrinsics.
    IntrinsicDef {
        id: IntrinsicId::AbsF64,
        name: "abs_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::SqrtF64,
        name: "sqrt_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::FloorF64,
        name: "floor_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::CeilF64,
        name: "ceil_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::RoundF64,
        name: "round_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::TruncF64,
        name: "trunc_f64",
        params: &[IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::CopysignF64,
        name: "copysign_f64",
        params: &[IntrinsicType::F64, IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::MinF64,
        name: "min_f64",
        params: &[IntrinsicType::F64, IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },
    IntrinsicDef {
        id: IntrinsicId::MaxF64,
        name: "max_f64",
        params: &[IntrinsicType::F64, IntrinsicType::F64],
        ret: IntrinsicType::F64,
    },

    // U64 bitwise operations.
    IntrinsicDef {
        id: IntrinsicId::BitandU64,
        name: "bitand_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::BitxorU64,
        name: "bitxor_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },

    // U64/I64 type conversion.
    IntrinsicDef {
        id: IntrinsicId::U64ToI64,
        name: "u64_to_i64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::I64,
    },
    IntrinsicDef {
        id: IntrinsicId::I64ToU64,
        name: "i64_to_u64",
        params: &[IntrinsicType::I64],
        ret: IntrinsicType::U64,
    },
];

/// Look up an intrinsic by name.
///
/// Returns the intrinsic ID and definition if found.
pub fn lookup_intrinsic(name: &str) -> Option<(IntrinsicId, &'static IntrinsicDef)> {
    INTRINSICS.iter().find(|def| def.name == name).map(|def| (def.id, def))
}

/// Get an intrinsic definition by ID.
pub fn get_intrinsic(id: IntrinsicId) -> Option<&'static IntrinsicDef> {
    INTRINSICS.iter().find(|def| def.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_intrinsic() {
        let (id, def) = lookup_intrinsic("bitnot_u32").expect("should find bitnot_u32");
        assert_eq!(id, IntrinsicId::BitnotU32);
        assert_eq!(def.params.len(), 1);
        assert_eq!(def.ret, IntrinsicType::U32);
    }

    #[test]
    fn test_lookup_unknown() {
        assert!(lookup_intrinsic("unknown_intrinsic").is_none());
    }

    #[test]
    fn test_get_intrinsic() {
        let def = get_intrinsic(IntrinsicId::PopcountU32).expect("should find popcount_u32");
        assert_eq!(def.name, "popcount_u32");
        assert_eq!(def.params.len(), 1);
    }
}
