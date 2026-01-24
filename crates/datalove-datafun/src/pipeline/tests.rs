use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use datalove_rt::c::DebugOutputMode;
use datalove_datafun_interp::InterpCtfeEvaluator;

fn make_db() -> crate::Database {
    crate::Database::default()
}

/// Helper struct that wraps compiler and executor for tests.
struct TestContext<'db> {
    compiler: ScriptCompiler<'db>,
    executor: ScriptExecutor,
}

impl<'db> TestContext<'db> {
    fn new(compiled: &CompiledModules<'db>, db: &'db dyn salsa::Database) -> Self {
        let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
        let compiler = compiled.script_compiler(db, evaluator).unwrap();
        let executor = compiled.script_executor(DebugOutputMode::Disabled, None).unwrap();
        Self { compiler, executor }
    }

    fn eval_fragment(&mut self, source: &str) -> ScriptUnitResult {
        let compiled = self.compiler.compile_fragment(source);
        let output = if let Some(ir_unit) = &compiled.ir_unit {
            self.executor.execute_fragment(ir_unit)
        } else {
            String::new()
        };
        ScriptUnitResult {
            typecheck: compiled.typecheck,
            ownership: compiled.ownership,
            lowering: compiled.lowering,
            ty: None,
            output,
        }
    }

    fn eval_expr(&mut self, source: &str) -> ScriptUnitResult {
        let compiled = self.compiler.compile_expr(source);
        let (ty, output) = if let Some(ir_unit) = &compiled.ir_unit {
            self.executor.execute_expr(ir_unit)
        } else {
            (None, String::new())
        };
        ScriptUnitResult {
            typecheck: compiled.typecheck,
            ownership: compiled.ownership,
            lowering: compiled.lowering,
            ty,
            output,
        }
    }

    fn destroy_all(&mut self) {
        self.executor.destroy_all();
    }
}

/// Test creating multiple script contexts from the same compiled modules.
#[test]
fn test_multiple_script_contexts() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    // Compile with no modules - just testing script context isolation.
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "compilation should succeed");

    // Create first script context.
    let mut ctx1 = TestContext::new(&compiled, &db);
    let result1 = ctx1.eval_fragment("let x = 10");
    assert!(matches!(result1.typecheck, TypecheckResult::Success), "ctx1 fragment should typecheck");

    // Create second script context from the same compiled modules.
    let mut ctx2 = TestContext::new(&compiled, &db);
    let result2 = ctx2.eval_fragment("let y = 20");
    assert!(matches!(result2.typecheck, TypecheckResult::Success), "ctx2 fragment should typecheck");

    // Each context should have independent state.
    let expr1 = ctx1.eval_expr("x");
    assert_eq!(expr1.output, "10");

    let expr2 = ctx2.eval_expr("y");
    assert_eq!(expr2.output, "20");

    // ctx1 should not see y, ctx2 should not see x.
    let bad1 = ctx1.eval_expr("y");
    assert!(matches!(bad1.typecheck, TypecheckResult::Error { .. }), "ctx1 should not see y");

    let bad2 = ctx2.eval_expr("x");
    assert!(matches!(bad2.typecheck, TypecheckResult::Error { .. }), "ctx2 should not see x");

    ctx1.destroy_all();
    ctx2.destroy_all();
}

