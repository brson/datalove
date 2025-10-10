use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use rmx::termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
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
        if path.extension().and_then(|s| s.to_str()) == Some("dle") {
            fixtures.push(path);
        }
    }

    fixtures.sort();
    fixtures
}

fn analyze_file(path: &Path) -> String {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let ast = datalove_datalit::parser::parse(&db, source);
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

    rmx::serde_json::to_string_pretty(&output).X()
}

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
        Type::U32 => json!(format!("{}u32", heap_str)),
        Type::F32 => json!(format!("{}f32", heap_str)),
        Type::Int => json!(format!("{}int", heap_str)),
        Type::String => json!(format!("{}string", heap_str)),
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

enum TestResult {
    Passed,
    Failed { expected: String, actual: String },
    Blessed,
    NoExpected,
}

fn run_test_case(dle_path: &Path) -> TestResult {
    let base_path = dle_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let analysis = analyze_file(dle_path);
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
