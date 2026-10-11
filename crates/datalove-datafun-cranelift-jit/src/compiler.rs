//! JIT compiler wrapping Cranelift's JITModule.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder, MemFlagsData};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use target_lexicon::Triple;

use datalove_datafun_ir::{CodeRef, CodeUnitId, Instruction, IrCodeUnit, IrModuleId, IrType};
use datalove_datafun_interp::{ExecutionContext, FuncIdentity, FunctionRegistry, IrInterpreter};
use datalove_datafun_cranelift::codegen::{self, build_signature_for_func, uses_sret};
use datalove_datafun_cranelift::CraneliftError;
use datalove_datafun_cranelift::runtime::RuntimeImports;
use datalove_datafun_cranelift::tydesc_emit::TyDescEmitter;
use datalove_datafun_cranelift::types::{align_shift, PTR_ALIGN, PTR_TYPE};

use crate::trampoline::{self, EncodedFuncKey};
use crate::JitError;

/// Size of the contiguous memory arena for JIT code and data.
///
/// All JIT-compiled code and associated data (type descriptors, constants) are
/// allocated from this arena so that PC-relative references between them stay
/// within the x86_64 32-bit offset limit. If compilation fails with "jit memory
/// region exhausted", increase this value.
const JIT_ARENA_SIZE: usize = 64 * 1024 * 1024;

/// A symbol no other function in the jit module will have.
///
/// Functions arrive here one at a time from whatever modules the program
/// reached, and a module's function is named by its own name alone. Two
/// modules sharing one -- `list.get` and `map.get` -- would otherwise collide;
/// see `FunctionCompiler::compile_as`.
fn unique_symbol(func: &IrCodeUnit) -> String {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}__jit{}", func.name, n)
}

/// Wrap a JIT compilation error with context about arena exhaustion.
///
/// When the arena runs out of space, cranelift reports a generic allocation
/// error. This annotates it so the user knows the arena size can be increased.
fn jit_err(context: &str, err: impl std::fmt::Display) -> JitError {
    let msg = err.to_string();
    if msg.contains("region exhausted") {
        JitError::CompilationFailed(format!(
            "{context}: {msg} (JIT arena is {}MB — increase JIT_ARENA_SIZE if programs are large)",
            JIT_ARENA_SIZE / (1024 * 1024),
        ))
    } else {
        JitError::CompilationFailed(format!("{context}: {msg}"))
    }
}

/// Wrap a codegen error, keeping whether it says the function cannot be
/// compiled rather than that compiling it went wrong.
///
/// The two used to be told apart by looking for `unsupported:` in the rendered
/// message, which made the prose of an error load-bearing and meant a genuine
/// codegen bug that happened to be worded that way turned the jit off for that
/// function without saying so.
fn codegen_err(context: &str, err: CraneliftError) -> JitError {
    match err {
        CraneliftError::Unsupported(msg) => JitError::Unsupported(format!("{context}: {msg}")),
        other => jit_err(context, other),
    }
}

/// JIT compiler using Cranelift.
pub struct JitCompiler {
    /// JIT module for in-memory code generation.
    jit_module: JITModule,
    /// Target ISA.
    isa: Arc<dyn TargetIsa>,
    /// Runtime function imports.
    runtime: RuntimeImports,
    /// Type descriptor emitter.
    tydesc_emitter: TyDescEmitter,
    /// Dispatch function FuncId (for JIT->interpreter calls).
    dispatch_func_id: FuncId,
    /// Shared native symbol table for rider functions.
    ///
    /// Populated after construction via `register_native_symbol()`.
    /// The JITModule's symbol lookup function checks this map.
    native_symbols: Arc<Mutex<HashMap<String, SendPtr>>>,
    /// Whatever the native symbols' code lives in.
    ///
    /// Worse than the interpreter's case, which at least keeps its pointers in
    /// one place: a native's address is emitted into the code cranelift
    /// generates, so clearing `native_symbols` would not take it back. Compiled
    /// code outlives the lookup that built it, so the library has to outlive
    /// the compiler.
    ///
    /// Opaque for the same reason as the interpreter's; see
    /// `NativeFunctionTable::code_owners`.
    code_owners: Mutex<Vec<Arc<dyn Any + Send + Sync>>>,
    /// Where each function's compiled code is, once it has some.
    ///
    /// A stub reads its callee's cell on every call and calls the code directly
    /// when it is there, so that a call between compiled functions does not go
    /// through `__jit_dispatch_call`; that round trip was over 80% of the
    /// jit's time on call-heavy code. The cell's address is emitted into the
    /// stub, so each is boxed to stay put and none is ever removed.
    code_cells: rustc_hash::FxHashMap<FuncIdentity, Box<AtomicUsize>>,
    /// The stub for each callee, made by the first compilation to call it and
    /// called by every later one: a stub depends on nothing but its callee,
    /// and making one is a Cranelift compilation of its own.
    stubs: rustc_hash::FxHashMap<FuncIdentity, FuncId>,
    /// The trampoline for each native rider function, by symbol and the
    /// address it was registered at, likewise.
    native_trampolines: HashMap<(String, u64), FuncId>,
}

