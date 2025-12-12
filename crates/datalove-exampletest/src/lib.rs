//! Example-based test harness for datalove.
//!
//! This crate provides infrastructure for running example-based tests that compare
//! actual output against expected output, with support for the "blessed" pattern
//! (updating expected output via BLESS=1 environment variable).
//!
//! ## Test Filtering
//!
//! Tests can be filtered by passing arguments after `--` to cargo test:
//!
//! ```bash
//! cargo test -p datalove-datafun --test parser_tests -- foo
//! ```
//!
//! This runs only tests whose names contain "foo". Multiple filters can be
//! provided and a test runs if it matches any filter.

use rmx::prelude::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

/// Parse command-line arguments for test filtering.
///
/// Returns a list of filter patterns. A test matches if its name contains
/// any of the patterns (case-sensitive substring match).
pub fn parse_test_filters() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// Check if a test name matches the given filters.
///
/// Returns true if filters is empty (run all tests) or if the test name
/// contains any of the filter strings as a substring.
pub fn matches_filters(test_name: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    filters.iter().any(|filter| test_name.contains(filter.as_str()))
}

/// Result of running a single test case.
#[derive(Debug)]
pub enum TestResult {
    /// Test passed (actual matches expected).
    Passed,
    /// Test failed (actual doesn't match expected).
    Failed { expected: String, actual: String },
    /// Test was blessed (expected file was updated).
    Blessed,
    /// No expected file exists yet.
    NoExpected,
    /// An error occurred during analysis.
    Error(String),
}

/// Configuration and runner for example-based tests.
pub struct ExampleTestRunner<F> {
    manifest_dir: PathBuf,
    fixture_subdir: String,
    file_extension: String,
    analyzer: F,
    allow_errors: bool,
}

