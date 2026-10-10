//! Per-function tracing JIT for the datalove interpreter.
//!
//! Tracks function call counts and compiles hot functions to native code using
//! Cranelift. Integrates with the interpreter for mixed-mode execution.
//!
//! # Usage
//!
//! ```ignore
//! use datalove_datafun_cranelift_jit::JitEngine;
//! use datalove_datafun_interp::IrInterpreter;
//! use datalove_rt::c::DebugOutputMode;
//!
//! let jit = JitEngine::new(100)?; // Compile after 100 calls
//! let mut interp = IrInterpreter::new_with_options(DebugOutputMode::Disabled, Some(Box::new(jit)));
//! ```

mod compiler;
pub(crate) mod bridge;
pub(crate) mod trampoline;
pub mod optimizing;
pub mod stats;

pub use trampoline::{DispatchContext, set_dispatch_context, clear_dispatch_context};
pub use optimizing::{OptimizingDispatcher, DispatcherConfig, DispatcherMode};
pub use stats::{CallFrom, FunctionStats, JitStats};

use std::any::Any;

use rustc_hash::FxHashMap;
use std::time::Instant;

use datalove_datafun_ir::{CodeRef, IrCodeUnit};
use datalove_datafun_interp::{
    CallDispatcher, DispatchCallContext, Destination, DispatchResult, ExecutionContext, FuncIdentity,
    FunctionRegistry, InterpError, IrInterpreter, Value,
};
use datalove_rt::c::LocalRtHandle;

use compiler::JitCompiler;

/// Error type for JIT operations.
#[derive(Debug)]
pub enum JitError {
    /// Cranelift compilation failed.
    CompilationFailed(String),
    /// The backend does not implement something this function uses.
    ///
    /// Distinct from a failure because it is not one: the function is left to
    /// the interpreter and the program runs. Kept as a variant rather than
    /// recognised from the message, which is what the dispatchers used to do.
    Unsupported(String),
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitError::CompilationFailed(msg) => write!(f, "JIT compilation failed: {}", msg),
            JitError::Unsupported(msg) => write!(f, "JIT cannot compile this: {}", msg),
        }
    }
}

impl std::error::Error for JitError {}

/// Tracks function execution state for JIT compilation decisions.
pub enum FunctionState {
    /// Function is interpreted; tracks call count for compilation trigger.
    Interpreted { call_count: u32 },
    /// Function has been compiled to native code.
    Compiled {
        /// Pointer to native code entry point.
        code_ptr: *const u8,
        /// Whether return uses sret convention.
        uses_sret: bool,
        /// Estimated code size in bytes.
        code_size: usize,
    },
}

/// Per-function tracing JIT engine.
///
/// Tracks function call counts and compiles hot functions to native code.
/// Single-threaded design - no synchronization overhead.
pub struct JitEngine {
    /// Function states (interpreted with call count, or compiled).
    ///
    /// `FxHashMap` because this is probed on every call that reaches the
    /// dispatcher and again on every call out of compiled code.
    pub(crate) states: FxHashMap<FuncIdentity, FunctionState>,
    /// Cranelift JIT compiler.
    compiler: JitCompiler,
    /// Call count threshold for triggering compilation.
    threshold: u32,
    /// Compilation statistics.
    stats: JitStats,
    /// Whether to count each call offered to the jit in `stats`.
    count_calls: bool,
}

impl JitEngine {
    /// Create a new JIT engine with the specified compilation threshold.
    pub fn new(threshold: u32) -> Result<Self, JitError> {
        Ok(Self {
            states: FxHashMap::default(),
            compiler: JitCompiler::new()?,
            threshold,
            stats: JitStats::default(),
            count_calls: false,
        })
    }

    /// Count every call offered to the jit, by function, in `stats`.
    ///
    /// Off by default: it is a second table probe on every call.
    pub fn count_calls(&mut self) {
        self.count_calls = true;
    }

