//! The lexical grammar in `botdocs/lexical-grammar.md`, checked against the lexer.
//!
//! The grammar, made executable by `datalove_doc_fixtures::lexical`, lexes
//! every input by longest match, and the lexer in `bcts` must make the same
//! tokens: kind, span, and whether a newline came before each. The inputs are
//! every datalit and datafun source the grammar tests read, each changed a
//! character at a time, strings generated from the grammar, and strings of
//! characters drawn from those the rules single out, from the seeds of
//! `DATALOVE_LEX_SEED` and `DATALOVE_LEX_COUNT` (see `bct::random::env_seeds`).

use rmx::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::ebnf::{Generator, join_chars};
use datalove_doc_fixtures::grammar_check::{files, mutate, shrink, workspace_dir};
use datalove_doc_fixtures::lexical::{DOC, Lexer, grammar, lexer_tokens, sample, sigils};
use datalove_doc_fixtures::datafun_grammar;

/// How the lexer and the grammar differ on a text, if they do.
fn differ(lexer: &Lexer, text: &str) -> Option<String> {
    let ours = lexer_tokens(text);
    match lexer.tokens(text) {
        Err(e) => Some(fmt!("the grammar cannot lex it: {e}")),
        Ok(theirs) if theirs == ours => None,
        Ok(theirs) => {
            let k = theirs.iter().zip(&ours).take_while(|(a, b)| a == b).count();
            Some(fmt!("token {k}: the grammar has {:?}, the lexer {:?}", theirs.get(k), ours.get(k)))
        }
    }
}

#[derive(Default)]
struct Found {
    /// Each difference, shrunk, by what it is, with an input it came from.
    cases: BTreeMap<String, (String, String)>,
    checked: usize,
}

impl Found {
    fn check(&mut self, lexer: &Lexer, text: &str) {
        self.checked += 1;
        if differ(lexer, text).is_none() {
            return;
        }
        let small = if self.cases.len() < 20 {
            shrink(text, &|t| differ(lexer, t).is_some())
        } else {
            text.S()
        };
        let what = differ(lexer, &small).X();
        self.cases.entry(fmt!("{small:?}: {what}")).or_insert_with(|| (small, text.S()));
    }

    fn finish(self, what: &str) {
        if self.cases.is_empty() {
            println!("{what}: {} inputs, the grammar and the lexer agree", self.checked);
            return;
        }
        let mut report = fmt!("{what}: the grammar and the lexer differ in {} cases over {} inputs\n", self.cases.len(), self.checked);
        for (case, (_, from)) in self.cases.iter().take(40) {
            let from: String = from.chars().take(200).collect();
            report.push_str(&fmt!("\n{case}\n    from {from:?}\n"));
        }
        panic!("{report}");
    }
}

#[test]
fn sigils_are_the_lexers() {
    let doc: BTreeSet<String> = sigils(&grammar());
    let lexer: BTreeSet<String> = enum_iterator::all::<bct::lexer::Sigil>().map(|s| s.as_str().S()).collect();
    let missing: Vec<_> = lexer.difference(&doc).collect();
    let extra: Vec<_> = doc.difference(&lexer).collect();
    assert!(missing.is_empty() && extra.is_empty(), "{DOC} lacks the sigils {missing:?} and lists {extra:?}, which are none");
}

#[test]
fn sources_agree() {
    let lexer = Lexer::new(&grammar());
    let mut found = Found::default();
    let mut sources: Vec<String> =
        files(&workspace_dir().join("crates"), &["dlt"]).into_iter().map(|p| std::fs::read_to_string(p).X()).collect();
    sources.extend(datafun_grammar::corpus().into_iter().map(|(_, text)| text));
    assert!(sources.len() > 600, "found only {} sources", sources.len());
    for text in &sources {
        found.check(&lexer, text);
        let mut rng = SeededRng::new(seed_of(text));
        for _ in 0..3 {
            found.check(&lexer, &mutate(text, &mut rng));
        }
    }
    found.finish("the sources and their mutants");
}

/// The characters the rules single out, and some they do not.
const POOL: &[char] = &[
    'a', 'Z', '1', '_', 'é', '١', ' ', '\t', '\r', '\n', ';', '/', '/', '*', '*', '"', '"', '\\', '|', '[', ']',
    '{', '}', '(', ')', '<', '>', '-', '+', '?', '!', '=', '.', ':', '%', '#', '@', '$', '~', ',', '&', '^',
    '\'', '`', '\u{0}', '\u{3000}', '\u{85}', '→', '€',
];

#[test]
fn generated_agree() {
    let g = grammar();
    let lexer = Lexer::new(&g);
    let generator = Generator::new(&g, &sample);
    let mut found = Found::default();
    for seed in env_seeds("DATALOVE_LEX", 0, 3000).iter() {
        let mut rng = SeededRng::new(seed);
        let n = rng.random_range(0..24);
        let drawn: String = (0..n).map(|_| POOL[rng.random_range(0..POOL.len())]).collect();
        found.check(&lexer, &drawn);
        let generated = join_chars(&generator.generate("source", &mut rng), &|_, _, _| "");
        found.check(&lexer, &generated);
    }
    found.finish("drawn and generated strings");
}
