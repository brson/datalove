//! Bitpacking for @data/@error.
//!
//! This module implements a tagged pointer scheme for loosely packing
//! Datalove types and values into 128 bits (two 64-bit words).
//! It is intended to be simple and explainable.
//!
//! See `notes/anytype.md` for the complete design specification.

#![allow(unused)]

use crate::*;

/// Dynamic type that can hold any Datalove value with runtime type information.
///
/// Layout: Two 64-bit words (primary and secondary).
/// The bottom 3 bits of primary are used as a tag to determine encoding.
#[repr(C)]
pub struct Data {
    primary: u64,
    secondary: u64,
}

/// Tag values stored in bottom 3 bits of primary word.
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Tag {
    /// Two pointers: tydesc and value.
    /// - primary: *const TyDesc (untagged, naturally aligned)
    /// - secondary: *const u8 (pointer to value)
    TwoPointers = 0,

    /// Small immediate value without tydesc.
    /// - primary bits 3-63: 61-bit value
    /// - secondary bits 0-7: TyTag enum
    SmallImmediate = 1,

    /// Reserved for future use.
    Reserved2 = 2,

    /// Reserved for future use.
    Reserved3 = 3,

    /// TyDesc pointer + inline 64-bit value.
    /// - primary: *const TyDesc (tagged with 0b100)
    /// - secondary: 64-bit immediate value
    InlineWithTyDesc = 4,

    /// Reserved for future use.
    Reserved5 = 5,

    /// Reserved for future use.
    Reserved6 = 6,

    /// Reserved for future use.
    Reserved7 = 7,
}

const TAG_MASK: u64 = 0b111;
const PTR_MASK: u64 = !TAG_MASK;
const VALUE_SHIFT: u32 = 3;

impl Data {
    // ============================================================================
    // Tag Manipulation
    // ============================================================================

    /// Extract the tag from the primary word.
    #[inline]
    pub fn tag(&self) -> Tag {
        let tag_bits = (self.primary & TAG_MASK) as u8;
        match tag_bits {
            0 => Tag::TwoPointers,
            1 => Tag::SmallImmediate,
            2 => Tag::Reserved2,
            3 => Tag::Reserved3,
            4 => Tag::InlineWithTyDesc,
            5 => Tag::Reserved5,
            6 => Tag::Reserved6,
            7 => Tag::Reserved7,
            _ => unreachable!(),
        }
    }

    /// Extract pointer from tagged primary word.
    #[inline]
    fn primary_ptr<T>(&self) -> *const T {
        (self.primary & PTR_MASK) as *const T
    }

    /// Create a tagged pointer.
    #[inline]
    fn tag_ptr<T>(ptr: *const T, tag: Tag) -> u64 {
        debug_assert_eq!(
            ptr as usize & TAG_MASK as usize,
            0,
            "pointer not 8-byte aligned"
        );
        (ptr as u64) | (tag as u8 as u64)
    }

    /// Extract 61-bit immediate value (unsigned).
    #[inline]
    fn immediate_u61(&self) -> u64 {
        self.primary >> VALUE_SHIFT
    }

    /// Extract 61-bit immediate value (sign-extended).
    #[inline]
    fn immediate_i61(&self) -> i64 {
        (self.primary as i64) >> VALUE_SHIFT
    }

    /// Create primary word from 61-bit immediate value.
    #[inline]
    fn pack_immediate_u61(value: u64, tag: Tag) -> u64 {
        debug_assert!(value < (1u64 << 61), "value too large for 61 bits");
        (value << VALUE_SHIFT) | (tag as u8 as u64)
    }

    // ============================================================================
    // Constructors - Tag 0 (Two Pointers)
    // ============================================================================

    /// Construct Data from two pointers (Tag 0).
    ///
    /// This is the default, unoptimized encoding used for heap-allocated values.
    pub fn from_pointers(tydesc: *const TyDesc, value: *const u8) -> Self {
        debug_assert_eq!(
            tydesc as usize & TAG_MASK as usize,
            0,
            "tydesc not 8-byte aligned"
        );
        Self {
            primary: tydesc as u64, // Untagged (tag = 0)
            secondary: value as u64,
        }
    }

    // ============================================================================
    // Constructors - Tag 1 (Small Immediate)
    // ============================================================================

