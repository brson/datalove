//! Worldfile analysis using the IR interpreter (interp3).
//!
//! This module provides test infrastructure for worldfiles that may contain
//! module sections along with scriptunit-fragment and scriptunit-expr sections.
//! Tests execute script units sequentially using the new IR-based interpreter.
//!
//! Script units are executed with a shared environment, allowing later units
//! to reference values, slots, and functions from earlier units.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::ir;
use datalove_datafun_compiler::tycheck::{
    ScriptUnitInput, ScriptUnitKind, ScriptUnitBatch, type_check_script_units,
    UnitTypecheckResultTracked,
};
use ir::interp::ScriptEnvironment;

/// Result of analyzing a worldfile with IR interpreter.
#[derive(Debug, Serialize, Deserialize)]
pub struct Ir3Analysis {
    /// Per-section results.
    pub sections: Vec<SectionResult>,
}

/// Result of analyzing one section.
#[derive(Debug, Serialize, Deserialize)]
pub struct SectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    pub name: Option<String>,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value (for expression units) or function call result.
    pub output: String,
}

/// Typecheck result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    Error {
        errors: Vec<String>,
    },
    Skipped,
}

/// Lowering result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success {
        /// IR dump.
        ir: String,
    },
    Error {
        message: String,
    },
    Skipped,
}

/// Parsed script unit info for processing.
struct ParsedUnit<'db> {
    #[allow(dead_code)]
    source: bct::input::Source,
    kind: ParsedUnitKind<'db>,
}

enum ParsedUnitKind<'db> {
    Fragment(datalove_datafun_compiler::ast::Script<'db>),
    Expr(datalove_datafun_compiler::ast::ExprFun<'db>),
}

/// Analyze a worldfile using the IR interpreter.
///
/// This function processes sections in order:
/// 1. Module sections: typechecked but not executed
/// 2. scriptunit-fragment sections: typechecked, lowered to IR, executed
/// 3. scriptunit-expr sections: typechecked, lowered to IR, executed, result captured
///
/// Script units share a `ScriptLowerContext` (for cross-unit name resolution during lowering)
/// and a `ScriptEnvironment` (for cross-unit value/function access during execution).
pub fn analyze_worldfile_ir3(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Ir3Analysis> {
    let mut results = Vec::new();

    // First pass: collect all modules for context.
    let mut pkglib_local = BTreeMap::<String, Package>::new();

    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            if library != "local" {
                continue;  // Skip non-local modules for now.
            }

            let pkg = pkglib_local.entry(package.C())
                .or_insert_with(|| Package {
                    name: package.C(),
                    modules: BTreeMap::new(),
                });

            let module_path_str = format!("{}/{}/{}", library, package, module);

            let pkg_module = PackageModule {
                name: module.C(),
                path: module_path_str.C().into(),
                text: source.C(),
            };

            pkg.modules.insert(module.C(), pkg_module);
        }
    }

    // Collect and parse all script units.
    let mut parsed_units: Vec<ParsedUnit> = Vec::new();
    let mut unit_inputs: Vec<ScriptUnitInput> = Vec::new();

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Modules are recorded separately.
            }
            WorldfileSection::ScriptFragment { source } => {
                let src = bct::input::Source::new(db, source.to_string());
                let parse_result = datalove_datafun_compiler::parser::parse(db, src);
                let script = parse_result.script(db);
                parsed_units.push(ParsedUnit {
                    source: src,
                    kind: ParsedUnitKind::Fragment(script),
                });
                unit_inputs.push(ScriptUnitInput::new(db, src, ScriptUnitKind::Fragment(script)));
            }
            WorldfileSection::ScriptExpr { source } => {
                let src = bct::input::Source::new(db, source.to_string());
                let expr = datalove_datafun_compiler::parser::parse_expr(db, src);
                parsed_units.push(ParsedUnit {
                    source: src,
                    kind: ParsedUnitKind::Expr(expr),
                });
                unit_inputs.push(ScriptUnitInput::new(db, src, ScriptUnitKind::Expr(expr)));
            }
        }
    }

    // Typecheck all units together (bindings shared across units).
    let batch = ScriptUnitBatch::new(db, unit_inputs);
    let typecheck_results = type_check_script_units(db, batch);
    let unit_results = typecheck_results.results(db);

    // Shared state for lowering and execution.
    let mut script_ctx = ir::lower::ScriptLowerContext::new();
    let mut env = ScriptEnvironment::new();
    let mut interp = ir::interp::IrInterpreter::new();

    // Process each section, using the pre-computed typecheck results.
    let mut unit_idx = 0;
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { library, package, module, .. } => {
                // For modules, just record that they exist.
                results.push(SectionResult {
                    section_type: "module".to_string(),
                    name: Some(format!("{}/{}/{}", library, package, module)),
                    typecheck: TypecheckResult::Skipped,
                    lowering: LoweringResult::Skipped,
                    output: String::new(),
                });
            }

            WorldfileSection::ScriptFragment { .. } => {
                let parsed_unit = &parsed_units[unit_idx];
                let tycheck_result = unit_results[unit_idx];
                unit_idx += 1;

                let result = process_fragment(
                    db,
                    parsed_unit,
                    tycheck_result,
                    &mut script_ctx,
                    &mut env,
                    &mut interp,
                );
                results.push(result);
            }

            WorldfileSection::ScriptExpr { .. } => {
                let parsed_unit = &parsed_units[unit_idx];
                let tycheck_result = unit_results[unit_idx];
                unit_idx += 1;

                let result = process_expr(
                    db,
                    parsed_unit,
                    tycheck_result,
                    &mut script_ctx,
                    &mut env,
                    &mut interp,
                );
                results.push(result);
            }
        }
    }

    Ok(Ir3Analysis { sections: results })
}

