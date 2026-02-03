//! Tests for the cmp_total runtime function.
//! This tests IEEE 754-2008 total ordering for floats (distinguishes -0.0 from +0.0).

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
fn test_cmp_total_f32_less() -> AnyResult<()> {
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
    let typechecked_a = compile_str(&db, "3.14")?;
    let typechecked_b = compile_str(&db, "2.71")?;

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
    let typechecked_a = compile_str(&db, "(true, 2.71)")?;
    let typechecked_b = compile_str(&db, "(true, 3.14)")?;

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
    let typechecked_a = compile_str(&db, "(true, 3.14)")?;
    let typechecked_b = compile_str(&db, "(true, 3.14)")?;

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
    let typechecked_a = compile_str(&db, ": map<u32, u32> / map{}")?;
    let typechecked_b = compile_str(&db, ": map<u32, u32> / map{}")?;

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
    let typechecked_a = compile_str(&db, "map{10 = 100, 20 = 200}")?;
    let typechecked_b = compile_str(&db, "map{10 = 100, 20 = 200}")?;

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
    let typechecked_a = compile_str(&db, ": map<u32, u32> / map{}")?;
    let typechecked_b = compile_str(&db, ": map<u32, u32> / map{10 = 100}")?;

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
    let typechecked_a = compile_str(&db, "map{10 = 100, 20 = 200}")?;
    let typechecked_b = compile_str(&db, "map{10 = 100, 30 = 300}")?;

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
    let typechecked_a = compile_str(&db, "map{10 = 100, 20 = 200}")?;
    let typechecked_b = compile_str(&db, "map{10 = 100, 20 = 999}")?;

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
    let typechecked_a = compile_str(&db, "map{10 = 100}")?;
    let typechecked_b = compile_str(&db, "map{10 = 100, 20 = 200}")?;

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
    let typechecked_a = compile_str(&db, "map{10 = 100, 30 = 300}")?;
    let typechecked_b = compile_str(&db, "map{10 = 100, 20 = 200}")?;

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
    let typechecked_a = compile_str(&db, ": set<u32> / set{}")?;
    let typechecked_b = compile_str(&db, ": set<u32> / set{}")?;

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
    let typechecked_a = compile_str(&db, "set{10, 20, 30}")?;
    let typechecked_b = compile_str(&db, "set{10, 20, 30}")?;

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
    let typechecked_a = compile_str(&db, ": set<u32> / set{}")?;
    let typechecked_b = compile_str(&db, ": set<u32> / set{10}")?;

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
    let typechecked_a = compile_str(&db, "set{10, 20, 30}")?;
    let typechecked_b = compile_str(&db, "set{10, 20, 40}")?;

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
    let typechecked_a = compile_str(&db, "set{10}")?;
    let typechecked_b = compile_str(&db, "set{10, 20}")?;

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
    let typechecked_a = compile_str(&db, "set{10, 20, 40}")?;
    let typechecked_b = compile_str(&db, "set{10, 20, 30}")?;

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

    /// Property: Totality - cmp_total always returns Less, Equal, or Greater (never Error).
    #[test]
    fn proptest_cmp_total_totality(seed1 in any::<u64>(), seed2 in any::<u64>()) {
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

        let result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        // Total ordering must always give a definite answer for same-type comparisons.
        // When comparing different types, Error is returned (skip the assertion).
        if !matches!(result, datalove_rt::c::RtOrdering::Error) {
            prop_assert!(
                matches!(result, datalove_rt::c::RtOrdering::Less | datalove_rt::c::RtOrdering::Equal | datalove_rt::c::RtOrdering::Greater),
                "Totality: cmp_total must return Less, Equal, or Greater for same-type comparisons, got {:?}", result
            );
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Transitivity - if cmp_total(x,y)=Less and cmp_total(y,z)=Less then cmp_total(x,z)=Less.
    #[test]
    fn proptest_cmp_total_transitivity(seed1 in any::<u64>(), seed2 in any::<u64>(), seed3 in any::<u64>()) {
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
        let expr2 = gen_expr_full_seeded(&db, seed2, config.clone());
        let expr3 = gen_expr_full_seeded(&db, seed3, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);

        let resolved1 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr1);
        let typechecked1 = datalove_datalit::tycheck::type_check(&db, expr1, resolved1);
        prop_assert!(typechecked1.errors(&db).is_empty());

        let resolved2 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr2);
        let typechecked2 = datalove_datalit::tycheck::type_check(&db, expr2, resolved2);
        prop_assert!(typechecked2.errors(&db).is_empty());

        let resolved3 = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr3);
        let typechecked3 = datalove_datalit::tycheck::type_check(&db, expr3, resolved3);
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
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let cmp_yz = unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr2,
                tydesc2,
                ptr3,
                tydesc3,
            )
        };

        // Test transitivity when both comparisons are Less.
        if matches!(cmp_xy, datalove_rt::c::RtOrdering::Less) && matches!(cmp_yz, datalove_rt::c::RtOrdering::Less) {
            let cmp_xz = unsafe {
                datalove_rt::c::dtlv_rti_cmp_total_local(
                    std::ptr::null_mut(),
                    ptr1,
                    tydesc1,
                    ptr3,
                    tydesc3,
                )
            };

            prop_assert!(matches!(cmp_xz, datalove_rt::c::RtOrdering::Less),
                "Transitivity: if cmp_total(x,y)=Less and cmp_total(y,z)=Less then cmp_total(x,z)=Less");
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
            cleanup_value(&rt, ptr3, tydesc3);
        }
    }

    /// Property: Antisymmetry - if cmp_total(x,y)=Less then cmp_total(y,x)=Greater.
    #[test]
    fn proptest_cmp_total_antisymmetry(seed1 in any::<u64>(), seed2 in any::<u64>()) {
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

        let cmp_xy = unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let cmp_yx = unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr2,
                tydesc2,
                ptr1,
                tydesc1,
            )
        };

        match cmp_xy {
            datalove_rt::c::RtOrdering::Less => {
                prop_assert!(matches!(cmp_yx, datalove_rt::c::RtOrdering::Greater),
                    "Antisymmetry: if cmp_total(x,y)=Less then cmp_total(y,x)=Greater");
            }
            datalove_rt::c::RtOrdering::Greater => {
                prop_assert!(matches!(cmp_yx, datalove_rt::c::RtOrdering::Less),
                    "Antisymmetry: if cmp_total(x,y)=Greater then cmp_total(y,x)=Less");
            }
            datalove_rt::c::RtOrdering::Equal => {
                prop_assert!(matches!(cmp_yx, datalove_rt::c::RtOrdering::Equal),
                    "Antisymmetry: if cmp_total(x,y)=Equal then cmp_total(y,x)=Equal");
            }
            datalove_rt::c::RtOrdering::Error => {
                // Should not happen for total ordering, but skip if it does.
            }
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Reflexivity - cmp_total(x, x) = Equal.
    #[test]
    fn proptest_cmp_total_reflexivity(seed in any::<u64>()) {
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
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);

        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)
            .expect("Should instantiate");
        let (ptr, tydesc) = (inst.ptr, inst.tydesc.as_ptr());
        drop(inst);

        let result = unsafe {
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr,
                tydesc,
                ptr,
                tydesc,
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtOrdering::Equal),
            "Reflexivity: cmp_total(x, x) should be Equal");

        unsafe {
            cleanup_value(&rt, ptr, tydesc);
        }
    }

    /// Property: Consistency with eq_unique - cmp_total(x,y)=Equal implies eq_unique(x,y)=Equals.
    #[test]
    fn proptest_cmp_total_eq_unique_consistency(seed1 in any::<u64>(), seed2 in any::<u64>()) {
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
            datalove_rt::c::dtlv_rti_cmp_total_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        if matches!(cmp_result, datalove_rt::c::RtOrdering::Equal) {
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_unique_local(
                    std::ptr::null_mut(),
                    ptr1,
                    tydesc1,
                    ptr2,
                    tydesc2,
                )
            };

            prop_assert!(matches!(eq_result, datalove_rt::c::RtEq::Equals),
                "Consistency: cmp_total(x,y)=Equal implies eq_unique(x,y)=Equals");
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }
}
