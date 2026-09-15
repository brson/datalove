//! "did you mean `x`?", and the distance that decides whether to say it.
//!
//! A compiler that knows the closed set a name was supposed to come from can
//! say which member the author probably meant. What stops that being annoying
//! is the budget: suggesting a word nobody meant is worse than silence, so a
//! long name may be wrong by more characters than a short one before the
//! nearest match is offered at all.

use rmx::prelude::*;

/// The option nearest `got`, if one is near enough to be worth naming.
///
/// A third of the word, rounded down and never less than one, so `attak`
/// finds `attack` and `gain` does not find `tab`.
///
/// An option that *is* `got` is never returned. A list holding the word
/// itself is the ordinary case -- "no property `radius`" is raised against
/// the table `radius` is in -- and "did you mean `radius`?" under it reads as
/// a taunt.
///
/// Ties go to whichever sorts first, so the answer does not depend on the
/// order the options arrived in.
pub fn closest<'a>(got: &str, options: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let budget = (got.chars().count() / 3).max(1);
    options
        .into_iter()
        .map(|o| (distance(got, o), o))
        .filter(|(d, _)| *d > 0 && *d <= budget)
        .min_by_key(|(d, o)| (*d, *o))
        .map(|(_, o)| o)
}

/// Levenshtein distance, over chars, two rows at a time.
///
/// Chars rather than bytes, so a name with an accent in it is one edit from
/// the same name without.
pub fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        rmx::std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[test]
fn test_distance() {
    assert_eq!(distance("", ""), 0);
    assert_eq!(distance("a", ""), 1);
    assert_eq!(distance("", "abc"), 3);
    assert_eq!(distance("attack", "attack"), 0);
    assert_eq!(distance("attak", "attack"), 1);
    assert_eq!(distance("kitten", "sitting"), 3);
    // Chars, not bytes: one substitution, not two.
    assert_eq!(distance("cafe", "caf\u{e9}"), 1);
}

#[test]
fn test_closest() {
    const SLOTS: &[&str] = &["attack", "release", "knee", "ratio"];
    let of = |got: &str| closest(got, SLOTS.iter().copied());

    assert_eq!(of("attak"), Some("attack"));
    assert_eq!(of("realease"), Some("release"));

    // Nothing near enough. `gain` is four letters, so the budget is one.
    assert_eq!(of("gain"), None);
    assert_eq!(of("kn"), None);

    // The word itself is not a suggestion.
    assert_eq!(of("attack"), None);

    // The budget grows with the word, so a long name may be wrong by more.
    assert_eq!(closest("anodized_aluminium", ["anodised_aluminium"]), Some("anodised_aluminium"));
    assert_eq!(closest("abc", ["abz"]), Some("abz"));
    assert_eq!(closest("abc", ["azz"]), None);
}

#[test]
fn test_closest_does_not_depend_on_order() {
    // Two options the same distance away: the answer is the same either way
    // round, which is what keeps a diagnostic stable across a rebuild.
    assert_eq!(closest("ab", ["ac", "ad"]), Some("ac"));
    assert_eq!(closest("ab", ["ad", "ac"]), Some("ac"));
}
