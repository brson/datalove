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
use std::sync::Arc;

use datalove_datafun_ast::ast::{ExprFun, ParsedStatements, Statement};
use datalove_datafun_compiler::lower::{
    lower_script_fragment_raw, lower_script_expr, lower_script_functions,
    lower_const_binding,
};
use datalove_datafun_const::{inline_script_consts, PreparedConst, ScriptFunctionConstsResult, evaluate_prepared_const};
use datalove_datafun_compiler::tracked_script_lower::{
    AccumulatedLowerBindings, build_func_id_map, collect_const_graph,
};
use datalove_datafun_compiler::tracked_script_ownership::{
    analyze_script_fragment_tracked, analyze_script_expr_tracked, ScriptAnalysisData,
    emit_ownership_diagnostics, AnalysisError,
};
use datalove_datafun_compiler::lower::ScriptFunctionAnalyses;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, CtfeError, IrCodeUnit, IrType, ResolvedConsts, ConstEvalError};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::{
    type_check_script_units, create_batch_spec_with_auto_adapt,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked,
    AutoAdaptMode,
};
use datalove_datafun_resolve::resolve_script_names;
use datalove_datafun_compiler::IrTypeExt;

use super::compiled_modules::{CompiledModules, SharedModuleContext};
use super::result::{TypecheckResult, OwnershipResult, LoweringResult, ScriptCompilationResult};

// ============================================================================
// Phase output types
// ============================================================================

/// Output from typecheck phase.
struct TypecheckOutput<'db> {
    result: UnitTypecheckResultTracked<'db>,
    expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
    call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
}

/// Output from ownership phase.
struct OwnershipOutput<'db> {
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: Option<ScriptAnalysisData<'db>>,
    /// Names this unit exports that hold no value.
    dead_exports: Vec<String>,
    /// Names from earlier units this unit assigned to.
    revived: Vec<String>,
}

/// Lowered functions from the lowering phase.
///
/// Functions are lowered once, then reused for const evaluation
/// and the final IR assembly.
#[derive(Clone)]
struct LoweredFunctions {
    /// Behind `Arc`s to match what module lowering produces, so the const
    /// evaluator takes one slice type from both paths.
    functions: Vec<std::sync::Arc<datalove_datafun_ir::IrCodeUnit>>,
    func_name_to_id: HashMap<String, datalove_datafun_ir::FuncId>,
}

/// Output from const evaluation phase.
struct ConstEvalOutput {
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

        for (salsa_module_id, parsed) in self.shared.parsed_graph.statements_only(db) {
            let module_path = salsa_module_id.path(db).clone();
            let module = self.shared.module_graph.iter_modules(db)
                .find(|m| m.id(db) == *salsa_module_id)
                .expect("module should exist in graph");
            let module_source = module.source(db);
            // A `ModuleSpec` owns its spans, so this is where they get built.
            // Building a script compiler is a deliberate act; compiling the
            // modules is not, which is why this no longer happens there.
            let spans = datalove_datafun_parser::module_spans(db, module).clone();
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
            skip_const_inlining: false,
            skip_specialization: false,
            shared_context: self.shared.clone(),
            auto_adapt_mode: AutoAdaptMode::Disabled,
            last_ownership_errors: Vec::new(),
            last_spans: None,
            dead_externals: Vec::new(),
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
        // Use CTFE evaluator with module registry for cross-module const function calls.
        let evaluator = Rc::new(RefCell::new(
            InterpCtfeEvaluator::with_module_registry(self.shared.module_registry.clone())
        ));
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
    /// When true, const bindings in functions are lowered as let bindings.
    skip_const_inlining: bool,
    /// When true, comptime calls are left naming the original function.
    skip_specialization: bool,
    /// Shared module context for cross-module CTFE function calls.
    shared_context: Arc<SharedModuleContext<'db>>,
    /// Auto-adapt mode for ownership analysis.
    auto_adapt_mode: AutoAdaptMode,
    /// Last ownership errors from compilation (for diagnostic rendering).
    last_ownership_errors: Vec<AnalysisError<'db>>,
    /// Spans for the last compilation unit (for diagnostic rendering).
    last_spans: Option<datalove_datafun_ast::spans::DatafunSpans<'db>>,
    /// Names earlier units exported without a value behind them.
    ///
    /// A unit copies out of the bindings earlier units own, so a name only
    /// ends up here when the unit that defined it gave the value away before
    /// it finished.
    dead_externals: Vec<String>,
}

impl<'db> ScriptCompiler<'db> {
    /// Compile a script fragment (statements).
    pub fn compile_fragment(&mut self, source: &str) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = &parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return result;
        }

        let stmts = parsed.statements.to_vec();
        let unit = ParsedUnit::Fragment { parsed: parsed.clone(), stmts };
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

