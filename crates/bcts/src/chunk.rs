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
use crate::split;

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
/// hence `target_chunks`. Tables from 64KB, below which their chunks in turn
/// cost up to 8%. Tensors only from 512KB and in chunks of 128KB: a tensor
/// does not grow dearer per byte as a list does, so chunks in turn have
/// nothing to win back and cost 10-25% unless they are big. See
/// `botdocs/compile-time.md`.
pub const DEFAULT_THRESHOLDS: &[(CollectionKind, Thresholds)] = &[
    (CollectionKind::List, Thresholds { min_bytes: 32 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
    (CollectionKind::Map, Thresholds { min_bytes: 32 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
    (CollectionKind::Set, Thresholds { min_bytes: 32 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
    (CollectionKind::Table, Thresholds { min_bytes: 64 << 10, min_elements: 16, chunk_bytes: 8 << 10, target_chunks: 64 }),
    (CollectionKind::Tensor, Thresholds { min_bytes: 512 << 10, min_elements: 16, chunk_bytes: 128 << 10, target_chunks: 64 }),
];

/// Thresholds that cut every collection of the kinds that can be cut, as
/// finely as they can be: for testing that a cut parse is the parse.
pub const CUT_EVERYTHING: &[(CollectionKind, Thresholds)] = &[
    (CollectionKind::List, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
    (CollectionKind::Map, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
    (CollectionKind::Set, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
    (CollectionKind::Table, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
    (CollectionKind::Tensor, Thresholds { min_bytes: 0, min_elements: 2, chunk_bytes: 0, target_chunks: u32::MAX }),
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
    pub chunks: Vec<PlannedChunk>,
    /// The ordinal among all the source's chunks of this collection's first.
    pub first_chunk: u32,
    /// A table's header row, as token positions, which every chunk of rows
    /// reads its column count from.
    pub header: Option<(u32, u32)>,
    /// A tensor's rank, which its chunks cannot see from their own commas;
    /// 0 for anything else.
    pub rank: u32,
}

/// One chunk of a planned collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct PlannedChunk {
    /// The token positions it runs between, in the collection's level: what
    /// `Cursor::between` takes. A chunk after the first starts where an
    /// element, row or group does.
    pub from: u32,
    pub to: u32,
    /// The ordinal in the collection of its first element, row or group,
    /// which a diagnostic may name: a table counts its rows after the
    /// header, and a tensor the groups at its outermost axis with something
    /// in them.
    pub first: u32,
}

impl PlannedCollection {
    /// A cursor over chunk `chunk`, in a source whose top level is `top`.
    pub fn chunk_cursor<'db>(&self, top: Cursor<'db>, chunk: usize) -> Cursor<'db> {
        let PlannedChunk { from, to, .. } = self.chunks[chunk];
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
    let text = lexed.source(db).text(db).as_str();
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
                    cut(kind, level, text, limits, bytes, room).map(|cut| (kind, cut))
                }),
            _ => None,
        };
        match planned {
            Some((kind, Cut { chunks, header, rank })) => {
                let count = u32::try_from(chunks.len()).X();
                plan.push(PlannedCollection {
                    open_byte: u32::try_from(token.span().start).X(),
                    kind,
                    chunks,
                    first_chunk: next_chunk,
                    header,
                    rank,
                });
                next_chunk += count;
                index = resume;
            }
            None => index += 1,
        }
    }
    plan
}

/// Where a collection is cut, and what its chunks need to know of it.
struct Cut {
    chunks: Vec<PlannedChunk>,
    header: Option<(u32, u32)>,
    rank: u32,
}

/// The chunks of one collection's level, or `None` if it is not worth
/// cutting or cannot be cut into at most `room` of them.
///
/// What a chunk starts at is where an element begins in a parse of the whole:
/// for a list, map or set, just past a comma, which a parse can disagree with
/// where an element with a mistake in it takes the comma (see
/// `datalit::parser::parse_chunk`); for a table, a row, which the lines say
/// before anything is parsed; and for a tensor, a group at its outermost axis,
/// which the runs of commas say. A rank-1 tensor is not cut: its elements are
/// written side by side, and where one ends only a parse can say.
fn cut(
    kind: CollectionKind,
    level: Cursor<'_>,
    text: &str,
    limits: Thresholds,
    bytes: usize,
    room: u32,
) -> Option<Cut> {
    if room < 2 {
        return None;
    }
    // Chunks big enough that there are about `target_chunks` of them, and at
    // most `room`.
    let chunk_bytes = (limits.chunk_bytes as usize)
        .max(bytes / limits.target_chunks.max(1) as usize)
        .max(bytes.div_ceil(room as usize));
    let mut chunker = Chunker { chunk_bytes, chunks: Vec::new(), start_byte: 0, ordinal: 0 };
    let mut header = None;
    let mut rank = 0;
    // Where each element, row or group begins, found in one walk of the level
    // with nothing kept per element: a collection worth cutting has tens of
    // thousands of them, and the plan is paid for in every mode.
    match kind {
        CollectionKind::List | CollectionKind::Map | CollectionKind::Set => {
            let mut cursor = level;
            // An element begins at the first token, and at the first one
            // after each comma.
            let mut begins = Some(level.position());
            while let Some(index) = cursor.next_index() {
                let token = cursor.raw(index);
                if let Some(at) = begins.take() {
                    chunker.item(at, token.span().start, true);
                }
                if cursor.is_plain(index) && token.kind == TokenKind::Sigil(Sigil::Comma) {
                    begins = Some(cursor.position());
                }
            }
        }
        CollectionKind::Table => {
            // A row begins at a token a line break came before, as
            // `split::Lines` has it; the first line is the header.
            let mut cursor = level;
            let mut header_from = None;
            while let Some(index) = cursor.next_index() {
                let token = cursor.raw(index);
                match header_from {
                    None => header_from = Some(index),
                    Some(from) if token.newline_before => {
                        if header.is_none() {
                            header = Some((from, index));
                        }
                        chunker.item(index, token.span().start, true);
                    }
                    Some(_) => {}
                }
            }
        }
        CollectionKind::Tensor => {
            let body = match split::split_tensor_header(text, level) {
                (Some(Err(_)), _) => return None,
                (_, body) => body,
            };
            rank = u32::try_from(split::max_comma_run(body) + 1).X();
            if rank < 2 {
                return None;
            }
            // A group begins at the first token, and at the first one after
            // each separator: `rank - 1` commas, counted off a run of them as
            // `split::split_commas` does, a shorter run staying in its group.
            let separator = rank - 1;
            let mut cursor = body;
            let mut begins = Some(body.position());
            let mut commas = 0;
            while let Some(index) = cursor.next_index() {
                let token = cursor.raw(index);
                if let Some(at) = begins.take() {
                    chunker.item(at, token.span().start, true);
                }
                if cursor.is_plain(index) && token.kind == TokenKind::Sigil(Sigil::Comma) {
                    commas += 1;
                    if commas == separator {
                        commas = 0;
                        begins = Some(cursor.position());
                    }
                } else {
                    commas = 0;
                }
            }
        }
    }

    if chunker.ordinal < limits.min_elements {
        return None;
    }
    let mut chunks = chunker.chunks;
    chunks.last_mut()?.to = level.end();
    (chunks.len() >= 2).then_some(Cut { chunks, header, rank })
}

/// Chunks made as the elements of a collection go by.
struct Chunker {
    chunk_bytes: usize,
    chunks: Vec<PlannedChunk>,
    /// Where the chunk being made begins.
    start_byte: usize,
    /// How many elements have gone by that count toward a chunk's `first`.
    ordinal: u32,
}

impl Chunker {
    /// An element beginning at token position `at` and byte `byte`; a chunk
    /// begins with it if the one before has bytes enough.
    fn item(&mut self, at: u32, byte: usize, counts: bool) {
        let begins_chunk = self.chunks.is_empty() || byte - self.start_byte >= self.chunk_bytes;
        if begins_chunk {
            if let Some(chunk) = self.chunks.last_mut() {
                chunk.to = at;
            }
            self.chunks.push(PlannedChunk { from: at, to: at, first: self.ordinal });
            self.start_byte = byte;
        }
        if counts {
            self.ordinal += 1;
        }
    }
}
