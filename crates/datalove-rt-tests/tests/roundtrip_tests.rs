use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use datalove_datalit as datalit;
use datalove_rt as rt;

fn find_test_fixtures() -> Vec<PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("roundtrip");

    let mut fixtures = Vec::new();
    if !fixtures_dir.exists() {
        return fixtures;
    }

    for entry in std::fs::read_dir(&fixtures_dir).X() {
        let entry = entry.X();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("dlt") {
            fixtures.push(path);
        }
    }

    fixtures.sort();
    fixtures
}

/// Compare two types for structural equality, recursively comparing nested types.
fn types_equal<'db>(
    db: &'db datalit::Database,
    type1: &datalit::tycheck::Type<'db>,
    type2: &datalit::tycheck::Type<'db>,
) -> bool {
    use datalit::tycheck::Type;

    match (type1, type2) {
        (Type::Bool, Type::Bool) => true,
        (Type::U8, Type::U8) => true,
        (Type::I8, Type::I8) => true,
        (Type::U16, Type::U16) => true,
        (Type::I16, Type::I16) => true,
        (Type::U32, Type::U32) => true,
        (Type::I32, Type::I32) => true,
        (Type::U64, Type::U64) => true,
        (Type::I64, Type::I64) => true,
        (Type::F32, Type::F32) => true,
        (Type::Int, Type::Int) => true,
        (Type::String, Type::String) => true,
        (Type::Data, Type::Data) => true,
        (Type::Error, Type::Error) => true,

        (Type::AnonTuple(t1), Type::AnonTuple(t2)) => {
            let fields1 = t1.fields(db);
            let fields2 = t2.fields(db);
            if fields1.len() != fields2.len() {
                return false;
            }
            fields1.iter().zip(fields2.iter()).all(|(f1, f2)| {
                let heap1 = std::mem::discriminant(&f1.heap(db));
                let heap2 = std::mem::discriminant(&f2.heap(db));
                heap1 == heap2 && types_equal(db, f1.ty(db), f2.ty(db))
            })
        }

        (Type::List(t1), Type::List(t2)) => {
            let elem1 = t1.element_type(db);
            let elem2 = t2.element_type(db);
            let heap1 = std::mem::discriminant(&elem1.heap(db));
            let heap2 = std::mem::discriminant(&elem2.heap(db));
            heap1 == heap2 && types_equal(db, elem1.ty(db), elem2.ty(db))
        }

        (Type::Option(t1), Type::Option(t2)) => {
            let inner1 = t1.inner_type(db);
            let inner2 = t2.inner_type(db);
            let heap1 = std::mem::discriminant(&inner1.heap(db));
            let heap2 = std::mem::discriminant(&inner2.heap(db));
            heap1 == heap2 && types_equal(db, inner1.ty(db), inner2.ty(db))
        }

        (Type::Result(t1), Type::Result(t2)) => {
            let inner1 = t1.inner_type(db);
            let inner2 = t2.inner_type(db);
            let heap1 = std::mem::discriminant(&inner1.heap(db));
            let heap2 = std::mem::discriminant(&inner2.heap(db));
            heap1 == heap2 && types_equal(db, inner1.ty(db), inner2.ty(db))
        }

        (Type::Map(m1), Type::Map(m2)) => {
            let key1 = m1.key_type(db);
            let key2 = m2.key_type(db);
            let value1 = m1.value_type(db);
            let value2 = m2.value_type(db);
            let key_heap1 = std::mem::discriminant(&key1.heap(db));
            let key_heap2 = std::mem::discriminant(&key2.heap(db));
            let value_heap1 = std::mem::discriminant(&value1.heap(db));
            let value_heap2 = std::mem::discriminant(&value2.heap(db));
            key_heap1 == key_heap2 && value_heap1 == value_heap2
                && types_equal(db, key1.ty(db), key2.ty(db))
                && types_equal(db, value1.ty(db), value2.ty(db))
        }

        (Type::Set(s1), Type::Set(s2)) => {
            let elem1 = s1.element_type(db);
            let elem2 = s2.element_type(db);
            let heap1 = std::mem::discriminant(&elem1.heap(db));
            let heap2 = std::mem::discriminant(&elem2.heap(db));
            heap1 == heap2 && types_equal(db, elem1.ty(db), elem2.ty(db))
        }

        _ => {
            // For other types or mismatched variants, use standard equality.
            type1 == type2
        }
    }
}