    /// Count a call in `stats` if calls are being counted.
    #[inline]
    pub(crate) fn note_call(&mut self, key: FuncIdentity, func: &IrCodeUnit, from: CallFrom, native: bool) {
        if self.count_calls {
            self.stats.count_call(key, &func.name, from, native);
        }
    }

    /// Register a native rider function symbol for JIT resolution.
    ///
    /// Call this after loading rider libraries and before executing code
    /// that calls native rider functions.
    pub fn register_native_symbol(&self, name: &str, addr: *const u8) {
        self.compiler.register_native_symbol(name, addr);
    }

    /// Hold what a registered native symbol's code lives in.
    ///
    /// Needed because the address is emitted into compiled code, which outlives
    /// the symbol table it was looked up in. See `JitCompiler::code_owners`.
    pub fn hold_code_owner(&self, owner: std::sync::Arc<dyn std::any::Any + Send + Sync>) {
        self.compiler.hold_code_owner(owner);
    }

    /// Get compilation statistics.
    pub fn stats(&self) -> &JitStats {
        &self.stats
    }

    /// Get the compilation threshold.
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Count a call to `func`, compiling it once it is hot.
    ///
    /// `ctx` is the context `func` runs in, which is where the callees its
    /// stubs name are looked up. A function the backend declines, or one too
    /// wide to be entered from the interpreter, is marked so that it is not
    /// asked about again. An error is a compilation that went wrong, not one
    /// that was declined.
    pub fn record_call(
        &mut self,
        key: FuncIdentity,
        func: &IrCodeUnit,
        ctx: &ExecutionContext<'_>,
        registry: &FunctionRegistry,
        interp: &mut IrInterpreter,
    ) -> Result<Recorded, JitError> {
        // A function too wide to be entered is one there is no point compiling.
        // Asked here rather than at the call, so that every way in agrees and
        // agrees before the work is done: finding out at the call meant a
        // program that ran under the interpreter failed under the jit.
        if !bridge::enterable(func) {
            self.states.insert(key, FunctionState::Interpreted { call_count: u32::MAX });
            return Ok(Recorded::Interpret);
        }

        let state = self.states.entry(key).or_insert(FunctionState::Interpreted { call_count: 0 });
        let call_count = match state {
            FunctionState::Compiled { code_ptr, uses_sret, .. } => {
                return Ok(Recorded::Compiled {
                    code_ptr: *code_ptr,
                    uses_sret: *uses_sret,
                    compiled_now: None,
                });
            }
            FunctionState::Interpreted { call_count } => call_count,
        };
        // u32::MAX means permanently interpreted.
        *call_count = call_count.saturating_add(1);
        if *call_count < self.threshold || *call_count == u32::MAX {
            return Ok(Recorded::Interpret);
        }

        let start = Instant::now();
        match self.compiler.compile_function(func, ctx, registry, interp) {
            Ok((code_ptr, uses_sret, code_size)) => {
                let compile_time = start.elapsed();
                self.stats.compiled_count += 1;
                self.stats.total_compile_time += compile_time;
                self.stats.total_code_size += code_size;
                let f = self.stats.function(key, &func.name);
                f.compile_time = Some(compile_time);
                f.code_size = code_size;

                *state = FunctionState::Compiled { code_ptr, uses_sret, code_size };
                self.compiler.publish(key, code_ptr);
                Ok(Recorded::Compiled { code_ptr, uses_sret, compiled_now: Some(code_size) })
            }
            Err(JitError::Unsupported(_)) => {
                // Not a failure: the function is left to the interpreter, and
                // asking again at every call would recompile it every time.
                *state = FunctionState::Interpreted { call_count: u32::MAX };
                self.stats.refused_count += 1;
                Ok(Recorded::Interpret)
            }
            Err(e) => Err(e),
        }
    }

