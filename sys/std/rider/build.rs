//! Generates the table of native functions this rider exports.
//!
//! The names come from `rider.dli`, the rider's interface: every `native fun`
//! declared there has a `dlr_std__`-prefixed function in `src/lib.rs`. A
//! datalove binary that links this crate reads the generated table to find
//! those functions by linker symbol, the same names the compiler emits calls
//! to. A declared function that is not implemented fails to compile here.

use std::fmt::Write as _;
use std::path::Path;

const INTERFACE: &str = "../rider.dli";

fn main() {
    println!("cargo:rerun-if-changed={INTERFACE}");

    let source = std::fs::read_to_string(INTERFACE)
        .unwrap_or_else(|e| panic!("unable to read {INTERFACE}: {e}"));

    let mut table = String::new();
    table.push_str(
        "/// Address of every function this rider exports, by linker symbol.\n\
         ///\n\
         /// Generated from `rider.dli` by `build.rs`.\n\
         pub fn symbols() -> Vec<(&'static str, *const ())> {\n\
         \x20   vec![\n",
    );

    for name in native_fun_names(&source) {
        writeln!(
            table,
            "        (\"dlr_std__{name}\", dlr_std__{name} as *const ()),"
        )
        .expect("writing to a string");
    }

    table.push_str("    ]\n}\n");

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("symbols.rs");
    std::fs::write(&out, table)
        .unwrap_or_else(|e| panic!("unable to write {}: {e}", out.display()));
}

/// The name of every `native fun` the interface declares, in source order.
///
/// A declaration is `native fun name(...)` or `native fun name<T>(...)`.
fn native_fun_names(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("native fun "))
        .map(|rest| {
            let end = rest
                .find(|c| c == '<' || c == '(')
                .unwrap_or_else(|| panic!("native fun declaration has no parameters: {rest}"));
            rest[..end].trim()
        })
        .collect()
}
