//! Running a script unit in the environment the units before it left.

use super::*;

impl IrInterpreter {
    /// Execute a script unit with access to previous units' values.
    ///
    /// After execution, the unit's frame and functions are added to the environment
    /// for subsequent units to reference.
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
        // A unit is registered once it finishes, so the count is the index this
        // one will take, and the index its `Local` references are relative to.
        let at = env.registry.unit_count();
        self.run_script_unit(at, false, unit, env, ret_dest, expr_dest)
    }

    /// Run a script unit again in place of the unit already at `at`.
    ///
    /// **The index is given rather than taken from the store**, because the
    /// unit's `Local` references and every `(unit, value)` reference a later
    /// unit holds of it are relative to where it sits, and re-executing must
    /// not move it. What the old frame owned is destroyed when the new frame
    /// takes its place; see [`FrameStore::replace_frame`].
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
            &UnitTypes::of(unit),
            &mut frame,
            ret_dest,
            expr_dest,
            &ctx,
            &env.registry,
            &mut env.frames,
        );
        self.release_retired_call_caches();

        // On error, destroy the frame and propagate the error. A unit being
        // re-executed keeps the frame it had: the numbering cannot have a hole
        // in it, and what that frame owns is destroyed when the session ends.
        if let Err(e) = result {
            frame.destroy_on_error(
                self.runtime.handle(),
                &script_ctx.unit_end_values,
                &script_ctx.unit_end_slots,
            );
            // A unit that failed is not kept, so the next unit takes its
            // index; what was worked out for this one's functions would be
            // used for that one's.
            if !replacing {
                self.forget_unit_bodies(at);
            }
            return Err(e);
        }

        if replacing {
            env.replace_unit(
                self.runtime.handle(),
                at,
                script_frame,
                unit.nested_units.clone(),
                script_ctx.unit_end_values.clone(),
                script_ctx.unit_end_slots.clone(),
            );
        } else {
            // Add this unit's frame and code units to the environment for future units.
            env.add_unit(
                script_frame,
                unit.nested_units.clone(),
                script_ctx.unit_end_values.clone(),
                script_ctx.unit_end_slots.clone(),
            );
        }

        result
    }
}
