use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;

fn type_to_json(db: &datalove_datalit::Database, ty: datalove_datalit::tycheck::TypeAndHeap) -> rmx::serde_json::Value {
    use datalove_datalit::tycheck::Type;

    let heap = ty.heap(db);
    let heap_str = match heap {
        datalove_datalit::ast::Heap::Local => "@",
        datalove_datalit::ast::Heap::Global => "#",
        datalove_datalit::ast::Heap::Omitted => "",
    };

    let ty_inner = ty.ty(db);

    match ty_inner {
        Type::Bool => json!(format!("{}bool", heap_str)),
        Type::U8 => json!(format!("{}u8", heap_str)),
        Type::I8 => json!(format!("{}i8", heap_str)),
        Type::U16 => json!(format!("{}u16", heap_str)),
        Type::I16 => json!(format!("{}i16", heap_str)),
        Type::U32 => json!(format!("{}u32", heap_str)),
        Type::I32 => json!(format!("{}i32", heap_str)),
        Type::U64 => json!(format!("{}u64", heap_str)),
        Type::I64 => json!(format!("{}i64", heap_str)),
        Type::F32 => json!(format!("{}f32", heap_str)),
        Type::F64 => json!(format!("{}f64", heap_str)),
        Type::Int => json!(format!("{}int", heap_str)),
        Type::String => json!(format!("{}string", heap_str)),
        Type::Data => json!(format!("{}data", heap_str)),
        Type::Error => json!(format!("{}error", heap_str)),

        Type::AnonTuple(t) => {
            let fields: Vec<_> = t.fields
                .iter()
                .map(|f| type_to_json(db, *f))
                .collect();
            json!({
                "kind": "AnonTuple",
                "heap": heap_str,
                "fields": fields
            })
        }

        Type::AnonStruct(s) => {
            let fields: Vec<_> = s.fields
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name.as_str(db),
                        "type": type_to_json(db, f.ty)
                    })
                })
                .collect();
            json!({
                "kind": "AnonStruct",
                "heap": heap_str,
                "fields": fields
            })
        }

        Type::AnonEnum(e) => {
            let variants: Vec<_> = e.variants
                .iter()
                .map(|v| {
                    let payload = v.payload.clone().map(|p| type_to_json(db, p));
                    json!({
                        "name": v.name.as_str(db),
                        "payload": payload
                    })
                })
                .collect();
            json!({
                "kind": "AnonEnum",
                "heap": heap_str,
                "variants": variants
            })
        }

        Type::List(l) => {
            json!({
                "kind": "List",
                "heap": heap_str,
                "element_type": type_to_json(db, l.element_type)
            })
        }

        Type::Map(m) => {
            json!({
                "kind": "Map",
                "heap": heap_str,
                "key_type": type_to_json(db, m.key_type),
                "value_type": type_to_json(db, m.value_type)
            })
        }

        Type::Set(s) => {
            json!({
                "kind": "Set",
                "heap": heap_str,
                "element_type": type_to_json(db, s.element_type)
            })
        }

        Type::Option(o) => {
            json!({
                "kind": "Option",
                "heap": heap_str,
                "inner_type": type_to_json(db, o.inner_type)
            })
        }

        Type::Result(r) => {
            json!({
                "kind": "Result",
                "heap": heap_str,
                "inner_type": type_to_json(db, r.inner_type)
            })
        }

        Type::Tensor(t) => {
            json!({
                "kind": "Tensor",
                "heap": heap_str,
                "element_type": type_to_json(db, t.element_type),
                "rank": t.rank
            })
        }

        Type::Table(t) => {
            let columns: Vec<_> = t.columns.iter().map(|f| {
                json!({
                    "name": f.name.as_str(db),
                    "type": type_to_json(db, f.ty)
                })
            }).collect();
            json!({
                "kind": "Table",
                "heap": heap_str,
                "columns": columns
            })
        }
    }
}

fn error_to_json(error: &datalove_datalit::tycheck::TypeError) -> rmx::serde_json::Value {
    use datalove_datalit::tycheck::TypeError;

    match error {
        TypeError::TypeMismatch { expected, actual } => {
            json!({
                "kind": "TypeMismatch",
                "expected": expected,
                "actual": actual
            })
        }
        TypeError::HeapMismatch { expected_heap, actual_heap } => {
            json!({
                "kind": "HeapMismatch",
                "expected_heap": expected_heap,
                "actual_heap": actual_heap
            })
        }
        TypeError::CannotSynthesize => {
            json!({
                "kind": "CannotSynthesize"
            })
        }
        TypeError::MissingField(name) => {
            json!({
                "kind": "MissingField",
                "field": name
            })
        }
        TypeError::ExtraField(name) => {
            json!({
                "kind": "ExtraField",
                "field": name
            })
        }
        TypeError::FieldOrderMismatch => {
            json!({
                "kind": "FieldOrderMismatch"
            })
        }
        TypeError::IntOutOfRange => {
            json!({
                "kind": "IntOutOfRange"
            })
        }
        TypeError::VariantNotFound(name) => {
            json!({
                "kind": "VariantNotFound",
                "variant": name
            })
        }
        TypeError::ArityMismatch { expected, actual } => {
            json!({
                "kind": "ArityMismatch",
                "expected": expected,
                "actual": actual
            })
        }
    }
}

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let ast = datalove_datalit::parser::parse_integration_test(&db, source);
    let resolved = datalove_datalit::resolve::resolve_names(&db, source, ast);
    let typechecked = datalove_datalit::tycheck::type_check(&db, ast, resolved);

    // Collect accumulated type diagnostics with spans.
    let type_diagnostics = datalove_datalit::tycheck::type_check::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, ast, resolved);
    let diagnostics: Vec<_> = type_diagnostics
        .iter()
        .map(|d| {
            let diag = d.to_diagnostic(&db);
            let code = diag.code.map(|c| c.as_str(&db).to_string());
            let labels: Vec<_> = diag.labels.iter().map(|label| {
                json!({
                    "span": [label.span.start, label.span.end],
                    "text": source_text[label.span.clone()].to_string(),
                    "message": label.message.map(|m| m.as_str(&db).to_string())
                })
            }).collect();
            json!({
                "code": code,
                "message": diag.message.as_str(&db),
                "labels": labels
            })
        })
        .collect();

    // Convert AST to serde format.
    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);

    // Convert root type to JSON-serializable format.
    let root_type_json = if let Some(root_type) = typechecked.root_type(&db) {
        Some(type_to_json(&db, root_type))
    } else {
        None
    };

    // Convert errors to JSON-serializable format.
    let errors: Vec<_> = typechecked.errors(&db)
        .iter()
        .map(|entry| {
            let error = entry.error(&db);
            error_to_json(&error)
        })
        .collect();

    let output = json!({
        "ast": serde_ast,
        "root_type": root_type_json,
        "errors": errors,
        "diagnostics": diagnostics
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("tycheck")
        .file_extension("dlt")
        .run();
}
