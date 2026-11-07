# Documentation Fixtures and Session Replays

How the documents show what the engine does, and how they stay true as it
changes. Explanation and how-to; written November 2026, when the first replay,
in the reactive scripting post, went in.

## Contents

- [Why a crate of its own](#user-content-why-a-crate-of-its-own)
- [Session replays](#user-content-session-replays)
- [The trace](#user-content-the-trace)
- [The player](#user-content-the-player)
- [Step tables in a replay](#user-content-step-tables-in-a-replay)
- [Adding a replay](#user-content-adding-a-replay)
- [Known gaps](#user-content-known-gaps)

## Why a crate of its own

A document that shows the engine's output goes stale silently when the output
changes. So the output is recorded by a test from a fixture, and the test
fails when what the document holds is not what the engine produces now.
`BLESS=1` records it again.

Those fixtures are not tests of any one crate: they are the documents', and
a fixture often needs a library of its own to run. So they live together in
`crates/datalove-doc-fixtures`, not beside whichever crate's tests happened to
need one. Everything a document in `mandocs` or `botdocs` shows that the
engine produced belongs there:

- `reactive/`, the step tables
  [script-reactivity-cases.md](script-reactivity-cases.md) is rendered from,
  run by `tests/reactive_tests.rs`.
- `replays/`, the sessions the posts play, run by `tests/replay_tests.rs`.

**Both are checked by step tables** (`src/step_tables.rs`): for each step, what
every unit should do, what it uses, and what it holds after. A table states
the rule, an `engine` table after it writes down where the engine does more
work than the rule asks, and the test fails if the engine does anything else.
The tables are never blessed, because they are the rules. Every step, with a
table or without, is also checked against a fresh run: a new engine runs the
session's current units in order, and each unit must end the same way and
hold the same bindings.

## Session replays

A post about the REPL wants to show a session changing over time. Embedding
the engine as wasm would mean publishing a large binary with every post, and a
terminal recording shows only what fit on the screen. A Datalove session is
deterministic, so instead the session is recorded once, as everything the REPL
would show after each input, and the page plays the recording back with a
small player that knows nothing of the engine.

```
replays/<name>.world          the session, and the modules and data it requires
        | datalove repl --load <world> --record <path>, or the test
        v
mandocs/assets/<name>.replay.js     the trace, registered by name
mandocs/assets/datalove-replay.js   the player
```

**A replay is one worldfile.** It is a session as `/save` writes it --
`scriptunit-fragment`, `scriptunit-expr` and `repl-command` sections in the
order they were typed -- with the `module`, `data` and `rider` sections of the
library it requires beside them, which become the session's library as they
do for `script-world`. So the session and everything it reads are in one file
the document can be checked against, with no workspace on disk to keep beside
it. `datalove_repl::read_world` reads one; `load_session`, for `repl --load`
without `--record`, still refuses the library sections, since the interactive
REPL takes its library from the workspace it starts in.

## The trace

`datalove_repl::record_world` (`crates/datalove-repl/src/record.rs`) builds an
engine over the worldfile's library, compiling its modules from the start as
`script-world` does, and refusing a worldfile whose modules do not compile.
It runs each input through a `Session` and after each one takes:

- the input, and the `Outcome` it came to, which the History tab shows;
- the session's units, each with its number, source, status, the outcome it
  last came to, whether the last step compiled and ran it, and its
  `UnitDetail`, which the code panel and the Detail tab show;
- the environment.

Each step holds the whole session, not a change to the one before it, so the
player can show any step without replaying the ones before it, and going back
costs nothing. The detail repeats from step to step; for the budget example
that comes to about 120KB, which is not worth compressing.

The statuses are the app's rule (`status_of` in `app.rs`) applied to the last
thing each unit came to. The last step's evals give that, since the session
keeps the last step until another replaces it.

The trace has a `version`, which the player checks. The enums are serde's
external tagging of the Rust types, `{"Ran": [...]}`, `"Reset"`, and so on,
so changing one of those types changes the format: bump `TRACE_VERSION` in
`record.rs` and `datalove-replay.js` together.

**Recording goes through `Session`, not `ReplApp`.** The session is
synchronous and can describe every unit directly. The app would give the
history's own rules and `/undo` and `/redo`, but it describes only the
selected unit, asking the engine on another thread. So a replay uses
units and `/edit`, `/edit-data`, `/delete`, `/insert` and `/move`; an `/undo`
in one records a command error.

## The player

`mandocs/assets/datalove-replay.js` is plain JavaScript with no dependencies. It
injects its own styles, uses the page's `--fg`, `--code-bg` and
`--font-code` if they are set, and highlights code with Prism if the page has
the `datalove` language. A page loads it, then the trace, and marks where the
player goes:

```html
<div class="datalove-replay" data-replay="reactive-transactions"></div>
<script src="../assets/datalove-replay.js"></script>
<script src="../assets/reactive-transactions.replay.js"></script>
```

The trace is a `.js` file calling `DataloveReplay.register(name, trace)`
rather than JSON the player fetches, so a page opened from the disk plays
too. `DataloveReplay.mount(element, trace)` puts a player in an element
directly.

It shows the code panel with each unit's bar and corner marks, the next input
with Back and Run buttons (and the arrow keys, Home and End while it has
focus), and the Environment, History and Detail tabs, choosing the tab and the
selected unit after each step as the REPL does. The ANSI colours of the
diagnostics are turned into spans, as `ansi.rs` does for the terminal.

**It differs from the REPL in one place:** the REPL's history forgets what
came before a reset, and the player marks the reset and keeps it, so a replay
can be read from its start.

**The trace files sit in `mandocs/assets`** with the other scripts and
images, and not beside the page that plays them. `datalove docs` copies
everything in each doc set's `assets` directory as a static asset.

## Step tables in a replay

A replay is checked against the rules as well as recorded, so a change to the
language breaks its test rather than quietly changing what the post shows. Its
worldfile may hold `expect` sections, and `engine` sections after them, as the
reactive fixtures do. An `expect` checks the inputs since the `expect` before
it, which are either units appended -- the script entered at the start, say
-- or one `/edit`, `/edit-data`, `/delete`, `/insert` or `/move`. Inputs after
the last `expect` have no table, though each is still checked against a fresh
run. Units are named by the numbers the session gives them.

The test takes the tables out before it records the session, so they are not
in the trace, and `datalove repl --load` refuses a worldfile that still has
them.

## Adding a replay

1. Write the session as a worldfile in
   `crates/datalove-doc-fixtures/replays/`, with the modules and data it
   requires. A session saved at the REPL with `/save` is a start.
2. Add a `Replay` naming it, its player name, and its output, to `REPLAYS` in
   `crates/datalove-doc-fixtures/tests/replay_tests.rs`. A test fails on a
   worldfile that is not listed.
3. Put an `expect` table after the steps worth checking; see
   [Step tables in a replay](#user-content-step-tables-in-a-replay).
4. Run `BLESS=1 cargo test -p datalove-doc-fixtures`.
5. Put the `div` and the two `script`s in the document.

To look at a trace by hand, run
`datalove repl --load <world> --record <path>`, which writes it as plain
JSON.

## Known gaps

- A replay shows what each unit did, and not why. The reasons are in its step
  tables' `because` column, which is written by hand, is not checked, and is
  not in the trace.
- The REPL has `/edit-data` and no command to edit a module, so a replay can
  show a data file changing under a session but not a module. The reactive
  fixtures can, through their `module-edit` steps.
- A unit's source is shown whole; there is no diff of what an edit changed.
