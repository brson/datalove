use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;
use std::collections::BTreeMap;
use crate::ast::*;
use crate::datalit;
use bct;

/// Type representation for datafun (extends datalit types with function types).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum Type<'db> {
    /// Datalit type (primitives, collections, etc.).
    Datalit(datalit::tycheck::Type<'db>),
    /// Function type: (param_types) -> return_type.
    Function(TypeFunction<'db>),
    /// Void/unit type for functions with no return value.
    Void,
}

#[salsa::tracked]
pub struct TypeAndHeap<'db> {
    pub heap: datalit::ast::Heap,
    #[returns(ref)]
    pub ty: Type<'db>,
}

#[salsa::tracked]
pub struct TypeFunction<'db> {
    pub param_types: Vec<TypeAndHeap<'db>>,
    pub return_type: TypeAndHeap<'db>,
}

/// Type error representation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum TypeError {
    TypeMismatch { expected: String, actual: String },
    UnresolvedName(String),
    CannotSynthesize,
    InvalidOperandType { op: String, ty: String },
    InvalidTupleElement { ty: String },
    ArityMismatch { expected: usize, actual: usize },
    NotAFunction(String),
    DatalitError(String),
    ResultRequiresErrorBinding,
    TryOutsideFunction { operator: String },
    TryTypeMismatch { operator: String, actual_type: String },
    TryReturnTypeMismatch { operator: String, return_type: String },
    // Datalit-compatible error types for error equivalence.
    IntOutOfRange,
    HeapMismatch { expected_heap: String, actual_heap: String },
    MissingField(String),
    ExtraField(String),
    FieldOrderMismatch,
    VariantNotFound(String),
    // Loop control flow errors.
    BreakOutsideLoop,
    ContinueOutsideLoop,
}

impl From<datalit::tycheck::TypeError> for TypeError {
    fn from(err: datalit::tycheck::TypeError) -> Self {
        match err {
            datalit::tycheck::TypeError::TypeMismatch { expected, actual } => {
                TypeError::TypeMismatch { expected, actual }
            }
            datalit::tycheck::TypeError::HeapMismatch { expected_heap, actual_heap } => {
                TypeError::HeapMismatch { expected_heap, actual_heap }
            }
            datalit::tycheck::TypeError::CannotSynthesize => TypeError::CannotSynthesize,
            datalit::tycheck::TypeError::UnresolvedName(name) => TypeError::UnresolvedName(name),
            datalit::tycheck::TypeError::MissingField(name) => TypeError::MissingField(name),
            datalit::tycheck::TypeError::ExtraField(name) => TypeError::ExtraField(name),
            datalit::tycheck::TypeError::FieldOrderMismatch => TypeError::FieldOrderMismatch,
            datalit::tycheck::TypeError::IntOutOfRange => TypeError::IntOutOfRange,
            datalit::tycheck::TypeError::VariantNotFound(name) => TypeError::VariantNotFound(name),
            datalit::tycheck::TypeError::ArityMismatch { expected, actual } => {
                TypeError::ArityMismatch { expected, actual }
            }
        }
    }
}

/// Type error entry with location info.
#[salsa::tracked]
pub struct TypeErrorEntry<'db> {
    pub error: TypeError,
}

/// Result of typechecking a script.
#[salsa::tracked]
pub struct TypecheckResult<'db> {
    /// The root script.
    pub root_script: Script<'db>,

    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,

    /// Expression types, indexed by ExprFun ID.
    #[returns(ref)]
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
}

/// Context for typechecking.
pub struct TypeContext<'db> {
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    /// Variable bindings (name -> type).
    variables: HashMap<InternedText<'db>, TypeAndHeap<'db>>,
    /// Function signatures (name -> function type).
    functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Expected return type for current function (if inside a function).
    expected_return_type: Option<TypeAndHeap<'db>>,
    errors: Vec<TypeError>,
    /// Expression types, indexed by ExprFun ID.
    expr_types: Vec<Option<TypeAndHeap<'db>>>,
    /// Current loop nesting depth (for validating break/continue).
    loop_depth: u32,
}

impl<'db> TypeContext<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        source: bct::input::Source,
    ) -> Self {
        TypeContext {
            db,
            source,
            variables: HashMap::new(),
            functions: HashMap::new(),
            expected_return_type: None,
            errors: Vec::new(),
            expr_types: Vec::new(),
            loop_depth: 0,
        }
    }

    fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    // Error emission helpers that both emit diagnostics and return TypeError.

    /// F001: Undefined variable.
    fn error_undefined_variable(&self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("cannot find value `{}` in this scope", name.as_str(self.db));
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F001")
                .primary_label(text, span, "not found in this scope")
                .emit_type();
        }
        TypeError::UnresolvedName(name.as_str(self.db).to_string())
    }

    /// F002: Undefined function.
    fn error_undefined_function(&self, expr: ExprFun<'db>, name: InternedText<'db>) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("cannot find function `{}` in this scope", name.as_str(self.db));
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F002")
                .primary_label(text, span, "not found in this scope")
                .emit_type();
        }
        TypeError::UnresolvedName(name.as_str(self.db).to_string())
    }

    /// F011: Cannot synthesize type.
    fn error_cannot_synthesize(&self, expr: ExprFun<'db>, message: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            datalove_diagnostic::DiagnosticBuilder::error(self.db, message)
                .code("F011")
                .primary_label(text, span, "cannot infer type")
                .emit_type();
        }
        TypeError::CannotSynthesize
    }

    /// F016: Type mismatch.
    fn error_type_mismatch(&self, expr: ExprFun<'db>, expected: &str, actual: &str, label: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("mismatched types: expected `{}`, found `{}`", expected, actual);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F016")
                .primary_label(text, span, label)
                .emit_type();
        }
        TypeError::TypeMismatch {
            expected: expected.to_string(),
            actual: actual.to_string(),
        }
    }

    /// F045: Function arity mismatch.
    fn error_arity_mismatch(&self, expr: ExprFun<'db>, expected: usize, actual: usize) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!(
                "this function takes {} argument{} but {} {} supplied",
                expected,
                if expected == 1 { "" } else { "s" },
                actual,
                if actual == 1 { "was" } else { "were" }
            );
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F045")
                .primary_label(text, span, &format!("expected {} arguments", expected))
                .emit_type();
        }
        TypeError::ArityMismatch { expected, actual }
    }

    /// F046: Result destructuring requires error binding.
    fn error_result_requires_binding(&self, expr: ExprFun<'db>) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            datalove_diagnostic::DiagnosticBuilder::error(self.db, "Result destructuring requires an else binding")
                .code("F046")
                .primary_label(text, span, "Result type here")
                .note("use `if let ok(x) = result { ... } else error(e) { ... }` to handle both cases")
                .emit_type();
        }
        TypeError::ResultRequiresErrorBinding
    }

    /// F026: Invalid operand type for operator.
    fn error_invalid_operand_type(&self, expr: ExprFun<'db>, op: &str, ty: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("invalid operand type `{}` for operator `{}`", ty, op);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F026")
                .primary_label(text, span, &format!("operator `{}` cannot be applied to type `{}`", op, ty))
                .emit_type();
        }
        TypeError::InvalidOperandType {
            op: op.to_string(),
            ty: ty.to_string(),
        }
    }

    /// F027: Invalid tuple element type.
    fn error_invalid_tuple_element(&self, expr: ExprFun<'db>, ty: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("tuple elements must be datalit types, found `{}`", ty);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F027")
                .primary_label(text, span, "invalid type for tuple element")
                .emit_type();
        }
        TypeError::InvalidTupleElement {
            ty: ty.to_string(),
        }
    }

    /// F047: Try operator used outside function.
    fn error_try_outside_function(&self, expr: ExprFun<'db>, operator: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("try operator `{}` can only be used inside a function", operator);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F047")
                .primary_label(text, span, "try operator here")
                .emit_type();
        }
        TypeError::TryOutsideFunction {
            operator: operator.to_string(),
        }
    }

    /// F048: Try operator type mismatch.
    fn error_try_type_mismatch(&self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("try operator `{}` requires {} type, found `{}`", operator, expected, actual);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F048")
                .primary_label(text, span, &format!("expected {}, found `{}`", expected, actual))
                .emit_type();
        }
        TypeError::TryTypeMismatch {
            operator: operator.to_string(),
            actual_type: actual.to_string(),
        }
    }

    /// F049: Try operator return type mismatch.
    fn error_try_return_type_mismatch(&self, expr: ExprFun<'db>, operator: &str, expected: &str, actual: &str) -> TypeError {
        if let Some((text, span)) = self.get_span(expr) {
            let msg = format!("try operator `{}` requires function to return {}, found `{}`", operator, expected, actual);
            datalove_diagnostic::DiagnosticBuilder::error(self.db, &msg)
                .code("F049")
                .primary_label(text, span, "try operator here")
                .note(&format!("function must return {} to use `{}` operator", expected, operator))
                .emit_type();
        }
        TypeError::TryReturnTypeMismatch {
            operator: operator.to_string(),
            return_type: actual.to_string(),
        }
    }

    /// Look up span for a datafun expression (on-demand).
    fn get_span(&self, expr: ExprFun<'db>) -> Option<(bct::text::Text<'db>, datalove_diagnostic::ByteSpan)> {
        let spans = crate::spans::datafun_spans(self.db, self.source);
        spans.lookup(self.db, expr).map(|entry| entry.to_text_and_span(self.db))
    }

    pub fn add_variable(&mut self, name: InternedText<'db>, ty: TypeAndHeap<'db>) {
        self.variables.insert(name, ty);
    }

    pub fn add_function(&mut self, name: InternedText<'db>, func_type: TypeFunction<'db>) {
        self.functions.insert(name, func_type);
    }

    pub fn lookup_variable(&self, name: InternedText<'db>) -> Option<TypeAndHeap<'db>> {
        self.variables.get(&name).copied()
    }

    fn lookup_function(&self, name: InternedText<'db>) -> Option<TypeFunction<'db>> {
        self.functions.get(&name).copied()
    }

    /// Store the type for an expression.
    fn store_expr_type(&mut self, expr: ExprFun<'db>, ty: TypeAndHeap<'db>) {
        use salsa::plumbing::AsId;
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
        let ty = synthesize_expr(self, expr)?;
        self.store_expr_type(expr, ty);
        Ok(ty)
    }
}

/// Typecheck a script.
#[salsa::tracked]
pub fn type_check<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    script: Script<'db>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, source);

    // First pass: collect all function signatures.
    for statement in script.statements(db) {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt);
        }
    }

    // Second pass: type check all statements (including function bodies).
    for statement in script.statements(db) {
        check_statement(&mut ctx, statement);
    }

    let errors = ctx
        .errors
        .into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();

    TypecheckResult::new(db, script, errors, ctx.expr_types)
}

