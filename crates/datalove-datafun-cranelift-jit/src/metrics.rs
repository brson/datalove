//! Metrics collection for optimization infrastructure.
//!
//! Tracks per-function and aggregate execution statistics.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use datalove_datafun_ir::CodeRef;

/// Execution mode for a function call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionMode {
    /// Executed via interpreter.
    Interpreted,
    /// Executed via JIT-compiled code.
    Jit,
    /// Executed via JIT-compiled inlined code.
    InlinedJit,
}

/// Per-function execution metrics.
#[derive(Clone, Debug, Default)]
pub struct FunctionMetrics {
    /// Total number of calls.
    pub call_count: u64,
    /// Total execution time in nanoseconds.
    pub total_time_ns: u64,
    /// Execution mode for this function (last used mode).
    pub mode: Option<ExecutionMode>,
    /// Whether this function was inlined into callers.
    pub inlined: bool,
    /// Whether this function was JIT-compiled.
    pub jit_compiled: bool,
}

/// Aggregate metrics across all functions.
#[derive(Clone, Debug, Default)]
pub struct AggregateMetrics {
    /// Total calls executed.
    pub total_calls: u64,
    /// Calls executed via interpreter.
    pub interpreted_calls: u64,
    /// Calls executed via JIT.
    pub jit_calls: u64,
    /// Calls executed via inlined JIT.
    pub inlined_jit_calls: u64,
    /// Total execution time for all calls.
    pub total_execution_time: Duration,
    /// Number of JIT-compiled functions.
    pub jit_compiled_count: u32,
    /// Total JIT code size in bytes.
    pub total_jit_code_size: usize,
    /// Number of inlined call sites.
    pub inlined_sites_count: u32,
    /// Number of inlining attempts skipped.
    pub inlinings_skipped: u32,
}

/// Configuration for metrics collection.
#[derive(Clone, Debug)]
pub struct MetricsConfig {
    /// Whether per-call timing is enabled.
    pub timing_enabled: bool,
    /// Sample rate for timing (1 = every call, N = every Nth call).
    pub timing_sample_rate: u32,
    /// Whether per-function metrics are collected.
    pub per_function_enabled: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            timing_enabled: true,
            timing_sample_rate: 1,
            per_function_enabled: true,
        }
    }
}

/// Metrics collector for optimization infrastructure.
///
/// Tracks per-function and aggregate execution statistics.
pub struct MetricsCollector {
    /// Per-function metrics.
    per_function: HashMap<CodeRef, FunctionMetrics>,
    /// Aggregate metrics.
    aggregate: AggregateMetrics,
    /// Configuration.
    config: MetricsConfig,
    /// Call counter for sampling.
    call_counter: u64,
}

impl MetricsCollector {
    /// Create a new metrics collector with default configuration.
    pub fn new() -> Self {
        Self::with_config(MetricsConfig::default())
    }

    /// Create a new metrics collector with custom configuration.
    pub fn with_config(config: MetricsConfig) -> Self {
        Self {
            per_function: HashMap::new(),
            aggregate: AggregateMetrics::default(),
            config,
            call_counter: 0,
        }
    }

    /// Check if timing should be recorded for this call (based on sample rate).
    fn should_time(&mut self) -> bool {
        if !self.config.timing_enabled {
            return false;
        }
        self.call_counter += 1;
        self.call_counter % self.config.timing_sample_rate as u64 == 0
    }

    /// Record a function call execution.
    ///
    /// Returns an optional start time for timing (if timing is enabled and sampled).
    pub fn start_call(&mut self) -> Option<Instant> {
        if self.should_time() {
            Some(Instant::now())
        } else {
            None
        }
    }

    /// Record completion of a function call.
    pub fn record_call(
        &mut self,
        code_ref: &CodeRef,
        mode: ExecutionMode,
        start_time: Option<Instant>,
    ) {
        // Update aggregate metrics.
        self.aggregate.total_calls += 1;
        match mode {
            ExecutionMode::Interpreted => self.aggregate.interpreted_calls += 1,
            ExecutionMode::Jit => self.aggregate.jit_calls += 1,
            ExecutionMode::InlinedJit => self.aggregate.inlined_jit_calls += 1,
        }

        // Calculate elapsed time if timing was enabled.
        let elapsed_ns = start_time
            .map(|start| start.elapsed().as_nanos() as u64)
            .unwrap_or(0);

        if let Some(start) = start_time {
            self.aggregate.total_execution_time += start.elapsed();
        }

        // Update per-function metrics if enabled.
        if self.config.per_function_enabled {
            let metrics = self.per_function.entry(code_ref.clone()).or_default();
            metrics.call_count += 1;
            metrics.total_time_ns += elapsed_ns;
            metrics.mode = Some(mode);
        }
    }

