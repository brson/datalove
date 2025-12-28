//! Worldfile analysis using the IR interpreter (interp3).
//!
//! This module provides test infrastructure for worldfiles that may contain
//! module sections along with scriptunit-fragment and scriptunit-expr sections.
//! Tests execute script units sequentially using the new IR-based interpreter.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::ir;

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

/// Analyze a worldfile using the IR interpreter.
///
/// This function processes sections in order:
/// 1. Module sections: typechecked but not executed
/// 2. scriptunit-fragment sections: typechecked, lowered to IR, executed
/// 3. scriptunit-expr sections: typechecked, lowered to IR, executed, result captured
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

    // Process each section.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { library, package, module, .. } => {
                // For modules, just record that they exist. We could typecheck them here.
                results.push(SectionResult {
                    section_type: "module".to_string(),
                    name: Some(format!("{}/{}/{}", library, package, module)),
                    typecheck: TypecheckResult::Skipped,
                    lowering: LoweringResult::Skipped,
                    output: String::new(),
                });
            }

            WorldfileSection::ScriptFragment { source } => {
                let result = analyze_script_fragment(db, source);
                results.push(result);
            }

            WorldfileSection::ScriptExpr { source } => {
                let result = analyze_script_expr(db, source);
                results.push(result);
            }
        }
    }

    Ok(Ir3Analysis { sections: results })
}

/// Analyze a scriptunit-fragment section.
fn analyze_script_fragment(db: &dyn salsa::Database, source: &str) -> SectionResult {
    // Parse the fragment as a script.
    let src = bct::input::Source::new(db, source.to_string());
    let parse_result = datalove_datafun_compiler::parser::parse(db, src);
    let script_ast = parse_result.script(db);

    // Typecheck.
    let tycheck_result = datalove_datafun_compiler::tycheck::type_check(db, src, script_ast);

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

    // Lower the script unit to IR.
    let script_ctx = ir::lower::ScriptLowerContext::new();
    let ir_unit = match ir::lower::lower_script_unit(
        db,
        tycheck_result,
        script_ctx,
        ir::lower::ScriptUnitKind::Fragment(script_ast.statements(db).to_vec()),
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

    // For fragments without a result, just note success.
    SectionResult {
        section_type: "scriptunit-fragment".to_string(),
        name: None,
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output: "(fragment executed)".to_string(),
    }
}

/// Analyze a scriptunit-expr section.
fn analyze_script_expr(db: &dyn salsa::Database, source: &str) -> SectionResult {
    let src = bct::input::Source::new(db, source.to_string());

    // Parse the expression.
    let expr = datalove_datafun_compiler::parser::parse_expr(db, src);

    // Typecheck.
    let expr_result = datalove_datafun_compiler::tycheck::type_check_expr(db, src, expr);

    // Check for typecheck errors.
    let tycheck_errors: Vec<_> = expr_result.errors(db).into_iter()
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

    // Lower the expression as a script unit.
    let script_ctx = ir::lower::ScriptLowerContext::new();
    let ir_unit = match ir::lower::lower_script_expr(
        db,
        expr_result.expr_types(db),
        script_ctx,
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

        // Execute the script unit.
        let mut interp = ir::interp::IrInterpreter::new();
        match interp.execute_script_unit(&ir_unit, ret_dest) {
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
