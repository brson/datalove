//! Function inlining pass for datafun IR.
//!
//! This crate provides:
//! - Inline directive types for specifying which functions to inline
//! - Function inlining transformation on IR modules
//! - Cross-module inlining support
//! - Dynamic inlining for interpreter optimization


use std::collections::HashMap;

use datalove_datafun_ir::{
    BlockId, CallSiteId, FuncId, FuncRef, Instruction, IrBlock, IrFunction, IrModule, IrModuleId,
    IrType, ModuleFunctionRegistry, Operand, ParamId, ParamMode, SlotDest, SlotId, SymbolTable,
    Terminator, ValueId,
};

/// Directive specifying which function calls to inline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineDirective {
    /// Inline all calls to `callee` within `caller` (same module).
    Inline { caller: String, callee: String },
    /// Inline only the Nth call (0-indexed) to `callee` within `caller`.
    InlineAt {
        caller: String,
        callee: String,
        call_index: usize,
    },
    /// Inline all calls to `callee` in all functions (same module).
    InlineAll { callee: String },
    /// Cross-module: Inline all calls to `callee_module::callee` within `caller_module::caller`.
    InlineCross {
        caller_module: String,
        caller: String,
        callee_module: String,
        callee: String,
    },
    /// Cross-module: Inline all calls to `callee_module::callee` in all functions across all modules.
    InlineCrossAll {
        callee_module: String,
        callee: String,
    },
}

/// Parse inline directives from source text.
///
/// Format:
/// ```text
/// inline caller_function callee_function
/// inline caller_function callee_function at N
/// inline-all callee_function
/// inline-cross caller_module::caller_func callee_module::callee_func
/// inline-cross-all callee_module::callee_func
/// ```
pub fn parse_inline_directives(source: &str) -> Result<Vec<InlineDirective>, String> {
    let mut directives = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line = line.trim();

        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();

        let directive = match parts.as_slice() {
            ["inline", caller, callee] => InlineDirective::Inline {
                caller: (*caller).to_string(),
                callee: (*callee).to_string(),
            },
            ["inline", caller, callee, "at", idx] => {
                let call_index: usize = idx.parse().map_err(|_| {
                    format!(
                        "line {}: invalid call index '{}' (expected integer)",
                        line_num + 1,
                        idx
                    )
                })?;
                InlineDirective::InlineAt {
                    caller: (*caller).to_string(),
                    callee: (*callee).to_string(),
                    call_index,
                }
            }
            ["inline-all", callee] => InlineDirective::InlineAll {
                callee: (*callee).to_string(),
            },
            ["inline-cross", caller_spec, callee_spec] => {
                let (caller_module, caller) = parse_qualified_name(caller_spec).ok_or_else(|| {
                    format!(
                        "line {}: invalid caller specification '{}' (expected module::function)",
                        line_num + 1,
                        caller_spec
                    )
                })?;
                let (callee_module, callee) = parse_qualified_name(callee_spec).ok_or_else(|| {
                    format!(
                        "line {}: invalid callee specification '{}' (expected module::function)",
                        line_num + 1,
                        callee_spec
                    )
                })?;
                InlineDirective::InlineCross {
                    caller_module,
                    caller,
                    callee_module,
                    callee,
                }
            }
            ["inline-cross-all", callee_spec] => {
                let (callee_module, callee) = parse_qualified_name(callee_spec).ok_or_else(|| {
                    format!(
                        "line {}: invalid callee specification '{}' (expected module::function)",
                        line_num + 1,
                        callee_spec
                    )
                })?;
                InlineDirective::InlineCrossAll {
                    callee_module,
                    callee,
                }
            }
            _ => {
                return Err(format!(
                    "line {}: invalid inline directive '{}' \
                     (expected 'inline caller callee', 'inline caller callee at N', \
                     'inline-all callee', 'inline-cross mod::caller mod::callee', \
                     or 'inline-cross-all mod::callee')",
                    line_num + 1,
                    line
                ))
            }
        };

        directives.push(directive);
    }

    Ok(directives)
}

/// Parse a qualified name like "module::function" into (module, function).
fn parse_qualified_name(spec: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = spec.split("::").collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Specifies which call sites to inline.
#[derive(Clone, Debug)]
pub enum CallSiteFilter {
    /// Inline all calls to the callee.
    All,
    /// Inline only the Nth call (0-indexed).
    AtIndex(usize),
}

/// Request to inline specific calls (single-module).
#[derive(Clone, Debug)]
pub struct InlineRequest {
    pub caller: FuncId,
    pub callee: FuncId,
    pub filter: CallSiteFilter,
}

/// Global function identifier (module + function).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlobalFuncId {
    pub module: IrModuleId,
    pub func: FuncId,
}

/// Request to inline specific calls (cross-module).
#[derive(Clone, Debug)]
pub struct CrossModuleInlineRequest {
    pub caller: GlobalFuncId,
    pub callee: GlobalFuncId,
    pub filter: CallSiteFilter,
}

/// Reason why an inlining was skipped.
#[derive(Clone, Debug)]
pub enum InlineSkipReason {
    /// The caller function was not found.
    CallerNotFound { name: String },
    /// The callee function was not found.
    CalleeNotFound { name: String },
    /// The caller module was not found.
    CallerModuleNotFound { name: String },
    /// The callee module was not found.
    CalleeModuleNotFound { name: String },
    /// Recursive call detected.
    RecursiveCall { func: String },
    /// The specified call site was not found.
    CallSiteNotFound {
        caller: String,
        callee: String,
        index: usize,
    },
    /// No calls to callee found in caller.
    NoCallsFound { caller: String, callee: String },
}

/// Result of the inlining pass.
#[derive(Clone, Debug)]
pub struct InlineResult {
    /// The transformed module.
    pub module: IrModule,
    /// Number of call sites inlined.
    pub inlined_count: usize,
    /// Reasons why some inlinings were skipped.
    pub skipped: Vec<InlineSkipReason>,
}