/// Test interleaved execution of script units from multiple contexts.
#[test]
fn test_interleaved_execution() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful());

    let mut ctx_a = TestContext::new(&compiled, &db);
    let mut ctx_b = TestContext::new(&compiled, &db);

    // Define simple identity functions in each context.
    let r1 = ctx_a.eval_fragment("fun id_a(n: u32): u32\n  ret n\nend fun");
    assert!(matches!(r1.typecheck, TypecheckResult::Success), "ctx_a fn def failed: {:?}", r1.typecheck);

    let r2 = ctx_b.eval_fragment("fun id_b(n: u32): u32\n  ret n\nend fun");
    assert!(matches!(r2.typecheck, TypecheckResult::Success), "ctx_b fn def failed: {:?}", r2.typecheck);

    // Interleave: A defines values, B defines values.
    let _ = ctx_a.eval_fragment("let a1: u32 = 5");
    let _ = ctx_b.eval_fragment("let b1: u32 = 10");

    // A uses its function, B uses its function.
    let _ = ctx_a.eval_fragment("let a2 = id_a(a1)");
    let _ = ctx_b.eval_fragment("let b2 = id_b(b1)");

    // Verify values.
    let result_a = ctx_a.eval_expr("a2");
    assert_eq!(result_a.output, "5");

    let result_b = ctx_b.eval_expr("b2");
    assert_eq!(result_b.output, "10");

    // Each context's function is isolated.
    let bad_a = ctx_a.eval_expr("id_b(1)");
    assert!(matches!(bad_a.typecheck, TypecheckResult::Error { .. }), "ctx_a should not see id_b");

    let bad_b = ctx_b.eval_expr("id_a(1)");
    assert!(matches!(bad_b.typecheck, TypecheckResult::Error { .. }), "ctx_b should not see id_a");

    ctx_a.destroy_all();
    ctx_b.destroy_all();
}

/// Test running scripts in parallel using threads with shared compiled modules.
///
/// Compiles modules once, then shares the compilation across threads.
/// Each thread gets a cloned database via `DbClone::dyn_clone()` and creates
/// its own script context from the shared compiled modules.
#[test]
fn test_parallel_script_execution() {
    use std::sync::Arc;
    use std::thread;
    use datalove_datafun_tycheck::DbClone;

    // Compile once on the main thread.
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    // Add a module with a function all threads will use.
    pipeline.add_module(&db, "local", "pkg", "math", r#"
fun square(n: int): int
  ret n * n
end fun
"#);

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "compilation failed: {:?}", compiled.all_errors());

    // Clone the compiled modules into an Arc for sharing.
    // (CompiledModules already has Arc<SharedModuleContext> internally.)
    let compiled = Arc::new(compiled);

    // Clone databases upfront - one per thread for parallel execution.
    // Can't clone inside parallel section since &dyn DbClone isn't Sync.
    let work: Vec<_> = (0..4)
        .map(|i| (db.dyn_clone(), Arc::clone(&compiled), i))
        .collect();

    let results = std::sync::Mutex::new(Vec::new());

    thread::scope(|s| {
        for (db_clone, compiled, i) in work {
            let results = &results;
            s.spawn(move || {
                // Use the cloned database for this thread.
                let db_ref = db_clone.as_salsa_db();

                // Create compiler and executor from the shared compiled modules.
                let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
                let mut compiler = compiled.script_compiler(db_ref, evaluator).unwrap();
                let mut executor = compiled.script_executor(DebugOutputMode::Disabled, None).unwrap();

                // Import and use the shared module function.
                let compiled_unit = compiler.compile_fragment("require module local/pkg/math\nimport math.square");
                assert!(matches!(compiled_unit.typecheck, TypecheckResult::Success),
                    "import failed: {:?}", compiled_unit.typecheck);
                if let Some(ir_unit) = &compiled_unit.ir_unit {
                    executor.execute_fragment(ir_unit);
                }

                // Define local variable and compute.
                let val = (i + 1) * 10;
                let compiled_unit = compiler.compile_fragment(&format!("let n: int = {}", val));
                if let Some(ir_unit) = &compiled_unit.ir_unit {
                    executor.execute_fragment(ir_unit);
                }
                let compiled_unit = compiler.compile_fragment("let result = square(n)");
                if let Some(ir_unit) = &compiled_unit.ir_unit {
                    executor.execute_fragment(ir_unit);
                }

                let compiled_unit = compiler.compile_expr("result");
                let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                    let (_, value) = executor.execute_expr(ir_unit);
                    value
                } else {
                    String::new()
                };
                executor.destroy_all();

                let expected = val * val;
                assert_eq!(output, format!("{}", expected),
                    "thread {} expected {} but got {}", i, expected, output);
                results.lock().unwrap().push((i, expected));
            });
        }
    });

    // Verify all threads completed.
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    let values: Vec<_> = results.into_iter().map(|(_, v)| v).collect();
    assert_eq!(values, vec![100, 400, 900, 1600]);
}

