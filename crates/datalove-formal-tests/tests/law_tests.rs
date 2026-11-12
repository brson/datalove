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

    assert!(meet_chains > 0 && adapt_chains > 0, "no chains to test");
    assert!(meet_failures.is_empty(), "meeting at the head is not transitive");
    assert!(adapt_failures.is_empty(), "@ is not transitive");
}

/// Every hint of exactly `size` nodes over a small grammar: `int`, `bool`, a
/// list, an option, a pair, and `rec` over three names. Small enough to list
/// them all, and rich enough to nest recursive types and use any name; three
/// names are what it takes to write a type beside its one-step unfolding, as
/// `rec L rec M [(L, M)]` beside `rec N [(rec L rec M [(L, M)], N)]`.
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
        for n in REC_NAMES {
            hints.push(TypeHint::Alias(TypeHintAlias { name: name(n), local_index: None }));
        }
    } else {
        for inner in small_hints(db, size - 1, memo) {
            hints.push(TypeHint::List(TypeHintList { element_type: Box::new(inner.C()) }));
            hints.push(TypeHint::Option(TypeHintOption { inner_type: Box::new(inner.C()) }));
            for n in REC_NAMES {
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

const REC_NAMES: [&str; 3] = ["L", "M", "N"];

/// Every small recursive type meets its one-step unfolding and what it
/// exposes to, and adapts to both and back. A type that starts with more than
/// one `rec` unfolds in one step to another recursive type, which differs from
/// what it exposes to; it is those that make meeting at the head transitive or
/// not, and they are counted, to be sure there are some. `FORMAL_LAWS_SIZE`
/// sets the size.
#[test]
fn unfolding_meets_exhaustive() {
    let max: usize = std::env::var("FORMAL_LAWS_SIZE").map(|s| s.parse().X()).unwrap_or(7);
    let db = &Db::default();
    let mut memo = BTreeMap::new();
    let mut hints = 0;
    let mut recursive = 0;
    let mut nested = 0;
    let mut failures = Vec::new();
    for size in 1..=max {
        for hint in small_hints(db, size, &mut memo) {
            hints += 1;
            let Ok(ty) = tc::convert_type_hint(db, &hint) else { continue };
            let Type::Rec(r) = &ty else { continue };
            recursive += 1;
            let unfolded = r.unfold();
            if matches!(unfolded, Type::Rec(_)) {
                nested += 1;
            }
            for other in [unfolded, tc::expose(&ty).into_owned()] {
                let meets = tc::types_equivalent_at_head(db, &ty, &other);
                let adapts = tc::can_adapt_to(db, &ty, &other) && tc::can_adapt_to(db, &other, &ty);
                if !meets || !adapts {
                    failures.push(fmt!("{} and {}", ty.syntax(db), other.syntax(db)));
                }
            }
        }
    }
    println!("{hints} hints up to size {max}: {recursive} recursive, {nested} starting with more than one rec");
    for f in failures.iter().take(10) {
        println!("  do not meet: {f}");
    }
    assert!(nested > 0, "no type starts with two recs, so the case that matters went untested");
    assert!(failures.is_empty(), "{} pairs that should meet do not", failures.len());
}
