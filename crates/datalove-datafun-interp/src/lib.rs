//! IR interpreter with frame-based execution.
//!
//! # Architecture
//!
//! The interpreter executes IR instructions using a frame-based memory model:
//!
//! - **Frame**: Flat byte buffer (`Vec<u8>`) holding all values and slots for a
//!   function call or script unit. Layout computed from type information.
//!
//! - **Value/Destination**: Pointer + type descriptor pairs. `Value` for reading,
//!   `Destination` for writing. Type descriptors (`TyDesc`) provide size, alignment,
//!   and type-specific metadata for runtime operations.
//!
//! - **Linear semantics**: Non-copy types (Int, String, List, etc.) use move semantics.
//!   Moved values are marked dropped to prevent double-free. Copy types use shallow copy.
//!
//! # Execution Modes
//!
//! **Functions**: Called via `call_with_env()` or `call_in_context()`. Arguments moved
//! into parameter slots, return value moved to caller-provided destination. Frame
//! destroyed after return.
//!
//! **Script units**: Executed via `execute_script_unit_in_env()`. Frames persist in
//! `FrameStore` for subsequent units to access via `ExternalValue`/`ExternalSlot`
//! operands. Supports early return via `!` and `?` operators.
//!
//! # Environment
//!
//! - `FunctionRegistry`: Stores functions from modules and previous script units.
//! - `FrameStore`: Stores frames from previous units for external value access.
//! - `ScriptEnvironment`: Combines registry and frame store for script execution.
//! - `ExecutionContext`: Local functions available during execution.
//!
//! # Runtime Integration
//!
//! All memory operations go through `datalove-rt`: allocation, deallocation, deep
//! copy, comparison, and pretty-printing. Type descriptors are constructed by
//! `IrTyDescTable` from `IrType` definitions.

mod error;
mod value;
mod layout;
mod tydesc;
mod frame;
mod env;
mod ops;
mod types;
mod collections;
mod dispatch;
mod intrinsics;
mod ctfe;
mod native;
mod bytecode;
mod api;
mod call;
mod consts;
mod script;
mod walker;

#[cfg(test)]
mod tests;

pub use error::InterpError;
pub use value::{Value, Destination};
pub use layout::{IrLayout, LayoutCache};
pub use tydesc::IrTyDescTable;
pub use bytecode::BcStats;
pub use frame::{Frame, FrameStack, FrameStore, ScriptFrame};
pub use env::{FunctionRegistry, ModuleFunctionRegistry, UnitFunctionRegistry, ScriptEnvironment, ExecutionContext};
pub use dispatch::{
    CallDispatcher, CompiledEntry, DispatchCallContext, DispatchResult, FuncIdentity, SitePolicy,
    MAX_ENTRY_WORDS,
};
pub use ctfe::InterpCtfeEvaluator;
pub use native::{NativeFunctionTable, NativeFnImpl, NativeResolver, NativeTarget};

/// Room to unpack borrowed values that have no address of their own.
///
/// One box per borrow, kept by whoever prepared the call so that the borrows
/// outlive it. See `borrow_through_wrapper`.
pub(crate) type BorrowScratch = Vec<Box<u64>>;
pub use datalove_rt::c::DebugOutputMode;

use std::cell::RefCell;
use std::rc::Rc;

use datalove_rtdt as rtdt;
use datalove_datafun_ir::{
    IrBlock, IrType, Instruction, Terminator,
    BlockId, Operand, SlotDest, ConstValue, ParamMode, CodeRef,
    IrCodeUnit,
};

/// Result of executing a script unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCompletion {
    /// Normal completion (fragment: no value; expr: value in expr_dest).
    Normal,
    /// Early return via `!` or `?` operator (Result<(), Error> written to ret_dest).
    EarlyReturn,
}

/// How the interpreter runs function bodies.
///
/// Script units always run on the IR walker; the engine decides only what
/// runs the functions they call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Walking the IR, with debug liveness checks: the reference every other
    /// engine is tested against.
    IrWalker,
    /// Register bytecode lowered from the IR.
    Bytecode,
    /// The bytecode as lowered, without its optimization pass: what the
    /// pass's rules are tested against.
    PlainBytecode,
}

impl Engine {
    /// The engine `DATALOVE_INTERP` names, `ir`, `bc` or `bc-plain`, or else
    /// the bytecode.
    pub fn from_env() -> Engine {
        match std::env::var("DATALOVE_INTERP").as_deref() {
            Ok("bc") | Err(_) => Engine::Bytecode,
            Ok("bc-plain") => Engine::PlainBytecode,
            Ok("ir") => Engine::IrWalker,
            Ok(other) => panic!("DATALOVE_INTERP is `{other}`, not `ir`, `bc` or `bc-plain`"),
        }
    }
}

/// Extract list struct pointer, element type descriptor, and element size from a list value.
unsafe fn list_element_info(list_val: &Value) -> (&rtdt::List, *const rtdt::TyDesc, usize) {
    unsafe {
        let list_struct = &*(list_val.ptr as *const rtdt::List);
        let element_tydesc = (*list_val.tydesc).type_info.list.element_tydesc;
        let element_size = (*element_tydesc).size as usize;
        (list_struct, element_tydesc, element_size)
    }
}

