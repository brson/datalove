//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use bct::input::Source;
use std::collections::BTreeMap;

use crate::{Command, ReplCommand, Eval, EvalLet, EvalExpr, EvalFun, InputParse, Input};
use crate::datafun;

pub struct Engine<'db> {
    db: &'db dyn datafun::Db,
    history: ReplHistory,
    /// Persistent interpreter context.
    interp_ctx: datafun::interp::InterpContext<'db>,
    /// Package world for the REPL (empty for now).
    package_world: datafun::package::PackageWorld,
    /// Typecheck result for the package world.
    typecheck_result: datafun::tycheck::PackageWorldTypecheckResult<'db>,
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

impl<'db> Engine<'db> {
    pub fn new(db: &'db dyn datafun::Db) -> AnyResult<Engine<'db>> {
        // Create empty package world.
        let empty_package_world = datafun::package_load::PackageWorld {
            pkglib_system: BTreeMap::new(),
            pkglib_local: BTreeMap::new(),
        };
        let package_world = datafun::package::import_from_loader(db, empty_package_world);

        // Resolve and typecheck the empty package world.
        let resolution = datafun::package_resolve::resolve_package_world_with_imports(db, package_world);
        let graph = resolution.result(db)
            .map_err(|e| rmx::anyhow::anyhow!("Package resolution failed: {:?}", e))?;

        let typecheck_result = datafun::tycheck::typecheck_package_world(db, graph);

        // Create interpreter context.
        let interp_ctx = datafun::interp::InterpContext::new_with_typecheck(db, package_world, typecheck_result)
            .map_err(|e| rmx::anyhow::anyhow!("Failed to create interpreter context: {:?}", e))?;

        Ok(Engine {
            db,
            history: ReplHistory::new(),
            interp_ctx,
            package_world,
            typecheck_result,
        })
    }

    fn reset(&mut self) {
        self.history = ReplHistory::new();

        // Create a fresh interpreter context.
        // Note: We reuse the same package_world and typecheck_result.
        if let Ok(ctx) = datafun::interp::InterpContext::new_with_typecheck(
            self.db,
            self.package_world,
            self.typecheck_result,
        ) {
            self.interp_ctx = ctx;
        }
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
        let new_unit = create_script_unit(self.db, source.C());

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
            Eval::SuccessLet(_) | Eval::SuccessExpr(_) | Eval::SuccessFun(_) => {
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
        let db = self.db;

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

            // Type check the full script to see all variable bindings.
            let dummy_source = bct::input::Source::new(db, String::new());
            let script_typecheck = datafun::tycheck::type_check(db, dummy_source, parsed_script);
            if !script_typecheck.errors(db).is_empty() {
                let errors: Vec<_> = script_typecheck.errors(db)
                    .iter()
                    .map(|e| format!("{:?}", e.error(db)))
                    .collect();
                return Eval::Error(format!("type error(s): {}", errors.join(", ")));
            }
        }

        // Update context with new script.
        self.interp_ctx.set_script(new_script);

        // Execute the script unit using the new interpreter.
        if let Err(e) = datafun::interp::execute_script_unit(&mut self.interp_ctx, new_script, unit_index) {
            return Eval::Error(format!("execution error: {:?}", e));
        }

        // Return information about the last statement.
        match unit_statements.last().X() {
            datafun::ast::Statement::Let(let_stmt) => {
                let name = let_stmt.name(db);
                let name_str = name.as_str(db).to_string();

                let ty_str = if let Some(type_and_heap) = datafun::tycheck::lookup_variable_type(db, parsed_script, name) {
                    datafun::tycheck::type_to_string(db, type_and_heap.ty(db))
                } else {
                    "unknown".to_string()
                };

                // Pretty-print from the script scope.
                // Extract value first to avoid borrow conflict.
                let value_opt = self.interp_ctx.script_scope.variables.get(&name).map(|v| v.value);
                let value_str = if let Some(value) = value_opt {
                    self.interp_ctx.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("error: {:?}", e))
                } else {
                    "error: variable not found".to_string()
                };

                return Eval::SuccessLet(EvalLet {
                    name: name_str,
                    ty: ty_str,
                    value: value_str,
                });
            }
            datafun::ast::Statement::Fun(fun) => {
                let name = fun.name(db);
                let name_str = name.as_str(db).to_string();

                return Eval::SuccessFun(EvalFun {
                    name: name_str,
                });
            }
            _ => {
                // Other statement types (if any) result in Nothing.
                Eval::Nothing
            }
        }
    }

