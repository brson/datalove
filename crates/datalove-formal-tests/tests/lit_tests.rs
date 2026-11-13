//! Datalit's literal typing held to the typing rules, through the Lean model.
//!
//! The model in `formal/Datalove/Lit.lean` is written from
//! `botdocs/datalit-typing-rules.md`, so where it and the Rust disagree, the
//! code and the doc do.
//!
//! Expressions come from `ast_gen`, which makes well-typed ones, and are then
//! mutated a few times at random: a hint dropped or replaced, a part swapped
//! for another expression, an integer or hex literal at a range's edge, a
//! variant renamed. Each is printed, parsed back, and synthesized, and what
//! the Rust synthesizes, or the code of the first diagnostic it reports, is
//! compared with the oracle's answer. A hinted expression `: T / e` is checked
//! against `T`, so both modes are asked about.
//!
//! Needs the oracle built, so it runs only when asked, as `oracle_tests` does:
//!
//! ```text
//! FORMAL_ORACLE_TEST=1 cargo test -p datalove-formal-tests --test lit_tests
//! ```
//!
//! Seeds come from `FORMAL_LIT_SEED` and `FORMAL_LIT_COUNT`.

use rmx::prelude::*;
use rmx::serde_json::{Value, json};
use bct::random::SeededRng;
use bct::text::InternedText;
use datalove_datalit::ast::*;
use datalove_datalit::ast_gen::{AstGenConfig, NumericStrategy, gen_expr_full_seeded, gen_type_hint};
use datalove_datalit::tycheck::Type;
use datalove_formal_tests::*;
use std::collections::BTreeMap;

const MAX_MUTATIONS: usize = 3;

const INTS: &[&str] = &[
    "0", "-1", "1", "127", "128", "-128", "-129", "255", "256", "32767", "-32769", "65535",
    "65536", "2147483647", "2147483648", "-2147483649", "4294967295", "4294967296",
    "9223372036854775807", "9223372036854775808", "-9223372036854775809",
    "18446744073709551615", "18446744073709551616", "123456789012345678901234567890",
];

const HEXES: &[&str] = &[
    "0x0", "0xff", "0x100", "0xffff", "0x10000", "0xffffffff", "0x100000000",
    "0xffffffffffffffff", "0x10000000000000000",
];

/// The expressions an expression is made of, one level down.
fn children<'a, 'db>(expr: &'a ExprFull<'db>) -> Vec<&'a ExprFull<'db>> {
    match expr.expr() {
        Expr::AnonTuple(t) => t.elements.iter().collect(),
        Expr::AnonStruct(s) => s.fields.iter().map(|f| &f.value).collect(),
        Expr::List(l) => l.elements.iter().collect(),
        Expr::Set(s) => s.elements.iter().collect(),
        Expr::Map(m) => m.entries.iter().flat_map(|e| [&e.key, &e.value]).collect(),
        Expr::Tensor(t) => t.elements.iter().collect(),
        Expr::Table(t) => t.rows.iter().flat_map(|r| r.elements.iter()).collect(),
        Expr::Some(s) => vec![&s.payload],
        Expr::Ok(o) => vec![&o.payload],
        Expr::Er(e) => vec![&e.payload],
        Expr::Data(d) => vec![&d.value],
        Expr::Error(e) => vec![&e.value],
        Expr::Term(t) => vec![&t.payload],
        Expr::Enum(e) => vec![&e.variant],
        Expr::Group(g) => vec![&g.inner],
        _ => vec![],
    }
}

/// How many expressions an expression holds, itself included.
fn count(expr: &ExprFull<'_>) -> usize {
    1 + children(expr).into_iter().map(count).sum::<usize>()
}

