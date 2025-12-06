# datalove-rt C ABI Cleanup

## Expected Conventions (from lib.rs:11-33)

1. All functions (except `init`) take a runtime handle
2. All value pointer args followed by their tydesc
3. Naming suffixes specify ownership/access semantics:
   - `_in` - `*mut` move in (callee owns)
   - `_out` - `*mut` move out (caller owns)
   - `_ref` - `*const` shared reference
   - `_mut` - `*mut` unique reference

## Issues Found

### A. Missing tydesc Pairing (violates convention #2)

| Function | Parameter | Issue |
|----------|-----------|-------|
| `dtlv_rti_clone_local` | `value_out` | No paired tydesc_out |
| `dtlv_rti_tensor_get_local` | `element_value_out` | No tydesc |
| `dtlv_rti_tensor_set_local` | `value_ptr` | No tydesc |
| `dtlv_rti_tensor_transpose_local` | shares tydesc | Input/output share `tensor_tydesc` |

### B. Missing Semantic Suffixes (violates convention #3)

Comparison functions use bare names instead of `_ref`:
- `dtlv_rti_eq`: `value_a`, `value_b` should be `value_a_ref`, `value_b_ref`
- `dtlv_rti_eq_unique`: same
- `dtlv_rti_cmp`: same
- `dtlv_rti_cmp_total`: same
- `dtlv_rti_int_cmp`: `a_ref`, `b_ref` - actually correct!

### C. Inconsistent `_local` Suffix

