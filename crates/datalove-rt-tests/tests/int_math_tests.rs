//! Tests for bigint arithmetic runtime functions.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;
use datalove_rt::c::{RtStatus, RtEq};

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

/// Helper to instantiate an int value from a datalit expression.
fn instantiate_int<'db>(
    db: &'db Database,
    rt: &Runtime,
    tydesc_table: &mut TyDescTable<'db>,
    expr: &str,
) -> AnyResult<(*const u8, *const datalove_rt::rtdt::TyDesc)> {
    let typechecked = compile_str(db, expr)?;
    let inst = instantiate2::instantiate_value(db, rt.handle(), tydesc_table, typechecked)?;
    Ok((inst.ptr, inst.tydesc.as_ptr()))
}

/// Helper to run a binary int operation and compare with expected result.
unsafe fn test_binary_int_op<'db, F>(
    db: &'db Database,
    rt: &Runtime,
    tydesc_table: &mut TyDescTable<'db>,
    a_expr: &str,
    b_expr: &str,
    expected_expr: &str,
    op: F,
) -> AnyResult<()>
where
    F: FnOnce(
        *mut u8, // LocalRtHandle
        *const u8,
        *const datalove_rt::rtdt::TyDesc,
        *const u8,
        *const datalove_rt::rtdt::TyDesc,
        *mut u8,
        *const datalove_rt::rtdt::TyDesc,
    ) -> RtStatus,
{
    let (ptr_a, tydesc_a) = instantiate_int(db, rt, tydesc_table, a_expr)?;
    let (ptr_b, tydesc_b) = instantiate_int(db, rt, tydesc_table, b_expr)?;
    let (ptr_expected, tydesc_expected) = instantiate_int(db, rt, tydesc_table, expected_expr)?;

    // Allocate result buffer.
    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
    };
    assert!(!result_ptr.is_null(), "Failed to allocate result buffer");

    // Call the operation.
    let status = op(rt.handle(), ptr_a, tydesc_a, ptr_b, tydesc_b, result_ptr, tydesc_a);
    assert_eq!(status, RtStatus::Ok, "Operation returned error");

    // Compare result with expected.
    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr,
            tydesc_a,
            ptr_expected,
            tydesc_expected,
        )
    };
    assert!(
        matches!(eq_result, RtEq::Equals),
        "Result does not match expected for {} op {} = {}",
        a_expr, b_expr, expected_expr
    );

    // Cleanup.
    unsafe {
        cleanup_value(rt, result_ptr, tydesc_a);
        cleanup_value(rt, ptr_a, tydesc_a);
        cleanup_value(rt, ptr_b, tydesc_b);
        cleanup_value(rt, ptr_expected, tydesc_expected);
    }

    Ok(())
}

// ============================================================================
// Addition Tests
// ============================================================================

#[test]
fn test_int_add_positive() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @5", ": @int / @3", ": @int / @8",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_add_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @42", ": @int / @0", ": @int / @42",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_add_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // -5 + 3 = -2
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @-5", ": @int / @3", ": @int / @-2",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_add_large() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // Multi-limb addition: 3000000000 + 2000000000 = 5000000000.
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @3000000000", ": @int / @2000000000", ": @int / @5000000000",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_add_result_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 5 + (-5) = 0
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @5", ": @int / @-5", ": @int / @0",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// Subtraction Tests
// ============================================================================

#[test]
fn test_int_sub_positive() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @8", ": @int / @3", ": @int / @5",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_sub(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_sub_result_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 3 - 8 = -5
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @3", ": @int / @8", ": @int / @-5",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_sub(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_sub_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 5 - 5 = 0
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @5", ": @int / @5", ": @int / @0",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_sub(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_sub_large() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 5000000000 - 3000000000 = 2000000000
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @5000000000", ": @int / @3000000000", ": @int / @2000000000",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_sub(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// Multiplication Tests
// ============================================================================