/// Extract map, key, and value type descriptors from a map value.
unsafe fn map_tydesc_info(map_val: &Value) -> (*const rtdt::TyDesc, *const rtdt::TyDesc, *const rtdt::TyDesc) {
    unsafe {
        let map_tydesc = map_val.tydesc;
        let key_tydesc = (*map_tydesc).type_info.map.key_tydesc;
        let value_tydesc = (*map_tydesc).type_info.map.value_tydesc;
        (map_tydesc, key_tydesc, value_tydesc)
    }
}

/// IR function interpreter.
pub struct IrInterpreter {
    runtime: datalove_rt::rust::Runtime,
    tydesc_table: IrTyDescTable,
    /// Frame layouts, kept so that calling a function does not recompute one.
    layout_cache: LayoutCache,
    /// Counts the times compiled bodies were forgotten; see
    /// `forget_compiled_bodies`. A call-site cache made in another epoch is
    /// not trusted.
    pub(crate) code_epoch: u64,
    /// Where function frames go, a call pushing one and its return popping it.
    frame_stack: FrameStack,
    /// Call-site caches the bytecode replaced, which a frame may still be
    /// borrowing a layout or a body from; see `bytecode::Plan`.
    retired_call_caches: Vec<bytecode::CallCache>,
    /// Optional call dispatcher for JIT integration.
    /// Uses RefCell to allow passing &mut self to dispatch_call.
    call_dispatcher: RefCell<Option<Box<dyn CallDispatcher>>>,
    /// Counts the dispatchers installed, so that a call site planned by what
    /// one said is not trusted under another; see `bytecode::CallCache`.
    pub(crate) dispatcher_epoch: std::cell::Cell<u64>,
    /// How many calls the next call offered to the dispatcher stands for; see
    /// `DispatchCallContext::weight`. One but while a planned site hands over
    /// the calls it counted.
    pub(crate) call_weight: u32,
    /// Native function dispatch table for rider functions.
    native_table: NativeFunctionTable,
    /// What runs function bodies.
    engine: Engine,
    /// What the bytecode lowering has done, for `DATALOVE_BC_STATS`.
    bc_stats: bytecode::BcStats,
    /// The consts `StaticRef` names, each built once.
    static_pool: StaticPool,
}

/// Consts of non-copy types, each built once and kept until the interpreter
/// is dropped.
///
/// A `StaticRef` holds its value in an `Arc`, which inlining and
/// specialization copy rather than rebuild, so the `Arc`'s address names the
/// value cheaply at every execution. Holding the `Arc` here keeps that address
/// from being reused by another value. Equal values reached through different
/// `Arc`s share one entry.
#[derive(Default)]
struct StaticPool {
    by_identity: rustc_hash::FxHashMap<usize, *const u8>,
    /// Keyed by the descriptor and the value, which hashes by its shape: this
    /// is asked once for each new `Arc` a const arrives in, and the const can
    /// be a whole dataset.
    by_value: rustc_hash::FxHashMap<(usize, datalove_datafun_ir::SharedConst), *const u8>,
    held: Vec<std::sync::Arc<ConstValue>>,
    /// Each value's storage and its descriptor, for destroying it.
    entries: Vec<(Box<[u64]>, *const rtdt::TyDesc)>,
}

impl Drop for IrInterpreter {
    fn drop(&mut self) {
        self.report_bc_stats();
        // Before the runtime the values were allocated from shuts down, which
        // is when it checks for leaks.
        for (mut storage, tydesc) in std::mem::take(&mut self.static_pool.entries) {
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(
                    self.runtime.handle(), storage.as_mut_ptr() as *mut u8, tydesc);
            }
        }
    }
}

/// The types a code unit gives its values and slots.
///
/// Carried alongside the blocks so that an instruction can ask whether what it
/// is about to consume is a copy. Borrowed rather than cloned: a frame is made
/// for every call, and the types do not change.
pub(crate) struct UnitTypes<'a> {
    value_types: &'a [IrType],
    slot_types: &'a [IrType],
    param_types: &'a [IrType],
}

impl<'a> UnitTypes<'a> {
    fn of(unit: &'a IrCodeUnit) -> Self {
        UnitTypes {
            value_types: &unit.value_types,
            slot_types: &unit.slot_types,
            param_types: unit.function_context()
                .map(|c| c.param_types.as_slice())
                .unwrap_or(&[]),
        }
    }

    /// Whether an operand names something a read leaves behind.
    fn is_copy(&self, operand: &Operand) -> bool {
        let ty = match operand {
            Operand::Value(id) | Operand::ValueRef(id) => self.value_types.get(id.0 as usize),
            Operand::Slot(id) => self.slot_types.get(id.0 as usize),
            Operand::Param(id) => self.param_types.get(id.0 as usize),
            // Something another unit owns is never this unit's to keep.
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => None,
        };
        ty.is_some_and(|t| t.is_copy())
    }
}