/// The expression with its `target`th part, in pre-order, replaced by `f` of
/// it.
fn rewrite<'db>(
    expr: &ExprFull<'db>,
    target: usize,
    counter: &mut usize,
    f: &mut dyn FnMut(&ExprFull<'db>) -> ExprFull<'db>,
) -> ExprFull<'db> {
    let here = *counter;
    *counter += 1;
    if here == target {
        return f(expr);
    }
    let mut each = |e: &ExprFull<'db>| rewrite(e, target, counter, f);
    let rebuilt = match expr.expr() {
        Expr::AnonTuple(t) => Expr::AnonTuple(ExprAnonTuple { elements: t.elements.iter().map(&mut each).collect() }),
        Expr::AnonStruct(s) => Expr::AnonStruct(ExprAnonStruct {
            fields: s.fields.iter().map(|fl| ExprStructField { name: fl.name, value: each(&fl.value) }).collect(),
        }),
        Expr::List(l) => Expr::List(ExprList { elements: l.elements.iter().map(&mut each).collect() }),
        Expr::Set(s) => Expr::Set(ExprSet { elements: s.elements.iter().map(&mut each).collect() }),
        Expr::Map(m) => Expr::Map(ExprMap {
            entries: m.entries.iter().map(|en| {
                let key = each(&en.key);
                ExprMapEntry { key, value: each(&en.value) }
            }).collect(),
        }),
        Expr::Tensor(t) => Expr::Tensor(ExprTensor {
            shape: t.shape.C(),
            elements: t.elements.iter().map(&mut each).collect(),
            header: t.header,
        }),
        Expr::Table(t) => Expr::Table(ExprTable {
            header: t.header.C(),
            rows: t.rows.iter()
                .map(|r| ExprTableRow { elements: r.elements.iter().map(&mut each).collect::<Vec<_>>().into() })
                .collect(),
        }),
        Expr::Some(s) => Expr::Some(ExprSome { payload: each(&s.payload) }),
        Expr::Ok(o) => Expr::Ok(ExprOk { payload: each(&o.payload) }),
        Expr::Er(e) => Expr::Er(ExprEr { payload: each(&e.payload) }),
        Expr::Data(d) => Expr::Data(ExprData { value: each(&d.value) }),
        Expr::Error(e) => Expr::Error(ExprError { value: each(&e.value) }),
        Expr::Term(t) => Expr::Term(ExprTerm { name: t.name, payload: each(&t.payload) }),
        Expr::Enum(e) => Expr::Enum(ExprEnum { variant: each(&e.variant) }),
        Expr::Group(g) => Expr::Group(ExprGroup { inner: each(&g.inner) }),
        other => other.C(),
    };
    ExprFull::new(expr.local_index(), expr.type_hint(), rebuilt)
}

