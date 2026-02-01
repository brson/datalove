//! Chaos dispatcher for testing JIT edge cases.
//!
//! Randomly decides whether to JIT or interpret each function call,
//! exposing issues in mixed-mode execution paths.

use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use datalove_datafun_ir::{FuncRef, IrFunction, IrCodeUnit};
use datalove_datafun_interp::{
    CallDispatcher, DispatchCallContext, Destination, DispatchResult, InterpError, Value,
    ExecutionContext, FrameStore, FunctionRegistry, IrInterpreter,
};
use datalove_rt::c::LocalRtHandle;

use crate::{bridge, FunctionKey, FunctionState, JitEngine};
use crate::{DispatchContext, set_dispatch_context, clear_dispatch_context};

/// A chaos dispatcher that randomly decides JIT vs interpret.
///
/// Used for testing to expose edge cases in mixed-mode execution.
pub struct ChaosDispatcher {
    /// Underlying JIT engine.
    jit: JitEngine,
    /// Random state derived from seed.
    rng_state: u64,
    /// Counter for generating different random values per call.
    call_counter: u64,
    /// Probability (0-100) of forcing JIT compilation.
    compile_probability: u32,
    /// Probability (0-100) of using JIT when available.
    use_jit_probability: u32,
}

impl ChaosDispatcher {
    /// Create a new chaos dispatcher with the given seed and probabilities.
    ///
    /// The seed should be derived from test input for reproducibility.
    /// Probabilities are clamped to 0-100.
    pub fn new(seed: u64, compile_probability: u32, use_jit_probability: u32) -> Result<Self, crate::JitError> {
        Ok(Self {
            jit: JitEngine::new(1)?, // Threshold 1: compile on first attempt
            rng_state: seed,
            call_counter: 0,
            compile_probability: compile_probability.min(100),
            use_jit_probability: use_jit_probability.min(100),
        })
    }

    /// Create from a hashable value (e.g., file path or contents).
    pub fn from_hashable<H: Hash>(value: &H, compile_probability: u32, use_jit_probability: u32) -> Result<Self, crate::JitError> {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        Self::new(hasher.finish(), compile_probability, use_jit_probability)
    }

    /// Generate a pseudo-random number in [0, 100).
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

    /// Decide whether to try compilation for this call.
    fn should_compile(&mut self) -> bool {
        self.random_percent() < self.compile_probability
    }

    /// Decide whether to use JIT code (when available).
    fn should_use_jit(&mut self) -> bool {
        self.random_percent() < self.use_jit_probability
    }

    /// Get mutable reference to the underlying JIT engine.
    pub fn jit_engine_mut(&mut self) -> &mut JitEngine {
        &mut self.jit
    }

    /// Execute a function with proper dispatch context setup.
    ///
    /// This is the key function that sets up DispatchContext for mixed-mode.
    pub fn execute_with_context<'a>(
        &mut self,
        func: &IrCodeUnit,
        args: Vec<Value>,
        ret_dest: Destination,
        interp: &mut IrInterpreter,
        ctx: &ExecutionContext<'a>,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        let func_ref = FuncRef::Local(datalove_datafun_ir::FuncId(func.id.0));
        let key = FunctionKey::from(&func_ref);
        let rt_handle = interp.runtime_handle();

        // Randomly decide whether to try JIT.
        let try_jit = self.should_compile();

        let func_ctx = func.function_context()
            .expect("execute_with_context requires a function code unit");

        if try_jit {
            // Try to compile with context (creates stubs for callees).
            match self.jit.record_call_with_context(key, func, ctx, registry) {
                Ok(Some((code_ptr, uses_sret))) => {
                    // Compiled! Now randomly decide whether to actually use it.
                    if self.should_use_jit() {
                        // Set up dispatch context for trampoline.
                        let mut dispatch_ctx = DispatchContext {
                            jit_engine: &mut self.jit,
                            interp,
                            exec_ctx: ctx,
                            registry,
                            frames,
                        };

                        // SAFETY: context is valid for duration of call.
                        unsafe { set_dispatch_context(&mut dispatch_ctx) };

                        // Call JIT code.
                        let result = unsafe {
                            bridge::call_jit(
                                code_ptr,
                                uses_sret,
                                rt_handle,
                                &[],
                                ret_dest,
                                &func_ctx.return_type,
                            )
                        };

                        // Clear dispatch context.
                        clear_dispatch_context();

                        return result.map_err(|e| InterpError::RuntimeError(e.to_string()));
                    }
                    // Fall through to interpreter.
                }
                Ok(None) => {
                    // Not compiled yet (shouldn't happen with threshold=1).
                }
                Err(e) => {
                    // Compilation failed, fall through to interpreter.
                    eprintln!("Chaos: compilation failed: {}", e);
                }
            }
        }

