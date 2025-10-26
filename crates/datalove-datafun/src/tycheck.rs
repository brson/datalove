use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
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

/// Exported function signatures from a module.
#[salsa::tracked]
pub struct ModuleExports<'db> {
    /// Package module this is for.
    pub package_module: bct::package2::PackageModule,

    /// Function signatures as a vector of (name, type) pairs.
    /// Using Vec because salsa::tracked requires Hash, which HashMap doesn't implement.
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,
}

/// Result of typechecking an entire package world.
#[salsa::tracked]
pub struct PackageWorldTypecheckResult<'db> {
    /// The package world module graph.
    pub graph: bct::package_resolve2::PackageWorldModuleGraph<'db>,

    /// Type errors encountered, per module.
    #[returns(ref)]
    pub module_errors: BTreeMap<bct::package2::PackageModule, Vec<TypeError>>,

    /// Module exports, per module.
    #[returns(ref)]
    pub module_exports: BTreeMap<bct::package2::PackageModule, ModuleExports<'db>>,
}

/// Context for typechecking.
pub struct TypeContext<'db> {
    db: &'db dyn crate::Db,
    /// Variable bindings (name -> type).
    variables: HashMap<InternedText<'db>, TypeAndHeap<'db>>,
    /// Function signatures (name -> function type).
    functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Expected return type for current function (if inside a function).
    expected_return_type: Option<TypeAndHeap<'db>>,
    errors: Vec<TypeError>,
    /// Expression types, indexed by ExprFun ID.
    expr_types: Vec<Option<TypeAndHeap<'db>>>,
    /// Expression spans for datafun expressions (for diagnostic emission).
    expr_spans: HashMap<salsa::Id, (bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
    /// Expression spans for datalit expressions (passed to datalit type checker).
    datalit_expr_spans: Vec<(datalit::ast::ExprFull<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
}

impl<'db> TypeContext<'db> {
    pub fn new(
        db: &'db dyn crate::Db,
        expr_spans: Vec<(ExprFun<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
        datalit_expr_spans: Vec<(datalit::ast::ExprFull<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
    ) -> Self {
        // Convert expr_spans Vec to HashMap for fast lookup.
        use salsa::plumbing::AsId;
        let expr_spans_map = expr_spans
            .into_iter()
            .map(|(expr, text, span)| (expr.as_id(), (text, span)))
            .collect();

        TypeContext {
            db,
            variables: HashMap::new(),
            functions: HashMap::new(),
            expected_return_type: None,
            errors: Vec::new(),
            expr_types: Vec::new(),
            expr_spans: expr_spans_map,
            datalit_expr_spans,
        }
    }

    fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    /// Look up the source location for an expression.
    fn get_span(&self, expr: ExprFun<'db>) -> Option<(bct::text::Text<'db>, datalove_diagnostic::ByteSpan)> {
        use salsa::plumbing::AsId;
        self.expr_spans.get(&expr.as_id()).cloned()
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
    script: Script<'db>,
    expr_spans: Vec<(ExprFun<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
    datalit_expr_spans: Vec<(datalit::ast::ExprFull<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, expr_spans, datalit_expr_spans);

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

/// Typecheck a script with package world support.
///
/// This version of type_check allows scripts to import functions from modules
/// in the package world.
#[salsa::tracked]
pub fn type_check_with_package_world<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    expr_spans: Vec<(ExprFun<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
    datalit_expr_spans: Vec<(datalit::ast::ExprFull<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
    package_world: crate::package::PackageWorld,
    package_world_typecheck: PackageWorldTypecheckResult<'db>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, expr_spans, datalit_expr_spans);

    // Build module alias map from require statements.
    let graph = package_world_typecheck.graph(db);
    let module_exports_map = package_world_typecheck.module_exports(db);
    let alias_map = build_script_module_alias_map(db, script, package_world, graph);

    // Process import statements to populate function signatures.
    for statement in script.statements(db) {
        if let Statement::Import(import) = statement {
            let module_name = import.module_name(db);
            let item_name = import.item_name(db);

            // Look up the module in the alias map.
            if let Some(&imported_module) = alias_map.get(&module_name) {
                // Look up the module exports.
                if let Some(exports) = module_exports_map.get(&imported_module) {
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
    let mut ctx = TypeContext::new(db, vec![], vec![]);

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

/// Typecheck an entire package world module graph.
///
/// This processes all modules in dependency order, allowing imports
/// to reference functions from already-typechecked modules.
#[salsa::tracked]
pub fn typecheck_package_world<'db>(
    db: &'db dyn crate::Db,
    graph: bct::package_resolve2::PackageWorldModuleGraph<'db>,
) -> PackageWorldTypecheckResult<'db> {
    let mut module_errors: BTreeMap<bct::package2::PackageModule, Vec<TypeError>> = BTreeMap::new();
    let mut module_exports_map: BTreeMap<bct::package2::PackageModule, ModuleExports<'db>> = BTreeMap::new();

    // Sort modules in dependency order.
    let sorted_modules = match topological_sort_modules(db, graph) {
        Ok(modules) => modules,
        Err(e) => {
            // If we can't sort, just process modules in arbitrary order.
            // Errors will be recorded per module.
            graph.map(db).keys().copied().collect()
        }
    };

    // Process each module in dependency order.
    for module in sorted_modules {
        // Parse the module.
        let source = module.text(db);
        let parse_result = crate::parser::parse(db, source);
        let script = parse_result.script;

        // Build module alias map for this module.
        let alias_map = build_module_alias_map(db, script, graph, module);

        // Create type context for this module.
        let mut ctx = TypeContext::new(db, parse_result.expr_spans.to_vec(), parse_result.datalit_expr_spans.to_vec());

        // Add imported functions to context.
        // Scan for import statements and resolve them.
        for statement in script.statements(db) {
            if let Statement::Import(import) = statement {
                let module_name = import.module_name(db);
                let item_name = import.item_name(db);

                // Look up the module in the alias map.
                if let Some(&imported_module) = alias_map.get(&module_name) {
                    // Look up the module exports.
                    if let Some(exports) = module_exports_map.get(&imported_module) {
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
                        // Module not found in exports (might be a cycle or missing).
                        ctx.add_error(TypeError::UnresolvedName(
                            format!("module {}", module_name.as_str(db))
                        ));
                    }
                } else {
                    // Module alias not found.
                    ctx.add_error(TypeError::UnresolvedName(
                        format!("module alias {}", module_name.as_str(db))
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
            module_errors.insert(module, ctx.errors.clone());
        }

        // Collect exports for this module.
        let exports_functions = collect_module_exports(db, script);
        let exports = ModuleExports::new(db, module, exports_functions);
        module_exports_map.insert(module, exports);
    }

    PackageWorldTypecheckResult::new(db, graph, module_errors, module_exports_map)
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
                ctx.add_error(TypeError::CannotSynthesize);
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
                        // Result destructuring requires error-binding else branch.
                        if else_body.is_none() || else_binding.is_none() {
                            ctx.add_error(TypeError::ResultRequiresErrorBinding);
                            return;
                        }
                        res.inner_type(db)
                    }
                    _ => {
                        ctx.add_error(TypeError::TypeMismatch {
                            expected: "Option or Result".S(),
                            actual: "other type".S(),
                        });
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
                            ctx.add_error(TypeError::TypeMismatch {
                                expected: "Result type for else binding".S(),
                                actual: "other type".S(),
                            });
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
        ExprFunKind::Datalit(datalit_expr) => {
            // Delegate to datalit type checker.
            // Pass datalit expr_spans collected during parsing for diagnostic emission.
            let resolved = datalit::resolve::resolve_names(db, datalit_expr, ctx.datalit_expr_spans.clone());
            let tycheck_result = datalit::tycheck::type_check(db, datalit_expr, resolved);

            // Check for errors.
            if !tycheck_result.errors(db).is_empty() {
                let first_error = &tycheck_result.errors(db)[0];
                let error_msg = format!("{:?}", first_error.error(db));
                return Err(TypeError::DatalitError(error_msg));
            }

            // Get the synthesized type.
            match tycheck_result.root_type(db) {
                Some(datalit_ty) => {
                    // Convert datalit TypeAndHeap to datafun TypeAndHeap.
                    let heap = datalit_ty.heap(db);
                    let ty = Type::Datalit(datalit_ty.ty(db).clone());
                    Ok(TypeAndHeap::new(db, heap, ty))
                }
                None => Err(TypeError::CannotSynthesize),
            }
        }

        ExprFunKind::Name(name) => {
            // Look up variable in context.
            ctx.lookup_variable(name)
                .ok_or_else(|| TypeError::UnresolvedName(name.as_str(db).to_string()))
        }

        ExprFunKind::BinOp(binop) => {
            synthesize_binop(ctx, binop)
        }

        ExprFunKind::UnaryOp(unaryop) => {
            synthesize_unaryop(ctx, unaryop)
        }

        ExprFunKind::FunctionCall(call) => {
            synthesize_function_call(ctx, call)
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
                        return Err(TypeError::InvalidTupleElement {
                            ty: type_to_string(db, elem_ty.ty(db)),
                        });
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
            synthesize_try_option(ctx, try_op)
        }

        ExprFunKind::TryResult(try_op) => {
            synthesize_try_result(ctx, try_op)
        }

        ExprFunKind::ParseError(_) => {
            Err(TypeError::CannotSynthesize)
        }
    }
}

/// Synthesize type for binary operation.
fn synthesize_binop<'db>(
    ctx: &mut TypeContext<'db>,
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
        return Err(TypeError::TypeMismatch {
            expected: type_to_string(db, lhs_ty.ty(db)),
            actual: type_to_string(db, rhs_ty.ty(db)),
        });
    }

    // Check that operands are numeric types.
    let operand_ty = lhs_ty.ty(db);
    if !is_numeric_type(operand_ty) {
        return Err(TypeError::InvalidOperandType {
            op: format!("{:?}", op),
            ty: type_to_string(db, operand_ty),
        });
    }

    // Determine result type based on operator.
    use BinOp::*;
    let result_ty = match op {
        // Basic arithmetic: only floats and bigints.
        Add | Sub | Mul => {
            if !is_float_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_ty),
                });
            }
            lhs_ty
        }

        // Bare division: only floats (bigints must use /! or /?).
        Div => {
            if !is_float_type(operand_ty) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_ty),
                });
            }
            lhs_ty
        }

        // Checked arithmetic: only fixed ints, plus division for bigints.
        AddChecked | SubChecked | MulChecked => {
            if !is_fixed_int_type(operand_ty) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_ty),
                });
            }
            // Convert datafun TypeAndHeap to datalit TypeAndHeap.
            let lhs_datalit_ty = match lhs_ty.ty(db) {
                Type::Datalit(dt) => datalit::tycheck::TypeAndHeap::new(db, lhs_ty.heap(db), dt.clone()),
                _ => {
                    return Err(TypeError::InvalidOperandType {
                        op: format!("{:?}", op),
                        ty: type_to_string(db, lhs_ty.ty(db)),
                    });
                }
            };
            let result_inner = datalit::tycheck::TypeResult::new(db, lhs_datalit_ty);
            let result_ty = Type::Datalit(datalit::tycheck::Type::Result(result_inner));
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, result_ty)
        }

        DivChecked => {
            // Division: fixed ints or bigints.
            if !is_fixed_int_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_ty),
                });
            }
            // Convert datafun TypeAndHeap to datalit TypeAndHeap.
            let lhs_datalit_ty = match lhs_ty.ty(db) {
                Type::Datalit(dt) => datalit::tycheck::TypeAndHeap::new(db, lhs_ty.heap(db), dt.clone()),
                _ => {
                    return Err(TypeError::InvalidOperandType {
                        op: format!("{:?}", op),
                        ty: type_to_string(db, lhs_ty.ty(db)),
                    });
                }
            };
            let result_inner = datalit::tycheck::TypeResult::new(db, lhs_datalit_ty);
            let result_ty = Type::Datalit(datalit::tycheck::Type::Result(result_inner));
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, result_ty)
        }

        // Optional arithmetic: only fixed ints, plus division for bigints.
        AddOptional | SubOptional | MulOptional => {
            if !is_fixed_int_type(operand_ty) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_ty),
                });
            }
            // Convert datafun TypeAndHeap to datalit TypeAndHeap.
            let lhs_datalit_ty = match lhs_ty.ty(db) {
                Type::Datalit(dt) => datalit::tycheck::TypeAndHeap::new(db, lhs_ty.heap(db), dt.clone()),
                _ => {
                    return Err(TypeError::InvalidOperandType {
                        op: format!("{:?}", op),
                        ty: type_to_string(db, lhs_ty.ty(db)),
                    });
                }
            };
            let option_inner = datalit::tycheck::TypeOption::new(db, lhs_datalit_ty);
            let option_ty = Type::Datalit(datalit::tycheck::Type::Option(option_inner));
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, option_ty)
        }

        DivOptional => {
            // Division: fixed ints or bigints.
            if !is_fixed_int_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_ty),
                });
            }
            // Convert datafun TypeAndHeap to datalit TypeAndHeap.
            let lhs_datalit_ty = match lhs_ty.ty(db) {
                Type::Datalit(dt) => datalit::tycheck::TypeAndHeap::new(db, lhs_ty.heap(db), dt.clone()),
                _ => {
                    return Err(TypeError::InvalidOperandType {
                        op: format!("{:?}", op),
                        ty: type_to_string(db, lhs_ty.ty(db)),
                    });
                }
            };
            let option_inner = datalit::tycheck::TypeOption::new(db, lhs_datalit_ty);
            let option_ty = Type::Datalit(datalit::tycheck::Type::Option(option_inner));
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, option_ty)
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
        return Err(TypeError::InvalidOperandType {
            op: format!("{:?}", op),
            ty: type_to_string(db, operand_type),
        });
    }

    // Determine result type based on operator.
    let result_ty = match op {
        // Bare negation: only floats and bigints.
        UnaryOp::Neg => {
            if !is_float_type(operand_type) && !is_bigint_type(operand_type) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_type),
                });
            }
            operand_ty
        }

        // Optional negation: only fixed ints, and not unsigned.
        UnaryOp::NegOptional => {
            if !is_fixed_int_type(operand_type) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_type),
                });
            }
            // Disallow -? for unsigned ints (footgun).
            if is_unsigned_int_type(operand_type) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_type),
                });
            }
            // Convert datafun TypeAndHeap to datalit TypeAndHeap.
            let operand_datalit_ty = match operand_ty.ty(db) {
                Type::Datalit(dt) => datalit::tycheck::TypeAndHeap::new(db, operand_ty.heap(db), dt.clone()),
                _ => {
                    return Err(TypeError::InvalidOperandType {
                        op: format!("{:?}", op),
                        ty: type_to_string(db, operand_ty.ty(db)),
                    });
                }
            };
            let option_inner = datalit::tycheck::TypeOption::new(db, operand_datalit_ty);
            let option_ty = Type::Datalit(datalit::tycheck::Type::Option(option_inner));
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, option_ty)
        }

        // Result negation: only fixed ints.
        UnaryOp::NegResult => {
            if !is_fixed_int_type(operand_type) {
                return Err(TypeError::InvalidOperandType {
                    op: format!("{:?}", op),
                    ty: type_to_string(db, operand_type),
                });
            }
            // Convert datafun TypeAndHeap to datalit TypeAndHeap.
            let operand_datalit_ty = match operand_ty.ty(db) {
                Type::Datalit(dt) => datalit::tycheck::TypeAndHeap::new(db, operand_ty.heap(db), dt.clone()),
                _ => {
                    return Err(TypeError::InvalidOperandType {
                        op: format!("{:?}", op),
                        ty: type_to_string(db, operand_ty.ty(db)),
                    });
                }
            };
            let result_inner = datalit::tycheck::TypeResult::new(db, operand_datalit_ty);
            let result_ty = Type::Datalit(datalit::tycheck::Type::Result(result_inner));
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, result_ty)
        }
    };

    Ok(result_ty)
}

