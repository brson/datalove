# Datalit CLI Commands

Commands for working with Datalit files.

## lit-tycheck

Type check a Datalit file and report errors.

```bash
datalove lit-tycheck <file.dlt>
```

### Example

```bash
datalove lit-tycheck example.dlt
```

Output:

```
No type errors found.
```

Or with errors:

```
Type errors found:
  Type mismatch: expected u32, found string
```

## lit-ast

Print the abstract syntax tree of a Datalit expression.

```bash
datalove lit-ast <file.dlt>
```

Useful for debugging the parser and understanding how Datalove parses your code.

### Example

```bash
datalove lit-ast example.dlt
```

## lit-pretty

Pretty print a Datalit file with consistent formatting.

```bash
datalove lit-pretty <file.dlt>
```

### Example

```bash
datalove lit-pretty example.dlt
```

Outputs a nicely formatted version of your Datalit code.

## lit-op

Run built-in operations on Datalit expressions.

```bash
datalove lit-op <expr1> <op> <expr2>
```

### Supported Operations

#### `eq` - Equality

Check if two values are equal:

```bash
datalove lit-op '1' eq '1'
# Output: true

datalove lit-op '[1,2,3]' eq '[1,2,4]'
# Output: false
```

#### `cmp` - Total Ordering Comparison

Compare two values using total ordering:

```bash
datalove lit-op '1' cmp '2'
# Output: less

datalove lit-op '[1,2,3]' cmp '[1,2,3]'
# Output: equal

datalove lit-op '5' cmp '3'
# Output: greater
```

### Examples

Compare structures:

```bash
datalove lit-op '{x=1,y=2}' eq '{x=1,y=2}'
# Output: true
```

Compare lists:

```bash
datalove lit-op '[1,2,3]' cmp '[1,2,4]'
# Output: less
```

## See Also

- [CLI Reference](index.md)
- [REPL](repl.md)