/// Wrapper for `*const u8` that implements `Send`.
///
/// Native function pointers from loaded rider libraries are safe to share
/// across threads because they point to immutable compiled code.
#[derive(Clone, Copy)]
pub struct SendPtr(*const u8);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

impl JitCompiler {
    /// Create a new JIT compiler for the host target.
    pub fn new(opt_level: crate::JitOptLevel) -> Result<Self, JitError> {
        use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam};

        let triple = Triple::host();

        // Build target ISA.
        let builder = cranelift_codegen::isa::lookup(triple.clone())
            .map_err(|e| JitError::CompilationFailed(format!("unsupported target: {}", e)))?;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", opt_level.as_str())
            .map_err(|e| JitError::CompilationFailed(format!("settings error: {}", e)))?;
        // Cranelift checks the IR it is given before compiling it, which is
        // for catching the codegen's mistakes: the tests, built with debug
        // assertions, keep it, and a release build does not pay for it.
        settings_builder.set("enable_verifier", if cfg!(debug_assertions) { "true" } else { "false" })
            .map_err(|e| JitError::CompilationFailed(format!("settings error: {}", e)))?;

        let flags = settings::Flags::new(settings_builder);
        let isa = builder.finish(flags)
            .map_err(|e| JitError::CompilationFailed(format!("isa error: {}", e)))?;

        // Build JIT module with all required symbols registered.
        let mut jit_builder = JITBuilder::with_isa(isa.clone(), cranelift_module::default_libcall_names());

        // Use ArenaMemoryProvider to allocate code and data from a single
        // contiguous region. The default SystemMemoryProvider uses separate
        // mmap regions for code and data which can land >2GB apart on x86_64,
        // causing 32-bit PC-relative relocations to overflow.
        let arena = cranelift_jit::ArenaMemoryProvider::new_with_size(JIT_ARENA_SIZE)
            .map_err(|e| JitError::CompilationFailed(format!(
                "failed to reserve {}MB JIT arena: {e}. \
                 The JIT compiler requires a contiguous memory region for code and data.",
                JIT_ARENA_SIZE / (1024 * 1024),
            )))?;
        jit_builder.memory_provider(Box::new(arena));

        register_libcalls(&mut jit_builder);

        // Register a lookup function for dynamically-added native rider symbols.
        let native_symbols: Arc<Mutex<HashMap<String, SendPtr>>> = Arc::new(Mutex::new(HashMap::new()));
        let lookup_symbols = native_symbols.clone();
        jit_builder.symbol_lookup_fn(Box::new(move |name| {
            lookup_symbols.lock().unwrap().get(name).map(|p| p.0)
        }));

        let mut jit_module = JITModule::new(jit_builder);

        // Declare runtime imports.
        let call_conv = isa.default_call_conv();
        let mut runtime = RuntimeImports::declare(&mut jit_module, call_conv)
            .map_err(|e| JitError::CompilationFailed(format!("runtime imports: {}", e)))?;

        // Declare the dispatch function signature.
        // __jit_dispatch_call(rt_handle, encoded_key, ret_dest, arg_count,
        //                     args, descriptors) -> void
        let mut dispatch_sig = cl_ir::Signature::new(call_conv);
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // rt_handle
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // encoded_key
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // ret_dest
        dispatch_sig.params.push(AbiParam::new(cl_types::I32)); // arg_count
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // args
        dispatch_sig.params.push(AbiParam::new(cl_types::I64)); // descriptors
        // No return value - all results written via ret_dest sret pointer.

