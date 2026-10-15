//! Parse every system library module and nothing else, for counting what
//! parsing costs.
//!
//! A whole compile mixes parsing with everything after it, and a profile of
//! one splits it only as finely as its samples. This is the parse on its own:
//! each iteration a fresh database, so nothing is a memo hit, and each module
//! through `parse_module_full`, lexing and bracing included. Count it with
//! `perf stat -e instructions:u` and subtract a run with zero iterations.
//!
//! Scratch tool. `cargo run --release -p datalove-bench --example parse_only -- [iterations]`.

use rmx::prelude::*;

use bct::module_graph::{Module, ModuleId};
use datalove_datafun as datafun;

fn main() {
    let iterations: usize = std::env::args().nth(1).map_or(10, |n| n.parse().X());

    let sys = datalove_sys_packages::system_library();
    let modules: Vec<(String, String)> = sys.library.packages.iter()
        .flat_map(|(p, pkg)| pkg.modules.iter()
            .map(move |(m, d)| (fmt!("sys/{p}/{m}"), d.source.to_string())))
        .collect();
    let bytes: usize = modules.iter().map(|(_, text)| text.len()).sum();

    let mut statements = 0;
    for _ in 0..iterations {
        let db = datafun::Database::default();
        for (path, text) in &modules {
            let source = bct::input::Source::new(&db, text.C());
            let module = Module::new(&db, ModuleId::new(&db, path.C()), source);
            let parsed = datalove_datafun_parser::parse_module_full(&db, module);
            statements += parsed.parsed.statements(&db).len();
        }
        // The database is forgotten, as the CLI does, so dropping its tables
        // is not counted as parsing.
        std::mem::forget(db);
    }
    println!("{} modules, {bytes} bytes, {iterations} iterations, {statements} statements", modules.len());
}
