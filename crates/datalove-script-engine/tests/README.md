# Script Engine Tests

`botdocs/repl-architecture.md` under "Tests" says what each suite covers. This
is how the fixture suite is laid out and run.

## Reactive fixtures

`reactive_tests` runs each `.world` file in `fixtures/reactive/`: script units,
then steps that edit, delete, insert or append units, each with an `expect`
table of what every unit should do. The module docs of `reactive_tests.rs` give
the format. `botdocs/script-reactivity-cases.md` is rendered from these
fixtures, and the test checks it matches.

```bash
cargo test -p datalove-script-engine --test reactive_tests
BLESS=1 cargo test -p datalove-script-engine --test reactive_tests
```

Here `BLESS=1` re-renders the document only. The step tables are never
blessed, because they are the rules.