    /// Construct Data from small immediate value without tydesc (Tag 1).
    ///
    /// Value must fit in 61 bits.
    pub fn from_immediate(value: u64, tytag: TyTag) -> Self {
        debug_assert!(value < (1u64 << 61), "value too large for 61 bits");
        Self {
            primary: Self::pack_immediate_u61(value, Tag::SmallImmediate),
            secondary: tytag as u64,
        }
    }

    /// Construct bool value (Tag 1).
    pub fn from_bool(value: bool) -> Self {
        Self::from_immediate(value as u64, TyTag::Bool)
    }

    /// Construct u32 value (Tag 1).
    pub fn from_u32(value: u32) -> Self {
        Self::from_immediate(value as u64, TyTag::U32)
    }

    // ============================================================================
    // Constructors - Tag 4 (Inline with TyDesc)
    // ============================================================================

    /// Construct Data from tydesc + inline 64-bit value (Tag 4).
    pub fn from_inline64(tydesc: *const TyDesc, value: u64) -> Self {
        debug_assert_eq!(
            tydesc as usize & TAG_MASK as usize,
            0,
            "tydesc not 8-byte aligned"
        );
        Self {
            primary: Self::tag_ptr(tydesc, Tag::InlineWithTyDesc),
            secondary: value,
        }
    }

    /// Construct f32 value (Tag 4).
    pub fn from_f32(value: f32, tydesc: *const TyDesc) -> Self {
        Self::from_inline64(tydesc, value.to_bits() as u64)
    }

    /// Construct u8 value (Tag 1).
    pub fn from_u8(value: u8) -> Self {
        Self::from_immediate(value as u64, TyTag::U8)
    }

    /// Construct i8 value (Tag 1).
    pub fn from_i8(value: i8) -> Self {
        // Reinterpret as u8, then widen to u64 (no sign extension).
        Self::from_immediate(value as u8 as u64, TyTag::I8)
    }

    /// Construct u16 value (Tag 1).
    pub fn from_u16(value: u16) -> Self {
        Self::from_immediate(value as u64, TyTag::U16)
    }

    /// Construct i16 value (Tag 1).
    pub fn from_i16(value: i16) -> Self {
        // Reinterpret as u16, then widen to u64 (no sign extension).
        Self::from_immediate(value as u16 as u64, TyTag::I16)
    }

    /// Construct i32 value (Tag 1).
    pub fn from_i32(value: i32) -> Self {
        // Reinterpret as u32, then widen to u64 (no sign extension).
        Self::from_immediate(value as u32 as u64, TyTag::I32)
    }

    /// Construct u64 value (Tag 4).
    pub fn from_u64(value: u64, tydesc: *const TyDesc) -> Self {
        Self::from_inline64(tydesc, value)
    }

    /// Construct i64 value (Tag 4).
    pub fn from_i64(value: i64, tydesc: *const TyDesc) -> Self {
        Self::from_inline64(tydesc, value as u64)
    }

    /// Construct f64 value (Tag 4).
    pub fn from_f64(value: f64, tydesc: *const TyDesc) -> Self {
        Self::from_inline64(tydesc, value.to_bits())
    }

    // ============================================================================
    // Constructors - Type-Specific (Tag 0)
    // ============================================================================

