# Parsing Large Collection Literals in Parallel

A plan for compiling a large collection literal on more than one core, in
datalit and in datafun alike: split it at the token tree into chunks, parse the
chunks as queries of their own on rayon, and do it only where a tuning bench
says it pays. Written October 2026, after a prototype for datalit lists.
Phases 0 to 2 are done; see [Progress](#user-content-progress).

## Contents

- [The problem](#user-content-the-problem)
- [What the prototype showed](#user-content-what-the-prototype-showed)
- [Scope](#user-content-scope)
- [The design](#user-content-the-design)
  - [Finding the collections](#user-content-finding-the-collections)
  - [The policy](#user-content-the-policy)
  - [Chunks in datalit](#user-content-chunks-in-datalit)
  - [Chunks in datafun](#user-content-chunks-in-datafun)
  - [Running the chunks](#user-content-running-the-chunks)
  - [After the parse](#user-content-after-the-parse)
- [Proving it changes nothing](#user-content-proving-it-changes-nothing)
- [The cutoff bench](#user-content-the-cutoff-bench)
- [Phases](#user-content-phases)
- [Progress](#user-content-progress)
- [Open questions](#user-content-open-questions)

## The problem

`DATALOVE_PARALLEL` spreads the compile across modules, so a program whose
work is one big literal gets nothing from it. The store demo is that program:
it compiles for about 205ms, almost all of it its 4MB of data, 20,000 orders in
one list in `orders.dlt`, and the parallel compile is slower than the
sequential one (225ms; see [On real programs](compile-time.md#user-content-on-real-programs)).

But a big literal is almost always a collection of independent elements, and
that is the shape that splits. What makes this a tuning problem rather than a
transformation is that it rarely pays: most collections are a handful of
elements, splitting one costs a database clone, a query and a rayon task per
chunk, and work on a worker thread costs more than the same work on the main
one (see [Parallel compilation](compile-time.md#user-content-parallel-compilation)).
So it has to be off below a size that is measured, per kind of collection,
rather than guessed.

## What the prototype showed

A prototype for a datalit file whose top level is one list (since reverted;
the design below is its generalization) took the store demo's compile from
218ms to 154ms on four cores, and from 152ms to 115ms with huge pages, with
the report unchanged. Chunking cost the sequential compile a few ms at most.
The parse went about 2.3x on four threads rather than 4x, the allocator again.

From a cold sequential profile of the store demo, what the data costs, of a
670ms process:

| Step | Query | Cost |
|---|---|---:|
| Lexing | `bcts::lexer::lex` | about 22ms |
| Bracket matching | `bcts::bracer::bracer` | a few ms |
| Parsing | `datalit::parser::parse` | about 113ms |
| Typing against the declared type | `type_check_with_expected` | about 20ms |
| Reading values for the IR | `data_file_value` (`Reader::read`) | about 52ms |

With the parse spread out, the main thread's compile is mostly the reader
(12% of the run's samples) and the lexer (5%).

## Scope

**Collections only, and only the outermost one that qualifies.** A collection
is what a bracket of these kinds opens, in either language:

| Sigil | Collection | Separator |
|---|---|---|
| `[` | list | `,` |
| `%{` | map | `,` between entries |
| `#{` | set | `,` |
| `{\|` | table | `;` or a newline between rows, after a header row |
| `[\|` | tensor | runs of `,`, one fewer than the rank; none at rank 1 |

Structs and tuples are left out: their fields are named or positional and few,
and splitting them buys nothing. A qualifying collection nested in one already
chosen is parsed inside its chunk, whole. Tables need their header in every
chunk, since a row is read against it. Tensors come last: a chunk boundary has
to fall on a separator of the highest rank, and a rank-1 tensor has no
separator at all, its elements being written side by side.

**The parse first, in both languages; what follows it only in datalit.** In
datalit the typing and the reading split the same way and are where the time
goes after the parse ([After the parse](#user-content-after-the-parse)). In
datafun a big literal goes on to typecheck, ownership and lowering, all of
which would want the same treatment; that waits for a real program that needs
it, since data that big belongs in a `.dlt`.

## The design

### Finding the collections

**The bracer already says where every bracket ends**, so a level of the token
tree is walked without looking inside its brackets: `Cursor::next_index` steps
over a whole `{...}` in one move. So finding the candidates and their chunk
boundaries is a pass over the token layout, before any parse, and it is the
same pass for both languages. It goes in `bcts`, next to the bracer:

- `collection_plan(lexed, policy) -> Vec<PlannedCollection>`, tracked. Each
  entry is the token index of a collection's open bracket, its sigil, and its
  chunks as `(from, to)` token positions of its level, each starting just past
  a separator, which is what `Cursor::between` takes.
- A candidate is a bracket of a collection sigil whose byte span and top-level
  separator count clear the policy's thresholds for that kind. Chunks are cut
  by bytes rather than by elements, at the first separator past each
  `target_chunk_bytes`, so that uneven elements still make even chunks.
- A source with bracket errors gets an empty plan, so that recovery, and what
  it reports, is exactly what it is today.

The plan cannot know the grammar: a `[` may be a list, or a type, or an index.
The parser decides, by consulting the plan only where it is about to parse a
collection literal, by the open bracket's token index. A planned bracket that
turns out not to be a literal is parsed as it is today, and the chunks warmed
for it are wasted; the thresholds make that rare. The cost of the plan pass
itself goes in the bench, since every source pays it.

### The policy

`ChunkPolicy` is a salsa input, one per database, that the pipeline sets:

- per collection kind, `min_bytes`, `min_elements` and `target_chunk_bytes`,
  with defaults from [the cutoff bench](#user-content-the-cutoff-bench);
- **disabled** under `ParallelMode::Sequential`, and wherever rayon's pool has
  one thread, as on wasm, so that a sequential parse is the parse of today and
  pays nothing for any of this.

Being an input, changing it invalidates the plans and nothing else, and the
parse queries do not need it passed down to them: `parse(source)` keeps its
signature, and every caller of it -- `data_file_type`, `data_file_value`,
`datalit_spans` -- is unchanged. A test or a CI configuration that wants every
collection chunked sets a policy with tiny thresholds.

### Chunks in datalit

As prototyped. `parse_chunk(source, open, chunk)` is a tracked query that runs
the parser a list already makes for its contents (`sub_parser_from_branch`)
over `inner.between(from, to)` and returns the chunk's elements and spans. The
parser, at a planned collection, assembles the elements out of the chunks'
memos in order.

**The numbering needs no renumbering.** Every `ExprFull` has a number from a
counter shared down the parse, and the number is in its hash, but it is only
ever a key: `DatalitSpans::lookup` finds a span by it and nothing indexes by
it. So each chunk numbers its expressions in a range of its own, above the
whole parse's, and they never collide. Whether to number every parse that way,
so that the same text numbers the same whether or not it was chunked, is an
open question.

### Chunks in datafun

The same plan, the same thresholds and the same warm-up, but a harder merge,
because datafun numbers densely. A body's expressions are an arena
(`ExprArena.exprs`, children before parents) indexed by `ExprId`, and later
phases keep per-expression tables as vectors in the same order (`BodyTable`).
Calls are numbered the same way, and statements have counters of their own.

So a datafun chunk parses into an arena of its own numbered from zero, and the
merge **relocates** it: appends its expressions to the body's arena and adds
the offset to every `ExprId` they hold, to the keys of its spans, and to its
call numbers. That wants a `map_ids` over `ExprFunKind` that matches every
variant exhaustively, so that a new variant cannot be forgotten by it.
Relocating is a pass over what the chunk made, which the bench measures against
the parse it replaces.

A chunk whose elements use anything numbered per body other than expressions
and calls -- a statement counter, which a block or a function literal in an
element would -- says so, and the parser parses that collection inline
instead. That keeps the merge to two kinds of number, at the cost of chunking
less; how often it happens is something to count before doing more.

A chunk parser is built with the parser state a collection's elements are
parsed with: the module and function they belong to, which only feed keys the
merge rewrites, and the parser's mode. Whether anything else in `Parser`
(`branch_context`, the script counters) changes how an element parses rather
than what it reports is to check in phase 4; if it does, it is part of the
chunk's key.

### Running the chunks

A chunk is a tracked query, and the parallel part runs them ahead of time on
database clones, outside any query, the pattern every phase already uses (see
the compiler guide's
[Parallel Execution](compiler-guide.md#user-content-parallel-execution)). The
parse that reaches a planned collection then finds every chunk in its memo.
This keeps diagnostics and dependencies where salsa tracks them, keeps
parallelism out of queries, as upstream salsa asks, and on wasm, where rayon
runs the warm-up inline, it is a sequential parse in pieces -- which the policy
turns off anyway.

The warm-up is shared, in the compiler crate, and runs in two places:

- **Module sources**, in the dependency walk that already parses each
  frontier on rayon (`extract_dependencies_with_mode`). A frontier's chunks go
  in the same batch as its modules, chunks first: a module parse that reaches
  a chunk another worker is still parsing blocks on it, which salsa handles.
  The plan needs the module lexed and braced, which is the start of its parse
  anyway.
- **Data sources**, before typechecking, for every source in `graph.data`.

### After the parse

In datalit, the two passes after the parse are queries over the whole list,
and with the parse spread out they are most of what is left:

- **Reading** splits the same way and is the larger: a chunk's elements read
  into a `Vec<ConstValue>` by a query keyed on the source, the chunk and the
  element type, warmed in phase 5 before `data_consts`, and `data_file_value`
  concatenating them.
- **Typing** splits when there is an expected type: each chunk's elements
  checked against the element type, and the errors put together in order. A
  list with no expected type infers its element type from all of them, which
  does not split.

**Lexing does not split at the token tree**, since the tree is built from the
tokens. Splitting the text at line starts would work only where a piece cannot
begin inside a string or a comment, which the lexer cannot yet say. It is 22ms
of the store demo and left alone here.

## Proving it changes nothing

The existing tests do not exercise any of this: their collections are all
below any sensible threshold. So equivalence is a suite of its own, run with a
policy that chunks everything:

- **Generated collections of every kind, in both languages**, with errors placed
  inside chunks and on each side of a boundary, trailing separators, empty
  collections, and qualifying collections nested in qualifying ones.
- **Each parsed chunked and unchunked and compared**: the trees structurally,
  ignoring datalit's numbers; the spans as a set of byte ranges; the
  diagnostics as a list, in order, since a chunked parse reports a chunk's
  diagnostics in chunk order and has to report them in the order a sequential
  parse would.
- **Downstream** as well: for datalit, the types and the values; for datafun,
  the typecheck and the lowered IR.

And the whole suite run once with that policy, as `just test-parallel` runs it
with `DATALOVE_PARALLEL`, so that a parse a fixture never meant to chunk is
compared too. The store demo's `just check` under it is the end-to-end test.

## The cutoff bench

The thresholds are numbers the bench produces, not ones the plan picks. It
goes in `datalove-bench`, beside `par_phases`, as `collection_cutoffs`:

- **Inputs**, generated: for each collection kind, elements of three shapes
  (a scalar, a small record like a store order, a nested collection), at sizes
  from 4KB to 16MB in doublings; in datalit as a data file, and in datafun as a
  module-level const.
- **Configurations**: unchunked (the baseline), chunked sequential (the pure
  overhead), and chunked parallel on 1, 2, 4 and all threads, each in a rayon
  pool of its own, at `target_chunk_bytes` from 16KB to 1MB in doublings.
- **Stages**: the plan pass alone; the parse; for datalit, the parse and the
  typing, and all three with the reading.
- **Each measurement cold**, a fresh database per iteration built outside the
  timer, the median of enough iterations to get under 2%, with and without
  `glibc.malloc.hugetlb=1`, since the allocator decides much of the result.

What it reports, per kind, stage and thread count: the smallest size at which
the best chunked configuration beats the baseline by a margin (5% to start),
the chunk size that gives that best, and the overhead of the plan pass on a
source with no collection worth chunking. Those are the policy's defaults. The
results go in [compile-time.md](compile-time.md) when they are taken, so that
retuning after a change to the parser or the allocator is a diff of a table.

## Phases

0. **The bench, before any chunking.** Generators, the baseline of every kind,
   stage and size, and the plan pass on its own. This says what a byte of each
   kind costs to parse, type and read, and whether anything but big datalit
   lists is worth going on to.
1. **The plan and the policy in `bcts`, and datalit lists**: the chunked parse
   as prototyped, the policy input, the equivalence suite for lists, and the
   bench's chunked configurations for them. Set the list thresholds.
2. **The other datalit collections**: maps, sets and tables, then tensors.
3. **Datalit's reading and typing in chunks**, warmed where they are asked for.
4. **Datafun**: `map_ids`, the relocating merge, the fallback for chunks that
   number statements, the module-source warm-up, and the equivalence suite for
   datafun. Only if phase 0 says a datafun literal big enough to matter is
   something anyone writes.
5. **Tuning and CI**: thresholds from the bench for every kind built, and a CI
   job that runs the suite with everything chunked.

Each phase pays only under `DATALOVE_PARALLEL`, which is not yet the default;
the allocator question ([Page faults](compile-time.md#user-content-page-faults))
decides how much each one is worth.

## Progress

**Phase 0**: `collection_cutoffs` in `datalove-bench`, and its baseline in
[compile-time.md](compile-time.md#user-content-large-collection-literals). The
parse is 59-98% of every case in both languages, and it costs more per byte as
the collection grows, from interning every number.

**Phase 1**: `bct::chunk` (the plan, the policy, the thresholds),
`datalit::parser::parse_chunk` and `Parser::parse_planned` in
`crates/datalove-datalit/src/parser/chunk.rs`, the warm-up for data files
(`warm_data_chunks` in `crates/datalove-datafun-compiler/src/compile.rs`), and
lists, maps and sets in datalit; `crates/datalove-datalit/tests/chunk_tests.rs`
is the equivalence suite. The defaults are measured, and the store demo
compiles in 138ms parallel, from 225ms. Where it came out other than as
designed above:

- **A collection is cut in every mode**, not only under `DATALOVE_PARALLEL`:
  the policy cannot see the mode, which is per compile while the policy is per
  database, and an input set per compile would need `&mut` where the pipeline
  has `&`. What depends on the mode is only the warm-up. So the thresholds
  are where the chunks cost a sequential parse nothing, which the bench shows
  they do from 32KB, and one parse in every mode is an equivalence for free.
- **There is no policy by default.** A database without one uses
  `DEFAULT_THRESHOLDS`, or `CUT_EVERYTHING` under `DATALOVE_CUT_COLLECTIONS`,
  which is how the whole suite runs with every collection cut. Tests and the
  bench create a policy when they want another.
- **A chunk's diagnostics are kept, not filed.** Salsa returns a query's own
  accumulated values before its dependencies', so diagnostics filed by the
  chunk queries would come after everything the parse that reached the
  collection filed itself, wherever they were in the source. A chunk keeps its
  diagnostics as `StoredDiagnostic`s, through a sink its sub-parsers share, and
  `parse_planned` files them where the collection is. The datalit parser asks
  `SeenNames::take_reported` for its duplicate-name diagnostic, rather than
  having `take` file it, for the same reason.
- **A chunk boundary is checked, and may be given up.** An element that does
  not parse can take the separator after it as part of its mistake (`[1,, 2]`),
  and then a whole parse goes on from somewhere the chunks do not. Each chunk
  says whether it ended by passing its last separator; if one did not, the
  collection is parsed whole. Only a collection with an error gets there.
- **Chunks grow with the collection**: at least `chunk_bytes`, and about
  `target_chunks` of them, since hundreds of small ones made the store demo
  slower in both modes than a few dozen bigger ones.
- **Numbers have gaps**: `span_key_tests` asserted that a parse numbers its
  expressions from 0 without one, which a chunked parse does not, by design.
  It asserts that for an uncut parse now, and only uniqueness for a cut one.

**Phase 2**: tables and tensors in datalit, with their own cases in
`chunk_tests`. A table's chunks are runs of rows, cut where `split::Lines`
would split them, so a boundary cannot be taken by a mistake the way a comma
can, and needs no fallback; a chunk reads the header's column count from the
plan's record of where the header is, and numbers its rows from the plan's
`first`. A tensor's chunks are runs of groups at its outermost axis, cut where
`split::split_commas` would; its rank comes from the plan, since a chunk's own
commas may not show it. A tensor chunk keeps its delimiter complaints apart
(`prelude`), which a whole parse makes for every group before parsing any, and
marks where each group's diagnostics end, so that the complaint about a group
of the wrong shape, which only the parse putting the chunks together can make
against the first group's, goes where a whole parse would put it. A table's or
tensor's delimiter complaints set `had_error` on the parser that reached it,
so a chunk's carries back there. A rank-1 tensor is not cut. The thresholds
differ from lists'; see [compile-time.md](compile-time.md#user-content-large-collection-literals).

## Open questions

- **Thresholds per kind, or one?** A list of records and a set of ints cost
  different amounts per byte; whether that is enough to want separate
  thresholds is what the bench is for.
- **Should chunking depend on the thread count?** Cutting by bytes makes the
  chunks a function of the source and the policy alone, so the same input
  parses the same way on every machine. Cutting into a few chunks per thread
  would suit the pool better but makes the parse machine-dependent, which is
  harmless to the output if the equivalence suite holds and awkward to debug.
- **Interning.** Each chunk's parser starts its interning cache empty, so a
  field name is interned once per chunk rather than once per file. Whether
  that costs enough to want a shared, pre-interned table is for the bench.
- **Edits.** An edit to one element changes the source, so every chunk of it
  parses again, as the whole file does today; and a chunk after the edit has
  moved spans, so it does not backdate. Keying chunks on their text rather than
  their position would let the untouched ones be memo hits, at the cost of
  hashing the text.
- **Numbering in datalit.** Number every parse in per-collection ranges, so
  that chunked and unchunked parses of the same text agree, or only the
  chunked ones?
- **Are datafun literals worth it at all?** Phase 0 answers this before phase
  4 is started.
