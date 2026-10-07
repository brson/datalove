//! Tests for destructor implementations (Int, List, Enum, Set).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;
use datalove_rtdt;

#[salsa::tracked(returns(copy))]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr(db);
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.clone());
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

fn compile_str<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    Ok(compile(db, source))
}

#[test]
fn test_destroy_int_small() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": int / 42")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Clone the int to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Int>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned int.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify int is cleared.
    let int_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Int) };
    assert!(int_val.data.is_null());
    assert_eq!(int_val.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
#[ignore] // can't parse
fn test_destroy_int_large() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": int / 1234567890123456789012345678901234567890")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Clone the int to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Int>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned int.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify int is cleared.
    let int_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Int) };
    assert!(int_val.data.is_null());
    assert_eq!(int_val.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_list_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": [u32] / []")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::List>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, datalove_rtdt::Index::ZERO);
    assert_eq!(list_val.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_list_primitives() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, "[1, 2, 3, 4, 5]")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::List>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, datalove_rtdt::Index::ZERO);
    assert_eq!(list_val.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_list_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#"["hello", "world", "test"]"#)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::List>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, datalove_rtdt::Index::ZERO);
    assert_eq!(list_val.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_list_nested() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#"[["a", "b"], ["c", "d"]]"#)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::List>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, datalove_rtdt::Index::ZERO);
    assert_eq!(list_val.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_set_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": #{u32} / #{}")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Set>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_set_primitives() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        ": #{u32} / #{ 1, 2, 3, 4, 5 }"
    )?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Set>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_set_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        r#": #{string} / #{ "apple", "banana", "cherry" }"#
    )?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Set>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_set_tuples() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        r#": #{(u32, string)} / #{ (1, "one"), (2, "two"), (3, "three") }"#
    )?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Set>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_set_large() -> AnyResult<()> {
    let db = Database::default();
    // Create a set with 11 elements (maximum for single leaf node).
    let typechecked = compile_str(&db,
        ": #{u32} / #{ 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11 }"
    )?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Set>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_option_none() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": ?u32 / none")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the option.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = inst.tydesc.size() as usize;
    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(tydesc_size);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_option_some_string() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#": ?string / some "test string""#)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the option.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = inst.tydesc.size() as usize;
    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(tydesc_size);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_result_ok_string() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#": !string / ok "success""#)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    // Destroy the result.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = inst.tydesc.size() as usize;
    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(tydesc_size);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

// Map destroy tests

#[test]
fn test_destroy_map_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": %{u32 = u32} / %{}")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Map>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let map_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Map) };
    assert!(map_val.root.is_null());
    assert_eq!(map_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_map_primitives() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, "%{10 = 100, 20 = 200, 30 = 300}")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Map>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let map_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Map) };
    assert!(map_val.root.is_null());
    assert_eq!(map_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_map_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#"%{"key1" = "val1", "key2" = "val2"}"#)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Map>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let map_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Map) };
    assert!(map_val.root.is_null());
    assert_eq!(map_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_map_tuples() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, "%{(1, 2) = (3, 4), (5, 6) = (7, 8)}")?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Map>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let map_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Map) };
    assert!(map_val.root.is_null());
    assert_eq!(map_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_map_nested() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#"%{"outer" = ["inner1", "inner2"]}"#)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Map>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let map_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Map) };
    assert!(map_val.root.is_null());
    assert_eq!(map_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_map_large() -> AnyResult<()> {
    let db = Database::default();
    let mut map_literal = String::from("%{");
    for i in 0..10 {
        if i > 0 {
            map_literal.push_str(", ");
        }
        map_literal.push_str(&format!("{} = {}", i, i * 10));
    }
    map_literal.push('}');

    let typechecked = compile_str(&db, &map_literal)?;

    let rt1 = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt1.handle(), &mut tydesc_table, typechecked)?;

    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Map>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let map_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Map) };
    assert!(map_val.root.is_null());
    assert_eq!(map_val.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up the instantiated value from the first runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt1.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt1.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}


// ==================== Property-Based Tests ====================

