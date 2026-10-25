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
    /// What each unit exports and what became of it, indexed by unit.
    units: Vec<UnitState>,
    pub env: ScriptEnvironment,
    interp: datalove_datafun_interp::IrInterpreter,
}

/// What became of a unit the executor was handed.
///
/// **Every unit of a session has one, whether it ran or not.** A unit that
/// fails stays in its session, so that an edit upstream can make it go green,
/// and only a unit that ran to its end offers its bindings: the rest hold their
/// index and nothing else.
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
    /// It compiled and was not run, because it reads from these units and they
    /// did not run to their end.
    Blocked { on: Vec<usize> },
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
}

impl UnitExports {
    fn of(ir_unit: &IrCodeUnit) -> UnitExports {
        let script_ctx = ir_unit.script_context()
            .expect("only a script code unit is executed as a unit");
        UnitExports {
            bindings: script_ctx.exports.clone(),
            value_types: ir_unit.value_types.clone(),
            slot_types: ir_unit.slot_types.clone(),
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
            units: Vec::new(),
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
        let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
        let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
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
        let shown_binding = script_ctx.result_name.clone();

        let output = if let Some(result_id) = script_ctx.result {
            let ret_type = IrType::Result(Box::new(IrType::Unit));
            let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
            let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
            let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
            let ret_dest = datalove_datafun_interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            let expr_type = &ir_unit.value_types[result_id.0 as usize];
            let expr_tydesc = self.interp.tydesc_table_mut().get_or_create(expr_type);
            let (expr_size, expr_align) = unsafe { ((*expr_tydesc).size, (*expr_tydesc).align) };
            let mut expr_buffer = AlignedBuffer::with_align(expr_size as usize, expr_align as usize);
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
            let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
            let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
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
    /// too and the next unit is unit 0. Whoever keeps the session's compiled
    /// side has to start it again to match; the REPL does.
    ///
    /// An early return is not this: it unwinds its own unit and nothing else,
    /// which is what lets that unit stay.
    fn failed(&mut self, error: InterpError) -> String {
        self.units.clear();
        self.rebuild_script_ctx();
        format!("Error: {error}")
    }

    /// Register bindings from an IR unit for future lookups.
    ///
    /// The unit index this assigns is the one external operands in later units
    /// will use to index the frame store, so the two have to agree, and both
    /// have to agree with the index the compiler gave the unit.
    fn register_bindings(&mut self, ir_unit: &IrCodeUnit) {
        self.check_next_index(ir_unit);
        self.units.push(UnitState { exports: UnitExports::of(ir_unit), status: UnitStatus::Ran });
        self.rebuild_script_ctx();
    }

    /// Check that `ir_unit` is the unit the executor is about to append.
    ///
    /// A caller that drives a compiler and an executor side by side has to
    /// hand the executor every unit the compiler keeps, a failed one through
    /// [`Self::skip_unit`]. Without this check, one that dropped a failure
    /// would have every later unit read the wrong frames rather than panic.
    fn check_next_index(&self, ir_unit: &IrCodeUnit) {
        assert_eq!(
            self.units.len(),
            self.env.frames.unit_count(),
            "script unit index diverged from the frame store",
        );
        assert_eq!(
            unit_index(ir_unit),
            self.units.len(),
            "the compiler numbered this unit differently from the executor",
        );
    }

    /// Fold the per-unit exports into the bindings on offer.
    ///
    /// Oldest first, so a later unit's export shadows an earlier one of the
    /// same name, which is how a session resolves a name.
    fn rebuild_script_ctx(&mut self) {
        let mut ctx = lower::ScriptLowerContext::new();
        for (index, unit) in self.units.iter().enumerate() {
            ctx.add_exports(
                index as u32,
                &unit.exports.bindings,
                &unit.exports.value_types,
                &unit.exports.slot_types,
            );
        }
        ctx.current_unit = self.units.len() as u32;
        self.script_ctx = ctx;
    }

    /// Take the next index for a unit that did not compile.
    ///
    /// It has no frame and exports nothing, and is there so that the units
    /// after it are numbered as the compiler numbered them.
    pub fn skip_unit(&mut self) {
        self.units.push(UnitState { exports: UnitExports::default(), status: UnitStatus::NotCompiled });
        self.env.add_empty_unit();
        self.rebuild_script_ctx();
    }

    /// Leave unit `index`, which an edit stopped compiling, with no frame.
    ///
    /// What its frame owned is destroyed, as re-running it would have done.
    pub fn clear_unit(&mut self, index: usize) {
        self.set_not_run(index, UnitExports::default(), UnitStatus::NotCompiled);
    }

    /// What became of unit `index`.
    pub fn unit_status(&self, index: usize) -> &UnitStatus {
        &self.units[index].status
    }

    /// Put unit `index` in the state of one that did not run.
    fn set_not_run(&mut self, index: usize, exports: UnitExports, status: UnitStatus) {
        self.units[index] = UnitState { exports, status };
        self.rebuild_script_ctx();
        self.env.clear_unit(self.interp.runtime_handle(), index as u32);
        self.interp.forget_compiled_bodies();
    }

    /// Mark the unit just appended as one that returned early.
    fn returned_early(&mut self) {
        self.units.last_mut().expect("the unit that returned is registered").status =
            UnitStatus::ReturnedEarly;
    }

    /// The units `ir_unit` reads from that did not run to their end.
    ///
    /// `index` is the unit's own index; everything it reads sits before it. A
    /// unit that is a bare name reads that binding outside its code, so the
    /// unit providing it counts too.
    fn blockers(&self, index: usize, ir_unit: &IrCodeUnit) -> Vec<usize> {
        let mut read: BTreeSet<usize> =
            ir_unit.script_units_read().into_iter().map(|unit| unit as usize).collect();
        let script_ctx = ir_unit.script_context().expect("a script code unit");
        if let Some(name) = &script_ctx.result_name {
            read.extend(self.provider_before(index, name));
        }
        assert!(
            read.iter().all(|unit| *unit < index),
            "unit {index} reads from units {read:?}, which do not all come before it",
        );
        read.into_iter().filter(|unit| self.units[*unit].status != UnitStatus::Ran).collect()
    }

    /// The nearest unit before `index` that exports a value or slot `name`.
    fn provider_before(&self, index: usize, name: &str) -> Option<usize> {
        (0..index).rev().find(|unit| {
            self.units[*unit].exports.bindings.iter().any(|(bound, binding)| {
                bound == name && !matches!(binding, ExportBinding::Function(_))
            })
        })
    }

    /// Append `ir_unit` without running it, if it reads from a unit that did
    /// not run to its end, and say why.
    fn block_appended(&mut self, ir_unit: &IrCodeUnit) -> Option<String> {
        self.check_next_index(ir_unit);
        let index = self.units.len();
        let on = self.blockers(index, ir_unit);
        if on.is_empty() {
            return None;
        }
        let output = blocked_output(&on);
        self.units.push(UnitState {
            exports: UnitExports::of(ir_unit),
            status: UnitStatus::Blocked { on },
        });
        self.env.add_empty_unit();
        self.rebuild_script_ctx();
        Some(output)
    }

    /// Run a unit again in place of the one at `index`.
    ///
    /// The frame that unit had is replaced and what it owned destroyed, and the
    /// names it exports are taken from the new IR, so a binding it no longer
    /// declares stops being on offer. The units after it keep their frames:
    /// only the ones the edit reaches are re-executed, and a unit that uses
    /// nothing of this one's holds no reference into it.
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
        index: usize,
        ir_unit: &IrCodeUnit,
    ) -> (Option<String>, String) {
        assert!(
            index < self.units.len(),
            "unit {index} has not been executed, so there is nothing to replace",
        );
        assert_eq!(
            unit_index(ir_unit), index,
            "the compiler numbered this unit differently from the executor",
        );
        let on = self.blockers(index, ir_unit);
        if !on.is_empty() {
            let output = blocked_output(&on);
            self.set_not_run(index, UnitExports::of(ir_unit), UnitStatus::Blocked { on });
            return (None, output);
        }
        self.units[index] = UnitState { exports: UnitExports::of(ir_unit), status: UnitStatus::Ran };
        self.rebuild_script_ctx();

        let script_ctx = ir_unit.script_context()
            .expect("rerun_unit requires a script code unit");
        let result_id = script_ctx.result;
        let shown_binding = script_ctx.result_name.clone();
        let result_ty = result_id
            .map(|id| format!("{}", &ir_unit.value_types[id.0 as usize]));

        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
        let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
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
            let (size, align) = unsafe { ((*expr_tydesc).size, (*expr_tydesc).align) };
            let mut buffer = AlignedBuffer::with_align(size as usize, align as usize);
            expr_dest = Some(datalove_datafun_interp::Destination {
                ptr: buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            });
            expr_buffer = Some(buffer);
        }

        let completion = self.interp.reexecute_script_unit_in_env(
            index as u32, ir_unit, &mut self.env, ret_dest, expr_dest);

        if let Ok(UnitCompletion::EarlyReturn) = completion {
            self.units[index].status = UnitStatus::ReturnedEarly;
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
                    let name = name.C();
                    let (ty, value) = self.get_binding(&name)
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

    /// Drop the runtime state of every unit from `len` on.
    ///
    /// What truncating a session needs, and what a splice needs before it runs
    /// the suffix it re-derived: splicing the unit list renumbers every unit
    /// after the splice point, so the frames from there on belong to nobody and
    /// the suffix is run again as if it were being appended. The values those
    /// frames held are destroyed rather than dropped -- see
    /// `FrameStore::truncate_units`.
    ///
    /// The bindings on offer are folded again from what is left, so a name only
    /// a dropped unit exported stops resolving.
    pub fn truncate_units(&mut self, len: usize) {
        assert!(
            len <= self.units.len(),
            "cannot truncate to {len} units; {} have been executed",
            self.units.len(),
        );
        self.units.truncate(len);
        self.rebuild_script_ctx();
        self.env.truncate_units(self.interp.runtime_handle(), len);
        self.interp.forget_compiled_bodies();
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

    /// The values unit `index` binds, in the order it binds them.
    ///
    /// Each is what the binding holds now, `None` for one given away. Unlike
    /// [`Self::get_environment`] this is one unit's frame, shadowed bindings
    /// included, and functions are left out. A unit that did not run binds
    /// nothing.
    pub fn unit_bindings(&mut self, index: usize) -> Vec<(String, Option<String>)> {
        if self.units[index].status != UnitStatus::Ran {
            return Vec::new();
        }
        let unit = index as u32;
        let held: Vec<(String, Option<_>)> = self.units[index].exports.bindings.iter()
            .filter_map(|(name, binding)| {
                let value = match binding {
                    ExportBinding::Value(id) => self.env.frames.external_value(unit, *id),
                    ExportBinding::Slot(id) => self.env.frames.external_slot(unit, *id),
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

    /// The unit that provides `name` to the session: the nearest that binds
    /// it, as the environment is built.
    pub fn provider_of(&self, name: &str) -> Option<usize> {
        self.script_ctx.values.get(name).map(|(unit, _)| *unit)
            .or_else(|| self.script_ctx.slots.get(name).map(|(unit, _)| *unit))
            .or_else(|| self.script_ctx.functions.get(name).map(|(unit, _)| *unit))
            .map(|unit| unit as usize)
    }

    /// Get the type and value of a binding by name; the value is `None` for
    /// a binding a later unit gave away.
    ///
    /// `None` when the nearest unit binding it did not run to its end, as when
    /// nothing binds it: what that unit bound is not on offer.
    pub fn get_binding(&mut self, name: &str) -> Option<(String, Option<String>)> {
        let provider = self.script_ctx.values.get(name).map(|(unit, _)| *unit)
            .or_else(|| self.script_ctx.slots.get(name).map(|(unit, _)| *unit));
        if provider.is_some_and(|unit| self.units[unit as usize].status != UnitStatus::Ran) {
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
        let ran = |unit: &u32| self.units[*unit as usize].status == UnitStatus::Ran;

        for (name, (unit, value_id)) in self.script_ctx.values.iter().filter(|(_, (unit, _))| ran(unit)) {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = self.env.frames.external_value(*unit, *value_id).map(|v| printed(&mut self.interp, &v));
            result.push((name.C(), "let".S(), ty, val));
        }

        for (name, (unit, slot_id)) in self.script_ctx.slots.iter().filter(|(_, (unit, _))| ran(unit)) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = self.env.frames.external_slot(*unit, *slot_id).map(|v| printed(&mut self.interp, &v));
            result.push((name.C(), "var".S(), ty, val));
        }

        for (name, _) in self.script_ctx.functions.iter().filter(|(_, (unit, _))| ran(unit)) {
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

/// The index the script compiler gave `ir_unit`.
fn unit_index(ir_unit: &IrCodeUnit) -> usize {
    ir_unit.script_context()
        .expect("only a script code unit is executed as a unit")
        .unit
        .expect("the script compiler numbers every unit it compiles") as usize
}

/// What a unit that was not run reports in place of its output.
fn blocked_output(on: &[usize]) -> String {
    format!("(not run: it reads from units {on:?}, which did not finish)")
}

/// A binding's value, printed.
fn printed(interp: &mut datalove_datafun_interp::IrInterpreter, value: &datalove_datafun_interp::Value) -> String {
    interp.pretty_print_value(value)
        .unwrap_or_else(|e| format!("<print error: {:?}>", e))
}
