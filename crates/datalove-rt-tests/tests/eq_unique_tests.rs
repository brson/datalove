//! Tests for the eq_unique runtime function.
//! This tests bitwise equality for floats (all bit patterns distinct).

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
fn test_eq_unique_f32_equals() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_u32_equals() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_tuple_equals() -> AnyResult<()> {
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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_tuple_not_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "(true, 3.14)")?;
    let typechecked_b = compile_str(&db, "(true, 2.71)")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

// Map equality tests

#[test]
fn test_eq_unique_map_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇⦈")?;
    let typechecked_b = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇⦈")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_map_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "⦇10 ↦ 100, 20 ↦ 200, 30 ↦ 300⦈")?;
    let typechecked_b = compile_str(&db, "⦇10 ↦ 100, 20 ↦ 200, 30 ↦ 300⦈")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_map_not_equals_different_keys() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇10 ↦ 100, 20 ↦ 200⦈")?;
    let typechecked_b = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇10 ↦ 100, 30 ↦ 300⦈")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_map_not_equals_different_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇10 ↦ 100, 20 ↦ 200⦈")?;
    let typechecked_b = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇10 ↦ 100, 20 ↦ 999⦈")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_map_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇10 ↦ 100, 20 ↦ 200, 30 ↦ 300⦈")?;
    let typechecked_b = compile_str(&db, ": ⦇u32 ↦ u32⦈ / ⦇10 ↦ 100, 20 ↦ 200⦈")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

// Set equality tests

#[test]
fn test_eq_unique_set_empty_equals() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦃u32⦄ / ⦃⦄")?;
    let typechecked_b = compile_str(&db, ": ⦃u32⦄ / ⦃⦄")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_set_equals_same_contents() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, "⦃10, 20, 30⦄")?;
    let typechecked_b = compile_str(&db, "⦃10, 20, 30⦄")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_set_not_equals_different_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦃u32⦄ / ⦃10, 20, 30⦄")?;
    let typechecked_b = compile_str(&db, ": ⦃u32⦄ / ⦃10, 20, 40⦄")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
    unsafe {
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_eq_unique_set_not_equals_different_sizes() -> AnyResult<()> {
    let db = Database::default();
    let typechecked_a = compile_str(&db, ": ⦃u32⦄ / ⦃10, 20, 30⦄")?;
    let typechecked_b = compile_str(&db, ": ⦃u32⦄ / ⦃10, 20⦄")?;

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
        datalove_rt::c::dtlv_rti_eq_unique_local(
            std::ptr::null_mut(),
            ptr_a,
            tydesc_a,
            ptr_b,
            tydesc_b,
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::NotEquals));
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

    /// Property: Reflexivity - eq_unique(x, x) = Equals for all x.
    #[test]
    fn proptest_eq_unique_reflexive(seed in any::<u64>()) {
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
            datalove_rt::c::dtlv_rti_eq_unique_local(
                std::ptr::null_mut(),
                ptr,
                tydesc,
                ptr,
                tydesc,
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Reflexivity: eq_unique(x, x) should be Equals");

        unsafe {
            cleanup_value(&rt, ptr, tydesc);
        }
    }

    /// Property: Symmetry - eq_unique(x, y) = eq_unique(y, x).
    #[test]
    fn proptest_eq_unique_symmetric(seed1 in any::<u64>(), seed2 in any::<u64>()) {
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

        let result_xy = unsafe {
            datalove_rt::c::dtlv_rti_eq_unique_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        let result_yx = unsafe {
            datalove_rt::c::dtlv_rti_eq_unique_local(
                std::ptr::null_mut(),
                ptr2,
                tydesc2,
                ptr1,
                tydesc1,
            )
        };

        prop_assert_eq!(result_xy, result_yx, "Symmetry: eq_unique(x, y) should equal eq_unique(y, x)");

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Consistency with clone - eq_unique(x, clone(x)) = Equals.
    #[test]
    fn proptest_eq_unique_consistency_with_clone(seed in any::<u64>()) {
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

        // Clone the value.
        let tydesc_ref = inst.tydesc.as_ref();
        let mut clone_buffer = datalove_rt::rust::AlignedBuffer::new(tydesc_ref.size as usize);
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
            datalove_rt::c::dtlv_rti_eq_unique_local(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Clone consistency: eq_unique(x, clone(x)) should be Equals");

        unsafe {
            cleanup_value(&rt, inst.ptr, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
        }
    }

    /// Property: eq_unique implies eq - if eq_unique(x,y)=Equals then eq(x,y)=Equals.
    #[test]
    fn proptest_eq_unique_implies_eq(seed1 in any::<u64>(), seed2 in any::<u64>()) {
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

        let eq_unique_result = unsafe {
            datalove_rt::c::dtlv_rti_eq_unique_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        if matches!(eq_unique_result, datalove_rt::c::RtEq::Equals) {
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
                "eq_unique(x,y)=Equals implies eq(x,y)=Equals");
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }

    /// Property: Consistency with cmp_total - eq_unique(x,y)=Equals implies cmp_total(x,y)=Equal.
    #[test]
    fn proptest_eq_unique_cmp_total_consistency(seed1 in any::<u64>(), seed2 in any::<u64>()) {
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

        let eq_unique_result = unsafe {
            datalove_rt::c::dtlv_rti_eq_unique_local(
                std::ptr::null_mut(),
                ptr1,
                tydesc1,
                ptr2,
                tydesc2,
            )
        };

        if matches!(eq_unique_result, datalove_rt::c::RtEq::Equals) {
            let cmp_result = unsafe {
                datalove_rt::c::dtlv_rti_cmp_total_local(
                    std::ptr::null_mut(),
                    ptr1,
                    tydesc1,
                    ptr2,
                    tydesc2,
                )
            };

            prop_assert!(matches!(cmp_result, datalove_rt::c::RtOrdering::Equal),
                "eq_unique(x,y)=Equals implies cmp_total(x,y)=Equal");
        }

        unsafe {
            cleanup_value(&rt, ptr1, tydesc1);
            cleanup_value(&rt, ptr2, tydesc2);
        }
    }
}
