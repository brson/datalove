//! AOT compilation backend using Cranelift.
//!
//! Compiles datafun IR directly to native code via Cranelift,
//! producing object files for linking.

pub mod types;
pub mod layout;

use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_object::{ObjectBuilder, ObjectModule, ObjectProduct};
use target_lexicon::Triple;

use datalove_datafun_ir::{IrFunction, IrModule, IrScriptUnit};

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
    pub fn compile_script_unit(&mut self, _unit: &IrScriptUnit) -> Result<ObjectProduct, AotError> {
        Err(AotError::Unsupported("script unit compilation not yet implemented".into()))
    }

    /// Compile a single function into the module.
    fn compile_function(
        &mut self,
        _module: &mut ObjectModule,
        _func: &IrFunction,
    ) -> Result<(), AotError> {
        // TODO: Implement function compilation in phase 2.
        Err(AotError::Unsupported("function compilation not yet implemented".into()))
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