/// Typecheck a script for diagnostic emission.
///
/// This is a tracked wrapper that parses and typechecks a source,
/// enabling diagnostic accumulation via Salsa.
#[salsa::tracked]
pub fn type_check_for_diagnostics<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> TypecheckResult<'db> {
    let script = crate::parser::parse_for_diagnostics(db, source);
    type_check(db, source, script)
}

/// Typecheck a script with module graph support.
///
/// This version of type_check allows scripts to import functions from modules
/// in the module graph. Used for script units that define functions requiring
/// access to imported function signatures.
#[salsa::tracked]
pub fn type_check_with_module_graph<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    script: Script<'db>,
    graph: crate::module_graph::ModuleGraph,
    graph_typecheck: crate::module_graph::ModuleGraphTypecheckResult<'db>,
) -> TypecheckResult<'db> {
    use crate::module_graph::ModuleId;

    let mut ctx = TypeContext::new(db, source);

    // Build path-to-id map from the module graph.
    let mut path_to_id: HashMap<String, ModuleId> = HashMap::new();
    for module in graph.iter_modules(db) {
        let id = module.id(db);
        path_to_id.insert(id.path(db).clone(), id);
    }

    // Build module alias map from require statements.
    let module_exports_map = graph_typecheck.module_exports(db);
    let alias_map = build_module_alias_map_for_graph(db, script, &path_to_id);

    // Process import statements to populate function signatures.
    for statement in script.statements(db) {
        if let Statement::Import(import) = statement {
            let module_name = import.module_name(db);
            let item_name = import.item_name(db);

            // Look up the module in the alias map.
            if let Some(&source_module_id) = alias_map.get(&module_name) {
                // Look up the module exports.
                if let Some(exports) = module_exports_map.get(&source_module_id) {
                    // Look up the function in the exports.
                    let func_opt = exports.functions(db).iter()
                        .find(|(name, _)| *name == item_name)
                        .map(|(_, func_type)| *func_type);

                    if let Some(func_type) = func_opt {
                        // Add the function to the context.
                        ctx.add_function(item_name, func_type);
                    } else {
                        ctx.add_error(TypeError::UnresolvedName(
                            format!("{}.{}", module_name.as_str(db), item_name.as_str(db))
                        ));
                    }
                } else {
                    ctx.add_error(TypeError::UnresolvedName(
                        format!("module {} (not typechecked)", module_name.as_str(db))
                    ));
                }
            } else {
                ctx.add_error(TypeError::UnresolvedName(
                    format!("module {} (not required)", module_name.as_str(db))
                ));
            }
        }
    }

    // First pass: collect all function signatures.
    for statement in script.statements(db) {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt);
        }
    }

    // Second pass: type check all statements (including function bodies).
    for statement in script.statements(db) {
        check_statement(&mut ctx, statement);
    }

    let errors = ctx
        .errors
        .into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();

    TypecheckResult::new(db, script, errors, ctx.expr_types)
}

/// Look up the type of a variable after typechecking.
///
/// This re-runs typechecking to get the variable type.
/// Since typechecking is memoized by Salsa, this is efficient.
#[salsa::tracked]
pub fn lookup_variable_type<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    name: InternedText<'db>,
) -> Option<TypeAndHeap<'db>> {
    // Create a dummy source since we don't have one available here.
    let dummy_source = bct::input::Source::new(db, String::new());
    let mut ctx = TypeContext::new(db, dummy_source);

    // First pass: collect all function signatures.
    for statement in script.statements(db) {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt);
        }
    }

    // Second pass: type check all statements.
    for statement in script.statements(db) {
        check_statement(&mut ctx, statement);
    }

    ctx.lookup_variable(name)
}

/// Typecheck a module graph (package-agnostic).
///
/// This is the core typechecking function that works with the package-agnostic
/// ModuleGraph abstraction. Modules are processed in dependency order.
/// Function-level imports are resolved on the fly from `require module` and `import` statements.
#[salsa::tracked]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn crate::Db,
    graph: crate::module_graph::ModuleGraph,
) -> crate::module_graph::ModuleGraphTypecheckResult<'db> {
    use crate::module_graph::{ModuleId, ModuleExports as MgModuleExports, ModuleImports as MgModuleImports, ModuleGraphTypecheckResult};

    let mut module_errors: BTreeMap<ModuleId, Vec<TypeError>> = BTreeMap::new();
    let mut module_exports_map: BTreeMap<ModuleId, MgModuleExports<'db>> = BTreeMap::new();
    let mut module_imports_map: BTreeMap<ModuleId, MgModuleImports<'db>> = BTreeMap::new();
    let mut function_analyses: Vec<(crate::ast::StmtFun<'db>, crate::function_analysis::FunctionAnalysis<'db>)> = Vec::new();

    // Build a map from module path to ModuleId for quick lookup.
    let mut path_to_id: HashMap<String, ModuleId> = HashMap::new();
    for module in graph.iter_modules(db) {
        let id = module.id(db);
        path_to_id.insert(id.path(db).clone(), id);
    }

    // Process each module in dependency order.
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let source = module.source(db);

        // Parse the module.
        let parse_result = crate::parser::parse(db, source);
        let script = parse_result.script(db);

        // Create type context for this module.
        let mut ctx = TypeContext::new(db, source);

        // Track imports for this module.
        let mut module_import_functions: Vec<(InternedText<'db>, ModuleId, InternedText<'db>)> = Vec::new();

        // Build module alias map from require module statements.
        let alias_map = build_module_alias_map_for_graph(db, script, &path_to_id);

        // Resolve function imports from import statements.
        for statement in script.statements(db) {
            if let Statement::Import(import) = statement {
                let module_name = import.module_name(db);
                let item_name = import.item_name(db);

                // Look up the module in the alias map.
                if let Some(&source_module_id) = alias_map.get(&module_name) {
                    // Look up the module exports.
                    if let Some(exports) = module_exports_map.get(&source_module_id) {
                        // Look up the function in the exports.
                        let func_opt = exports.functions(db).iter()
                            .find(|(name, _)| *name == item_name)
                            .map(|(_, func_type)| *func_type);

                        if let Some(func_type) = func_opt {
                            ctx.add_function(item_name, func_type);
                            module_import_functions.push((item_name, source_module_id, item_name));
                        } else {
                            ctx.add_error(TypeError::UnresolvedName(
                                format!("{}.{}", module_name.as_str(db), item_name.as_str(db))
                            ));
                        }
                    } else {
                        ctx.add_error(TypeError::UnresolvedName(
                            format!("module {} (not typechecked yet)", module_name.as_str(db))
                        ));
                    }
                } else {
                    ctx.add_error(TypeError::UnresolvedName(
                        format!("module {} (not required)", module_name.as_str(db))
                    ));
                }
            }
        }

        // First pass: collect all function signatures from this module.
        for statement in script.statements(db) {
            if let Statement::Fun(stmt) = statement {
                collect_function_signature(&mut ctx, &stmt);
            }
        }

        // Second pass: type check all statements.
        for statement in script.statements(db) {
            check_statement(&mut ctx, statement);
        }

        // Collect errors for this module.
        if !ctx.errors.is_empty() {
            module_errors.insert(module_id, ctx.errors.clone());
        }

        // Collect exports for this module.
        let exports_functions = collect_module_exports(db, script);
        let exports = MgModuleExports::new(db, module_id, exports_functions);
        module_exports_map.insert(module_id, exports);

        // Collect imports for this module.
        let imports = MgModuleImports::new(db, module_id, module_import_functions);
        module_imports_map.insert(module_id, imports);

        // Analyze all functions in this module.
        let errors = ctx
            .errors
            .iter()
            .map(|e| TypeErrorEntry::new(db, e.clone()))
            .collect();
        let module_typecheck_result = TypecheckResult::new(db, script, errors, ctx.expr_types);

        for statement in script.statements(db) {
            if let Statement::Fun(func_stmt) = statement {
                let analysis = crate::function_analysis::analyze_function(db, *func_stmt, module_typecheck_result);
                function_analyses.push((*func_stmt, analysis));
            }
        }
    }

    ModuleGraphTypecheckResult::new(db, graph, module_errors, module_exports_map, module_imports_map, function_analyses)
}

/// Collect function signature without checking body (first pass).
pub fn collect_function_signature<'db>(
    ctx: &mut TypeContext<'db>,
    stmt: &StmtFun<'db>,
) {
    let db = ctx.db;
    let name = stmt.name(db);
    let params = stmt.params(db);
    let return_type = stmt.return_type(db);

    // Convert parameter types.
    let mut param_types = Vec::new();
    for param in params {
        match convert_type_hint(db, param.type_hint(db)) {
            Ok(ty) => param_types.push(ty),
            Err(e) => {
                ctx.add_error(e);
                return;
            }
        }
    }

    // Convert return type (default to Void if not specified).
    let ret_ty = match return_type {
        Some(type_hint) => {
            match convert_type_hint(db, type_hint) {
                Ok(ty) => ty,
                Err(e) => {
                    ctx.add_error(e);
                    return;
                }
            }
        }
        None => {
            // Functions without explicit return type default to void.
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, Type::Void)
        }
    };

    // Create function type and add to context.
    let func_type = TypeFunction::new(db, param_types, ret_ty);
    ctx.add_function(name, func_type);
}