    /// Offer a call the interpreter is making to compiled code.
    ///
    /// Counts the call, compiles the function if it is now hot, and runs it if
    /// it is compiled and `use_compiled` agrees. Every dispatcher goes through
    /// here, so they agree on the context a callee is compiled and run in and
    /// on what a refusal means; they differ only in whether they ask and
    /// whether they then use what they get.
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_with(
        &mut self,
        code_ref: &CodeRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
        use_compiled: impl FnOnce() -> bool,
    ) -> JitDispatch {
        let key = FuncIdentity::of(code_ref, call_ctx.exec_ctx.unit());
        let callee_ctx = call_ctx.exec_ctx.for_callee(code_ref, call_ctx.registry);
        match self.record_call(key, func, &callee_ctx, call_ctx.registry, call_ctx.interp) {
            Err(e) => JitDispatch {
                result: DispatchResult::Handled(Err(InterpError::RuntimeError(e.to_string()))),
            },
            Ok(Recorded::Interpret) => {
                self.note_call(key, func, CallFrom::Interpreter, false);
                JitDispatch { result: DispatchResult::NotHandled }
            }
            Ok(Recorded::Compiled { code_ptr, uses_sret, .. }) => {
                if !use_compiled() {
                    self.note_call(key, func, CallFrom::Interpreter, false);
                    return JitDispatch { result: DispatchResult::NotHandled };
                }
                self.note_call(key, func, CallFrom::Interpreter, true);
                let descriptor_params = &func.function_context()
                    .expect("a compiled function is a function")
                    .descriptor_params;
                let shape_descriptors = call_ctx.shape_descriptors;
                // What the code's stubs call back into the interpreter with,
                // for the length of the call.
                let mut dispatch_ctx = DispatchContext {
                    jit_engine: self,
                    interp: call_ctx.interp,
                    exec_ctx: &callee_ctx,
                    registry: call_ctx.registry,
                    frames: call_ctx.frames,
                };
                // SAFETY: the context outlives the call, and is cleared after it.
                unsafe { set_dispatch_context(&mut dispatch_ctx) };
                // SAFETY: code_ptr is compiled code for `func`, whose arguments
                // these are.
                unsafe {
                    bridge::call_jit(
                        code_ptr, uses_sret, rt_handle, args, ret_dest,
                        descriptor_params, shape_descriptors,
                    )
                };
                clear_dispatch_context();
                JitDispatch { result: DispatchResult::Handled(Ok(())) }
            }
        }
    }
}

/// What recording a call found out about the function.
pub enum Recorded {
    /// Run it in the interpreter: it has not been called often enough yet, or
    /// it cannot be compiled.
    Interpret,
    /// Run its compiled code.
    Compiled {
        code_ptr: *const u8,
        uses_sret: bool,
        /// The size of the code, when this call is the one that compiled it.
        compiled_now: Option<usize>,
    },
}

/// What `JitEngine::dispatch_with` did with a call.
pub struct JitDispatch {
    /// What to tell the interpreter.
    pub result: DispatchResult,
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datafun_ir::{
        IrBlock, Instruction, Terminator, Operand,
        ValueId, BlockId, ConstValue, IrType, IrCodeUnit,
        CodeUnitId, CodeUnitContext, FunctionContext, SymbolTable,
    };
    use datalove_datafun_interp::{
        ExecutionContext, FrameStore, FunctionRegistry, IrInterpreter,
    };

    /// Helper to create a function code unit for tests.
    fn make_func_unit(
        id: u32,
        name: &str,
        params: Vec<datalove_datafun_ir::ParamId>,
        param_types: Vec<IrType>,
        return_type: IrType,
        blocks: Vec<IrBlock>,
        value_types: Vec<IrType>,
    ) -> IrCodeUnit {
        IrCodeUnit {
            id: CodeUnitId(id),
            name: name.to_string(),
            blocks,
            value_count: value_types.len() as u32,
            slot_count: 0,
            value_types,
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![],
            symbols: SymbolTable::default(),
            context: CodeUnitContext::Function(FunctionContext {
            descriptor_params: Vec::new(),
                params,
                param_modes: vec![],
                param_types,
                return_type,
                tracked_params: vec![],
            descriptor_shapes: Vec::new(),
            }),
            nested_units: vec![],
        }
    }

