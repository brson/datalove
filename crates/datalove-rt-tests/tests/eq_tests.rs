//! Tests for the eq runtime function.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rt_local::RtLocal;

#[salsa::tracked]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr;
    let resolved = datalove_datalit::resolve::resolve_names(db, parsed);
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

fn compile_str<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    Ok(compile(db, source))
}

#[test]
fn test_eq_bool_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@true")?;
    let typechecked_b = compile_str(&db, "@true")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_bool_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@true")?;
    let typechecked_b = compile_str(&db, "@false")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_u32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_u32_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@99")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_f32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@3.14")?;
    let typechecked_b = compile_str(&db, "@3.14")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_f32_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@3.14")?;
    let typechecked_b = compile_str(&db, "@2.71")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_string_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"hello""#)?;
    let typechecked_b = compile_str(&db, r#"@"hello""#)?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_string_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"hello""#)?;
    let typechecked_b = compile_str(&db, r#"@"world""#)?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_int_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @int / @42")?;
    let typechecked_b = compile_str(&db, ": @int / @42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_int_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @int / @42")?;
    let typechecked_b = compile_str(&db, ": @int / @99")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @42)")?;
    let typechecked_b = compile_str(&db, "@(@true, @42)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @42)")?;
    let typechecked_b = compile_str(&db, "@(@true, @99)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_nested_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@(@1, @2), @(@3, @4))")?;
    let typechecked_b = compile_str(&db, "@(@(@1, @2), @(@3, @4))")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_nested_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@(@1, @2), @(@3, @4))")?;
    let typechecked_b = compile_str(&db, "@(@(@1, @2), @(@3, @5))")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_struct_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile_str(&db, "@{x = @1, y = @2}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_struct_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile_str(&db, "@{x = @1, y = @3}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_list_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @3]")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_list_not_equals_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @4]")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_list_not_equals_lengths() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2]")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_enum_no_payload_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_enum_no_payload_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile_str(&db, ": @enum Status { Ok, Error } / @enum Error")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_enum_with_payload_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
    let typechecked_b = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_enum_with_payload_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
    let typechecked_b = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@99)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_option_none_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @none")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_option_some_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_option_none_vs_some() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_option_some_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @99")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_result_ok_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_result_ok_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @99")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_result_err_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#": @!@u32 / @error "err5""#)?;
    let typechecked_b = compile_str(&db, r#": @!@u32 / @error "err5""#)?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_result_err_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#": @!@u32 / @error "err5""#)?;
    let typechecked_b = compile_str(&db, r#": @!@u32 / @error "err10""#)?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_result_ok_vs_err() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, r#": @!@u32 / @error "err5""#)?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_type_mismatch() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@true")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Error));
    Ok(())
}

// Map equality tests

#[test]
fn test_eq_map_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_map_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
#[ignore] // TODO: B-tree equality with different insertion orders not yet implemented
fn test_eq_map_equals_different_literal_order() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, "@map{@30 = @300, @10 = @100, @20 = @200}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_map_not_equals_different_keys() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @30 = @300}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_map_not_equals_different_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @999}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_map_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

// Set equality tests

#[test]
fn test_eq_set_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_set_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, "@set{@10, @20, @30}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
#[ignore] // TODO: B-tree equality with different insertion orders not yet implemented
fn test_eq_set_equals_different_literal_order() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, "@set{@30, @10, @20}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_set_not_equals_different_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{@10, @20, @40}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_set_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{@10, @20}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst_a.ptr,
            inst_a.tydesc,
            inst_b.ptr,
            inst_b.tydesc,
        )
    };

    assert!(matches!(result, datalove_rt::RtEq::NotEquals));
    Ok(())
}
