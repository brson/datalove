//! Tests for destructor implementations (Int, List, Enum, Set).

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
fn test_destroy_int_small() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @int / @42")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the int to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Int>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned int.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify int is cleared.
    let int_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Int) };
    assert!(int_val.data.is_null());
    assert_eq!(int_val.capacity, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
#[ignore] // can't parse
fn test_destroy_int_large() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @int / @1234567890123456789012345678901234567890")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the int to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::Int>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned int.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify int is cleared.
    let int_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Int) };
    assert!(int_val.data.is_null());
    assert_eq!(int_val.capacity, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_list_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @[@u32] / @[]")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::List>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, 0);
    assert_eq!(list_val.capacity, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_list_primitives() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, "@[@1, @2, @3, @4, @5]")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::List>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, 0);
    assert_eq!(list_val.capacity, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_list_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, r#"@[@"hello", @"world", @"test"]"#)?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::List>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, 0);
    assert_eq!(list_val.capacity, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_list_nested() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, r#"@[@[@"a", @"b"], @[@"c", @"d"]]"#)?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Clone the list to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let mut cloned_buffer = vec![0u8; std::mem::size_of::<datalove_rt::rtdt::List>()];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned list.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify list is cleared.
    let list_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::List) };
    assert!(list_val.data.is_null());
    assert_eq!(list_val.size, 0);
    assert_eq!(list_val.capacity, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_enum_no_payload() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @enum Status { Ok, Error } / @enum Ok")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the enum.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
fn test_destroy_enum_with_primitive_payload() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Ok(@42)")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the enum.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
fn test_destroy_enum_with_string_payload() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, r#": @enum Result { Ok(@u32), Err(@string) } / @enum Result.Err(@"error message")"#)?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the enum.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
fn test_destroy_enum_with_nested_payload() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, r#": @enum Msg { Text(@string), Items(@[@string]) } / @enum Msg.Items(@[@"a", @"b", @"c"])"#)?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the enum.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
fn test_destroy_set_empty() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @set <@u32> / @set {}")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
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

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_set_primitives() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        ": @set <@u32> / @set { @1, @2, @3, @4, @5 }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
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

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_set_strings() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        r#": @set <@string> / @set { @"apple", @"banana", @"cherry" }"#
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
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

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_set_tuples() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db,
        r#": @set <@(@u32, @string)> / @set { @(1, "one"), @(2, "two"), @(3, "three") }"#
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
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

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_set_large() -> AnyResult<()> {
    let db = Database::default();
    // Create a set with 11 elements (maximum for single leaf node).
    let typechecked = compile(&db,
        ": @set <@u32> / @set { @1, @2, @3, @4, @5, @6, @7, @8, @9, @10, @11 }"
    )?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the set.
    // Clone to get a runtime-allocated copy.
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

    // Destroy the cloned value.
    let status = unsafe {
        datalove_rt::dtlv_rti_any_destroy_local(
            rt,
            cloned_buffer.as_mut_ptr(),
            inst.tydesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify set is cleared.
    let set_val = unsafe { &*(cloned_buffer.as_ptr() as *const datalove_rt::rtdt::Set) };
    assert!(set_val.root.is_null());
    assert_eq!(set_val.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_destroy_option_none() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, ": @?@u32 / @none")?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the option.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
#[ignore] // TODO: Implicit option wrapping doesn't work with strings
fn test_destroy_option_some_string() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, r#": @?@string / @"test string""#)?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the option.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
#[ignore] // TODO: Implicit result wrapping doesn't work with strings
fn test_destroy_result_ok_string() -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile(&db, r#": @!@string / @"success""#)?;

    let (_tydesc_table, _value_heap, inst) = instantiate::instantiate_value(&db, typechecked)?;

    // Destroy the result.
    // Clone to get a runtime-allocated copy.
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc_size = unsafe { (*inst.tydesc).size as usize };
    let mut cloned_buffer = vec![0u8; tydesc_size];

    let status = unsafe {
        datalove_rt::dtlv_rti_clone_local(
            rt,
            inst.value,
            inst.tydesc,
            cloned_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Destroy the cloned value.
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
