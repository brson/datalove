//! Tests for the eq_unique runtime function.
//! This tests bitwise equality for floats (all bit patterns distinct).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rt_local::RtLocal;

fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    let parsed = datalove_datalit::parser::parse(db, source);
    let resolved = datalove_datalit::resolve::resolve_names(db, parsed);
    let typechecked = datalove_datalit::tycheck::type_check(db, parsed, resolved);
    Ok(typechecked)
}

#[test]
fn test_eq_unique_f32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@3.14")?;
    let typechecked_b = compile(&db, "@3.14")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq_unique(
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
fn test_eq_unique_u32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@42")?;
    let typechecked_b = compile(&db, "@42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq_unique(
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
fn test_eq_unique_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@true, @3.14)")?;
    let typechecked_b = compile(&db, "@(@true, @3.14)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq_unique(
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
fn test_eq_unique_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@(@true, @3.14)")?;
    let typechecked_b = compile(&db, "@(@true, @2.71)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst_a = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_a)?;
    let inst_b = instantiate2::instantiate_value(&db, &mut rt, &mut tydesc_table, typechecked_b)?;

    let result = unsafe {
        datalove_rt::dtlv_rti_eq_unique(
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
