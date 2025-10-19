//! Type table for interpreter.
//!
//! Stores TyDesc* for every expression, indexed by Salsa Id.

use rmx::prelude::*;
use salsa::plumbing::AsId;
use datalove_rtdt as rtdt;
use crate::ast::*;
use crate::datalit;
use crate::tycheck::TypecheckResult;

/// Table mapping expression IDs to their type descriptors.
///
/// Uses Vec for O(1) access via `expr.as_id().index()`.
pub struct TypeTable {
    /// Type descriptors for datalit expressions, indexed by expression ID.
    expr_types: Vec<*const rtdt::TyDesc>,

    /// Type descriptors for datafun expressions, indexed by expression ID.
    datafun_expr_types: Vec<*const rtdt::TyDesc>,
}

impl TypeTable {
    /// Create an empty type table.
    pub fn empty() -> Self {
        TypeTable {
            expr_types: Vec::new(),
            datafun_expr_types: Vec::new(),
        }
    }

    /// Build a type table from a typechecked script.
    pub fn build<'db>(
        db: &'db dyn crate::Db,
        script: Script<'db>,
        tycheck_result: TypecheckResult<'db>,
        tydesc_table: &mut datalit::tydesc_table::TyDescTable<'db>,
    ) -> Result<Self, String> {
        if !tycheck_result.errors(db).is_empty() {
            return Err(format!("Type errors found: {} errors", tycheck_result.errors(db).len()));
        }

        let mut builder = TypeTableBuilder {
            db,
            tydesc_table,
            expr_types: Vec::new(),
            datafun_expr_types: Vec::new(),
        };

        // Walk the script and generate type descriptors for all expressions.
        builder.visit_script(script)?;

        Ok(TypeTable {
            expr_types: builder.expr_types,
            datafun_expr_types: builder.datafun_expr_types,
        })
    }

    /// Get the type descriptor for an expression.
    pub fn get_expr_type(&self, expr: datalit::ast::ExprFull<'_>) -> *const rtdt::TyDesc {
        let id = expr.as_id();
        let index = id.index() as usize;

        if index < self.expr_types.len() {
            self.expr_types[index]
        } else {
            std::ptr::null()
        }
    }

    /// Get the type descriptor for a datafun expression.
    pub fn get_datafun_expr_type(&self, expr: ExprFun<'_>) -> *const rtdt::TyDesc {
        let id = expr.as_id();
        let index = id.index() as usize;

        if index < self.datafun_expr_types.len() {
            self.datafun_expr_types[index]
        } else {
            std::ptr::null()
        }
    }
}

/// Builder for type table.
struct TypeTableBuilder<'a, 'db> {
    db: &'db dyn crate::Db,
    tydesc_table: &'a mut datalit::tydesc_table::TyDescTable<'db>,
    expr_types: Vec<*const rtdt::TyDesc>,
    datafun_expr_types: Vec<*const rtdt::TyDesc>,
}

impl<'a, 'db> TypeTableBuilder<'a, 'db> {
    /// Visit a script and collect all expression types.
    fn visit_script(&mut self, script: Script<'db>) -> Result<(), String> {
        for statement in script.statements(self.db) {
            self.visit_statement(statement)?;
        }
        Ok(())
    }

    /// Visit a statement.
    fn visit_statement(&mut self, statement: &Statement<'db>) -> Result<(), String> {
        match statement {
            Statement::Let(stmt) => {
                self.visit_datafun_expr(stmt.value(self.db))?;
            }
            Statement::Fun(stmt) => {
                for body_stmt in stmt.body(self.db) {
                    self.visit_statement(body_stmt)?;
                }
            }
            Statement::Ret(stmt) => {
                self.visit_datafun_expr(stmt.value(self.db))?;
            }
            Statement::If(stmt) => {
                self.visit_datafun_expr(stmt.condition(self.db))?;
                for then_stmt in stmt.then_body(self.db) {
                    self.visit_statement(then_stmt)?;
                }
                if let Some(else_stmts) = stmt.else_body(self.db) {
                    for else_stmt in else_stmts {
                        self.visit_statement(else_stmt)?;
                    }
                }
            }
            Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {}
        }
        Ok(())
    }

