//! Script compilation for script units.
//!
//! The [`ScriptCompiler`] incrementally compiles script fragments and expressions
//! to IR. It maintains accumulated state across compilations to support REPL-style
//! workflows where later units can reference bindings from earlier ones.
//!
//! Create a compiler via [`CompiledModules::script_compiler_default()`]. For execution,
//! use a separate [`ScriptExecutor`](super::ScriptExecutor).
//!
//! # Example
//!
//! ```ignore
//! let mut compiler = compiled.script_compiler_default(&db).unwrap();
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
use datalove_datafun_compiler::lower::{lower_script_fragment_raw, lower_script_expr, evaluate_consts, evaluate_script_function_consts, PreResolvedConsts};
use datalove_datafun_compiler::tracked_script_lower::{
    AccumulatedLowerBindings, build_func_id_map, collect_const_graph,
};
use datalove_datafun_compiler::tracked_script_ownership::{
    analyze_script_fragment_tracked, analyze_script_expr_tracked, ScriptAnalysisData,
};
use datalove_datafun_compiler::ownership_analysis::ScriptFunctionAnalyses;
use datalove_datafun_ir::{ConstBindingGraph, ConstValue, CtfeEvaluator, IrScriptUnit, IrType, ResolvedConsts};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::{
    type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked, ResolvedCallTarget, Type,
};
use datalove_datafun_resolve::resolve_script_names;
use datalove_datafun_compiler::ir_ext::IrTypeExt;

use super::compiled_modules::CompiledModules;
use super::result::{TypecheckResult, OwnershipResult, LoweringResult, ScriptCompilationResult};

// ============================================================================
// Phase output types
// ============================================================================

/// Output from typecheck phase.
struct TypecheckOutput<'db> {
    result: UnitTypecheckResultTracked<'db>,
    expr_types: &'db [Option<Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
}

/// Output from ownership phase.
struct OwnershipOutput<'db> {
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: Option<ScriptAnalysisData>,
}

/// Output from const evaluation phase.
struct ConstEvalOutput {
    const_graph: ConstBindingGraph,
    resolved_consts: ResolvedConsts,
    func_consts: HashMap<String, (IrType, ConstValue)>,
}

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
    /// The `ctfe_evaluator` is used for compile-time evaluation of const expressions.
    /// Use `InterpCtfeEvaluator` from the interpreter crate.
    ///
    /// Returns `None` if module compilation failed (has errors).
    /// The compiler handles only compilation; use `script_executor()` for execution.
    pub fn script_compiler(
        &self,
        db: &'db dyn salsa::Database,
        ctfe_evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    ) -> Option<ScriptCompiler<'db>> {
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
            let name_resolution = resolve_script_names(db, module_source, parsed.clone());

            module_specs.push(ModuleSpec::new(
                module_path.clone(),
                module_source,
                spans,
                parsed.clone(),
                *salsa_module_id,
                name_resolution,
            ));
        }

        Some(ScriptCompiler {
            db,
            accumulated_unit_specs: Vec::new(),
            accumulated_lower_bindings: AccumulatedLowerBindings::default(),
            module_specs,
            last_source: None,
            last_batch_spec: None,
            ctfe_evaluator,
        })
    }

    /// Create a script compiler with the default interpreter-based CTFE evaluator.
    ///
    /// This is the recommended way to create a script compiler. It uses the
    /// interpreter for compile-time evaluation of const expressions.
    ///
    /// Returns `None` if module compilation failed (has errors).
    pub fn script_compiler_default(
        &self,
        db: &'db dyn salsa::Database,
    ) -> Option<ScriptCompiler<'db>> {
        let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
        self.script_compiler(db, evaluator)
    }
}

