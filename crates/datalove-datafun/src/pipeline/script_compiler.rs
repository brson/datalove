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

use salsa::plumbing::AsId;
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
};
use datalove_datafun_compiler::lower::ScriptFunctionAnalyses;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, CtfeError, IrScriptUnit, IrType, ResolvedConsts, ConstEvalError};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::{
    type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked, ResolvedCallTarget, Type,
};
use datalove_datafun_resolve::resolve_script_names;
use datalove_datafun_compiler::IrTypeExt;

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

/// Lowered functions from the lowering phase.
///
/// Functions are lowered once, then reused for const evaluation
/// and the final IR assembly.
#[derive(Clone)]
struct LoweredFunctions {
    functions: Vec<datalove_datafun_ir::IrFunction>,
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
            skip_const_inlining: false,
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
    /// When true, const bindings in functions are lowered as let bindings.
    skip_const_inlining: bool,
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

    /// Skip compile-time const evaluation and inlining.
    ///
    /// When enabled, const bindings are evaluated at runtime instead of being
    /// replaced with literal values at compile time. This is useful for testing
    /// and debugging const expressions.
    pub fn set_skip_const_inlining(&mut self, enabled: bool) {
        self.skip_const_inlining = enabled;
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

        // Build func_param_types for type alias support.
        let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
        for (name, func_type) in typecheck.result.function_types(self.db) {
            let param_types: Vec<IrType> = func_type.param_types(self.db)
                .iter()
                .map(|ty| IrType::from_tycheck(self.db, ty))
                .collect();
            func_param_types.insert(name.text(self.db).S(), param_types);
        }

        // Lower just the functions.
        match lower_script_functions(
            self.db,
            typecheck.expr_types,
            typecheck.call_targets,
            stmts,
            &ownership.func_analyses,
            Some(&func_param_types),
            &func_id_map,
            script_ctx,
        ) {
            Ok((functions, func_name_to_id)) => {
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
        expr_types: &'db [Option<Type<'db>>],
        call_targets: &'db [Option<ResolvedCallTarget<'db>>],
        lowered_funcs: &LoweredFunctions,
    ) -> Result<ResolvedConsts, ConstEvalError> {
        let mut resolved = ResolvedConsts::new();
        let mut resolved_consts_map: HashMap<String, (IrType, ConstValue)> = HashMap::new();

        for binding in &const_graph.bindings {
            // Find the expression for this binding.
            let expr = statements.iter()
                .find_map(|s| match s {
                    Statement::Const(c) if c.value.as_id() == binding.stmt_id => Some(c.value),
                    _ => None,
                })
                .expect("const binding expression not found");

            // Lower the const binding to get either a simple value or an IR unit.
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
        expr_types: &'db [Option<Type<'db>>],
        call_targets: &'db [Option<ResolvedCallTarget<'db>>],
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

                for func_body_stmt in func_stmt.body(self.db).iter() {
                    if let Statement::Const(const_stmt) = func_body_stmt {
                        let name = const_stmt.name.text(self.db).to_string();
                        let init_expr = const_stmt.value;

                        // Get the type from the typechecker.
                        let expr_id = init_expr.as_id();
                        let index = expr_id.index() as usize;
                        let ir_type = match expr_types.get(index).cloned().flatten() {
                            Some(ty) => IrType::from_tycheck(self.db, &ty),
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

                // Const bindings are lowered as let bindings.
                // Const inlining happens in phase_const_inline after IR assembly.
                // Pass lowered functions to avoid re-lowering them.
                let lowered_funcs_arg = if lowered_funcs.functions.is_empty() {
                    None
                } else {
                    Some((lowered_funcs.functions.clone(), lowered_funcs.func_name_to_id.clone()))
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
    fn phase_const_inline(
        &self,
        ir_unit: IrScriptUnit,
        consts: &ConstEvalOutput,
    ) -> IrScriptUnit {
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
