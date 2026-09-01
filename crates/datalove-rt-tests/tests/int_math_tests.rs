//! Tests for bigint arithmetic runtime functions.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;
use datalove_rt::c::{RtStatus, RtEq};

#[salsa::tracked(returns(copy))]
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

/// Helper to instantiate an int value from a datalit expression.
fn instantiate_int<'db>(
    db: &'db Database,
    rt: &Runtime,
    tydesc_table: &mut TyDescTable<'db>,
    expr: &str,
) -> AnyResult<(*const u8, *const datalove_rtdt::TyDesc)> {
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
        *const datalove_rtdt::TyDesc,
        *const u8,
        *const datalove_rtdt::TyDesc,
        *mut u8,
        *const datalove_rtdt::TyDesc,
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
            ": int / 5", ": int / 3", ": int / 8",
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
            ": int / 42", ": int / 0", ": int / 42",
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
            ": int / -5", ": int / 3", ": int / -2",
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
            ": int / 3000000000", ": int / 2000000000", ": int / 5000000000",
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
            ": int / 5", ": int / -5", ": int / 0",
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
            ": int / 8", ": int / 3", ": int / 5",
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
            ": int / 3", ": int / 8", ": int / -5",
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
            ": int / 5", ": int / 5", ": int / 0",
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
            ": int / 5000000000", ": int / 3000000000", ": int / 2000000000",
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
            ": int / 6", ": int / 7", ": int / 42",
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
            ": int / 42", ": int / 0", ": int / 0",
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
            ": int / -6", ": int / 7", ": int / -42",
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
            ": int / -6", ": int / -7", ": int / 42",
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
            ": int / 1000000", ": int / 1000000", ": int / 1000000000000",
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

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 42")?;
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / -42")?;

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

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / -42")?;
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 42")?;

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

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 0")?;
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 0")?;

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
            ": int / 42", ": int / 6", ": int / 7",
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
            ": int / 43", ": int / 6", ": int / 7",
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
            ": int / -42", ": int / 6", ": int / -7",
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
            ": int / 42", ": int / -6", ": int / -7",
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
            ": int / -42", ": int / -6", ": int / 7",
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

    let (ptr_a, tydesc_a) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 42")?;
    let (ptr_b, tydesc_b) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 0")?;

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
            ": int / 1000000000000", ": int / 1000000", ": int / 1000000",
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
            ": int / 0", ": int / 42", ": int / 0",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// Multi-limb Division Tests (Knuth's Algorithm D)
// ============================================================================

#[test]
fn test_int_div_multi_limb_divisor() -> AnyResult<()> {
    // Tests the Knuth Algorithm D path (divisor has > 1 limb).
    // 10^19 / 10^10 = 10^9
    // 10^19 = 10000000000000000000 (3 limbs in base 2^32)
    // 10^10 = 10000000000 (2 limbs in base 2^32)
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 10000000000000000000",
            ": int / 10000000000",
            ": int / 1000000000",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_multi_limb_exact() -> AnyResult<()> {
    // 4294967297 * 3 = 12884901891
    // 4294967297 = 2^32 + 1 (2 limbs)
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 12884901891",
            ": int / 4294967297",
            ": int / 3",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_multi_limb_with_remainder() -> AnyResult<()> {
    // 12884901892 / 4294967297 = 3 (with remainder 1, truncated)
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 12884901892",
            ": int / 4294967297",
            ": int / 3",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_multi_limb_negative() -> AnyResult<()> {
    // -12884901891 / 4294967297 = -3
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / -12884901891",
            ": int / 4294967297",
            ": int / -3",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_dividend_smaller_than_divisor() -> AnyResult<()> {
    // 100 / 4294967297 = 0 (quotient is zero when dividend < divisor)
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 100",
            ": int / 4294967297",
            ": int / 0",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_multi_limb_no_shift() -> AnyResult<()> {
    // Test Knuth's Algorithm D when shift == 0 (divisor MSB already has high bit set).
    // 2^31 = 2147483648 has high bit set in its low limb.
    // 2^31 * 2^32 + 2^31 = 9223372039002259456 (3 limbs)
    // 9223372039002259456 / (2^31 + 2^32) = 2147483648 / 6442450944 ...
    // Actually, let me use simpler numbers:
    // (2^31 + 1) = 2147483649 has bit 31 set.
    // For a 2-limb divisor with high bit set in the second limb:
    // 2^63 = 9223372036854775808 has [0, 2^31] as limbs (2 limbs, MSB=2^31, high bit set).
    // Let's divide 2^64 / 2^63 = 2.
    // 18446744073709551616 / 9223372036854775808 = 2
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 18446744073709551616",
            ": int / 9223372036854775808",
            ": int / 2",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// Large Number Arithmetic Tests
// ============================================================================

#[test]
fn test_int_add_very_large() -> AnyResult<()> {
    // Addition that requires carry propagation across multiple limbs.
    // 2^64 + 2^64 = 2^65
    // 18446744073709551616 + 18446744073709551616 = 36893488147419103232
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 18446744073709551616",
            ": int / 18446744073709551616",
            ": int / 36893488147419103232",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_sub_very_large() -> AnyResult<()> {
    // 36893488147419103232 - 18446744073709551616 = 18446744073709551616
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 36893488147419103232",
            ": int / 18446744073709551616",
            ": int / 18446744073709551616",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_sub(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_mul_very_large() -> AnyResult<()> {
    // 2^32 * 2^32 = 2^64
    // 4294967296 * 4294967296 = 18446744073709551616
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 4294967296",
            ": int / 4294967296",
            ": int / 18446744073709551616",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_mul(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_add_different_sign_b_larger() -> AnyResult<()> {
    // Tests the path where |b| > |a| with different signs.
    // 3 + (-8) = -5 (|b| > |a|, result has sign of b)
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 3", ": int / -8", ": int / -5",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_sub_a_is_zero() -> AnyResult<()> {
    // 0 - 5 = -5
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 0", ": int / 5", ": int / -5",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_sub(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_add_a_is_zero() -> AnyResult<()> {
    // 0 + (-5) = -5
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 0", ": int / -5", ": int / -5",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_add(rt, a, a_td, b, b_td, out, out_td)
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
    use rmx::proptest::prelude::*;

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

            let a_expr = format!(": int / {}", a);
            let b_expr = format!(": int / {}", b);

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

            let a_expr = format!(": int / {}", a);
            let b_expr = format!(": int / {}", b);

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

            let a_expr = format!(": int / {}", a);
            let b_expr = format!(": int / {}", b);

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

            let a_expr = format!(": int / {}", a);

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

            let a_expr = format!(": int / {}", a);
            let b_expr = format!(": int / {}", b);

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

// ============================================================================
// Knuth Algorithm D corner case tests
// ============================================================================

#[test]
fn test_int_div_knuth_refinement_three_limb_check() -> AnyResult<()> {
    // Triggers the refinement loop via three-limb comparison.
    // Dividend: 2^95 - 1 = 39614081257132168796771975167
    // Divisor: 2^63 + 2^32 - 1 = 9223372041149743103
    // This makes q_hat start at 0xFFFFFFFF, then refinement decrements to 0xFFFFFFFE.
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 39614081257132168796771975167",
            ": int / 9223372041149743103",
            ": int / 4294967294",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_knuth_refinement_overflow_check() -> AnyResult<()> {
    // Triggers refinement via large quotient estimate.
    // Dividend: 2^95 = 39614081257132168796771975168
    // Divisor: 2^63 - 1 = 9223372036854775807
    // This produces a quotient slightly larger than 2^32.
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 39614081257132168796771975168",
            ": int / 9223372036854775807",
            ": int / 4294967296",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_knuth_add_back() -> AnyResult<()> {
    // Attempt to trigger the add-back correction path.
    // This is the rarest path in Knuth's Algorithm D (probability ~2^-31).
    // Dividend: 0x7FFFFFFF_FFFFFFFF_00000000 = 39614081257132168792477007872
    // Divisor: 0x80000000_FFFFFFFF = 9223372041149743103
    // After refinement, q_hat may still be 1 too high, requiring add-back.
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 39614081257132168792477007872",
            ": int / 9223372041149743103",
            ": int / 4294967294",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_knuth_multiple_refinements() -> AnyResult<()> {
    // Test case that may require multiple refinement iterations.
    // Dividend: 2^96 - 1 = 79228162514264337593543950335
    // Divisor: 2^63 + 2^62 = 13835058055282163712
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 79228162514264337593543950335",
            ": int / 13835058055282163712",
            ": int / 5726623061",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_knuth_borrow_propagation() -> AnyResult<()> {
    // Test borrow propagation in multiply-subtract step.
    // Large dividend and divisor with specific bit patterns.
    // Dividend: 2^127 - 1
    // Divisor: 2^63 + 2^32
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 170141183460469231731687303715884105727",
            ": int / 9223372041149743104",
            ": int / 18446744065119617027",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

#[test]
fn test_int_div_knuth_refinement_break() -> AnyResult<()> {
    // Triggers the 'break' in refinement loop when r_hat overflows after incrementing.
    // Constructed so:
    // - q_hat * v[n-2] > (r_hat << 32) | u[j+n-2] (refinement triggers)
    // - After q_hat -= 1, r_hat += v[n-1] causes r_hat >= 2^32 (break)
    // Dividend: 0x80000000_FFFFFFFE_00000000 = 39614081275578912861891592192
    // Divisor: 0x80000001_FFFFFFFF = 9223372045444710399
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 39614081275578912861891592192",
            ": int / 9223372045444710399",
            ": int / 4294967294",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

// ============================================================================
// int_from_limbs Tests
// ============================================================================

#[test]
fn test_int_from_limbs_zero() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 0")?;

    // Allocate result buffer.
    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_expected, 1)
    };
    assert!(!result_ptr.is_null());

    // Construct zero from empty limbs.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_from_limbs(
            rt.handle(),
            std::ptr::null(),  // limbs_ptr (null is ok for zero)
            0,                 // limb_count
            false,             // negative
            result_ptr,
            tydesc_expected,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    // Compare result with expected.
    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr, tydesc_expected,
            ptr_expected, tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals), "Zero from limbs does not match expected");

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_expected);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_from_limbs_single_positive() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    // 42 as a single limb.
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 42")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_expected, 1)
    };
    assert!(!result_ptr.is_null());

    let limbs: [u32; 1] = [42];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_from_limbs(
            rt.handle(),
            limbs.as_ptr(),
            1,
            false,
            result_ptr,
            tydesc_expected,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr, tydesc_expected,
            ptr_expected, tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals), "42 from limbs does not match expected");

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_expected);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_from_limbs_single_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    // -42 as a single limb with negative flag.
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / -42")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_expected, 1)
    };
    assert!(!result_ptr.is_null());

    let limbs: [u32; 1] = [42];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_from_limbs(
            rt.handle(),
            limbs.as_ptr(),
            1,
            true,  // negative
            result_ptr,
            tydesc_expected,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr, tydesc_expected,
            ptr_expected, tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals), "-42 from limbs does not match expected");

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_expected);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_from_limbs_multi_positive() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    // 2^32 + 1 = 4294967297 as two limbs: [1, 1] (little-endian).
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 4294967297")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_expected, 1)
    };
    assert!(!result_ptr.is_null());

    let limbs: [u32; 2] = [1, 1];  // low limb first: 1 + 1*2^32 = 4294967297
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_from_limbs(
            rt.handle(),
            limbs.as_ptr(),
            2,
            false,
            result_ptr,
            tydesc_expected,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr, tydesc_expected,
            ptr_expected, tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals), "4294967297 from limbs does not match expected");

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_expected);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_from_limbs_multi_negative() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    // -4294967297 as two limbs with negative flag.
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / -4294967297")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_expected, 1)
    };
    assert!(!result_ptr.is_null());

    let limbs: [u32; 2] = [1, 1];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_from_limbs(
            rt.handle(),
            limbs.as_ptr(),
            2,
            true,  // negative
            result_ptr,
            tydesc_expected,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr, tydesc_expected,
            ptr_expected, tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals), "-4294967297 from limbs does not match expected");

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_expected);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

#[test]
fn test_int_from_limbs_large() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    // 2^64 = 18446744073709551616 as three limbs: [0, 0, 1].
    let (ptr_expected, tydesc_expected) = instantiate_int(&db, &rt, &mut tydesc_table, ": int / 18446744073709551616")?;

    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), tydesc_expected, 1)
    };
    assert!(!result_ptr.is_null());

    let limbs: [u32; 3] = [0, 0, 1];  // 0 + 0*2^32 + 1*2^64 = 2^64
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_from_limbs(
            rt.handle(),
            limbs.as_ptr(),
            3,
            false,
            result_ptr,
            tydesc_expected,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            result_ptr, tydesc_expected,
            ptr_expected, tydesc_expected,
        )
    };
    assert!(matches!(eq_result, RtEq::Equals), "2^64 from limbs does not match expected");

    unsafe {
        cleanup_value(&rt, result_ptr, tydesc_expected);
        cleanup_value(&rt, ptr_expected, tydesc_expected);
    }
    Ok(())
}

