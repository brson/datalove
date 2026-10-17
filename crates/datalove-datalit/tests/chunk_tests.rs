//! A collection parsed in chunks parses to what it parses to whole.
//!
//! Each case is parsed twice, on a database whose policy cuts nothing and on
//! one whose policy cuts every collection that can be cut after every element,
//! and the two are compared: the trees, less the numbers a parse gives its
//! expressions; the spans, as byte ranges; the parse diagnostics, in order;
//! and the type and the type diagnostics. See `bct::chunk`.

use rmx::prelude::*;
use rmx::serde_json::{Value, json};

use bct::chunk::{ChunkPolicy, CUT_EVERYTHING};
use bct::input::Source;
use datalove_datalit::{Database, ast_serde, parser, resolve, tycheck};
use datalove_diagnostic::{ParseDiagnostic, TypeDiagnostic};

/// Everything a parse and a typecheck of `text` say, and how many collections
/// the source was planned to cut.
fn analyze(text: &str, cut: bool) -> (Value, usize) {
    let db = Database::default();
    let thresholds = if cut { CUT_EVERYTHING.to_vec() } else { Vec::new() };
    ChunkPolicy::new(&db, thresholds);

    let source = Source::new(&db, text.S());
    let planned = bct::chunk::collection_plan(&db, bct::lexer::lex(&db, source)).len();
    let parsed = parser::parse(&db, source);
    let expr = parsed.expr(&db);

    let mut spans: Vec<(usize, usize)> = parsed.expr_spans(&db).iter()
        .map(|entry| (entry.span.start, entry.span.end))
        .collect();
    spans.sort();

    let diagnostics = |found: Vec<bct::diagnostic::Diagnostic<'_>>| -> Vec<Value> {
        found.iter()
            .map(|diag| json!({
                "code": diag.code.map(|c| c.as_str(&db).S()),
                "message": diag.message.as_str(&db),
                "labels": diag.labels.iter().map(|l| (l.span.start, l.span.end)).collect::<Vec<_>>(),
            }))
            .collect()
    };
    let parse_diagnostics = diagnostics(parser::parse::accumulated::<ParseDiagnostic>(&db, source)
        .iter().map(|d| d.to_diagnostic(&db)).collect());

    let resolved = resolve::resolve_names(&db, source, expr.clone());
    let checked = tycheck::type_check(&db, expr.clone(), resolved);
    let root_type = checked.root_type(&db).as_ref().map(|ty| tycheck::type_to_string(&db, ty));
    let type_diagnostics = diagnostics(tycheck::type_check::accumulated::<TypeDiagnostic>(&db, expr.clone(), resolved)
        .iter().map(|d| d.to_diagnostic(&db)).collect());

    let report = json!({
        "ast": ast_serde::ExprFull::from_ast(&db, expr),
        "spans": spans,
        "parse_diagnostics": parse_diagnostics,
        "type": root_type,
        "type_diagnostics": type_diagnostics,
    });
    (report, planned)
}

/// Parse `text` both ways and insist they agree, and that the cut parse did
/// cut `planned` collections.
fn check(text: &str, planned: usize) {
    let (whole, none) = analyze(text, false);
    let (cut, cut_count) = analyze(text, true);
    assert_eq!(none, 0, "a policy that cuts nothing planned a cut of {text:?}");
    assert_eq!(cut_count, planned, "how many collections of {text:?} were cut");
    assert_eq!(cut, whole, "{text:?} parsed in chunks is not what it parses to whole");
}

#[test]
fn lists() {
    check("[1, 2, 3, 4, 5]", 1);
    check("[1, 2, 3,]", 1);
    check("[\"a\", \"b\", 0x1f, -4, 2.5, true, none, some 3]", 1);
    check("[{a = 1, b = \"x\"}, {a = 2, b = \"y\"}, {a = 3, b = \"z\"}]", 1);
    check("[atom Red, atom Green, atom Blue]", 1);
}

#[test]
fn maps_and_sets() {
    check("%{1 = 2, 3 = 4, 5 = 6}", 1);
    check("%{\"a\" = [1, 2], \"b\" = [3]}", 1);
    check("#{1, 2, 3, 4}", 1);
    check("#{[1], [2, 3], []}", 1);
}

#[test]
fn only_the_outermost_is_cut() {
    check("[[1, 2], [3, 4], [5, 6]]", 1);
    check("%{1 = #{1, 2}, 2 = #{3, 4}}", 1);
}

#[test]
fn collections_inside_other_things() {
    check("{xs = [1, 2, 3], ys = [4, 5]}", 2);
    check(": [u32] / [1, 2, 3]", 1);
    check("(1, [2, 3, 4], #{5, 6})", 2);
    check("some [1, 2, 3]", 1);
}

#[test]
fn elements_with_their_own_types() {
    check("[: u32 / 1, : u64 / 2, : Alias / 3]", 1);
    check("[: [u32] / [1, 2], : [u32] / []]", 1);
}

#[test]
fn errors_inside_elements() {
    check("[1, 2x, 3, 4]", 1);
    check("[1, 0x, 3, -]", 1);
    check("[{a = }, {a = 1}, {a 2}]", 1);
    check("%{1 = 2, 3 4, 5 = 6}", 1);
    check("%{1 = 2, = 4, 5 = 6}", 1);
}

#[test]
fn a_missing_separator_stops_the_collection() {
    // Whole, the parse stops at the missing comma and complains of what is
    // left; cut, so does the chunk it falls in, and the chunks after it are
    // not used.
    check("[1, 2 3, 4, 5]", 1);
    check("[1, 2, 3, 4 5]", 1);
    // An error before it means the leftover is not complained of again.
    check("[1, 2x, 3 4, 5]", 1);
    check("%{1 = 2, 3 = 4 5 = 6, 7 = 8}", 1);
}

#[test]
fn empty_elements() {
    check("[1,, 2, 3]", 1);
    check("[1, 2, , 3]", 1);
    check("#{1, 2,,}", 1);
}

#[test]
fn types_that_do_not_check() {
    check(": [u8] / [1, 2, 300, 4]", 1);
    check(": [u32] / [1, \"two\", 3]", 1);
    check(": %{u32 = string} / %{1 = \"a\", 2 = 3}", 1);
}

/// A long list with something wrong every so often, cut after every element,
/// so that each kind of mistake falls on each side of a boundary.
#[test]
fn a_long_list_with_mistakes() {
    let elements: Vec<String> = (0..300)
        .map(|i| match i % 37 {
            5 => S("12z"),
            11 => fmt!("{{a = {i}, a = {i}}}"),
            19 => S("{a = }"),
            23 => fmt!(": u8 / {}", i * 10),
            _ => fmt!("{{a = {i}, b = [{i}, {}]}}", i + 1),
        })
        .collect();
    check(&fmt!("[{}]", elements.join(",\n  ")), 1);
}