        let dispatch_func_id = jit_module
            .declare_function("__jit_dispatch_call", Linkage::Import, &dispatch_sig)
            .map_err(|e| JitError::CompilationFailed(format!("declare dispatch: {}", e)))?;

        // Replace all imported FuncIds with local indirect-call trampolines.
        // On x86_64, cranelift-jit uses 32-bit PC-relative relocations for
        // imports, which overflow when the target is >2GB from JIT memory.
        trampoline_all_runtime_imports(&mut jit_module, &mut runtime)?;
        let dispatch_func_id = trampoline_import(
            &mut jit_module,
            dispatch_func_id,
            trampoline::dispatch_fn_ptr() as u64,
        )?;

        let tydesc_emitter = TyDescEmitter::new();

        Ok(Self {
            jit_module,
            isa,
            runtime,
            tydesc_emitter,
            dispatch_func_id,
            native_symbols,
            code_owners: Mutex::new(Vec::new()),
            code_cells: Default::default(),
            stubs: Default::default(),
            native_trampolines: HashMap::new(),
        })
    }

    /// Register a native rider function symbol for JIT resolution.
    ///
    /// Must be called before compiling any function that calls this native.
    pub fn register_native_symbol(&self, name: &str, addr: *const u8) {
        self.native_symbols.lock().unwrap().insert(name.to_string(), SendPtr(addr));
    }

    /// Hold what a registered native symbol's code lives in.
    ///
    /// See [`JitCompiler::code_owners`]. `&self` to match
    /// `register_native_symbol`, which a caller reaching the engine through a
    /// dispatcher has only a shared reference to.
    pub fn hold_code_owner(&self, owner: Arc<dyn Any + Send + Sync>) {
        self.code_owners.lock().expect("jit code owners poisoned").push(owner);
    }

    /// The address of the cell holding `key`'s compiled code, zero until it has some.
    fn code_cell(&mut self, key: FuncIdentity) -> *const AtomicUsize {
        &**self.code_cells.entry(key).or_insert_with(|| Box::new(AtomicUsize::new(0)))
    }

    /// Have every stub for `key` call `code_ptr` directly from now on.
    pub fn publish(&mut self, key: FuncIdentity, code_ptr: *const u8) {
        let cell = self.code_cell(key);
        // SAFETY: the cell is boxed and never removed.
        unsafe { (*cell).store(code_ptr as usize, Ordering::Relaxed) };
    }

    /// Compile a function to native code.
    ///
    /// Each callee gets a stub that calls its compiled code once it has some
    /// and goes through the trampoline to the interpreter until then. `ctx` is
    /// the context the function runs in, which is where its callees are looked
    /// up.
    ///
    /// Returns (code_ptr, uses_sret, code_size).
    pub fn compile_function<'a>(
        &mut self,
        func: &IrCodeUnit,
        ctx: &ExecutionContext<'a>,
        registry: &FunctionRegistry,
        interp: &mut IrInterpreter,
    ) -> Result<(*const u8, bool, usize), JitError> {
        self.compile_entry(func, ctx, registry, interp, None)
    }

    /// Compile an entry into `func` at a loop header, for a frame the
    /// interpreter is running it in; see `codegen::OsrSpec`. Returns the code,
    /// whether it takes `sret`, and an estimate of its size.
    pub fn compile_osr<'a>(
        &mut self,
        func: &IrCodeUnit,
        ctx: &ExecutionContext<'a>,
        registry: &FunctionRegistry,
        interp: &mut IrInterpreter,
        spec: codegen::OsrSpec,
    ) -> Result<(*const u8, bool, usize), JitError> {
        self.compile_entry(func, ctx, registry, interp, Some(spec))
    }

    /// Compile `func`, entered at its start or, given `osr`, at a loop header.
    fn compile_entry<'a>(
        &mut self,
        func: &IrCodeUnit,
        ctx: &ExecutionContext<'a>,
        registry: &FunctionRegistry,
        interp: &mut IrInterpreter,
        osr: Option<codegen::OsrSpec>,
    ) -> Result<(*const u8, bool, usize), JitError> {
        // Collect all Call targets in this function.
        let callees = self.collect_call_targets(func);

        // Create stubs for each callee.
        let mut local_funcs: HashMap<CodeUnitId, codegen::LocalCallee> = HashMap::new();
        let mut external_funcs: HashMap<(u32, CodeUnitId), codegen::LocalCallee> = HashMap::new();
        let mut module_funcs: HashMap<(IrModuleId, CodeUnitId), FuncId> = HashMap::new();

        for code_ref in callees {
            // Look up the callee's IR to get its signature.
            let callee_ir = ctx.get_unit(&code_ref, registry);

            // Native rider functions: create local trampoline stubs that use
            // indirect calls via absolute address. Direct imports would require
            // 32-bit relative relocations which can overflow if the shared
            // library is loaded >2GB from JIT code memory.
            if let Some(native_ctx) = callee_ir.native_context() {
                let sig = codegen::build_native_signature(native_ctx, self.isa.as_ref());
                let addr = self.native_symbols.lock().unwrap()
                    .get(native_ctx.symbol())
                    .map(|p| p.0 as u64)
                    .unwrap_or_else(|| panic!("native symbol not registered: {}", native_ctx.symbol()));
                let cached = (native_ctx.symbol().to_owned(), addr);
                let func_id = match self.native_trampolines.get(&cached) {
                    Some(&id) => id,
                    None => {
                        let id = self.create_native_trampoline(native_ctx.symbol(), &sig, addr)?;
                        self.native_trampolines.insert(cached, id);
                        id
                    }
                };
                if let CodeRef::Module { module, id } = &code_ref {
                    module_funcs.insert((*module, CodeUnitId(id.0)), func_id);
                }
                continue;
            }

            // Create a stub for this callee.
            let key = FuncIdentity::of(&code_ref, ctx.unit());
            let stub_id = match self.stubs.get(&key) {
                Some(&id) => id,
                None => {
                    let id = self.create_stub_for_callee(key, &callee_ir)?;
                    self.stubs.insert(key, id);
                    id
                }
            };

            // Register in appropriate map.
            match &code_ref {
                CodeRef::Local(id) => {
                    local_funcs.insert(
                        CodeUnitId(id.0), codegen::LocalCallee::of(&callee_ir, stub_id));
                }
                CodeRef::Module { module, id } => {
                    module_funcs.insert((*module, CodeUnitId(id.0)), stub_id);
                }
                CodeRef::External { unit, id } => {
                    // Keyed by the unit as well, because the id alone is the
                    // callee's position in its own unit's list and this body may
                    // also have a local function of that number.
                    external_funcs.insert(
                        (*unit, CodeUnitId(id.0)),
                        codegen::LocalCallee::of(&callee_ir, stub_id));
                }
            }
        }

        // Build a FunctionCompiler with stub mappings.
        let mut compiler = codegen::FunctionCompiler::new(
            func,
            self.isa.as_ref(),
            &mut self.jit_module,
            self.runtime.clone(),
            &mut self.tydesc_emitter,
            Some(registry),
        );
        compiler.set_local_funcs(local_funcs);
        compiler.set_external_funcs(external_funcs);
        compiler.set_module_funcs(module_funcs);
        compiler.set_static_consts(static_consts_of(func, interp));
        compiler.abandon_on_interpreter_errors();

        // Compile and get the Cranelift FuncId.
        let cl_func_id = match osr {
            None => compiler.compile_as(&unique_symbol(func)),
            Some(spec) => compiler.compile_osr_as(&unique_symbol(func), spec),
        }.map_err(|e| codegen_err("compile", e))?;

        // Finalize to get executable code.
        self.jit_module.finalize_definitions()
            .map_err(|e| jit_err("finalize", e))?;

        // Get the code pointer and size.
        let code_ptr = self.jit_module.get_finalized_function(cl_func_id);

        // Estimate code size from IR function.
        let code_size = estimate_code_size(func);

        // Determine if function uses sret.
        let func_ctx = func.function_context()
            .expect("function must be a function code unit");
        let sret = uses_sret(&func_ctx.return_type);

        Ok((code_ptr, sret, code_size))
    }

    /// Collect all unique Call targets in a code unit.
    fn collect_call_targets(&self, func: &IrCodeUnit) -> HashSet<CodeRef> {
        let mut targets = HashSet::new();

        // A `ComptimeCall` is a call too, where specialization left one: the
        // codegen compiles it as one, and its callee needs a stub as much.
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Some((code_ref, _)) = inst.call_target() {
                    targets.insert(code_ref.clone());
                }
            }
        }

        targets
    }

    /// Create a local trampoline for a native rider function.
    ///
    /// Uses an indirect call with an absolute address to avoid 32-bit
    /// relative relocation overflow when the shared library is far from
    /// JIT code memory.
    fn create_native_trampoline(
        &mut self,
        symbol: &str,
        sig: &cl_ir::Signature,
        addr: u64,
    ) -> Result<FuncId, JitError> {
        create_indirect_trampoline(&mut self.jit_module, symbol, sig, addr)
    }

    /// Create a stub function for a callee that dispatches through the trampoline.
    ///
    /// The stub has the same signature as the callee and internally calls
    /// __jit_dispatch_call with the encoded function key.
    fn create_stub_for_callee(
        &mut self,
        key: FuncIdentity,
        callee: &IrCodeUnit,
    ) -> Result<FuncId, JitError> {
        // Generate unique stub name.
        static STUB_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let stub_num = STUB_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stub_name = format!("__jit_stub_{}_{}", callee.name, stub_num);

        // Build signature matching the callee.
        let sig = build_signature_for_func(callee, self.isa.as_ref());

        // Declare the stub.
        let stub_id = self.jit_module
            .declare_function(&stub_name, Linkage::Local, &sig)
            .map_err(|e| jit_err("declare stub", e))?;

        // Define the stub.
        let cell = self.code_cell(key);
        self.define_stub(stub_id, key, cell, callee, &sig)?;

        Ok(stub_id)
    }

    /// Define a stub function body.
    ///
    /// The stub calls the callee's compiled code if `cell` holds any, and
    /// otherwise calls __jit_dispatch_call, which counts the call toward
    /// compiling it and interprets it until then. The stub has the callee's own
    /// signature, so the direct call passes its parameters through unchanged.
    fn define_stub(
        &mut self,
        stub_id: FuncId,
        key: FuncIdentity,
        cell: *const AtomicUsize,
        callee: &IrCodeUnit,
        sig: &cl_ir::Signature,
    ) -> Result<(), JitError> {
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, stub_id.as_u32()),
            sig.clone(),
        );

        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Get function parameters.
        let params: Vec<_> = builder.block_params(entry_block).to_vec();

        let cell_addr = builder.ins().iconst(PTR_TYPE, cell as i64);
        let code_ptr = builder.ins().load(PTR_TYPE, MemFlagsData::trusted(), cell_addr, 0);
        let direct_block = builder.create_block();
        let dispatch_block = builder.create_block();
        builder.ins().brif(code_ptr, direct_block, &[], dispatch_block, &[]);

        builder.switch_to_block(direct_block);
        builder.seal_block(direct_block);
        let sig_ref = builder.import_signature(sig.clone());
        builder.ins().call_indirect(sig_ref, code_ptr, &params);
        builder.ins().return_(&[]);

        builder.switch_to_block(dispatch_block);
        builder.seal_block(dispatch_block);

        // rt_handle is always first param.
        let rt_handle = params[0];

        // Determine if callee uses sret.
        let callee_ctx = callee.function_context()
            .expect("callee must be a function code unit");
        let callee_uses_sret = uses_sret(&callee_ctx.return_type);

        // Get sret ptr if applicable.
        let (sret_ptr, user_args_start) = if callee_uses_sret {
            (Some(params[1]), 2)
        } else {
            (None, 1)
        };

        // User arguments, and after them the descriptors for the parameters
        // whose own type does not describe what arrives. Both have to go
        // across: what the stub hands the call to works out each argument's
        // type from the callee's signature, and for those parameters the
        // signature says `data` where the caller has something else.
        let user_args_end = user_args_start + callee_ctx.param_types.len();
        let user_args = &params[user_args_start..user_args_end].to_vec();
        let descriptors = params[user_args_end..].to_vec();

        // Build args array on stack.
        // Allocate stack slot for args array.
        let arg_count = user_args.len();
        let args_slot = if arg_count > 0 {
            let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                (arg_count * 8) as u32,
                align_shift(PTR_ALIGN),
            ));
            // Store each arg pointer in the array.
            for (i, &arg) in user_args.iter().enumerate() {
                let offset = (i * 8) as i32;
                builder.ins().stack_store(PTR_TYPE, arg, slot, offset);
            }
            Some(slot)
        } else {
            None
        };

        // Get pointer to args array (or null if no args).
        let args_ptr = if let Some(slot) = args_slot {
            builder.ins().stack_addr(PTR_TYPE, slot, 0)
        } else {
            builder.ins().iconst(PTR_TYPE, 0)
        };

        // The descriptors travel the same way, in the order the callee's
        // `descriptor_params` names, which is the order they arrived in.
        let descriptors_ptr = if descriptors.is_empty() {
            builder.ins().iconst(PTR_TYPE, 0)
        } else {
            let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                (descriptors.len() * 8) as u32,
                align_shift(PTR_ALIGN),
            ));
            for (i, &tydesc) in descriptors.iter().enumerate() {
                builder.ins().stack_store(PTR_TYPE, tydesc, slot, (i * 8) as i32);
            }
            builder.ins().stack_addr(PTR_TYPE, slot, 0)
        };

        // Encode function key.
        let encoded_key = EncodedFuncKey::from_identity(key);
        let encoded_key_val = builder.ins().iconst(cl_types::I64, encoded_key.as_u64() as i64);

        // Get return destination pointer.
        // All non-Unit returns use sret. Get the return destination pointer.
        let ret_dest = if let Some(sret) = sret_ptr {
            sret
        } else {
            // Unit return - null pointer.
            builder.ins().iconst(PTR_TYPE, 0)
        };

        // arg_count.
        let arg_count_val = builder.ins().iconst(cl_types::I32, arg_count as i64);

        // Call __jit_dispatch_call (void return - result written via ret_dest).
        let dispatch_ref = self.jit_module.declare_func_in_func(self.dispatch_func_id, builder.func);
        let call_args = [
            rt_handle, encoded_key_val, ret_dest, arg_count_val,
            args_ptr, descriptors_ptr,
        ];
        builder.ins().call(dispatch_ref, &call_args);

        // All stubs return void. Sret results are written by the callee.
        builder.ins().return_(&[]);

        builder.finalize(self.jit_module.target_config());

        // Define the function in the module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        self.jit_module
            .define_function(stub_id, &mut ctx)
            .map_err(|e| jit_err("define stub", e))?;

        Ok(())
    }
}

