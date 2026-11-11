//! The type relations held to the Lean model's oracle.
//!
//! Generates type hints from seeds, converts each with `convert_type_hint`,
//! and asks the Rust and the oracle the same questions about the hints and
//! the types they come to: what a hint converts to or the first thing wrong
//! with it; for a type, whether it is copy, whether the model finds it well
//! formed, and what it exposes to; for a pair, `≡`, `≈` and `⇝`.
//!
//! Needs the oracle built (`just formal-build`), so it runs only when asked:
//!
//! ```text
//! FORMAL_ORACLE_TEST=1 cargo test -p datalove-formal-tests --test oracle_tests
//! ```
//!
//! `just test-formal` builds and runs it. Seeds are taken as every generative
//! suite takes them, from `FORMAL_ORACLE_SEED` and `FORMAL_ORACLE_COUNT`.

use rmx::prelude::*;
use rmx::serde_json::{Value, json};
use datalove_datalit::tycheck::{self as tc, Type};
use datalove_formal_tests::*;

const HINTS_PER_SEED: usize = 20;
const HINT_DEPTH: u32 = 5;

/// A question for both sides: the query the oracle reads, the answer the
/// Rust gave, and what to print if they differ.
struct Case {
    query: Value,
    rust: Value,
    what: String,
}

#[test]
fn oracle_agrees() {
    if std::env::var("FORMAL_ORACLE_TEST").is_err() {
        println!("Skipping the oracle tests (set FORMAL_ORACLE_TEST=1 to run)");
        return;
    }
    let seeds = bct::random::env_seeds("FORMAL_ORACLE", 0, 200);
    let db = &Db::default();
    let (pool, hints) = type_pool(db, seeds.iter(), HINTS_PER_SEED, HINT_DEPTH);
    let generated = pool.len();
    let mut cases = Vec::new();
    for (i, (seed, hint, converted)) in hints.iter().enumerate() {
        cases.push(Case {
            query: json!({ "convert": hint_to_json(db, hint) }),
            rust: conversion_to_json(db, converted),
            what: fmt!("seed {seed} hint {i}: convert {}", hint.syntax(db)),
        });
    }
    let mut rec_count = 0;
    for ty in &pool {
        if matches!(ty, Type::Rec(_)) {
            rec_count += 1;
        }
        cases.push(Case {
            query: json!({ "type": type_to_json(db, ty) }),
            rust: json!({
                "copy": datalove_datafun_tycheck::is_copy_type(db, ty),
                // Every type the Rust converts, and every one made beside them,
                // is well formed.
                "wf": true,
                "expose": type_to_json(db, &tc::expose(ty)),
            }),
            what: fmt!("type {}", ty.syntax(db)),
        });
    }

    let pairs = pairs(db, &pool, seeds.start);
    let mut related = [0usize; 3];
    for (a, b) in &pairs {
        let equiv = tc::types_equivalent(db, a, b);
        assert_eq!(
            equiv, a == b,
            "types_equivalent and == disagree on {} and {}", a.syntax(db), b.syntax(db),
        );
        let meet = tc::types_equivalent_at_head(db, a, b);
        let adapt = tc::can_adapt_to(db, a, b);
        related[0] += equiv as usize;
        related[1] += meet as usize;
        related[2] += adapt as usize;
        cases.push(Case {
            query: json!({ "pair": [type_to_json(db, a), type_to_json(db, b)] }),
            rust: json!({ "equiv": equiv, "meet": meet, "adapt": adapt }),
            what: fmt!("pair {} and {}", a.syntax(db), b.syntax(db)),
        });
    }

    let queries: Vec<Value> = cases.iter().map(|c| c.query.C()).collect();
    let answers = ask_oracle(&queries);

    let mut conversions = std::collections::BTreeMap::<String, usize>::new();
    let mut mismatches = Vec::new();
    for (case, answer) in cases.iter().zip(&answers) {
        let answer = if case.query.get("convert").is_some() {
            let outcome = match answer.get("err") {
                Some(e) => e["kind"].as_str().X().S(),
                None => "ok".S(),
            };
            *conversions.entry(outcome).or_default() += 1;
            comparable_conversion(answer)
        } else {
            answer.C()
        };
        if answer != case.rust {
            mismatches.push(fmt!("{}\n  rust:   {}\n  oracle: {}", case.what, case.rust, answer));
        }
    }

    println!(
        "{} cases over seeds {}..{}: conversions {conversions:?}; \
         {generated} distinct types, {rec_count} recursive; \
         {} pairs, {} equivalent, {} meeting, {} adapting",
        cases.len(), seeds.start, seeds.start.wrapping_add(seeds.count),
        pairs.len(), related[0], related[1], related[2],
    );
    if !mismatches.is_empty() {
        for m in mismatches.iter().take(20) {
            println!("{m}");
        }
        panic!("the Rust and the oracle disagree on {} of {} cases", mismatches.len(), cases.len());
    }
}