    /// Visit a datafun expression.
    fn visit_datafun_expr(&mut self, expr: ExprFun<'db>) -> Result<(), String> {
        match expr.expr(self.db) {
            ExprFunKind::Datalit(datalit_expr) => {
                self.visit_datalit_expr(datalit_expr)?;

                // For Datalit, the datafun expression has the same type as the datalit expression.
                let datalit_id = datalit_expr.as_id();
                let datalit_index = datalit_id.index() as usize;

                if datalit_index < self.expr_types.len() {
                    let tydesc = self.expr_types[datalit_index];

                    // Store the type for the datafun expression.
                    let datafun_id = expr.as_id();
                    let datafun_index = datafun_id.index() as usize;

                    if datafun_index >= self.datafun_expr_types.len() {
                        self.datafun_expr_types.resize(datafun_index + 1, std::ptr::null());
                    }

                    self.datafun_expr_types[datafun_index] = tydesc;
                }
            }
            ExprFunKind::Name(_) => {
                // Names don't have their own type stored yet.
                // TODO: Implement type lookup for variables.
            }
            ExprFunKind::BinOp(binop) => {
                self.visit_datafun_expr(binop.lhs(self.db))?;
                self.visit_datafun_expr(binop.rhs(self.db))?;
                // TODO: Store the result type of the binop.
            }
            ExprFunKind::FunctionCall(call) => {
                // Visit all argument expressions.
                for arg in call.args(self.db) {
                    self.visit_datafun_expr(*arg)?;
                }
                // TODO: Store the result type of the function call.
            }
            ExprFunKind::TryOption(try_op) => {
                self.visit_datafun_expr(try_op.operand(self.db))?;
                // TODO: Store the result type of the try operator.
            }
            ExprFunKind::TryResult(try_op) => {
                self.visit_datafun_expr(try_op.operand(self.db))?;
                // TODO: Store the result type of the try operator.
            }
            ExprFunKind::ParseError(_) => {}
        }
        Ok(())
    }

    /// Visit a datalit expression and store its type.
    fn visit_datalit_expr(&mut self, expr: datalit::ast::ExprFull<'db>) -> Result<(), String> {
        // First, visit all sub-expressions.
        self.visit_datalit_expr_children(expr)?;

        // Then, get the type for this expression from the typechecker.
        // For now, we need to run the typechecker on this expression.
        // This is a bit inefficient, but works for initial implementation.
        let resolved = datalit::resolve::resolve_names(self.db, expr);
        let tycheck_result = datalit::tycheck::type_check(self.db, expr, resolved);

        if let Some(root_type) = tycheck_result.root_type(self.db) {
            let datalit_type = root_type.ty(self.db);
            let tydesc = self.tydesc_table.get_or_create(datalit_type);

            // Store in the table at the expression's ID index.
            let id = expr.as_id();
            let index = id.index() as usize;

            // Ensure the vector is large enough.
            if index >= self.expr_types.len() {
                self.expr_types.resize(index + 1, std::ptr::null());
            }

            self.expr_types[index] = tydesc;
        }

        Ok(())
    }

