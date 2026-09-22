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

| | Before | After |
|---|---|---|
| lex | 15.58ms | **4.58ms** |
| lex + brace | 15.55ms | 5.07ms |
| lex + brace + parse | 23.86ms | **14.91ms** |

So before: lexing 15.6ms, bracing under half a millisecond, parsing proper 8.3ms.
Lexing was two thirds of the front end, and 12 MB/s -- slow enough for a lexer that
something had to be wrong rather than merely unoptimized.

End to end, `datalove script` on a trivial script went from 98.8ms to **66.3ms**, and
repl startup from 96.8ms to **67.9ms**. Lexing is now 40 MB/s.

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

## What is left, and what it would cost

**Every token's text is allocated and then interned.** `InternedText::new(db, S(&text[range]))`
builds a `String` and hashes it into salsa, once per token, for whitespace and
punctuation as much as for identifiers. In the profile after the four fixes:

| | |
|---|---|
| `FxHasher` | 5.7% |
| salsa `Configuration::execute` | 5.5% |
| `Tokenizer::intern` | 3.3% |
| `String` | 2.5% |
| `malloc` + `free` | 4.9% |
| **total** | **~22% of lexing** |

A `Token` already carries `span: Range<usize>` into the chunk, so its text is
recoverable without interning at all. Interning presumably buys `Token: Copy` and
cheap keyword comparison, so dropping it is a change across the parser rather than a
local one -- but it is the largest single item remaining, and most of what it interns
is punctuation nobody compares by name.

**`peek` is still 8.6%.** A char-at-a-time lexer that re-slices and decodes UTF-8 per
character. A byte cursor with an ASCII fast path would help, and would restructure
the tokenizer.

**Bracing is under half a millisecond** for 183KB. Nothing to do.

**Parsing proper is now about 10ms of the 15ms**, and has not been profiled in
detail. It is the next place to look if the front end still matters after the
interning question is settled.
