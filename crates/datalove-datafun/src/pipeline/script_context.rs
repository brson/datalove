//! Script execution context with persistent bindings.

use rmx::prelude::*;
use std::sync::Arc;

use datalove_datafun_ast::ast::{ExprFun, ParsedStatements, Statement};
use datalove_datafun_ir::{IrType, IrScriptUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_compiler::tracked_script_lower::{
    AccumulatedLowerBindings, lower_script_fragment_tracked, lower_script_expr_tracked,
};
use datalove_datafun_compiler::tracked_script_ownership::{
    analyze_script_fragment_tracked, analyze_script_expr_tracked,
};
use datalove_datafun_tycheck::{
    type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
};
use datalove_datafun_interp::{CallDispatcher, ScriptEnvironment, UnitCompletion};
use datalove_rt::rust::AlignedBuffer;

use super::compiled_modules::CompiledModules;
use super::result::{
    TypecheckResult, LoweringResult,
    ScriptUnitResult, ScriptLowerResult, ScriptCompilationResult,
};

/// Parsed script unit ready for compilation.
enum ParsedUnit<'db> {
    /// A fragment (statements) with for_aot flag.
    Fragment {
        parsed: ParsedStatements<'db>,
        stmts: Vec<Statement<'db>>,
        for_aot: bool,
    },
    /// A single expression.
    Expr(ExprFun<'db>),
}

// Extension impl for CompiledModules to create script contexts.
impl<'db> CompiledModules<'db> {
    /// Create a script execution context.
    ///
    /// Returns `None` if module compilation failed (has errors).
    /// Can be called multiple times to create independent contexts sharing the
    /// same module compilation. The optional `call_dispatcher` enables JIT or
    /// custom call dispatch.
    pub fn script_context(
        &self,
        db: &'db dyn salsa::Database,
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> Option<ScriptCompilationContext<'db>> {
        // Don't create script context if module compilation failed.
        if self.has_errors() {
            return None;
        }

        let script_ctx = lower::ScriptLowerContext::new();
        let mut module_specs = Vec::new();

        // Build a map of spans for quick lookup.
        let spans_map: std::collections::HashMap<_, _> = self.shared.parsed_graph.spans(db).iter()
            .map(|(id, spans)| (*id, spans.clone()))
            .collect();

        for (salsa_module_id, parsed) in self.shared.parsed_graph.statements_only(db) {
            let module_path = salsa_module_id.path(db).clone();
            let module_source = self.shared.module_graph.iter_modules(db)
                .find(|m| m.id(db) == *salsa_module_id)
                .map(|m| m.source(db))
                .expect("module should exist in graph");
            let spans = spans_map.get(salsa_module_id).cloned()
                .expect("spans should exist for module");

            module_specs.push(ModuleSpec::new(
                module_path.clone(),
                module_source,
                spans,
                parsed.clone(),
                *salsa_module_id,
            ));
        }

        // Create a new ScriptEnvironment that shares the module registry.
        let env = ScriptEnvironment::with_module_registry(Arc::clone(&self.shared.module_registry));

        Some(ScriptCompilationContext {
            db,
            script_ctx,
            env,
            accumulated_unit_specs: Vec::new(),
            accumulated_lower_bindings: AccumulatedLowerBindings::default(),
            module_specs,
            interp: datalove_datafun_interp::IrInterpreter::new_with_options(debug_mode, call_dispatcher),
            last_source: None,
            last_batch_spec: None,
        })
    }
}

/// Script execution context with persistent bindings.
///
/// Compiles and executes script fragments (`eval_fragment`) and expressions
/// (`eval_expr`). Bindings from `let` and `var` statements persist across
/// evaluations.
pub struct ScriptCompilationContext<'db> {
    db: &'db dyn salsa::Database,
    pub script_ctx: lower::ScriptLowerContext,
    pub env: ScriptEnvironment,
    accumulated_unit_specs: Vec<ScriptUnitSpec<'db>>,
    accumulated_lower_bindings: AccumulatedLowerBindings,
    module_specs: Vec<ModuleSpec<'db>>,
    interp: datalove_datafun_interp::IrInterpreter,
    last_source: Option<bct::input::Source>,
    last_batch_spec: Option<ScriptBatchSpec<'db>>,
}

impl<'db> ScriptCompilationContext<'db> {
    /// Compile and execute a script fragment (statements like `let x = 1`).
    pub fn eval_fragment(&mut self, source: &str) -> ScriptUnitResult {
        let compiled = self.compile_fragment_inner(source, false);
        if let Some(ir_unit) = &compiled.ir_unit {
            let output = self.execute_fragment(ir_unit);
            ScriptUnitResult {
                typecheck: compiled.typecheck,
                lowering: compiled.lowering,
                ty: None,
                output,
            }
        } else {
            ScriptUnitResult {
                typecheck: compiled.typecheck,
                lowering: compiled.lowering,
                ty: None,
                output: String::new(),
            }
        }
    }

