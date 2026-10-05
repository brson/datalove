//! End-to-end tests for native rider functions.
//!
//! Tests that modules can import and call native functions defined in rider
//! interface sections, with Rust implementations registered in the interpreter.

use rmx::prelude::*;
use std::sync::Arc;

use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;
use datalove_datafun_interp::{IrInterpreter, NativeFnImpl, NativeResolver, Value, Destination};
use datalove_datafun::pipeline::{ModuleCompilationPipeline, CompilerOptions};

#[test]
fn test_native_rider_int_add() {
    let result = run_main_with_int_add(r#"
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/test/main
----------
require rider testlib
import testlib.int_add

fun main(): i32
    ret int_add(3, 4)
end fun
"#);
    assert_eq!(result, 7, "int_add(3, 4) should return 7");
}

/// A comment after `require rider` is not part of the rider's name.
///
/// Riders were found by scanning module source a line at a time, which read
/// this as a rider named `testlib // the native half` and resolved nothing.
#[test]
fn test_native_rider_require_with_comment() {
    let result = run_main_with_int_add(r#"
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/test/main
----------
require rider testlib // the native half
import testlib.int_add

fun main(): i32
    ret int_add(3, 4)
end fun
"#);
    assert_eq!(result, 7, "int_add(3, 4) should return 7");
}

#[test]
fn test_native_rider_qualified_call() {
    let result = run_main_with_int_add(r#"
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/test/main
----------
require rider testlib

fun main(): i32
    ret testlib.int_add(3, 4)
end fun
"#);
    assert_eq!(result, 7, "testlib.int_add(3, 4) should return 7");
}

/// A module-level const calling a native, evaluated while the module compiles.
#[test]
fn test_native_rider_module_const() {
    let result = run_main(r#"
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/test/main
----------
require rider testlib
import testlib.int_add

const SEVEN: i32 = int_add(3, 4)

fun main(): i32
    ret SEVEN
end fun
"#, Some(Arc::new(IntAddResolver)));
    assert_eq!(result, Ok(7), "SEVEN should be int_add(3, 4)");
}

/// A native that cannot be found fails the const, not the compiler.
#[test]
fn test_native_rider_module_const_unresolved() {
    let result = run_main(r#"
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/test/main
----------
require rider testlib
import testlib.int_add

const SEVEN: i32 = int_add(3, 4)

fun main(): i32
    ret SEVEN
end fun
"#, Some(Arc::new(FailingResolver)));
    let errors = result.expect_err("an unresolvable native should fail compilation");
    assert!(errors.contains("the rider would not build"), "unexpected errors: {}", errors);
}

/// Resolves `int_add`, as a built rider would.
struct IntAddResolver;

impl NativeResolver for IntAddResolver {
    fn resolve(&self, symbol: &str) -> Result<NativeFnImpl, String> {
        assert_eq!(symbol, "dlr_testlib__int_add");
        Ok(int_add_impl())
    }
}

/// Resolves nothing, as a rider failing to build does.
struct FailingResolver;

impl NativeResolver for FailingResolver {
    fn resolve(&self, _symbol: &str) -> Result<NativeFnImpl, String> {
        Err("the rider would not build".S())
    }
}

/// The test rider's `int_add`.
fn int_add_impl() -> NativeFnImpl {
    Box::new(
        |_rt: datalove_rt::c::LocalRtHandle, args: &[Value], dest: Destination,
         _supplied: &[*const datalove_rtdt::TyDesc]| {
            unsafe {
                let a = *(args[0].ptr as *const i32);
                let b = *(args[1].ptr as *const i32);
                *(dest.ptr as *mut i32) = a + b;
            }
            Ok(())
        }
    )
}

/// Compile a worldfile whose `local/test/main` has an `i32` `main`, and run it
/// with the rider's `int_add` registered.
fn run_main_with_int_add(worldfile: &str) -> i32 {
    run_main(worldfile, None).expect("compilation failed")
}

/// Compile a worldfile whose `local/test/main` has an `i32` `main`, giving its
/// consts `natives`, and run it with the rider's `int_add` registered.
///
/// Errs with the compilation errors, if any.
fn run_main(worldfile: &str, natives: Option<Arc<dyn NativeResolver>>) -> Result<i32, String> {
    let mut db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(worldfile.as_bytes()).X();

    // Build pipeline from sections.
    let mut pipeline = ModuleCompilationPipeline::from_sections(&db, &parsed.sections, CompilerOptions::default());
    if let Some(natives) = natives {
        pipeline.set_natives(natives);
    }

    assert!(pipeline.contains_module("local", "test", "main"));

    // Compile.
    let (compiled, db) = pipeline.compile(&mut db);

    if compiled.has_errors() {
        return Err(compiled.all_errors().join("\n"));
    }

    // Find main function.
    let main_module_id = compiled.shared.module_graph.iter_modules(db)
        .find(|m| m.id(db).path(db) == "local/test/main")
        .map(|m| m.id(db))
        .expect("main module not found");

    let (main_ir_module_id, main_func_id) = compiled.shared.func_id_map
        .get(&(main_module_id, "main".S()))
        .expect("main function not found");

    let main_code_unit = compiled.shared.module_registry
        .get_module_function_as_unit(*main_ir_module_id, datalove_datafun_ir::CodeUnitId(main_func_id.0))
        .expect("main function not in registry")
        .clone();

    // Create interpreter with native function table.
    let mut interp = IrInterpreter::new();

    // Register the native int_add implementation.
    interp.native_table_mut().register("dlr_testlib__int_add", int_add_impl());

    // Allocate return buffer.
    let mut tydesc_table = datalove_datafun_interp::IrTyDescTable::new();
    let ret_ir_type = main_code_unit.return_type().expect("main has no return type");
    let ret_tydesc = tydesc_table.get_or_create(ret_ir_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    // Execute main.
    let mut env = datalove_datafun_interp::ScriptEnvironment::with_module_registry(
        Arc::clone(&compiled.shared.module_registry)
    );
    interp.call_with_env(&main_code_unit, Vec::new(), ret_dest, &env).expect("main failed");

    // Read the result.
    let result = unsafe { *(ret_buffer.as_ptr() as *const i32) };

    // Cleanup.
    env.destroy_live_values(interp.runtime_handle());
    Ok(result)
}
