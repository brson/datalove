//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use bct::input::Source;

use crate::{Command, ReplCommand, Eval, EvalLet, EvalExpr, InputParse, Input};
use crate::datafun;

pub struct Engine {
    db: datafun::Database,
    history: ReplHistory,
}

struct ReplHistory {
    entries: Vec<HistoryEntry>,
}


struct HistoryEntry {
    command: Command,
    last_eval: Eval,
    /// Not all commands produce script units.
    script_status: Option<ScriptUnitStatus>,
}

struct ScriptUnitStatus {
    script_unit: datafun::script::ScriptUnit,
    /// Whether the script unit is scheduled for evaluation.
    ///
    /// This is automatically turned off for new script units
    /// if type checking fails.
    active: bool,
}

impl ReplHistory {
    fn new() -> Self {
        ReplHistory {
            entries: Vec::new(),
        }
    }

    fn build_script(&self, db: &dyn datafun::Db) -> datafun::script::Script {
        let active_units: Vec<_> = self.entries.iter()
            .filter_map(|entry| entry.script_status.as_ref())
            .filter(|status| status.active)
            .map(|status| status.script_unit)
            .collect();

        datafun::script::Script::new(db, active_units)
    }

    fn build_script_with_unit(&self, db: &dyn datafun::Db, unit: datafun::script::ScriptUnit) -> datafun::script::Script {
        let mut units: Vec<_> = self.entries.iter()
            .filter_map(|entry| entry.script_status.as_ref())
            .filter(|status| status.active)
            .map(|status| status.script_unit)
            .collect();
        units.push(unit);

        datafun::script::Script::new(db, units)
    }

    fn add_script_entry(
        &mut self,
        command: Command,
        eval: Eval,
        script_unit: datafun::script::ScriptUnit,
        active: bool,
    ) {
        self.entries.push(HistoryEntry {
            command,
            last_eval: eval,
            script_status: Some(ScriptUnitStatus {
                script_unit,
                active,
            }),
        });
    }

    fn add_non_script_entry(&mut self, command: Command, eval: Eval) {
        self.entries.push(HistoryEntry {
            command,
            last_eval: eval,
            script_status: None,
        });
    }

    fn deactivate_last(&mut self) {
        let mut last = self.entries.last_mut().X();
        let mut script_unit = last.script_status.as_mut().X();
        script_unit.active = false;
    }
}

impl Engine {
    pub fn new() -> AnyResult<Engine> {
        Ok(Engine {
            db: datafun::Database::default(),
            history: ReplHistory::new(),
        })
    }

    fn reset(&mut self) {
        self.db = datafun::Database::default();
        self.history = ReplHistory::new();
    }