/// Check a statement.
fn check_statement<'db>(
    ctx: &mut TypeContext<'db>,
    statement: &Statement<'db>,
) {
    let db = ctx.db;

    match statement {
        Statement::Let(stmt) => {
            let name = stmt.name(db);
            let value = stmt.value(db);

            // If type hint is provided, check against it.
            // Otherwise, synthesize type from value.
            let var_type = match stmt.type_hint(db) {
                Some(type_hint) => {
                    // Convert type hint to expected type and check value.
                    match convert_type_hint(db, type_hint) {
                        Ok(expected_type) => {
                            match check_expr(ctx, value, expected_type) {
                                Ok(()) => Some(expected_type),
                                Err(e) => {
                                    ctx.add_error(e);
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            ctx.add_error(e);
                            None
                        }
                    }
                }
                None => {
                    // Synthesize type from value.
                    match ctx.synthesize_expr(value) {
                        Ok(ty) => Some(ty),
                        Err(e) => {
                            ctx.add_error(e);
                            None
                        }
                    }
                }
            };

            // Add variable to context if we got a type.
            if let Some(ty) = var_type {
                ctx.add_variable(name, ty);
            }
        }

        Statement::Fun(stmt) => {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let body = stmt.body(db);

            // Function signature should already be collected in first pass.
            // Look it up to get param types and return type.
            let func_type = match ctx.lookup_function(name) {
                Some(func_type) => func_type,
                None => {
                    // Function not in context (shouldn't happen in normal flow).
                    // Collect signature now for error resilience.
                    collect_function_signature(ctx, stmt);
                    match ctx.lookup_function(name) {
                        Some(func_type) => func_type,
                        None => return, // Errors already recorded.
                    }
                }
            };

            let param_types = func_type.param_types(db);
            let ret_ty = func_type.return_type(db);

            // Create new context for function body with parameters in scope.
            let saved_variables = ctx.variables.clone();
            let saved_return_type = ctx.expected_return_type;

            // Add parameters to context.
            for (param, param_ty) in params.iter().zip(param_types.iter()) {
                ctx.add_variable(param.name(db), *param_ty);
            }

            ctx.expected_return_type = Some(ret_ty);

            // Check function body.
            for stmt in body {
                check_statement(ctx, stmt);
            }

            // Restore context.
            ctx.variables = saved_variables;
            ctx.expected_return_type = saved_return_type;
        }

        Statement::Ret(stmt) => {
            let value = stmt.value(db);

            // Check return value against expected return type.
            if let Some(expected_ret_ty) = ctx.expected_return_type {
                if let Err(e) = check_expr(ctx, value, expected_ret_ty) {
                    ctx.add_error(e);
                }
            } else {
                // F011: Cannot synthesize return type.
                ctx.add_error(ctx.error_cannot_synthesize(value, "cannot infer return type"));
            }
        }

        Statement::Require(_) => {
            // TODO: implement require type checking.
        }

        Statement::Import(_) => {
            // TODO: implement import type checking.
            // This will be handled in typecheck_package_world.
        }

        Statement::If(stmt) => {
            let condition = stmt.condition(db);
            let then_binding = stmt.then_binding(db);
            let then_body = stmt.then_body(db);
            let else_binding = stmt.else_binding(db);
            let else_body = stmt.else_body(db);

            // If there's a binding, this is destructuring syntax.
            if let Some(binding_name) = then_binding {
                // Synthesize the condition type.
                let condition_ty = match ctx.synthesize_expr(condition) {
                    Ok(ty) => ty,
                    Err(e) => {
                        ctx.add_error(e);
                        return;
                    }
                };

                // Extract inner type from Option or Result.
                let inner_ty = match condition_ty.ty(db) {
                    Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                        opt.inner_type(db)
                    }
                    Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                        // F046: Result destructuring requires error-binding else branch.
                        if else_body.is_none() || else_binding.is_none() {
                            ctx.add_error(ctx.error_result_requires_binding(condition));
                            return;
                        }
                        res.inner_type(db)
                    }
                    _ => {
                        // F017: If/match condition type mismatch.
                        ctx.add_error(ctx.error_type_mismatch(
                            condition,
                            "Option or Result",
                            "other type",
                            "expected Option or Result type for destructuring"
                        ));
                        return;
                    }
                };

                // Convert datalit TypeAndHeap to datafun TypeAndHeap.
                let inner_heap = inner_ty.heap(db);
                let inner_type = Type::Datalit(inner_ty.ty(db).clone());
                let binding_ty = TypeAndHeap::new(db, inner_heap, inner_type);

                // Add binding to context for then body.
                ctx.variables.insert(binding_name, binding_ty);

                // Type check then body.
                for stmt in then_body {
                    check_statement(ctx, stmt);
                }

                // Remove binding from context.
                ctx.variables.remove(&binding_name);

                // Type check else body if present.
                if let Some(else_stmts) = else_body {
                    // If there's an else binding, bind error type for Result.
                    if let Some(else_binding_name) = else_binding {
                        if let Type::Datalit(datalit::tycheck::Type::Result(_)) = condition_ty.ty(db) {
                            // Bind Error type.
                            let error_ty = TypeAndHeap::new(
                                db,
                                datalit::ast::Heap::Omitted,
                                Type::Datalit(datalit::tycheck::Type::Error),
                            );
                            ctx.variables.insert(else_binding_name, error_ty);

                            for stmt in else_stmts {
                                check_statement(ctx, stmt);
                            }

                            ctx.variables.remove(&else_binding_name);
                        } else {
                            let actual_type = type_to_string(db, condition_ty.ty(db));
                            ctx.add_error(ctx.error_type_mismatch(
                                condition,
                                "Result",
                                &actual_type,
                                "else binding requires Result type"
                            ));
                        }
                    } else {
                        for stmt in else_stmts {
                            check_statement(ctx, stmt);
                        }
                    }
                }
            } else {
                // No binding: check condition is bool type.
                let bool_type = TypeAndHeap::new(
                    db,
                    datalit::ast::Heap::Omitted,
                    Type::Datalit(datalit::tycheck::Type::Bool),
                );

                if let Err(e) = check_expr(ctx, condition, bool_type) {
                    ctx.add_error(e);
                }

                // Type check then body.
                for stmt in then_body {
                    check_statement(ctx, stmt);
                }

                // Type check else body if present.
                if let Some(else_stmts) = else_body {
                    for stmt in else_stmts {
                        check_statement(ctx, stmt);
                    }
                }
            }
        }

        Statement::Loop(stmt) => {
            // Increment loop depth for body.
            ctx.loop_depth += 1;

            // Type check loop body.
            for body_stmt in stmt.body(db) {
                check_statement(ctx, body_stmt);
            }

            // Restore loop depth.
            ctx.loop_depth -= 1;
        }

        Statement::Break(_) => {
            if ctx.loop_depth == 0 {
                ctx.add_error(TypeError::BreakOutsideLoop);
            }
            // Break is valid - no further type checking needed.
        }

        Statement::Continue(_) => {
            if ctx.loop_depth == 0 {
                ctx.add_error(TypeError::ContinueOutsideLoop);
            }
            // Continue is valid - no further type checking needed.
        }

        Statement::ParseError(_) => {
            // Skip parse errors.
        }
    }
}