    fn eval_expression(&mut self, source: String) -> Eval {
        let db = self.db;

        // Create the script with the expression wrapped in a let statement.
        // This is temporary - we won't persist it to the history.
        let (new_script, temp_var) = create_expression_script(
            db,
            &self.history,
            &source,
        );

        let parsed_script = parse_full_script(db, new_script);
        let units = new_script.units(db);
        let temp_unit_index = units.len() - 1;

        // Type check the full script to see all variable bindings.
        let dummy_source = bct::input::Source::new(db, String::new());
        let script_typecheck = datafun::tycheck::type_check(db, dummy_source, parsed_script);
        if !script_typecheck.errors(db).is_empty() {
            let errors: Vec<_> = script_typecheck.errors(db)
                .iter()
                .map(|e| format!("{:?}", e.error(db)))
                .collect();
            return Eval::Error(format!("type error(s): {}", errors.join(", ")));
        }

        // Update context with new script.
        self.interp_ctx.set_script(new_script);

        // Execute the temp unit using the new interpreter.
        if let Err(e) = datafun::interp::execute_script_unit(&mut self.interp_ctx, new_script, temp_unit_index) {
            return Eval::Error(format!("execution error: {:?}", e));
        }

        let name = bct::text::InternedText::new(db, S(temp_var));

        let ty_str = if let Some(type_and_heap) = datafun::tycheck::lookup_variable_type(db, parsed_script, name) {
            datafun::tycheck::type_to_string(db, type_and_heap.ty(db))
        } else {
            "unknown".to_string()
        };

        // Pretty-print from the script scope.
        // Extract value first to avoid borrow conflict.
        let value_opt = self.interp_ctx.script_scope.variables.get(&name).map(|v| v.value);
        let value_str = if let Some(value) = value_opt {
            self.interp_ctx.pretty_print_value(&value)
                .unwrap_or_else(|e| format!("error: {:?}", e))
        } else {
            "error: variable not found".to_string()
        };

        // Remove the temporary variable from the script scope.
        if let Some(var) = self.interp_ctx.script_scope.variables.remove(&name) {
            datafun::interp::destroy_value(&mut self.interp_ctx, var.value);
        }

        Eval::SuccessExpr(EvalExpr {
            expr_kind: "expression".to_string(),
            ty: ty_str,
            value: value_str,
        })
    }

    /// Get current environment bindings (functions and let statements).
    /// Returns a list of (name, type, value) triples.
    pub fn get_environment(&mut self) -> Vec<(String, String, String)> {
        let mut bindings = Vec::new();

        let db = self.db;

        // Build the full script for type lookup.
        let script = self.history.build_script(db);
        let parsed_script = parse_full_script(db, script);

        // Add functions.
        for (name, _fun) in &self.interp_ctx.script_scope.functions {
            bindings.push((
                name.as_str(db).to_string(),
                "function".to_string(),
                "".to_string(),
            ));
        }

        // Collect variable names and values for pretty-printing.
        // We need to collect the value pointers first to avoid borrow conflicts.
        // Only include Available variables - Moved ones have invalid pointers.
        let var_data: Vec<_> = self.interp_ctx.script_scope.variables.iter()
            .filter(|(_, var)| var.state == datafun::interp::ScriptVarState::Available)
            .map(|(name, var)| (*name, var.value))
            .collect();

        // Add variables with types and values.
        for (name, value) in var_data {
            // Get the type from the typechecker.
            let ty_str = if let Some(type_and_heap) = datafun::tycheck::lookup_variable_type(db, parsed_script, name) {
                datafun::tycheck::type_to_string(db, type_and_heap.ty(db))
            } else {
                "unknown".to_string()
            };

            // Get the value by pretty-printing.
            let value_str = self.interp_ctx.pretty_print_value(&value)
                .unwrap_or_else(|_| "error".to_string());

            bindings.push((
                name.as_str(db).to_string(),
                ty_str,
                value_str,
            ));
        }

        // Sort bindings by name for deterministic output.
        bindings.sort_by(|a, b| a.0.cmp(&b.0));

        bindings
    }

    /// Execute a script file line by line and output JSON results.
    pub fn run_script(db: &'db dyn datafun::Db, script_path: &std::path::Path) -> AnyResult<()> {
        let mut engine = Self::new(db)?;
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

impl<'db> Drop for Engine<'db> {
    fn drop(&mut self) {
        // Clean up only Available variables (not Moved ones, which have been transferred).
        let vars: Vec<_> = self.interp_ctx.script_scope.variables
            .drain()
            .filter(|(_, var)| var.state == datafun::interp::ScriptVarState::Available)
            .map(|(_, var)| var.value)
            .collect();

        for value in vars {
            datafun::interp::destroy_value(&mut self.interp_ctx, value);
        }
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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

        // Add a let statement.
        let parse_result = engine.parse_input(Input::Input("let x: @int = @10".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                // Should return SuccessLet for let statement.
                assert!(matches!(eval_result, Eval::SuccessLet(_)));
            }
            _ => panic!("Expected Command"),
        }

        // Evaluate an expression using the previous binding.
        let parse_result = engine.parse_input(Input::Input("x + : @int / @5".to_string()));
        match parse_result {
            InputParse::Command(cmd) => {
                let eval_result = engine.eval(cmd);
                match eval_result {
                    Eval::SuccessExpr(eval_expr) => {
                        // The result should contain "15".
                        assert!(eval_expr.value.contains("15"), "Expected result to contain '15', got: {}", eval_expr.value);
                        assert_eq!(eval_expr.ty, "int");
                    }
                    other => panic!("Expected Eval::SuccessExpr, got {:?}", other),
                }
            }
            other => panic!("Expected Command, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_error_handling() {
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
        let db = datafun::Database::default();
        let mut engine = Engine::new(&db).unwrap();

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