    /// Compile and execute an expression, returning its value.
    pub fn eval_expr(&mut self, source: &str) -> ScriptUnitResult {
        let compiled = self.compile_expr_inner(source, false);
        if let Some(ir_unit) = &compiled.ir_unit {
            let (ty, output) = self.execute_expr(ir_unit);
            ScriptUnitResult {
                typecheck: compiled.typecheck,
                lowering: compiled.lowering,
                ty,
                output,
            }
        } else {
            ScriptUnitResult {
                typecheck: compiled.typecheck,
                lowering: compiled.lowering,
                ty: None,
                output: String::new(),
            }
        }
    }

    /// Lower a fragment to IR for AOT compilation.
    pub fn lower_fragment_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        let compiled = self.compile_fragment_inner(source, true);
        ScriptLowerResult {
            typecheck: compiled.typecheck,
            lowering: compiled.lowering,
            ir_unit: compiled.ir_unit,
        }
    }

    /// Lower an expression to IR for AOT compilation.
    pub fn lower_expr_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        let compiled = self.compile_expr_inner(source, true);
        ScriptLowerResult {
            typecheck: compiled.typecheck,
            lowering: compiled.lowering,
            ir_unit: compiled.ir_unit,
        }
    }

    /// Internal: compile a fragment through all phases without executing.
    fn compile_fragment_inner(&mut self, source: &str, for_aot: bool) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return result;
        }

        let stmts = parsed.statements.to_vec();
        let unit = ParsedUnit::Fragment { parsed, stmts, for_aot };
        self.compile_unit_inner(src, unit)
    }

    /// Internal: compile an expression through all phases without executing.
    fn compile_expr_inner(&mut self, source: &str, _for_aot: bool) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        let parse_diags = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return result;
        }

        let unit = ParsedUnit::Expr(expr);
        self.compile_unit_inner(src, unit)
    }

    /// Check for parse errors and return early result if any.
    fn check_parse_errors(&self, parse_diags: &[&datalove_diagnostic::ParseDiagnostic]) -> Option<ScriptCompilationResult> {
        if parse_diags.is_empty() {
            return None;
        }
        let parse_errors: Vec<String> = parse_diags.iter()
            .map(|d| {
                let diag = d.to_diagnostic(self.db);
                diag.message.as_str(self.db).S()
            })
            .collect();
        Some(ScriptCompilationResult {
            typecheck: TypecheckResult::ParseError { errors: parse_errors },
            lowering: LoweringResult::Skipped,
            ir_unit: None,
        })
    }

    /// Internal: compile a parsed unit through typecheck, ownership, and lowering.
    fn compile_unit_inner(
        &mut self,
        src: bct::input::Source,
        unit: ParsedUnit<'db>,
    ) -> ScriptCompilationResult {
        // Create unit spec for typechecking.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_kind = match &unit {
            ParsedUnit::Fragment { parsed, .. } => ScriptUnitKind::Fragment(parsed.clone()),
            ParsedUnit::Expr(expr) => ScriptUnitKind::Expr(*expr),
        };
        let unit_spec = ScriptUnitSpec::new(src, spans, unit_kind);
        self.accumulated_unit_specs.push(unit_spec);

        // Run typechecking.
        let batch_spec = create_batch_spec(
            self.db,
            src,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        // Check for typecheck errors.
        let tycheck_errors: Vec<_> = tycheck_result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !tycheck_errors.is_empty() {
            return ScriptCompilationResult {
                typecheck: TypecheckResult::Error { errors: tycheck_errors },
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            };
        }

        // Run ownership analysis and lowering (unit-kind-specific).
        let lower_output = match unit {
            ParsedUnit::Fragment { stmts, for_aot, .. } => {
                let ownership_result = analyze_script_fragment_tracked(
                    self.db,
                    tycheck_result,
                    stmts.clone(),
                    for_aot,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    return ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        lowering: LoweringResult::Error { message: error_msg },
                        ir_unit: None,
                    };
                }
                lower_script_fragment_tracked(
                    self.db,
                    tycheck_result,
                    self.module_specs.clone(),
                    self.accumulated_lower_bindings.clone(),
                    stmts,
                    ownership_result,
                )
            }
            ParsedUnit::Expr(expr) => {
                let ownership_result = analyze_script_expr_tracked(
                    self.db,
                    tycheck_result,
                    expr,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    return ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        lowering: LoweringResult::Error { message: error_msg },
                        ir_unit: None,
                    };
                }
                lower_script_expr_tracked(
                    self.db,
                    tycheck_result,
                    self.module_specs.clone(),
                    self.accumulated_lower_bindings.clone(),
                    expr,
                    ownership_result,
                )
            }
        };

        // Check for lowering errors.
        if let Some(error) = lower_output.error(self.db).as_ref() {
            return ScriptCompilationResult {
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: error.clone() },
                ir_unit: None,
            };
        }

        let ir_unit = lower_output.ir_unit(self.db).clone().expect("ir_unit should be Some when error is None");
        let ir_dump = format!("{}", ir_unit);

        // Update accumulated state for next unit.
        let unit_index = self.accumulated_lower_bindings.current_unit;
        self.accumulated_lower_bindings.add_exports(
            unit_index,
            lower_output.new_exports(self.db),
            lower_output.value_types(self.db),
            lower_output.slot_types(self.db),
        );

        // Also update script_ctx for interpreter use.
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptCompilationResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    /// Internal: execute an already-compiled fragment unit.
    fn execute_fragment(&mut self, ir_unit: &IrScriptUnit) -> String {
        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
        let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
        let ret_dest = datalove_datafun_interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        match self.interp.execute_script_unit_in_env(ir_unit, &mut self.env, ret_dest, None) {
            Ok(UnitCompletion::Normal) => "(fragment executed)".S(),
            Ok(UnitCompletion::EarlyReturn) => {
                let value = datalove_datafun_interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let output_str = self.interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));
                let _ = self.interp.destroy_value(&value);
                output_str
            }
            Err(e) => format!("Error: {:?}", e),
        }
    }

    /// Internal: execute an already-compiled expression unit.
    ///
    /// Returns (result_type, output_string).
    fn execute_expr(&mut self, ir_unit: &IrScriptUnit) -> (Option<String>, String) {
        let result_ty = ir_unit.result
            .map(|id| format!("{}", &ir_unit.value_types[id.0 as usize]));

        let output = if let Some(result_id) = ir_unit.result {
            let ret_type = IrType::Result(Box::new(IrType::Unit));
            let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
            let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
            let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
            let ret_dest = datalove_datafun_interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            let expr_type = &ir_unit.value_types[result_id.0 as usize];
            let expr_tydesc = self.interp.tydesc_table_mut().get_or_create(expr_type);
            let (expr_size, expr_align) = unsafe { ((*expr_tydesc).size, (*expr_tydesc).align) };
            let mut expr_buffer = AlignedBuffer::with_align(expr_size as usize, expr_align as usize);
            let expr_dest = datalove_datafun_interp::Destination {
                ptr: expr_buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            };

            match self.interp.execute_script_unit_in_env(ir_unit, &mut self.env, ret_dest, Some(expr_dest)) {
                Ok(UnitCompletion::Normal) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: expr_buffer.as_mut_ptr(),
                        tydesc: expr_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    let _ = self.interp.destroy_value(&value);
                    output_str
                }
                Ok(UnitCompletion::EarlyReturn) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: ret_buffer.as_mut_ptr(),
                        tydesc: ret_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    let _ = self.interp.destroy_value(&value);
                    output_str
                }
                Err(e) => format!("Error: {:?}", e),
            }
        } else {
            "(fragment executed)".S()
        };

        (result_ty, output)
    }

    /// Get the type and value of a binding by name.
    pub fn get_binding(&mut self, name: &str) -> Option<(String, String)> {
        use datalove_datafun_interp::InterpError;

        if let Some((unit, value_id)) = self.script_ctx.values.get(name) {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_value(*unit, *value_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedValue(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            return Some((ty, val));
        }

        if let Some((unit, slot_id)) = self.script_ctx.slots.get(name) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_slot(*unit, *slot_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedSlot(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            return Some((ty, val));
        }

        None
    }

    /// Get all bindings as (name, kind, type, value) tuples.
    pub fn get_environment(&mut self) -> Vec<(String, String, String, String)> {
        use datalove_datafun_interp::InterpError;
        let mut result = Vec::new();

        for (name, (unit, value_id)) in &self.script_ctx.values {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_value(*unit, *value_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedValue(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            result.push((name.C(), "let".S(), ty, val));
        }

        for (name, (unit, slot_id)) in &self.script_ctx.slots {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_slot(*unit, *slot_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedSlot(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            result.push((name.C(), "var".S(), ty, val));
        }

        for (name, _) in &self.script_ctx.functions {
            result.push((name.C(), "fun".S(), "function".S(), "-".S()));
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Get parse diagnostics from the last evaluation.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Get type diagnostics from the last evaluation.
    pub fn get_type_diagnostics(&self) -> Vec<&datalove_diagnostic::TypeDiagnostic> {
        if let Some(batch_spec) = self.last_batch_spec {
            type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(self.db, batch_spec)
        } else {
            Vec::new()
        }
    }

    /// Get the database reference.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// Get buffered debug output.
    pub fn get_debug_buffer(&self) -> String {
        self.interp.get_debug_buffer()
    }

    /// Clear buffered debug output.
    pub fn clear_debug_buffer(&self) {
        self.interp.clear_debug_buffer();
    }

    /// Destroy all allocated runtime values.
    pub fn destroy_all(&mut self) {
        self.env.destroy_all(self.interp.runtime_handle());
    }
}