    fn make_test_function() -> IrCodeUnit {
        // fn test() -> i32 { 42 }
        make_func_unit(
            0,
            "test",
            vec![],
            vec![],
            IrType::I32,
            vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(42) },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(0))),
                },
            }],
            vec![IrType::I32],
        )
    }

    /// Record one call to `func`, a function with nothing to call, as unit 0's
    /// function 0.
    fn record(jit: &mut JitEngine, func: &IrCodeUnit) -> Recorded {
        let ctx = ExecutionContext::new(0, std::slice::from_ref(func));
        let registry = FunctionRegistry::new();
        let key = FuncIdentity::Unit { unit: 0, id: CodeUnitId(0) };
        let mut interp = IrInterpreter::new();
        jit.record_call(key, func, &ctx, &registry, &mut interp).expect("compilation failed")
    }

    #[test]
    fn test_jit_engine_creation() {
        let jit = JitEngine::new(100);
        assert!(jit.is_ok());
    }

    #[test]
    fn test_call_counting() {
        let mut jit = JitEngine::new(3).unwrap();
        let func = make_test_function();

        // First two calls should not trigger compilation.
        assert!(matches!(record(&mut jit, &func), Recorded::Interpret));
        assert!(matches!(record(&mut jit, &func), Recorded::Interpret));

        // Third call should trigger compilation.
        match record(&mut jit, &func) {
            Recorded::Compiled { code_ptr, compiled_now, .. } => {
                assert!(!code_ptr.is_null(), "compiled code pointer should not be null");
                assert!(compiled_now.is_some(), "this call compiled it");
            }
            Recorded::Interpret => panic!("expected compilation at threshold"),
        }
        // And the fourth finds it compiled.
        assert!(matches!(record(&mut jit, &func), Recorded::Compiled { compiled_now: None, .. }));
    }

    /// A function borrowing a static const compiles, against the address the
    /// interpreter's pool built it at, rather than being left to the
    /// interpreter.
    #[test]
    fn test_static_const_compiles_against_the_pool() {
        let value = std::sync::Arc::new(ConstValue::List(vec![
            ConstValue::String("a".into()), ConstValue::String("b".into()),
        ]));
        let list = IrType::List(Box::new(IrType::String));
        let func = make_func_unit(
            0, "borrows", vec![], vec![], list.clone(),
            vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::StaticRef { dest: ValueId(0), value: datalove_datafun_ir::SharedConst(value.clone()) },
                    Instruction::Clone { dest: ValueId(1), src: Operand::ValueRef(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            }],
            vec![IrType::Ref(Box::new(list.clone())), list.clone()],
        );

        let mut jit = JitEngine::new(1).unwrap();
        let mut interp = IrInterpreter::new();
        let ctx = ExecutionContext::new(0, std::slice::from_ref(&func));
        let registry = FunctionRegistry::new();
        let key = FuncIdentity::Unit { unit: 0, id: CodeUnitId(0) };
        let recorded = jit.record_call(key, &func, &ctx, &registry, &mut interp)
            .expect("compilation failed");
        assert!(matches!(recorded, Recorded::Compiled { .. }), "refused rather than compiled");

        // Compiling it built the value, so asking again finds the same one.
        let first = interp.static_const(&value, &list);
        let again = interp.static_const(&std::sync::Arc::new((*value).clone()), &list);
        assert_eq!(first, again, "equal values share one entry");
    }

    #[test]
    fn test_compilation_produces_code() {
        let mut jit = JitEngine::new(1).unwrap(); // Compile immediately
        let func = make_test_function();

        match record(&mut jit, &func) {
            Recorded::Compiled { code_ptr, uses_sret, .. } => {
                assert!(!code_ptr.is_null());
                assert!(uses_sret, "all non-Unit returns use sret");
            }
            Recorded::Interpret => panic!("expected immediate compilation"),
        }
    }

    #[test]
    fn test_execute_jit_code() {
        // Create runtime FIRST, like the integration test does.
        let runtime = datalove_rt::rust::Runtime::new();

        let mut jit = JitEngine::new(1).unwrap();
        let func = make_test_function();

        // Compile the function.
        let Recorded::Compiled { code_ptr, uses_sret, .. } = record(&mut jit, &func) else {
            panic!("expected immediate compilation");
        };
        assert!(uses_sret, "all non-Unit returns use sret");

        // Runtime is already created.
        let rt_handle = runtime.handle();

        // Allocate space for return value.
        let mut result_buf: usize = 0;
        let ret_dest = Destination {
            ptr: &mut result_buf as *mut usize as *mut u8,
            tydesc: std::ptr::null(),
        };

        // Call the JIT code. Result written to ret_dest via sret.
        unsafe {
            bridge::call_jit(code_ptr, uses_sret, rt_handle, &[], ret_dest, &[], &[]);
        }

        // Extract i32 from the buffer.
        let result = result_buf as i32;
        assert_eq!(result, 42, "test function should return 42");
    }

    /// Test full integration: interpreter -> dispatcher -> JIT.
    #[test]
    fn test_interpreter_jit_integration() {
        use datalove_datafun_interp::{
            IrInterpreter, ExecutionContext, FunctionRegistry, FrameStore,
        };
        use datalove_datafun_ir::ParamId;

        // Create callee: fn identity(a: i32) -> i32 { a }
        // Note: We use identity instead of arithmetic because fixed-width
        // integer arithmetic widens to Int; arithmetic on i32 uses BinOpChecked.
        let identity_fn = make_func_unit(
            0,
            "identity",
            vec![ParamId(0)],
            vec![IrType::I32],
            IrType::I32,
            vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            }],
            vec![],
        );

        // Create caller: fn main() -> i32 { identity(42) }
        let main_fn = make_func_unit(
            1,
            "main",
            vec![],
            vec![],
            IrType::I32,
            vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(42) },
                    Instruction::Call {
                        dest: ValueId(1),
                        func: CodeRef::Local(CodeUnitId(0)),
                        args: vec![
                            Operand::Value(ValueId(0)),
                        ],
                        type_args: Vec::new(),
                        shape_descriptors: Vec::new(),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            }],
            vec![IrType::I32, IrType::I32],
        );

        // Set up interpreter with JIT dispatcher (threshold=1: compile on first call).
        let jit = JitEngine::new(1).expect("JitEngine creation failed");
        let mut interp = IrInterpreter::new_with_options(
            datalove_rt::c::DebugOutputMode::Disabled,
            Some(Box::new(jit)),
        );

        // Set up execution context with both functions.
        let functions: Vec<IrCodeUnit> = vec![identity_fn, main_fn.clone()];
        let ctx = ExecutionContext::new(0, &functions);
        let registry = FunctionRegistry::new();
        let mut frames = FrameStore::new();

        // Prepare return destination.
        let mut result: usize = 0; // usize for alignment
        let ret_tydesc = interp.tydesc_table_mut().get_or_create(&IrType::I32);
        let ret_dest = Destination {
            ptr: &mut result as *mut usize as *mut u8,
            tydesc: ret_tydesc,
        };

        // Execute main, which calls identity(42).
        // The call to identity should go through the JIT dispatcher.
        interp.call_in_context(&main_fn, None, vec![], ret_dest, &ctx, &registry, &mut frames)
            .expect("execution failed");

        // Verify result.
        assert_eq!(result as i32, 42, "identity(42) should equal 42");
    }

    /// Test JIT code calling back to interpreter via trampoline.
    ///
    /// This test:
    /// 1. Compiles `main()` to JIT (which calls `identity()`)
    /// 2. `identity()` is NOT compiled, so the call goes through the trampoline
    /// 3. The trampoline dispatches to the interpreter
    /// 4. Result flows back through the trampoline to JIT code
    #[test]
    fn test_jit_calls_interpreter() {
        use datalove_datafun_ir::ParamId;

        // Create callee: fn identity(a: i32) -> i32 { a }
        // Note: We use identity instead of arithmetic because fixed-width
        // integer arithmetic widens to Int; arithmetic on i32 uses BinOpChecked.
        let identity_fn = make_func_unit(
            0,
            "identity",
            vec![ParamId(0)],
            vec![IrType::I32],
            IrType::I32,
            vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            }],
            vec![],
        );

        // Create caller: fn main() -> i32 { identity(42) }
        let main_fn = make_func_unit(
            1,
            "main",
            vec![],
            vec![],
            IrType::I32,
            vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(42) },
                    Instruction::Call {
                        dest: ValueId(1),
                        func: CodeRef::Local(CodeUnitId(0)),
                        args: vec![
                            Operand::Value(ValueId(0)),
                        ],
                        type_args: Vec::new(),
                        shape_descriptors: Vec::new(),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            }],
            vec![IrType::I32, IrType::I32],
        );

        // Set up context with both functions.
        let functions: Vec<IrCodeUnit> = vec![identity_fn, main_fn.clone()];
        let ctx = ExecutionContext::new(0, &functions);
        let registry = FunctionRegistry::new();

        // Create JIT engine.
        let mut jit = JitEngine::new(1).expect("JitEngine creation failed");

        // Compile main() with context (creates stub for identity()).
        let main_key = FuncIdentity::Unit { unit: 0, id: CodeUnitId(1) };
        let Recorded::Compiled { code_ptr, uses_sret, .. } = jit
            .record_call(main_key, &main_fn, &ctx, &registry, &mut IrInterpreter::new())
            .expect("compilation failed")
        else {
            panic!("should compile on first call");
        };

        assert!(uses_sret, "all non-Unit returns use sret");

        // Set up interpreter and dispatch context.
        let mut interp = IrInterpreter::new();
        let mut frames = FrameStore::new();

        // Create dispatch context for the trampoline.
        let mut dispatch_ctx = DispatchContext {
            jit_engine: &mut jit,
            interp: &mut interp,
            exec_ctx: &ctx,
            registry: &registry,
            frames: &mut frames,
        };

        // Set the dispatch context.
        // SAFETY: context is valid for the duration of the call.
        unsafe { set_dispatch_context(&mut dispatch_ctx) };

        // Prepare return destination.
        let mut result: usize = 0;
        let ret_tydesc = dispatch_ctx.interp.tydesc_table_mut().get_or_create(&IrType::I32);
        let ret_dest = Destination {
            ptr: &mut result as *mut usize as *mut u8,
            tydesc: ret_tydesc,
        };

        // Get runtime handle.
        let rt_handle = dispatch_ctx.interp.runtime_handle();

        // Call the JIT-compiled main().
        // This will:
        // 1. Execute JIT code for main()
        // 2. main() calls identity() via stub
        // 3. Stub calls __jit_dispatch_call
        // 4. Dispatcher sees identity() is not compiled
        // 5. Dispatcher calls interpreter to execute identity()
        // 6. Result flows back through the chain
        // SAFETY: code_ptr is valid JIT code.
        unsafe {
            bridge::call_jit(code_ptr, uses_sret, rt_handle, &[], ret_dest, &[], &[]);
        }

        // Clear dispatch context.
        clear_dispatch_context();

        // Verify result: identity(42) = 42.
        assert_eq!(result as i32, 42, "identity(42) should equal 42");
    }
}

impl CallDispatcher for JitEngine {
    fn dispatch_call(
        &mut self,
        code_ref: &CodeRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        self.dispatch_with(code_ref, func, args, ret_dest, rt_handle, call_ctx, || true).result
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
