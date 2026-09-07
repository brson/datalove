//! AOT compilation backend using Cranelift.
//!
//! Compiles datafun IR directly to native code via Cranelift,
//! producing object files for linking with the datalove runtime.
//!
//! # Architecture
//!
//! The AOT compiler uses a three-pass approach for whole-world compilation:
//!
//! 1. **Type collection**: Collect all types from all functions and script units.
//! 2. **Declaration**: Declare all functions and emit all type descriptors upfront.
//! 3. **Definition**: Compile function bodies with full call graph visibility.
//!
//! # Key types
//!
//! - [`AotCompiler`]: Main entry point for compilation.
//! - [`codegen::FunctionCompiler`]: Compiles individual IR functions to Cranelift IR.
//! - [`tydesc_emit::TyDescEmitter`]: Emits runtime type descriptors as static data.
//!
//! # Generated code structure
//!
//! For a script unit, the compiler generates:
//! - `__script_body(rt: *mut u8)`: The script body taking a runtime handle.
//! - `main()`: Entry point that initializes runtime, runs body, cleans up.

// Re-export shared codegen infrastructure.
pub use datalove_datafun_cranelift::{
    codegen, index_types, layout, runtime, tydesc_emit, types,
    CraneliftError,
};

use std::collections::{BTreeMap, HashMap};

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam, InstBuilder};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule, ObjectProduct};
use target_lexicon::Triple;

use datalove_datafun_ir::{
    IrCodeUnit, IrModule, IrModuleId, IrType,
};

/// Errors during AOT compilation.
#[derive(Debug)]
pub enum AotError {
    /// Cranelift codegen error.
    Codegen(String),
    /// Module error.
    Module(String),
    /// Unsupported feature.
    Unsupported(String),
}

impl std::fmt::Display for AotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AotError::Codegen(msg) => write!(f, "codegen error: {}", msg),
            AotError::Module(msg) => write!(f, "module error: {}", msg),
            AotError::Unsupported(msg) => write!(f, "unsupported: {}", msg),
        }
    }
}

impl std::error::Error for AotError {}

impl From<CraneliftError> for AotError {
    fn from(e: CraneliftError) -> Self {
        match e {
            CraneliftError::Codegen(msg) => AotError::Codegen(msg),
            CraneliftError::Module(msg) => AotError::Module(msg),
            CraneliftError::Unsupported(msg) => AotError::Unsupported(msg),
        }
    }
}

/// AOT compiler context.
///
/// Holds Cranelift state for compiling IR to native code.
pub struct AotCompiler {
    /// Target ISA for code generation.
    isa: std::sync::Arc<dyn TargetIsa>,
}

impl AotCompiler {
    /// Create a new AOT compiler for the host target.
    pub fn new_for_host() -> Result<Self, AotError> {
        Self::new_for_target(Triple::host())
    }

    /// Create a new AOT compiler for a specific target triple.
    pub fn new_for_target(triple: Triple) -> Result<Self, AotError> {
        let builder = cranelift_codegen::isa::lookup(triple)
            .map_err(|e| AotError::Codegen(format!("unsupported target: {}", e)))?;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed")
            .map_err(|e| AotError::Codegen(format!("settings error: {}", e)))?;
        // Enable position-independent code to avoid linker warnings about DT_TEXTREL.
        settings_builder.set("is_pic", "true")
            .map_err(|e| AotError::Codegen(format!("settings error: {}", e)))?;

        let flags = settings::Flags::new(settings_builder);
        let isa = builder.finish(flags)
            .map_err(|e| AotError::Codegen(format!("isa error: {}", e)))?;

        Ok(Self { isa })
    }

    /// Compile an IR module to an object file.
    pub fn compile_module(&mut self, module: &IrModule) -> Result<ObjectProduct, AotError> {
        let obj_builder = ObjectBuilder::new(
            self.isa.clone(),
            "module",
            cranelift_module::default_libcall_names(),
        ).map_err(|e| AotError::Module(format!("object builder error: {}", e)))?;

        let mut obj_module = ObjectModule::new(obj_builder);

        for func in &module.functions {
            self.compile_function(&mut obj_module, func)?;
        }

        Ok(obj_module.finish())
    }

    /// Compile an IR script unit to an object file.
    ///
    /// This method collects types only from the script unit itself. For whole-world
    /// compilation with modules, use `compile_script_unit_with_world_types` instead.
    ///
    /// Generates:
    /// - `__script_body(rt: *mut u8)` - The script body that takes runtime handle
    /// - `main()` - Entry point that initializes runtime, runs body, cleans up
    pub fn compile_script_unit(&mut self, unit: &IrCodeUnit) -> Result<ObjectProduct, AotError> {
        let types = tydesc_emit::collect_types_from_script_unit(unit);
        let empty_registry = datalove_datafun_ir::FunctionRegistry::new();
        self.compile_script_unit_with_types(unit, types, &empty_registry)
    }

