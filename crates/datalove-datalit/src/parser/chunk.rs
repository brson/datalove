//! A collection parsed in chunks, each a query of its own.
//!
//! [`bct::chunk::collection_plan`] says which of a source's collections to
//! parse this way and where their chunks are. A chunk is parsed by the parser
//! a collection makes for its contents, over the chunk's run of elements, and
//! comes back with everything that parser would have left behind: its
//! elements, their spans, its diagnostics, whether it reported an error and
//! where it stopped short, if it did. [`Parser::parse_planned`] puts the chunks
//! back together into what a parse of the whole collection makes, which the
//! chunks being queries lets the compiler parse ahead of time, in parallel.
//!
//! **A chunk boundary is a guess.** The plan cuts after a separator, but an
//! element that does not parse can take the separator after it as part of its
//! mistake, and then a parse of the whole collection goes on from somewhere
//! else. So a chunk says whether it ended by passing its last separator, as a
//! parse of the whole would, and where one did not, the chunks are not used:
//! the collection is parsed whole instead. Only a collection with an error in
//! it can get there.

use rmx::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use bct::chunk::{CollectionKind, chunk_number_base, CHUNK_NUMBERS};
use bct::diagnostic::StoredDiagnostic;
use bct::input::Source;
use bct::text::TextSpan;
use bct::bracer::Cursor;
use bct::{bracer, lexer};

use crate::ast;
use crate::parser_util::{TokenStream, TokenStreamExt};
use super::state::{EmitTo, Parser, emit_stored, not_exhausted_error};

/// The elements of one chunk.
#[derive(Clone, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub enum ChunkItems<'db> {
    /// Of a list or a set.
    Exprs(Vec<ast::ExprFull<'db>>),
    /// Of a map.
    Entries(Vec<ast::ExprMapEntry<'db>>),
}

impl<'db> ChunkItems<'db> {
    pub(super) fn into_exprs(self) -> Vec<ast::ExprFull<'db>> {
        match self {
            ChunkItems::Exprs(exprs) => exprs,
            ChunkItems::Entries(_) => bug!(),
        }
    }

    pub(super) fn into_entries(self) -> Vec<ast::ExprMapEntry<'db>> {
        match self {
            ChunkItems::Entries(entries) => entries,
            ChunkItems::Exprs(_) => bug!(),
        }
    }

    fn extend(&mut self, other: &ChunkItems<'db>) {
        match (self, other) {
            (ChunkItems::Exprs(all), ChunkItems::Exprs(more)) => all.extend(more.iter().cloned()),
            (ChunkItems::Entries(all), ChunkItems::Entries(more)) => all.extend(more.iter().cloned()),
            _ => bug!(),
        }
    }
}

/// What one chunk of a collection parsed to.
#[derive(Clone, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct ChunkParse<'db> {
    pub items: ChunkItems<'db>,
    pub spans: Vec<ast::ParseSpanEntry>,
    /// In the order they were made, for the parse that asks to file.
    pub diagnostics: Vec<StoredDiagnostic>,
    /// Whether the chunk's own level reported an error, which a parse of the
    /// whole collection would carry into the chunks after it.
    pub had_error: bool,
    /// Where the tokens the chunk did not get to begin, if it stopped short.
    /// A parse of the whole collection stops there too.
    pub leftover: Option<TextSpan>,
    /// Whether it ended by passing a separator with nothing after it, which
    /// every chunk but the last does when the cut was where a parse of the
    /// whole collection separates its elements.
    pub ended_on_separator: bool,
}

