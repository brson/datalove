//! Script compilation for script units.
//!
//! The [`ScriptCompiler`] incrementally compiles script fragments and expressions
//! to IR. It maintains accumulated state across compilations to support REPL-style
//! workflows where later units can reference bindings from earlier ones.
//!
//! Create a compiler via [`CompiledModules::script_compiler()`]. For execution,
//! use a separate [`ScriptExecutor`](super::ScriptExecutor).
//!
//! # Example
//!
//! ```ignore
//! let mut compiler = compiled.script_compiler(&db).unwrap();
//!
//! // Compile a fragment (statements).
//! let result = compiler.compile_fragment("let x = 42");
//! if let Some(ir_unit) = result.ir_unit {
//!     // Pass to executor for execution.
//! }
//!
//! // Later compilation can reference x.
//! let result2 = compiler.compile_fragment("let y = x + 1");
//! ```

use rmx::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use datalove_datafun_ast::ast::{ExprFun, ParsedStatements, Statement};
use datalove_datafun_compiler::lower::{lower_script_fragment_raw, lower_script_expr, evaluate_consts, PreResolvedConsts};
use datalove_datafun_compiler::tracked_script_lower::{
    AccumulatedLowerBindings, lower_script_fragment_tracked, lower_script_expr_tracked,
    build_func_id_map, collect_const_graph,
};
use datalove_datafun_compiler::tracked_script_ownership::{
    analyze_script_fragment_tracked, analyze_script_expr_tracked,
};
use datalove_datafun_ir::{CtfeEvaluator, IrScriptUnit, IrType, ResolvedConsts};
use datalove_datafun_tycheck::{
    type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
};
use datalove_datafun_compiler::ir_ext::IrTypeExt;

use super::compiled_modules::CompiledModules;
use super::result::{TypecheckResult, OwnershipResult, LoweringResult, ScriptCompilationResult};

/// Parsed script unit ready for compilation.
enum ParsedUnit<'db> {
    /// A fragment (statements).
    Fragment {
        parsed: ParsedStatements<'db>,
        stmts: Vec<Statement<'db>>,
    },
    /// A single expression.
    Expr(ExprFun<'db>),
}

// Extension impl for CompiledModules to create script compiler.
impl<'db> CompiledModules<'db> {
    /// Create a script compiler for compiling script units.
    ///
    /// Returns `None` if module compilation failed (has errors).
    /// The compiler handles only compilation; use `script_executor()` for execution.
    pub fn script_compiler(&self, db: &'db dyn salsa::Database) -> Option<ScriptCompiler<'db>> {
        if self.has_errors() {
            return None;
        }

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

        Some(ScriptCompiler {
            db,
            accumulated_unit_specs: Vec::new(),
            accumulated_lower_bindings: AccumulatedLowerBindings::default(),
            module_specs,
            last_source: None,
            last_batch_spec: None,
            ctfe_evaluator: None,
        })
    }
}

/// Script compiler for compiling script units.
///
/// Handles compilation only. No interpreter dependency.
/// Use `compile_fragment()` or `compile_expr()` to compile units.
///
/// For compile-time evaluation of complex const expressions (CTFE), use
/// `set_ctfe_evaluator()` to provide an evaluator (e.g., `InterpCtfeEvaluator`
/// from the interpreter crate).
pub struct ScriptCompiler<'db> {
    db: &'db dyn salsa::Database,
    accumulated_unit_specs: Vec<ScriptUnitSpec<'db>>,
    accumulated_lower_bindings: AccumulatedLowerBindings,
    module_specs: Vec<ModuleSpec<'db>>,
    last_source: Option<bct::input::Source>,
    last_batch_spec: Option<ScriptBatchSpec<'db>>,
    /// Optional CTFE evaluator for complex const expressions.
    ctfe_evaluator: Option<Rc<RefCell<dyn CtfeEvaluator>>>,
}