/// Synthesize type for function call.
fn synthesize_function_call<'db>(
    ctx: &mut TypeContext<'db>,
    call: ExprFunctionCall<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let name = call.name(db);
    let args = call.args(db);

    // Look up function signature.
    let func_type = ctx.lookup_function(name)
        .ok_or_else(|| TypeError::UnresolvedName(name.as_str(db).to_string()))?;

    let param_types = func_type.param_types(db);
    let return_type = func_type.return_type(db);

    // Check argument count.
    if args.len() != param_types.len() {
        return Err(TypeError::ArityMismatch {
            expected: param_types.len(),
            actual: args.len(),
        });
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
    try_op: ExprTryOption<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let operand = try_op.operand(db);

    // Verify we're inside a function.
    let expected_return = ctx.expected_return_type
        .ok_or_else(|| TypeError::TryOutsideFunction { operator: "?".to_string() })?;

    // Synthesize operand type.
    let operand_ty = ctx.synthesize_expr(operand)?;

    // Operand must be Option<T>.
    let inner_ty = match operand_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            opt.inner_type(db)
        }
        _ => {
            return Err(TypeError::TryTypeMismatch {
                operator: "?".to_string(),
                actual_type: type_to_string(db, operand_ty.ty(db)),
            });
        }
    };

    // Function return type must be Option<U> for some U.
    match expected_return.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(_)) => {
            // OK, function returns Option type.
        }
        _ => {
            return Err(TypeError::TryReturnTypeMismatch {
                operator: "?".to_string(),
                return_type: type_to_string(db, expected_return.ty(db)),
            });
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
    try_op: ExprTryResult<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let operand = try_op.operand(db);

    // Verify we're inside a function.
    let expected_return = ctx.expected_return_type
        .ok_or_else(|| TypeError::TryOutsideFunction { operator: "!".to_string() })?;

    // Synthesize operand type.
    let operand_ty = ctx.synthesize_expr(operand)?;

    // Operand must be Result<T>.
    let inner_ty = match operand_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            res.inner_type(db)
        }
        _ => {
            return Err(TypeError::TryTypeMismatch {
                operator: "!".to_string(),
                actual_type: type_to_string(db, operand_ty.ty(db)),
            });
        }
    };

    // Function return type must be Result<U> for some U.
    match expected_return.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Result(_)) => {
            // OK, function returns Result type.
        }
        _ => {
            return Err(TypeError::TryReturnTypeMismatch {
                operator: "!".to_string(),
                return_type: type_to_string(db, expected_return.ty(db)),
            });
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
        ExprFunKind::Datalit(datalit_expr) => {
            // If expected type is a datalit type, use bidirectional typing.
            if let Type::Datalit(expected_datalit_ty) = expected.ty(db) {
                // Convert datafun TypeAndHeap back to datalit TypeAndHeap.
                let expected_datalit = datalit::tycheck::TypeAndHeap::new(
                    db,
                    expected.heap(db),
                    expected_datalit_ty.clone(),
                );

                // Resolve names and type check with expected type.
                let resolved = datalit::resolve::resolve_names(db, datalit_expr, vec![]);
                let tycheck_result = datalit::tycheck::type_check_with_expected(
                    db,
                    datalit_expr,
                    resolved,
                    Some(expected_datalit),
                );

                // Check for errors.
                if !tycheck_result.errors(db).is_empty() {
                    let first_error = &tycheck_result.errors(db)[0];
                    let error_msg = format!("{:?}", first_error.error(db));
                    return Err(TypeError::DatalitError(error_msg));
                }

                Ok(())
            } else {
                // Expected type is not a datalit type (e.g., function type).
                // Fall back to synthesis + comparison.
                let synthesized = ctx.synthesize_expr(expr)?;
                if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                    Ok(())
                } else {
                    Err(TypeError::TypeMismatch {
                        expected: type_to_string(db, expected.ty(db)),
                        actual: type_to_string(db, synthesized.ty(db)),
                    })
                }
            }
        }

        // For non-datalit expressions, use synthesis + comparison with coercion support.
        _ => {
            let synthesized = ctx.synthesize_expr(expr)?;

            // Check for exact type match first.
            if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                return Ok(());
            }

            // If exact match fails, check for automatic coercion to Option/Result.
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
                _ => {}
            }

            // No match or coercion possible.
            Err(TypeError::TypeMismatch {
                expected: type_to_string(db, expected.ty(db)),
                actual: type_to_string(db, synthesized.ty(db)),
            })
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
        .map_err(|e| TypeError::DatalitError(format!("{:?}", e)))?;

    let heap = datalit_ty.heap(db);
    let ty = Type::Datalit(datalit_ty.ty(db).clone());

    Ok(TypeAndHeap::new(db, heap, ty))
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

