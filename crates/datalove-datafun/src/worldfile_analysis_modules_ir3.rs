//! Module-only worldfile analysis using the IR interpreter (interp3).
//!
//! This module provides test infrastructure for worldfiles that contain
//! only module sections (no script/scriptunit/expr sections). Tests execute
//! a nullary `main` function from the `local/test/main` module using the
//! new IR-based interpreter.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::ir;

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

/// Typecheck result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    Error {
        errors: Vec<String>,
    },
}

/// Lowering result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success {
        /// IR dump of the main function.
        ir: String,
    },
    Error {
        message: String,
    },
    Skipped,
}

/// Analyze a module-only worldfile using the IR interpreter.
///
/// This function:
/// 1. Validates that there are no script/scriptunit/expr sections
/// 2. Loads all module sections into a PackageWorld
/// 3. Typechecks the modules
/// 4. Lowers the `main` function to IR
/// 5. Executes the IR function
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

    // Extract modules to build PackageWorld.
    let mut pkglib_system = BTreeMap::new();
    let mut pkglib_local = BTreeMap::new();

    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            let library_map = match library.as_str() {
                "sys" => &mut pkglib_system,
                "local" => &mut pkglib_local,
                other => bail!("unknown library '{other}' (must be 'sys' or 'local')"),
            };

            let pkg = library_map.entry(package.C())
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

    // Verify local/test/main module exists.
    let local_lib = pkglib_local.get("test")
        .ok_or_else(|| anyhow!("missing local/test package"))?;
    if !local_lib.modules.contains_key("main") {
        bail!("missing local/test/main module");
    }

    // Get the main module's source for parsing.
    let main_source = local_lib.modules.get("main").unwrap().text.clone();

    // Parse the module source to get AST.
    let source = bct::input::Source::new(db, main_source);
    let parse_result = datalove_datafun_compiler::parser::parse(db, source);
    let script_ast = parse_result.script(db);

    // Typecheck the script to get expression types.
    let tycheck_result = datalove_datafun_compiler::tycheck::type_check(db, source, script_ast);

    // Check for typecheck errors.
    let tycheck_errors: Vec<_> = tycheck_result.errors(db).into_iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();
    if !tycheck_errors.is_empty() {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Error { errors: tycheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // Find the main function in the AST.
    let main_func = script_ast.statements(db)
        .iter()
        .filter_map(|stmt| {
            if let datalove_datafun_compiler::ast::Statement::Fun(func) = stmt {
                if func.name(db).text(db) == "main" {
                    return Some(*func);
                }
            }
            None
        })
        .next()
        .ok_or_else(|| anyhow!("main function not found in local/test/main module"))?;

    // Verify main is nullary.
    if !main_func.params(db).is_empty() {
        bail!("main function must have no parameters");
    }

    // Lower the main function to IR.
    let ir_func = match ir::lower::lower_function(db, tycheck_result, main_func) {
        Ok(f) => f,
        Err(e) => {
            return Ok(ModulesIr3Analysis {
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: format!("{}", e) },
                output: String::new(),
            });
        }
    };

    // Capture IR dump.
    let ir_dump = format!("{}", ir_func);

    // Get return type from AST.
    let ret_type_hint = main_func.return_type(db);
    if ret_type_hint.is_none() {
        bail!("main function must have a return type");
    }

    // Create IR interpreter and execute.
    let mut interp = ir::interp::IrInterpreter::new();
    let mut tydesc_table = ir::interp::IrTyDescTable::new();

    // Convert return type from AST TypeHint to IrType.
    let ret_ir_type = ir::IrType::from_type_hint(db, &ret_type_hint.unwrap());

    let ret_tydesc = tydesc_table.get_or_create(&ret_ir_type);
    let ret_size = unsafe { (*ret_tydesc).size };

    // Allocate return buffer.
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = ir::interp::Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    // Execute the function.
    let output = match interp.call(&ir_func, Vec::new(), ret_dest) {
        Ok(()) => {
            // Pretty-print the return value.
            let value = ir::interp::Value {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };
            pretty_print_value(&value)
        }
        Err(e) => format!("Error: {:?}", e),
    };

    Ok(ModulesIr3Analysis {
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    })
}

/// Pretty-print a value based on its type descriptor.
fn pretty_print_value(value: &ir::interp::Value) -> String {
    use datalove_rt::rtdt::{TyTag, TyInfoTuple};

    let tag = unsafe { (*value.tydesc).type_tag };
    match tag {
        TyTag::Tuple => {
            // Check if it's the unit type (0 fields).
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