/// Estimate code size for a code unit based on IR complexity.
///
/// Uses heuristics since Cranelift doesn't expose compiled code size directly.
/// Estimates ~10 bytes per instruction + 20 bytes per block for control flow.
fn estimate_code_size(func: &IrCodeUnit) -> usize {
    let mut instruction_count = 0;
    for block in &func.blocks {
        instruction_count += block.instructions.len();
    }
    let block_count = func.blocks.len();

    // Base estimate: 10 bytes per instruction, 20 bytes per block.
    let base = instruction_count * 10 + block_count * 20;

    // Add overhead for function prologue/epilogue (~32 bytes).
    let overhead = 32;

    // Add overhead for parameters (~8 bytes each for stack setup).
    let param_overhead = func.function_context()
        .map(|ctx| ctx.params.len() * 8)
        .unwrap_or(0);

    base + overhead + param_overhead
}

/// Create a local trampoline function that calls an external address indirectly.
///
/// On x86_64, cranelift-jit uses 32-bit PC-relative relocations for imported
/// functions. When the target symbol is in the main binary or a shared library
/// loaded >2GB from JIT code memory, these relocations overflow. This function
/// creates a local trampoline in JIT memory that uses `call_indirect` with the
/// absolute address, avoiding PC-relative relocations entirely.
fn create_indirect_trampoline(
    jit_module: &mut JITModule,
    symbol: &str,
    sig: &cl_ir::Signature,
    addr: u64,
) -> Result<FuncId, JitError> {
    static TRAMP_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let num = TRAMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tramp_name = format!("__jit_tramp_{}_{}", symbol, num);

    // Declare as local so it lives in JIT memory.
    let tramp_id = jit_module
        .declare_function(&tramp_name, Linkage::Local, sig)
        .map_err(|e| jit_err("declare trampoline", e))?;

    // Build a function that loads the absolute address and does call_indirect.
    let mut cl_func = cl_ir::Function::with_name_signature(
        cl_ir::UserFuncName::user(0, tramp_id.as_u32()),
        sig.clone(),
    );

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);

    let params: Vec<_> = builder.block_params(entry_block).to_vec();

    // Load absolute address of the target function.
    let addr_val = builder.ins().iconst(cl_types::I64, addr as i64);

    // Declare the signature for the indirect call.
    let sig_ref = builder.import_signature(sig.clone());

    // Call indirectly through the absolute address.
    let call = builder.ins().call_indirect(sig_ref, addr_val, &params);

    // Return the result (if any).
    let results: Vec<_> = builder.inst_results(call).to_vec();
    builder.ins().return_(&results);

    builder.finalize(jit_module.target_config());

    // Define the function in the JIT module.
    let mut ctx = cranelift_codegen::Context::for_function(cl_func);
    jit_module.define_function(tramp_id, &mut ctx)
        .map_err(|e| jit_err("define trampoline", e))?;

    Ok(tramp_id)
}

