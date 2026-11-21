# Documentation Fixtures

The fixtures behind the documentation, and the tests that keep it true.
`botdocs/doc-fixtures.md` explains them.

- `reactive/` -- the step tables `botdocs/script-reactivity-cases.md` is
  rendered from. The module docs of `tests/reactive_tests.rs` give the format.
- `replays/` -- the sessions the posts play, each a worldfile with the modules
  and data it requires, and step tables of its own. The module docs of
  `tests/replay_tests.rs` give the format.

`tests/lexical_grammar_tests.rs`, `tests/datalit_grammar_tests.rs` and
`tests/datafun_grammar_tests.rs` have no fixtures: they run the grammars in
`botdocs/lexical-grammar.md`, `botdocs/datalit-ebnf.md` and
`botdocs/datafun-ebnf.md` against the lexer and parsers, with the recognizer
and generator in `src/ebnf.rs`, the checking in `src/grammar_check.rs`, and
the grammars made executable in `src/lexical.rs`, `src/token_grammar.rs`,
`src/datalit_grammar.rs` and `src/datafun_grammar.rs`, and how an expression
groups by the grammar's parse tree in `src/grouping.rs`, and the grammars'
`no X: CODE` notes in `src/deletion.rs`. `tests/recovery_tests.rs` measures how the parsers
recover from errors, and holds their diagnostics to a well-formed shape.

```bash
cargo test -p datalove-doc-fixtures
BLESS=1 cargo test -p datalove-doc-fixtures
```

`BLESS=1` re-renders the document and records the replays again. The step
tables are never blessed, because they are the rules.