    /// Emit ownership diagnostics from the last compilation.
    ///
    /// This should be called when ownership errors occurred to emit proper
    /// ariadne diagnostics with span information. Call this before displaying
    /// the error message.
    pub fn emit_ownership_diagnostics(&self) {
        if let Some(spans) = &self.last_spans {
            emit_ownership_diagnostics(self.db, &self.last_ownership_errors, spans);
        }
    }

    /// Get ownership diagnostics from the last compilation (salsa accumulator path).
    ///
    /// Note: This currently returns empty because ownership diagnostics are
    /// emitted outside of tracked functions. Use `get_ownership_errors()` and
    /// `get_last_spans()` for direct rendering instead.
    pub fn get_ownership_diagnostics(&self) -> Vec<&datalove_diagnostic::OwnershipDiagnostic> {
        Vec::new()
    }

    /// Get the structured ownership errors from the last compilation.
    ///
    /// Use with `get_last_spans()` for direct diagnostic rendering.
    pub fn get_ownership_errors(&self) -> &[AnalysisError] {
        &self.last_ownership_errors
    }

    /// Get the spans from the last compilation for diagnostic rendering.
    pub fn get_last_spans(&self) -> Option<&datalove_datafun_ast::spans::DatafunSpans> {
        self.last_spans.as_ref()
    }

    /// Get the database reference.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// Skip const parameter specialization.
    ///
    /// When enabled, a comptime call keeps naming the original function, which
    /// still takes the const argument. Used for differential testing.
    pub fn set_skip_specialization(&mut self, enabled: bool) {
        self.skip_specialization = enabled;
    }

    /// Skip compile-time const evaluation and inlining.
    ///
    /// When enabled, const bindings are evaluated at runtime instead of being
    /// replaced with literal values at compile time. This is useful for testing
    /// and debugging const expressions.
    pub fn set_skip_const_inlining(&mut self, enabled: bool) {
        self.skip_const_inlining = enabled;
    }

    /// Set auto-adapt mode for ownership analysis.
    ///
    /// When enabled, recoverable ownership errors (use-after-move, double-move,
    /// move-in-loop) are suppressed, treating them as if `@` was inserted.
    pub fn set_auto_adapt_mode(&mut self, mode: AutoAdaptMode) {
        self.auto_adapt_mode = mode;
    }

