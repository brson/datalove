//! Datalove runtime.
//!
//! The `c` module is the runtime's interface: the C API, and everything else
//! outside the runtime may know about it, which is the `RtHeader` a handle
//! points at and the data types of `rtdt`.
//! The `rust` module wraps the C API in Rust abstractions, and goes through it
//! like any other caller.
//! The implementation is `c::impls`, private to `c`, so that nothing else, in
//! this crate or out of it, reaches past the interface.
//!
//! - "rt" calls are called by the language and have a restricted ABI.
//! - "rti" calls are emitted only by the compiler and have whatever ABI is needed.
//!
//! ## Calling conventions
//!
//! - All functions (except `init`) take a runtime handle,
//!   even if it isn't needed.
//! - All other arguments are either pointers to datalit values
//!   or to type descriptors.
//! - All value pointer arguments are followed by their tydesc,
//!   even if it isn't needed. It should be debug_asserted at least.
//!
//! ## Argument types
//!
//! Value pointers have naming conventions and datalove semantics:
//!
//! - `in` - a `*mut` move in.
//!   Callee becomes owner.
//!   Caller may not read again without moving a new value there.
//! - `out` - a `*mut` move out.
//!   Caller becomes owner.
//! - `ref` - a `*const` shared reference.
//!   Callee may not write.
//! - `mut` - a `*mut` unique reference.
//!   Callee may write.
//!
//! ## Invariants
//!
//! Datalove has a closed type system and all tydescs will be valid
//! for the duration, and new tydescs may not be created.
//!
//! Argument pointers are never null, they instead use the datalit option.


pub mod c;
pub mod rust;

mod abi_table;