/// Synthesize a type for an expression.
fn synthesize_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    match expr_kind {
        ExprFunKind::Name(name) => {
            // F001: Undefined variable.
            ctx.lookup_variable(name)
                .ok_or_else(|| ctx.error_undefined_variable(expr, name))
        }

        ExprFunKind::BinOp(binop) => {
            synthesize_binop(ctx, expr, binop)
        }

        ExprFunKind::UnaryOp(unaryop) => {
            synthesize_unaryop(ctx, expr, unaryop)
        }

        ExprFunKind::FunctionCall(call) => {
            synthesize_function_call(ctx, expr, call)
        }

        ExprFunKind::Tuple(tuple) => {
            // Synthesize type for each element.
            let elements = tuple.elements(db);
            let mut datalit_element_types = Vec::new();

            for elem in elements {
                let elem_ty = ctx.synthesize_expr(*elem)?;

                // Extract datalit TypeAndHeap from datafun TypeAndHeap.
                // Tuple elements must be datalit types.
                match elem_ty.ty(db) {
                    Type::Datalit(datalit_ty) => {
                        let datalit_elem_ty = datalit::tycheck::TypeAndHeap::new(
                            db,
                            elem_ty.heap(db),
                            datalit_ty.clone(),
                        );
                        datalit_element_types.push(datalit_elem_ty);
                    }
                    _ => {
                        return Err(ctx.error_invalid_tuple_element(
                            *elem,
                            &type_to_string(db, elem_ty.ty(db))
                        ));
                    }
                }
            }

            // Create datalit tuple type.
            let datalit_tuple_ty = datalit::tycheck::Type::AnonTuple(
                datalit::tycheck::TypeAnonTuple::new(db, datalit_element_types)
            );

            // Wrap in datafun type.
            // Use Heap::Omitted since tuple heap is determined by element heaps.
            let ty = Type::Datalit(datalit_tuple_ty);
            Ok(TypeAndHeap::new(db, datalit::ast::Heap::Omitted, ty))
        }

        ExprFunKind::TryOption(try_op) => {
            synthesize_try_option(ctx, expr, try_op)
        }

        ExprFunKind::TryResult(try_op) => {
            synthesize_try_result(ctx, expr, try_op)
        }

        ExprFunKind::ParseError(_) => {
            Err(ctx.error_cannot_synthesize(expr, "cannot type check parse error"))
        }

        // New inline variants - simple literals.
        // All these check for type hints first.
        ExprFunKind::True(lit) => {
            if let Some(type_hint) = lit.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            let heap = lit.heap(db);
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(TypeAndHeap::new(db, heap, ty))
        }
        ExprFunKind::False(lit) => {
            if let Some(type_hint) = lit.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            let heap = lit.heap(db);
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(TypeAndHeap::new(db, heap, ty))
        }
        ExprFunKind::None(lit) => {
            // None requires type hint to determine the inner type.
            if let Some(type_hint) = lit.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            Err(ctx.error_cannot_synthesize(expr, "cannot infer type for None value"))
        }
        ExprFunKind::Int(int_expr) => {
            // If type hint present, use it and validate the value fits.
            if let Some(type_hint) = int_expr.type_hint(db) {
                let result_ty = convert_type_hint(db, type_hint)?;
                // Validate integer value fits within the type.
                let value_str = int_expr.value(db).as_str(db);
                if let Type::Datalit(datalit_ty) = result_ty.ty(db) {
                    check_int_fits_wrapped_type(value_str, datalit_ty, db)?;
                }
                // Validate heap compatibility between type hint and expression.
                let expected_heap = unwrap_wrapper_heap(db, result_ty);
                let actual_heap = int_expr.heap(db);
                if !heaps_compatible(expected_heap, actual_heap) {
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(expected_heap),
                        actual_heap: heap_to_string(actual_heap),
                    });
                }
                return Ok(result_ty);
            }
            let heap = int_expr.heap(db);
            let value_str = int_expr.value(db).as_str(db);
            // Parse as u32 by default.
            if value_str.parse::<u32>().is_ok() {
                let ty = Type::Datalit(datalit::tycheck::Type::U32);
                Ok(TypeAndHeap::new(db, heap, ty))
            } else if value_str.parse::<i32>().is_ok() {
                let ty = Type::Datalit(datalit::tycheck::Type::I32);
                Ok(TypeAndHeap::new(db, heap, ty))
            } else {
                Err(ctx.error_cannot_synthesize(expr, "integer literal out of range"))
            }
        }
        ExprFunKind::Float(float_expr) => {
            if let Some(type_hint) = float_expr.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            let heap = float_expr.heap(db);
            let ty = Type::Datalit(datalit::tycheck::Type::F32);
            Ok(TypeAndHeap::new(db, heap, ty))
        }
        ExprFunKind::Hex(hex_expr) => {
            // If type hint present, use it and validate the value fits.
            if let Some(type_hint) = hex_expr.type_hint(db) {
                let result_ty = convert_type_hint(db, type_hint)?;
                // Validate hex value fits within the type.
                let value_str = hex_expr.value(db).as_str(db);
                if let Type::Datalit(datalit_ty) = result_ty.ty(db) {
                    check_hex_fits_wrapped_type(value_str, datalit_ty, db)?;
                }
                // Validate heap compatibility between type hint and expression.
                let expected_heap = unwrap_wrapper_heap(db, result_ty);
                let actual_heap = hex_expr.heap(db);
                if !heaps_compatible(expected_heap, actual_heap) {
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(expected_heap),
                        actual_heap: heap_to_string(actual_heap),
                    });
                }
                return Ok(result_ty);
            }
            let heap = hex_expr.heap(db);
            let value_str = hex_expr.value(db).as_str(db);
            let hex_part = value_str.trim_start_matches('-').trim_start_matches("0x").trim_start_matches("0X");
            if u32::from_str_radix(hex_part, 16).is_ok() && !value_str.starts_with('-') {
                let ty = Type::Datalit(datalit::tycheck::Type::U32);
                Ok(TypeAndHeap::new(db, heap, ty))
            } else {
                Err(ctx.error_cannot_synthesize(expr, "hex literal out of range"))
            }
        }
        ExprFunKind::String(str_expr) => {
            if let Some(type_hint) = str_expr.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            let heap = str_expr.heap(db);
            let ty = Type::Datalit(datalit::tycheck::Type::String);
            Ok(TypeAndHeap::new(db, heap, ty))
        }

        // Collection types.
        ExprFunKind::List(list_expr) => {
            if let Some(type_hint) = list_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches heap mismatches).
                check_list_elements(ctx, list_expr.elements(db), expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_list(ctx, expr, list_expr)
        }
        ExprFunKind::Set(set_expr) => {
            if let Some(type_hint) = set_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches heap mismatches).
                check_set_elements(ctx, set_expr.elements(db), expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_set(ctx, expr, set_expr)
        }
        ExprFunKind::Map(map_expr) => {
            if let Some(type_hint) = map_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check entries against expected type (catches heap mismatches).
                check_map_entries(ctx, map_expr.entries(db), expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_map(ctx, expr, map_expr)
        }
        ExprFunKind::Tensor(tensor_expr) => {
            if let Some(type_hint) = tensor_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check rank and elements against expected type.
                check_tensor_shape_and_elements(ctx, tensor_expr, expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_tensor(ctx, expr, tensor_expr)
        }

        // Aggregate types.
        ExprFunKind::AnonTuple(tuple_expr) => {
            if let Some(type_hint) = tuple_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches arity mismatches).
                check_tuple_elements(ctx, tuple_expr.elements(db), expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_anon_tuple(ctx, expr, tuple_expr)
        }
        ExprFunKind::AnonStruct(struct_expr) => {
            if let Some(type_hint) = struct_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check fields against expected type (catches arity mismatches).
                check_struct_fields(ctx, struct_expr.fields(db), expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_anon_struct(ctx, expr, struct_expr)
        }
        ExprFunKind::AnonEnum(enum_expr) => {
            if let Some(type_hint) = enum_expr.type_hint(db) {
                let expected_ty = convert_type_hint(db, type_hint)?;
                check_enum_variant(ctx, enum_expr.variant_name(db), enum_expr.payload(db), &expected_ty)?;
                return Ok(expected_ty);
            }
            Err(ctx.error_cannot_synthesize(expr, "anonymous enum requires type hint"))
        }

        // Wrapper types.
        ExprFunKind::Data(data_expr) => {
            if let Some(type_hint) = data_expr.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            synthesize_inline_data(ctx, expr, data_expr)
        }
        ExprFunKind::Err(err_expr) => {
            if let Some(type_hint) = err_expr.type_hint(db) {
                return convert_type_hint(db, type_hint);
            }
            synthesize_inline_err(ctx, expr, err_expr)
        }
    }
}

/// Synthesize type for binary operation.
fn synthesize_binop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    binop: ExprBinOp<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let op = binop.op(db);
    let lhs = binop.lhs(db);
    let rhs = binop.rhs(db);

    // Synthesize types for operands.
    let lhs_ty = ctx.synthesize_expr(lhs)?;
    let rhs_ty = ctx.synthesize_expr(rhs)?;

    // Check that operands have the same type.
    if !types_equivalent(db, lhs_ty.ty(db), rhs_ty.ty(db)) {
        return Err(ctx.error_type_mismatch(
            expr,
            &type_to_string(db, lhs_ty.ty(db)),
            &type_to_string(db, rhs_ty.ty(db)),
            "operands must have the same type"
        ));
    }

    // Check that operands are numeric types.
    let operand_ty = lhs_ty.ty(db);
    if !is_numeric_type(operand_ty) {
        return Err(ctx.error_invalid_operand_type(
            expr,
            &format!("{:?}", op),
            &type_to_string(db, operand_ty)
        ));
    }

    // Determine result type based on operator.
    use BinOp::*;
    let result_ty = match op {
        // Basic arithmetic: floats, bigints, and fixed ints (which widen to int).
        Add | Sub | Mul => {
            if is_float_type(operand_ty) {
                // Floats return float.
                lhs_ty
            } else if is_bigint_type(operand_ty) {
                // Bigints return bigint.
                lhs_ty
            } else if is_fixed_int_type(operand_ty) {
                // Fixed ints widen to int.
                let int_ty = Type::Datalit(datalit::tycheck::Type::Int);
                TypeAndHeap::new(db, datalit::ast::Heap::Omitted, int_ty)
            } else {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_ty)
                ));
            }
        }

        // Bare division: only floats (bigints must use /! or /?).
        Div => {
            if !is_float_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_ty)
                ));
            }
            lhs_ty
        }

        // Checked arithmetic: only fixed ints, plus division for bigints.
        // Checked operators yield element type directly (not wrapped in Result).
        // On overflow, the function early-returns with an error.
        AddChecked | SubChecked | MulChecked => {
            if !is_fixed_int_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_ty)
                ));
            }
            // Verify we're inside a function with Result return type.
            let op_str = format!("{:?}", op);
            let expected_return = ctx.expected_return_type
                .ok_or_else(|| ctx.error_try_outside_function(expr, &op_str))?;
            match expected_return.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                    // OK, function returns Result type.
                }
                _ => {
                    return Err(ctx.error_try_return_type_mismatch(
                        expr,
                        &op_str,
                        "Result",
                        &type_to_string(db, expected_return.ty(db))
                    ));
                }
            }
            // Return element type directly.
            lhs_ty
        }

        DivChecked => {
            // Division: fixed ints or bigints.
            if !is_fixed_int_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_ty)
                ));
            }
            // Verify we're inside a function with Result return type.
            let op_str = format!("{:?}", op);
            let expected_return = ctx.expected_return_type
                .ok_or_else(|| ctx.error_try_outside_function(expr, &op_str))?;
            match expected_return.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                    // OK, function returns Result type.
                }
                _ => {
                    return Err(ctx.error_try_return_type_mismatch(
                        expr,
                        &op_str,
                        "Result",
                        &type_to_string(db, expected_return.ty(db))
                    ));
                }
            }
            // Return element type directly.
            lhs_ty
        }

        // Optional arithmetic: only fixed ints, early-returns None on overflow.
        AddOptional | SubOptional | MulOptional => {
            if !is_fixed_int_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_ty)
                ));
            }
            // Verify we're inside a function with Option return type.
            let op_str = format!("{:?}", op);
            let expected_return = ctx.expected_return_type
                .ok_or_else(|| ctx.error_try_outside_function(expr, &op_str))?;
            match expected_return.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // OK, function returns Option type.
                }
                _ => {
                    return Err(ctx.error_try_return_type_mismatch(
                        expr,
                        &op_str,
                        "Option",
                        &type_to_string(db, expected_return.ty(db))
                    ));
                }
            }
            // Return element type directly.
            lhs_ty
        }

        DivOptional => {
            // Division: fixed ints or bigints, early-returns None on overflow/div0.
            if !is_fixed_int_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_ty)
                ));
            }
            // Verify we're inside a function with Option return type.
            let op_str = format!("{:?}", op);
            let expected_return = ctx.expected_return_type
                .ok_or_else(|| ctx.error_try_outside_function(expr, &op_str))?;
            match expected_return.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // OK, function returns Option type.
                }
                _ => {
                    return Err(ctx.error_try_return_type_mismatch(
                        expr,
                        &op_str,
                        "Option",
                        &type_to_string(db, expected_return.ty(db))
                    ));
                }
            }
            // Return element type directly.
            lhs_ty
        }

        // Comparison: bool.
        Lt | Gt | Le | Ge | Eq | Ne => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, bool_ty)
        }
    };

    Ok(result_ty)
}

/// Synthesize type for unary operation.
fn synthesize_unaryop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    unaryop: ExprUnaryOp<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let op = unaryop.op(db);
    let operand = unaryop.operand(db);

    // Synthesize type for operand.
    let operand_ty = ctx.synthesize_expr(operand)?;

    // Check that operand is a numeric type.
    let operand_type = operand_ty.ty(db);
    if !is_numeric_type(operand_type) {
        return Err(ctx.error_invalid_operand_type(
            expr,
            &format!("{:?}", op),
            &type_to_string(db, operand_type)
        ));
    }

    // Determine result type based on operator.
    let result_ty = match op {
        // Bare negation: only floats and bigints.
        UnaryOp::Neg => {
            if !is_float_type(operand_type) && !is_bigint_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_type)
                ));
            }
            operand_ty
        }

        // Optional negation: only fixed ints, and not unsigned.
        // Returns element type directly; on overflow, early-returns None.
        UnaryOp::NegOptional => {
            if !is_fixed_int_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_type)
                ));
            }
            // Disallow -? for unsigned ints (footgun).
            if is_unsigned_int_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_type)
                ));
            }
            // Verify we're inside a function with Option return type.
            let op_str = format!("{:?}", op);
            let expected_return = ctx.expected_return_type
                .ok_or_else(|| ctx.error_try_outside_function(expr, &op_str))?;
            match expected_return.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // OK, function returns Option type.
                }
                _ => {
                    return Err(ctx.error_try_return_type_mismatch(
                        expr,
                        &op_str,
                        "Option",
                        &type_to_string(db, expected_return.ty(db))
                    ));
                }
            }
            // Return element type directly (wrapping happens at function boundary).
            operand_ty
        }

        // Result negation: only fixed ints.
        // Returns element type directly; on overflow, early-returns Err.
        UnaryOp::NegResult => {
            if !is_fixed_int_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    &format!("{:?}", op),
                    &type_to_string(db, operand_type)
                ));
            }
            // Verify we're inside a function with Result return type.
            let op_str = format!("{:?}", op);
            let expected_return = ctx.expected_return_type
                .ok_or_else(|| ctx.error_try_outside_function(expr, &op_str))?;
            match expected_return.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                    // OK, function returns Result type.
                }
                _ => {
                    return Err(ctx.error_try_return_type_mismatch(
                        expr,
                        &op_str,
                        "Result",
                        &type_to_string(db, expected_return.ty(db))
                    ));
                }
            }
            // Return element type directly (wrapping happens at function boundary).
            operand_ty
        }
    };

    Ok(result_ty)
}