    /// Record that a function was JIT-compiled.
    pub fn record_jit_compile(&mut self, code_ref: &CodeRef, code_size: usize) {
        self.aggregate.jit_compiled_count += 1;
        self.aggregate.total_jit_code_size += code_size;

        if self.config.per_function_enabled {
            let metrics = self.per_function.entry(code_ref.clone()).or_default();
            metrics.jit_compiled = true;
        }
    }

    /// Record that a call site was inlined.
    pub fn record_inlining(&mut self, caller_ref: &CodeRef) {
        self.aggregate.inlined_sites_count += 1;

        if self.config.per_function_enabled {
            let metrics = self.per_function.entry(caller_ref.clone()).or_default();
            metrics.inlined = true;
        }
    }

    /// Record that an inlining attempt was skipped.
    pub fn record_inlining_skipped(&mut self) {
        self.aggregate.inlinings_skipped += 1;
    }

    /// Get aggregate metrics.
    pub fn aggregate(&self) -> &AggregateMetrics {
        &self.aggregate
    }

    /// Get per-function metrics for a specific function.
    pub fn function_metrics(&self, code_ref: &CodeRef) -> Option<&FunctionMetrics> {
        self.per_function.get(code_ref)
    }

    /// Get all per-function metrics.
    pub fn all_function_metrics(&self) -> &HashMap<CodeRef, FunctionMetrics> {
        &self.per_function
    }

    /// Get a summary of metrics.
    pub fn summary(&self) -> MetricsSummary {
        let avg_call_time_ns = if self.aggregate.total_calls > 0 {
            self.aggregate.total_execution_time.as_nanos() as u64 / self.aggregate.total_calls
        } else {
            0
        };

        let jit_ratio = if self.aggregate.total_calls > 0 {
            (self.aggregate.jit_calls + self.aggregate.inlined_jit_calls) as f64
                / self.aggregate.total_calls as f64
        } else {
            0.0
        };

        MetricsSummary {
            total_calls: self.aggregate.total_calls,
            jit_compiled_functions: self.aggregate.jit_compiled_count,
            total_code_size: self.aggregate.total_jit_code_size,
            inlined_sites: self.aggregate.inlined_sites_count,
            avg_call_time_ns,
            jit_execution_ratio: jit_ratio,
        }
    }

    /// Reset all metrics.
    pub fn reset(&mut self) {
        self.per_function.clear();
        self.aggregate = AggregateMetrics::default();
        self.call_counter = 0;
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of collected metrics.
#[derive(Clone, Debug)]
pub struct MetricsSummary {
    /// Total number of function calls.
    pub total_calls: u64,
    /// Number of JIT-compiled functions.
    pub jit_compiled_functions: u32,
    /// Total generated code size in bytes.
    pub total_code_size: usize,
    /// Number of inlined call sites.
    pub inlined_sites: u32,
    /// Average call execution time in nanoseconds.
    pub avg_call_time_ns: u64,
    /// Ratio of calls executed via JIT (0.0 to 1.0).
    pub jit_execution_ratio: f64,
}

impl std::fmt::Display for MetricsSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Optimization Metrics Summary:")?;
        writeln!(f, "  Total calls: {}", self.total_calls)?;
        writeln!(f, "  JIT-compiled functions: {}", self.jit_compiled_functions)?;
        writeln!(f, "  Total code size: {} bytes", self.total_code_size)?;
        writeln!(f, "  Inlined sites: {}", self.inlined_sites)?;
        writeln!(f, "  Avg call time: {} ns", self.avg_call_time_ns)?;
        writeln!(f, "  JIT execution ratio: {:.1}%", self.jit_execution_ratio * 100.0)?;
        Ok(())
    }
}
