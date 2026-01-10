//! Parser unit tests.

use rmx::prelude::*;
use bct::input::Source;
use crate::ast;
use super::parse_for_test;

#[test]
fn test_parse_bool() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@true"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_bool_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @bool / @true"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    assert!(matches!(type_hint, ast::TypeHint::Bool));
    let expr = ast.expr(db).expr(db);
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_int() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@42"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Int(e) => assert_eq!(e.value(db).as_str(db), "42"),
        _ => panic!("expected int"),
    }
}

#[test]
fn test_parse_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@(@true, @1)"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements(db).len(), 2),
        _ => panic!("expected tuple"),
    }
}

#[test]
fn test_parse_list() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@[@1, @2, @3]"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements(db).len(), 3),
        _ => panic!("expected list"),
    }
}

#[test]
fn test_parse_float() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@1.0"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Float(e) => assert_eq!(e.value(db).as_str(db), "1.0"),
        ast::Expr::Int(_) => panic!("expected float, got Int"),
        _ => panic!("expected float, got something else"),
    }
}

#[test]
fn test_parse_float_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @f32 / @1.0"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    assert!(matches!(type_hint, ast::TypeHint::F32));
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Float(e) => assert_eq!(e.value(db).as_str(db), "1.0"),
        _ => panic!("expected float"),
    }
}

#[test]
fn test_parse_anon_enum_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @enum { Foo, Bar(@u32) } / @enum Foo"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::AnonEnum(e) => {
            let variants = &e.variants;
            assert_eq!(variants.len(), 2);
        }
        _ => panic!("expected anonymous enum type hint"),
    }
}

#[test]
fn test_parse_string() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(r#": @string / @"hello world""#));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::String(s) => {
            assert_eq!(s.value(db).as_str(db), r#""hello world""#);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn test_parse_map() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @map <@u32, @u32> / @map { @0 = @5, @2 = @2 }"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::Map(_) => {}
        _ => panic!("expected map type hint"),
    }
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Map(m) => {
            assert_eq!(m.entries(db).len(), 2);
        }
        _ => panic!("expected map expr"),
    }
}

#[test]
fn test_parse_set() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @set <@u32> / @set { @1, @2, @3 }"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::Set(_) => {}
        _ => panic!("expected set type hint"),
    }
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Set(s) => {
            assert_eq!(s.elements(db).len(), 3);
        }
        _ => panic!("expected set expr"),
    }
}

#[test]
fn test_parse_enum_variant_no_payload() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Foo"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Foo");
            assert!(e.payload(db).is_none());
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_enum_variant_with_payload() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Bar(@2)"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Bar");
            assert!(e.payload(db).is_some());
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_enum_variant_with_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Baz(@(@true, @1))"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Baz");
            assert!(e.payload(db).is_some());
            // Verify the payload is a tuple.
            let payload = e.payload(db).unwrap();
            match payload.expr(db).expr(db) {
                ast::Expr::AnonTuple(t) => assert_eq!(t.elements(db).len(), 2),
                _ => panic!("expected tuple payload"),
            }
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_enum_variant_with_extra_tokens_error() {
    let ref db = crate::Database::default();
    // This should error: Ok(@u32, @string) - multiple types without explicit tuple.
    let source = Source::new(db, S(": @enum { Ok(@u32, @string) } / @enum Ok(@1)"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::AnonEnum(e) => {
            let variants = &e.variants;
            assert_eq!(variants.len(), 1);
            let variant = &variants[0];
            assert_eq!(variant.name.as_str(db), "Ok");
            // Check that the payload contains a parse error.
            match variant.payload {
                Some(payload_type) => {
                    match payload_type.type_hint(db) {
                        ast::TypeHint::ParseError(_) => {
                            // Expected! This is the parse error for extra tokens.
                        }
                        _ => panic!("expected parse error for extra tokens in enum variant payload"),
                    }
                }
                None => panic!("expected payload with parse error"),
            }
        }
        _ => panic!("expected anonymous enum type hint"),
    }
}

#[test]
fn test_parse_list_multiline() {
    // Datalit parser doesn't split on newlines, but newlines in whitespace are fine.
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@[\n@1,\n@2,\n@3\n]"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements(db).len(), 3),
        _ => panic!("expected list"),
    }
}

#[test]
fn test_parse_tuple_multiline() {
    // Datalit parser doesn't split on newlines, but newlines in whitespace are fine.
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@(\n@true,\n@1\n)"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements(db).len(), 2),
        _ => panic!("expected tuple"),
    }
}