/// Synthesize type for function call.
fn synthesize_function_call<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    call: ExprFunctionCall<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let name = call.name(db);
    let args = call.args(db);

    // F002: Undefined function.
    let func_type = ctx.lookup_function(name)
        .ok_or_else(|| ctx.error_undefined_function(expr, name))?;

    let param_types = func_type.param_types(db);
    let return_type = func_type.return_type(db);

    // F045: Function arity mismatch.
    if args.len() != param_types.len() {
        return Err(ctx.error_arity_mismatch(expr, param_types.len(), args.len()));
    }

    // Check each argument type.
    for (arg, expected_param_ty) in args.iter().zip(param_types.iter()) {
        check_expr(ctx, *arg, *expected_param_ty)?;
    }

    // Return the function's return type.
    Ok(return_type)
}

/// Synthesize type for try-option operator (?).
fn synthesize_try_option<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    try_op: ExprTryOption<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let operand = try_op.operand(db);

    // Verify we're inside a function.
    let expected_return = ctx.expected_return_type
        .ok_or_else(|| ctx.error_try_outside_function(expr, "?"))?;

    // Synthesize operand type.
    let operand_ty = ctx.synthesize_expr(operand)?;

    // Operand must be Option<T>.
    let inner_ty = match operand_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            opt.inner_type(db)
        }
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "?",
                "Option",
                &type_to_string(db, operand_ty.ty(db))
            ));
        }
    };

    // Function return type must be Option<U> for some U.
    match expected_return.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(_)) => {
            // OK, function returns Option type.
        }
        _ => {
            return Err(ctx.error_try_return_type_mismatch(
                expr,
                "?",
                "Option",
                &type_to_string(db, expected_return.ty(db))
            ));
        }
    }

    // Return the unwrapped type T.
    let heap = inner_ty.heap(db);
    let ty = Type::Datalit(inner_ty.ty(db).clone());
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for try-result operator (!).
fn synthesize_try_result<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    try_op: ExprTryResult<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let operand = try_op.operand(db);

    // Verify we're inside a function.
    let expected_return = ctx.expected_return_type
        .ok_or_else(|| ctx.error_try_outside_function(expr, "!"))?;

    // Synthesize operand type.
    let operand_ty = ctx.synthesize_expr(operand)?;

    // Operand must be Result<T>.
    let inner_ty = match operand_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            res.inner_type(db)
        }
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "!",
                "Result",
                &type_to_string(db, operand_ty.ty(db))
            ));
        }
    };

    // Function return type must be Result<U> for some U.
    match expected_return.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Result(_)) => {
            // OK, function returns Result type.
        }
        _ => {
            return Err(ctx.error_try_return_type_mismatch(
                expr,
                "!",
                "Result",
                &type_to_string(db, expected_return.ty(db))
            ));
        }
    }

    // Return the unwrapped type T.
    let heap = inner_ty.heap(db);
    let ty = Type::Datalit(inner_ty.ty(db).clone());
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Check if a type is numeric.
fn is_numeric_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(
                datalit_ty,
                datalit::tycheck::Type::U8 |
                datalit::tycheck::Type::I8 |
                datalit::tycheck::Type::U16 |
                datalit::tycheck::Type::I16 |
                datalit::tycheck::Type::U32 |
                datalit::tycheck::Type::I32 |
                datalit::tycheck::Type::U64 |
                datalit::tycheck::Type::I64 |
                datalit::tycheck::Type::F32 |
                datalit::tycheck::Type::Int
            )
        }
        Type::Function(_) => false,
        Type::Void => false,
    }
}

fn is_float_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(datalit_ty, datalit::tycheck::Type::F32)
        }
        _ => false,
    }
}

fn is_bigint_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(datalit_ty, datalit::tycheck::Type::Int)
        }
        _ => false,
    }
}

fn is_fixed_int_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(
                datalit_ty,
                datalit::tycheck::Type::U8 |
                datalit::tycheck::Type::I8 |
                datalit::tycheck::Type::U16 |
                datalit::tycheck::Type::I16 |
                datalit::tycheck::Type::U32 |
                datalit::tycheck::Type::I32 |
                datalit::tycheck::Type::U64 |
                datalit::tycheck::Type::I64
            )
        }
        _ => false,
    }
}

fn is_unsigned_int_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(
                datalit_ty,
                datalit::tycheck::Type::U8 |
                datalit::tycheck::Type::U16 |
                datalit::tycheck::Type::U32 |
                datalit::tycheck::Type::U64
            )
        }
        _ => false,
    }
}

/// Check an expression against an expected type.
fn check_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    expected: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    match expr_kind {
        // Handle None literals specially - they can check against any Option type.
        ExprFunKind::None(_lit) => {
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // None checks against any Option<T>.
                    ctx.store_expr_type(expr, expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, "None", "None requires Option type"))
                }
            }
        }

        // Handle integer literals specially - they can coerce to expected integer types.
        ExprFunKind::Int(_int_expr) => {
            match expected.ty(db) {
                Type::Datalit(expected_datalit_ty) => {
                    // Check if expected type is a numeric type.
                    if is_numeric_type(expected.ty(db)) {
                        ctx.store_expr_type(expr, expected);
                        return Ok(());
                    }
                    // If expected is Option<numeric>, allow coercion.
                    if let datalit::tycheck::Type::Option(opt) = expected_datalit_ty {
                        let inner_ty = opt.inner_type(db);
                        if is_numeric_type(&Type::Datalit(inner_ty.ty(db).clone())) {
                            ctx.store_expr_type(expr, expected);
                            return Ok(());
                        }
                    }
                    // If expected is Data, allow coercion (any type coerces to data).
                    if let datalit::tycheck::Type::Data = expected_datalit_ty {
                        ctx.store_expr_type(expr, expected);
                        return Ok(());
                    }
                    // Otherwise, synthesize and compare.
                    let synthesized = ctx.synthesize_expr(expr)?;
                    if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected.ty(db));
                        let actual_str = type_to_string(db, synthesized.ty(db));
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // For other non-datalit expressions, use synthesis + comparison with coercion support.
        _ => {
            let synthesized = ctx.synthesize_expr(expr)?;

            // Check for exact type match first.
            if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                return Ok(());
            }

            // If exact match fails, try numeric widening.
            if let (Type::Datalit(synth_ty), Type::Datalit(expect_ty)) = (synthesized.ty(db), expected.ty(db)) {
                if datalit::tycheck::can_widen_to(synth_ty, expect_ty) {
                    return Ok(());
                }
            }

            // If widening fails, check for automatic coercion to Option/Result/Data.
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                    // Allow coercion from T to Option<T>.
                    let inner_ty = opt.inner_type(db);
                    if types_equivalent(db, synthesized.ty(db), &Type::Datalit(inner_ty.ty(db).clone())) {
                        return Ok(());
                    }
                }
                Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                    // Allow coercion from T to Result<T>.
                    let inner_ty = res.inner_type(db);
                    if types_equivalent(db, synthesized.ty(db), &Type::Datalit(inner_ty.ty(db).clone())) {
                        return Ok(());
                    }
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to data.
                    return Ok(());
                }
                _ => {}
            }

            // Check if synthesized is Data and expected is Option<data> or Result<data>.
            if let Type::Datalit(datalit::tycheck::Type::Data) = synthesized.ty(db) {
                match expected.ty(db) {
                    Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                        if let datalit::tycheck::Type::Data = opt.inner_type(db).ty(db) {
                            return Ok(());
                        }
                    }
                    Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                        if let datalit::tycheck::Type::Data = res.inner_type(db).ty(db) {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }

            // No match or coercion possible.
            let expected_str = type_to_string(db, expected.ty(db));
            let actual_str = type_to_string(db, synthesized.ty(db));
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
        }
    }
}

/// Convert a datalit type hint to a datafun type.
pub fn convert_type_hint<'db>(
    db: &'db dyn crate::Db,
    type_hint_and_heap: datalit::ast::TypeHintAndHeap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    // Delegate to datalit's convert_type_hint.
    let datalit_ty = datalit::tycheck::convert_type_hint(db, type_hint_and_heap)
        .map_err(TypeError::from)?;

    let heap = datalit_ty.heap(db);
    let ty = Type::Datalit(datalit_ty.ty(db).clone());

    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Check if an integer value fits within a given type.
