//! Type checking context.
//!
//! Provides TypeContext for tracking bindings and errors during typechecking,
//! and ScriptTypeContext for tracking accumulated bindings across script units.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::text::{InternedText, TextSpan};
use salsa::plumbing::AsId;

use datalove_datafun_ast::ast::*;
use datalove_datafun_intrinsics::IntrinsicId;

pub use datalove_datafun_ast::spans::DatafunSpans;
pub use bct::module_graph::ModuleId;

pub use crate::{
    PendingDiagnostic,
    Type,
    TypeFunction,
    TypeError,
    ResolvedCallTarget,
};

/// Context for typechecking.
pub struct TypeContext<'db> {
    pub(crate) db: &'db dyn crate::Db,
    /// Pre-computed spans for error reporting.
    pub(crate) spans: DatafunSpans,
    /// Current module being typechecked (for pending diagnostics).
    pub(crate) current_module_id: Option<ModuleId>,
    /// Variable bindings (name -> (type, is_mutable)).
    pub(crate) variables: HashMap<InternedText<'db>, (Type<'db>, bool)>,
    /// Function signatures (name -> function type).
    pub(crate) functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Function ASTs for resolving call targets (name -> (AST, module_id)).
    pub(crate) function_asts: HashMap<InternedText<'db>, (StmtFun<'db>, Option<ModuleId>)>,
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
    pub(crate) expr_types: Vec<Option<Type<'db>>>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    pub(crate) call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
    /// Stack of loop depth (for validating break/continue are inside a loop).
    pub(crate) loop_depth: usize,
    /// Whether we're in a reference context (ref/mut/out param or binop operand).
    /// Move-type field projections are allowed in ref context.
    pub(crate) ref_context: bool,
    /// Resolved intrinsic targets, indexed by ExprFun ID.
    pub(crate) intrinsic_targets: Vec<Option<IntrinsicId>>,
}

