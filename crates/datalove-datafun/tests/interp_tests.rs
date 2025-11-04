//! Tests for the new analysis-driven interpreter.

use datalove_datafun as datafun;
use datafun::Database;
use bct::input::Source;

#[test]
fn test_interp_function_definition() {
    // Set leak check to ignore since we haven't implemented cleanup yet.
    unsafe {
        std::env::set_var("DATALOVE_LEAK_CHECK", "ignore");
    }

    // Test that we can define a function at script level.
    let db = Database::default();

    let source_text = r#"
fun test(): u32
    ret @42
end fun
"#;

    // Create a script with one unit.
    let source = Source::new(&db, source_text.to_string());
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    // Create an empty package world.
    let empty_sys = rmx::std::collections::BTreeMap::new();
    let empty_local = rmx::std::collections::BTreeMap::new();
    let package_world = datafun::package::PackageWorld::new(&db, empty_sys, empty_local);

    // Try to execute the script (should fail with "no output variable" but should define the function).
    let result = datafun::interp::execute_script(&db, script, package_world);

    // We expect NoOutputVariable error since we didn't define an output variable.
    assert!(matches!(result, Err(datafun::interp::InterpError::NoOutputVariable)));
}

#[test]
fn test_interp_empty_script() {
    unsafe { std::env::set_var("DATALOVE_LEAK_CHECK", "ignore"); }

    // Test that an empty script fails with NoOutputVariable.
    let db = Database::default();

    let source_text = "";

    // Create a script with one unit.
    let source = Source::new(&db, source_text.to_string());
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    // Create an empty package world.
    let empty_sys = rmx::std::collections::BTreeMap::new();
    let empty_local = rmx::std::collections::BTreeMap::new();
    let package_world = datafun::package::PackageWorld::new(&db, empty_sys, empty_local);

    // Try to execute the script.
    let result = datafun::interp::execute_script(&db, script, package_world);

    // We expect NoOutputVariable error.
    assert!(matches!(result, Err(datafun::interp::InterpError::NoOutputVariable)));
}

#[test]
fn test_interp_u32_literal() {
    unsafe { std::env::set_var("DATALOVE_LEAK_CHECK", "ignore"); }

    // Test that we can evaluate a simple u32 literal.
    let db = Database::default();

    let source_text = r#"
let output = @42
"#;

    // Create a script with one unit.
    let source = Source::new(&db, source_text.to_string());
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    // Create an empty package world.
    let empty_sys = rmx::std::collections::BTreeMap::new();
    let empty_local = rmx::std::collections::BTreeMap::new();
    let package_world = datafun::package::PackageWorld::new(&db, empty_sys, empty_local);

    // Execute the script.
    let result = datafun::interp::execute_script(&db, script, package_world);

    // Should succeed.
    assert!(result.is_ok(), "Script execution failed: {:?}", result);

    // Check the value.
    let script_result = result.unwrap();
    let value_u32 = unsafe { *(script_result.value.ptr as *const u32) };
    assert_eq!(value_u32, 42);
}

#[test]
fn test_interp_bool_literals() {
    unsafe { std::env::set_var("DATALOVE_LEAK_CHECK", "ignore"); }

    // Test that we can evaluate boolean literals.
    let db = Database::default();

    // Test true.
    let source_text_true = r#"
let output = @true
"#;

    let source = Source::new(&db, source_text_true.to_string());
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    let empty_sys = rmx::std::collections::BTreeMap::new();
    let empty_local = rmx::std::collections::BTreeMap::new();
    let package_world = datafun::package::PackageWorld::new(&db, empty_sys, empty_local);

    let result = datafun::interp::execute_script(&db, script, package_world);
    assert!(result.is_ok(), "Script execution failed: {:?}", result);

    let script_result = result.unwrap();
    let value_bool = unsafe { *script_result.value.ptr };
    assert_eq!(value_bool, 1);

    // Test false.
    let db2 = Database::default();
    let source_text_false = r#"
let output = @false
"#;

    let source2 = Source::new(&db2, source_text_false.to_string());
    let unit2 = datafun::script::ScriptUnit::new(&db2, source2);
    let script2 = datafun::script::Script::new(&db2, vec![unit2]);

    let empty_sys2 = rmx::std::collections::BTreeMap::new();
    let empty_local2 = rmx::std::collections::BTreeMap::new();
    let package_world2 = datafun::package::PackageWorld::new(&db2, empty_sys2, empty_local2);

    let result2 = datafun::interp::execute_script(&db2, script2, package_world2);
    assert!(result2.is_ok(), "Script execution failed: {:?}", result2);

    let script_result2 = result2.unwrap();
    let value_bool2 = unsafe { *script_result2.value.ptr };
    assert_eq!(value_bool2, 0);
}

#[test]
fn test_interp_string_literal() {
    unsafe { std::env::set_var("DATALOVE_LEAK_CHECK", "ignore"); }

    // Test that we can evaluate a string literal.
    let db = Database::default();

    let source_text = r#"
let output = "hello"
"#;

    let source = Source::new(&db, source_text.to_string());
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    let empty_sys = rmx::std::collections::BTreeMap::new();
    let empty_local = rmx::std::collections::BTreeMap::new();
    let package_world = datafun::package::PackageWorld::new(&db, empty_sys, empty_local);

    let result = datafun::interp::execute_script(&db, script, package_world);
    assert!(result.is_ok(), "Script execution failed: {:?}", result);

    // Check the value is a valid string pointer.
    let script_result = result.unwrap();
    let string_ptr = script_result.value.ptr as *const datalove_rtdt::String;
    assert!(!string_ptr.is_null());

    // Verify it's a string type.
    unsafe {
        let string_ref = &*string_ptr;
        assert_eq!(string_ref.size, 5);  // "hello" is 5 bytes.
    }
}