/// A literal of a type, at most `depth` collections deep, so that a recursive
/// type's literal ends. Its parts carry no hints: checked against the type, a
/// literal takes its parts' types from it.
fn literal_of<'db>(db: &'db Db, rng: &mut SeededRng, ty: &Type<'db>, depth: u32) -> ExprFull<'db> {
    use datalove_datalit::tycheck::{expose, is_float_type, is_fixed_int_type};
    let text = |s: &str| InternedText::new(db, s.S());
    let full = |e: Expr<'db>| ExprFull::new(None, None, e);
    let many = |rng: &mut SeededRng| if depth == 0 { 0 } else { rng.random_range(0..3) };
    let ty = expose(ty).into_owned();
    let expr = match &ty {
        Type::Bool => Expr::True,
        t if is_fixed_int_type(t) || matches!(t, Type::Int) => Expr::Int(ExprInt { value: text("1") }),
        t if is_float_type(t) => Expr::Float(ExprFloat { value: text("1.5") }),
        Type::String => Expr::String(ExprString { value: text("\"s\"") }),
        Type::AnonTuple(t) => Expr::AnonTuple(ExprAnonTuple {
            elements: t.fields.iter().map(|f| literal_of(db, rng, f, depth)).collect(),
        }),
        Type::AnonStruct(st) => Expr::AnonStruct(ExprAnonStruct {
            fields: st.fields.iter().map(|f| ExprStructField { name: f.name, value: literal_of(db, rng, &f.ty, depth) }).collect(),
        }),
        Type::List(l) => Expr::List(ExprList {
            elements: (0..many(rng)).map(|_| literal_of(db, rng, &l.element_type, depth - 1)).collect(),
        }),
        Type::Set(st) => Expr::Set(ExprSet {
            elements: (0..many(rng).min(1)).map(|_| literal_of(db, rng, &st.element_type, depth - 1)).collect(),
        }),
        Type::Map(m) => Expr::Map(ExprMap {
            entries: (0..many(rng).min(1)).map(|_| ExprMapEntry {
                key: literal_of(db, rng, &m.key_type, depth - 1),
                value: literal_of(db, rng, &m.value_type, depth - 1),
            }).collect(),
        }),
        Type::Option(o) => if depth > 0 && rng.random_bool(0.5) {
            Expr::Some(ExprSome { payload: literal_of(db, rng, &o.inner_type, depth) })
        } else {
            Expr::None
        },
        Type::Result(r) => if rng.random_bool(0.6) {
            Expr::Ok(ExprOk { payload: literal_of(db, rng, &r.inner_type, depth) })
        } else {
            Expr::Er(ExprEr { payload: full(Expr::Error(ExprError { value: full(Expr::String(ExprString { value: text("\"e\"") })) })) })
        },
        Type::Tensor(t) => {
            let filled = depth > 0 && rng.random_bool(0.6);
            Expr::Tensor(ExprTensor {
                shape: vec![if filled { 1 } else { 0 }; t.rank as usize],
                elements: if filled { vec![literal_of(db, rng, &t.element_type, depth - 1)] } else { vec![] },
                header: true,
            })
        }
        Type::Table(t) => Expr::Table(ExprTable {
            header: t.columns.iter().map(|c| c.name).collect(),
            rows: (0..many(rng)).map(|_| ExprTableRow {
                elements: t.columns.iter().map(|c| literal_of(db, rng, &c.ty, depth - 1)).collect::<Vec<_>>().into(),
            }).collect(),
        }),
        Type::Atom(a) => Expr::Atom(ExprAtom { name: a.name }),
        Type::Term(t) => Expr::Term(ExprTerm { name: t.name, payload: literal_of(db, rng, &t.payload, depth) }),
        Type::Enum(e) => {
            let v = &e.variants[rng.random_range(0..e.variants.len())];
            let variant = match &v.payload {
                None => Expr::Atom(ExprAtom { name: v.name }),
                Some(p) => Expr::Term(ExprTerm { name: v.name, payload: literal_of(db, rng, p, depth) }),
            };
            if rng.random_bool(0.3) { Expr::Enum(ExprEnum { variant: full(variant) }) } else { variant }
        }
        Type::Data => Expr::Data(ExprData { value: full(Expr::Int(ExprInt { value: text("1") })) }),
        Type::Error => Expr::Error(ExprError { value: full(Expr::String(ExprString { value: text("\"e\"") })) }),
        Type::Var(_) | Type::Rec(_) | Type::RecVar(_) => unreachable!("a closed type exposed has none at its head"),
        _ => unreachable!("every scalar is handled above"),
    };
    full(expr)
}

/// An expression to start from: one `ast_gen` makes, or a literal of a type
/// the hint generator of the oracle tests makes, which has recursive types,
/// with the hint on it.
fn start<'db>(db: &'db Db, rng: &mut SeededRng) -> ExprFull<'db> {
    if rng.random_bool(0.5) {
        let cfg = config(rng);
        return gen_expr_full_seeded(db, rng.random(), cfg);
    }
    let hint = HintGen::new(db, rng.random()).top(4);
    let body = match datalove_datalit::tycheck::convert_type_hint(db, &hint) {
        Ok(ty) => literal_of(db, rng, &ty, 2),
        // A hint that is not a type is a test too; what it is on matters less.
        Err(_) => gen_expr_full_seeded(db, rng.random(), AstGenConfig { max_depth: 1, ..AstGenConfig::default() }),
    };
    ExprFull::new(None, Some(hint), body.expr().C())
}

fn config(rng: &mut SeededRng) -> AstGenConfig {
    AstGenConfig {
        max_depth: rng.random_range(1..4),
        include_type_hints: rng.random_bool(0.7),
        numeric_strategy: if rng.random_bool(0.5) { NumericStrategy::CornerCases } else { NumericStrategy::Mixed },
        ..AstGenConfig::default()
    }
}

