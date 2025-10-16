# REPL Engine Tests

Example-based tests for the Datalove REPL engine.

## Test Format

Test files use the `.repl` extension and are located in `tests/fixtures/engine/`.

Each test file contains multiple inputs separated by `---` on its own line:

```
let x = 42
---
x
---
let y = x + 10
---
y
```

## Running Tests

Run all engine tests:
```bash
cargo test -p datalove-repl --test engine_tests
```

## Updating Expected Output

When the engine behavior changes or new tests are added, update the expected output:
```bash
BLESS=1 cargo test -p datalove-repl --test engine_tests
```

## Test Output

For each input, the test records:
- The input string
- The parse result (InputParse)
- The eval result (Eval), if applicable
- The environment after the step (list of name, type, value bindings)

Output is serialized as JSON in `.out.expected` files.

## Example Tests

- `basic.repl` - Simple let bindings and expressions
- `simple.repl` - Multiple let bindings with expressions
- `errors.repl` - Parse and type errors
