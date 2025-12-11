//! Tests that exercise known parser panics.
//!
//! Each test documents a specific panic case in the datalit parser.
//! Tests use `#[should_panic]` to verify the panic occurs.
//! When the panics are fixed to return ParseError nodes instead,
//! those tests are updated to verify the error node is returned.

use rmx::prelude::*;

fn parse(source: &str) -> String {
    let db = datalove_datalit::Database::default();
    let src = bct::input::Source::new(&db, source.S());
    let ast = datalove_datalit::parser::parse_integration_test(&db, src);
    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);
    rmx::serde_json::to_string_pretty(&serde_ast).X()
}

// =============================================================================
// Type hint parsing: fixed cases (now return error nodes)
// =============================================================================

/// `tuple` keyword without name or parens.
/// Now returns a parse error node instead of panicking.
#[test]
fn type_hint_tuple_missing_name() {
    let json = parse(": tuple / @42");
    assert!(json.contains("ParseError") || json.contains("Error"), "expected parse error node");
}

/// `struct` keyword without name or braces.
/// Now returns a parse error node instead of panicking.
#[test]
fn type_hint_struct_missing_name() {
    let json = parse(": struct / @42");
    assert!(json.contains("ParseError") || json.contains("Error"), "expected parse error node");
}

/// `enum` keyword without name or braces.
/// Now returns a parse error node instead of panicking.
#[test]
fn type_hint_named_enum_missing_name() {
    let json = parse(": enum / @42");
    assert!(json.contains("ParseError") || json.contains("Error"), "expected parse error node");
}

/// Line 557: struct field without name in non-empty braces
/// Struct field expects `name: type`
/// Note: Empty braces `{}` don't trigger the field parser
#[test]
#[should_panic(expected = "expected name")]
fn type_hint_struct_field_missing_name() {
    // Need actual content in braces to trigger field parsing
    parse(": struct Foo { : i32 } / @42");
}

/// Line 564: enum variant without name in non-empty braces
/// Enum variant expects a name
/// Note: Empty braces `{}` don't trigger the variant parser
#[test]
#[should_panic(expected = "expected name")]
fn type_hint_enum_variant_missing_name() {
    // Need actual content in braces to trigger variant parsing
    parse(": enum { (i32) } / @42");
}

// =============================================================================
// Type hint parsing: need_sigil panics
// =============================================================================

/// Line 360: map type hint missing comma between key and value type
/// `map<K, V>` expects a comma
#[test]
#[should_panic(expected = "expected sigil ,")]
fn type_hint_map_missing_comma() {
    parse(": map<i32 i32> / @42");
}

/// Line 449: tensor type hint missing comma between element type and rank
/// `tensor<T, N>` expects a comma
#[test]
#[should_panic(expected = "expected sigil ,")]
fn type_hint_tensor_missing_comma() {
    parse(": tensor<i32 2> / @42");
}

/// Line 558: struct field missing colon between name and type
/// Struct field expects `name: type`
#[test]
#[should_panic(expected = "expected sigil :")]
fn type_hint_struct_field_missing_colon() {
    parse(": struct Foo { x i32 } / @42");
}

// =============================================================================
// Expression parsing: need_sigil panics
// =============================================================================

/// Line 609: type-hinted expression missing forward slash
/// `: type / expr` expects a `/` after the type
#[test]
#[should_panic(expected = "expected sigil /")]
fn expr_type_hint_missing_slash() {
    parse(": i32 @42");
}

/// Line 1065: map entry missing equals between key and value
/// `map { k = v }` expects `=`
#[test]
#[should_panic(expected = "expected sigil =")]
fn expr_map_entry_missing_equals() {
    parse("@map { @1 @2 }");
}

/// Line 1239: struct field missing equals between name and value
/// `struct Foo { x = v }` expects `=`
#[test]
#[should_panic(expected = "expected sigil =")]
fn expr_struct_field_missing_equals() {
    parse("@struct Foo { x @42 }");
}

// =============================================================================
// Expression parsing: need_name panics
// =============================================================================

/// Line 928: tuple expression without name
/// `tuple` expects a name like `tuple Foo(...)`
#[test]
#[should_panic(expected = "expected name")]
fn expr_tuple_missing_name() {
    parse("@tuple");
}

/// Line 961: struct expression without name
/// `struct` expects a name like `struct Foo{...}`
#[test]
#[should_panic(expected = "expected name")]
fn expr_struct_missing_name() {
    parse("@struct");
}

/// Line 998: enum expression without variant name
/// `enum` expects a variant name
#[test]
#[should_panic(expected = "expected name")]
fn expr_enum_missing_variant_name() {
    parse("@enum");
}

/// Line 1002: named enum expression missing variant name after dot
/// `enum Foo.` expects a variant name after the dot
#[test]
#[should_panic(expected = "expected name")]
fn expr_named_enum_missing_variant_after_dot() {
    parse("@enum Foo.");
}

/// Line 1238: struct field missing name
/// `struct Foo { = v }` expects a name before `=`
#[test]
#[should_panic(expected = "expected name")]
fn expr_struct_field_missing_name() {
    parse("@struct Foo { = @42 }");
}

// =============================================================================
// Float parsing edge cases
// Note: The float panics at lines 745 and 1129 are guarded by peek-ahead checks
// so they may not be easily triggerable with simple malformed input.
// These are left as documentation of potential panic points.
// =============================================================================
