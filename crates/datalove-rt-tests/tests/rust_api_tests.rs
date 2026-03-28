//! Tests for the safe Rust wrapper API in rust.rs.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::{Runtime, MemGuard, ValueGuard};
use datalove_rt::c::RtStatus;

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

// ==================== Runtime Tests ====================

#[test]
fn test_runtime_new() {
    let rt = Runtime::new();
    assert!(!rt.handle().is_null());
}

#[test]
fn test_runtime_default() {
    let rt = Runtime::default();
    assert!(!rt.handle().is_null());
}

#[test]
fn test_runtime_handle() {
    let rt = Runtime::new();
    let handle1 = rt.handle();
    let handle2 = rt.handle();
    assert_eq!(handle1, handle2);
    assert!(!handle1.is_null());
}

#[test]
fn test_runtime_drop() {
    // Create and drop runtime in inner scope.
    {
        let _rt = Runtime::new();
    }
    // If we get here, drop succeeded.
}

#[test]
fn test_runtime_multiple_instances() {
    let rt1 = Runtime::new();
    let rt2 = Runtime::new();
    assert!(!rt1.handle().is_null());
    assert!(!rt2.handle().is_null());
    assert_ne!(rt1.handle(), rt2.handle());
}

// ==================== MemGuard Tests ====================

#[test]
fn test_memguard_new() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Allocate with MemGuard.
    let guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1);
    assert!(guard.is_some());
    let guard = guard.unwrap();
    assert!(!guard.ptr().is_null());

    // Clean up the instantiated value.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_memguard_ptr() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();
    let ptr1 = guard.ptr();
    let ptr2 = guard.ptr();
    assert_eq!(ptr1, ptr2);
    assert!(!ptr1.is_null());

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_memguard_tydesc() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();
    let tydesc = guard.tydesc();
    assert_eq!(tydesc, inst.tydesc.as_ptr());

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_memguard_leak() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();
    let leaked_ptr = guard.leak();
    assert!(!leaked_ptr.is_null());

    // Manually clean up the leaked memory.
    unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, leaked_ptr);
    }

    // Clean up inst.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_memguard_into_value() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();
    let mem_ptr = guard.ptr();

    // Copy the value into the MemGuard's buffer.
    let tydesc = inst.tydesc.as_ref();
    unsafe {
        std::ptr::copy_nonoverlapping(inst.ptr, mem_ptr, tydesc.size as usize);
    }

    // Clone the inner data if needed.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            mem_ptr,
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    // Convert to ValueGuard.
    let value_guard = unsafe { guard.into_value() };
    assert_eq!(value_guard.ptr(), mem_ptr);
    assert_eq!(value_guard.tydesc(), inst.tydesc.as_ptr());

    // Clean up the original instantiated value.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    // value_guard will clean up itself on drop.
    Ok(())
}

#[test]
fn test_memguard_drop() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Create and drop MemGuard in inner scope.
    {
        let _guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();
    }

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

// ==================== ValueGuard Tests ====================

#[test]
fn test_valueguard_from_raw() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the value into a new allocation.
    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), inst.tydesc.as_ptr(), 1)
    };
    assert!(!cloned_ptr.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_ptr,
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    // Take ownership with ValueGuard.
    let value_guard = unsafe {
        ValueGuard::from_raw(rt.handle(), inst.tydesc.as_ptr(), cloned_ptr)
    };
    assert_eq!(value_guard.ptr(), cloned_ptr);

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    // value_guard will clean up on drop.
    drop(value_guard);

    Ok(())
}

#[test]
fn test_valueguard_ptr() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), inst.tydesc.as_ptr(), 1)
    };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_ptr,
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let value_guard = unsafe {
        ValueGuard::from_raw(rt.handle(), inst.tydesc.as_ptr(), cloned_ptr)
    };

    let ptr1 = value_guard.ptr();
    let ptr2 = value_guard.ptr();
    assert_eq!(ptr1, ptr2);
    assert_eq!(ptr1, cloned_ptr);

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_valueguard_tydesc() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), inst.tydesc.as_ptr(), 1)
    };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_ptr,
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let value_guard = unsafe {
        ValueGuard::from_raw(rt.handle(), inst.tydesc.as_ptr(), cloned_ptr)
    };
    assert_eq!(value_guard.tydesc(), inst.tydesc.as_ptr());

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_valueguard_leak() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), inst.tydesc.as_ptr(), 1)
    };
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_ptr,
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let value_guard = unsafe {
        ValueGuard::from_raw(rt.handle(), inst.tydesc.as_ptr(), cloned_ptr)
    };

    let leaked_ptr = value_guard.leak();
    assert_eq!(leaked_ptr, cloned_ptr);

    // Manually clean up the leaked value.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), leaked_ptr, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, leaked_ptr);
    }

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

#[test]
fn test_valueguard_drop() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": u32 / 42")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Create and drop ValueGuard in inner scope.
    {
        let cloned_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt.handle(), inst.tydesc.as_ptr(), 1)
        };
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                cloned_ptr,
                inst.tydesc.as_ptr(),
            )
        };
        assert_eq!(status, RtStatus::Ok);

        let _value_guard = unsafe {
            ValueGuard::from_raw(rt.handle(), inst.tydesc.as_ptr(), cloned_ptr)
        };
        // Guard drops here.
    }

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}

// ==================== Integration Tests ====================

#[test]
fn test_memguard_to_valueguard_workflow() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, "\"hello world\"")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Allocate with MemGuard.
    let mem_guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();

    // Clone value into MemGuard.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            mem_guard.ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    // Convert to ValueGuard for RAII cleanup.
    let _value_guard = unsafe { mem_guard.into_value() };

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    // ValueGuard handles cleanup.
    Ok(())
}

#[test]
fn test_guards_with_complex_type() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": ⦃u32⦄ / ⦃ 1, 2, 3 ⦄")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Test MemGuard with complex type.
    let mem_guard = MemGuard::new(rt.handle(), inst.tydesc.as_ptr(), 1).unwrap();
    let tydesc = mem_guard.tydesc();
    assert!(!tydesc.is_null());

    // Clone into it.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            mem_guard.ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, RtStatus::Ok);

    // Convert to ValueGuard.
    let value_guard = unsafe { mem_guard.into_value() };

    // Verify contents by comparing with original.
    let eq_result = unsafe {
        datalove_rt::c::dtlv_rti_eq_local(
            std::ptr::null_mut(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            value_guard.ptr(),
            value_guard.tydesc(),
        )
    };
    assert!(matches!(eq_result, datalove_rt::c::RtEq::Equals));

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }

    Ok(())
}