#[test]
fn test_int_mul_positive() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @6", ": @int / @7", ": @int / @42",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_mul(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_mul_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @42", ": @int / @0", ": @int / @0",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_mul(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_mul_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // -6 * 7 = -42
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @-6", ": @int / @7", ": @int / @-42",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_mul(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_mul_both_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // -6 * -7 = 42
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @-6", ": @int / @-7", ": @int / @42",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_mul(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_mul_large() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 1000000 * 1000000 = 1000000000000
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @1000000", ": @int / @1000000", ": @int / @1000000000000",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_mul(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// Negation Tests
// ============================================================================

#[test]
fn test_int_neg_positive() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @42")?;
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @-42")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
    };
    assert!(!result_ptr.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_neg(rt.handle(), ptr_a, tydesc_a, result_ptr, tydesc_a)
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr,
            tydesc_a,
            ptr_expected,
            tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals));

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_a);
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_neg_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @-42")?;
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @42")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
    };
    assert!(!result_ptr.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_neg(rt.handle(), ptr_a, tydesc_a, result_ptr, tydesc_a)
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr,
            tydesc_a,
            ptr_expected,
            tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals));

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_a);
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_neg_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @0")?;
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @0")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
    };
    assert!(!result_ptr.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_neg(rt.handle(), ptr_a, tydesc_a, result_ptr, tydesc_a)
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr,
            tydesc_a,
            ptr_expected,
            tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals));

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_a);
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

// ============================================================================
// Division Tests
// ============================================================================

#[test]
fn test_int_div_exact() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 42 / 6 = 7
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @42", ": @int / @6", ": @int / @7",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_truncate() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 43 / 6 = 7 (truncated)
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @43", ": @int / @6", ": @int / @7",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_negative_dividend() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // -42 / 6 = -7
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @-42", ": @int / @6", ": @int / @-7",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_negative_divisor() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 42 / -6 = -7
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @42", ": @int / @-6", ": @int / @-7",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_both_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // -42 / -6 = 7
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @-42", ": @int / @-6", ": @int / @7",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_by_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @42")?;
    let (ptr_b, tydesc_b) = instantiate_int(&db, &rt, &mut tydesc_table, ": @int / @0")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
    };
    assert!(!result_ptr.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_div_checked(
            rt.handle(),
            ptr_a, tydesc_a,
            ptr_b, tydesc_b,
            result_ptr, tydesc_a,
        )
    };
    assert_eq!(status, RtStatus::Error, "Division by zero should return Error");

    unsafe {
        // Free result buffer (no destroy needed since division failed).
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), tydesc_a, 1, result_ptr);
        cleanup_value(&rt, ptr_a, tydesc_a);
        cleanup_value(&rt, ptr_b, tydesc_b);
    }
    Ok(())
}

#[test]
fn test_int_div_large() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 1000000000000 / 1000000 = 1000000
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @1000000000000", ": @int / @1000000", ": @int / @1000000",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_zero_dividend() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        // 0 / 42 = 0
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": @int / @0", ": @int / @42", ": @int / @0",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// Property-based Tests
// ============================================================================

