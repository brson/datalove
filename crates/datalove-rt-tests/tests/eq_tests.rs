//! Tests for the eq runtime function.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;

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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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
        datalove_rt::c::dtlv_rti_eq_local(
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

// ==================== Property-Based Tests ====================

use proptest::prelude::*;
use datalove_datalit::ast_gen::*;

proptest! {
    #![proptest_config(ProptestConfig {
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    /// Property: Reflexivity - for all x, eq(x, x) = Equals.
    #[test]
    fn proptest_eq_reflexive(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty(), "Generated expression should typecheck");

        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // DEBUG: Print what type we're comparing
        eprintln!("Testing seed {}, type_tag: {:?}", seed, inst.tydesc.as_ref().type_tag);

        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                inst.ptr,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Reflexivity: eq(x, x) should always be Equals");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt.handle(),
                inst.ptr as *mut u8,
                inst.tydesc.as_ptr(),
            );
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt.handle(),
                inst.tydesc.as_ptr(),
                1,
                inst.ptr as *mut u8,
            );
        }
    }

    /// Property: Symmetry - for all x, y, eq(x, y) = eq(y, x).
    #[test]
    fn proptest_eq_symmetric(seed1 in any::<u64>(), seed2 in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr1 = gen_expr_full_seeded(&db, seed1, config.clone());
        let expr2 = gen_expr_full_seeded(&db, seed2, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);

        let resolved1 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr1);
        let typechecked1 = datalove_datalit::tycheck::type_check(&db, expr1, resolved1);
        prop_assert!(typechecked1.errors(&db).is_empty());

        let resolved2 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr2);
        let typechecked2 = datalove_datalit::tycheck::type_check(&db, expr2, resolved2);
        prop_assert!(typechecked2.errors(&db).is_empty());

        let inst1 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked1)
            .expect("Should instantiate");
        let (ptr1, tydesc1) = (inst1.ptr, inst1.tydesc.as_ptr());
        drop(inst1);

        let inst2 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked2)
            .expect("Should instantiate");
        let (ptr2, tydesc2) = (inst2.ptr, inst2.tydesc.as_ptr());
        drop(inst2);

        let result_xy = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let result_yx = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                ptr2,
                tydesc2,
                ptr1,
                tydesc1,
            )
        };

        prop_assert_eq!(result_xy, result_yx, "Symmetry: eq(x, y) should equal eq(y, x)");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), ptr1 as *mut u8, tydesc1);
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), tydesc1, 1, ptr1 as *mut u8);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), ptr2 as *mut u8, tydesc2);
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), tydesc2, 1, ptr2 as *mut u8);
        }
    }

    /// Property: Consistency with clone - for all x, eq(x, clone(x)) = Equals.
    #[test]
    fn proptest_eq_consistency_with_clone(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Clone the value into a buffer.
        let tydesc = inst.tydesc.as_ref();
        let mut clone_buffer = vec![0u8; tydesc.size as usize];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_mut_ptr(),
                inst.tydesc.as_ptr(),
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Clone consistency: eq(x, clone(x)) should be Equals");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
        }
    }

    /// Property: Numeric boundary testing with MIN/MAX values.
    #[test]
    fn proptest_eq_numeric_boundaries(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            numeric_strategy: NumericStrategy::CornerCases,
            type_weights: TypeWeights::default(),
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Test reflexivity with boundary values.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                inst.ptr,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Boundary values: eq(x, x) should be Equals");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
        }
    }

    /// Property: Moderate structures with 100-200 elements, depth 3-4.
    #[test]
    fn proptest_eq_moderate_structures(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            max_collection_size: 30,
            max_depth: 3,
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Test reflexivity with moderate-sized structures.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                inst.ptr,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Moderate structures: eq(x, x) should be Equals");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
        }
    }
}
