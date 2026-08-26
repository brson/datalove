use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;

fn type_hint_to_string(db: &dyn salsa::Database, type_hint: &datalove_datalit::ast::TypeHint) -> String {
    use datalove_datalit::ast::TypeHint;

    // Format type for display.
    match type_hint {
        TypeHint::Bool => "bool".to_string(),
        TypeHint::U8 => "u8".to_string(),
        TypeHint::I8 => "i8".to_string(),
        TypeHint::U16 => "u16".to_string(),
        TypeHint::I16 => "i16".to_string(),
        TypeHint::U32 => "u32".to_string(),
        TypeHint::I32 => "i32".to_string(),
        TypeHint::U64 => "u64".to_string(),
        TypeHint::I64 => "i64".to_string(),
        TypeHint::Index => "index".to_string(),
        TypeHint::Offset => "offset".to_string(),
        TypeHint::F32 => "f32".to_string(),
        TypeHint::F64 => "f64".to_string(),
        TypeHint::String => "string".to_string(),
        TypeHint::Int => "int".to_string(),
        TypeHint::Result(inner) => {
            let inner_str = type_hint_to_string(db, &inner.inner_type);
            format!("!{}", inner_str)
        }
        TypeHint::Option(inner) => {
            let inner_str = type_hint_to_string(db, &inner.inner_type);
            format!("?{}", inner_str)
        }
        TypeHint::List(inner) => {
            let inner_str = type_hint_to_string(db, &inner.element_type);
            format!("[{}]", inner_str)
        }
        TypeHint::Map(inner) => {
            let key_str = type_hint_to_string(db, &inner.key_type);
            let val_str = type_hint_to_string(db, &inner.value_type);
            format!("{{{}: {}}}", key_str, val_str)
        }
        TypeHint::Set(inner) => {
            let inner_str = type_hint_to_string(db, &inner.element_type);
            format!("{{{}}}", inner_str)
        }
        TypeHint::AnonTuple(tuple) => {
            let fields: Vec<_> = tuple.fields.iter()
                .map(|f| type_hint_to_string(db, f))
                .collect();
            format!("({})", fields.join(", "))
        }
        TypeHint::AnonStruct(s) => {
            let fields: Vec<_> = s.fields.iter()
                .map(|f| format!("{}: {}", f.name.as_str(db), type_hint_to_string(db, &f.type_hint)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }

        TypeHint::Tensor(t) => {
            let elem_str = type_hint_to_string(db, &t.element_type);
            format!("[|{}, {}|]", elem_str, t.rank)
        }
        TypeHint::Data => "data".to_string(),
        TypeHint::Error => "error".to_string(),
        TypeHint::ParseError(_) => "?".to_string(),
        TypeHint::Table(_) => "table".to_string(),
        TypeHint::Alias(name) => name.as_str(db).to_string(),
        TypeHint::Atom(a) => format!("atom {}", a.name.as_str(db)),
        TypeHint::Term(t) => format!("term {} {}", t.name.as_str(db), type_hint_to_string(db, &t.payload)),
        TypeHint::Enum(e) => {
            let variants: Vec<_> = e.variants.iter().map(|v| {
                match &v.payload {
                    Some(p) => format!("term {} {}", v.name.as_str(db), type_hint_to_string(db, p)),
                    None => format!("atom {}", v.name.as_str(db)),
                }
            }).collect();
            format!("enum{{{}}}", variants.join(", "))
        }
    }
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
        TypeError::ConstNotAllowedInModule(name) => {
            json!({
                "kind": "ConstNotAllowedInModule",
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

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun_parser::parse_for_diagnostics(&db, source);
    let spans = datalove_datafun_parser::datafun_spans(&db, source);

    // Resolve names for the script.
    let name_resolution = datalove_datafun_resolve::resolve_script_names(&db, source, script.clone());

    // Use the tracked functions directly so we can get accumulated diagnostics.
    let unit_spec = datalove_datafun_tycheck::ScriptUnitSpec::new(
        source,
        spans,
        datalove_datafun_tycheck::ScriptUnitKind::Fragment(script.clone(), name_resolution),
    );
    let batch_spec = datalove_datafun_tycheck::create_batch_spec(&db, source, vec![unit_spec], vec![]);
    let results = datalove_datafun_tycheck::type_check_script_units(&db, batch_spec);
    let tycheck_result = results.results(&db)[0];

    // Collect accumulated type diagnostics.
    let type_diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, batch_spec);
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
            let notes: Vec<_> = diag.notes.iter().map(|n| n.as_str(&db).to_string()).collect();
            let mut obj = json!({
                "code": code,
                "message": diag.message.as_str(&db),
                "labels": labels
            });
            if !notes.is_empty() {
                obj["notes"] = json!(notes);
            }
            obj
        })
        .collect();

    // Collect type judgements for variables and functions.
    let mut judgements = Vec::new();
    let expr_types = tycheck_result.expr_types(&db);
    for statement in &script.statements {
        match statement {
            datalove_datafun_ast::ast::Statement::Let(let_stmt) => {
                let name = let_stmt.name;
                // Get type from the let statement's value expression.
                let value_expr = let_stmt.value;
                let key = datalove_datafun_ast::ast::ExprKey::of(&db, value_expr);
                if let Some(ty) = expr_types.get(&key) {
                    judgements.push(json!({
                        "kind": "variable",
                        "name": name.as_str(&db),
                        "type": datalove_datafun_tycheck::type_to_string(&db, ty)
                    }));
                }
            }
            datalove_datafun_ast::ast::Statement::Fun(fun_stmt) => {
                let name = fun_stmt.name(&db);
                let params = fun_stmt.params(&db);
                let return_type = fun_stmt.return_type(&db);

                let param_types: Vec<_> = params.iter().map(|p| {
                    json!({
                        "name": p.name.as_str(&db),
                        "type": type_hint_to_string(&db, &p.type_hint)
                    })
                }).collect();

                let ret_ty_str = if let Some(ref rt) = return_type {
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
        "errors": errors,
        "diagnostics": diagnostics
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("tycheck")
        .file_extension("dfs")
        .run();
}
