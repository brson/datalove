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
//!
//! The parser must group an expression as the grammar does: the spans of the
//! expressions it builds must be those of the grammar's one parse tree, read
//! by `datalove_doc_fixtures::grouping`, for generated expressions drawn
//! from the seeds of `DATALOVE_DF_GROUPING_SEED` and
//! `DATALOVE_DF_GROUPING_COUNT`.
//!
//! The generated strings must reach every rule, alternative, option and
//! repetition of the grammar: see `datalove_doc_fixtures::ebnf::Coverage`.
//!
//! The grammar's `no X: CODE` notes must hold: see
//! `datalove_doc_fixtures::deletion`, on strings from the seeds of
//! `DATALOVE_DF_DELETION_SEED` and `DATALOVE_DF_DELETION_COUNT`.

use rmx::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::datafun_grammar::{
    DOC, Start, corpus, doc_text, grammar, parse_codes, parse_diags, parse_expr_spans, recognizers, sample,
};
use datalove_doc_fixtures::deletion::Deletions;
use datalove_doc_fixtures::ebnf::{Coverage, Generator, Parse};
use datalove_doc_fixtures::grouping::{expression_rules, grammar_grouping, parser_grouping, render};
use datalove_doc_fixtures::token_grammar::{Recognizers, TokenGrammar, join_spaced, join_tokens};
use datalove_doc_fixtures::grammar_check::{Findings, Judge, example_wrong, examples, mutate, outside_codes, shrink};

/// The rule an expression is.
const EXPR: &str = "expr";

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
    let starts = [Start::Script, Start::Module, Start::Interface];
    let mut coverage = Coverage::new(&generator, &starts.map(Start::rule));
    let seeds = env_seeds("DATALOVE_DF_GRAMMAR", 0, 600);
    for seed in seeds.iter() {
        let start = starts[(seed % 3) as usize];
        let mut rng = SeededRng::new(seed);
        let (pieces, trace) = generator.generate_traced(start.rule(), &mut rng);
        let text = join_tokens(&pieces, &mut rng);
        // Spacing is drawn at random, and some decides what is glued.
        if !checker.derives(start, &text) {
            dropped += 1;
            continue;
        }
        coverage.add(&trace);
        checker.check(&mut findings, start, &text);
        checker.check(&mut findings, start, &mutate(&text, &mut SeededRng::derive(seed, "mutate")));
    }
    let n = seeds.count;
    assert!(dropped < n / 2, "{dropped} of {n} generated strings were not derived");
    println!("{dropped} of {n} generated strings were not derived");
    findings.finish("generated strings and their mutants");
    println!("{}", coverage.summary());
    coverage.assert_full("the generated strings");
}

/// How the grammar and the parser group `text` as one expression, if both
/// take it and they differ; `Err` says why they were not compared.
fn grouping_differs(recs: &Recognizers, exprs: &BTreeSet<String>, text: &str) -> Result<Option<String>, Skip> {
    let tokens = recs.tokens(text);
    if tokens.len() > MAX_TOKENS {
        return Err(Skip::Long);
    }
    let (diags, spans) = parse_expr_spans(text).map_err(Skip::Panicked)?;
    if !diags.is_empty() {
        return Err(Skip::Refused);
    }
    let tree = match recs.parse(EXPR, &tokens) {
        Parse::Tree(tree) => tree,
        Parse::Rejected => return Err(Skip::Underived),
        Parse::Ambiguous(rule, _) => return Err(Skip::Ambiguous(rule)),
    };
    let ours = grammar_grouping(&tree, &tokens, exprs);
    let theirs = parser_grouping(&spans, &tokens);
    Ok((ours != theirs).then(|| fmt!("the grammar groups it {}, the parser {}", render(text, &ours), render(text, &theirs))))
}

/// The most tokens of an expression whose grouping is compared.
const MAX_TOKENS: usize = 40;

/// Why an expression was not compared.
enum Skip {
    Panicked(String),
    /// The parser refuses it, by a rule outside the grammar.
    Refused,
    /// It is longer than [`MAX_TOKENS`]: a parse tree costs the cube of the
    /// length, and a few operators show how they group.
    Long,
    /// The grammar does not derive it, which a generated string can miss
    /// where its spacing decides what is glued.
    Underived,
    /// The grammar derives it in two ways, which an expression must not.
    Ambiguous(String),
}

