# Coercion Implementation Report

## Runtime Value Coercions

Located in `crates/datalove-datafun-compiler/src/interp/coerce.rs`:

1. **T -> Option\<T\>** - Wraps value in `Some` (lines 96-130)
2. **T -> Result\<T\>** - Wraps value in `Ok` (lines 132-166)
3. **T -> Data** - Wraps any value in existential `Data` container (lines 168-183)
4. **Data -> Option\<Data\>** - Wraps Data in `Some` (lines 185-214)
5. **Data -> Result\<Data\>** - Wraps Data in `Ok` (lines 216-243)
6. **Int -> u32 narrowing** - `narrow_int_to_u32` function (lines 11-60)

## Type-Level Coercions

Located in `crates/datalove-datalit/src/tycheck.rs`:

7. **Anonymous Struct -> Named Struct** - Field-by-field coercion (line 1152)
8. **Anonymous Enum -> Named Enum** - Variant matching (line 1309)
9. **Anonymous Tuple -> Named Tuple** - Arity-checked coercion (line 1510)
10. **Numeric Widening** - Via `can_widen_to` (lines 1825-1829)

## Infrastructure

- **`CoercionError`** enum in `datafun-compiler/src/tycheck.rs:2547`
  - `TypeMismatch`
  - `ArityMismatch`
- **`check_type_coercion`** - Centralized validation (line 2565)
- **`coerce_value_to_dest`** - Runtime coercion entry point (`interp/coerce.rs:62`)

## Complexity Notes

The Data-related coercions (3, 4, 5) and Option/Result coercions (1, 2) interact to create chains:
- T -> Data, then Data -> Option\<Data\>
- T -> Option\<T\>, then Option\<T\> -> Data

This chaining complicates interpreter cleanup efforts.
