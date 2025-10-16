//! Tests for clone runtime functions (Map and Set tree cloning).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate};

fn compile<'db>(db: &'db Database, source_text: &str) -> AnyResult<datalove_datalit::tycheck::TypecheckResult<'db>> {
    let source = bct::input::Source::new(db, source_text.to_string());
    let parsed = datalove_datalit::parser::parse(db, source);
    let resolved = datalove_datalit::resolve::resolve_names(db, parsed);
    let typechecked = datalove_datalit::tycheck::type_check(db, parsed, resolved);
    Ok(typechecked)
}

#[test]
fn test_clone_empty_map() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @map <@u32, @string> / @map {}")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the map.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned map is empty.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 0);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_map_single_entry() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @map <@u32, @string> / @map { 1 = \"hello\" }")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the map.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned map has one entry.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(!cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 1);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_map_multiple_entries() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        ": @map <@u32, @string> / @map { 1 = \"one\", 2 = \"two\", 3 = \"three\", 4 = \"four\", 5 = \"five\" }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the map.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned map has five entries.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(!cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 5);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_map_nested_values() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        ": @map <@u32, @(@u32, @string)> / @map { 1 = @(10, \"first\"), 2 = @(20, \"second\") }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the map.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Map>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned map has two entries.
    let cloned_map = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Map) };
    assert!(!cloned_map.root.is_null());
    assert_eq!(cloned_map.len, 2);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_empty_set() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @set <@u32> / @set {}")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the set.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned set is empty.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 0);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_single_element() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @set <@u32> / @set { 42 }")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the set.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned set has one element.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 1);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_multiple_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        ": @set <@u32> / @set { 1, 2, 3, 4, 5 }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the set.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned set has five elements.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 5);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_string_elements() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        ": @set <@string> / @set { \"apple\", \"banana\", \"cherry\" }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the set.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned set has three elements.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 3);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_clone_set_nested_tuples() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        ": @set <@(@u32, @string)> / @set { @(1, \"one\"), @(2, \"two\") }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the set.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Set>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify cloned set has two elements.
    let cloned_set = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(!cloned_set.root.is_null());
    assert_eq!(cloned_set.len, 2);

    // Clean up.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}
