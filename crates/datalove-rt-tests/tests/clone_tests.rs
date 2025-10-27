//! Tests for clone runtime functions (Map and Set tree cloning).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;

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
fn test_clone_empty_map() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": @map <@u32, @string> / @map {}")?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the map.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned map is empty.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 0);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_map_single_entry() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": @map <@u32, @string> / @map { 1 = \"hello\" }")?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the map.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned map has one entry.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(!cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 1);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_map_multiple_entries() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        ": @map <@u32, @string> / @map { 1 = \"one\", 2 = \"two\", 3 = \"three\", 4 = \"four\", 5 = \"five\" }"
    )?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the map.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned map has five entries.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(!cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 5);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_map_nested_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        ": @map <@u32, @(@u32, @string)> / @map { 1 = @(10, \"first\"), 2 = @(20, \"second\") }"
    )?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the map.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned map has two entries.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(!cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 2);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_empty_set() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": @set <@u32> / @set {}")?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the set.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned set is empty.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 0);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_single_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, ": @set <@u32> / @set { 42 }")?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the set.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned set has one element.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 1);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_multiple_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        ": @set <@u32> / @set { 1, 2, 3, 4, 5 }"
    )?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the set.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned set has five elements.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 5);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_string_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        ": @set <@string> / @set { \"apple\", \"banana\", \"cherry\" }"
    )?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the set.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned set has three elements.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 3);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_nested_tuples() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db,
        ": @set <@(@u32, @string)> / @set { @(1, \"one\"), @(2, \"two\") }"
    )?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    // Clone the set.
    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify cloned set has two elements.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 2);

    // Clean up both original and clone.
    // Destroy contents of original.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc,
            1,
            inst.ptr as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Destroy contents of clone (cloned_buffer itself is freed by Rust when it drops).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
