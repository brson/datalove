//! Type checking context.
//!
//! Provides TypeContext for tracking bindings and errors during typechecking,
//! and ScriptTypeContext for tracking accumulated bindings across script units.

use rmx::prelude::*;
use std::collections::{BTreeMap, HashMap};
use bct::text::{InternedText, TextSpan};

use datalove_datafun_ast::ast::*;
use datalove_datafun_intrinsics::IntrinsicId;

pub use datalove_datafun_ast::spans::DatafunSpans;
use crate::{CallTargets, ExprTypes};
pub use bct::module_graph::ModuleId;

pub use crate::{
    PendingDiagnostic,
    RecoveryHint,
    Type,
    TypeFunction,
    TypeError,
    ResolvedCallTarget,
    ModuleNameResolution,
    CollectedNames,
    AutoAdaptMode,
};

pub use crate::types::ComptimeCallSiteRegistry;
use datalove_datafun_common::can_clone_coerce_to;
use std::collections::HashSet;

/// Context for typechecking.
pub struct TypeContext<'db> {
    pub(crate) db: &'db dyn crate::Db,
    /// Pre-computed spans for error reporting.
    pub(crate) spans: DatafunSpans<'db>,
    /// Current module being typechecked (for pending diagnostics).
    pub(crate) current_module_id: Option<ModuleId<'db>>,
    /// Variable bindings (name -> (type, is_mutable)).
    pub(crate) variables: HashMap<InternedText<'db>, (Type<'db>, bool)>,
    /// Function signatures (name -> function type).
    pub(crate) functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Function ASTs for resolving call targets (name -> (AST, module_id)).
    pub(crate) function_asts: HashMap<InternedText<'db>, (StmtFun<'db>, Option<ModuleId<'db>>)>,
    /// Type aliases (name -> resolved type).
    pub(crate) type_aliases: HashMap<InternedText<'db>, Type<'db>>,
    /// Expected return type for current function (if inside a function).
    pub(crate) expected_return_type: Option<Type<'db>>,
    /// Whether current function has no declared return type (void function).
    pub(crate) is_void_function: bool,
    pub(crate) errors: Vec<TypeError>,
    /// Pending diagnostics for post-hoc span enrichment.
    pub(crate) pending_diagnostics: Vec<PendingDiagnostic<'db>>,
    /// Expression types, indexed by ExprFun ID.
    pub(crate) expr_types: ExprTypes<'db>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    pub(crate) call_targets: CallTargets<'db>,
    /// Stack of loop depth (for validating break/continue are inside a loop).
    pub(crate) loop_depth: usize,
    /// Whether we're in a reference context (ref/mut/out param or binop operand).
    /// Move-type field projections are allowed in ref context.
    pub(crate) ref_context: bool,
    /// Whether we're in a mutable binding context (mut/out param).
    /// View types (e.g. tensor sub-views) are not allowed in mut context.
    pub(crate) mut_context: bool,
    /// Resolved intrinsic targets, keyed by the expression's stable key.
    pub(crate) intrinsic_targets: BTreeMap<ExprKey<'db>, IntrinsicId>,
    /// Auto-adapt mode for this context.
    pub(crate) auto_adapt_mode: AutoAdaptMode,
    /// Auto-adaptations that were applied (for reporting and IR lowering).
    pub(crate) auto_adaptations: Vec<AutoAdaptation<'db>>,
    /// Names of const bindings (for validating comptime args).
    pub(crate) const_bindings: HashSet<InternedText<'db>>,
    /// Whether we are checking the value of a const binding.
    ///
    /// A const expression may only name other consts, since it is evaluated
    /// before anything a parameter or a let could be bound to exists.
    pub(crate) in_const_expr: bool,
    /// Registry of comptime call sites and functions.
    pub(crate) comptime_registry: ComptimeCallSiteRegistry<'db>,
}

/// Record of an auto-adaptation that was applied.
#[derive(Clone)]
pub struct AutoAdaptation<'db> {
    /// The expression that was adapted.
    pub expr_key: ExprKey<'db>,
    /// The original type of the expression.
    pub from_type: Type<'db>,
    /// The type after adaptation.
    pub to_type: Type<'db>,
}

