use rmx::prelude::*;

use std::ops::Range;

use crate::input::Source;

/// Byte span type alias.
pub type ByteSpan = Range<usize>;

/// A span of a source.
///
/// Names the source by its `Source`, which is an input and so lives as long as
/// the database: a span can be filed in a table, kept across a revision and
/// read again afterwards. Spans used to name a `Text`, a tracked struct holding
/// a copy of the source's text, which salsa deletes when the query that made
/// it runs again, so a span that outlived its revision named nothing.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TextSpan {
    pub source: Source,
    pub span: ByteSpan,
}

impl TextSpan {
    pub fn new(source: Source, span: ByteSpan) -> Self {
        TextSpan { source, span }
    }

    pub fn start(&self) -> usize {
        self.span.start
    }

    pub fn end(&self) -> usize {
        self.span.end
    }

    pub fn with_end(&self, end: usize) -> Self {
        TextSpan { source: self.source, span: self.span.start..end }
    }

    pub fn with_span(&self, span: ByteSpan) -> Self {
        TextSpan { source: self.source, span }
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct InternedText<'db> {
    #[returns(ref)]
    pub text: String,
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
