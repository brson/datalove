//! Re-export parser utilities from bcts.

pub use bct::parser_util::{is_numeric_literal, TokenStream, TokenStreamExt};

// Re-export TextSpan from bct for convenience.
pub use bct::text::TextSpan;
