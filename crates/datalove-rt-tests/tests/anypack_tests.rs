//! Tests for anypack bitpacking (Data/Error types).

use datalove_rtdt::*;

// Helper to create a TyDesc for testing.
fn make_tydesc(type_tag: TyTag) -> TyDesc {
    TyDesc {
        type_tag,
        size: 8,
        align: 8,
        type_info: TyInfo {
            nothing: TyInfoNothing,
        },
    }
}

// ==================== Tag Tests (via constructors) ====================

#[test]
fn test_tag_two_pointers_via_from_pointers() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_pointers(&tydesc, &int_val as *const Int as *const u8);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
}

#[test]
fn test_tag_small_immediate_via_from_bool() {
    let data = Data::from_bool(true);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
}

#[test]
fn test_tag_inline_with_tydesc_via_from_u64() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(42, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::InlineWithTyDesc);
}

// ==================== Bool Tests ====================

#[test]
fn test_from_bool_true() {
    let data = Data::from_bool(true);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::Bool);
    assert_eq!(data.as_bool(), Some(true));
}

#[test]
fn test_from_bool_false() {
    let data = Data::from_bool(false);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::Bool);
    assert_eq!(data.as_bool(), Some(false));
}

#[test]
fn test_as_bool_wrong_type() {
    let data = Data::from_u32(42);
    assert_eq!(data.as_bool(), None);
}

#[test]
fn test_as_bool_inline_with_tydesc() {
    let tydesc = make_tydesc(TyTag::Bool);
    let data = Data::from_inline64(&tydesc, 1);
    assert_eq!(data.as_bool(), Some(true));

    let data = Data::from_inline64(&tydesc, 0);
    assert_eq!(data.as_bool(), Some(false));
}

// ==================== Integer Tests (SmallImmediate) ====================

#[test]
fn test_from_u8() {
    let data = Data::from_u8(42);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::U8);
    assert_eq!(data.as_u8(), Some(42));
}

#[test]
fn test_from_u8_max() {
    let data = Data::from_u8(255);
    assert_eq!(data.as_u8(), Some(255));
}

#[test]
fn test_from_i8_positive() {
    let data = Data::from_i8(42);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::I8);
    assert_eq!(data.as_i8(), Some(42));
}

#[test]
fn test_from_i8_negative() {
    let data = Data::from_i8(-42);
    assert_eq!(data.as_i8(), Some(-42));
}

#[test]
fn test_from_i8_extremes() {
    let data = Data::from_i8(127);
    assert_eq!(data.as_i8(), Some(127));

    let data = Data::from_i8(-128);
    assert_eq!(data.as_i8(), Some(-128));
}

#[test]
fn test_from_u16() {
    let data = Data::from_u16(1000);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::U16);
    assert_eq!(data.as_u16(), Some(1000));
}

#[test]
fn test_from_u16_max() {
    let data = Data::from_u16(65535);
    assert_eq!(data.as_u16(), Some(65535));
}

#[test]
fn test_from_i16_positive() {
    let data = Data::from_i16(1000);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::I16);
    assert_eq!(data.as_i16(), Some(1000));
}

#[test]
fn test_from_i16_negative() {
    let data = Data::from_i16(-1000);
    assert_eq!(data.as_i16(), Some(-1000));
}

#[test]
fn test_from_i16_extremes() {
    let data = Data::from_i16(32767);
    assert_eq!(data.as_i16(), Some(32767));

    let data = Data::from_i16(-32768);
    assert_eq!(data.as_i16(), Some(-32768));
}

#[test]
fn test_from_u32() {
    let data = Data::from_u32(42);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::U32);
    assert_eq!(data.as_u32(), Some(42));
}

#[test]
fn test_from_u32_large() {
    let data = Data::from_u32(3_000_000_000);
    assert_eq!(data.as_u32(), Some(3_000_000_000));
}

#[test]
fn test_from_i32_positive() {
    let data = Data::from_i32(42);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::I32);
    assert_eq!(data.as_i32(), Some(42));
}

#[test]
fn test_from_i32_negative() {
    let data = Data::from_i32(-42);
    assert_eq!(data.as_i32(), Some(-42));
}

#[test]
fn test_from_i32_extremes() {
    let data = Data::from_i32(i32::MAX);
    assert_eq!(data.as_i32(), Some(i32::MAX));

    let data = Data::from_i32(i32::MIN);
    assert_eq!(data.as_i32(), Some(i32::MIN));
}

// ==================== Integer Tests (InlineWithTyDesc) ====================

