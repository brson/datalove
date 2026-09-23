# Profiling the Parser

The front-end report ends by saying parsing is the largest phase left and has not been
looked at. This is that. Same instrument -- `crates/datalove-bench/benches/frontend.rs`,
`parse` case -- and the same corpus, **24 modules, 187,832 bytes**.

The cases nest, so `parse` includes lexing and bracing. Parsing proper is `parse` minus
`brace`, which was about 10ms of the 13.7ms.

## What the profile said

Nothing was hot. The flat profile had a long tail and no single item above 6%, which is
usually a sign there is nothing cheap left. It was not: the same cost was spread across
many symbols, because the same mistake was made in several places.

Three quarters of the parser's own time went on **moving tokens around and asking salsa
for things it had already been given**, rather than on parsing.

## The bracer's iterator asked salsa for the same four vectors per token

`BracerIter::next` walks four sequences in step -- the tokens, the branches, and two
lists of closing braces the recovery inserted or removed -- and it read every one of
them back out of salsa on each call:

```rust
let tokens = &self.tree.chunk(self.db).tokens(self.db)[0..self.real_token_range.C().end];
let branches = &self.tree.branches(self.db)[0..self.branches.C().end];
let inserted_closes = &self.tree.inserted_closes(self.db)[0..self.inserted_closes.C().end];
let removed_closes = &self.tree.removed_closes(self.db)[0..self.removed_closes.C().end];
```

Five tracked-struct field reads -- the chunk, its tokens, and three of the bracer's own
-- to advance by one token. In the profile that was `Table::get_raw<Value<Bracer>>` at
6.1% plus `Bracer::inserted_closes`, `branches`, `chunk`, `removed_closes`,
`IngredientCache::get_or_create`, `ChunkLex::tokens` and `Table::get_raw<ChunkLex>`,
which together came to about **15%**.

They are `#[returns(ref)]` fields, so a `&'db` borrow of each outlives any iterator over
them, and the sub-iterator a branch hands out shares the same four. Reading them once in
`Bracer::iter` and carrying the slices is **1.15x**, and every one of those symbols
disappears from the profile.

This is the same shape as the bug the front-end report opens with, where the lexer's
`peek` went through salsa twice per character. Worth naming as a pattern: a `#[salsa::tracked]`
field read looks like a field access and is a table lookup.

## The parser cloned every token twice to keep one integer

`TokenStream::next` was 5.2%:

```rust
let token = tokens.get(*pos).cloned();
if token.is_some() {
    *pos += 1;
    *last_token = token.C();     // second clone
}
```

`last_token` exists so that `prev_end()` and `last_byte_end()` can answer where the
previous token ended -- and those are its only two readers, and both of them want
`token.span().end`. So a whole `TreeToken` was cloned on the way past to keep one
`usize`. It is `last_end: Option<usize>` now, in both this parser and datalit's.

Beside it, `eat_name` was re-interning text the lexer had already interned:

```rust
token.word_str(self.db()).map(|word| InternedText::new(self.db(), word.S()))
```

`word_str` resolves the token's `InternedText` back to a `&str` through salsa's interned
table, `.S()` allocates a `String` of it, and `InternedText::new` hashes that to arrive
at the value the token was already holding. It returns `token.text`. Together: **1.04x**.

## A `TreeToken` was 264 bytes and a `Token` is 32

```
Token        32
TreeToken   264
BracerIter  184
```

`TreeToken` is either a token or a brace group, and the brace group carried its
sub-iterator by value. An enum is as big as its largest variant, so **every token in the
stream was 264 bytes** -- eight times what the token in it needs -- and the parser moves
and clones them constantly: `split_lines` builds a `Vec<TreeToken>` per line, the parser
holds one, `peek_owned` clones out of it, and the two-slot lookahead buffer shifts them.

