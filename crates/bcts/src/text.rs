use rmx::prelude::*;

use std::ops::Range;

/// Byte span type alias.
pub type ByteSpan = Range<usize>;

/// A span of a text, for use during parsing and error reporting.
#[derive(Clone)]
pub struct TextSpan<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
}

impl<'db> TextSpan<'db> {
    pub fn new(text: Text<'db>, span: ByteSpan) -> Self {
        TextSpan { text, span }
    }

    pub fn start(&self) -> usize {
        self.span.start
    }

    pub fn end(&self) -> usize {
        self.span.end
    }

    pub fn with_end(&self, end: usize) -> Self {
        TextSpan { text: self.text, span: self.span.start..end }
    }

    pub fn with_span(&self, span: ByteSpan) -> Self {
        TextSpan { text: self.text, span }
    }
}

/// The text of a source.
///
/// Carries the `Source` it came from as an identity field, so anything holding
/// a `Text` can name that source. That matters because a `Text` is a tracked
/// struct: salsa deletes it when the query that made it runs again, and an id
/// held past that point is not safe to read. A `Source` is an input and lasts
/// as long as the database, so it is what gets stored.
#[salsa::tracked]
pub struct Text<'db> {
    #[returns(copy)]
    pub source: crate::input::Source,
    #[returns(ref)]
    pub text: String,
}

#[salsa::interned]
#[derive(Debug)]
pub struct InternedText<'db> {
    #[returns(ref)]
    pub text: String,
}

impl<'db> Text<'db> {
    pub fn as_str(&self, db: &'db dyn crate::Db) -> &'db str {
        self.text(db).as_str()
    }
}

impl<'db> InternedText<'db> {
    pub fn as_str(&self, db: &'db dyn crate::Db) -> &'db str {
        self.text(db).as_str()
    }
}

/// Ordering for InternedText based on internal salsa ID.
///
/// This enables use in BTreeMap for Salsa-tracked structs that need Hash/Eq.
/// The ordering is arbitrary but stable within a compilation session.
impl<'db> Ord for InternedText<'db> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use salsa::plumbing::AsId;
        self.as_id().cmp(&other.as_id())
    }
}

impl<'db> PartialOrd for InternedText<'db> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[test]
fn interned_text_eq() {
    let ref db = crate::Database::default();
    let s1 = InternedText::new(db, S("abc"));
    let s2 = InternedText::new(db, S("abc"));
    assert_eq!(s1, s2);
}
