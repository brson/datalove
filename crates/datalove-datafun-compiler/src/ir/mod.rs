//! SSA-based intermediate representation for datafun functions.
//!
//! This module re-exports types from datalove-datafun-ir and adds
//! lowering, interpretation, and type conversion that require
//! access to the full compiler.

// Re-export all IR types from the IR crate.
pub use datalove_datafun_ir::*;

// Local submodules that depend on the compiler.
pub mod lower;
pub mod drop_analysis;

/// Extension trait for IrType to add conversion from tycheck types.
///
/// This is separate from the core IrType because it requires access
/// to the tycheck module which is in the compiler crate.
pub trait IrTypeExt {
    /// Convert from typechecker type to IR type.
    fn from_tycheck<'db>(db: &'db dyn crate::Db, ty: &crate::tycheck::TypeAndHeap<'db>) -> IrType;
}

impl IrTypeExt for IrType {
    fn from_tycheck<'db>(db: &'db dyn crate::Db, ty: &crate::tycheck::TypeAndHeap<'db>) -> IrType {
        use crate::tycheck::Type as TyType;

        match ty.ty(db) {
            TyType::Datalit(dl_ty) => IrType::from_datalit(db, dl_ty),
            TyType::Function(_) => {
                todo!("function types in IR")
            }
        }
    }
}
