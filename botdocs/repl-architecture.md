# REPL Architecture

How the REPL is put together, and the decisions that shaped it. Explanation;
describes the REPL as of October 2026.

Every line typed at the REPL is a script unit, compiled and run by an `Engine`
on a worker thread, and the terminal UI talks to that engine only through
requests and the responses keyed to them. The engine keeps the session as a
list of units that can be edited, spliced, saved and replayed. A unit that
fails stays in the list. The modules compiled are the ones the units require,
re-settled on every step. How an edit reaches the units that depend on it is
[script-reactivity-architecture.md](script-reactivity-architecture.md).

## Contents

- [Crates](#user-content-crates)
- [The request/response shape](#user-content-the-requestresponse-shape)
- [The engine](#user-content-the-engine)
- [What a line does](#user-content-what-a-line-does)
- [Editing, not only appending](#user-content-editing-not-only-appending)
- [Saving and loading a session](#user-content-saving-and-loading-a-session)
- [`script-world` runs through the engine](#user-content-script-world-runs-through-the-engine)
- [When there is no engine](#user-content-when-there-is-no-engine)
- [The terminal UI](#user-content-the-terminal-ui)
- [Tests](#user-content-tests)
- [Known gaps](#user-content-known-gaps)

## Crates

```
datalove-repl-rat/      Widgets, rendering, and the terminal that drives them
  src/lib.rs            RatatuiApp: ReplApp plus a TextArea
  src/render.rs         The three-panel layout, history, menu, crash and engine modals
  src/term.rs           Terminal setup, event loop, key bindings

datalove-repl/          Engine and the UI-agnostic app
  src/lib.rs            Input classification, the wire types, load_session
  src/engine.rs         Engine: compiles and evaluates; run_source
  src/app.rs            ReplApp state machine, ReplExecutor trait
  src/executor_threaded.rs  The one executor: a worker thread owning an Engine
```

`datalove-cli` calls `datalove_repl_rat::run()` for the interactive REPL and
`Engine::run_script` for `repl --script`. Both take the system library, and
the workspace's own `local` library if it has one, from the caller; see
[The engine](#user-content-the-engine).

## The request/response shape

Input flows one way and results come back keyed by id:

```
submit_input(text) -> Input -> submit_parse(id) -> InputParse
                                     |
                            (if Command) submit_eval(id) -> Eval + environment
```

`ReplExecutor` abstracts where that happens. `ThreadedExecutor` puts the
engine on a worker thread so the UI stays responsive. Everything crossing the
boundary is `Serialize`/`Deserialize`.

**Results are matched to history entries by id, not by position.** The app
pushes an entry when input is submitted, which can be before the previous
entry's evaluation has answered. So "the last entry" is not necessarily the
entry a result belongs to, and someone typing during an evaluation would
otherwise get the result attached to the wrong line. Results for entries a
crash reset cleared are dropped.

Executors are constructed by the caller and handed to `ReplApp`, which is
what lets the tests drive the state machine with a scripted mock.

## The engine

`Engine::new(sys)` takes a `SystemLibrary` rather than going looking for one.
`Engine::with_local` adds the workspace's own library,
`Engine::with_options` also says where `debuglog` goes and which modules to
compile regardless, and `Engine::in_database` takes a database the caller made,
which is how a test records what salsa runs. The library carries the addresses
of the native rider functions alongside the sources. They become a
`RiderNatives` given to both the pipeline, for consts, and the executor, so a
session can `require module sys/std/string` and call into it. The engine keeps
the pipeline, the descriptor and the natives, so a crash reset rebuilds the
compiler and executor from them without building or loading a rider again.

`ThreadedExecutor::spawn(sys, local)` takes `fn() -> SystemLibrary` instead of
a value. The native addresses are raw pointers and so not `Send`, and the
worker thread builds its own library. `datalove-cli` passes
`datalove::sys_packages::system_library`, which is the copy embedded in the
binary; see [compiler-guide.md](compiler-guide.md) under "The Shipped Binary".

**The roots follow the units.** Before every step -- an append, an edit, a
splice -- `Engine::with_compiler` narrows the pipeline's roots to what the
session's units will require once the step is done, plus the caller's
`also_roots`, and compiles. A unit adding `require module sys/std/u8` brings
`u8` into the compile, and truncating it away takes `u8` back out. When the
module set moves, the executor is given the new module registry and the
natives the new modules call before anything runs. The units already lowered
are left alone, which is sound because a module's `IrModuleId` is given by
path and kept for the pipeline's life.

**A required module that does not compile is left out**, with every required
module that reaches it, and the rest of the roots stay (`settle_roots` in
`crates/datalove-repl/src/engine.rs`). The unit that requires it fails on its
`require`, and the engine replaces that error with the module's own. One
broken module costs only the units that require it, as one failed unit costs
only the units that read it. So the engine holds modules with errors outside
the compile, and keeps what they said in `Engine::module_errors`. It starts
whatever its modules do. The one error no root's leaving can fix is a rider
interface's, and that still stops it starting.

`ModuleErrors` has the failing modules' messages by path (`broken`), the
lowering errors, and the type diagnostics in their stored form,
`StoredDiagnostic`. That form names its `Source`s but holds no database
lifetime, so a caller in the same process renders them against `Engine::db`.
A caller across a thread, as the interactive REPL is, has only the messages,
for now.

`Engine::run_source` is the one loop over a `---`-separated script. Both
`repl --script` and the fixture tests use it, so the driver and the tests
cannot drift.

## What a line does

Evaluation reports what a fragment defined by reading the compiled unit's
`script_context().exports` -- values, slots, and functions -- rather than by
re-scanning the source text. All four error phases are checked, ownership
included: an ownership error leaves lowering `Skipped`, so checking only
lowering would let a use-after-move inside a fragment through as a successful
binding.

**Every line is a script unit, and follows the language's ownership rules
across lines** as inside one. `let b = a` moves `a` out of the unit that bound
it, and a later line that names `a` is D013. Naming a binding as a bare
expression shows it without taking it. Units do not copy out of earlier
bindings, both to keep script units as close to the language as possible and
because copying stops being available once there are types that cannot be
cloned. [compiler-guide.md](compiler-guide.md) under "Ownership across units"
has how this is checked. Re-running a unit after an edit rewinds to the
earlier bindings it took or wrote; see
[script-reactivity-architecture.md](script-reactivity-architecture.md#user-content-5-the-reach-goes-back).

**A line that fails stays in the session** unless it fails with a runtime
error. A line that does not compile reports `Eval::Error`. One that returns
early through `!` or `?` reports `Eval::ReturnedEarly`. One that reads from
either is compiled and not run, reporting `Eval::Blocked`. Each stays as a
unit offering no bindings, so the history and the engine's units line up one
to one for script inputs, and an edit upstream can make any of them go green.
The app shows all three as errors and clears nothing.
[script-reactivity-architecture.md](script-reactivity-architecture.md#user-content-units-that-did-not-run-to-their-end)
has how the layers keep a failed unit's names from the units after it.

**A runtime error empties the session.** What the frames the error unwound
owned is not destroyed a value at a time. The runtime discards all its memory
instead (`IrInterpreter::discard_runtime_state`), and every binding the
session had was in it. The executor drops its units, and the engine starts its
compiled session again and reports `Eval::ErrorReset`, which the app treats
like a crash reset. Re-running the earlier units to bring the session back
was considered and not done: a unit that fails again would loop.

A panic inside the engine, while parsing or evaluating, is caught and reported
as `CrashReset`. The engine rebuilds its compiler and executor, and the app
clears the history and shows the crash modal.

`/reset` asks for the same reset on purpose. The engine resets before it
answers, so the environment that comes back with the answer is already empty.
The app then drops only the history entries up to and including the
`/reset`. Anything typed while the reset was running was evaluated in the new
session, so it stays.

## Editing, not only appending

`Engine::edit_unit(unit, source)` changes one line of the session and
re-derives what the change reaches, leaving the rest of the session with the
lowering and the frames it had. `Engine::truncate_units`, `remove_unit` and
`insert_unit` splice the list and re-derive the whole suffix. A unit of the
suffix that stops compiling stays, as one appended does.
[script-reactivity-architecture.md](script-reactivity-architecture.md) has how
the reach is decided, and why the engine owns its database rather than
borrowing it.

`Engine::edit_module(library, package, module, source)` does the same from a
module edit. Its seeds are the edited module, the modules that require it directly or not, and
any module the edit broke or fixed. The units importing from those are
re-derived, and then the units reading what those compute. The broken-or-fixed
seed is what the import edge alone would miss: a module that leaves the compile
because one it requires broke is in no graph to be found from.

**A module edit that does not compile is kept.** The roots are settled again,
so the module leaves the compile with the modules requiring it, and the units
reached fail saying why. An edit that fixes the module brings it and them
back. A module no unit requires is not compiled, so an edit to one reports
nothing until something requires it. Adding and removing modules mid-session
is not offered.

## Saving and loading a session

`/save` writes the history to `repl-session.world` in the workspace root,
which is the directory the REPL was started in, the one it looks for `local`
in. Script units are `scriptunit-fragment` and `scriptunit-expr` sections,
and REPL commands are `repl-command` sections. `repl-command` exists only for
this. `repl --load <worldfile>` replays a saved history before the first
prompt.

**What is saved is the app's history, not the engine's session.** The
engine's session holds the script units. The history also has the slash
commands and whatever a reset took out of the session. So the app writes the
file when the engine hands `/save` back to it as `CallerInterpret`. A few
things are left out:

- Anything typed after the `/save`. Those lines can still be waiting on a
  parse, while everything before it has been answered, because the engine
  answers in order.
- The line that only opened a multiline input. The input it opened holds it
  again.
- Empty inputs.
- Whatever a reset already took out of the history.

An app given no path with `set_save_path`, as in the tests, answers `/save`
with an error.

**A load replays through the app,** so the inputs show up in the history as
though they had been typed. `ReplApp::replay` sends each one straight to
evaluation rather than through parsing. The file already says what each input
is, and parsing a `fun` on its own would ask for more lines. A failure fails
again and a command runs again. The exception is `/save`, which `load_session`
drops: replaying one would overwrite the file being loaded with only the part
of it read so far. The file is read before the terminal is set up, so a bad
one is reported on the ordinary screen. A worldfile with module, data or
rider sections is refused.

## `script-world` runs through the engine

`datalove script-world` runs a worldfile's script units in order through one
`Engine`, so a worldfile behaves the way the same units typed at the REPL
would. A unit that fails to compile, returns early, or is blocked on one that
did stays, and the units after it still run. A runtime error ends the run.
It refuses `repl-command` sections, which are `repl --load`'s.

Two things about how the CLI drives it:

- **The worldfile's own modules are compiled from the start.** They are
  roots whether or not a unit requires them, so `Engine::with_options` is
  given them as `also_roots`. If any does not compile, the CLI renders the
  engine's `module_errors` and runs nothing: an error in a module the author
  wrote is an error in the worldfile. Library modules the units require join
  as the units come, as at the REPL.
- **Diagnostics are rendered from inside the eval.** A compiler's diagnostics
  are those of the last unit it compiled, and the engine lets each compiler go
  when its operation is over. So `Engine::eval_reporting` takes a callback
  that is handed the compiler while it is still alive.

**Locations are the worldfile's lines.** Each section is a source of its own,
so a diagnostic counts its lines from the section. The parser records how many
lines of the file come before each section (`ParsedWorldfile::line_offsets`).
The CLI maps each section's `Source` to that, and gives the map to the
renderer (`bct::render::LineOffsets`). The renderer puts that many empty lines
in front of the text it hands ariadne and moves the spans to match. Ariadne's
own `with_display_line_offset` would not do, because it moves the line number
in a frame's header and not those in its gutter. A unit's source is made when
it is compiled, so its entry is added in the `eval_reporting` callback, from
`last_source`. Sources from one worldfile all show as that file.

## When there is no engine

Everything above assumes the engine started. When it does not, the app has to
say so, because the request/response shape hides the failure completely: input
is submitted, an entry is pushed as `Parsing`, and a response never comes.
Startup can fail on a rider interface error, or on building the riders when
`DATALOVE_BUILD_SYS_RIDERS` asks for them to be built rather than taken from
the binary.

So the worker reports its lifecycle. `start_engine` catches both a failed and
a panicking startup and sends `WorkerResponse::EngineDead { message }`;
success sends `EngineReady`. `ThreadedExecutor::try_recv_response` also turns
a channel disconnect into `EngineDead`, once, since reporting it repeatedly
would spin `poll_results`. So a worker that dies mid-session cannot wedge the
UI either. The renderer shows "starting engine..." until ready and an "Engine
Gone" modal after, and `run()` prints the message again once the terminal is
restored, so it survives in the scrollback.

## The terminal UI

The screen has three panels: history, environment and input. `run()` redirects
stderr to `datalove-repl.stderr` in the temp directory while the terminal is in
raw mode, and a `TerminalGuard` restores the terminal on the way out, panics
included.

### Keys and focus

One of the three panels has focus, and the input panel has it at the start.
Tab moves focus down through the panels and Shift+Tab moves it up; both wrap
around. The focused panel has a white border and the others dark gray. The
input panel stays yellow while it has focus in multiline mode. Focus lives on
`ReplApp` as a `Panel`, alongside the menu state.

Typing only reaches the textarea while the input has focus. In single-line
mode Enter submits. In multiline mode Enter inserts a newline and Alt+Enter
submits. Ctrl+D exits, and Esc opens a menu of Resume and Exit. Neither works
while the menu or a modal is open: those take the keys first, the menu Up,
Down, Enter and Esc, and a modal only Enter.

### Scrolling

The scroll keys are Up/Down, which move a line, and PageUp/PageDown, which
move a page. `ReplApp::scroll_target` decides where they go. When History or
Environment has focus, that pane takes the plain keys and the other pane takes
them with Ctrl. The single-line input has no use for them, so it lends the
plain keys to History and the Ctrl ones to Environment. Multiline input keeps
all of them, Ctrl included, for moving its cursor, so neither pane scrolls
from there until focus moves.

The history pane follows the newest output, which is where a session spends
nearly all of its time. So its scroll position is held as a distance *up from
the bottom* rather than down from the top. Zero needs no maintenance as
entries arrive, and submitting resets it: someone who submits wants to see
what it did, wherever they had scrolled to read. The environment scrolls down
from its first row.

Only the renderer knows how tall a pane is or how many lines its contents came
to, and both change every frame. It returns them as a `HistoryView`, which is
what `record_history_view` bounds the scroll against, so paging past the
oldest entry lands on it. Both panes are a `ScrollState`, clamped when a key
is pressed rather than on the next frame. The offsets live on `ReplApp`, out
of the widgets, which is what lets `app_tests` page around with no terminal at
all.

## Tests

`crates/datalove-repl/tests/`:

- **engine_tests** -- the `.repl` fixtures in `fixtures/engine/` through
  `Engine::run_source`, snapshotting the parse, eval, and environment after
  every input as JSON. They run against
  `datalove_sys_packages::system_library()`, so the suite covers the library
  and the linked riders the binary actually ships. `BLESS=1` updates them.
- **engine_edit_tests** -- the edit path: `edit_unit`, `edit_module` and the
  splices, the units each reaches, and the environment afterwards; modules
  joining, leaving and breaking; and units that did not compile, parse, or run
  to their end staying and going green. Separate from `engine_tests` because
  that one is `harness = false` and has a `main` of its own, so `#[test]`
  functions cannot live beside it. The reach itself is measured in
  `datalove-datafun`'s `script_exec_reactivity_tests` and
  `script_scenario_tests`; these say the engine wires it up.
- **reactive_tests** -- the step tables in `fixtures/reactive/` against the
  engine, and [script-reactivity-cases.md](script-reactivity-cases.md) against
  the tables. `BLESS=1` re-renders the doc.
- **session_tests** -- `/save` and `--load` through the app and a real
  engine on its worker thread: a saved history replays to the same outcomes
  and environment, and saving it again writes the same file.
- **app_tests** -- the state machine against a scripted `MockExecutor`, with no
  engine at all, which is how the interleavings are reachable: a result
  arriving after a later input, a crash reset, the multiline round trip, the
  engine reporting itself ready or dead, scrolling the history, and what
  `/save` writes.

Neither the rendering nor the key bindings have tests. `render.rs` would take
ratatui's `TestBackend`; `term.rs` has no seam, since `run()` does terminal
setup and the loop together.

## Known gaps

- Multiline input opens on a leading `fun`, `if`, `match`, `loop` or `for`,
  and reads to its `end`. A brace-delimited input cannot be typed across
  lines: `is_open_brace_tree` is hardcoded false. See
  [issues.md](issues.md#user-content-the-repl-never-reads-a-brace-delimited-input-over-several-lines).
- No cancellation. `WorkerRequest::Shutdown` is never sent, and an infinite
  loop in evaluated code wedges the worker with no interrupt key.
- `/exit`, `/help`, `/reset` and `/save` are the only commands, and help
  prints the word "help".
- Errors are plain strings with no rendered diagnostics, and type errors are
  their `Debug` form, though `ScriptCompiler` exposes the spans needed for
  better. See
  [issues.md](issues.md#user-content-the-repl-prints-errors-as-rust-values).
- Mouse capture is enabled while only key events are read, which disables
  terminal text selection and drops the mouse wheel for no benefit.
- Nothing scrolls a pane while the menu or a modal is open.
- An edit to a module no unit requires reports nothing, since only required
  modules are compiled; see
  [issues.md](issues.md#user-content-an-edit-to-a-module-no-unit-requires-reports-nothing).
