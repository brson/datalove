//! A rider library built against a different runtime interface is refused.
//!
//! Nothing else in the suite can catch this going wrong. A rider reaches the
//! runtime through a table it finds at run time, so its library resolves
//! nothing when it loads and no link step fails when the two sides disagree;
//! the check in `rider_load` is the only thing between a mismatch and a call
//! made with the wrong arguments. A guard nobody exercises is worth nothing,
//! so these build libraries that lie about their interface and confirm they
//! are turned away.

use std::path::{Path, PathBuf};
use std::process::Command;

use datalove_datafun::pipeline::rider_load::load_rider_library;
use datalove_datafun_interp::NativeFunctionTable;

/// Compile one source file to a shared library in `dir`, and return its path.
fn cdylib(dir: &Path, name: &str, source: &str) -> PathBuf {
    let src_path = dir.join(format!("{name}.rs"));
    std::fs::write(&src_path, source).expect("writing the source");

    let out = dir.join(format!("lib{name}.so"));
    let status = Command::new("rustc")
        .arg("--edition").arg("2021")
        .arg("--crate-type").arg("cdylib")
        .arg("--crate-name").arg(name)
        .arg("-o").arg(&out)
        .arg(&src_path)
        .status()
        .expect("running rustc");
    assert!(status.success(), "rustc failed to build {name}");

    out
}

/// A library claiming an interface that is not this one is refused.
#[test]
fn a_different_interface_is_refused() {
    let dir = rmx::tempfile::tempdir().expect("a temp dir");

    // Deliberately not this datalove's value. A rider exporting this is one
    // built against a table of some other shape.
    let wrong = datalove_rti::ABI_VERSION ^ 1;
    let lib = cdylib(dir.path(), "wrong_abi", &format!(
        "#[no_mangle] pub static DLR_ABI_VERSION: u64 = {wrong};\n\
         #[no_mangle] pub extern \"C\" fn dlr_test__nothing() {{}}\n"
    ));

    let mut table = NativeFunctionTable::new();
    let result = load_rider_library(
        &lib, "test", &["dlr_test__nothing".to_string()], &mut table);

    let error = match result {
        Ok(_) => panic!("a mismatched library must be refused"),
        Err(e) => e,
    };
    let message = format!("{:#}", error);
    assert!(
        message.contains("different runtime interface"),
        "the error should say the interface differs, got: {message}",
    );
    // Both values belong in the message: knowing only that they differ
    // leaves nobody able to say which side is the old one.
    assert!(
        message.contains(&format!("{:#018x}", wrong))
            && message.contains(&format!("{:#018x}", datalove_rti::ABI_VERSION)),
        "the error should name both versions, got: {message}",
    );
}

/// A library that says nothing about its interface is refused too.
///
/// Silence is not agreement. It means a library built before the check
/// existed, or one that is not a rider library at all.
#[test]
fn saying_nothing_is_refused() {
    let dir = rmx::tempfile::tempdir().expect("a temp dir");

    let lib = cdylib(dir.path(), "silent_abi",
        "#[no_mangle] pub extern \"C\" fn dlr_test__nothing() {}\n");

    let mut table = NativeFunctionTable::new();
    let result = load_rider_library(
        &lib, "test", &["dlr_test__nothing".to_string()], &mut table);

    let error = match result {
        Ok(_) => panic!("a library with no version must be refused"),
        Err(e) => e,
    };
    let message = format!("{:#}", error);
    assert!(
        message.contains("does not say which runtime interface"),
        "the error should say the library is silent, got: {message}",
    );
}

/// The matching case loads, so the two tests above are refusing for the
/// reason they claim rather than because nothing ever loads.
#[test]
fn the_same_interface_loads() {
    let dir = rmx::tempfile::tempdir().expect("a temp dir");

    let lib = cdylib(dir.path(), "right_abi", &format!(
        "#[no_mangle] pub static DLR_ABI_VERSION: u64 = {};\n\
         #[no_mangle] pub extern \"C\" fn dlr_test__nothing() {{}}\n",
        datalove_rti::ABI_VERSION,
    ));

    let mut table = NativeFunctionTable::new();
    let loaded = load_rider_library(
        &lib, "test", &["dlr_test__nothing".to_string()], &mut table);

    assert!(loaded.is_ok(), "a matching library must load");
}
