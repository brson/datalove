use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use serde::{Serialize, Deserialize};

use datalove_repl as repl;

fn find_test_fixtures() -> Vec<PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repl");

    let mut fixtures = Vec::new();
    if !fixtures_dir.exists() {
        return fixtures;
    }

    for entry in std::fs::read_dir(&fixtures_dir).X() {
        let entry = entry.X();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("dls") {
            fixtures.push(path);
        }
    }

    fixtures.sort();
    fixtures
}

/// A single REPL history entry for testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryEntry {
    /// Request ID for this entry.
    id: u64,
    /// The input text submitted.
    input: String,
    /// Parse result if available.
    parse_result: Option<repl::CommandParse>,
    /// Evaluation result if available.
    eval_result: Option<repl::Eval>,
}

/// Run a script through the REPL engine and capture history.
fn run_script(path: &Path) -> String {
    let script_content = std::fs::read_to_string(path).X();
    let mut engine = repl::Engine::new().X();
    let mut history: Vec<HistoryEntry> = Vec::new();
    let mut next_id = 0u64;

    // Buffer for accumulating multiline input.
    let mut multiline_buffer: Vec<String> = Vec::new();

    for line in script_content.lines() {
        // Accumulate line if we're in multiline mode.
        if !multiline_buffer.is_empty() {
            multiline_buffer.push(line.to_string());
        }

        // Determine what to parse: accumulated buffer or current line.
        let input_to_parse = if !multiline_buffer.is_empty() {
            multiline_buffer.join("\n")
        } else {
            line.to_string()
        };

        // Parse the input.
        let parse_result = engine.parse_input(&input_to_parse);

        match parse_result {
            repl::CommandParse::ReadMultiline => {
                // Need more input. Start accumulating if we haven't already.
                if multiline_buffer.is_empty() {
                    multiline_buffer.push(line.to_string());
                }

                // Create a history entry showing we're waiting for more.
                let entry = HistoryEntry {
                    id: next_id,
                    input: line.to_string(),
                    parse_result: Some(parse_result.clone()),
                    eval_result: None,
                };
                history.push(entry);
                next_id += 1;
                continue;
            }
            repl::CommandParse::Empty => {
                // Empty input, record it.
                let entry = HistoryEntry {
                    id: next_id,
                    input: line.to_string(),
                    parse_result: Some(parse_result.clone()),
                    eval_result: None,
                };
                history.push(entry);
                next_id += 1;
            }
            repl::CommandParse::Command(ref command) => {
                // Got a complete command, evaluate it.
                let eval_result = engine.eval(command.clone());

                let entry = HistoryEntry {
                    id: next_id,
                    input: line.to_string(),
                    parse_result: Some(parse_result.clone()),
                    eval_result: Some(eval_result),
                };
                history.push(entry);
                next_id += 1;

                // Clear multiline buffer if we were accumulating.
                multiline_buffer.clear();
            }
        }
    }

    // Serialize the entire history as pretty JSON.
    serde_json::to_string_pretty(&history).X() + "\n"
}

enum TestResult {
    Passed,
    Failed { expected: String, actual: String },
    Blessed,
    NoExpected,
}

fn run_test_case(dls_path: &Path) -> TestResult {
    let base_path = dls_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let output = run_script(dls_path);
    std::fs::write(&actual_path, &output).X();

    let bless = std::env::var("BLESS").is_ok();

    if bless {
        std::fs::copy(&actual_path, &expected_path).X();
        TestResult::Blessed
    } else if expected_path.exists() {
        let expected = std::fs::read_to_string(&expected_path).X();
        if output != expected {
            TestResult::Failed { expected, actual: output }
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
        eprintln!("No test fixtures found in tests/fixtures/repl/");
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