/// Result of cross-module inlining.
#[derive(Clone)]
pub struct CrossModuleInlineResult {
    /// The transformed function registry.
    pub registry: ModuleFunctionRegistry,
    /// Number of call sites inlined.
    pub inlined_count: usize,
    /// Reasons why some inlinings were skipped.
    pub skipped: Vec<InlineSkipReason>,
}

impl std::fmt::Debug for CrossModuleInlineResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrossModuleInlineResult")
            .field("inlined_count", &self.inlined_count)
            .field("skipped", &self.skipped)
            .finish_non_exhaustive()
    }
}

/// Context for cross-module inlining operations.
pub struct CrossModuleInlineContext {
    /// Module name to IrModuleId mapping.
    pub module_names: HashMap<String, IrModuleId>,
    /// Per-module symbol tables.
    pub module_symbols: HashMap<IrModuleId, SymbolTable>,
}

impl CrossModuleInlineContext {
    /// Create a new cross-module inline context.
    pub fn new() -> Self {
        Self {
            module_names: HashMap::new(),
            module_symbols: HashMap::new(),
        }
    }

    /// Register a module with its name and symbol table.
    pub fn register_module(&mut self, name: String, module_id: IrModuleId, symbols: SymbolTable) {
        self.module_names.insert(name, module_id);
        self.module_symbols.insert(module_id, symbols);
    }

    /// Look up a function by module name and function name.
    pub fn lookup_function(&self, module_name: &str, func_name: &str) -> Option<GlobalFuncId> {
        let module_id = self.module_names.get(module_name)?;
        let symbols = self.module_symbols.get(module_id)?;
        let func_def = symbols.functions.iter().find(|f| f.name == func_name)?;
        Some(GlobalFuncId {
            module: *module_id,
            func: func_def.id,
        })
    }

    /// Iterate over all modules.
    pub fn iter_modules(&self) -> impl Iterator<Item = (&String, &IrModuleId)> {
        self.module_names.iter()
    }
}

impl Default for CrossModuleInlineContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve inline directives to concrete requests using the module's symbol table.
pub fn resolve_directives(
    module: &IrModule,
    directives: &[InlineDirective],
) -> (Vec<InlineRequest>, Vec<InlineSkipReason>) {
    let mut requests = Vec::new();
    let mut skipped = Vec::new();

    // Build name to FuncId map.
    let name_to_id: HashMap<&str, FuncId> = module
        .symbols
        .functions
        .iter()
        .map(|def| (def.name.as_str(), def.id))
        .collect();

    for directive in directives {
        match directive {
            InlineDirective::Inline { caller, callee } => {
                let Some(&caller_id) = name_to_id.get(caller.as_str()) else {
                    skipped.push(InlineSkipReason::CallerNotFound {
                        name: caller.clone(),
                    });
                    continue;
                };
                let Some(&callee_id) = name_to_id.get(callee.as_str()) else {
                    skipped.push(InlineSkipReason::CalleeNotFound {
                        name: callee.clone(),
                    });
                    continue;
                };

                // Check for recursion.
                if caller_id == callee_id {
                    skipped.push(InlineSkipReason::RecursiveCall {
                        func: caller.clone(),
                    });
                    continue;
                }

                requests.push(InlineRequest {
                    caller: caller_id,
                    callee: callee_id,
                    filter: CallSiteFilter::All,
                });
            }

            InlineDirective::InlineAt {
                caller,
                callee,
                call_index,
            } => {
                let Some(&caller_id) = name_to_id.get(caller.as_str()) else {
                    skipped.push(InlineSkipReason::CallerNotFound {
                        name: caller.clone(),
                    });
                    continue;
                };
                let Some(&callee_id) = name_to_id.get(callee.as_str()) else {
                    skipped.push(InlineSkipReason::CalleeNotFound {
                        name: callee.clone(),
                    });
                    continue;
                };

                if caller_id == callee_id {
                    skipped.push(InlineSkipReason::RecursiveCall {
                        func: caller.clone(),
                    });
                    continue;
                }

                requests.push(InlineRequest {
                    caller: caller_id,
                    callee: callee_id,
                    filter: CallSiteFilter::AtIndex(*call_index),
                });
            }

            InlineDirective::InlineAll { callee } => {
                let Some(&callee_id) = name_to_id.get(callee.as_str()) else {
                    skipped.push(InlineSkipReason::CalleeNotFound {
                        name: callee.clone(),
                    });
                    continue;
                };

                // Add request for each function that is not the callee.
                for def in &module.symbols.functions {
                    if def.id != callee_id {
                        requests.push(InlineRequest {
                            caller: def.id,
                            callee: callee_id,
                            filter: CallSiteFilter::All,
                        });
                    }
                }
            }

            // Cross-module directives are skipped in single-module mode.
            InlineDirective::InlineCross { .. } | InlineDirective::InlineCrossAll { .. } => {}
        }
    }

    (requests, skipped)
}

