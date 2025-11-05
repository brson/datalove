//! Tests for the eq_unique runtime function.
//! This tests bitwise equality for floats (all bit patterns distinct).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::impls::rt_local::RtLocal;

#[salsa::tracked]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr;
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed);
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

fn compile_str<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    Ok(compile(db, source))
}

#[test]
fn test_eq_unique_f32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@3.14")?;
    let typechecked_b = compile_str(&db, "@3.14")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_u32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@42")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @3.14)")?;
    let typechecked_b = compile_str(&db, "@(@true, @3.14)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @3.14)")?;
    let typechecked_b = compile_str(&db, "@(@true, @2.71)")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    Ok(())
}

// Map equality tests

#[test]
fn test_eq_unique_map_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_map_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_map_not_equals_different_keys() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @30 = @300}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_unique_map_not_equals_different_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @999}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_unique_map_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    Ok(())
}

// Set equality tests

#[test]
fn test_eq_unique_set_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_set_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, "@set{@10, @20, @30}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    Ok(())
}

#[test]
fn test_eq_unique_set_not_equals_different_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{@10, @20, @40}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    Ok(())
}

#[test]
fn test_eq_unique_set_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{@10, @20}")?;

    let mut rt = RtLocal::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, &mut *rt as *mut _ as datalove_rt::c::LocalRtHandle, &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq_unique(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    Ok(())
}