#[test]
fn test_from_u64() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(12345678901234, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::InlineWithTyDesc);
    assert_eq!(data.tytag(), TyTag::U64);
    assert_eq!(data.as_u64(), Some(12345678901234));
}

#[test]
fn test_from_u64_max() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(u64::MAX, &tydesc);
    assert_eq!(data.as_u64(), Some(u64::MAX));
}

#[test]
fn test_from_i64_positive() {
    let tydesc = make_tydesc(TyTag::I64);
    let data = Data::from_i64(12345678901234, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::InlineWithTyDesc);
    assert_eq!(data.tytag(), TyTag::I64);
    assert_eq!(data.as_i64(), Some(12345678901234));
}

#[test]
fn test_from_i64_negative() {
    let tydesc = make_tydesc(TyTag::I64);
    let data = Data::from_i64(-12345678901234, &tydesc);
    assert_eq!(data.as_i64(), Some(-12345678901234));
}

#[test]
fn test_from_i64_extremes() {
    let tydesc = make_tydesc(TyTag::I64);
    let data = Data::from_i64(i64::MAX, &tydesc);
    assert_eq!(data.as_i64(), Some(i64::MAX));

    let data = Data::from_i64(i64::MIN, &tydesc);
    assert_eq!(data.as_i64(), Some(i64::MIN));
}

// ==================== Float Tests ====================

#[test]
fn test_from_f32() {
    let tydesc = make_tydesc(TyTag::F32);
    let data = Data::from_f32(3.14, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::InlineWithTyDesc);
    assert_eq!(data.tytag(), TyTag::F32);
    let f = data.as_f32().unwrap();
    assert!((f - 3.14).abs() < 0.01);
}

#[test]
fn test_from_f32_negative() {
    let tydesc = make_tydesc(TyTag::F32);
    let data = Data::from_f32(-2.5, &tydesc);
    assert_eq!(data.as_f32(), Some(-2.5));
}

#[test]
fn test_from_f64() {
    let tydesc = make_tydesc(TyTag::F64);
    let data = Data::from_f64(3.14159265358979, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::InlineWithTyDesc);
    assert_eq!(data.tytag(), TyTag::F64);
    assert_eq!(data.as_f64(), Some(3.14159265358979));
}

#[test]
fn test_from_f64_negative() {
    let tydesc = make_tydesc(TyTag::F64);
    let data = Data::from_f64(-1.0, &tydesc);
    assert_eq!(data.as_f64(), Some(-1.0));
}

#[test]
fn test_from_f64_extremes() {
    let tydesc = make_tydesc(TyTag::F64);
    let data = Data::from_f64(f64::MAX, &tydesc);
    assert_eq!(data.as_f64(), Some(f64::MAX));

    let data = Data::from_f64(f64::MIN, &tydesc);
    assert_eq!(data.as_f64(), Some(f64::MIN));
}

// ==================== Two Pointers Tests ====================

#[test]
fn test_from_pointers() {
    let tydesc = make_tydesc(TyTag::Int);
    let mock_value: *const u8 = std::ptr::without_provenance(0xABC8); // Must be aligned.
    let data = Data::from_pointers(&tydesc, mock_value);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tydesc(), &tydesc as *const TyDesc);
    assert_eq!(data.value_ptr(), mock_value);
}

#[test]
fn test_from_int() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_int(&int_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Int);
    assert_eq!(data.as_int(), Some(&int_val as *const Int));
}

#[test]
fn test_from_string() {
    let tydesc = make_tydesc(TyTag::String);
    let string_val = String { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };
    let data = Data::from_string(&string_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::String);
    assert_eq!(data.as_string(), Some(&string_val as *const String));
}

#[test]
fn test_from_list() {
    let tydesc = make_tydesc(TyTag::List);
    let list_val = List { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };
    let data = Data::from_list(&list_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::List);
    assert_eq!(data.as_list(), Some(&list_val as *const List));
}

#[test]
fn test_from_map() {
    let tydesc = make_tydesc(TyTag::Map);
    let map_val = Map { root: std::ptr::null(), len: Index::ZERO };
    let data = Data::from_map(&map_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Map);
    assert_eq!(data.as_map(), Some(&map_val as *const Map));
}

#[test]
fn test_from_set() {
    let tydesc = make_tydesc(TyTag::Set);
    let set_val = Set { root: std::ptr::null(), len: Index::ZERO };
    let data = Data::from_set(&set_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Set);
    assert_eq!(data.as_set(), Some(&set_val as *const Set));
}