/// Compile and instantiate a datalit value.
#[salsa::tracked]
fn compile<'db>(
    db: &'db dyn salsa::Database,
    source: bct::input::Source,
) -> (datalit::ast::ExprFull<'db>, datalit::resolve::ResolvedExpr<'db>, datalit::tycheck::TypecheckResult<'db>) {
    let parse_result = datalit::parser::parse(db, source);
    let parsed = parse_result.expr;
    let resolved = datalit::resolve::resolve_names(db, parsed);
    let typechecked = datalit::tycheck::type_check(db, parsed, resolved);
    (parsed, resolved, typechecked)
}

fn compile_and_instantiate<'db>(
    db: &'db datalit::Database,
    rt: &mut rt::rt_local::RtLocal,
    tydesc_table: &mut datalit::tydesc_table::TyDescTable<'db>,
    source_text: &str,
) -> Result<(datalit::instantiate2::InstantiatedValue, datalit::tycheck::TypecheckResult<'db>), String> {
    let source = bct::input::Source::new(db, source_text.S());
    let (parsed, _resolved, typechecked) = compile(db, source);

    // Check if we have a root type.
    if typechecked.root_type(db).is_none() {
        return Err(format!("No root type for source: {}", source_text));
    }

    // Check for typecheck errors.
    let errors = typechecked.errors(db);
    if !errors.is_empty() {
        return Err(format!("Type check errors: {} error(s)", errors.len()));
    }

    let inst_value = datalit::instantiate2::instantiate_value(db, rt, tydesc_table, typechecked)
        .map_err(|e| format!("Instantiation error: {}", e))?;

    Ok((inst_value, typechecked))
}

/// Pretty-print a runtime value to a string with type hint.
fn rt_pretty_print<'db>(
    db: &'db datalit::Database,
    ty: &datalit::tycheck::TypeAndHeap<'db>,
    value_ref: *const u8,
    tydesc_ref: *const rt::rtdt::TyDesc,
) -> Result<String, String> {
    datalit::pretty::pretty_print_runtime_value(db, ty, value_ref, tydesc_ref)
}

