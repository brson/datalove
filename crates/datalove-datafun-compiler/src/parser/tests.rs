//! Parser unit tests.

use rmx::prelude::*;
use bct::input::Source;
use crate::ast;
use crate::datalit;
use super::parse_for_test;

#[test]
fn test_parse_let_simple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @42"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            assert!(stmt.type_hint(db).is_none());
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_let_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x: @u32 = @42"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            assert!(stmt.type_hint(db).is_some());
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_fun_simple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("fun foo()\nend fun"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Fun(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "foo");
            assert_eq!(stmt.params(db).len(), 0);
            assert!(stmt.return_type(db).is_none());
            assert_eq!(stmt.body(db).len(), 0);
        }
        _ => panic!("expected fun statement"),
    }
}

#[test]
fn test_parse_fun_with_params() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("fun increment(accum: @u64, amount: @u8): !@u64\nend fun"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Fun(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "increment");
            assert_eq!(stmt.params(db).len(), 2);
            assert_eq!(stmt.params(db)[0].name(db).as_str(db), "accum");
            assert_eq!(stmt.params(db)[1].name(db).as_str(db), "amount");
            assert!(stmt.return_type(db).is_some());
        }
        _ => panic!("expected fun statement"),
    }
}

#[test]
fn test_parse_fun_with_list_param() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("fun identity(a: @[@u32]): @[@u32]\n  ret a\nend fun"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Fun(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "identity");
            assert_eq!(stmt.params(db).len(), 1);
            assert_eq!(stmt.params(db)[0].name(db).as_str(db), "a");
            // Check that return type is present.
            assert!(stmt.return_type(db).is_some());

            // Check if it's a ParseError.
            let param_type = stmt.params(db)[0].type_hint(db);
            match param_type.type_hint(db) {
                datalit::ast::TypeHint::ParseError(_) => {
                    panic!("Parameter type hint is a ParseError!");
                }
                datalit::ast::TypeHint::List(_) => {
                    // Good!
                }
                _ => panic!("Expected List type hint"),
            }
        }
        _ => panic!("expected fun statement"),
    }
}

#[test]
fn test_parse_fun_multiline_params() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("fun increment(\n  accum: @u64, amount: @u8,\n): !@u64\n  ret @0\nend fun"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Fun(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "increment");
            assert_eq!(stmt.params(db).len(), 2);
            assert_eq!(stmt.body(db).len(), 1);
            // Check the body has a ret statement.
            match &stmt.body(db)[0] {
                ast::Statement::Ret(_) => {}
                _ => panic!("expected ret statement in body"),
            }
        }
        _ => panic!("expected fun statement"),
    }
}

#[test]
fn test_parse_require() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("require module sys/std/bool"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Require(ast::StmtRequire::Module(stmt)) => {
            assert_eq!(stmt.import_space(db).as_str(db), "sys");
            assert_eq!(stmt.package_alias(db).as_str(db), "std");
            assert_eq!(stmt.module_alias(db).as_str(db), "bool");
        }
        _ => panic!("expected require module statement"),
    }
}

#[test]
fn test_parse_import() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("import u32.negate"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Import(stmt) => {
            assert_eq!(stmt.module_name(db).as_str(db), "u32");
            assert_eq!(stmt.item_name(db).as_str(db), "negate");
        }
        _ => panic!("expected import statement"),
    }
}

#[test]
fn test_parse_import_with_require() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("require module sys/std/u32\nimport u32.negate"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 2);
    match &statements[0] {
        ast::Statement::Require(ast::StmtRequire::Module(stmt)) => {
            assert_eq!(stmt.module_alias(db).as_str(db), "u32");
        }
        _ => panic!("expected require module statement"),
    }
    match &statements[1] {
        ast::Statement::Import(stmt) => {
            assert_eq!(stmt.module_name(db).as_str(db), "u32");
            assert_eq!(stmt.item_name(db).as_str(db), "negate");
        }
        _ => panic!("expected import statement"),
    }
}

