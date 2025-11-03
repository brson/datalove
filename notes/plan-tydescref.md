# TyDescRef Migration Analysis

## Overview

This document tracks the migration from raw `*const TyDesc` pointers to safe `TyDescRef` wrappers throughout the codebase. The goal is to eliminate unsafe pointer dereferences and make TyDesc access safer.

## Completed Work

### Phase 1: API Foundation
- ✅ Added `TypeTable::get_expr_type_ref()` returning `Option<TyDescRef<'_>>`
- ✅ Added `TypeTable::get_datafun_expr_type_ref()` returning `Option<TyDescRef<'_>>`
- ✅ Added `TyDescTable::get_or_create_ref()` returning `TyDescRef<'_>`
- ✅ Added `TyDescTable::create_option_from_inner_tydesc_ref()`
- ✅ Added `TyDescTable::create_result_from_inner_tydesc_ref()`

### Phase 2: Test Code Migration
- ✅ Eliminated **6 unsafe blocks** from TypeTable tests
- All test assertions now use `.type_tag()` instead of `unsafe { (*tydesc).type_tag }`

### Phase 3: Interpreter Core
- ✅ Fixed instantiate2.rs main allocation (1 dereference)
- ✅ Fixed all **4 TyDesc dereferences** in interp.rs:
  - `(*condition_tydesc).type_tag` → `condition_tydesc_ref.type_tag()`
  - `(*condition_tydesc).type_info.option.inner_tydesc` → `condition_tydesc_ref.option_inner_ty().as_ptr()`
  - `(*condition_tydesc).type_info.result.ok_tydesc` → `condition_tydesc_ref.result_ok_ty().as_ptr()`
  - `(*tydesc).type_tag` → `tydesc_ref.type_tag()`

### Results
- **11+ unsafe blocks eliminated**
- **All 488+ tests passing**
- **Zero TyDesc dereferences** in interp.rs
- Demonstrated viability of TyDescRef approach

## Remaining Migration Opportunities

### Statistics
- **Total remaining tydesc dereferences**: 319 across 12 files
- **Field access patterns** (`(*tydesc).field`): 241 occurrences

### Top Files by Dereference Count
1. `instantiate2.rs`: 113 dereferences
2. `tydesc_table.rs`: 74 dereferences
3. `value.rs`: 56 dereferences
4. `eval_datafun.rs`: 22 dereferences
5. `layout.rs`: 20 dereferences

---

## High-Priority Targets (Recommended Next Steps)

### 1. eval_datalit.rs - EASIEST WIN ✓
**Impact**: 1 unsafe block eliminated
**Effort**: 5 minutes
**Priority**: HIGH

**Current Code**:
```rust
let type_tag = unsafe { (*inst.tydesc).type_tag };
```

**Proposed Fix**:
```rust
let tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(inst.tydesc) };
let type_tag = tydesc_ref.type_tag();
```

**Location**: `crates/datalove-datafun/src/interp_old/eval_datalit.rs:45`

---

### 2. InstantiatedValue Struct - BIGGEST WIN 🎯
**Impact**: ~80-100 unsafe blocks eliminated
**Effort**: 2-4 hours
**Priority**: HIGH

**Current Structure**:
```rust
pub struct InstantiatedValue {
    pub ptr: *const u8,
    pub tydesc: *const rtdt::TyDesc,
}
```

**Proposed Change**:
```rust
pub struct InstantiatedValue<'a> {
    pub ptr: *const u8,
    pub tydesc: rtdt::TyDescRef<'a>,
}
```

**Benefits**:
- 113 dereferences eliminated in instantiate2.rs
- InstantiatedValue is short-lived (created and consumed quickly)
- Lifetime naturally ties to TyDescTable which owns the TyDescs
- All field accesses become safe

**Affected Files**:
- `crates/datalove-datalit/src/instantiate2.rs` (primary usage)
- Any callers of `instantiate_value()` function

**Challenges**:
- Need to add lifetime parameter to struct
- Propagate lifetime through all users
- Moderate effort but high payoff

---

### 3. Layout Computation Functions - PURE WIN 🎯
**Impact**: 20 unsafe blocks eliminated
**Effort**: 1-2 hours
**Priority**: HIGH