#[cfg(feature = "slow_tests")]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // Generate small integers for testing (to avoid very expensive multi-limb operations).
    fn small_int() -> impl Strategy<Value = i64> {
        -1_000_000_000i64..=1_000_000_000i64
    }

    fn nonzero_small_int() -> impl Strategy<Value = i64> {
        prop_oneof![
            -1_000_000_000i64..=-1i64,
            1i64..=1_000_000_000i64,
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            max_shrink_iters: 0,
            ..ProptestConfig::default()
        })]

        #[test]
        fn proptest_int_add_commutative(a in small_int(), b in small_int()) {
            let db = Database::default();
            let rt = Runtime::new();
            let mut tydesc_table = TyDescTable::new(&db);

            let a_expr = format!(": @int / @{}", a);
            let b_expr = format!(": @int / @{}", b);

            let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, &a_expr).unwrap();
            let (ptr_b, tydesc_b) = instantiate_int(&db, &rt, &mut tydesc_table, &b_expr).unwrap();

            // a + b
            let result1_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status1 = unsafe {
                datalove_rt::c::dtlv_rti_int_add(
                    rt.handle(), ptr_a, tydesc_a, ptr_b, tydesc_b, result1_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status1, RtStatus::Ok);

            // b + a
            let result2_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status2 = unsafe {
                datalove_rt::c::dtlv_rti_int_add(
                    rt.handle(), ptr_b, tydesc_b, ptr_a, tydesc_a, result2_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status2, RtStatus::Ok);

            // Compare results.
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_local(
                    std::ptr::null_mut(),
                    result1_ptr, tydesc_a,
                    result2_ptr, tydesc_a,
                )
            };
            prop_assert!(matches!(eq_result, RtEq::Equals), "a + b != b + a for a={}, b={}", a, b);

            unsafe {
                cleanup_value(&rt, result1_ptr, tydesc_a);
                cleanup_value(&rt, result2_ptr, tydesc_a);
                cleanup_value(&rt, ptr_a, tydesc_a);
                cleanup_value(&rt, ptr_b, tydesc_b);
            }
        }

        #[test]
        fn proptest_int_mul_commutative(a in small_int(), b in small_int()) {
            let db = Database::default();
            let rt = Runtime::new();
            let mut tydesc_table = TyDescTable::new(&db);

            let a_expr = format!(": @int / @{}", a);
            let b_expr = format!(": @int / @{}", b);

            let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, &a_expr).unwrap();
            let (ptr_b, tydesc_b) = instantiate_int(&db, &rt, &mut tydesc_table, &b_expr).unwrap();

            // a * b
            let result1_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status1 = unsafe {
                datalove_rt::c::dtlv_rti_int_mul(
                    rt.handle(), ptr_a, tydesc_a, ptr_b, tydesc_b, result1_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status1, RtStatus::Ok);

            // b * a
            let result2_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status2 = unsafe {
                datalove_rt::c::dtlv_rti_int_mul(
                    rt.handle(), ptr_b, tydesc_b, ptr_a, tydesc_a, result2_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status2, RtStatus::Ok);

            // Compare results.
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_local(
                    std::ptr::null_mut(),
                    result1_ptr, tydesc_a,
                    result2_ptr, tydesc_a,
                )
            };
            prop_assert!(matches!(eq_result, RtEq::Equals), "a * b != b * a for a={}, b={}", a, b);

            unsafe {
                cleanup_value(&rt, result1_ptr, tydesc_a);
                cleanup_value(&rt, result2_ptr, tydesc_a);
                cleanup_value(&rt, ptr_a, tydesc_a);
                cleanup_value(&rt, ptr_b, tydesc_b);
            }
        }

        #[test]
        fn proptest_int_add_sub_inverse(a in small_int(), b in small_int()) {
            let db = Database::default();
            let rt = Runtime::new();
            let mut tydesc_table = TyDescTable::new(&db);

            let a_expr = format!(": @int / @{}", a);
            let b_expr = format!(": @int / @{}", b);

            let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, &a_expr).unwrap();
            let (ptr_b, tydesc_b) = instantiate_int(&db, &rt, &mut tydesc_table, &b_expr).unwrap();

            // tmp = a + b
            let tmp_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status1 = unsafe {
                datalove_rt::c::dtlv_rti_int_add(
                    rt.handle(), ptr_a, tydesc_a, ptr_b, tydesc_b, tmp_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status1, RtStatus::Ok);

            // result = tmp - b = (a + b) - b = a
            let result_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status2 = unsafe {
                datalove_rt::c::dtlv_rti_int_sub(
                    rt.handle(), tmp_ptr, tydesc_a, ptr_b, tydesc_b, result_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status2, RtStatus::Ok);

            // result should equal a.
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_local(
                    std::ptr::null_mut(),
                    result_ptr, tydesc_a,
                    ptr_a, tydesc_a,
                )
            };
            prop_assert!(matches!(eq_result, RtEq::Equals), "(a + b) - b != a for a={}, b={}", a, b);

            unsafe {
                cleanup_value(&rt, tmp_ptr, tydesc_a);
                cleanup_value(&rt, result_ptr, tydesc_a);
                cleanup_value(&rt, ptr_a, tydesc_a);
                cleanup_value(&rt, ptr_b, tydesc_b);
            }
        }

        #[test]
        fn proptest_int_neg_double(a in small_int()) {
            let db = Database::default();
            let rt = Runtime::new();
            let mut tydesc_table = TyDescTable::new(&db);

            let a_expr = format!(": @int / @{}", a);

            let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, &a_expr).unwrap();

            // neg1 = -a
            let neg1_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status1 = unsafe {
                datalove_rt::c::dtlv_rti_int_neg(rt.handle(), ptr_a, tydesc_a, neg1_ptr, tydesc_a)
            };
            prop_assert_eq!(status1, RtStatus::Ok);

            // neg2 = -(-a) = a
            let neg2_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status2 = unsafe {
                datalove_rt::c::dtlv_rti_int_neg(rt.handle(), neg1_ptr, tydesc_a, neg2_ptr, tydesc_a)
            };
            prop_assert_eq!(status2, RtStatus::Ok);

            // neg2 should equal a.
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_local(
                    std::ptr::null_mut(),
                    neg2_ptr, tydesc_a,
                    ptr_a, tydesc_a,
                )
            };
            prop_assert!(matches!(eq_result, RtEq::Equals), "-(-a) != a for a={}", a);

            unsafe {
                cleanup_value(&rt, neg1_ptr, tydesc_a);
                cleanup_value(&rt, neg2_ptr, tydesc_a);
                cleanup_value(&rt, ptr_a, tydesc_a);
            }
        }

        #[test]
        fn proptest_int_mul_div_inverse(a in small_int(), b in nonzero_small_int()) {
            let db = Database::default();
            let rt = Runtime::new();
            let mut tydesc_table = TyDescTable::new(&db);

            let a_expr = format!(": @int / @{}", a);
            let b_expr = format!(": @int / @{}", b);

            let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, &a_expr).unwrap();
            let (ptr_b, tydesc_b) = instantiate_int(&db, &rt, &mut tydesc_table, &b_expr).unwrap();

            // tmp = a * b
            let tmp_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status1 = unsafe {
                datalove_rt::c::dtlv_rti_int_mul(
                    rt.handle(), ptr_a, tydesc_a, ptr_b, tydesc_b, tmp_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status1, RtStatus::Ok);

            // result = tmp / b = (a * b) / b = a
            let result_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_a, 1)
            };
            let status2 = unsafe {
                datalove_rt::c::dtlv_rti_int_div_checked(
                    rt.handle(), tmp_ptr, tydesc_a, ptr_b, tydesc_b, result_ptr, tydesc_a,
                )
            };
            prop_assert_eq!(status2, RtStatus::Ok);

            // result should equal a.
            let eq_result = unsafe {
                datalove_rt::c::dtlv_rti_eq_local(
                    std::ptr::null_mut(),
                    result_ptr, tydesc_a,
                    ptr_a, tydesc_a,
                )
            };
            prop_assert!(matches!(eq_result, RtEq::Equals), "(a * b) / b != a for a={}, b={}", a, b);

            unsafe {
                cleanup_value(&rt, tmp_ptr, tydesc_a);
                cleanup_value(&rt, result_ptr, tydesc_a);
                cleanup_value(&rt, ptr_a, tydesc_a);
                cleanup_value(&rt, ptr_b, tydesc_b);
            }
        }
    }
}
