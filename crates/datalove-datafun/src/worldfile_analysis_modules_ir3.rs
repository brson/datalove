//! Module-only worldfile analysis using the IR interpreter (interp3).
//!
//! This module provides test infrastructure for worldfiles that contain
//! only module sections (no script/scriptunit/expr sections). Tests execute
//! a nullary `main` function from the `local/test/main` module using the
//! new IR-based interpreter.
//!
//! Functions defined in any module can call other functions from the same or
//! other modules via the `ScriptEnvironment`.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_compiler::ir;

use crate::pipeline::{
    ModuleCompilationPipeline, TypecheckResult, LoweringResult,
};

/// Result of analyzing a module-only worldfile with IR interpreter.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModulesIr3Analysis {
    /// Typecheck result for all modules.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value from calling main().
    pub output: String,
}

/// Analyze a module-only worldfile using the IR interpreter.
///
/// This function:
/// 1. Validates that there are no script/scriptunit/expr sections
/// 2. Builds a module graph from all module sections
/// 3. Typechecks all modules together (handles inter-module imports)
/// 4. Lowers all functions from all modules to IR
/// 5. Executes the `main` function (with access to all other functions)
/// 6. Returns the output value
pub fn analyze_modules_worldfile_ir3(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<ModulesIr3Analysis> {
    // Validate: only module sections allowed.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::ScriptFragment { .. } => {
                bail!("scriptunit-fragment section not allowed in module-only worldfile");
            }
            WorldfileSection::ScriptExpr { .. } => {
                bail!("scriptunit-expr section not allowed in module-only worldfile");
            }
        }
    }

    // Build pipeline and add modules.
    let mut pipeline = ModuleCompilationPipeline::new(db);
    pipeline.add_modules_from_sections(&parsed.sections);

    // Verify local/test/main module exists.
    let local_lib = pipeline.pkglib_local().get("test")
        .ok_or_else(|| anyhow!("missing local/test package"))?;
    if !local_lib.modules.contains_key("main") {
        bail!("missing local/test/main module");
    }

    // Compile modules (typecheck, drop analysis, lower).
    let mut compiled = pipeline.compile();

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Error { errors: vec![err.clone()] },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // Check for typecheck errors.
    let all_typecheck_errors: Vec<String> = compiled.path_to_errors.values()
        .flatten()
        .cloned()
        .collect();
    if !all_typecheck_errors.is_empty() {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Error { errors: all_typecheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // Check for drop analysis errors.
    let all_drop_errors: Vec<String> = compiled.drop_analysis_errors.values()
        .flatten()
        .cloned()
        .collect();
    if !all_drop_errors.is_empty() {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Error {
                message: format!("Drop analysis errors: {}", all_drop_errors.join("; ")),
            },
            output: String::new(),
        });
    }

    // Check for lowering errors.
    let lowering_errors: Vec<String> = compiled.module_lowering_results.values()
        .flatten()
        .filter(|s| s.starts_with("Error") || s.starts_with("Drop analysis error") || s.starts_with("Missing drop analysis"))
        .cloned()
        .collect();
    if !lowering_errors.is_empty() {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Error { message: lowering_errors.join("\n") },
            output: String::new(),
        });
    }

    // Find main function.
    let (main_module_id, main_func_id) = compiled.all_module_functions.get("main")
        .ok_or_else(|| anyhow!("main function not found"))?;

    let main_func = compiled.env.registry.get_module_function(*main_module_id, *main_func_id)
        .ok_or_else(|| anyhow!("main function not in registry"))?;

    // Verify main is nullary.
    if !main_func.params.is_empty() {
        bail!("main function must have no parameters");
    }

    // Collect IR dump from all modules.
    let ir_dump: String = compiled.module_lowering_results.values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    // Create IR interpreter and execute main.
    let mut interp = ir::interp::IrInterpreter::new();
    let mut tydesc_table = ir::interp::IrTyDescTable::new();

    // Infer return type from the IR function.
    let ret_ir_type = main_func.infer_return_type();
    let ret_tydesc = tydesc_table.get_or_create(&ret_ir_type);
    let ret_size = unsafe { (*ret_tydesc).size };

    // Allocate return buffer.
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = ir::interp::Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    // Execute main with the environment (so it can call other functions).
    let output = match interp.call_with_env(main_func, Vec::new(), ret_dest, &compiled.env) {
        Ok(()) => {
            // Pretty-print the return value.
            let value = ir::interp::Value {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };
            let output_str = interp.pretty_print_value(&value)
                .unwrap_or_else(|e| format!("Error: {:?}", e));
            // Destroy the value to free any allocations.
            let _ = interp.destroy_value(&value);
            output_str
        }
        Err(e) => format!("Error: {:?}", e),
    };

    // Cleanup: destroy all values in frames to prevent memory leaks.
    compiled.env.destroy_all(interp.runtime_handle());

    Ok(ModulesIr3Analysis {
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    })
}