/// One random change to one part of an expression.
fn mutate<'db>(db: &'db Db, rng: &mut SeededRng, expr: &ExprFull<'db>) -> ExprFull<'db> {
    let target = rng.random_range(0..count(expr));
    let kind = rng.random_range(0..9);
    let seed: u64 = rng.random();
    let hint_config = config(rng);
    let hint = gen_type_hint(db, &mut SeededRng::new(seed), &hint_config, 1);
    let text = |s: &str| InternedText::new(db, s.S());
    let int = INTS[rng.random_range(0..INTS.len())];
    let hex = HEXES[rng.random_range(0..HEXES.len())];
    rewrite(expr, target, &mut 0, &mut |e| match kind {
        0 => ExprFull::new(None, None, e.expr().C()),
        1 => {
            let small = AstGenConfig { max_depth: 1, ..hint_config.C() };
            gen_expr_full_seeded(db, seed, small)
        }
        2 => ExprFull::new(None, e.type_hint(), Expr::Int(ExprInt { value: text(int) })),
        3 => ExprFull::new(None, e.type_hint(), Expr::Hex(ExprHex { value: text(hex) })),
        // A variant renamed, or made the other kind under its own name.
        4 => match (e.expr(), seed % 2 == 0) {
            (Expr::Atom(_), true) => ExprFull::new(None, e.type_hint(), Expr::Atom(ExprAtom { name: text("Zz") })),
            (Expr::Atom(a), false) => ExprFull::new(None, e.type_hint(), Expr::Term(ExprTerm {
                name: a.name,
                payload: ExprFull::new(None, None, Expr::Int(ExprInt { value: text("1") })),
            })),
            (Expr::Term(t), true) => ExprFull::new(None, e.type_hint(), Expr::Term(ExprTerm { name: text("Zz"), payload: t.payload.C() })),
            (Expr::Term(t), false) => ExprFull::new(None, e.type_hint(), Expr::Atom(ExprAtom { name: t.name })),
            _ => e.C(),
        },
        5 => ExprFull::new(None, Some(hint.C()), e.expr().C()),
        // The shape of an aggregate: a field dropped or renamed, an element
        // added, a column renamed.
        6 => {
            let reshaped = match e.expr() {
                Expr::AnonStruct(st) if !st.fields.is_empty() => {
                    let mut fields = st.fields.C();
                    if seed % 2 == 0 {
                        fields.pop();
                    } else {
                        fields[0].name = text("zz");
                    }
                    Expr::AnonStruct(ExprAnonStruct { fields })
                }
                Expr::AnonTuple(t) => {
                    let mut elements = t.elements.C();
                    elements.push(ExprFull::new(None, None, Expr::Int(ExprInt { value: text("1") })));
                    Expr::AnonTuple(ExprAnonTuple { elements })
                }
                Expr::Table(t) if !t.header.is_empty() => {
                    let mut header = t.header.C();
                    header[0] = text("zz");
                    Expr::Table(ExprTable { header, rows: t.rows.C() })
                }
                other => other.C(),
            };
            ExprFull::new(None, e.type_hint(), reshaped)
        }
        7 => {
            let other = HintGen::new(db, seed).top(3);
            ExprFull::new(None, Some(other), e.expr().C())
        }
        // Every hint in the part gone, so that inference types all of it.
        _ => strip_hints(e),
    })
}

fn hint_has_parse_error(hint: &TypeHint<'_>) -> bool {
    match hint {
        TypeHint::ParseError(_) => true,
        TypeHint::AnonTuple(t) => t.fields.iter().any(hint_has_parse_error),
        TypeHint::AnonStruct(s) => s.fields.iter().any(|f| hint_has_parse_error(&f.type_hint)),
        TypeHint::Table(t) => t.columns.iter().any(|c| hint_has_parse_error(&c.type_hint)),
        TypeHint::List(l) => hint_has_parse_error(&l.element_type),
        TypeHint::Set(s) => hint_has_parse_error(&s.element_type),
        TypeHint::Map(m) => hint_has_parse_error(&m.key_type) || hint_has_parse_error(&m.value_type),
        TypeHint::Option(o) => hint_has_parse_error(&o.inner_type),
        TypeHint::Result(r) => hint_has_parse_error(&r.inner_type),
        TypeHint::Tensor(t) => hint_has_parse_error(&t.element_type),
        TypeHint::Term(t) => hint_has_parse_error(&t.payload),
        TypeHint::Enum(e) => e.variants.iter().any(|v| v.payload.as_deref().is_some_and(hint_has_parse_error)),
        TypeHint::Rec(r) => hint_has_parse_error(&r.body),
        _ => false,
    }
}