#[test]
fn test_parse_expr_bare_name() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = accum"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::Name(name) => {
                    assert_eq!(name.as_str(db), "accum");
                }
                _ => panic!("expected name expression"),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_expr_datalit() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @42"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::Int(int_expr) => {
                    // Successfully parsed as inline int.
                    assert_eq!(int_expr.value(db).as_str(db), "42");
                }
                other => panic!("expected Int expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_expr_binop_checked() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = a +! b"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::BinOp(binop) => {
                    assert_eq!(binop.op(db), ast::BinOp::AddChecked);
                }
                _ => panic!("expected binop expression"),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_expr_binop_optional() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = a +? b"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::BinOp(binop) => {
                    assert_eq!(binop.op(db), ast::BinOp::AddOptional);
                }
                _ => panic!("expected binop expression"),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_expr_binop_basic() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = a + b"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::BinOp(binop) => {
                    assert_eq!(binop.op(db), ast::BinOp::Add);
                }
                _ => panic!("expected binop expression"),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_expr_binop_comparison() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = a .< b"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::BinOp(binop) => {
                    assert_eq!(binop.op(db), ast::BinOp::Lt);
                }
                _ => panic!("expected binop expression"),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_expr_binop_precedence() {
    // Test that multiplication has higher precedence than addition.
    // "a + b * c" should parse as "a + (b * c)".
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = a + b * c"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::BinOp(binop) => {
                    // Top level should be addition.
                    assert_eq!(binop.op(db), ast::BinOp::Add);
                    // RHS should be multiplication.
                    match binop.rhs(db).expr(db) {
                        ast::ExprFunKind::BinOp(rhs_binop) => {
                            assert_eq!(rhs_binop.op(db), ast::BinOp::Mul);
                        }
                        _ => panic!("expected binop for rhs"),
                    }
                }
                _ => panic!("expected binop expression"),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_fun_with_binop_in_ret() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("fun increment(accum: @u64, amount: @u8): !@u64\n  ret accum +! amount\nend fun"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Fun(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "increment");
            assert_eq!(stmt.body(db).len(), 1);
            // Check the body has a ret statement with binop.
            match &stmt.body(db)[0] {
                ast::Statement::Ret(ret) => {
                    let value = ret.value(db).expect("expected ret with value");
                    match value.expr(db) {
                        ast::ExprFunKind::BinOp(binop) => {
                            assert_eq!(binop.op(db), ast::BinOp::AddChecked);
                        }
                        _ => panic!("expected binop in ret"),
                    }
                }
                _ => panic!("expected ret statement in body"),
            }
        }
        _ => panic!("expected fun statement"),
    }
}

#[test]
fn test_parse_multiple_statements_with_semicolon() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @1; let y = @2"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 2);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
        }
        _ => panic!("expected let statement"),
    }
    match &statements[1] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "y");
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_semicolon_with_newline_mix() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @1; let y = @2\nlet z = @3"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 3);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
        }
        _ => panic!("expected let statement"),
    }
    match &statements[1] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "y");
        }
        _ => panic!("expected let statement"),
    }
    match &statements[2] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "z");
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_require_with_semicolon() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("require module sys/std/bool; let x = @42"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 2);
    match &statements[0] {
        ast::Statement::Require(ast::StmtRequire::Module(stmt)) => {
            assert_eq!(stmt.import_space(db).as_str(db), "sys");
            assert_eq!(stmt.package_alias(db).as_str(db), "std");
            assert_eq!(stmt.module_alias(db).as_str(db), "bool");
        }
        _ => panic!("expected require module statement"),
    }
    match &statements[1] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
        }
        _ => panic!("expected let statement"),
    }
}

// Tests for complex datalit expressions enabled by direct token parsing.

#[test]
fn test_parse_datalit_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @(1, 2, 3)"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::AnonTuple(tuple) => {
                    assert_eq!(tuple.elements(db).len(), 3);
                }
                other => panic!("expected AnonTuple expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_datalit_list() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @[1, 2, 3]"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::List(list) => {
                    assert_eq!(list.elements(db).len(), 3);
                }
                other => panic!("expected List expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_datalit_map() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @map { @1 = @10, @2 = @20 }"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::Map(map) => {
                    assert_eq!(map.entries(db).len(), 2);
                }
                other => panic!("expected Map expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_datalit_nested_tuple_in_list() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @[(1, 2), (3, 4)]"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::List(list) => {
                    // List with nested tuples.
                    assert_eq!(list.elements(db).len(), 2);
                }
                other => panic!("expected List expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_datalit_nested_list_in_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @(@[@1, @2, @3], @100)"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::AnonTuple(tuple) => {
                    // Tuple with nested list.
                    assert_eq!(tuple.elements(db).len(), 2);
                }
                other => panic!("expected AnonTuple expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_datalit_set() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @set { @1, @2, @3 }"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::Set(set) => {
                    assert_eq!(set.elements(db).len(), 3);
                }
                other => panic!("expected Set expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}

#[test]
fn test_parse_datalit_deeply_nested() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
    let script = parse_for_test(db, source);
    let statements = script.statements(db);
    assert_eq!(statements.len(), 1);
    match &statements[0] {
        ast::Statement::Let(stmt) => {
            assert_eq!(stmt.name(db).as_str(db), "x");
            match stmt.value(db).expr(db) {
                ast::ExprFunKind::AnonTuple(tuple) => {
                    // Deeply nested tuple.
                    assert_eq!(tuple.elements(db).len(), 2);
                }
                other => panic!("expected AnonTuple expression, got {:?}", std::mem::discriminant(&other)),
            }
        }
        _ => panic!("expected let statement"),
    }
}