#[test]
fn test_from_option() {
    let tydesc = make_tydesc(TyTag::Option);
    let option_val = Option { tag: OptionTag::None };
    let data = Data::from_option(&option_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Option);
    assert_eq!(data.as_option(), Some(&option_val as *const Option));
}

#[test]
fn test_from_result() {
    let tydesc = make_tydesc(TyTag::Result);
    let result_val = Result { tag: ResultTag::Ok };
    let data = Data::from_result(&result_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Result);
    assert_eq!(data.as_result(), Some(&result_val as *const Result));
}

#[test]
fn test_from_data() {
    let inner_data = Data::from_bool(true);
    let tydesc = make_tydesc(TyTag::Data);
    let data = Data::from_data(&inner_data, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Data);
    assert_eq!(data.as_data(), Some(&inner_data as *const Data));
}

#[test]
fn test_from_error() {
    // Use from_pointers to create an error-like value, then wrap it.
    let inner_tydesc = make_tydesc(TyTag::String);
    let string_val = String { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };

    // Create an Error by using the Data representation.
    let error_data = Data::from_pointers(&inner_tydesc, &string_val as *const String as *const u8);
    // Transmute to Error (same layout).
    let error_val: Error = unsafe { std::mem::transmute(error_data) };

    let tydesc = make_tydesc(TyTag::Error);
    let data = Data::from_error(&error_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Error);
    assert_eq!(data.as_error(), Some(&error_val as *const Error));
}

// ==================== tydesc() Tests ====================

#[test]
fn test_tydesc_two_pointers() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_int(&int_val, &tydesc);
    assert_eq!(data.tydesc(), &tydesc as *const TyDesc);
}

#[test]
fn test_tydesc_inline_with_tydesc() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(42, &tydesc);
    assert_eq!(data.tydesc(), &tydesc as *const TyDesc);
}

#[test]
fn test_tydesc_small_immediate() {
    let data = Data::from_bool(true);
    // SmallImmediate has no tydesc pointer, returns null.
    assert!(data.tydesc().is_null());
}

// ==================== value_ptr() Tests ====================

#[test]
fn test_value_ptr() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_int(&int_val, &tydesc);
    assert_eq!(data.value_ptr(), &int_val as *const Int as *const u8);
}

#[test]
fn test_value_ptr_as() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_int(&int_val, &tydesc);
    assert_eq!(data.value_ptr_as::<Int>(), &int_val as *const Int);
}

#[test]
#[should_panic(expected = "value not stored as pointer")]
fn test_value_ptr_wrong_tag() {
    let data = Data::from_bool(true);
    let _ = data.value_ptr();
}

// ==================== Wrong Type Accessor Tests ====================

#[test]
fn test_as_u8_wrong_type() {
    let data = Data::from_u32(42);
    assert_eq!(data.as_u8(), None);
}

#[test]
fn test_as_i8_wrong_type() {
    let data = Data::from_u32(42);
    assert_eq!(data.as_i8(), None);
}

#[test]
fn test_as_u16_wrong_type() {
    let data = Data::from_u32(42);
    assert_eq!(data.as_u16(), None);
}

#[test]
fn test_as_i16_wrong_type() {
    let data = Data::from_u32(42);
    assert_eq!(data.as_i16(), None);
}

#[test]
fn test_as_u32_wrong_type() {
    let data = Data::from_bool(true);
    assert_eq!(data.as_u32(), None);
}

#[test]
fn test_as_i32_wrong_type() {
    let data = Data::from_bool(true);
    assert_eq!(data.as_i32(), None);
}

#[test]
fn test_as_u64_wrong_type() {
    let tydesc = make_tydesc(TyTag::I64);
    let data = Data::from_i64(42, &tydesc);
    assert_eq!(data.as_u64(), None);
}

#[test]
fn test_as_i64_wrong_type() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(42, &tydesc);
    assert_eq!(data.as_i64(), None);
}

#[test]
fn test_as_f32_wrong_type() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(42, &tydesc);
    assert_eq!(data.as_f32(), None);
}

#[test]
fn test_as_f64_wrong_type() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(42, &tydesc);
    assert_eq!(data.as_f64(), None);
}

#[test]
fn test_as_int_wrong_type() {
    let tydesc = make_tydesc(TyTag::String);
    let string_val = String { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };
    let data = Data::from_string(&string_val, &tydesc);
    assert_eq!(data.as_int(), None);
}