#[cfg(feature = "slow_tests")]
use rmx::proptest::prelude::*;
#[cfg(feature = "slow_tests")]
use datalove_datalit::ast_gen::*;

#[cfg(feature = "slow_tests")]
proptest! {
    #![proptest_config(ProptestConfig {
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    /// Property: Destroy moderate structures with depth 3-4.
    #[test]
    #[ignore] // ~25s - too slow for regular test runs
    fn proptest_destroy_moderate_structures(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            max_collection_size: 30,
            max_depth: 3,
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt, &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Destroy the moderate-sized structure.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                inst.ptr as *mut u8,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "Destroy should succeed for moderate structures");

        // Free the memory.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt,
                inst.tydesc.as_ptr(),
                1,
                inst.ptr as *mut u8,
            );
            datalove_rt::c::dtlv_rti_shutdown(rt);
        }
    }

    /// Property: Destroy all types - test destruction across all datalit types.
    #[test]
    fn proptest_destroy_all_types(seed in any::<u64>()) {
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

        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt, &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Destroy the value.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                inst.ptr as *mut u8,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "Destroy should succeed for all types");

        // Free the memory.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt,
                inst.tydesc.as_ptr(),
                1,
                inst.ptr as *mut u8,
            );
            datalove_rt::c::dtlv_rti_shutdown(rt);
        }
    }

    /// Property: Deep nesting destruction - test with depth 3-4.
    #[test]
    #[ignore] // slow
    fn proptest_destroy_deep_nesting(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            max_depth: 4,
            max_collection_size: 30,
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = datalove_rt::c::dtlv_rti_init();
        prop_assert!(!rt.is_null());
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assert!(typechecked.errors(&db).is_empty());

        let inst = instantiate2::instantiate_value(&db, rt, &mut tydesc_table, typechecked)
            .expect("Should instantiate");

        // Destroy the deeply nested structure.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt,
                inst.ptr as *mut u8,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok,
            "Destroy should succeed for deeply nested structures");

        // Free the memory.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt,
                inst.tydesc.as_ptr(),
                1,
                inst.ptr as *mut u8,
            );
            datalove_rt::c::dtlv_rti_shutdown(rt);
        }
    }
}

// ============================================================================
// Table destroy tests
// ============================================================================

#[test]
fn test_destroy_empty_table() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": {| x: u32 |} / {| x |}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone to get a runtime-allocated copy.
    let rt2 = datalove_rt::c::dtlv_rti_init();
    assert!(!rt2.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Table>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt2,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned table.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt2,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify table is cleared.
    let table = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Table) };
    assert!(table.data.is_null());
    assert_eq!(table.len, datalove_rtdt::Index::ZERO);
    assert_eq!(table.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt2) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_table_with_rows() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": {| x: u32, y: u32 |} / {| x, y; 1, 2; 3, 4 |}")?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone to get a runtime-allocated copy.
    let rt2 = datalove_rt::c::dtlv_rti_init();
    assert!(!rt2.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Table>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt2,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify table has data.
    let table_before = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Table) };
    assert!(!table_before.data.is_null());
    assert_eq!(table_before.len, datalove_rtdt::Index(2));

    // Destroy the cloned table.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt2,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify table is cleared.
    let table = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Table) };
    assert!(table.data.is_null());
    assert_eq!(table.len, datalove_rtdt::Index::ZERO);
    assert_eq!(table.capacity, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt2) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}

#[test]
fn test_destroy_table_with_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, r#": {| name: string |} / {| name; "Alice"; "Bob"; "Charlie" |}"#)?;

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone to get a runtime-allocated copy.
    let rt2 = datalove_rt::c::dtlv_rti_init();
    assert!(!rt2.is_null());

    let mut cloned_buffer = datalove_rt::rust::AlignedBuffer::new(std::mem::size_of::<datalove_rtdt::Table>());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt2,
            inst.ptr,
            inst.tydesc.as_ptr(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy the cloned table (should properly destroy nested strings).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt2,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify table is cleared.
    let table = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rtdt::Table) };
    assert!(table.data.is_null());
    assert_eq!(table.len, datalove_rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt2) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clean up original.
    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
    }
    Ok(())
}
