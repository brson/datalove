//! Tests for the eq_unique runtime function.
//! This tests bitwise equality for floats (all bit patterns distinct).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rt_local::RtLocal;

fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr;
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

// Map equality tests

#[test]
fn test_eq_unique_map_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @map<@u32, @u32> / @map{}")?;
    let typechecked_b = compile(&db, ": @map<@u32, @u32> / @map{}")?;

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
fn test_eq_unique_map_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;

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
fn test_eq_unique_map_not_equals_different_keys() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile(&db, ": @map<@u32, @u32> / @map{@10 = @100, @30 = @300}")?;

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

#[test]
fn test_eq_unique_map_not_equals_different_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @999}")?;

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

#[test]
fn test_eq_unique_map_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;

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

// Set equality tests

#[test]
fn test_eq_unique_set_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @set<@u32> / @set{}")?;
    let typechecked_b = compile(&db, ": @set<@u32> / @set{}")?;

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
fn test_eq_unique_set_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile(&db, "@set{@10, @20, @30}")?;

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
fn test_eq_unique_set_not_equals_different_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile(&db, ": @set<@u32> / @set{@10, @20, @40}")?;

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

#[test]
fn test_eq_unique_set_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile(&db, ": @set<@u32> / @set{@10, @20}")?;

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
