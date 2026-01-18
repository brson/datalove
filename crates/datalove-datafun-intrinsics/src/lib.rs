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

    // U8 operations (160-179).
    BitnotU8 = 160,
    BitandU8 = 161,
    BitorU8 = 162,
    BitxorU8 = 163,
    ShlU8 = 164,
    ShrU8 = 165,
    PopcountU8 = 166,
    ClzU8 = 167,
    CtzU8 = 168,
    ReverseBitsU8 = 169,
    AddWrappingU8 = 170,
    SubWrappingU8 = 171,
    MulWrappingU8 = 172,
    RemU8 = 173,
    U8ToI8 = 174,
    I8ToU8 = 175,

    // I8 operations (176-179).
    NegWrappingI8 = 176,
    SshrI8 = 177,
    SremI8 = 178,

    // U16 operations (180-199).
    BitnotU16 = 180,
    BitandU16 = 181,
    BitorU16 = 182,
    BitxorU16 = 183,
    ShlU16 = 184,
    ShrU16 = 185,
    PopcountU16 = 186,
    ClzU16 = 187,
    CtzU16 = 188,
    SwapBytesU16 = 189,
    ReverseBitsU16 = 190,
    AddWrappingU16 = 191,
    SubWrappingU16 = 192,
    MulWrappingU16 = 193,
    RemU16 = 194,
    U16ToI16 = 195,
    I16ToU16 = 196,

    // I16 operations (197-199).
    NegWrappingI16 = 197,
    SshrI16 = 198,
    SremI16 = 199,

    // U64 additional operations (200-219).
    BitnotU64 = 200,
    BitorU64 = 201,
    ShlU64 = 202,
    ShrU64 = 203,
    PopcountU64 = 204,
    ClzU64 = 205,
    CtzU64 = 206,
    SwapBytesU64 = 207,
    ReverseBitsU64 = 208,
    AddWrappingU64 = 209,
    SubWrappingU64 = 210,
    MulWrappingU64 = 211,
    RemU64 = 212,

    // I64 operations (213-219).
    NegWrappingI64 = 213,
    SshrI64 = 214,
    SremI64 = 215,

    // Usize operations (220-239).
    BitnotUsize = 220,
    BitandUsize = 221,
    BitorUsize = 222,
    BitxorUsize = 223,
    ShlUsize = 224,
    ShrUsize = 225,
    PopcountUsize = 226,
    ClzUsize = 227,
    CtzUsize = 228,
    SwapBytesUsize = 229,
    ReverseBitsUsize = 230,
    AddWrappingUsize = 231,
    SubWrappingUsize = 232,
    MulWrappingUsize = 233,
    RemUsize = 234,
    UsizeToIsize = 235,

    // Isize operations (250-269).
    IsizeToUsize = 250,
    NegWrappingIsize = 251,
    SshrIsize = 252,
    SremIsize = 253,
}

