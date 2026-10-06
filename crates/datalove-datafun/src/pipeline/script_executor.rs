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
//! if let Some((ty, val)) = executor.get_binding("x") {
//!     println!("x: {} = {}", ty, val);
//! }
//!
//! // Clean up when done.
//! executor.destroy_live_values();
//! ```

use rmx::prelude::*;
use std::sync::Arc;

use datalove_datafun_ir::{ExportBinding, IrType, IrCodeUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_interp::{CallDispatcher, ScriptEnvironment, UnitCompletion};
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

        let env = ScriptEnvironment::with_module_registry(Arc::clone(&self.shared.module_registry));
        let interp = datalove_datafun_interp::IrInterpreter::new_with_options(debug_mode, call_dispatcher);

        Some(ScriptExecutor {
            script_ctx: lower::ScriptLowerContext::new(),
            unit_exports: Vec::new(),
            env,
            interp,
        })
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

        let env = ScriptEnvironment::with_module_registry(module_registry);
        let interp = datalove_datafun_interp::IrInterpreter::new_with_options(debug_mode, call_dispatcher);

        Some(ScriptExecutor {
            script_ctx: lower::ScriptLowerContext::new(),
            unit_exports: Vec::new(),
            env,
            interp,
        })
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
    /// The bindings on offer, folded from `unit_exports`.
    ///
    /// Derived rather than accumulated into, because re-executing unit `i`
    /// replaces what unit `i` exports and a fold would leave the names it used
    /// to export standing.
    script_ctx: lower::ScriptLowerContext,
    /// What each executed unit exports, indexed by unit.
    unit_exports: Vec<UnitExports>,
    pub env: ScriptEnvironment,
    interp: datalove_datafun_interp::IrInterpreter,
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
    /// Execute a compiled fragment unit.
    ///
    /// Registers bindings from the unit and executes it.
    pub fn execute_fragment(&mut self, ir_unit: &IrCodeUnit) -> String {
        // Register bindings for future lookups.
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
                let value = datalove_datafun_interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let output_str = self.interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));
                self.interp.destroy_value(&value);
                output_str
            }
            Err(e) => format!("Error: {:?}", e),
        }
    }

    /// Execute a compiled expression unit.
    ///
    /// Registers bindings from the unit and executes it, returning (type, value).
    pub fn execute_expr(&mut self, ir_unit: &IrCodeUnit) -> (Option<String>, String) {
        // Register bindings for future lookups.
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
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    self.interp.destroy_value(&value);
                    output_str
                }
                Ok(UnitCompletion::EarlyReturn) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: ret_buffer.as_mut_ptr(),
                        tydesc: ret_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    self.interp.destroy_value(&value);
                    output_str
                }
                Err(e) => format!("Error: {:?}", e),
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
                        return (Some(ty), value);
                    }
                    None => "(fragment executed)".S(),
                },
                Err(e) => format!("Error: {:?}", e),
            }
        };

        (result_ty, output)
    }

    /// Register bindings from an IR unit for future lookups.
    ///
    /// The unit index this assigns is the one external operands in later units
    /// will use to index the frame store, so the two have to agree. They only
    /// do while every unit that is registered goes on to execute: a unit that
    /// errors out leaves no frame behind.
    fn register_bindings(&mut self, ir_unit: &IrCodeUnit) {
        assert_eq!(
            self.unit_exports.len(),
            self.env.frames.unit_count(),
            "script unit index diverged from the frame store",
        );
        self.unit_exports.push(UnitExports::of(ir_unit));
        self.rebuild_script_ctx();
    }

    /// Fold the per-unit exports into the bindings on offer.
    ///
    /// Oldest first, so a later unit's export shadows an earlier one of the
    /// same name, which is how a session resolves a name.
    fn rebuild_script_ctx(&mut self) {
        let mut ctx = lower::ScriptLowerContext::new();
        for (index, exports) in self.unit_exports.iter().enumerate() {
            ctx.add_exports(
                index as u32,
                &exports.bindings,
                &exports.value_types,
                &exports.slot_types,
            );
        }
        ctx.current_unit = self.unit_exports.len() as u32;
        self.script_ctx = ctx;
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
    pub fn reexecute_unit(
        &mut self,
        index: usize,
        ir_unit: &IrCodeUnit,
    ) -> (Option<String>, String) {
        assert!(
            index < self.unit_exports.len(),
            "unit {index} has not been executed, so there is nothing to replace",
        );
        self.unit_exports[index] = UnitExports::of(ir_unit);
        self.rebuild_script_ctx();

        let script_ctx = ir_unit.script_context()
            .expect("reexecute_unit requires a script code unit");
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

        let output = match completion {
            Err(e) => format!("Error: {:?}", e),
            Ok(UnitCompletion::Normal) => match (&mut expr_buffer, &shown_binding) {
                (Some(buffer), _) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: buffer.as_mut_ptr(),
                        tydesc: expr_tydesc,
                    };
                    let printed = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    self.interp.destroy_value(&value);
                    printed
                }
                (None, Some(name)) => {
                    let name = name.C();
                    let (ty, value) = self.get_binding(&name)
                        .expect("the unit named a binding the executor registered");
                    return (Some(ty), value);
                }
                (None, None) => "(fragment executed)".S(),
            },
            Ok(UnitCompletion::EarlyReturn) => {
                let value = datalove_datafun_interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let printed = self.interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));
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
            len <= self.unit_exports.len(),
            "cannot truncate to {len} units; {} have been executed",
            self.unit_exports.len(),
        );
        self.unit_exports.truncate(len);
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

    /// Get the type and value of a binding by name.
    pub fn get_binding(&mut self, name: &str) -> Option<(String, String)> {
        if let Some((unit, value_id)) = self.script_ctx.values.get(name) {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = if let Some(v) = self.env.frames.external_value(*unit, *value_id) {
                self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e))
            } else {
                "<moved>".S()
            };
            return Some((ty, val));
        }

        if let Some((unit, slot_id)) = self.script_ctx.slots.get(name) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = if let Some(v) = self.env.frames.external_slot(*unit, *slot_id) {
                self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e))
            } else {
                "<moved>".S()
            };
            return Some((ty, val));
        }

        None
    }

    /// Get all bindings as (name, kind, type, value) tuples.
    pub fn get_environment(&mut self) -> Vec<(String, String, String, String)> {
        let mut result = Vec::new();

        for (name, (unit, value_id)) in &self.script_ctx.values {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = if let Some(v) = self.env.frames.external_value(*unit, *value_id) {
                self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e))
            } else {
                "<moved>".S()
            };
            result.push((name.C(), "let".S(), ty, val));
        }

        for (name, (unit, slot_id)) in &self.script_ctx.slots {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = if let Some(v) = self.env.frames.external_slot(*unit, *slot_id) {
                self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e))
            } else {
                "<moved>".S()
            };
            result.push((name.C(), "var".S(), ty, val));
        }

        for (name, _) in &self.script_ctx.functions {
            result.push((name.C(), "fun".S(), "function".S(), "-".S()));
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
    /// Useful for inspecting dispatcher state (like inliner stats) after execution.
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
}
