//! Tests for equivalence between datalit and datafun parsing and typechecking.
//!
//! These tests verify that datafun's inline literal expressions produce the same
//! AST structure and typecheck results as datalit for pure datalit expressions,
//! one hand-written expression of each kind. Generated ones, and broken ones,
//! are `literal_equiv_tests`'.

use rmx::prelude::*;

use datalove_datafun_compiler::funlit_equiv::{
    datafun_expr_to_datalit_serde,
    datalit_typecheck_to_serde,
    datafun_unit_typecheck_to_serde,
};

/// Extract expression from a let statement in a ParsedStatements.
fn extract_let_value<'db>(
    db: &'db datalove_datafun_compiler::Database,
    parsed: datalove_datafun_ast::ast::ParsedStatements<'db>,
) -> Result<datalove_datafun_ast::ast::ExprFun<'db>, String> {
    let statements = parsed.statements(db);
    if statements.len() != 1 {
        return Err(format!("expected 1 statement, got {}", statements.len()));
    }
    match &statements[0] {
        datalove_datafun_ast::ast::Statement::Let(stmt) => Ok(parsed.body(db).arena.get(stmt.value)),
        other => Err(format!("expected Let statement, got {:?}", std::mem::discriminant(other))),
    }
}

/// Test that a single expression parses equivalently in both parsers.
fn test_parse_equiv(db: &datalove_datafun_compiler::Database, expr_text: &str) -> Result<(), String> {
    // Parse with datalit.
    let datalit_source = bct::input::Source::new(db, expr_text.S());
    let datalit_parsed = datalove_datalit::parser::parse_integration_test(db, datalit_source);
    let datalit_serde = datalove_datalit::ast_serde::ExprFull::from_ast(db, datalit_parsed);

    // Parse with datafun (wrap in "let _x = " prefix).
    let datafun_text = format!("let _x = {}", expr_text);
    let datafun_source = bct::input::Source::new(db, datafun_text.C());
    let datafun_parsed = datalove_datafun_parser::parse_integration_test(db, datafun_source);

    // Extract expression from let statement.
    let datafun_expr = extract_let_value(db, datafun_parsed.C())?;

    // Convert to datalit serde.
    let datafun_serde = datafun_expr_to_datalit_serde(db, datafun_expr)
        .map_err(|e| format!("Datafun to datalit conversion failed: {}", e))?;

    // Compare.
    if datalit_serde != datafun_serde {
        return Err(format!(
            "AST mismatch for '{}'\nDatalit: {:?}\nDatafun: {:?}",
            expr_text, datalit_serde, datafun_serde
        ));
    }

    Ok(())
}

/// Test that a single expression typechecks equivalently in both typecheckers.
fn test_typecheck_equiv(db: &datalove_datafun_compiler::Database, expr_text: &str) -> Result<(), String> {
    // First verify parsing works.
    test_parse_equiv(db, expr_text)?;

    // Parse and typecheck with datalit.
    let datalit_source = bct::input::Source::new(db, expr_text.S());
    let datalit_parsed = datalove_datalit::parser::parse_integration_test(db, datalit_source);
    let datalit_resolved = datalove_datalit::resolve::resolve_names(db, datalit_source, datalit_parsed.C());
    let datalit_result = datalove_datalit::tycheck::type_check(db, datalit_parsed, datalit_resolved);
    let datalit_serde = datalit_typecheck_to_serde(db, datalit_result);

    // Parse and typecheck with datafun (wrap in "let _x = " prefix) using production path.
    let datafun_text = format!("let _x = {}", expr_text);
    let datafun_source = bct::input::Source::new(db, datafun_text.C());
    let datafun_script = datalove_datafun_parser::parse_integration_test(db, datafun_source);
    let datafun_result = datalove_datafun_tycheck::type_check_single_script(db, datafun_source);

    // Extract expression for type lookup.
    let datafun_expr = extract_let_value(db, datafun_script.C())?;

    let datafun_serde = datafun_unit_typecheck_to_serde(db, datafun_result, datafun_expr);

    // Compare.
    if datalit_serde != datafun_serde {
        return Err(format!(
            "Typecheck mismatch for '{}'\nDatalit: {:?}\nDatafun: {:?}",
            expr_text, datalit_serde, datafun_serde
        ));
    }

    Ok(())
}

