//! Script-level consts, and the one place they do not reach.
//!
//! **A script-level const is never available to a function body.** Not across a
//! unit boundary and not even in the same unit. A *module*-level const in a
//! module function body is fine, which is what makes this a defect rather than
//! an unimplemented feature: the shapes are the same and one of them works.
//!
//! The typechecker agrees the program is legal -- a const is the one enclosing
//! binding a function body may name -- and lowering then cannot build it,
//! reporting `binding not available yet`. The cause is that module function
//! lowering is handed the module's consts, where script function lowering is
//! handed only the function's own; a script-level const is never passed in.
//!
//! **Nothing covered this.** `interp_constlet` has fixtures for a const declared
//! *inside* a function, and for a script const read by a script `let`, and one
//! named `006_script_const_cross_unit` whose const and use are in the same unit.
//! None of them puts a script-level const in a function body, and the whole
//! suite runs with `skip_const_inlining`, so the ordinary path for script consts
//! was thinly covered too.
//!
//! These assert what happens today, failures included, so that fixing it fails
//! these tests and says which ones to tighten. See
//! `botdocs/plan-script-reactivity.md`.

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

/// Lowering failed, naming `binding`. Tighten this when the gap closes.
fn assert_lowering_cannot_reach(result: &ScriptCompilationResult, binding: &str, what: &str) {
    match &result.lowering {
        LoweringResult::Error { message } => assert!(
            message.contains(binding),
            "{what}: expected lowering to fail on {binding}, got {message}",
        ),
        other => panic!(
            "{what} lowers now, so this gap has closed -- tighten this test: {other:?}",
        ),
    }
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
// What does not
// ============================================================================

/// **The defect.** A function body cannot read a script-level const, even in the
/// same unit.
///
/// Same unit, so this is not about crossing a boundary. A script-level const is
/// simply not among what a function body is lowered against.
#[test]
fn a_function_body_cannot_read_a_script_const_in_its_own_unit() {
    let results = compile(&["const K: u32 = 5\nfun f(): u32\n    ret K\nend fun\n"]);
    assert_typechecks(&results[0], "a body reading a const beside it");
    assert_lowering_cannot_reach(&results[0], "K", "a body reading a const beside it");
}

/// Nor from an earlier unit.
#[test]
fn a_function_body_cannot_read_a_script_const_from_an_earlier_unit() {
    let results = compile(&["const K: u32 = 5\n", "fun f(): u32\n    ret K\nend fun\n"]);
    assert_lowers(&results[0], "the unit declaring the const");
    assert_typechecks(&results[1], "a body reading an earlier unit's const");
    assert_lowering_cannot_reach(&results[1], "K", "a body reading an earlier unit's const");
}

/// And the function is then not callable, which is how this is usually met.
///
/// The defining unit's lowering failed, so the function was never registered,
/// so the call has nothing to resolve. The `UnresolvedName` is the symptom two
/// steps downstream of the cause, which is worth knowing because it is what
/// sends you looking in the wrong place.
#[test]
fn and_so_the_function_is_not_callable() {
    let results = compile(&[
        "const K: u32 = 5\nfun f(): u32\n    ret K\nend fun\n",
        "let v = f()\n",
    ]);
    assert_lowering_cannot_reach(&results[0], "K", "the defining unit");
    match &results[1].typecheck {
        TypecheckResult::Error { errors } => assert!(
            errors.iter().any(|e| e.contains("f")),
            "expected the call not to resolve: {errors:?}",
        ),
        other => panic!("the call resolved, so the gap has closed: {other:?}"),
    }
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
