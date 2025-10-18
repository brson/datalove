use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;
use std::collections::BTreeMap;

fn error_to_json(error: &datalove_datafun::tycheck::TypeError) -> rmx::serde_json::Value {
    use datalove_datafun::tycheck::TypeError;

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
    }
}

fn typeandheap_to_string(db: &dyn datalove_datafun::Db, tah: datalove_datafun::tycheck::TypeAndHeap) -> String {
    use datalove_datafun::tycheck::Type;
    use datalove_datalit::ast::Heap;

    let heap_prefix = match tah.heap(db) {
        Heap::Local => "@",
        Heap::Global => "#",
        Heap::Omitted => "",
    };

    let type_str = match tah.ty(db) {
        Type::Datalit(dt) => datalove_datalit::tycheck::type_to_string(db, dt),
        Type::Function(func) => {
            let params: Vec<_> = func.param_types(db).iter()
                .map(|p| typeandheap_to_string(db, *p))
                .collect();
            let ret = typeandheap_to_string(db, func.return_type(db));
            format!("({}) -> {}", params.join(", "), ret)
        }
        Type::Void => "void".to_string(),
    };

    if matches!(tah.ty(db), Type::Void) || matches!(tah.ty(db), Type::Function(_)) {
        type_str
    } else {
        format!("{}{}", heap_prefix, type_str)
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

    let graph = result.ok().X();

    // Typecheck the package world.
    let typecheck_result = datalove_datafun::tycheck::typecheck_package_world(&db, graph);

    // Collect module information.
    let mut modules_output = BTreeMap::new();

    let module_exports = typecheck_result.module_exports(&db);
    let module_errors = typecheck_result.module_errors(&db);

    // Get all modules from the graph and sort by name for consistent ordering.
    let mut modules: Vec<_> = graph.map(&db).keys().copied().collect();
    modules.sort_by_key(|m| m.name(&db).to_string());

    for module in modules {
        let module_name = module.name(&db).to_string();

        // Get function exports.
        let mut functions = Vec::new();
        if let Some(exports) = module_exports.get(&module) {
            for (name, func_type) in exports.functions(&db) {
                let param_types: Vec<_> = func_type.param_types(&db).iter().map(|p| {
                    typeandheap_to_string(&db, *p)
                }).collect();

                let return_type = typeandheap_to_string(&db, func_type.return_type(&db));

                functions.push(json!({
                    "name": name.as_str(&db),
                    "params": param_types,
                    "return_type": return_type
                }));
            }
        }

        // Get errors.
        let errors: Vec<_> = module_errors.get(&module)
            .map(|errs| errs.iter().map(|e| error_to_json(e)).collect())
            .unwrap_or_else(Vec::new);

        modules_output.insert(module_name, json!({
            "functions": functions,
            "errors": errors
        }));
    }

    let output = json!({
        "modules": modules_output
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("tycheck_world")
        .file_extension("world")
        .run();
}
