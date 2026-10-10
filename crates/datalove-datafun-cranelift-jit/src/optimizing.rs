//! Dispatcher that decides when to JIT-compile and run native code.
//!
//! The OptimizingDispatcher provides a unified optimization pipeline:
//! 1. Check if function is JIT-compiled; if so, execute native code
//! 2. If not compiled, record call count for future compilation
//! 3. Fall back to interpreter if needed

use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use datalove_datafun_ir::{BlockId, CodeRef, IrCodeUnit};
use datalove_datafun_interp::{
    CallDispatcher, CompiledEntry, DispatchCallContext, DispatchResult, Destination, FuncIdentity,
    InterpError, SitePolicy, Value,
};
use datalove_rt::c::LocalRtHandle;

use crate::{JitEngine, JitError, JitOptLevel, DEFAULT_JIT_THRESHOLD};

/// Execution mode for the dispatcher.
#[derive(Clone, Debug)]
pub enum DispatcherMode {
    /// Use threshold-based heuristics.
    Tuned {
        /// JIT compilation threshold (number of calls before compiling).
        jit_threshold: u32,
    },
    /// Seeded pseudo-random decisions for testing.
    Chaos {
        /// Random seed for reproducibility.
        seed: u64,
        /// Probability (0-100) of triggering JIT compilation.
        compile_probability: u32,
        /// Probability (0-100) of using JIT code when available.
        use_jit_probability: u32,
    },
}

impl Default for DispatcherMode {
    fn default() -> Self {
        DispatcherMode::Tuned {
            jit_threshold: DEFAULT_JIT_THRESHOLD,
        }
    }
}

/// Configuration for the optimizing dispatcher.
#[derive(Clone, Debug)]
pub struct DispatcherConfig {
    /// Enable JIT compilation.
    pub jit_enabled: bool,
    /// Execution mode.
    pub mode: DispatcherMode,
    /// How hard Cranelift works on what it compiles.
    pub opt_level: JitOptLevel,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self::production()
    }
}

impl DispatcherConfig {
    /// Production defaults: JIT, tuned mode.
    pub fn production() -> Self {
        Self {
            jit_enabled: true,
            mode: DispatcherMode::Tuned {
                jit_threshold: DEFAULT_JIT_THRESHOLD,
            },
            opt_level: JitOptLevel::default(),
        }
    }

    /// Testing: JIT, chaos mode with given seed.
    pub fn chaos(seed: u64) -> Self {
        Self {
            jit_enabled: true,
            mode: DispatcherMode::Chaos {
                seed,
                compile_probability: 50,
                use_jit_probability: 50,
            },
            opt_level: JitOptLevel::default(),
        }
    }

    /// Derive seed from hashable value (for test reproducibility).
    pub fn chaos_from_hashable<H: Hash>(value: H) -> Self {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        Self::chaos(hasher.finish())
    }

    /// Pure interpreter: JIT disabled.
    pub fn interpreter_only() -> Self {
        Self {
            jit_enabled: false,
            mode: DispatcherMode::Tuned {
                jit_threshold: u32::MAX,
            },
            opt_level: JitOptLevel::default(),
        }
    }

    /// Chaos mode with custom probabilities.
    pub fn chaos_with_probabilities(
        seed: u64,
        compile_probability: u32,
        use_jit_probability: u32,
    ) -> Self {
        Self {
            jit_enabled: true,
            mode: DispatcherMode::Chaos {
                seed,
                compile_probability: compile_probability.min(100),
                use_jit_probability: use_jit_probability.min(100),
            },
            opt_level: JitOptLevel::default(),
        }
    }
}

/// Dispatcher that JIT-compiles hot functions and runs their native code.
///
/// Dispatch flow:
/// 1. Check if JIT-compiled; if so, execute native code
/// 2. If not compiled, record call count, maybe trigger compilation
/// 3. Fall back to interpreter if needed
pub struct OptimizingDispatcher {
    /// JIT engine for native code compilation.
    jit: JitEngine,
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

/// The probabilities, in percent, of chaos mode's two decisions.
struct ChaosProbabilities {
    compile: u32,
    use_jit: u32,
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
        let (jit_threshold, rng_state) = match &config.mode {
            DispatcherMode::Tuned { jit_threshold } => (*jit_threshold, 0),
            DispatcherMode::Chaos { seed, .. } => {
                // In chaos mode, use threshold=1 so compilation is always possible.
                (1, *seed)
            }
        };

        let chaos = match &config.mode {
            DispatcherMode::Chaos { compile_probability, use_jit_probability, .. } => {
                Some(ChaosProbabilities {
                    compile: *compile_probability,
                    use_jit: *use_jit_probability,
                })
            }
            DispatcherMode::Tuned { .. } => None,
        };

        let mut jit = JitEngine::with_opt_level(jit_threshold, config.opt_level)?;
        if chaos.is_some() {
            // So that a loop can be entered at any iteration, which the draws
            // in `loop_policy` then choose.
            jit.set_osr_threshold(1);
        }

        Ok(Self {
            jit,
            chaos,
            config,
            rng: ChaosRng { state: rng_state, call_counter: 0 },
        })
    }