/// Test compile-run-compile-run pattern with incremental compilation.
#[test]
fn test_compile_run_compile_run() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    // First compilation: add a module with a simple identity function.
    pipeline.add_module(&db, "local", "pkg", "v1", r#"
fun value(x: u32): u32
  ret x
end fun
"#);

    let compiled1 = pipeline.compile_fresh(&db);
    assert!(compiled1.is_successful(), "first compilation failed: {:?}", compiled1.all_errors());

    // First run: import and call module function.
    {
        let mut ctx = TestContext::new(&compiled1, &db);
        let r = ctx.eval_fragment("require module local/pkg/v1\nimport v1.value");
        assert!(matches!(r.typecheck, TypecheckResult::Success), "import failed: {:?}", r.typecheck);
        let result = ctx.eval_expr("value(100)");
        assert_eq!(result.output, "100");
        ctx.destroy_all();
    }

    // Create a second context from the same compilation.
    {
        let mut ctx2 = TestContext::new(&compiled1, &db);
        let r = ctx2.eval_fragment("require module local/pkg/v1\nimport v1.value");
        assert!(matches!(r.typecheck, TypecheckResult::Success), "second import failed: {:?}", r.typecheck);
        let result = ctx2.eval_expr("value(200)");
        assert_eq!(result.output, "200");
        ctx2.destroy_all();
    }
}

/// Test that unit functions are isolated between contexts.
#[test]
fn test_isolated_unit_functions() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful());

    let mut ctx1 = TestContext::new(&compiled, &db);
    let mut ctx2 = TestContext::new(&compiled, &db);

    // Define functions with the same name returning different values.
    let r1_def = ctx1.eval_fragment("fun local_fn(x: u32): u32\n  ret 10\nend fun");
    assert!(matches!(r1_def.typecheck, TypecheckResult::Success), "ctx1 fn def failed: {:?}", r1_def.typecheck);

    let r2_def = ctx2.eval_fragment("fun local_fn(x: u32): u32\n  ret 20\nend fun");
    assert!(matches!(r2_def.typecheck, TypecheckResult::Success), "ctx2 fn def failed: {:?}", r2_def.typecheck);

    // Local functions are isolated to their context.
    let r1_local = ctx1.eval_expr("local_fn(5)");
    assert!(matches!(r1_local.typecheck, TypecheckResult::Success), "ctx1 fn call failed: {:?}", r1_local.typecheck);
    assert_eq!(r1_local.output, "10");

    let r2_local = ctx2.eval_expr("local_fn(5)");
    assert!(matches!(r2_local.typecheck, TypecheckResult::Success), "ctx2 fn call failed: {:?}", r2_local.typecheck);
    assert_eq!(r2_local.output, "20");

    ctx1.destroy_all();
    ctx2.destroy_all();
}

/// Test creating many script contexts doesn't cause issues.
#[test]
fn test_many_script_contexts() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful());

    // Create many contexts.
    for i in 0..20u32 {
        let mut ctx = TestContext::new(&compiled, &db);
        let r = ctx.eval_fragment("fun id(x: u32): u32\n  ret x\nend fun");
        assert!(matches!(r.typecheck, TypecheckResult::Success), "fn def failed: {:?}", r.typecheck);
        let result = ctx.eval_expr(&format!("id({})", i));
        assert_eq!(result.output, format!("{}", i));
        ctx.destroy_all();
    }
}

