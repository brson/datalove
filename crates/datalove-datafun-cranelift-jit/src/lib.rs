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
pub mod chaos;
pub mod optimizing;
pub mod metrics;

pub use trampoline::{DispatchContext, set_dispatch_context, clear_dispatch_context};
pub use chaos::ChaosDispatcher;
pub use optimizing::{OptimizingDispatcher, DispatcherConfig, DispatcherMode};
pub use metrics::{MetricsCollector, FunctionMetrics, AggregateMetrics, ExecutionMode};

use std::any::Any;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use datalove_datafun_ir::{FuncId, FuncRef, IrFunction, IrCodeUnit, IrModuleId, IrType};
use datalove_datafun_interp::{CallDispatcher, DispatchCallContext, Destination, DispatchResult, InterpError, Value};
use datalove_rt::c::LocalRtHandle;

use compiler::JitCompiler;

/// Error type for JIT operations.
#[derive(Debug)]
pub enum JitError {
    /// Cranelift compilation failed.
    CompilationFailed(String),
    /// Function not found.
    FunctionNotFound(FunctionKey),
    /// Bridge call failed.
    BridgeCallFailed(String),
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitError::CompilationFailed(msg) => write!(f, "JIT compilation failed: {}", msg),
            JitError::FunctionNotFound(key) => write!(f, "function not found: {:?}", key),
            JitError::BridgeCallFailed(msg) => write!(f, "JIT bridge call failed: {}", msg),
        }
    }
}

impl std::error::Error for JitError {}

/// Key for identifying functions in the dispatch table.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FunctionKey {
    /// Module ID (None for local or external unit functions).
    pub module_id: Option<IrModuleId>,
    /// Function ID within the module or unit.
    pub func_id: FuncId,
    /// Script unit number (for external unit functions).
    pub unit: Option<u32>,
}

impl FunctionKey {
    /// Create a key for a local function.
    pub fn local(func_id: FuncId) -> Self {
        Self {
            module_id: None,
            func_id,
            unit: None,
        }
    }

    /// Create a key for a module function.
    pub fn module(module_id: IrModuleId, func_id: FuncId) -> Self {
        Self {
            module_id: Some(module_id),
            func_id,
            unit: None,
        }
    }

    /// Create a key for an external unit function.
    pub fn external(unit: u32, func_id: FuncId) -> Self {
        Self {
            module_id: None,
            func_id,
            unit: Some(unit),
        }
    }
}

impl From<&FuncRef> for FunctionKey {
    fn from(func_ref: &FuncRef) -> Self {
        match func_ref {
            FuncRef::Local(func_id) => FunctionKey::local(*func_id),
            FuncRef::Module { module, func } => FunctionKey::module(*module, *func),
            FuncRef::External { unit, func } => FunctionKey::external(*unit, *func),
        }
    }
}

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

/// Statistics about JIT compilation activity.
#[derive(Clone, Debug, Default)]
pub struct JitStats {
    /// Total number of functions compiled.
    pub compiled_count: u32,
    /// Total compilation time across all functions.
    pub total_compile_time: Duration,
    /// Total generated code size in bytes.
    pub total_code_size: usize,
    /// Number of compilation failures (function fell back to interpreter).
    pub compilation_failures: u32,
    /// Per-function compilation times (function name -> duration).
    pub per_function_compile_time: HashMap<String, Duration>,
    /// Per-function code sizes (function name -> bytes).
    pub per_function_code_size: HashMap<String, usize>,
}

/// Per-function tracing JIT engine.
///
/// Tracks function call counts and compiles hot functions to native code.
/// Single-threaded design - no synchronization overhead.
pub struct JitEngine {
    /// Function states (interpreted with call count, or compiled).
    pub(crate) states: HashMap<FunctionKey, FunctionState>,
    /// Cranelift JIT compiler.
    compiler: JitCompiler,
    /// Call count threshold for triggering compilation.
    threshold: u32,
    /// Compilation statistics.
    stats: JitStats,
}

impl JitEngine {
    /// Create a new JIT engine with the specified compilation threshold.
    pub fn new(threshold: u32) -> Result<Self, JitError> {
        Ok(Self {
            states: HashMap::new(),
            compiler: JitCompiler::new()?,
            threshold,
            stats: JitStats::default(),
        })
    }

    /// Get compilation statistics.
    pub fn stats(&self) -> &JitStats {
        &self.stats
    }

