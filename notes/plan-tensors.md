# Tensor Runtime Representation

## Overview

Design for heap-allocated strided multidimensional arrays (tensors) based on Julia's array design.
Rank is compile-time, shape/layout/stride are runtime. Single type represents both owned tensors and views.

## Key Design Decisions

### Linear Type System Integration

Datalove has a linear type system where all values are owned.
Creating a view transforms a tensor by value:
- Pass tensor to std function
- Function modifies internal pointers/offsets
- Returns transformed tensor by value

No ownership tracking needed - linear types ensure exactly one owner of the base buffer.

### Pointer Design: Base + Offset

Store pointer to base of buffer allocation plus offset to view's first element.
This differs from storing direct pointer to first element.

**Rationale:**
- Required for linear type system: when transforming owned tensor to view, need to preserve base pointer
- Enables proper deallocation: always free from base pointer
- Simpler reasoning: offset makes view relationship explicit

### Storage Choices

- **Shape/stride arrays**: Heap-allocated (consistent, simple)
- **Element type**: Not stored (passed separately by compiler via TyDesc as with all Datalove types)
- **Ownership**: Not tracked (linear types guarantee single owner)

## Core Structure

**Status**: Implemented in crates/datalove-rtdt/src/lib.rs:236-243

```rust
#[repr(C)]
pub struct Tensor {
    pub ptr_base: *mut u8,       // Pointer to base of data buffer allocation
    pub offset_elems: u32,        // Offset in elements from base to first element of view
    pub capacity_elems: u32,      // Total capacity of base buffer in elements
    pub shape: *const u32,        // Heap-allocated array of dimension sizes (length = rank)
    pub strides: *const u32,      // Heap-allocated array of strides in elements (length = rank)
    pub layout: TensorLayout,     // Layout tag
}
```

**Field Details:**

- `ptr_base`: Base allocation pointer. Always points to start of buffer, even for views. Used for deallocation.
- `offset_elems`: Element offset from base to this view's first element. Zero for full tensors.
- `capacity_elems`: Total capacity of base buffer in elements. Follows List/String pattern.
- `shape`: Points to heap array of N dimension sizes (N = rank, known at compile time).
- `strides`: Points to heap array of N stride values in elements.
- `layout`: Enum indicating memory layout convention.

## Layout Options

**Status**: Implemented in crates/datalove-rtdt/src/lib.rs:246-253

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TensorLayout {
    RowMajor = 1,           // C-style: last dimension varies fastest, strides = [D₁×...×Dₙ, D₂×...×Dₙ, ..., Dₙ, 1]
    ColMajor = 2,           // Fortran/Julia-style: first dimension varies fastest, strides = [1, D₁, D₁×D₂, ..., D₁×...×Dₙ₋₁]
    RowMajorTransposed = 3, // Row-major with transposed dimension ordering
    ColMajorTransposed = 4, // Column-major with transposed dimension ordering
}
```

**Layout Examples** for shape [3, 4]:

- **RowMajor**: strides = [4, 1], linear order = row₀ | row₁ | row₂
- **ColMajor**: strides = [1, 3], linear order = col₀ | col₁ | col₂ | col₃
- **RowMajorTransposed**: strides = [1, 3], represents transpose view of row-major data
- **ColMajorTransposed**: strides = [4, 1], represents transpose view of column-major data

## Type System Integration

**Status**: Implemented in crates/datalove-rtdt/src/lib.rs

### TyDesc Extension

Tensor variants added to type descriptor:

- `TyTag::Tensor = 0x54` (line 379)
- `TyInfo::tensor` union variant (line 398)
- `TyInfoTensor` struct (lines 474-477)

```rust
#[repr(u8)]
pub enum TyTag {
    // ... existing tags ...
    Tensor = 0x54,  // In 0x50 range with collections
}

