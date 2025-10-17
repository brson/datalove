use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;
use crate::ast::*;
use crate::datalit;

/// Type representation for datafun (extends datalit types with function types).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum Type<'db> {
    /// Datalit type (primitives, collections, etc.).
    Datalit(datalit::tycheck::Type<'db>),
    /// Function type: (param_types) -> return_type.
    Function(TypeFunction<'db>),
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
    ArityMismatch { expected: usize, actual: usize },
    NotAFunction(String),
    DatalitError(String),
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
}

/// Context for typechecking.
struct TypeContext<'db> {
    db: &'db dyn crate::Db,
    /// Variable bindings (name -> type).
    variables: HashMap<InternedText<'db>, TypeAndHeap<'db>>,
    /// Function signatures (name -> function type).
    functions: HashMap<InternedText<'db>, TypeFunction<'db>>,
    /// Expected return type for current function (if inside a function).
    expected_return_type: Option<TypeAndHeap<'db>>,
    errors: Vec<TypeError>,
}

impl<'db> TypeContext<'db> {
    fn new(db: &'db dyn crate::Db) -> Self {
        TypeContext {
            db,
            variables: HashMap::new(),
            functions: HashMap::new(),
            expected_return_type: None,
            errors: Vec::new(),
        }
    }

    fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    fn add_variable(&mut self, name: InternedText<'db>, ty: TypeAndHeap<'db>) {
        self.variables.insert(name, ty);
    }

    fn add_function(&mut self, name: InternedText<'db>, func_type: TypeFunction<'db>) {
        self.functions.insert(name, func_type);
    }

    fn lookup_variable(&self, name: InternedText<'db>) -> Option<TypeAndHeap<'db>> {
        self.variables.get(&name).copied()
    }

    fn lookup_function(&self, name: InternedText<'db>) -> Option<TypeFunction<'db>> {
        self.functions.get(&name).copied()
    }
}

/// Typecheck a script.
#[salsa::tracked]
pub fn type_check<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db);

    // Type check each statement in order.
    for statement in script.statements(db) {
        check_statement(&mut ctx, statement);
    }

    let errors = ctx
        .errors
        .into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();

    TypecheckResult::new(db, script, errors)
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
    let mut ctx = TypeContext::new(db);

    // Type check each statement in order.
    for statement in script.statements(db) {
        check_statement(&mut ctx, statement);
    }

    ctx.lookup_variable(name)
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
                    match synthesize_expr(ctx, value) {
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
            let return_type = stmt.return_type(db);
            let body = stmt.body(db);

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

            // Convert return type.
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
                    // TODO: infer return type from body.
                    ctx.add_error(TypeError::CannotSynthesize);
                    return;
                }
            };

            // Create function type and add to context.
            let func_type = TypeFunction::new(db, param_types.clone(), ret_ty);
            ctx.add_function(name, func_type);

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
            // We need to create a dummy resolved expr for datalit.
            let resolved = datalit::resolve::resolve_names(db, datalit_expr);
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
    let lhs_ty = synthesize_expr(ctx, lhs)?;
    let rhs_ty = synthesize_expr(ctx, rhs)?;

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
        // Basic arithmetic: same type.
        Add | Sub | Mul | Div |
        AddSaturating | SubSaturating | MulSaturating | DivSaturating => {
            lhs_ty
        }

        // Checked arithmetic: Result<T>.
        AddChecked | SubChecked | MulChecked | DivChecked => {
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

        // Optional arithmetic: Option<T>.
        AddOptional | SubOptional | MulOptional | DivOptional => {
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
    }
}

/// Check an expression against an expected type.
fn check_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    expected: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Try synthesis first.
    let synthesized = synthesize_expr(ctx, expr)?;

    // Check if types match.
    if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
        Ok(())
    } else {
        Err(TypeError::TypeMismatch {
            expected: type_to_string(db, expected.ty(db)),
            actual: type_to_string(db, synthesized.ty(db)),
        })
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::prelude::*;

    #[test]
    fn test_tycheck_simple_let() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x: @u32 = @42"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_binop_add() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = a + b"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Will have errors because 'a' and 'b' are unresolved.
        assert!(tycheck_result.errors(&db).len() > 0);
    }

    #[test]
    fn test_tycheck_binop_checked() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @1 +! @2"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors.
        // The result type should be Result<u32>.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_fun_simple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("fun foo(): @u32\n  ret @42\nend fun"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_fun_params() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): @u32\n  ret a + b\nend fun"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_fun_checked() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): !@u32\n  ret a +! b\nend fun"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors.
        // Return type is Result<u32>, and a +! b returns Result<u32>.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_type_mismatch() {
        let db = crate::Database::default();
        // Fun returns u32 but body returns Result<u32>.
        let source = bct::input::Source::new(&db, S("fun add(a: @u32, b: @u32): @u32\n  ret a +! b\nend fun"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have a type mismatch error.
        assert!(tycheck_result.errors(&db).len() > 0);
    }

    // Tests for complex datalit expressions enabled by direct token parsing

    #[test]
    fn test_tycheck_datalit_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - tuple of integers.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[1, 2, 3]"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - list of integers.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_map() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @map { @1 = @10, @2 = @20 }"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - map with integer keys and integer values.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_nested_tuple_in_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[(1, 2), (3, 4)]"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - list of tuples.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_nested_list_in_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@1, @2, @3], @100)"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - tuple with nested list.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_set() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @set { @1, @2, @3 }"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - set of integers.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_deeply_nested() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - deeply nested structure.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }

    #[test]
    fn test_tycheck_datalit_tuple_in_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[@(@1, @2), @(@3, @4), @(@5, @6)]"));
        let script = crate::parser::parse(&db, source);
        let tycheck_result = type_check(&db, script);

        // Should have no errors - list of tuples.
        assert_eq!(tycheck_result.errors(&db).len(), 0);
    }
}
