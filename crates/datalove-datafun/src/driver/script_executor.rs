//! Script execution for compiled IR units.
//!
//! The [`ScriptExecutor`] runs compiled IR units and maintains runtime state
//! (bindings, values) across executions. It wraps the IR interpreter and
//! provides a higher-level API for REPL-style workflows.
//!
//! Create an executor via [`CompiledModules::script_executor()`]. For compilation,
//! use a separate [`ScriptCompiler`](super::ScriptCompiler).
//!
//! # Example
//!
//! ```ignore
//! let mut executor = compiled.script_executor(DebugOutputMode::Disabled, None).unwrap();
//!
//! // Execute a compiled IR unit.
//! let output = executor.execute_fragment(&ir_unit);
//!
//! // Query bindings.
//! if let Some((ty, Some(val))) = executor.get_binding("x") {
//!     println!("x: {} = {}", ty, val);
//! }
//!
//! // Clean up when done.
//! executor.destroy_live_values();
//! ```

use rmx::prelude::*;
use std::collections::BTreeSet;
use bct::lookup::LookupMap;
use std::sync::Arc;

use datalove_datafun_ir::{ExportBinding, IrType, IrCodeUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_interp::{CallDispatcher, InterpError, ScriptEnvironment, UnitCompletion};
use datalove_rt::rust::AlignedBuffer;

use super::compiled_modules::CompiledModules;

// Extension impl for CompiledModules to create script executor.
impl<'db> CompiledModules<'db> {
    /// Create a script executor for executing compiled script units.
    ///
    /// Returns `None` if module compilation failed (has errors).
    /// The executor handles only execution; use `script_compiler()` for compilation.
    pub fn script_executor(
        &self,
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> Option<ScriptExecutor> {
        if self.has_errors() {
            return None;
        }

        Some(ScriptExecutor::new(Arc::clone(&self.shared.module_registry), debug_mode, call_dispatcher))
    }

    /// Create a script executor with a custom module registry.
    ///
    /// This allows tests to inject modified module functions (e.g., after inlining)
    /// while still using the compiled module context for script compilation.
    ///
    /// Returns `None` if module compilation failed (has errors).
    pub fn script_executor_with_module_registry(
        &self,
        module_registry: Arc<datalove_datafun_interp::ModuleFunctionRegistry>,
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> Option<ScriptExecutor> {
        if self.has_errors() {
            return None;
        }

        Some(ScriptExecutor::new(module_registry, debug_mode, call_dispatcher))
    }

    /// Get a function registry containing module functions for AOT compilation.
    ///
    /// This provides access to module function types without creating an executor.
    /// The returned registry has an empty unit registry (only module functions).
    pub fn module_registry(&self) -> datalove_datafun_interp::FunctionRegistry {
        datalove_datafun_interp::FunctionRegistry::with_module_registry(
            Arc::clone(&self.shared.module_registry)
        )
    }
}

/// Script executor for executing compiled script units.
///
/// Handles execution only. No salsa/compilation dependency.
pub struct ScriptExecutor {
    /// The bindings on offer, folded from each unit's exports.
    ///
    /// Derived rather than accumulated into, because re-executing unit `i`
    /// replaces what unit `i` exports and a fold would leave the names it used
    /// to export standing.
    ///
    /// The fold takes in the units that did not run as well, so that a name
    /// one of them binds shadows an earlier binding of it here as it does in
    /// the compiler: the units after it were lowered against it. Reading a
    /// binding asks whether its unit ran.
    script_ctx: lower::ScriptLowerContext,
    /// The units' ids, in order.
    ///
    /// A unit is known by its id everywhere else: its frame, its functions
    /// and every reference a later unit's IR holds of it. Only folding the
    /// exports and checking that a unit reads from units before it ask
    /// where it sits.
    order: Vec<u32>,
    /// What each unit exports and what became of it, by id.
    units: LookupMap<u32, UnitState>,
    pub env: ScriptEnvironment,
    interp: datalove_datafun_interp::IrInterpreter,
}

/// What became of a unit the executor was handed.
///
/// **Every unit of a session has one, whether it ran or not.** A unit that
/// fails stays in its session, so that an edit upstream can make it go green,
/// and only a unit that ran to its end offers its bindings: the rest hold their
/// place and their id and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitStatus {
    /// It ran to its end.
    Ran,
    /// It did not compile, so there was nothing to run.
    NotCompiled,
    /// It ran and returned early from its top level, through `!` or `?`.
    ///
    /// Its frame stays, holding whatever it bound before it returned, and is
    /// destroyed when the unit runs again or the session ends. What it wrote
    /// into an earlier unit's bindings before it returned stays written.
    ReturnedEarly,
    /// It compiled and was not run, because it reads from these units, by id,
    /// and they did not run to their end.
    Blocked { on: Vec<u32> },
}

/// One unit's exports and what became of it.
#[derive(Clone)]
struct UnitState {
    exports: UnitExports,
    status: UnitStatus,
}

/// One unit's exports and the types they are of.
#[derive(Clone, Default)]
struct UnitExports {
    bindings: Vec<(String, ExportBinding)>,
    value_types: Vec<IrType>,
    slot_types: Vec<IrType>,
    /// The slots of earlier units it writes, by unit id and slot index; see
    /// [`IrCodeUnit::script_slots_written`].
    writes: BTreeSet<(u32, u32)>,
}

impl UnitExports {
    fn of(ir_unit: &IrCodeUnit) -> UnitExports {
        let script_ctx = ir_unit.script_context()
            .expect("only a script code unit is executed as a unit");
        UnitExports {
            bindings: script_ctx.exports.C(),
            value_types: ir_unit.value_types.C(),
            slot_types: ir_unit.slot_types.C(),
            writes: ir_unit.script_slots_written(),
        }
    }
}

impl ScriptExecutor {
    /// An executor running against `module_registry`, with no compiler behind it.
    ///
    /// Everything it keeps comes from the units it is handed, so the registry
    /// and the units may have been compiled elsewhere and deserialized: that is
    /// how the IR replay tests run the interpreter under Miri without the
    /// compiler.
    pub fn new(
        module_registry: Arc<datalove_datafun_interp::ModuleFunctionRegistry>,
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> ScriptExecutor {
        ScriptExecutor {
            script_ctx: lower::ScriptLowerContext::new(),
            order: Vec::new(),
            units: LookupMap::default(),
            env: ScriptEnvironment::with_module_registry(module_registry),
            interp: datalove_datafun_interp::IrInterpreter::new_with_options(debug_mode, call_dispatcher),
        }
    }

    /// Execute a compiled fragment unit, as the next unit.
    ///
    /// Registers bindings from the unit and executes it, unless it reads from a
    /// unit that did not run to its end; [`Self::unit_status`] says which.
    pub fn execute_fragment(&mut self, ir_unit: &IrCodeUnit) -> String {
        if let Some(output) = self.block_appended(ir_unit) {
            return output;
        }
        self.register_bindings(ir_unit);

        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let mut ret_buffer = AlignedBuffer::for_tydesc(unsafe { &*ret_tydesc });
        let ret_dest = datalove_datafun_interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        match self.interp.execute_script_unit_in_env(ir_unit, &mut self.env, ret_dest, None) {
            Ok(UnitCompletion::Normal) => "(fragment executed)".S(),
            Ok(UnitCompletion::EarlyReturn) => {
                self.returned_early();
                let value = datalove_datafun_interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let output_str = self.interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {e}"));
                self.interp.destroy_value(&value);
                output_str
            }
            Err(e) => self.failed(e),
        }
    }

    /// Execute a compiled expression unit, as the next unit.
    ///
    /// Registers bindings from the unit and executes it, returning (type,
    /// value), unless it reads from a unit that did not run to its end; then
    /// the type is `None` and [`Self::unit_status`] says why.
    pub fn execute_expr(&mut self, ir_unit: &IrCodeUnit) -> (Option<String>, String) {
        if let Some(output) = self.block_appended(ir_unit) {
            return (None, output);
        }
        self.register_bindings(ir_unit);

        // Get the script context for result field
        let script_ctx = ir_unit.script_context()
            .expect("execute_expr requires a script code unit");
        let result_ty = script_ctx.result
            .map(|id| format!("{}", &ir_unit.value_types[id.0 as usize]));
        let shown_binding = script_ctx.result_name.C();

        let output = if let Some(result_id) = script_ctx.result {
            let ret_type = IrType::Result(Box::new(IrType::Unit));
            let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
            let mut ret_buffer = AlignedBuffer::for_tydesc(unsafe { &*ret_tydesc });
            let ret_dest = datalove_datafun_interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            let expr_type = &ir_unit.value_types[result_id.0 as usize];
            let expr_tydesc = self.interp.tydesc_table_mut().get_or_create(expr_type);
            let mut expr_buffer = AlignedBuffer::for_tydesc(unsafe { &*expr_tydesc });
            let expr_dest = datalove_datafun_interp::Destination {
                ptr: expr_buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            };

            match self.interp.execute_script_unit_in_env(ir_unit, &mut self.env, ret_dest, Some(expr_dest)) {
                Ok(UnitCompletion::Normal) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: expr_buffer.as_mut_ptr(),
                        tydesc: expr_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {e}"));
                    self.interp.destroy_value(&value);
                    output_str
                }
                Ok(UnitCompletion::EarlyReturn) => {
                    self.returned_early();
                    let value = datalove_datafun_interp::Value {
                        ptr: ret_buffer.as_mut_ptr(),
                        tydesc: ret_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {e}"));
                    self.interp.destroy_value(&value);
                    output_str
                }
                Err(e) => self.failed(e),
            }
        } else {
            // The unit computes nothing. Run it anyway so it takes its place
            // in the frame store, then read the binding it named.
            let ret_type = IrType::Result(Box::new(IrType::Unit));
            let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
            let mut ret_buffer = AlignedBuffer::for_tydesc(unsafe { &*ret_tydesc });
            let ret_dest = datalove_datafun_interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            match self.interp.execute_script_unit_in_env(ir_unit, &mut self.env, ret_dest, None) {
                Ok(_) => match &shown_binding {
                    Some(name) => {
                        let (ty, value) = self.get_binding(name)
                            .expect("the unit named a binding the executor registered");
                        return (Some(ty), value.expect("an expression unit's value is its own, and held"));
                    }
                    None => "(fragment executed)".S(),
                },
                Err(e) => self.failed(e),
            }
        };

        (result_ty, output)
    }

    /// What a unit that failed at run time leaves: nothing.
    ///
    /// The interpreter freed everything the runtime had allocated, every
    /// earlier unit's values included, and dropped the units (see
    /// `IrInterpreter::discard_runtime_state`), so the bindings on offer go
    /// too. Whoever keeps the session's compiled side has to start it again
    /// to match; the REPL does.
    ///
    /// An early return is not this: it unwinds its own unit and nothing else,
    /// which is what lets that unit stay.
    fn failed(&mut self, error: InterpError) -> String {
        self.units.clear();
        self.order.clear();
        self.rebuild_script_ctx();
        format!("Error: {error}")
    }

    /// Register bindings from an IR unit for future lookups, as the next unit.
    fn register_bindings(&mut self, ir_unit: &IrCodeUnit) {
        let id = self.check_new(ir_unit);
        self.order.push(id);
        self.units.insert(id, UnitState { exports: UnitExports::of(ir_unit), status: UnitStatus::Ran });
        self.rebuild_script_ctx();
    }

    /// Check that `ir_unit` is a unit the executor does not hold yet, and
    /// give its id.
    ///
    /// A caller that drives a compiler and an executor side by side has to
    /// hand the executor every unit the compiler keeps, a failed one through
    /// [`Self::skip_unit`].
    fn check_new(&self, ir_unit: &IrCodeUnit) -> u32 {
        let id = unit_id(ir_unit);
        assert!(!self.units.contains_key(&id), "unit {id} is appended and the executor holds it already");
        id
    }

    /// Fold the per-unit exports into the bindings on offer.
    ///
    /// Oldest first, so a later unit's export shadows an earlier one of the
    /// same name, which is how a session resolves a name.
    fn rebuild_script_ctx(&mut self) {
        let mut ctx = lower::ScriptLowerContext::new();
        for id in &self.order {
            let unit = &self.units[id];
            ctx.add_exports(
                *id,
                &unit.exports.bindings,
                &unit.exports.value_types,
                &unit.exports.slot_types,
            );
        }
        self.script_ctx = ctx;
    }

    /// Append unit `id`, which did not compile.
    ///
    /// It has no frame and exports nothing, and holds its place so that it
    /// can be run again in it once an edit makes it compile.
    pub fn skip_unit(&mut self, id: u32) {
        assert!(!self.units.contains_key(&id), "unit {id} is appended and the executor holds it already");
        self.order.push(id);
        self.units.insert(id, UnitState { exports: UnitExports::default(), status: UnitStatus::NotCompiled });
        self.rebuild_script_ctx();
    }

    /// Put unit `id`, not run yet, in at position `at`.
    ///
    /// For a unit spliced into a session: it is run in its place by
    /// [`Self::rerun_unit`], or left with no frame by [`Self::clear_unit`] if
    /// it did not compile. No other unit moves anything but its place.
    pub fn insert_unit(&mut self, at: usize, id: u32) {
        assert!(!self.units.contains_key(&id), "unit {id} is inserted and the executor holds it already");
        self.order.insert(at, id);
        self.units.insert(id, UnitState { exports: UnitExports::default(), status: UnitStatus::NotCompiled });
        self.rebuild_script_ctx();
    }

    /// Move unit `id` to position `to`, counted in the session as it is
    /// after the move.
    ///
    /// Its frame stays until it is run again in its new place, which a move
    /// always does.
    pub fn move_unit(&mut self, id: u32, to: usize) {
        let from = self.position(id);
        self.order.remove(from);
        self.order.insert(to, id);
        self.rebuild_script_ctx();
    }

    /// Take unit `id` out of the session, destroying what its frame owned.
    ///
    /// Nothing that stays reads from it once what its going reaches has run
    /// again. Its id may be given again, to the unit put back.
    pub fn remove_unit(&mut self, id: u32) {
        assert!(self.units.remove(&id).is_some(), "unit {id} is removed and the executor does not hold it");
        self.order.retain(|held| *held != id);
        self.rebuild_script_ctx();
        self.env.clear_unit(self.interp.runtime_handle(), id);
        self.interp.forget_compiled_bodies();
    }

    /// Take out every unit from position `len` on, as [`Self::remove_unit`]
    /// does one.
    ///
    /// The bindings on offer are folded again from what is left, so a name
    /// only a dropped unit exported stops resolving.
    pub fn truncate_units(&mut self, len: usize) {
        assert!(
            len <= self.order.len(),
            "cannot truncate to {len} units; the executor holds {}",
            self.order.len(),
        );
        for id in self.order.split_off(len) {
            self.units.remove(&id);
            self.env.clear_unit(self.interp.runtime_handle(), id);
        }
        self.rebuild_script_ctx();
        self.interp.forget_compiled_bodies();
    }

    /// Leave unit `id`, which an edit stopped compiling, with no frame.
    ///
    /// What its frame owned is destroyed, as re-running it would have done.
    pub fn clear_unit(&mut self, id: u32) {
        self.set_not_run(id, UnitExports::default(), UnitStatus::NotCompiled);
    }

    /// What became of unit `id`.
    pub fn unit_status(&self, id: u32) -> &UnitStatus {
        &self.unit(id).status
    }

    /// The units' ids, in order.
    pub fn unit_ids(&self) -> &[u32] {
        &self.order
    }

    /// Unit `id`'s state, which the executor must hold.
    fn unit(&self, id: u32) -> &UnitState {
        self.units.get(&id).unwrap_or_else(|| panic!("the executor holds no unit {id}"))
    }

    /// Where unit `id` sits.
    fn position(&self, id: u32) -> usize {
        self.order.iter().position(|held| *held == id)
            .unwrap_or_else(|| panic!("the executor holds no unit {id}"))
    }

    /// Put unit `id` in the state of one that did not run.
    fn set_not_run(&mut self, id: u32, exports: UnitExports, status: UnitStatus) {
        assert!(self.units.contains_key(&id), "the executor holds no unit {id}");
        self.units.insert(id, UnitState { exports, status });
        self.rebuild_script_ctx();
        self.env.clear_unit(self.interp.runtime_handle(), id);
        self.interp.forget_compiled_bodies();
    }

    /// Mark the unit just appended as one that returned early.
    fn returned_early(&mut self) {
        let id = *self.order.last().expect("the unit that returned is registered");
        self.units.get_mut(&id).expect("the unit that returned is registered").status =
            UnitStatus::ReturnedEarly;
    }

    /// The units `ir_unit` reads from that did not run to their end, by id.
    ///
    /// `at` is where the unit sits; everything it reads sits before it. A
    /// unit that is a bare name reads that binding outside its code, so the
    /// unit providing it counts too, and its slot, if it is one, as read.
    ///
    /// **So does a unit between that did not run to its end and would have
    /// written a slot this one reads, which holds nothing.** A unit that did
    /// not finish did not write the slot, so it holds what it held before,
    /// which is read as it is. But if that was nothing, the unit would have
    /// revived a binding given away, and the code after it, compiled as if
    /// it had, has nothing to read. A unit that writes the slot without
    /// reading it does not care what it held: nor does the reach, which runs
    /// such a unit again without putting back what it held, and a later
    /// unit's take may have left it empty.
    fn blockers(&self, at: usize, ir_unit: &IrCodeUnit) -> Vec<u32> {
        let mut read: BTreeSet<u32> = ir_unit.script_units_read().into_iter().collect();
        let mut slots_read = ir_unit.script_slots_read();
        let script_ctx = ir_unit.script_context().expect("a script code unit");
        if let Some(name) = &script_ctx.result_name {
            let provider = self.provider_before(at, name);
            read.extend(provider);
            let slot = provider.and_then(|provider| {
                self.unit(provider).exports.bindings.iter().find_map(|(bound, binding)| match binding {
                    ExportBinding::Slot(slot) if bound == name => Some((provider, slot.0)),
                    _ => None,
                })
            });
            slots_read.extend(slot);
        }
        let empty = |(unit, slot): (u32, u32)| {
            self.unit(unit).status == UnitStatus::Ran
                && self.env.frames.external_slot(unit, datalove_datafun_ir::SlotId(slot)).is_none()
        };
        read.extend(self.order[..at].iter().copied().filter(|writer| {
            let state = self.unit(*writer);
            state.status != UnitStatus::Ran
                && state.exports.writes.iter().any(|slot| slots_read.contains(slot) && empty(*slot))
        }));
        assert!(
            read.iter().all(|unit| self.order[..at].contains(unit)),
            "the unit at {at} reads from units {read:?}, which do not all come before it",
        );
        read.into_iter().filter(|unit| self.unit(*unit).status != UnitStatus::Ran).collect()
    }

    /// The nearest unit before position `at` that exports a value or slot
    /// `name`, by id.
    fn provider_before(&self, at: usize, name: &str) -> Option<u32> {
        self.order[..at].iter().rev().copied().find(|unit| {
            self.unit(*unit).exports.bindings.iter().any(|(bound, binding)| {
                bound == name && !matches!(binding, ExportBinding::Function(_))
            })
        })
    }

    /// Append `ir_unit` without running it, if it reads from a unit that did
    /// not run to its end, and say why.
    fn block_appended(&mut self, ir_unit: &IrCodeUnit) -> Option<String> {
        let id = self.check_new(ir_unit);
        let on = self.blockers(self.order.len(), ir_unit);
        if on.is_empty() {
            return None;
        }
        let output = blocked_output(&on);
        self.order.push(id);
        self.units.insert(id, UnitState {
            exports: UnitExports::of(ir_unit),
            status: UnitStatus::Blocked { on },
        });
        self.rebuild_script_ctx();
        Some(output)
    }

    /// Run unit `id` again, in its place.
    ///
    /// The frame that unit had is replaced and what it owned destroyed, and the
    /// names it exports are taken from the new IR, so a binding it no longer
    /// declares stops being on offer. The units after it keep their frames:
    /// only the ones the edit reaches are re-executed, and a unit that uses
    /// nothing of this one's holds no reference into it. A unit spliced in is
    /// run for the first time through here.
    ///
    /// Returns the result type and the printed result, the way
    /// [`Self::execute_expr`] does, because **an edit can reach an expression
    /// unit as readily as a fragment**: a session's history holds both, and an
    /// expression unit computes a value that has to be given somewhere to land
    /// and destroyed once read. Running one with no destination for it panics
    /// in the interpreter, where `UnitEnd` carries a result.
    ///
    /// The unit it replaces may not have run, and this unit may not run
    /// either, if it reads from a unit that did not run to its end; then the
    /// frame it had is destroyed and the type is `None`.
    pub fn rerun_unit(
        &mut self,
        id: u32,
        ir_unit: &IrCodeUnit,
    ) -> (Option<String>, String) {
        assert_eq!(
            unit_id(ir_unit), id,
            "the compiler numbered this unit differently from the executor",
        );
        let on = self.blockers(self.position(id), ir_unit);
        if !on.is_empty() {
            let output = blocked_output(&on);
            self.set_not_run(id, UnitExports::of(ir_unit), UnitStatus::Blocked { on });
            return (None, output);
        }
        self.units.insert(id, UnitState { exports: UnitExports::of(ir_unit), status: UnitStatus::Ran });
        self.rebuild_script_ctx();

        let script_ctx = ir_unit.script_context()
            .expect("rerun_unit requires a script code unit");
        let result_id = script_ctx.result;
        let shown_binding = script_ctx.result_name.C();
        let result_ty = result_id
            .map(|id| format!("{}", &ir_unit.value_types[id.0 as usize]));

        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let mut ret_buffer = AlignedBuffer::for_tydesc(unsafe { &*ret_tydesc });
        let ret_dest = datalove_datafun_interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // Somewhere for an expression's value to land. A fragment computes
        // none, and an expression that is just a name computes none either --
        // it names a binding, which is read back below.
        let mut expr_buffer = None;
        let mut expr_dest = None;
        let mut expr_tydesc = std::ptr::null();
        if let Some(id) = result_id {
            let expr_type = &ir_unit.value_types[id.0 as usize];
            expr_tydesc = self.interp.tydesc_table_mut().get_or_create(expr_type);
            let mut buffer = AlignedBuffer::for_tydesc(unsafe { &*expr_tydesc });
            expr_dest = Some(datalove_datafun_interp::Destination {
                ptr: buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            });
            expr_buffer = Some(buffer);
        }

        let completion = self.interp.reexecute_script_unit_in_env(
            id, ir_unit, &mut self.env, ret_dest, expr_dest);

        if let Ok(UnitCompletion::EarlyReturn) = completion {
            self.units.get_mut(&id).expect("the unit run is held").status = UnitStatus::ReturnedEarly;
        }
        let output = match completion {
            Err(e) => self.failed(e),
            Ok(UnitCompletion::Normal) => match (&mut expr_buffer, &shown_binding) {
                (Some(buffer), _) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: buffer.as_mut_ptr(),
                        tydesc: expr_tydesc,
                    };
                    let printed = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {e}"));
                    self.interp.destroy_value(&value);
                    printed
                }
                (None, Some(name)) => {
                    // As the unit finds it where it sits: a later unit may
                    // bind the name again.
                    let (ty, value) = self.binding_before(self.position(id), name)
                        .expect("the unit named a binding the executor registered");
                    return (Some(ty), value.expect("an expression unit's value is its own, and held"));
                }
                (None, None) => "(fragment executed)".S(),
            },
            Ok(UnitCompletion::EarlyReturn) => {
                let value = datalove_datafun_interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let printed = self.interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {e}"));
                self.interp.destroy_value(&value);
                printed
            }
        };

        (result_ty, output)
    }

    /// Point the executor at a module compilation done since it was built.
    ///
    /// **An edited module reaches a running session only through here.** A
    /// script unit's IR names a module function by `CodeRef::Module` and the
    /// executor resolves that against the registry it was given, so without
    /// this a re-executed unit calls the module as it was when the session
    /// started. The frames are untouched: a module edit moves nothing a script
    /// unit owns.
    pub fn set_module_registry(
        &mut self,
        module_registry: Arc<datalove_datafun_interp::ModuleFunctionRegistry>,
    ) {
        self.env.set_module_registry(module_registry);
        self.interp.forget_compiled_bodies();
    }

    /// The values unit `id` binds, in the order it binds them.
    ///
    /// Each is what the binding holds now, `None` for one given away. Unlike
    /// [`Self::get_environment`] this is one unit's frame, shadowed bindings
    /// included, and functions are left out. A unit that did not run binds
    /// nothing.
    pub fn unit_bindings(&mut self, id: u32) -> Vec<(String, Option<String>)> {
        if self.unit(id).status != UnitStatus::Ran {
            return Vec::new();
        }
        let held: Vec<(String, Option<_>)> = self.unit(id).exports.bindings.iter()
            .filter_map(|(name, binding)| {
                let value = match binding {
                    ExportBinding::Value(value) => self.env.frames.external_value(id, *value),
                    ExportBinding::Slot(slot) => self.env.frames.external_slot(id, *slot),
                    ExportBinding::Function(_) => return None,
                };
                Some((name.C(), value))
            })
            .collect();
        held.into_iter()
            .map(|(name, value)| {
                let printed = value.map(|value| {
                    self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("<print error: {:?}>", e))
                });
                (name, printed)
            })
            .collect()
    }

    /// The unit that provides `name` to the session, by id: the nearest that
    /// binds it, as the environment is built.
    pub fn provider_of(&self, name: &str) -> Option<u32> {
        self.script_ctx.values.get(name).map(|(unit, _)| *unit)
            .or_else(|| self.script_ctx.slots.get(name).map(|(unit, _)| *unit))
            .or_else(|| self.script_ctx.functions.get(name).map(|(unit, _)| *unit))
    }

    /// The binding `name` as a unit at position `at` finds it, its type and
    /// its value printed; the value is `None` if it was given away.
    fn binding_before(&mut self, at: usize, name: &str) -> Option<(String, Option<String>)> {
        let unit = self.provider_before(at, name)?;
        let exports = &self.unit(unit).exports;
        let (_, binding) = exports.bindings.iter().rev().find(|(bound, _)| bound == name).X();
        let (ty, value) = match binding {
            ExportBinding::Value(id) => (
                format!("{}", exports.value_types[id.0 as usize]),
                self.env.frames.external_value(unit, *id),
            ),
            ExportBinding::Slot(id) => (
                format!("{}", exports.slot_types[id.0 as usize]),
                self.env.frames.external_slot(unit, *id),
            ),
            ExportBinding::Function(_) => unreachable!("`provider_before` finds values and slots"),
        };
        Some((ty, value.map(|v| printed(&mut self.interp, &v))))
    }

    /// Get the type and value of a binding by name; the value is `None` for
    /// a binding a later unit gave away.
    ///
    /// `None` when the nearest unit binding it did not run to its end, as when
    /// nothing binds it: what that unit bound is not on offer.
    pub fn get_binding(&mut self, name: &str) -> Option<(String, Option<String>)> {
        let provider = self.script_ctx.values.get(name).map(|(unit, _)| *unit)
            .or_else(|| self.script_ctx.slots.get(name).map(|(unit, _)| *unit));
        if provider.is_some_and(|unit| self.unit(unit).status != UnitStatus::Ran) {
            return None;
        }

        if let Some((unit, value_id)) = self.script_ctx.values.get(name) {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let value = self.env.frames.external_value(*unit, *value_id);
            return Some((ty, value.map(|v| printed(&mut self.interp, &v))));
        }

        if let Some((unit, slot_id)) = self.script_ctx.slots.get(name) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let value = self.env.frames.external_slot(*unit, *slot_id);
            return Some((ty, value.map(|v| printed(&mut self.interp, &v))));
        }

        None
    }

    /// Get all bindings as (name, kind, type, value) tuples; the value is
    /// `None` for a binding a later unit gave away.
    ///
    /// Only the units that ran to their end offer bindings, so a name whose
    /// nearest binding is in one that did not is left out.
    pub fn get_environment(&mut self) -> Vec<(String, String, String, Option<String>)> {
        let mut result = Vec::new();
        let units = &self.units;
        let ran = |unit: &u32| units[unit].status == UnitStatus::Ran;

        for (name, (unit, value_id)) in self.script_ctx.values.iter_unordered().filter(|(_, (unit, _))| ran(unit)) {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = self.env.frames.external_value(*unit, *value_id).map(|v| printed(&mut self.interp, &v));
            result.push((name.C(), "let".S(), ty, val));
        }

        for (name, (unit, slot_id)) in self.script_ctx.slots.iter_unordered().filter(|(_, (unit, _))| ran(unit)) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = self.env.frames.external_slot(*unit, *slot_id).map(|v| printed(&mut self.interp, &v));
            result.push((name.C(), "var".S(), ty, val));
        }

        for (name, _) in self.script_ctx.functions.iter_unordered().filter(|(_, (unit, _))| ran(unit)) {
            result.push((name.C(), "fun".S(), "function".S(), Some("-".S())));
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Get buffered debug output.
    pub fn get_debug_buffer(&self) -> String {
        self.interp.get_debug_buffer()
    }

    /// Clear buffered debug output.
    pub fn clear_debug_buffer(&self) {
        self.interp.clear_debug_buffer();
    }

    /// Destroy live values in all frames.
    pub fn destroy_live_values(&mut self) {
        self.env.destroy_live_values(self.interp.runtime_handle());
    }

    /// Get mutable access to the native function table for registering native functions.
    pub fn native_table_mut(&mut self) -> &mut datalove_datafun_interp::NativeFunctionTable {
        self.interp.native_table_mut()
    }

    /// Tell the call dispatcher, if there is one, about natives compiled code
    /// may call; see [`CallDispatcher::natives_registered`].
    pub fn natives_registered(&self, symbols: &[(String, *const u8)], owner: Arc<dyn std::any::Any + Send + Sync>) {
        self.interp.natives_registered(symbols, owner);
    }

    /// Take the call dispatcher from the interpreter.
    ///
    /// Returns the dispatcher if one was set, leaving None in its place.
    /// Useful for inspecting dispatcher state (like JIT stats) after execution.
    pub fn take_dispatcher(&self) -> Option<Box<dyn CallDispatcher>> {
        self.interp.take_dispatcher()
    }

    /// Set or replace the call dispatcher.
    pub fn set_dispatcher(&self, dispatcher: Box<dyn CallDispatcher>) {
        self.interp.set_dispatcher(dispatcher);
    }

    /// Choose what runs function bodies, before running any.
    pub fn set_engine(&mut self, engine: datalove_datafun_interp::Engine) {
        self.interp.set_engine(engine);
    }

    /// What the interpreter's bytecode lowering has done; see
    /// `IrInterpreter::bytecode_stats`.
    pub fn bytecode_stats(&self) -> &datalove_datafun_interp::BcStats {
        self.interp.bytecode_stats()
    }
}

/// The id the script compiler gave `ir_unit`.
fn unit_id(ir_unit: &IrCodeUnit) -> u32 {
    ir_unit.script_context()
        .expect("only a script code unit is executed as a unit")
        .unit
        .expect("the script compiler numbers every unit it compiles")
}

/// What a unit that was not run reports in place of its output.
fn blocked_output(on: &[u32]) -> String {
    format!("(not run: it reads from units {on:?}, which did not finish)")
}

/// A binding's value, printed.
fn printed(interp: &mut datalove_datafun_interp::IrInterpreter, value: &datalove_datafun_interp::Value) -> String {
    interp.pretty_print_value(value)
        .unwrap_or_else(|e| format!("<print error: {:?}>", e))
}