Boxing the sub-iterator takes `TreeToken` to **88 bytes** for no extra allocation in the
common case, since a branch is rare and a token now carries nothing. **1.07x**, and
`Option<&TreeToken>::cloned` drops out of the profile.

88 is still not 32. The rest is `open: Token` and `close: Option<Token>`, and boxing the
whole branch payload would reach about 40 -- one allocation per branch, the same count as
now. It was not done: it turns the struct variant into a tuple variant and so touches all
59 `TreeToken::Branch` sites, and the shrink before it bought 1.07x, so the next 2.2x of
shrink is unlikely to be worth that diff.

## One that looked obvious and measured as nothing

`BracerIter::next` had ten `debug!` calls dumping its internal indices per token, and
`log::debug!` is not compiled out in release here -- no `release_max_level` feature is
set -- so each was a live atomic load, a compare and a branch, eleven per token,
splitting the function into basic blocks the optimizer then cannot merge across. That is
a good story and it predicted a real win.

The first measurement agreed: 9.83ms against 9.05ms, three rounds, consistent.

Interleaved properly it is **nothing** -- six alternating rounds gave a median of 9.05
against 9.10, with the two distributions fully overlapping. The first reading was machine
drift; the absolute numbers across runs of this same binary ranged from 8.9ms to 10.9ms
during the session.

The standing lesson from the front-end report held again, and cost a measurement to
relearn: **alternate the variants within one run and compare medians**. A block of A
followed by a block of B measures the machine as much as the code.

Those logs are gone now, but for their own reasons rather than this one: they were
scaffolding from when the bracer was being written, they have not been used in a long
time, and ten lines of internal indices per token is not output anyone can read. The
per-chunk dump of the whole brace map went with them. The one in `dbglex` stays, since it
is `#[cfg(test)]` and naming the input still helps when a bracer test fails.

So the count above is unchanged by the removal: parsing is 1.42x from the three sections
before this one, and nothing from this one.

## Where it ended up

Four alternating rounds, `fastest`:

| | before | after |
|---|---|---|
| lex | 3.47ms | 3.52ms |
| lex + brace | 3.75ms | 3.87ms |
| lex + brace + parse | 13.70ms | **10.90ms** (1.26x) |
| parsing proper | 9.95ms | **7.03ms** (1.42x) |

`lex` and `brace` are untouched and should be: nothing here changed the lexer or the
bracer query, only how their output is read.

End to end, `datalove script` on a trivial script, four alternating rounds comparing
minima: 77.9ms against 72.2ms, about **1.08x**. The variance there is wide -- one round
of four gave 75.7ms for the new binary against 78.7ms for the old -- so that figure is
worth less than the bench numbers and is quoted with that caveat.

## What is left

**`split_lines` resolves every whitespace token through salsa** to ask whether it
contains a newline:

```rust
TokenKind::Whitespace if t.text.as_str(db).contains('\n') => Some(Delimiter::Newline),
```

That is an interned-table lookup and a string scan for each of the corpus's 23,623
whitespace tokens, to recover something the lexer knew while it was scanning those exact
bytes. `TokenKind::Whitespace` appears in only six places, so carrying the flag on the
variant is a contained change. It is worth perhaps 1% and was left.

**Lexing is now 21% of the `parse` profile** -- `peek` 5.9%, the tokenizer loop 5.1%,
`basic_source_map` 4.5%, hashing the token vector 3.2%, and so on. That is the phase the
front-end report covers, and a byte cursor for it was written, measured at 1.48x on `lex`
alone, and judged not a big enough win to keep.

**`BracerIter::next` is still the largest parser-side item** at about 10%, now that it is
only doing arithmetic. Per token it computes four slice bounds, takes four `Option`s,
builds a four-tuple and matches on it. A state machine that tracked which of the four
sequences can produce the next item would do less, but the four-way match is also what
makes the recovery rules legible, and they are subtle.

