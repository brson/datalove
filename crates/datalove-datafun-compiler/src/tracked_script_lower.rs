//! Salsa-tracked API for script unit lowering.
//!
//! Provides memoized, per-unit script lowering following the same pattern as
//! per-unit typechecking. Each unit is lowered independently, enabling
//! incremental recompilation when new units are added to a batch.

use rmx::prelude::*;
use std::collections::HashMap;
use datalove_datafun_ast::ast::Statement;
use datalove_datafun_ir::{IrScriptUnit, IrType, IrModuleId, FuncId, ValueId, SlotId, ExportBinding};
use datalove_datafun_tycheck::{
    UnitTypecheckResultTracked, ModuleSpec,
};

use crate::module_graph::ModuleId;
use crate::lower;
use crate::ir_ext::IrTypeExt;
use crate::tracked_script_ownership::ScriptUnitOwnershipResult;

/// Accumulated bindings passed to subsequent script units for lowering.
///
/// Plain data struct (not tracked) - compared by Salsa via Eq/Hash.
/// Contains exports from all prior units in the batch.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
#[derive(salsa::Update)]
pub struct AccumulatedLowerBindings {
    /// Value bindings: (name, unit_index, value_id).
    pub values: Vec<(String, u32, ValueId)>,
    /// Slot bindings: (name, unit_index, slot_id).
    pub slots: Vec<(String, u32, SlotId)>,
    /// Value types: (name, type).
    pub value_types: Vec<(String, IrType)>,
    /// Slot types: (name, type).
    pub slot_types: Vec<(String, IrType)>,
    /// Function bindings: (name, unit_index, func_id).
    pub functions: Vec<(String, u32, FuncId)>,
    /// Current unit index.
    pub current_unit: u32,
}

impl AccumulatedLowerBindings {
    /// Convert to ScriptLowerContext for lowering.
    pub fn to_script_lower_context(&self) -> lower::ScriptLowerContext {
        let mut ctx = lower::ScriptLowerContext::new();
        ctx.current_unit = self.current_unit;

        for (name, unit, value_id) in &self.values {
            ctx.values.insert(name.clone(), (*unit, *value_id));
        }
        for (name, unit, slot_id) in &self.slots {
            ctx.slots.insert(name.clone(), (*unit, *slot_id));
        }
        for (name, ty) in &self.value_types {
            ctx.value_types.insert(name.clone(), ty.clone());
        }
        for (name, ty) in &self.slot_types {
            ctx.slot_types.insert(name.clone(), ty.clone());
        }
        for (name, unit, func_id) in &self.functions {
            ctx.functions.insert(name.clone(), (*unit, *func_id));
        }

        ctx
    }

    /// Update from exports of a completed unit.
    pub fn add_exports(&mut self, unit_index: u32, exports: &[(String, ExportBinding)], value_types: &[IrType], slot_types: &[IrType]) {
        for (name, binding) in exports {
            match binding {
                ExportBinding::Value(v) => {
                    // Remove any slot with same name.
                    self.slots.retain(|(n, _, _)| n != name);
                    self.slot_types.retain(|(n, _)| n != name);
                    // Remove any existing value with same name (shadowing).
                    self.values.retain(|(n, _, _)| n != name);
                    self.value_types.retain(|(n, _)| n != name);

                    self.values.push((name.clone(), unit_index, *v));
                    if let Some(ty) = value_types.get(v.0 as usize) {
                        self.value_types.push((name.clone(), ty.clone()));
                    }
                }
                ExportBinding::Slot(s) => {
                    // Remove any value with same name.
                    self.values.retain(|(n, _, _)| n != name);
                    self.value_types.retain(|(n, _)| n != name);
                    // Remove any existing slot with same name (shadowing).
                    self.slots.retain(|(n, _, _)| n != name);
                    self.slot_types.retain(|(n, _)| n != name);

                    self.slots.push((name.clone(), unit_index, *s));
                    if let Some(ty) = slot_types.get(s.0 as usize) {
                        self.slot_types.push((name.clone(), ty.clone()));
                    }
                }
                ExportBinding::Function(func_id) => {
                    // Remove any existing function with same name.
                    self.functions.retain(|(n, _, _)| n != name);
                    self.functions.push((name.clone(), unit_index, *func_id));
                }
            }
        }

        self.current_unit = unit_index + 1;
    }
}

/// Output from lower_script_unit_tracked.
///
/// Includes the lowered IR unit AND new exports for accumulation by the caller.
#[salsa::tracked]
pub struct ScriptUnitLowerOutput<'db> {
    /// The lowered IR unit (None if lowering failed).
    #[returns(ref)]
    pub ir_unit: Option<IrScriptUnit>,
    /// Lowering error message if failed.
    #[returns(ref)]
    pub error: Option<String>,
    /// New exports from this unit (for accumulation).
    #[returns(ref)]
    pub new_exports: Vec<(String, ExportBinding)>,
    /// Value types from this unit.
    #[returns(ref)]
    pub value_types: Vec<IrType>,
    /// Slot types from this unit.
    #[returns(ref)]
    pub slot_types: Vec<IrType>,
}