impl IrInterpreter {
    /// Create a new interpreter with default settings (debug output disabled).
    pub fn new() -> Self {
        Self::new_with_options(datalove_rt::c::DebugOutputMode::Disabled, None)
    }

    /// Create a new interpreter with the specified debug output mode.
    pub fn new_with_debug_mode(debug_mode: datalove_rt::c::DebugOutputMode) -> Self {
        Self::new_with_options(debug_mode, None)
    }

    /// Create a new interpreter with all configuration options.
    pub fn new_with_options(
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> Self {
        Self {
            runtime: datalove_rt::rust::Runtime::new_with_debug_mode(debug_mode),
            tydesc_table: IrTyDescTable::new(),
            layout_cache: LayoutCache::new(),
            code_epoch: 0,
            frame_stack: FrameStack::new(),
            retired_call_caches: Vec::new(),
            call_dispatcher: RefCell::new(call_dispatcher),
            dispatcher_epoch: std::cell::Cell::new(0),
            call_weight: 1,
            native_table: NativeFunctionTable::new(),
            engine: Engine::from_env(),
            static_pool: StaticPool::default(),
            bc_stats: bytecode::BcStats {
                counting: std::env::var_os("DATALOVE_BC_STATS").is_some(),
                ..Default::default()
            },
        }
    }

    /// What lowering bodies to bytecode has done so far: how many, how many
    /// ops, and which kinds of op and of escape.
    pub fn bytecode_stats(&self) -> &bytecode::BcStats {
        &self.bc_stats
    }

    /// Get a mutable reference to the native function table.
    pub fn native_table_mut(&mut self) -> &mut NativeFunctionTable {
        &mut self.native_table
    }

    /// Get the runtime handle for memory management.
    pub fn runtime_handle(&self) -> datalove_rt::c::LocalRtHandle {
        self.runtime.handle()
    }

    /// Take the call dispatcher out of the interpreter.
    ///
    /// Returns the dispatcher if one was set, leaving None in its place.
    /// Useful for inspecting dispatcher state (like JIT stats) after execution.
    pub fn take_dispatcher(&self) -> Option<Box<dyn CallDispatcher>> {
        self.dispatcher_epoch.set(self.dispatcher_epoch.get() + 1);
        self.call_dispatcher.borrow_mut().take()
    }

    /// Install `dispatcher` in place of the one taken out to make a call,
    /// while that call runs, standing in for it.
    ///
    /// A call the dispatcher made takes it out of the interpreter for as long
    /// as the call runs, so that the dispatcher and the interpreter can each
    /// be borrowed. Compiled code calling back into the interpreter would run
    /// with no dispatcher at all, and nothing it called could be compiled or
    /// entered, unless something stands in. What stands in has to act for the
    /// same dispatcher, since plans made while it does are kept: the
    /// dispatcher epoch does not change.
    pub fn lend_dispatcher(&self, dispatcher: Box<dyn CallDispatcher>) {
        let mut slot = self.call_dispatcher.borrow_mut();
        assert!(slot.is_none(), "a dispatcher lent while one is installed");
        *slot = Some(dispatcher);
    }

    /// Take back what `lend_dispatcher` installed.
    pub fn reclaim_dispatcher(&self) -> Box<dyn CallDispatcher> {
        self.call_dispatcher.borrow_mut().take().expect("a dispatcher was lent")
    }

    /// Set or replace the call dispatcher.
    pub fn set_dispatcher(&self, dispatcher: Box<dyn CallDispatcher>) {
        self.dispatcher_epoch.set(self.dispatcher_epoch.get() + 1);
        *self.call_dispatcher.borrow_mut() = Some(dispatcher);
    }

    /// What runs function bodies.
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// Choose what runs function bodies, before running any.
    pub fn set_engine(&mut self, engine: Engine) {
        debug_assert!(self.frame_stack.is_empty(), "switching engines mid-call");
        self.engine = engine;
    }

    /// Get mutable access to the type descriptor table.
    pub fn tydesc_table_mut(&mut self) -> &mut IrTyDescTable {
        &mut self.tydesc_table
    }

}

impl Default for IrInterpreter {
    fn default() -> Self {
        Self::new()
    }
}


/// Copy `size` bytes, without a call into `memcpy` for the sizes a scalar has.
///
/// The size is only known at run time, so a plain `copy_nonoverlapping` is a
/// call, and most of what the interpreter copies is one to eight bytes.
#[inline]
unsafe fn copy_bytes(src: *const u8, dest: *mut u8, size: usize) {
    unsafe {
        match size {
            0 => {}
            1 => std::ptr::copy_nonoverlapping(src, dest, 1),
            2 => std::ptr::copy_nonoverlapping(src, dest, 2),
            4 => std::ptr::copy_nonoverlapping(src, dest, 4),
            8 => std::ptr::copy_nonoverlapping(src, dest, 8),
            _ => std::ptr::copy_nonoverlapping(src, dest, size),
        }
    }
}
