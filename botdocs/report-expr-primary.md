# Primary vs Full Expression Parsing for Keyword Payloads

**Status: Implemented.** The expr.rs path was switched to `parse_expr_primary`,
matching literal.rs. Test fixtures updated to parenthesize binop payloads.

## Summary

Several keywords (`some`, `ok`, `er`, `data`, `error`)
consume a payload expression.
There were two code paths that parsed these keywords,
and they disagreed on whether the payload
is a primary expression or a full expression.

This matters for the planned `atom`/`term` design,
where postfix `@` would coerce a term to a tagged union type.
With full-expression payloads, `term Bar 1@` parses as `term Bar (1@)`,
requiring parentheses: `(term Bar 1)@`.
With primary-expression payloads, `term Bar 1@` parses as `(term Bar 1)@`,
which is the desired behavior.

## Primary vs full expressions

Primary expressions are self-delimiting atoms
where the parser knows where they end without lookahead:

- Literals: `42`, `"hello"`, `true`, `false`
- Names and calls: `x`, `foo(a, b)`
- Parenthesized: `(anything)`
- Collection literals: `[1, 2]`, `{ x = 1 }`, `set { 1 }`
- Type hints: `: u32 / expr`

Full expressions add binary operators: `x + 1`, `a .> b and c`.

The parsing flow in `parse_expr_binop`:

1. `parse_expr_primary()` returns a primary
2. `parse_postfix_try_operators()` attaches `@`, `?`, `!`, `.field`
3. Precedence climbing loop for binary operators

Postfix operators bind to whatever `parse_expr_primary` returned.

## The two code paths

In `parse_expr_primary` (expr.rs:306-330), the main expression path:

- `some`, `ok`, `er` call `parse_expr_primary()` (line 310)
- `data`, `error` call `parse_expr_primary()` (line 323)

In `parse_lit_expr` (literal.rs:120-146), the `: type / expr` path:

- `some`, `ok`, `er` call `parse_expr_primary()` (lines 122, 127, 132)
- `data`, `error` call `parse_expr_primary()` (lines 138, 144)

Both paths now use primary-only parsing.

## Which keywords are affected

Only keywords with bare (non-delimited) payloads are affected:

| Keyword            | Payload             | Affected |
|--------------------|---------------------|----------|
| `some`, `ok`, `er` | bare expression     | yes      |
| `data`, `error`    | bare expression     | yes      |
| `none/true/false`  | none                | no       |
| `map`, `set`       | `{ ... }` braces    | no       |
| `icall`            | `name(args)` parens | no       |
| `[| ... |]`        | tensor literal       | no       |

Delimiter-enclosed keywords (`map`, `set`, `icall`) and sigil-delimited
literals (`[| |]` tensors) are immune because brackets/braces/parens mark
the payload boundary.

Future `atom` (no payload) is unaffected.
Future `term` (bare payload) would be affected.

## Behavioral difference

With `parse_expr_full` (old expr.rs path):

```
some x + 1      ->  some(x + 1)       payload includes binop
some x@         ->  some(x@)          postfix is inside payload
ok a +! b       ->  ok(a +! b)
term Bar 1@      ->  term Bar (1@)      @ on the integer, not the term
```

With `parse_expr_primary` (current behavior in both paths):

```
some x + 1      ->  (some x) + 1      binop escapes
some x@         ->  (some x)@         postfix escapes
ok a +! b       ->  (ok a) +! b
term Bar 1@      ->  (term Bar 1)@      @ on the whole term
```

Complex payloads require parentheses under primary-only parsing:

```
some(x + 1)     ->  some((x + 1))     works either way
ok(a +! b)      ->  ok((a +! b))      works either way
```

## Practical impact

The existing examples in botspec already parenthesize complex payloads:

```datalove
ret ok (a +! b)
ret some (a +? b)
```

The main case that changes is `some x@` / `ok x@`
(widen-then-wrap), which would need to become `some(x@)` / `ok(x@)`.
This is a niche pattern.

## Resolution

The two paths were reconciled by switching expr.rs to `parse_expr_primary`,
matching what literal.rs already does.
This was a one-line change per keyword group in `parse_expr_primary`.
It makes `term Bar 1@` work naturally for term coercion
and resolves the inconsistency between the two code paths.
