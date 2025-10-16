//! Tests for the eq runtime function.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate};

fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    let parsed = datalove_datalit::parser::parse(db, source);
    let resolved = datalove_datalit::resolve::resolve_names(db, parsed);
    let typechecked = datalove_datalit::tycheck::type_check(db, parsed, resolved);
    Ok(typechecked)
}

#[test]
fn test_eq_bool_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@true")?;
    let typechecked_b = compile(&db, "@true")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_bool_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@true")?;
    let typechecked_b = compile(&db, "@false")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_u32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_u32_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@99")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_f32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@3.14")?;
    let typechecked_b = compile(&db, "@3.14")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_f32_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@3.14")?;
    let typechecked_b = compile(&db, "@2.71")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_string_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#"@"hello""#)?;
    let typechecked_b = compile(&db, r#"@"hello""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_string_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#"@"hello""#)?;
    let typechecked_b = compile(&db, r#"@"world""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_int_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @int / @42")?;
    let typechecked_b = compile(&db, ": @int / @42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_int_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @int / @42")?;
    let typechecked_b = compile(&db, ": @int / @99")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@true, @42)")?;
    let typechecked_b = compile(&db, "@(@true, @42)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@true, @42)")?;
    let typechecked_b = compile(&db, "@(@true, @99)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_nested_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@(@1, @2), @(@3, @4))")?;
    let typechecked_b = compile(&db, "@(@(@1, @2), @(@3, @4))")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_nested_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@(@1, @2), @(@3, @4))")?;
    let typechecked_b = compile(&db, "@(@(@1, @2), @(@3, @5))")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_struct_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile(&db, "@{x = @1, y = @2}")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_struct_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile(&db, "@{x = @1, y = @3}")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_list_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile(&db, "@[@1, @2, @3]")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_list_not_equals_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile(&db, "@[@1, @2, @4]")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_list_not_equals_lengths() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile(&db, "@[@1, @2]")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_enum_no_payload_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_enum_no_payload_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile(&db, ": @enum Status { Ok, Error } / @enum Error")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_enum_with_payload_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
    let typechecked_b = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_enum_with_payload_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
    let typechecked_b = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@99)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_option_none_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile(&db, ": @?@u32 / @none")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_option_some_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile(&db, ": @?@u32 / @42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_option_none_vs_some() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile(&db, ": @?@u32 / @42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_option_some_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile(&db, ": @?@u32 / @99")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_eq_result_ok_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile(&db, ": @!@u32 / @42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_eq_result_ok_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile(&db, ": @!@u32 / @99")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_eq_result_err_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#": @!@u32 / @error "err5""#)?;
    let typechecked_b = compile(&db, r#": @!@u32 / @error "err5""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_eq_result_err_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#": @!@u32 / @error "err5""#)?;
    let typechecked_b = compile(&db, r#": @!@u32 / @error "err10""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_eq_result_ok_vs_err() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile(&db, r#": @!@u32 / @error "err5""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_type_mismatch() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@true")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Error));
    Ok(())
}
