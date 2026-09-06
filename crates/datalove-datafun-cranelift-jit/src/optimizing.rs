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
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use datalove_datafun_ir::{CodeRef, IrCodeUnit};
use datalove_datafun_interp::{
    CallDispatcher, DispatchCallContext, DispatchResult, Destination,
    DynamicInliner, DynamicInlinerConfig, InterpError, Value,
};
use datalove_rt::c::LocalRtHandle;

use crate::bridge;
use crate::metrics::{ExecutionMode, MetricsCollector, MetricsConfig};
use crate::trampoline::{set_dispatch_context, clear_dispatch_context, DispatchContext};
use crate::{FunctionKey, FunctionState, JitEngine, JitError};

/// Execution mode for the dispatcher.
#[derive(Clone, Debug)]
pub enum DispatcherMode {
    /// Use threshold-based heuristics.
    Tuned {
        /// JIT compilation threshold (number of calls before compiling).
        jit_threshold: u32,
        /// Inlining threshold (number of calls before inlining).
        inline_threshold: u32,
    },
    /// Seeded pseudo-random decisions for testing.
    Chaos {
        /// Random seed for reproducibility.
        seed: u64,
        /// Probability (0-100) of triggering JIT compilation.
        compile_probability: u32,
        /// Probability (0-100) of using JIT code when available.
        use_jit_probability: u32,
        /// Probability (0-100) of performing inlining.
        inline_probability: u32,
    },
}

impl Default for DispatcherMode {
    fn default() -> Self {
        DispatcherMode::Tuned {
            jit_threshold: 100,
            inline_threshold: 50,
        }
    }
}

/// Configuration for the optimizing dispatcher.
#[derive(Clone, Debug)]
pub struct DispatcherConfig {
    /// Enable JIT compilation.
    pub jit_enabled: bool,
    /// Enable dynamic inlining.
    pub inlining_enabled: bool,
    /// Execution mode.
    pub mode: DispatcherMode,
    /// Enable metrics collection.
    pub metrics_enabled: bool,
    /// Configuration for metrics collection.
    pub metrics_config: MetricsConfig,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self::production()
    }
}

impl DispatcherConfig {
    /// Production defaults: JIT + inlining, tuned mode.
    pub fn production() -> Self {
        Self {
            jit_enabled: true,
            inlining_enabled: true,
            mode: DispatcherMode::Tuned {
                jit_threshold: 100,
                inline_threshold: 50,
            },
            metrics_enabled: true,
            metrics_config: MetricsConfig::default(),
        }
    }

    /// Testing: JIT + inlining, chaos mode with given seed.
    pub fn chaos(seed: u64) -> Self {
        Self {
            jit_enabled: true,
            inlining_enabled: true,
            mode: DispatcherMode::Chaos {
                seed,
                compile_probability: 50,
                use_jit_probability: 50,
                inline_probability: 50,
            },
            metrics_enabled: false,
            metrics_config: MetricsConfig::default(),
        }
    }

    /// Derive seed from hashable value (for test reproducibility).
    pub fn chaos_from_hashable<H: Hash>(value: H) -> Self {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        Self::chaos(hasher.finish())
    }

    /// Pure interpreter: both JIT and inlining disabled.
    pub fn interpreter_only() -> Self {
        Self {
            jit_enabled: false,
            inlining_enabled: false,
            mode: DispatcherMode::Tuned {
                jit_threshold: u32::MAX,
                inline_threshold: u32::MAX,
            },
            metrics_enabled: false,
            metrics_config: MetricsConfig::default(),
        }
    }

    /// JIT only, no inlining.
    pub fn jit_only() -> Self {
        Self {
            jit_enabled: true,
            inlining_enabled: false,
            mode: DispatcherMode::Tuned {
                jit_threshold: 100,
                inline_threshold: u32::MAX,
            },
            metrics_enabled: true,
            metrics_config: MetricsConfig::default(),
        }
    }