    /// Get the compilation threshold.
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Record a function call and trigger compilation if threshold reached.
    ///
    /// This version compiles functions without call support. Use `record_call_with_context`
    /// for functions that call other functions.
    ///
    /// Returns the compiled code pointer if the function was just compiled
    /// or was already compiled.
    pub fn record_call(
        &mut self,
        key: FunctionKey,
        func: &IrCodeUnit,
    ) -> Result<Option<(*const u8, bool)>, JitError> {
        let state = self.states.entry(key).or_insert(FunctionState::Interpreted { call_count: 0 });

        match state {
            FunctionState::Interpreted { call_count } => {
                // Use saturating_add to avoid overflow. u32::MAX indicates permanently
                // interpreted (e.g., function uses unsupported features).
                *call_count = call_count.saturating_add(1);
                if *call_count >= self.threshold && *call_count != u32::MAX {
                    // Compile the function with timing.
                    let start = Instant::now();
                    let (code_ptr, uses_sret, code_size) = self.compiler.compile_function(func)?;
                    let compile_time = start.elapsed();

                    // Update stats.
                    self.stats.compiled_count += 1;
                    self.stats.total_compile_time += compile_time;
                    self.stats.total_code_size += code_size;
                    self.stats.per_function_compile_time.insert(func.name.clone(), compile_time);
                    self.stats.per_function_code_size.insert(func.name.clone(), code_size);

                    *state = FunctionState::Compiled { code_ptr, uses_sret, code_size };
                    Ok(Some((code_ptr, uses_sret)))
                } else {
                    Ok(None)
                }
            }
            FunctionState::Compiled { code_ptr, uses_sret, .. } => {
                Ok(Some((*code_ptr, *uses_sret)))
            }
        }
    }