    pub fn parse_input(&mut self, input: Input) -> InputParse {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.parse_input_impl(input.clone())
        }));

        match result {
            Ok(parse_result) => parse_result,
            Err(panic_info) => {
                self.reset();
                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                InputParse::CrashReset(format!("Parse panic: {}", panic_msg))
            }
        }
    }

    fn parse_input_impl(&mut self, input: Input) -> InputParse {
        match input {
            Input::Input(s) => self.parse_input_oneline(&s),
            Input::Multiline(s) => self.parse_input_multiline(&s),
        }
    }

    fn parse_input_oneline(&mut self, input: &str) -> InputParse {
        match crate::classify_input(input) {
            crate::InputKind::Whitespace => InputParse::Empty,
            crate::InputKind::ReplCommand => Command::repl_command(input),
            crate::InputKind::OnelineStatement => Command::script_statement(input),
            crate::InputKind::MultilineStatement => InputParse::ReadMultiline(S(input)),
            crate::InputKind::OpenBraceTree => InputParse::ReadMultiline(S(input)),
            crate::InputKind::Expression => Command::expression(input),
        }
    }

    fn parse_input_multiline(&mut self, input: &str) -> InputParse {
        match crate::classify_input(input) {
            crate::InputKind::Whitespace => InputParse::Empty,
            crate::InputKind::ReplCommand => Command::repl_command(input),
            crate::InputKind::OnelineStatement => Command::script_statement(input),
            crate::InputKind::MultilineStatement => Command::script_statement(input),
            crate::InputKind::OpenBraceTree => todo!(),
            crate::InputKind::Expression => Command::expression(input),
        }
    }

    pub fn eval(&mut self, command: Command) -> Eval {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.eval_impl(command.clone())
        }));

        match result {
            Ok(eval_result) => eval_result,
            Err(panic_info) => {
                self.reset();
                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                Eval::CrashReset(format!("Eval panic: {}", panic_msg))
            }
        }
    }

    fn eval_impl(&mut self, command: Command) -> Eval {
        match command {
            Command::ReplCommand(ref repl_command) => {
                let eval = self.eval_repl_command(repl_command.C());
                self.history.add_non_script_entry(command, eval.C());
                eval
            }
            Command::ScriptStatement(source) => {
                self.eval_script_statement(source)
            }
            Command::Expression(ref source) => {
                let eval = self.eval_expression(source.C());
                self.history.add_non_script_entry(command, eval.C());
                eval
            }
        }
    }

    fn eval_repl_command(&mut self, command: ReplCommand) -> Eval {
        match command {
            ReplCommand::Unknown => {
                Eval::Error("unknown command".to_string())
            }
            ReplCommand::Help => {
                Eval::CallerInterpret(command)
            }
            ReplCommand::Exit => {
                Eval::CallerInterpret(command)
            }
        }
    }

    fn eval_script_statement(&mut self, source: String) -> Eval {
        let new_unit = create_script_unit(&self.db, source.C());

        let eval = self.eval_script_with_unit(new_unit);

        match &eval {
            Eval::Error(_) => {
                self.history.add_script_entry(
                    Command::ScriptStatement(source),
                    eval.C(),
                    new_unit,
                    false,
                );
            }
            Eval::SuccessLet(_) | Eval::SuccessExpr(_) => {
                self.history.add_script_entry(
                    Command::ScriptStatement(source),
                    eval.C(),
                    new_unit,
                    true,
                );
            }
            Eval::Nothing | Eval::CallerInterpret(_) | Eval::CrashReset(_) => {
                bug!()
            }
        }

        eval
    }
    
    fn eval_script_with_unit(&mut self, unit: datafun::script::ScriptUnit) -> Eval {
        let db = &self.db;

        let new_script = self.history.build_script_with_unit(db, unit);
        let unit_index = new_script.units(db).len() - 1;
        let parsed_unit = datafun::parser::parse_script_unit(db, new_script, unit_index);
        let unit_statements = parsed_unit.statements(db);
        let parsed_script = parse_full_script(db, new_script);

        {
            if unit_statements.is_empty() {
                return Eval::Error("no statements parsed".to_string());
            }

            // Check for parse errors.
            for stmt in unit_statements {
                if let datafun::ast::Statement::ParseError(err) = stmt {
                    let msg = err.message(db).as_str(db).to_string();
                    return Eval::Error(format!("parse error: {}", msg));
                }
            }

            // Run resolution to check if the unit is valid.
            let fun_resolution = datafun::resolution::resolve_functions(db, new_script);
            let let_resolution = datafun::resolution::resolve_let_statement(db, new_script, unit_index);
            //todo check resolution

            // Type check the script.
            let tycheck_result = datafun::tycheck::type_check(db, parsed_script);
            if !tycheck_result.errors(db).is_empty() {
                let errors: Vec<_> = tycheck_result.errors(db)
                    .iter()
                    .map(|e| format!("{:?}", e.error(db)))
                    .collect();
                return Eval::Error(format!("type error(s): {}", errors.join(", ")));
            }
        }

        let result = execute_with_interpreter_impl(db, new_script);
        match result {
            Ok(mut ctx) => {
                if let datafun::ast::Statement::Let(let_stmt) = unit_statements.last().X() {
                    let name = let_stmt.name(db);
                    let name_str = name.as_str(db).to_string();

                    let ty_str = if let Some(type_and_heap) = datafun::tycheck::lookup_variable_type(db, parsed_script, name) {
                        datafun::tycheck::type_to_string(db, type_and_heap.ty(db))
                    } else {
                        "unknown".to_string()
                    };

                    let value_str = ctx.pretty_print_variable(name)
                        .unwrap_or_else(|e| format!("error: {:?}", e));

                    return Eval::SuccessLet(EvalLet {
                        name: name_str,
                        ty: ty_str,
                        value: value_str,
                    });
                }

                Eval::Nothing
            }
            Err(e) => Eval::Error(format!("execution error: {:?}", e)),
        }
    }

    /// Execute or re-execute the full script using the interpreter.
    ///
    /// Returns the InterpContext after execution, which the caller must handle.
    fn execute_with_interpreter(
        &self,
        script: datafun::script::Script,
    ) -> Result<datafun::interp::InterpContext<'_>, datafun::interp::InterpError> {
        execute_with_interpreter_impl(&self.db, script)
    }

    fn eval_expression(&mut self, source: String) -> Eval {
        let db = &self.db;

        // Create the script with the expression wrapped in a let statement.
        // This is temporary - we won't persist it to the history.
        let (new_script, temp_var) = create_expression_script(
            db,
            &self.history,
            &source,
        );

        // Parse the full script for type information.
        let parsed_script = parse_full_script(db, new_script);

        // Type check the script.
        let tycheck_result = datafun::tycheck::type_check(db, parsed_script);
        if !tycheck_result.errors(db).is_empty() {
            let errors: Vec<_> = tycheck_result.errors(db)
                .iter()
                .map(|e| format!("{:?}", e.error(db)))
                .collect();
            return Eval::Error(format!("type error(s): {}", errors.join(", ")));
        }

        // Execute the script and pretty-print the result.
        match execute_with_interpreter_impl(db, new_script) {
            Ok(mut ctx) => {
                let temp_name = bct::text::InternedText::new(db, S(temp_var));

                // Get the type from the typechecker.
                let ty_str = if let Some(type_and_heap) = datafun::tycheck::lookup_variable_type(db, parsed_script, temp_name) {
                    datafun::tycheck::type_to_string(db, type_and_heap.ty(db))
                } else {
                    "unknown".to_string()
                };

                // Get the value by pretty-printing.
                let value_str = match ctx.pretty_print_variable(temp_name) {
                    Ok(s) => s,
                    Err(e) => return Eval::Error(format!("failed to pretty-print: {:?}", e)),
                };

                // Return SuccessExpr with the result.
                // Note: We don't update the history here, so _expr_result won't be in the environment.
                Eval::SuccessExpr(EvalExpr {
                    expr_kind: "expression".to_string(),
                    ty: ty_str,
                    value: value_str,
                })
            }
            Err(e) => Eval::Error(format!("execution error: {:?}", e)),
        }
    }

    /// Get current environment bindings (functions and let statements).
    /// Returns a list of (name, type, value) triples.
    pub fn get_environment(&self) -> Vec<(String, String, String)> {
        let mut bindings = Vec::new();

        // Build a script from the current history.
        let script = self.history.build_script(&self.db);

        let db = &self.db;

        // Get resolved function units.
        let fun_resolution = datafun::resolution::resolve_functions(db, script);
        let green_units = fun_resolution.green_units(db);

        // Extract function names from green units.
        for &fun_idx in green_units {
            let parsed = datafun::parser::parse_script_unit(db, script, fun_idx);
            for stmt in parsed.statements(db) {
                if let datafun::ast::Statement::Fun(fun) = stmt {
                    let name = fun.name(db).as_str(db).to_string();
                    bindings.push((name, "function".to_string(), "".to_string()));
                }
            }
        }

        // Parse the full script for typechecking and interpretation.
        let parsed_script = parse_full_script(db, script);

        // Execute the script to get values.
        let mut ctx = match execute_with_interpreter_impl(db, script) {
            Ok(ctx) => ctx,
            Err(_) => {
                // If execution failed, still return let bindings but with error markers.
                let units = script.units(db);
                for unit_idx in 0..units.len() {
                    let parsed = datafun::parser::parse_script_unit(db, script, unit_idx);
                    for stmt in parsed.statements(db) {
                        if let datafun::ast::Statement::Let(let_stmt) = stmt {
                            let name = let_stmt.name(db).as_str(db).to_string();
                            bindings.push((name, "error".to_string(), "".to_string()));
                        }
                    }
                }
                return bindings;
            }
        };

        // Extract let bindings with types and values.
        let units = script.units(db);
        for unit_idx in 0..units.len() {
            let parsed = datafun::parser::parse_script_unit(db, script, unit_idx);
            for stmt in parsed.statements(db) {
                if let datafun::ast::Statement::Let(let_stmt) = stmt {
                    let name = let_stmt.name(db);
                    let name_str = name.as_str(db).to_string();

                    // Get the type from the typechecker.
                    let ty_str = if let Some(type_and_heap) = datafun::tycheck::lookup_variable_type(db, parsed_script, name) {
                        datafun::tycheck::type_to_string(db, type_and_heap.ty(db))
                    } else {
                        "unknown".to_string()
                    };

                    // Get the value by pretty-printing.
                    let value_str = ctx.pretty_print_variable(name)
                        .unwrap_or_else(|_| "error".to_string());

                    bindings.push((name_str, ty_str, value_str));
                }
            }
        }

        bindings
    }

    /// Execute a script file line by line and output JSON results.
    pub fn run_script(script_path: &std::path::Path) -> AnyResult<()> {
        let mut engine = Self::new()?;
        let contents = std::fs::read_to_string(script_path)
            .context("failed to read script file")?;

        for line in contents.lines() {
            let parse_result = engine.parse_input(Input::Input(line.to_string()));
            let eval_result = match &parse_result {
                InputParse::Command(cmd) => Some(engine.eval(cmd.clone())),
                _ => None,
            };

            let output = serde_json::json!({
                "input": line,
                "parse": parse_result,
                "eval": eval_result,
            });

            println!("{}", serde_json::to_string(&output)?);
        }

        Ok(())
    }
}




