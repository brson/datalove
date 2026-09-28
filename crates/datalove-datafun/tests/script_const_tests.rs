//! Script-level consts, and where a function body may name one.
//!
//! **A script-level const is in scope for a function body**, in its own unit
//! and in the units that follow, the way a *module*-level const is in scope for
//! a module function body. The typechecker has always said so -- a const is the
//! one enclosing binding a function body may name -- and lowering now builds
//! it: the script pipeline lowers functions in two strata, as the module
//! pipeline does, so a body naming a const declared beside it is lowered once
//! the const has a value.
//!
//! It did not, for a while. A body naming a script const failed lowering with
//! `binding not available yet`, and the function was then never registered, so
//! a later call reported `UnresolvedName` -- the symptom two steps downstream of
//! the cause, which is what sent you looking in the wrong place.
//!
//! **Nothing covered this.** `interp_constlet` has fixtures for a const declared
//! *inside* a function, and for a script const read by a script `let`, and one
//! named `006_script_const_cross_unit` whose const and use are in the same unit.
//! None of them put a script-level const in a function body, and the whole
//! suite runs with `skip_const_inlining`, so the ordinary path for script consts
//! was thinly covered too. See `botdocs/plan-script-reactivity.md`.

use rmx::prelude::*;

use datalove_datafun as datafun;
use datalove_datafun::pipeline::{
    CompilerOptions, LoweringResult, ModuleCompilationPipeline, ScriptCompilationResult,
    TypecheckResult,
};

/// Compile `units` in order, returning every unit's result.
fn compile(units: &[&str]) -> Vec<ScriptCompilationResult> {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new(CompilerOptions::default());
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    let mut compiler = compiled.script_compiler_default(&db).expect("a script compiler");
    units.iter().map(|unit| compiler.compile_fragment(unit)).collect()
}

/// Compile and run `units` in order, then evaluate `expr`.
///
/// Returns every unit's result and what `expr` came to, so that a test can say
/// the value is right rather than only that lowering got through.
fn compile_and_eval(units: &[&str], expr: &str) -> (Vec<ScriptCompilationResult>, String) {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new(CompilerOptions::default());
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    let mut compiler = compiled.script_compiler_default(&db).expect("a script compiler");
    let mut executor = compiled
        .script_executor(datafun::DebugOutputMode::Disabled, None)
        .expect("a script executor");

    let mut results = Vec::new();
    let mut fragment_outputs = Vec::new();
    for unit in units {
        let result = compiler.compile_fragment(unit);
        if let Some(ir_unit) = &result.ir_unit {
            fragment_outputs.push(executor.execute_fragment(ir_unit));
        }
        results.push(result);
    }

    let evaluated = compiler.compile_expr(expr);
    let value = match &evaluated.ir_unit {
        Some(ir_unit) => executor.execute_expr(ir_unit).1,
        None => String::new(),
    };
    // Before anything can fail, so a failing assertion is the only thing the
    // test reports.
    executor.destroy_live_values();

    for (unit, output) in units.iter().zip(fragment_outputs.iter()) {
        assert!(!output.starts_with("Error:"), "running {unit:?}: {output}");
    }
    assert_typechecks(&evaluated, expr);
    assert_lowers(&evaluated, expr);
    (results, value)
}

fn assert_typechecks(result: &ScriptCompilationResult, what: &str) {
    assert!(
        matches!(result.typecheck, TypecheckResult::Success { .. }),
        "{what} should typecheck: {:?}", result.typecheck,
    );
}

fn assert_lowers(result: &ScriptCompilationResult, what: &str) {
    assert!(
        matches!(result.lowering, LoweringResult::Success { .. }),
        "{what} should lower: {:?}", result.lowering,
    );
}

// ============================================================================
// What works
// ============================================================================

/// A const declared inside a function body works.
///
/// This is `interp_constlet`'s `001_const_in_script_function`, here so that the
/// failures below are not mistaken for consts being broken in general.
#[test]
fn a_const_inside_a_function_body_works() {
    let results = compile(&["fun answer(): u32\n    const X: u32 = 42\n    ret X\nend fun\n"]);
    assert_typechecks(&results[0], "a const declared in the body");
    assert_lowers(&results[0], "a const declared in the body");
}