/// Lower a single script fragment unit with accumulated context from prior units.
///
/// Memoized: if typecheck_result, module_specs, accumulated, statements, and ownership_result
/// all match a previous call, returns the cached result.
///
/// Caller must first call `analyze_script_fragment_tracked` to get the ownership_result.
#[salsa::tracked]
pub fn lower_script_fragment_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    module_specs: Vec<ModuleSpec<'db>>,
    accumulated: AccumulatedLowerBindings,
    statements: Vec<Statement<'db>>,
    ownership_result: ScriptUnitOwnershipResult<'db>,
) -> ScriptUnitLowerOutput<'db> {
    // Check for ownership analysis errors first.
    let ownership_errors = ownership_result.errors(db);
    if !ownership_errors.is_empty() {
        return ScriptUnitLowerOutput::new(
            db,
            None,
            Some(ownership_errors.join("\n")),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    // Build func_id_map from module specs (for cross-module call resolution).
    let func_id_map = build_func_id_map(db, &module_specs);

    // Build map of function name -> resolved param types for type alias support.
    let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
    for (name, func_type) in typecheck_result.function_types(db) {
        let param_types: Vec<IrType> = func_type.param_types(db)
            .iter()
            .map(|ty| IrType::from_tycheck(db, ty))
            .collect();
        func_param_types.insert(name.text(db).S(), param_types);
    }

    // Get function analyses from ownership result.
    let func_analyses = ownership_result.to_function_analyses_map(db, &statements);

    // Get script analysis from ownership result.
    let script_analysis = ownership_result.script_analysis(db).clone()
        .expect("script_analysis required for fragment units");

    // Convert accumulated bindings to ScriptLowerContext.
    let script_ctx = accumulated.to_script_lower_context();

    // Lower the fragment.
    match lower::lower_script_fragment_raw(
        db,
        expr_types,
        call_targets,
        &func_id_map,
        script_ctx,
        statements,
        func_analyses,
        script_analysis,
        Some(&func_param_types),
    ) {
        Ok(ir_unit) => {
            let exports = ir_unit.exports.clone();
            let value_types = ir_unit.value_types.clone();
            let slot_types = ir_unit.slot_types.clone();
            ScriptUnitLowerOutput::new(db, Some(ir_unit), None, exports, value_types, slot_types)
        }
        Err(e) => {
            ScriptUnitLowerOutput::new(db, None, Some(format!("{}", e)), Vec::new(), Vec::new(), Vec::new())
        }
    }
}

/// Lower a single script expression unit with accumulated context from prior units.
///
/// Memoized: if typecheck_result, module_specs, accumulated, expr, and ownership_result
/// all match a previous call, returns the cached result.
///
/// Caller must first call `analyze_script_expr_tracked` to get the ownership_result
/// (which will be minimal for expressions).
#[salsa::tracked]
pub fn lower_script_expr_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    module_specs: Vec<ModuleSpec<'db>>,
    accumulated: AccumulatedLowerBindings,
    expr: datalove_datafun_ast::ast::ExprFun<'db>,
    ownership_result: ScriptUnitOwnershipResult<'db>,
) -> ScriptUnitLowerOutput<'db> {
    // Check for ownership analysis errors first (should be empty for expressions).
    let ownership_errors = ownership_result.errors(db);
    if !ownership_errors.is_empty() {
        return ScriptUnitLowerOutput::new(
            db,
            None,
            Some(ownership_errors.join("\n")),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    // Build func_id_map from module specs.
    let func_id_map = build_func_id_map(db, &module_specs);

    // Convert accumulated bindings to ScriptLowerContext.
    let script_ctx = accumulated.to_script_lower_context();

    // Lower the expression.
    match lower::lower_script_expr(
        db,
        expr_types,
        call_targets,
        &func_id_map,
        script_ctx,
        expr,
    ) {
        Ok(ir_unit) => {
            let exports = ir_unit.exports.clone();
            let value_types = ir_unit.value_types.clone();
            let slot_types = ir_unit.slot_types.clone();
            ScriptUnitLowerOutput::new(db, Some(ir_unit), None, exports, value_types, slot_types)
        }
        Err(e) => {
            ScriptUnitLowerOutput::new(db, None, Some(format!("{}", e)), Vec::new(), Vec::new(), Vec::new())
        }
    }
}

/// Build func_id_map from module specs for cross-module call resolution.
fn build_func_id_map<'db>(
    db: &'db dyn salsa::Database,
    module_specs: &[ModuleSpec<'db>],
) -> HashMap<(ModuleId, String), (IrModuleId, FuncId)> {
    let mut func_id_map = HashMap::new();

    for (ir_module_idx, spec) in module_specs.iter().enumerate() {
        let ir_module_id = IrModuleId(ir_module_idx as u32);
        let mut next_func_id: u32 = 0;

        for statement in &spec.parsed.statements {
            if let Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).S();
                let func_id = FuncId(next_func_id);
                next_func_id += 1;
                func_id_map.insert((spec.module_id, func_name), (ir_module_id, func_id));
            }
        }
    }

    func_id_map
}