    /// Get the JIT engine for inspection.
    pub fn jit(&self) -> &JitEngine {
        &self.jit
    }

    /// Get the JIT engine mutably, to configure it.
    pub fn jit_mut(&mut self) -> &mut JitEngine {
        &mut self.jit
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
        // Offer the call to the JIT, which counts it, may compile it, and runs
        // compiled code. In chaos mode,
        // compiling and using what was compiled are separate draws, so that
        // functions are compiled and then entered from interpreted code only
        // sometimes, which is what mixed-mode execution needs exercised.
        if self.config.jit_enabled && self.chaos_should_compile() {
            let use_probability = self.chaos.as_ref().map(|c| c.use_jit);
            let rng = &mut self.rng;
            let dispatch = self.jit.dispatch_with(
                code_ref, func, args, ret_dest, rt_handle, call_ctx,
                || rng.roll(use_probability));
            if let DispatchResult::Handled(result) = dispatch.result {
                return DispatchResult::Handled(result);
            }
        }

        DispatchResult::NotHandled
    }

    /// The engine's policy when tuned. In chaos mode a site draws, each time
    /// it asks, between the engine's policy -- entering compiled code, or
    /// counting -- and offering every call, so that the planned calls and the
    /// offered ones both run, and mix.
    fn site_policy(&mut self, func: FuncIdentity, body: &IrCodeUnit) -> SitePolicy {
        if !self.config.jit_enabled {
            return SitePolicy::Interpret;
        }
        let use_probability = self.chaos.as_ref().map(|c| c.use_jit);
        if self.rng.roll(use_probability) {
            self.jit.site_policy(func, body)
        } else {
            SitePolicy::EveryCall
        }
    }

    fn call_compiled(
        &mut self,
        func: FuncIdentity,
        entry: CompiledEntry,
        words: &[usize],
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> Result<(), InterpError> {
        self.jit.call_compiled(func, entry, words, call_ctx);
        Ok(())
    }

    /// The engine's policy when tuned. In chaos mode each asking draws
    /// whether to count toward compiling at all, and, given compiled code,
    /// whether to enter it now or run another iteration and ask again, so
    /// that loops are entered at all sorts of iterations, in all sorts of
    /// frames.
    fn loop_policy(
        &mut self,
        func: FuncIdentity,
        body: &IrCodeUnit,
        header: BlockId,
        iterations: u32,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> Result<SitePolicy, InterpError> {
        if !self.config.jit_enabled {
            return Ok(SitePolicy::Interpret);
        }
        if !self.chaos_should_compile() {
            return Ok(SitePolicy::Count(1));
        }
        let policy = self.jit.loop_policy(func, body, header, iterations, call_ctx)?;
        let use_probability = self.chaos.as_ref().map(|c| c.use_jit);
        Ok(match policy {
            SitePolicy::Enter(_) if !self.rng.roll(use_probability) => SitePolicy::Count(1),
            policy => policy,
        })
    }


    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatcher_config_production() {
        let config = DispatcherConfig::production();
        assert!(config.jit_enabled);
        match config.mode {
            DispatcherMode::Tuned { jit_threshold } => {
                assert_eq!(jit_threshold, 100);
            }
            _ => panic!("Expected Tuned mode"),
        }
    }

    #[test]
    fn test_dispatcher_config_chaos() {
        let config = DispatcherConfig::chaos(12345);
        assert!(config.jit_enabled);
        match config.mode {
            DispatcherMode::Chaos { seed, compile_probability, use_jit_probability } => {
                assert_eq!(seed, 12345);
                assert_eq!(compile_probability, 50);
                assert_eq!(use_jit_probability, 50);
            }
            _ => panic!("Expected Chaos mode"),
        }
    }

    #[test]
    fn test_dispatcher_config_interpreter_only() {
        let config = DispatcherConfig::interpreter_only();
        assert!(!config.jit_enabled);
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
        let config = DispatcherConfig::chaos_with_probabilities(42, 80, 60);
        match config.mode {
            DispatcherMode::Chaos { seed, compile_probability, use_jit_probability } => {
                assert_eq!(seed, 42);
                assert_eq!(compile_probability, 80);
                assert_eq!(use_jit_probability, 60);
            }
            _ => panic!("Expected Chaos mode"),
        }
    }

    #[test]
    fn test_dispatcher_config_chaos_probability_clamping() {
        let config = DispatcherConfig::chaos_with_probabilities(0, 200, 150);
        match config.mode {
            DispatcherMode::Chaos { compile_probability, use_jit_probability, .. } => {
                assert_eq!(compile_probability, 100);
                assert_eq!(use_jit_probability, 100);
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
        let config = DispatcherConfig::chaos_with_probabilities(42, 100, 100);
        let mut d = OptimizingDispatcher::with_config(config).unwrap();

        for _ in 0..100 {
            assert!(d.chaos_should_compile());
            assert!(d.chaos_should_use_jit());
        }

        // With 0% probability, should always return false.
        let config = DispatcherConfig::chaos_with_probabilities(42, 0, 0);
        let mut d = OptimizingDispatcher::with_config(config).unwrap();

        for _ in 0..100 {
            assert!(!d.chaos_should_compile());
            assert!(!d.chaos_should_use_jit());
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
        }
    }
}
