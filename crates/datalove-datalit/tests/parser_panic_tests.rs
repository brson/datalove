//! Tests that verify the parser returns ParseError nodes instead of panicking.
//!
//! These tests exercise error cases in the datalit parser.
//! The parser should return ParseError nodes for all malformed input.

use rmx::prelude::*;

fn parse(source: &str) -> String {
    let db = datalove_datalit::Database::default();
    let src = bct::input::Source::new(&db, source.S());
    let ast = datalove_datalit::parser::parse_integration_test(&db, src);
    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);
    rmx::serde_json::to_string_pretty(&serde_ast).X()
}

fn assert_parse_error(json: &str, context: &str) {
    // Check for ParseError nodes or <error> placeholder names.
    assert!(
        json.contains("ParseError") || json.contains(r#""<error>""#),
        "expected parse error node in {}, got: {}",
        context,
        json
    );
}

// =============================================================================
// Type hint parsing: error cases
// =============================================================================

/// `tuple` keyword without name or parens.
#[test]
fn type_hint_tuple_missing_name() {
    let json = parse(": tuple / @42");
    assert_parse_error(&json, "tuple without name");
}

/// `enum` keyword without name or braces.
#[test]
fn type_hint_named_enum_missing_name() {
    let json = parse(": enum / @42");
    assert_parse_error(&json, "enum without name");
}

/// Struct field without name in non-empty braces.
/// Struct field expects `name: type`.
#[test]
fn type_hint_struct_field_missing_name() {
    let json = parse(": { : i32 } / @42");
    assert_parse_error(&json, "struct field without name");
}

/// Enum variant without name in non-empty braces.
/// Enum variant expects a name.
#[test]
fn type_hint_enum_variant_missing_name() {
    let json = parse(": enum { (i32) } / @42");
    // This should produce an error for the missing variant name.
    assert_parse_error(&json, "enum variant without name");
}

/// Map type hint missing comma between key and value type.
/// `map<K, V>` expects a comma.
#[test]
fn type_hint_map_missing_comma() {
    let json = parse(": map<i32 i32> / @42");
    assert_parse_error(&json, "map missing comma");
}

/// Tensor type hint missing comma between element type and rank.
/// `[|T, N|]` expects a comma.
#[test]
fn type_hint_tensor_missing_comma() {
    let json = parse(": [|i32 2|] / 42");
    assert_parse_error(&json, "tensor missing comma");
}

/// Struct field missing colon between name and type.
/// Struct field expects `name: type`.
#[test]
fn type_hint_struct_field_missing_colon() {
    let json = parse(": { x i32 } / @42");
    assert_parse_error(&json, "struct field missing colon");
}

// =============================================================================
// Expression parsing: error cases
// =============================================================================

/// Type-hinted expression missing forward slash.
/// `: type / expr` expects a `/` after the type.
#[test]
fn expr_type_hint_missing_slash() {
    let json = parse(": i32 @42");
    assert_parse_error(&json, "type hint missing slash");
}

/// Map entry missing equals between key and value.
/// `map { k = v }` expects `=`.
#[test]
fn expr_map_entry_missing_equals() {
    let json = parse("map { @1 @2 }");
    assert_parse_error(&json, "map entry missing equals");
}

/// Struct field missing equals between name and value.
/// Struct field expects `name = value`.
#[test]
fn expr_struct_field_missing_equals() {
    let json = parse("{ x @42 }");
    assert_parse_error(&json, "struct field missing equals");
}

/// Tuple expression without name.
/// `tuple` expects a name like `tuple Foo(...)`.
#[test]
fn expr_tuple_missing_name() {
    let json = parse("@tuple");
    assert_parse_error(&json, "tuple without name");
}

/// Enum expression without variant name.
/// `enum` expects a variant name.
#[test]
fn expr_enum_missing_variant_name() {
    let json = parse("enum");
    assert_parse_error(&json, "enum without variant name");
}

/// Struct field missing name.
/// Struct field expects a name before `=`.
#[test]
fn expr_struct_field_missing_name() {
    let json = parse("{ = @42 }");
    assert_parse_error(&json, "struct field missing name");
}
