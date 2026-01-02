//! IR type extension trait.
//!
//! Adds conversion from tycheck types to IR types.

use datalove_datafun_ir::IrType;

/// Extension trait for IrType to add conversion from tycheck types.
///
/// This is separate from the core IrType because it requires access
/// to the tycheck module which is in the compiler crate.
pub trait IrTypeExt {
    /// Convert from typechecker type to IR type.
    fn from_tycheck<'db>(db: &'db dyn crate::Db, ty: &datalove_datafun_tycheck::TypeAndHeap<'db>) -> IrType;
}

impl IrTypeExt for IrType {
    fn from_tycheck<'db>(db: &'db dyn crate::Db, ty: &datalove_datafun_tycheck::TypeAndHeap<'db>) -> IrType {
        use datalove_datafun_tycheck::Type as TyType;

        match ty.ty(db) {
            TyType::Datalit(dl_ty) => IrType::from_datalit(db, dl_ty),
            TyType::Function(_) => {
                todo!("function types in IR")
            }
        }
    }
}