        // Use interpreter.
        let func_unit = IrCodeUnit::from(func.clone());
        interp.call_in_context(&func_unit, None, args, ret_dest, ctx, registry, frames)
    }
}

impl CallDispatcher for ChaosDispatcher {
    fn dispatch_call(
        &mut self,
        func_ref: &FuncRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        use datalove_datafun_interp::ExecutionContext;

        let key = FunctionKey::from(func_ref);

        // Randomly decide whether to try JIT.
        if !self.should_compile() {
            return DispatchResult::NotHandled;
        }

        // For external functions, use the callee's unit's context to find local functions.
        // _callee_ctx_owned keeps the context alive for the duration of this function.
        let _callee_ctx_owned: Option<ExecutionContext>;
        let compile_ctx = match func_ref {
            FuncRef::External { unit, .. } => {
                match call_ctx.registry.unit_functions(*unit) {
                    Some(unit_funcs) => {
                        _callee_ctx_owned = Some(ExecutionContext::new(unit_funcs));
                        _callee_ctx_owned.as_ref().unwrap()
                    }
                    None => {
                        return DispatchResult::NotHandled;
                    }
                }
            }
            _ => {
                _callee_ctx_owned = None;
                call_ctx.exec_ctx
            }
        };

        // Use record_call_with_context to enable JIT for functions with calls.
        match self.jit.record_call_with_context(key, func, compile_ctx, call_ctx.registry) {
            Ok(Some((code_ptr, uses_sret))) => {
                // Compiled! Randomly decide whether to use it.
                if self.should_use_jit() {
                    // Set up dispatch context for mixed-mode execution.
                    let mut dispatch_ctx = DispatchContext {
                        jit_engine: &mut self.jit,
                        interp: call_ctx.interp,
                        exec_ctx: compile_ctx,
                        registry: call_ctx.registry,
                        frames: call_ctx.frames,
                    };

                    // SAFETY: context is valid for duration of call.
                    unsafe { set_dispatch_context(&mut dispatch_ctx) };

                    let result = unsafe {
                        bridge::call_jit(
                            code_ptr,
                            uses_sret,
                            rt_handle,
                            args,
                            ret_dest,
                            func.return_type().expect("JIT dispatch requires function return type"),
                        )
                    };

                    // Clear dispatch context.
                    clear_dispatch_context();

                    match result {
                        Ok(()) => DispatchResult::Handled(Ok(())),
                        Err(e) => DispatchResult::Handled(Err(InterpError::RuntimeError(e.to_string()))),
                    }
                } else {
                    DispatchResult::NotHandled
                }
            }
            Ok(None) => DispatchResult::NotHandled,
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("unsupported:")
                    || error_str.contains("not yet declared")
                    || error_str.contains("Duplicate definition")
                {
                    // Mark as non-JITable.
                    self.jit.states.insert(key, FunctionState::Interpreted { call_count: u32::MAX });
                    DispatchResult::NotHandled
                } else {
                    DispatchResult::Handled(Err(InterpError::RuntimeError(error_str)))
                }
            }
        }
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
    fn test_chaos_dispatcher_creation() {
        let chaos = ChaosDispatcher::new(12345, 50, 50);
        assert!(chaos.is_ok());
    }

    #[test]
    fn test_chaos_from_hashable() {
        let chaos1 = ChaosDispatcher::from_hashable(&"test_input_1", 50, 50).unwrap();
        let chaos2 = ChaosDispatcher::from_hashable(&"test_input_1", 50, 50).unwrap();
        let chaos3 = ChaosDispatcher::from_hashable(&"test_input_2", 50, 50).unwrap();

        // Same input should produce same initial state.
        assert_eq!(chaos1.rng_state, chaos2.rng_state);
        // Different input should produce different state.
        assert_ne!(chaos1.rng_state, chaos3.rng_state);
    }

    #[test]
    fn test_random_distribution() {
        let mut chaos = ChaosDispatcher::new(42, 50, 50).unwrap();

        let mut compile_count = 0;
        for _ in 0..1000 {
            if chaos.should_compile() {
                compile_count += 1;
            }
        }

        // Should be roughly 50% (allow wide margin for randomness).
        assert!(compile_count > 300 && compile_count < 700,
            "Expected ~500 compiles, got {}", compile_count);
    }
}
