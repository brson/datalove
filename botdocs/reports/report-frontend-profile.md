# Profiling the Front End

Lexing, bracing and parsing, measured on the standard library, which is what every
datalove invocation compiles before it looks at the program.

The instrument is `crates/datalove-bench/benches/frontend.rs`. The phases nest and
each is salsa-memoized on the one below, so the cases are cumulative -- `brace`
includes `lex`, `parse` includes both -- and a phase on its own is a subtraction. A
fresh database and fresh `Source` inputs are built per iteration outside the timer;
reusing them would measure a memo hit.

Corpus: **24 modules, 187,832 bytes**.

## Where the time went

At the start: lexing 15.58ms, bracing under half a millisecond, parsing proper 8.3ms,
for a total of 23.86ms. Lexing was two thirds of the front end, and 12 MB/s -- slow
enough for a lexer that something had to be wrong rather than merely unoptimized.

Two rounds of work follow. The first four fixes took lexing to 4.58ms and the whole
front end to 14.91ms; then interning took them to 2.82ms and 10.95ms. The table at the
end has the final numbers.

## Four things were wrong

**`peek` went through salsa twice per call.**

```rust
fn peek(&self) -> Option<char> {
    self.chunk.text(self.db).as_str(self.db)[self.range.C()].chars().next()
}
```

Two salsa field reads, a slice and a UTF-8 decode, for one character -- and `peek`
is what every other method in the tokenizer is built on. The tokenizer already held
`chunk_text`, and `intern` used it, so the two paths disagreed about where the text
came from and `peek` took the slow one. It holds a `&'db str` now.

**`eat_char` asserted its way to a second peek per character.**

```rust
fn eat_char(&mut self, ch: char) {
    assert!(self.peek() == Some(ch));
```

An `assert!`, not a `debug_assert!`, so it ran in release. Every caller had just
peeked the character it was passing, so every character consumed cost two peeks
instead of one. `eat_word` and `eat_whitespace` peek in their loop condition and
then call `eat_char`, which peeked again.

**`is_sigil_start` asked all seventy-one sigils for their first character, per
character classified.**

```rust
fn is_sigil_start(ch: char) -> bool {
    enum_iterator::all::<Sigil>().map(|s| s.start_char()).any(|c| c == ch)
}
```

And `start_char` is `self.as_str().chars().next()`, so that is seventy-one `as_str`
calls and seventy-one UTF-8 decodes to answer one yes-or-no question, for every
character the tokenizer looked at. It is one array index now.

**`eat_sigil` tried all seventy-one against every sigil token.** `Sigil::as_str` was
7.5% of the time spent lexing on its own. Sigils are now bucketed by the byte they
start with, so a sigil token tries the one to four that could match rather than all
of them.

That last one had a trap worth naming. The old scan took the **first match in
declaration order**, and the enum declares the three-character sigils before the
two- and one-character ones they begin with, so `+?=` is found before `+`.
Longest-match was an emergent property of the variant order. Bucketing by first byte
keeps declaration order within each bucket, and a sigil can only match if its first
byte matches, so the candidates are the same list in the same order and the
behaviour is identical. Reordering the enum would still break the lexer, which is
now written down where the table is built.

## Two things that looked wrong and were not

`char::is_alphanumeric` and `char::is_whitespace` walk Unicode property tables, and
datalove source is overwhelmingly ASCII, so an ASCII fast path in `is_word_start`
and the whitespace checks looked free. It measured as **no change at all** -- 4.581ms
against 4.604ms, inside the noise. The 6% the profile attributes to `is_word_start`
is the call and the branch, not a table walk.

Worse, it would not have been free: `char::is_ascii_whitespace` does not accept
vertical tab and `char::is_whitespace` does, so the fast path would have quietly
reclassified `\x0B`. Both changes were reverted.

## Interning, which was the largest item left

**A census first**, because the answer depended on it. The standard library's 24
modules are 61,952 tokens:

| kind | count | bytes |
|---|---|---|
| word | 22,689 | 98,936 |
| **whitespace** | **23,623** | 38,138 |
| **sigil** | **14,597** | 15,115 |
| comment | 1,038 | 47,845 |
| string | 5 | 28 |

So **62% of everything interned is whitespace or punctuation**, and between them they
hold a few dozen distinct strings: the sigils are drawn from at most seventy-one
spellings, and the whitespace runs from the handful a formatter emits. Salsa
deduplicates them, but only after hashing each occurrence -- so it was being asked
38,220 times to be told one of about eighty answers.

Two changes, measured separately because the second is a cache in front of a cache
and has to earn that.

**The allocation was pure waste.** `InternedText::new(db, S(&text[range]))` built a
`String` for every token before interning it. Salsa's interning takes a key that only
has to be `HashEqLike` with the stored field, and only calls the assembler on a miss,
so `InternedText::new(db, &text[range])` interns straight from the slice and allocates
once per *distinct* string rather than once per token. One line.

**Sigils need no hash at all**, since the token kind already says which sigil it is: an
array indexed by `Sigil as usize` answers 14,597 of the chunk's lookups by index.
Everything else goes through a per-chunk `FxHashMap<&str, InternedText>`, which still
costs a hash but reaches salsa's sharded concurrent map once per distinct string in
the chunk instead of once per token.

Interleaved twice, since the differences are near the noise and single readings had
already misled once:

| | round 1 | round 2 |
|---|---|---|
| neither | 4.161ms | 4.185ms |
| sigil array only | 3.321ms | 3.419ms |
| both | **3.005ms** | **3.010ms** |

The array is 1.24x and the memo a further 1.12x, so both stay.

## Where it ended up

| | Original | Now |
|---|---|---|
| lex | 15.58ms | **2.82ms** (5.5x) |
| lex + brace | 15.55ms | 3.05ms |
| lex + brace + parse | 23.86ms | **10.95ms** (2.2x) |
| `datalove script`, trivial script | 98.8ms | **64.8ms** |
| repl startup | 96.8ms | **64.7ms** |

Lexing went from 12 MB/s to 65 MB/s.

## What is left

**A `Token` still carries an `InternedText` it may not need.** Every token already has
`span: Range<usize>` into the chunk, so its text is recoverable without interning at
all; interning presumably buys `Token: Copy` and cheap keyword comparison. With the
allocation gone and the common cases indexed, what remains is one hash and a probe per
non-sigil token, which is no longer the largest thing in the profile. Removing it
entirely would be a change across the parser rather than a local one, and should be
measured against the current numbers rather than the original ones.

**`peek` is the largest single item now.** A char-at-a-time lexer that re-slices and
decodes UTF-8 per character. A byte cursor with an ASCII fast path would help, and
would restructure the tokenizer.

**Parsing proper is about 8ms of the 11ms** and has not been profiled in detail. It is
now the front end's largest phase by a wide margin, and the next place to look.

**Bracing is a quarter of a millisecond** for 183KB. Nothing to do.