/// Test that module functions are shared across contexts.
#[test]
fn test_shared_module_functions() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();

    pipeline.add_module(&db, "local", "pkg", "math", r#"
fun id(x: u32): u32
  ret x
end fun
"#);

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "compilation failed: {:?}", compiled.all_errors());

    // Create two contexts that both use the module function.
    let mut ctx1 = TestContext::new(&compiled, &db);
    let mut ctx2 = TestContext::new(&compiled, &db);

    // Both contexts import and use the module function.
    let r1 = ctx1.eval_fragment("require module local/pkg/math\nimport math.id");
    assert!(matches!(r1.typecheck, TypecheckResult::Success), "ctx1 import failed: {:?}", r1.typecheck);
    let r2 = ctx2.eval_fragment("require module local/pkg/math\nimport math.id");
    assert!(matches!(r2.typecheck, TypecheckResult::Success), "ctx2 import failed: {:?}", r2.typecheck);

    let r1 = ctx1.eval_expr("id(10)");
    assert_eq!(r1.output, "10");

    let r2 = ctx2.eval_expr("id(20)");
    assert_eq!(r2.output, "20");

    ctx1.destroy_all();
    ctx2.destroy_all();
}

/// Verify per-unit memoization: prior units are cached when adding new units.
///
/// This test verifies the behavioral correctness of the per-unit memoization
/// implementation. While we can't directly observe cache hits, we verify that:
/// 1. Each unit typechecks successfully
/// 2. Bindings from prior units are available in subsequent units
/// 3. Values are computed correctly across unit boundaries
#[test]
fn test_per_unit_memoization_behavior() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);
    let mut ctx = TestContext::new(&compiled, &db);

    // Unit 1: define x.
    let r1 = ctx.eval_fragment("let x: u32 = 10");
    assert!(matches!(r1.typecheck, TypecheckResult::Success),
        "unit 1 failed: {:?}", r1.typecheck);

    // Unit 2: define y using x (assignment, not arithmetic).
    let r2 = ctx.eval_fragment("let y: u32 = x");
    assert!(matches!(r2.typecheck, TypecheckResult::Success),
        "unit 2 failed: {:?}", r2.typecheck);

    // Unit 3: define z using y.
    let r3 = ctx.eval_fragment("let z: u32 = y");
    assert!(matches!(r3.typecheck, TypecheckResult::Success),
        "unit 3 failed: {:?}", r3.typecheck);

    // Verify values propagate correctly.
    let rx = ctx.eval_expr("x");
    assert_eq!(rx.output, "10");
    let ry = ctx.eval_expr("y");
    assert_eq!(ry.output, "10");
    let rz = ctx.eval_expr("z");
    assert_eq!(rz.output, "10");

    ctx.destroy_all();
}

/// Test that functions defined in earlier units are available in later units.
#[test]
fn test_per_unit_function_propagation() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);
    let mut ctx = TestContext::new(&compiled, &db);

    // Unit 1: define an identity function.
    let r1 = ctx.eval_fragment("fun id(n: u32): u32\n  ret n\nend fun");
    assert!(matches!(r1.typecheck, TypecheckResult::Success),
        "function def failed: {:?}", r1.typecheck);

    // Unit 2: use the function.
    let r2 = ctx.eval_fragment("let a = id(42)");
    assert!(matches!(r2.typecheck, TypecheckResult::Success),
        "function use failed: {:?}", r2.typecheck);

    // Unit 3: define another function that calls the first.
    let r3 = ctx.eval_fragment("fun id2(n: u32): u32\n  ret id(n)\nend fun");
    assert!(matches!(r3.typecheck, TypecheckResult::Success),
        "nested function def failed: {:?}", r3.typecheck);

    // Verify values.
    let ra = ctx.eval_expr("a");
    assert_eq!(ra.output, "42");

    let ri2 = ctx.eval_expr("id2(99)");
    assert_eq!(ri2.output, "99");

    ctx.destroy_all();
}

