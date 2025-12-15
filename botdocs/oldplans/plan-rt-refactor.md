# Refactoring Plan: Three-Layer Architecture for datalove-rt

## Progress Status

- [x] Phase 1: Create C and Rust Modules - **COMPLETED**
- [x] Phase 2: Organize impl Module - **COMPLETED**
- [x] Phase 3: Migrate External Callers - **COMPLETED**
- [ ] Phase 4: Finalize Privacy

## Initial State (Before Refactoring)

- `lib.rs` contained: module declarations (many pub), C-ABI functions, and types (1343 lines)
- Internal modules (alloc, rt_local, clone, string, btreemap, set, list, tensor, destroy, cmp, pretty, int_math) were public
- External crates used internal APIs directly:
  - `datalove-datalit`: uses `rt_local::RtLocal`, `string::*`, `clone::clone_value`
  - `datalove-cli`: uses `rt_local::RtLocal`
  - `datalove-datafun`: uses `rt_local::RtLocal`, `clone::clone_value`
  - `datalove-rt-tests`: uses various internal impl functions (e.g., `btreemap::btreemap_clone_from_slice_impl`)

## Current State (After Phase 2)

- `lib.rs` now contains: module declarations, `pub mod c;`, `pub mod rust;`, `pub mod impls;`, and re-exports (~115 lines)
- `src/c.rs` contains all C-ABI functions and types (~1200 lines)
- `src/rust.rs` contains safe Rust wrapper API (~70 lines)
- `src/impls.rs` contains re-exports of all internal modules (~20 lines)
- Internal modules still public (for transition period)
- External crates can now access internals via `impls` module
- Ready for Phase 3: migrating external callers to use `rust` module

## Target Architecture

Three public modules exposing different layers:

1. **impl** - internal implementation (eventually private or controlled access)
2. **c** - C-ABI surface for FFI
3. **rust** - safe Rust wrapper API

## Phase 1: Create C and Rust Modules - ✅ COMPLETED

### Step 1: Create `src/c.rs` - ✅ Done

Moved all C-ABI functions from lib.rs:
- All `dtlv_rti_*` extern "C" functions (70+ functions)
- Public types: `LocalRtHandle`, `RtStatus`, `RtEq`, `RtOrdering`
- Functions call internal modules via `crate::` paths
- File size: ~1200 lines

### Step 2: Create `src/rust.rs` - ✅ Done

Created safe wrapper types and functions:
- `Runtime` struct wrapping `RtLocal`
- `Runtime::new()` - creates new runtime instance
- `Runtime::handle()` - access raw handle for C-ABI
- `Runtime::rt_local_mut()` and `Runtime::rt_local()` - unsafe access to internals
- Implements `Default` and `Drop` for RAII handle management
- File size: ~70 lines

### Step 3: Update `src/lib.rs` - ✅ Done

- Added `pub mod c;` and `pub mod rust;`
- Kept internal module declarations as `pub` for now (transition period)
- Removed all C function definitions (now in c.rs)
- Re-exported all C-ABI types and functions at crate root for backward compatibility
- Reduced from 1343 lines to ~115 lines
- Kept module docs and top-level docs

### Phase 1 Test Results - ✅ All Passed

- `cargo test -p datalove-rt`: 87 tests passed
- `cargo test -p datalove-rt-tests`: All tests passed (300+ tests)
- `cargo build -p datalove-datalit`: Success
- `cargo build -p datalove-cli`: Success
- `cargo build -p datalove-datafun`: Success
- `cargo test --all`: Full workspace test suite passes

### Phase 1 Achievements

- Three-layer architecture established with `c` and `rust` modules
- Full backward compatibility maintained via re-exports
- All external crates continue to work without changes
- Code organization improved significantly (lib.rs reduced by 92%)

## Phase 2: Organize impl Module - ✅ COMPLETED

### Step 4: Create `src/impls.rs` - ✅ Done

Created new module to organize internal implementations:
- Created `src/impls.rs` with re-exports of all internal modules
- Re-exports: `alloc`, `rt_local`, `clone`, `string`, `pretty`, `btreemap`, `set`, `list`, `tensor`, `destroy`
- Added `pub mod impls;` declaration in lib.rs
- File size: ~20 lines
- Provides single entry point for accessing internal implementations