impl<F> ExampleTestRunner<F>
where
    F: Fn(&Path) -> Result<String, String>,
{
    /// Create a new test runner with the given analyzer function.
    ///
    /// The analyzer function takes a path to a test fixture file and returns
    /// either the test output (Ok) or an error message (Err).
    ///
    /// **Important:** You must pass `env!("CARGO_MANIFEST_DIR")` as the first argument.
    /// This ensures the path is resolved from the test crate, not the library crate.
    ///
    /// # Example
    /// ```ignore
    /// ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
    /// ```
    pub fn new(manifest_dir: impl Into<PathBuf>, analyzer: F) -> Self {
        Self {
            manifest_dir: manifest_dir.into(),
            fixture_subdir: String::new(),
            file_extension: String::new(),
            analyzer,
            allow_errors: false,
        }
    }

    /// Set the subdirectory within tests/fixtures/ where test files are located.
    pub fn fixture_subdir(mut self, subdir: impl Into<String>) -> Self {
        self.fixture_subdir = subdir.into();
        self
    }

    /// Set the file extension to search for (without the dot).
    pub fn file_extension(mut self, ext: impl Into<String>) -> Self {
        self.file_extension = ext.into();
        self
    }

    /// Allow tests that return errors (adds Error variant to results).
    pub fn allow_errors(mut self, allow: bool) -> Self {
        self.allow_errors = allow;
        self
    }

    /// Find test fixture files in the configured directory.
    fn find_test_fixtures(&self) -> Vec<PathBuf> {
        let fixtures_dir = self.manifest_dir
            .join("tests")
            .join("fixtures")
            .join(&self.fixture_subdir);

        let mut fixtures = Vec::new();
        if !fixtures_dir.exists() {
            return fixtures;
        }

        for entry in std::fs::read_dir(&fixtures_dir).X() {
            let entry = entry.X();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some(&self.file_extension) {
                fixtures.push(path);
            }
        }

        fixtures.sort();
        fixtures
    }

    /// Run a single test case.
    fn run_test_case(&self, input_path: &Path) -> TestResult {
        let base_path = input_path.with_extension("");
        let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
        let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

        let analysis = match (self.analyzer)(input_path) {
            Ok(result) => result,
            Err(error) => {
                if self.allow_errors {
                    // Write error to actual file for debugging.
                    let _ = std::fs::write(&actual_path, &error);
                    return TestResult::Error(error);
                } else {
                    // Treat errors as test output.
                    error
                }
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
                TestResult::Failed {
                    expected,
                    actual: analysis,
                }
            } else {
                TestResult::Passed
            }
        } else {
            TestResult::NoExpected
        }
    }

    /// Run all tests and report results.
    ///
    /// This is the main entry point. It will find all test fixtures, run them,
    /// print colored output, and exit with an appropriate status code.
    ///
    /// Test filtering is supported via command-line arguments. Pass filter
    /// strings after `--` to cargo test, e.g.:
    ///
    /// ```bash
    /// cargo test -p crate --test test_name -- filter
    /// ```
    pub fn run(self) -> ! {
        let filters = parse_test_filters();
        let all_fixtures = self.find_test_fixtures();

        if all_fixtures.is_empty() {
            eprintln!(
                "No test fixtures found in tests/fixtures/{}/",
                self.fixture_subdir
            );
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
            eprintln!(
                "No tests matched the filter(s): {:?}",
                filters
            );
            std::process::exit(0);
        }

        let mut stdout = StandardStream::stdout(ColorChoice::Auto);
        let mut stderr = StandardStream::stderr(ColorChoice::Auto);

        // Print filter info if filtering is active.
        if !filters.is_empty() {
            stdout
                .set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))
                .X();
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
            match self.run_test_case(fixture) {
                TestResult::Passed => {
                    stdout
                        .set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true))
                        .X();
                    write!(&mut stdout, "  PASS ").X();
                    stdout.reset().X();
                    writeln!(&mut stdout, " {}", test_name).X();
                    passed += 1;
                }
                TestResult::Failed { expected, actual } => {
                    stdout
                        .set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))
                        .X();
                    write!(&mut stdout, "  FAIL ").X();
                    stdout.reset().X();
                    writeln!(&mut stdout, " {}", test_name).X();

                    stderr
                        .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
                        .X();
                    writeln!(&mut stderr, "\nExpected:").X();
                    stderr.reset().X();
                    writeln!(&mut stderr, "{}", expected).X();
                    stderr
                        .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
                        .X();
                    writeln!(&mut stderr, "Actual:").X();
                    stderr.reset().X();
                    writeln!(&mut stderr, "{}", actual).X();
                    failed += 1;
                }
                TestResult::Blessed => {
                    stdout
                        .set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))
                        .X();
                    write!(&mut stdout, "  BLESS").X();
                    stdout.reset().X();
                    writeln!(&mut stdout, " {}", test_name).X();
                    blessed += 1;
                }
                TestResult::NoExpected => {
                    stdout
                        .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)).set_bold(true))
                        .X();
                    write!(&mut stdout, "  WARN ").X();
                    stdout.reset().X();
                    writeln!(&mut stdout, " {} (no expected file)", test_name).X();
                    no_expected += 1;
                }
                TestResult::Error(error) => {
                    stdout
                        .set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))
                        .X();
                    write!(&mut stdout, "  ERROR").X();
                    stdout.reset().X();
                    writeln!(&mut stdout, " {}", test_name).X();

                    stderr
                        .set_color(ColorSpec::new().set_fg(Some(Color::Red)))
                        .X();
                    writeln!(&mut stderr, "\nError:").X();
                    stderr.reset().X();
                    writeln!(&mut stderr, "{}", error).X();
                    errors += 1;
                }
            }
        }

        writeln!(&mut stdout).X();
        write!(&mut stdout, "Results: ").X();

        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Green)))
            .X();
        write!(&mut stdout, "{} passed", passed).X();
        stdout.reset().X();
        write!(&mut stdout, ", ").X();

        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Red)))
            .X();
        write!(&mut stdout, "{} failed", failed).X();
        stdout.reset().X();
        write!(&mut stdout, ", ").X();

        if self.allow_errors {
            stdout
                .set_color(ColorSpec::new().set_fg(Some(Color::Red)))
                .X();
            write!(&mut stdout, "{} errors", errors).X();
            stdout.reset().X();
            write!(&mut stdout, ", ").X();
        }

        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))
            .X();
        write!(&mut stdout, "{} blessed", blessed).X();
        stdout.reset().X();
        write!(&mut stdout, ", ").X();

        stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
            .X();
        write!(&mut stdout, "{} no expected", no_expected).X();
        stdout.reset().X();
        writeln!(&mut stdout).X();

        if failed > 0 || errors > 0 {
            stderr
                .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
                .X();
            writeln!(&mut stderr, "\nRun with BLESS=1 to update expected output.").X();
            stderr.reset().X();
            std::process::exit(1);
        }

        if no_expected > 0 {
            stderr
                .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
                .X();
            writeln!(&mut stderr, "\nRun with BLESS=1 to create expected files.").X();
            stderr.reset().X();
            std::process::exit(1);
        }

        std::process::exit(0);
    }
}