#[test]
fn test_as_string_wrong_type() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_int(&int_val, &tydesc);
    assert_eq!(data.as_string(), None);
}

#[test]
fn test_as_list_wrong_type() {
    let tydesc = make_tydesc(TyTag::Set);
    let set_val = Set { root: std::ptr::null(), len: Index::ZERO };
    let data = Data::from_set(&set_val, &tydesc);
    assert_eq!(data.as_list(), None);
}

#[test]
fn test_as_map_wrong_type() {
    let tydesc = make_tydesc(TyTag::Set);
    let set_val = Set { root: std::ptr::null(), len: Index::ZERO };
    let data = Data::from_set(&set_val, &tydesc);
    assert_eq!(data.as_map(), None);
}

#[test]
fn test_as_set_wrong_type() {
    let tydesc = make_tydesc(TyTag::Map);
    let map_val = Map { root: std::ptr::null(), len: Index::ZERO };
    let data = Data::from_map(&map_val, &tydesc);
    assert_eq!(data.as_set(), None);
}

#[test]
fn test_as_option_wrong_type() {
    let tydesc = make_tydesc(TyTag::Result);
    let result_val = Result { tag: ResultTag::Ok };
    let data = Data::from_result(&result_val, &tydesc);
    assert_eq!(data.as_option(), None);
}

#[test]
fn test_as_result_wrong_type() {
    let tydesc = make_tydesc(TyTag::Option);
    let option_val = Option { tag: OptionTag::None };
    let data = Data::from_option(&option_val, &tydesc);
    assert_eq!(data.as_result(), None);
}

#[test]
fn test_as_data_wrong_type() {
    // Create error-like value.
    let inner_tydesc = make_tydesc(TyTag::String);
    let string_val = String { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };
    let error_data = Data::from_pointers(&inner_tydesc, &string_val as *const String as *const u8);
    let error_val: Error = unsafe { std::mem::transmute(error_data) };

    let tydesc = make_tydesc(TyTag::Error);
    let data = Data::from_error(&error_val, &tydesc);
    assert_eq!(data.as_data(), None);
}

#[test]
fn test_as_error_wrong_type() {
    let inner_data = Data::from_bool(true);
    let tydesc = make_tydesc(TyTag::Data);
    let data = Data::from_data(&inner_data, &tydesc);
    assert_eq!(data.as_error(), None);
}

// ==================== Error Type Tests ====================

#[test]
fn test_error_tydesc() {
    let tydesc = make_tydesc(TyTag::String);
    let string_val = String { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };
    // Create Error via transmute from Data.
    let data = Data::from_pointers(&tydesc, &string_val as *const String as *const u8);
    let error: Error = unsafe { std::mem::transmute(data) };
    assert_eq!(error.tydesc(), &tydesc as *const TyDesc);
}

#[test]
fn test_error_value_ptr() {
    let tydesc = make_tydesc(TyTag::String);
    let string_val = String { size: Index::ZERO, capacity: Index::ZERO, data: std::ptr::null_mut() };
    let data = Data::from_pointers(&tydesc, &string_val as *const String as *const u8);
    let error: Error = unsafe { std::mem::transmute(data) };
    assert_eq!(error.value_ptr(), &string_val as *const String as *const u8);
}

// ==================== TyTag Tests ====================

#[test]
fn test_needs_heap_allocation() {
    // Types that need heap allocation.
    assert!(TyTag::Int.needs_heap_allocation());
    assert!(TyTag::String.needs_heap_allocation());
    assert!(TyTag::List.needs_heap_allocation());
    assert!(TyTag::Map.needs_heap_allocation());
    assert!(TyTag::Set.needs_heap_allocation());
    assert!(TyTag::Tuple.needs_heap_allocation());
    assert!(TyTag::Struct.needs_heap_allocation());
    assert!(TyTag::Enum.needs_heap_allocation());
    assert!(TyTag::Data.needs_heap_allocation());
    assert!(TyTag::Error.needs_heap_allocation());

    // Types that don't need heap allocation.
    assert!(!TyTag::Bool.needs_heap_allocation());
    assert!(!TyTag::U8.needs_heap_allocation());
    assert!(!TyTag::I8.needs_heap_allocation());
    assert!(!TyTag::U16.needs_heap_allocation());
    assert!(!TyTag::I16.needs_heap_allocation());
    assert!(!TyTag::U32.needs_heap_allocation());
    assert!(!TyTag::I32.needs_heap_allocation());
    assert!(!TyTag::U64.needs_heap_allocation());
    assert!(!TyTag::I64.needs_heap_allocation());
    assert!(!TyTag::F32.needs_heap_allocation());
    assert!(!TyTag::F64.needs_heap_allocation());
}

