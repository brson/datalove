//! The codes against their docs, and the workspace against open-coded codes.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use datalove_diagnostic_codes::{DCode, FCode, PCode, TCode};
use enum_iterator::all;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every live code of a family, as shown, with its number.
fn live<C: enum_iterator::Sequence + Copy>(as_str: fn(C) -> &'static str, number: fn(C) -> u16) -> Vec<(String, u16)> {
    all::<C>().map(|c| (as_str(c).to_string(), number(c))).collect()
}

fn family(prefix: char) -> Vec<(String, u16)> {
    match prefix {
        'D' => live(DCode::as_str, DCode::number),
        'P' => live(PCode::as_str, PCode::number),
        'T' => live(TCode::as_str, TCode::number),
        'F' => live(FCode::as_str, FCode::number),
        _ => unreachable!(),
    }
}

fn retired(prefix: char) -> &'static [u16] {
    match prefix {
        'D' => DCode::RETIRED,
        'P' => PCode::RETIRED,
        'T' => TCode::RETIRED,
        'F' => FCode::RETIRED,
        _ => unreachable!(),
    }
}

/// Parse a code such as `T005` into its prefix and number.
fn parse_code(text: &str) -> Option<(char, u16)> {
    let mut chars = text.chars();
    let prefix = chars.next()?;
    let digits = chars.as_str();
    if !"DPTF".contains(prefix) || digits.len() != 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((prefix, digits.parse().ok()?))
}

/// The codes in the first cell of a markdown table's rows, a range such as
/// `T005-T012` taken as every number in it.
fn tabled_codes(doc: &str, prefix: char) -> BTreeSet<u16> {
    let path = workspace_root().join("botdocs").join(doc);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut codes = BTreeSet::new();
    for line in text.lines() {
        let Some(row) = line.strip_prefix('|') else { continue };
        let Some(cell) = row.split('|').next() else { continue };
        let cell = cell.trim();
        let (first, last) = match cell.split_once('-') {
            Some((a, b)) => (a, b),
            None => (cell, cell),
        };
        let (Some((p1, n1)), Some((p2, n2))) = (parse_code(first), parse_code(last)) else { continue };
        assert_eq!(p1, p2, "{doc}: a range across families, `{cell}`");
        if p1 == prefix {
            codes.extend(n1..=n2);
        }
    }
    codes
}

/// Hold a doc's table to the live codes of a family in a range of numbers.
fn check_table(doc: &str, prefix: char, range: std::ops::RangeInclusive<u16>) {
    let tabled = tabled_codes(doc, prefix);
    let live: BTreeSet<u16> = family(prefix).into_iter().map(|(_, n)| n).filter(|n| range.contains(n)).collect();
    let missing: Vec<_> = live.difference(&tabled).map(|n| format!("{prefix}{n:03}")).collect();
    let unknown: Vec<_> = tabled.difference(&live).map(|n| format!("{prefix}{n:03}")).collect();
    assert!(missing.is_empty(), "botdocs/{doc} has no row for {missing:?}");
    assert!(unknown.is_empty(), "botdocs/{doc} has rows for {unknown:?}, which are not live codes");
}

#[test]
fn a_code_shows_its_prefix_and_number() {
    for prefix in ['D', 'P', 'T', 'F'] {
        for (text, number) in family(prefix) {
            assert_eq!(text, format!("{prefix}{number:03}"));
        }
    }
}

#[test]
fn no_live_code_is_retired() {
    for prefix in ['D', 'P', 'T', 'F'] {
        for (text, number) in family(prefix) {
            assert!(!retired(prefix).contains(&number), "{text} is retired");
        }
    }
}

#[test]
fn the_docs_table_every_code() {
    check_table("botspec.md", 'D', 1..=16);
    check_table("compiler-guide.md", 'D', 1..=16);
    check_table("datalit-ebnf.md", 'D', 17..=999);
    check_table("datafun-ebnf.md", 'P', 1..=999);
    check_table("datalit-typing-rules.md", 'T', 1..=999);
    check_table("datafun-typing-rules.md", 'F', 1..=999);
}

/// The code a line of source writes as a string literal, as `"P088"` or
/// within one as `[P088]`, if it writes one.
fn open_coded(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    (0..bytes.len().saturating_sub(5)).find_map(|i| {
        let (open, close) = (bytes[i], bytes[i + 5]);
        let delimited = (open == b'"' && close == b'"') || (open == b'[' && close == b']');
        let text = line.get(i + 1..i + 5)?;
        (delimited && parse_code(text).is_some()).then_some(text)
    })
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A diagnostic code is named through this crate, never written out.
///
/// Test code may write codes, to say what output it expects: a file's lines
/// from its `#[cfg(test)]` on, and `tests.rs` files, are not checked.
#[test]
fn no_crate_writes_a_code_as_a_string() {
    let crates = workspace_root().join("crates");
    let mut files = vec![];
    for entry in fs::read_dir(&crates).unwrap() {
        let dir = entry.unwrap().path();
        if dir.ends_with("datalove-diagnostic-codes") {
            continue;
        }
        let src = dir.join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(!files.is_empty());

    let mut found = vec![];
    for file in files {
        if file.ends_with("tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(code) = open_coded(line) {
                found.push(format!("{}:{}: {code}", file.strip_prefix(&crates).unwrap().display(), i + 1));
            }
        }
    }
    assert!(found.is_empty(), "codes written as strings; name them with datalove-diagnostic-codes:\n{}", found.join("\n"));
}
