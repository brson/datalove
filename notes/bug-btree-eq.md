# B-tree Equality Bug

## Issue

B-tree equality comparison fails when maps or sets contain the same elements but were created with different insertion orders.

## Location

`crates/datalove-rt-tests/tests/eq_tests.rs`

Two tests are marked with `#[ignore]`:
- `test_eq_map_equals_different_literal_order` (line 1776)
- `test_eq_set_equals_different_literal_order` (line 2079)

## Examples

### Map Example

```rust
let map_a = @map{@10 = @100, @20 = @200, @30 = @300}
let map_b = @map{@30 = @300, @10 = @100, @20 = @200}
// Should be equal, but currently fails
```

### Set Example

```rust
let set_a = @set{@10, @20, @30}
let set_b = @set{@30, @10, @20}
// Should be equal, but currently fails
```

## Expected Behavior

Two maps/sets should be considered equal if they contain the same elements, regardless of:
- Insertion order
- Internal tree structure
- Node layout

## Current Behavior

The equality check fails because different insertion orders produce different internal B-tree structures, and the current implementation performs structural comparison rather than logical comparison.

## Root Cause

The equality implementation likely compares B-tree nodes directly rather than comparing the actual key-value pairs or elements. When items are inserted in different orders, the tree can have:
- Different node splits
- Different tree shapes
- Different internal layouts

Even though the logical contents are identical, the structural representation differs.

## Solution

Implement proper iterator-based equality that:
1. Iterates through both trees in sorted order
2. Compares elements/pairs logically
3. Ignores internal tree structure

This is the standard approach for comparing ordered data structures.

## Status

Known limitation documented with TODO comments.
Tests remain in the codebase to document expected behavior for future implementation.

## Related Code

- B-tree map implementation: `crates/datalove-rt/src/impls/btreemap.rs`
- B-tree set implementation: `crates/datalove-rt/src/impls/set.rs`
- Equality function: `crates/datalove-rt/src/c.rs` (`dtlv_rti_eq`)
