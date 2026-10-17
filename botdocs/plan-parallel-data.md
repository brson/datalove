# Reading a Large Data File in Parallel

How the compile of a program that requires a large datalit file could use more
than one core, and what a prototype of the first step measured. Written October
2026; prototyped and reverted, not implemented.

## Contents

- [The problem](#user-content-the-problem)
- [Where the time goes](#user-content-where-the-time-goes)
- [Splitting a list at its token tree](#user-content-splitting-a-list-at-its-token-tree)
- [What the prototype measured](#user-content-what-the-prototype-measured)
- [The rest of the work](#user-content-the-rest-of-the-work)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## The problem

The store demo (`demos/store`) compiles for about 205ms, and almost all of it is
its 4MB of data: 20,000 orders in `orders.dlt`, with customers and products
beside them. `DATALOVE_PARALLEL` does not help, and makes it slower (225ms;
see [On real programs](compile-time.md#user-content-on-real-programs)): the
compiler parallelizes across modules, and all of the data hangs off the one
module that requires it, so there is nothing to spread. A data file is,
though, almost always one big list of independent records, and that is
exactly the shape that splits.

## Where the time goes

From a profile of a cold sequential run, of a 670ms process:

| Step | Query | Cost |
|---|---|---:|
| Lexing | `bcts::lexer::lex` | about 22ms |
| Bracket matching | `bcts::bracer::bracer` | a few ms |
| Parsing | `datalit::parser::parse` | about 113ms |
| Typing against the declared type | `type_check_with_expected` | about 20ms |
| Reading values for the IR | `data_file_value` (`Reader::read`) | about 52ms |

Each runs over the whole file in one query, and each of the last three walks
the same list one element at a time.

## Splitting a list at its token tree

The bracer already says where every bracket ends, so a level of the token tree
can be walked without looking inside its brackets: `Cursor::next_index` steps
over a whole `{...}` in one move. Walking the top-level list's level and
counting commas finds every element boundary for the price of one step per
element, and `Cursor::between(from, to)` makes a cursor over any run of them.
A parser over such a cursor is the parser a list already makes for its
contents (`sub_parser_from_branch`), so nothing in the grammar changes.

Two things tie a parse to one thread, and neither needs to.

- **The numbering.** Every `ExprFull` is numbered, from a counter shared down
  the parse, and the number is in its hash. But the number is only ever a key:
  `DatalitSpans::lookup` finds a span by it, and nothing indexes by it. So a
  chunk can number its expressions in a range of its own, `(chunk + 1) << 20`
  upward, without renumbering anything after.
- **The database.** A parser holds `&dyn Db`, which is not `Sync`, interns
  through it and reports diagnostics as accumulators. Rather than make the
  parser work without one, each chunk is a tracked query of its own, keyed on
  the source and the chunk's index, and the parallel part runs them ahead of
  time, outside any query, on database clones: the pattern every phase already
  uses (see the compiler guide's
  [Parallel Execution](compiler-guide.md#user-content-parallel-execution)). The
  whole-file `parse` then assembles the chunks out of their memos. This keeps
  diagnostics and dependencies where salsa tracks them, keeps parallelism out of
  queries (which upstream salsa asks for), and on wasm, where the warm-up runs
  inline, it is the sequential parse in pieces.

The prototype was three functions in `crates/datalove-datalit/src/parser/mod.rs`:

- `list_chunks(source) -> Vec<(u32, u32)>`, tracked: the source lexed and
  braced, and if its top level is one bracket-list with no bracket errors, the
  token positions of every 256th element boundary. Empty otherwise, which
  includes any list of fewer than 512 elements.
- `parse_chunk(source, chunk) -> (Vec<ExprFull>, Vec<ParseSpanEntry>)`,
  tracked: a parser over `inner.between(from, to)` with its counter at
  `(chunk + 1) << 20`, running `parse_comma_separated(parse_expr_full)`.
- `parse` asking `list_chunks` first, and when it is not empty, building the
  root `ExprList` (numbered 0) from every chunk's elements and spans.

And a warm-up in `compile_modules`, under `ParallelMode::Parallel`, before
typechecking: `list_chunks` for every source in `graph.data`, then
`parse_chunk` for every chunk of every file on rayon. `ParseSpanEntry` needed
`#[derive(salsa::SalsaValue)]` to be in a memo.

## What the prototype measured

Compile time from `script --time`, median of 11 runs, four cores; the report was
the same as `check.py`'s in every mode:

| | Compile |
|---|---:|
| Sequential, before | 205-212ms |
| Sequential, chunks in turn | 218ms |
| Parallel, before | 225-230ms |
| Parallel, chunks in parallel | 154ms |
| Sequential, huge pages | 152ms |
| Parallel, chunks in parallel, huge pages | 115ms |

So parsing alone takes 29% off the compile, or 24% with huge pages, and
chunking costs the sequential compile a few ms at most. The parse itself goes
about 2.3x on four threads rather than 4x, for the reason the rest of the
parallel compile does: allocation on worker threads (see
[Parallel compilation](compile-time.md#user-content-parallel-compilation)).

## The rest of the work

With the parse spread out, the main thread's compile is mostly what reads the
data after it, which is still one query over the whole list: `Reader::read`
is about 12% of the run's samples and lexing about 5%, and the datalit
typecheck about 3%, on one worker but on the critical path all the same.

- **Reading values** splits the same way and is the largest of them: a chunk's
  elements read to a `Vec<ConstValue>` in a query keyed on the source, the
  chunk and the element type, warmed in phase 5 before `data_consts`, and
  `data_file_value` concatenating them into the list.
- **Typing** splits the same way when the expected type is a list: each chunk's
  elements checked against the element type, and the errors put together. A
  list with no expected type has its element type inferred across all of them,
  which does not split.
- **Lexing** does not split at the token tree, since the tree is built from the
  tokens. Splitting the text at line starts and lexing the pieces apart would
  work only where a piece cannot start inside a string or a comment, which the
  lexer would have to be able to say. It is 22ms.

## Phases

1. The chunked parse as prototyped, with tests that make a list long enough to
   chunk and check that its diagnostics and spans match an unchunked parse;
   the existing tests are all of lists too short to.
2. The chunked reader.
3. The chunked typecheck, for a list with an expected type.

Each is worth measuring on its own, and each is only a win under
`DATALOVE_PARALLEL`, which [is not yet on by
default](compile-time.md#user-content-on-real-programs).

## Open questions

- Is 256 elements a chunk the right size? It made 79 chunks of `orders.dlt`, a
  few dozen per thread, which leaves room for stealing. Fewer, larger chunks
  would intern less twice, since each chunk's parser has its own interning
  cache.
- Should the numbering change for every parse rather than only a chunked one,
  so that the same text numbers its expressions the same way whether or not
  it was chunked? Nothing depends on that today.
- Is a data file the only place this shape matters? A module is a sequence of
  independent top-level items as well, and its parser has the same token tree
  to split at.
