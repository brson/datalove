//! The suite runs with leak checking on.
//!
//! A runtime checks for leaks only when `DATALOVE_LEAK_CHECK` asks it to, and
//! the justfile sets the variable on every recipe that runs tests. The fixtures
//! that exist to be checked by the leak detector, and the `free` checks built
//! on it, pass just the same without it. So this leaks through a runtime that
//! reads its mode from the environment, as every other test's does, and fails
//! if nothing notices: a recipe that lost the variable fails here.

use rmx::prelude::*;

use datalove_datalit::{Database, instantiate2};
use datalove_datalit::tydesc_table::TyDescTable;
use datalove_rt::rust::Runtime;

#[salsa::tracked(returns(copy))]
fn compile<'db>(db: &'db dyn salsa::Database, source: bct::input::Source) -> datalove_datalit::tycheck::TypecheckResult<'db> {
    let parse_result = datalove_datalit::parser::parse(db, source);
    let parsed = parse_result.expr(db);
    let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.clone());
    datalove_datalit::tycheck::type_check(db, parsed, resolved)
}

#[test]
#[should_panic(expected = "DATALOVE RUNTIME LEAK DETECTED")]
fn a_leak_panics_at_shutdown() {
    let db = Database::default();
    let source = bct::input::Source::new(&db, S(": u32 / 42"));
    let typechecked = compile(&db, source);
    let rt = Runtime::new();
    let mut tydesc_table = TyDescTable::new(&db);
    let _leaked = instantiate2::instantiate_value(&db, rt.handle(), &mut tydesc_table, typechecked).X();
    drop(rt);
}