### Phase 2 Test Results - ✅ All Passed

- `cargo test -p datalove-rt --lib`: 87 tests passed
- `cargo test -p datalove-rt-tests`: All tests passed (300+ tests)
- `cargo build -p datalove-datalit -p datalove-cli -p datalove-datafun`: Success
- `cargo test --all --lib`: Full workspace test suite passes (287 tests)

### Phase 2 Achievements

- Organized internal modules under `impls` namespace
- Maintained full backward compatibility
- All external crates continue to work without changes
- Three-layer architecture now fully exposed: `impls`, `c`, and `rust` modules

## Phase 3: Migrate External Callers - ✅ COMPLETED

### Step 5: Update external crates to use appropriate module paths - ✅ Done

Migration strategy:
- **datalove-cli**: Migrated to use `rust::Runtime` for RAII wrapper
  - Changed `RtLocal::new()` to `Runtime::new()`
  - Used `runtime.rt_local_mut()` to get mutable reference for library calls
  - Removed manual `shutdown()` call (handled by Drop)
  - Used `runtime.handle()` to get C-ABI handle
- **datalove-datalit**: Updated to use `impls::rt_local::RtLocal`
  - Changed all `datalove_rt::rt_local::RtLocal` to `datalove_rt::impls::rt_local::RtLocal`
  - Library functions continue to accept `&mut RtLocal` parameters
- **datalove-datafun**: Updated to use `impls` paths
  - Changed `datalove_rt::rt_local::RtLocal` to `datalove_rt::impls::rt_local::RtLocal`
  - Changed `datalove_rt::clone::clone_value` to `datalove_rt::impls::clone::clone_value`
- **datalove-rt-tests**: Updated to use `impls` paths
  - Changed all `rt::rt_local::RtLocal` to `rt::impls::rt_local::RtLocal`
  - Changed all `datalove_rt::rt_local::RtLocal` to `datalove_rt::impls::rt_local::RtLocal`
  - Test infrastructure continues to use internal implementation for direct testing

### Phase 3 Test Results - ✅ All Passed

- `cargo test -p datalove-rt --lib`: 87 tests passed
- `cargo test -p datalove-rt-tests`: All tests passed (300+ tests)
- `cargo build -p datalove-cli`: Success
- `cargo build -p datalove-datalit`: Success
- `cargo build -p datalove-datafun`: Success
- `cargo test --all --lib`: Full workspace test suite passes (287 tests)

### Phase 3 Achievements

- CLI code uses safe `rust::Runtime` wrapper with RAII
- Library code uses `impls` module for internal implementation access
- Clear separation: public safe API (`rust`) vs internal implementation (`impls`)
- All external crates successfully migrated without breaking changes
- Full backward compatibility maintained via re-exports
- All tests passing across entire workspace

## Phase 4: Finalize Privacy

### Step 6: Make internal modules private

- Change `pub mod` to `mod` for internal modules
- Expose only via `impls` module if needed for tests
- Ensure only `c`, `rust`, and optionally `impls` modules are public
- Update documentation to guide users to `rust` module primarily

## Testing Strategy

- Run test suite after each step
- `cargo test -p datalove-rt` after creating c.rs and rust.rs
- `cargo test -p datalove-rt-tests` after each phase
- Build all dependent crates to catch migration issues:
  - `cargo build -p datalove-datalit`
  - `cargo build -p datalove-cli`
  - `cargo build -p datalove-datafun`
- Full workspace test: `cargo test --all` before finalizing

## Open Questions

1. Should `datalove-rt-tests` use the `rust` module or continue using internal APIs?
   - Pro for internals: more direct testing of implementation
   - Pro for rust module: tests the actual public API

2. Should the `impls` module be public or use `pub(crate)`?
   - If public: allows escape hatch for advanced users
   - If private: forces migration to safe API

3. C-ABI backward compatibility: ensure existing compiled code can still link

## Implementation Notes

- Preserve all existing C-ABI function signatures exactly
- Internal module code stays unchanged initially
- Focus is on API organization, not implementation changes
- Can combine steps if tests pass cleanly
