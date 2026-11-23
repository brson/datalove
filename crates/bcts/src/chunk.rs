//! Where a large collection literal can be cut, so that its parts parse apart.
//!
//! A collection of thousands of elements is thousands of independent parses
//! in a row, and the bracer already says where every bracket ends, so the
//! elements can be found without parsing any of them: a level of the token
//! tree is walked a token or a whole bracket at a time. [`plan`] does that
//! once per source, before any parse, and says for each collection worth it
//! where its chunks begin and end. A parser that reaches one of those
//! collections can then parse each chunk as a query of its own, which a
//! compiler can run ahead of time on several threads.
//!
//! What is here knows nothing of a language's collections. Which brackets
//! open one, where its elements begin and what else its chunks need to know
//! of it are the language's to say, through the callback [`plan`] takes; the
//! datalove languages say it in `datalit::chunk_plan`. See
//! `botdocs/plan-parallel-collections.md`.

use rmx::prelude::*;

use crate::bracer::{Cursor, bracer};
use crate::lexer::{Lexed, Sigil, Token, TokenKind};

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

/// Where one collection is cut.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct Cuts {
    /// Where its open bracket starts, which is how a parser that has reached
    /// the bracket finds it in a plan.
    pub open_byte: u32,
    pub chunks: Vec<PlannedChunk>,
    /// The ordinal among all the source's chunks of this collection's first.
    pub first_chunk: u32,
}

/// One chunk of a planned collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct PlannedChunk {
    /// The token positions it runs between, in the collection's level: what
    /// `Cursor::between` takes. A chunk after the first starts where an
    /// element does.
    pub from: u32,
    pub to: u32,
    /// The ordinal in the collection of its first element, which a
    /// diagnostic may name.
    pub first: u32,
}

impl Cuts {
    /// A cursor over chunk `chunk`, in a source whose top level is `top`.
    pub fn chunk_cursor<'db>(&self, top: Cursor<'db>, chunk: usize) -> Cursor<'db> {
        let PlannedChunk { from, to, .. } = self.chunks[chunk];
        top.between(from, to)
    }
}

/// The contents of a bracket that might be a collection worth cutting.
pub struct Level<'db> {
    /// Its tokens, between the brackets.
    pub cursor: Cursor<'db>,
    /// How long it is, brackets and all.
    pub bytes: usize,
    /// How many chunks it may be cut into before the source has [`MAX_CHUNKS`].
    pub room: u32,
}

/// The collections of a source worth parsing in chunks, in source order, each
/// with where it is cut and what `cut` says of it.
///
/// One pass over the bracer's layout, calling `cut` with each bracket that
/// opens a level and the level, which says where the collection that bracket
/// opens is cut, if it opens one worth cutting. Only the outermost collection
/// that is cut is planned; one inside it is parsed whole, in its chunk. A
/// source with bracket errors is planned not at all, so that what recovery
/// makes of it, and reports, is what it always was. Nor is one shorter than
/// `smallest`, which is nearly every source, and costs no more than that.
///
/// The plan does not know the grammar: in most languages a `[` may be a list,
/// a type or an index. A parser consults it only where it is about to parse a
/// collection literal, so a bracket planned that is not one costs the plan,
/// not the parse.
pub fn plan<'db, X>(
    db: &'db dyn crate::Db,
    lexed: Lexed<'db>,
    smallest: usize,
    mut cut: impl FnMut(&'db Token, Level<'db>) -> Option<(Vec<PlannedChunk>, X)>,
) -> Vec<(Cuts, X)> {
    if lexed.source(db).text(db).len() < smallest {
        return Vec::new();
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
        let level = Level {
            cursor: top.between(u32::try_from(index + 1).X(), u32::try_from(resume).X()),
            bytes: tokens[resume - 1].span().end - token.span().start,
            room: MAX_CHUNKS - next_chunk,
        };
        match cut(token, level) {
            Some((chunks, extra)) => {
                let count = u32::try_from(chunks.len()).X();
                let cuts = Cuts {
                    open_byte: u32::try_from(token.span().start).X(),
                    chunks,
                    first_chunk: next_chunk,
                };
                plan.push((cuts, extra));
                next_chunk += count;
                index = resume;
            }
            None => index += 1,
        }
    }
    plan
}

