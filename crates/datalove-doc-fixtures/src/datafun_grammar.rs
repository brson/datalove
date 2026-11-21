//! The datafun grammar in `botdocs/datafun-ebnf.md`, made executable.
//!
//! The grammar is over tokens, read as [`crate::token_grammar`] reads them,
//! with `EOL`s. A rule that is `? as in the datalit grammar ?` is that rule
//! of the datalit grammar, with every rule it uses, over tokens or in
//! characters as the datalit grammar writes it; one the two grammars both
//! write must be written the same in both.

use rmx::prelude::*;
use std::collections::BTreeSet;

use bct::random::SeededRng;

use crate::datalit_grammar;
use crate::ebnf::{Expr, Grammar};
use crate::grammar_check::{Diag, catch_panic, diags, files, workspace_dir};
use crate::token_grammar::{Recognizers, TokenGrammar, char_rule_names};

pub const DOC: &str = "botdocs/datafun-ebnf.md";

pub fn doc_text() -> String {
    std::fs::read_to_string(workspace_dir().join(DOC)).X()
}

const FROM_DATALIT: &str = "as in the datalit grammar";

/// The grammar of [`DOC`], with the rules it takes from the datalit grammar.
pub fn grammar() -> TokenGrammar {
    let doc = doc_text();
    let mut g = Grammar::from_markdown(&doc);
    let mut chars = char_rule_names(&doc, DOC);
    let dl = datalit_grammar::ebnf();
    let dl_chars = datalit_grammar::char_rules();
    let from = Expr::Special(FROM_DATALIT.S());
    let mut stack: Vec<String> = g.rules.iter().filter(|r| r.body == from).map(|r| r.name.C()).collect();
    assert!(stack.contains(&"type".S()), "{DOC} does not take type from the datalit grammar");
    g.rules.retain(|r| r.body != from);
    chars.retain(|c| !stack.contains(c));
    let mut seen = BTreeSet::new();
    while let Some(name) = stack.pop() {
        if !seen.insert(name.C()) {
            continue;
        }
        let rule = dl.get(&name).unwrap_or_else(|| panic!("{name} is no rule of the datalit grammar"));
        match g.get(&name) {
            Some(own) => assert_eq!(own.body, rule.body, "{name} is written differently in the two grammars"),
            None => {
                g.rules.push(rule.C());
                if dl_chars.contains(&name) {
                    chars.push(name);
                }
            }
        }
        rule.body.visit(&mut |e| {
            if let Expr::Ref(r) = e {
                stack.push(r.C());
            }
        });
    }
    TokenGrammar::new(&g, &chars, datalit_grammar::special)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Script,
    Module,
    Interface,
}

impl Start {
    pub fn rule(self) -> &'static str {
        match self {
            Start::Script => "script",
            Start::Module => "module",
            Start::Interface => "interface",
        }
    }
}

pub fn recognizers(g: &TokenGrammar) -> Recognizers {
    Recognizers::new(g, &[Start::Script.rule(), Start::Module.rule(), Start::Interface.rule()], true)
}

// ============================================================================
// The parser
// ============================================================================

/// What the parser reports of `text` as a `start`, or its panic.
pub fn parse_diags(start: Start, text: &str) -> Result<Vec<Diag>, String> {
    use datalove_datafun_parser as p;
    use datalove_diagnostic::ParseDiagnostic;
    catch_panic(|| {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let found = match start {
            Start::Script => p::parse::accumulated::<ParseDiagnostic>(&db, source),
            Start::Module => p::parse_module_for_test::accumulated::<ParseDiagnostic>(&db, source),
            Start::Interface => p::parse_rider_interface::accumulated::<ParseDiagnostic>(&db, source),
        };
        diags(&db, &found)
    })
}

/// What the parser reports of `text` as one expression, or its panic.
pub fn parse_expr_diags(text: &str) -> Result<Vec<Diag>, String> {
    use datalove_diagnostic::ParseDiagnostic;
    catch_panic(|| {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let found = datalove_datafun_parser::parse_expr::accumulated::<ParseDiagnostic>(&db, source);
        diags(&db, &found)
    })
}

/// What the parser reports of `text` as one expression, and the byte spans
/// of the expressions it builds, or its panic.
pub fn parse_expr_spans(text: &str) -> Result<(Vec<Diag>, Vec<std::ops::Range<usize>>), String> {
    use datalove_diagnostic::ParseDiagnostic;
    catch_panic(|| {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let found = datalove_datafun_parser::parse_expr::accumulated::<ParseDiagnostic>(&db, source);
        let spans = datalove_datafun_parser::parse_expr(&db, source)
            .expr_spans
            .iter()
            .map(|e| e.span.start as usize..e.span.end as usize)
            .collect();
        (diags(&db, &found), spans)
    })
}

/// The codes the parser refuses `text` with as a `start`, or its panic.
pub fn parse_codes(start: Start, text: &str) -> Result<Vec<String>, String> {
    parse_diags(start, text).map(|ds| ds.into_iter().map(|d| d.code).collect())
}

// ============================================================================
// Inputs
// ============================================================================

/// Every datafun source under `crates/` and `sys/`, and what it is parsed as.
pub fn corpus() -> Vec<(Start, String)> {
    let root = workspace_dir();
    let mut out = vec![];
    for dir in ["crates", "sys"] {
        for path in files(&root.join(dir), &["dfs", "dfm", "dli", "world"]) {
            let text = std::fs::read_to_string(&path).X();
            match path.extension().X().to_str().X() {
                "dfs" => out.push((Start::Script, text)),
                "dfm" => out.push((Start::Module, text)),
                "dli" => out.push((Start::Interface, text)),
                _ => {
                    for section in datalove_script_steps::sections(&text) {
                        let kind = section.header.split_whitespace().next().unwrap_or("");
                        match kind {
                            "module" | "module-edit" | "module-add" => out.push((Start::Module, section.content)),
                            "scriptunit-fragment" | "scriptunit-edit" | "scriptunit-insert" => {
                                out.push((Start::Script, section.content))
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    out
}

/// A string for a special sequence or island, when generating.
pub fn sample(g: &TokenGrammar, text: &str, rng: &mut SeededRng) -> String {
    crate::token_grammar::sample(g, &datalit_grammar::char_sample, text, rng)
}
