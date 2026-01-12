//! Type checking context.
//!
//! Provides TypeContext for tracking bindings and errors during typechecking,
//! and ScriptTypeContext for tracking accumulated bindings across script units.

use std::collections::HashMap;
use bct::text::{InternedText, TextSpan};
use salsa::plumbing::AsId;

use datalove_datafun_ast::ast::*;

pub use crate::{
    DatafunSpans,
    TypeAndHeap,
    TypeFunction,
    TypeError,
    ResolvedCallTarget,
    ModuleId,
};

/// Context for typechecking.
pub struct TypeContext<'db> {
    pub(crate) db: &'db dyn crate::Db,
    /// Pre-computed spans for error reporting.
    pub(crate) spans: DatafunSpans,
    /// Variable bindings (name -> type).
    pub(crate) variables: HashMap<InternedText<'db>, TypeAndHeap<'db>>,
    /// Function signatures (name -> function type).
    pub(crate) functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Function ASTs for resolving call targets (name -> (AST, module_id)).
    pub(crate) function_asts: HashMap<InternedText<'db>, (StmtFun<'db>, Option<ModuleId>)>,
    /// Expected return type for current function (if inside a function).
    pub(crate) expected_return_type: Option<TypeAndHeap<'db>>,
    /// Whether current function has no declared return type (void function).
    pub(crate) is_void_function: bool,
    pub(crate) errors: Vec<TypeError>,
    /// Expression types, indexed by ExprFun ID.
    pub(crate) expr_types: Vec<Option<TypeAndHeap<'db>>>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    pub(crate) call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
    /// Stack of loop depth (for validating break/continue are inside a loop).
    pub(crate) loop_depth: usize,
    /// Whether we're in a reference context (ref/mut/out param or binop operand).
    /// Move-type field projections are allowed in ref context.
    pub(crate) ref_context: bool,
}

