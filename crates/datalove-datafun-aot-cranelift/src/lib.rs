//! AOT compilation backend using Cranelift.
//!
//! Compiles datafun IR directly to native code via Cranelift,
//! producing object files for linking.

pub mod codegen;
pub mod layout;
pub mod runtime;
pub mod tydesc_emit;
pub mod types;

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, AbiParam, InstBuilder};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule, ObjectProduct};
use target_lexicon::Triple;

use datalove_datafun_ir::{
    IrBlock, IrFunction, IrModule, IrScriptUnit, IrType, Terminator,
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

/// AOT compiler context.
///
/// Holds Cranelift state for compiling multiple functions.
pub struct AotCompiler {
    /// Target ISA for code generation.
    isa: std::sync::Arc<dyn TargetIsa>,
    /// Type descriptor table for layout computation.
    #[allow(dead_code)]
    tydesc_table: layout::TyDescTable,
}

impl AotCompiler {
    /// Create a new AOT compiler for the host target.
    pub fn new_for_host() -> Result<Self, AotError> {
        let builder = cranelift_codegen::isa::lookup(Triple::host())
            .map_err(|e| AotError::Codegen(format!("unsupported target: {}", e)))?;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed")
            .map_err(|e| AotError::Codegen(format!("settings error: {}", e)))?;

        let flags = settings::Flags::new(settings_builder);
        let isa = builder.finish(flags)
            .map_err(|e| AotError::Codegen(format!("isa error: {}", e)))?;

        Ok(Self {
            isa,
            tydesc_table: layout::TyDescTable::new(),
        })
    }

    /// Create a new AOT compiler for a specific target triple.
    pub fn new_for_target(triple: Triple) -> Result<Self, AotError> {
        let builder = cranelift_codegen::isa::lookup(triple)
            .map_err(|e| AotError::Codegen(format!("unsupported target: {}", e)))?;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed")
            .map_err(|e| AotError::Codegen(format!("settings error: {}", e)))?;

        let flags = settings::Flags::new(settings_builder);
        let isa = builder.finish(flags)
            .map_err(|e| AotError::Codegen(format!("isa error: {}", e)))?;

        Ok(Self {
            isa,
            tydesc_table: layout::TyDescTable::new(),
        })
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
    pub fn compile_script_unit(&mut self, unit: &IrScriptUnit) -> Result<ObjectProduct, AotError> {
        let types = tydesc_emit::collect_types_from_script_unit(unit);
        self.compile_script_unit_with_types(unit, types)
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
        unit: &IrScriptUnit,
        world_funcs: impl Iterator<Item = &'a IrFunction>,
    ) -> Result<ObjectProduct, AotError> {
        // Collect types from world functions and the script unit.
        let mut types = tydesc_emit::collect_types_from_script_unit(unit);
        tydesc_emit::collect_types_from_functions(world_funcs, &mut types);
        self.compile_script_unit_with_types(unit, types)
    }

    /// Compile an IR script unit with pre-collected types.
    fn compile_script_unit_with_types(
        &mut self,
        unit: &IrScriptUnit,
        types: std::collections::HashSet<IrType>,
    ) -> Result<ObjectProduct, AotError> {
        let obj_builder = ObjectBuilder::new(
            self.isa.clone(),
            "script",
            cranelift_module::default_libcall_names(),
        ).map_err(|e| AotError::Module(format!("object builder error: {}", e)))?;

        let mut obj_module = ObjectModule::new(obj_builder);

        // Declare runtime imports.
        let call_conv = self.isa.default_call_conv();
        let runtime = runtime::RuntimeImports::declare(&mut obj_module, call_conv)?;

        // Emit all TyDescs upfront (whole-world compilation).
        let mut tydesc_emitter = tydesc_emit::TyDescEmitter::new();
        tydesc_emitter.emit_all(&mut obj_module, types)?;

        // Convert script unit to a function with rt_handle as first param.
        let body_func = self.script_unit_to_function(unit);

        // Compile the body function with pre-populated TyDesc cache.
        let compiler = codegen::FunctionCompiler::new_with_runtime_and_tydescs(
            &body_func,
            self.isa.as_ref(),
            &mut obj_module,
            runtime,
            tydesc_emitter,
        );
        let body_func_id = compiler.compile()?;

        // Generate the entry point.
        self.compile_entry_point(&mut obj_module, body_func_id)?;

        Ok(obj_module.finish())
    }

    /// Convert an IrScriptUnit to an IrFunction for compilation.
    fn script_unit_to_function(&self, unit: &IrScriptUnit) -> IrFunction {
        // Convert UnitEnd terminators to Return.
        // Note: rt_handle is implicit - codegen adds it to all function signatures.
        let blocks: Vec<IrBlock> = unit.blocks.iter().map(|block| {
            let terminator = match &block.terminator {
                Terminator::UnitEnd { result: _ } => {
                    // Convert to Return with no value (script body doesn't return).
                    Terminator::Return { value: None }
                }
                Terminator::UnitEarlyReturn { value: _ } => {
                    Terminator::Return { value: None }
                }
                other => other.clone(),
            };
            IrBlock {
                id: block.id,
                instructions: block.instructions.clone(),
                terminator,
            }
        }).collect();

        IrFunction {
            id: datalove_datafun_ir::FuncId(0),
            name: "__script_body".to_string(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            blocks,
            value_count: unit.value_count,
            slot_count: unit.slot_count,
            value_types: unit.value_types.clone(),
            slot_types: unit.slot_types.clone(),
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
        let runtime = runtime::RuntimeImports::declare(module, call_conv)?;

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
        let init_ref = module.declare_func_in_func(runtime.init, builder.func);
        let call_inst = builder.ins().call(init_ref, &[]);
        let rt_handle = builder.inst_results(call_inst)[0];

        // Call dtlv_rti_set_debug_mode(rt, Stderr=0).
        let set_debug_ref = module.declare_func_in_func(runtime.set_debug_mode, builder.func);
        let stderr_mode = builder.ins().iconst(cl_types::I8, 0); // Stderr = 0
        builder.ins().call(set_debug_ref, &[rt_handle, stderr_mode]);

        // Call __script_body(rt).
        let body_ref = module.declare_func_in_func(body_func_id, builder.func);
        builder.ins().call(body_ref, &[rt_handle]);

        // Call dtlv_rti_shutdown(rt).
        let shutdown_ref = module.declare_func_in_func(runtime.shutdown, builder.func);
        builder.ins().call(shutdown_ref, &[rt_handle]);

        // Return 0.
        let zero = builder.ins().iconst(cl_types::I32, 0);
        builder.ins().return_(&[zero]);

        builder.finalize();

        // Define function in module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        module
            .define_function(main_id, &mut ctx)
            .map_err(|e| AotError::Codegen(format!("define main: {}", e)))?;

        Ok(())
    }

    /// Compile a single function into the module.
    fn compile_function(
        &mut self,
        module: &mut ObjectModule,
        func: &IrFunction,
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