/// Simplified type for intrinsic parameters and return values.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum IntrinsicType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    Usize,
    Isize,
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

    // U8 bitwise operations.
    IntrinsicDef {
        id: IntrinsicId::BitnotU8,
        name: "bitnot_u8",
        params: &[IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::BitandU8,
        name: "bitand_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::BitorU8,
        name: "bitor_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::BitxorU8,
        name: "bitxor_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },

    // U8 shift operations.
    IntrinsicDef {
        id: IntrinsicId::ShlU8,
        name: "shl_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::ShrU8,
        name: "shr_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },

    // U8 bit counting operations.
    IntrinsicDef {
        id: IntrinsicId::PopcountU8,
        name: "popcount_u8",
        params: &[IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::ClzU8,
        name: "clz_u8",
        params: &[IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::CtzU8,
        name: "ctz_u8",
        params: &[IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },

    // U8 bit manipulation.
    IntrinsicDef {
        id: IntrinsicId::ReverseBitsU8,
        name: "reverse_bits_u8",
        params: &[IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },

    // U8 wrapping arithmetic.
    IntrinsicDef {
        id: IntrinsicId::AddWrappingU8,
        name: "add_wrapping_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::SubWrappingU8,
        name: "sub_wrapping_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::MulWrappingU8,
        name: "mul_wrapping_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },
    IntrinsicDef {
        id: IntrinsicId::RemU8,
        name: "rem_u8",
        params: &[IntrinsicType::U8, IntrinsicType::U8],
        ret: IntrinsicType::U8,
    },

    // U8/I8 type conversion.
    IntrinsicDef {
        id: IntrinsicId::U8ToI8,
        name: "u8_to_i8",
        params: &[IntrinsicType::U8],
        ret: IntrinsicType::I8,
    },
    IntrinsicDef {
        id: IntrinsicId::I8ToU8,
        name: "i8_to_u8",
        params: &[IntrinsicType::I8],
        ret: IntrinsicType::U8,
    },

    // I8 operations.
    IntrinsicDef {
        id: IntrinsicId::NegWrappingI8,
        name: "neg_wrapping_i8",
        params: &[IntrinsicType::I8],
        ret: IntrinsicType::I8,
    },
    IntrinsicDef {
        id: IntrinsicId::SshrI8,
        name: "sshr_i8",
        params: &[IntrinsicType::I8, IntrinsicType::U8],
        ret: IntrinsicType::I8,
    },
    IntrinsicDef {
        id: IntrinsicId::SremI8,
        name: "srem_i8",
        params: &[IntrinsicType::I8, IntrinsicType::I8],
        ret: IntrinsicType::I8,
    },

    // U16 bitwise operations.
    IntrinsicDef {
        id: IntrinsicId::BitnotU16,
        name: "bitnot_u16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::BitandU16,
        name: "bitand_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::BitorU16,
        name: "bitor_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::BitxorU16,
        name: "bitxor_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },

    // U16 shift operations.
    IntrinsicDef {
        id: IntrinsicId::ShlU16,
        name: "shl_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::ShrU16,
        name: "shr_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },

    // U16 bit counting operations.
    IntrinsicDef {
        id: IntrinsicId::PopcountU16,
        name: "popcount_u16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::ClzU16,
        name: "clz_u16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::CtzU16,
        name: "ctz_u16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },

    // U16 byte/bit manipulation.
    IntrinsicDef {
        id: IntrinsicId::SwapBytesU16,
        name: "swap_bytes_u16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::ReverseBitsU16,
        name: "reverse_bits_u16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },

    // U16 wrapping arithmetic.
    IntrinsicDef {
        id: IntrinsicId::AddWrappingU16,
        name: "add_wrapping_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::SubWrappingU16,
        name: "sub_wrapping_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::MulWrappingU16,
        name: "mul_wrapping_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },
    IntrinsicDef {
        id: IntrinsicId::RemU16,
        name: "rem_u16",
        params: &[IntrinsicType::U16, IntrinsicType::U16],
        ret: IntrinsicType::U16,
    },

    // U16/I16 type conversion.
    IntrinsicDef {
        id: IntrinsicId::U16ToI16,
        name: "u16_to_i16",
        params: &[IntrinsicType::U16],
        ret: IntrinsicType::I16,
    },
    IntrinsicDef {
        id: IntrinsicId::I16ToU16,
        name: "i16_to_u16",
        params: &[IntrinsicType::I16],
        ret: IntrinsicType::U16,
    },

    // I16 operations.
    IntrinsicDef {
        id: IntrinsicId::NegWrappingI16,
        name: "neg_wrapping_i16",
        params: &[IntrinsicType::I16],
        ret: IntrinsicType::I16,
    },
    IntrinsicDef {
        id: IntrinsicId::SshrI16,
        name: "sshr_i16",
        params: &[IntrinsicType::I16, IntrinsicType::U16],
        ret: IntrinsicType::I16,
    },
    IntrinsicDef {
        id: IntrinsicId::SremI16,
        name: "srem_i16",
        params: &[IntrinsicType::I16, IntrinsicType::I16],
        ret: IntrinsicType::I16,
    },

    // U64 additional bitwise operations.
    IntrinsicDef {
        id: IntrinsicId::BitnotU64,
        name: "bitnot_u64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::BitorU64,
        name: "bitor_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },

    // U64 shift operations.
    IntrinsicDef {
        id: IntrinsicId::ShlU64,
        name: "shl_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::ShrU64,
        name: "shr_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },

    // U64 bit counting operations.
    IntrinsicDef {
        id: IntrinsicId::PopcountU64,
        name: "popcount_u64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::ClzU64,
        name: "clz_u64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::CtzU64,
        name: "ctz_u64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },

    // U64 byte/bit manipulation.
    IntrinsicDef {
        id: IntrinsicId::SwapBytesU64,
        name: "swap_bytes_u64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::ReverseBitsU64,
        name: "reverse_bits_u64",
        params: &[IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },

    // U64 wrapping arithmetic.
    IntrinsicDef {
        id: IntrinsicId::AddWrappingU64,
        name: "add_wrapping_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::SubWrappingU64,
        name: "sub_wrapping_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::MulWrappingU64,
        name: "mul_wrapping_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },
    IntrinsicDef {
        id: IntrinsicId::RemU64,
        name: "rem_u64",
        params: &[IntrinsicType::U64, IntrinsicType::U64],
        ret: IntrinsicType::U64,
    },

    // I64 operations.
    IntrinsicDef {
        id: IntrinsicId::NegWrappingI64,
        name: "neg_wrapping_i64",
        params: &[IntrinsicType::I64],
        ret: IntrinsicType::I64,
    },
    IntrinsicDef {
        id: IntrinsicId::SshrI64,
        name: "sshr_i64",
        params: &[IntrinsicType::I64, IntrinsicType::U64],
        ret: IntrinsicType::I64,
    },
    IntrinsicDef {
        id: IntrinsicId::SremI64,
        name: "srem_i64",
        params: &[IntrinsicType::I64, IntrinsicType::I64],
        ret: IntrinsicType::I64,
    },

    // Usize bitwise operations.
    IntrinsicDef {
        id: IntrinsicId::BitnotUsize,
        name: "bitnot_usize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::BitandUsize,
        name: "bitand_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::BitorUsize,
        name: "bitor_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::BitxorUsize,
        name: "bitxor_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },

    // Usize shift operations.
    IntrinsicDef {
        id: IntrinsicId::ShlUsize,
        name: "shl_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::U32],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::ShrUsize,
        name: "shr_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::U32],
        ret: IntrinsicType::Usize,
    },

    // Usize bit counting operations.
    IntrinsicDef {
        id: IntrinsicId::PopcountUsize,
        name: "popcount_usize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::ClzUsize,
        name: "clz_usize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::U32,
    },
    IntrinsicDef {
        id: IntrinsicId::CtzUsize,
        name: "ctz_usize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::U32,
    },

    // Usize byte/bit manipulation.
    IntrinsicDef {
        id: IntrinsicId::SwapBytesUsize,
        name: "swap_bytes_usize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::ReverseBitsUsize,
        name: "reverse_bits_usize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },

    // Usize wrapping arithmetic.
    IntrinsicDef {
        id: IntrinsicId::AddWrappingUsize,
        name: "add_wrapping_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::SubWrappingUsize,
        name: "sub_wrapping_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::MulWrappingUsize,
        name: "mul_wrapping_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },
    IntrinsicDef {
        id: IntrinsicId::RemUsize,
        name: "rem_usize",
        params: &[IntrinsicType::Usize, IntrinsicType::Usize],
        ret: IntrinsicType::Usize,
    },

    // Usize/Isize type conversion.
    IntrinsicDef {
        id: IntrinsicId::UsizeToIsize,
        name: "usize_to_isize",
        params: &[IntrinsicType::Usize],
        ret: IntrinsicType::Isize,
    },
    IntrinsicDef {
        id: IntrinsicId::IsizeToUsize,
        name: "isize_to_usize",
        params: &[IntrinsicType::Isize],
        ret: IntrinsicType::Usize,
    },

    // Isize operations.
    IntrinsicDef {
        id: IntrinsicId::NegWrappingIsize,
        name: "neg_wrapping_isize",
        params: &[IntrinsicType::Isize],
        ret: IntrinsicType::Isize,
    },
    IntrinsicDef {
        id: IntrinsicId::SshrIsize,
        name: "sshr_isize",
        params: &[IntrinsicType::Isize, IntrinsicType::U32],
        ret: IntrinsicType::Isize,
    },
    IntrinsicDef {
        id: IntrinsicId::SremIsize,
        name: "srem_isize",
        params: &[IntrinsicType::Isize, IntrinsicType::Isize],
        ret: IntrinsicType::Isize,
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