**Current Signatures**:
```rust
pub fn compute_tuple_layout(tydesc: *const TyDesc) -> TupleLayout
pub fn compute_struct_layout(tydesc: *const TyDesc) -> StructLayout
pub fn compute_enum_layout(tydesc: *const TyDesc) -> EnumLayout
pub fn compute_option_layout(tydesc: *const TyDesc) -> OptionLayout
pub fn compute_result_layout(tydesc: *const TyDesc) -> ResultLayout
```

**Proposed Change**:
```rust
pub fn compute_tuple_layout(tydesc: TyDescRef) -> TupleLayout
pub fn compute_struct_layout(tydesc: TyDescRef) -> StructLayout
// ... etc
```

**Benefits**:
- Pure internal functions, no FFI concerns
- Clean API improvement
- All callers can easily convert at call site
- No structural changes needed

**Location**: `crates/datalove-rtdt/src/layout.rs`

---

## Medium-Priority Targets

### 4. eval_datafun.rs
**Impact**: ~10-15 unsafe blocks eliminated
**Effort**: 2-3 hours
**Priority**: MEDIUM-HIGH

**Dereferences**: 22 occurrences (mostly in clone operations)

**Patterns**:
- Accessing `type_info` unions (9 occurrences)
- Clone operations via `rt::c::dtlv_rti_any_clone_local`
- All in heap value cloning logic

**Migration Approach**:
TyDescRef already has methods like `list_element_ty()`, `map_key_ty()`, etc. Can use these instead of raw `.type_info` access.

---

### 5. tydesc_table.rs Tests
**Impact**: ~50-60 unsafe blocks eliminated
**Effort**: 1-2 hours
**Priority**: MEDIUM

**Dereferences**: 74 occurrences (mostly test assertions)

**Patterns**:
```rust
unsafe { assert_eq!((*tydesc).type_tag, TyTag::Bool); }
```

**Proposed Fix**:
```rust
let td = unsafe { TyDescRef::from_ptr(tydesc) };
assert_eq!(td.type_tag(), TyTag::Bool);
```

**Location**: `crates/datalove-datalit/src/tydesc_table.rs` (tests section)

---

### 6. Set Implementation Helpers
**Impact**: ~10 unsafe blocks eliminated
**Effort**: 1 hour
**Priority**: MEDIUM

**Dereferences**: 15 occurrences in `rt/src/impls/set.rs`

**Patterns**: Multiple helper functions accessing `(*key_tydesc).size` and `.align`

---

## Low-Priority / High-Complexity Targets

### 7. Value Enum - STRUCTURAL CHANGE ⚠️
**Impact**: ~20-25 unsafe blocks eliminated
**Effort**: 8+ hours (major refactoring)
**Priority**: LOW (requires design discussion)

**Current Structure**:
```rust
pub enum Value {
    Bool(bool),
    U32(u32),
    Int { ptr: *mut rtdt::Int, tydesc: *const rtdt::TyDesc },
    String { ptr: *mut rtdt::String, tydesc: *const rtdt::TyDesc },
    // ... 12 variants with tydesc fields
}
```

**Dereferences**: 56 occurrences
- 44 accessing `.size`/`.align` in allocation methods
- 12 accessing `.type_tag` for type checking

**Proposed Change**:
```rust
pub enum Value<'a> {
    Bool(bool),
    U32(u32),
    Int { ptr: *mut rtdt::Int, tydesc: rtdt::TyDescRef<'a> },
    String { ptr: *mut rtdt::String, tydesc: rtdt::TyDescRef<'a> },
    // ...
}
```

**Challenges**:
- Adding lifetime to Value propagates throughout entire interpreter
- InterpContext would need lifetime: `InterpContext<'db, 'tydesc>`
- All Value-using code needs lifetime annotations
- High complexity, high risk
- Value is fundamental to old interpreter design

**Recommendation**: **NOT RECOMMENDED** for old interpreter. Consider for new interpreter design instead.

---

## Cannot/Should Not Change

### C FFI Boundary
**Files**: `crates/datalove-rt/src/c.rs`

All FFI functions must take raw `*const rtdt::TyDesc` parameters. This is correct and cannot change.

**Examples**:
```rust
pub unsafe extern "C" fn dtlv_rti_clone_local(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc_in: *const rtdt::TyDesc,  // Must be raw pointer
    value_out: *mut u8,
) -> RtStatus
```

### TyInfo Structs
C FFI types must store raw pointers:
```rust
pub struct TyInfoTupleField {
    pub offset: u32,
    pub tydesc: *const TyDesc,  // Cannot change - C FFI type
}
```

