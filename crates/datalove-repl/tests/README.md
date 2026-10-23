# REPL Tests

`botdocs/repl-architecture.md` under "Tests" says what each suite covers. This
is how the fixture suite is laid out and run. The reactive fixtures are the
script engine's, in `crates/datalove-script-engine/tests/`.

## Engine fixtures

`engine_tests` runs each `.repl` file in `fixtures/engine/` through
`run_source`. Inputs are separated by `---` on its own line:

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
