//! Dynamic inliner for interpreter-based call site optimization.
//!
//! Tracks call site execution counts and performs inlining when thresholds are reached.

use std::any::Any;
use std::collections::HashMap;

use datalove_datafun_ir::{CallSiteId, FuncRef, IrFunction};
use datalove_rt::c::LocalRtHandle;

use crate::dispatch::{
    CallDispatcher, CallSiteInfo, DispatchCallContext, DispatchResult, ScriptFuncId,
};
use crate::value::{Destination, Value};

/// Key for tracking call site counts.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CallSiteKey {
    /// The function containing the call site (globally unique).
    caller: ScriptFuncId,
    /// The call site within the caller.
    call_site_id: CallSiteId,
}

/// State for a tracked call site.
#[derive(Clone, Debug)]
struct CallSiteState {
    /// Number of times this call site has been executed.
    call_count: u32,
    /// Whether inlining has been attempted for this site.
    inlined: bool,
}

/// Configuration for dynamic inlining.
#[derive(Clone, Debug)]
pub struct DynamicInlinerConfig {
    /// Minimum call count before considering inlining.
    pub threshold: u32,
}

impl Default for DynamicInlinerConfig {
    fn default() -> Self {
        Self {
            threshold: 100,
        }
    }
}

/// Dynamic inliner that tracks call counts and triggers inlining.
///
/// Implements `CallDispatcher` to intercept calls and track execution counts.
/// When a call site reaches the threshold, it triggers inlining of that site.
pub struct DynamicInliner {
    /// Configuration.
    config: DynamicInlinerConfig,
    /// Call site execution counts.
    call_sites: HashMap<CallSiteKey, CallSiteState>,
    /// Cache of inlined functions.
    ///
    /// When a function is modified by inlining, the new version is stored here.
    /// Key is the globally unique function ID of the modified caller.
    inlined_functions: HashMap<ScriptFuncId, IrFunction>,
    /// Statistics.
    stats: InlinerStats,
}

/// Statistics about inlining activity.
#[derive(Clone, Debug, Default)]
pub struct InlinerStats {
    /// Total calls tracked.
    pub calls_tracked: u64,
    /// Successful inlinings performed.
    pub inlinings_performed: u32,
    /// Inlining attempts that were skipped.
    pub inlinings_skipped: u32,
}

impl DynamicInliner {
    /// Create a new dynamic inliner with default configuration.
    pub fn new() -> Self {
        Self::with_config(DynamicInlinerConfig::default())
    }

    /// Create a new dynamic inliner with custom configuration.
    pub fn with_config(config: DynamicInlinerConfig) -> Self {
        Self {
            config,
            call_sites: HashMap::new(),
            inlined_functions: HashMap::new(),
            stats: InlinerStats::default(),
        }
    }

    /// Get inliner statistics.
    pub fn stats(&self) -> &InlinerStats {
        &self.stats
    }

    /// Get an inlined version of a function, if available.
    pub fn get_inlined_function(&self, func_id: ScriptFuncId) -> Option<&IrFunction> {
        self.inlined_functions.get(&func_id)
    }

    /// Record a call and check if inlining should be triggered.
    ///
    /// Returns `true` if inlining was triggered for this call site.
    fn record_call(
        &mut self,
        call_site_info: &CallSiteInfo,
        _callee: &IrFunction,
    ) -> bool {
        self.stats.calls_tracked += 1;

        let key = CallSiteKey {
            caller: call_site_info.caller,
            call_site_id: call_site_info.call_site_id,
        };

        let state = self.call_sites.entry(key).or_insert(CallSiteState {
            call_count: 0,
            inlined: false,
        });

        state.call_count = state.call_count.saturating_add(1);

        // Check if we should trigger inlining.
        if state.call_count >= self.config.threshold && !state.inlined {
            state.inlined = true;
            return true;
        }

        false
    }

    /// Perform inlining for a call site.
    ///
    /// This modifies the caller function to inline the callee at the specified site.
    fn perform_inlining(
        &mut self,
        call_site_info: &CallSiteInfo,
        caller: &IrFunction,
        callee: &IrFunction,
    ) {
        // Find the call site index by iterating through instructions.
        let mut call_index = 0;
        let mut found = false;

        'outer: for block in &caller.blocks {
            for instr in &block.instructions {
                if let datalove_datafun_ir::Instruction::Call { site_id, func, .. } = instr {
                    if *site_id == call_site_info.call_site_id {
                        // Verify this call is to the expected callee.
                        let matches = match func {
                            FuncRef::Local(id) => *id == callee.id,
                            FuncRef::Module { func: id, .. } => *id == callee.id,
                            FuncRef::External { func: id, .. } => *id == callee.id,
                        };
                        if matches {
                            found = true;
                            break 'outer;
                        }
                    }
                    call_index += 1;
                }
            }
        }

        if !found {
            self.stats.inlinings_skipped += 1;
            return;
        }

        // Get the caller to inline into (may be a previously inlined version).
        let caller_to_use = self
            .inlined_functions
            .get(&call_site_info.caller)
            .unwrap_or(caller);

        if let Some(inlined_func) = inline_call_site_by_index(caller_to_use, callee, call_index) {
            self.inlined_functions.insert(call_site_info.caller, inlined_func);
            self.stats.inlinings_performed += 1;
        } else {
            self.stats.inlinings_skipped += 1;
        }
    }
}

impl Default for DynamicInliner {
    fn default() -> Self {
        Self::new()
    }
}

impl CallDispatcher for DynamicInliner {
    fn dispatch_call(
        &mut self,
        _func_ref: &FuncRef,
        func: &IrFunction,
        _args: &[Value],
        _ret_dest: Destination,
        _rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        // Only track calls if we have call site info.
        let Some(call_site_info) = call_ctx.call_site_info else {
            return DispatchResult::NotHandled;
        };

        // Record the call and check if inlining should be triggered.
        let should_inline = self.record_call(&call_site_info, func);

        if should_inline {
            // Look up the caller function in local functions.
            // Only inline if caller is in the current context (same unit).
            if call_site_info.caller.unit.is_none() {
                if let Some(caller) = call_ctx.exec_ctx.find_local_function(call_site_info.caller.func_id) {
                    self.perform_inlining(&call_site_info, caller, func);
                }
            }
        }

        // Always fall through to interpreter - we're tracking, not executing.
        DispatchResult::NotHandled
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_optimized_function(&self, func_id: ScriptFuncId) -> Option<&IrFunction> {
        self.get_inlined_function(func_id)
    }
}

/// Inline a specific call site by index.
///
/// This is a simplified version that works with the dynamic inliner.
pub fn inline_call_site_by_index(
    caller: &IrFunction,
    callee: &IrFunction,
    call_index: usize,
) -> Option<IrFunction> {
    // Find the call site at the given index.
    let mut current_index = 0;

    for (block_idx, block) in caller.blocks.iter().enumerate() {
        for (instr_idx, instr) in block.instructions.iter().enumerate() {
            if let datalove_datafun_ir::Instruction::Call { dest, args, .. } = instr {
                if current_index == call_index {
                    // Found the call site - use the existing inlining function.
                    let site = datalove_datafun_inline::CallSite {
                        block_idx,
                        instr_idx,
                        dest: *dest,
                        args: args.clone(),
                    };
                    return datalove_datafun_inline::inline_call_site(caller, callee, &site);
                }
                current_index += 1;
            }
        }
    }

    None
}
