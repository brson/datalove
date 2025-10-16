# CLI Reference

The `datalove` command-line interface provides tools for working with Datalove code.

## Commands

### Datalit Commands

- [lit-tycheck](lit-commands.md#lit-tycheck) - Type check Datalit files
- [lit-ast](lit-commands.md#lit-ast) - Print AST of Datalit expressions
- [lit-pretty](lit-commands.md#lit-pretty) - Pretty print Datalit files
- [lit-op](lit-commands.md#lit-op) - Run operations on Datalit values

### REPL

- [repl](repl.md) - Start interactive REPL or run scripts

### Documentation

- `docs build` - Build static documentation site
- `docs serve` - Serve documentation locally
- `docs init` - Initialize documentation structure

## Quick Examples

Type check a file:

```bash
datalove lit-tycheck example.dlt
```

Pretty print:

```bash
datalove lit-pretty example.dlt
```

Compare values:

```bash
datalove lit-op '1' eq '1'
datalove lit-op '[1,2,3]' cmp '[1,2,4]'
```

Start REPL:

```bash
datalove repl
```

## Getting Help

```bash
datalove --help
datalove <command> --help
```
