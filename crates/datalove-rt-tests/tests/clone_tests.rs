//! Tests for clone runtime functions (Map and Set tree cloning).

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;

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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Free original container memory.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
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
            inst.tydesc.as_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}


// ==================== Property-Based Tests ====================

use proptest::prelude::*;
use datalove_datalit::ast_gen::*;
use datalove_rt::rust::Runtime;

proptest! {
    #![proptest_config(ProptestConfig {
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    /// Property: Clone equals original - for all x, eq(x, clone(x)) = Equals.
    #[test]
    fn proptest_clone_equals_original(seed in any::<u64>()) {
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

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assume!(typechecked.errors(&db).is_empty());

        let inst = match instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        // Clone the value into a buffer.
        let tydesc = inst.tydesc.as_ref();
        let mut clone_buffer = vec![0u8; tydesc.size as usize];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_mut_ptr(),
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Test equality.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Clone should equal original");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
        }
    }

    /// Property: Clone transitivity - for all x, eq(clone(clone(x)), x) = Equals.
    #[test]
    fn proptest_clone_transitivity(seed in any::<u64>()) {
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

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assume!(typechecked.errors(&db).is_empty());

        let inst = match instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        let tydesc = inst.tydesc.as_ref();
        let buffer_size = tydesc.size as usize;

        // Clone once.
        let mut clone1_buffer = vec![0u8; buffer_size];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone1_buffer.as_mut_ptr(),
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Clone twice.
        let mut clone2_buffer = vec![0u8; buffer_size];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                clone1_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
                clone2_buffer.as_mut_ptr(),
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Test equality between double-cloned and original.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq(
                std::ptr::null_mut(),
                clone2_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
                inst.ptr,
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Double-cloned value should equal original");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone1_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone2_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 5,
        max_shrink_iters: 0,
        ..ProptestConfig::default()
    })]

    #[test]
    fn proptest_clone_moderate_containers_depth3(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            max_collection_size: 50,
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

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assume!(typechecked.errors(&db).is_empty());

        let inst = match instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        // Clone the moderate-sized container.
        let tydesc = inst.tydesc.as_ref();
        let mut clone_buffer = vec![0u8; tydesc.size as usize];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_mut_ptr(),
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Test equality.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Cloned moderate container should equal original");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
        }
    }

    #[test]
    #[ignore] // sometimes iloops
    fn proptest_clone_moderate_containers_depth2(seed in any::<u64>()) {
        let db = Database::default();
        let config = AstGenConfig {
            max_collection_size: 100,
            max_depth: 2,
            type_weights: TypeWeights {
                named_tuple_type: 0,
                named_struct_type: 0,
                named_enum_type: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expr = gen_expr_full_seeded(&db, seed, config);

        let rt = Runtime::new();
        let mut tydesc_table = TyDescTable::new(&db);
        let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
        prop_assume!(typechecked.errors(&db).is_empty());

        let inst = match instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        // Clone the moderate-sized container.
        let tydesc = inst.tydesc.as_ref();
        let mut clone_buffer = vec![0u8; tydesc.size as usize];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                rt.handle(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_mut_ptr(),
            )
        };
        prop_assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Test equality.
        let result = unsafe {
            datalove_rt::c::dtlv_rti_eq(
                std::ptr::null_mut(),
                inst.ptr,
                inst.tydesc.as_ptr(),
                clone_buffer.as_ptr(),
                inst.tydesc.as_ptr(),
            )
        };

        prop_assert!(matches!(result, datalove_rt::c::RtEq::Equals),
            "Cloned moderate container should equal original");

        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
            datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
            datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
        }
    }
}

/// Regression test for clone leak detected with seed 980509222901775213.
#[test]
#[ignore] // slow
fn test_clone_leak_regression_seed_980509222901775213() {
    let db = Database::default();
    let seed = 980509222901775213u64;

    let config = AstGenConfig {
        max_collection_size: 150,
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

    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let resolved = datalove_datalit::resolve::resolve_names(&db, bct::input::Source::new(&db, String::new()), expr);
    let typechecked = datalove_datalit::tycheck::type_check(&db, expr, resolved);
    assert!(typechecked.errors(&db).is_empty(), "Type checking failed for seed {}", seed);

    let inst = match instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked) {
        Ok(v) => v,
        Err(e) => {
            println!("Instantiation failed for seed {}: {:?}", seed, e);
            return;
        }
    };

    // Clone the moderate-sized container.
    let tydesc = inst.tydesc.as_ref();
    let mut clone_buffer = vec![0u8; tydesc.size as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            clone_buffer.as_mut_ptr(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Test equality.
    let result = unsafe {
        datalove_rt::c::dtlv_rti_eq(
            std::ptr::null_mut(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            clone_buffer.as_ptr(),
            inst.tydesc.as_ptr(),
        )
    };

    assert!(matches!(result, datalove_rt::c::RtEq::Equals),
        "Cloned moderate container should equal original");

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), inst.ptr as *mut u8, inst.tydesc.as_ptr());
        datalove_rt::c::dtlv_rti_mem_free_local(rt.handle(), inst.tydesc.as_ptr(), 1, inst.ptr as *mut u8);
        datalove_rt::c::dtlv_rti_any_destroy_local(rt.handle(), clone_buffer.as_mut_ptr(), inst.tydesc.as_ptr());
    }
}