    /// Compile an IR script unit with pre-collected world types.
    ///
    /// Use this when compiling in a world with modules. Pass types collected from
    /// all module functions and prior script units.
    ///
    /// Generates:
    /// - `__script_body(rt: *mut u8)` - The script body that takes runtime handle
    /// - `main()` - Entry point that initializes runtime, runs body, cleans up
    pub fn compile_script_unit_with_world_types<'a>(
        &mut self,
        unit: &IrCodeUnit,
        world_units: impl Iterator<Item = &'a IrCodeUnit>,
        registry: &datalove_datafun_ir::FunctionRegistry,
    ) -> Result<ObjectProduct, AotError> {
        // Collect types from world code units and the script unit.
        let mut types = tydesc_emit::collect_types_from_script_unit(unit);
        tydesc_emit::collect_types_from_code_units(world_units, &mut types);
        self.compile_script_unit_with_types(unit, types, registry)
    }

    /// Compile an IR script unit with pre-collected types.
    fn compile_script_unit_with_types(
        &mut self,
        unit: &IrCodeUnit,
        types: std::collections::BTreeSet<IrType>,
        registry: &datalove_datafun_ir::FunctionRegistry,
    ) -> Result<ObjectProduct, AotError> {
        let obj_builder = ObjectBuilder::new(
            self.isa.clone(),
            "script",
            cranelift_module::default_libcall_names(),
        ).map_err(|e| AotError::Module(format!("object builder error: {}", e)))?;

        let mut obj_module = ObjectModule::new(obj_builder);

        // Declare runtime imports.
        let call_conv = self.isa.default_call_conv();
        let runtime_imports = runtime::RuntimeImports::declare(&mut obj_module, call_conv)?;

        // Emit all TyDescs upfront (whole-world compilation).
        let mut tydesc_emitter = tydesc_emit::TyDescEmitter::new();
        tydesc_emitter.emit_all(&mut obj_module, types)?;

        // === Three-pass compilation for local and module functions ===

        // Pass 1: Declare all local functions (nested units) to get Cranelift FuncIds.
        let mut local_funcs: HashMap<datalove_datafun_ir::CodeUnitId, codegen::LocalCallee> =
            HashMap::new();
        for func in &unit.nested_units {
            let sig = codegen::build_signature_for_func(func, self.isa.as_ref());
            let func_id = datalove_datafun_ir::CodeUnitId(func.id.0);
            let cl_func_id = obj_module
                .declare_function(&func.name, Linkage::Local, &sig)
                .map_err(|e| AotError::Module(format!("declare function {}: {}", func.name, e)))?;
            local_funcs.insert(func_id, codegen::LocalCallee::of(func, cl_func_id));
        }

        // Pass 2: Declare all module functions to get Cranelift FuncIds.
        // Native code units are declared as imports with their linker symbol.
        // Ordered, so that function bodies land in the object file in the same
        // order on every run.
        let mut module_funcs: BTreeMap<(IrModuleId, datalove_datafun_ir::CodeUnitId), FuncId> = BTreeMap::new();
        for ((module_id, func_id), ir_unit) in registry.iter_module_code_units_with_ids() {
            if let Some(ctx) = ir_unit.native_context() {
                // Native function: declare as import with C ABI signature.
                let sig = codegen::build_native_signature(ctx, self.isa.as_ref());
                let cl_func_id = obj_module
                    .declare_function(&ctx.symbol, Linkage::Import, &sig)
                    .map_err(|e| AotError::Module(format!("declare native import {}: {}", ctx.symbol, e)))?;
                module_funcs.insert((module_id, func_id), cl_func_id);
                continue;
            }
            let name = format!("__mod_{}_{}", module_id.0, ir_unit.name);
            let sig = codegen::build_signature_for_func(ir_unit, self.isa.as_ref());
            let cl_func_id = obj_module
                .declare_function(&name, Linkage::Local, &sig)
                .map_err(|e| AotError::Module(format!("declare module function {}: {}", name, e)))?;
            module_funcs.insert((module_id, func_id), cl_func_id);
        }

        // Pass 3a: Compile all local functions with pre-declared FuncIds.
        for func in &unit.nested_units {
            let func_id = datalove_datafun_ir::CodeUnitId(func.id.0);
            let cl_func_id = local_funcs[&func_id].func_id;
            let mut compiler = codegen::FunctionCompiler::new_with_runtime_and_tydescs(
                func,
                self.isa.as_ref(),
                &mut obj_module,
                runtime_imports.clone(),
                tydesc_emitter.clone(),
                Some(registry),
            );
            compiler.set_local_funcs(local_funcs.clone());
            compiler.set_module_funcs(module_funcs.iter().map(|(k, v)| (*k, *v)).collect());
            compiler.compile_predeclared(cl_func_id)?;
        }

        // Pass 3b: Compile all module functions with pre-declared FuncIds.
        // Skip native code units — they have no blocks (resolved by the linker).
        for ((module_id, func_id), &cl_func_id) in &module_funcs {
            let ir_unit = registry.get_module_function_as_unit(*module_id, *func_id)
                .ok_or_else(|| AotError::Module(format!("module function not found: {:?}, {:?}", module_id, func_id)))?;
            if ir_unit.native_context().is_some() {
                continue;
            }

            let mut compiler = codegen::FunctionCompiler::new_with_runtime_and_tydescs(
                ir_unit,
                self.isa.as_ref(),
                &mut obj_module,
                runtime_imports.clone(),
                tydesc_emitter.clone(),
                Some(registry),
            );
            compiler.set_local_funcs(local_funcs.clone());
            compiler.set_module_funcs(module_funcs.iter().map(|(k, v)| (*k, *v)).collect());
            compiler.compile_predeclared(cl_func_id)?;
        }

        // Convert script unit to a function-like code unit for compilation.
        let body_func = self.script_unit_to_function(unit);

        // Compile the body function with pre-populated local_funcs and module_funcs.
        let mut compiler = codegen::FunctionCompiler::new_with_runtime_and_tydescs(
            &body_func,
            self.isa.as_ref(),
            &mut obj_module,
            runtime_imports,
            tydesc_emitter,
            Some(registry),
        );
        compiler.set_local_funcs(local_funcs);
        compiler.set_module_funcs(module_funcs.into_iter().collect());
        let body_func_id = compiler.compile()?;

        // Generate the entry point.
        self.compile_entry_point(&mut obj_module, body_func_id)?;

        Ok(obj_module.finish())
    }

    /// Convert a script code unit to a function-like code unit for compilation.
    ///
    /// The script body becomes a function with no params and Unit return type.
    fn script_unit_to_function(&self, unit: &IrCodeUnit) -> IrCodeUnit {
        // Keep terminators as-is; compile_terminator handles Exit/EarlyExit.
        // Note: rt_handle is implicit - codegen adds it to all function signatures.
        IrCodeUnit {
            id: datalove_datafun_ir::CodeUnitId(0),
            name: "__script_body".to_string(),
            blocks: unit.blocks.clone(),
            value_count: unit.value_count,
            slot_count: unit.slot_count,
            call_site_count: unit.call_site_count,
            value_types: unit.value_types.clone(),
            slot_types: unit.slot_types.clone(),
            tracked_slots: unit.tracked_slots.clone(),
            const_values: unit.const_values.clone(),
            symbols: unit.symbols.clone(),
            context: datalove_datafun_ir::CodeUnitContext::Function(
                datalove_datafun_ir::FunctionContext {
                    descriptor_params: Vec::new(),
                    params: vec![],
                    param_modes: vec![],
                    param_types: vec![],
                    return_type: datalove_datafun_ir::IrType::Unit,
                    tracked_params: vec![],
                    descriptor_shapes: Vec::new(),
                }
            ),
            nested_units: vec![], // Functions don't have nested units
        }
    }

    /// Generate the main entry point.
    fn compile_entry_point(
        &self,
        module: &mut ObjectModule,
        body_func_id: cranelift_module::FuncId,
    ) -> Result<(), AotError> {
        let call_conv = self.isa.default_call_conv();

        // Declare runtime functions we need.
        let runtime_imports = runtime::RuntimeImports::declare(module, call_conv)?;

        // Signature: main() -> i32
        let mut sig = cl_ir::Signature::new(call_conv);
        sig.returns.push(AbiParam::new(cl_types::I32));

        let main_id = module
            .declare_function("main", Linkage::Export, &sig)
            .map_err(|e| AotError::Module(format!("declare main: {}", e)))?;

        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, main_id.as_u32()),
            sig,
        );

        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Call dtlv_rti_init() to get runtime handle.
        let init_ref = module.declare_func_in_func(runtime_imports.init, builder.func);
        let call_inst = builder.ins().call(init_ref, &[]);
        let rt_handle = builder.inst_results(call_inst)[0];

        // Call dtlv_rti_set_debug_mode(rt, Stderr=0).
        let set_debug_ref = module.declare_func_in_func(runtime_imports.set_debug_mode, builder.func);
        let stderr_mode = builder.ins().iconst(cl_types::I8, 0); // Stderr = 0
        builder.ins().call(set_debug_ref, &[rt_handle, stderr_mode]);

        // Call __script_body(rt).
        let body_ref = module.declare_func_in_func(body_func_id, builder.func);
        builder.ins().call(body_ref, &[rt_handle]);

        // Call dtlv_rti_shutdown(rt).
        let shutdown_ref = module.declare_func_in_func(runtime_imports.shutdown, builder.func);
        builder.ins().call(shutdown_ref, &[rt_handle]);

        // Return 0.
        let zero = builder.ins().iconst(cl_types::I32, 0);
        builder.ins().return_(&[zero]);

        builder.finalize(self.isa.frontend_config());

        // Define function in module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        module
            .define_function(main_id, &mut ctx)
            .map_err(|e| AotError::Codegen(format!("define main: {}", e)))?;

        Ok(())
    }

    /// Compile a single code unit into the module.
    fn compile_function(
        &mut self,
        module: &mut ObjectModule,
        func: &IrCodeUnit,
    ) -> Result<(), AotError> {
        let compiler = codegen::FunctionCompiler::new(func, self.isa.as_ref(), module);
        compiler.compile()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_creation() {
        let compiler = AotCompiler::new_for_host();
        assert!(compiler.is_ok());
    }
}
