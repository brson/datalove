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
//! let result = compiler.compile_fragment("let x = 42", false);
//! if let Some(ir_unit) = result.ir_unit {
//!     // Pass to executor for execution.
//! }
//!
//! // Later compilation can reference x.
//! let result2 = compiler.compile_fragment("let y = x + 1", false);
//! ```

use rmx::prelude::*;

use datalove_datafun_ast::ast::{ExprFun, ParsedStatements, Statement};
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

use super::compiled_modules::CompiledModules;
use super::result::{TypecheckResult, LoweringResult, ScriptCompilationResult};

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
        })
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
}

impl<'db> ScriptCompiler<'db> {
    /// Compile a script fragment (statements).
    ///
    /// If `for_aot` is true, emits drops for script-level bindings (for AOT compilation).
    pub fn compile_fragment(&mut self, source: &str, for_aot: bool) -> ScriptCompilationResult {
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

        ScriptCompilationResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }
}