/// A script-level const read by a script-level `let` works, in the same unit.
#[test]
fn a_script_const_read_by_a_let_works() {
    let results = compile(&["const K: u32 = 5\nlet v = K\n"]);
    assert_typechecks(&results[0], "a let reading a script const");
    assert_lowers(&results[0], "a let reading a script const");
}

/// And across a unit boundary, which is what `006` is named for and does not do.
#[test]
fn a_script_const_read_by_a_later_unit_s_let_works() {
    let results = compile(&["const K: u32 = 5\n", "let v = K\n"]);
    assert_typechecks(&results[1], "a later unit's let reading a script const");
    assert_lowers(&results[1], "a later unit's let reading a script const");
}

// ============================================================================
// A function body naming a script-level const
// ============================================================================

/// A function body reads a script-level const declared beside it.
///
/// Same unit, so the const has no value when the body is first reached: the
/// body is held back, the const is evaluated, and the body is lowered against
/// it. The value has to come out right, not merely lower.
#[test]
fn a_function_body_can_read_a_script_const_in_its_own_unit() {
    let (results, value) = compile_and_eval(
        &["const K: u32 = 5\nfun f(): u32\n    ret K\nend fun\n"],
        "f()",
    );
    assert_typechecks(&results[0], "a body reading a const beside it");
    assert_lowers(&results[0], "a body reading a const beside it");
    assert_eq!(value, "5", "f() should return the const's value");
}

/// And from an earlier unit, whose const is still in scope.
#[test]
fn a_function_body_can_read_a_script_const_from_an_earlier_unit() {
    let (results, value) = compile_and_eval(
        &["const K: u32 = 5\n", "fun f(): u32\n    ret K\nend fun\n"],
        "f()",
    );
    assert_lowers(&results[0], "the unit declaring the const");
    assert_typechecks(&results[1], "a body reading an earlier unit's const");
    assert_lowers(&results[1], "a body reading an earlier unit's const");
    assert_eq!(value, "5", "f() should return the earlier unit's const value");
}

/// And so the function is callable, which is how this is usually met.
///
/// When the defining unit's lowering failed the function was never registered,
/// so a later call reported `UnresolvedName` -- the symptom two steps downstream
/// of the cause. The call resolves now, and the value reaches the caller.
#[test]
fn and_so_the_function_is_callable() {
    let (results, value) = compile_and_eval(
        &[
            "const K: u32 = 5\nfun f(): u32\n    ret K\nend fun\n",
            "let v = f()\n",
        ],
        "v",
    );
    assert_lowers(&results[0], "the defining unit");
    assert_typechecks(&results[1], "the calling unit");
    assert_lowers(&results[1], "the calling unit");
    assert_eq!(value, "5", "the call should return the const's value");
}

// ============================================================================
// The contrast that makes it a defect
// ============================================================================

/// A *module*-level const read by a module function body works.
///
/// The same shape as the failures above, in a module rather than a script, and
/// it compiles and runs. So the machinery exists and the script path does not
/// reach it: `lower_module_functions` is handed the module's consts, where the
/// script path passes a function only its own.
#[test]
fn a_module_const_read_by_a_module_function_works() {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new(CompilerOptions::default());
    pipeline.add_module(&db, "local", "test", "m",
        "const K: u32 = 5\n\nfun f(): u32\n    ret K\nend fun\n");

    let compiled = pipeline.compile_fresh(&db);
    assert!(
        compiled.is_successful(),
        "a module const in a module function body should compile: {:?}",
        compiled.all_errors(),
    );

    let mut compiler = compiled.script_compiler_default(&db).expect("a script compiler");
    let result = compiler.compile_fragment(
        "require module local/test/m\nimport m.f\n\nlet v = f()\n");
    assert_typechecks(&result, "calling a module function that reads a module const");
    assert_lowers(&result, "calling a module function that reads a module const");
}
