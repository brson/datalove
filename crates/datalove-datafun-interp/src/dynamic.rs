//! Dynamic inliner for interpreter-based call site optimization.
//!
//! Tracks call site execution counts and performs inlining when thresholds are reached.

use std::any::Any;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use datalove_datafun_ir::{CallSiteId, CodeRef, IrCodeUnit};
use datalove_rt::c::LocalRtHandle;

use crate::dispatch::{
    CallDispatcher, CallSiteInfo, DispatchCallContext, DispatchResult, FuncIdentity,
};
use crate::value::{Destination, Value};

/// Key for tracking call site counts.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CallSiteKey {
    /// The function containing the call site.
    caller: FuncIdentity,
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
    call_sites: FxHashMap<CallSiteKey, CallSiteState>,
    /// Cache of inlined code units.
    ///
    /// When a code unit is modified by inlining, the new version is stored here.
    /// Keyed by the caller it replaces, which has to name the owning unit: a
    /// bare `CodeRef::Local` means a different function in every script unit.
    ///
    /// Shared rather than owned outright so that handing one to the interpreter
    /// costs a reference count rather than a copy of every block in the body.
    inlined_functions: FxHashMap<FuncIdentity, Rc<IrCodeUnit>>,
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
            call_sites: FxHashMap::default(),
            inlined_functions: FxHashMap::default(),
            stats: InlinerStats::default(),
        }
    }

    /// Get inliner statistics.
    pub fn stats(&self) -> &InlinerStats {
        &self.stats
    }

    /// Get an inlined version of a code unit, if available.
    pub fn get_inlined_function(&self, func: FuncIdentity) -> Option<&Rc<IrCodeUnit>> {
        self.inlined_functions.get(&func)
    }

    /// Record a call and check if inlining should be triggered.
    ///
    /// Returns `true` if inlining was triggered for this call site.
    fn record_call(
        &mut self,
        call_site_info: &CallSiteInfo,
        _callee: &IrCodeUnit,
    ) -> bool {
        self.stats.calls_tracked += 1;

        let key = CallSiteKey {
            caller: call_site_info.caller_identity(),
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
    /// This modifies the caller code unit to inline the callee at the specified site.
    fn perform_inlining(
        &mut self,
        call_site_info: &CallSiteInfo,
        caller: &IrCodeUnit,
        callee: &IrCodeUnit,
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
                            CodeRef::Local(id) => id.0 == callee.id.0,
                            CodeRef::Module { id, .. } => id.0 == callee.id.0,
                            CodeRef::External { id, .. } => id.0 == callee.id.0,
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
            .get(&call_site_info.caller_identity())
            .map(|unit| &**unit)
            .unwrap_or(caller);

        if let Some(inlined_unit) = inline_call_site_by_index(caller_to_use, callee, call_index) {
            self.inlined_functions
                .insert(call_site_info.caller_identity(), Rc::new(inlined_unit));
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
        code_ref: &CodeRef,
        func: &IrCodeUnit,
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
            // Look up the caller function based on its CodeRef type.
            let caller = match &call_site_info.caller {
                CodeRef::Local(id) => {
                    call_ctx.exec_ctx.find_local_function(*id)
                }
                CodeRef::Module { module, id } => {
                    call_ctx.registry.get_module_function_as_unit(*module, *id)
                }
                CodeRef::External { unit, id } => {
                    call_ctx.registry.get_external_function_as_unit(*unit, *id)
                }
            };

            if let Some(caller) = caller {
                // A callee from an earlier unit calls its own unit's functions
                // by local reference, which in the caller's body would mean the
                // caller's unit.
                match code_ref {
                    CodeRef::External { unit, .. } => {
                        let callee = datalove_datafun_inline::with_calls_into_unit(func, *unit);
                        self.perform_inlining(&call_site_info, caller, &callee);
                    }
                    CodeRef::Local(_) | CodeRef::Module { .. } => {
                        self.perform_inlining(&call_site_info, caller, func);
                    }
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

    fn get_optimized_function(&self, func: FuncIdentity) -> Option<Rc<IrCodeUnit>> {
        self.get_inlined_function(func).map(Rc::clone)
    }
}

/// Inline a specific call site by index.
///
/// This is a simplified version that works with the dynamic inliner.
pub fn inline_call_site_by_index(
    caller: &IrCodeUnit,
    callee: &IrCodeUnit,
    call_index: usize,
) -> Option<IrCodeUnit> {
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