#[test]
fn generated_group_alike() {
    let g = grammar();
    let sampler = |text: &str, rng: &mut SeededRng| sample(&g, text, rng);
    // Grouping shows as well in a small expression, which the parser is
    // likelier to take whole: a long one nearly always has a payload with
    // an operator after it (P065) or a call on something not a name (P072).
    let generator = Generator::new(&g.typed, &sampler).max_depth(0);
    let recs = Recognizers::new(&g, &[EXPR], false);
    let exprs = expression_rules(&g.typed, EXPR);
    let mut differ: BTreeMap<String, String> = BTreeMap::new();
    let (mut compared, mut refused, mut underived) = (0, 0, 0);
    let seeds = env_seeds("DATALOVE_DF_GROUPING", 0, 600);
    for seed in seeds.iter() {
        // Each seed draws until the parser takes one short enough.
        let mut rng = SeededRng::new(seed);
        let found = (0..20).find_map(|_| {
            let text = join_spaced(&generator.generate(EXPR, &mut rng));
            match grouping_differs(&recs, &exprs, &text) {
                Err(Skip::Refused | Skip::Long) => None,
                found => Some((text, found)),
            }
        });
        let Some((text, found)) = found else {
            refused += 1;
            continue;
        };
        let what = match found {
            Ok(None) => {
                compared += 1;
                continue;
            }
            Err(Skip::Underived) => {
                underived += 1;
                continue;
            }
            Err(Skip::Refused | Skip::Long) => bug!("a refused or long expression is drawn again"),
            Err(Skip::Panicked(e)) => fmt!("the parser panicked: {e}"),
            Err(Skip::Ambiguous(rule)) => fmt!("the grammar derives it two ways, in `{rule}`"),
            Ok(Some(_)) => {
                compared += 1;
                let small = shrink(&text, &|t| matches!(grouping_differs(&recs, &exprs, t), Ok(Some(_))));
                let Ok(Some(what)) = grouping_differs(&recs, &exprs, &small) else { bug!("a shrunk case differs") };
                what
            }
        };
        differ.entry(what).or_insert(text);
    }
    let n = seeds.count;
    println!("{compared} of {n} generated expressions compared; {refused} refused or too long 20 times, {underived} not derived");
    assert!(compared > n / 2, "only {compared} of {n} generated expressions were compared");
    let report: Vec<String> = differ.iter().take(30).map(|(what, from)| fmt!("{what}\n    from {from:?}")).collect();
    assert!(differ.is_empty(), "the grammar and the parser differ on {} expressions:\n\n{}", differ.len(), report.join("\n\n"));
}

/// The grammar's notes of what a missing part is refused with hold.
#[test]
fn deletion_notes_hold() {
    let g = grammar();
    let sampler = |text: &str, rng: &mut SeededRng| sample(&g, text, rng);
    // Short strings, which the parser takes whole more often: a long one
    // nearly always breaks a rule outside the grammar somewhere.
    let generator = Generator::new(&g.typed, &sampler).max_depth(0).lean();
    let starts = [Start::Script, Start::Module, Start::Interface];
    let recs = Recognizers::types_written_out(&g, &starts.map(Start::rule), true);
    let parse = |start: &str, text: &str| {
        let start = [Start::Script, Start::Module, Start::Interface].into_iter().find(|s| s.rule() == start).X();
        parse_diags(start, text)
    };
    let mut deletions = Deletions::new(&g, &recs, &parse);
    for ex in examples(&doc_text(), DOC).iter().filter(|ex| ex.codes.is_empty()) {
        deletions.check(&ex.context, &ex.text);
    }
    for seed in env_seeds("DATALOVE_DF_DELETION", 0, 600).iter() {
        let start = starts[(seed % 3) as usize];
        // Each seed draws until the parser takes one whole.
        let mut rng = SeededRng::new(seed);
        for _ in 0..20 {
            if deletions.check(start.rule(), &join_spaced(&generator.generate(start.rule(), &mut rng))) {
                break;
            }
        }
    }
    deletions.finish("generated strings");
}
