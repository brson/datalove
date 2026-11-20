//! The datafun grammar in `botdocs/datafun-ebnf.md`, checked against the parser.
//!
//! The grammar, made executable by `datalove_doc_fixtures::datafun_grammar`,
//! is given every input along with the parser, as a script, a module or a
//! rider interface:
//!
//! - what the parser accepts, the grammar must derive;
//! - what the grammar derives, the parser must accept, or refuse only with
//!   the codes the document lists as refused outside the grammar.
//!
//! The inputs are every `.dfs`, `.dfm` and `.dli` file under `crates/` and
//! `sys/`, the module and script sections of every worldfile there, strings
//! generated from the grammar, and all of those mutated a character at a
//! time. A string is generated from each seed of `DATALOVE_DF_GRAMMAR_SEED`
//! and `DATALOVE_DF_GRAMMAR_COUNT` (see `bct::random::env_seeds`).
//!
//! The document's `examples` blocks are inputs too, each of which must also
//! get exactly the codes the block gives it.

use rmx::prelude::*;
use std::collections::BTreeSet;

use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::datafun_grammar::{
    DOC, Start, corpus, doc_text, grammar, parse_codes, parse_diags, recognizers, sample,
};
use datalove_doc_fixtures::ebnf::Generator;
use datalove_doc_fixtures::token_grammar::{Recognizers, TokenGrammar, join_tokens};
use datalove_doc_fixtures::grammar_check::{Findings, Judge, example_wrong, examples, mutate, outside_codes};

struct Checker {
    recs: Recognizers,
    outside: BTreeSet<String>,
}

impl Checker {
    fn new(g: &TokenGrammar) -> Checker {
        Checker { recs: recognizers(g), outside: outside_codes(&doc_text(), DOC) }
    }

    fn derives(&self, start: Start, text: &str) -> bool {
        self.recs.derives(start.rule(), text)
    }

    fn check(&self, findings: &mut Findings, start: Start, text: &str) {
        let derives = |t: &str| self.derives(start, t);
        let parse = |t: &str| parse_codes(start, t);
        let judge = Judge { derives: &derives, parse: &parse, outside: &self.outside };
        findings.check(&judge, start.rule(), text);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn doc_examples() {
    let g = grammar();
    let checker = Checker::new(&g);
    let examples = examples(&doc_text(), DOC);
    assert!(examples.len() > 20, "found only {} examples in {DOC}", examples.len());
    let mut findings = Findings::default();
    let mut wrong = vec![];
    for ex in &examples {
        let start = match ex.context.as_str() {
            "script" => Start::Script,
            "module" => Start::Module,
            "interface" => Start::Interface,
            other => panic!("{DOC}:{}: `{other}` is not script, module or interface", ex.line),
        };
        wrong.extend(example_wrong(DOC, ex, parse_diags(start, &ex.text)));
        checker.check(&mut findings, start, &ex.text);
    }
    assert!(wrong.is_empty(), "{DOC} is wrong about:\n{}", wrong.join("\n"));
    findings.finish("the document's examples");
}

#[test]
fn corpus_agrees() {
    let g = grammar();
    let checker = Checker::new(&g);
    let mut findings = Findings::default();
    let corpus = corpus();
    assert!(corpus.len() > 500, "found only {} datafun sources", corpus.len());
    for (start, text) in corpus {
        let mut rng = SeededRng::new(seed_of(&text));
        checker.check(&mut findings, start, &text);
        checker.check(&mut findings, start, &mutate(&text, &mut rng));
    }
    findings.finish("the datafun sources and their mutants");
}

#[test]
fn generated_agree() {
    let g = grammar();
    let sampler = |text: &str, rng: &mut SeededRng| sample(&g, text, rng);
    let generator = Generator::new(&g.typed, &sampler);
    let checker = Checker::new(&g);
    let mut findings = Findings::default();
    let mut dropped = 0;
    let seeds = env_seeds("DATALOVE_DF_GRAMMAR", 0, 600);
    for seed in seeds.iter() {
        let start = [Start::Script, Start::Module, Start::Interface][(seed % 3) as usize];
        let mut rng = SeededRng::new(seed);
        let text = join_tokens(&generator.generate(start.rule(), &mut rng), &mut rng);
        // Spacing is drawn at random, and some decides what is glued.
        if !checker.derives(start, &text) {
            dropped += 1;
            continue;
        }
        checker.check(&mut findings, start, &text);
        checker.check(&mut findings, start, &mutate(&text, &mut SeededRng::derive(seed, "mutate")));
    }
    let n = seeds.count;
    assert!(dropped < n / 2, "{dropped} of {n} generated strings were not derived");
    println!("{dropped} of {n} generated strings were not derived");
    findings.finish("generated strings and their mutants");
}
