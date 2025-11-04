//! Tests for the new analysis-driven interpreter.

use datalove_datafun as datafun;
use datafun::Database;
use bct::input::Source;

#[test]
fn test_interp_function_definition() {
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
