//! Datalit typechecker.
//!
//! Provides type checking for datalit expressions.
//!
//! The typechecker uses bidirectional typing: it can either synthesize a type
//! from an expression or check an expression against an expected type.

mod api;
mod check;
mod context;
pub mod infer;
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
    OpenLiteral,
    TypeAnonTuple,
    TypeAnonStruct,
    TypeNamedField,

    TypeList,
    TypeMap,
    TypeSet,
    TypeOption,
    TypeResult,
    TypeTensor,
    TypeTable,
    TypeAtom,
    TypeTerm,
    TypeEnum,
    TypeEnumVariant,
    TypeRec,
    RecName,
    RecError,
    RecErrorKind,
    TypeNames,
    convert_type_hint_with,
    expose,
    contains_rec,
    // Error type.
    TypeError,
    // Type predicates.
    is_numeric_type,
    is_float_type,
    is_bigint_type,
    is_fixed_int_type,
    is_unsigned_int_type,
    is_bool_type,
    // Type equivalence.
    types_equivalent,
    can_widen_to,
    can_adapt_to,
    // Unit and shape helpers.
    unit_type,
    check_tensor_element_count,
    // Integer range checking.
    check_int_fits_type,
    check_hex_fits_type,
    // Type conversion.
    convert_type_hint,
    // Type to string.
    type_to_string,
    // Diagnostic helpers.
    int_type_range_info,
    hex_type_range_info,
};
