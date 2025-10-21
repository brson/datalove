# List Runtime API Sketch

This document outlines the runtime API for List operations, modeled after the BTreeMap API.

## Data Structure

```rust
#[repr(C)]
pub struct List {
    pub data: *const u8,    // Type-aligned pointer to element buffer.
    pub size: u32,          // Number of elements currently stored.
    pub capacity: u32,      // Number of elements allocated.
}
```

## Core Operations

### Creation and Destruction

- `list_create_impl` - Create an empty list.
- `list_destroy_impl` - Destroy list and free all elements and buffer.
- `list_clear_impl` - Clear all elements (keeps buffer, resets to empty).

### Element Access

- `list_get_impl` - Get element at index, returns `Option<T>`.
  - Clones the element if found.
  - Returns None if index out of bounds.

- `list_set_impl` - Set element at index (replaces existing).
  - Moves the new element in.
  - Destroys the old element.
  - Returns Error if index out of bounds.

### Stack Operations

- `list_push_impl` - Push element to end.
  - Moves the element into the list.
  - Grows buffer if needed.

- `list_pop_impl` - Pop element from end, returns `Option<T>`.
  - Moves the element out.
  - Returns None if list is empty.

### Insert and Remove

- `list_insert_impl` - Insert at index, shifting elements right.
  - Accepts `index == len` (equivalent to push).
  - Returns Error if `index > len`.
  - Moves the element in.

- `list_remove_impl` - Remove at index, shifting elements left, returns `Option<T>`.
  - Moves the element out.
  - Returns None if index out of bounds.

### Capacity Management

- `list_reserve_impl` - Reserve capacity for at least `additional` more elements.
  - Does nothing if capacity is already sufficient.
  - Grows using Vec-like strategy (double or required, whichever is larger).

- `list_shrink_to_fit_impl` - Shrink capacity to fit current size.

### Bulk Operations

- `list_create_from_slice_impl` - Create list from slice of elements.
  - Clones all elements from the slice.

- `list_extend_from_slice_impl` - Append slice of elements to list.
  - Clones all elements from the slice.

## FFI Entry Points

All FFI functions follow the naming convention `dtlv_rti_list_*_local`:

1. `dtlv_rti_list_create_local`
2. `dtlv_rti_list_destroy_local`
3. `dtlv_rti_list_clear_local`
4. `dtlv_rti_list_get`
5. `dtlv_rti_list_set_local`
6. `dtlv_rti_list_push_local`
7. `dtlv_rti_list_pop_local`
8. `dtlv_rti_list_insert_local`
9. `dtlv_rti_list_remove_local`
10. `dtlv_rti_list_reserve_local`
11. `dtlv_rti_list_shrink_to_fit_local`
12. `dtlv_rti_list_create_from_slice_local`
13. `dtlv_rti_list_extend_from_slice_local`

## Implementation Notes

### Growth Strategy

Following Rust's `Vec`:
- Double capacity or use required capacity, whichever is larger.
- Minimum capacity of 4 elements.

### Memory Management

- Elements are moved (not cloned) for push/pop/insert/remove/set.
- Elements are cloned for get and slice operations.
- Elements are properly destroyed when removed or when the list is destroyed.
- Buffer reallocation uses the runtime allocator with proper alignment.

### Type Safety

- All operations receive type descriptors for proper handling of:
  - Element size and alignment.
  - Element destruction.
  - Element cloning.

### Error Handling

- Operations return `RtStatus::Ok` or `RtStatus::Error`.
- Out-of-bounds access returns `None` via Option (not an error status).
- Allocation failures return `RtStatus::Error`.

## Comparison with BTreeMap API

| Aspect | BTreeMap | List |
|--------|----------|------|
| Structure | B+tree with nodes | Contiguous array |
| Lookup | By key (logarithmic) | By index (constant) |
| Insert | Sorted position | At index or end |
| Remove | By key | By index |
| Growth | Node allocation | Buffer reallocation |
| Access pattern | Key-value pairs | Indexed elements |

## Implementation Status

All functions are currently stubbed with `todo!()` and require implementation.
The API compiles and type-checks correctly.
