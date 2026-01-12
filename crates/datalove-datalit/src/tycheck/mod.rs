//! Datalit typechecker.
//!
//! Provides type checking for datalit expressions.
//!
//! The typechecker uses bidirectional typing: it can either synthesize a type
//! from an expression or check an expression against an expected type.

mod api;
mod check;
mod context;
mod synthesize;
pub mod types;

// Re-export public API.
pub use api::{
    type_check,
    type_check_with_expected,
    TypecheckResult,
    TypeErrorEntry,
};

// Re-export types.
pub use types::{
    // Type representation.
    Type,
    TypeAndHeap,
    TypeAnonTuple,
    TypeAnonStruct,
    TypeNamedField,
    TypeAnonEnum,
    TypeEnumVariant,
    TypeList,
    TypeMap,
    TypeSet,
    TypeOption,
    TypeResult,
    TypeTensor,
    // Error type.
    TypeError,
    // Heap utilities.
    heaps_compatible,
    heap_to_string,
    // Type predicates.
    is_numeric_type,
    is_float_type,
    is_bigint_type,
    is_fixed_int_type,
    is_unsigned_int_type,
    is_bool_type,
    // Type equivalence.
    types_equivalent,
    types_and_heaps_equivalent,
    can_widen_to,
    // Element compatibility.
    check_element_compatible,
    check_map_entry_compatible,
    check_type_coercion,
    // Empty collection types.
    unit_type,
    empty_list_type,
    empty_set_type,
    empty_map_type,
    empty_tensor_type,
    check_tensor_element_count,
    // Integer range checking.
    check_int_fits_type,
    check_int_fits_wrapped_type,
    check_hex_fits_type,
    check_hex_fits_wrapped_type,
    // Type conversion.
    convert_type_hint,
    // Type to string.
    type_to_string,
    // Diagnostic helpers.
    int_type_range_info,
    hex_type_range_info,
};