impl<'db> ScriptCompiler<'db> {
    /// Compile a script fragment (statements).
    pub fn compile_fragment(&mut self, source: &str) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return result;
        }

        let stmts = parsed.statements.to_vec();
        let unit = ParsedUnit::Fragment { parsed, stmts };
        self.compile_unit_inner(src, unit)
    }

    /// Compile a script expression.
    pub fn compile_expr(&mut self, source: &str) -> ScriptCompilationResult {
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

    /// Get parse diagnostics from the last compilation.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Get type diagnostics from the last compilation.
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

    /// Set a CTFE evaluator for complex const expressions.
    ///
    /// When set, the compiler can evaluate const expressions that involve
    /// operations (like `const X = 1 + 2` or `const Y = -42`) at compile time.
    /// Without an evaluator, only simple literals are supported for const bindings.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use datalove_datafun_interp::InterpCtfeEvaluator;
    ///
    /// let mut compiler = compiled.script_compiler(&db).unwrap();
    /// compiler.set_ctfe_evaluator(Rc::new(RefCell::new(InterpCtfeEvaluator::new())));
    /// ```
    pub fn set_ctfe_evaluator(&mut self, evaluator: Rc<RefCell<dyn CtfeEvaluator>>) {
        self.ctfe_evaluator = Some(evaluator);
    }

    /// Get a clone of the CTFE evaluator if one is set.
    pub fn ctfe_evaluator(&self) -> Option<Rc<RefCell<dyn CtfeEvaluator>>> {
        self.ctfe_evaluator.clone()
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
            ownership: OwnershipResult::Skipped,
            lowering: LoweringResult::Skipped,
            ir_unit: None,
        })
    }

    /// Compile a parsed unit through typecheck, ownership, and lowering.
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
            // Remove failed unit so subsequent units don't see its bindings.
            self.accumulated_unit_specs.pop();
            return ScriptCompilationResult {
                typecheck: TypecheckResult::Error { errors: tycheck_errors },
                ownership: OwnershipResult::Skipped,
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            };
        }

        // Run ownership analysis and lowering (unit-kind-specific).
        // If CTFE evaluator is set, use raw lowering functions to pass it through.
        // Otherwise, use tracked functions for salsa memoization.
        let (ir_unit, new_exports, value_types, slot_types): (
            IrScriptUnit,
            Vec<(String, datalove_datafun_ir::ExportBinding)>,
            Vec<IrType>,
            Vec<IrType>,
        ) = match unit {
            ParsedUnit::Fragment { stmts, .. } => {
                let ownership_result = analyze_script_fragment_tracked(
                    self.db,
                    tycheck_result,
                    stmts.clone(),
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    // Remove failed unit so subsequent units don't see its bindings.
                    self.accumulated_unit_specs.pop();
                    return ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    };
                }

                if let Some(ref evaluator) = self.ctfe_evaluator {
                    // Use 3-phase CTFE pipeline for memoization.

                    // Phase 1: Collect const graph (memoized).
                    let const_graph = collect_const_graph(
                        self.db,
                        stmts.clone(),
                        tycheck_result,
                    );

                    // Phase 2: Evaluate consts (not memoized - needs evaluator).
                    let resolved_consts = if !const_graph.is_empty() {
                        let expr_types = tycheck_result.expr_types(self.db);
                        match evaluate_consts(
                            self.db,
                            &const_graph,
                            &stmts,
                            expr_types,
                            evaluator.clone(),
                        ) {
                            Ok(resolved) => resolved,
                            Err(e) => {
                                return ScriptCompilationResult {
                                    typecheck: TypecheckResult::Success,
                                    ownership: OwnershipResult::Success,
                                    lowering: LoweringResult::Error {
                                        message: format!("const evaluation error: {}", e),
                                    },
                                    ir_unit: None,
                                };
                            }
                        }
                    } else {
                        ResolvedConsts::new()
                    };

                    // Phase 3: Lower with pre-resolved consts.
                    // We still use raw lowering but pass the evaluator for any
                    // remaining inline evaluation. The pre-resolved consts
                    // are added to the context by the lowering code.
                    let expr_types = tycheck_result.expr_types(self.db);
                    let call_targets = tycheck_result.call_targets(self.db);
                    let func_id_map = build_func_id_map(self.db, &self.module_specs);
                    let script_ctx = self.accumulated_lower_bindings.to_script_lower_context();

                    // Build func_param_types for type alias support.
                    let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
                    for (name, func_type) in tycheck_result.function_types(self.db) {
                        let param_types: Vec<IrType> = func_type.param_types(self.db)
                            .iter()
                            .map(|ty| IrType::from_tycheck(self.db, ty))
                            .collect();
                        func_param_types.insert(name.text(self.db).S(), param_types);
                    }

                    // Get function and script analyses from ownership result.
                    let func_analyses = ownership_result.to_function_analyses_map(self.db, &stmts);
                    let script_analysis = ownership_result.script_analysis(self.db).clone()
                        .expect("script_analysis required for fragment units");

                    // Phase 3: Lower with pre-resolved consts.
                    // Pass resolved values to avoid re-evaluating consts inline.
                    let pre_resolved = if !const_graph.is_empty() {
                        Some(PreResolvedConsts {
                            graph: &const_graph,
                            values: &resolved_consts,
                        })
                    } else {
                        None
                    };

                    match lower_script_fragment_raw(
                        self.db,
                        expr_types,
                        call_targets,
                        &func_id_map,
                        script_ctx,
                        stmts,
                        func_analyses,
                        script_analysis,
                        Some(&func_param_types),
                        self.ctfe_evaluator.clone(),
                        pre_resolved,
                    ) {
                        Ok(ir_unit) => {
                            let exports = ir_unit.exports.clone();
                            let value_types = ir_unit.value_types.clone();
                            let slot_types = ir_unit.slot_types.clone();
                            (ir_unit, exports, value_types, slot_types)
                        }
                        Err(e) => {
                            // Remove failed unit so subsequent units don't see its bindings.
                            self.accumulated_unit_specs.pop();
                            return ScriptCompilationResult {
                                typecheck: TypecheckResult::Success,
                                ownership: OwnershipResult::Success,
                                lowering: LoweringResult::Error { message: format!("{}", e) },
                                ir_unit: None,
                            };
                        }
                    }
                } else {
                    // Use tracked lowering (salsa memoization, no CTFE).
                    let lower_output = lower_script_fragment_tracked(
                        self.db,
                        tycheck_result,
                        self.module_specs.clone(),
                        self.accumulated_lower_bindings.clone(),
                        stmts,
                        ownership_result,
                    );

                    if let Some(error) = lower_output.error(self.db).as_ref() {
                        // Remove failed unit so subsequent units don't see its bindings.
                        self.accumulated_unit_specs.pop();
                        return ScriptCompilationResult {
                            typecheck: TypecheckResult::Success,
                            ownership: OwnershipResult::Success,
                            lowering: LoweringResult::Error { message: error.clone() },
                            ir_unit: None,
                        };
                    }

                    let ir_unit = lower_output.ir_unit(self.db).clone()
                        .expect("ir_unit should be Some when error is None");
                    (
                        ir_unit,
                        lower_output.new_exports(self.db).clone(),
                        lower_output.value_types(self.db).clone(),
                        lower_output.slot_types(self.db).clone(),
                    )
                }
            }
            ParsedUnit::Expr(expr) => {
                let ownership_result = analyze_script_expr_tracked(
                    self.db,
                    tycheck_result,
                    expr,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    // Remove failed unit so subsequent units don't see its bindings.
                    self.accumulated_unit_specs.pop();
                    return ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    };
                }

                if self.ctfe_evaluator.is_some() {
                    // Use raw lowering with CTFE evaluator.
                    let expr_types = tycheck_result.expr_types(self.db);
                    let call_targets = tycheck_result.call_targets(self.db);
                    let func_id_map = build_func_id_map(self.db, &self.module_specs);
                    let script_ctx = self.accumulated_lower_bindings.to_script_lower_context();

                    match lower_script_expr(
                        self.db,
                        expr_types,
                        call_targets,
                        &func_id_map,
                        script_ctx,
                        expr,
                        self.ctfe_evaluator.clone(),
                    ) {
                        Ok(ir_unit) => {
                            let exports = ir_unit.exports.clone();
                            let value_types = ir_unit.value_types.clone();
                            let slot_types = ir_unit.slot_types.clone();
                            (ir_unit, exports, value_types, slot_types)
                        }
                        Err(e) => {
                            // Remove failed unit so subsequent units don't see its bindings.
                            self.accumulated_unit_specs.pop();
                            return ScriptCompilationResult {
                                typecheck: TypecheckResult::Success,
                                ownership: OwnershipResult::Success,
                                lowering: LoweringResult::Error { message: format!("{}", e) },
                                ir_unit: None,
                            };
                        }
                    }
                } else {
                    // Use tracked lowering (salsa memoization, no CTFE).
                    let lower_output = lower_script_expr_tracked(
                        self.db,
                        tycheck_result,
                        self.module_specs.clone(),
                        self.accumulated_lower_bindings.clone(),
                        expr,
                        ownership_result,
                    );

                    if let Some(error) = lower_output.error(self.db).as_ref() {
                        // Remove failed unit so subsequent units don't see its bindings.
                        self.accumulated_unit_specs.pop();
                        return ScriptCompilationResult {
                            typecheck: TypecheckResult::Success,
                            ownership: OwnershipResult::Success,
                            lowering: LoweringResult::Error { message: error.clone() },
                            ir_unit: None,
                        };
                    }

                    let ir_unit = lower_output.ir_unit(self.db).clone()
                        .expect("ir_unit should be Some when error is None");
                    (
                        ir_unit,
                        lower_output.new_exports(self.db).clone(),
                        lower_output.value_types(self.db).clone(),
                        lower_output.slot_types(self.db).clone(),
                    )
                }
            }
        };

        let ir_dump = format!("{}", ir_unit);

        // Update accumulated state for next unit.
        let unit_index = self.accumulated_lower_bindings.current_unit;
        self.accumulated_lower_bindings.add_exports(
            unit_index,
            &new_exports,
            &value_types,
            &slot_types,
        );

        ScriptCompilationResult {
            typecheck: TypecheckResult::Success,
            ownership: OwnershipResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }
}
