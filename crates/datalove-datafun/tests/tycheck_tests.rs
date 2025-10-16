use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use rmx::serde_json::json;

fn find_test_fixtures() -> Vec<PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tycheck");

    let mut fixtures = Vec::new();
    if !fixtures_dir.exists() {
        return fixtures;
    }

    for entry in std::fs::read_dir(&fixtures_dir).X() {
        let entry = entry.X();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("dfs") {
            fixtures.push(path);
        }
    }

    fixtures.sort();
    fixtures
}

fn type_hint_to_string(db: &dyn datalove_datafun::Db, type_hint: datalove_datalit::ast::TypeHintAndHeap) -> String {
    use datalove_datalit::ast::{TypeHint, Heap};

    let heap_prefix = match type_hint.heap(db) {
        Heap::Local => "@",
        Heap::Global => "#",
        Heap::Omitted => "",
    };

    let base_type = match type_hint.type_hint(db) {
        TypeHint::U32 => "u32",
        TypeHint::F32 => "f32",
        TypeHint::Bool => "bool",
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
        TypeHint::NamedTuple(_) |
        TypeHint::AnonStruct(_) |
        TypeHint::NamedStruct(_) |
        TypeHint::AnonEnum(_) |
        TypeHint::NamedEnum(_) |
        TypeHint::Data |
        TypeHint::Error |
        TypeHint::ParseError(_) => {
            return "?".to_string();
        }
    };

    format!("{}{}", heap_prefix, base_type)
}

fn analyze_file(path: &Path) -> String {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun::parser::parse(&db, source);
    let tycheck_result = datalove_datafun::tycheck::type_check(&db, script);

    // Collect type judgements for variables and functions.
    let mut judgements = Vec::new();
    for statement in script.statements(&db) {
        match statement {
            datalove_datafun::ast::Statement::Let(let_stmt) => {
                let name = let_stmt.name(&db);
                if let Some(ty) = datalove_datafun::tycheck::lookup_variable_type(&db, script, name) {
                    judgements.push(json!({
                        "kind": "variable",
                        "name": name.as_str(&db),
                        "type": datalove_datafun::tycheck::type_to_string(&db, ty.ty(&db))
                    }));
                }
            }
            datalove_datafun::ast::Statement::Fun(fun_stmt) => {
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

    rmx::serde_json::to_string_pretty(&output).X()
}

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

enum TestResult {
    Passed,
    Failed { expected: String, actual: String },
    Blessed,
    NoExpected,
}

fn run_test_case(dfs_path: &Path) -> TestResult {
    let base_path = dfs_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let analysis = analyze_file(dfs_path);
    std::fs::write(&actual_path, &analysis).X();

    let bless = std::env::var("BLESS").is_ok();

    if bless {
        std::fs::copy(&actual_path, &expected_path).X();
        TestResult::Blessed
    } else if expected_path.exists() {
        let expected = std::fs::read_to_string(&expected_path).X();
        if analysis != expected {
            TestResult::Failed { expected, actual: analysis }
        } else {
            TestResult::Passed
        }
    } else {
        TestResult::NoExpected
    }
}

fn main() {
    let fixtures = find_test_fixtures();

    if fixtures.is_empty() {
        eprintln!("No test fixtures found in tests/fixtures/tycheck/");
        std::process::exit(1);
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);

    let mut passed = 0;
    let mut failed = 0;
    let mut blessed = 0;
    let mut no_expected = 0;

    for fixture in &fixtures {
        let test_name = fixture.file_stem().X().to_str().X();
        match run_test_case(fixture) {
            TestResult::Passed => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true)).X();
                write!(&mut stdout, "  PASS ").X();
                stdout.reset().X();
                writeln!(&mut stdout, " {}", test_name).X();
                passed += 1;
            }
            TestResult::Failed { expected, actual } => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true)).X();
                write!(&mut stdout, "  FAIL ").X();
                stdout.reset().X();
                writeln!(&mut stdout, " {}", test_name).X();

                stderr.set_color(ColorSpec::new().set_fg(Some(Color::Yellow))).X();
                writeln!(&mut stderr, "\nExpected:").X();
                stderr.reset().X();
                writeln!(&mut stderr, "{}", expected).X();
                stderr.set_color(ColorSpec::new().set_fg(Some(Color::Yellow))).X();
                writeln!(&mut stderr, "Actual:").X();
                stderr.reset().X();
                writeln!(&mut stderr, "{}", actual).X();
                failed += 1;
            }
            TestResult::Blessed => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true)).X();
                write!(&mut stdout, "  BLESS").X();
                stdout.reset().X();
                writeln!(&mut stdout, " {}", test_name).X();
                blessed += 1;
            }
            TestResult::NoExpected => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)).set_bold(true)).X();
                write!(&mut stdout, "  WARN ").X();
                stdout.reset().X();
                writeln!(&mut stdout, " {} (no expected file)", test_name).X();
                no_expected += 1;
            }
        }
    }

    writeln!(&mut stdout).X();
    write!(&mut stdout, "Results: ").X();

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green))).X();
    write!(&mut stdout, "{} passed", passed).X();
    stdout.reset().X();
    write!(&mut stdout, ", ").X();

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))).X();
    write!(&mut stdout, "{} failed", failed).X();
    stdout.reset().X();
    write!(&mut stdout, ", ").X();

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan))).X();
    write!(&mut stdout, "{} blessed", blessed).X();
    stdout.reset().X();
    write!(&mut stdout, ", ").X();

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow))).X();
    write!(&mut stdout, "{} no expected", no_expected).X();
    stdout.reset().X();
    writeln!(&mut stdout).X();

    if failed > 0 {
        stderr.set_color(ColorSpec::new().set_fg(Some(Color::Yellow))).X();
        writeln!(&mut stderr, "\nRun with BLESS=1 to update expected output.").X();
        stderr.reset().X();
        std::process::exit(1);
    }

    if no_expected > 0 {
        stderr.set_color(ColorSpec::new().set_fg(Some(Color::Yellow))).X();
        writeln!(&mut stderr, "\nRun with BLESS=1 to create expected files.").X();
        stderr.reset().X();
        std::process::exit(1);
    }
}