/// Topologically sort modules by dependencies.
/// Returns modules in dependency order (leaves first).
fn topological_sort_modules<'db>(
    db: &'db dyn crate::Db,
    graph: bct::package_resolve2::PackageWorldModuleGraph<'db>,
) -> Result<Vec<bct::package2::PackageModule>, TypeError> {
    use std::collections::VecDeque;

    let map = graph.map(db);

    // Build dependency maps:
    // dependencies: module -> set of modules it depends on
    // dependents: module -> set of modules that depend on it
    let mut dependencies: BTreeMap<bct::package2::PackageModule, BTreeSet<bct::package2::PackageModule>> = BTreeMap::new();
    let mut dependents: BTreeMap<bct::package2::PackageModule, BTreeSet<bct::package2::PackageModule>> = BTreeMap::new();

    for &module in map.keys() {
        dependencies.insert(module, BTreeSet::new());
        dependents.insert(module, BTreeSet::new());
    }

    for (&module, deps) in map.iter() {
        for (_, resolved) in deps.iter() {
            if let bct::package_resolve2::ResolvedPackageModule::Resolved(dep_module) = resolved {
                dependencies.get_mut(&module).unwrap().insert(*dep_module);
                dependents.get_mut(dep_module).unwrap().insert(module);
            }
        }
    }

    // Calculate in-degree for each module (number of dependencies).
    let mut in_degree: HashMap<bct::package2::PackageModule, usize> = HashMap::new();
    for (&module, deps) in &dependencies {
        in_degree.insert(module, deps.len());
    }

    // Start with modules that have no dependencies (in-degree 0).
    let mut queue: VecDeque<bct::package2::PackageModule> = VecDeque::new();
    for (&module, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(module);
        }
    }

    let mut sorted = Vec::new();

    while let Some(module) = queue.pop_front() {
        sorted.push(module);

        // For each module that depends on this one, decrease its in-degree.
        if let Some(dependent_modules) = dependents.get(&module) {
            for &dependent in dependent_modules {
                let degree = in_degree.get_mut(&dependent).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(dependent);
                }
            }
        }
    }

    // If sorted doesn't contain all modules, there's a cycle (shouldn't happen with validated graph).
    if sorted.len() != map.len() {
        return Err(TypeError::DatalitError("cycle in module dependencies".to_string()));
    }

    Ok(sorted)
}

