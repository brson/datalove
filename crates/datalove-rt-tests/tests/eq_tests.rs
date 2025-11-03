//! Tests for the eq runtime function.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;

#[salsa::tracked]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr;
    let resolved = datalove_datalit::resolve::resolve_names(db, parsed, parse_result.expr_spans);
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

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_bool_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@true")?;
    let typechecked_b = compile_str(&db, "@false")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_u32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_u32_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@99")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_f32_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@3.14")?;
    let typechecked_b = compile_str(&db, "@3.14")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_f32_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@3.14")?;
    let typechecked_b = compile_str(&db, "@2.71")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_string_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"hello""#)?;
    let typechecked_b = compile_str(&db, r#"@"hello""#)?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_string_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"@"hello""#)?;
    let typechecked_b = compile_str(&db, r#"@"world""#)?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_int_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @int / @42")?;
    let typechecked_b = compile_str(&db, ": @int / @42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_int_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @int / @42")?;
    let typechecked_b = compile_str(&db, ": @int / @99")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @42)")?;
    let typechecked_b = compile_str(&db, "@(@true, @42)")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @42)")?;
    let typechecked_b = compile_str(&db, "@(@true, @99)")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_nested_tuple_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@(@1, @2), @(@3, @4))")?;
    let typechecked_b = compile_str(&db, "@(@(@1, @2), @(@3, @4))")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_nested_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@(@1, @2), @(@3, @4))")?;
    let typechecked_b = compile_str(&db, "@(@(@1, @2), @(@3, @5))")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_struct_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile_str(&db, "@{x = @1, y = @2}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_struct_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@{x = @1, y = @2}")?;
    let typechecked_b = compile_str(&db, "@{x = @1, y = @3}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_list_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @3]")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_list_not_equals_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2, @4]")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_list_not_equals_lengths() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@[@1, @2, @3]")?;
    let typechecked_b = compile_str(&db, "@[@1, @2]")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_enum_no_payload_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_enum_no_payload_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Status { Ok, Error } / @enum Ok")?;
    let typechecked_b = compile_str(&db, ": @enum Status { Ok, Error } / @enum Error")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_enum_with_payload_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
    let typechecked_b = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_enum_with_payload_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;
    let typechecked_b = compile_str(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@99)")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_option_none_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @none")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_option_some_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_option_none_vs_some() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @none")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_option_some_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @?@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @?@u32 / @99")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_result_ok_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_result_ok_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, ": @!@u32 / @99")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_result_err_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#": @!@u32 / @error "err5""#)?;
    let typechecked_b = compile_str(&db, r#": @!@u32 / @error "err5""#)?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_result_err_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#": @!@u32 / @error "err5""#)?;
    let typechecked_b = compile_str(&db, r#": @!@u32 / @error "err10""#)?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_result_ok_vs_err() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @!@u32 / @42")?;
    let typechecked_b = compile_str(&db, r#": @!@u32 / @error "err5""#)?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_type_mismatch() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@42")?;
    let typechecked_b = compile_str(&db, "@true")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Error));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

// Map equality tests

#[test]
fn test_eq_map_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_map_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
#[ignore] // TODO: B-tree equality with different insertion orders not yet implemented
fn test_eq_map_equals_different_literal_order() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, "@map{@30 = @300, @10 = @100, @20 = @200}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_map_not_equals_different_keys() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @30 = @300}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_map_not_equals_different_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @999}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_map_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200, @30 = @300}")?;
    let typechecked_b = compile_str(&db, ": @map<@u32, @u32> / @map{@10 = @100, @20 = @200}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

// Set equality tests

#[test]
fn test_eq_set_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_set_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, "@set{@10, @20, @30}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
#[ignore] // TODO: B-tree equality with different insertion orders not yet implemented
fn test_eq_set_equals_different_literal_order() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, "@set{@30, @10, @20}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_set_not_equals_different_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{@10, @20, @40}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}

#[test]
fn test_eq_set_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, ": @set<@u32> / @set{@10, @20}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let (ptr_a, tydesc_a) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_a)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };
    let (ptr_b, tydesc_b) = {
        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked_b)?;
        (inst.ptr, inst.tydesc.as_ptr())
    };

    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_a as *mut u8,
            tydesc_a,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_a,
            1,
            ptr_a as *mut u8,
        );
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            ptr_b as *mut u8,
            tydesc_b,
        );
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            tydesc_b,
            1,
            ptr_b as *mut u8,
        );
    }
    Ok(())
}
