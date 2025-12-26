//! Frame layout computation.

use rmx::prelude::*;
use bct::text::InternedText;
use super::{SlotId, SlotKind};
use super::type_sizing::{compute_datafun_type_layout, TypeLayout};
use crate::ast::{ExprFun, ExprAnonStruct, StmtLet, StmtVar, StmtSet, StmtIf};

/// Align a value up to the given alignment.
/// Alignment must be a power of 2.
#[inline]
fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Complete frame layout with all slots.
#[salsa::tracked]
pub struct FrameLayout<'db> {
    pub total_size: u32,
    pub total_align: u32,
    #[returns(ref)]
    pub slots: Vec<SlotInfo<'db>>,
    /// Maps Name expressions to their resolved slot IDs.
    #[returns(ref)]
    pub name_resolutions: Vec<NameResolution<'db>>,
    /// Indexed lookup: ExprFun salsa ID -> SlotInfo for Name expressions.
    #[returns(ref)]
    pub name_expr_slot_index: Vec<Option<SlotInfo<'db>>>,
    /// Maps let statements to their destination slot IDs.
    #[returns(ref)]
    pub let_stmt_slots: Vec<LetStmtSlot<'db>>,
    /// Maps var statements to their destination slot IDs.
    #[returns(ref)]
    pub var_stmt_slots: Vec<VarStmtSlot<'db>>,
    /// Maps set statements to their target slot IDs.
    #[returns(ref)]
    pub set_stmt_slots: Vec<SetStmtSlot<'db>>,
    /// Maps if-statement bindings to their slot IDs.
    #[returns(ref)]
    pub if_binding_slots: Vec<IfBindingSlot<'db>>,
    /// Precomputed field evaluation order for anonymous struct literals.
    #[returns(ref)]
    pub struct_field_orders: Vec<StructFieldOrder<'db>>,
}

/// Maps a Name expression to its resolved slot.
#[salsa::tracked]
pub struct NameResolution<'db> {
    pub expr: ExprFun<'db>,
    pub slot_id: SlotId,
}

/// Maps a let statement to its destination slot.
#[salsa::tracked]
pub struct LetStmtSlot<'db> {
    pub stmt: StmtLet<'db>,
    pub slot_id: SlotId,
}

/// Maps a var statement to its destination slot.
#[salsa::tracked]
pub struct VarStmtSlot<'db> {
    pub stmt: StmtVar<'db>,
    pub slot_id: SlotId,
}

/// Maps a set statement to its target slot.
#[salsa::tracked]
pub struct SetStmtSlot<'db> {
    pub stmt: StmtSet<'db>,
    pub slot_id: SlotId,
}

/// Maps an if-statement binding to its slot.
#[salsa::tracked]
pub struct IfBindingSlot<'db> {
    pub stmt: StmtIf<'db>,
    /// True for then-binding, false for else-binding.
    pub is_then_binding: bool,
    pub slot_id: SlotId,
}

/// Precomputed field evaluation order for an anonymous struct literal.
///
/// Maps source field order to canonical (alphabetical) order.
#[salsa::tracked]
pub struct StructFieldOrder<'db> {
    pub expr: ExprAnonStruct<'db>,
    /// Permutation from source order to canonical order.
    /// `permutation[i]` is the source index of the field at canonical position `i`.
    #[returns(ref)]
    pub permutation: Vec<usize>,
}

/// Information about a single slot in the frame.
#[salsa::tracked]
pub struct SlotInfo<'db> {
    pub slot_id: SlotId,
    pub name: Option<InternedText<'db>>,  // None for temporaries
    pub kind: SlotKind,
    pub offset: u32,
    pub ty: crate::tycheck::TypeAndHeap<'db>,
    pub expr: Option<crate::ast::ExprFun<'db>>,  // For temporaries: the creating expression
}

impl<'db> FrameLayout<'db> {
    /// Get a slot by its ID.
    pub fn get_slot(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Option<SlotInfo<'db>> {
        self.slots(db).iter().find(|s| s.slot_id(db) == slot_id).copied()
    }

