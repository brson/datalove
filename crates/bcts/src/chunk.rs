//! Where a large collection literal can be cut, so that its parts parse apart.
//!
//! A collection of thousands of elements is thousands of independent parses
//! in a row, and the bracer already says where every bracket ends, so the
//! elements can be found without parsing any of them: a level of the token
//! tree is walked a token or a whole bracket at a time. [`collection_plan`]
//! does that once per source, before any parse, and says for each collection
//! worth it where its chunks begin and end. A parser that reaches one of those
//! collections can then parse each chunk as a query of its own, which the
//! compiler runs ahead of time on several threads.
//!
//! Which collections are worth it is a [`ChunkPolicy`], a salsa input; see
//! `botdocs/plan-parallel-collections.md`.

use rmx::prelude::*;

use crate::bracer::{Cursor, bracer};
use crate::lexer::{Lexed, Sigil, TokenKind};

/// The kinds of collection a bracket can open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(salsa::SalsaValue)]
pub enum CollectionKind {
    /// `[ ... ]`
    List,
    /// `%{ ... }`
    Map,
    /// `#{ ... }`
    Set,
    /// `{| ... |}`
    Table,
    /// `[| ... |]`
    Tensor,
}

impl CollectionKind {
    /// The collection a bracket of this sigil opens, if it opens one.
    pub fn of(sigil: Sigil) -> Option<CollectionKind> {
        match sigil {
            Sigil::BracketOpen => Some(CollectionKind::List),
            Sigil::PercentBraceOpen => Some(CollectionKind::Map),
            Sigil::HashBraceOpen => Some(CollectionKind::Set),
            Sigil::BracePipeOpen => Some(CollectionKind::Table),
            Sigil::BracketPipeOpen => Some(CollectionKind::Tensor),
            _ => None,
        }
    }
}

/// When a collection of one kind is cut into chunks, and how finely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct Thresholds {
    /// The smallest collection, open bracket to close, that is cut.
    pub min_bytes: u32,
    /// The fewest elements a collection that is cut has.
    pub min_elements: u32,
    /// The fewest bytes that go in a chunk: each ends at the first separator
    /// past this many bytes from its start.
    pub chunk_bytes: u32,
    /// How many chunks a collection is cut into, unless that would make them
    /// smaller than `chunk_bytes`. Chunks grow with the collection, so that a
    /// big one is not hundreds of chunks, each with a cost of its own.
    pub target_chunks: u32,
}

/// The thresholds in force, per kind; a kind not named is never cut.
///
/// An input, so that changing it plans every source again. A database that
/// has none uses [`DEFAULT_THRESHOLDS`], or [`CUT_EVERYTHING`] if
/// `DATALOVE_CUT_COLLECTIONS` is set, which is how a whole test suite is run
/// with every collection cut. One that wants others has to create it before
/// anything is parsed: a plan made without it does not know to be made again
/// once it exists.
#[salsa::input(singleton)]
pub struct ChunkPolicy {
    #[returns(ref)]
    pub thresholds: Vec<(CollectionKind, Thresholds)>,
}

