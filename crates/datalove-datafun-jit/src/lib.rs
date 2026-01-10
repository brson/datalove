//! Per-function tracing JIT for the datalove interpreter.
//!
//! Tracks function call counts and compiles hot functions to native code using
//! Cranelift. Integrates with the interpreter for mixed-mode execution.
//!
//! # Usage
//!
//! ```ignore
//! use datalove_datafun_jit::JitEngine;
//! use datalove_datafun_interp::IrInterpreter;
//!
//! let mut interp = IrInterpreter::new();
//! let jit = JitEngine::new(100)?; // Compile after 100 calls
//! interp.set_call_dispatcher(Box::new(jit));
//! ```

mod compiler;
mod bridge;

use std::collections::HashMap;

use datalove_datafun_ir::{FuncId, FuncRef, IrFunction, IrModuleId};
use datalove_datafun_interp::{CallDispatcher, Destination, DispatchResult, InterpError, Value};
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
    },
}

/// Per-function tracing JIT engine.
///
/// Tracks function call counts and compiles hot functions to native code.
/// Single-threaded design - no synchronization overhead.
pub struct JitEngine {
    /// Function states (interpreted with call count, or compiled).
    states: HashMap<FunctionKey, FunctionState>,
    /// Cranelift JIT compiler.
    compiler: JitCompiler,
    /// Call count threshold for triggering compilation.
    threshold: u32,
}

impl JitEngine {
    /// Create a new JIT engine with the specified compilation threshold.
    pub fn new(threshold: u32) -> Result<Self, JitError> {
        Ok(Self {
            states: HashMap::new(),
            compiler: JitCompiler::new()?,
            threshold,
        })
    }

    /// Record a function call and trigger compilation if threshold reached.
    ///
    /// Returns the compiled code pointer if the function was just compiled
    /// or was already compiled.
    pub fn record_call(
        &mut self,
        key: FunctionKey,
        func: &IrFunction,
    ) -> Result<Option<(*const u8, bool)>, JitError> {
        let state = self.states.entry(key).or_insert(FunctionState::Interpreted { call_count: 0 });

        match state {
            FunctionState::Interpreted { call_count } => {
                *call_count += 1;
                if *call_count >= self.threshold {
                    // Compile the function.
                    let (code_ptr, uses_sret) = self.compiler.compile_function(func)?;
                    *state = FunctionState::Compiled { code_ptr, uses_sret };
                    Ok(Some((code_ptr, uses_sret)))
                } else {
                    Ok(None)
                }
            }
            FunctionState::Compiled { code_ptr, uses_sret } => {
                Ok(Some((*code_ptr, *uses_sret)))
            }
        }
    }

    /// Get compiled code for a function if available.
    pub fn get_compiled(&self, key: &FunctionKey) -> Option<(*const u8, bool)> {
        match self.states.get(key) {
            Some(FunctionState::Compiled { code_ptr, uses_sret }) => {
                Some((*code_ptr, *uses_sret))
            }
            _ => None,
        }
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
    ) -> Result<(), JitError> {
        // SAFETY: caller guarantees code_ptr and args are valid.
        unsafe { bridge::call_jit(code_ptr, uses_sret, rt_handle, args, ret_dest) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datafun_ir::{
        IrBlock, Instruction, Terminator, BinOp, Operand,
        ValueId, BlockId, ConstValue, IrType,
    };

    fn make_add_function() -> IrFunction {
        // fn add() -> i32 { 1 + 2 }
        IrFunction {
            id: FuncId(0),
            name: "add".to_string(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            return_type: IrType::I32,
            blocks: vec![IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I32(1) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I32(2) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(2))),
                },
            }],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I32, IrType::I32, IrType::I32],
            slot_types: vec![],
        }
    }

    #[test]
    fn test_jit_engine_creation() {
        let jit = JitEngine::new(100);
        assert!(jit.is_ok());
    }

    #[test]
    fn test_call_counting() {
        let mut jit = JitEngine::new(3).unwrap();
        let func = make_add_function();
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
        let func = make_add_function();
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
        let mut jit = JitEngine::new(1).unwrap();
        let func = make_add_function();
        let key = FunctionKey::local(FuncId(0));

        // Compile the function.
        let (code_ptr, uses_sret) = jit.record_call(key, &func).unwrap().unwrap();
        assert!(!uses_sret);

        // Set up runtime and return destination.
        let runtime = datalove_rt::rust::Runtime::new();
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
        unsafe {
            jit.call_jit(code_ptr, uses_sret, rt_handle, &[], ret_dest).unwrap();
        }

        // Extract i32 from the buffer.
        let result = result_buf as i32;
        assert_eq!(result, 3, "1 + 2 should equal 3");
    }
}

impl CallDispatcher for JitEngine {
    fn dispatch_call(
        &mut self,
        func_ref: &FuncRef,
        func: &IrFunction,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
    ) -> DispatchResult {
        let key = FunctionKey::from(func_ref);

        // Record call and check if we should compile/use JIT.
        match self.record_call(key, func) {
            Ok(Some((code_ptr, uses_sret))) => {
                // JIT code available - call it.
                // SAFETY: code_ptr is a valid JIT-compiled function for this signature.
                let result = unsafe {
                    self.call_jit(code_ptr, uses_sret, rt_handle, args, ret_dest)
                };
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
                // Compilation failed, report error but don't fall through.
                DispatchResult::Handled(Err(InterpError::RuntimeError(e.to_string())))
            }
        }
    }
}