**Salsa allocation for AST nodes is about 3%** (`ExprFun` tracked-struct allocate,
`ZalsaLocal::allocate`, `ExprKey::of`), and general allocation about 4.4%. Neither has
been looked at.

# Startup, Revisited

Coming back to where the time goes in a cold `datalove` invocation, after the parser
work above.

| | |
|---|---|
| `datalove --help`, so process start | 2.9ms |
| `script --no-sys` on a trivial script | 3.6ms |
| `script` on the same script | 60.5ms |
| `repl --script` on an empty script | 58.8ms |

So **the standard library is 55ms and everything else is under a millisecond**, which is
what it was before and is the only thing worth looking at.

## The profile said nothing, loudly

Compiling the library has no hot spot at all -- the largest single symbol is 2.2%. It is
not concentrated in parsing, typechecking, resolution or lowering. Aggregating the flat
profile by defining crate, and counting the unsymbolized `libc.so.6` addresses, which are
malloc internals and `memcpy`:

| | |
|---|---|
| `alloc`, `[malloc]`, unsymbolized libc, `hashbrown`, `std` | **~40%** |
| `bcts` + parser + datalit (the front end) | ~17% |
| `salsa` + `boxcar` | ~10% |
| tycheck, lower, ownership, ir, ast, compiler | ~13% |

Compiling is allocation-bound, and there was no `#[global_allocator]`: everything went
through glibc malloc, which is not built for this shape of work. rustc ships jemalloc for
this reason.

## mimalloc on the cli, measured and not kept

Four alternating rounds, 50 runs each:

| | glibc | mimalloc |
|---|---|---|
| repl startup, min | 54.9ms | **41.1ms** |
| repl startup, mean | 56.4ms | 42.4ms |
| `script`, min | 55.4ms | **41.6ms** |
| user time | 43.2ms | 37.4ms |
| **system time** | **12.9ms** | **4.5ms** |

**1.33x**, and far more consistent between rounds than anything else measured this
session -- the four rounds land within 0.3ms of each other on both sides.

The system time is the interesting column: it falls by 2.9x. Most of what glibc malloc
was costing here was not CPU in the allocator, it was returning memory to the kernel and
faulting it back in. Binary size goes up 146KB, 0.4%.

A C toolchain is needed to build mimalloc, which costs nothing here: the AOT backend
already shells out to `cc`.

**This is not in the tree.** It was an experiment, it passed the full suite, and it was
backed out on the call that the allocator is not the thing to change right now. The
numbers are kept because they size the opportunity, and because the system-time column
says something about the workload that is true whoever allocates for it.

## It does not extend to the test suite, and why that matters

`std_all_tests` is 108s wall with **70s of system time**, which is the same signature at
scale, so the allocator looked like a large win there too. It segfaults instead.

The cause is worth more than the speedup: the rider path builds a `cdylib` and `dlopen`s
it, that library carries its own copy of `datalove-rt` and its own Rust allocator, and
the two copies share `AllocLocal`'s `HashMap` on the heap. Today both resolve to glibc
malloc so it works by coincidence. Written up in `botdocs/issues.md`; the cli is
unaffected because it links riders in rather than loading them.

## Where startup ended up

What is committed is the parser work, so a cold start is 58.8ms for the repl and 60.5ms
for a trivial `script`, down from the 63-65ms the front-end report last recorded. The
allocator would take both to about 41ms, and is available whenever it is wanted.

Next, in order of what the profile now owes: allocation is ~40% and the allocator is the
one-line version of addressing it; the front end is ~17%, and a byte cursor for the lexer
was written and measured at 1.48x on `lex` before being set aside; salsa's own bookkeeping
is ~10%.

Separately, and often confused with the above: the *runtime* allocator is not part of
that 40%. It is written up in `botdocs/reports/report-runtime-allocator.md`, which found
that it does mmap directly for payloads but keeps a leak-tracking hash map on the Rust
heap that is enabled by default in release and costs 17% of an allocation-heavy program.
