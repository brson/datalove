//! Running a script unit in the environment the units before it left.

use rmx::prelude::*;
use super::*;

impl IrInterpreter {
    /// Execute a script unit with access to previous units' values.
    ///
    /// After execution, the unit's frame and functions are added to the environment
    /// for subsequent units to reference, under the id the script compiler
    /// gave the unit, or, for a unit built without one, the next id.
    ///
    /// Returns `UnitCompletion::Normal` for regular completion, or
    /// `UnitCompletion::EarlyReturn` if `!` or `?` triggered early return.
    ///
    /// - `ret_dest`: Destination for early return (always `Result<(), Error>` type)
    /// - `expr_dest`: Destination for expression result (for expr units, `None` for fragments)
    pub fn execute_script_unit_in_env(
        &mut self,
        unit: &IrCodeUnit,
        env: &mut ScriptEnvironment,
        ret_dest: Destination,
        expr_dest: Option<Destination>,
    ) -> Result<UnitCompletion, InterpError> {
        let at = unit.script_context()
            .expect("execute_script_unit_in_env requires a script code unit")
            .unit
            .unwrap_or_else(|| env.registry.next_unit());
        assert!(
            !env.frames.has_frame(at) && env.registry.unit_functions(at).is_none_or(<[_]>::is_empty),
            "unit {at} is run as a new unit and already has a frame or functions",
        );
        self.run_script_unit(at, false, unit, env, ret_dest, expr_dest)
    }

    /// Run a script unit again in place of whatever unit `at` had.
    ///
    /// Every `(unit, value)` reference a later unit holds of it names it by
    /// `at`. What the old frame owned is destroyed when the new frame takes
    /// its place; see [`FrameStore::set_frame`]. The compiled bodies are
    /// forgotten, since `at` may have named other functions before.
    ///
    /// A unit reads only from the units before it, so re-running one cannot
    /// read what its own previous run left.
    pub fn reexecute_script_unit_in_env(
        &mut self,
        at: u32,
        unit: &IrCodeUnit,
        env: &mut ScriptEnvironment,
        ret_dest: Destination,
        expr_dest: Option<Destination>,
    ) -> Result<UnitCompletion, InterpError> {
        self.run_script_unit(at, true, unit, env, ret_dest, expr_dest)
    }

    fn run_script_unit(
        &mut self,
        at: u32,
        replacing: bool,
        unit: &IrCodeUnit,
        env: &mut ScriptEnvironment,
        ret_dest: Destination,
        expr_dest: Option<Destination>,
    ) -> Result<UnitCompletion, InterpError> {
        if replacing {
            self.forget_compiled_bodies();
        }
        let script_ctx = unit.script_context()
            .expect("execute_script_unit_in_env requires a script code unit");

        // Compute layout.
        let layout = Rc::new(IrLayout::of_unit(unit, &mut self.tydesc_table));

        // Create frame with live value tracking for script cleanup.
        let script_frame = ScriptFrame::new(unit, layout);
        let mut frame = script_frame.frame();

        let ctx = ExecutionContext::new(at, &unit.nested_units);

        // Execute blocks with registry for function lookups and frames for slot access.
        let result = self.execute_blocks(
            &unit.blocks,
            &mut frame,
            ret_dest,
            expr_dest,
            &ctx,
            &env.registry,
            &mut env.frames,
        );
        self.release_retired_call_caches();

        // On error, free everything, this unit's values and every earlier
        // unit's alike, and drop the units: what the frames the error unwound
        // owned is not destroyed a value at a time, so the runtime discards
        // all its memory instead (see `discard_runtime_state`). The session
        // is empty afterwards.
        if let Err(e) = result {
            drop(script_frame);
            env.discard_units();
            self.discard_runtime_state();
            return Err(e);
        }

        env.set_unit(
            self.runtime.handle(),
            at,
            script_frame,
            unit.nested_units.C(),
            script_ctx.unit_end_values.C(),
            script_ctx.unit_end_slots.C(),
        );

        result
    }
}
