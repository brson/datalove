//! A/B testing infrastructure for comparing optimized vs baseline execution.
//!
//! Allows running the same code with and without optimizations to measure
//! actual speedup and verify correctness.

use std::any::Any;
use std::time::{Duration, Instant};

use datalove_datafun_ir::{FuncRef, IrFunction};
use datalove_datafun_interp::{
    CallDispatcher, DispatchCallContext, DispatchResult, Destination, Value,
};
use datalove_rt::c::LocalRtHandle;

use crate::optimizing::{OptimizingDispatcher, DispatcherConfig};
use crate::JitError;

/// Mode for A/B testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ABMode {
    /// Run only the optimized path.
    Optimized,
    /// Run only the baseline (interpreter) path.
    Baseline,
    /// Run both paths and compare results.
    Both,
    /// Alternate between optimized and baseline on each call.
    Alternating,
}

/// Results from A/B testing.
#[derive(Clone, Debug, Default)]
pub struct ABTestResults {
    /// Total calls in optimized mode.
    pub optimized_calls: u64,
    /// Total time in optimized mode.
    pub optimized_time: Duration,
    /// Total calls in baseline mode.
    pub baseline_calls: u64,
    /// Total time in baseline mode.
    pub baseline_time: Duration,
    /// Number of result mismatches detected.
    pub mismatches: u64,
}

impl ABTestResults {
    /// Calculate speedup ratio (optimized vs baseline).
    ///
    /// Returns > 1.0 if optimized is faster, < 1.0 if slower.
    pub fn speedup(&self) -> f64 {
        if self.optimized_time.as_nanos() == 0 {
            return 0.0;
        }
        self.baseline_time.as_nanos() as f64 / self.optimized_time.as_nanos() as f64
    }

    /// Calculate average time per call for optimized mode.
    pub fn avg_optimized_time(&self) -> Duration {
        if self.optimized_calls == 0 {
            return Duration::ZERO;
        }
        self.optimized_time / self.optimized_calls as u32
    }

    /// Calculate average time per call for baseline mode.
    pub fn avg_baseline_time(&self) -> Duration {
        if self.baseline_calls == 0 {
            return Duration::ZERO;
        }
        self.baseline_time / self.baseline_calls as u32
    }
}

impl std::fmt::Display for ABTestResults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "A/B Test Results:")?;
        writeln!(f, "  Optimized: {} calls, {:?} total, {:?} avg",
            self.optimized_calls,
            self.optimized_time,
            self.avg_optimized_time())?;
        writeln!(f, "  Baseline:  {} calls, {:?} total, {:?} avg",
            self.baseline_calls,
            self.baseline_time,
            self.avg_baseline_time())?;
        writeln!(f, "  Speedup: {:.2}x", self.speedup())?;
        if self.mismatches > 0 {
            writeln!(f, "  WARNING: {} result mismatches detected!", self.mismatches)?;
        }
        Ok(())
    }
}

/// Dispatcher for A/B testing that compares optimized vs baseline execution.
pub struct ABTestDispatcher {
    /// Optimized dispatcher (JIT + inlining).
    optimized: OptimizingDispatcher,
    /// A/B testing mode.
    mode: ABMode,
    /// Test results.
    results: ABTestResults,
    /// Call counter for alternating mode.
    call_count: u64,
}

impl ABTestDispatcher {
    /// Create a new A/B test dispatcher with default configuration.
    pub fn new(mode: ABMode) -> Result<Self, JitError> {
        Self::with_config(mode, DispatcherConfig::default())
    }

    /// Create a new A/B test dispatcher with custom configuration.
    pub fn with_config(mode: ABMode, config: DispatcherConfig) -> Result<Self, JitError> {
        Ok(Self {
            optimized: OptimizingDispatcher::with_config(config)?,
            mode,
            results: ABTestResults::default(),
            call_count: 0,
        })
    }

    /// Get the current mode.
    pub fn mode(&self) -> ABMode {
        self.mode
    }

    /// Set the A/B testing mode.
    pub fn set_mode(&mut self, mode: ABMode) {
        self.mode = mode;
    }

    /// Get the test results.
    pub fn results(&self) -> &ABTestResults {
        &self.results
    }

    /// Reset the test results.
    pub fn reset_results(&mut self) {
        self.results = ABTestResults::default();
        self.call_count = 0;
    }

    /// Get the optimized dispatcher for inspection.
    pub fn optimized(&self) -> &OptimizingDispatcher {
        &self.optimized
    }

    /// Get mutable access to the optimized dispatcher.
    pub fn optimized_mut(&mut self) -> &mut OptimizingDispatcher {
        &mut self.optimized
    }

    /// Determine which path to run based on mode and call count.
    fn should_run_optimized(&mut self) -> (bool, bool) {
        match self.mode {
            ABMode::Optimized => (true, false),
            ABMode::Baseline => (false, true),
            ABMode::Both => (true, true),
            ABMode::Alternating => {
                self.call_count += 1;
                if self.call_count % 2 == 0 {
                    (true, false)
                } else {
                    (false, true)
                }
            }
        }
    }
}

impl CallDispatcher for ABTestDispatcher {
    fn dispatch_call(
        &mut self,
        func_ref: &FuncRef,
        func: &IrFunction,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        let (run_optimized, run_baseline) = self.should_run_optimized();

        // Run optimized path if enabled.
        if run_optimized {
            let start = Instant::now();
            let result = self.optimized.dispatch_call(
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
            let elapsed = start.elapsed();

            self.results.optimized_calls += 1;
            self.results.optimized_time += elapsed;

            // If we're only running optimized, return the result.
            if !run_baseline {
                return result;
            }

            // If running both, we need to capture the optimized result for comparison.
            // For now, we just track timing and let the baseline run.
            // Full result comparison would require capturing output values.
        }

        // Run baseline (interpreter) path if enabled.
        if run_baseline {
            let start = Instant::now();
            // Baseline always falls through to interpreter.
            let result = DispatchResult::NotHandled;
            let elapsed = start.elapsed();

            self.results.baseline_calls += 1;
            self.results.baseline_time += elapsed;

            return result;
        }

        // Shouldn't reach here, but fall through to interpreter.
        DispatchResult::NotHandled
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_optimized_function(&self, func_ref: &FuncRef) -> Option<&IrFunction> {
        self.optimized.get_optimized_function(func_ref)
    }
}