#[repr(C)]
pub union TyInfo {
    // ... existing variants ...
    pub tensor: TyInfoTensor,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TyInfoTensor {
    pub element_tydesc: *const TyDesc,
    pub rank: u32,
}
```

Compile-time type `Tensor<T, N>` maps to:
- Runtime struct: `Tensor` (same for all types/ranks)
- Type descriptor: `TyDesc` with `type_tag = TyTag::Tensor`, `type_info.tensor.element_tydesc = &TYDESC_T`, `type_info.tensor.rank = N`

## Memory Layout Examples

### Example 1: Owned 2D Tensor

Tensor with shape [10, 20], element type u32, row-major layout:

```
Tensor struct:
  ptr_base:      → [800 bytes: 10×20×4 bytes]
  offset_elems:  0
  shape:         → [10, 20]
  strides:       → [20, 1]
  layout:        RowMajor

Heap allocations:
  1. Data buffer: 800 bytes (10×20×4)
  2. Shape array: 8 bytes (2×4)
  3. Strides array: 8 bytes (2×4)
```

### Example 2: View Transformation

Starting with 10×20 tensor, create view of subregion [5:10, 10:15] (5×5 submatrix):

**Before (full tensor):**
```
Tensor {
  ptr_base:      → [800 bytes buffer]
  offset_elems:  0
  shape:         → [10, 20]
  strides:       → [20, 1]
  layout:        RowMajor
}
```

**After (view transformation):**
```
Tensor {
  ptr_base:      → [same 800 bytes buffer]  // Base pointer preserved
  offset_elems:  110                         // = 5×20 + 10 (element offset to [5,10])
  shape:         → [5, 5]                    // Same heap allocation
  strides:       → [20, 1]                   // Same heap allocation, strides unchanged
  layout:        RowMajor
}
```

Linear type system ensures original tensor is consumed, so base buffer has single owner.

### Example 3: Transpose View

Transform 10×20 row-major tensor to transposed view (20×10):

**Before:**
```
Tensor {
  ptr_base:      → [800 bytes buffer]
  offset_elems:  0
  shape:         → [10, 20]
  strides:       → [20, 1]
  layout:        RowMajor
}
```

**After:**
```
Tensor {
  ptr_base:      → [same buffer]
  offset_elems:  0
  shape:         → [20, 10]        // Dimensions swapped
  strides:       → [1, 20]         // Strides swapped
  layout:        ColMajorTransposed
}
```

Zero-copy transpose: just swap shape/strides and update layout tag.

## Comparison with Julia

### Julia 1.11+ Design

```c
// Simplified Julia 1.11+ structure
struct jl_array_t {
    jl_genericmemoryref_t ref;  // Contains ptr_or_offset and mem pointer
    size_t dimsize[];           // Inline dimensions
}

struct jl_genericmemoryref_t {
    void *ptr_or_offset;
    jl_genericmemory_t *mem;
}

struct jl_genericmemory_t {
    size_t length;
    void *ptr;
}
```

Julia's `ptr_or_offset` can be:
- Direct pointer to first element (for non-offset views)
- Offset value (for offset views)

Requires checking which interpretation is valid.

### Our Design Differences

1. **Always base + offset**: We always store base pointer + element offset (simpler, consistent)
2. **Separate strides**: Explicit stride array (Julia computes from layout assumptions)
3. **Heap shape/strides**: Julia inlines small dimension arrays, we always heap-allocate
4. **Layout tags**: Explicit layout enum (Julia uses flags and conventions)

**Advantages:**
- Simpler reasoning: base and offset always have same meaning
- More flexible: arbitrary stride patterns supported
- Consistent allocation: no special cases for rank

**Tradeoffs:**
- Extra allocations for shape/stride (but simplifies implementation)
- Explicit layout tags may be redundant with stride patterns (can optimize later)

## Future Work

### Layout Computation Functions

Need functions to compute strides from shape and layout:

```rust
pub fn compute_row_major_strides(shape: &[u32]) -> Vec<u32>;
pub fn compute_col_major_strides(shape: &[u32]) -> Vec<u32>;
```

Row-major: `strides[i] = product(shape[i+1..])`
Col-major: `strides[i] = product(shape[..i])`

### Tensor Operations

Standard library functions needed:

Implemented:

- `dtlv_rti_tensor_create_from_slice_local` - creates tensor from flat slice + shape (by-move) + layout
- `dtlv_rti_tensor_destroy_local` - frees all three allocations (shape, strides, data)
- `dtlv_rti_tensor_get_local` - clones element at specified indices (caller must destroy)
- `dtlv_rti_tensor_set_local` - sets element at specified indices (destroys old, clones new)
- `dtlv_rti_tensor_transpose_local` - creates zero-copy transposed view with permuted dimensions

Planned:

- `tensor_slice(tensor: Tensor, ranges: *const SliceRange) -> Tensor`
- `tensor_reshape(tensor: Tensor, new_shape: *const u32) -> Result<Tensor, Error>`

### Layout Information

**Status**: Implemented in crates/datalove-rtdt/src/lib.rs:256-259

Computed layout struct added:

```rust
pub struct TensorLayoutInfo {
    pub size: u32,               // sizeof(Tensor) struct
    pub align: u32,              // alignment
}
```

Note: Unlike the plan, data_offset and total_elements are not included.
The Tensor struct is fixed-size so data_offset is not needed.
Total elements can be computed from shape when needed.

### Memory Management

Deallocation must free three allocations:
1. Shape array
2. Stride array
3. Data buffer (using `ptr_base`)

Drop/destroy function needs rank parameter to know shape/stride array sizes.

### Advanced Features

Consider for future:

- **Slice metadata caching**: Cache commonly used slice calculations
- **Inline small tensors**: Optimize rank-1 or small tensors with inline storage
- **Broadcasting**: Operations on tensors with compatible shapes
- **SIMD operations**: Vectorized element-wise operations for contiguous layouts
- **GPU interop**: Pointer exchange with GPU tensor libraries

## Implementation Status Summary

### Completed

**Data Structures** (crates/datalove-rtdt/src/lib.rs):
- `Tensor` struct (lines 236-243) - includes capacity_elems field
- `TensorLayout` enum (lines 246-253)
- `TensorLayoutInfo` struct (lines 256-259)
- `TyTag::Tensor` variant (line 379)
- `TyInfo::tensor` variant (line 398)
- `TyInfoTensor` struct (lines 474-477)

**Runtime Functions** (crates/datalove-rt/src/tensor.rs):
- Stride computation helpers (lines 14-45):
  - `compute_row_major_strides` - computes strides for row-major layout
  - `compute_col_major_strides` - computes strides for column-major layout
- `tensor_create_from_slice_impl` (lines 51-203) - creates tensor from slice + shape + layout
- `tensor_get_impl` (lines 206-267) - clones element at indices to output buffer
- `tensor_set_impl` (lines 269-330) - sets element at indices (destroy + clone)
- `tensor_transpose_impl` (lines 332-459) - zero-copy transpose with dimension permutation
- `tensor_destroy_impl` (lines 464-549) - frees all allocations

**FFI Exports** (crates/datalove-rt/src/lib.rs):
- `dtlv_rti_tensor_create_from_slice_local` (lines 1042-1076)
- `dtlv_rti_tensor_destroy_local` (lines 1078-1093)
- `dtlv_rti_tensor_get_local` (lines 1096-1119) - takes element_value_out buffer for cloned element
- `dtlv_rti_tensor_set_local` (lines 1121-1145)
- `dtlv_rti_tensor_transpose_local` (lines 1147-1171)

**Tests** (crates/datalove-rt-tests/tests/tensor_tests.rs):
- 28 comprehensive tests covering:
  - Tensor creation (1D, 2D, 3D with row-major and column-major layouts)
  - Tensor destruction
  - Element access (get/set for 1D, 2D, 3D tensors)
  - Clone and destroy pattern for tensor_get (test_tensor_get_clones_and_caller_destroys)
  - Transpose (2D row/col-major, 3D, identity, invalid permutations)
  - Edge cases (null pointers, mismatched sizes, out-of-bounds access)

### Type-Generic Runtime Operations

**Status**: Completed

Tensor support added to all type-generic runtime modules:

- **destroy.rs** (lines 101-158): Already implemented - frees data buffer, shape array, and strides array
- **clone.rs** (lines 284-364): Deep clone implementation - allocates new buffers, recursively clones elements
- **cmp.rs** (eq_tydesc, lines 236-243): Type descriptor equality - compares element type and rank
- **cmp.rs** (eq_value, lines 536-620): Value equality following Julia semantics - compares shapes and visible elements
- **cmp.rs** (cmp_value, lines 1065-1160): Lexicographic ordering - compares shapes then elements

**Equality Semantics** (following Julia):
- Tensors with different shapes are not equal
- Only visible elements are compared (layout/strides ignored)
- Element-wise comparison using strides to compute offsets

**Ordering Semantics**:
- Shapes compared lexicographically dimension-by-dimension
- If shapes equal, elements compared lexicographically in iteration order

### Next Steps

Implement remaining tensor operations:
- `tensor_slice` - create view of subregion
- `tensor_reshape` - change shape (if compatible)

## Slice and Reshape Implementation Plan

### Overview

Both `slice` and `reshape` transform a tensor by value (move semantics).
They return `Result<Tensor, Error>` where:
- **Ok**: Contains the transformed tensor
- **Err**: Contains packed original unchanged tensor for reclamation

This allows the caller to recover the original tensor on failure, maintaining linear type system invariants.

### Result Type Structure

Result type layout (from rtdt/src/lib.rs:286-303):
```rust
#[repr(C)]
pub struct Result {
    pub tag: ResultTag,  // 1 byte: Ok=1, Err=2
    // Padding, then payload at computed offset
}

pub enum ResultTag {
    Ok = 1,
    Err = 2,
}
```

Layout computation (rtdt/layout module):
- Tag size: 1 byte
- Payload offset: align_up(1, max(align(T), align(Error)))
- Total size: tag_size + padding + max(size(T), size(Error))

Error type (rtdt/src/lib.rs:335-338):
```rust
pub struct Error {
    primary: u64,
    secondary: u64,
}
```

Error uses anypack encoding (same as Data):
- Can pack (tydesc, value_ptr) for heap values
- Can inline small values
- See anypack.rs for construction methods

### Tensor Slice

**Signature:**
```rust
pub unsafe fn tensor_slice_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    ranges_ptr: *const SliceRange,
    result_value_out: *mut u8,
    result_tydesc_ref: TyDescRef,
) -> RtStatus
```

**Input:**
- `tensor_value_in`: Moved-in tensor to slice
- `ranges_ptr`: Array of rank SliceRange structs (one per dimension)
- `result_value_out`: Output buffer for Result<Tensor, Error>

**SliceRange Structure:**
```rust
#[repr(C)]
pub struct SliceRange {
    pub start: u32,  // Inclusive start index
    pub end: u32,    // Exclusive end index
}
```

**Algorithm:**

1. **Validation Phase:**
   - Check null pointers
   - Validate rank > 0
   - Extract tensor fields: ptr_base, offset_elems, capacity_elems, shape, strides, layout
   - For each dimension i:
     - Validate: 0 <= ranges[i].start < ranges[i].end <= shape[i]
   - If any validation fails: goto error path

2. **Error Path (validation failed):**
   - Pack original tensor into Error value
   - Construct Error with tensor's tydesc and pointer to original tensor value
   - Write Error to result payload at computed offset
   - Set result.tag = ResultTag::Err
   - Return RtStatus::Ok (function succeeded, result contains error)

3. **Success Path (validation passed):**
   - Allocate new shape array (rank * sizeof(u32))
   - Allocate new strides array (rank * sizeof(u32))
   - If allocation fails: goto error path
   - Compute new offset: offset_elems += sum(ranges[i].start * strides[i])
   - Copy strides (unchanged from input)
   - Compute new shape: new_shape[i] = ranges[i].end - ranges[i].start
   - Free input tensor's shape/strides arrays
   - Construct output tensor with new shape/strides and updated offset
   - Write tensor to result payload at computed offset
   - Set result.tag = ResultTag::Ok
   - Return RtStatus::Ok

**Key Points:**
- Slicing creates a view: shares same data buffer (ptr_base unchanged)
- Only offset changes (moves first element pointer)
- Shape becomes smaller (subregion dimensions)
- Strides unchanged (same memory layout)
- Input tensor's shape/strides freed after successful transformation
- Input tensor's data buffer ownership transferred to output tensor
- On error, entire input tensor structure preserved in Error

**Example:**
Slice [10, 20] tensor with ranges [[2:7], [5:15]]:
- Input: offset=0, shape=[10,20], strides=[20,1]
- New offset: 0 + (2*20 + 5*1) = 45 elements
- New shape: [5, 10] (7-2=5 rows, 15-5=10 cols)
- Strides: [20, 1] (unchanged)
- Result: view of 5x10 subregion starting at original[2,5]

### Tensor Reshape

**Signature:**
```rust
pub unsafe fn tensor_reshape_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    new_shape_in: *mut u8,  // List<u32>, moved in
    new_shape_tydesc_ref: TyDescRef,
    result_value_out: *mut u8,
    result_tydesc_ref: TyDescRef,
) -> RtStatus
```

**Input:**
- `tensor_value_in`: Moved-in tensor to reshape
- `new_shape_in`: Moved-in List<u32> with new dimensions
- `result_value_out`: Output buffer for Result<Tensor, Error>

**Algorithm:**

1. **Validation Phase:**
   - Check null pointers
   - Extract new_shape from List (shape data ptr, new rank)
   - Validate new_rank > 0
   - Compute total elements from new_shape
   - Check tensor is contiguous (required for reshape):
     - Verify strides match either row-major or col-major pattern
     - Row-major: strides[i] == product(shape[i+1..])
     - Col-major: strides[i] == product(shape[..i])
   - Compute current total elements from current shape
   - Validate: new_total_elems == current_total_elems
   - Validate: offset_elems == 0 (can only reshape full tensor, not view)
   - If any validation fails: goto error path

2. **Error Path (validation failed):**
   - Destroy new_shape List (not needed)
   - Need to pack BOTH tensor and new_shape into Error
   - Problem: Error holds single value, but we have two to return
   - **Solution:** Pack tensor only (new_shape already destroyed)
   - Pack original tensor into Error value
   - Write Error to result payload
   - Set result.tag = ResultTag::Err
   - Return RtStatus::Ok

3. **Success Path (validation passed):**
   - Extract layout from tensor (preserve row/col-major)
   - Compute new strides based on layout:
     - Row-major: strides[i] = product(new_shape[i+1..])
     - Col-major: strides[i] = product(new_shape[..i])
   - Allocate new strides array
   - If allocation fails: goto error path (must destroy new_shape List)
   - Steal shape array from new_shape List (take ownership of its data pointer)
   - Free new_shape List struct (but not its data - we're using it)
   - Free old strides array from input tensor
   - Construct output tensor with new shape/strides arrays
   - Copy other fields: ptr_base, offset_elems (must be 0), capacity_elems, layout
   - Write tensor to result payload
   - Set result.tag = ResultTag::Ok
   - Return RtStatus::Ok

**Key Points:**
- Reshape changes shape but not total elements
- Requires contiguous tensor (view creation destroyed contiguity)
- Requires offset_elems == 0 (only full tensor, not subview)
- Strides recomputed based on new shape and layout
- Data buffer unchanged (zero-copy operation)
- Input tensor's shape freed, new_shape's data array reused
- Input tensor's strides freed, new strides allocated
- On error, original tensor returned in Error, new_shape destroyed

**Contiguity Check:**
```rust
fn is_contiguous(shape: &[u32], strides: &[u32]) -> bool {
    is_row_major_contiguous(shape, strides) || is_col_major_contiguous(shape, strides)
}