impl<'db> TypeContext<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        spans: DatafunSpans<'db>,
    ) -> Self {
        Self::with_options(db, spans, None, AutoAdaptMode::Disabled)
    }

    /// Create a TypeContext for a specific module.
    pub fn with_module_id(
        db: &'db dyn crate::Db,
        spans: DatafunSpans<'db>,
        module_id: Option<ModuleId<'db>>,
    ) -> Self {
        Self::with_options(db, spans, module_id, AutoAdaptMode::Disabled)
    }

    /// Create a TypeContext with all options.
    pub fn with_options(
        db: &'db dyn crate::Db,
        spans: DatafunSpans<'db>,
        module_id: Option<ModuleId<'db>>,
        auto_adapt_mode: AutoAdaptMode,
    ) -> Self {
        TypeContext {
            db,
            spans,
            current_module_id: module_id,
            variables: HashMap::new(),
            functions: HashMap::new(),
            function_asts: HashMap::new(),
            type_aliases: HashMap::new(),
            expected_return_type: None,
            is_void_function: false,
            errors: Vec::new(),
            pending_diagnostics: Vec::new(),
            expr_types: ExprTypes::new(),
            call_targets: CallTargets::new(),
            loop_depth: 0,
            ref_context: false,
            mut_context: false,
            intrinsic_targets: BTreeMap::new(),
            auto_adapt_mode,
            auto_adaptations: Vec::new(),
            const_bindings: HashSet::new(),
            in_const_expr: false,
            comptime_registry: ComptimeCallSiteRegistry::new(),
        }
    }

    /// Get the auto-adapt mode.
    pub fn auto_adapt_mode(&self) -> AutoAdaptMode {
        self.auto_adapt_mode
    }

    /// Get the auto-adaptations that were applied.
    pub fn auto_adaptations(&self) -> &[AutoAdaptation<'db>] {
        &self.auto_adaptations
    }

    /// Get the pending diagnostics.
    pub fn pending_diagnostics(&self) -> &[PendingDiagnostic<'db>] {
        &self.pending_diagnostics
    }

    /// Get the type errors.
    pub fn errors(&self) -> &[TypeError] {
        &self.errors
    }

    pub fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    /// Run `attempt`, discarding anything it reported if it does not succeed.
    ///
    /// For speculative checks that fall back to another way of typing the same
    /// expression. Without this the abandoned attempt still reports, and the
    /// reader sees the failure of a path the compiler chose not to take
    /// alongside the error it settled on.
    pub fn try_check<R>(
        &mut self,
        attempt: impl FnOnce(&mut Self) -> Option<R>,
    ) -> Option<R> {
        let errors = self.errors.len();
        let diagnostics = self.pending_diagnostics.len();
        match attempt(self) {
            Some(result) => Some(result),
            None => {
                self.errors.truncate(errors);
                self.pending_diagnostics.truncate(diagnostics);
                None
            }
        }
    }

    // Error collection helpers that collect pending diagnostics and return TypeError.
    // Diagnostics are emitted at the end of typechecking via emit_pending_diagnostics().

    /// F001: Undefined variable.
    pub fn error_undefined_variable(&mut self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedVariable {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            name,
        });
        TypeError::UnresolvedName(name.as_str(self.db).S())
    }

    /// F059: A name imported twice.
    ///
    /// Filed against the second import, which is the one that would have
    /// taken over a name the first had already bound.
    pub fn error_duplicate_import(
        &mut self,
        local_index: u32,
        name: InternedText<'db>,
        first: &str,
        second: &str,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::DuplicateImport {
            local_index,
            module_id: self.current_module_id,
            name,
            first: InternedText::new(self.db, first.S()),
        });
        TypeError::DuplicateImport {
            name: name.as_str(self.db).S(),
            first: first.S(),
            second: second.S(),
        }
    }

    /// F002: Undefined function.
    pub fn error_undefined_function(&mut self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedFunction {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            name,
        });
        TypeError::UnresolvedName(name.as_str(self.db).S())
    }

    /// F011: Cannot synthesize type.
    pub fn error_cannot_synthesize(&mut self, expr: ExprFun<'db>, message: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::CannotSynthesize {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            message: InternedText::new(self.db, message.S()),
        });
        TypeError::CannotSynthesize
    }

    /// F016: Type mismatch (simple version without recovery hint).
    pub fn error_type_mismatch(&mut self, expr: ExprFun<'db>, expected: &str, actual: &str, label: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
            label: InternedText::new(self.db, label.S()),
            recovery_hint: RecoveryHint::None,
        });
        TypeError::TypeMismatch {
            expected: expected.S(),
            actual: actual.S(),
        }
    }

    /// Try to auto-adapt a type mismatch, or return an error.
    ///
    /// If auto_adapt_mode is enabled and the mismatch is recoverable via `@`,
    /// records the adaptation and returns Ok(()). Otherwise, creates a pending
    /// diagnostic (with recovery hint if applicable) and returns Err.
    ///
    /// This is the main entry point for handling type mismatches in expressions.
    pub fn check_type_mismatch_or_adapt(
        &mut self,
        expr: ExprFun<'db>,
        expected: &Type<'db>,
        actual: &Type<'db>,
        label: &str,
    ) -> Result<(), TypeError> {
        // Check if @ can fix this mismatch.
        let can_adapt = can_clone_coerce_to(actual, expected, self.db);

        if can_adapt && self.auto_adapt_mode.is_enabled() {
            // Auto-adapt: record the adaptation and store the expected type.
            self.auto_adaptations.push(AutoAdaptation {
                expr_key: ExprKey::of(self.db, expr),
                from_type: actual.clone(),
                to_type: expected.clone(),
            });

            // Store the adapted (expected) type for this expression.
            self.store_expr_type(expr, expected);

            // Optionally report what was adapted.
            if self.auto_adapt_mode.should_report() {
                use datalove_datafun_common::type_to_string;
                let from_str = type_to_string(self.db, actual);
                let to_str = type_to_string(self.db, expected);
                // TODO: Emit an info-level diagnostic for the adaptation.
                // For now, we just silently adapt.
                let _ = (from_str, to_str);
            }

            return Ok(());
        }

        // Cannot auto-adapt (or auto-adapt disabled): emit error with recovery hint.
        Err(self.error_type_mismatch_with_types(expr, expected, actual, label))
    }

    /// F016: Type mismatch with recovery hint computation.
    ///
    /// This version takes Type references and checks if the error is recoverable
    /// via the @ (adapt) operator using can_clone_coerce_to.
    pub fn error_type_mismatch_with_types(
        &mut self,
        expr: ExprFun<'db>,
        expected: &Type<'db>,
        actual: &Type<'db>,
        label: &str,
    ) -> TypeError {
        use datalove_datafun_common::type_to_string;

        let expected_str = type_to_string(self.db, expected);
        let actual_str = type_to_string(self.db, actual);

        // Compute recovery hint: can @ fix this mismatch?
        let recovery_hint = if can_clone_coerce_to(actual, expected, self.db) {
            let description = format_adapt_description(self.db, actual, expected);
            RecoveryHint::InsertAdapt { description }
        } else {
            RecoveryHint::None
        };

        self.pending_diagnostics.push(PendingDiagnostic::TypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected_str.C()),
            actual: InternedText::new(self.db, actual_str.C()),
            label: InternedText::new(self.db, label.S()),
            recovery_hint,
        });
        TypeError::TypeMismatch {
            expected: expected_str,
            actual: actual_str,
        }
    }

    /// F045: Function arity mismatch.
    pub fn error_arity_mismatch(
        &mut self,
        expr: ExprFun<'db>,
        func_name: InternedText<'db>,
        expected: usize,
        actual: usize,
    ) -> TypeError {
        let (func_local_index, func_module_id) = self.lookup_function_ast(func_name)
            .map(|(func_ast, mod_id)| (func_ast.local_index(self.db), mod_id))
            .unwrap_or((0, None));

        self.pending_diagnostics.push(PendingDiagnostic::ArityMismatch {
            call_expr_key: ExprKey::of(self.db, expr),
            call_module_id: self.current_module_id,
            func_name,
            func_local_index,
            func_module_id,
            expected,
            actual,
        });
        TypeError::ArityMismatch { expected, actual }
    }

    /// F046: Result destructuring requires error binding.
    pub fn error_result_requires_binding(&mut self, expr: ExprFun<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::ResultRequiresBinding {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
        });
        TypeError::ResultRequiresErrorBinding
    }

    /// F026: Invalid operand type for operator.
    pub fn error_invalid_operand_type(&mut self, expr: ExprFun<'db>, op: &str, ty: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::InvalidOperandType {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            op: InternedText::new(self.db, op.S()),
            ty: InternedText::new(self.db, ty.S()),
        });
        TypeError::InvalidOperandType {
            op: op.S(),
            ty: ty.S(),
        }
    }

    /// F048: Try operator type mismatch.
    pub fn error_try_type_mismatch(&mut self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TryTypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            operator: InternedText::new(self.db, operator.S()),
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
        });
        TypeError::TryTypeMismatch {
            operator: operator.S(),
            actual_type: actual.S(),
        }
    }

    /// F049: Try operator return type mismatch.
    pub fn error_try_return_type_mismatch(&mut self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TryReturnTypeMismatch {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            operator: InternedText::new(self.db, operator.S()),
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
        });
        TypeError::TryReturnTypeMismatch {
            operator: operator.S(),
            return_type: actual.S(),
        }
    }

    /// Look up span for a datafun expression.
    pub fn get_span(&self, expr: ExprFun<'db>) -> Option<TextSpan<'db>> {
        self.spans.lookup(self.db, expr).map(|entry| {
            let (text, span) = entry.to_text_and_span(self.db);
            TextSpan::new(text, span)
        })
    }

    /// Emit all pending diagnostics using local spans.
    ///
    /// Called at the end of typechecking for non-module-graph paths.
    pub fn emit_pending_diagnostics(&self) {
        let span_lookup = crate::emit::LocalSpanLookup::new(&self.spans);
        crate::emit::emit_pending_diagnostics(self.db, &self.pending_diagnostics, &span_lookup);
    }

    /// F050: Break outside loop.
    pub fn error_break_outside_loop(&mut self, stmt: &StmtBreak) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::BreakOutsideLoop {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::BreakOutsideLoop
    }

    /// F051: Continue outside loop.
    pub fn error_continue_outside_loop(&mut self, stmt: &StmtContinue) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::ContinueOutsideLoop {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::ContinueOutsideLoop
    }

    /// F052: Void function cannot return a value.
    pub fn error_void_function_returns_value(&mut self, stmt: &StmtRet) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::VoidFunctionReturnsValue {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::VoidFunctionReturnsValue
    }

    /// F053: Non-void function requires return value.
    pub fn error_function_requires_return_value(&mut self, stmt: &StmtRet) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::FunctionRequiresReturnValue {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
        });
        TypeError::FunctionRequiresReturnValue
    }

    /// F054: Undefined variable in set statement.
    pub fn error_undefined_variable_set(&mut self, stmt: &StmtSet, name: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedVariableSet {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
            name: InternedText::new(self.db, name.S()),
        });
        TypeError::UndefinedVariable
    }

    /// F055: Cannot assign to immutable variable.
    pub fn error_variable_not_mutable(&mut self, stmt: &StmtSet, name: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::VariableNotMutable {
            local_index: stmt.local_index,
            module_id: self.current_module_id,
            name: InternedText::new(self.db, name.S()),
        });
        TypeError::VariableNotMutable
    }

    /// F058: A const expression referenced a binding that is not itself const.
    pub fn error_non_const_in_const_expr(
        &mut self,
        expr: ExprFun<'db>,
        name: InternedText<'db>,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::NonConstInConstExpr {
            expr_key: ExprKey::of(self.db, expr),
            module_id: self.current_module_id,
            name,
        });
        TypeError::NonConstInConstExpr(name.as_str(self.db).S())
    }

    /// F057: Call-site mode marker disagrees with the declared parameter mode.
    pub fn error_argument_mode_mismatch(
        &mut self,
        arg: ExprFun<'db>,
        param_idx: usize,
        expected: &str,
        found: &str,
    ) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::ArgumentModeMismatch {
            expr_key: ExprKey::of(self.db, arg),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected.S()),
            found: InternedText::new(self.db, found.S()),
        });
        TypeError::ArgumentModeMismatch {
            param_idx,
            expected: expected.S(),
            found: found.S(),
        }
    }

    /// Check if we're at module top level (not inside a function).
    pub fn is_module_top_level(&self) -> bool {
        self.current_module_id.is_some() && self.expected_return_type.is_none()
    }

    pub fn add_variable(&mut self, name: InternedText<'db>, ty: Type<'db>, is_mutable: bool) {
        self.variables.insert(name, (ty, is_mutable));
    }

    /// Add a function signature only (for imports where AST comes from another module).
    pub fn add_function(&mut self, name: InternedText<'db>, func_type: TypeFunction<'db>) {
        self.functions.insert(name, func_type);
    }

    /// Add a function with its AST (for local function definitions).
    pub fn add_function_with_ast(
        &mut self,
        name: InternedText<'db>,
        func_type: TypeFunction<'db>,
        func_ast: StmtFun<'db>,
        module_id: Option<ModuleId<'db>>,
    ) {
        self.functions.insert(name, func_type);
        self.function_asts.insert(name, (func_ast, module_id));
    }

    /// Add an imported function with its resolved AST.
    pub fn add_imported_function(
        &mut self,
        name: InternedText<'db>,
        func_type: TypeFunction<'db>,
        func_ast: StmtFun<'db>,
        source_module_id: ModuleId<'db>,
    ) {
        self.functions.insert(name, func_type);
        self.function_asts.insert(name, (func_ast, Some(source_module_id)));
    }

    /// Execute a closure in a new variable scope.
    ///
    /// Saves and restores the variable bindings around the closure,
    /// so any variables added inside are not visible outside.
    pub fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.variables.clone();
        let result = f(self);
        self.variables = saved;
        result
    }

    pub fn lookup_variable(&self, name: InternedText<'db>) -> Option<Type<'db>> {
        self.variables.get(&name).map(|(ty, _)| ty.clone())
    }

    pub fn lookup_variable_mutability(&self, name: InternedText<'db>) -> Option<bool> {
        self.variables.get(&name).map(|(_, is_mutable)| *is_mutable)
    }

    pub fn lookup_function(&self, name: InternedText<'db>) -> Option<TypeFunction<'db>> {
        self.functions.get(&name).copied()
    }

    /// Add a type alias to the context.
    pub fn add_type_alias(&mut self, name: InternedText<'db>, ty: Type<'db>) {
        self.type_aliases.insert(name, ty);
    }

    /// Look up a type alias by name.
    pub fn lookup_type_alias(&self, name: InternedText<'db>) -> Option<Type<'db>> {
        self.type_aliases.get(&name).cloned()
    }

    /// Look up the resolved function AST by name.
    pub fn lookup_function_ast(&self, name: InternedText<'db>) -> Option<(StmtFun<'db>, Option<ModuleId<'db>>)> {
        self.function_asts.get(&name).copied()
    }

    /// Store resolved call target for a function call expression.
    pub fn store_call_target(&mut self, call: ExprFunctionCall<'db>, func: StmtFun<'db>, module_id: Option<ModuleId<'db>>) {
        self.call_targets.insert(
            ExprKey::of_call(self.db, call),
            ResolvedCallTarget::new(self.db, func, module_id),
        );
    }

    /// Store the type for an expression.
    pub fn store_expr_type(&mut self, expr: ExprFun<'db>, ty: &Type<'db>) {
        self.expr_types.insert(ExprKey::of(self.db, expr), ty.clone());
    }

    /// Synthesize the type of an expression.
    pub fn synthesize_expr(&mut self, expr: ExprFun<'db>) -> Result<Type<'db>, TypeError> {
        let ty = crate::synthesize::synthesize_expr(self, expr)?;
        self.store_expr_type(expr, &ty);
        Ok(ty)
    }

    /// Store resolved intrinsic target for an intrinsic call expression.
    ///
    /// Keyed like [`Self::store_expr_type`] rather than indexed by the
    /// expression's salsa id. A `Vec` indexed by `id.index()` grew to the
    /// highest index salsa had handed out - 3764 slots to hold 8 targets in
    /// one measured fixture - and the index alone drops the generation that
    /// tells two expressions in a reused slot apart.
    pub fn store_intrinsic_target(&mut self, expr: ExprFun<'db>, intrinsic: IntrinsicId) {
        self.intrinsic_targets.insert(ExprKey::of(self.db, expr), intrinsic);
    }

    /// Get resolved intrinsic target for an expression.
    pub fn get_intrinsic_target(&self, expr: ExprFun<'db>) -> Option<IntrinsicId> {
        self.intrinsic_targets.get(&ExprKey::of(self.db, expr)).copied()
    }

    // ========================================================================
    // Comptime Support
    // ========================================================================

    /// Add a const binding to track.
    pub fn add_const_binding(&mut self, name: InternedText<'db>) {
        self.const_bindings.insert(name);
    }

    /// Check if a name is a const binding.
    pub fn is_const_binding(&self, name: InternedText<'db>) -> bool {
        self.const_bindings.contains(&name)
    }

    /// Get a reference to the comptime registry.
    pub fn comptime_registry(&self) -> &ComptimeCallSiteRegistry<'db> {
        &self.comptime_registry
    }

    /// Get a mutable reference to the comptime registry.
    pub fn comptime_registry_mut(&mut self) -> &mut ComptimeCallSiteRegistry<'db> {
        &mut self.comptime_registry
    }

    /// Take ownership of the comptime registry.
    pub fn take_comptime_registry(&mut self) -> ComptimeCallSiteRegistry<'db> {
        std::mem::take(&mut self.comptime_registry)
    }

    /// Seed context from a pre-computed name resolution.
    ///
    /// Populates type aliases, function signatures, and function ASTs from
    /// the memoized name resolution pass. This replaces the inline Pass 0/1
    /// collection in typecheck_module.
    pub fn seed_from_name_resolution(
        &mut self,
        name_resolution: &ModuleNameResolution<'db>,
        module_id: Option<ModuleId<'db>>,
    ) {
        self.record_resolution_errors(name_resolution.errors(self.db), module_id);

        // Add type aliases.
        for (name, ty) in name_resolution.type_aliases(self.db) {
            self.type_aliases.insert(*name, ty.clone());
        }

        // Add function signatures with ASTs.
        // First build a map of ASTs for lookup.
        let ast_map: HashMap<bct::text::InternedText<'db>, StmtFun<'db>> = name_resolution.function_asts(self.db)
            .iter()
            .map(|(name, ast)| (*name, *ast))
            .collect();

        for (name, func_type) in name_resolution.functions(self.db) {
            if let Some(&func_ast) = ast_map.get(name) {
                self.add_function_with_ast(*name, *func_type, func_ast, module_id);
            } else {
                self.add_function(*name, *func_type);
            }
        }
    }

    /// Seed context from collected names (used by scripts).
    ///
    /// Take the errors name resolution found, and give a span to the ones
    /// that have somewhere to point.
    ///
    /// A signature that does not resolve leaves its function unregistered, so
    /// every call to it reports F002 as well. That one names a real symptom
    /// and no cause, which is why the cause is not left as a line in a list.
    fn record_resolution_errors(
        &mut self,
        errors: &[TypeError],
        module_id: Option<ModuleId<'db>>,
    ) {
        for error in errors {
            if let TypeError::TypeParamNotErasable { param, position, fun_local_index } = error {
                self.pending_diagnostics.push(PendingDiagnostic::TypeParamNotErasable {
                    local_index: *fun_local_index,
                    module_id,
                    param: InternedText::new(self.db, param.S()),
                    position: InternedText::new(self.db, position.S()),
                });
            }
            self.add_error(error.clone());
        }
    }

    /// Like `seed_from_name_resolution` but takes `CollectedNames` directly
    /// instead of the tracked `ModuleNameResolution` struct.
    pub fn seed_from_collected_names(
        &mut self,
        collected: &CollectedNames<'db>,
        module_id: Option<ModuleId<'db>>,
    ) {
        self.record_resolution_errors(&collected.errors, module_id);

        // Add type aliases.
        for (name, ty) in &collected.type_aliases {
            self.type_aliases.insert(*name, ty.clone());
        }

        // Add function signatures with ASTs.
        let ast_map: HashMap<bct::text::InternedText<'db>, StmtFun<'db>> = collected.function_asts
            .iter()
            .map(|(name, ast)| (*name, *ast))
            .collect();

        for (name, func_type) in &collected.functions {
            if let Some(&func_ast) = ast_map.get(name) {
                self.add_function_with_ast(*name, *func_type, func_ast, module_id);
            } else {
                self.add_function(*name, *func_type);
            }
        }
    }
}

/// Format a description of what the @ operator does for this conversion.
fn format_adapt_description<'db>(
    db: &'db dyn salsa::Database,
    from: &Type<'db>,
    to: &Type<'db>,
) -> String {
    use datalove_datafun_common::{type_to_string, types_equivalent};

    if types_equivalent(db, from, to) {
        "clone the value".to_string()
    } else {
        format!("convert from `{}` to `{}`", type_to_string(db, from), type_to_string(db, to))
    }
}