/// An expression with no hint anywhere in it.
fn strip_hints<'db>(expr: &ExprFull<'db>) -> ExprFull<'db> {
    let mut stripped = ExprFull::new(None, None, expr.expr().C());
    for i in 1..count(&stripped) {
        stripped = rewrite(&stripped, i, &mut 0, &mut |e| ExprFull::new(None, None, e.expr().C()));
    }
    stripped
}

/// Whether a parse of an expression failed anywhere in it.
fn has_parse_error(expr: &ExprFull<'_>) -> bool {
    matches!(expr.expr(), Expr::ParseError(_))
        || expr.type_hint().is_some_and(|h| hint_has_parse_error(&h))
        || children(expr).into_iter().any(has_parse_error)
}

/// Expressions the typing rules give as examples, with what they say each
/// synthesizes, or the code it is reported with; and cases found by hand. They
/// are held to the doc as well as to the oracle.
const CORPUS: &[(&str, &str)] = &[
    ("5", "int"),
    ("0x10", "int"),
    ("1.5", "f64"),
    ("[1, 2]", "[int]"),
    ("[1, : u8 / 2]", "[u8]"),
    ("[: u8 / 1, 2]", "[u8]"),
    ("[: u8 / 2, 1]", "[u8]"),
    ("[some 1, none]", "[?int]"),
    ("[[], [1]]", "[[int]]"),
    ("[(1, : u8 / 1), (: u8 / 2, 2)]", "[(u8, u8)]"),
    ("{| x; 1; : u8 / 2 |}", "{| x: u8 |}"),
    ("atom A", "atom A"),
    ("term A 1", "term A int"),
    ("data 1", "data"),
    ("error \"e\"", "error"),
    ("[]", "T016"),
    ("none", "T016"),
    ("er error \"e\"", "T016"),
    ("enum { atom A }", "T016"),
    ("{| x |}", "T016"),
    ("[none]", "T016"),
    ("{| |}", "{||}"),
    ("[1, 2, \"three\"]", "T022"),
    (": [u16] / [: u8 / 42]", "T040"),
    (": i32 / 0xFF", "T032"),
    (": int / [none]", "T032"),
    (": [|u8, 2|] / [||]", "T048"),
    (": {| x: int |} / {||}", "T054"),
    ("[{ next = [] }, : rec L { next: [L] } / { next = [] }]", "[rec L { next: [L] }]"),
    (": rec L { next: [L] } / { next = [] }", "rec L { next: [L] }"),
    ("[({ next = [] }, 1), (: rec L { next: [L] } / { next = [] }, 2)]", "[({ next: [rec L { next: [L] }] }, int)]"),
    ("[(: rec L { next: [L] } / { next = [] }, 1), ({ next = [] }, 2)]", "[(rec L { next: [L] }, int)]"),
    ("data none", "T016"),
    ("data : ?u32 / none", "data"),
    ("enum { 1 }", "T016"),
    // An atom then a hinted term of its name: the first in source order keeps
    // the list's type, so the hint is what is reported.
    ("[atom A, : term A int / term A 1]", "T040"),
    ("[term A 1, : atom A / atom A]", "T040"),
];

