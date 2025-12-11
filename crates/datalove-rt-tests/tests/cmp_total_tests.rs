//! Tests for the cmp_total runtime function.
//! This tests IEEE 754-2008 total ordering for floats (distinguishes -0.0 from +0.0).

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

/// Clean up an instantiated value.
unsafe fn cleanup_value(rt: &Runtime, ptr: *const u8, tydesc: *const datalove_rt::rtdt::TyDesc) {
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), ptr as *mut u8, tydesc);
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), tydesc, 1, ptr as *mut u8);
    }
}



#[test]
fn test_cmp_total_f32_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@2.71")?;
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_f32_equal() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_f32_greater() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_tuple_less() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @2.71)")?;
    let typechecked_b = compile_str(&db, "@(@true, @3.14)")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_tuple_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@(@true, @3.14)")?;
    let typechecked_b = compile_str(&db, "@(@true, @3.14)")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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

// Map comparison tests

#[test]
fn test_cmp_total_map_empty_equal() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_map_equal() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @200}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_map_empty_vs_nonempty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @map<@u32, @u32> / @map{}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_map_less_by_key() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @30 = @300}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_map_less_by_value() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @20 = @200}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @999}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_map_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @200}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_map_greater_by_key() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@map{@10 = @100, @30 = @300}")?;
    let typechecked_b = compile_str(&db, "@map{@10 = @100, @20 = @200}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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

// Set comparison tests

#[test]
fn test_cmp_total_set_empty_equal() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_set_equal() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_set_empty_vs_nonempty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": @set<@u32> / @set{}")?;
    let typechecked_b = compile_str(&db, "@set{@10}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_set_less_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @30}")?;
    let typechecked_b = compile_str(&db, "@set{@10, @20, @40}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_set_less_by_length() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10}")?;
    let typechecked_b = compile_str(&db, "@set{@10, @20}")?;

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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
fn test_cmp_total_set_greater_by_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "@set{@10, @20, @40}")?;
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
        datalove_rt::c::dtlv_rti_cmp_total_local(
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
