//! Combined dispatcher that integrates JIT compilation with dynamic inlining.
//!
//! The OptimizingDispatcher provides a unified optimization pipeline:
//! 1. Track call sites for inlining decisions
//! 2. Get the best available IR (inlined version if available)
//! 3. Check if function is JIT-compiled; if so, execute native code
//! 4. If not compiled, record call count for future compilation
//! 5. Fall back to interpreter if needed
//! 6. Record timing if metrics enabled

use std::any::Any;
use std::time::Instant;

use datalove_datafun_ir::{FuncRef, IrFunction};
use datalove_datafun_interp::{
    CallDispatcher, DispatchCallContext, DispatchResult, Destination,
    DynamicInliner, DynamicInlinerConfig, InterpError, Value,
};
use datalove_rt::c::LocalRtHandle;

use crate::bridge;
use crate::metrics::{ExecutionMode, MetricsCollector, MetricsConfig};
use crate::trampoline::{set_dispatch_context, clear_dispatch_context, DispatchContext};
use crate::{FunctionKey, FunctionState, JitEngine, JitError};

/// Configuration for the optimizing dispatcher.
#[derive(Clone, Debug)]
pub struct OptimizingConfig {
    /// JIT compilation threshold (number of calls before compiling).
    pub jit_threshold: u32,
    /// Inlining threshold (number of calls before inlining).
    pub inline_threshold: u32,
    /// Whether metrics collection is enabled.
    pub metrics_enabled: bool,
    /// Configuration for metrics collection.
    pub metrics_config: MetricsConfig,
}

impl Default for OptimizingConfig {
    fn default() -> Self {
        Self {
            jit_threshold: 100,
            inline_threshold: 50,
            metrics_enabled: true,
            metrics_config: MetricsConfig::default(),
        }
    }
}

/// Combined optimizer that integrates JIT compilation with dynamic inlining.
///
/// Dispatch flow:
/// 1. Record call site for inlining decisions
/// 2. Get best IR (inlined version if available)
/// 3. Check if JIT-compiled; if so, execute native code
/// 4. If not compiled, record call count, maybe trigger compilation
/// 5. Fall back to interpreter if needed
/// 6. Record timing if metrics enabled
pub struct OptimizingDispatcher {
    /// Dynamic inliner for call site optimization.
    inliner: DynamicInliner,
    /// JIT engine for native code compilation.
    jit: JitEngine,
    /// Metrics collector (optional).
    metrics: Option<MetricsCollector>,
    /// Configuration.
    config: OptimizingConfig,
}

impl OptimizingDispatcher {
    /// Create a new optimizing dispatcher with default configuration.
    pub fn new() -> Result<Self, JitError> {
        Self::with_config(OptimizingConfig::default())
    }

    /// Create a new optimizing dispatcher with custom configuration.
    pub fn with_config(config: OptimizingConfig) -> Result<Self, JitError> {
        let inliner = DynamicInliner::with_config(DynamicInlinerConfig {
            threshold: config.inline_threshold,
        });
        let jit = JitEngine::new(config.jit_threshold)?;
        let metrics = if config.metrics_enabled {
            Some(MetricsCollector::with_config(config.metrics_config.clone()))
        } else {
            None
        };

        Ok(Self {
            inliner,
            jit,
            metrics,
            config,
        })
    }

    /// Get the inliner for inspection.
    pub fn inliner(&self) -> &DynamicInliner {
        &self.inliner
    }

    /// Get the JIT engine for inspection.
    pub fn jit(&self) -> &JitEngine {
        &self.jit
    }

    /// Get the metrics collector if enabled.
    pub fn metrics(&self) -> Option<&MetricsCollector> {
        self.metrics.as_ref()
    }

    /// Get mutable access to metrics collector.
    pub fn metrics_mut(&mut self) -> Option<&mut MetricsCollector> {
        self.metrics.as_mut()
    }

    /// Get the configuration.
    pub fn config(&self) -> &OptimizingConfig {
        &self.config
    }

    /// Check if we have an inlined version of a function.
    fn has_inlined_version(&self, func_ref: &FuncRef) -> bool {
        self.inliner.get_inlined_function(func_ref).is_some()
    }