/// Create a new script unit from source text.
///
/// This creates Salsa input structs (ScriptUnit, Source)
/// which don't require being in a tracked function context.
fn create_script_unit(
    db: &dyn datafun::Db,
    source_text: String,
) -> datafun::script::ScriptUnit {
    // Create Source input.
    let source_input = Source::new(db, source_text);

    // Create new ScriptUnit input.
    datafun::script::ScriptUnit::new(db, source_input)
}

/// Tracked function to parse all script units into a single AST Script.
#[salsa::tracked]
fn parse_full_script<'db>(
    db: &'db dyn datafun::Db,
    script: datafun::script::Script,
) -> datafun::ast::Script<'db> {
    // Parse all units and collect their statements into a single ast::Script.
    let mut all_statements = Vec::new();
    let units = script.units(db);
    for unit_idx in 0..units.len() {
        let parsed_unit = datafun::parser::parse_script_unit(db, script, unit_idx);
        all_statements.extend(parsed_unit.statements(db).iter().cloned());
    }

    datafun::ast::Script::new(db, all_statements)
}

fn execute_with_interpreter_impl(
    db: &dyn datafun::Db,
    script: datafun::script::Script,
) -> Result<datafun::interp::InterpContext<'_>, datafun::interp::InterpError> {
    // Parse the full script using a tracked function.
    let parsed_script = parse_full_script(db, script);

    // Type check the script.
    let tycheck_result = datafun::tycheck::type_check(db, parsed_script);
    if !tycheck_result.errors(db).is_empty() {
        return Err(datafun::interp::InterpError::TypeError(
            format!("{} type error(s)", tycheck_result.errors(db).len())
        ));
    }

    // Build type table.
    let type_table = datafun::type_table::TypeTable::build(db, parsed_script, tycheck_result)
        .map_err(|e| datafun::interp::InterpError::RuntimeError(format!("failed to build type table: {}", e)))?;

    // Create interpreter context.
    let mut ctx = datafun::interp::InterpContext::new(db, type_table);

    // Execute the full script.
    ctx.execute(parsed_script)?;

    Ok(ctx)
}

/// Create a script for an expression evaluation.
///
/// This wraps the expression in a let statement and creates the script,
/// but does not execute it. The caller must execute and pretty-print.
fn create_expression_script(
    db: &dyn datafun::Db,
    history: &ReplHistory,
    expression: &str,
) -> (datafun::script::Script, &'static str) {
    // Wrap the expression in a let statement with a temporary variable.
    let temp_var = "_expr_result";
    let let_statement = format!("let {} = {}", temp_var, expression);

    // Create script unit for the temporary let statement.
    let temp_unit = create_script_unit(db, let_statement);

    // Build script from history plus the temporary unit.
    let new_script = history.build_script_with_unit(db, temp_unit);

    (new_script, temp_var)
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
