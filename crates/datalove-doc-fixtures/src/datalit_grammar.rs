//! The datalit grammar in `botdocs/datalit-ebnf.md`, made executable.
//!
//! The grammar is over tokens, read as [`crate::token_grammar`] reads them,
//! with no `EOL`s: a newline means nothing in datalit but in a table. The
//! meaning of each special sequence of its character rules is here, and a
//! new one fails until it is given one.
//!
//! `tests/datalit_grammar_tests.rs` checks it against the datalit parser, and
//! the datafun grammar's test uses its type rules and literals.

use rmx::prelude::*;
use bct::random::SeededRng;

use crate::ebnf::Grammar;
use crate::grammar_check::{Diag, catch_panic, diags, workspace_dir};
use crate::token_grammar::{CharTest, Recognizers, TokenGrammar, char_rule_names};

pub const DOC: &str = "botdocs/datalit-ebnf.md";

/// The start rule.
pub const START: &str = "datalit";

pub fn doc_text() -> String {
    std::fs::read_to_string(workspace_dir().join(DOC)).X()
}

/// What the datalit parser reports of `text`, or its panic.
pub fn parse_diags(text: &str) -> Result<Vec<Diag>, String> {
    catch_panic(|| {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let found = datalove_datalit::parser::parse_integration_test::accumulated::<
            datalove_diagnostic::ParseDiagnostic,
        >(&db, source);
        diags(&db, &found)
    })
}

/// The codes the datalit parser refuses `text` with, or its panic.
pub fn parse_codes(text: &str) -> Result<Vec<String>, String> {
    parse_diags(text).map(|ds| ds.into_iter().map(|d| d.code).collect())
}

const STRING_CHAR: &str = r#"any character except '"' and '\', newlines included"#;

/// What each special sequence of the character rules means.
pub fn special(text: &str) -> CharTest {
    match text {
        STRING_CHAR => |s, i| s[i] != '"' && s[i] != '\\',
        t => panic!("no meaning is given here for the special sequence `? {t} ?`"),
    }
}

/// A character for a special sequence of the character rules, when generating.
pub fn char_sample(text: &str, rng: &mut SeededRng) -> String {
    let pool: &[char] = match text {
        STRING_CHAR => &['a', 'Z', ' ', '\n', '{', '|', ';', 'é', '/', '*'],
        t => panic!("no sample is given here for the special sequence `? {t} ?`"),
    };
    pool[rng.random_range(0..pool.len())].S()
}

/// The grammar of [`DOC`] as written.
pub fn ebnf() -> Grammar {
    Grammar::from_markdown(&doc_text())
}

/// The names of the rules [`DOC`] writes in characters.
pub fn char_rules() -> Vec<String> {
    char_rule_names(&doc_text(), DOC)
}

pub fn grammar() -> TokenGrammar {
    TokenGrammar::new(&ebnf(), &char_rules(), special)
}

pub fn recognizers(g: &TokenGrammar) -> Recognizers {
    Recognizers::new(g, &[START], false)
}

/// A string for a special sequence or island, when generating.
pub fn sample(g: &TokenGrammar, text: &str, rng: &mut SeededRng) -> String {
    crate::token_grammar::sample(g, &char_sample, text, rng)
}
