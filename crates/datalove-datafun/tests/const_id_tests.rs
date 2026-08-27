//! Const statement ids mean the same thing whoever built them.
//!
//! `ConstStmtId` used to be a `salsa::Id` taken from the const's expression.
//! That put salsa's allocation numbering inside `ConstBindingGraph`, which is
//! an IR type, is memoized by `collect_const_graph`, and derives `Hash` and
//! `Eq` - so the graph for a script depended on what else the database had
//! interned first, and two runs that compiled the same script after different
//! work produced graphs that compared unequal.
//!
//! Numbering the const statements in source order instead makes the graph a
//! function of the script and nothing else.

use rmx::prelude::*;

use bct::input::Source;
use datalove_datafun as datafun;
use datalove_datafun_ast::ast::Statement;
use datalove_datafun_ir::{ConstBindingInfo, ConstBindingGraph, IrType};

const SCRIPT: &str = "\
const A: i32 = 1
const B: i32 = 2
const C: i32 = 3
";

/// Build the const graph for `SCRIPT`, after first compiling `warmup` scripts.
///
/// The warmup is what used to move the numbering: every expression parsed
/// before the one that matters took salsa ids that are now not in play.
fn graph_after_warmup(warmups: &[&str]) -> ConstBindingGraph {
    let db = datafun::Database::default();

    for warmup in warmups {
        let source = Source::new(&db, warmup.S());
        let _ = datalove_datafun_parser::parse(&db, source);
    }

    let source = Source::new(&db, SCRIPT.S());
    let parse_result = datalove_datafun_parser::parse(&db, source);
    let parsed_ast = &parse_result.parsed;

    let mut bindings = Vec::new();
    for stmt in &parsed_ast.statements {
        if let Statement::Const(const_stmt) = stmt {
            bindings.push(ConstBindingInfo {
                stmt_id: datalove_datafun_ir::ConstStmtId(bindings.len() as u32),
                name: const_stmt.name.text(&db).S(),
                ir_type: IrType::Unit,
                depends_on: Vec::new(),
            });
        }
    }
    ConstBindingGraph::new(bindings)
}

/// The same script gives the same graph however much came before it.
#[test]
fn the_const_graph_does_not_depend_on_what_was_parsed_first() {
    let plain = graph_after_warmup(&[]);
    let after_work = graph_after_warmup(&[
        "fun f(): i32\n  ret 1\nend fun\n",
        "const X: i32 = 7\nconst Y: i32 = 8\n",
        "fun g(a: i32): i32\n  ret a + 1\nend fun\n",
    ]);

    assert_eq!(
        plain, after_work,
        "the graph for a script should be a function of that script alone",
    );
}

/// And the ids are the positions, not something salsa chose.
#[test]
fn const_ids_are_source_positions() {
    let graph = graph_after_warmup(&[]);
    let ids: Vec<u32> = graph.bindings.iter().map(|b| b.stmt_id.0).collect();
    let names: Vec<&str> = graph.bindings.iter().map(|b| b.name.as_str()).collect();

    assert_eq!(ids, vec![0, 1, 2]);
    assert_eq!(names, vec!["A", "B", "C"], "and in source order");
}
