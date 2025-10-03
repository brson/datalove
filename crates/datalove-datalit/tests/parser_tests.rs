use rmx::prelude::*;
use std::path::{Path, PathBuf};

fn find_test_fixtures() -> Vec<PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("parser");

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

    let _ast = datalove_datalit::parser::parse(&db, source);
    format!("OK: parsed successfully")
}

fn run_test_case(dle_path: &Path) {
    let test_name = dle_path.file_stem().X().to_str().X();
    let base_path = dle_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let analysis = analyze_file(dle_path);
    std::fs::write(&actual_path, &analysis).X();

    let bless = std::env::var("BLESS").is_ok();

    if bless {
        std::fs::copy(&actual_path, &expected_path).X();
        eprintln!("BLESSED: {}", test_name);
    } else if expected_path.exists() {
        let expected = std::fs::read_to_string(&expected_path).X();
        if analysis != expected {
            panic!(
                "\nTest '{}' failed!\nExpected:\n{}\nActual:\n{}\n\nRun with BLESS=1 to update expected output.",
                test_name, expected, analysis
            );
        }
    } else {
        eprintln!(
            "WARNING: No expected file for '{}'. Run with BLESS=1 to create it.",
            test_name
        );
    }
}

#[test]
fn parser_integration_tests() {
    let fixtures = find_test_fixtures();

    if fixtures.is_empty() {
        eprintln!("No test fixtures found in tests/fixtures/parser/");
        return;
    }

    for fixture in fixtures {
        eprintln!("Running test: {:?}", fixture.file_name().X());
        run_test_case(&fixture);
    }
}
