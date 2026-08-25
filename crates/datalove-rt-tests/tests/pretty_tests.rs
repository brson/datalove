//! Tests for pretty-printing runtime values.
//!
//! Parses datalit source, instantiates to runtime, pretty-prints, and verifies output.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::c::RtStatus;
use datalove_rtdt as rtdt;

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

/// Creates a String tydesc for output.
fn create_string_tydesc() -> rtdt::TyDesc {
    rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
    }
}

/// Extracts the contents of a runtime String as a Rust String.
unsafe fn get_string_contents(string: &rtdt::String) -> String {
    unsafe {
        if string.data.is_null() || string.size == rtdt::Index::ZERO {
            return String::new();
        }
        let bytes = std::slice::from_raw_parts(string.data, string.size.as_usize());
        String::from_utf8_lossy(bytes).to_string()
    }
}

/// Pretty-prints a value and returns the result as a String.
fn pretty_print_value(
    rt: &datalove_rt::rust::Runtime,
    inst: &instantiate2::InstantiatedValue,
) -> AnyResult<String> {
    let string_tydesc = create_string_tydesc();

    unsafe {
        // Create output string.
        let mut output_string = std::mem::MaybeUninit::<rtdt::String>::uninit();
        let status = datalove_rt::c::dtlv_rti_string_create_local(
            rt.handle(),
            output_string.as_mut_ptr() as *mut u8,
            &string_tydesc,
        );
        assert_eq!(status, RtStatus::Ok, "Failed to create output string");
        let mut output_string = output_string.assume_init();

        // Pretty-print.
        let status = datalove_rt::c::dtlv_rti_pretty_print_local(
            rt.handle(),
            inst.ptr,
            inst.tydesc.as_ptr(),
            &mut output_string as *mut rtdt::String as *mut u8,
            &string_tydesc,
        );
        assert_eq!(status, RtStatus::Ok, "Pretty print failed");

        let result = get_string_contents(&output_string);

        // Clean up output string.
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt.handle(),
            &mut output_string as *mut rtdt::String as *mut u8,
            &string_tydesc,
        );

        Ok(result)
    }
}

/// Cleans up an instantiated value.
fn cleanup_value(rt: &datalove_rt::rust::Runtime, inst: &instantiate2::InstantiatedValue) {
    unsafe {
        // Destroy contents.
        let status = datalove_rt::c::dtlv_rti_any_destroy_local(
            rt.handle(),
            inst.ptr as *mut u8,
            inst.tydesc.as_ptr(),
        );
        assert_eq!(status, RtStatus::Ok);

        // Free container memory.
        let status = datalove_rt::c::dtlv_rti_mem_free_local(
            rt.handle(),
            inst.tydesc.as_ptr(),
            1,
            inst.ptr as *mut u8,
        );
        assert_eq!(status, RtStatus::Ok);
    }
}

/// Tests pretty-printing with given input and expected output.
fn test_pretty(source: &str, expected: &str) -> AnyResult<()> {
    let db = Database::default();
    let typechecked = compile_str(&db, source)?;

    let rt = datalove_rt::rust::Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let inst = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked)?;

    let result = pretty_print_value(&rt, &inst)?;
    assert_eq!(result, expected, "source: {}", source);

    cleanup_value(&rt, &inst);
    Ok(())
}

// Primitives

#[test]
fn test_pretty_bool_true() -> AnyResult<()> {
    test_pretty(": bool / true", "true")
}

#[test]
fn test_pretty_bool_false() -> AnyResult<()> {
    test_pretty(": bool / false", "false")
}

#[test]
fn test_pretty_u8() -> AnyResult<()> {
    test_pretty(": u8 / 42", "42")
}

#[test]
fn test_pretty_i8_positive() -> AnyResult<()> {
    test_pretty(": i8 / 42", "42")
}

#[test]
fn test_pretty_i8_negative() -> AnyResult<()> {
    test_pretty(": i8 / -42", "-42")
}

#[test]
fn test_pretty_u16() -> AnyResult<()> {
    test_pretty(": u16 / 1000", "1000")
}

#[test]
fn test_pretty_i16_negative() -> AnyResult<()> {
    test_pretty(": i16 / -1000", "-1000")
}

#[test]
fn test_pretty_u32() -> AnyResult<()> {
    test_pretty(": u32 / 12345", "12345")
}

#[test]
fn test_pretty_i32_negative() -> AnyResult<()> {
    test_pretty(": i32 / -12345", "-12345")
}

#[test]
fn test_pretty_u64() -> AnyResult<()> {
    test_pretty(": u64 / 9876543210", "9876543210")
}

#[test]
fn test_pretty_i64_negative() -> AnyResult<()> {
    test_pretty(": i64 / -9876543210", "-9876543210")
}

#[test]
fn test_pretty_f32() -> AnyResult<()> {
    test_pretty(": f32 / 3.14", "3.14")
}