/// Build module alias map from require module statements.
/// Maps module aliases to their resolved PackageModules.
fn build_module_alias_map<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    graph: bct::package_resolve2::PackageWorldModuleGraph<'db>,
    current_module: bct::package2::PackageModule,
) -> HashMap<InternedText<'db>, bct::package2::PackageModule> {
    let mut alias_map = HashMap::new();

    // Get the dependencies for this module from the graph.
    let map = graph.map(db);
    if let Some(deps) = map.get(&current_module) {
        // Build a map from import demands to resolved modules.
        let mut demand_to_module = HashMap::new();
        for (demand, resolved) in deps {
            if let bct::package_resolve2::ResolvedPackageModule::Resolved(module) = resolved {
                demand_to_module.insert(demand, *module);
            }
        }

        // Scan script for require module statements and map aliases.
        for statement in script.statements(db) {
            if let Statement::Require(StmtRequire::Module(req)) = statement {
                let import_space = req.import_space(db);
                let package_alias = req.package_alias(db);
                let module_alias = req.module_alias(db);

                let demand = (
                    import_space.as_str(db).S(),
                    package_alias.as_str(db).S(),
                    module_alias.as_str(db).S(),
                );

                if let Some(&resolved_module) = demand_to_module.get(&demand) {
                    alias_map.insert(module_alias, resolved_module);
                }
            }
        }
    }

    alias_map
}

