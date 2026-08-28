use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;
use std::collections::BTreeMap;
use datalove_datafun_resolve::{resolve_all_names, resolve_all_exports, build_all_function_ast_maps};
use datalove_datafun_tycheck::AutoAdaptMode;

fn diagnostic_to_json(db: &dyn datalove_datafun::Db, diag: &bct::diagnostic::Diagnostic) -> rmx::serde_json::Value {
    let code = diag.code.map(|c| c.as_str(db).to_string());
    let labels: Vec<_> = diag.labels.iter().map(|label| {
        let source_text = label.text.text(db);
        let text_slice = source_text.get(label.span.clone()).unwrap_or("");
        json!({
            "span": [label.span.start, label.span.end],
            "text": text_slice,
            "message": label.message.map(|m| m.as_str(db).to_string())
        })
    }).collect();
    json!({
        "code": code,
        "message": diag.message.as_str(db),
        "labels": labels
    })
}

fn error_to_json(error: &datalove_datafun_tycheck::TypeError) -> rmx::serde_json::Value {
    use datalove_datafun_tycheck::TypeError;

    match error {
        TypeError::TypeMismatch { expected, actual } => {
            json!({
                "kind": "TypeMismatch",
                "expected": expected,
                "actual": actual
            })
        }
        TypeError::UnresolvedName(name) => {
            json!({
                "kind": "UnresolvedName",
                "name": name
            })
        }
        TypeError::CannotSynthesize => {
            json!({
                "kind": "CannotSynthesize"
            })
        }
        TypeError::InvalidOperandType { op, ty } => {
            json!({
                "kind": "InvalidOperandType",
                "op": op,
                "type": ty
            })
        }
        TypeError::ArityMismatch { expected, actual } => {
            json!({
                "kind": "ArityMismatch",
                "expected": expected,
                "actual": actual
            })
        }
        TypeError::NotAFunction(name) => {
            json!({
                "kind": "NotAFunction",
                "name": name
            })
        }
        TypeError::DatalitError(msg) => {
            json!({
                "kind": "DatalitError",
                "message": msg
            })
        }
        TypeError::ResultRequiresErrorBinding => {
            json!("ResultRequiresErrorBinding")
        }
        TypeError::TryTypeMismatch { operator, actual_type } => {
            json!({
                "kind": "TryTypeMismatch",
                "operator": operator,
                "actual_type": actual_type
            })
        }
        TypeError::TryReturnTypeMismatch { operator, return_type } => {
            json!({
                "kind": "TryReturnTypeMismatch",
                "operator": operator,
                "return_type": return_type
            })
        }
        TypeError::IntOutOfRange => {
            json!({
                "kind": "IntOutOfRange"
            })
        }
        TypeError::MissingField(name) => {
            json!({
                "kind": "MissingField",
                "name": name
            })
        }
        TypeError::ExtraField(name) => {
            json!({
                "kind": "ExtraField",
                "name": name
            })
        }
        TypeError::FieldOrderMismatch => {
            json!({
                "kind": "FieldOrderMismatch"
            })
        }
        TypeError::VariantNotFound(name) => {
            json!({
                "kind": "VariantNotFound",
                "name": name
            })
        }
        TypeError::BreakOutsideLoop => {
            json!({
                "kind": "BreakOutsideLoop"
            })
        }
        TypeError::ContinueOutsideLoop => {
            json!({
                "kind": "ContinueOutsideLoop"
            })
        }
        TypeError::FieldIndexOutOfBounds { index, tuple_size } => {
            json!({
                "kind": "FieldIndexOutOfBounds",
                "index": index,
                "tuple_size": tuple_size
            })
        }
        TypeError::FieldNotFound { field_name, ty } => {
            json!({
                "kind": "FieldNotFound",
                "field_name": field_name,
                "type": ty
            })
        }
        TypeError::ProjectionOnNonAggregate { ty } => {
            json!({
                "kind": "ProjectionOnNonAggregate",
                "type": ty
            })
        }
        TypeError::NonCopyFieldProjection { field_ty } => {
            json!({
                "kind": "NonCopyFieldProjection",
                "field_type": field_ty
            })
        }
        TypeError::NonCopyIndexProjection { elem_ty } => {
            json!({
                "kind": "NonCopyIndexProjection",
                "element_type": elem_ty
            })
        }
        TypeError::ViewTypeMutBinding { view_ty } => {
            json!({
                "kind": "ViewTypeMutBinding",
                "view_type": view_ty
            })
        }
        TypeError::VoidFunctionReturnsValue => {
            json!({
                "kind": "VoidFunctionReturnsValue"
            })
        }
        TypeError::FunctionRequiresReturnValue => {
            json!({
                "kind": "FunctionRequiresReturnValue"
            })
        }
        TypeError::UndefinedVariable => {
            json!({
                "kind": "UndefinedVariable"
            })
        }
        TypeError::VariableNotMutable => {
            json!({
                "kind": "VariableNotMutable"
            })
        }
        TypeError::UnresolvedTypeAlias(name) => {
            json!({
                "kind": "UnresolvedTypeAlias",
                "name": name
            })
        }
        TypeError::DuplicateTypeAlias(name) => {
            json!({
                "kind": "DuplicateTypeAlias",
                "name": name
            })
        }
        TypeError::CannotShadowPrimitive(name) => {
            json!({
                "kind": "CannotShadowPrimitive",
                "name": name
            })
        }
        TypeError::NonConstInConstExpr(name) => {
            json!({
                "kind": "NonConstInConstExpr",
                "name": name
            })
        }
        TypeError::ComptimeArgNotConstBinding { param_idx, reason } => {
            json!({
                "kind": "ComptimeArgNotConstBinding",
                "param_idx": param_idx,
                "reason": reason
            })
        }
        TypeError::ArgumentModeMismatch { param_idx, expected, found } => {
            json!({
                "kind": "ArgumentModeMismatch",
                "param_idx": param_idx,
                "expected": expected,
                "found": found
            })
        }
    }
}

