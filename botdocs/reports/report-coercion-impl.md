# Coercion Implementation Report

## Runtime Value Coercions

Located in `crates/datalove-datafun-compiler/src/interp/coerce.rs`:

1. **T -> Option\<T\>** - Wraps value in `Some` (line 44)
2. **T -> Result\<T\>** - Wraps value in `Ok` (line 80)
4. **Data -> Option\<Data\>** - Wraps Data in `Some` (line 133)
5. **Data -> Result\<Data\>** - Wraps Data in `Ok` (line 164)

## Type-Level Coercions

Located in `crates/datalove-datalit/src/tycheck.rs`:

- **Numeric Widening** - Via `can_widen_to` (line 1439)

Named type coercions (anonymous struct/enum/tuple to named equivalents) were removed
along with the named type system itself.

## Infrastructure

- **`CoercionError`** enum in `datafun-compiler/src/tycheck.rs:2517`
  - `TypeMismatch`
  - `ArityMismatch`
- **`check_type_coercion`** - Centralized validation (line 2545)
- **`coerce_value_to_dest`** - Runtime coercion entry point (`interp/coerce.rs:14`)

## Complexity Notes

The Data-related coercions (3, 4, 5) and Option/Result coercions (1, 2) interact to create chains:
- T -> Data, then Data -> Option\<Data\>
- T -> Option\<T\>, then Option\<T\> -> Data

This chaining complicates interpreter cleanup efforts.
