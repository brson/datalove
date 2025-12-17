use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;

fn type_hint_to_string(db: &dyn datalove_datafun_compiler::Db, type_hint: datalove_datalit::ast::TypeHintAndHeap) -> String {
    use datalove_datalit::ast::{TypeHint, Heap};

    let heap_prefix = match type_hint.heap(db) {
        Heap::Local => "@",
        Heap::Global => "#",
        Heap::Omitted => "",
    };

    let base_type = match type_hint.type_hint(db) {
        TypeHint::Bool => "bool",
        TypeHint::U8 => "u8",
        TypeHint::I8 => "i8",
        TypeHint::U16 => "u16",
        TypeHint::I16 => "i16",
        TypeHint::U32 => "u32",
        TypeHint::I32 => "i32",
        TypeHint::U64 => "u64",
        TypeHint::I64 => "i64",
        TypeHint::F32 => "f32",
        TypeHint::String => "string",
        TypeHint::Int => "int",
        TypeHint::Result(inner) => {
            let inner_str = type_hint_to_string(db, inner.inner_type(db));
            return format!("!{}", inner_str);
        }
        TypeHint::Option(inner) => {
            let inner_str = type_hint_to_string(db, inner.inner_type(db));
            return format!("?{}", inner_str);
        }
        TypeHint::List(inner) => {
            let inner_str = type_hint_to_string(db, inner.element_type(db));
            return format!("[{}]", inner_str);
        }
        TypeHint::Map(inner) => {
            let key_str = type_hint_to_string(db, inner.key_type(db));
            let val_str = type_hint_to_string(db, inner.value_type(db));
            return format!("{{{}: {}}}", key_str, val_str);
        }
        TypeHint::Set(inner) => {
            let inner_str = type_hint_to_string(db, inner.element_type(db));
            return format!("{{{}}}", inner_str);
        }
        TypeHint::AnonTuple(_) |
        TypeHint::AnonStruct(_) |
        TypeHint::AnonEnum(_) |
        TypeHint::Tensor(_) |
        TypeHint::Data |
        TypeHint::Error |
        TypeHint::ParseError(_) => {
            return "?".to_string();
        }
    };

    format!("{}{}", heap_prefix, base_type)
}

fn error_to_json(error: &datalove_datafun_compiler::tycheck::TypeError) -> rmx::serde_json::Value {
    use datalove_datafun_compiler::tycheck::TypeError;

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
        TypeError::InvalidTupleElement { ty } => {
            json!({
                "kind": "InvalidTupleElement",
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
        TypeError::TryOutsideFunction { operator } => {
            json!({
                "kind": "TryOutsideFunction",
                "operator": operator
            })
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
        TypeError::HeapMismatch { expected_heap, actual_heap } => {
            json!({
                "kind": "HeapMismatch",
                "expected_heap": expected_heap,
                "actual_heap": actual_heap
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
    }
}

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun_compiler::parser::parse_for_diagnostics(&db, source);
    let tycheck_result = datalove_datafun_compiler::tycheck::type_check(&db, source, script);

    // Collect type judgements for variables and functions.
    let mut judgements = Vec::new();
    for statement in script.statements(&db) {
        match statement {
            datalove_datafun_compiler::ast::Statement::Let(let_stmt) => {
                let name = let_stmt.name(&db);
                if let Some(ty) = datalove_datafun_compiler::tycheck::lookup_variable_type(&db, script, name) {
                    judgements.push(json!({
                        "kind": "variable",
                        "name": name.as_str(&db),
                        "type": datalove_datafun_compiler::tycheck::type_to_string(&db, ty.ty(&db))
                    }));
                }
            }
            datalove_datafun_compiler::ast::Statement::Fun(fun_stmt) => {
                let name = fun_stmt.name(&db);
                let params = fun_stmt.params(&db);
                let return_type = fun_stmt.return_type(&db);

                let param_types: Vec<_> = params.iter().map(|p| {
                    json!({
                        "name": p.name(&db).as_str(&db),
                        "type": type_hint_to_string(&db, p.type_hint(&db))
                    })
                }).collect();

                let ret_ty_str = if let Some(rt) = return_type {
                    type_hint_to_string(&db, rt)
                } else {
                    "?".to_string()
                };

                judgements.push(json!({
                    "kind": "function",
                    "name": name.as_str(&db),
                    "params": param_types,
                    "return_type": ret_ty_str
                }));
            }
            _ => {}
        }
    }

    // Convert errors to JSON-serializable format.
    let errors: Vec<_> = tycheck_result.errors(&db)
        .iter()
        .map(|entry| {
            let error = entry.error(&db);
            error_to_json(&error)
        })
        .collect();

    let output = json!({
        "judgements": judgements,
        "errors": errors
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("tycheck")
        .file_extension("dfs")
        .run();
}