/// Replace an imported FuncId with a local trampoline that calls the import indirectly.
///
/// Looks up the function's signature from the module declarations and creates
/// a trampoline using the given absolute address.
fn trampoline_import(
    jit_module: &mut JITModule,
    func_id: FuncId,
    addr: u64,
) -> Result<FuncId, JitError> {
    let decl = jit_module.declarations().get_function_decl(func_id);
    let name = decl.linkage_name(func_id).into_owned();
    let sig = decl.signature.clone();
    create_indirect_trampoline(jit_module, &name, &sig, addr)
}

/// Replace every runtime import with a local trampoline that calls the
/// runtime function's absolute address.
fn trampoline_all_runtime_imports(
    jit_module: &mut JITModule,
    runtime: &mut RuntimeImports,
) -> Result<(), JitError> {
    let addresses = runtime_addresses();
    for func_id in runtime.all_mut() {
        let decl = jit_module.declarations().get_function_decl(*func_id);
        let name = decl.linkage_name(*func_id);
        let addr = *addresses.get(name.as_ref())
            .unwrap_or_else(|| panic!("no address for runtime import {name}"));
        *func_id = trampoline_import(jit_module, *func_id, addr as u64)?;
    }
    Ok(())
}

/// The address of each runtime function compiled code may import, by symbol.
fn runtime_addresses() -> HashMap<&'static str, *const u8> {
    macro_rules! addresses {
        ($($f:ident,)*) => {
            HashMap::from([$((stringify!($f), datalove_rt::c::$f as *const u8)),*])
        };
    }
    addresses! {
        dtlv_rti_init,
        dtlv_rti_shutdown,
        dtlv_rti_set_debug_mode,
        dtlv_rti_debuglog_local,
        dtlv_rti_any_destroy_local,
        dtlv_rti_mem_alloc_raw_local,
        dtlv_rti_mem_free_raw_local,
        dtlv_rti_field_offset,
        dtlv_rti_field_tydesc,
        dtlv_rti_field_read_local,
        dtlv_rti_element_tydesc,
        dtlv_rti_btreemap_contains_key_erased_local,
        dtlv_rti_btreemap_get_value_ref_erased_local,
        dtlv_rti_list_set_erased_local,
        dtlv_rti_tensor_set_erased_local,
        dtlv_rti_btreemap_set_value_erased_local,
        dtlv_rti_string_create_local,
        dtlv_rti_string_push_bytes_local,
        dtlv_rti_string_from_bytes,
        dtlv_rti_list_create_local,
        dtlv_rti_list_push_local,
        dtlv_rti_list_build_from_slice_local,
        dtlv_rti_btreeset_create_local,
        dtlv_rti_btreeset_insert_local,
        dtlv_rti_btreemap_create_local,
        dtlv_rti_btreemap_insert_local,
        dtlv_rti_list_push_erased_local,
        dtlv_rti_btreeset_insert_erased_local,
        dtlv_rti_btreemap_insert_erased_local,
        dtlv_rti_btreemap_contains_key_local,
        dtlv_rti_btreemap_get_value_ref_local,
        dtlv_rti_btreemap_set_value_local,
        dtlv_rti_tensor_init_local,
        dtlv_rti_tensor_hyperplane_clone_local,
        dtlv_rti_table_create_local,
        dtlv_rti_table_push_row_local,
        dtlv_rti_table_build_from_rows_local,
        dtlv_rti_int_add,
        dtlv_rti_int_sub,
        dtlv_rti_int_mul,
        dtlv_rti_int_div_checked,
        dtlv_rti_int_neg,
        dtlv_rti_int_add_assign,
        dtlv_rti_int_sub_assign,
        dtlv_rti_int_mul_assign,
        dtlv_rti_int_div_assign_checked,
        dtlv_rti_int_from_fixed,
        dtlv_rti_int_from_limbs,
        dtlv_rti_cmp_local,
        dtlv_rti_eq_local,
        dtlv_rti_move_value_local,
        dtlv_rti_clone_local,
        dtlv_rti_error_from_local,
        dtlv_rti_data_from_local,
        dtlv_rti_dyn_binop,
        dtlv_rti_dyn_binop_checked,
        dtlv_rti_dyn_neg_checked,
        dtlv_rti_erase_local,
        dtlv_rti_reify_local,
        dtlv_rti_data_parts,
        dtlv_rti_data_borrow,
        dtlv_rti_list_get_erased_local,
        dtlv_rti_clone_erased_local,
    }
}

