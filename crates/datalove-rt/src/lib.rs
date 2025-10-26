//! Datalove runtime.
//!
//! The `c` module contains the C runtime API.
//! The `rust` module wraps it with safer Rusty accessors.
//! The `impls` module contains the runtime implementation.
//! It is private and should not be accessed outside the crate.
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

#![allow(unused)]

use rmx::prelude::*;

pub use datalove_rtdt as rtdt;

pub mod c;
pub mod rust;
pub mod impls;

mod cmp;
pub mod alloc;
pub mod rt_local;
pub mod clone;
pub mod string;
pub mod pretty;
pub mod btreemap;
pub mod set;
pub mod list;
pub mod tensor;
pub mod destroy;
mod int_math;