/// Resolve cross-module inline directives using the context.
pub fn resolve_cross_module_directives(
    ctx: &CrossModuleInlineContext,
    directives: &[InlineDirective],
) -> (Vec<CrossModuleInlineRequest>, Vec<InlineSkipReason>) {
    let mut requests = Vec::new();
    let mut skipped = Vec::new();

    for directive in directives {
        match directive {
            InlineDirective::InlineCross {
                caller_module,
                caller,
                callee_module,
                callee,
            } => {
                let Some(caller_id) = ctx.lookup_function(caller_module, caller) else {
                    if !ctx.module_names.contains_key(caller_module) {
                        skipped.push(InlineSkipReason::CallerModuleNotFound {
                            name: caller_module.clone(),
                        });
                    } else {
                        skipped.push(InlineSkipReason::CallerNotFound {
                            name: format!("{}::{}", caller_module, caller),
                        });
                    }
                    continue;
                };

                let Some(callee_id) = ctx.lookup_function(callee_module, callee) else {
                    if !ctx.module_names.contains_key(callee_module) {
                        skipped.push(InlineSkipReason::CalleeModuleNotFound {
                            name: callee_module.clone(),
                        });
                    } else {
                        skipped.push(InlineSkipReason::CalleeNotFound {
                            name: format!("{}::{}", callee_module, callee),
                        });
                    }
                    continue;
                };

                // Check for recursion (same function).
                if caller_id == callee_id {
                    skipped.push(InlineSkipReason::RecursiveCall {
                        func: format!("{}::{}", caller_module, caller),
                    });
                    continue;
                }

                requests.push(CrossModuleInlineRequest {
                    caller: caller_id,
                    callee: callee_id,
                    filter: CallSiteFilter::All,
                });
            }

            InlineDirective::InlineCrossAll {
                callee_module,
                callee,
            } => {
                let Some(callee_id) = ctx.lookup_function(callee_module, callee) else {
                    if !ctx.module_names.contains_key(callee_module) {
                        skipped.push(InlineSkipReason::CalleeModuleNotFound {
                            name: callee_module.clone(),
                        });
                    } else {
                        skipped.push(InlineSkipReason::CalleeNotFound {
                            name: format!("{}::{}", callee_module, callee),
                        });
                    }
                    continue;
                };

                // Add request for each function in each module (except the callee itself).
                for (_mod_name, &mod_id) in ctx.iter_modules() {
                    if let Some(symbols) = ctx.module_symbols.get(&mod_id) {
                        for func_def in &symbols.functions {
                            let caller_global = GlobalFuncId {
                                module: mod_id,
                                func: func_def.id,
                            };
                            // Skip recursive calls.
                            if caller_global != callee_id {
                                requests.push(CrossModuleInlineRequest {
                                    caller: caller_global,
                                    callee: callee_id,
                                    filter: CallSiteFilter::All,
                                });
                            }
                        }
                    }
                }
            }

            // Single-module directives are skipped in cross-module mode.
            InlineDirective::Inline { .. }
            | InlineDirective::InlineAt { .. }
            | InlineDirective::InlineAll { .. } => {}
        }
    }

    (requests, skipped)
}

/// Information about a call site to inline.
#[derive(Clone, Debug)]
pub struct CallSite {
    pub block_idx: usize,
    pub instr_idx: usize,
    pub dest: ValueId,
    pub args: Vec<Operand>,
}

/// Find all call sites to a specific callee in a function (single-module).
fn find_call_sites(func: &IrFunction, callee_id: FuncId) -> Vec<CallSite> {
    let mut sites = Vec::new();

    for (block_idx, block) in func.blocks.iter().enumerate() {
        for (instr_idx, instr) in block.instructions.iter().enumerate() {
            if let Instruction::Call { dest, func: func_ref, args, .. } = instr {
                // Match both Local and Module function references.
                let matches = match func_ref {
                    FuncRef::Local(id) => *id == callee_id,
                    FuncRef::Module { func, .. } => *func == callee_id,
                    FuncRef::External { .. } => false,
                };
                if matches {
                    sites.push(CallSite {
                        block_idx,
                        instr_idx,
                        dest: *dest,
                        args: args.clone(),
                    });
                }
            }
        }
    }

    sites
}

/// Find all call sites to a specific callee (cross-module aware).
fn find_cross_module_call_sites(
    func: &IrFunction,
    caller_module: IrModuleId,
    target: GlobalFuncId,
) -> Vec<CallSite> {
    let mut sites = Vec::new();

    for (block_idx, block) in func.blocks.iter().enumerate() {
        for (instr_idx, instr) in block.instructions.iter().enumerate() {
            if let Instruction::Call { dest, func: func_ref, args, .. } = instr {
                let matches = match func_ref {
                    // Local call - matches if we're in the same module as target.
                    FuncRef::Local(id) => {
                        caller_module == target.module && *id == target.func
                    }
                    // Module call - check both module and func ID.
                    FuncRef::Module { module, func } => {
                        *module == target.module && *func == target.func
                    }
                    FuncRef::External { .. } => false,
                };
                if matches {
                    sites.push(CallSite {
                        block_idx,
                        instr_idx,
                        dest: *dest,
                        args: args.clone(),
                    });
                }
            }
        }
    }

    sites
}

/// ID remapping context for inlining a callee into a caller.
struct RemapContext {
    value_offset: u32,
    slot_offset: u32,
    block_offset: u32,
    call_site_offset: u32,
}

impl RemapContext {
    fn remap_value(&self, v: ValueId) -> ValueId {
        ValueId(v.0 + self.value_offset)
    }

    fn remap_slot(&self, s: SlotId) -> SlotId {
        SlotId(s.0 + self.slot_offset)
    }

    fn remap_call_site(&self, c: CallSiteId) -> CallSiteId {
        CallSiteId(c.0 + self.call_site_offset)
    }

    fn remap_block(&self, b: BlockId) -> BlockId {
        BlockId(b.0 + self.block_offset)
    }

    fn remap_operand(&self, op: &Operand) -> Operand {
        match op {
            Operand::Value(v) => Operand::Value(self.remap_value(*v)),
            Operand::ValueRef(v) => Operand::ValueRef(self.remap_value(*v)),
            Operand::Slot(s) => Operand::Slot(self.remap_slot(*s)),
            // Params will be replaced with actual arguments, not remapped.
            Operand::Param(p) => Operand::Param(*p),
            // External operands are not remapped.
            Operand::ExternalValue { unit, value } => Operand::ExternalValue {
                unit: *unit,
                value: *value,
            },
            Operand::ExternalSlot { unit, slot } => Operand::ExternalSlot {
                unit: *unit,
                slot: *slot,
            },
        }
    }

