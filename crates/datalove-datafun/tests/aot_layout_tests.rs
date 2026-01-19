//! Tests that verify AOT layout calculations match the interpreter's TyDesc layouts.
//!
//! These tests ensure ABI compatibility between AOT-compiled code and the runtime
//! by comparing the AOT crate's computed layouts against the interpreter's TyDescTable.

use datalove_datafun_aot_cranelift::types::{ir_type_to_cranelift, TypeLayout};
use datalove_datafun_interp::IrTyDescTable;
use datalove_datafun_ir::IrType;
use datalove_rtdt::TyDescRef;

/// Get layout from interpreter's TyDescTable for an IrType.
fn interp_layout(ty: &IrType) -> TypeLayout {
    let mut table = IrTyDescTable::new();
    let tydesc_ptr = table.get_or_create(ty);
    let tydesc = unsafe { TyDescRef::from_ptr(tydesc_ptr) };
    TypeLayout {
        size: tydesc.size(),
        align: tydesc.align(),
    }
}

/// Get layout from AOT crate for an IrType.
fn aot_layout(ty: &IrType) -> TypeLayout {
    ir_type_to_cranelift(ty).layout()
}

/// Assert that AOT and interpreter layouts match for a given type.
fn assert_layouts_match(ty: &IrType) {
    let interp = interp_layout(ty);
    let aot = aot_layout(ty);
    assert_eq!(
        interp, aot,
        "Layout mismatch for {:?}:\n  interp: size={}, align={}\n  aot:    size={}, align={}",
        ty, interp.size, interp.align, aot.size, aot.align
    );
}

// Scalar types

#[test]
fn test_bool_layout_matches() {
    assert_layouts_match(&IrType::Bool);
}

#[test]
fn test_u8_layout_matches() {
    assert_layouts_match(&IrType::U8);
}

#[test]
fn test_u16_layout_matches() {
    assert_layouts_match(&IrType::U16);
}

#[test]
fn test_u32_layout_matches() {
    assert_layouts_match(&IrType::U32);
}

#[test]
fn test_u64_layout_matches() {
    assert_layouts_match(&IrType::U64);
}

#[test]
fn test_i8_layout_matches() {
    assert_layouts_match(&IrType::I8);
}

#[test]
fn test_i16_layout_matches() {
    assert_layouts_match(&IrType::I16);
}

#[test]
fn test_i32_layout_matches() {
    assert_layouts_match(&IrType::I32);
}

#[test]
fn test_i64_layout_matches() {
    assert_layouts_match(&IrType::I64);
}

#[test]
fn test_f32_layout_matches() {
    assert_layouts_match(&IrType::F32);
}

// Runtime types

#[test]
fn test_int_layout_matches() {
    assert_layouts_match(&IrType::Int);
}

#[test]
fn test_string_layout_matches() {
    assert_layouts_match(&IrType::String);
}

#[test]
fn test_data_layout_matches() {
    assert_layouts_match(&IrType::Data);
}

#[test]
fn test_error_layout_matches() {
    assert_layouts_match(&IrType::Error);
}

// Collection types

#[test]
fn test_list_u32_layout_matches() {
    assert_layouts_match(&IrType::List(Box::new(IrType::U32)));
}

#[test]
fn test_list_string_layout_matches() {
    assert_layouts_match(&IrType::List(Box::new(IrType::String)));
}

#[test]
fn test_set_u32_layout_matches() {
    assert_layouts_match(&IrType::Set(Box::new(IrType::U32)));
}

#[test]
fn test_map_string_u32_layout_matches() {
    assert_layouts_match(&IrType::Map(
        Box::new(IrType::String),
        Box::new(IrType::U32),
    ));
}

#[test]
fn test_tensor_f32_rank2_layout_matches() {
    assert_layouts_match(&IrType::Tensor(Box::new(IrType::F32), 2));
}

// Tuple types

#[test]
fn test_unit_layout_matches() {
    assert_layouts_match(&IrType::Unit);
}

#[test]
fn test_tuple_empty_layout_matches() {
    assert_layouts_match(&IrType::Tuple(vec![]));
}

#[test]
fn test_tuple_single_layout_matches() {
    assert_layouts_match(&IrType::Tuple(vec![IrType::U32]));
}

#[test]
fn test_tuple_u32_u64_layout_matches() {
    assert_layouts_match(&IrType::Tuple(vec![IrType::U32, IrType::U64]));
}

#[test]
fn test_tuple_mixed_alignment_layout_matches() {
    // (u8, u32, u8, u64)
    assert_layouts_match(&IrType::Tuple(vec![
        IrType::U8,
        IrType::U32,
        IrType::U8,
        IrType::U64,
    ]));
}

