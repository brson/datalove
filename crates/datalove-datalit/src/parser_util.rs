//! Re-export parser utilities from bcts.

pub use bct::parser_util::{
    eat_number, is_decimal_run, is_hex_word, is_identifier, is_numeric_literal, is_number_word,
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

/// What a string literal holds: the text between its quotes, escapes read.
///
/// Both parsers call this on every string they read and report what it
/// refuses, so that a value built later can take the literal as read.
pub fn string_literal_value(raw: &str) -> Result<String, bct::escapes::EscapeError> {
    let inner = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
        .expect("the lexer gives a string token its quotes");
    bct::escapes::process_escape_sequences(inner)
}

/// What to say about an escape a string literal cannot hold.
pub fn escape_complaint(error: &bct::escapes::EscapeError) -> (String, String) {
    use bct::escapes::EscapeError;
    let label = match error {
        EscapeError::InvalidEscape { escape, .. } => rmx::std::format!("`\\{escape}` is not an escape"),
        EscapeError::InvalidUnicodeEscape { reason, .. } => reason.clone(),
        EscapeError::UnterminatedUnicodeEscape { .. } => "a `\\u{` escape is closed with `}`".to_string(),
    };
    (
        "invalid escape in string literal".to_string(),
        rmx::std::format!("{label}; the escapes are \\\" \\\\ \\n \\r \\t \\0 and \\u{{...}}"),
    )
}

/// The names a list has given so far, for refusing one given twice.
///
/// Struct fields, table columns and enum variants are each named once. Both
/// languages read their names through this, so they say the same thing about
/// a repeated one.
#[derive(Default)]
pub struct SeenNames<'db>(rmx::std::collections::HashSet<bct::text::InternedText<'db>>);

impl<'db> SeenNames<'db> {
    /// Take a name, reporting it if it was taken already. False if it was.
    pub fn take(
        &mut self,
        db: &'db dyn salsa::Database,
        name: bct::text::InternedText<'db>,
        ts: TextSpan<'db>,
        what: &str,
    ) -> bool {
        use datalove_diagnostic::DiagnosticBuilderExt;
        if self.0.insert(name) {
            return true;
        }
        let name = name.as_str(db);
        bct::diagnostic::DiagnosticBuilder::error(db, &rmx::std::format!("`{name}` is named twice"))
            .code("D038")
            .primary_label(ts, &rmx::std::format!("a {what} is named once"))
            .emit_parse();
        false
    }
}