/// Process a script fragment unit.
fn process_fragment<'db>(
    db: &'db dyn salsa::Database,
    parsed_unit: &ParsedUnit<'db>,
    tycheck_result: UnitTypecheckResultTracked<'db>,
    script_ctx: &mut ir::lower::ScriptLowerContext,
    env: &mut ScriptEnvironment,
    interp: &mut ir::interp::IrInterpreter,
) -> SectionResult {
    // Check for typecheck errors.
    let tycheck_errors: Vec<_> = tycheck_result.errors(db).into_iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();
    if !tycheck_errors.is_empty() {
        return SectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: tycheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        };
    }

    let script = match &parsed_unit.kind {
        ParsedUnitKind::Fragment(s) => *s,
        _ => unreachable!(),
    };

    // Lower using the typecheck result's expr_types.
    let ir_unit = match ir::lower::lower_script_fragment_raw(
        db,
        tycheck_result.expr_types(db),
        script_ctx.clone(),
        script.statements(db).to_vec(),
    ) {
        Ok(unit) => unit,
        Err(e) => {
            return SectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: format!("{}", e) },
                output: String::new(),
            };
        }
    };

    // Format IR dump.
    let ir_dump = format!("{}", ir_unit);

    // Execute the fragment with shared environment.
    let mut tydesc_table = ir::interp::IrTyDescTable::new();
    let unit_tydesc = tydesc_table.get_or_create(&ir::IrType::Unit);
    let mut dummy_buffer = [0u8; 0];
    let ret_dest = ir::interp::Destination {
        ptr: dummy_buffer.as_mut_ptr(),
        tydesc: unit_tydesc,
    };

    let output = match interp.execute_script_unit_in_env(&ir_unit, env, ret_dest) {
        Ok(()) => "(fragment executed)".to_string(),
        Err(e) => format!("Error: {:?}", e),
    };

    // Update script context with exports from this unit for subsequent lowering.
    let unit_index = script_ctx.current_unit;
    script_ctx.add_exports(unit_index, &ir_unit.exports);
    script_ctx.current_unit += 1;

    SectionResult {
        section_type: "scriptunit-fragment".to_string(),
        name: None,
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    }
}

