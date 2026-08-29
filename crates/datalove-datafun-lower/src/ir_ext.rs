//! IR type extension trait.
//!
//! Adds conversion from common types to IR types.

use datalove_datafun_common::Type;
use datalove_datafun_ir::IrType;

/// Extension trait for IrType to add conversion from common types.
///
/// This is separate from the core IrType because it requires access
/// to the common types module.
pub trait IrTypeExt {
    /// Convert from common type to IR type.
    fn from_tycheck<'db>(db: &'db dyn salsa::Database, ty: &Type<'db>) -> IrType;
}

impl IrTypeExt for IrType {
    fn from_tycheck<'db>(db: &'db dyn salsa::Database, ty: &Type<'db>) -> IrType {
        match ty {
            // A type parameter is erased to `data` inside this, at whatever
            // depth it sits: the function is compiled once over `data`, which
            // holds a value of any type and carries what it needs to clone and
            // drop it. The call site wraps on the way in and moves the value
            // back out on the way back.
            Type::Datalit(dl_ty) => IrType::from_datalit(db, dl_ty),
            Type::Function(_) => {
                todo!("function types in IR")
            }
        }
    }
}