#[test]
fn test_tuple_with_string_layout_matches() {
    assert_layouts_match(&IrType::Tuple(vec![IrType::String, IrType::U32]));
}

#[test]
fn test_tuple_nested_layout_matches() {
    assert_layouts_match(&IrType::Tuple(vec![
        IrType::Tuple(vec![IrType::U32, IrType::U64]),
        IrType::U8,
    ]));
}

// Struct types

#[test]
fn test_struct_single_field_layout_matches() {
    assert_layouts_match(&IrType::Struct(vec![
        ("x".into(), IrType::U32),
    ]));
}

#[test]
fn test_struct_two_fields_layout_matches() {
    assert_layouts_match(&IrType::Struct(vec![
        ("x".into(), IrType::U32),
        ("y".into(), IrType::U64),
    ]));
}

#[test]
fn test_struct_with_string_layout_matches() {
    assert_layouts_match(&IrType::Struct(vec![
        ("name".into(), IrType::String),
        ("value".into(), IrType::U32),
    ]));
}

// Enum types

#[test]
fn test_enum_no_payloads_layout_matches() {
    assert_layouts_match(&IrType::Enum(vec![
        ("A".into(), None),
        ("B".into(), None),
        ("C".into(), None),
    ]));
}

#[test]
fn test_enum_with_u32_payload_layout_matches() {
    assert_layouts_match(&IrType::Enum(vec![
        ("None".into(), None),
        ("Some".into(), Some(IrType::U32)),
    ]));
}

#[test]
fn test_enum_with_u64_payload_layout_matches() {
    assert_layouts_match(&IrType::Enum(vec![
        ("None".into(), None),
        ("Some".into(), Some(IrType::U64)),
    ]));
}

#[test]
fn test_enum_mixed_payloads_layout_matches() {
    assert_layouts_match(&IrType::Enum(vec![
        ("None".into(), None),
        ("SomeU32".into(), Some(IrType::U32)),
        ("SomeString".into(), Some(IrType::String)),
    ]));
}

// Option types

#[test]
fn test_option_u32_layout_matches() {
    assert_layouts_match(&IrType::Option(Box::new(IrType::U32)));
}

#[test]
fn test_option_u64_layout_matches() {
    assert_layouts_match(&IrType::Option(Box::new(IrType::U64)));
}

#[test]
fn test_option_string_layout_matches() {
    assert_layouts_match(&IrType::Option(Box::new(IrType::String)));
}

#[test]
fn test_option_tuple_layout_matches() {
    assert_layouts_match(&IrType::Option(Box::new(
        IrType::Tuple(vec![IrType::U32, IrType::U64])
    )));
}

// Result types

#[test]
fn test_result_u32_layout_matches() {
    assert_layouts_match(&IrType::Result(Box::new(IrType::U32)));
}

#[test]
fn test_result_u64_layout_matches() {
    assert_layouts_match(&IrType::Result(Box::new(IrType::U64)));
}

#[test]
fn test_result_string_layout_matches() {
    assert_layouts_match(&IrType::Result(Box::new(IrType::String)));
}

// Nested composite types

#[test]
fn test_option_of_list_layout_matches() {
    assert_layouts_match(&IrType::Option(Box::new(
        IrType::List(Box::new(IrType::U32))
    )));
}

#[test]
fn test_list_of_option_layout_matches() {
    assert_layouts_match(&IrType::List(Box::new(
        IrType::Option(Box::new(IrType::U32))
    )));
}

#[test]
fn test_result_of_option_layout_matches() {
    assert_layouts_match(&IrType::Result(Box::new(
        IrType::Option(Box::new(IrType::String))
    )));
}

#[test]
fn test_tuple_of_options_layout_matches() {
    assert_layouts_match(&IrType::Tuple(vec![
        IrType::Option(Box::new(IrType::U32)),
        IrType::Option(Box::new(IrType::String)),
    ]));
}

#[test]
fn test_struct_with_option_field_layout_matches() {
    assert_layouts_match(&IrType::Struct(vec![
        ("id".into(), IrType::U64),
        ("name".into(), IrType::Option(Box::new(IrType::String))),
    ]));
}

#[test]
fn test_deeply_nested_layout_matches() {
    // Result<Option<(String, List<u32>)>>
    assert_layouts_match(&IrType::Result(Box::new(
        IrType::Option(Box::new(
            IrType::Tuple(vec![
                IrType::String,
                IrType::List(Box::new(IrType::U32)),
            ])
        ))
    )));
}
