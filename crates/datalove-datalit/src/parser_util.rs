//! Re-export parser utilities from bcts.

pub use bct::parser_util::{
    eat_number, is_decimal_run, is_numeric_literal, is_number_word,
    strip_separators,
    Number, Radix, TokenStream, TokenStreamExt,
};

// Re-export TextSpan from bct for convenience.
pub use bct::text::TextSpan;

/// What to say about a number written with letters on the end.
///
/// datalove has no numeric suffixes: a type is written in a hint before the
/// value, not against its digits. Said here so that both languages say it the
/// same way.
pub fn suffix_complaint(suffix: &str) -> (String, String) {
    (
        rmx::std::format!("numeric suffixes are not supported"),
        rmx::std::format!("`{suffix}` is not part of the number; write a type hint, as in `: u8 / 1`"),
    )
}