/// What a database without a [`ChunkPolicy`] cuts.
///
/// From `collection_cutoffs --cutoffs` in `datalove-bench`, October 2026, on
/// four cores: from 32KB a collection of any shape parses in half the time or
/// less cut into 8KB chunks on four threads, and within noise of whole when
/// the chunks parse one after another, which is what a sequential compile and
/// a build with no threads do. Below 32KB the chunks cost the sequential parse
/// 2-5%. A big collection wants bigger chunks: the store demo's 3.6MB of
/// orders compiles faster in either mode in chunks of 32-64KB than of 8KB,
/// hence `target_chunks`. See `botdocs/compile-time.md`.
pub const DEFAULT_THRESHOLDS: &[(CollectionKind, Thresholds)] = &[
    (CollectionKind::List, Thresholds { min_bytes: 32 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
    (CollectionKind::Map, Thresholds { min_bytes: 32 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
    (CollectionKind::Set, Thresholds { min_bytes: 32 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
];

/// Thresholds that cut every collection of the kinds that can be cut, as
/// finely as they can be: for testing that a cut parse is the parse.
pub const CUT_EVERYTHING: &[(CollectionKind, Thresholds)] = &[
    (CollectionKind::List, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
    (CollectionKind::Map, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
    (CollectionKind::Set, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
];

/// The most chunks a source is cut into, all its collections together.
///
/// A chunk numbers what it parses in a range of its own, and the ranges have
/// to fit in a `u32` beside the source's own numbering; see
/// [`chunk_number_base`].
pub const MAX_CHUNKS: u32 = 2048;

/// How many numbers a chunk has to number what it parses.
pub const CHUNK_NUMBERS: u32 = 1 << 20;

/// Where the numbers of a source's `ordinal`th chunk begin.
///
/// Above every number the source's own parse hands out, which stays below
/// `1 << 31`, so the two never meet.
pub fn chunk_number_base(ordinal: u32) -> u32 {
    assert!(ordinal < MAX_CHUNKS, "chunk {ordinal} of a source cut into more than {MAX_CHUNKS}");
    (1 << 31) + ordinal * CHUNK_NUMBERS
}

/// A collection to be parsed in chunks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct PlannedCollection {
    /// Where its open bracket starts, which is how a parser that has reached
    /// the bracket finds it here.
    pub open_byte: u32,
    pub kind: CollectionKind,
    /// The token positions each chunk runs between, in the collection's
    /// level: what `Cursor::between` takes. A chunk after the first starts
    /// just past a separator.
    pub chunks: Vec<(u32, u32)>,
    /// The ordinal among all the source's chunks of this collection's first.
    pub first_chunk: u32,
}

impl PlannedCollection {
    /// A cursor over chunk `chunk`, in a source whose top level is `top`.
    pub fn chunk_cursor<'db>(&self, top: Cursor<'db>, chunk: usize) -> Cursor<'db> {
        let (from, to) = self.chunks[chunk];
        top.between(from, to)
    }
}

/// The planned collection a bracket opening at `open_byte` is, if any.
pub fn planned_at(plan: &[PlannedCollection], open_byte: usize) -> Option<(usize, &PlannedCollection)> {
    let open_byte = u32::try_from(open_byte).ok()?;
    let index = plan.binary_search_by_key(&open_byte, |planned| planned.open_byte).ok()?;
    Some((index, &plan[index]))
}

/// The collections of a source worth parsing in chunks, in source order, and
/// where their chunks are.
///
/// One pass over the bracer's layout. Only the outermost collection that
/// qualifies is planned; one inside it is parsed whole, in its chunk. A source
/// with bracket errors is planned not at all, so that what recovery makes of
/// it, and reports, is what it always was.
///
/// The plan does not know the grammar: a `[` may be a list, a type or an
/// index. A parser consults it only where it is about to parse a collection
/// literal, so a bracket planned that is not one costs the plan, not the parse.
#[salsa::tracked(returns(ref))]
pub fn collection_plan<'db>(db: &'db dyn crate::Db, lexed: Lexed<'db>) -> Vec<PlannedCollection> {
    static CUT_ALL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let policy = ChunkPolicy::try_get(db);
    let thresholds: &[(CollectionKind, Thresholds)] = match &policy {
        Some(policy) => policy.thresholds(db),
        None if *CUT_ALL.get_or_init(|| std::env::var_os("DATALOVE_CUT_COLLECTIONS").is_some()) => CUT_EVERYTHING,
        None => DEFAULT_THRESHOLDS,
    };
    // A source too small to hold a collection worth cutting, which is nearly
    // every source, costs no more than this.
    let smallest = thresholds.iter().map(|(_, limits)| limits.min_bytes as usize).min();
    match smallest {
        None => return Vec::new(),
        Some(smallest) if lexed.source(db).text(db).len() < smallest => return Vec::new(),
        Some(_) => {}
    }
    let bracer = bracer(db, lexed);
    if !bracer.errors(db).is_empty() {
        return Vec::new();
    }

    let tokens = lexed.tokens(db);
    let top = bracer.iter(db);
    let mut plan = Vec::new();
    let mut next_chunk = 0;
    let mut index = 0;
    while index < tokens.len() {
        let Some(resume) = bracer.resume(db, index) else {
            index += 1;
            continue;
        };
        let token = &tokens[index];
        let planned = match token.kind {
            TokenKind::Sigil(sigil) => CollectionKind::of(sigil)
                .and_then(|kind| Some((kind, thresholds.iter().find(|(k, _)| *k == kind)?.1)))
                .and_then(|(kind, limits)| {
                    let bytes = tokens[resume - 1].span().end - token.span().start;
                    if bytes < limits.min_bytes as usize {
                        return None;
                    }
                    let level = top.between(u32::try_from(index + 1).X(), u32::try_from(resume).X());
                    let room = MAX_CHUNKS - next_chunk;
                    cut(kind, level, &tokens, limits, bytes, room).map(|chunks| (kind, chunks))
                }),
            _ => None,
        };
        match planned {
            Some((kind, chunks)) => {
                let count = u32::try_from(chunks.len()).X();
                plan.push(PlannedCollection {
                    open_byte: u32::try_from(token.span().start).X(),
                    kind,
                    chunks,
                    first_chunk: next_chunk,
                });
                next_chunk += count;
                index = resume;
            }
            None => index += 1,
        }
    }
    plan
}

/// The chunks of one collection's level, or `None` if it is not worth
/// cutting or cannot be cut into at most `room` of them.
fn cut(
    kind: CollectionKind,
    level: Cursor<'_>,
    tokens: &[crate::lexer::Token],
    limits: Thresholds,
    bytes: usize,
    room: u32,
) -> Option<Vec<(u32, u32)>> {
    match kind {
        CollectionKind::List | CollectionKind::Map | CollectionKind::Set => {}
        CollectionKind::Table | CollectionKind::Tensor => return None,
    }
    if room < 2 {
        return None;
    }
    // Chunks big enough that there are about `target_chunks` of them, and at
    // most `room`.
    let chunk_bytes = (limits.chunk_bytes as usize)
        .max(bytes / limits.target_chunks.max(1) as usize)
        .max(bytes.div_ceil(room as usize));

    let end = level.end();
    let mut cursor = level;
    let mut chunks = Vec::new();
    let mut from = cursor.position();
    let mut chunk_start = None;
    let mut separators: u32 = 0;
    while let Some(index) = cursor.next_index() {
        let token = &tokens[index as usize];
        let start = *chunk_start.get_or_insert(token.span().start);
        if cursor.is_plain(index) && token.kind == TokenKind::Sigil(Sigil::Comma) {
            separators += 1;
            let after = cursor.position();
            if token.span().end - start >= chunk_bytes && !level.between(after, end).is_empty() {
                chunks.push((from, after));
                from = after;
                chunk_start = None;
            }
        }
    }
    chunks.push((from, end));

    let elements = separators + 1;
    (elements >= limits.min_elements && chunks.len() >= 2).then_some(chunks)
}