    fn remap_slot_dest(&self, dest: &SlotDest) -> SlotDest {
        match dest {
            SlotDest::Local(s) => SlotDest::Local(self.remap_slot(*s)),
            SlotDest::External { unit, slot } => SlotDest::External {
                unit: *unit,
                slot: *slot,
            },
        }
    }

    fn remap_instruction(&self, instr: &Instruction) -> Instruction {
        match instr {
            Instruction::Const { dest, value } => Instruction::Const {
                dest: self.remap_value(*dest),
                value: value.clone(),
            },
            Instruction::Copy { dest, src } => Instruction::Copy {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::Move { dest, src } => Instruction::Move {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::BinOp { dest, op, lhs, rhs } => Instruction::BinOp {
                dest: self.remap_value(*dest),
                op: *op,
                lhs: self.remap_operand(lhs),
                rhs: self.remap_operand(rhs),
            },
            Instruction::UnaryOp { dest, op, operand } => Instruction::UnaryOp {
                dest: self.remap_value(*dest),
                op: *op,
                operand: self.remap_operand(operand),
            },
            Instruction::BinOpChecked {
                dest,
                overflow,
                op,
                lhs,
                rhs,
            } => Instruction::BinOpChecked {
                dest: self.remap_value(*dest),
                overflow: self.remap_value(*overflow),
                op: *op,
                lhs: self.remap_operand(lhs),
                rhs: self.remap_operand(rhs),
            },
            Instruction::UnaryOpChecked {
                dest,
                overflow,
                op,
                operand,
            } => Instruction::UnaryOpChecked {
                dest: self.remap_value(*dest),
                overflow: self.remap_value(*overflow),
                op: *op,
                operand: self.remap_operand(operand),
            },
            Instruction::Widen { dest, src } => Instruction::Widen {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::WidenFixed { dest, src } => Instruction::WidenFixed {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::Clone { dest, src } => Instruction::Clone {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::Call { site_id, dest, func, args } => Instruction::Call {
                site_id: self.remap_call_site(*site_id),
                dest: self.remap_value(*dest),
                func: func.clone(),
                args: args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Instruction::Pack { dest, ty, fields } => Instruction::Pack {
                dest: self.remap_value(*dest),
                ty: ty.clone(),
                fields: fields.iter().map(|f| self.remap_operand(f)).collect(),
            },
            Instruction::Unpack { dests, src } => Instruction::Unpack {
                dests: dests.iter().map(|d| self.remap_value(*d)).collect(),
                src: self.remap_operand(src),
            },
            Instruction::GetField {
                dest,
                src,
                field_index,
            } => Instruction::GetField {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
                field_index: *field_index,
            },
            Instruction::GetFieldRef {
                dest,
                src,
                field_index,
            } => Instruction::GetFieldRef {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
                field_index: *field_index,
            },
            Instruction::WrapSome { dest, inner } => Instruction::WrapSome {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::WrapNone { dest } => Instruction::WrapNone {
                dest: self.remap_value(*dest),
            },
            Instruction::WrapOk { dest, inner } => Instruction::WrapOk {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::WrapErr { dest, inner } => Instruction::WrapErr {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::EnumVariant {
                dest,
                variant_index,
                payload,
            } => Instruction::EnumVariant {
                dest: self.remap_value(*dest),
                variant_index: *variant_index,
                payload: payload.as_ref().map(|p| self.remap_operand(p)),
            },
            Instruction::UnwrapOption { dest, is_some, src } => Instruction::UnwrapOption {
                dest: self.remap_value(*dest),
                is_some: self.remap_value(*is_some),
                src: self.remap_operand(src),
            },
            Instruction::UnwrapResult {
                ok_dest,
                err_dest,
                is_ok,
                src,
            } => Instruction::UnwrapResult {
                ok_dest: self.remap_value(*ok_dest),
                err_dest: self.remap_value(*err_dest),
                is_ok: self.remap_value(*is_ok),
                src: self.remap_operand(src),
            },
            Instruction::ErrorFrom { dest, inner } => Instruction::ErrorFrom {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::DataFrom { dest, inner } => Instruction::DataFrom {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::ListNew { dest, elements } => Instruction::ListNew {
                dest: self.remap_value(*dest),
                elements: elements.iter().map(|e| self.remap_operand(e)).collect(),
            },
            Instruction::SetNew { dest, elements } => Instruction::SetNew {
                dest: self.remap_value(*dest),
                elements: elements.iter().map(|e| self.remap_operand(e)).collect(),
            },
            Instruction::MapNew { dest, entries } => Instruction::MapNew {
                dest: self.remap_value(*dest),
                entries: entries
                    .iter()
                    .map(|(k, v)| (self.remap_operand(k), self.remap_operand(v)))
                    .collect(),
            },
            Instruction::TensorNew {
                dest,
                shape,
                elements,
            } => Instruction::TensorNew {
                dest: self.remap_value(*dest),
                shape: shape.clone(),
                elements: elements.iter().map(|e| self.remap_operand(e)).collect(),
            },
            Instruction::TableNew { dest, rows } => Instruction::TableNew {
                dest: self.remap_value(*dest),
                rows: rows.iter().map(|r| self.remap_operand(r)).collect(),
            },
            Instruction::SlotStoreCopy { dest, value } => Instruction::SlotStoreCopy {
                dest: self.remap_slot_dest(dest),
                value: self.remap_operand(value),
            },
            Instruction::SlotStoreCopyTracked { dest, value } => {
                Instruction::SlotStoreCopyTracked {
                    dest: self.remap_slot_dest(dest),
                    value: self.remap_operand(value),
                }
            }
            Instruction::SlotStoreMove { dest, value } => Instruction::SlotStoreMove {
                dest: self.remap_slot_dest(dest),
                value: self.remap_operand(value),
            },
            Instruction::SlotStoreMoveTracked { dest, value } => {
                Instruction::SlotStoreMoveTracked {
                    dest: self.remap_slot_dest(dest),
                    value: self.remap_operand(value),
                }
            }
            Instruction::SetField {
                slot,
                field_path,
                value,
            } => Instruction::SetField {
                slot: self.remap_slot_dest(slot),
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::SetFieldTracked {
                slot,
                field_path,
                value,
            } => Instruction::SetFieldTracked {
                slot: self.remap_slot_dest(slot),
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::ParamStore { param, value } => Instruction::ParamStore {
                param: *param,
                value: self.remap_operand(value),
            },
            Instruction::ParamStoreTracked { param, value } => Instruction::ParamStoreTracked {
                param: *param,
                value: self.remap_operand(value),
            },
            Instruction::ParamSetField {
                param,
                field_path,
                value,
            } => Instruction::ParamSetField {
                param: *param,
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::ParamSetFieldTracked {
                param,
                field_path,
                value,
            } => Instruction::ParamSetFieldTracked {
                param: *param,
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::RefStore { dest, value } => Instruction::RefStore {
                dest: self.remap_operand(dest),
                value: self.remap_operand(value),
            },
            Instruction::RefStoreTracked { dest, value } => Instruction::RefStoreTracked {
                dest: self.remap_operand(dest),
                value: self.remap_operand(value),
            },
            Instruction::RefSetField {
                dest,
                field_path,
                value,
            } => Instruction::RefSetField {
                dest: self.remap_operand(dest),
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::RefSetFieldTracked {
                dest,
                field_path,
                value,
            } => Instruction::RefSetFieldTracked {
                dest: self.remap_operand(dest),
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::SlotLoadCopy { dest, slot } => Instruction::SlotLoadCopy {
                dest: self.remap_value(*dest),
                slot: self.remap_slot(*slot),
            },
            Instruction::SlotLoadMove { dest, slot } => Instruction::SlotLoadMove {
                dest: self.remap_value(*dest),
                slot: self.remap_slot(*slot),
            },
            Instruction::SlotLoadMoveTracked { dest, slot } => Instruction::SlotLoadMoveTracked {
                dest: self.remap_value(*dest),
                slot: self.remap_slot(*slot),
            },
            Instruction::Drop { operand } => Instruction::Drop {
                operand: self.remap_operand(operand),
            },
            Instruction::DropTracked { operand } => Instruction::DropTracked {
                operand: self.remap_operand(operand),
            },
            Instruction::DropViaRef { ref_value } => Instruction::DropViaRef {
                ref_value: self.remap_value(*ref_value),
            },
            Instruction::UnitEndDrop { operand } => Instruction::UnitEndDrop {
                operand: self.remap_operand(operand),
            },
            Instruction::UnitEndDropTracked { operand } => Instruction::UnitEndDropTracked {
                operand: self.remap_operand(operand),
            },
            Instruction::DebugLog { operand } => Instruction::DebugLog {
                operand: self.remap_operand(operand),
            },
            Instruction::Intrinsic {
                dest,
                intrinsic,
                args,
            } => Instruction::Intrinsic {
                dest: self.remap_value(*dest),
                intrinsic: intrinsic.clone(),
                args: args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Instruction::Nop => Instruction::Nop,
        }
    }

    fn remap_terminator(&self, term: &Terminator, continuation_block: BlockId) -> Terminator {
        match term {
            Terminator::Goto { target, args } => Terminator::Goto {
                target: self.remap_block(*target),
                args: args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Terminator::Branch {
                cond,
                then_block,
                then_args,
                else_block,
                else_args,
            } => Terminator::Branch {
                cond: self.remap_operand(cond),
                then_block: self.remap_block(*then_block),
                then_args: then_args.iter().map(|a| self.remap_operand(a)).collect(),
                else_block: self.remap_block(*else_block),
                else_args: else_args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Terminator::Return { value } => {
                // Return becomes a goto to the continuation block.
                Terminator::Goto {
                    target: continuation_block,
                    args: value.iter().map(|v| self.remap_operand(v)).collect(),
                }
            }
            // These shouldn't appear in function bodies being inlined.
            Terminator::UnitEnd { result } => Terminator::UnitEnd {
                result: result.as_ref().map(|r| self.remap_operand(r)),
            },
            Terminator::UnitEarlyReturn { value } => Terminator::UnitEarlyReturn {
                value: self.remap_operand(value),
            },
        }
    }
}

/// Inline a single call site in a function.
///
/// Returns the new function with the call inlined, or None if inlining failed.
pub fn inline_call_site(
    caller: &IrFunction,
    callee: &IrFunction,
    site: &CallSite,
) -> Option<IrFunction> {
    let mut new_func = caller.clone();

    // Set up remapping context.
    let remap = RemapContext {
        value_offset: caller.value_count,
        slot_offset: caller.slot_count,
        block_offset: caller.blocks.len() as u32,
        call_site_offset: caller.call_site_count,
    };

    // The continuation block receives the return value.
    // Its block ID is after all the inlined blocks.
    let continuation_block_id = BlockId(remap.block_offset + callee.blocks.len() as u32);

    // Split the original block at the call site.
    let orig_block = &caller.blocks[site.block_idx];

    // Instructions before the call stay in the original block.
    let before_call: Vec<Instruction> = orig_block.instructions[..site.instr_idx].to_vec();

    // Instructions after the call go to the continuation block.
    let after_call: Vec<Instruction> = orig_block.instructions[site.instr_idx + 1..].to_vec();

    // The original terminator goes to the continuation block.
    let orig_terminator = orig_block.terminator.clone();

    // Build parameter binding instructions for In params only.
    // Ref params don't need bindings - we directly substitute the argument operand.
    // Also build a map from ParamId to replacement operand for use in substitution.
    let mut param_bindings: Vec<Instruction> = Vec::new();
    let mut param_replacements: HashMap<ParamId, Operand> = HashMap::new();

    for (i, (param_id, arg)) in callee.params.iter().zip(site.args.iter()).enumerate() {
        let param_type = &callee.param_types[param_id.0 as usize];
        let param_mode = &callee.param_modes[param_id.0 as usize];

        if matches!(param_mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out) {
            // Ref/Mut/Out params borrow - directly use the argument operand.
            // No binding needed, no ownership transfer.
            // For Mut/Out params, ParamStore/ParamStoreTracked will be converted to RefStore/RefStoreTracked.
            param_replacements.insert(*param_id, arg.clone());
        } else {
            // In params transfer ownership - create a binding.
            let dest_value = remap.remap_value(ValueId(callee.value_count + i as u32));

            let instr = if param_type.is_copy() {
                Instruction::Copy {
                    dest: dest_value,
                    src: arg.clone(),
                }
            } else {
                Instruction::Move {
                    dest: dest_value,
                    src: arg.clone(),
                }
            };
            param_bindings.push(instr);
            param_replacements.insert(*param_id, Operand::Value(dest_value));
        }
    }

    // Update the original block: keep instructions before call, jump to inlined entry.
    let inlined_entry_block = remap.remap_block(BlockId(0));
    new_func.blocks[site.block_idx] = IrBlock {
        id: BlockId(site.block_idx as u32),
        params: orig_block.params.clone(),
        instructions: before_call,
        terminator: Terminator::Goto {
            target: inlined_entry_block,
            args: vec![],
        },
    };

    // Copy callee blocks with remapped IDs.
    for (i, block) in callee.blocks.iter().enumerate() {
        let mut new_instructions: Vec<Instruction> = Vec::new();

        // Add parameter bindings to the entry block.
        if i == 0 {
            new_instructions.extend(param_bindings.clone());
        }

        // Remap and copy instructions, replacing Param operands with the replacements.
        for instr in &block.instructions {
            let remapped = remap.remap_instruction(instr);
            // Replace Param operands with the corresponding replacement operands.
            let replaced = replace_params_in_instruction(&remapped, &param_replacements);
            new_instructions.push(replaced);
        }

        let new_block = IrBlock {
            id: remap.remap_block(block.id),
            params: block.params.iter().map(|v| remap.remap_value(*v)).collect(),
            instructions: new_instructions,
            terminator: remap.remap_terminator(&block.terminator, continuation_block_id),
        };
        new_func.blocks.push(new_block);
    }

    // Create continuation block.
    // If the callee returns a value, it becomes a block parameter.
    let cont_params = if callee.return_type != IrType::Unit {
        vec![site.dest]
    } else {
        vec![]
    };

    let continuation_block = IrBlock {
        id: continuation_block_id,
        params: cont_params,
        instructions: after_call,
        terminator: orig_terminator,
    };
    new_func.blocks.push(continuation_block);

    // Update function metadata.
    // Add callee's values + param binding values.
    let extra_values = callee.value_count + callee.params.len() as u32;
    new_func.value_count += extra_values;
    new_func.slot_count += callee.slot_count;
    new_func.call_site_count += callee.call_site_count;

    // Extend type arrays.
    new_func.value_types.extend(callee.value_types.iter().cloned());
    // Add types for param binding values.
    for param_id in &callee.params {
        let ty = callee.param_types[param_id.0 as usize].clone();
        new_func.value_types.push(ty);
    }
    new_func.slot_types.extend(callee.slot_types.iter().cloned());

    // Extend tracked slots (with offset).
    for slot in &callee.tracked_slots {
        new_func.tracked_slots.push(remap.remap_slot(*slot));
    }

    Some(new_func)
}

/// Replace Param operands in an instruction with the corresponding replacement operands.
fn replace_params_in_instruction(
    instr: &Instruction,
    replacements: &HashMap<ParamId, Operand>,
) -> Instruction {
    let replace_operand = |op: &Operand| -> Operand {
        if let Operand::Param(p) = op {
            if let Some(replacement) = replacements.get(p) {
                return replacement.clone();
            }
        }
        op.clone()
    };

    match instr {
        Instruction::Copy { dest, src } => Instruction::Copy {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Move { dest, src } => Instruction::Move {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::BinOp { dest, op, lhs, rhs } => Instruction::BinOp {
            dest: *dest,
            op: *op,
            lhs: replace_operand(lhs),
            rhs: replace_operand(rhs),
        },
        Instruction::UnaryOp { dest, op, operand } => Instruction::UnaryOp {
            dest: *dest,
            op: *op,
            operand: replace_operand(operand),
        },
        Instruction::BinOpChecked {
            dest,
            overflow,
            op,
            lhs,
            rhs,
        } => Instruction::BinOpChecked {
            dest: *dest,
            overflow: *overflow,
            op: *op,
            lhs: replace_operand(lhs),
            rhs: replace_operand(rhs),
        },
        Instruction::UnaryOpChecked {
            dest,
            overflow,
            op,
            operand,
        } => Instruction::UnaryOpChecked {
            dest: *dest,
            overflow: *overflow,
            op: *op,
            operand: replace_operand(operand),
        },
        Instruction::Widen { dest, src } => Instruction::Widen {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::WidenFixed { dest, src } => Instruction::WidenFixed {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Clone { dest, src } => Instruction::Clone {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Call { site_id, dest, func, args } => Instruction::Call {
            site_id: *site_id,
            dest: *dest,
            func: func.clone(),
            args: args.iter().map(replace_operand).collect(),
        },
        Instruction::Pack { dest, ty, fields } => Instruction::Pack {
            dest: *dest,
            ty: ty.clone(),
            fields: fields.iter().map(replace_operand).collect(),
        },
        Instruction::Unpack { dests, src } => Instruction::Unpack {
            dests: dests.clone(),
            src: replace_operand(src),
        },
        Instruction::GetField {
            dest,
            src,
            field_index,
        } => Instruction::GetField {
            dest: *dest,
            src: replace_operand(src),
            field_index: *field_index,
        },
        Instruction::GetFieldRef {
            dest,
            src,
            field_index,
        } => Instruction::GetFieldRef {
            dest: *dest,
            src: replace_operand(src),
            field_index: *field_index,
        },
        Instruction::WrapSome { dest, inner } => Instruction::WrapSome {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::WrapOk { dest, inner } => Instruction::WrapOk {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::WrapErr { dest, inner } => Instruction::WrapErr {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::EnumVariant {
            dest,
            variant_index,
            payload,
        } => Instruction::EnumVariant {
            dest: *dest,
            variant_index: *variant_index,
            payload: payload.as_ref().map(replace_operand),
        },
        Instruction::UnwrapOption { dest, is_some, src } => Instruction::UnwrapOption {
            dest: *dest,
            is_some: *is_some,
            src: replace_operand(src),
        },
        Instruction::UnwrapResult {
            ok_dest,
            err_dest,
            is_ok,
            src,
        } => Instruction::UnwrapResult {
            ok_dest: *ok_dest,
            err_dest: *err_dest,
            is_ok: *is_ok,
            src: replace_operand(src),
        },
        Instruction::ErrorFrom { dest, inner } => Instruction::ErrorFrom {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::DataFrom { dest, inner } => Instruction::DataFrom {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::ListNew { dest, elements } => Instruction::ListNew {
            dest: *dest,
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::SetNew { dest, elements } => Instruction::SetNew {
            dest: *dest,
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::MapNew { dest, entries } => Instruction::MapNew {
            dest: *dest,
            entries: entries
                .iter()
                .map(|(k, v)| (replace_operand(k), replace_operand(v)))
                .collect(),
        },
        Instruction::TensorNew {
            dest,
            shape,
            elements,
        } => Instruction::TensorNew {
            dest: *dest,
            shape: shape.clone(),
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::TableNew { dest, rows } => Instruction::TableNew {
            dest: *dest,
            rows: rows.iter().map(replace_operand).collect(),
        },
        Instruction::SlotStoreCopy { dest, value } => Instruction::SlotStoreCopy {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreCopyTracked { dest, value } => Instruction::SlotStoreCopyTracked {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreMove { dest, value } => Instruction::SlotStoreMove {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreMoveTracked { dest, value } => Instruction::SlotStoreMoveTracked {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SetField {
            slot,
            field_path,
            value,
        } => Instruction::SetField {
            slot: slot.clone(),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::SetFieldTracked {
            slot,
            field_path,
            value,
        } => Instruction::SetFieldTracked {
            slot: slot.clone(),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::ParamStore { param, value } => {
            // If the param is being replaced, convert to RefStore.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefStore {
                    dest: replacement.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamStore {
                    param: *param,
                    value: replace_operand(value),
                }
            }
        }
        Instruction::ParamStoreTracked { param, value } => {
            // If the param is being replaced, convert to RefStoreTracked.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefStoreTracked {
                    dest: replacement.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamStoreTracked {
                    param: *param,
                    value: replace_operand(value),
                }
            }
        }
        Instruction::ParamSetField {
            param,
            field_path,
            value,
        } => {
            // If the param is being replaced, convert to RefSetField.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefSetField {
                    dest: replacement.clone(),
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamSetField {
                    param: *param,
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            }
        }
        Instruction::ParamSetFieldTracked {
            param,
            field_path,
            value,
        } => {
            // If the param is being replaced, convert to RefSetFieldTracked.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefSetFieldTracked {
                    dest: replacement.clone(),
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamSetFieldTracked {
                    param: *param,
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            }
        }
        Instruction::RefStore { dest, value } => Instruction::RefStore {
            dest: replace_operand(dest),
            value: replace_operand(value),
        },
        Instruction::RefStoreTracked { dest, value } => Instruction::RefStoreTracked {
            dest: replace_operand(dest),
            value: replace_operand(value),
        },
        Instruction::RefSetField {
            dest,
            field_path,
            value,
        } => Instruction::RefSetField {
            dest: replace_operand(dest),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::RefSetFieldTracked {
            dest,
            field_path,
            value,
        } => Instruction::RefSetFieldTracked {
            dest: replace_operand(dest),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::Drop { operand } => Instruction::Drop {
            operand: replace_operand(operand),
        },
        Instruction::DropTracked { operand } => Instruction::DropTracked {
            operand: replace_operand(operand),
        },
        Instruction::DebugLog { operand } => Instruction::DebugLog {
            operand: replace_operand(operand),
        },
        Instruction::Intrinsic {
            dest,
            intrinsic,
            args,
        } => Instruction::Intrinsic {
            dest: *dest,
            intrinsic: intrinsic.clone(),
            args: args.iter().map(replace_operand).collect(),
        },
        // These don't have replaceable operands.
        Instruction::Const { .. }
        | Instruction::WrapNone { .. }
        | Instruction::SlotLoadCopy { .. }
        | Instruction::SlotLoadMove { .. }
        | Instruction::SlotLoadMoveTracked { .. }
        | Instruction::DropViaRef { .. }
        | Instruction::UnitEndDrop { .. }
        | Instruction::UnitEndDropTracked { .. }
        | Instruction::Nop => instr.clone(),
    }
}

/// Perform function inlining on a module according to the given directives.
pub fn inline_module(module: &IrModule, directives: &[InlineDirective]) -> InlineResult {
    let (requests, mut skipped) = resolve_directives(module, directives);

    let mut current_module = module.clone();
    let mut inlined_count = 0;

    for request in &requests {
        // Find caller and callee in current state of module.
        let caller_idx = current_module
            .functions
            .iter()
            .position(|f| f.id == request.caller);
        let callee_idx = current_module
            .functions
            .iter()
            .position(|f| f.id == request.callee);

        let (Some(caller_idx), Some(callee_idx)) = (caller_idx, callee_idx) else {
            continue;
        };

        let caller = &current_module.functions[caller_idx];
        let callee = &current_module.functions[callee_idx];

        // Find call sites.
        let call_sites = find_call_sites(caller, request.callee);

        if call_sites.is_empty() {
            let caller_name = caller.name.clone();
            let callee_name = callee.name.clone();
            skipped.push(InlineSkipReason::NoCallsFound {
                caller: caller_name,
                callee: callee_name,
            });
            continue;
        }

        // Determine which sites to inline.
        let sites_to_inline: Vec<&CallSite> = match &request.filter {
            CallSiteFilter::All => call_sites.iter().collect(),
            CallSiteFilter::AtIndex(idx) => {
                if *idx < call_sites.len() {
                    vec![&call_sites[*idx]]
                } else {
                    let caller_name = caller.name.clone();
                    let callee_name = callee.name.clone();
                    skipped.push(InlineSkipReason::CallSiteNotFound {
                        caller: caller_name,
                        callee: callee_name,
                        index: *idx,
                    });
                    continue;
                }
            }
        };

        // Inline each site (in reverse order to avoid index invalidation).
        let mut updated_caller = current_module.functions[caller_idx].clone();
        for site in sites_to_inline.into_iter().rev() {
            // Recompute site location in updated caller.
            let new_sites = find_call_sites(&updated_caller, request.callee);
            // Find the matching site by comparing block and instruction indices.
            // For simplicity, just use the site directly if it's still valid.
            if let Some(new_site) = new_sites.iter().find(|s| {
                s.block_idx == site.block_idx && s.instr_idx == site.instr_idx
            }) {
                if let Some(inlined) = inline_call_site(&updated_caller, callee, new_site) {
                    updated_caller = inlined;
                    inlined_count += 1;
                }
            }
        }

        current_module.functions[caller_idx] = updated_caller;
    }

    InlineResult {
        module: current_module,
        inlined_count,
        skipped,
    }
}

/// Perform cross-module function inlining according to the given directives.
///
/// This function handles inlining functions across module boundaries.
pub fn inline_cross_module(
    registry: &ModuleFunctionRegistry,
    ctx: &CrossModuleInlineContext,
    directives: &[InlineDirective],
) -> CrossModuleInlineResult {
    let (requests, mut skipped) = resolve_cross_module_directives(ctx, directives);

    // Clone the registry for mutation.
    let mut new_registry = registry.clone();
    let mut inlined_count = 0;

    for request in &requests {
        // Get the caller and callee functions.
        let Some(caller) = new_registry.get_module_function(request.caller.module, request.caller.func) else {
            continue;
        };
        let Some(callee) = registry.get_module_function(request.callee.module, request.callee.func) else {
            continue;
        };

        // Find call sites (cross-module aware).
        let call_sites = find_cross_module_call_sites(caller, request.caller.module, request.callee);

        if call_sites.is_empty() {
            skipped.push(InlineSkipReason::NoCallsFound {
                caller: caller.name.clone(),
                callee: callee.name.clone(),
            });
            continue;
        }

        // Determine which sites to inline.
        let sites_to_inline: Vec<&CallSite> = match &request.filter {
            CallSiteFilter::All => call_sites.iter().collect(),
            CallSiteFilter::AtIndex(idx) => {
                if *idx < call_sites.len() {
                    vec![&call_sites[*idx]]
                } else {
                    skipped.push(InlineSkipReason::CallSiteNotFound {
                        caller: caller.name.clone(),
                        callee: callee.name.clone(),
                        index: *idx,
                    });
                    continue;
                }
            }
        };

        // Inline each site (in reverse order to avoid index invalidation).
        let mut updated_caller = caller.clone();
        for site in sites_to_inline.into_iter().rev() {
            // Recompute site location in updated caller.
            let new_sites = find_cross_module_call_sites(&updated_caller, request.caller.module, request.callee);
            // Find the matching site by comparing block and instruction indices.
            if let Some(new_site) = new_sites.iter().find(|s| {
                s.block_idx == site.block_idx && s.instr_idx == site.instr_idx
            }) {
                if let Some(inlined) = inline_call_site(&updated_caller, callee, new_site) {
                    updated_caller = inlined;
                    inlined_count += 1;
                }
            }
        }

        // Update the registry with the modified caller.
        new_registry.add_module_function(request.caller.module, request.caller.func, updated_caller);
    }

    CrossModuleInlineResult {
        registry: new_registry,
        inlined_count,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_inline_directives() {
        let source = r#"
            // Comment
            inline foo bar
            inline baz qux at 2
            inline-all helper
        "#;

        let directives = parse_inline_directives(source).unwrap();
        assert_eq!(directives.len(), 3);

        assert_eq!(
            directives[0],
            InlineDirective::Inline {
                caller: "foo".to_string(),
                callee: "bar".to_string(),
            }
        );

        assert_eq!(
            directives[1],
            InlineDirective::InlineAt {
                caller: "baz".to_string(),
                callee: "qux".to_string(),
                call_index: 2,
            }
        );

        assert_eq!(
            directives[2],
            InlineDirective::InlineAll {
                callee: "helper".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_cross_module_directives() {
        let source = r#"
            inline-cross main::caller base::callee
            inline-cross-all helper::util
        "#;

        let directives = parse_inline_directives(source).unwrap();
        assert_eq!(directives.len(), 2);

        assert_eq!(
            directives[0],
            InlineDirective::InlineCross {
                caller_module: "main".to_string(),
                caller: "caller".to_string(),
                callee_module: "base".to_string(),
                callee: "callee".to_string(),
            }
        );

        assert_eq!(
            directives[1],
            InlineDirective::InlineCrossAll {
                callee_module: "helper".to_string(),
                callee: "util".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_invalid_directive() {
        let source = "invalid directive line";
        let result = parse_inline_directives(source);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_cross_directive() {
        let source = "inline-cross invalid_no_colons also_invalid";
        let result = parse_inline_directives(source);
        assert!(result.is_err());
    }
}
