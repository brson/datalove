# Refactoring Plan: Three-Layer Architecture for datalove-rt

## Current State

- `lib.rs` contains: module declarations (many pub), C-ABI functions, and types
- Internal modules (alloc, rt_local, clone, string, btreemap, set, list, tensor, destroy, cmp, pretty, int_math) are public
- External crates use internal APIs directly:
  - `datalove-datalit`: uses `rt_local::RtLocal`, `string::*`, `clone::clone_value`
  - `datalove-cli`: uses `rt_local::RtLocal`
  - `datalove-datafun`: uses `rt_local::RtLocal`, `clone::clone_value`
  - `datalove-rt-tests`: uses various internal impl functions (e.g., `btreemap::btreemap_clone_from_slice_impl`)

## Target Architecture

Three public modules exposing different layers:

1. **impl** - internal implementation (eventually private or controlled access)
2. **c** - C-ABI surface for FFI
3. **rust** - safe Rust wrapper API

## Phase 1: Create C and Rust Modules

### Step 1: Create `src/c.rs`

Move all C-ABI functions from lib.rs:
- All `dtlv_rti_*` extern "C" functions
- Public types: `LocalRtHandle`, `RtStatus`, `RtEq`, `RtOrdering`
- Functions call internal modules via `crate::` paths

### Step 2: Create `src/rust.rs`

Create safe wrapper types and functions:
- `Runtime` struct wrapping `RtLocal`
- Safe methods calling either C-ABI or internal impl
- Idiomatic Rust API for external callers
- Handle management with RAII

### Step 3: Update `src/lib.rs`

- Add `pub mod c;` and `pub mod rust;`
- Keep internal module declarations as `pub` for now (transition period)
- Remove C function definitions (now in c.rs)
- Keep module docs and top-level docs

## Phase 2: Organize impl Module

### Step 4: Create impl visibility

Choose approach:
- Option A: Create `pub mod impl_` in lib.rs that re-exports internal modules
- Option B: Use `#[doc(hidden)]` on internal modules to discourage direct use
- Option C: Create `src/impl_/mod.rs` with submodules

Recommended: Option A for clarity - explicit `impl_` module

## Phase 3: Migrate External Callers

### Step 5: Update external crates to use rust module

Files to update:
- `crates/datalove-datalit/src/instantiate2.rs`
  - Replace `rt_local::RtLocal` with `rust::Runtime`
  - Replace direct `string::*` calls with rust module equivalents
- `crates/datalove-cli/src/main.rs`
  - Replace `rt_local::RtLocal` with `rust::Runtime`
- `crates/datalove-datafun/src/eval_datafun.rs`
  - Replace `rt_local::RtLocal` with `rust::Runtime`
  - Replace `clone::clone_value` with rust module equivalent
- `crates/datalove-rt-tests/tests/*.rs`
  - Consider: keep using internals (via `impl_` module) as it's test infrastructure
  - Or: migrate to rust module for more realistic testing

## Phase 4: Finalize Privacy

### Step 6: Make internal modules private

- Change `pub mod` to `mod` for internal modules
- Expose only via `impl_` module if needed for tests
- Ensure only `c`, `rust`, and optionally `impl_` modules are public
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

2. Should the `impl_` module be public or use `pub(crate)`?
   - If public: allows escape hatch for advanced users
   - If private: forces migration to safe API

3. C-ABI backward compatibility: ensure existing compiled code can still link

## Implementation Notes

- Preserve all existing C-ABI function signatures exactly
- Internal module code stays unchanged initially
- Focus is on API organization, not implementation changes
- Can combine steps if tests pass cleanly
