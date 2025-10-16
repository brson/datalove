# REPL

The Datalove Read-Eval-Print Loop (REPL) for interactive programming.

## Starting the REPL

```bash
datalove repl
```

## Basic Usage

Type expressions and see results immediately:

```datalove
> 1 + 2
3

> [1, 2, 3]
[1, 2, 3]

> {name = "Ada", age = 36}
{name = "Ada", age = 36}
```

## Running Scripts

Execute a Datalove script file:

```bash
datalove repl --script example.dls
```

## REPL Features

The Datalove REPL is designed to be a modern, powerful interactive environment:

### Planned Features

- **Undo/Redo**: Step backward and forward through your session
- **Rewind/Replay**: Return to earlier states and continue from there
- **Hot-reloading**: Modify code and see changes instantly
- **Virtualized I/O**: Record and replay I/O operations

### Current Implementation

The REPL is currently in active development. Basic expression evaluation is supported.

## Terminal Interface

The REPL uses a modern Ratatui-based terminal interface for rich text rendering and editing.

## Web Interface

A web-based REPL is available through the egui_ratatui implementation, providing WASM-compatible interactive programming.

## Keyboard Shortcuts

- `Ctrl+D` or `Ctrl+C`: Exit the REPL
- (More shortcuts coming as the REPL develops)

## See Also

- [CLI Reference](index.md)
- [Getting Started](../getting-started/first-steps.md)
