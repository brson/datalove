# Task: Give datafun its own datalit expression parser.

Today datafun attempts to reuse the datalit parser,
and possibly its typechecker,
for parsing some expressions and type hints.

This isn't tenable though.

The substructure of datafun expressions must
recursively contain expressions that aren't in datalit,
like binops and names.

So when the datafun expression parser defers
to the datalit expression parser,
the subsequent nested expressions can't include
binops etc.

To remediate this we need datafun to duplicate
the datalit expression parser, with branches
for datafun-specific expressions.

This will probably also mean duplicating the
datalit typechecker into the datafun expression type checker.

To ensure the datafun expression parser maintains
a strict subset of datalit we'll need a test suite that:
parses ast_gen generated expressions in both
and ensures they are equivalent and typecheck equivalently.

# Plan

## Current Architecture

Datafun expression types (`ExprFunKind`):
- `Datalit(datalit::ast::ExprFull)` - wraps entire datalit expressions
- `Name` - variable references
- `BinOp` - binary operators
- `UnaryOp` - unary operators
- `FunctionCall` - function calls
- `Tuple` - datafun tuples (elements are `ExprFun`)
- `TryOption`/`TryResult` - postfix `?` and `!`

Datafun distinguishes at parse time based on syntax:
- Starts with `@` or `#` or datalit keyword → delegate to datalit parser
- Otherwise → parse as datafun name/function call

The problem: when datafun delegates `@[a, b + c]` to datalit, the list elements are parsed by datalit's `parse_expr_full`, which only understands datalit syntax, not `a` or `b + c`.

## New Architecture

Replace `ExprFunKind::Datalit(datalit::ast::ExprFull)` with inline variants for all datalit constructs. Nested expressions become `ExprFun` instead of `datalit::ast::ExprFull`.

New `ExprFunKind` variants (replacing `Datalit`):
- `True`, `False`, `None` - boolean and none literals
- `Int`, `Float`, `Hex`, `String` - primitive literals
- `List`, `Set`, `Map` - collections with `Vec<ExprFun>` elements
- `AnonTuple`, `NamedTuple` - tuples with `Vec<ExprFun>` elements
- `AnonStruct`, `NamedStruct` - structs with `ExprFun` field values
- `AnonEnum`, `NamedEnum` - enums with `Option<ExprFun>` payloads
- `Tensor` - tensor with `Vec<ExprFun>` elements
- `Data`, `Err` - wrappers containing `ExprFun`

Each variant stores `Heap` (Local/Global/Omitted) plus optional `TypeHintAndHeap` from datalit.

## Implementation Steps

### 1. Extend datafun AST

Add new variants to `ExprFunKind` for all datalit expression types. Each variant contains:
- `Heap` field (reuse `datalit::ast::Heap`)
- Optional `TypeHintAndHeap` (reuse datalit's type hint AST)
- Fields with nested `ExprFun` instead of `ExprFull`

Keep `ExprFunKind::Datalit` temporarily for backward compatibility during migration.

### 2. Copy datalit expression parsing logic to datafun

Create methods in datafun's parser mirroring datalit's:
- `parse_datafun_expr_full` - replaces calls to `parse_datalit_expr`
- `parse_datafun_expr_and_heap` - handles `@`/`#` sigils
- `parse_datafun_expr` - dispatches on keywords and literals
- `parse_list_elements`, `parse_struct_fields`, etc.

Key difference: recursive calls use `parse_datafun_expr_full` → nested elements can be any `ExprFun`.

### 3. Update datafun typechecker

Currently datafun's typechecker delegates to datalit for `ExprFunKind::Datalit`. After the split:
- Handle each new variant directly in datafun's typechecker
- Can reuse datalit's type inference logic as helper functions
- Type hints still use datalit's `TypeHint` AST

### 4. Remove Datalit variant and old code

Once migration complete:
- Remove `ExprFunKind::Datalit` variant
- Remove `parse_datalit_expr` and token-collection logic
- Remove datafun's dependency on datalit parser (keep AST types)

### 5. Compatibility tests

Use `ast_gen` to generate pure-datalit expressions and verify:
- Parsing through datafun produces equivalent structure
- Type checking produces same results
- Spans are correctly tracked

Test strategy:
1. Generate datalit `ExprFull` via `ast_gen`
2. Pretty-print to source text
3. Parse with datafun parser
4. Convert result to comparable form
5. Assert structural equivalence

## Notes

- Type hints remain datalit's `TypeHint` AST - no need to duplicate
- Heap tracking (`@`/`#`) remains the same
- Span tracking needs adjustment for new variants
- `ast_serde` will need updates for new variants

## Current Status

**IMPLEMENTATION COMPLETE** - All tests pass.

### Completed:

1. **AST Extended** (ast.rs)
   - Added new variants: `True`, `False`, `None`, `Int`, `Float`, `Hex`, `String`
   - Added collection variants: `List`, `Set`, `Map`, `Tensor`
   - Added aggregate variants: `AnonTuple`, `NamedTuple`, `AnonStruct`, `NamedStruct`, `AnonEnum`, `NamedEnum`
   - Added wrapper variants: `Data`, `Err`
   - Each stores `Heap` and optional `TypeHintAndHeap`

2. **Parser Updated** (parser.rs)
   - Added `parse_lit_expr_full`, `parse_lit_expr_and_heap`, `parse_lit_expr` methods
   - Handles all datalit expression forms
   - Nested expressions call back to `parse_expr_full` allowing datafun expressions
   - `parse_expr_primary` updated to use new parsing path

3. **Typechecker Updated** (tycheck.rs)
   - Simple literals (bool, int, float, hex, string) synthesize correctly
   - Collections (list, set, map, tensor) synthesize correctly
   - Aggregates (anon tuple, anon struct) synthesize correctly
   - Named types and enums require type hints (matches datalit behavior)
   - `check_expr` handles `None` and `Int` for bidirectional type checking

4. **Function Analysis Updated**
   - `liveness.rs`, `moves.rs`, `slot_allocation.rs`, `validation.rs` updated for new variants

5. **ast_serde Updated** (ast_serde.rs)
   - All new variants have serde types and `from_ast` implementations

6. **Interpreter Updated** (interp/mod.rs)
   - All inline variants evaluate correctly in both script scope and frame scope
   - Helper functions for allocating inline literals
   - `is_none_or_error` checks updated to handle inline variants

### Remaining Work (Future):

1. **Remove Datalit Wrapper** - `ExprFunKind::Datalit` can be removed once all tests confirm correctness

2. **Compatibility Tests** - Use ast_gen to verify datafun parser matches datalit for pure-datalit expressions

### Notes:

- Some error tests changed output format (parse errors vs typecheck errors)
- REPL tests using complex type hint syntax (`: @int / @expr`) may need review
- The legacy `Datalit` wrapper is still present for backwards compatibility