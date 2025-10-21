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

Implemented (stubs in rt/lib.rs and rt/src/tensor.rs):

- `dtlv_rti_tensor_create_from_slice_local` - creates tensor from flat slice + shape (by-move) + layout
- `dtlv_rti_tensor_destroy_local` - frees all three allocations (shape, strides, data)

Planned:

- `tensor_slice(tensor: Tensor, ranges: *const SliceRange) -> Tensor`
- `tensor_transpose(tensor: Tensor, perm: *const u32) -> Tensor`
- `tensor_reshape(tensor: Tensor, new_shape: *const u32) -> Result<Tensor, Error>`
- `tensor_get(tensor: &Tensor, indices: *const u32) -> *const u8`
- `tensor_set(tensor: &mut Tensor, indices: *const u32, value: *const u8)`

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

**Runtime API Stubs** (crates/datalove-rt/src/lib.rs and src/tensor.rs):
- `dtlv_rti_tensor_create_from_slice_local` (lib.rs:1042)
- `dtlv_rti_tensor_destroy_local` (lib.rs:1079)
- Implementation stubs in src/tensor.rs (todo!)

### Next Steps

1. Implement stride computation helpers (row-major and column-major)
2. Implement `tensor_create_from_slice_impl`:
   - Extract shape from moved-in list argument
   - Allocate data buffer with capacity
   - Copy elements from slice
   - Allocate and populate shape array
   - Compute and allocate strides array
3. Implement `tensor_destroy_impl`:
   - Free shape array
   - Free strides array
   - Free data buffer
4. Add support in clone, destroy, eq, and cmp modules
5. Implement tensor operations (slice, transpose, reshape, get, set)