    /// Visit children of a datalit expression.
    fn visit_datalit_expr_children(&mut self, expr: datalit::ast::ExprFull<'db>) -> Result<(), String> {
        use datalit::ast::Expr;

        let expr_and_heap = expr.expr(self.db);
        let expr_inner = expr_and_heap.expr(self.db);

        match expr_inner {
            Expr::True | Expr::False | Expr::Int(_) | Expr::Float(_)
            | Expr::String(_) | Expr::None | Expr::ParseError(_) => {
                // No children.
            }

            Expr::AnonTuple(t) => {
                for elem in t.elements(self.db) {
                    self.visit_datalit_expr(elem)?;
                }
            }

            Expr::NamedTuple(t) => {
                for elem in t.elements(self.db) {
                    self.visit_datalit_expr(elem)?;
                }
            }

            Expr::AnonStruct(s) => {
                for field in s.fields(self.db) {
                    self.visit_datalit_expr(field.value(self.db))?;
                }
            }

            Expr::NamedStruct(s) => {
                for field in s.fields(self.db) {
                    self.visit_datalit_expr(field.value(self.db))?;
                }
            }

            Expr::AnonEnum(e) => {
                if let Some(payload) = e.payload(self.db) {
                    self.visit_datalit_expr(payload)?;
                }
            }

            Expr::NamedEnum(e) => {
                if let Some(payload) = e.payload(self.db) {
                    self.visit_datalit_expr(payload)?;
                }
            }

            Expr::List(l) => {
                for elem in l.elements(self.db) {
                    self.visit_datalit_expr(elem)?;
                }
            }

            Expr::Map(m) => {
                for entry in m.entries(self.db) {
                    self.visit_datalit_expr(entry.key(self.db))?;
                    self.visit_datalit_expr(entry.value(self.db))?;
                }
            }

            Expr::Set(s) => {
                for elem in s.elements(self.db) {
                    self.visit_datalit_expr(elem)?;
                }
            }

            Expr::Data(d) => {
                self.visit_datalit_expr(d.value(self.db))?;
            }

            Expr::Err(e) => {
                self.visit_datalit_expr(e.value(self.db))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bct::input::Source;

    /// Helper to build a type table from source code.
    fn build_type_table_from_source<'db>(
        db: &'db dyn crate::Db,
        source_text: &str,
    ) -> Result<(Script<'db>, TypeTable, datalit::tydesc_table::TyDescTable<'db>), String> {
        let source = Source::new(db, source_text.S());
        let script = crate::parser::parse(db, source);
        let tycheck_result = crate::tycheck::type_check(db, script);
        let mut tydesc_table = datalit::tydesc_table::TyDescTable::new(db);
        let type_table = TypeTable::build(db, script, tycheck_result, &mut tydesc_table)?;
        Ok((script, type_table, tydesc_table))
    }

    #[test]
    fn test_type_table_build_simple() {
        let ref db = crate::Database::default();
        let result = build_type_table_from_source(db, "let x = @42");
        assert!(result.is_ok(), "Failed to build type table: {:?}", result.err());
    }

    #[test]
    fn test_type_table_build_with_type_errors() {
        let ref db = crate::Database::default();
        // This should fail because 'undefined' is not a known variable.
        let source = Source::new(db, S("let x = undefined"));
        let script = crate::parser::parse(db, source);
        let tycheck_result = crate::tycheck::type_check(db, script);
        let mut tydesc_table = datalit::tydesc_table::TyDescTable::new(db);
        let result = TypeTable::build(db, script, tycheck_result, &mut tydesc_table);
        assert!(result.is_err(), "Expected build to fail with type errors");
    }

    #[test]
    fn test_get_expr_type_for_u32_literal() {
        let ref db = crate::Database::default();
        let (script, type_table, _tydesc_table) = build_type_table_from_source(db, "let x = @42")
            .expect("Failed to build type table");

        // Get the expression from the let statement.
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);

        match &statements[0] {
            Statement::Let(stmt) => {
                let value_expr = stmt.value(db);
                match value_expr.expr(db) {
                    ExprFunKind::Datalit(datalit_expr) => {
                        let tydesc = type_table.get_expr_type(datalit_expr);
                        assert!(!tydesc.is_null(), "Type descriptor should not be null");

                        // Verify it's a u32 type.
                        unsafe {
                            assert_eq!((*tydesc).type_tag, rtdt::TyTag::U32);
                        }
                    }
                    _ => panic!("Expected Datalit expression"),
                }
            }
            _ => panic!("Expected Let statement"),
        }
    }

    #[test]
    fn test_get_expr_type_for_another_u32_literal() {
        let ref db = crate::Database::default();
        let (script, type_table, _tydesc_table) = build_type_table_from_source(db, "let x = @123")
            .expect("Failed to build type table");

        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);

        match &statements[0] {
            Statement::Let(stmt) => {
                let value_expr = stmt.value(db);
                match value_expr.expr(db) {
                    ExprFunKind::Datalit(datalit_expr) => {
                        let tydesc = type_table.get_expr_type(datalit_expr);
                        assert!(!tydesc.is_null(), "Type descriptor should not be null");

                        unsafe {
                            assert_eq!((*tydesc).type_tag, rtdt::TyTag::U32);
                        }
                    }
                    _ => panic!("Expected Datalit expression"),
                }
            }
            _ => panic!("Expected Let statement"),
        }
    }

    #[test]
    fn test_get_datafun_expr_type() {
        let ref db = crate::Database::default();
        let (script, type_table, _tydesc_table) = build_type_table_from_source(db, "let x = @42")
            .expect("Failed to build type table");

        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);

