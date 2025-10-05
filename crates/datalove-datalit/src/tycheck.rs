use rmx::prelude::*;
use crate::ast;

#[salsa::tracked]
pub struct ExprTypes<'db> {
}

#[salsa::tracked]
pub fn type_check<'db>(
    db: &'db dyn crate::Db,
    expr: ast::ExprFull<'db>,
) -> ExprTypes<'db> {
    todo!()
}