#[test]
fn test_pretty_f32_integer() -> AnyResult<()> {
    test_pretty(": f32 / 42.0", "42")
}

#[test]
fn test_pretty_f32_negative() -> AnyResult<()> {
    test_pretty(": f32 / -2.5", "-2.5")
}

// Bigints

#[test]
fn test_pretty_int_zero() -> AnyResult<()> {
    test_pretty(": int / 0", "0")
}

#[test]
fn test_pretty_int_small_positive() -> AnyResult<()> {
    test_pretty(": int / 42", "42")
}

#[test]
fn test_pretty_int_small_negative() -> AnyResult<()> {
    test_pretty(": int / -42", "-42")
}

#[test]
fn test_pretty_int_large() -> AnyResult<()> {
    // 2^64 = 18446744073709551616
    test_pretty(": int / 18446744073709551616", "18446744073709551616")
}

#[test]
fn test_pretty_int_large_negative() -> AnyResult<()> {
    test_pretty(": int / -18446744073709551616", "-18446744073709551616")
}

// Strings

#[test]
fn test_pretty_string_simple() -> AnyResult<()> {
    test_pretty(": string / \"hello\"", "\"hello\"")
}

#[test]
fn test_pretty_string_empty() -> AnyResult<()> {
    test_pretty(": string / \"\"", "\"\"")
}

#[test]
fn test_pretty_string_with_spaces() -> AnyResult<()> {
    test_pretty(": string / \"hello world\"", "\"hello world\"")
}

