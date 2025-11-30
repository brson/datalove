# Plan: Hex Literal Support for Float Special Values

## Problem

Float special values (NaN, infinity, MIN, MAX) cannot be tested because:
1. Parser doesn't support scientific notation or special float literals
2. `Expr::Hex` is parsed and type-checked but **not instantiated**
3. AST generator excludes these values with FIXME comments

Per botspec (section 1.6): Hex literals like `0xABABABAB` with f32 type hint are interpreted as raw bit patterns.

## Current State

| Component | Hex Ints | Hex Floats |
|-----------|----------|------------|
| Parsing | Done | Done (same syntax) |
| Type checking | Done | Done (validates u32 range) |
| Instantiation | **Missing** | **Missing** |
| AST generation | **Missing** | **Missing** |

## Implementation

### 1. Add `Expr::Hex` cases to `instantiate2.rs`

Add match arms in `instantiate_expr_into` for hex literals:

```rust
// Hex literals for integers - parse as hex, write value
(Expr::Hex(hex_expr), Type::U8) => instantiate_hex_u8(rt, db, hex_expr, dest_ptr),
(Expr::Hex(hex_expr), Type::U16) => instantiate_hex_u16(rt, db, hex_expr, dest_ptr),
(Expr::Hex(hex_expr), Type::U32) => instantiate_hex_u32(rt, db, hex_expr, dest_ptr),
(Expr::Hex(hex_expr), Type::U64) => instantiate_hex_u64(rt, db, hex_expr, dest_ptr),
(Expr::Hex(hex_expr), Type::Int) => instantiate_hex_bigint(rt, db, hex_expr, dest_ptr),

// Hex literal for f32 - interpret as bit pattern
(Expr::Hex(hex_expr), Type::F32) => instantiate_hex_f32(rt, db, hex_expr, dest_ptr),
```

The f32 case uses `f32::from_bits(parsed_u32)`.

### 2. Add hex generation to `ast_gen.rs`

Add `gen_hex_expr` function that generates hex literals for:
- Integer types (u8, u16, u32, u64)
- f32 special values via bit patterns

Special f32 values to include:
- `0x7FC00000` - quiet NaN
- `0x7F800000` - +infinity
- `0xFF800000` - -infinity
- `0xFF7FFFFF` - f32::MIN
- `0x7F7FFFFF` - f32::MAX
- `0x00000001` - smallest positive subnormal
- `0x80000001` - smallest negative subnormal

Modify `gen_f32_expr` to use hex for special values instead of excluding them.

### 3. `canon.rs` hex comparison

Already implemented (lines 47-52) - compares hex as unsigned integers via `parse_hex`.

### 4. Add instantiation tests

Add tests in `datalove-rt-tests` or `datalove-datalit` for:
- Hex integer instantiation (various sizes)
- Hex f32 bit patterns (NaN, infinity, MIN, MAX)
- Roundtrip: generate -> instantiate -> verify value

## Files to Modify

1. `crates/datalove-datalit/src/instantiate2.rs` - add Expr::Hex cases
2. `crates/datalove-datalit/src/ast_gen.rs` - add hex generation, include special floats
3. `crates/datalove-datalit/src/canon.rs` - add Expr::Hex comparison (already done)
4. `crates/datalove-rt-tests/tests/` - add hex instantiation tests

## Key Bit Patterns

| Value | Hex | Description |
|-------|-----|-------------|
| NaN (quiet) | `0x7FC00000` | Standard quiet NaN |
| +Infinity | `0x7F800000` | Positive infinity |
| -Infinity | `0xFF800000` | Negative infinity |
| f32::MAX | `0x7F7FFFFF` | ~3.4028235e38 |
| f32::MIN | `0xFF7FFFFF` | ~-3.4028235e38 |
| +0.0 | `0x00000000` | Positive zero |
| -0.0 | `0x80000000` | Negative zero |
