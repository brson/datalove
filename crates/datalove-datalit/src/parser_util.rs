//! Re-export parser utilities from bcts.

pub use bct::parser_util::{
    eat_exponent_tail, float_exponent, is_decimal_run, is_numeric_literal,
    strip_separators,
    FloatExponent, TokenStream, TokenStreamExt,
};

// Re-export TextSpan from bct for convenience.
pub use bct::text::TextSpan;
