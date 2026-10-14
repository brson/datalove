# A Parser That Reads Tokens in Place

A plan for rewriting how the datafun and datalit parsers get their tokens, so
that a parse allocates for the AST it returns rather than for the tokens it
reads. Written October 2026; not implemented. The measurements behind it are
in [Where Compile Time Goes](compile-time.md).

## Contents

- [The problem](#user-content-the-problem)
- [Where the allocation is](#user-content-where-the-allocation-is)
- [The design](#user-content-the-design)
- [What does not change](#user-content-what-does-not-change)
- [Recovery](#user-content-recovery)
- [Migrating](#user-content-migrating)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## The problem

Parsing `sys/std` -- 216KB of source, 6,390 expressions -- makes 35,726
allocations totalling 12MB: 56 bytes for every byte of source, 1.9KB for
every expression. Lexing and bracket matching together make under a thousand.
Parsing is a quarter of a cold compile, about 250 instructions per byte, and
the allocation is the largest single part of it: malloc and free, the copying
`realloc` does, and the page faults that fresh memory costs. See
[compile-time.md](compile-time.md).

The parser copies its input. Tokens are lexed once into a flat array, and then
copied, as 104-byte `TreeToken`s, into a vector per line and per group; the
parser copies them again as it reads them. Every line and every bracket gets
a parser of its own. Two attempts to trim this at the edges -- a small handle
in place of a boxed sub-iterator, and one set of span tables instead of one
per nested parser -- saved 0.7% between them. The copying is the structure,
so the structure is what changes.

## Where the allocation is

Allocations during parsing, by size, for the parse of `sys/std`:

| size | count | bytes |
|---|---:|---:|
| up to 128 bytes | 21,324 | 0.55MB |
| 129 to 1,024 bytes | 14,683 | 6.88MB |
| 1KB to 64KB | 719 | 4.62MB |

The middle row is token vectors. A `TreeToken` is 104 bytes, and a line's
vector grows to four and then eight of them: 416 and 832 bytes, twelve
thousand times. The bottom row is mostly the vectors of lines and groups over
a whole file, and their growth. The top row is mostly the AST: statement and
argument vectors, names.

How a parse gets there:

1. `lex_chunk` produces the tokens of a file, `Vec<Token>`, 24 bytes each.
   One allocation, and the one array everything else could read.
2. `bracer` matches brackets, with recovery, into a table of branches.
3. `BracerIter` walks the two, producing `TreeToken`s: a token, or a branch
   carrying the handle to iterate its contents.
4. `split_lines` collects the top level into a `Vec<TreeToken>` per line.
5. `parse_statements` makes a `Parser` per line, which owns the line's
   vector. A function body is the following lines, pulled from the same
   iterator.
6. Inside a line, each bracket the grammar enters makes a sub-parser
   (`sub_parser`) over its contents, and comma-separated contents are split
   into vectors again (`split_commas`, the tensor parser's groups).

## The design

**Tokens stay where the lexer put them.** Nothing after step 1 copies one.

**The bracer produces a match table, not a tree.** For each token, a `u32`:
for an open bracket, the index its contents end at; for a close bracket that
recovery threw away, a mark saying to skip it. The bracer's recovery
algorithm is kept as it is and only its output changes (see
[Recovery](#user-content-recovery)). It is the same length as the token array,
built in the same pass, and one more allocation.

**A cursor in place of a token stream.** A `Cursor` is the token and match
slices and a range of indices, `Copy`, a few words. Its operations are what
the parsers already ask of a `TokenStream`, without handing out tokens:

- the kind, text and span of the token at the cursor, and of the one after it;
- advancing past it, where an open bracket advances past its close;
- entering a bracket, which gives a cursor over its contents;
- the end of the last token consumed, which `prev_end` gives now.

Lookahead of any depth is free, since nothing is buffered.

**Lines are found, not collected.** A line ends where a token at the cursor's
level has `newline_before`, or at a `;`; a newline inside brackets is already
inside a branch, which the cursor steps over. A `Lines` iterator yields one
cursor per line by scanning forward at its level, so the stray-delimiter check
(P032) is part of the scan rather than a pass over collected groups.
Statement parsers that consume the lines after their own -- a function body,
an `if`'s arms, a `match`'s cases -- take the `Lines` iterator by `&mut`
where they take `remaining_lines` now. Comma-separated contents are found the
same way, by a `Commas` iterator.

**One parser per parse.** The `Parser` holds the database, the text, the span
tables, the identity counters and the cursor. Entering a bracket saves the
cursor, replaces it with the bracket's, and restores it after, checking that
the contents were used up; the enclosing branch's span, which error messages
point at (`branch_context`), is saved and restored with it. A line is the
same: the cursor becomes the line's. Sub-parsers, and merging their counters
and tables back, are gone. So is the case where a sub-parser's counters were
not merged and the next expression took the same key.

**Names are interned through a cache.** `TokenStream::intern` exists for this
and nothing caches yet: a per-parse map from the name's text to its
`InternedText` saves salsa a hash and a probe for every repeat. `sys/std`
says `self`, `ret`, `u32` and the like thousands of times.

**Tokens shrink to 12 bytes.** A `u32` start and length, the kind and the
newline flag. A source over 4GB is not a concern, and every reader of a token
reads a third of the memory.

What a parse allocates afterwards is the AST, the span tables, and the three
token-side arrays. The aim is under 10,000 allocations and 1.5MB for `sys/std`,
from 35,726 and 12MB, and parsing's instructions at least halved. Both are
targets to measure against, not predictions.

## What does not change

The rewrite is contained in `bcts` and the two parsers.

- The AST. `ExprFun`, `StmtFun` and the rest keep their shape and their salsa
  identities, so `parse_module_full`'s output -- statements, span tables,
  qualified calls -- is what it was, and nothing downstream sees a
  difference. Whether `ExprFun` should be a tracked struct is a separate
  question: it is 3.1% of a cold compile (`create_expr`), and a per-function
  arena indexed by the local index would keep the stable key, the `Copy`
  handle and the cheap equality the parse firewall needs. Not part of this.
- The salsa queries. `basic_source_map`, `lex_chunk` and `bracer` stay
  queries on a source, so an edit re-lexes the file it is in and no other.
- The diagnostics. Every message and code is produced where it is now, against
  the same spans.
- The grammar, including its line structure and `end` keywords.

## Recovery

The bracer recovers from unmatched brackets by inserting closes where an open
was never closed and removing closes that match nothing, and reports both
(P050, P051). A parser sees the result only as a branch's extent: its
contents, and `end_byte`, where its span ends whether the close was real or
not. The match table carries exactly that. An open bracket's entry is where
its contents end; whether a real close follows is whether the token there is
the matching close; a removed close is skipped by the cursor. So the bracer
keeps its algorithm and its errors, and converting its output is a matter of
writing its branches into the table rather than into a list.

## Migrating

The datafun and datalit parsers share `bcts`'s `TokenStream` and
`TokenStreamExt`, and the type-hint parser is generic over `TokenStream`, so
datafun's parser calls into code datalit owns. A `Cursor` cannot implement the
current trait, which hands out `&TreeToken`. So the shared helpers, the type
hints and both parsers move to the new trait in one change rather than one at
a time. That is about five thousand lines, nearly all of them mechanical: a
match on `TreeToken::Branch { inner, .. }` becomes entering a bracket.

What makes one change safe is checking it against the old one. Before the
cutover, a differential harness parses a corpus with both parsers and
compares everything they return: statements, span tables, qualified calls
and diagnostics, with the AST compared through `ast_serde` so that salsa ids
do not enter into it. The corpus is the parser fixtures (about 200 datafun,
160 datalit), the CLI's 96 error fixtures, every source in `sys/`, every
`.world` fixture's modules and scripts, and programs from `worldgen`, which
covers combinations nobody wrote by hand. Error inputs matter most, since
recovery is where two parsers that agree on correct programs come apart; the
harness should also run each corpus file truncated at every line, to
manufacture unclosed brackets and blocks.

The old parser stays in the tree until the harness is clean and the suites
pass, then goes in a separate commit.

## Phases

1. **The match table and the cursor**, in `bcts`, beside what exists: the
   bracer writes both its branches and the table, and a test walks every
   fixture both ways and checks that the cursor sees the same tokens,
   branches and extents the `TreeToken`s do.
2. **`Lines` and `Commas`**, checked the same way against `split_lines` and
   `split_commas`, stray delimiters included.
3. **The differential harness**, against the parser as it is, so that it is
   known to pass before anything it compares changes.
4. **The cutover**: the new trait, the type hints, both parsers, one
   `Parser` per parse. The harness and the suites pass.
5. **Removing** `TreeToken`, `BracerIter`, `split_lines`, the branch list and
   the sub-parser machinery.
6. **The name cache and 12-byte tokens**, each measured on its own.

Each phase is measured against the `sys/std` numbers above: allocations and
bytes during parsing (a counting allocator, as in
[compile-time.md](compile-time.md)), and user instructions.

## Open questions

- **Should the bracer fold into the lexer?** Both run once per source and
  re-run together on any edit, so as separate queries they are two memos for
  one piece of work. Folding them saves a memo and a pass, and makes the
  lexer responsible for recovery, which it is not now.
- **Does the REPL's line-at-a-time parsing fit `Lines`?** A script unit is
  parsed a line at a time with counters carried between lines
  (`ScriptCounters`). One parser per parse should make that simpler, but the
  script path is the one most likely to depend on how the current one splits.
- **Is the AST's own allocation worth attacking next?** After this, statement
  and argument vectors are most of what is left. Small-vector storage for
  argument lists, or the expression arena above, would be the next step, and
  should wait for the numbers this produces.