### TypeTable Internal Storage
Already well-abstracted with safe accessor methods. Internal `Vec<*const rtdt::TyDesc>` storage is fine since it provides `get_expr_type_ref()` returning TyDescRef.

---

## Estimated Impact by Phase

### Phase 1 (Quick Wins) - 1-2 hours
**Targets**: eval_datalit.rs, layout functions
- **Dereferences eliminated**: ~21
- **Unsafe blocks eliminated**: ~21
- **Effort**: Low

### Phase 2 (High Impact) - 2-4 hours
**Targets**: InstantiatedValue struct
- **Dereferences eliminated**: ~113
- **Unsafe blocks eliminated**: ~80-100
- **Effort**: Medium

### Phase 3 (Medium Priority) - 4-6 hours
**Targets**: eval_datafun.rs, tydesc_table tests, set helpers
- **Dereferences eliminated**: ~111
- **Unsafe blocks eliminated**: ~75-90
- **Effort**: Medium

### Phase 4 (Future/Optional) - 8+ hours
**Targets**: Value enum refactoring
- **Dereferences eliminated**: ~56
- **Unsafe blocks eliminated**: ~20-25
- **Effort**: High (major design changes)

**Total Phases 1-3**: ~245 dereferences eliminated, ~176-211 unsafe blocks eliminated

---

## Recommended Action Plan

### Immediate Next Steps (Phases 1-2)
1. **Fix eval_datalit.rs** (5 min)
   - Single line change
   - Immediate win

2. **Migrate layout computation functions** (1-2 hours)
   - Change function signatures to take TyDescRef
   - Update all callers (use `.as_ptr()` if needed)
   - Pure internal API improvement

3. **Add lifetime to InstantiatedValue** (2-4 hours)
   - Change struct definition
   - Update instantiate_value() to return InstantiatedValue<'_>
   - Update all field accesses to use TyDescRef methods
   - Major impact, moderate effort

### Follow-up Work (Phase 3)
4. **Update eval_datafun.rs** (2-3 hours)
   - Convert clone operations to use TyDescRef
   - Use TyDescRef methods for type_info access

5. **Clean up tydesc_table tests** (1-2 hours)
   - Convert test assertions to use TyDescRef

6. **Update set implementation** (1 hour)
   - Convert helper functions

### Future Considerations (Phase 4)
7. **Value enum lifetime discussion**
   - Requires broader design conversation
   - Consider for new interpreter, not old one

---

## Design Principles

### When to Use TyDescRef
✓ **Use TyDescRef when**:
- Reading type descriptor fields (size, align, type_tag)
- Traversing type_info unions
- Internal computation functions
- Passing types between internal functions

✗ **Use raw pointers when**:
- FFI boundaries (required by C ABI)
- Long-term storage without clear lifetime (e.g., TypeTable internal storage)
- When adding lifetimes would cause cascading complexity

### Migration Pattern
```rust
// At API boundary: convert immediately
fn some_function(tydesc: *const TyDesc) {
    let td = unsafe { TyDescRef::from_ptr(tydesc) };
    // Use td.size(), td.align(), td.type_tag() - all safe!
}
```

---

## Success Metrics

### Current State (After Initial Migration)
- ✅ 11+ unsafe blocks eliminated
- ✅ 0 TyDesc dereferences in interp.rs core
- ✅ All tests passing
- ✅ Safe API methods available

### Target State (After Phases 1-3)
- 🎯 187-222 total unsafe blocks eliminated
- 🎯 ~256 dereferences eliminated (11 done + 245 remaining)
- 🎯 Major safety improvement in instantiation and layout code
- 🎯 Clean internal APIs using TyDescRef

---

## Notes

- TyDescRef already exists in `crates/datalove-rtdt/src/tydesc_ref.rs` and is well-designed
- All necessary TyDescRef methods already exist (list_element_ty, option_inner_ty, etc.)
- The migration is mostly mechanical once structural decisions are made
- Biggest payoff: InstantiatedValue (113 dereferences)
- Easiest win: eval_datalit.rs (1 dereference, 5 minutes)
- Most questionable: Value enum (requires major design changes)

---

## References

- TyDescRef implementation: `crates/datalove-rtdt/src/tydesc_ref.rs`
- TyDesc definition: `crates/datalove-rtdt/src/lib.rs`
- Example successful migration: `crates/datalove-datafun/src/interp_old/interp.rs`