    /// Get the temporary slot for a given expression.
    pub fn get_temp_slot_for_expr(
        self,
        db: &'db dyn crate::Db,
        expr: crate::ast::ExprFun<'db>,
    ) -> Option<SlotInfo<'db>> {
        self.slots(db).iter()
            .find(|s| s.kind(db) == SlotKind::Temporary && s.expr(db) == Some(expr))
            .copied()
    }

    /// Get the slot for a Name expression using indexed lookup.
    pub fn get_slot_for_name_expr(
        self,
        db: &'db dyn crate::Db,
        expr: ExprFun<'db>,
    ) -> Option<SlotInfo<'db>> {
        use salsa::plumbing::AsId;
        let index = expr.as_id().index() as usize;
        self.name_expr_slot_index(db).get(index).copied().flatten()
    }

    /// Get the destination slot for a let statement.
    pub fn get_slot_for_let_stmt(
        self,
        db: &'db dyn crate::Db,
        stmt: StmtLet<'db>,
    ) -> Option<SlotInfo<'db>> {
        let slot_id = self.let_stmt_slots(db).iter()
            .find(|ls| ls.stmt(db) == stmt)
            .map(|ls| ls.slot_id(db))?;
        self.get_slot(db, slot_id)
    }

    /// Get the destination slot for a var statement.
    pub fn get_slot_for_var_stmt(
        self,
        db: &'db dyn crate::Db,
        stmt: StmtVar<'db>,
    ) -> Option<SlotInfo<'db>> {
        let slot_id = self.var_stmt_slots(db).iter()
            .find(|vs| vs.stmt(db) == stmt)
            .map(|vs| vs.slot_id(db))?;
        self.get_slot(db, slot_id)
    }

    /// Get the target slot for a set statement.
    pub fn get_slot_for_set_stmt(
        self,
        db: &'db dyn crate::Db,
        stmt: StmtSet<'db>,
    ) -> Option<SlotInfo<'db>> {
        let slot_id = self.set_stmt_slots(db).iter()
            .find(|ss| ss.stmt(db) == stmt)
            .map(|ss| ss.slot_id(db))?;
        self.get_slot(db, slot_id)
    }

    /// Compute frame layout from allocated slots with types.
    pub fn compute_layout(
        db: &'db dyn crate::Db,
        slots: Vec<(SlotId, Option<InternedText<'db>>, SlotKind, crate::tycheck::TypeAndHeap<'db>, Option<crate::ast::ExprFun<'db>>)>,
        name_resolutions: Vec<NameResolution<'db>>,
        let_stmt_slots: Vec<LetStmtSlot<'db>>,
        var_stmt_slots: Vec<VarStmtSlot<'db>>,
        set_stmt_slots: Vec<SetStmtSlot<'db>>,
        if_binding_slots: Vec<IfBindingSlot<'db>>,
        struct_field_orders: Vec<StructFieldOrder<'db>>,
    ) -> Self {
        use salsa::plumbing::AsId;
        use std::mem::{size_of, align_of};

        let mut offset = 0u32;
        let mut max_align = 1u32;
        let mut slot_infos = Vec::new();

        for (slot_id, name, kind, ty, expr) in slots {
            // Compute size and alignment for this slot.
            // Reference slots are always pointer-sized, regardless of the referenced type.
            // Local and Temporary slots use the actual type size.
            let layout = if kind == SlotKind::Reference {
                TypeLayout {
                    size: size_of::<usize>() as u32,
                    align: align_of::<usize>() as u32,
                }
            } else {
                compute_datafun_type_layout(db, ty)
            };

            // Align offset to this slot's alignment requirement.
            offset = align_up(offset, layout.align);

            // Create slot info.
            let slot_info = SlotInfo::new(db, slot_id, name, kind, offset, ty, expr);
            slot_infos.push(slot_info);

            // Advance offset by slot size.
            offset += layout.size;

            // Track maximum alignment.
            max_align = max_align.max(layout.align);
        }

        // Total size must be aligned to maximum alignment.
        let total_size = align_up(offset, max_align);

        // Build indexed lookup for name expressions (O(1) access by ExprFun salsa ID).
        let mut name_expr_slot_index: Vec<Option<SlotInfo<'db>>> = Vec::new();
        for nr in &name_resolutions {
            let expr = nr.expr(db);
            let slot_id = nr.slot_id(db);
            // Find SlotInfo by slot_id.
            let slot_info = slot_infos.iter().find(|s| s.slot_id(db) == slot_id).copied();
            // Extend vector to fit expression ID.
            let index = expr.as_id().index() as usize;
            if index >= name_expr_slot_index.len() {
                name_expr_slot_index.resize(index + 1, None);
            }
            name_expr_slot_index[index] = slot_info;
        }

        FrameLayout::new(
            db,
            total_size,
            max_align,
            slot_infos,
            name_resolutions,
            name_expr_slot_index,
            let_stmt_slots,
            var_stmt_slots,
            set_stmt_slots,
            if_binding_slots,
            struct_field_orders,
        )
    }

    /// Get the precomputed field evaluation order for a struct literal.
    pub fn get_struct_field_order(
        self,
        db: &'db dyn crate::Db,
        expr: ExprAnonStruct<'db>,
    ) -> Option<&'db [usize]> {
        self.struct_field_orders(db).iter()
            .find(|sfo| sfo.expr(db) == expr)
            .map(|sfo| sfo.permutation(db).as_slice())
    }
}