/// Process a script expression unit.
fn process_expr<'db>(
    db: &'db dyn salsa::Database,
    parsed_unit: &ParsedUnit<'db>,
    tycheck_result: UnitTypecheckResultTracked<'db>,
    script_ctx: &mut ir::lower::ScriptLowerContext,
    env: &mut ScriptEnvironment,
    interp: &mut ir::interp::IrInterpreter,
) -> SectionResult {
    // Check for typecheck errors.
    let tycheck_errors: Vec<_> = tycheck_result.errors(db).into_iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();
    if !tycheck_errors.is_empty() {
        return SectionResult {
            section_type: "scriptunit-expr".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: tycheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        };
    }

    let expr = match &parsed_unit.kind {
        ParsedUnitKind::Expr(e) => *e,
        _ => unreachable!(),
    };

    // Lower the expression as a script unit.
    let ir_unit = match ir::lower::lower_script_expr(
        db,
        tycheck_result.expr_types(db),
        script_ctx.clone(),
        expr,
    ) {
        Ok(unit) => unit,
        Err(e) => {
            return SectionResult {
                section_type: "scriptunit-expr".to_string(),
                name: None,
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: format!("{}", e) },
                output: String::new(),
            };
        }
    };

    let ir_dump = format!("{}", ir_unit);

    // Execute the script unit if it has a result.
    let output = if let Some(result_id) = ir_unit.result {
        // Get result type from the unit's value_types.
        let result_type = &ir_unit.value_types[result_id.0 as usize];

        // Create tydesc table and get result tydesc.
        let mut tydesc_table = ir::interp::IrTyDescTable::new();
        let ret_tydesc = tydesc_table.get_or_create(result_type);
        let ret_size = unsafe { (*ret_tydesc).size };

        // Allocate return buffer.
        let mut ret_buffer = vec![0u8; ret_size as usize];
        let ret_dest = ir::interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // Execute the script unit with shared environment.
        match interp.execute_script_unit_in_env(&ir_unit, env, ret_dest) {
            Ok(()) => {
                let value = ir::interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                pretty_print_value(&value)
            }
            Err(e) => format!("Error: {:?}", e),
        }
    } else {
        "(fragment executed)".to_string()
    };

    // Update script context with exports from this unit.
    let unit_index = script_ctx.current_unit;
    script_ctx.add_exports(unit_index, &ir_unit.exports);
    script_ctx.current_unit += 1;

    SectionResult {
        section_type: "scriptunit-expr".to_string(),
        name: None,
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    }
}

/// Pretty-print a value based on its type descriptor.
fn pretty_print_value(value: &ir::interp::Value) -> String {
    use datalove_rt::rtdt::{TyTag, TyInfoTuple};

    let tag = unsafe { (*value.tydesc).type_tag };
    match tag {
        TyTag::Tuple => {
            let tuple_info: TyInfoTuple = unsafe { (*value.tydesc).type_info.tuple };
            if tuple_info.num_fields == 0 {
                "()".to_string()
            } else {
                format!("<tuple:{}>", tuple_info.num_fields)
            }
        }
        TyTag::Bool => {
            let v = unsafe { *(value.ptr as *const bool) };
            if v { "true".to_string() } else { "false".to_string() }
        }
        TyTag::U8 => {
            let v = unsafe { *(value.ptr as *const u8) };
            format!("@{}", v)
        }
        TyTag::U16 => {
            let v = unsafe { *(value.ptr as *const u16) };
            format!("@{}", v)
        }
        TyTag::U32 => {
            let v = unsafe { *(value.ptr as *const u32) };
            format!("@{}", v)
        }
        TyTag::U64 => {
            let v = unsafe { *(value.ptr as *const u64) };
            format!("@{}", v)
        }
        TyTag::I8 => {
            let v = unsafe { *(value.ptr as *const i8) };
            format!("{}", v)
        }
        TyTag::I16 => {
            let v = unsafe { *(value.ptr as *const i16) };
            format!("{}", v)
        }
        TyTag::I32 => {
            let v = unsafe { *(value.ptr as *const i32) };
            format!("{}", v)
        }
        TyTag::I64 => {
            let v = unsafe { *(value.ptr as *const i64) };
            format!("{}", v)
        }
        TyTag::F32 => {
            let v = unsafe { *(value.ptr as *const f32) };
            format!("{}", v)
        }
        TyTag::F64 => {
            let v = unsafe { *(value.ptr as *const f64) };
            format!("{}", v)
        }
        _ => format!("<{:?}>", tag),
    }
}
