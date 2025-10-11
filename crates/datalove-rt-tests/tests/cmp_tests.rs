//! Tests for the cmp runtime function.

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
fn test_cmp_bool_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@false")?;
    let typechecked_b = compile(&db, "@true")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_bool_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@true")?;
    let typechecked_b = compile(&db, "@true")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_u32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@99")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_u32_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@99")?;
    let typechecked_b = compile(&db, "@42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_u32_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_f32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@2.71")?;
    let typechecked_b = compile(&db, "@3.14")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_f32_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@3.14")?;
    let typechecked_b = compile(&db, "@3.14")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_string_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#"@"apple""#)?;
    let typechecked_b = compile(&db, r#"@"banana""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_string_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#"@"zebra""#)?;
    let typechecked_b = compile(&db, r#"@"apple""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_string_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, r#"@"hello""#)?;
    let typechecked_b = compile(&db, r#"@"hello""#)?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_int_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @int / @42")?;
    let typechecked_b = compile(&db, ": @int / @99")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_int_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @int / @42")?;
    let typechecked_b = compile(&db, ": @int / @42")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_tuple_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@1, @2)")?;
    let typechecked_b = compile(&db, "@(@1, @3)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_tuple_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@1, @2)")?;
    let typechecked_b = compile(&db, "@(@1, @2)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_tuple_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@2, @1)")?;
    let typechecked_b = compile(&db, "@(@1, @99)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_struct_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile(&db, "@{x = @1, y = @3}")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_struct_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile(&db, "@{x = @1, y = @2}")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_list_less_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile(&db, "@[@1, @2, @4]")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_list_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@[@1, @2]")?;
    let typechecked_b = compile(&db, "@[@1, @2, @3]")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_list_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile(&db, "@[@1, @2, @3]")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_enum_less_by_discriminant() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile(&db, ": @enum Status { Ok, Error } / @enum Error")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_enum_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_enum_with_payload_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@10)")?;
    let typechecked_b = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@20)")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_type_mismatch() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@true")?;

    let (_tydesc_table_a, _value_heap_a, inst_a) = instantiate::instantiate_value(&db, typechecked_a)?;
    let (_tydesc_table_b, _value_heap_b, inst_b) = instantiate::instantiate_value(&db, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_cmp(
            std::ptr::null_mut(),
            inst_a.value,
            inst_a.tydesc,
            inst_b.value,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtOrdering::Error));
    Ok(())
}