    /// Get current auto-adapt mode.
    pub fn auto_adapt_mode(&self) -> AutoAdaptMode {
        self.auto_adapt_mode
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

    /// Compile a parsed unit through the pipeline.
    ///
    /// Pipeline phases:
    /// 1. Analysis
    ///    a. Typecheck - type inference and checking
    ///    b. Ownership - borrow checking and drop scheduling
    /// 2. Lowering - generate IR (const bindings lowered as let bindings)
    /// 3. Const Evaluation - evaluate const expressions at compile time
    /// 4. Const Inlining - replace const bindings with evaluated values
    fn compile_unit_inner(
        &mut self,
        src: bct::input::Source,
        unit: ParsedUnit<'db>,
    ) -> ScriptCompilationResult {
        // Phase 1a: Typecheck
        let typecheck = match self.phase_typecheck(src, &unit) {
            Ok(tc) => tc,
            Err(result) => return result,
        };

        // Phase 1b: Ownership Analysis
        let ownership = match self.phase_ownership(&unit, &typecheck) {
            Ok(own) => own,
            Err(result) => return result,
        };

        // Phase 2: Lowering
        // Functions are lowered first, then reused for const evaluation.
        let lowered_funcs = match self.phase_lower_functions(&unit, &typecheck, &ownership) {
            Ok(lf) => lf,
            Err(result) => return result,
        };

        // Phase 3: Const Evaluation
        // Evaluates const expressions using the lowered functions + interpretation.
        let consts = match self.phase_const_eval(&unit, &typecheck, &lowered_funcs) {
            Ok(c) => c,
            Err(result) => return result,
        };

        // Phase 2 (continued): Assemble final IR
        // Combines lowered functions with script-level code.
        let ir_unit = match self.phase_assemble_ir(&unit, &typecheck, &ownership, &consts, &lowered_funcs) {
            Ok(ir) => ir,
            Err(result) => return result,
        };

        // Phase 4: Const Inlining
        // Replaces const initializer expressions with their evaluated values.
        // Skipped in skip_const_inlining mode (consts evaluated at runtime).
        let ir_unit = self.phase_const_inline(ir_unit, &consts);
        let ir_unit = self.phase_resolve_shape_descriptors(ir_unit);

        // Phase 5: Const parameter specialization.
        let ir_unit = match self.phase_specialize(ir_unit, &unit, &typecheck, &lowered_funcs) {
            Ok(ir) => ir,
            Err(result) => {
                self.accumulated_unit_specs.pop();
                return result;
            }
        };

        // Update accumulated state
        self.update_accumulated_state(&ir_unit, &ownership);

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

        // Run typechecking. The mode has to reach here too, not just ownership
        // analysis: a mismatch `@` would fix is a type error first.
        let batch_spec = create_batch_spec_with_auto_adapt(
            self.db,
            src,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
            self.auto_adapt_mode,
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
    // Phase 1b: Ownership Analysis
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
                    self.auto_adapt_mode,
                    self.dead_externals.clone(),
                );
                if !ownership_result.errors(self.db).is_empty() {
                    // Store structured errors and spans for CLI rendering.
                    self.last_ownership_errors = ownership_result.structured_errors(self.db).to_vec();
                    if let Some(unit_spec) = self.accumulated_unit_specs.last() {
                        self.last_spans = Some(unit_spec.spans.clone());
                    }
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    self.accumulated_unit_specs.pop();
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                let mut func_analyses = ownership_result.to_function_analyses_map(self.db, stmts);
                let mut script_analysis = ownership_result.script_analysis(self.db).clone();

                // Type adaptations are decided during typechecking, and lowering
                // needs them alongside the ones ownership analysis decided.
                let type_adapts = typecheck.result.adapt_sites(self.db);
                if !type_adapts.is_empty() {
                    if let Some(analysis) = script_analysis.as_mut() {
                        analysis.adapt_sites.extend(type_adapts);
                    }
                    for analysis in func_analyses.values_mut() {
                        analysis.adapt_sites.extend(type_adapts);
                    }
                }
                Ok(OwnershipOutput {
                    func_analyses,
                    script_analysis,
                    dead_exports: ownership_result.dead_exports(self.db).clone(),
                    revived: ownership_result.revived_exports(self.db).clone(),
                })
            }
            ParsedUnit::Expr(expr) => {
                let ownership_result = analyze_script_expr_tracked(
                    self.db,
                    typecheck.result,
                    *expr,
                    self.auto_adapt_mode,
                    self.dead_externals.clone(),
                );
                if !ownership_result.errors(self.db).is_empty() {
                    // Store structured errors and spans for CLI rendering.
                    self.last_ownership_errors = ownership_result.structured_errors(self.db).to_vec();
                    if let Some(unit_spec) = self.accumulated_unit_specs.last() {
                        self.last_spans = Some(unit_spec.spans.clone());
                    }
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
                    dead_exports: Vec::new(),
                    revived: Vec::new(),
                })
            }
        }
    }

    // ========================================================================
    // Phase 2a: Lower Functions
    // ========================================================================

    /// Lower functions to IR.
    ///
    /// Functions are lowered once, then reused for const evaluation
    /// and final IR assembly.
    fn phase_lower_functions(
        &mut self,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
    ) -> Result<LoweredFunctions, ScriptCompilationResult> {
        let ParsedUnit::Fragment { stmts, .. } = unit else {
            // Expressions don't have function definitions.
            return Ok(LoweredFunctions {
                functions: Vec::new(),
                func_name_to_id: HashMap::new(),
            });
        };

        // Get accumulated context for cross-unit function resolution.
        let script_ctx = self.accumulated_lower_bindings.to_script_lower_context();

        // Get module function ID map for resolving module function calls.
        let func_id_map = build_func_id_map(self.db, &self.module_specs);

        // Build func_param_types and func_return_types for type alias support.
        let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
        let mut func_return_types: HashMap<String, IrType> = HashMap::new();
        for (name, func_type) in typecheck.result.function_types(self.db) {
            let param_types: Vec<IrType> = func_type.param_types(self.db)
                .iter()
                .map(|ty| IrType::from_tycheck(self.db, ty))
                .collect();
            func_param_types.insert(name.text(self.db).S(), param_types);

            let return_type = IrType::from_tycheck(self.db, &func_type.return_type(self.db));
            func_return_types.insert(name.text(self.db).S(), return_type);
        }

        // Lower just the functions.
        match lower_script_functions(
            self.db,
            typecheck.expr_types,
            typecheck.call_targets,
            stmts,
            &ownership.func_analyses,
            Some(&func_param_types),
            Some(&func_return_types),
            &func_id_map,
            script_ctx,
        ) {
            Ok((functions, func_name_to_id)) => {
                let functions = functions.into_iter().map(std::sync::Arc::new).collect();
                Ok(LoweredFunctions { functions, func_name_to_id })
            }
            Err(e) => {
                self.accumulated_unit_specs.pop();
                Err(ScriptCompilationResult {
                    typecheck: TypecheckResult::Success,
                    ownership: OwnershipResult::Success,
                    lowering: LoweringResult::Error {
                        message: format!("function lowering error: {}", e),
                    },
                    ir_unit: None,
                })
            }
        }
    }

    // ========================================================================
    // Phase 3: Const Evaluation
    // ========================================================================

    /// Evaluate compile-time constants (fragment only).
    ///
    /// Uses the "lower then evaluate" pattern:
    /// 1. For each const binding in dependency order, lower it to IR or extract simple value
    /// 2. Evaluate the IR unit via CTFE if needed
    fn phase_const_eval(
        &mut self,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<ConstEvalOutput, ScriptCompilationResult> {
        let ParsedUnit::Fragment { stmts, .. } = unit else {
            // Expressions don't have const bindings.
            return Ok(ConstEvalOutput {
                resolved_consts: ResolvedConsts::new(),
                func_consts: HashMap::new(),
            });
        };

        // Collect const graph (memoized).
        let const_graph = collect_const_graph(self.db, stmts.clone(), typecheck.result);

        // Evaluate script-level consts.
        // Skip if skip_const_inlining is enabled - consts will be lowered as let bindings.
        let resolved_consts = if !self.skip_const_inlining && !const_graph.is_empty() {
            match self.evaluate_script_consts(
                &const_graph,
                stmts,
                typecheck.expr_types,
                typecheck.call_targets,
                lowered_funcs,
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
        // Skip if skip_const_inlining is enabled - consts will be lowered as let bindings.
        let func_consts_result = if !self.skip_const_inlining {
            self.evaluate_function_consts(
                stmts,
                typecheck.expr_types,
                typecheck.call_targets,
                &script_level_consts,
                lowered_funcs,
            )
        } else {
            ScriptFunctionConstsResult::empty()
        };

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
            resolved_consts,
            func_consts: func_consts_result.consts,
        })
    }

    /// Evaluate script-level const bindings using "lower then evaluate" pattern.
    fn evaluate_script_consts(
        &self,
        const_graph: &datalove_datafun_ir::ConstBindingGraph,
        statements: &[Statement<'db>],
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<ResolvedConsts, ConstEvalError> {
        let mut resolved = ResolvedConsts::new();
        let mut resolved_consts_map: HashMap<String, (IrType, ConstValue)> = HashMap::new();

        // A binding's id is its position among the const statements, so the
        // expressions only have to be gathered once.
        let const_exprs: Vec<_> = statements.iter()
            .filter_map(|s| match s {
                Statement::Const(c) => Some(c.value),
                _ => None,
            })
            .collect();

        for binding in &const_graph.bindings {
            let expr = const_exprs[binding.stmt_id.0 as usize];

            // Lower the const binding to get either a simple value or an IR unit.
            // Pass the module func_id_map for cross-module CTFE function calls.
            let (unit_opt, value_opt) = lower_const_binding(
                self.db,
                expr,
                &binding.ir_type,
                expr_types,
                call_targets,
                &resolved_consts_map,
                None, // Script-level consts don't have a function return type
                &lowered_funcs.functions,
                &lowered_funcs.func_name_to_id,
                Some(&self.shared_context.func_id_map),
            ).map_err(|e| ConstEvalError::LoweringFailed {
                binding_name: binding.name.clone(),
                message: e.to_string(),
            })?;

            // Evaluate to get the const value.
            let value = match (unit_opt, value_opt) {
                (None, Some(v)) => v,
                (Some(unit), None) => {
                    let prepared = PreparedConst::Unit(unit);
                    evaluate_prepared_const(&prepared, &binding.ir_type, &self.ctfe_evaluator)
                        .map_err(|e| self.ctfe_error_to_const_eval_error(e, &binding.name))?
                }
                _ => unreachable!("lower_const_binding returns exactly one of unit or value"),
            };

            resolved.insert(binding.stmt_id, binding.name.clone(), value.clone());
            resolved_consts_map.insert(binding.name.clone(), (binding.ir_type.clone(), value));
        }

        Ok(resolved)
    }

    /// Evaluate function-level const bindings using "lower then evaluate" pattern.
    fn evaluate_function_consts(
        &self,
        statements: &[Statement<'db>],
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        script_level_consts: &HashMap<String, (IrType, ConstValue)>,
        lowered_funcs: &LoweredFunctions,
    ) -> ScriptFunctionConstsResult {
        let mut consts = HashMap::new();
        let mut errors = Vec::new();

        for statement in statements {
            if let Statement::Fun(func_stmt) = statement {
                let func_name = func_stmt.name(self.db).text(self.db);
                // Get function's return type for early-return operators.
                let func_return_type = func_stmt.return_type(self.db)
                    .map(|ty| IrType::from_type_hint(self.db, &ty));
                // Track local consts for this function.
                let mut func_local_consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();

                // Names whose value this function does not have one of: its
                // const parameters, and the consts already deferred for naming
                // one, since those have no value here either.
                let mut deferred: std::collections::BTreeSet<String> = func_stmt.params(self.db)
                    .iter()
                    .filter(|p| p.is_comptime)
                    .map(|p| p.name.text(self.db).to_string())
                    .collect();

                for func_body_stmt in func_stmt.body(self.db).iter() {
                    if let Statement::Const(const_stmt) = func_body_stmt {
                        let name = const_stmt.name.text(self.db).to_string();
                        let init_expr = const_stmt.value;

                        // Get the type from the typechecker.
                        let key = datalove_datafun_ast::ast::ExprKey::of(self.db, init_expr);
                        let ir_type = match expr_types.get(&key) {
                            Some(ty) => IrType::from_tycheck(self.db, ty),
                            None => {
                                errors.push(format!("{}::{}: missing type information", func_name, name));
                                continue;
                            }
                        };

                        // Build lookup map: script-level + function-local consts.
                        let mut lookup_map = script_level_consts.clone();
                        for (local_name, (ty, val)) in &func_local_consts {
                            lookup_map.insert(local_name.clone(), (ty.clone(), val.clone()));
                        }

                        // Lower the const binding.
                        // Pass the module func_id_map for cross-module CTFE function calls.
                        let lower_result = lower_const_binding(
                            self.db,
                            init_expr,
                            &ir_type,
                            expr_types,
                            call_targets,
                            &lookup_map,
                            func_return_type.clone(),
                            &lowered_funcs.functions,
                            &lowered_funcs.func_name_to_id,
                            Some(&self.shared_context.func_id_map),
                        );

                        let value = match lower_result {
                            Ok((None, Some(v))) => v,
                            Ok((Some(unit), None)) => {
                                let prepared = PreparedConst::Unit(unit);
                                match evaluate_prepared_const(&prepared, &ir_type, &self.ctfe_evaluator) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        errors.push(format!("{}::{}: {}", func_name, name, e));
                                        continue;
                                    }
                                }
                            }
                            Ok(_) => unreachable!("lower_const_binding returns exactly one of unit or value"),
                            // A const naming a const parameter has a value per
                            // instantiation rather than one, so there is nothing
                            // to evaluate until the copies are made. It lowers
                            // as an ordinary binding, and specialization
                            // evaluates it once the parameter has a value.
                            Err(datalove_datafun_compiler::lower::LowerError::BindingNotAvailable(ref missing))
                                if deferred.contains(missing) =>
                            {
                                deferred.insert(name.clone());
                                continue;
                            }
                            Err(e) => {
                                errors.push(format!("{}::{}: {}", func_name, name, e));
                                continue;
                            }
                        };

                        // Store locally for other consts in this function.
                        func_local_consts.insert(name.clone(), (ir_type.clone(), value.clone()));

                        // Store with qualified name for the result.
                        let qualified_name = format!("{}::{}", func_name, name);
                        consts.insert(qualified_name, (ir_type, value));
                    }
                }
            }
        }

        ScriptFunctionConstsResult { consts, errors }
    }

    /// Convert a CtfeError to a ConstEvalError.
    fn ctfe_error_to_const_eval_error(&self, e: CtfeError, binding_name: &str) -> ConstEvalError {
        match e {
            CtfeError::InterpError(msg) if msg.contains("gas") => {
                ConstEvalError::GasExpired { binding_name: binding_name.to_string() }
            }
            CtfeError::InterpError(msg) => {
                ConstEvalError::LoweringFailed {
                    binding_name: binding_name.to_string(),
                    message: msg,
                }
            }
            CtfeError::EarlyReturn(msg) => {
                ConstEvalError::EarlyReturn {
                    binding_name: binding_name.to_string(),
                    message: msg,
                }
            }
            CtfeError::UnsupportedType(ty) => {
                ConstEvalError::UnsupportedType {
                    binding_name: binding_name.to_string(),
                    type_name: ty,
                }
            }
        }
    }

    // ========================================================================
    // Phase 2b: Assemble Final IR
    // ========================================================================

    /// Assemble the final IR by combining lowered functions with script-level code.
    ///
    /// Const bindings are lowered as let bindings with their initializer expressions.
    /// Const inlining happens as a separate pass after IR assembly.
    fn phase_assemble_ir(
        &mut self,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
        _consts: &ConstEvalOutput,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<IrCodeUnit, ScriptCompilationResult> {
        let func_id_map = build_func_id_map(self.db, &self.module_specs);
        let script_ctx = self.accumulated_lower_bindings.to_script_lower_context();

        match unit {
            ParsedUnit::Fragment { stmts, .. } => {
                // Build func_param_types and func_return_types for type alias support.
                let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
                let mut func_return_types: HashMap<String, IrType> = HashMap::new();
                for (name, func_type) in typecheck.result.function_types(self.db) {
                    let param_types: Vec<IrType> = func_type.param_types(self.db)
                        .iter()
                        .map(|ty| IrType::from_tycheck(self.db, ty))
                        .collect();
                    func_param_types.insert(name.text(self.db).S(), param_types);

                    let return_type = IrType::from_tycheck(self.db, &func_type.return_type(self.db));
                    func_return_types.insert(name.text(self.db).S(), return_type);
                }

                let script_analysis = ownership.script_analysis.clone()
                    .expect("script_analysis required for fragment units");

                // Const bindings are lowered as let bindings.
                // Const inlining happens in phase_const_inline after IR assembly.
                // Pass lowered functions to avoid re-lowering them.
                let lowered_funcs_arg = if lowered_funcs.functions.is_empty() {
                    None
                } else {
                    Some((
                        lowered_funcs.functions.iter().map(|f| (**f).clone()).collect(),
                        lowered_funcs.func_name_to_id.clone(),
                    ))
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
                    Some(&func_return_types),
                    lowered_funcs_arg,
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
    // Phase 4: Const Inlining
    // ========================================================================

    /// Inline evaluated const values into the lowered IR.
    ///
    /// This replaces const initializer expressions with their pre-computed
    /// literal values. In skip_const_inlining mode, this is skipped so const
    /// expressions are evaluated at runtime instead of compile time.
    /// Say what each call in the script hands a generic that needs a
    /// descriptor, and give the script's own functions the shapes their
    /// callees need of them.
    ///
    /// The functions defined beside the script settled this among themselves
    /// while they were lowered, but only what they said to each other: a call
    /// into a module was not in front of them, because the modules are lowered
    /// elsewhere. Here both are to hand, so the closure is run again with the
    /// module functions and the natives standing as fixed points -- their
    /// signatures are settled and nothing here may add to them.
    ///
    /// A script has no type parameters of its own, so it never forwards one;
    /// everything it hands over is a descriptor for a type it named outright.
    fn phase_resolve_shape_descriptors(&self, mut ir_unit: IrCodeUnit) -> IrCodeUnit {
        use datalove_datafun_ir::{CodeRef, CodeUnitId, DescriptorShape, Instruction, IrModuleId};

        /// A function this closure has to name, whether beside the script or
        /// in a module.
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        enum Key {
            Local(u32),
            Module(u32, u32),
        }

        let registry = self.shared_context.module_registry.clone();
        let module_shapes = |module: IrModuleId, id: CodeUnitId| -> Vec<DescriptorShape> {
            let Some(unit) = registry.get_module_function_as_unit(module, id) else {
                return Vec::new();
            };
            // A native says this from its own context, having no function
            // context to say it in.
            if let Some(native) = unit.native_context() {
                return native.descriptor_shapes.clone();
            }
            unit.function_context()
                .map(|c| c.descriptor_shapes.clone())
                .unwrap_or_default()
        };
        let key_of = |code_ref: &CodeRef| -> Option<Key> {
            match code_ref {
                CodeRef::Local(id) => Some(Key::Local(id.0)),
                CodeRef::Module { module, id } => Some(Key::Module(module.0, id.0)),
                CodeRef::External { .. } => None,
            }
        };

        let mut shapes: HashMap<Key, Vec<DescriptorShape>> = HashMap::new();
        let mut calls: HashMap<Key, Vec<(Key, Vec<DescriptorShape>)>> = HashMap::new();

        for unit in &ir_unit.nested_units {
            let key = Key::Local(unit.id.0);
            shapes.insert(key, unit.function_context()
                .map(|c| c.descriptor_shapes.clone()).unwrap_or_default());
            let mut sites = Vec::new();
            for block in &unit.blocks {
                for instr in &block.instructions {
                    let Some((func, type_args)) = instr.call_target() else { continue };
                    // Every callee's set is wanted, whether or not this site
                    // binds anything: a module callee is a fixed point that has
                    // to be in `shapes` for the closure to see it.
                    if let CodeRef::Module { module, id } = func {
                        shapes.entry(Key::Module(module.0, id.0)).or_insert_with(
                            || module_shapes(*module, CodeUnitId(id.0)));
                    }
                    if type_args.is_empty() {
                        continue;
                    }
                    if let Some(callee) = key_of(func) {
                        sites.push((callee, type_args.to_vec()));
                    }
                }
            }
            calls.insert(key, sites);
        }

        // Refused only by a shape that grows without end, which the module path
        // reports against the module that wrote it; a script naming the same
        // functions gets the same answer there.
        let _ = datalove_datafun_ir::close_shapes(&calls, &mut shapes);

        for unit in &mut ir_unit.nested_units {
            let Some(shape_set) = shapes.get(&Key::Local(unit.id.0)) else { continue };
            if let datalove_datafun_ir::CodeUnitContext::Function(ctx) = &mut unit.context {
                ctx.descriptor_shapes = shape_set.clone();
            }
        }

        // A module function's set is read from the registry rather than from
        // `shapes`, which holds only the ones some nested unit happened to
        // call. The script's own body may call others.
        let callee_shapes = |code_ref: &CodeRef| -> Vec<DescriptorShape> {
            match code_ref {
                CodeRef::Local(id) => shapes.get(&Key::Local(id.0)).cloned().unwrap_or_default(),
                CodeRef::Module { module, id } => module_shapes(*module, CodeUnitId(id.0)),
                CodeRef::External { .. } => Vec::new(),
            }
        };
        for unit in &mut ir_unit.nested_units {
            let own = shapes.get(&Key::Local(unit.id.0)).cloned().unwrap_or_default();
            let _ = datalove_datafun_ir::set_call_descriptors(unit, &own, &callee_shapes);
        }
        // Failure here means the script would forward a type parameter, which
        // it has none of, so it cannot happen.
        let _ = datalove_datafun_ir::set_call_descriptors(
            &mut ir_unit, &[], &callee_shapes);
        ir_unit
    }

    // ========================================================================
    // Phase 5: Const Parameter Specialization
    // ========================================================================

    /// Give each comptime call in this unit a copy of its callee to run.
    ///
    /// The copies go in this unit's own `nested_units`, so a script line can
    /// name an instantiation no module call site asked for without the module
    /// it came from having to change. A call whose const argument did not
    /// survive as a constant -- under `skip_const_inlining` none of them do --
    /// keeps naming the original, which still takes it.
    ///
    /// This runs after the descriptors are resolved, so that a copy arrives
    /// with the ones its own module worked out and nothing here recomputes
    /// them against a script that has no type parameters of its own.
    /// Evaluate one function's const bindings for one instantiation.
    ///
    /// The mirror of what the module pipeline does for its own copies: seed the
    /// const parameters with what this instantiation passes, and every const in
    /// the body becomes evaluable by the same CTFE that evaluates every other
    /// const. Returns the values under their local names, which is how the
    /// copy's `const_values` records them.
    #[allow(clippy::too_many_arguments)]
    fn instantiation_consts_from(
        &self,
        statements: &[Statement<'db>],
        func_name: &str,
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        comptime_param_indices: &[usize],
        values: &[ConstValue],
        lowered: &[std::sync::Arc<IrCodeUnit>],
        func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    ) -> (HashMap<String, ConstValue>, Vec<String>) {
        let Some(func_stmt) = statements.iter().find_map(|stmt| match stmt {
            Statement::Fun(f) if f.name(self.db).text(self.db) == func_name => Some(f),
            _ => None,
        }) else {
            return (HashMap::new(), Vec::new());
        };

        let params = func_stmt.params(self.db);
        let mut seeded: HashMap<String, (IrType, ConstValue)> = HashMap::new();
        for (&param_idx, value) in comptime_param_indices.iter().zip(values.iter()) {
            let Some(param) = params.get(param_idx) else { continue };
            seeded.insert(
                param.name.text(self.db).to_string(),
                (datalove_datafun_ir::ir_type_of_const_value(value), value.clone()),
            );
        }

        let func_return_type = func_stmt.return_type(self.db)
            .map(|ty| IrType::from_type_hint(self.db, &ty));

        let mut evaluated = HashMap::new();
        let mut errors = Vec::new();
        for body_stmt in func_stmt.body(self.db).iter() {
            let Statement::Const(const_stmt) = body_stmt else { continue };
            let name = const_stmt.name.text(self.db).to_string();
            let key = datalove_datafun_ast::ast::ExprKey::of(self.db, const_stmt.value);
            let Some(ty) = expr_types.get(&key) else {
                errors.push(format!("{}::{}: missing type information", func_name, name));
                continue;
            };
            let ir_type = IrType::from_tycheck(self.db, ty);

            let lowered_const = lower_const_binding(
                self.db, const_stmt.value, &ir_type, expr_types, call_targets, &seeded,
                func_return_type.clone(), lowered, func_name_to_id,
                Some(&self.shared_context.func_id_map),
            );
            let value = match lowered_const {
                Ok((None, Some(v))) => v,
                Ok((Some(unit), None)) => {
                    match evaluate_prepared_const(
                        &PreparedConst::Unit(unit), &ir_type, &self.ctfe_evaluator)
                    {
                        Ok(v) => v,
                        Err(e) => {
                            errors.push(format!("{}::{}: {}", func_name, name, e));
                            continue;
                        }
                    }
                }
                Ok(_) => unreachable!("lower_const_binding returns exactly one of unit or value"),
                Err(e) => {
                    errors.push(format!("{}::{}: {}", func_name, name, e));
                    continue;
                }
            };

            seeded.insert(name.clone(), (ir_type, value.clone()));
            evaluated.insert(name, value);
        }

        (evaluated, errors)
    }

    fn phase_specialize(
        &self,
        ir_unit: IrCodeUnit,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<IrCodeUnit, ScriptCompilationResult> {
        use datalove_datafun_compiler::specialize::CalleeKey;

        if self.skip_specialization {
            return Ok(ir_unit);
        }

        let registry = self.shared_context.module_registry.clone();
        let module_unit = |module, id| registry.get_module_function_as_unit(module, id).cloned();

        // A copy's const bindings are evaluated here, where the parameters
        // finally have values. The source is the script's own statements for a
        // function defined beside it, and the module's for one it called into.
        let script_stmts: &[Statement<'db>] = match unit {
            ParsedUnit::Fragment { stmts, .. } => stmts.as_slice(),
            ParsedUnit::Expr(_) => &[],
        };
        let instantiation_consts = |callee: CalleeKey, indices: &[usize], values: &[ConstValue]| {
            match callee {
                CalleeKey::Local(id) => {
                    let Some(name) = lowered_funcs.func_name_to_id.iter()
                        .find(|(_, func_id)| func_id.0 == id.0)
                        .map(|(name, _)| name.clone())
                    else {
                        return (HashMap::new(), Vec::new());
                    };
                    self.instantiation_consts_from(
                        script_stmts, &name, typecheck.expr_types, typecheck.call_targets,
                        indices, values,
                        &lowered_funcs.functions, &lowered_funcs.func_name_to_id,
                    )
                }
                CalleeKey::Module(module, id) => {
                    let Some(original) = registry.get_module_function_as_unit(module, id) else {
                        return (HashMap::new(), Vec::new());
                    };
                    let name = original.name.clone();
                    // The module's own expressions were typechecked with the
                    // module graph, not with this script.
                    let Some((module_id, parsed)) = self.shared_context.parsed_graph
                        .statements_only(self.db)
                        .get(module.0 as usize)
                    else {
                        return (HashMap::new(), Vec::new());
                    };
                    let module_results = self.shared_context.graph_typecheck.module_results(self.db);
                    let Some(single) = module_results.get(module_id) else {
                        return (HashMap::new(), Vec::new());
                    };
                    let funcs: Vec<std::sync::Arc<IrCodeUnit>> = registry.iter_module_code_units_with_ids()
                        .filter(|((m, _), _)| *m == module)
                        .map(|(_, u)| std::sync::Arc::new(u.clone()))
                        .collect();
                    let names: HashMap<String, datalove_datafun_ir::FuncId> = funcs.iter()
                        .map(|u| (u.name.clone(), datalove_datafun_ir::FuncId(u.id.0)))
                        .collect();
                    self.instantiation_consts_from(
                        &parsed.statements, &name,
                        single.expr_types(self.db), single.call_targets(self.db),
                        indices, values, &funcs, &names,
                    )
                }
            }
        };

        let (ir_unit, errors) = datalove_datafun_compiler::specialize::specialize_script_unit(
            &ir_unit, &module_unit, &instantiation_consts);

        if !errors.is_empty() {
            return Err(ScriptCompilationResult {
                typecheck: TypecheckResult::Success,
                ownership: OwnershipResult::Success,
                lowering: LoweringResult::Error { message: errors.join("; ") },
                ir_unit: None,
            });
        }

        Ok(ir_unit)
    }

    fn phase_const_inline(
        &self,
        ir_unit: IrCodeUnit,
        consts: &ConstEvalOutput,
    ) -> IrCodeUnit {
        // In skip_const_inlining mode, skip inlining - consts are evaluated at runtime.
        if self.skip_const_inlining {
            return ir_unit;
        }

        // Build the const values map for inlining.
        // Combine script-level and function-level consts.
        let mut const_values: HashMap<String, ConstValue> = HashMap::new();

        // Add script-level consts.
        for (name, value) in consts.resolved_consts.iter() {
            const_values.insert(name.to_string(), value.clone());
        }

        // Add function-level consts (already qualified as "func_name::const_name").
        for (qualified_name, (_, value)) in &consts.func_consts {
            // Extract the local const name for function-level consts.
            // The key in func_consts is "func_name::const_name".
            if let Some(pos) = qualified_name.rfind("::") {
                let local_name = &qualified_name[pos + 2..];
                const_values.insert(local_name.to_string(), value.clone());
            }
        }

        // Run the const inlining pass.
        inline_script_consts(ir_unit, &const_values)
    }

    // ========================================================================
    // Accumulated State
    // ========================================================================

    /// Update accumulated state after successful compilation.
    fn update_accumulated_state(&mut self, ir_unit: &IrCodeUnit, ownership: &OwnershipOutput<'db>) {
        // A name is live again if this unit exported or assigned to it, and
        // dead if this unit gave away what it exported.
        if let Some(script_ctx) = ir_unit.script_context() {
            for (name, _) in &script_ctx.exports {
                self.dead_externals.retain(|dead| dead != name);
            }
        }
        for name in &ownership.revived {
            self.dead_externals.retain(|dead| dead != name);
        }
        self.dead_externals.extend(ownership.dead_exports.iter().cloned());

        let unit_index = self.accumulated_lower_bindings.current_unit;
        if let Some(script_ctx) = ir_unit.script_context() {
            self.accumulated_lower_bindings.add_exports(
                unit_index,
                &script_ctx.exports,
                &ir_unit.value_types,
                &ir_unit.slot_types,
            );
        }
    }
}