/// Returns Ok(()) if the value fits, Err(IntOutOfRange) if not.
fn check_int_fits_type(value_str: &str, ty: &datalit::tycheck::Type<'_>) -> Result<(), TypeError> {
    match ty {
        datalit::tycheck::Type::U8 => {
            value_str.parse::<u8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I8 => {
            value_str.parse::<i8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::U16 => {
            value_str.parse::<u16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I16 => {
            value_str.parse::<i16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::U32 => {
            value_str.parse::<u32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I32 => {
            value_str.parse::<i32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::U64 => {
            value_str.parse::<u64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I64 => {
            value_str.parse::<i64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::Int => {
            // Int is arbitrary precision, always fits.
            Ok(())
        }
        _ => Ok(()), // Non-integer types don't need range checking.
    }
}

/// Check if an integer value fits within the innermost integer type of a possibly wrapped type.
/// Handles Option<u8>, Result<u8>, etc.
fn check_int_fits_wrapped_type(value_str: &str, ty: &datalit::tycheck::Type<'_>, db: &dyn crate::Db) -> Result<(), TypeError> {
    match ty {
        datalit::tycheck::Type::Option(opt) => {
            check_int_fits_wrapped_type(value_str, opt.inner_type(db).ty(db), db)
        }
        datalit::tycheck::Type::Result(res) => {
            check_int_fits_wrapped_type(value_str, res.inner_type(db).ty(db), db)
        }
        _ => check_int_fits_type(value_str, ty),
    }
}

/// Check if a hex value fits within a given type.
fn check_hex_fits_type(value_str: &str, ty: &datalit::tycheck::Type<'_>) -> Result<(), TypeError> {
    let is_negative = value_str.starts_with('-');
    let hex_part = value_str
        .trim_start_matches('-')
        .trim_start_matches("0x")
        .trim_start_matches("0X");

    match ty {
        datalit::tycheck::Type::U8 if !is_negative => {
            u8::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I8 => {
            // For signed types, parse as unsigned first then check range.
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 128 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 127 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::U16 if !is_negative => {
            u16::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I16 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 32768 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 32767 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::U32 if !is_negative => {
            u32::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I32 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 2147483648 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 2147483647 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::U64 if !is_negative => {
            u64::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I64 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 9223372036854775808 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 9223372036854775807 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::Int => Ok(()), // Arbitrary precision.
        _ if is_negative => Err(TypeError::IntOutOfRange), // Unsigned type with negative value.
        _ => Ok(()), // Non-integer types.
    }
}

/// Check if a hex value fits within the innermost integer type of a possibly wrapped type.
fn check_hex_fits_wrapped_type(value_str: &str, ty: &datalit::tycheck::Type<'_>, db: &dyn crate::Db) -> Result<(), TypeError> {
    match ty {
        datalit::tycheck::Type::Option(opt) => {
            check_hex_fits_wrapped_type(value_str, opt.inner_type(db).ty(db), db)
        }
        datalit::tycheck::Type::Result(res) => {
            check_hex_fits_wrapped_type(value_str, res.inner_type(db).ty(db), db)
        }
        _ => check_hex_fits_type(value_str, ty),
    }
}

/// Unwrap Option/Result types to get the innermost heap.
/// Used when checking heap compatibility for typed literals.
fn unwrap_wrapper_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            unwrap_wrapper_heap_datalit(db, opt.inner_type(db))
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            unwrap_wrapper_heap_datalit(db, res.inner_type(db))
        }
        _ => ty.heap(db),
    }
}

/// Unwrap Option/Result types from a datalit TypeAndHeap.
fn unwrap_wrapper_heap_datalit<'db>(
    db: &'db dyn crate::Db,
    ty: datalit::tycheck::TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        datalit::tycheck::Type::Option(opt) => {
            unwrap_wrapper_heap_datalit(db, opt.inner_type(db))
        }
        datalit::tycheck::Type::Result(res) => {
            unwrap_wrapper_heap_datalit(db, res.inner_type(db))
        }
        _ => ty.heap(db),
    }
}

/// Check if two heaps are compatible.
/// Omitted heap is generic and compatible with any heap.
fn heaps_compatible(h1: datalit::ast::Heap, h2: datalit::ast::Heap) -> bool {
    use datalit::ast::Heap;
    match (h1, h2) {
        (Heap::Local, Heap::Local) => true,
        (Heap::Global, Heap::Global) => true,
        // Omitted is compatible with any heap (generic).
        (Heap::Omitted, _) => true,
        (_, Heap::Omitted) => true,
        _ => false,
    }
}

/// Convert a heap to a string for error messages.
fn heap_to_string(heap: datalit::ast::Heap) -> String {
    use datalit::ast::Heap;
    match heap {
        Heap::Local => "@".to_string(),
        Heap::Global => "#".to_string(),
        Heap::Omitted => "".to_string(),
    }
}

/// Extract the outer heap from an expression.
///
/// Returns the heap sigil used on the expression itself (e.g. `@` in `@{...}`).
/// Returns `Heap::Omitted` for expressions that don't have an explicit heap.
fn get_expr_heap<'db>(db: &'db dyn crate::Db, expr: ExprFun<'db>) -> datalit::ast::Heap {
    use crate::ast::ExprFunKind;
    match expr.expr(db) {
        ExprFunKind::True(lit) | ExprFunKind::False(lit) | ExprFunKind::None(lit) => lit.heap(db),
        ExprFunKind::Int(e) => e.heap(db),
        ExprFunKind::Float(e) => e.heap(db),
        ExprFunKind::Hex(e) => e.heap(db),
        ExprFunKind::String(e) => e.heap(db),
        ExprFunKind::List(e) => e.heap(db),
        ExprFunKind::Set(e) => e.heap(db),
        ExprFunKind::Map(e) => e.heap(db),
        ExprFunKind::Tensor(e) => e.heap(db),
        ExprFunKind::AnonTuple(e) => e.heap(db),
        ExprFunKind::AnonStruct(e) => e.heap(db),
        ExprFunKind::AnonEnum(e) => e.heap(db),
        ExprFunKind::Data(e) => e.heap(db),
        ExprFunKind::Err(e) => e.heap(db),
        // Non-literal expressions don't have an outer heap.
        _ => datalit::ast::Heap::Omitted,
    }
}

/// Check if two types are equivalent.
fn types_equivalent<'db>(db: &'db dyn crate::Db, t1: &Type<'db>, t2: &Type<'db>) -> bool {
    match (t1, t2) {
        (Type::Datalit(d1), Type::Datalit(d2)) => {
            datalit::tycheck::types_equivalent(db, d1, d2)
        }
        (Type::Function(f1), Type::Function(f2)) => {
            // Check parameter types.
            let p1 = f1.param_types(db);
            let p2 = f2.param_types(db);
            if p1.len() != p2.len() {
                return false;
            }
            for (param1, param2) in p1.iter().zip(p2.iter()) {
                if !types_equivalent(db, param1.ty(db), param2.ty(db)) {
                    return false;
                }
            }

            // Check return type.
            types_equivalent(db, f1.return_type(db).ty(db), f2.return_type(db).ty(db))
        }
        _ => false,
    }
}

/// Convert a type to a string for error messages.
pub fn type_to_string<'db>(db: &'db dyn crate::Db, ty: &Type<'db>) -> String {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::type_to_string(db, datalit_ty),
        Type::Function(func) => {
            let params: Vec<_> = func
                .param_types(db)
                .iter()
                .map(|p| type_to_string(db, p.ty(db)))
                .collect();
            let ret = type_to_string(db, func.return_type(db).ty(db));
            format!("({}) -> {}", params.join(", "), ret)
        }
        Type::Void => "void".to_string(),
    }
}

/// Build module alias map from require module statements for ModuleGraph.
///
/// Maps module aliases to ModuleIds by parsing require statements and matching
/// against the path_to_id map.
fn build_module_alias_map_for_graph<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    path_to_id: &HashMap<String, crate::module_graph::ModuleId>,
) -> HashMap<InternedText<'db>, crate::module_graph::ModuleId> {
    let mut alias_map = HashMap::new();

    for statement in script.statements(db) {
        if let Statement::Require(StmtRequire::Module(req)) = statement {
            let import_space = req.import_space(db);
            let package_alias = req.package_alias(db);
            let module_alias = req.module_alias(db);

            // Build the module path from the require statement.
            let path = format!(
                "{}/{}/{}",
                import_space.as_str(db),
                package_alias.as_str(db),
                module_alias.as_str(db)
            );

            // Look up the ModuleId by path.
            if let Some(&module_id) = path_to_id.get(&path) {
                alias_map.insert(module_alias, module_id);
            }
        }
    }

    alias_map
}


/// Collect function signatures exported from a module.
fn collect_module_exports<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
) -> Vec<(InternedText<'db>, TypeFunction<'db>)> {
    let mut functions = Vec::new();

    // Collect all top-level function signatures.
    for statement in script.statements(db) {
        if let Statement::Fun(stmt) = statement {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let return_type = stmt.return_type(db);

            // Convert parameter types.
            let mut param_types = Vec::new();
            let mut has_error = false;
            for param in params {
                match convert_type_hint(db, param.type_hint(db)) {
                    Ok(ty) => param_types.push(ty),
                    Err(_) => {
                        has_error = true;
                        break;
                    }
                }
            }

            if has_error {
                continue;
            }

            // Convert return type (default to Void if not specified).
            let ret_ty = match return_type {
                Some(type_hint) => {
                    match convert_type_hint(db, type_hint) {
                        Ok(ty) => ty,
                        Err(_) => continue,
                    }
                }
                None => {
                    // Functions without explicit return type default to void.
                    TypeAndHeap::new(db, datalit::ast::Heap::Omitted, Type::Void)
                }
            };

            // Create function type.
            let func_type = TypeFunction::new(db, param_types, ret_ty);
            functions.push((name, func_type));
        }
    }

    functions
}

// Helper functions for synthesizing inline expression types.

/// Convert datafun TypeAndHeap to datalit TypeAndHeap.
fn to_datalit_type_and_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> Result<datalit::tycheck::TypeAndHeap<'db>, TypeError> {
    match ty.ty(db) {
        Type::Datalit(datalit_ty) => {
            Ok(datalit::tycheck::TypeAndHeap::new(db, ty.heap(db), datalit_ty.clone()))
        }
        _ => Err(TypeError::CannotSynthesize),
    }
}