    /// Record a function call with context for mixed-mode execution.
    ///
    /// This version creates stubs for all callees, enabling JIT code to call
    /// back to the interpreter for non-compiled functions.
    ///
    /// Returns the compiled code pointer if the function was just compiled
    /// or was already compiled.
    pub fn record_call_with_context<'a>(
        &mut self,
        key: FunctionKey,
        func: &IrCodeUnit,
        ctx: &datalove_datafun_interp::ExecutionContext<'a>,
        registry: &datalove_datafun_interp::FunctionRegistry,
    ) -> Result<Option<(*const u8, bool)>, JitError> {
        let state = self.states.entry(key).or_insert(FunctionState::Interpreted { call_count: 0 });

        match state {
            FunctionState::Interpreted { call_count } => {
                // Use saturating_add to avoid overflow. u32::MAX indicates permanently
                // interpreted (e.g., function uses unsupported features).
                *call_count = call_count.saturating_add(1);
                if *call_count >= self.threshold && *call_count != u32::MAX {
                    // Compile the function with context and timing.
                    let start = Instant::now();
                    let (code_ptr, uses_sret, code_size) = self.compiler.compile_function_with_context(func, ctx, registry)?;
                    let compile_time = start.elapsed();

                    // Update stats.
                    self.stats.compiled_count += 1;
                    self.stats.total_compile_time += compile_time;
                    self.stats.total_code_size += code_size;
                    self.stats.per_function_compile_time.insert(func.name.clone(), compile_time);
                    self.stats.per_function_code_size.insert(func.name.clone(), code_size);

                    *state = FunctionState::Compiled { code_ptr, uses_sret, code_size };
                    Ok(Some((code_ptr, uses_sret)))
                } else {
                    Ok(None)
                }
            }
            FunctionState::Compiled { code_ptr, uses_sret, .. } => {
                Ok(Some((*code_ptr, *uses_sret)))
            }
        }
    }

    /// Get compiled code for a function if available.
    pub fn get_compiled(&self, key: &FunctionKey) -> Option<(*const u8, bool)> {
        match self.states.get(key) {
            Some(FunctionState::Compiled { code_ptr, uses_sret, .. }) => {
                Some((*code_ptr, *uses_sret))
            }
            _ => None,
        }
    }

    /// Record a compilation failure for stats tracking.
    pub fn record_compilation_failure(&mut self) {
        self.stats.compilation_failures += 1;
    }

    /// Call a JIT-compiled function.
    ///
    /// Bridges between interpreter's Value/Destination and native calling convention.
    ///
    /// # Safety
    ///
    /// code_ptr must be a valid JIT-compiled function. args must match the function's signature.
    pub unsafe fn call_jit(
        &self,
        code_ptr: *const u8,
        uses_sret: bool,
        rt_handle: LocalRtHandle,
        args: &[Value],
        ret_dest: Destination,
        return_type: &IrType,
    ) -> Result<(), JitError> {
        // SAFETY: caller guarantees code_ptr and args are valid.
        unsafe { bridge::call_jit(code_ptr, uses_sret, rt_handle, args, ret_dest, return_type) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datafun_ir::{
        IrBlock, Instruction, Terminator, Operand,
        ValueId, BlockId, ConstValue, IrType, IrCodeUnit,
    };
    use datalove_datafun_interp::{
        ExecutionContext, FrameStore, FunctionRegistry, IrInterpreter,
    };

    fn make_test_function() -> IrCodeUnit {
        // fn test() -> i32 { 42 }
        IrCodeUnit::from(IrFunction {
            id: FuncId(0),
            name: "test".to_string(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            return_type: IrType::I32,
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(42) },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(0))),
                },
            }],
            value_count: 1,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            tracked_params: vec![],
            const_values: vec![],
        })
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
        let key = FunctionKey::local(FuncId(0));

        // First two calls should not trigger compilation.
        assert!(jit.record_call(key, &func).unwrap().is_none());
        assert!(jit.record_call(key, &func).unwrap().is_none());

        // Third call should trigger compilation.
        let result = jit.record_call(key, &func);
        match result {
            Ok(Some((ptr, _))) => {
                assert!(!ptr.is_null(), "compiled code pointer should not be null");
            }
            Ok(None) => panic!("expected compilation at threshold"),
            Err(e) => panic!("compilation failed: {}", e),
        }
    }

    #[test]
    fn test_compilation_produces_code() {
        let mut jit = JitEngine::new(1).unwrap(); // Compile immediately
        let func = make_test_function();
        let key = FunctionKey::local(FuncId(0));

        let result = jit.record_call(key, &func);
        match result {
            Ok(Some((ptr, uses_sret))) => {
                assert!(!ptr.is_null());
                assert!(!uses_sret, "i32 return should not use sret");
            }
            Ok(None) => panic!("expected immediate compilation"),
            Err(e) => panic!("compilation failed: {}", e),
        }
    }

    #[test]
    fn test_execute_jit_code() {
        // Create runtime FIRST, like the integration test does.
        let runtime = datalove_rt::rust::Runtime::new();

        let mut jit = JitEngine::new(1).unwrap();
        let func = make_test_function();
        let key = FunctionKey::local(FuncId(0));

        // Compile the function.
        let (code_ptr, uses_sret) = jit.record_call(key, &func).unwrap().unwrap();
        assert!(!uses_sret);

        // Runtime is already created.
        let rt_handle = runtime.handle();

        // Allocate space for return value (use usize for proper alignment).
        let mut result_buf: usize = 0;
        let ret_dest = Destination {
            ptr: &mut result_buf as *mut usize as *mut u8,
            tydesc: std::ptr::null(), // Not used for scalar returns.
        };

        // Call the JIT code.
        // Function takes: (rt_handle) -> i32
        // No user args, scalar return.
        let return_type = IrType::I32;
        unsafe {
            jit.call_jit(code_ptr, uses_sret, rt_handle, &[], ret_dest, &return_type).unwrap();
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
        let identity_fn = IrFunction {
            id: FuncId(0),
            name: "identity".to_string(),
            params: vec![ParamId(0)],
            param_modes: vec![],
            param_types: vec![IrType::I32],
            return_type: IrType::I32,
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            }],
            value_count: 0,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![],
            slot_types: vec![],
            tracked_slots: vec![],
            tracked_params: vec![],
            const_values: vec![],
        };

        // Create caller: fn main() -> i32 { identity(42) }
        let main_fn = IrFunction {
            id: FuncId(1),
            name: "main".to_string(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            return_type: IrType::I32,
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(42) },
                    Instruction::Call {
                        site_id: datalove_datafun_ir::CallSiteId(0),
                        dest: ValueId(1),
                        func: datalove_datafun_ir::FuncRef::Local(FuncId(0)),
                        args: vec![
                            Operand::Value(ValueId(0)),
                        ],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            }],
            value_count: 2,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![IrType::I32, IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            tracked_params: vec![],
            const_values: vec![],
        };

        // Set up interpreter with JIT dispatcher (threshold=1: compile on first call).
        let jit = JitEngine::new(1).expect("JitEngine creation failed");
        let mut interp = IrInterpreter::new_with_options(
            datalove_rt::c::DebugOutputMode::Disabled,
            Some(Box::new(jit)),
        );

        // Set up execution context with both functions (convert to IrCodeUnit).
        let main_code_unit = IrCodeUnit::from(main_fn);
        let functions: Vec<IrCodeUnit> = vec![IrCodeUnit::from(identity_fn), main_code_unit.clone()];
        let ctx = ExecutionContext::new(&functions);
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
        interp.call_in_context(&main_code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames)
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
        let identity_fn = IrFunction {
            id: FuncId(0),
            name: "identity".to_string(),
            params: vec![ParamId(0)],
            param_modes: vec![],
            param_types: vec![IrType::I32],
            return_type: IrType::I32,
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            }],
            value_count: 0,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![],
            slot_types: vec![],
            tracked_slots: vec![],
            tracked_params: vec![],
            const_values: vec![],
        };

        // Create caller: fn main() -> i32 { identity(42) }
        let main_fn = IrFunction {
            id: FuncId(1),
            name: "main".to_string(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            return_type: IrType::I32,
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(42) },
                    Instruction::Call {
                        site_id: datalove_datafun_ir::CallSiteId(0),
                        dest: ValueId(1),
                        func: datalove_datafun_ir::FuncRef::Local(FuncId(0)),
                        args: vec![
                            Operand::Value(ValueId(0)),
                        ],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            }],
            value_count: 2,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![IrType::I32, IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            tracked_params: vec![],
            const_values: vec![],
        };

        // Set up context with both functions (convert to IrCodeUnit).
        let identity_code_unit = IrCodeUnit::from(identity_fn);
        let main_code_unit = IrCodeUnit::from(main_fn);
        let functions: Vec<IrCodeUnit> = vec![identity_code_unit, main_code_unit.clone()];
        let ctx = ExecutionContext::new(&functions);
        let registry = FunctionRegistry::new();

        // Create JIT engine.
        let mut jit = JitEngine::new(1).expect("JitEngine creation failed");

        // Compile main() with context (creates stub for identity()).
        let main_key = FunctionKey::local(FuncId(1));
        let (code_ptr, uses_sret) = jit
            .record_call_with_context(main_key, &main_code_unit, &ctx, &registry)
            .expect("compilation failed")
            .expect("should compile on first call");

        assert!(!uses_sret, "i32 return should not use sret");

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
        let return_type = IrType::I32;
        unsafe {
            bridge::call_jit(code_ptr, uses_sret, rt_handle, &[], ret_dest, &return_type)
                .expect("JIT call failed");
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
        func_ref: &FuncRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        use datalove_datafun_interp::ExecutionContext;

        let key = FunctionKey::from(func_ref);

        // For external functions, we need to use the callee's unit's context to find
        // its local functions. For local/module functions, use the caller's context.
        // _callee_ctx_owned keeps the context alive for the duration of this function.
        let _callee_ctx_owned: Option<ExecutionContext>;
        let compile_ctx = match func_ref {
            FuncRef::External { unit, .. } => {
                // External function - get context from callee's unit.
                match call_ctx.registry.unit_functions(*unit) {
                    Some(unit_funcs) => {
                        _callee_ctx_owned = Some(ExecutionContext::new(unit_funcs));
                        _callee_ctx_owned.as_ref().unwrap()
                    }
                    None => {
                        // Unit not found, fall back to interpreter.
                        return DispatchResult::NotHandled;
                    }
                }
            }
            _ => {
                // Local or module function - use caller's context.
                _callee_ctx_owned = None;
                call_ctx.exec_ctx
            }
        };

        // Use record_call_with_context to enable JIT for functions with calls.
        // This creates stubs for callees so JIT code can call back to interpreter.
        match self.record_call_with_context(key, func, compile_ctx, call_ctx.registry) {
            Ok(Some((code_ptr, uses_sret))) => {
                // JIT code available - set up dispatch context and call it.
                // The trampoline needs this context to route calls back to the interpreter.
                // Use the callee's context for runtime dispatch as well.
                let mut dispatch_ctx = DispatchContext {
                    jit_engine: self,
                    interp: call_ctx.interp,
                    exec_ctx: compile_ctx,
                    registry: call_ctx.registry,
                    frames: call_ctx.frames,
                };

                // SAFETY: context is valid for duration of call.
                unsafe { set_dispatch_context(&mut dispatch_ctx) };

                // SAFETY: code_ptr is a valid JIT-compiled function for this signature.
                let result = unsafe {
                    bridge::call_jit(code_ptr, uses_sret, rt_handle, args, ret_dest, func.return_type().expect("JIT dispatch requires function return type"))
                };

                // Clear dispatch context.
                clear_dispatch_context();

                match result {
                    Ok(()) => DispatchResult::Handled(Ok(())),
                    Err(e) => DispatchResult::Handled(Err(InterpError::RuntimeError(e.to_string()))),
                }
            }
            Ok(None) => {
                // Not yet compiled, fall through to interpreter.
                DispatchResult::NotHandled
            }
            Err(e) => {
                // Check if this is an error that we should fall back for.
                let error_str = e.to_string();
                if error_str.contains("unsupported:")
                    || error_str.contains("Duplicate definition")
                    || error_str.contains("not yet declared")
                {
                    // Mark as not JIT-able and fall back to interpreter.
                    self.states.insert(key, FunctionState::Interpreted { call_count: u32::MAX });
                    DispatchResult::NotHandled
                } else {
                    // Compilation failed with error, report it.
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
