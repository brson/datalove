# Datalit vs Datafun Differences

Datalove has a layered design where Datafun builds on top of Datalit. This document tracks semantic and syntactic differences between the two layers.

## Overview

- **Datalit** (.dlt): Pure data literal language. No computation, just values.
- **Datafun** (.dfs/.dfm): Functional layer with expressions, statements, functions, control flow.

## Syntactic Differences

### Parentheses: Grouping vs Tuple

| Context | `(expr)` meaning | `(expr,)` meaning |
|---------|------------------|-------------------|
| **Datafun** | Grouping (returns expr's type) | Single-element tuple |
| **Datalit** | Single-element tuple | Single-element tuple |

In Datafun, parentheses serve dual purpose: grouping for precedence (`ok (a /! b)`) and tuple construction (`(a, b)`). A single element without trailing comma is grouping.

In Datalit, parentheses after a heap marker construct tuples. `@(42)` is a single-element tuple, not grouping. This is intentional since datalit has no expressions that need precedence grouping.

### Type Hint Syntax

| Layer | Syntax | Example |
|-------|--------|---------|
| **Datalit** | `: type / expr` | `: @u32 / 42` |
| **Datafun** | `name: type` in declarations | `let x: u32 = 42` |

Datalit uses the type-hint-before-value syntax for explicit typing. Datafun uses standard declaration syntax.

### Keywords

These are keywords only in Datafun, not Datalit:

- `some` - construct Option Some variant
- `ok` - construct Result Ok variant
- `er` - construct Result Err variant
- `fun`, `let`, `ret`, `if`, `else`, `loop`, `break`, `continue`
- `require`, `import`

### Literals

These are parsed in both layers but have Datalit syntax:

- `@none` - Option None
- `@error "msg"` - Error value
- `@true`, `@false` - Booleans
- `@42`, `#42` - Integers with heap annotation

## Semantic Differences

### Expressions vs Values

- **Datalit**: Only literal values. No variables, operators, or function calls.
- **Datafun**: Full expressions including names, operators, calls, and all datalit values.

### Heap Annotations

Both layers use `@` (local) and `#` (global) heap annotations. In Datalit they're required on most literals. In Datafun they're often inferred.

## Parser Implementation

The parser (`parser.rs`) handles both layers:

- `parse_datalit_expr` - Datalit expressions
- `parse_datafun_expr` - Datafun expressions (includes datalit as subset)
- `parse_lit_anon_tuple` - Datalit tuple parsing (always tuple)
- `parse_datafun_tuple` - Datafun tuple parsing (grouping if single element)