/// Chunk `chunk` of the `collection`th collection planned in `source`.
#[salsa::tracked(returns(ref))]
pub fn parse_chunk<'db>(
    db: &'db dyn crate::Db,
    source: Source,
    collection: u32,
    chunk: u32,
) -> ChunkParse<'db> {
    let lexed = lexer::lex(db, source);
    let plan = bct::chunk::collection_plan(db, lexed);
    let planned = &plan[collection as usize];
    let cursor = planned.chunk_cursor(bracer::bracer(db, lexed).iter(db), chunk as usize);

    let base = chunk_number_base(planned.first_chunk + chunk);
    let kept = Rc::new(RefCell::new(Vec::new()));
    let mut parser = Parser::from_branch_numbering_aliases(db, cursor, source, base);
    parser.expr_counter = base;
    parser.plan = plan;
    parser.sink = Some(Rc::clone(&kept));

    // `parse_comma_separated`, saying how it ended.
    let mut ended_on_separator = false;
    let mut parse_items = |parser: &mut Parser<'db>, item: &mut dyn FnMut(&mut Parser<'db>)| {
        if parser.peek().is_none() {
            return;
        }
        loop {
            item(parser);
            if !parser.eat_sigil(bct::lexer::Sigil::Comma) {
                break;
            }
            if parser.peek().is_none() {
                ended_on_separator = true;
                break;
            }
        }
    };
    let items = match planned.kind {
        CollectionKind::List | CollectionKind::Set => {
            let mut exprs = Vec::new();
            parse_items(&mut parser, &mut |p| exprs.push(p.parse_expr_full()));
            ChunkItems::Exprs(exprs)
        }
        CollectionKind::Map => {
            let mut entries = Vec::new();
            parse_items(&mut parser, &mut |p| entries.push(p.parse_map_entry()));
            ChunkItems::Entries(entries)
        }
        CollectionKind::Table | CollectionKind::Tensor => todo!("cutting a {:?}", planned.kind),
    };
    let leftover = parser.peek().is_some().then(|| parser.peek_text_span());
    assert!(parser.expr_counter - base < CHUNK_NUMBERS && parser.alias_counter - base < CHUNK_NUMBERS,
        "a chunk numbered more than {CHUNK_NUMBERS} expressions");

    let spans = parser.take_expr_spans();
    let had_error = parser.had_error;
    drop(parser);
    ChunkParse {
        items,
        spans,
        diagnostics: Rc::try_unwrap(kept).ok().X().into_inner(),
        had_error,
        leftover,
        ended_on_separator,
    }
}

impl<'db> Parser<'db> {
    /// The elements of the `index`th planned collection, whose bracket this
    /// parser has just passed and whose contents are `inner`, put together
    /// from its chunks.
    ///
    /// What a parse of the whole collection would have made and reported:
    /// each chunk's diagnostics filed in turn, and the parse stopping at the
    /// first chunk that stopped short, with the complaint about what it left
    /// unless the collection had already reported an error. If a chunk before
    /// that ended other than on a separator, the cuts were not where the
    /// elements divide, and the collection is parsed whole.
    pub(super) fn parse_planned(&mut self, index: usize, inner: Cursor<'db>) -> ChunkItems<'db> {
        let planned = &self.plan[index];
        let kind = planned.kind;
        let collection = u32::try_from(index).X();
        let parts: Vec<&ChunkParse<'db>> = (0..planned.chunks.len())
            .map(|chunk| parse_chunk(self.db, self.source_text(), collection, u32::try_from(chunk).X()))
            .collect();
        let used = parts.iter().position(|part| part.leftover.is_some()).map_or(parts.len(), |at| at + 1);
        if parts[..used - 1].iter().any(|part| !part.ended_on_separator) {
            return self.parse_collection_whole(kind, inner);
        }

        let mut items = match kind {
            CollectionKind::Map => ChunkItems::Entries(Vec::new()),
            _ => ChunkItems::Exprs(Vec::new()),
        };
        let mut had_error = false;
        for part in &parts[..used] {
            for diagnostic in &part.diagnostics {
                emit_stored(self.db, &self.sink, diagnostic.clone());
            }
            items.extend(&part.items);
            self.expr_spans.extend(part.spans.iter().cloned());
            had_error |= part.had_error;
            if let Some(ts) = &part.leftover {
                if !had_error {
                    not_exhausted_error(self.db, ts.clone()).emit_to(&self.sink);
                }
                break;
            }
        }
        items
    }

    /// The elements of a collection parsed the ordinary way, as one run.
    fn parse_collection_whole(&mut self, kind: CollectionKind, inner: Cursor<'db>) -> ChunkItems<'db> {
        let mut sub_parser = self.sub_parser_from_branch(inner);
        let items = match kind {
            CollectionKind::List | CollectionKind::Set => {
                ChunkItems::Exprs(sub_parser.parse_comma_separated(|p| p.parse_expr_full()))
            }
            CollectionKind::Map => ChunkItems::Entries(sub_parser.parse_comma_separated(|p| p.parse_map_entry())),
            CollectionKind::Table | CollectionKind::Tensor => todo!("cutting a {kind:?}"),
        };
        sub_parser.error_if_not_exhausted();
        self.merge_spans_from(&mut sub_parser);
        items
    }
}