/// Script compiler for compiling script units.
///
/// Handles compilation only. No interpreter dependency.
/// Use `compile_fragment()` or `compile_expr()` to compile units.
pub struct ScriptCompiler<'db> {
    db: &'db dyn salsa::Database,
    accumulated_unit_specs: Vec<ScriptUnitSpec<'db>>,
    accumulated_lower_bindings: AccumulatedLowerBindings,
    module_specs: Vec<ModuleSpec<'db>>,
    last_source: Option<bct::input::Source>,
    last_batch_spec: Option<ScriptBatchSpec<'db>>,
    /// CTFE evaluator for const expression evaluation.
    ctfe_evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
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

    /// Compile a parsed unit through typecheck, ownership, const eval, and lowering.
    ///
    /// Pipeline phases:
    /// 1. Typecheck - type inference and checking
    /// 2. Ownership Analysis - borrow checking and drop scheduling
    /// 3. Const Evaluation - compile-time const evaluation (fragment only)
    /// 4. IR Lowering - generate IR
    fn compile_unit_inner(
        &mut self,
        src: bct::input::Source,
        unit: ParsedUnit<'db>,
    ) -> ScriptCompilationResult {
        // Phase 1: Typecheck
        let typecheck = match self.phase_typecheck(src, &unit) {
            Ok(tc) => tc,
            Err(result) => return result,
        };

        // Phase 2: Ownership Analysis
        let ownership = match self.phase_ownership(&unit, &typecheck) {
            Ok(own) => own,
            Err(result) => return result,
        };

        // Phase 3: Const Evaluation (fragment only)
        let consts = match self.phase_const_eval(&unit, &typecheck) {
            Ok(c) => c,
            Err(result) => return result,
        };

        // Phase 4: IR Lowering
        let ir_unit = match self.phase_lower(&unit, &typecheck, &ownership, &consts) {
            Ok(ir) => ir,
            Err(result) => return result,
        };

        // Update accumulated state
        self.update_accumulated_state(&ir_unit);

        let ir_dump = format!("{}", ir_unit);
        ScriptCompilationResult {
            typecheck: TypecheckResult::Success,
            ownership: OwnershipResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    // ========================================================================
    // Phase 1: Typecheck
    // ========================================================================

    /// Run typechecking on the unit.
    fn phase_typecheck(
        &mut self,
        src: bct::input::Source,
        unit: &ParsedUnit<'db>,
    ) -> Result<TypecheckOutput<'db>, ScriptCompilationResult> {
        // Create unit spec.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_kind = match unit {
            ParsedUnit::Fragment { parsed, .. } => {
                let name_resolution = resolve_script_names(self.db, src, parsed.clone());
                ScriptUnitKind::Fragment(parsed.clone(), name_resolution)
            }
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
        let result = *all_results.last().unwrap();

        // Check for errors.
        let errors: Vec<_> = result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !errors.is_empty() {
            self.accumulated_unit_specs.pop();
            return Err(ScriptCompilationResult {
                typecheck: TypecheckResult::Error { errors },
                ownership: OwnershipResult::Skipped,
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            });
        }

        Ok(TypecheckOutput {
            result,
            expr_types: result.expr_types(self.db),
            call_targets: result.call_targets(self.db),
        })
    }

    // ========================================================================
    // Phase 2: Ownership Analysis
    // ========================================================================

    /// Run ownership analysis on the unit.
    fn phase_ownership(
        &mut self,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
    ) -> Result<OwnershipOutput<'db>, ScriptCompilationResult> {
        match unit {
            ParsedUnit::Fragment { stmts, .. } => {
                let ownership_result = analyze_script_fragment_tracked(
                    self.db,
                    typecheck.result,
                    stmts.clone(),
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    self.accumulated_unit_specs.pop();
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                let func_analyses = ownership_result.to_function_analyses_map(self.db, stmts);
                let script_analysis = ownership_result.script_analysis(self.db).clone();
                Ok(OwnershipOutput { func_analyses, script_analysis })
            }
            ParsedUnit::Expr(expr) => {
                let ownership_result = analyze_script_expr_tracked(
                    self.db,
                    typecheck.result,
                    *expr,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    self.accumulated_unit_specs.pop();
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                Ok(OwnershipOutput {
                    func_analyses: HashMap::new(),
                    script_analysis: None,
                })
            }
        }
    }

    // ========================================================================
    // Phase 3: Const Evaluation
    // ========================================================================

    /// Evaluate compile-time constants (fragment only).
    fn phase_const_eval(
        &mut self,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
    ) -> Result<ConstEvalOutput, ScriptCompilationResult> {
        let ParsedUnit::Fragment { stmts, .. } = unit else {
            // Expressions don't have const bindings.
            return Ok(ConstEvalOutput {
                const_graph: ConstBindingGraph::default(),
                resolved_consts: ResolvedConsts::new(),
                func_consts: HashMap::new(),
            });
        };

        // Collect const graph (memoized).
        let const_graph = collect_const_graph(self.db, stmts.clone(), typecheck.result);

        // Evaluate script-level consts.
        let resolved_consts = if !const_graph.is_empty() {
            match evaluate_consts(
                self.db,
                &const_graph,
                stmts,
                typecheck.expr_types,
                self.ctfe_evaluator.clone(),
            ) {
                Ok(resolved) => resolved,
                Err(e) => {
                    self.accumulated_unit_specs.pop();
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Success,
                        lowering: LoweringResult::Error {
                            message: format!("const evaluation error: {}", e),
                        },
                        ir_unit: None,
                    });
                }
            }
        } else {
            ResolvedConsts::new()
        };

        // Build script-level consts map for function const evaluation.
        let script_level_consts: HashMap<String, (IrType, ConstValue)> = resolved_consts.iter()
            .map(|(name, value)| {
                let ir_type = const_graph.bindings.iter()
                    .find(|b| &b.name == name)
                    .map(|b| b.ir_type.clone())
                    .unwrap_or(IrType::Unit);
                (name.to_string(), (ir_type, value.clone()))
            })
            .collect();

        // Evaluate function-level consts.
        let func_consts_result = evaluate_script_function_consts(
            self.db,
            stmts,
            typecheck.expr_types,
            &script_level_consts,
            self.ctfe_evaluator.clone(),
        );

        if !func_consts_result.errors.is_empty() {
            self.accumulated_unit_specs.pop();
            return Err(ScriptCompilationResult {
                typecheck: TypecheckResult::Success,
                ownership: OwnershipResult::Success,
                lowering: LoweringResult::Error {
                    message: format!("const evaluation errors:\n  {}", func_consts_result.errors.join("\n  ")),
                },
                ir_unit: None,
            });
        }

        Ok(ConstEvalOutput {
            const_graph,
            resolved_consts,
            func_consts: func_consts_result.consts,
        })
    }

    // ========================================================================
    // Phase 4: IR Lowering
    // ========================================================================

    /// Lower to IR.
    fn phase_lower(
        &mut self,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
        consts: &ConstEvalOutput,
    ) -> Result<IrScriptUnit, ScriptCompilationResult> {
        let func_id_map = build_func_id_map(self.db, &self.module_specs);
        let script_ctx = self.accumulated_lower_bindings.to_script_lower_context();

        match unit {
            ParsedUnit::Fragment { stmts, .. } => {
                // Build func_param_types for type alias support.
                let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
                for (name, func_type) in typecheck.result.function_types(self.db) {
                    let param_types: Vec<IrType> = func_type.param_types(self.db)
                        .iter()
                        .map(|ty| IrType::from_tycheck(self.db, ty))
                        .collect();
                    func_param_types.insert(name.text(self.db).S(), param_types);
                }

                let script_analysis = ownership.script_analysis.clone()
                    .expect("script_analysis required for fragment units");

                let pre_resolved = if !consts.const_graph.is_empty() || !consts.func_consts.is_empty() {
                    Some(PreResolvedConsts {
                        graph: &consts.const_graph,
                        values: &consts.resolved_consts,
                        func_consts: &consts.func_consts,
                    })
                } else {
                    None
                };

                lower_script_fragment_raw(
                    self.db,
                    typecheck.expr_types,
                    typecheck.call_targets,
                    &func_id_map,
                    script_ctx,
                    stmts.clone(),
                    ownership.func_analyses.clone(),
                    script_analysis,
                    Some(&func_param_types),
                    pre_resolved,
                ).map_err(|e| {
                    self.accumulated_unit_specs.pop();
                    ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Success,
                        lowering: LoweringResult::Error { message: format!("{}", e) },
                        ir_unit: None,
                    }
                })
            }
            ParsedUnit::Expr(expr) => {
                lower_script_expr(
                    self.db,
                    typecheck.expr_types,
                    typecheck.call_targets,
                    &func_id_map,
                    script_ctx,
                    *expr,
                ).map_err(|e| {
                    self.accumulated_unit_specs.pop();
                    ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Success,
                        lowering: LoweringResult::Error { message: format!("{}", e) },
                        ir_unit: None,
                    }
                })
            }
        }
    }

    // ========================================================================
    // Accumulated State
    // ========================================================================

    /// Update accumulated state after successful compilation.
    fn update_accumulated_state(&mut self, ir_unit: &IrScriptUnit) {
        let unit_index = self.accumulated_lower_bindings.current_unit;
        self.accumulated_lower_bindings.add_exports(
            unit_index,
            &ir_unit.exports,
            &ir_unit.value_types,
            &ir_unit.slot_types,
        );
    }
}
