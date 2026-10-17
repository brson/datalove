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
    /// Of a table: its rows, after the header.
    Rows(Vec<ast::ExprTableRow<'db>>),
    /// Of a tensor: the groups at its outermost axis with something in them.
    Groups(Vec<TensorGroup<'db>>),
}

/// One group at a tensor's outermost axis, parsed.
#[derive(Clone, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct TensorGroup<'db> {
    /// Its shape, which the first group's is the shape every other has to be.
    pub shape: Vec<u32>,
    pub elements: Vec<ast::ExprFull<'db>>,
    /// What a group of the wrong shape is reported at.
    pub span: TextSpan,
    /// Where its diagnostics end in the chunk's, so that the complaint about
    /// its shape, which only the parse that put the chunks together can make,
    /// goes after them as it would in a parse of the whole.
    pub diagnostics_end: u32,
}

impl<'db> ChunkItems<'db> {
    pub(super) fn into_exprs(self) -> Vec<ast::ExprFull<'db>> {
        match self {
            ChunkItems::Exprs(exprs) => exprs,
            _ => bug!(),
        }
    }

    pub(super) fn into_entries(self) -> Vec<ast::ExprMapEntry<'db>> {
        match self {
            ChunkItems::Entries(entries) => entries,
            _ => bug!(),
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
    /// What a tensor's chunk reports of its delimiters, which a parse of the
    /// whole reports for every group before it parses any.
    pub prelude: Vec<StoredDiagnostic>,
    /// In the order they were made, for the parse that asks to file.
    pub diagnostics: Vec<StoredDiagnostic>,
    /// Whether the chunk's own level reported an error, which a parse of the
    /// whole collection would carry into the chunks after it. A table's or a
    /// tensor's level is the parser that reached the collection's, so it
    /// carries there too.
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
    let mut prelude = Vec::new();
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
        CollectionKind::Table => {
            // The column count every row is checked against, as the header
            // makes it; what reading the header reports is the parse of the
            // whole collection's to file, so it is put aside here.
            let (from, to) = planned.header.X();
            let header = cursor.between(from, to);
            let scratch = std::mem::replace(&mut parser.sink, Some(Rc::new(RefCell::new(Vec::new()))));
            let had_error = parser.had_error;
            let columns = parser.split_table_cells(header).len();
            parser.sink = scratch;
            parser.had_error = had_error;

            let first = planned.chunks[chunk as usize].first as usize;
            let rows = bct::split::Lines::new(cursor).enumerate()
                .map(|(i, row)| {
                    let elements = parser.parse_table_data_row(row);
                    if elements.len() != columns && !elements.is_empty() {
                        let ts = parser.extract_group_text_span(row);
                        bct::diagnostic::DiagnosticBuilder::error(db,
                            &format!("row {} has {} columns but header has {}",
                                first + i + 1, elements.len(), columns))
                            .code("D030")
                            .primary_label(ts, "column count mismatch")
                            .emit_to(&parser.sink);
                    }
                    ast::ExprTableRow { elements }
                })
                .collect();
            ChunkItems::Rows(rows)
        }
        CollectionKind::Tensor => {
            let groups = bct::split::split_commas(cursor, planned.rank as usize - 1);
            parser.report_stray_delimiters(&groups, "parts");
            let last = chunk as usize + 1 == planned.chunks.len();
            if let Some(written) = bct::split::trailing_delimiter(&groups).filter(|_| last) {
                parser.had_error = true;
                bct::split::trailing_delimiter_error(db, source, &written, "parts")
                    .code("D033")
                    .emit_to(&parser.sink);
            }
            prelude = std::mem::take(&mut *kept.borrow_mut());
            let groups = bct::split::nonempty_groups(groups).into_iter()
                .map(|group| {
                    let (shape, elements) = parser.parse_tensor_multicomma(group, planned.rank - 1);
                    TensorGroup {
                        shape,
                        elements,
                        span: parser.extract_group_text_span(group),
                        diagnostics_end: u32::try_from(kept.borrow().len()).X(),
                    }
                })
                .collect();
            ChunkItems::Groups(groups)
        }
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
        prelude,
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
        let parts = self.chunk_parts(index);
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

    /// A planned table, whose bracket this parser has just passed and whose
    /// contents are `inner`: the header read here, and the rows from the chunks.
    pub(super) fn parse_planned_table(&mut self, index: usize, inner: Cursor<'db>) -> ast::Expr<'db> {
        let planned = &self.plan[index];
        let (from, to) = planned.header.X();
        let header = self.parse_table_header(inner.between(from, to));
        let mut rows = Vec::new();
        for part in self.chunk_parts(index) {
            for diagnostic in &part.diagnostics {
                emit_stored(self.db, &self.sink, diagnostic.clone());
            }
            self.had_error |= part.had_error;
            self.expr_spans.extend(part.spans.iter().cloned());
            let ChunkItems::Rows(more) = &part.items else { bug!() };
            rows.extend(more.iter().cloned());
        }
        ast::Expr::Table(ast::ExprTable { header, rows })
    }

    /// A planned tensor, whose bracket this parser has just passed and whose
    /// contents are `inner`, put together as `parse_tensor_expr` makes one:
    /// every chunk's delimiter complaints first, then each group's
    /// diagnostics and, if its shape is not the first group's, the complaint
    /// about that.
    pub(super) fn parse_planned_tensor(&mut self, index: usize, inner: Cursor<'db>) -> ast::Expr<'db> {
        let rank = self.plan[index].rank;
        let (header, _) = bct::split::split_tensor_header(self.text(), inner);
        let header = match header {
            None => None,
            Some(Ok(header)) => Some(header),
            Some(Err(_)) => panic!("a tensor whose header is wrong is not planned"),
        };
        let parts = self.chunk_parts(index);
        for part in &parts {
            for diagnostic in &part.prelude {
                emit_stored(self.db, &self.sink, diagnostic.clone());
            }
            self.had_error |= part.had_error;
        }
        let mut inner_shape: Option<&Vec<u32>> = None;
        let mut elements = Vec::new();
        let mut group_idx = 0;
        for part in &parts {
            self.expr_spans.extend(part.spans.iter().cloned());
            let ChunkItems::Groups(groups) = &part.items else { bug!() };
            let mut filed = 0;
            for group in groups {
                for diagnostic in &part.diagnostics[filed..group.diagnostics_end as usize] {
                    emit_stored(self.db, &self.sink, diagnostic.clone());
                }
                filed = group.diagnostics_end as usize;
                match inner_shape {
                    None => inner_shape = Some(&group.shape),
                    Some(expected) if *expected != group.shape => {
                        bct::diagnostic::DiagnosticBuilder::error(self.db,
                            &format!("inconsistent tensor shape at group {}: expected {:?} but got {:?}",
                                group_idx, expected, group.shape))
                            .code("D023")
                            .primary_label(group.span.clone(), "shape mismatch")
                            .emit_to(&self.sink);
                    }
                    Some(_) => {}
                }
                elements.extend(group.elements.iter().cloned());
                group_idx += 1;
            }
            for diagnostic in &part.diagnostics[filed..] {
                emit_stored(self.db, &self.sink, diagnostic.clone());
            }
        }
        let mut shape = vec![group_idx as u32];
        if let Some(inner) = inner_shape {
            shape.extend(inner.iter().copied());
        }

        let Some(header) = header else {
            return ast::Expr::Tensor(ast::ExprTensor { shape, elements, header: false });
        };
        let written = crate::parser_util::tensor_body_complaint(&header.extents, rank as usize, &shape, elements.len());
        if let Some((message, label)) = written {
            let ts = TextSpan::new(self.source_text(), header.span);
            return self.emit_expr_error(ts, &message, "D041", &label);
        }
        ast::Expr::Tensor(ast::ExprTensor { shape: header.extents, elements, header: true })
    }

    /// Every chunk of the `index`th planned collection.
    fn chunk_parts(&self, index: usize) -> Vec<&'db ChunkParse<'db>> {
        let collection = u32::try_from(index).X();
        (0..self.plan[index].chunks.len())
            .map(|chunk| parse_chunk(self.db, self.source_text(), collection, u32::try_from(chunk).X()))
            .collect()
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