impl<'db> TypeContext<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        spans: DatafunSpans,
    ) -> Self {
        TypeContext {
            db,
            spans,
            variables: HashMap::new(),
            functions: HashMap::new(),
            function_asts: HashMap::new(),
            expected_return_type: None,
            is_void_function: false,
            errors: Vec::new(),
            expr_types: Vec::new(),
            call_targets: Vec::new(),
            loop_depth: 0,
            ref_context: false,
        }
    }

    pub fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    // Error emission helpers that both emit diagnostics and return TypeError.

    /// F001: Undefined variable.
    pub fn error_undefined_variable(&self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!("cannot find value `{}` in this scope", name.as_str(self.db));
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F001")
                .primary_label(ts.clone(), "not found in this scope")
                .emit_type();
        }
        TypeError::UnresolvedName(name.as_str(self.db).to_string())
    }

    /// F002: Undefined function.
    pub fn error_undefined_function(&self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!("cannot find function `{}` in this scope", name.as_str(self.db));
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F002")
                .primary_label(ts.clone(), "not found in this scope")
                .emit_type();
        }
        TypeError::UnresolvedName(name.as_str(self.db).to_string())
    }

    /// F011: Cannot synthesize type.
    pub fn error_cannot_synthesize(&self, expr: ExprFun<'db>, message: &str) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            datalove_diagnostic::DiagnosticBuilder::error(self.db, message)
                .code("F011")
                .primary_label(ts.clone(), "cannot infer type")
                .emit_type();
        }
        TypeError::CannotSynthesize
    }

    /// F016: Type mismatch.
    pub fn error_type_mismatch(&self, expr: ExprFun<'db>, expected: &str, actual: &str, label: &str) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!("mismatched types: expected `{}`, found `{}`", expected, actual);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F016")
                .primary_label(ts.clone(), label)
                .emit_type();
        }
        TypeError::TypeMismatch {
            expected: expected.to_string(),
            actual: actual.to_string(),
        }
    }

    /// F045: Function arity mismatch.
    pub fn error_arity_mismatch(&self, expr: ExprFun<'db>, expected: usize, actual: usize) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!(
                "this function takes {} argument{} but {} {} supplied",
                expected,
                if expected == 1 { "" } else { "s" },
                actual,
                if actual == 1 { "was" } else { "were" }
            );
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F045")
                .primary_label(ts.clone(), &format!("expected {} arguments", expected))
                .emit_type();
        }
        TypeError::ArityMismatch { expected, actual }
    }

    /// F046: Result destructuring requires error binding.
    pub fn error_result_requires_binding(&self, expr: ExprFun<'db>) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            datalove_diagnostic::DiagnosticBuilder::error(self.db, "Result destructuring requires an else binding")
                .code("F046")
                .primary_label(ts.clone(), "Result type here")
                .note("use `if let ok(x) = result { ... } else error(e) { ... }` to handle both cases")
                .emit_type();
        }
        TypeError::ResultRequiresErrorBinding
    }

    /// F026: Invalid operand type for operator.
    pub fn error_invalid_operand_type(&self, expr: ExprFun<'db>, op: &str, ty: &str) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!("invalid operand type `{}` for operator `{}`", ty, op);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F026")
                .primary_label(ts.clone(), &format!("operator `{}` cannot be applied to type `{}`", op, ty))
                .emit_type();
        }
        TypeError::InvalidOperandType {
            op: op.to_string(),
            ty: ty.to_string(),
        }
    }

    /// F048: Try operator type mismatch.
    pub fn error_try_type_mismatch(&self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!("try operator `{}` requires {} type, found `{}`", operator, expected, actual);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F048")
                .primary_label(ts.clone(), &format!("expected {}, found `{}`", expected, actual))
                .emit_type();
        }
        TypeError::TryTypeMismatch {
            operator: operator.to_string(),
            actual_type: actual.to_string(),
        }
    }

    /// F049: Try operator return type mismatch.
    pub fn error_try_return_type_mismatch(&self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        if let Some(ts) = self.get_span(expr) {
            let msg = format!("try operator `{}` requires function to return {}, found `{}`", operator, expected, actual);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F049")
                .primary_label(ts.clone(), "try operator here")
                .note(&format!("function must return {} to use `{}` operator", expected, operator))
                .emit_type();
        }
        TypeError::TryReturnTypeMismatch {
            operator: operator.to_string(),
            return_type: actual.to_string(),
        }
    }

    /// Look up span for a datafun expression.
    pub fn get_span(&self, expr: ExprFun<'db>) -> Option<TextSpan<'db>> {
        self.spans.lookup(expr).map(|entry| {
            let (text, span) = entry.to_text_and_span(self.db);
            TextSpan::new(text, span)
        })
    }

    pub fn add_variable(&mut self, name: InternedText<'db>, ty: TypeAndHeap<'db>) {
        self.variables.insert(name, ty);
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

    pub fn lookup_variable(&self, name: InternedText<'db>) -> Option<TypeAndHeap<'db>> {
        self.variables.get(&name).copied()
    }

    pub fn lookup_function(&self, name: InternedText<'db>) -> Option<TypeFunction<'db>> {
        self.functions.get(&name).copied()
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
    pub fn store_expr_type(&mut self, expr: ExprFun<'db>, ty: TypeAndHeap<'db>) {
        let id = expr.as_id();
        let index = id.index() as usize;

        // Ensure the vector is large enough.
        if index >= self.expr_types.len() {
            self.expr_types.resize(index + 1, None);
        }

        self.expr_types[index] = Some(ty);
    }

    /// Synthesize the type of an expression.
    pub fn synthesize_expr(&mut self, expr: ExprFun<'db>) -> Result<TypeAndHeap<'db>, TypeError> {
        let ty = crate::synthesize::synthesize_expr(self, expr)?;
        self.store_expr_type(expr, ty);
        Ok(ty)
    }
}

/// Context for typechecking sequential script units.
///
/// Tracks bindings exported from previous units so subsequent units
/// can reference them.
#[derive(Clone, Default)]
pub struct ScriptTypeContext<'db> {
    /// Variable bindings from previous units: name -> type.
    pub variables: HashMap<InternedText<'db>, TypeAndHeap<'db>>,
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
                    if let Some(ty) = result.expr_types.get(expr_id).and_then(|t| *t) {
                        self.variables.insert(name, ty);
                    }
                }
                Statement::Var(var_stmt) => {
                    let name = var_stmt.name;
                    let value_expr = var_stmt.value;
                    let expr_id = value_expr.as_id().index() as usize;
                    if let Some(ty) = result.expr_types.get(expr_id).and_then(|t| *t) {
                        self.variables.insert(name, ty);
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
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
    /// Resolved call targets, indexed by ExprFunctionCall ID.
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
}

/// Result of typechecking an expression (non-salsa version for context-aware checking).
pub struct ExprTypecheckResultRaw<'db> {
    /// Type errors encountered.
    pub errors: Vec<TypeError>,
    /// Expression types, indexed by ExprFun ID.
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
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
        let ty = crate::types::convert_type_hint(db, param.type_hint).ok()?;
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
