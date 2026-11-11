//! Laws of the type relations, held to the Rust directly.
//!
//! These are what the Lean model in `formal/` proves of its transcription, or
//! would prove: a law that fails here is one the model cannot prove, and
//! shows a case where the language does something irregular. They need no
//! oracle, so they run with the rest of the suite.
//!
//! Transitivity is tested on chains the pairs give: for each two related
//! pairs `(a, b)` and `(b, c)` among the pairs asked about, `(a, c)` is asked.
//! Seeds come from `FORMAL_LAWS_SEED` and `FORMAL_LAWS_COUNT`.

use rmx::prelude::*;
use datalove_datalit::ast::TypeHint;
use datalove_datalit::tycheck::{self as tc, Type};
use datalove_formal_tests::*;
use std::collections::BTreeMap;

/// The failures of a transitive law over chains of the pairs where it holds.
fn transitivity_failures<'db>(
    db: &'db Db,
    pairs: &[(Type<'db>, Type<'db>)],
    relation: impl Fn(&Type<'db>, &Type<'db>) -> bool,
) -> (usize, Vec<String>) {
    let key = |t: &Type<'db>| type_to_json(db, t).S();
    let mut after: BTreeMap<String, Vec<&Type<'db>>> = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for (a, b) in pairs {
        if relation(a, b) && seen.insert((key(a), key(b))) {
            after.entry(key(a)).or_default().push(b);
        }
    }
    let mut chains = 0;
    let mut failures = Vec::new();
    for (a, b) in pairs {
        if !relation(a, b) {
            continue;
        }
        for c in after.get(&key(b)).into_iter().flatten() {
            chains += 1;
            if !relation(a, c) {
                failures.push(fmt!("{}  then  {}  then  {}", a.syntax(db), b.syntax(db), c.syntax(db)));
            }
        }
    }
    failures.sort();
    failures.dedup();
    (chains, failures)
}

fn report(law: &str, chains: usize, failures: &[String]) {
    println!("{law}: {chains} chains, {} failing", failures.len());
    for f in failures.iter().take(10) {
        println!("  {f}");
    }
}

#[test]
fn relation_laws() {
    let seeds = bct::random::env_seeds("FORMAL_LAWS", 0, 50);
    let db = &Db::default();
    let (pool, _) = type_pool(db, seeds.iter(), 20, 5);
    let pairs = pairs(db, &pool, seeds.start);

    let (meet_chains, meet_failures) =
        transitivity_failures(db, &pairs, |a, b| tc::types_equivalent_at_head(db, a, b));
    report("meeting at the head is transitive", meet_chains, &meet_failures);
    let (adapt_chains, adapt_failures) =
        transitivity_failures(db, &pairs, |a, b| tc::can_adapt_to(db, a, b));
    report("@ is transitive", adapt_chains, &adapt_failures);

    // Two recursive types that unfold to one type are one type. Meeting at
    // the head is transitive just when this holds; see
    // `formal/Datalove/Proofs/Meet.lean`.
    let mut by_exposure: BTreeMap<String, Vec<&Type<'_>>> = BTreeMap::new();
    for ty in pool.iter().filter(|t| matches!(t, Type::Rec(_))) {
        by_exposure.entry(type_to_json(db, &tc::expose(ty)).S()).or_default().push(ty);
    }
    let collisions: Vec<String> = by_exposure.values()
        .filter(|tys| tys.len() > 1)
        .map(|tys| tys.iter().map(|t| t.syntax(db).S()).collect::<Vec<_>>().join(" and "))
        .collect();
    println!("{} recursive types, {} exposures shared", by_exposure.values().map(|v| v.len()).sum::<usize>(), collisions.len());

    assert!(meet_chains > 0 && adapt_chains > 0, "no chains to test");
    assert!(collisions.is_empty(), "recursive types that unfold alike: {collisions:?}");
    assert!(meet_failures.is_empty(), "meeting at the head is not transitive");
    assert!(adapt_failures.is_empty(), "@ is not transitive");
}

/// Every hint of exactly `size` nodes over a small grammar: `int`, `bool`, a
/// list, an option, a pair, and `rec` over two names. Small enough to list
/// them all, and rich enough to nest recursive types and use either name.
fn small_hints<'db>(db: &'db Db, size: usize, memo: &mut BTreeMap<usize, Vec<TypeHint<'db>>>) -> Vec<TypeHint<'db>> {
    use datalove_datalit::ast::*;
    if let Some(hints) = memo.get(&size) {
        return hints.C();
    }
    let name = |n: &str| bct::text::InternedText::new(db, n.S());
    let mut hints = Vec::new();
    if size == 1 {
        hints.push(TypeHint::Int);
        hints.push(TypeHint::Bool);
        for n in ["L", "M"] {
            hints.push(TypeHint::Alias(TypeHintAlias { name: name(n), local_index: None }));
        }
    } else {
        for inner in small_hints(db, size - 1, memo) {
            hints.push(TypeHint::List(TypeHintList { element_type: Box::new(inner.C()) }));
            hints.push(TypeHint::Option(TypeHintOption { inner_type: Box::new(inner.C()) }));
            for n in ["L", "M"] {
                hints.push(TypeHint::Rec(TypeHintRec { name: name(n), body: Box::new(inner.C()), local_index: None }));
            }
        }
        for left in 1..size - 1 {
            let lefts = small_hints(db, left, memo);
            let rights = small_hints(db, size - 1 - left, memo);
            for l in &lefts {
                for r in &rights {
                    hints.push(TypeHint::AnonTuple(TypeHintAnonTuple { fields: vec![l.C(), r.C()] }));
                }
            }
        }
    }
    memo.insert(size, hints.C());
    hints
}

/// No two well-formed recursive types, up to a size, unfold alike: the
/// `ExposeInjective` the proofs of transitivity assume, held to every case
/// small enough to list. `FORMAL_LAWS_SIZE` sets the size.
#[test]
fn expose_injective_exhaustive() {
    let max: usize = std::env::var("FORMAL_LAWS_SIZE").map(|s| s.parse().X()).unwrap_or(8);
    let db = &Db::default();
    let mut memo = BTreeMap::new();
    let mut by_exposure: BTreeMap<String, Vec<Type<'_>>> = BTreeMap::new();
    let mut hints = 0;
    for size in 1..=max {
        for hint in small_hints(db, size, &mut memo) {
            hints += 1;
            if let Ok(ty @ Type::Rec(_)) = tc::convert_type_hint(db, &hint) {
                let exposed = type_to_json(db, &tc::expose(&ty)).S();
                let same = by_exposure.entry(exposed).or_default();
                if !same.contains(&ty) {
                    same.push(ty);
                }
            }
        }
    }
    let recursive: usize = by_exposure.values().map(|v| v.len()).sum();
    println!("{hints} hints up to size {max}: {recursive} distinct recursive types");
    let collisions: Vec<String> = by_exposure.values()
        .filter(|tys| tys.len() > 1)
        .map(|tys| tys.iter().map(|t| t.syntax(db).S()).collect::<Vec<_>>().join(" and "))
        .collect();
    assert!(recursive > 0);
    assert!(collisions.is_empty(), "recursive types that unfold alike: {collisions:?}");
}
