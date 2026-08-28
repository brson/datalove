# Datafun Operator Precedence

Bot-maintained reference for operator precedence in datafun expressions.
Source: `crates/datalove-datafun-parser/src/expr.rs`

## Precedence Table (Highest to Lowest)

| Level | Operators | Associativity | Description |
|-------|-----------|---------------|-------------|
| 1 | `()` | - | Parenthesized grouping |
| 2 | `-` `-?` `-!` `not` `some` `ok` `er` `data` `error` | Right (prefix) | Unary operators |
| 3 | `.field` `.0` `[i]` `@` `?` `!` | Left (postfix) | Field, index, adapt, try |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Left | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Left | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Left | Comparison |
| 7 | `and` | Left | Logical AND |
| 8 | `or` `xor` | Left | Logical OR/XOR |

## Operator Categories

### Unary Prefix (Level 2)

| Operator | Description | Operand Types |
|----------|-------------|---------------|
| `-` | Numeric negation | int, f32, f64; signed fixed ints in checking position |
| `-?` | Optional negation (returns ?T) | Signed fixed ints |
| `-!` | Result negation (returns !T) | Signed fixed ints |
| `not` | Logical negation | bool |
| `some` `ok` `er` | Wrap a payload | Any |
| `data` `error` | Wrap a value | Any |

Bare `-` on a signed fixed int only typechecks where the expected type is
known, since synthesis restricts bare negation to int and the floats.

### Postfix (Level 3)

| Operator | Description | Operand Types |
|----------|-------------|---------------|
| `.field` | Struct field projection | Structs, tables |
| `.0` | Tuple index projection | Tuples |
| `[i]` | Index, producing a fallible place | Lists, maps, tensors, tables |
| `@` | Adapt: clone, widen, or coerce | See below |
| `?` | Unwrap Option, early-return none | ?T |
| `!` | Unwrap Result, early-return error | !T |

All six are parsed by the same loop and chain left to right, so `a.b[0]?@`
applies each in turn.

`@` needs to know its target type and cannot synthesize one, so it is only
valid where an expected type reaches it. See botspec section 6.8.

### Binary Multiplicative (Level 4)

| Operator | Description | Result |
|----------|-------------|--------|
| `*` | Multiply | f32, f64, int (widen fixed ints with `@` first) |
| `/` | Divide | f32, f64 only (use `/!` or `/?` for ints) |
| `*!` | Checked multiply | !T, early-return on overflow |
| `/!` | Checked divide | !T, early-return on div0/overflow |
| `*?` | Optional multiply | ?T, early-return on overflow |
| `/?` | Optional divide | ?T, early-return on div0/overflow |

### Binary Additive (Level 5)

| Operator | Description | Result |
|----------|-------------|--------|
| `+` | Add | f32, f64, int (widen fixed ints with `@` first) |
| `-` | Subtract | f32, f64, int (widen fixed ints with `@` first) |
| `+!` | Checked add | !T, early-return on overflow |
| `-!` | Checked subtract | !T, early-return on overflow |
| `+?` | Optional add | ?T, early-return on overflow |
| `-?` | Optional subtract | ?T, early-return on overflow |

### Binary Comparison (Level 6)

| Operator | Description |
|----------|-------------|
| `.<` | Less than |
| `.>` | Greater than |
| `<=` | Less or equal |
| `>=` | Greater or equal |
| `==` | Equal |
| `!=` | Not equal |

All return `bool`. Operands must be the same numeric type; `bool` and
`string` are not comparable with these, so `a == b` on two bools is an
error.

### Binary Logical (Levels 7-8)

| Operator | Level | Description |
|----------|-------|-------------|
| `and` | 7 | Logical AND |
| `or` | 8 | Logical OR |
| `xor` | 8 | Logical XOR |

All require `bool` operands and return `bool`.

## Examples

```datalove
// Precedence demonstration
not a or b          // (not a) or b
a and b or c        // (a and b) or c
a == b and c == d   // (a == b) and (c == d)
a + b * c           // a + (b * c)
-x + y              // (-x) + y
x? + y              // (x?) + y
a.b[0]?             // ((a.b)[0])?
```

## Where Postfix Attaches Around a Prefix

Postfix operators are applied to the result of the whole prefix expression,
not to the operand inside it:

```datalove
-x@                 // (-x)@   -- negate, then adapt
not x?              // (not x)?
```

`some`, `ok`, `er`, `data` and `error` are the exception. They collect
postfix onto their payload, so the postfix lands inside them:

```datalove
some x@             // some (x@)   -- adapt, then wrap
ok x?               // ok (x?)
```

The difference is visible in the lowered IR: `-x@` emits `neg` then `clone`,
while `some x@` emits `clone` then `some`. It comes from the parser, where
`-` and `not` take a bare primary as their operand while the payload
keywords run the postfix loop over theirs. Parenthesize to be unambiguous.

## Implementation Notes

- Binary operators use precedence-climbing algorithm
- Higher precedence number = binds tighter
- Unary prefix operators bind tighter than any binary operator
- Postfix operators bind looser than unary prefix, so they apply to the
  prefix expression as a whole, except under the payload keywords above
- `or` and `xor` share the same precedence level