/// Round-trip test: parse -> instantiate -> rt pretty-print -> parse -> instantiate -> rt pretty-print.
///
/// Both pretty-prints should be identical, and all three sources should typecheck identically.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Step 1: Parse, type check, and instantiate the original datalit.
    let db = datalit::Database::default();
    let mut rt_inst = rt::rt_local::RtLocal::new();
    let mut tydesc_table = datalit::tydesc_table::TyDescTable::new(&db);
    let (inst1, tycheck1) = compile_and_instantiate(&db, &mut rt_inst, &mut tydesc_table, &source_text)?;

    // Extract the root type from the first typecheck.
    let type1 = tycheck1.root_type(&db).X();

    // Step 2: Pretty-print using runtime pretty printer.
    let pretty1 = rt_pretty_print(&db, &type1, inst1.ptr, inst1.tydesc)?;

    // Step 3: Parse, type check, and instantiate the pretty-printed output.
    let (inst2, tycheck2) = compile_and_instantiate(&db, &mut rt_inst, &mut tydesc_table, &pretty1)?;

    // Extract the root type from the second typecheck.
    let type2 = tycheck2.root_type(&db).X();

    // Step 4: Pretty-print again.
    let pretty2 = rt_pretty_print(&db, &type2, inst2.ptr, inst2.tydesc)?;

    // Step 5: Parse and typecheck the second pretty-print to get the third type.
    let source3 = bct::input::Source::new(&db, pretty2.S());
    let (_parsed3, _resolved3, tycheck3) = compile(&db, source3);
    let type3 = tycheck3.root_type(&db).X();

    // Step 6: Check that all three types and heaps are identical.
    // We compare the actual type and heap content, not salsa identity.
    let heap1 = std::mem::discriminant(&type1.heap(&db));
    let heap2 = std::mem::discriminant(&type2.heap(&db));
    let heap3 = std::mem::discriminant(&type3.heap(&db));

    if heap1 != heap2 || !types_equal(&db, type1.ty(&db), type2.ty(&db)) {
        unsafe {
            let rt_handle = &mut *rt_inst as *mut rt::rt_local::RtLocal as *mut u8;
            rt::dtlv_rti_any_destroy_local(rt_handle, inst1.ptr as *mut u8, inst1.tydesc);
            rt::dtlv_rti_mem_free_local(rt_handle, inst1.tydesc, 1, inst1.ptr as *mut u8);
            rt::dtlv_rti_any_destroy_local(rt_handle, inst2.ptr as *mut u8, inst2.tydesc);
            rt::dtlv_rti_mem_free_local(rt_handle, inst2.tydesc, 1, inst2.ptr as *mut u8);
            rt_inst.shutdown();
        }
        return Err(format!(
            "Types differ between original and first pretty-print:\nOriginal: {}\nFirst:    {}",
            source_text, pretty1
        ));
    }

    if heap2 != heap3 || !types_equal(&db, type2.ty(&db), type3.ty(&db)) {
        unsafe {
            let rt_handle = &mut *rt_inst as *mut rt::rt_local::RtLocal as *mut u8;
            rt::dtlv_rti_any_destroy_local(rt_handle, inst1.ptr as *mut u8, inst1.tydesc);
            rt::dtlv_rti_mem_free_local(rt_handle, inst1.tydesc, 1, inst1.ptr as *mut u8);
            rt::dtlv_rti_any_destroy_local(rt_handle, inst2.ptr as *mut u8, inst2.tydesc);
            rt::dtlv_rti_mem_free_local(rt_handle, inst2.tydesc, 1, inst2.ptr as *mut u8);
            rt_inst.shutdown();
        }
        return Err(format!(
            "Types differ between first and second pretty-print:\nFirst:  {}\nSecond: {}",
            pretty1, pretty2
        ));
    }

    // Step 7: Check that both pretty-prints are identical.
    if pretty1 != pretty2 {
        unsafe {
            let rt_handle = &mut *rt_inst as *mut rt::rt_local::RtLocal as *mut u8;
            rt::dtlv_rti_any_destroy_local(rt_handle, inst1.ptr as *mut u8, inst1.tydesc);
            rt::dtlv_rti_mem_free_local(rt_handle, inst1.tydesc, 1, inst1.ptr as *mut u8);
            rt::dtlv_rti_any_destroy_local(rt_handle, inst2.ptr as *mut u8, inst2.tydesc);
            rt::dtlv_rti_mem_free_local(rt_handle, inst2.tydesc, 1, inst2.ptr as *mut u8);
            rt_inst.shutdown();
        }
        return Err(format!(
            "Pretty-prints differ:\nFirst:  {}\nSecond: {}",
            pretty1, pretty2
        ));
    }

    // Clean up instantiated values before shutdown.
    unsafe {
        let rt_handle = &mut *rt_inst as *mut rt::rt_local::RtLocal as *mut u8;
        rt::dtlv_rti_any_destroy_local(rt_handle, inst1.ptr as *mut u8, inst1.tydesc);
        rt::dtlv_rti_mem_free_local(rt_handle, inst1.tydesc, 1, inst1.ptr as *mut u8);
        rt::dtlv_rti_any_destroy_local(rt_handle, inst2.ptr as *mut u8, inst2.tydesc);
        rt::dtlv_rti_mem_free_local(rt_handle, inst2.tydesc, 1, inst2.ptr as *mut u8);
        rt_inst.shutdown();
    }
    Ok(pretty1)
}

enum TestResult {
    Passed,
    Failed { expected: String, actual: String },
    Blessed,
    NoExpected,
    Error(String),
}

fn run_test_case(dlt_path: &Path) -> TestResult {
    let base_path = dlt_path.with_extension("");
    let actual_path = PathBuf::from(format!("{}.out.actual", base_path.display()));
    let expected_path = PathBuf::from(format!("{}.out.expected", base_path.display()));

    let analysis = match analyze_file(dlt_path) {
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
    let fixtures = find_test_fixtures();

    if fixtures.is_empty() {
        eprintln!("No test fixtures found in tests/fixtures/roundtrip/");
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
