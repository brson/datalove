//! Datalit and datafun must agree on what a literal of a given type is.
//!
//! The two spell an expected type differently - datalit hangs it off the
//! literal as `: f64 / -3.9`, datafun annotates the binding - and they reach
//! it by different routes. Datalit carries a sign inside the literal token,
//! while datafun parses a negation as an operator over an unsigned literal,
//! so the expected type has to travel through one more node to arrive. That
//! difference is a place the two can drift apart, and have.

use rmx::prelude::*;

use datalove_datafun_resolve::resolve_script_names;

/// A type and a literal, checked against each other.
type Case = (&'static str, &'static str);

/// The cases the two languages have to answer the same way.
///
/// Negative floats are the ones that drifted; the integers around them are
/// here because the rule that decides them is the same rule.
const CASES: &[Case] = &[
    // Floats take a literal of either sign.
    ("f64", "3.9"),
    ("f64", "-3.9"),
    ("f64", "-0.0"),
    ("f32", "0.5"),
    ("f32", "-0.5"),
    // An exponent is part of the literal, in every shape it is written.
    ("f64", "1.0e300"),
    ("f64", "1.0E300"),
    ("f64", "2.5e-10"),
    ("f64", "2.5e+10"),
    ("f64", "1e300"),
    ("f64", "1e-7"),
    ("f64", "-1.0e-7"),
    ("f32", "1e30"),
    // No float takes an integer literal: the two families do not convert.
    ("f64", "5"),
    ("f64", "-5"),
    ("f32", "-5"),
    // Integers take what fits, with the sign counted in.
    ("int", "5"),
    ("int", "-5"),
    ("i32", "5"),
    ("i32", "-5"),
    ("i32", "2147483647"),
    ("i32", "2147483648"),
    ("i32", "-2147483648"),
    ("i32", "-2147483649"),
    ("i8", "-128"),
    ("i8", "-129"),
    ("i64", "-9223372036854775808"),
    ("u8", "255"),
    ("u8", "256"),
    ("u8", "-1"),
    ("u32", "5"),
    ("u32", "-5"),
    ("index", "-5"),
];

/// The name of each error a typechecker raised, without its payload.
///
/// The two report the same conditions through separate error types, so the
/// variant is what can be compared; the payloads name the same types in
/// different words.
fn error_kinds(errors: Vec<String>) -> Vec<String> {
    errors.into_iter()
        .map(|error| error.split(['{', '(', ' ']).next().unwrap_or(&error).to_string())
        .collect()
}

fn datalit_errors(db: &datalove_datafun_compiler::Database, ty: &str, literal: &str) -> Vec<String> {
    let src = bct::input::Source::new(db, format!(": {ty} / {literal}"));
    let parsed = datalove_datalit::parser::parse_integration_test(db, src);
    let resolved = datalove_datalit::resolve::resolve_names(db, src, parsed);
    let result = datalove_datalit::tycheck::type_check(db, parsed, resolved);

    error_kinds(result.errors(db).iter().map(|e| format!("{:?}", e.error(db))).collect())
}

fn datafun_errors(db: &datalove_datafun_compiler::Database, ty: &str, literal: &str) -> Vec<String> {
    let src = bct::input::Source::new(db, format!("let x: {ty} = {literal}"));
    let script = datalove_datafun_parser::parse_integration_test(db, src);
    let spans = datalove_datafun_parser::datafun_spans(db, src);
    let names = resolve_script_names(db, src, script.clone());
    let result = datalove_datafun_tycheck::type_check_single_script(db, src, spans, script.clone(), names);

    error_kinds(result.errors(db).iter().map(|e| format!("{:?}", e.error(db))).collect())
}

#[test]
fn literal_types_agree_between_datalit_and_datafun() {
    let db = datalove_datafun_compiler::Database::default();

    let mut disagreements = Vec::new();
    for (ty, literal) in CASES {
        let datalit = datalit_errors(&db, ty, literal);
        let datafun = datafun_errors(&db, ty, literal);
        if datalit != datafun {
            disagreements.push(format!(
                "  {ty} / {literal}: datalit {datalit:?}, datafun {datafun:?}"
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "datalit and datafun disagree on {} of {} literals:\n{}",
        disagreements.len(),
        CASES.len(),
        disagreements.join("\n"),
    );
}

/// The cases above are only worth comparing if they reach a verdict at all.
///
/// Two typecheckers that both fell over would agree perfectly, so this pins
/// down which of them accept and which reject.
#[test]
fn literal_types_are_decided_as_expected() {
    let db = datalove_datafun_compiler::Database::default();

    let accepted: Vec<Case> = CASES.iter()
        .filter(|(ty, literal)| datafun_errors(&db, ty, literal).is_empty())
        .copied()
        .collect();

    assert_eq!(
        accepted,
        vec![
            ("f64", "3.9"),
            ("f64", "-3.9"),
            ("f64", "-0.0"),
            ("f32", "0.5"),
            ("f32", "-0.5"),
            ("f64", "1.0e300"),
            ("f64", "1.0E300"),
            ("f64", "2.5e-10"),
            ("f64", "2.5e+10"),
            ("f64", "1e300"),
            ("f64", "1e-7"),
            ("f64", "-1.0e-7"),
            ("f32", "1e30"),
            ("int", "5"),
            ("int", "-5"),
            ("i32", "5"),
            ("i32", "-5"),
            ("i32", "2147483647"),
            ("i32", "-2147483648"),
            ("i8", "-128"),
            ("i64", "-9223372036854775808"),
            ("u8", "255"),
            ("u32", "5"),
        ],
    );
}
