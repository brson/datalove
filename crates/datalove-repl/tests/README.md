# REPL Tests

`botdocs/repl-architecture.md` under "Tests" says what each suite covers. This
is how the two fixture suites are laid out and run.

## Engine fixtures

`engine_tests` runs each `.repl` file in `fixtures/engine/` through
`Engine::run_source`. Inputs are separated by `---` on its own line:

```
let x = 42
---
x
---
let y = x + 10
---
y
```

For each input it records the input, the parse result (`InputParse`), the
eval result (`Eval`) if there is one, and the environment afterwards. The
record is JSON, compared against `<name>.out.expected`. A run writes
`<name>.out.actual` alongside, which git ignores.

```bash
cargo test -p datalove-repl --test engine_tests
BLESS=1 cargo test -p datalove-repl --test engine_tests
```

`BLESS=1` rewrites the expected files from the actual ones. It also writes one
for a new fixture that has none.

## Reactive fixtures

`reactive_tests` runs each `.world` file in `fixtures/reactive/`: script units,
then steps that edit, delete, insert or append units, each with an `expect`
table of what every unit should do. The module docs of `reactive_tests.rs` give
the format. `botdocs/script-reactivity-cases.md` is rendered from these
fixtures, and the test checks it matches.

```bash
cargo test -p datalove-repl --test reactive_tests
BLESS=1 cargo test -p datalove-repl --test reactive_tests
```

Here `BLESS=1` re-renders the document only. The step tables are never
blessed, because they are the rules.
