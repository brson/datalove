//! Which of a source's collection literals are parsed in chunks, and where
//! they are cut.
//!
//! `bct::chunk` does the cutting; this says what it cuts in the datalove
//! languages: which brackets open a collection, where each kind's elements
//! begin, and how big one has to be before cutting it is worth it. Which
//! collections are worth it is a [`ChunkPolicy`], a salsa input; see
//! `botdocs/plan-parallel-collections.md`.

use rmx::prelude::*;

use bct::chunk::{Chunker, Cuts, Level, PlannedChunk, Thresholds};
use bct::lexer::{Lexed, Sigil, Token, TokenKind};

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

/// A collection to be parsed in chunks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct PlannedCollection {
    pub cuts: Cuts,
    pub form: CollectionForm,
}

/// What kind of collection a planned one is, with what its chunks need to
/// know of it that they cannot see in their own tokens.
///
/// A chunk's `first` counts what that kind's chunks begin at: a list's, map's
/// or set's elements, a table's rows after the header, and the groups at a
/// tensor's outermost axis with something in them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub enum CollectionForm {
    List,
    Map,
    Set,
    /// The header row, as token positions, which every chunk of rows reads
    /// its column count from.
    Table { header: (u32, u32) },
    /// The rank, which the chunks cannot see from their own commas.
    Tensor { rank: u32 },
}

impl CollectionForm {
    pub fn kind(self) -> CollectionKind {
        match self {
            CollectionForm::List => CollectionKind::List,
            CollectionForm::Map => CollectionKind::Map,
            CollectionForm::Set => CollectionKind::Set,
            CollectionForm::Table { .. } => CollectionKind::Table,
            CollectionForm::Tensor { .. } => CollectionKind::Tensor,
        }
    }
}

/// The planned collection a bracket opening at `open_byte` is, if any.
pub fn planned_at(plan: &[PlannedCollection], open_byte: usize) -> Option<(usize, &PlannedCollection)> {
    bct::chunk::planned_at(plan, open_byte, |planned| &planned.cuts)
}

/// The collections of a source worth parsing in chunks, in source order, and
/// where their chunks are; see `bct::chunk::plan`.
#[salsa::tracked(returns(ref))]
pub fn collection_plan<'db>(db: &'db dyn crate::Db, lexed: Lexed<'db>) -> Vec<PlannedCollection> {
    static CUT_ALL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let policy = ChunkPolicy::try_get(db);
    let thresholds: &[(CollectionKind, Thresholds)] = match &policy {
        Some(policy) => policy.thresholds(db),
        None if *CUT_ALL.get_or_init(|| std::env::var_os("DATALOVE_CUT_COLLECTIONS").is_some()) => CUT_EVERYTHING,
        None => DEFAULT_THRESHOLDS,
    };
    let Some(smallest) = thresholds.iter().map(|(_, limits)| limits.min_bytes as usize).min() else {
        return Vec::new();
    };
    let text = lexed.source(db).text(db).as_str();
    bct::chunk::plan(db, lexed, smallest, |token, level| cut(thresholds, text, token, level))
        .into_iter()
        .map(|(cuts, form)| PlannedCollection { cuts, form })
        .collect()
}

/// The chunks of the collection `open` opens, if it opens one worth cutting.
///
/// What a chunk starts at is where an element begins in a parse of the whole:
/// for a list, map or set, just past a comma, which a parse can disagree with
/// where an element with a mistake in it takes the comma (see
/// `parser::parse_chunk`); for a table, a row, which the lines say before
/// anything is parsed; and for a tensor, a group at its outermost axis, which
/// the runs of commas say. A rank-1 tensor is not cut: its elements are
/// written side by side, and where one ends only a parse can say.
fn cut(
    thresholds: &[(CollectionKind, Thresholds)],
    text: &str,
    open: &Token,
    level: Level<'_>,
) -> Option<(Vec<PlannedChunk>, CollectionForm)> {
    let TokenKind::Sigil(sigil) = open.kind else { return None };
    let kind = CollectionKind::of(sigil)?;
    let limits = thresholds.iter().find(|(k, _)| *k == kind)?.1;
    let mut chunker = Chunker::new(limits, &level)?;
    let form = match kind {
        CollectionKind::List | CollectionKind::Map | CollectionKind::Set => {
            bct::chunk::walk_commas(level.cursor, &mut chunker);
            match kind {
                CollectionKind::List => CollectionForm::List,
                CollectionKind::Map => CollectionForm::Map,
                _ => CollectionForm::Set,
            }
        }
        CollectionKind::Table => {
            // A row is a line, as the parsers read them; the first is the
            // header.
            let mut lines = bct::split::Lines::new(level.cursor);
            let header = lines.next()?;
            for line in lines {
                let at = line.position();
                chunker.item(at, line.raw(at).span().start);
            }
            CollectionForm::Table { header: (header.position(), header.end()) }
        }
        CollectionKind::Tensor => {
            let body = match crate::parser_util::split_tensor_header(text, level.cursor) {
                (Some(Err(_)), _) => return None,
                (_, body) => body,
            };
            let rank = u32::try_from(bct::split::max_comma_run(body) + 1).X();
            if rank < 2 {
                return None;
            }
            // A group begins at the first token, and at the first one after
            // each separator: `rank - 1` commas, counted off a run of them as
            // `bct::split::split_commas` does, a shorter run staying in its
            // group.
            let separator = rank - 1;
            let mut cursor = body;
            let mut begins = Some(body.position());
            let mut commas = 0;
            while let Some(index) = cursor.next_index() {
                let token = cursor.raw(index);
                if let Some(at) = begins.take() {
                    chunker.item(at, token.span().start);
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
            CollectionForm::Tensor { rank }
        }
    };
    Some((chunker.finish(level.cursor)?, form))
}