/// Synthesize type for inline list expression.
fn synthesize_inline_list<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    list_expr: crate::ast::ExprList<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = list_expr.heap(db);
    let elements = list_expr.elements(db);

    if elements.is_empty() {
        // Empty list - can't synthesize element type.
        // Default to List<()>.
        let elem_ty = datalit::tycheck::TypeAndHeap::new(
            db, heap, datalit::tycheck::Type::AnonTuple(datalit::tycheck::TypeAnonTuple::new(db, vec![]))
        );
        let ty = Type::Datalit(datalit::tycheck::Type::List(
            datalit::tycheck::TypeList::new(db, elem_ty)
        ));
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    // Synthesize type of first element.
    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = to_datalit_type_and_heap(db, first_ty)?;

    // Check remaining elements for type and heap compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        // Check heap compatibility.
        if !heaps_compatible(first_ty.heap(db), elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(first_ty.heap(db)),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }
    }

    let ty = Type::Datalit(datalit::tycheck::Type::List(
        datalit::tycheck::TypeList::new(db, first_datalit)
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline set expression.
fn synthesize_inline_set<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    set_expr: crate::ast::ExprSet<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = set_expr.heap(db);
    let elements = set_expr.elements(db);

    if elements.is_empty() {
        let elem_ty = datalit::tycheck::TypeAndHeap::new(
            db, heap, datalit::tycheck::Type::AnonTuple(datalit::tycheck::TypeAnonTuple::new(db, vec![]))
        );
        let ty = Type::Datalit(datalit::tycheck::Type::Set(
            datalit::tycheck::TypeSet::new(db, elem_ty)
        ));
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = to_datalit_type_and_heap(db, first_ty)?;

    // Check remaining elements for heap compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        if !heaps_compatible(first_ty.heap(db), elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(first_ty.heap(db)),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Set(
        datalit::tycheck::TypeSet::new(db, first_datalit)
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline map expression.
fn synthesize_inline_map<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    map_expr: crate::ast::ExprMap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = map_expr.heap(db);
    let entries = map_expr.entries(db);

    if entries.is_empty() {
        let unit_ty = datalit::tycheck::TypeAndHeap::new(
            db, heap, datalit::tycheck::Type::AnonTuple(datalit::tycheck::TypeAnonTuple::new(db, vec![]))
        );
        let ty = Type::Datalit(datalit::tycheck::Type::Map(
            datalit::tycheck::TypeMap::new(db, unit_ty.clone(), unit_ty)
        ));
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    let first_key_ty = ctx.synthesize_expr(entries[0].key(db))?;
    let first_key_datalit = to_datalit_type_and_heap(db, first_key_ty)?;
    let first_value_ty = ctx.synthesize_expr(entries[0].value(db))?;
    let first_value_datalit = to_datalit_type_and_heap(db, first_value_ty)?;

    // Check remaining entries for heap compatibility.
    for entry in &entries[1..] {
        let key_ty = ctx.synthesize_expr(entry.key(db))?;
        let value_ty = ctx.synthesize_expr(entry.value(db))?;
        // Check key heap compatibility.
        if !heaps_compatible(first_key_ty.heap(db), key_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(first_key_ty.heap(db)),
                actual_heap: heap_to_string(key_ty.heap(db)),
            });
        }
        // Check value heap compatibility.
        if !heaps_compatible(first_value_ty.heap(db), value_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(first_value_ty.heap(db)),
                actual_heap: heap_to_string(value_ty.heap(db)),
            });
        }
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Map(
        datalit::tycheck::TypeMap::new(db, first_key_datalit, first_value_datalit)
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline tensor expression.
fn synthesize_inline_tensor<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    tensor_expr: crate::ast::ExprTensor<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = tensor_expr.heap(db);
    let shape = tensor_expr.shape(db);
    let elements = tensor_expr.elements(db);

    // Rank is the number of dimensions in the shape.
    let rank = shape.len() as u32;

    if elements.is_empty() {
        let elem_ty = datalit::tycheck::TypeAndHeap::new(
            db, heap, datalit::tycheck::Type::F32
        );
        let ty = Type::Datalit(datalit::tycheck::Type::Tensor(
            datalit::tycheck::TypeTensor::new(db, elem_ty, rank)
        ));
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = to_datalit_type_and_heap(db, first_ty)?;

    // Check remaining elements for heap compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        if !heaps_compatible(first_ty.heap(db), elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(first_ty.heap(db)),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Tensor(
        datalit::tycheck::TypeTensor::new(db, first_datalit, rank)
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline anonymous tuple.
fn synthesize_inline_anon_tuple<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    tuple_expr: crate::ast::ExprAnonTuple<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = tuple_expr.heap(db);
    let elements = tuple_expr.elements(db);

    let mut elem_types = Vec::new();
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        let elem_datalit = to_datalit_type_and_heap(db, elem_ty)?;
        elem_types.push(elem_datalit);
    }

    let ty = Type::Datalit(datalit::tycheck::Type::AnonTuple(
        datalit::tycheck::TypeAnonTuple::new(db, elem_types)
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline anonymous struct.
fn synthesize_inline_anon_struct<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    struct_expr: crate::ast::ExprAnonStruct<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = struct_expr.heap(db);
    let fields = struct_expr.fields(db);

    let mut field_types = Vec::new();
    for field in fields {
        let field_ty = ctx.synthesize_expr(field.value(db))?;
        let field_datalit = to_datalit_type_and_heap(db, field_ty)?;
        field_types.push(datalit::tycheck::TypeNamedField::new(db, field.name(db), field_datalit));
    }

    let ty = Type::Datalit(datalit::tycheck::Type::AnonStruct(
        datalit::tycheck::TypeAnonStruct::new(db, field_types)
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline data expression.
fn synthesize_inline_data<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    data_expr: crate::ast::ExprData<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = data_expr.heap(db);
    let value = data_expr.value(db);

    // Type-check the inner value.
    let _ = ctx.synthesize_expr(value)?;

    // Data synthesizes to Type::Data (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Data);
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline err expression.
fn synthesize_inline_err<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    err_expr: crate::ast::ExprErr<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = err_expr.heap(db);
    let value = err_expr.value(db);

    // Type-check the inner value.
    let _ = ctx.synthesize_expr(value)?;

    // Error synthesizes to Type::Error (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Error);
    Ok(TypeAndHeap::new(db, heap, ty))
}

// Helper functions for checking collection elements against expected types.

/// Unwrap Option/Result wrappers to get inner type.
/// Used to check collection elements when type hint includes Option/Result.
fn unwrap_wrapper_types<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> TypeAndHeap<'db> {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            // Convert datalit TypeAndHeap to datafun TypeAndHeap.
            let inner = opt.inner_type(db);
            let datafun_inner = TypeAndHeap::new(
                db,
                inner.heap(db),
                Type::Datalit(inner.ty(db).clone()),
            );
            unwrap_wrapper_types(db, datafun_inner)
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            // Convert datalit TypeAndHeap to datafun TypeAndHeap.
            let inner = res.inner_type(db);
            let datafun_inner = TypeAndHeap::new(
                db,
                inner.heap(db),
                Type::Datalit(inner.ty(db).clone()),
            );
            unwrap_wrapper_types(db, datafun_inner)
        }
        _ => ty,
    }
}

/// Error type for coercion failures.
enum CoercionError {
    TypeMismatch { expected: String, actual: String },
    ArityMismatch { expected: usize, actual: usize },
}

impl From<CoercionError> for TypeError {
    fn from(err: CoercionError) -> Self {
        match err {
            CoercionError::TypeMismatch { expected, actual } => {
                TypeError::TypeMismatch { expected, actual }
            }
            CoercionError::ArityMismatch { expected, actual } => {
                TypeError::ArityMismatch { expected, actual }
            }
        }
    }
}

/// Check if a type can be coerced to an expected type.
///
/// Follows datalit's coercion rules:
/// - T matches T
/// - T matches Option<T> (implicit Some wrapping)
/// - T matches Result<T> (implicit Ok wrapping)
fn check_type_coercion<'db>(
    db: &'db dyn crate::Db,
    actual: &datalit::tycheck::Type<'db>,
    expected: &datalit::tycheck::TypeAndHeap<'db>,
) -> Result<(), CoercionError> {
    let expected_ty = expected.ty(db);

    // Check exact match.
    if datalit::tycheck::types_equivalent(db, actual, expected_ty) {
        return Ok(());
    }

    // Check Option coercion: T can match Option<T>.
    if let datalit::tycheck::Type::Option(opt) = expected_ty {
        let inner = opt.inner_type(db);
        if datalit::tycheck::types_equivalent(db, actual, inner.ty(db)) {
            return Ok(());
        }
        // For error reporting, use the inner type since that's what datalit does.
        return check_type_arity_or_mismatch(db, actual, inner.ty(db));
    }

    // Check Result coercion: T can match Result<T>.
    if let datalit::tycheck::Type::Result(res) = expected_ty {
        let inner = res.inner_type(db);
        if datalit::tycheck::types_equivalent(db, actual, inner.ty(db)) {
            return Ok(());
        }
        // For error reporting, use the inner type since that's what datalit does.
        return check_type_arity_or_mismatch(db, actual, inner.ty(db));
    }

    // Check Data coercion: ANY type T can coerce to data.
    if let datalit::tycheck::Type::Data = expected_ty {
        return Ok(());
    }

    // Check Data can coerce to Option<data> or Result<data>.
    if let datalit::tycheck::Type::Data = actual {
        if let datalit::tycheck::Type::Option(opt) = expected_ty {
            if let datalit::tycheck::Type::Data = opt.inner_type(db).ty(db) {
                return Ok(());
            }
        }
        if let datalit::tycheck::Type::Result(res) = expected_ty {
            if let datalit::tycheck::Type::Data = res.inner_type(db).ty(db) {
                return Ok(());
            }
        }
    }

    // No coercion possible - check for arity mismatch.
    check_type_arity_or_mismatch(db, actual, expected_ty)
}

/// Check if two types have an arity mismatch (for structs/tuples).
/// Returns ArityMismatch if the types are the same kind but different arity.
/// Returns TypeMismatch otherwise.
fn check_type_arity_or_mismatch<'db>(
    db: &'db dyn crate::Db,
    actual: &datalit::tycheck::Type<'db>,
    expected: &datalit::tycheck::Type<'db>,
) -> Result<(), CoercionError> {
    // Check for struct arity mismatch.
    if let (
        datalit::tycheck::Type::AnonStruct(actual_struct),
        datalit::tycheck::Type::AnonStruct(expected_struct),
    ) = (actual, expected) {
        let actual_count = actual_struct.fields(db).len();
        let expected_count = expected_struct.fields(db).len();
        if actual_count != expected_count {
            return Err(CoercionError::ArityMismatch {
                expected: expected_count,
                actual: actual_count,
            });
        }
    }

    // Check for tuple arity mismatch.
    if let (
        datalit::tycheck::Type::AnonTuple(actual_tuple),
        datalit::tycheck::Type::AnonTuple(expected_tuple),
    ) = (actual, expected) {
        let actual_count = actual_tuple.fields(db).len();
        let expected_count = expected_tuple.fields(db).len();
        if actual_count != expected_count {
            return Err(CoercionError::ArityMismatch {
                expected: expected_count,
                actual: actual_count,
            });
        }
    }

    // Default to type mismatch.
    Err(CoercionError::TypeMismatch {
        expected: datalit::tycheck::type_to_string(db, expected),
        actual: datalit::tycheck::type_to_string(db, actual),
    })
}

/// Check list elements against expected type.
fn check_list_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual list type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the list type.
    let elem_type = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::List(list_ty)) => {
            list_ty.element_type(db)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check each element against expected element type.
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;

        // Extract actual datalit type.
        let actual_datalit_ty = match elem_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue, // Skip non-datalit types.
        };

        // Check type compatibility with coercion.
        if let Err(err) = check_type_coercion(db, actual_datalit_ty, &elem_type) {
            return Err(TypeError::from(err));
        }

        // Check heap compatibility (use inner heap for Option/Result).
        let expected_heap = unwrap_wrapper_heap_datalit(db, elem_type);
        if !heaps_compatible(expected_heap, elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }

        // Also check expression's outer heap (for cases where type hint provides heap
        // but the expression literal has a different heap, e.g. `@{...}` vs `#{...}`).
        let expr_heap = get_expr_heap(db, *elem);
        if !heaps_compatible(expected_heap, expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(expr_heap),
            });
        }
    }

    Ok(())
}

/// Check set elements against expected type.
fn check_set_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual set type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the set type.
    let elem_type = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Set(set_ty)) => {
            set_ty.element_type(db)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check each element against expected element type.
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;

        // Extract actual datalit type.
        let actual_datalit_ty = match elem_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue,
        };

        // Check type compatibility with coercion.
        if let Err(err) = check_type_coercion(db, actual_datalit_ty, &elem_type) {
            return Err(TypeError::from(err));
        }

        // Check heap compatibility.
        let expected_heap = unwrap_wrapper_heap_datalit(db, elem_type);
        if !heaps_compatible(expected_heap, elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }

        // Also check expression's outer heap.
        let expr_heap = get_expr_heap(db, *elem);
        if !heaps_compatible(expected_heap, expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(expr_heap),
            });
        }
    }

    Ok(())
}