// ============================================================================
// Manual tests for specific expressions
// ============================================================================

#[test]
fn test_simple_bool_true() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "true").X();
    test_typecheck_equiv(&db, "true").X();
}

#[test]
fn test_simple_bool_false() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "false").X();
    test_typecheck_equiv(&db, "false").X();
}

#[test]
fn test_simple_int() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "42").X();
    test_typecheck_equiv(&db, "42").X();
}

#[test]
fn test_simple_string() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, r#""hello""#).X();
    test_typecheck_equiv(&db, r#""hello""#).X();
}

#[test]
fn test_simple_list() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "[1, 2, 3]").X();
    test_typecheck_equiv(&db, "[1, 2, 3]").X();
}

#[test]
fn test_nested_list() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "[[1, 2], [3, 4]]").X();
    test_typecheck_equiv(&db, "[[1, 2], [3, 4]]").X();
}

#[test]
fn test_anon_tuple() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "(1, 2, 3)").X();
    test_typecheck_equiv(&db, "(1, 2, 3)").X();
}

#[test]
fn test_anon_struct() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "{x = 1, y = 2}").X();
    test_typecheck_equiv(&db, "{x = 1, y = 2}").X();
}

// Note: Skipping map test - {1: 10} syntax not supported (datalit uses struct syntax)
// Note: Skipping set test - {1, 2, 3} syntax not supported (datalit uses `set {...}`)
// Note: Skipping option_none test - `: type / value` syntax not supported by datafun parser

#[test]
fn test_1_tuple() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "(42,)").X();
    test_typecheck_equiv(&db, "(42,)").X();
}

#[test]
fn test_parens_grouping() {
    let db = datalove_datafun_compiler::Database::default();
    // (42) is grouping, not a tuple - both parsers should produce the same result.
    test_parse_equiv(&db, "(42)").X();
    test_typecheck_equiv(&db, "(42)").X();
}

// ============================================================================
// Keyword payload tests (primary expression parsing)
// ============================================================================

#[test]
fn test_some_integer() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "some 42").X();
    test_typecheck_equiv(&db, "some 42").X();
}

#[test]
fn test_some_tuple() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "some(1, 2)").X();
    test_typecheck_equiv(&db, "some(1, 2)").X();
}

#[test]
fn test_some_list() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "some [1, 2]").X();
    test_typecheck_equiv(&db, "some [1, 2]").X();
}

#[test]
fn test_ok_integer() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "ok 42").X();
    test_typecheck_equiv(&db, "ok 42").X();
}

#[test]
fn test_ok_string() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, r#"ok "hello""#).X();
    test_typecheck_equiv(&db, r#"ok "hello""#).X();
}

#[test]
fn test_er_primary() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "er 42").X();
    test_typecheck_equiv(&db, "er 42").X();
}

#[test]
fn test_er_error() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, r#"er error "msg""#).X();
    test_typecheck_equiv(&db, r#"er error "msg""#).X();
}

#[test]
fn test_data_integer() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "data 42").X();
    test_typecheck_equiv(&db, "data 42").X();
}

#[test]
fn test_data_string() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, r#"data "hello""#).X();
    test_typecheck_equiv(&db, r#"data "hello""#).X();
}

#[test]
fn test_data_list() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "data [1, 2]").X();
    test_typecheck_equiv(&db, "data [1, 2]").X();
}

#[test]
fn test_error_string() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, r#"error "msg""#).X();
    test_typecheck_equiv(&db, r#"error "msg""#).X();
}

#[test]
fn test_none() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "none").X();
    test_typecheck_equiv(&db, "none").X();
}
