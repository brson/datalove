# Documentation Fixtures

The fixtures behind the documentation, and the tests that keep it true.
`botdocs/doc-fixtures.md` explains them.

- `reactive/` -- the step tables `botdocs/script-reactivity-cases.md` is
  rendered from. The module docs of `tests/reactive_tests.rs` give the format.
- `replays/` -- the sessions the posts play, each a worldfile with the modules
  and data it requires, and step tables of its own. The module docs of
  `tests/replay_tests.rs` give the format.

```bash
cargo test -p datalove-doc-fixtures
BLESS=1 cargo test -p datalove-doc-fixtures
```

`BLESS=1` re-renders the document and records the replays again. The step
tables are never blessed, because they are the rules.
