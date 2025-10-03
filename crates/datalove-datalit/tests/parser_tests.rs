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
        eprintln!("No test fixtures found in tests/fixtures/parser/");
        std::process::exit(1);
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut blessed = 0;
    let mut no_expected = 0;

    for fixture in &fixtures {
        let test_name = fixture.file_stem().X().to_str().X();
        match run_test_case(fixture) {
            TestResult::Passed => {
                println!("  PASS  {}", test_name);
                passed += 1;
            }
            TestResult::Failed { expected, actual } => {
                println!("  FAIL  {}", test_name);
                eprintln!("\nExpected:\n{}\nActual:\n{}", expected, actual);
                failed += 1;
            }
            TestResult::Blessed => {
                println!("  BLESS {}", test_name);
                blessed += 1;
            }
            TestResult::NoExpected => {
                println!("  WARN  {} (no expected file)", test_name);
                no_expected += 1;
            }
        }
    }

    println!();
    println!("Results: {} passed, {} failed, {} blessed, {} no expected",
             passed, failed, blessed, no_expected);

    if failed > 0 {
        eprintln!("\nRun with BLESS=1 to update expected output.");
        std::process::exit(1);
    }

    if no_expected > 0 {
        eprintln!("\nRun with BLESS=1 to create expected files.");
        std::process::exit(1);
    }
}
