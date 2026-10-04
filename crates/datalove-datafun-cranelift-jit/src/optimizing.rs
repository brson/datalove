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
use std::rc::Rc;
use std::hash::{Hash, Hasher};

use datalove_datafun_ir::{CodeRef, IrCodeUnit};
use datalove_datafun_interp::{
    CallDispatcher, DispatchCallContext, DispatchResult, Destination,
    DynamicInliner, DynamicInlinerConfig, FuncIdentity, Value,
};
use datalove_rt::c::LocalRtHandle;

use crate::metrics::{ExecutionMode, MetricsCollector, MetricsConfig};
use crate::{JitEngine, JitError};

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
    /// The probabilities of chaos mode, or `None` in tuned mode.
    chaos: Option<ChaosProbabilities>,
    /// Random decisions for chaos mode.
    rng: ChaosRng,
}

/// The pseudo-random decisions chaos mode makes, seeded for reproducibility.
///
/// Its own type so that a decision can be drawn while the JIT engine beside
/// it is borrowed: whether to use compiled code is decided after the call has
/// been recorded and the code compiled.
struct ChaosRng {
    /// xorshift64 state.
    state: u64,
    /// Mixed into each draw so that a zero state still moves.
    call_counter: u64,
}

/// The probabilities, in percent, of chaos mode's three decisions.
struct ChaosProbabilities {
    compile: u32,
    use_jit: u32,
    inline: u32,
}

impl ChaosRng {
    /// A pseudo-random number in [0, 100).
    fn percent(&mut self) -> u32 {
        self.call_counter += 1;
        let mut x = self.state.wrapping_add(self.call_counter);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        (x % 100) as u32
    }

    /// Whether to go ahead, given a probability in percent; always, given none.
    fn roll(&mut self, probability: Option<u32>) -> bool {
        match probability {
            Some(p) => self.percent() < p,
            None => true,
        }
    }
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

        let chaos = match &config.mode {
            DispatcherMode::Chaos { compile_probability, use_jit_probability, inline_probability, .. } => {
                Some(ChaosProbabilities {
                    compile: *compile_probability,
                    use_jit: *use_jit_probability,
                    inline: *inline_probability,
                })
            }
            DispatcherMode::Tuned { .. } => None,
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
            chaos,
            config,
            rng: ChaosRng { state: rng_state, call_counter: 0 },
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

    /// A pseudo-random number in [0, 100) from the chaos generator.
    #[cfg(test)]
    fn random_percent(&mut self) -> u32 {
        self.rng.percent()
    }

    /// Decide whether to record the call and compile in chaos mode.
    fn chaos_should_compile(&mut self) -> bool {
        let p = self.chaos.as_ref().map(|c| c.compile);
        self.rng.roll(p)
    }

    /// Decide whether to use compiled code in chaos mode.
    #[cfg(test)]
    fn chaos_should_use_jit(&mut self) -> bool {
        let p = self.chaos.as_ref().map(|c| c.use_jit);
        self.rng.roll(p)
    }

    /// Decide whether to perform inlining in chaos mode.
    fn chaos_should_inline(&mut self) -> bool {
        let p = self.chaos.as_ref().map(|c| c.inline);
        self.rng.roll(p)
    }

    /// Check if we have an inlined version of a function.
    fn has_inlined_version(&self, func: FuncIdentity) -> bool {
        self.inliner.get_inlined_function(func).is_some()
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
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        // Start timing if metrics enabled.
        let start_time = self.metrics.as_mut().and_then(|m| m.start_call());

        // Determine whether to attempt inlining based on mode.
        let should_inline = self.config.inlining_enabled && self.chaos_should_inline();

        // Step 1: Record call site for inlining decisions.
        // The inliner tracks call sites and may trigger inlining.
        if should_inline {
            if let Some(call_site_info) = &call_ctx.call_site_info {
                // The inliner resolves the caller itself, so there is nothing to
                // look up here. Its answer is always `NotHandled` -- it counts
                // call sites and rewrites bodies, it does not execute -- so what
                // it did is read back out of it rather than returned.
                self.inliner.dispatch_call(
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
                        shape_descriptors: call_ctx.shape_descriptors,
                    },
                );

                if self.inliner.get_inlined_function(call_site_info.caller_identity()).is_some() {
                    if let Some(metrics) = &mut self.metrics {
                        metrics.record_inlining(call_site_info.caller_identity());
                    }
                }
            }
        }

        let func_id = FuncIdentity::of(code_ref, call_ctx.exec_ctx.unit());

        // Step 2-4: Offer the call to the JIT, which counts it, may compile
        // it, and runs compiled code. The inliner modifies the caller, not the
        // callee, so the callee IR is the same either way. In chaos mode,
        // compiling and using what was compiled are separate draws, so that
        // functions are compiled and then entered from interpreted code only
        // sometimes, which is what mixed-mode execution needs exercised.
        if self.config.jit_enabled && self.chaos_should_compile() {
            let is_inlined = self.has_inlined_version(func_id);
            let use_probability = self.chaos.as_ref().map(|c| c.use_jit);
            let rng = &mut self.rng;
            let dispatch = self.jit.dispatch_with(
                code_ref, func, args, ret_dest, rt_handle, call_ctx,
                || rng.roll(use_probability));

            if let Some(metrics) = &mut self.metrics {
                if let Some(code_size) = dispatch.compiled_now {
                    metrics.record_jit_compile(func_id, code_size);
                }
                if matches!(dispatch.result, DispatchResult::Handled(Ok(()))) {
                    let mode = if is_inlined { ExecutionMode::InlinedJit } else { ExecutionMode::Jit };
                    metrics.record_call(func_id, mode, start_time);
                }
            }
            if let DispatchResult::Handled(result) = dispatch.result {
                return DispatchResult::Handled(result);
            }
        }

        // Step 5: Fall back to interpreter.
        // Record interpreted execution in metrics.
        if let Some(metrics) = &mut self.metrics {
            metrics.record_call(func_id, ExecutionMode::Interpreted, start_time);
        }

        DispatchResult::NotHandled
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_optimized_function(&self, func: FuncIdentity) -> Option<Rc<IrCodeUnit>> {
        self.inliner.get_inlined_function(func).map(Rc::clone)
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