#[test]
fn test_can_inline() {
    // Types that can be inlined.
    assert!(TyTag::Bool.can_inline());
    assert!(TyTag::U8.can_inline());
    assert!(TyTag::I8.can_inline());
    assert!(TyTag::U16.can_inline());
    assert!(TyTag::I16.can_inline());
    assert!(TyTag::U32.can_inline());
    assert!(TyTag::I32.can_inline());
    assert!(TyTag::U64.can_inline());
    assert!(TyTag::I64.can_inline());
    assert!(TyTag::F32.can_inline());
    assert!(TyTag::F64.can_inline());

    // Types that cannot be inlined.
    assert!(!TyTag::Int.can_inline());
    assert!(!TyTag::String.can_inline());
    assert!(!TyTag::List.can_inline());
    assert!(!TyTag::Map.can_inline());
    assert!(!TyTag::Set.can_inline());
    assert!(!TyTag::Tuple.can_inline());
    assert!(!TyTag::Struct.can_inline());
    assert!(!TyTag::Enum.can_inline());
}

// ==================== Debug Tests ====================

#[test]
fn test_debug_small_immediate() {
    let data = Data::from_bool(true);
    let debug_str = format!("{:?}", data);
    assert!(debug_str.contains("Data"));
    assert!(debug_str.contains("SmallImmediate"));
}

#[test]
fn test_debug_inline_with_tydesc() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_u64(42, &tydesc);
    let debug_str = format!("{:?}", data);
    assert!(debug_str.contains("Data"));
    assert!(debug_str.contains("InlineWithTyDesc"));
    assert!(debug_str.contains("U64"));
}

#[test]
fn test_debug_two_pointers() {
    let tydesc = make_tydesc(TyTag::Int);
    let int_val = Int { data: std::ptr::null(), size_and_sign: 0, capacity: Index::ZERO };
    let data = Data::from_int(&int_val, &tydesc);
    let debug_str = format!("{:?}", data);
    assert!(debug_str.contains("Data"));
    assert!(debug_str.contains("TwoPointers"));
    assert!(debug_str.contains("Int"));
}

// ==================== Inline64 Direct Tests ====================

#[test]
fn test_from_inline64_direct() {
    let tydesc = make_tydesc(TyTag::U64);
    let data = Data::from_inline64(&tydesc, 0xDEADBEEF);
    assert_eq!(data.tag(), anypack::Tag::InlineWithTyDesc);
    // Verify by extracting as u64.
    assert_eq!(data.as_u64(), Some(0xDEADBEEF));
}

// ==================== from_immediate Direct Tests ====================

#[test]
fn test_from_immediate_direct() {
    let data = Data::from_immediate(12345, TyTag::U32);
    assert_eq!(data.tag(), anypack::Tag::SmallImmediate);
    assert_eq!(data.tytag(), TyTag::U32);
}

// ==================== Option Some Variant ====================

#[test]
fn test_from_option_some() {
    let tydesc = make_tydesc(TyTag::Option);
    let option_val = Option { tag: OptionTag::Some };
    let data = Data::from_option(&option_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Option);
    let extracted = data.as_option().unwrap();
    unsafe {
        assert_eq!((*extracted).tag, OptionTag::Some);
    }
}

// ==================== Result Err Variant ====================

#[test]
fn test_from_result_err() {
    let tydesc = make_tydesc(TyTag::Result);
    let result_val = Result { tag: ResultTag::Err };
    let data = Data::from_result(&result_val, &tydesc);
    assert_eq!(data.tag(), anypack::Tag::TwoPointers);
    assert_eq!(data.tytag(), TyTag::Result);
    let extracted = data.as_result().unwrap();
    unsafe {
        assert_eq!((*extracted).tag, ResultTag::Err);
    }
}

// ==================== Additional TyTag Coverage ====================

#[test]
fn test_tytag_option_result() {
    assert!(!TyTag::Option.needs_heap_allocation());
    assert!(!TyTag::Result.needs_heap_allocation());
    assert!(!TyTag::Option.can_inline());
    assert!(!TyTag::Result.can_inline());
}

#[test]
fn test_tytag_tensor() {
    assert!(!TyTag::Tensor.needs_heap_allocation());
    assert!(!TyTag::Tensor.can_inline());
}