/// Register the libm functions Cranelift calls when legalizing float
/// instructions.
///
/// On some platforms dlsym can't find these (e.g. static linking), so they are
/// registered explicitly.
fn register_libcalls(jit_builder: &mut JITBuilder) {
    unsafe extern "C" {
        fn floor(x: f64) -> f64;
        fn floorf(x: f32) -> f32;
        fn ceil(x: f64) -> f64;
        fn ceilf(x: f32) -> f32;
        fn sqrt(x: f64) -> f64;
        fn sqrtf(x: f32) -> f32;
        fn trunc(x: f64) -> f64;
        fn truncf(x: f32) -> f32;
        fn nearbyint(x: f64) -> f64;
        fn nearbyintf(x: f32) -> f32;
        fn fma(x: f64, y: f64, z: f64) -> f64;
        fn fmaf(x: f32, y: f32, z: f32) -> f32;
    }
    jit_builder.symbol("floor", floor as *const u8);
    jit_builder.symbol("floorf", floorf as *const u8);
    jit_builder.symbol("ceil", ceil as *const u8);
    jit_builder.symbol("ceilf", ceilf as *const u8);
    jit_builder.symbol("sqrt", sqrt as *const u8);
    jit_builder.symbol("sqrtf", sqrtf as *const u8);
    jit_builder.symbol("trunc", trunc as *const u8);
    jit_builder.symbol("truncf", truncf as *const u8);
    jit_builder.symbol("nearbyint", nearbyint as *const u8);
    jit_builder.symbol("nearbyintf", nearbyintf as *const u8);
    jit_builder.symbol("fma", fma as *const u8);
    jit_builder.symbol("fmaf", fmaf as *const u8);
}

/// Where each const a `StaticRef` in `func` names lives: in the interpreter's
/// pool, which builds it now if nothing has yet.
///
/// The address is the interpreter's, and so is the heap behind it, which the
/// compiled code runs against too. The engine belongs to the interpreter, so
/// the code cannot outlive the pool.
fn static_consts_of(func: &IrCodeUnit, interp: &mut IrInterpreter) -> codegen::StaticConsts {
    let mut statics = codegen::StaticConsts::new();
    for block in &func.blocks {
        for instr in &block.instructions {
            let Instruction::StaticRef { dest, value } = instr else { continue };
            let IrType::Ref(ty) = &func.value_types[dest.0 as usize] else {
                panic!("a static ref's destination is a reference");
            };
            let addr = interp.static_const(value, ty);
            statics.insert(codegen::static_const_key(value), codegen::StaticConstLoc::Address(addr as usize));
        }
    }
    statics
}