fn is_row_major_contiguous(shape: &[u32], strides: &[u32]) -> bool {
    for i in 0..shape.len() {
        let expected = shape[i+1..].iter().product::<u32>();
        if strides[i] != expected {
            return false;
        }
    }
    true
}

fn is_col_major_contiguous(shape: &[u32], strides: &[u32]) -> bool {
    for i in 0..shape.len() {
        let expected = shape[..i].iter().product::<u32>();
        if strides[i] != expected {
            return false;
        }
    }
    true
}
```

**Example:**
Reshape [6, 10] row-major tensor to [3, 4, 5]:
- Input: shape=[6,10], strides=[10,1], 60 elements, offset=0, row-major
- Validation: 60 == 3*4*5 ✓, offset==0 ✓, contiguous ✓
- New strides (row-major): [4*5=20, 5, 1]
- Result: shape=[3,4,5], strides=[20,5,1], same data buffer

**Error Cases:**
- Non-contiguous tensor: created by slice/transpose
- View tensor: offset_elems != 0
- Size mismatch: new_shape elements != current elements
- Invalid new_shape: rank=0 or contains zeros

### FFI Exports

**Slice:**
```rust
#[no_mangle]
pub unsafe extern "C" fn dtlv_rti_tensor_slice_local(
    rt_handle: RtHandle,
    tensor_value_in: *mut u8,
    tensor_tydesc: TyDescHandle,
    ranges_ptr: *const u8,
    result_value_out: *mut u8,
    result_tydesc: TyDescHandle,
) -> RtStatus
```

**Reshape:**
```rust
#[no_mangle]
pub unsafe extern "C" fn dtlv_rti_tensor_reshape_local(
    rt_handle: RtHandle,
    tensor_value_in: *mut u8,
    tensor_tydesc: TyDescHandle,
    new_shape_in: *mut u8,
    new_shape_tydesc: TyDescHandle,
    result_value_out: *mut u8,
    result_tydesc: TyDescHandle,
) -> RtStatus
```

### Testing Strategy

**Slice Tests:**
1. Valid 2D slice (middle subregion)
2. Valid 3D slice (various ranges)
3. Edge case: slice to single element
4. Edge case: slice full range (identity)
5. Error: out of bounds (start >= end)
6. Error: invalid range (start > dimension)
7. Error: end > dimension size
8. Verify: result is Ok with correct tensor
9. Verify: error result contains original tensor
10. Verify: sliced tensor can be destroyed
11. Verify: error tensor can be extracted and destroyed

**Reshape Tests:**
1. Valid reshape: 2D to 3D (e.g., [6,10] -> [3,4,5])
2. Valid reshape: 3D to 2D (e.g., [2,3,4] -> [6,4])
3. Valid reshape: same rank, different shape ([12] -> [3,4])
4. Error: size mismatch ([6,10] -> [7,9])
5. Error: non-contiguous tensor (after slice)
6. Error: view tensor (offset != 0)
7. Edge case: reshape to 1D (flatten)
8. Edge case: reshape from 1D to nD
9. Verify: layout preserved (row-major stays row-major)
10. Verify: data buffer unchanged (pointer equality)
11. Verify: error result contains original tensor

### Implementation Order

1. Define SliceRange struct in rtdt/src/lib.rs
2. Implement contiguity check helpers in tensor.rs
3. Implement tensor_slice_impl in tensor.rs
4. Implement tensor_reshape_impl in tensor.rs
5. Add FFI exports in rt/src/lib.rs
6. Write comprehensive tests in rt-tests/tests/tensor_tests.rs
7. Update plan document with "Implemented" status
