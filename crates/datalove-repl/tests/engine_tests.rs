use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use datalove_repl as repl;
use datalove_exampletest::{parse_test_filters, matches_filters};

/// Find all test fixture files with .repl extension.
fn find_test_fixtures() -> Vec<PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("engine");

    let mut fixtures = Vec::new();
    if !fixtures_dir.exists() {
        return fixtures;
    }

    for entry in std::fs::read_dir(&fixtures_dir).X() {
        let entry = entry.X();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("repl") {
            fixtures.push(path);
        }
    }

    fixtures.sort();
    fixtures
}

/// Environment binding (name, type, value).
#[derive(serde::Serialize)]
struct EnvBinding {
    name: String,
    ty: String,
    value: String,
}

/// Test output for a single input to the REPL engine.
#[derive(serde::Serialize)]
struct InputResult {
    input: String,
    parse: repl::InputParse,
    eval: Option<repl::Eval>,
    environment: Vec<EnvBinding>,
}

/// Process a REPL fixture file.
///
/// The file contains multiple inputs separated by a line containing only "---".
/// For each input, we record the parse result, eval result, and environment.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = repl::datafun::Database::default();
    let mut engine = repl::Engine::new(&db)
        .map_err(|e| format!("Failed to create engine: {}", e))?;

    let mut results = Vec::new();

    // Split the file into sections by "---" separator.
    let sections: Vec<&str> = source_text.split("\n---\n").collect();

    for section in sections {
        let input = section.trim();
        if input.is_empty() {
            continue;
        }

        // Detect if input contains newlines and use appropriate input type.
        let repl_input = if input.contains('\n') {
            repl::Input::Multiline(input.to_string())
        } else {
            repl::Input::Input(input.to_string())
        };

        let parse_result = engine.parse_input(repl_input);

        let eval_result = match &parse_result {
            repl::InputParse::Command(cmd) => {
                Some(engine.eval(cmd.clone()))
            }
            _ => None,
        };

        let environment = engine.get_environment()
            .into_iter()
            .map(|(name, ty, value)| EnvBinding { name, ty, value })
            .collect();

        results.push(InputResult {
            input: input.to_string(),
            parse: parse_result,
            eval: eval_result,
            environment,
        });
    }

    // Serialize results to pretty JSON.
    serde_json::to_string_pretty(&results)
        .map_err(|e| format!("Failed to serialize results: {}", e))
}

enum TestResult {
    Passed,
    Failed { expected: String, actual: String },
    Blessed,
    NoExpected,
    Error(String),
}

fn run_test_case(repl_path: &Path) -> TestResult {
    let base_path = repl_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let analysis = match analyze_file(repl_path) {
        Ok(result) => result,
        Err(error) => return TestResult::Error(error),
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
    let filters = parse_test_filters();
    let all_fixtures = find_test_fixtures();

    if all_fixtures.is_empty() {
        eprintln!("No test fixtures found in tests/fixtures/engine/");
        std::process::exit(1);
    }

    // Filter fixtures based on command-line arguments.
    let fixtures: Vec<_> = all_fixtures
        .into_iter()
        .filter(|f| {
            let name = f.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            matches_filters(name, &filters)
        })
        .collect();

    if fixtures.is_empty() {
        eprintln!("No tests matched the filter(s): {:?}", filters);
        std::process::exit(0);
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);

    // Print filter info if filtering is active.
    if !filters.is_empty() {
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan))).X();
        write!(&mut stdout, "Filter: ").X();
        stdout.reset().X();
        writeln!(&mut stdout, "{:?}", filters).X();
        writeln!(&mut stdout).X();
    }

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