fn type_to_string(db: &dyn datalove_datafun::Db, ty: &datalove_datafun_tycheck::Type) -> String {
    use datalove_datafun_tycheck::Type;

    match ty {
        Type::Datalit(dt) => datalove_datalit::tycheck::type_to_string(db, dt),
        Type::Function(func) => {
            let params: Vec<_> = func.param_types(db).iter()
                .map(|p| type_to_string(db, p))
                .collect();
            let ret = type_to_string(db, &func.return_type(db));
            format!("({}) -> {}", params.join(", "), ret)
        }
    }
}

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun::Database::default();

    // Load the worldfile.
    let package_world_raw = datalove_datafun::package_load_worldfile::load_world_from_worldfile(source_text.as_bytes()).X();
    let package_world = datalove_datafun::package::import_from_loader(&db, package_world_raw);

    // Resolve imports.
    let resolution = datalove_datafun::package_resolve::resolve_package_world_with_imports(&db, package_world);
    let result = resolution.result(&db);

    // Check if import resolution failed.
    if let Err(e) = result {
        // Return error in JSON format.
        let output = json!({
            "modules": {},
            "resolution_error": format!("{:?}", e)
        });
        return Ok(rmx::serde_json::to_string_pretty(&output).X());
    }

    let pkg_graph = result.ok().X();

    // Convert to package-agnostic ModuleGraph, parse, and typecheck.
    let graph_with_requires = datalove_datafun::to_module_graph(&db, package_world, pkg_graph);
    let module_graph = graph_with_requires.graph;
    let parsed_graph = datalove_datafun::module_graph::parse_module_graph(&db, module_graph, graph_with_requires.resolved_requires, Vec::new());
    let all_names = resolve_all_names(&db, parsed_graph);
    let all_exports = resolve_all_exports(&db, parsed_graph);
    let all_function_asts = build_all_function_ast_maps(&db, parsed_graph);
    let typecheck_result = datalove_datafun_tycheck::typecheck_module_graph(&db, parsed_graph, all_names, all_exports, all_function_asts, AutoAdaptMode::Disabled);

    // Collect accumulated type diagnostics with spans.
    let type_diagnostics = datalove_datafun_tycheck::typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, parsed_graph, all_names, all_exports, all_function_asts, AutoAdaptMode::Disabled);
    let diagnostics: Vec<_> = type_diagnostics
        .iter()
        .map(|d| diagnostic_to_json(&db, &d.to_diagnostic(&db)))
        .collect();

    // Collect module information.
    let mut modules_output = BTreeMap::new();

    let graph = typecheck_result.graph(&db);
    let module_exports = typecheck_result.module_exports(&db);
    let module_errors = typecheck_result.module_errors(&db);

    // Get all modules from the graph and sort by path for consistent ordering.
    let mut module_ids: Vec<_> = graph.module_by_id(&db).keys().copied().collect();
    module_ids.sort_by_key(|id: &datalove_datafun::module_graph::ModuleId| id.path(&db).clone());

    for module_id in module_ids {
        // Extract just the module name from the path (e.g., "sys/std/u32" -> "u32").
        let path = module_id.path(&db);
        let module_name = path.split('/').last().unwrap_or(&path).to_string();

        // Get function exports.
        let mut functions = Vec::new();
        if let Some(exports) = module_exports.get(&module_id) {
            for (name, func_type) in exports.functions(&db) {
                let param_types: Vec<_> = func_type.param_types(&db).iter().map(|p| {
                    type_to_string(&db, p)
                }).collect();

                let return_type = type_to_string(&db, &func_type.return_type(&db));

                functions.push(json!({
                    "name": name.as_str(&db),
                    "params": param_types,
                    "return_type": return_type
                }));
            }
        }

        // Get errors.
        let errors: Vec<_> = module_errors.get(&module_id)
            .map(|errs| errs.iter().map(|e| error_to_json(e)).collect())
            .unwrap_or_else(Vec::new);

        modules_output.insert(module_name, json!({
            "functions": functions,
            "errors": errors
        }));
    }

    let output = json!({
        "modules": modules_output,
        "diagnostics": diagnostics
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("tycheck_world")
        .file_extension("world")
        .run();
}