/// The cut collection a bracket opening at `open_byte` is, if any, in a plan
/// in source order.
pub fn planned_at<T>(plan: &[T], open_byte: usize, cuts: impl Fn(&T) -> &Cuts) -> Option<(usize, &T)> {
    let open_byte = u32::try_from(open_byte).ok()?;
    let index = plan.binary_search_by_key(&open_byte, |planned| cuts(planned).open_byte).ok()?;
    Some((index, &plan[index]))
}

/// Chunks made as the elements of a collection go by.
pub struct Chunker {
    limits: Thresholds,
    chunk_bytes: usize,
    chunks: Vec<PlannedChunk>,
    /// Where the chunk being made begins.
    start_byte: usize,
    /// How many elements have gone by.
    ordinal: u32,
}

impl Chunker {
    /// A chunker for `level`, or `None` if it is too small to cut or there is
    /// no room left in the source to cut it.
    pub fn new(limits: Thresholds, level: &Level<'_>) -> Option<Chunker> {
        if level.bytes < limits.min_bytes as usize || level.room < 2 {
            return None;
        }
        // Chunks big enough that there are about `target_chunks` of them, and
        // at most `room`.
        let chunk_bytes = (limits.chunk_bytes as usize)
            .max(level.bytes / limits.target_chunks.max(1) as usize)
            .max(level.bytes.div_ceil(level.room as usize));
        Some(Chunker { limits, chunk_bytes, chunks: Vec::new(), start_byte: 0, ordinal: 0 })
    }

    /// An element beginning at token position `at` and byte `byte`; a chunk
    /// begins with it if the one before has bytes enough.
    pub fn item(&mut self, at: u32, byte: usize) {
        let begins_chunk = self.chunks.is_empty() || byte - self.start_byte >= self.chunk_bytes;
        if begins_chunk {
            if let Some(chunk) = self.chunks.last_mut() {
                chunk.to = at;
            }
            self.chunks.push(PlannedChunk { from: at, to: at, first: self.ordinal });
            self.start_byte = byte;
        }
        self.ordinal += 1;
    }

    /// The chunks of `level`, which every element of has gone by, or `None`
    /// if there are too few elements or chunks for cutting to be worth it.
    pub fn finish(self, level: Cursor<'_>) -> Option<Vec<PlannedChunk>> {
        if self.ordinal < self.limits.min_elements {
            return None;
        }
        let mut chunks = self.chunks;
        chunks.last_mut()?.to = level.end();
        (chunks.len() >= 2).then_some(chunks)
    }
}

/// Every element of a level whose elements are separated by commas.
///
/// An element begins at the first token, and at the first one after each
/// comma. Found in one walk of the level with nothing kept per element: a
/// collection worth cutting has tens of thousands of them, and the plan is
/// paid for in every mode.
///
/// Where an element is said to begin is where it begins in a parse of the
/// whole only if that parse ends each element at a comma, which one with a
/// mistake in it may not; a parser of chunks has to check that the chunks
/// ended where it cut them.
pub fn walk_commas(level: Cursor<'_>, chunker: &mut Chunker) {
    let mut cursor = level;
    let mut begins = Some(level.position());
    while let Some(index) = cursor.next_index() {
        let token = cursor.raw(index);
        if let Some(at) = begins.take() {
            chunker.item(at, token.span().start);
        }
        if cursor.is_plain(index) && token.kind == TokenKind::Sigil(Sigil::Comma) {
            begins = Some(cursor.position());
        }
    }
}
