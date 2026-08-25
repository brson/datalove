use rmx::prelude::*;

use std::ops::Range;
use std::{iter, mem};

/// Byte span type alias.
pub type ByteSpan = Range<usize>;

/// Lightweight text + span pair for passing around without salsa overhead.
///
/// Unlike `SubText` which is salsa-tracked, this is a simple struct
/// for temporary use during parsing and error reporting.
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

#[salsa::tracked]
pub struct Text<'db> {
    #[returns(ref)]
    pub text: String,
}

#[salsa::tracked]
pub struct SubText<'db> {
    #[returns(copy)]
    pub text: Text<'db>,
    #[returns(clone)]
    pub range: Range<usize>,
}

#[salsa::interned]
#[derive(Debug)]
pub struct InternedText<'db> {
    #[returns(ref)]
    pub text: String,
}

#[salsa::interned]
#[derive(Debug)]
pub struct InternedSubText<'db> {
    #[returns(copy)]
    pub text: InternedText<'db>,
    #[returns(clone)]
    pub range: Range<usize>,
}

impl<'db> Text<'db> {
    pub fn as_sub(
        &self,
        db: &'db dyn crate::Db,
    ) -> SubText<'db> {
        SubText::new(
            db,
            *self,
            (0..self.text(db).len()),
        )
    }

    pub fn sub(
        &self,
        db: &'db dyn crate::Db,
        range: Range<usize>,
    ) -> SubText<'db> {
        SubText::new(
            db,
            *self,
            range,
        )
    }
}

impl<'db> SubText<'db> {
    pub fn sub(
        &self,
        db: &'db dyn crate::Db,
        range: Range<usize>,
    ) -> Option<SubText<'db>> {
        let text_len = self.text(db).text(db).len();
        let subrange = (0..text_len).subrange(range)?;
        Some(SubText::new(
            db,
            self.text(db),
            subrange,
        ))
    }
}

impl<'db> InternedText<'db> {
    pub fn as_sub(
        &self,
        db: &'db dyn crate::Db,
    ) -> InternedSubText<'db> {
        InternedSubText::new(
            db,
            *self,
            (0..self.text(db).len()),
        )
    }

    pub fn sub(
        &self,
        db: &'db dyn crate::Db,
        range: Range<usize>,
    ) -> InternedSubText<'db> {
        InternedSubText::new(
            db,
            *self,
            range,
        )
    }
}

impl<'db> InternedSubText<'db> {
    pub fn sub(
        &self,
        db: &'db dyn crate::Db,
        range: Range<usize>,
    ) -> Option<InternedSubText<'db>> {
        let text_len = self.text(db).text(db).len();
        let subrange = (0..text_len).subrange(range)?;
        Some(InternedSubText::new(
            db,
            self.text(db),
            subrange,
        ))
    }
}

impl<'db> Text<'db> {
    pub fn as_str(&self, db: &'db dyn crate::Db) -> &'db str {
        self.text(db).as_str()
    }
}

impl<'db> SubText<'db> {
    pub fn as_str(&self, db: &'db dyn crate::Db) -> &'db str {
        &self.text(db).text(db)[self.range(db)]
    }
}

impl<'db> InternedText<'db> {
    pub fn as_str(&self, db: &'db dyn crate::Db) -> &'db str {
        self.text(db).as_str()
    }
}

impl<'db> InternedSubText<'db> {
    pub fn as_str(&self, db: &'db dyn crate::Db) -> &'db str {
        &self.text(db).text(db)[self.range(db)]
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

    #[salsa::tracked(returns(copy))]
    fn test_sub<'db>(
        db: &'db dyn crate::Db,
        s1: InternedText<'db>,
        s2: InternedText<'db>,
    ) {
        let s1 = s1.sub(db, 1..2);
        let s2 = s2.sub(db, 1..2);
        assert_eq!(s1, s2);
    }

    test_sub(db, s1, s2);
}
