//! Module-only worldfile analysis using the IR interpreter.
//!
//! This module provides test infrastructure for worldfiles that contain
//! only module sections (no script/scriptunit/expr sections). Tests execute
//! a nullary `main` function from the `local/test/main` module using the
//! IR-based interpreter.
//!
//! Functions defined in any module can call other functions from the same or
//! other modules via the `ScriptEnvironment`.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};

use crate::pipeline::{
    ModuleCompilationPipeline, CompilerOptions, TypecheckResult, LoweringResult,
};

/// Result of analyzing a module-only worldfile with IR interpreter.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModulesAnalysis {
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
pub fn analyze_modules_worldfile(
    db: &mut crate::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<ModulesAnalysis> {
    // Validate: only module and rider sections allowed.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::Rider { .. } => {}
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {
                bail!("module action sections not allowed in module-only worldfile (use memo tests)");
            }
            WorldfileSection::ScriptFragment { .. } => {
                bail!("scriptunit-fragment section not allowed in module-only worldfile");
            }
            WorldfileSection::ScriptExpr { .. } => {
                bail!("scriptunit-expr section not allowed in module-only worldfile");
            }
            WorldfileSection::InlineDirectives { .. } => {
                // Inline directives are for inlining tests only, ignore.
            }
        }
    }

    // Build pipeline from sections.
    let mut pipeline = ModuleCompilationPipeline::from_sections(
        db,
        &parsed.sections,
        CompilerOptions { keep_ir_dumps: true, ..CompilerOptions::default() },
    );

    // Verify local/test/main module exists.
    if !pipeline.contains_module("local", "test", "main") {
        bail!("missing local/test/main module");
    }

    // Compile modules (typecheck, drop analysis, lower).
    let (compiled, db) = pipeline.compile(db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        return Ok(ModulesAnalysis {
            typecheck: TypecheckResult::Error { errors: vec![err.clone()] },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // A module that did not parse has nothing to typecheck, and this is where
    // that has to be said: a parse diagnostic is not a typecheck error.
    let parse_errors = compiled.all_parse_errors();
    if !parse_errors.is_empty() {
        return Ok(ModulesAnalysis {
            typecheck: TypecheckResult::ParseError { errors: parse_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // Check for typecheck errors using consolidated helper.
    let all_typecheck_errors = compiled.all_typecheck_errors();
    if !all_typecheck_errors.is_empty() {
        return Ok(ModulesAnalysis {
            typecheck: TypecheckResult::Error { errors: all_typecheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // Check for lowering errors.
    let all_lowering_errors = compiled.all_lowering_errors();
    if !all_lowering_errors.is_empty() {
        return Ok(ModulesAnalysis {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Error { message: all_lowering_errors.join("\n") },
            output: String::new(),
        });
    }

    // Find main function.
    // First, find the ModuleId for "local/test/main" from the module graph.
    let main_salsa_module_id = compiled.shared.module_graph.iter_modules(db)
        .find(|m| m.id(db).path(db) == "local/test/main")
        .map(|m| m.id(db))
        .ok_or_else(|| anyhow!("main module not found"))?;

    // Look up the main function using (ModuleId<'db>, "main") key.
    let (main_ir_module_id, main_func_id) = compiled.shared.func_id_map
        .get(&(main_salsa_module_id, "main".S()))
        .ok_or_else(|| anyhow!("main function not found"))?;

    let main_code_unit = compiled.shared.module_registry.get_module_function_as_unit(*main_ir_module_id, datalove_datafun_ir::CodeUnitId(main_func_id.0))
        .ok_or_else(|| anyhow!("main function not in registry"))?
        .clone();

    // Verify main is a function and is nullary.
    let main_ctx = main_code_unit.function_context()
        .ok_or_else(|| anyhow!("main is not a function"))?;
    if !main_ctx.params.is_empty() {
        bail!("main function must have no parameters");
    }

    // Collect IR dump from all modules.
    let ir_dump: String = compiled.module_ir_dumps.values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    // Create IR interpreter and execute main.
    let mut interp = datalove_datafun_interp::IrInterpreter::new();
    let mut tydesc_table = datalove_datafun_interp::IrTyDescTable::new();

    // Get return type from the IR function.
    let ret_ir_type = main_code_unit.return_type()
        .ok_or_else(|| anyhow!("main function has no return type"))?;
    let ret_tydesc = tydesc_table.get_or_create(ret_ir_type);
    let ret_size = unsafe { (*ret_tydesc).size };

    // Allocate return buffer.
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = datalove_datafun_interp::Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    // Create a temporary ScriptEnvironment from the shared module registry.
    let mut env = datalove_datafun_interp::ScriptEnvironment::with_module_registry(
        std::sync::Arc::clone(&compiled.shared.module_registry)
    );
    let output = match interp.call_with_env(&main_code_unit, Vec::new(), ret_dest, &env) {
        Ok(()) => {
            // Pretty-print the return value.
            let value = datalove_datafun_interp::Value {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };
            let output_str = interp.pretty_print_value(&value)
                .unwrap_or_else(|e| format!("Error: {:?}", e));
            // Destroy the value to free any allocations.
            interp.destroy_value(&value);
            output_str
        }
        Err(e) => format!("Error: {:?}", e),
    };

    // Cleanup: destroy all values in frames to prevent memory leaks.
    env.destroy_live_values(interp.runtime_handle());

    Ok(ModulesAnalysis {
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    })
}
