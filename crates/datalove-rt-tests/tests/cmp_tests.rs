//! Tests for the cmp runtime function.

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
fn test_cmp_bool_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@false")?;
    let typechecked_b = compile_str(&db, "@true")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_bool_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@true")?;
    let typechecked_b = compile_str(&db, "@true")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_u32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@99")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_u32_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@99")?;
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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_u32_equal() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_f32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@2.71")?;
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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_f32_equal() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_string_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"apple""#)?;
    let typechecked_b = compile_str(&db, r#"@"banana""#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_string_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"zebra""#)?;
    let typechecked_b = compile_str(&db, r#"@"apple""#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_string_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"hello""#)?;
    let typechecked_b = compile_str(&db, r#"@"hello""#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_int_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @int / @42")?;
    let typechecked_b = compile_str(&db, ": @int / @99")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_int_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @int / @42")?;
    let typechecked_b = compile_str(&db, ": @int / @42")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_tuple_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@1, @2)")?;
    let typechecked_b = compile_str(&db, "@(@1, @3)")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_tuple_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@1, @2)")?;
    let typechecked_b = compile_str(&db, "@(@1, @2)")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_tuple_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@2, @1)")?;
    let typechecked_b = compile_str(&db, "@(@1, @99)")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_struct_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile_str(&db, "@{x = @1, y = @3}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_struct_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile_str(&db, "@{x = @1, y = @2}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_list_less_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @4]")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_list_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @3]")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_list_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @3]")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_enum_less_by_discriminant() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile_str(&db, ": @enum Status { Ok, Error } / @enum Error")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_enum_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_enum_with_payload_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@10)")?;
    let typechecked_b = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@20)")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_option_none_vs_none() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @none")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_option_none_less_than_some() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @42")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_option_some_greater_than_none() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @none")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_option_some_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @42")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_option_some_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @10")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @20")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_cmp_result_err_less_than_ok() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @err")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @42")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_cmp_result_ok_greater_than_err() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @err")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_cmp_result_ok_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @42")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_cmp_result_ok_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @10")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @20")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_cmp_result_err_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @err")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @err")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
#[ignore] // Result type instantiation not yet implemented
fn test_cmp_result_err_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @err")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @err")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_type_mismatch() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@true")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Error));
    Ok(())
}

#[test]
fn test_cmp_map_empty_vs_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@string, @u32> / @map{}")?;
    let typechecked_b = compile_str(&db, ": @map<@string, @u32> / @map{}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_map_empty_vs_nonempty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@string, @u32> / @map{}")?;
    let typechecked_b = compile_str(&db, r#"@map{"a" = @1}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_map_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@map{"a" = @1, "b" = @2}"#)?;
    let typechecked_b = compile_str(&db, r#"@map{"a" = @1, "b" = @2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_map_less_by_key() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@map{"a" = @1, "b" = @2}"#)?;
    let typechecked_b = compile_str(&db, r#"@map{"a" = @1, "c" = @2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_map_less_by_value() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@map{"a" = @1, "b" = @2}"#)?;
    let typechecked_b = compile_str(&db, r#"@map{"a" = @1, "b" = @3}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_map_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@map{"a" = @1}"#)?;
    let typechecked_b = compile_str(&db, r#"@map{"a" = @1, "b" = @2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_map_greater_by_key() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@map{"a" = @1, "c" = @2}"#)?;
    let typechecked_b = compile_str(&db, r#"@map{"a" = @1, "b" = @2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_set_empty_vs_empty() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_set_empty_vs_nonempty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{}")?;
    let typechecked_b = compile_str(&db, "@set{@1}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_set_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@1, @2, @3}")?;
    let typechecked_b = compile_str(&db, "@set{@1, @2, @3}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    Ok(())
}

#[test]
fn test_cmp_set_less_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@1, @2}")?;
    let typechecked_b = compile_str(&db, "@set{@1, @3}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_set_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@1, @2}")?;
    let typechecked_b = compile_str(&db, "@set{@1, @2, @3}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}

#[test]
fn test_cmp_set_greater_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@1, @3}")?;
    let typechecked_b = compile_str(&db, "@set{@1, @2}")?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    Ok(())
}

#[test]
fn test_cmp_set_with_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@set{"apple", "banana"}"#)?;
    let typechecked_b = compile_str(&db, r#"@set{"apple", "cherry"}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    Ok(())
}