#[test]
fn test_pretty_string_with_quotes() -> AnyResult<()> {
    // Source uses \" to represent embedded quotes.
    test_pretty(r#": string / "say \"hello\"""#, r#""say \"hello\"""#)
}

#[test]
fn test_pretty_string_with_newline() -> AnyResult<()> {
    // Source has actual newline character, pretty-printed output escapes it.
    test_pretty(": string / \"line1\nline2\"", "\"line1\\nline2\"")
}

#[test]
fn test_pretty_string_with_tab() -> AnyResult<()> {
    // Source has actual tab character, pretty-printed output escapes it.
    test_pretty(": string / \"col1\tcol2\"", "\"col1\\tcol2\"")
}

#[test]
fn test_pretty_string_with_backslash() -> AnyResult<()> {
    // Source uses \\ to represent literal backslashes, pretty-printed output escapes them.
    test_pretty(r#": string / "path\\to\\file""#, r#""path\\to\\file""#)
}

// Tuples

#[test]
fn test_pretty_tuple_pair() -> AnyResult<()> {
    test_pretty(": (u32, bool) / (42, true)", "(42, true)")
}

#[test]
fn test_pretty_tuple_triple() -> AnyResult<()> {
    test_pretty(
        ": (u32, string, bool) / (1, \"hi\", false)",
        "(1, \"hi\", false)",
    )
}

#[test]
fn test_pretty_tuple_nested() -> AnyResult<()> {
    test_pretty(
        ": ((u32, u32), bool) / ((1, 2), true)",
        "((1, 2), true)",
    )
}

// Option

#[test]
fn test_pretty_option_none() -> AnyResult<()> {
    test_pretty(": ?u32 / none", "none")
}

#[test]
fn test_pretty_option_some() -> AnyResult<()> {
    test_pretty(": ?u32 / some 42", "some 42")
}

#[test]
fn test_pretty_option_some_string() -> AnyResult<()> {
    test_pretty(": ?string / some \"hello\"", "some \"hello\"")
}

// Result

#[test]
fn test_pretty_result_ok() -> AnyResult<()> {
    test_pretty(": !u32 / ok 42", "ok 42")
}

// Lists

#[test]
fn test_pretty_list_empty() -> AnyResult<()> {
    test_pretty(": [u32] / []", "[]")
}

#[test]
fn test_pretty_list_single() -> AnyResult<()> {
    test_pretty(": [u32] / [1]", "[1]")
}

#[test]
fn test_pretty_list_multiple() -> AnyResult<()> {
    test_pretty(": [u32] / [1, 2, 3]", "[1, 2, 3]")
}

#[test]
fn test_pretty_list_strings() -> AnyResult<()> {
    test_pretty(
        ": [string] / [\"a\", \"b\", \"c\"]",
        "[\"a\", \"b\", \"c\"]",
    )
}

// Sets

#[test]
fn test_pretty_set_empty() -> AnyResult<()> {
    test_pretty(": #{u32} / #{}", "#{}")
}

#[test]
fn test_pretty_set_single() -> AnyResult<()> {
    test_pretty(": #{u32} / #{42}", "#{42}")
}

#[test]
fn test_pretty_set_multiple() -> AnyResult<()> {
    // Sets are ordered, so output should be sorted.
    test_pretty(": #{u32} / #{3, 1, 2}", "#{1, 2, 3}")
}

#[test]
fn test_pretty_set_strings() -> AnyResult<()> {
    test_pretty(
        ": #{string} / #{\"banana\", \"apple\"}",
        "#{\"apple\", \"banana\"}",
    )
}

// Maps

#[test]
fn test_pretty_map_empty() -> AnyResult<()> {
    test_pretty(": %{u32 = string} / %{}", "%{}")
}

#[test]
fn test_pretty_map_single() -> AnyResult<()> {
    test_pretty(
        ": %{u32 = string} / %{1 = \"one\"}",
        "%{1 = \"one\"}",
    )
}

#[test]
fn test_pretty_map_multiple() -> AnyResult<()> {
    // Maps are ordered by key.
    test_pretty(
        ": %{u32 = string} / %{2 = \"two\", 1 = \"one\"}",
        "%{1 = \"one\", 2 = \"two\"}",
    )
}

#[test]
fn test_pretty_map_string_keys() -> AnyResult<()> {
    test_pretty(
        ": %{string = u32} / %{\"b\" = 2, \"a\" = 1}",
        "%{\"a\" = 1, \"b\" = 2}",
    )
}

// Complex nested structures

#[test]
fn test_pretty_list_of_tuples() -> AnyResult<()> {
    test_pretty(
        ": [(u32, string)] / [(1, \"a\"), (2, \"b\")]",
        "[(1, \"a\"), (2, \"b\")]",
    )
}

#[test]
fn test_pretty_map_of_lists() -> AnyResult<()> {
    test_pretty(
        ": %{string = [u32]} / %{\"nums\" = [1, 2, 3]}",
        "%{\"nums\" = [1, 2, 3]}",
    )
}

#[test]
fn test_pretty_set_of_tuples() -> AnyResult<()> {
    test_pretty(
        ": #{(u32, u32)} / #{(1, 2), (3, 4)}",
        "#{(1, 2), (3, 4)}",
    )
}

#[test]
fn test_pretty_option_of_list() -> AnyResult<()> {
    test_pretty(
        ": ?[u32] / some [1, 2, 3]",
        "some [1, 2, 3]",
    )
}

#[test]
fn test_pretty_deeply_nested() -> AnyResult<()> {
    test_pretty(
        ": %{string = ?[(u32, bool)]} / %{\"data\" = some [(1, true)]}",
        "%{\"data\" = some [(1, true)]}",
    )
}

// Tensors

#[test]
fn test_pretty_tensor_1d() -> AnyResult<()> {
    test_pretty(
        ": [|u32, 1|] / [| 1 2 3 |]",
        "[| 1 2 3 |]",
    )
}

#[test]
fn test_pretty_tensor_1d_empty() -> AnyResult<()> {
    test_pretty(
        ": [|u32, 1|] / [| |]",
        "[| |]",
    )
}

#[test]
fn test_pretty_tensor_2d() -> AnyResult<()> {
    test_pretty(
        ": [|u32, 2|] / [| 1 2 3, 4 5 6 |]",
        "[| 1 2 3, 4 5 6 |]",
    )
}

#[test]
fn test_pretty_tensor_2d_single_row() -> AnyResult<()> {
    test_pretty(
        ": [|u32, 2|] / [| 1 2 3, |]",
        "[| 1 2 3, |]",
    )
}

#[test]
fn test_pretty_tensor_3d() -> AnyResult<()> {
    test_pretty(
        ": [|u32, 3|] / [| 1 2, 3 4,, 5 6, 7 8 |]",
        "[| 1 2, 3 4,, 5 6, 7 8 |]",
    )
}

#[test]
fn test_pretty_tensor_f32() -> AnyResult<()> {
    test_pretty(
        ": [|f32, 1|] / [| 1.5 2.5 3.5 |]",
        "[| 1.5 2.5 3.5 |]",
    )
}

// Tables

#[test]
fn test_pretty_table_empty() -> AnyResult<()> {
    test_pretty(
        ": {| x: u32, y: u32 |} / {| x, y |}",
        "{| x, y |}",
    )
}

#[test]
fn test_pretty_table_single_row() -> AnyResult<()> {
    test_pretty(
        ": {| x: u32, y: u32 |} / {| x, y; 1, 2 |}",
        "{| x, y; 1, 2 |}",
    )
}

#[test]
fn test_pretty_table_multiple_rows() -> AnyResult<()> {
    test_pretty(
        ": {| x: u32, y: u32 |} / {| x, y; 1, 2; 3, 4 |}",
        "{| x, y; 1, 2; 3, 4 |}",
    )
}

#[test]
fn test_pretty_table_with_strings() -> AnyResult<()> {
    test_pretty(
        r#": {| name: string, age: u32 |} / {| name, age; "Alice", 30 |}"#,
        r#"{| name, age; "Alice", 30 |}"#,
    )
}