    /// Construct Int (bigint) value (Tag 0).
    pub fn from_int(int_ptr: *const Int, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, int_ptr as *const u8)
    }

    /// Construct String value (Tag 0).
    pub fn from_string(string_ptr: *const String, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, string_ptr as *const u8)
    }

    /// Construct List value (Tag 0).
    pub fn from_list(list_ptr: *const List, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, list_ptr as *const u8)
    }

    /// Construct Map value (Tag 0).
    pub fn from_map(map_ptr: *const Map, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, map_ptr as *const u8)
    }

    /// Construct Set value (Tag 0).
    pub fn from_set(set_ptr: *const Set, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, set_ptr as *const u8)
    }

    /// Construct Option value (Tag 0).
    pub fn from_option(option_ptr: *const Option, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, option_ptr as *const u8)
    }

    /// Construct Result value (Tag 0).
    pub fn from_result(result_ptr: *const Result, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, result_ptr as *const u8)
    }

    /// Construct nested Data value (Tag 0).
    pub fn from_data(data_ptr: *const Data, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, data_ptr as *const u8)
    }

    /// Construct Error value (Tag 0).
    pub fn from_error(error_ptr: *const Error, tydesc: *const TyDesc) -> Self {
        Self::from_pointers(tydesc, error_ptr as *const u8)
    }

    // ============================================================================
    // Accessors - Type Descriptor
    // ============================================================================

    /// Get the type descriptor for this value.
    ///
    /// For Tag 0 and Tag 4, this is a direct pointer.
    /// For Tag 1, the TyTag is stored in secondary and would need synthesis.
    pub fn tydesc(&self) -> *const TyDesc {
        match self.tag() {
            Tag::TwoPointers => self.primary as *const TyDesc,
            Tag::InlineWithTyDesc => self.primary_ptr::<TyDesc>(),
            Tag::SmallImmediate => {
                // Would need to synthesize tydesc from TyTag stored in secondary.
                // For now, return null.
                std::ptr::null()
            }
            _ => panic!("invalid tag"),
        }
    }

    /// Get the TyTag for this value.
    ///
    /// If tydesc is available, extract from it.
    /// If Tag 1, extract from secondary.
    pub fn tytag(&self) -> TyTag {
        match self.tag() {
            Tag::TwoPointers | Tag::InlineWithTyDesc => {
                let tydesc = self.tydesc();
                debug_assert!(!tydesc.is_null());
                unsafe { (*tydesc).type_tag }
            }
            Tag::SmallImmediate => {
                let tytag_u8 = (self.secondary & 0xFF) as u8;
                unsafe { std::mem::transmute(tytag_u8) }
            }
            _ => panic!("invalid tag"),
        }
    }

    // ============================================================================
    // Accessors - Value Pointers
    // ============================================================================

    /// Get pointer to value (only valid for Tag 0).
    pub fn value_ptr(&self) -> *const u8 {
        match self.tag() {
            Tag::TwoPointers => self.secondary as *const u8,
            _ => panic!("value not stored as pointer"),
        }
    }

    /// Get pointer to value as specific type (only valid for Tag 0).
    pub fn value_ptr_as<T>(&self) -> *const T {
        self.value_ptr() as *const T
    }

    // ============================================================================
    // Accessors - Type-Specific
    // ============================================================================

    /// Try to extract as bool.
    pub fn as_bool(&self) -> std::option::Option<bool> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::Bool => {
                std::option::Option::Some(self.immediate_u61() != 0)
            }
            Tag::InlineWithTyDesc if self.tytag() == TyTag::Bool => {
                std::option::Option::Some(self.secondary != 0)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as u32.
    pub fn as_u32(&self) -> std::option::Option<u32> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::U32 => {
                let value = self.immediate_u61();
                debug_assert!(value <= u32::MAX as u64);
                std::option::Option::Some(value as u32)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as f32.
    pub fn as_f32(&self) -> std::option::Option<f32> {
        match self.tag() {
            Tag::InlineWithTyDesc if self.tytag() == TyTag::F32 => {
                std::option::Option::Some(f32::from_bits(self.secondary as u32))
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as u8.
    pub fn as_u8(&self) -> std::option::Option<u8> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::U8 => {
                let value = self.immediate_u61();
                std::option::Option::Some(value as u8)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as i8.
    pub fn as_i8(&self) -> std::option::Option<i8> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::I8 => {
                let value = self.immediate_u61();
                std::option::Option::Some(value as u8 as i8)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as u16.
    pub fn as_u16(&self) -> std::option::Option<u16> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::U16 => {
                let value = self.immediate_u61();
                std::option::Option::Some(value as u16)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as i16.
    pub fn as_i16(&self) -> std::option::Option<i16> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::I16 => {
                let value = self.immediate_u61();
                // Stored as u16, reinterpret as i16.
                std::option::Option::Some(value as u16 as i16)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as i32.
    pub fn as_i32(&self) -> std::option::Option<i32> {
        match self.tag() {
            Tag::SmallImmediate if self.tytag() == TyTag::I32 => {
                let value = self.immediate_u61();
                std::option::Option::Some(value as u32 as i32)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as u64.
    pub fn as_u64(&self) -> std::option::Option<u64> {
        match self.tag() {
            Tag::InlineWithTyDesc if self.tytag() == TyTag::U64 => {
                std::option::Option::Some(self.secondary)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as i64.
    pub fn as_i64(&self) -> std::option::Option<i64> {
        match self.tag() {
            Tag::InlineWithTyDesc if self.tytag() == TyTag::I64 => {
                std::option::Option::Some(self.secondary as i64)
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as f64.
    pub fn as_f64(&self) -> std::option::Option<f64> {
        match self.tag() {
            Tag::InlineWithTyDesc if self.tytag() == TyTag::F64 => {
                std::option::Option::Some(f64::from_bits(self.secondary))
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as Int pointer.
    pub fn as_int(&self) -> std::option::Option<*const Int> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Int => {
                std::option::Option::Some(self.value_ptr_as::<Int>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as String pointer.
    pub fn as_string(&self) -> std::option::Option<*const String> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::String => {
                std::option::Option::Some(self.value_ptr_as::<String>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as List pointer.
    pub fn as_list(&self) -> std::option::Option<*const List> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::List => {
                std::option::Option::Some(self.value_ptr_as::<List>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as Map pointer.
    pub fn as_map(&self) -> std::option::Option<*const Map> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Map => {
                std::option::Option::Some(self.value_ptr_as::<Map>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as Set pointer.
    pub fn as_set(&self) -> std::option::Option<*const Set> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Set => {
                std::option::Option::Some(self.value_ptr_as::<Set>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as Option pointer.
    pub fn as_option(&self) -> std::option::Option<*const Option> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Option => {
                std::option::Option::Some(self.value_ptr_as::<Option>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as Result pointer.
    pub fn as_result(&self) -> std::option::Option<*const Result> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Result => {
                std::option::Option::Some(self.value_ptr_as::<Result>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as nested Data pointer.
    pub fn as_data(&self) -> std::option::Option<*const Data> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Data => {
                std::option::Option::Some(self.value_ptr_as::<Data>())
            }
            _ => std::option::Option::None,
        }
    }

    /// Try to extract as Error pointer.
    pub fn as_error(&self) -> std::option::Option<*const Error> {
        match self.tag() {
            Tag::TwoPointers if self.tytag() == TyTag::Error => {
                std::option::Option::Some(self.value_ptr_as::<Error>())
            }
            _ => std::option::Option::None,
        }
    }
}

// ============================================================================
// TyTag Extensions
// ============================================================================

impl TyTag {
    /// Check if this type tag represents a type that needs heap allocation.
    pub fn needs_heap_allocation(&self) -> bool {
        matches!(
            self,
            TyTag::Int
                | TyTag::String
                | TyTag::List
                | TyTag::Map
                | TyTag::Set
                | TyTag::Tuple  // Large tuples
                | TyTag::Struct // Large structs
                | TyTag::Enum   // Large enums
                | TyTag::Data
                | TyTag::Error
        )
    }

    /// Check if this type tag represents a type that can be inlined.
    pub fn can_inline(&self) -> bool {
        matches!(
            self,
            TyTag::Bool
                | TyTag::U8
                | TyTag::I8
                | TyTag::U16
                | TyTag::I16
                | TyTag::U32
                | TyTag::I32
                | TyTag::F32
                | TyTag::U64
                | TyTag::I64
                | TyTag::F64
        )
    }
}

// ============================================================================
// Debug Implementation
// ============================================================================

impl std::fmt::Debug for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("Data");
        debug_struct
            .field("primary", &format_args!("0x{:016x}", self.primary))
            .field("secondary", &format_args!("0x{:016x}", self.secondary))
            .field("tag", &self.tag());

        // Only try to get tytag if tydesc is not null.
        let tydesc = self.tydesc();
        if !tydesc.is_null() {
            debug_struct.field("tytag", &self.tytag());
        }

        debug_struct.finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create real TyDesc instances for testing.
    fn make_tydesc(type_tag: TyTag) -> TyDesc {
        TyDesc {
            type_tag,
            size: 0,
            align: 0,
            type_info: TyInfo {
                nothing: TyInfoNothing,
            },
        }
    }

    #[test]
    fn test_bool_true() {
        let data = Data::from_bool(true);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::Bool);
        assert_eq!(data.as_bool(), std::option::Option::Some(true));
    }

    #[test]
    fn test_bool_false() {
        let data = Data::from_bool(false);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::Bool);
        assert_eq!(data.as_bool(), std::option::Option::Some(false));
    }

    #[test]
    fn test_u32_small() {
        let data = Data::from_u32(42);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::U32);
        assert_eq!(data.as_u32(), std::option::Option::Some(42));
    }

    #[test]
    fn test_u32_large() {
        let data = Data::from_u32(3_000_000_000);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::U32);
        assert_eq!(data.as_u32(), std::option::Option::Some(3_000_000_000));
    }

    #[test]
    fn test_f32() {
        let tydesc = make_tydesc(TyTag::F32);
        let data = Data::from_f32(3.14, &tydesc);
        assert_eq!(data.tag(), Tag::InlineWithTyDesc);
        assert_eq!(data.tytag(), TyTag::F32);
        let f = data.as_f32().unwrap();
        assert!((f - 3.14).abs() < 0.01);
    }

    #[test]
    fn test_two_pointers() {
        let tydesc = make_tydesc(TyTag::Int);
        let mock_value = 0xABCD as *const u8;
        let data = Data::from_pointers(&tydesc, mock_value);
        assert_eq!(data.tag(), Tag::TwoPointers);
        assert_eq!(data.tydesc(), &tydesc as *const TyDesc);
        assert_eq!(data.value_ptr(), mock_value);
    }

    #[test]
    fn test_tag_extraction() {
        // Tag 0 (untagged pointer)
        let data = Data {
            primary: 0x1000,
            secondary: 0x2000,
        };
        assert_eq!(data.tag(), Tag::TwoPointers);

        // Tag 1
        let data = Data {
            primary: 0x1001,
            secondary: 0,
        };
        assert_eq!(data.tag(), Tag::SmallImmediate);

        // Tag 4
        let data = Data {
            primary: 0x1004,
            secondary: 0,
        };
        assert_eq!(data.tag(), Tag::InlineWithTyDesc);
    }

    #[test]
    fn test_option() {
        // Create a mock Option struct
        let option_val = Option {
            tag: OptionTag::None,
        };

        let tydesc = make_tydesc(TyTag::Option);
        let data = Data::from_option(&option_val, &tydesc);

        assert_eq!(data.tag(), Tag::TwoPointers);
        assert_eq!(data.tytag(), TyTag::Option);
        assert_eq!(data.as_option(), std::option::Option::Some(&option_val as *const Option));
    }

    #[test]
    fn test_result() {
        let result_val = Result {
            tag: ResultTag::Ok,
        };

        let tydesc = make_tydesc(TyTag::Result);
        let data = Data::from_result(&result_val, &tydesc);

        assert_eq!(data.tag(), Tag::TwoPointers);
        assert_eq!(data.tytag(), TyTag::Result);
        assert_eq!(data.as_result(), std::option::Option::Some(&result_val as *const Result));
    }

    #[test]
    fn test_nested_data() {
        let inner_data = Data::from_bool(true);

        let tydesc = make_tydesc(TyTag::Data);
        let data = Data::from_data(&inner_data, &tydesc);

        assert_eq!(data.tag(), Tag::TwoPointers);
        assert_eq!(data.tytag(), TyTag::Data);
        assert_eq!(data.as_data(), std::option::Option::Some(&inner_data as *const Data));
    }

    #[test]
    fn test_error() {
        let error_val = Error {
            data: 0,
            tydesc: std::ptr::null(),
        };

        let tydesc = make_tydesc(TyTag::Error);
        let data = Data::from_error(&error_val, &tydesc);

        assert_eq!(data.tag(), Tag::TwoPointers);
        assert_eq!(data.tytag(), TyTag::Error);
        assert_eq!(data.as_error(), std::option::Option::Some(&error_val as *const Error));
    }

    #[test]
    fn test_u8() {
        let data = Data::from_u8(42);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::U8);
        assert_eq!(data.as_u8(), std::option::Option::Some(42));

        let data = Data::from_u8(255);
        assert_eq!(data.as_u8(), std::option::Option::Some(255));
    }

    #[test]
    fn test_i8() {
        let data = Data::from_i8(42);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::I8);
        assert_eq!(data.as_i8(), std::option::Option::Some(42));

        let data = Data::from_i8(-42);
        assert_eq!(data.as_i8(), std::option::Option::Some(-42));

        let data = Data::from_i8(127);
        assert_eq!(data.as_i8(), std::option::Option::Some(127));

        let data = Data::from_i8(-128);
        assert_eq!(data.as_i8(), std::option::Option::Some(-128));
    }

    #[test]
    fn test_u16() {
        let data = Data::from_u16(1000);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::U16);
        assert_eq!(data.as_u16(), std::option::Option::Some(1000));

        let data = Data::from_u16(65535);
        assert_eq!(data.as_u16(), std::option::Option::Some(65535));
    }

    #[test]
    fn test_i16() {
        let data = Data::from_i16(1000);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::I16);
        assert_eq!(data.as_i16(), std::option::Option::Some(1000));

        let data = Data::from_i16(-1000);
        assert_eq!(data.as_i16(), std::option::Option::Some(-1000));

        let data = Data::from_i16(32767);
        assert_eq!(data.as_i16(), std::option::Option::Some(32767));

        let data = Data::from_i16(-32768);
        assert_eq!(data.as_i16(), std::option::Option::Some(-32768));
    }

    #[test]
    fn test_i32_small() {
        let data = Data::from_i32(42);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::I32);
        assert_eq!(data.as_i32(), std::option::Option::Some(42));

        let data = Data::from_i32(-42);
        assert_eq!(data.as_i32(), std::option::Option::Some(-42));
    }

    #[test]
    fn test_i32_large() {
        let data = Data::from_i32(2_000_000_000);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.tytag(), TyTag::I32);
        assert_eq!(data.as_i32(), std::option::Option::Some(2_000_000_000));

        let data = Data::from_i32(-2_000_000_000);
        assert_eq!(data.tag(), Tag::SmallImmediate);
        assert_eq!(data.as_i32(), std::option::Option::Some(-2_000_000_000));

        let data = Data::from_i32(i32::MAX);
        assert_eq!(data.as_i32(), std::option::Option::Some(i32::MAX));

        let data = Data::from_i32(i32::MIN);
        assert_eq!(data.as_i32(), std::option::Option::Some(i32::MIN));
    }

    #[test]
    fn test_u64() {
        let tydesc = make_tydesc(TyTag::U64);

        let data = Data::from_u64(12345678901234, &tydesc);
        assert_eq!(data.tag(), Tag::InlineWithTyDesc);
        assert_eq!(data.tytag(), TyTag::U64);
        assert_eq!(data.as_u64(), std::option::Option::Some(12345678901234));

        let data = Data::from_u64(u64::MAX, &tydesc);
        assert_eq!(data.as_u64(), std::option::Option::Some(u64::MAX));
    }

    #[test]
    fn test_i64() {
        let tydesc = make_tydesc(TyTag::I64);

        let data = Data::from_i64(12345678901234, &tydesc);
        assert_eq!(data.tag(), Tag::InlineWithTyDesc);
        assert_eq!(data.tytag(), TyTag::I64);
        assert_eq!(data.as_i64(), std::option::Option::Some(12345678901234));

        let data = Data::from_i64(-12345678901234, &tydesc);
        assert_eq!(data.as_i64(), std::option::Option::Some(-12345678901234));

        let data = Data::from_i64(i64::MAX, &tydesc);
        assert_eq!(data.as_i64(), std::option::Option::Some(i64::MAX));

        let data = Data::from_i64(i64::MIN, &tydesc);
        assert_eq!(data.as_i64(), std::option::Option::Some(i64::MIN));
    }

    #[test]
    fn test_f64() {
        let tydesc = make_tydesc(TyTag::F64);

        let data = Data::from_f64(3.14159265358979, &tydesc);
        assert_eq!(data.tag(), Tag::InlineWithTyDesc);
        assert_eq!(data.tytag(), TyTag::F64);
        assert_eq!(data.as_f64(), std::option::Option::Some(3.14159265358979));

        let data = Data::from_f64(-1.0, &tydesc);
        assert_eq!(data.as_f64(), std::option::Option::Some(-1.0));

        let data = Data::from_f64(f64::MAX, &tydesc);
        assert_eq!(data.as_f64(), std::option::Option::Some(f64::MAX));

        let data = Data::from_f64(f64::MIN, &tydesc);
        assert_eq!(data.as_f64(), std::option::Option::Some(f64::MIN));
    }
}
