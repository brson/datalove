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
        Type::Int => json!(format!("{}int", heap_str)),
        Type::String => json!(format!("{}string", heap_str)),
        Type::Data => json!(format!("{}data", heap_str)),
        Type::Error => json!(format!("{}error", heap_str)),

        Type::AnonTuple(t) => {
            let fields: Vec<_> = t.fields(db)
                .iter()
                .map(|f| type_to_json(db, *f))
                .collect();
            json!({
                "kind": "AnonTuple",
                "heap": heap_str,
                "fields": fields
            })
        }

        Type::NamedTuple(t) => {
            let name = t.name(db).as_str(db);
            let fields: Vec<_> = t.fields(db)
                .iter()
                .map(|f| type_to_json(db, *f))
                .collect();
            json!({
                "kind": "NamedTuple",
                "heap": heap_str,
                "name": name,
                "fields": fields
            })
        }

        Type::AnonStruct(s) => {
            let fields: Vec<_> = s.fields(db)
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name(db).as_str(db),
                        "type": type_to_json(db, f.ty(db))
                    })
                })
                .collect();
            json!({
                "kind": "AnonStruct",
                "heap": heap_str,
                "fields": fields
            })
        }

        Type::NamedStruct(s) => {
            let name = s.name(db).as_str(db);
            let fields: Vec<_> = s.fields(db)
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name(db).as_str(db),
                        "type": type_to_json(db, f.ty(db))
                    })
                })
                .collect();
            json!({
                "kind": "NamedStruct",
                "heap": heap_str,
                "name": name,
                "fields": fields
            })
        }

        Type::AnonEnum(e) => {
            let variants: Vec<_> = e.variants(db)
                .iter()
                .map(|v| {
                    let payload = v.payload(db).map(|p| type_to_json(db, p));
                    json!({
                        "name": v.name(db).as_str(db),
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

        Type::NamedEnum(e) => {
            let name = e.name(db).as_str(db);
            let variants: Vec<_> = e.variants(db)
                .iter()
                .map(|v| {
                    let payload = v.payload(db).map(|p| type_to_json(db, p));
                    json!({
                        "name": v.name(db).as_str(db),
                        "payload": payload
                    })
                })
                .collect();
            json!({
                "kind": "NamedEnum",
                "heap": heap_str,
                "name": name,
                "variants": variants
            })
        }

        Type::List(l) => {
            json!({
                "kind": "List",
                "heap": heap_str,
                "element_type": type_to_json(db, l.element_type(db))
            })
        }

        Type::Map(m) => {
            json!({
                "kind": "Map",
                "heap": heap_str,
                "key_type": type_to_json(db, m.key_type(db)),
                "value_type": type_to_json(db, m.value_type(db))
            })
        }

        Type::Set(s) => {
            json!({
                "kind": "Set",
                "heap": heap_str,
                "element_type": type_to_json(db, s.element_type(db))
            })
        }

        Type::Option(o) => {
            json!({
                "kind": "Option",
                "heap": heap_str,
                "inner_type": type_to_json(db, o.inner_type(db))
            })
        }

        Type::Result(r) => {
            json!({
                "kind": "Result",
                "heap": heap_str,
                "inner_type": type_to_json(db, r.inner_type(db))
            })
        }

        Type::Tensor(t) => {
            json!({
                "kind": "Tensor",
                "heap": heap_str,
                "element_type": type_to_json(db, t.element_type(db)),
                "rank": t.rank(db)
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
        TypeError::UnresolvedName(name) => {
            json!({
                "kind": "UnresolvedName",
                "name": name
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
    let resolved = datalove_datalit::resolve::resolve_names(&db, ast);
    let typechecked = datalove_datalit::tycheck::type_check(&db, ast, resolved);

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
        "errors": errors
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("tycheck")
        .file_extension("dlt")
        .run();
}