impl<'db> TypeContext<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        spans: DatafunSpans,
    ) -> Self {
        Self::with_module_id(db, spans, None)
    }

    /// Create a TypeContext for a specific module.
    pub fn with_module_id(
        db: &'db dyn crate::Db,
        spans: DatafunSpans,
        module_id: Option<ModuleId>,
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
            expr_types: Vec::new(),
            call_targets: Vec::new(),
            loop_depth: 0,
            ref_context: false,
            intrinsic_targets: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    // Error collection helpers that collect pending diagnostics and return TypeError.
    // Diagnostics are emitted at the end of typechecking via emit_pending_diagnostics().

    /// F001: Undefined variable.
    pub fn error_undefined_variable(&mut self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedVariable {
            expr_id: expr.as_id().index(),
            module_id: self.current_module_id,
            name,
        });
        TypeError::UnresolvedName(name.as_str(self.db).S())
    }

    /// F002: Undefined function.
    pub fn error_undefined_function(&mut self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::UndefinedFunction {
            expr_id: expr.as_id().index(),
            module_id: self.current_module_id,
            name,
        });
        TypeError::UnresolvedName(name.as_str(self.db).S())
    }

    /// F011: Cannot synthesize type.
    pub fn error_cannot_synthesize(&mut self, expr: ExprFun<'db>, message: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::CannotSynthesize {
            expr_id: expr.as_id().index(),
            module_id: self.current_module_id,
            message: InternedText::new(self.db, message.S()),
        });
        TypeError::CannotSynthesize
    }

    /// F016: Type mismatch.
    pub fn error_type_mismatch(&mut self, expr: ExprFun<'db>, expected: &str, actual: &str, label: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::TypeMismatch {
            expr_id: expr.as_id().index(),
            module_id: self.current_module_id,
            expected: InternedText::new(self.db, expected.S()),
            actual: InternedText::new(self.db, actual.S()),
            label: InternedText::new(self.db, label.S()),
        });
        TypeError::TypeMismatch {
            expected: expected.S(),
            actual: actual.S(),
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
            call_expr_id: expr.as_id().index(),
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
            expr_id: expr.as_id().index(),
            module_id: self.current_module_id,
        });
        TypeError::ResultRequiresErrorBinding
    }

    /// F026: Invalid operand type for operator.
    pub fn error_invalid_operand_type(&mut self, expr: ExprFun<'db>, op: &str, ty: &str) -> TypeError {
        self.pending_diagnostics.push(PendingDiagnostic::InvalidOperandType {
            expr_id: expr.as_id().index(),
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
            expr_id: expr.as_id().index(),
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
            expr_id: expr.as_id().index(),
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
        self.spans.lookup(expr).map(|entry| {
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
        module_id: Option<ModuleId>,
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
        source_module_id: ModuleId,
    ) {
        self.functions.insert(name, func_type);
        self.function_asts.insert(name, (func_ast, Some(source_module_id)));
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
    pub fn lookup_function_ast(&self, name: InternedText<'db>) -> Option<(StmtFun<'db>, Option<ModuleId>)> {
        self.function_asts.get(&name).copied()
    }

    /// Store resolved call target for a function call expression.
    pub fn store_call_target(&mut self, call: ExprFunctionCall<'db>, func: StmtFun<'db>, module_id: Option<ModuleId>) {
        let id = call.as_id();
        let index = id.index() as usize;

        // Ensure the vector is large enough.
        if index >= self.call_targets.len() {
            self.call_targets.resize(index + 1, None);
        }

        self.call_targets[index] = Some(ResolvedCallTarget::new(self.db, func, module_id));
    }

    /// Store the type for an expression.
    pub fn store_expr_type(&mut self, expr: ExprFun<'db>, ty: &Type<'db>) {
        let id = expr.as_id();
        let index = id.index() as usize;

        // Ensure the vector is large enough.
        if index >= self.expr_types.len() {
            self.expr_types.resize(index + 1, None);
        }

        self.expr_types[index] = Some(ty.clone());
    }

    /// Synthesize the type of an expression.
    pub fn synthesize_expr(&mut self, expr: ExprFun<'db>) -> Result<Type<'db>, TypeError> {
        let ty = crate::synthesize::synthesize_expr(self, expr)?;
        self.store_expr_type(expr, &ty);
        Ok(ty)
    }

    /// Store resolved intrinsic target for an intrinsic call expression.
    pub fn store_intrinsic_target(&mut self, expr: ExprFun<'db>, intrinsic: IntrinsicId) {
        let id = expr.as_id();
        let index = id.index() as usize;

        // Ensure the vector is large enough.
        if index >= self.intrinsic_targets.len() {
            self.intrinsic_targets.resize(index + 1, None);
        }

        self.intrinsic_targets[index] = Some(intrinsic);
    }

    /// Get resolved intrinsic target for an expression.
    pub fn get_intrinsic_target(&self, expr: ExprFun<'db>) -> Option<IntrinsicId> {
        let id = expr.as_id();
        let index = id.index() as usize;
        self.intrinsic_targets.get(index).and_then(|t| *t)
    }
}

/// Context for typechecking sequential script units.
///
/// Tracks bindings exported from previous units so subsequent units
/// can reference them.
#[derive(Clone, Default)]
pub struct ScriptTypeContext<'db> {
    /// Variable bindings from previous units: name -> (type, is_mutable).
    pub variables: HashMap<InternedText<'db>, (Type<'db>, bool)>,
    /// Function signatures from previous units: name -> signature.
    pub functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
}

impl<'db> ScriptTypeContext<'db> {
    /// Create a new empty script type context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add exported bindings from a typechecked script unit.
    ///
    /// Extracts let/var bindings and function definitions from the parsed statements
    /// and adds them to the context for subsequent units.
    pub fn add_script_exports(
        &mut self,
        db: &'db dyn crate::Db,
        parsed: ParsedStatements<'db>,
        result: &ScriptTypecheckResultRaw<'db>,
    ) {
        // Extract let/var bindings and function definitions from the parsed statements.
        for stmt in &parsed.statements {
            match stmt {
                Statement::Let(let_stmt) => {
                    let name = let_stmt.name;
                    // Look up the type from the expression via Salsa ID.
                    let value_expr = let_stmt.value;
                    let expr_id = value_expr.as_id().index() as usize;
                    if let Some(ty) = result.expr_types.get(expr_id).and_then(|t| t.clone()) {
                        self.variables.insert(name, (ty, false)); // Let bindings are immutable.
                    }
                }
                Statement::Var(var_stmt) => {
                    let name = var_stmt.name;
                    // Get type from value expression if present, otherwise from type hint.
                    let ty = if let Some(value_expr) = var_stmt.value {
                        let expr_id = value_expr.as_id().index() as usize;
                        result.expr_types.get(expr_id).and_then(|t| t.clone())
                    } else if let Some(type_hint) = var_stmt.type_hint.clone() {
                        crate::types::convert_type_hint(db, type_hint).ok()
                    } else {
                        None
                    };
                    if let Some(ty) = ty {
                        self.variables.insert(name, (ty, true)); // Var bindings are mutable.
                    }
                }
                Statement::Fun(fun_stmt) => {
                    let name = fun_stmt.name(db);
                    // Build the function type from the signature.
                    if let Some(func_type) = build_function_type_from_stmt(db, *fun_stmt) {
                        self.functions.insert(name, func_type);
                    }
                }
                _ => {}
            }
        }
    }

    /// Add exported bindings from a typechecked expression unit.
    ///
    /// Expression units don't export bindings, but we still need to track
    /// them in the sequence for unit indexing.
    pub fn add_expr_exports(&mut self) {
        // Expression units don't export anything.
    }
}

/// Result of typechecking a script (non-salsa version for context-aware checking).
pub struct ScriptTypecheckResultRaw<'db> {
    /// The root parsed statements.
    pub root_parsed: ParsedStatements<'db>,
    /// Type errors encountered.
    pub errors: Vec<TypeError>,
    /// Expression types, indexed by ExprFun ID.
    pub expr_types: Vec<Option<Type<'db>>>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
}

/// Result of typechecking an expression (non-salsa version for context-aware checking).
pub struct ExprTypecheckResultRaw<'db> {
    /// Type errors encountered.
    pub errors: Vec<TypeError>,
    /// Expression types, indexed by ExprFun ID.
    pub expr_types: Vec<Option<Type<'db>>>,
}

/// Build a TypeFunction from a function statement.
pub fn build_function_type_from_stmt<'db>(
    db: &'db dyn crate::Db,
    stmt: StmtFun<'db>,
) -> Option<TypeFunction<'db>> {
    let params = stmt.params(db);
    let return_type = stmt.return_type(db);

    // Convert parameter types and collect modes.
    let mut param_types = Vec::new();
    let mut param_modes = Vec::new();
    for param in params {
        let ty = crate::types::convert_type_hint(db, param.type_hint.clone()).ok()?;
        param_types.push(ty);
        param_modes.push(param.mode);
    }

    // Convert return type (default to unit if not specified).
    let ret_ty = match return_type {
        Some(type_hint) => crate::types::convert_type_hint(db, type_hint).ok()?,
        None => crate::types::unit_type(db),
    };

    Some(TypeFunction::new(db, param_types, param_modes, ret_ty))
}