    /// Try to execute via JIT if compiled.
    ///
    /// Returns Some(result) if JIT execution was attempted, None to fall back.
    fn try_jit_execution(
        &mut self,
        func_ref: &FuncRef,
        func: &IrFunction,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: &mut DispatchCallContext<'_, '_>,
        start_time: Option<Instant>,
        is_inlined: bool,
    ) -> Option<DispatchResult> {
        use datalove_datafun_interp::ExecutionContext;

        let key = FunctionKey::from(func_ref);

        // For external functions, get context from callee's unit.
        let _callee_ctx_owned: Option<ExecutionContext>;
        let compile_ctx = match func_ref {
            FuncRef::External { unit, .. } => {
                match call_ctx.registry.unit_functions(*unit) {
                    Some(unit_funcs) => {
                        _callee_ctx_owned = Some(ExecutionContext::new(unit_funcs));
                        _callee_ctx_owned.as_ref().unwrap()
                    }
                    None => {
                        return None;
                    }
                }
            }
            _ => {
                _callee_ctx_owned = None;
                call_ctx.exec_ctx
            }
        };

        // Try to get or compile JIT code.
        match self.jit.record_call_with_context(key, func, compile_ctx, call_ctx.registry) {
            Ok(Some((code_ptr, uses_sret))) => {
                // Record JIT compilation in metrics if this was a new compilation.
                if let Some(metrics) = &mut self.metrics {
                    if let Some(FunctionState::Compiled { code_size, .. }) = self.jit.states.get(&key) {
                        metrics.record_jit_compile(func_ref, *code_size);
                    }
                }

                // Set up dispatch context for potential callbacks.
                let mut dispatch_ctx = DispatchContext {
                    jit_engine: &mut self.jit,
                    interp: call_ctx.interp,
                    exec_ctx: compile_ctx,
                    registry: call_ctx.registry,
                    frames: call_ctx.frames,
                };

                // SAFETY: context is valid for duration of call.
                unsafe { set_dispatch_context(&mut dispatch_ctx) };

                // Execute JIT code.
                // SAFETY: code_ptr is a valid JIT-compiled function.
                let result = unsafe {
                    bridge::call_jit(code_ptr, uses_sret, rt_handle, args, ret_dest, &func.return_type)
                };

                clear_dispatch_context();

                // Record execution in metrics.
                if let Some(metrics) = &mut self.metrics {
                    let mode = if is_inlined {
                        ExecutionMode::InlinedJit
                    } else {
                        ExecutionMode::Jit
                    };
                    metrics.record_call(func_ref, mode, start_time);
                }

                match result {
                    Ok(()) => Some(DispatchResult::Handled(Ok(()))),
                    Err(e) => Some(DispatchResult::Handled(Err(InterpError::RuntimeError(e.to_string())))),
                }
            }
            Ok(None) => {
                // Not yet compiled, fall through to interpreter.
                None
            }
            Err(e) => {
                // Check if this is a fallback-worthy error.
                let error_str = e.to_string();
                if error_str.contains("unsupported:")
                    || error_str.contains("Duplicate definition")
                    || error_str.contains("not yet declared")
                {
                    // Mark as not JIT-able and fall back.
                    self.jit.states.insert(key, FunctionState::Interpreted { call_count: u32::MAX });
                    self.jit.record_compilation_failure();
                    None
                } else {
                    Some(DispatchResult::Handled(Err(InterpError::RuntimeError(error_str))))
                }
            }
        }
    }
}

impl Default for OptimizingDispatcher {
    fn default() -> Self {
        Self::new().expect("OptimizingDispatcher creation should not fail")
    }
}

impl CallDispatcher for OptimizingDispatcher {
    fn dispatch_call(
        &mut self,
        func_ref: &FuncRef,
        func: &IrFunction,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        mut call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        // Start timing if metrics enabled.
        let start_time = self.metrics.as_mut().and_then(|m| m.start_call());

        // Step 1: Record call site for inlining decisions.
        // The inliner tracks call sites and may trigger inlining.
        if let Some(call_site_info) = &call_ctx.call_site_info {
            // Look up the caller function.
            let caller = match &call_site_info.caller {
                FuncRef::Local(func_id) => {
                    call_ctx.exec_ctx.find_local_function(*func_id)
                }
                FuncRef::Module { module, func: func_id } => {
                    call_ctx.registry.get_module_function(*module, *func_id)
                }
                FuncRef::External { .. } => None,
            };

            if let Some(caller) = caller {
                // Record the call in the inliner (may trigger inlining).
                let inliner_result = self.inliner.dispatch_call(
                    func_ref,
                    func,
                    args,
                    ret_dest,
                    rt_handle,
                    DispatchCallContext {
                        exec_ctx: call_ctx.exec_ctx,
                        registry: call_ctx.registry,
                        frames: call_ctx.frames,
                        interp: call_ctx.interp,
                        call_site_info: call_ctx.call_site_info.clone(),
                    },
                );

                // Check if inlining was performed.
                if self.inliner.get_inlined_function(&call_site_info.caller).is_some() {
                    if let Some(metrics) = &mut self.metrics {
                        metrics.record_inlining(&call_site_info.caller);
                    }
                }

                // Inliner always returns NotHandled, so we continue.
                let _ = inliner_result;
                let _ = caller;
            }
        }

        // Step 2: Check if we have an inlined version.
        let is_inlined = self.has_inlined_version(func_ref);

        // Step 3-4: Try JIT execution.
        // Note: We always pass the original function to JIT. The inliner modifies
        // the caller, not the callee, so the callee IR is the same either way.
        if let Some(result) = self.try_jit_execution(
            func_ref,
            func,
            args,
            ret_dest,
            rt_handle,
            &mut call_ctx,
            start_time,
            is_inlined,
        ) {
            return result;
        }

        // Step 5: Fall back to interpreter.
        // Record interpreted execution in metrics.
        if let Some(metrics) = &mut self.metrics {
            metrics.record_call(func_ref, ExecutionMode::Interpreted, start_time);
        }

        DispatchResult::NotHandled
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_optimized_function(&self, func_ref: &FuncRef) -> Option<&IrFunction> {
        self.inliner.get_inlined_function(func_ref)
    }
}