/// Check map entries against expected type.
fn check_map_entries<'db>(
    ctx: &mut TypeContext<'db>,
    entries: &[crate::ast::ExprMapEntry<'db>],
    expected_ty: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual map type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the key and value types from the map type.
    let (key_type, value_type) = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Map(map_ty)) => {
            (map_ty.key_type(db), map_ty.value_type(db))
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check each entry against expected types.
    for entry in entries {
        let key_ty = ctx.synthesize_expr(entry.key(db))?;
        let value_ty = ctx.synthesize_expr(entry.value(db))?;

        // Extract actual datalit types.
        let actual_key_ty = match key_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue,
        };
        let actual_value_ty = match value_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue,
        };

        // Check key type compatibility with coercion.
        if let Err(err) = check_type_coercion(db, actual_key_ty, &key_type) {
            return Err(TypeError::from(err));
        }

        // Check key heap compatibility.
        let expected_key_heap = unwrap_wrapper_heap_datalit(db, key_type);
        if !heaps_compatible(expected_key_heap, key_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_key_heap),
                actual_heap: heap_to_string(key_ty.heap(db)),
            });
        }

        // Also check key expression's outer heap.
        let key_expr_heap = get_expr_heap(db, entry.key(db));
        if !heaps_compatible(expected_key_heap, key_expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_key_heap),
                actual_heap: heap_to_string(key_expr_heap),
            });
        }

        // Check value type compatibility with coercion.
        if let Err(err) = check_type_coercion(db, actual_value_ty, &value_type) {
            return Err(TypeError::from(err));
        }

        // Check value heap compatibility.
        let expected_value_heap = unwrap_wrapper_heap_datalit(db, value_type);
        if !heaps_compatible(expected_value_heap, value_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_value_heap),
                actual_heap: heap_to_string(value_ty.heap(db)),
            });
        }

        // Also check value expression's outer heap.
        let value_expr_heap = get_expr_heap(db, entry.value(db));
        if !heaps_compatible(expected_value_heap, value_expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_value_heap),
                actual_heap: heap_to_string(value_expr_heap),
            });
        }
    }

    Ok(())
}

/// Check tensor shape and elements against expected type.
fn check_tensor_shape_and_elements<'db>(
    ctx: &mut TypeContext<'db>,
    tensor_expr: crate::ast::ExprTensor<'db>,
    expected_ty: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let elements = tensor_expr.elements(db);
    let shape = tensor_expr.shape(db);

    // Unwrap Option/Result wrappers to get the actual tensor type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract tensor type info.
    let (elem_type, expected_rank) = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Tensor(tensor_ty)) => {
            (tensor_ty.element_type(db), tensor_ty.rank(db))
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check rank matches type hint.
    let actual_rank = shape.len() as u32;
    if actual_rank != expected_rank {
        return Err(TypeError::ArityMismatch {
            expected: expected_rank as usize,
            actual: actual_rank as usize,
        });
    }

    // Check element count matches shape product.
    let expected_count = shape.iter().map(|&d| d as usize).product::<usize>();
    if elements.len() != expected_count {
        return Err(TypeError::ArityMismatch {
            expected: expected_count,
            actual: elements.len(),
        });
    }

    // Check each element against expected element type.
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;

        // Extract actual datalit type.
        let actual_datalit_ty = match elem_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue,
        };

        // Check type compatibility with coercion.
        if let Err(err) = check_type_coercion(db, actual_datalit_ty, &elem_type) {
            return Err(TypeError::from(err));
        }

        // Check heap compatibility.
        let expected_heap = unwrap_wrapper_heap_datalit(db, elem_type);
        if !heaps_compatible(expected_heap, elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }

        // Also check expression's outer heap.
        let expr_heap = get_expr_heap(db, *elem);
        if !heaps_compatible(expected_heap, expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(expr_heap),
            });
        }
    }

    Ok(())
}

/// Check tuple elements against expected type.
fn check_tuple_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual tuple type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the tuple type.
    let expected_fields = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonTuple(tuple_ty)) => {
            tuple_ty.fields(db)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check arity.
    if elements.len() != expected_fields.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_fields.len(),
            actual: elements.len(),
        });
    }

    // Check each element against expected field type.
    for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
        let elem_ty = ctx.synthesize_expr(*elem)?;

        // Extract actual datalit type.
        let actual_datalit_ty = match elem_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue, // Skip non-datalit types.
        };

        // Check type compatibility with coercion.
        if let Err(err) = check_type_coercion(db, actual_datalit_ty, expected_field) {
            return Err(TypeError::from(err));
        }

        // Check heap compatibility.
        let expected_heap = unwrap_wrapper_heap_datalit(db, *expected_field);
        if !heaps_compatible(expected_heap, elem_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(elem_ty.heap(db)),
            });
        }

        // Also check expression's outer heap.
        let expr_heap = get_expr_heap(db, *elem);
        if !heaps_compatible(expected_heap, expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(expr_heap),
            });
        }
    }

    Ok(())
}

/// Check struct fields against expected type.
fn check_struct_fields<'db>(
    ctx: &mut TypeContext<'db>,
    fields: &[crate::ast::ExprStructField<'db>],
    expected_ty: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual struct type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the struct type.
    let expected_fields = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonStruct(struct_ty)) => {
            struct_ty.fields(db)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check arity.
    if fields.len() != expected_fields.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_fields.len(),
            actual: fields.len(),
        });
    }

    // Check each field against expected field type.
    for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
        // Check field name matches.
        let field_name = field.name(db);
        let expected_name = expected_field.name(db);
        if field_name != expected_name {
            return Err(TypeError::FieldOrderMismatch);
        }

        let field_value_ty = ctx.synthesize_expr(field.value(db))?;

        // Extract actual datalit type.
        let actual_datalit_ty = match field_value_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => continue, // Skip non-datalit types.
        };

        // Check type compatibility with coercion.
        let expected_field_ty = expected_field.ty(db);
        if let Err(err) = check_type_coercion(db, actual_datalit_ty, &expected_field_ty) {
            return Err(TypeError::from(err));
        }

        // Check heap compatibility.
        let expected_heap = unwrap_wrapper_heap_datalit(db, expected_field_ty);
        if !heaps_compatible(expected_heap, field_value_ty.heap(db)) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(field_value_ty.heap(db)),
            });
        }

        // Also check expression's outer heap.
        let expr_heap = get_expr_heap(db, field.value(db));
        if !heaps_compatible(expected_heap, expr_heap) {
            return Err(TypeError::HeapMismatch {
                expected_heap: heap_to_string(expected_heap),
                actual_heap: heap_to_string(expr_heap),
            });
        }
    }

    Ok(())
}

/// Check enum variant exists in type hint.
fn check_enum_variant<'db>(
    ctx: &mut TypeContext<'db>,
    variant_name: bct::text::InternedText<'db>,
    payload: Option<ExprFun<'db>>,
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual enum type.
    let inner_ty = unwrap_wrapper_types(db, *expected_ty);

    // Extract the variants from the enum type.
    let expected_variants = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonEnum(enum_ty)) => {
            enum_ty.variants(db)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Look up the variant by name.
    let expected_variant = expected_variants
        .iter()
        .find(|v| v.name(db) == variant_name)
        .ok_or_else(|| {
            TypeError::VariantNotFound(variant_name.as_str(db).to_string())
        })?;

    // Check payload type if both have payloads.
    // If payload presence differs (expression has payload but type hint doesn't, or vice versa),
    // don't fail here - let the type comparison at a higher level catch the mismatch.
    // This matches datalit's Check-TypedAnonEnum behavior which compares enum types rather
    // than individual variant payloads.
    if let (Some(payload_expr), Some(expected_payload_ty)) = (payload, expected_variant.payload(db)) {
        // Synthesize payload type and check against expected.
        let payload_ty = ctx.synthesize_expr(payload_expr)?;
        let actual_datalit_ty = match payload_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => return Ok(()), // Non-datalit types handled elsewhere.
        };
        if let Err(err) = check_type_coercion(db, actual_datalit_ty, &expected_payload_ty) {
            return Err(TypeError::from(err));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tracked compile helper that does full parse + typecheck pipeline.
    #[salsa::tracked]
    fn compile_for_test<'db>(
        db: &'db dyn crate::Db,
        source: bct::input::Source,
    ) -> TypecheckResult<'db> {
        let parse_result = crate::parser::parse(db, source);
        type_check(
            db,
            source,
            parse_result.script(db)
        )
    }

    #[test]
    fn test_tycheck_simple_let() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x: @u32 = @42"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_binop_add() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = a + b"));
        let tycheck_result = compile_for_test(&db, source);

        // Will have errors because 'a' and 'b' are unresolved.
        assert!(tycheck_result.errors(&db).len() > 0);
    }

    #[test]
    fn test_tycheck_binop_checked() {
        let db = crate::Database::default();
        // Checked operators can only be used inside functions with Result return type.
        // Using +! at top level should produce an error.
        let source = bct::input::Source::new(&db, S("let x = @1 +! @2"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have an error: checked operator outside function.
        assert!(tycheck_result.errors(&db).len() > 0);
    }

    #[test]
    fn test_tycheck_fun_simple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("fun foo(): @u32\n  ret @42\nend fun"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_fun_params() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("fun add(a: @int, b: @int): @int\n  ret a + b\nend fun"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_fun_checked() {
        let db = crate::Database::default();
        // Checked operators yield element type directly but require Result return type.
        // Function returns !@u32, and a +! b returns u32, which coerces to Ok(u32).
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): !@u32\n  ret a +! b\nend fun"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_type_mismatch() {
        let db = crate::Database::default();
        // Fun returns bool but body returns u32.
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): bool\n  ret a +! b\nend fun"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have a type mismatch error.
        assert!(tycheck_result.errors(&db).len() > 0);
    }

    // Tests for complex datalit expressions enabled by direct token parsing

    #[test]
    fn test_tycheck_datalit_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - tuple of integers.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[1, 2, 3]"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - list of integers.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_map() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @map { @1 = @10, @2 = @20 }"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - map with integer keys and integer values.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_nested_tuple_in_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[(1, 2), (3, 4)]"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - list of tuples.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_nested_list_in_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@1, @2, @3], @100)"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - tuple with nested list.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_set() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @set { @1, @2, @3 }"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - set of integers.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_deeply_nested() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - deeply nested structure.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_tuple_in_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[@(@1, @2), @(@3, @4), @(@5, @6)]"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors - list of tuples.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }
}