#[test]
fn literal_typing_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the literal typing tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_LIT", 0, 300);
    let db = &Db::default();
    let mut queries = Vec::new();
    let mut rust = Vec::new();
    let mut sources = Vec::new();
    let mut skipped = 0;
    let mut several = 0;
    let mut doc_failures = Vec::new();
    let cases = CORPUS.iter().map(|(text, expected)| (None, text.S(), Some(*expected)))
        .chain(seeds.iter().map(|seed| {
            let mut rng = SeededRng::new(seed);
            let mut expr = start(db, &mut rng);
            for _ in 0..rng.random_range(0..=MAX_MUTATIONS) {
                expr = mutate(db, &mut rng, &expr);
            }
            (Some(seed), datalove_datalit::pretty::pretty_print(db, &expr), None)
        }));
    for (seed, text, expected) in cases {
        let seed = seed.map(|s| fmt!("seed {s}")).unwrap_or_else(|| "corpus".S());
        let source = bct::input::Source::new(db, text.C());
        let parsed = datalove_datalit::parser::parse_integration_test(db, source);
        if has_parse_error(&parsed) {
            if std::env::var("FORMAL_LIT_SHOW_SKIPPED").is_ok() {
                println!("skipped, {seed}: {text}");
            }
            skipped += 1;
            continue;
        }
        let resolved = datalove_datalit::resolve::resolve_names(db, source, parsed.C());
        let result = datalove_datalit::tycheck::type_check(db, parsed.C(), resolved);
        let diagnostics = datalove_datalit::tycheck::type_check::accumulated::<datalove_diagnostic::TypeDiagnostic>(
            db, parsed.C(), resolved,
        );
        let codes: Vec<String> = diagnostics.iter()
            .map(|d| d.to_diagnostic(db).code.map(|c| c.as_str(db).S()).unwrap_or_else(|| "none".S()))
            .collect();
        if codes.len() > 1 {
            several += 1;
        }
        if let Some(expected) = expected {
            let got = match (result.root_type(db), codes.first()) {
                (Some(ty), None) => datalove_datalit::tycheck::type_to_string(db, ty),
                (_, Some(code)) => code.C(),
                (None, None) => "nothing".S(),
            };
            if got != expected {
                doc_failures.push(fmt!("{text}: the doc says {expected}, the Rust says {got}"));
            }
        }
        // A diagnostic decides it: a hinted expression that does not check
        // still has the hint's type, and its error besides.
        let answer = match (result.root_type(db), codes.first()) {
            (_, Some(code)) => json!({ "err": code }),
            (Some(ty), None) => json!({ "ok": type_to_json(db, ty) }),
            (None, None) => panic!("{seed}: neither a type nor a diagnostic"),
        };
        queries.push(json!({ "synth": expr_to_json(db, &parsed) }));
        rust.push(answer);
        sources.push(fmt!("{seed}: {text}"));
    }

    let answers = ask_oracle(&queries);
    let mut outcomes = BTreeMap::<String, usize>::new();
    let mut mismatches = BTreeMap::<(String, String), Vec<String>>::new();
    for ((rust, oracle), source) in rust.iter().zip(&answers).zip(&sources) {
        let outcome = |a: &Value| a.get("err").map(|c| c.as_str().X().S()).unwrap_or_else(|| "ok".S());
        *outcomes.entry(outcome(rust)).or_default() += 1;
        if rust != oracle {
            mismatches.entry((outcome(rust), outcome(oracle))).or_default().push(fmt!(
                "{source}\n    rust:   {rust}\n    oracle: {oracle}",
            ));
        }
    }
    println!(
        "{} expressions over seeds {}..{} ({skipped} skipped for parse errors, {several} with several diagnostics): {outcomes:?}",
        rust.len(), seeds.start, seeds.start.wrapping_add(seeds.count),
    );
    for ((r, o), cases) in &mismatches {
        println!("rust {r}, oracle {o}: {} cases, for example", cases.len());
        for c in cases.iter().take(3) {
            println!("  {c}");
        }
    }
    for f in &doc_failures {
        println!("doc: {f}");
    }
    let total: usize = mismatches.values().map(|v| v.len()).sum();
    assert!(doc_failures.is_empty(), "the Rust differs from the doc's examples on {}", doc_failures.len());
    assert!(total == 0, "the Rust and the typing rules disagree on {total} of {} expressions", rust.len());
}
