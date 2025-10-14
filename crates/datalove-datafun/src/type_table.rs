//! Type table for interpreter.
//!
//! Stores TyDesc* for every expression, indexed by Salsa Id.

use rmx::prelude::*;
use datalove_rtdt as rtdt;
use crate::ast::*;
use crate::datalit;
use crate::tycheck::{Type, TypecheckResult};
use crate::tydesc_gen::TyDescCache;

/// Table mapping expression IDs to their type descriptors.
///
/// Uses Vec for O(1) access via `expr.as_id().index()`.
pub struct TypeTable {
    /// Type descriptors indexed by expression ID.
    expr_types: Vec<*const rtdt::TyDesc>,

    /// Cache that owns all TyDesc allocations.
    _cache: TyDescCache,
}

impl TypeTable {
    /// Build a type table from a typechecked script.
    pub fn build<'db>(
        db: &'db dyn crate::Db,
        script: Script<'db>,
        tycheck_result: TypecheckResult<'db>,
    ) -> Result<Self, String> {
        if !tycheck_result.errors(db).is_empty() {
            return Err(format!("Type errors found: {} errors", tycheck_result.errors(db).len()));
        }

        let mut cache = TyDescCache::new();
        let mut builder = TypeTableBuilder {
            db,
            cache: &mut cache,
            expr_types: Vec::new(),
        };

        // Walk the script and generate type descriptors for all expressions.
        builder.visit_script(script)?;

        Ok(TypeTable {
            expr_types: builder.expr_types,
            _cache: cache,
        })
    }

    /// Get the type descriptor for an expression.
    pub fn get_expr_type(&self, _expr: datalit::ast::ExprFull<'_>) -> *const rtdt::TyDesc {
        // TODO: Implement ID-based lookup once we figure out the salsa API.
        // For now, return null to allow compilation.
        std::ptr::null()
    }

    /// Get the type descriptor for a datafun expression.
    pub fn get_datafun_expr_type(&self, _expr: ExprFun<'_>) -> *const rtdt::TyDesc {
        // TODO: Implement ID-based lookup once we figure out the salsa API.
        // For now, return null to allow compilation.
        std::ptr::null()
    }
}

/// Builder for type table.
struct TypeTableBuilder<'a, 'db> {
    db: &'db dyn crate::Db,
    cache: &'a mut TyDescCache,
    expr_types: Vec<*const rtdt::TyDesc>,
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
            Statement::Require(_) | Statement::ParseError(_) => {}
        }
        Ok(())
    }

    /// Visit a datafun expression.
    fn visit_datafun_expr(&mut self, expr: ExprFun<'db>) -> Result<(), String> {
        match expr.expr(self.db) {
            ExprFunKind::Datalit(datalit_expr) => {
                self.visit_datalit_expr(datalit_expr)?;
            }
            ExprFunKind::Name(_) => {
                // Names don't have their own type stored.
            }
            ExprFunKind::BinOp(binop) => {
                self.visit_datafun_expr(binop.lhs(self.db))?;
                self.visit_datafun_expr(binop.rhs(self.db))?;
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
            let datafun_type = Type::Datalit(root_type.ty(self.db).clone());
            let _tydesc = self.cache.generate(self.db, &datafun_type);

            // TODO: Store in the table at the expression's ID index.
            // Need to figure out how to get ID from salsa tracked struct.
            // let id = expr.as_id();
            // let index = id.index() as usize;
            //
            // // Ensure the vector is large enough.
            // if index >= self.expr_types.len() {
            //     self.expr_types.resize(index + 1, std::ptr::null());
            // }
            //
            // self.expr_types[index] = tydesc;
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
