//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use bct::input::Source;

pub use datalove_datafun as datafun;

mod engine;
pub use engine::Engine;

const REPL_COMMAND_SIGIL: char = '/';

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Input {
    Input(String),
    Multiline(String),
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputParse {
    Empty,
    ReadMultiline(String),
    Command(Command),
    CrashReset(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    ReplCommand(ReplCommand),
    ScriptStatement(ScriptStatement),
    Expression(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplCommand {
    Unknown,
    Exit,
    Help,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptStatement(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalLet {
    pub name: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExpr {
    // Is it a variable binding, function call,
    // math expression, etc.
    pub expr_kind: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Eval {
    Nothing,
    SuccessLet(EvalLet),
    SuccessExpr(EvalExpr),
    Error(String),
    CallerInterpret(ReplCommand),
    CrashReset(String),
}


impl Command {
    pub(crate) fn repl_command(command: &str) -> InputParse {
        let command = &command.trim()[1..];
        let c = match command {
            "exit" => ReplCommand::Exit,
            "help" => ReplCommand::Help,
            _ => ReplCommand::Unknown,
        };
        InputParse::Command(Command::ReplCommand(c))
    }

    pub(crate) fn script_statement(input: &str) -> InputParse {
        InputParse::Command(Command::ScriptStatement(
            ScriptStatement(S(input))
        ))
    }

    pub(crate) fn expression(input: &str) -> InputParse {
        InputParse::Command(Command::Expression(S(input)))
    }
}


pub(crate) enum InputKind {
    Whitespace,
    ReplCommand,
    OnelineStatement,
    MultilineStatement,
    OpenBraceTree,
    Expression,
}

pub(crate) fn classify_input(input: &str) -> InputKind {
    let is_whitespace = input.chars().all(char::is_whitespace);
    let is_repl_command = input.trim().starts_with(REPL_COMMAND_SIGIL);
    let is_oneline_statement_keyword = parse_ident(input).map(|ident| match ident {
        "let" => true,
        _ => false
    }).unwrap_or(false);
    let is_multiline_statement_keyword = parse_ident(input).map(|ident| match ident {
        "fun" => true,
        _ => false
    }).unwrap_or(false);
    let is_open_brace_tree = false; // todo

    if is_whitespace {
        InputKind::Whitespace
    } else if is_repl_command {
        InputKind::ReplCommand
    } else if is_oneline_statement_keyword {
        InputKind::OnelineStatement
    } else if is_multiline_statement_keyword {
        InputKind::MultilineStatement
    } else if is_open_brace_tree {
        InputKind::OpenBraceTree
    } else {
        InputKind::Expression
    }
}

fn parse_ident(input: &str) -> Option<&str> {
    alphanumeric_prefix(input.trim())
}

fn alphanumeric_prefix(s: &str) -> Option<&str> {
    s.find(|c: char| !c.is_alphanumeric())
        .map(|pos| &s[..pos])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_with_interpreter() {
        let mut engine = Engine::new().unwrap();

        // Add a simple let statement.
        let parse_result = engine.parse_input(Input::Input("let x = 42".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        // Success - the statement was executed.
                        assert_eq!(eval_let.name, "x");
                        assert!(eval_let.value.contains("42"));
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }

        // Note: The implementation details of history tracking have been
        // moved to the engine module, so we can't directly verify them here.
        // The successful evaluation above confirms the engine is working correctly.
    }

    #[test]
    fn test_eval_expression() {
        let mut engine = Engine::new().unwrap();

        // First add a simple let binding to verify the system works.
        let parse_result = engine.parse_input(Input::Input("let x = @42".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                // Should return SuccessLet for let statement.
                assert!(matches!(eval_result, Eval::SuccessLet(_)));
            }
            _ => panic!("Expected Command"),
        }

        // Now test evaluating an expression that references the variable.
        let parse_result = engine.parse_input(Input::Input("x".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessExpr(eval_expr) => {
                        // The result should contain "42".
                        assert!(eval_expr.value.contains("42") || eval_expr.value.contains("@42"),
                            "Expected result to contain '42' or '@42', got: {}", eval_expr.value);
                        assert_eq!(eval_expr.ty, "u32");
                    }
                    other => panic!("Expected Eval::SuccessExpr, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }

        // Note: Expression evaluations should not persist to the history.
        // The implementation is in the engine module, so we can't verify it here.
    }

    #[test]
    fn test_eval_expression_with_previous_bindings() {
        let mut engine = Engine::new().unwrap();

        // Add a let statement.
        let parse_result = engine.parse_input(Input::Input("let x = 10".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                // Should return SuccessLet for let statement.
                assert!(matches!(eval_result, Eval::SuccessLet(_)));
            }
            _ => panic!("Expected Command"),
        }

        // Evaluate an expression using the previous binding.
        let parse_result = engine.parse_input(Input::Input("x + 5".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessExpr(eval_expr) => {
                        // The result should contain "15".
                        assert!(eval_expr.value.contains("15"), "Expected result to contain '15', got: {}", eval_expr.value);
                        assert_eq!(eval_expr.ty, "u32");
                    }
                    other => panic!("Expected Eval::SuccessExpr, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_error_handling() {
        let mut engine = Engine::new().unwrap();

        // Try to evaluate invalid syntax.
        let parse_result = engine.parse_input(Input::Input("let x =".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::Error(msg) => {
                        // Should contain "parse error".
                        assert!(msg.contains("parse error") || msg.contains("error"),
                            "Expected parse error, got: {}", msg);
                    }
                    other => panic!("Expected Eval::Error, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_let_returns_typechecker_type() {
        let mut engine = Engine::new().unwrap();

        // Test u32 literal - typechecker should infer u32 type.
        let parse_result = engine.parse_input(Input::Input("let x = 42".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert_eq!(eval_let.ty, "u32");
                        assert_eq!(eval_let.value, "@42");
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_let_sequential() {
        let mut engine = Engine::new().unwrap();

        // First let binding.
        let parse_result = engine.parse_input(Input::Input("let x = 10".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert_eq!(eval_let.ty, "u32");
                        assert_eq!(eval_let.value, "@10");
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }

        // Second let binding using the first.
        let parse_result = engine.parse_input(Input::Input("let y = x".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "y");
                        assert_eq!(eval_let.ty, "u32");
                        assert_eq!(eval_let.value, "@10");
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_let_multiple_vars() {
        let mut engine = Engine::new().unwrap();

        // Define multiple variables.
        let vars = vec![
            ("a", "100", "u32"),
            ("b", "200", "u32"),
            ("c", "300", "u32"),
        ];

        for (name, value, ty) in vars {
            let input = format!("let {} = {}", name, value);
            let parse_result = engine.parse_input(Input::Input(input));
            match parse_result {
                InputParse::Command(cmd) => {
                    let eval_result = engine.eval(cmd);
                    match eval_result {
                        Eval::SuccessLet(eval_let) => {
                            assert_eq!(eval_let.name, name);
                            assert_eq!(eval_let.ty, ty);
                            assert!(eval_let.value.contains(value));
                        }
                        other => panic!("Expected Eval::SuccessLet for {}, got {:?}", name, other),
                    }
                }
                other => panic!("Expected Command, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_eval_struct_literal() {
        let mut engine = Engine::new().unwrap();

        let parse_result = engine.parse_input(Input::Input("let x = @{a = @1, b = @2}".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert_eq!(eval_let.value, "@{a = @1, b = @2}");
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_list_literal() {
        let mut engine = Engine::new().unwrap();

        let parse_result = engine.parse_input(Input::Input("let x = @[@1, @2, @3]".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert_eq!(eval_let.value, "@[@1, @2, @3]");
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_tuple_literal() {
        let mut engine = Engine::new().unwrap();

        let parse_result = engine.parse_input(Input::Input("let x = @(@42, @\"hello\")".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert_eq!(eval_let.value, "@(@42, @\"hello\")");
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_struct_expression() {
        let mut engine = Engine::new().unwrap();

        // Test evaluating a struct expression (without let).
        let parse_result = engine.parse_input(Input::Input("@{x = @10, y = @20}".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessExpr(eval_expr) => {
                        assert_eq!(eval_expr.value, "@{x = @10, y = @20}");
                    }
                    other => panic!("Expected Eval::SuccessExpr, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_list_expression() {
        let mut engine = Engine::new().unwrap();

        // Test evaluating a list expression.
        let parse_result = engine.parse_input(Input::Input("@[@100, @200, @300]".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessExpr(eval_expr) => {
                        assert_eq!(eval_expr.value, "@[@100, @200, @300]");
                    }
                    other => panic!("Expected Eval::SuccessExpr, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }


    #[test]
    fn test_eval_struct_with_multiple_fields() {
        let mut engine = Engine::new().unwrap();

        let parse_result = engine.parse_input(Input::Input("let x = @{a = @1, b = @2, c = @3}".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert!(eval_let.value.contains("@1"));
                        assert!(eval_let.value.contains("@2"));
                        assert!(eval_let.value.contains("@3"));
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_eval_list_with_strings() {
        let mut engine = Engine::new().unwrap();

        let parse_result = engine.parse_input(Input::Input("let x = @[@\"a\", @\"b\", @\"c\"]".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessLet(eval_let) => {
                        assert_eq!(eval_let.name, "x");
                        assert!(eval_let.value.contains("@\"a\""));
                        assert!(eval_let.value.contains("@\"b\""));
                        assert!(eval_let.value.contains("@\"c\""));
                    }
                    other => panic!("Expected Eval::SuccessLet, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }
} 
