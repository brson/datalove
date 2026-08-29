use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;

fn type_to_json(db: &datalove_datalit::Database, ty: &datalove_datalit::tycheck::Type) -> rmx::serde_json::Value {
    use datalove_datalit::tycheck::Type;

    match ty {
        Type::Var(name) => json!(name.as_str(db)),
        Type::Bool => json!("bool"),
        Type::U8 => json!("u8"),
        Type::I8 => json!("i8"),
        Type::U16 => json!("u16"),
        Type::I16 => json!("i16"),
        Type::U32 => json!("u32"),
        Type::I32 => json!("i32"),
        Type::U64 => json!("u64"),
        Type::I64 => json!("i64"),
        Type::Index => json!("index"),
        Type::Offset => json!("offset"),
        Type::F32 => json!("f32"),
        Type::F64 => json!("f64"),
        Type::Int => json!("int"),
        Type::String => json!("string"),
        Type::Data => json!("data"),
        Type::Error => json!("error"),

        Type::AnonTuple(t) => {
            let fields: Vec<_> = t.fields
                .iter()
                .map(|f| type_to_json(db, f))
                .collect();
            json!({
                "kind": "AnonTuple",
                "fields": fields
            })
        }

        Type::AnonStruct(s) => {
            let fields: Vec<_> = s.fields
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name.as_str(db),
                        "type": type_to_json(db, &*f.ty)
                    })
                })
                .collect();
            json!({
                "kind": "AnonStruct",
                "fields": fields
            })
        }


        Type::List(l) => {
            json!({
                "kind": "List",
                "element_type": type_to_json(db, &*l.element_type)
            })
        }

        Type::Map(m) => {
            json!({
                "kind": "Map",
                "key_type": type_to_json(db, &*m.key_type),
                "value_type": type_to_json(db, &*m.value_type)
            })
        }

        Type::Set(s) => {
            json!({
                "kind": "Set",
                "element_type": type_to_json(db, &*s.element_type)
            })
        }

        Type::Option(o) => {
            json!({
                "kind": "Option",
                "inner_type": type_to_json(db, &*o.inner_type)
            })
        }

        Type::Result(r) => {
            json!({
                "kind": "Result",
                "inner_type": type_to_json(db, &*r.inner_type)
            })
        }

        Type::Tensor(t) => {
            json!({
                "kind": "Tensor",
                "element_type": type_to_json(db, &*t.element_type),
                "rank": t.rank
            })
        }

        Type::Table(t) => {
            let columns: Vec<_> = t.columns.iter().map(|f| {
                json!({
                    "name": f.name.as_str(db),
                    "type": type_to_json(db, &*f.ty)
                })
            }).collect();
            json!({
                "kind": "Table",
                "columns": columns
            })
        }

        Type::Atom(a) => {
            json!({
                "kind": "Atom",
                "name": a.name.as_str(db)
            })
        }

        Type::Term(t) => {
            json!({
                "kind": "Term",
                "name": t.name.as_str(db),
                "payload": type_to_json(db, &t.payload)
            })
        }

        Type::Enum(e) => {
            let variants: Vec<_> = e.variants.iter().map(|v| {
                json!({
                    "name": v.name.as_str(db),
                    "payload": v.payload.as_ref().map(|p| type_to_json(db, p))
                })
            }).collect();
            json!({
                "kind": "Enum",
                "variants": variants
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
    let root_type_json = if let Some(root_type) = typechecked.root_type(&db).clone() {
        Some(type_to_json(&db, &root_type))
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
