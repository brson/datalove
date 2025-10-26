//! Datalove runtime.
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

// Re-export public C-ABI types and functions for backward compatibility.
pub use c::{LocalRtHandle, RtStatus, RtEq, RtOrdering};
pub use c::dtlv_rti_init;
pub use c::dtlv_rti_shutdown;
pub use c::dtlv_rti_mem_alloc_local;
pub use c::dtlv_rti_mem_free_local;
pub use c::dtlv_rti_clone_local;
pub use c::dtlv_rti_eq;
pub use c::dtlv_rti_eq_unique;
pub use c::dtlv_rti_cmp;
pub use c::dtlv_rti_cmp_total;
pub use c::dtlv_rti_int_add;
pub use c::dtlv_rti_int_sub;
pub use c::dtlv_rti_int_mul;
pub use c::dtlv_rti_int_neg;
pub use c::dtlv_rti_int_div_checked;
pub use c::dtlv_rti_any_destroy_local;
pub use c::dtlv_rti_string_create_local;
pub use c::dtlv_rti_string_destroy_local;
pub use c::dtlv_rti_string_push_bytes_local;
pub use c::dtlv_rti_string_clear_local;
pub use c::dtlv_rti_pretty_print_local;
pub use c::dtlv_rti_btreemap_create_local;
pub use c::dtlv_rti_btreemap_clone_from_slice_local;
pub use c::dtlv_rti_btreemap_destroy_local;
pub use c::dtlv_rti_btreemap_insert_local;
pub use c::dtlv_rti_btreemap_remove_local;
pub use c::dtlv_rti_btreemap_get;
pub use c::dtlv_rti_btreemap_get_local;
pub use c::dtlv_rti_btreemap_clear_local;
pub use c::dtlv_rti_btreeset_create_local;
pub use c::dtlv_rti_btreeset_destroy_local;
pub use c::dtlv_rti_btreeset_insert_local;
pub use c::dtlv_rti_btreeset_remove_local;
pub use c::dtlv_rti_btreeset_contains_local;
pub use c::dtlv_rti_btreeset_clear_local;
pub use c::dtlv_rti_btreeset_clone_from_slice_local;
pub use c::dtlv_rti_list_create_local;
pub use c::dtlv_rti_list_create_from_slice_local;
pub use c::dtlv_rti_list_destroy_local;
pub use c::dtlv_rti_list_clear_local;
pub use c::dtlv_rti_list_get;
pub use c::dtlv_rti_list_set_local;
pub use c::dtlv_rti_list_push_local;
pub use c::dtlv_rti_list_pop_local;
pub use c::dtlv_rti_list_insert_local;
pub use c::dtlv_rti_list_remove_local;
pub use c::dtlv_rti_list_reserve_local;
pub use c::dtlv_rti_list_shrink_to_fit_local;
pub use c::dtlv_rti_list_extend_from_slice_local;
pub use c::dtlv_rti_tensor_create_from_slice_local;
pub use c::dtlv_rti_tensor_destroy_local;
pub use c::dtlv_rti_tensor_get_local;
pub use c::dtlv_rti_tensor_set_local;
pub use c::dtlv_rti_tensor_transpose_local;
pub use c::dtlv_rti_tensor_slice_local;
pub use c::dtlv_rti_tensor_reshape_local;