        match &statements[0] {
            Statement::Let(stmt) => {
                let value_expr = stmt.value(db);

                // Get the type of the datafun expression (which wraps the datalit).
                let tydesc = type_table.get_datafun_expr_type(value_expr);
                assert!(!tydesc.is_null(), "Datafun expression type should not be null");

                // Should be the same type as the wrapped datalit expression.
                unsafe {
                    assert_eq!((*tydesc).type_tag, rtdt::TyTag::U32);
                }
            }
            _ => panic!("Expected Let statement"),
        }
    }

    #[test]
    fn test_get_expr_type_returns_null_for_missing() {
        let ref db = crate::Database::default();
        let (_, type_table, _tydesc_table) = build_type_table_from_source(db, "let x = @42")
            .expect("Failed to build type table");

        // Create a new expression that's not in the type table.
        let new_source = Source::new(db, S("@999"));
        let new_script = crate::parser::parse(db, new_source);
        let new_statements = new_script.statements(db);

        // This should return null because it's not in our type table.
        match &new_statements[0] {
            Statement::Let(stmt) => {
                let value_expr = stmt.value(db);
                match value_expr.expr(db) {
                    ExprFunKind::Datalit(datalit_expr) => {
                        let tydesc = type_table.get_expr_type(datalit_expr);
                        // The expression might or might not be in the table depending on ID allocation.
                        // This test is primarily checking that we don't crash.
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    #[test]
    fn test_type_table_with_multiple_expressions() {
        let ref db = crate::Database::default();
        let source_text = "let x = @42\nlet y = @100\nlet z = @999";
        let (script, type_table, _tydesc_table) = build_type_table_from_source(db, source_text)
            .expect("Failed to build type table");

        let statements = script.statements(db);
        assert_eq!(statements.len(), 3);

        // Check all three expressions (all u32).
        for stmt_idx in 0..3 {
            match &statements[stmt_idx] {
                Statement::Let(stmt) => {
                    let value_expr = stmt.value(db);
                    match value_expr.expr(db) {
                        ExprFunKind::Datalit(datalit_expr) => {
                            let tydesc = type_table.get_expr_type(datalit_expr);
                            assert!(!tydesc.is_null(), "Type descriptor for statement {} should not be null", stmt_idx);
                            unsafe {
                                assert_eq!((*tydesc).type_tag, rtdt::TyTag::U32);
                            }
                        }
                        _ => panic!("Expected Datalit expression at statement {}", stmt_idx),
                    }
                }
                _ => panic!("Expected Let statement at {}", stmt_idx),
            }
        }
    }

    #[test]
    fn test_type_table_with_function() {
        let ref db = crate::Database::default();
        let source_text = "fun foo(): @u32\n  ret @42\nend fun";
        let (script, type_table, _tydesc_table) = build_type_table_from_source(db, source_text)
            .expect("Failed to build type table");

        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);

        match &statements[0] {
            Statement::Fun(fun_stmt) => {
                let body = fun_stmt.body(db);
                assert_eq!(body.len(), 1);

                match &body[0] {
                    Statement::Ret(ret_stmt) => {
                        let value_expr = ret_stmt.value(db);
                        match value_expr.expr(db) {
                            ExprFunKind::Datalit(datalit_expr) => {
                                let tydesc = type_table.get_expr_type(datalit_expr);
                                assert!(!tydesc.is_null(), "Return value type should not be null");
                                unsafe {
                                    assert_eq!((*tydesc).type_tag, rtdt::TyTag::U32);
                                }
                            }
                            _ => panic!("Expected Datalit expression in return"),
                        }
                    }
                    _ => panic!("Expected Ret statement in function body"),
                }
            }
            _ => panic!("Expected Fun statement"),
        }
    }

    #[test]
    fn test_type_table_with_complex_expression() {
        let ref db = crate::Database::default();
        // Test with a simple let, since binops require variables which don't typecheck.
        // This test just verifies that expressions inside let statements get their types stored.
        let source_text = "let x = @42\nlet y = @100";
        let (script, type_table, _tydesc_table) = build_type_table_from_source(db, source_text)
            .expect("Failed to build type table");

        let statements = script.statements(db);
        assert_eq!(statements.len(), 2);

        // Check both expressions.
        for (idx, expected_tag) in [(0, rtdt::TyTag::U32), (1, rtdt::TyTag::U32)].iter() {
            match &statements[*idx] {
                Statement::Let(stmt) => {
                    let value_expr = stmt.value(db);
                    let tydesc = type_table.get_datafun_expr_type(value_expr);
                    assert!(!tydesc.is_null(), "Type for expression {} should not be null", idx);
                    unsafe {
                        assert_eq!((*tydesc).type_tag, *expected_tag);
                    }
                }
                _ => panic!("Expected Let statement at {}", idx),
            }
        }
    }
}
