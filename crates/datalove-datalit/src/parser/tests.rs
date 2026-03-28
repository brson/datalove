//! Parser unit tests.

use rmx::prelude::*;
use bct::input::Source;
use crate::ast;
use super::parse_for_test;

#[test]
fn test_parse_bool() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("true"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_bool_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": bool / true"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap();
    assert!(matches!(type_hint, ast::TypeHint::Bool));
    let expr = ast.expr(db).clone();
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_int() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("42"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::Int(e) => assert_eq!(e.value.as_str(db), "42"),
        _ => panic!("expected int"),
    }
}

#[test]
fn test_parse_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("(true, 1)"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements.len(), 2),
        _ => panic!("expected tuple"),
    }
}

#[test]
fn test_parse_list() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("[1, 2, 3]"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements.len(), 3),
        _ => panic!("expected list"),
    }
}

#[test]
fn test_parse_float() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("1.0"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::Float(e) => assert_eq!(e.value.as_str(db), "1.0"),
        ast::Expr::Int(_) => panic!("expected float, got Int"),
        _ => panic!("expected float, got something else"),
    }
}

#[test]
fn test_parse_float_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": f32 / 1.0"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap();
    assert!(matches!(type_hint, ast::TypeHint::F32));
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::Float(e) => assert_eq!(e.value.as_str(db), "1.0"),
        _ => panic!("expected float"),
    }
}


#[test]
fn test_parse_string() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(r#": string / "hello world""#));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::String(s) => {
            assert_eq!(s.value.as_str(db), r#""hello world""#);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn test_parse_map() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": ⦇u32 ↦ u32⦈ / ⦇ 0 ↦ 5, 2 ↦ 2 ⦈"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap();
    match type_hint {
        ast::TypeHint::Map(_) => {}
        _ => panic!("expected map type hint"),
    }
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::Map(m) => {
            assert_eq!(m.entries.len(), 2);
        }
        _ => panic!("expected map expr"),
    }
}

#[test]
fn test_parse_set() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": ⦃u32⦄ / ⦃ 1, 2, 3 ⦄"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap();
    match type_hint {
        ast::TypeHint::Set(_) => {}
        _ => panic!("expected set type hint"),
    }
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::Set(s) => {
            assert_eq!(s.elements.len(), 3);
        }
        _ => panic!("expected set expr"),
    }
}





#[test]
fn test_parse_list_multiline() {
    // Datalit parser doesn't split on newlines, but newlines in whitespace are fine.
    let ref db = crate::Database::default();
    let source = Source::new(db, S("[\n1,\n2,\n3\n]"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements.len(), 3),
        _ => panic!("expected list"),
    }
}

#[test]
fn test_parse_tuple_multiline() {
    // Datalit parser doesn't split on newlines, but newlines in whitespace are fine.
    let ref db = crate::Database::default();
    let source = Source::new(db, S("(\ntrue,\n1\n)"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).clone();
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements.len(), 2),
        _ => panic!("expected tuple"),
    }
}
