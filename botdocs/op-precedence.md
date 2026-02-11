# Datafun Operator Precedence

Bot-maintained reference for operator precedence in datafun expressions.
Source: `crates/datalove-datafun-parser/src/expr.rs`

## Precedence Table (Highest to Lowest)

| Level | Operators | Associativity | Description |
|-------|-----------|---------------|-------------|
| 1 | `()` | - | Parenthesized grouping |
| 2 | `-` `-?` `-!` `not` | Right (prefix) | Unary operators |
| 3 | `?` `!` | Left (postfix) | Try operators |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Left | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Left | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Left | Comparison |
| 7 | `and` | Left | Logical AND |
| 8 | `or` `xor` | Left | Logical OR/XOR |

## Operator Categories

### Unary Prefix (Level 2)

| Operator | Description | Operand Types |
|----------|-------------|---------------|
| `-` | Numeric negation | int, signed fixed ints, f32 |
| `-?` | Optional negation (returns ?T) | Signed fixed ints |
| `-!` | Result negation (returns !T) | Signed fixed ints |
| `not` | Logical negation | bool |

### Postfix Try (Level 3)

| Operator | Description | Operand Types |
|----------|-------------|---------------|
| `?` | Unwrap Option, early-return none | ?T |
| `!` | Unwrap Result, early-return error | !T |

### Binary Multiplicative (Level 4)

| Operator | Description | Result |
|----------|-------------|--------|
| `*` | Multiply | Same type or widened to int |
| `/` | Divide | f32 only (use `/!` or `/?` for ints) |
| `*!` | Checked multiply | !T, early-return on overflow |
| `/!` | Checked divide | !T, early-return on div0/overflow |
| `*?` | Optional multiply | ?T, early-return on overflow |
| `/?` | Optional divide | ?T, early-return on div0/overflow |

### Binary Additive (Level 5)

| Operator | Description | Result |
|----------|-------------|--------|
| `+` | Add | Same type or widened to int |
| `-` | Subtract | Same type or widened to int |
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

All return `bool`. Operands must be same numeric type.

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
```

## Implementation Notes

- Binary operators use precedence-climbing algorithm
- Higher precedence number = binds tighter
- Unary prefix operators bind tighter than any binary operator
- Postfix try operators bind between unary prefix and binary
- `or` and `xor` share the same precedence level
