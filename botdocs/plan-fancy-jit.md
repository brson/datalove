# JIT + Inliner Integration with Metrics

**Goal**: Production-ready optimization infrastructure with accurate timing.

## Current State

**JitEngine** (`cranelift-jit/src/lib.rs`):
- Tracks per-function call counts, compiles at threshold
- No timing, no code size, no stats beyond call counts
- Implements `CallDispatcher`

**DynamicInliner** (`interp/src/dynamic.rs`):
- Tracks per-call-site counts, inlines at threshold
- Has `InlinerStats { calls_tracked, inlinings_performed, inlinings_skipped }`
- Implements `CallDispatcher`

**Problem**: Both implement `CallDispatcher` independently - no composition, no timing, can't measure if optimizations help.

## Proposed Architecture

### Core Insight
JIT and inlining are complementary: **inline hot call sites** -> **JIT compile the optimized IR**.

### Combined Dispatcher
```rust
pub struct OptimizingDispatcher {
    inliner: DynamicInliner,
    jit: JitEngine,
    metrics: Option<MetricsCollector>,
    config: OptimizingConfig,
}
```

Dispatch flow:
1. Record call site for inlining decisions
2. Get best IR (inlined version if available)
3. Check if JIT-compiled; if so, execute native code
4. If not compiled, record call count, maybe trigger compilation
5. Fall back to interpreter if needed
6. Record timing if metrics enabled

### Metrics to Track

**Aggregate** (always on, low overhead):
- `jit_compiled_count`, `total_jit_code_size`
- `inlined_sites_count`, `inlinings_skipped`
- Execution mode counts (interpreted vs JIT)

**Timing** (optional, configurable):
- Per-function total time
- Sampled timing (every Nth call) to reduce overhead
- JIT compilation time

**Per-function** (optional):
- Call count, execution time, code size
- Whether inlined, whether JIT-compiled

### A/B Testing
```rust
pub struct ABTestDispatcher {
    optimized: OptimizingDispatcher,
    baseline: NoopDispatcher,  // pure interpreter
    mode: ABMode,  // Both, Optimized, Baseline, Alternating
}
```
- Run same code with/without optimizations
- Compare total time, verify same results
- Statistical analysis of speedup

## Implementation Plan

### Phase 1: Add JitStats to JitEngine
**Files**: `cranelift-jit/src/lib.rs`, `cranelift-jit/src/compiler.rs`

- Add `JitStats` struct with compile_time, code_size, compiled_count
- Track compilation time in `compile_function_with_context`
- Add `stats()` method to JitEngine
- Expose code size (estimate from function IR size if Cranelift doesn't expose)

### Phase 2: Create MetricsCollector
**Files**: New `interp/src/metrics.rs`

```rust
pub struct MetricsCollector {
    per_function: HashMap<FuncRef, FunctionMetrics>,
    aggregate: AggregateMetrics,
    timing_enabled: bool,
}

pub struct FunctionMetrics {
    pub call_count: u64,
    pub total_time_ns: u64,
    pub mode: ExecutionMode,  // Interpreted, Jit, InlinedJit
}
```

- Per-call timing (accept ~20-50ns overhead for accuracy)
- Aggregate stats updated on each call
- Methods: `record_call()`, `record_jit_compile()`, `summary()`

### Phase 3: Create OptimizingDispatcher
**Files**: New `cranelift-jit/src/optimizing.rs` (in JIT crate since it uses JIT)

```rust
pub struct OptimizingDispatcher {
    inliner: DynamicInliner,
    jit: JitEngine,
    metrics: MetricsCollector,
}
```

Dispatch logic:
1. Check inliner for optimized IR
2. Check JIT for compiled code
3. Execute via JIT if available, else record call for future compilation
4. Fall back to interpreter, record timing
5. Update metrics

### Phase 4: A/B Testing Infrastructure
**Files**: New `cranelift-jit/src/ab_test.rs`

- `ABTestDispatcher` wrapping optimized + baseline
- Modes: Both, Optimized, Baseline, Alternating
- Result comparison (hash outputs for correctness)
- Statistical speedup analysis

### Phase 5: Integration & Tests
**Files**: Tests in `datalove-datafun/tests/`

- `interp_optimizing_tests.rs` - correctness with OptimizingDispatcher
- `interp_ab_tests.rs` - A/B comparison on fixtures
- Benchmark integration for perf measurement

## Key Files

| File | Purpose |
|------|---------|
| `cranelift-jit/src/lib.rs` | JitEngine - add JitStats |
| `cranelift-jit/src/compiler.rs` | Track compile time |
| `interp/src/metrics.rs` | NEW: MetricsCollector |
| `cranelift-jit/src/optimizing.rs` | NEW: OptimizingDispatcher |
| `cranelift-jit/src/ab_test.rs` | NEW: ABTestDispatcher |

## Verification

1. Run existing chaos tests with OptimizingDispatcher
2. Verify metrics show expected call counts
3. A/B test on fixtures: optimized should match baseline results
4. Benchmark: measure actual speedup on representative workloads