// ============================================================================
// Multi-limb division
// ============================================================================

/// A quotient digit large enough to make one partial product exceed the range
/// of a signed 64-bit integer.
///
/// The subtraction step works a limb at a time, so it has only the low half of
/// each partial product to subtract and carries the high half. Taking the
/// whole product instead overflows here, which is what this divisor is chosen
/// to provoke.
#[test]
fn test_int_div_wide_partial_product() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    unsafe {
        test_binary_int_op(
            &db, &rt, &mut tydesc_table,
            ": int / 83362684186361745304358588555239549351",
            ": int / 1185984975656608777",
            ": int / 70289831572452108813",
            |rt, a, a_td, b, b_td, out, out_td| {
                datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
            },
        )?;
    }
    Ok(())
}

/// Division across limb counts, against the same arithmetic done in u128.
#[test]
fn test_int_div_matches_u128() -> AnyResult<()> {
    let db = Database::default();
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);

    // Values that put large limbs at the top of the divisor, where the
    // quotient digit estimate is largest, at every limb count from two up.
    // Literals are limited to what fits a signed 128-bit integer, so these
    // stay below that while still putting large limbs at the top of the
    // divisor, where the quotient digit estimate is largest.
    let cases: &[(u128, u128)] = &[
        (i128::MAX as u128, u64::MAX as u128),
        (i128::MAX as u128, (u64::MAX as u128) - 1),
        (i128::MAX as u128, 0xFFFF_FFFF_0000_0001),
        (i128::MAX as u128, 0x7FFF_FFFF_FFFF_FFFF_FFFF_FFFF),
        (i128::MAX as u128 - 1, 0x8000_0000_0000_0000),
        (0x7FFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_0000, 0xFFFF_FFFF_FFFF_FFFF_0001),
        (0x1234_5678_9ABC_DEF0_1234_5678_9ABC_DEF0, 0xFEDC_BA98_7654_3210),
        (0x1234_5678_9ABC_DEF0_1234_5678_9ABC_DEF0, 0xFFFF_FFFF_FFFF_FFFF_FFFF),
        (1 << 126, (1 << 64) + 1),
        (1 << 126, (1 << 96) - 1),
        (83362684186361745304358588555239549351, 1185984975656608777),
    ];

    for &(dividend, divisor) in cases {
        let expected = dividend / divisor;
        unsafe {
            test_binary_int_op(
                &db, &rt, &mut tydesc_table,
                &format!(": int / {dividend}"),
                &format!(": int / {divisor}"),
                &format!(": int / {expected}"),
                |rt, a, a_td, b, b_td, out, out_td| {
                    datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
                },
            ).with_context(|| format!("{dividend} / {divisor}"))?;
        }
    }
    Ok(())
}

use rmx::proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    /// Property: a quotient of values that fit 128 bits is the one 128-bit
    /// arithmetic gives, whatever limb counts the two sides come to.
    ///
    /// The divisor is held to half the width of the dividend, which is where
    /// the quotient digits are largest and the partial products widest. An
    /// even spread would mostly generate quotients of nought or one.
    #[test]
    fn proptest_int_div_matches_u128(
        dividend in (1u128 << 64)..=(i128::MAX as u128),
        divisor in 1u128..=(u64::MAX as u128),
    ) {
        let db = Database::default();
        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);

        let expected = dividend / divisor;
        unsafe {
            test_binary_int_op(
                &db, &rt, &mut tydesc_table,
                &format!(": int / {dividend}"),
                &format!(": int / {divisor}"),
                &format!(": int / {expected}"),
                |rt, a, a_td, b, b_td, out, out_td| {
                    datalove_rt::c::dtlv_rti_int_div_checked(rt, a, a_td, b, b_td, out, out_td)
                },
            ).expect("division matches");
        }
    }
}
