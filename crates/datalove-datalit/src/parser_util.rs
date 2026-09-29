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

/// The kind of thing a declaration names, which decides what it may not be
/// called.
///
/// Datalove has no words reserved everywhere. A word is reserved for a kind
/// of name only where it can stand where that name is read and mean something
/// else there; see `botdocs/report-reserved-words.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// A `let`, `var` or `const`, or a binding in `if`, `else` or `case`.
    Value,
    /// A function's parameter.
    Parameter,
    /// A function.
    Function,
    /// A type alias or type parameter.
    Type,
}

/// Words that start an expression, and so cannot be read as a value's name.
///
/// `enum` is special only before `{`, so it is left to be a name.
pub const EXPRESSION_WORDS: &[&str] = &[
    "true", "false", "none", "some", "ok", "er", "data", "error",
    "atom", "term", "not", "icall",
];

/// Words that start a statement, and so cannot be read as the name a call
/// statement begins with.
pub const STATEMENT_WORDS: &[&str] = &[
    "let", "var", "const", "set", "fun", "native", "ret", "if", "match",
    "loop", "break", "continue", "require", "import", "type", "debuglog",
];

/// Words written before a parameter's name to say how it is passed.
pub const PARAMETER_MODE_WORDS: &[&str] = &["mut", "out", "ref", "const"];

/// The names of the primitive types.
pub const PRIMITIVE_TYPE_NAMES: &[&str] = &[
    "bool", "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64",
    "index", "offset", "f32", "f64", "int", "string", "data", "error",
];

/// Words that are a type, or start one, where a type is read.
pub fn is_type_word(word: &str) -> bool {
    PRIMITIVE_TYPE_NAMES.contains(&word) || matches!(word, "atom" | "term" | "enum")
}

/// Why a word cannot name a thing of this kind, if it cannot.
pub fn reserved_for(word: &str, kind: NameKind) -> Option<&'static str> {
    let expression = EXPRESSION_WORDS.contains(&word);
    match kind {
        NameKind::Value if expression => Some("it starts an expression"),
        NameKind::Parameter if expression => Some("it starts an expression"),
        NameKind::Parameter if PARAMETER_MODE_WORDS.contains(&word) => {
            Some("it says how a parameter is passed")
        }
        NameKind::Function if expression => Some("it starts an expression"),
        NameKind::Function if STATEMENT_WORDS.contains(&word) => Some("it starts a statement"),
        NameKind::Type if is_type_word(word) => Some("it is a type, or starts one"),
        _ => None,
    }
}

/// What to say about a declaration named with a reserved word.
pub fn reserved_complaint(word: &str, reason: &str) -> (String, String) {
    (
        rmx::std::format!("`{word}` cannot be declared here"),
        rmx::std::format!("`{word}` is reserved, since {reason}"),
    )
}

/// What a type name that names nothing may have been meant as.
///
/// A primitive written in another case, or the name of a collection, whose
/// type is written with a sigil. Asked only once no alias or parameter of the
/// name was found, since both are free to use these names.
pub fn type_name_suggestion(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    if PRIMITIVE_TYPE_NAMES.contains(&lower.as_str()) {
        return Some(rmx::std::format!("did you mean `{lower}`?"));
    }
    let spelled = match lower.as_str() {
        "tuple" => "(T, ...)",
        "list" => "[T]",
        "map" => "%{K = V}",
        "set" => "#{T}",
        "table" => "{| name: T |}",
        "tensor" => "[|T, N|]",
        _ => return None,
    };
    Some(rmx::std::format!("a {lower} type is written `{spelled}`"))
}

/// What is wrong with a tensor body written under a shape header, if
/// anything.
///
/// A body under a header is written flat, with no commas, and filled in
/// row-major order, or shaped exactly as the header says. Both parsers ask
/// here so that they refuse the same bodies in the same words.
pub fn tensor_body_complaint(
    extents: &[u32],
    body_rank: usize,
    body_shape: &[u32],
    element_count: usize,
) -> Option<(String, String)> {
    let spelled = |shape: &[u32]| {
        shape.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(" ")
    };
    if body_rank == 1 {
        let holds: u64 = extents.iter().map(|&e| e as u64).product();
        if holds == element_count as u64 {
            return None;
        }
        return Some((
            rmx::std::format!("a tensor of shape `{}` holds {holds} elements, and {element_count} are written", spelled(extents)),
            "the shape".to_string(),
        ));
    }
    if body_shape == extents {
        return None;
    }
    Some((
        rmx::std::format!(
            "the shape says `{}`, and the body is shaped `{}`",
            spelled(extents), spelled(body_shape),
        ),
        "a body under a shape is written flat, or shaped as it says".to_string(),
    ))
}

/// Whether a tensor of this shape can only be written with a shape header.
///
/// Without one, the rank is read off the widest comma run between the
/// body's parts, and every extent off how many parts there are. So a zero
/// extent above the innermost axis cannot be written, and nor can a leading
/// extent of one, whose axis never shows a separator.
pub fn tensor_needs_header(shape: &[u32]) -> bool {
    shape.len() > 1 && (shape[0] == 1 || shape.contains(&0))
}
