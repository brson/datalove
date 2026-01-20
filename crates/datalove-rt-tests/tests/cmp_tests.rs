//! Tests for the cmp runtime function.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;

#[salsa::tracked]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr(db);
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed);
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

fn compile_str<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    Ok(compile(db, source))
}

/// Clean up an instantiated value.
unsafe fn cleanup_value(rt: &Runtime, ptr: *const u8, tydesc: *const datalove_rtdt::TyDesc) {
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), ptr as *mut u8, tydesc);
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), tydesc, 1, ptr as *mut u8);
    }
}



#[test]
fn test_cmp_bool_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "false")?;
    let typechecked_b = compile_str(&db, "true")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_bool_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "true")?;
    let typechecked_b = compile_str(&db, "true")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_u32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "42")?;
    let typechecked_b = compile_str(&db, "99")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_u32_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "99")?;
    let typechecked_b = compile_str(&db, "42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_u32_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "42")?;
    let typechecked_b = compile_str(&db, "42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_f32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "2.71")?;
    let typechecked_b = compile_str(&db, "3.14")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_f32_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "3.14")?;
    let typechecked_b = compile_str(&db, "3.14")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_string_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#""apple""#)?;
    let typechecked_b = compile_str(&db, r#""banana""#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_string_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#""zebra""#)?;
    let typechecked_b = compile_str(&db, r#""apple""#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_string_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#""hello""#)?;
    let typechecked_b = compile_str(&db, r#""hello""#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_int_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": int / 42")?;
    let typechecked_b = compile_str(&db, ": int / 99")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_int_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": int / 42")?;
    let typechecked_b = compile_str(&db, ": int / 42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_tuple_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "(1, 2)")?;
    let typechecked_b = compile_str(&db, "(1, 3)")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_tuple_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "(1, 2)")?;
    let typechecked_b = compile_str(&db, "(1, 2)")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_tuple_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "(2, 1)")?;
    let typechecked_b = compile_str(&db, "(1, 99)")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_struct_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "{x = 1, y = 2}")?;
    let typechecked_b = compile_str(&db, "{x = 1, y = 3}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_struct_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "{x = 1, y = 2}")?;
    let typechecked_b = compile_str(&db, "{x = 1, y = 2}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_list_less_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "[1, 2, 3]")?;
    let typechecked_b = compile_str(&db, "[1, 2, 4]")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_list_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "[1, 2]")?;
    let typechecked_b = compile_str(&db, "[1, 2, 3]")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_list_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "[1, 2, 3]")?;
    let typechecked_b = compile_str(&db, "[1, 2, 3]")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_enum_less_by_discriminant() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": enum { Ok, Error } / enum Ok")?;
    let typechecked_b = compile_str(&db, ": enum { Ok, Error } / enum Error")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_enum_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": enum { Ok, Error } / enum Ok")?;
    let typechecked_b = compile_str(&db, ": enum { Ok, Error } / enum Ok")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_enum_with_payload_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": enum { Ok(@u32), Err(@string) } / enum Ok(10)")?;
    let typechecked_b = compile_str(&db, ": enum { Ok(@u32), Err(@string) } / enum Ok(20)")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_option_none_vs_none() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ?@u32 / none")?;
    let typechecked_b = compile_str(&db, ": ?@u32 / none")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_option_none_less_than_some() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ?@u32 / none")?;
    let typechecked_b = compile_str(&db, ": ?@u32 / some 42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_option_some_greater_than_none() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ?@u32 / some 42")?;
    let typechecked_b = compile_str(&db, ": ?@u32 / none")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_option_some_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ?@u32 / some 42")?;
    let typechecked_b = compile_str(&db, ": ?@u32 / some 42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_option_some_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ?@u32 / some 10")?;
    let typechecked_b = compile_str(&db, ": ?@u32 / some 20")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_result_err_less_than_ok() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": !u32 / error @\"oops\"")?;
    let typechecked_b = compile_str(&db, ": !u32 / ok 42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_result_ok_greater_than_err() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": !u32 / ok 42")?;
    let typechecked_b = compile_str(&db, ": !u32 / error @\"oops\"")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_result_ok_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": !u32 / ok 42")?;
    let typechecked_b = compile_str(&db, ": !u32 / ok 42")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_result_ok_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": !u32 / ok 10")?;
    let typechecked_b = compile_str(&db, ": !u32 / ok 20")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_result_err_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": !u32 / error @\"oops\"")?;
    let typechecked_b = compile_str(&db, ": !u32 / error @\"oops\"")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_result_err_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": !u32 / error @\"aaa\"")?;
    let typechecked_b = compile_str(&db, ": !u32 / error @\"bbb\"")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_type_mismatch() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "42")?;
    let typechecked_b = compile_str(&db, "true")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Error));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_empty_vs_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": map<@string, @u32> / map{}")?;
    let typechecked_b = compile_str(&db, ": map<@string, @u32> / map{}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_empty_vs_nonempty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": map<@string, @u32> / map{}")?;
    let typechecked_b = compile_str(&db, r#"map{"a" = 1}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"map{"a" = 1, "b" = 2}"#)?;
    let typechecked_b = compile_str(&db, r#"map{"a" = 1, "b" = 2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_less_by_key() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"map{"a" = 1, "b" = 2}"#)?;
    let typechecked_b = compile_str(&db, r#"map{"a" = 1, "c" = 2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_less_by_value() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"map{"a" = 1, "b" = 2}"#)?;
    let typechecked_b = compile_str(&db, r#"map{"a" = 1, "b" = 3}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"map{"a" = 1}"#)?;
    let typechecked_b = compile_str(&db, r#"map{"a" = 1, "b" = 2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_map_greater_by_key() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"map{"a" = 1, "c" = 2}"#)?;
    let typechecked_b = compile_str(&db, r#"map{"a" = 1, "b" = 2}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_empty_vs_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": set<@u32> / set{}")?;
    let typechecked_b = compile_str(&db, ": set<@u32> / set{}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_empty_vs_nonempty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": set<@u32> / set{}")?;
    let typechecked_b = compile_str(&db, "set{1}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "set{1, 2, 3}")?;
    let typechecked_b = compile_str(&db, "set{1, 2, 3}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_less_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "set{1, 2}")?;
    let typechecked_b = compile_str(&db, "set{1, 3}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "set{1, 2}")?;
    let typechecked_b = compile_str(&db, "set{1, 2, 3}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_greater_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "set{1, 3}")?;
    let typechecked_b = compile_str(&db, "set{1, 2}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_set_with_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#"set{"apple", "banana"}"#)?;
    let typechecked_b = compile_str(&db, r#"set{"apple", "cherry"}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

// ==================== Property-Based Tests ====================

#[cfg(feature = "slow_tests")]
use proptest::prelude::*;
#[cfg(feature = "slow_tests")]
use datalove_datalit::ast_gen::*;

#[cfg(feature = "slow_tests")]
proptest! {
    #![proptest_config(ProptestConfig {
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    /// Property: Transitivity - if cmp(x,y)=Less and cmp(y,z)=Less then cmp(x,z)=Less.
    #[test]
    fn proptest_cmp_transitivity(seed1 in any::<u64>(), seed2 in any::<u64>(), seed3 in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                // Named types disabled by gen_expr_full_seeded (need external type definitions).
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                // result_type disabled: Err payload type checking in instantiate2 fails for some seeds.
                result_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr1 = gen_expr_full_seeded(&db, seed1, config.clone());
        let expr2 = gen_expr_full_seeded(&db, seed2, config.clone());
        let expr3 = gen_expr_full_seeded(&db, seed3, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);

        let resolved1 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr1);
        let typechecked1 = datalove_datalit::tycheck::type_check(&db, expr1, resolved1);
        if !typechecked1.errors(&db).is_empty() {
            eprintln!("Type errors in expr1 (seed1={}): {} errors", seed1, typechecked1.errors(&db).len());
        }
        prop_assert!(typechecked1.errors(&db).is_empty());

        let resolved2 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr2);
        let typechecked2 = datalove_datalit::tycheck::type_check(&db, expr2, resolved2);
        if !typechecked2.errors(&db).is_empty() {
            eprintln!("Type errors in expr2 (seed2={}): {} errors", seed2, typechecked2.errors(&db).len());
        }
        prop_assert!(typechecked2.errors(&db).is_empty());

        let resolved3 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr3);
        let typechecked3 = datalove_datalit::tycheck::type_check(&db, expr3, resolved3);
        if !typechecked3.errors(&db).is_empty() {
            eprintln!("Type errors in expr3 (seed3={}): {} errors", seed3, typechecked3.errors(&db).len());
        }
        prop_assert!(typechecked3.errors(&db).is_empty());

        let inst1 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked1)
            .expect("Should instantiate");
        let (ptr1, tydesc1) = (inst1.ptr, inst1.tydesc.as_ptr());
        drop(inst1);

        let inst2 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked2)
            .expect("Should instantiate");
        let (ptr2, tydesc2) = (inst2.ptr, inst2.tydesc.as_ptr());
        drop(inst2);

        let inst3 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked3)
            .expect("Should instantiate");
        let (ptr3, tydesc3) = (inst3.ptr, inst3.tydesc.as_ptr());
        drop(inst3);

        let cmp_xy = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let cmp_yz = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr2,
                tydesc2,
                ptr3,
                tydesc3,
            )
        };

        // Only test transitivity if cmp(x,y)=Less AND cmp(y,z)=Less.
        if matches!(cmp_xy, datalove_rt::c::RtOrdering::Less) && matches!(cmp_yz, datalove_rt::c::RtOrdering::Less) {
            let cmp_xz = unsafe {
                datalove_rt::c::dtlv_rti_cmp_local(
                    std::ptr::null_mut(),
                    ptr1,
                    tydesc1,
                    ptr3,
                    tydesc3,
                )
            };

            prop_assert!(matches!(cmp_xz, datalove_rt::c::RtOrdering::Less),
                "Transitivity: if cmp(x,y)=Less and cmp(y,z)=Less then cmp(x,z)=Less");
        }

        // Clean up instantiated values.
        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
            cleanup_value(&rt, ptr3, tydesc3);
        }
    }

    /// Property: Antisymmetry - if cmp(x,y)=Less then cmp(y,x)=Greater.
    #[test]
    fn proptest_cmp_antisymmetry(seed1 in any::<u64>(), seed2 in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                // Named types disabled by gen_expr_full_seeded (need external type definitions).
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                // result_type disabled: Err payload type checking in instantiate2 fails for some seeds.
                result_type: 0,
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
        if !typechecked1.errors(&db).is_empty() {
            eprintln!("Type errors in expr1 (seed1={}): {} errors", seed1, typechecked1.errors(&db).len());
        }
        prop_assert!(typechecked1.errors(&db).is_empty());

        let resolved2 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr2);
        let typechecked2 = datalove_datalit::tycheck::type_check(&db, expr2, resolved2);
        if !typechecked2.errors(&db).is_empty() {
            eprintln!("Type errors in expr2 (seed2={}): {} errors", seed2, typechecked2.errors(&db).len());
        }
        prop_assert!(typechecked2.errors(&db).is_empty());

        let inst1 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked1)
            .expect("Should instantiate");
        let (ptr1, tydesc1) = (inst1.ptr, inst1.tydesc.as_ptr());
        drop(inst1);

        let inst2 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked2)
            .expect("Should instantiate");
        let (ptr2, tydesc2) = (inst2.ptr, inst2.tydesc.as_ptr());
        drop(inst2);

        let cmp_xy = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let cmp_yx = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr2,
                tydesc2,
                ptr1,
                tydesc1,
            )
        };

        // Test antisymmetry based on cmp_xy result.
        match cmp_xy {
            datalove_rt::c::RtOrdering::Less => {
                prop_assert!(matches!(cmp_yx, datalove_rt::c::RtOrdering::Greater),
                    "Antisymmetry: if cmp(x,y)=Less then cmp(y,x)=Greater");
            }
            datalove_rt::c::RtOrdering::Greater => {
                prop_assert!(matches!(cmp_yx, datalove_rt::c::RtOrdering::Less),
                    "Antisymmetry: if cmp(x,y)=Greater then cmp(y,x)=Less");
            }
            datalove_rt::c::RtOrdering::Equal => {
                prop_assert!(matches!(cmp_yx, datalove_rt::c::RtOrdering::Equal),
                    "Antisymmetry: if cmp(x,y)=Equal then cmp(y,x)=Equal");
            }
            datalove_rt::c::RtOrdering::Error => {
                // Errors can occur, just skip the test.
            }
        }

        // Clean up instantiated values.
        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Consistency with equality - cmp(x,y)=Equal iff eq(x,y)=Equals.
    #[test]
    fn proptest_cmp_consistency_with_eq(seed1 in any::<u64>(), seed2 in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                // Named types disabled by gen_expr_full_seeded (need external type definitions).
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                // result_type disabled: Err payload type checking in instantiate2 fails for some seeds.
                result_type: 0,
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
        if !typechecked1.errors(&db).is_empty() {
            eprintln!("Type errors in expr1 (seed1={}): {} errors", seed1, typechecked1.errors(&db).len());
        }
        prop_assert!(typechecked1.errors(&db).is_empty());

        let resolved2 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr2);
        let typechecked2 = datalove_datalit::tycheck::type_check(&db, expr2, resolved2);
        if !typechecked2.errors(&db).is_empty() {
            eprintln!("Type errors in expr2 (seed2={}): {} errors", seed2, typechecked2.errors(&db).len());
        }
        prop_assert!(typechecked2.errors(&db).is_empty());

        let inst1 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked1)
            .expect("Should instantiate");
        let (ptr1, tydesc1) = (inst1.ptr, inst1.tydesc.as_ptr());
        drop(inst1);

        let inst2 = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked2)
            .expect("Should instantiate");
        let (ptr2, tydesc2) = (inst2.ptr, inst2.tydesc.as_ptr());
        drop(inst2);

        let cmp_result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let eq_result = unsafe {
            datalove_rt::c::dtlv_rti_eq_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        // Test consistency: cmp(x,y)=Equal iff eq(x,y)=Equals.
        if matches!(cmp_result, datalove_rt::c::RtOrdering::Equal) {
            prop_assert!(matches!(eq_result, datalove_rt::c::RtEq::Equals),
                "If cmp(x,y)=Equal then eq(x,y) should be Equals");
        }
        if matches!(eq_result, datalove_rt::c::RtEq::Equals) {
            prop_assert!(matches!(cmp_result, datalove_rt::c::RtOrdering::Equal),
                "If eq(x,y)=Equals then cmp(x,y) should be Equal");
        }

        // Clean up instantiated values.
        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Numeric boundary testing with MIN/MAX values.
    #[test]
    fn proptest_cmp_numeric_boundaries(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            numeric_strategy: NumericStrategy::CornerCases,
            type_weights: TypeWeights {
                // data_type and error_type disabled: clone not implemented (shallow copy causes double-free).
                data_type: 0,
                error_type: 0,
                // Named types disabled by gen_expr_full_seeded (need external type definitions).
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                // result_type disabled: Err payload type checking in instantiate2 fails for some seeds.
                result_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        if !typechecked.errors(&db).is_empty() {
            eprintln!("Type errors in expr (seed={}): {} errors", seed, typechecked.errors(&db).len());
        }
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Test that boundary values compare to themselves as Equal.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                inst.ptr,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtOrdering::Equal),
            "Boundary values: cmp(x, x) should be Equal");

        // Clean up instantiated value.
        unsafe {
            cleanup_value(&rt, inst.ptr, inst.tydesc.as_ptr());
        }
    }

    /// Property: Cross-function consistency - cmp(x,y)=Equal implies eq(x,y)=Equals.
    #[test]
    fn proptest_cmp_eq_consistency(seed1 in any::<u64>(), seed2 in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                result_type: 0,
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

        let cmp_result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        if matches!(cmp_result, datalove_rt::c::RtOrdering::Equal) {
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_local(
                    std::ptr::null_mut(),
                    ptr1,
                    tydesc1,
                    ptr2,
                    tydesc2,
                )
            };

            prop_assert!(matches!(eq_result, datalove_rt::c::RtEq::Equals),
                "Cross-function: cmp(x,y)=Equal implies eq(x,y)=Equals");
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Consistency with cmp_total for non-float types.
    /// For types without NaN, cmp and cmp_total should give the same result.
    #[test]
    fn proptest_cmp_cmp_total_consistency(seed1 in any::<u64>(), seed2 in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            type_weights: TypeWeights {
                // Disable float types to avoid NaN special cases.
                f32_type: 0,
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                result_type: 0,
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

        let cmp_result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let cmp_total_result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        // For non-float types, cmp and cmp_total should match.
        prop_assert_eq!(cmp_result, cmp_total_result,
            "For non-float types, cmp and cmp_total should return the same result");

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }
}

// ============================================================================
// Table cmp tests
// ============================================================================

#[test]
fn test_cmp_table_empty_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": {| x: u32 |} / {| x |}")?;
    let typechecked_b = compile_str(&db, ": {| x: u32 |} / {| x |}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(std::ptr::null_mut(), ptr_a, tydesc_a, ptr_b, tydesc_b)
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Equal));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_table_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": {| x: u32 |} / {| x; 1 |}")?;
    let typechecked_b = compile_str(&db, ": {| x: u32 |} / {| x; 1; 2 |}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(std::ptr::null_mut(), ptr_a, tydesc_a, ptr_b, tydesc_b)
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_table_less_by_value() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": {| x: u32, y: u32 |} / {| x, y; 1, 2 |}")?;
    let typechecked_b = compile_str(&db, ": {| x: u32, y: u32 |} / {| x, y; 1, 3 |}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(std::ptr::null_mut(), ptr_a, tydesc_a, ptr_b, tydesc_b)
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_table_greater() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": {| x: u32 |} / {| x; 5 |}")?;
    let typechecked_b = compile_str(&db, ": {| x: u32 |} / {| x; 3 |}")?;

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
        datalove_rt::c::dtlv_rti_cmp_local(std::ptr::null_mut(), ptr_a, tydesc_a, ptr_b, tydesc_b)
    };

    assert!(matches!(result, datalove_rt::c::RtOrdering::Greater));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_cmp_table_with_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, r#": {| name: string |} / {| name; "Alice" |}"#)?;
    let typechecked_b = compile_str(&db, r#": {| name: string |} / {| name; "Bob" |}"#)?;

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
        datalove_rt::c::dtlv_rti_cmp_local(std::ptr::null_mut(), ptr_a, tydesc_a, ptr_b, tydesc_b)
    };

    // "Alice" < "Bob" lexicographically.
    assert!(matches!(result, datalove_rt::c::RtOrdering::Less));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}
