# Plan: Parse Error Recovery

How the datalit and datafun parsers came to recover from errors by rules a
test can hold them to. Finished November 2026; the rules are the Recovery
sections of [Datalit Grammar](datalit-ebnf.md#recovery) and
[Datafun Grammar](datafun-ebnf.md#recovery). This is the
reasoning behind them.

## Contents

- [The measurement](#the-measurement)
- [What it found](#what-it-found)
- [What was decided](#what-was-decided)
- [What was weighed](#what-was-weighed)
- [What is left](#what-is-left)

## The measurement

Every clean source the grammar tests read is changed a character at a time,
and what the parser reports of each change is read against the grammar's
Earley recognizer, which says where the grammar first cannot go on
(`Recognizer::viable_prefix`): the earliest any parser of the grammar could
see an error. `crates/datalove-doc-fixtures/tests/recovery_tests.rs` has
`recovery_contract`, which holds the rules, `diagnostics_are_well_formed`,
and `report`, which prints the measurement by hand:

```bash
DATALOVE_LEAK_CHECK=panic cargo test --release -p datalove-doc-fixtures \
    --test recovery_tests report -- --ignored --nocapture
```

`DATALOVE_RECOVERY_SEED` and `DATALOVE_RECOVERY_COUNT` give how many changes
`recovery_contract` makes of each source; three by default, and it was run at
forty.

## What it found

The first run, over about 9,400 changes, found the diagnostics well formed and
datalit's first error where the grammar stops. Datafun's was there 96% of the
time, and one change could make dozens of errors:

- Errors pointed past the bad token: P002 at the end of `fun g@`, and the
  type faults when datalit's type parser ran on datafun's tokens.
- A stray line in a module was P077 and P001 both, and a broken function
  header left its body at the top level: one change made 51 errors.
- A broken header lost its block; a broken `end` left its block to run to the
  end of the source, and the blocks around it with it.
- An unclosed bracket or string took the `end` lines after it, and the block
  they closed reported it was never closed.
- A bracket the bracer could not close was reported closed when its contents
  ended with a close of a bracket inside it (`TreeToken::Branch::close`), and a
  stray close passed over took its line break with it, joining two lines.
- Spans of errors in a stream with no tokens yet pointed at the start of the
  source, as P081 in `let d=0[` did.

## What was decided

- **One error for each statement**, the first in the order of the source.
  The parsers keep their diagnostics in a sink and file them once the source
  is read, through one filter, rather than each site deciding whether to
  report; a datalit literal is one statement.
- **Errors point at the token where the grammar stops**, or a construct ending
  there, or the gap before a missing token.
- **A broken header still opens its block**, and a line that begins no
  statement opens one if the `end` lines after it only match with it.
- **Blocks end by their lines**: the innermost block's `end`, an enclosing
  block's `end` (this one lost its own), an `end` naming nothing open (P093,
  new), an `else` or `case` for an enclosing `if` or `match`, or a declaration
  when the block's own `end` never comes.
- **A missing `end` an error already accounts for is not reported.**

## What was weighed

**Matching blocks before parsing**, as the bracer matches brackets, was
proposed and not done. It is only needed to tell a declaration inside a block
(P074) from a block that lost its `end` above a declaration (P034), and
datafun has no nested functions, so scanning ahead for the block's own `end`
when a declaration line comes is enough.

**At most one error for each fault** cannot be held as it stands: a fault is
not something a test can see. One change to a clean source is one fault, so
the test holds one change to errors in one statement, but for a change that
moves a statement boundary, which breaks two statements at once.

**Suppressing a missing `end` after an error in the block** can hide a second
fault: a line that begins no statement and a block that really has no `end`
make one error, not two. The other way round made two errors of one fault far
more often, a broken `end` line being the common case.

## What is left

- The bracer's recovery decides which bracket a stray close closes, and a
  close inserted inside a bracket that spans lines closes the outer one early,
  moving its lines out. The contract excuses such a change rather than holding
  the bracer to anything.
- A literal shared by both languages can still get different codes after its
  first error is dropped; the report's shared-syntax section shows them.
- Precedence and associativity are not checked: the recognizer says whether
  a source is derived, not how.
