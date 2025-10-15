use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use datalove_datafun as datafun;

fn find_test_fixtures() -> Vec<PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("interp");

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

/// Run a script and extract the value of the 'output' variable.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    // Parse the script.
    let script = datafun::parser::parse(&db, source);

    // Type check the script.
    let tycheck_result = datafun::tycheck::type_check(&db, script);
    if !tycheck_result.errors(&db).is_empty() {
        return Err(format!("Type check errors: {} error(s)", tycheck_result.errors(&db).len()));
    }

    // Build type table.
    let type_table = datafun::type_table::TypeTable::build(&db, script, tycheck_result)
        .map_err(|e| format!("Failed to build type table: {}", e))?;

    // Create interpreter context.
    let mut ctx = datafun::interp::InterpContext::new(&db, type_table);

    // Execute the script.
    ctx.execute(script)
        .map_err(|e| format!("Execution error: {:?}", e))?;

    // Pretty-print the 'output' variable.
    let output_name = bct::text::InternedText::new(&db, S("output"));
    ctx.pretty_print_variable(output_name)
        .map_err(|e| format!("Failed to pretty-print output: {:?}", e))
}

enum TestResult {
    Passed,
    Failed { expected: String, actual: String },
    Blessed,
    NoExpected,
    Error(String),
}

fn run_test_case(dfs_path: &Path) -> TestResult {
    let base_path = dfs_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let analysis = match analyze_file(dfs_path) {
        Ok(result) => result,
        Err(error) => {
            // Write error to actual file for debugging.
            let _ = std::fs::write(&actual_path, &error);
            return TestResult::Error(error);
        }
    };

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
        eprintln!("No test fixtures found in tests/fixtures/interp/");
        std::process::exit(1);
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);

    let mut passed = 0;
    let mut failed = 0;
    let mut blessed = 0;
    let mut no_expected = 0;
    let mut errors = 0;

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
            TestResult::Error(error) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true)).X();
                write!(&mut stdout, "  ERROR").X();
                stdout.reset().X();
                writeln!(&mut stdout, " {}", test_name).X();

                stderr.set_color(ColorSpec::new().set_fg(Some(Color::Red))).X();
                writeln!(&mut stderr, "\nError:").X();
                stderr.reset().X();
                writeln!(&mut stderr, "{}", error).X();
                errors += 1;
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

    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))).X();
    write!(&mut stdout, "{} errors", errors).X();
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

    if failed > 0 || errors > 0 {
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