Functions without `_local`:
- `dtlv_rti_eq`, `dtlv_rti_eq_unique`, `dtlv_rti_cmp`, `dtlv_rti_cmp_total`
- `dtlv_rti_int_cmp`
- `dtlv_rti_list_get`
- `dtlv_rti_btreemap_get` (but there's also `_get_local` version!)

### D. Duplicate/Confusing Function Pairs

- `dtlv_rti_btreemap_get` vs `dtlv_rti_btreemap_get_local`
  - Nearly identical, both call `btreemap_get_impl`
  - `get` uses `btreemap_value_mut` (*mut), `get_local` uses `btreemap_value_ref` (*const)
  - Both are read operations - should use `_ref`

### E. Parameter Naming Inconsistencies

1. **tydesc naming varies**:
   - Generic: `tydesc`
   - Prefixed: `btreemap_tydesc`, `key_tydesc`, `element_tydesc`
   - Suffixed: `tydesc_in`, `tydesc_a`
   - Recommendation: Always use prefix matching the value param

2. **Slice parameters**:
   - `slice_ptr_ref` + `slice_ptr_len` - `ptr` in name is redundant
   - Should be `slice_ref` + `slice_len`

3. **Value naming**:
   - `value_ptr` (tensor_set) - should be `element_ref` or `element_in`
   - `perm_ptr`, `indices_ptr` - raw pointers without semantic suffix

### F. Unused tydesc Parameters

Several bigint functions accept tydescs but don't use them:
- `dtlv_rti_int_add`: `a_tydesc`, `b_tydesc`, `result_tydesc` unused
- Same for `int_sub`, `int_mul`, `int_neg`, `int_div_checked`

Per convention, these should still be passed for uniformity, but worth documenting.

### G. Return Type Inconsistency

- Most comparison functions return `RtEq` or `RtOrdering`
- `dtlv_rti_int_cmp` returns raw `i32`

### H. Parameter Order Inconsistencies

Different patterns for "create from slice" operations:
- `list_create_from_slice`: slice params first, then list output
- `tensor_create_from_slice`: slice, then shape, then tensor output
- `btreemap_clone_from_slice`: map output first, then slice

## Implementation Plan

### Convention: `_local` suffix
- ALL functions get `_local` suffix for consistency
- Future: `_global` variants for global heap
- Add debug assertions to verify pointers come from local heap

---

### 0. Add `_local` to All Functions

Rename functions currently missing `_local`:
- `dtlv_rti_eq` -> `dtlv_rti_eq_local`
- `dtlv_rti_eq_unique` -> `dtlv_rti_eq_unique_local`
- `dtlv_rti_cmp` -> `dtlv_rti_cmp_local`
- `dtlv_rti_cmp_total` -> `dtlv_rti_cmp_total_local`
- `dtlv_rti_int_cmp` -> `dtlv_rti_int_cmp_local`
- `dtlv_rti_list_get` -> `dtlv_rti_list_get_local`

Add heap assertion to `AllocLocal` in `alloc.rs`:
```rust
impl AllocLocal {
    /// Check if a pointer was allocated by this allocator.
    #[cfg(debug_assertions)]
    pub fn contains_ptr(&self, ptr: *const u8) -> bool {
        self.active_allocations.contains_key(&(ptr as *mut u8))
    }
}
```

Add assertion helper to `c.rs`:
```rust
#[cfg(debug_assertions)]
unsafe fn debug_assert_local_heap(rt: LocalRtHandle, ptr: *const u8) {
    if !ptr.is_null() {
        let rt_ref = &*(rt as *mut rt_local::RtLocal);
        debug_assert!(
            rt_ref.alloc.contains_ptr(ptr),
            "pointer {:p} not from local heap",
            ptr
        );
    }
}
```

Call in each `_local` function before using value pointers.

---

### 1. Fix Missing tydesc Pairing

**c.rs changes:**

`dtlv_rti_clone_local` (line 128):
- Add `tydesc_out: *const rtdt::TyDesc` after `value_out`
- (Strict consistency: every value pointer gets its own tydesc)

`dtlv_rti_tensor_get_local` (line 1216):
- Add `element_tydesc: *const rtdt::TyDesc` after `element_value_out`

`dtlv_rti_tensor_set_local` (line 1242):
- Rename `value_ptr` -> `element_ref`
- Add `element_tydesc: *const rtdt::TyDesc` after it

`dtlv_rti_tensor_transpose_local` (line 1268):
- Rename `tensor_value_in` -> `tensor_ref` (it's read-only source)
- Rename `tensor_tydesc` -> `tensor_tydesc_ref`
- Add `tensor_tydesc_out: *const rtdt::TyDesc` for `tensor_value_out`

---

### 2. Fix Semantic Suffix Naming

**Comparison functions - add `_ref` suffix:**

`dtlv_rti_eq` (line 143):
- `value_a` -> `value_a_ref`
- `value_b` -> `value_b_ref`
- `tydesc_a` -> `tydesc_a_ref` (or just match the value name prefix)
- `tydesc_b` -> `tydesc_b_ref`

Same pattern for:
- `dtlv_rti_eq_unique` (line 164)
- `dtlv_rti_cmp` (line 181)
- `dtlv_rti_cmp_total` (line 199)

---

### 3. Remove Duplicate Function

`dtlv_rti_btreemap_get` vs `dtlv_rti_btreemap_get_local`:
- Keep only `dtlv_rti_btreemap_get_local`
- Delete `dtlv_rti_btreemap_get` (lines 558-590)
- Fix `btreemap_value_mut` -> `btreemap_value_ref` (it's read-only)

---

### 4. Standardize Parameter Naming

**Slice parameters:**
- `slice_ptr_ref` -> `slice_ref` (ptr is redundant)
- Keep `slice_len` or rename to `slice_ptr_len` -> `slice_len`

**Tensor raw pointers:**
- `indices_ptr` - acceptable (array of indices, not a value type)
- `perm_ptr` - acceptable (same reason)

**tydesc naming rule:**
- tydesc name = value param name with `_tydesc` instead of semantic suffix
- Example: `value_a_ref` pairs with `value_a_tydesc`

---

### 5. Documentation Updates

Add to c.rs module doc:

```rust
//! ## Naming Conventions
//!
//! Function suffixes:
//! - `_local` - operates on local heap (all current functions)
//! - (future) `_global` - operates on global heap
//!
//! Parameter suffixes (ownership semantics):
//! - `_in` - move in, callee owns, `*mut`
//! - `_out` - move out, caller owns, `*mut`
//! - `_ref` - shared borrow, `*const`
//! - `_mut` - unique borrow, `*mut`
//!
//! Every value pointer is followed by its tydesc.
//!
//! ## Debug Assertions
//!
//! In debug builds, functions assert that pointers belong to the local heap.
//!
//! ## Error Handling
//!
//! Functions return `RtStatus::Error` on null required params.
//! Argument pointers should never be null in correct generated code.
```

---

## Files to Modify

**Core API:**
- `crates/datalove-rt/src/c.rs` - all function signature changes + docs
- `crates/datalove-rt/src/lib.rs` - update module docs if needed
- `crates/datalove-rt/src/impls/alloc.rs` - add `contains_ptr` for heap assertions

**Callers (need param updates):**
- `crates/datalove-datafun/src/interp/mod.rs` - interpreter calls C API
- `crates/datalove-datafun/src/interp_old/interp.rs`
- `crates/datalove-datafun/src/interp_old/eval_datafun.rs`

**Tests:**
- `crates/datalove-rt-tests/tests/clone_tests.rs`
- `crates/datalove-rt-tests/tests/tensor_tests.rs`
- `crates/datalove-rt-tests/tests/btreemap_tests.rs`
- `crates/datalove-rt-tests/tests/btreemap_proptests.rs`
- `crates/datalove-rt-tests/tests/eq_tests.rs`
- `crates/datalove-rt-tests/tests/destroy_tests.rs`