/// Build module alias map for a script (not a module in package world).
///
/// This maps module aliases (from require statements) to actual package modules.
pub fn build_script_module_alias_map<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    package_world: crate::package::PackageWorld,
    _graph: bct::package_resolve2::PackageWorldModuleGraph<'db>,
) -> HashMap<InternedText<'db>, bct::package2::PackageModule> {
    let mut alias_map = HashMap::new();

    // Build a map from (import_space, package_name, module_name) to PackageModule.
    // We need to traverse the package world structure to find modules by their hierarchy.
    let mut hierarchy_map = HashMap::new();

    // Create the package world map.
    let world_map = crate::package::package_world_map(db, package_world);

    for (import_space, packages) in world_map.map(db) {
        for (package_name, package) in packages {
            for (module_name, module) in package.modules(db) {
                let key = (
                    import_space.as_str().S(),
                    package_name.as_str().S(),
                    module_name.as_str().S(),
                );
                hierarchy_map.insert(key, *module);
            }
        }
    }

    // Scan script for require module statements and map to modules.
    for statement in script.statements(db) {
        if let Statement::Require(StmtRequire::Module(req)) = statement {
            let import_space = req.import_space(db);
            let package_alias = req.package_alias(db);
            let module_alias = req.module_alias(db);

            let key = (
                import_space.as_str(db).S(),
                package_alias.as_str(db).S(),
                module_alias.as_str(db).S(),
            );

            if let Some(&module) = hierarchy_map.get(&key) {
                alias_map.insert(module_alias, module);
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

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::prelude::*;

    /// Tracked compile helper that does full parse + typecheck pipeline.
    #[salsa::tracked]
    fn compile_for_test<'db>(
        db: &'db dyn crate::Db,
        source: bct::input::Source,
    ) -> TypecheckResult<'db> {
        let parse_result = crate::parser::parse(db, source);
        type_check(
            db,
            parse_result.script,
            parse_result.expr_spans,
            parse_result.datalit_expr_spans
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
        let source = bct::input::Source::new(&db, S("let x = @1 +! @2"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors.
        // The result type should be Result<u32>.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
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
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): !@u32\n  ret a +! b\nend fun"));
        let tycheck_result = compile_for_test(&db, source);

        // Should have no errors.
        // Return type is Result<u32>, and a +! b returns Result<u32>.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_type_mismatch() {
        let db = crate::Database::default();
        // Fun returns u32 but body returns Result<u32>.
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): @u32\n  ret a +! b\nend fun"));
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
