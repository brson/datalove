//! The datalit grammar in `botdocs/datalit-ebnf.md`, checked against the parser.
//!
//! The grammar, made executable by `datalove_doc_fixtures::datalit_grammar`,
//! is given every input along with `datalove_datalit::parser::parse`:
//!
//! - what the parser accepts, the grammar must derive;
//! - what the grammar derives, the parser must accept, or refuse only with
//!   the codes the document lists as refused outside the grammar.
//!
//! The inputs are the document's `examples` blocks, each of which must also
//! get exactly the codes the block gives it, every `.dlt` file under
//! `crates/`, strings generated from the grammar, and all of those mutated a
//! character at a time. A disagreement is shrunk to the shortest input that
//! still disagrees the same way.
//!
//! A string is generated from each seed of `DATALOVE_GRAMMAR_SEED` and
//! `DATALOVE_GRAMMAR_COUNT` (see `bct::random::env_seeds`).

use rmx::prelude::*;

use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::datalit_grammar::{DOC, doc_text, grammar, parse_codes, parse_diags, repair, sample, term};
use datalove_doc_fixtures::ebnf::{Generator, Recognizer, join_chars};
use datalove_doc_fixtures::grammar_check::{Findings, Judge, example_wrong, examples, files, mutate, outside_codes, workspace_dir};

fn recognizer() -> Recognizer<[char]> {
    Recognizer::new(&grammar(), "datalit", &term)
}

fn derives(rec: &Recognizer<[char]>, text: &str) -> bool {
    let cs: Vec<char> = text.chars().collect();
    rec.accepts(&cs, cs.len())
}

#[test]
fn doc_examples() {
    let doc = doc_text();
    let examples = examples(&doc, DOC);
    assert!(examples.len() > 20, "found only {} examples in {DOC}", examples.len());
    let rec = recognizer();
    let outside = outside_codes(&doc, DOC);
    let judge = Judge { derives: &|t| derives(&rec, t), parse: &parse_codes, outside: &outside };
    let mut findings = Findings::default();
    let mut wrong = vec![];
    for ex in &examples {
        assert_eq!(ex.context, "datalit", "{DOC}:{}: datalit examples are all `datalit`", ex.line);
        wrong.extend(example_wrong(DOC, ex, parse_diags(&ex.text)));
        findings.check(&judge, "datalit", &ex.text);
    }
    assert!(wrong.is_empty(), "{DOC} is wrong about:\n{}", wrong.join("\n"));
    findings.finish("the document's examples");
}

#[test]
fn corpus_agrees() {
    let rec = recognizer();
    let outside = outside_codes(&doc_text(), DOC);
    let judge = Judge { derives: &|t| derives(&rec, t), parse: &parse_codes, outside: &outside };
    let paths = files(&workspace_dir().join("crates"), &["dlt"]);
    assert!(paths.len() > 100, "found only {} .dlt files", paths.len());
    let mut findings = Findings::default();
    for path in paths {
        let text = std::fs::read_to_string(path).X();
        let mut rng = SeededRng::new(seed_of(&text));
        findings.check(&judge, "datalit", &text);
        for _ in 0..2 {
            findings.check(&judge, "datalit", &mutate(&text, &mut rng));
        }
    }
    findings.finish("the .dlt files and their mutants");
}

#[test]
fn generated_agree() {
    let g = grammar();
    let generator = Generator::new(&g, &sample);
    let rec = recognizer();
    let outside = outside_codes(&doc_text(), DOC);
    let judge = Judge { derives: &|t| derives(&rec, t), parse: &parse_codes, outside: &outside };
    let mut findings = Findings::default();
    let mut dropped = 0;
    let seeds = env_seeds("DATALOVE_GRAMMAR", 0, 1500);
    for seed in seeds.iter() {
        let text = join_chars(&generator.generate("datalit", &mut SeededRng::new(seed)), &repair);
        // A repair can fail: a newline after a line comment in a table row
        // ends the row, and a name can be a type word.
        if !derives(&rec, &text) {
            dropped += 1;
            continue;
        }
        findings.check(&judge, "datalit", &text);
        findings.check(&judge, "datalit", &mutate(&text, &mut SeededRng::derive(seed, "mutate")));
    }
    let n = seeds.count;
    assert!(dropped < n / 2, "{dropped} of {n} generated strings were not derived");
    findings.finish("generated strings and their mutants");
}