/// Test per-unit lowering memoization with cross-unit function calls.
///
/// Verifies that ownership analysis and IR lowering work correctly with
/// accumulated bindings from prior units, and that memoization doesn't
/// break cross-unit function resolution.
#[test]
fn test_per_unit_lowering_memoization() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);
    let mut ctx = TestContext::new(&compiled, &db);

    // Unit 1: define a function that returns a list (ownership implications).
    let r1 = ctx.eval_fragment("fun make_list(n: u32): [u32]\n  ret [n, n]\nend fun");
    assert!(matches!(r1.typecheck, TypecheckResult::Success),
        "make_list function failed: {:?}", r1.typecheck);

    // Unit 2: call the function and store result.
    let r2 = ctx.eval_fragment("let items = make_list(42)");
    assert!(matches!(r2.typecheck, TypecheckResult::Success),
        "items failed: {:?}", r2.typecheck);

    // Unit 3: define another function that uses the first.
    let r3 = ctx.eval_fragment("fun make_double(n: u32): [u32]\n  ret make_list(n)\nend fun");
    assert!(matches!(r3.typecheck, TypecheckResult::Success),
        "make_double failed: {:?}", r3.typecheck);

    // Verify values.
    let ri = ctx.eval_expr("items");
    assert_eq!(ri.output, "[42, 42]");

    let rd = ctx.eval_expr("make_double(10)");
    assert_eq!(rd.output, "[10, 10]");

    ctx.destroy_all();
}

/// Regression test: function lowering must not corrupt script tracking state.
///
/// Previously, when lowering functions defined inside a script unit, the
/// function's tracking state (from its parameters) would overwrite the
/// script's tracking state. This caused script-level non-Copy bindings
/// to appear as "Copy" and not be included in tracked_values, leading to
/// memory leaks when destroy_all skipped them.
///
/// This test defines functions with parameters (which create tracking entries)
/// followed by a let binding of a non-Copy type (list). If the tracking state
/// is corrupted, the list won't be properly tracked and will leak.
#[test]
fn test_function_lowering_preserves_script_tracking() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);
    let mut ctx = TestContext::new(&compiled, &db);

    // Define functions with parameters, then a non-Copy let binding.
    // The bug was that the function parameters' tracking categories
    // would overwrite the script's tracking, making the list appear
    // as Copy and not get destroyed.
    let r = ctx.eval_fragment(r#"
fun get_value(): int
    ret 42
end fun

fun process(a: int, b: int): int
    ret a + b
end fun

let my_list: [int] = [1, 2, 3, 4]
debuglog my_list
debuglog process(get_value(), 10)
"#);
    assert!(matches!(r.typecheck, TypecheckResult::Success),
        "fragment failed: {:?}", r.typecheck);

    // This will panic with a leak if my_list wasn't properly tracked.
    ctx.destroy_all();
}

/// Similar regression test with Option<String> to ensure wrapper types are tracked.
#[test]
fn test_function_lowering_preserves_option_tracking() {
    let db = make_db();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);
    let mut ctx = TestContext::new(&compiled, &db);

    // Functions with Option parameters and return types.
    // The key is that after processing function params, we have a
    // non-Copy let binding (string) that must be tracked.
    let r = ctx.eval_fragment(r#"
fun make_opt(n: int): ?int
    ret some n
end fun

fun unwrap_or(opt: ?int, default: int): int
    if opt |val|
        ret val
    end if
    ret default
end fun

let my_string: string = "hello world"
let result = unwrap_or(make_opt(100), 0)
debuglog my_string
"#);
    assert!(matches!(r.typecheck, TypecheckResult::Success),
        "fragment failed: {:?}", r.typecheck);

    // This will panic with a leak if my_string wasn't properly tracked.
    ctx.destroy_all();
}