    /// Chaos mode with custom probabilities.
    pub fn chaos_with_probabilities(
        seed: u64,
        compile_probability: u32,
        use_jit_probability: u32,
        inline_probability: u32,
    ) -> Self {
        Self {
            jit_enabled: true,
            inlining_enabled: true,
            mode: DispatcherMode::Chaos {
                seed,
                compile_probability: compile_probability.min(100),
                use_jit_probability: use_jit_probability.min(100),
                inline_probability: inline_probability.min(100),
            },
            metrics_enabled: false,
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
    config: DispatcherConfig,
    /// Random state for chaos mode (xorshift64).
    rng_state: u64,
    /// Call counter for chaos mode RNG.
    call_counter: u64,
}

impl OptimizingDispatcher {
    /// Create a new optimizing dispatcher with default configuration.
    pub fn new() -> Result<Self, JitError> {
        Self::with_config(DispatcherConfig::default())
    }

    /// Create a new optimizing dispatcher with custom configuration.
    pub fn with_config(config: DispatcherConfig) -> Result<Self, JitError> {
        let (jit_threshold, inline_threshold, rng_state) = match &config.mode {
            DispatcherMode::Tuned { jit_threshold, inline_threshold } => {
                (*jit_threshold, *inline_threshold, 0)
            }
            DispatcherMode::Chaos { seed, .. } => {
                // In chaos mode, use threshold=1 so compilation is always possible.
                (1, 1, *seed)
            }
        };

        let inliner = DynamicInliner::with_config(DynamicInlinerConfig {
            threshold: if config.inlining_enabled { inline_threshold } else { u32::MAX },
        });
        let jit = JitEngine::new(jit_threshold)?;
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
            rng_state,
            call_counter: 0,
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
    pub fn config(&self) -> &DispatcherConfig {
        &self.config
    }

    /// Generate a pseudo-random number in [0, 100) for chaos mode.
    fn random_percent(&mut self) -> u32 {
        // Simple xorshift64 PRNG.
        self.call_counter += 1;
        let mut x = self.rng_state.wrapping_add(self.call_counter);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        (x % 100) as u32
    }

    /// Decide whether to try compilation in chaos mode.
    fn chaos_should_compile(&mut self) -> bool {
        let prob = if let DispatcherMode::Chaos { compile_probability, .. } = &self.config.mode {
            Some(*compile_probability)
        } else {
            None
        };
        match prob {
            Some(p) => self.random_percent() < p,
            None => true,
        }
    }

    /// Decide whether to use JIT code in chaos mode.
    fn chaos_should_use_jit(&mut self) -> bool {
        let prob = if let DispatcherMode::Chaos { use_jit_probability, .. } = &self.config.mode {
            Some(*use_jit_probability)
        } else {
            None
        };
        match prob {
            Some(p) => self.random_percent() < p,
            None => true,
        }
    }

    /// Decide whether to perform inlining in chaos mode.
    fn chaos_should_inline(&mut self) -> bool {
        let prob = if let DispatcherMode::Chaos { inline_probability, .. } = &self.config.mode {
            Some(*inline_probability)
        } else {
            None
        };
        match prob {
            Some(p) => self.random_percent() < p,
            None => true,
        }
    }

    /// Check if we have an inlined version of a function.
    fn has_inlined_version(&self, code_ref: &CodeRef) -> bool {
        self.inliner.get_inlined_function(code_ref).is_some()
    }

    /// Try to execute via JIT if compiled.
    ///
    /// Returns Some(result) if JIT execution was attempted, None to fall back.
    fn try_jit_execution(
        &mut self,
        code_ref: &CodeRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: &mut DispatchCallContext<'_, '_>,
        start_time: Option<Instant>,
        is_inlined: bool,
    ) -> Option<DispatchResult> {
        use datalove_datafun_interp::ExecutionContext;

        let key = FunctionKey::from(code_ref);

        // For external functions, get context from callee's unit.
        let _callee_ctx_owned: Option<ExecutionContext>;
        let compile_ctx = match code_ref {
            CodeRef::External { unit, .. } => {
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
                        metrics.record_jit_compile(code_ref, *code_size);
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
                let func_ctx = func.function_context()
                    .expect("JIT function must have function context");
                let result = unsafe {
                    bridge::call_jit(code_ptr, uses_sret, rt_handle, args, ret_dest, &func_ctx.return_type, &func_ctx.descriptor_params, &[])
                };

                clear_dispatch_context();

                // Record execution in metrics.
                if let Some(metrics) = &mut self.metrics {
                    let mode = if is_inlined {
                        ExecutionMode::InlinedJit
                    } else {
                        ExecutionMode::Jit
                    };
                    metrics.record_call(code_ref, mode, start_time);
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
        code_ref: &CodeRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        mut call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        // Start timing if metrics enabled.
        let start_time = self.metrics.as_mut().and_then(|m| m.start_call());

        // Determine whether to attempt inlining based on mode.
        let should_inline = self.config.inlining_enabled && self.chaos_should_inline();

        // Step 1: Record call site for inlining decisions.
        // The inliner tracks call sites and may trigger inlining.
        if should_inline {
            if let Some(call_site_info) = &call_ctx.call_site_info {
                // Look up the caller function.
                let caller = match &call_site_info.caller {
                    CodeRef::Local(id) => {
                        call_ctx.exec_ctx.find_local_function(*id)
                    }
                    CodeRef::Module { module, id } => {
                        call_ctx.registry.get_module_function_as_unit(*module, *id)
                    }
                    CodeRef::External { .. } => None,
                };

                if let Some(caller) = caller {
                    // Record the call in the inliner (may trigger inlining).
                    let inliner_result = self.inliner.dispatch_call(
                        code_ref,
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
        }

        // Step 2: Check if we have an inlined version.
        let is_inlined = self.has_inlined_version(code_ref);

        // Determine whether to attempt JIT based on mode.
        let should_try_jit = self.config.jit_enabled && self.chaos_should_compile();

        // Step 3-4: Try JIT execution.
        // Note: We always pass the original function to JIT. The inliner modifies
        // the caller, not the callee, so the callee IR is the same either way.
        if should_try_jit {
            // In chaos mode, we may also decide not to use the JIT even if compiled.
            let use_jit_if_compiled = self.chaos_should_use_jit();

            if use_jit_if_compiled {
                if let Some(result) = self.try_jit_execution(
                    code_ref,
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
            }
        }

        // Step 5: Fall back to interpreter.
        // Record interpreted execution in metrics.
        if let Some(metrics) = &mut self.metrics {
            metrics.record_call(code_ref, ExecutionMode::Interpreted, start_time);
        }

        DispatchResult::NotHandled
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_optimized_function(&self, code_ref: &CodeRef) -> Option<&IrCodeUnit> {
        self.inliner.get_inlined_function(code_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatcher_config_production() {
        let config = DispatcherConfig::production();
        assert!(config.jit_enabled);
        assert!(config.inlining_enabled);
        assert!(config.metrics_enabled);
        match config.mode {
            DispatcherMode::Tuned { jit_threshold, inline_threshold } => {
                assert_eq!(jit_threshold, 100);
                assert_eq!(inline_threshold, 50);
            }
            _ => panic!("Expected Tuned mode"),
        }
    }

    #[test]
    fn test_dispatcher_config_chaos() {
        let config = DispatcherConfig::chaos(12345);
        assert!(config.jit_enabled);
        assert!(config.inlining_enabled);
        assert!(!config.metrics_enabled);
        match config.mode {
            DispatcherMode::Chaos { seed, compile_probability, use_jit_probability, inline_probability } => {
                assert_eq!(seed, 12345);
                assert_eq!(compile_probability, 50);
                assert_eq!(use_jit_probability, 50);
                assert_eq!(inline_probability, 50);
            }
            _ => panic!("Expected Chaos mode"),
        }
    }

    #[test]
    fn test_dispatcher_config_interpreter_only() {
        let config = DispatcherConfig::interpreter_only();
        assert!(!config.jit_enabled);
        assert!(!config.inlining_enabled);
        assert!(!config.metrics_enabled);
    }

    #[test]
    fn test_dispatcher_config_jit_only() {
        let config = DispatcherConfig::jit_only();
        assert!(config.jit_enabled);
        assert!(!config.inlining_enabled);
        assert!(config.metrics_enabled);
    }

    #[test]
    fn test_dispatcher_config_chaos_from_hashable() {
        let config1 = DispatcherConfig::chaos_from_hashable("test_input");
        let config2 = DispatcherConfig::chaos_from_hashable("test_input");
        let config3 = DispatcherConfig::chaos_from_hashable("other_input");

        // Same input should produce same seed.
        match (&config1.mode, &config2.mode) {
            (
                DispatcherMode::Chaos { seed: s1, .. },
                DispatcherMode::Chaos { seed: s2, .. },
            ) => {
                assert_eq!(s1, s2);
            }
            _ => panic!("Expected Chaos modes"),
        }

        // Different input should produce different seed.
        match (&config1.mode, &config3.mode) {
            (
                DispatcherMode::Chaos { seed: s1, .. },
                DispatcherMode::Chaos { seed: s3, .. },
            ) => {
                assert_ne!(s1, s3);
            }
            _ => panic!("Expected Chaos modes"),
        }
    }

    #[test]
    fn test_dispatcher_config_chaos_with_probabilities() {
        let config = DispatcherConfig::chaos_with_probabilities(42, 80, 60, 40);
        match config.mode {
            DispatcherMode::Chaos { seed, compile_probability, use_jit_probability, inline_probability } => {
                assert_eq!(seed, 42);
                assert_eq!(compile_probability, 80);
                assert_eq!(use_jit_probability, 60);
                assert_eq!(inline_probability, 40);
            }
            _ => panic!("Expected Chaos mode"),
        }
    }

    #[test]
    fn test_dispatcher_config_chaos_probability_clamping() {
        let config = DispatcherConfig::chaos_with_probabilities(0, 200, 150, 999);
        match config.mode {
            DispatcherMode::Chaos { compile_probability, use_jit_probability, inline_probability, .. } => {
                assert_eq!(compile_probability, 100);
                assert_eq!(use_jit_probability, 100);
                assert_eq!(inline_probability, 100);
            }
            _ => panic!("Expected Chaos mode"),
        }
    }

    #[test]
    fn test_optimizing_dispatcher_creation_default() {
        let dispatcher = OptimizingDispatcher::new();
        assert!(dispatcher.is_ok());
    }

    #[test]
    fn test_optimizing_dispatcher_creation_with_config() {
        let config = DispatcherConfig::chaos(42);
        let dispatcher = OptimizingDispatcher::with_config(config);
        assert!(dispatcher.is_ok());
    }

    #[test]
    fn test_optimizing_dispatcher_chaos_rng_determinism() {
        // Create two dispatchers with the same chaos seed.
        let config1 = DispatcherConfig::chaos(12345);
        let config2 = DispatcherConfig::chaos(12345);

        let mut d1 = OptimizingDispatcher::with_config(config1).unwrap();
        let mut d2 = OptimizingDispatcher::with_config(config2).unwrap();

        // They should produce the same random sequence.
        for _ in 0..100 {
            assert_eq!(d1.random_percent(), d2.random_percent());
        }
    }

    #[test]
    fn test_optimizing_dispatcher_chaos_rng_different_seeds() {
        let config1 = DispatcherConfig::chaos(1);
        let config2 = DispatcherConfig::chaos(2);

        let mut d1 = OptimizingDispatcher::with_config(config1).unwrap();
        let mut d2 = OptimizingDispatcher::with_config(config2).unwrap();

        // Different seeds should produce different sequences.
        let mut different_count = 0;
        for _ in 0..100 {
            if d1.random_percent() != d2.random_percent() {
                different_count += 1;
            }
        }
        // Should be mostly different (allow some collisions).
        assert!(different_count > 50);
    }

    #[test]
    fn test_chaos_probability_methods() {
        // With 100% probability, should always return true.
        let config = DispatcherConfig::chaos_with_probabilities(42, 100, 100, 100);
        let mut d = OptimizingDispatcher::with_config(config).unwrap();

        for _ in 0..100 {
            assert!(d.chaos_should_compile());
            assert!(d.chaos_should_use_jit());
            assert!(d.chaos_should_inline());
        }

        // With 0% probability, should always return false.
        let config = DispatcherConfig::chaos_with_probabilities(42, 0, 0, 0);
        let mut d = OptimizingDispatcher::with_config(config).unwrap();

        for _ in 0..100 {
            assert!(!d.chaos_should_compile());
            assert!(!d.chaos_should_use_jit());
            assert!(!d.chaos_should_inline());
        }
    }

    #[test]
    fn test_tuned_mode_always_returns_true_for_chaos_methods() {
        let config = DispatcherConfig::production();
        let mut d = OptimizingDispatcher::with_config(config).unwrap();

        // In tuned mode, chaos methods should always return true.
        for _ in 0..100 {
            assert!(d.chaos_should_compile());
            assert!(d.chaos_should_use_jit());
            assert!(d.chaos_should_inline());
        }
    }
}
