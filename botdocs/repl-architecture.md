# REPL Architecture

How the REPL is put together, and the decisions that shaped it. Explanation;
describes the REPL as of October 2026.

Every line typed at the REPL is a script unit, compiled and run by the script
engine's `Engine` on a worker thread, and the terminal UI talks to that engine only through
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
- [Editing at the prompt](#user-content-editing-at-the-prompt)
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
  src/render.rs         The three-panel layout, code, tabs, menu, crash and engine modals
  src/term.rs           Terminal setup, event loop, key bindings

datalove-repl/          The REPL's inputs and commands, and the UI-agnostic app
  src/lib.rs            Input classification, REPL commands, the wire types,
                        eval, load_session
  src/session.rs        Session: an Engine and the numbers of its units
  src/batch.rs          run_source and run_script, over `---`-separated inputs
  src/app.rs            ReplApp state machine, ReplExecutor trait
  src/executor_threaded.rs  The one executor: a worker thread owning a Session

datalove-script-engine/ The engine, which knows nothing of the REPL
  src/lib.rs            Unit, Eval, and the bindings an Eval reports
  src/engine.rs         Engine: compiles, evaluates and re-derives units
```

**The engine is a crate of its own because the REPL is only one way to drive
it.** It takes a `Unit`, a fragment or an expression, and answers with an
`Eval`. How a unit came to be typed, that the session has slash commands, and
what the history shows are the REPL's, in `datalove-repl`. So
`datalove script-world` and the engine's own tests use the engine with no
REPL in sight, and a notebook or an embedding would too.

The REPL wraps the engine's types rather than adding to them. Its `Command` is
a `ReplCommand` or a `Unit`, and its `Eval` is the engine's `Eval` for a unit,
or what a REPL command came to. `datalove_repl::eval` is where the two meet.
It hands a unit to the engine and a command back to the app, resetting the
engine first for a `/reset`.

`datalove-cli` calls `datalove_repl_rat::run()` for the interactive REPL and
`datalove_repl::run_script` for `repl --script`. Both take the system library,
and the workspace's own `local` library if it has one, from the caller; see
[The engine](#user-content-the-engine). `script-world` uses `Engine` directly.

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
otherwise get the result attached to the wrong line. A result always has its
entry: the engine answers in order, and the history only forgets entries up
to one whose answer reset the session, by which time all of them have been
answered.

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
`crates/datalove-script-engine/src/engine.rs`). The unit that requires it fails on its
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

`datalove_repl::run_source` is the one loop over a `---`-separated script.
It reads each input as the prompt would, so it is the REPL's rather than the
engine's. Both `repl --script` and the fixture tests use it, so the driver and
the tests cannot drift.

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
compiled session again and reports `Eval::ErrorReset`. Re-running the
earlier units to bring the session back was considered and not done: a unit
that fails again would loop.

A panic inside the engine while evaluating is caught and reported as
`CrashReset`, and the engine rebuilds its compiler and executor.

`/reset` asks for the same reset on purpose. `datalove_repl::eval` resets the
engine on the worker thread before it answers, in turn with the units around
it, so the environment that comes back with the answer is already empty.

**All three resets forget the same history**: the entries up to and including
the one that reset, and no more. Anything typed while it was running was
evaluated in the new session, so it stays, and its results land. A runtime
error or a crash also opens a modal saying what happened. This matters most
for a replay, which submits every saved input before the engine has answered
any: everything after a unit that fails at run time still runs, and still
shows.

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

## Editing at the prompt

Units are edited, deleted and inserted by slash commands, so an edit is typed
like anything else and goes into the history like anything else, to be saved
and replayed:

```
/edit N <source>
/delete N
/insert before N <source>
/insert after N <source>
```

The source is whatever follows `N`, on its line and the lines after it, each
keeping its indentation. An inserted unit is a fragment or an expression as a
typed one would be. An edit keeps the unit's kind, since the engine has no way
to change it, and one that would change it is an error. So is a number no
unit has, or arguments a command does not take. None of them changes the
session.

**`N` is a number the unit is given when it is made, and keeps.** Units are
numbered from 1 as they are appended or inserted, a number is never reused,
and any reset starts again at 1. Positions were the other choice, counted
from either end, and both replay correctly, since a replay rebuilds the session
a step at a time. But the commands stay in the history to be read, and a
position goes stale there: counted from the newest unit it changes with every
line typed, and counted from the first it changes with every insert and delete
before it. A number does not, and nor does an error kept in the history
naming one. The code panel shows each unit's number in a gutter, so a command
can be typed from what is on screen. An inserted unit's number puts it out of
order there, which is the cost: the order the panel shows is the session's,
and the number only names the unit. The reactive fixtures name units the same
way, and spell an inserted one `1a`; that keeps the order readable, until two
are inserted in the same place.

**The numbers are kept beside the engine, by `Session`**, since the engine
knows units only by position. It wraps the `Engine` and carries out every
command that changes the session, turning a number into a position for the
engine and the positions in what comes back -- the units an edit reached, the
ones a `Blocked` waits on -- into numbers. It asserts after every command that
it and the engine hold the same number of units. Both the worker and
`run_source` drive one. It also catches a panic in the engine's edits, which,
unlike evaluating a unit, the engine does not catch itself, and reports it as
a crash that reset the session.

An edit, a delete or an insert answers with `Eval::Edited`: what each unit it
reached came to, by number, in the session's order. A unit that fails at run
time among them empties the session, as one appended does, and the app treats
the answer as that reset.


`/save` writes the history to `repl-session.world` in the workspace root,
which is the directory the REPL was started in, the one it looks for `local`
in. Script units are `scriptunit-fragment` and `scriptunit-expr` sections,
and REPL commands are `repl-command` sections. `repl-command` exists only for
this. `repl --load <worldfile>` replays a saved history before the first
prompt, and `/load` replays `repl-session.world` in place of the session.

**What is saved is the app's history, not the engine's session.** The
engine's session holds the script units. The history also has the slash
commands and whatever a reset took out of the session. So the app writes the
file when `datalove_repl::eval` hands `/save` back to it as `CallerInterpret`. A few
things are left out:

- Anything typed after the `/save`. Those lines can still be waiting on a
  parse, while everything before it has been answered, because the engine
  answers in order.
- The line that only opened a multiline input. The input it opened holds it
  again.
- Empty inputs.
- Whatever a reset already took out of the history.

An app given no path with `set_session_path`, as in the tests, answers `/save`
and `/load` with an error.

**A load replays through the app,** so the inputs show up in the history as
though they had been typed. `ReplApp::replay` sends each one straight to
evaluation rather than through parsing. The file already says what each input
is, and parsing a `fun` on its own would ask for more lines. A failure fails
again and a command runs again. The exceptions are `/save` and `/load`, which
`load_session` drops: replaying a `/save` would overwrite the file being
loaded with only the part of it read so far, and replaying a `/load` would
load it again, without end. For `--load` the file is read before the terminal
is set up, so a bad one is reported on the ordinary screen. A worldfile with
module, data or rider sections is refused.

**`/load` resets before it replays**, so the session becomes the one saved
rather than the saved one on top of what is there. The app reads the file when
the engine hands `/load` back, and replays a `/reset` ahead of the saved
inputs. That reset is ordered with everything else the engine is asked, and
forgets the history up to itself as `/reset` does, the `/load` included. A file
that cannot be read is an error on the `/load`, and the session is left
alone.

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

The screen has three panels: the code at the top, the input, and a tabbed
panel at the bottom. `run()` redirects stderr to `datalove-repl.stderr` in the
temp directory while the terminal is in raw mode, and a `TerminalGuard`
restores the terminal on the way out, panics included.

### Code, not a transcript

**The top panel is the session's code, not what was typed.** A REPL's usual
transcript interleaves code, slash commands, and their output, which reads as
a log of the session rather than the program it has become. The code panel
shows only the units, as their full source, with a rule between each. The
transcript is still there, as the History tab.

Each unit has a bar beside it colored for what the last step did with it:
green if it ran, red if it did not compile, yellow if it returned early,
magenta if it was blocked, and cyan while it waits on the engine. A unit the
last step neither compiled nor ran is gray, whatever it did before. So the bar
says what the last thing typed reached, which once units can be edited is the
question the panel has to answer.

**The app keeps its own copy of the session's units**, numbered as the
`Session` numbers them, and changes it as each answered command changed the
engine's: an appended unit is added and numbered, an edit, a delete or an
insert is made, and a reset empties it. The bars come from the same answers:
each step marks every unit untouched, and then the one appended, or the ones
an edit reached, with what they came to. The code is these units, then the
units on their way to the engine, which shows them without numbers since it
has not given them any yet.

**It is checked against the engine's.** Every eval answer carries the
`Session`'s units, numbers and sources. When the app's copy differs, a rule of
the app's about what a command does is wrong, which is a bug in the app. So it
is shown as one, in a modal with both lists of numbers, rather than mended
quietly, and the app takes the engine's units from then on. The session tests
fail on any mismatch, so a rule that a new command breaks shows up there
first.

**The code is the session as it will be once everything submitted is
answered**, not as the engine has it now. A unit shows from when it parses,
before it runs, so a `/reset` has to take the units before it from when it
parses too, or the units typed after it show appended to the session they will
never see. The history keeps the old units until the engine answers the reset,
since they are the engine's session until then, and `code_units` shows only
the units after the last reset still in it. Appended units and resets are the
only guesses the code makes: an edit, a delete or an insert shows once it is
answered. `/load` gets this for nothing: it is a `/reset`
and the saved units, replayed together.

**While the code panel has focus, one unit is selected**, its source in the
yellow that marks the tab shown, and Up and Down move between units. Focusing
the panel selects the newest unit, or the one selected before if it is still
there. The selection is kept by the unit's number, so units added or removed
around it leave it where it was. A reset clears it, since the new session
numbers from 1 again and the old number would land on a different unit. It is
what an edit made from the panel will act on.

### Tabs

The bottom panel has two tabs, Environment and History. F2 cycles them, and
Alt+1 and Alt+2 select one. Input selects the tab it changes when it parses:
code the Environment, a REPL command the History, which is the only place what
a command did shows.

**The keys are the ones every terminal sends.** The convention elsewhere,
Ctrl+PgUp and Ctrl+PgDn, is taken by terminal emulators for their own tabs,
and Ctrl+Tab reaches a program as itself only through the kitty keyboard
protocol; most terminals send it as a plain Tab. `run()` turns on that
protocol's escape-code disambiguation where the terminal supports it, so
Ctrl+Tab cycles the tabs there too, and the guard turns it off again. One
conflict remains: GNOME Terminal takes Alt+1 and Alt+2 for its own tabs when it
has more than one open.

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
move a page. `ReplApp::scroll_target` decides where they go. When the code or
the bottom panel has focus, that panel takes the plain keys and the other
takes them with Ctrl. The single-line input has no use for them, so it lends
the plain keys to the code and the Ctrl ones to the bottom panel. Multiline
input keeps all of them, Ctrl included, for moving its cursor, so neither
panel scrolls from there until focus moves. The bottom panel scrolls the tab
it shows, and each tab keeps its own place. The focused code panel's arrows
move the selection rather than scrolling a line, and the view moves just far
enough to show the selected unit, using where each unit was last laid out;
its page keys still scroll a page.

The code and the history follow the newest unit and the newest output, which
is where a session spends nearly all of its time. So their scroll positions
are held as a distance *up from the bottom* rather than down from the top.
Zero needs no maintenance as entries arrive, and submitting resets both:
someone who submits wants to see what it did, wherever they had scrolled to
read. The environment scrolls down from its first row.

Only the renderer knows how tall a pane is or how many lines its contents came
to, and both change every frame. It returns them as a `ScrollBackView` or an
`EnvironmentView`, which the `record_*_view` methods bound the scroll against,
so paging past the first unit or the oldest entry lands on it. Only the tab
shown is laid out, so only its bounds are recorded, and a hidden tab's are
recorded when it is shown, before a key can reach it. Each pane is a
`ScrollState`, clamped when a key is pressed rather than on the next frame. The offsets live on `ReplApp`, out
of the widgets, which is what lets `app_tests` page around with no terminal at
all.

## Tests

`crates/datalove-repl/tests/`:

- **engine_tests** -- the `.repl` fixtures in `fixtures/engine/` through
  `run_source`, snapshotting the parse, eval, and environment after
  every input as JSON. They run against
  `datalove_sys_packages::system_library()`, so the suite covers the library
  and the linked riders the binary actually ships. `BLESS=1` updates them.
  They are the engine's behaviour as typed at the REPL, slash commands and
  all, which is why they stay here.
- **session_tests** -- `/save`, `--load` and `/load` through the app and a
  real engine on its worker thread: a saved history replays to the same
  outcomes and environment, saving it again writes the same file, and a
  session edited, inserted into and deleted from loads back to the same
  numbered units. A mismatch between the app's units and the engine's fails
  any of them.
- **command_tests** -- how `/edit`, `/delete` and `/insert` are read, sources
  over several lines, comments before a command, and the usage of each.
- **app_tests** -- the state machine against a scripted `MockExecutor`, with no
  engine at all, which is how the interleavings are reachable: a result
  arriving after a later input, a crash reset, the multiline round trip, the
  engine reporting itself ready or dead, the code units and what the last
  step did with each, edits applied when answered, a mismatch reported, the
  tab input selects, scrolling each pane, and what `/save` writes. The mock
  fills in the session an answer reports, as the engine would, for appends
  and resets; a test that edits gives it explicitly.

`crates/datalove-script-engine/tests/`:

- **engine_edit_tests** -- the edit path: `edit_unit`, `edit_module` and the
  splices, the units each reaches, and the environment afterwards; modules
  joining, leaving and breaking; and units that did not compile, parse, or run
  to their end staying and going green. The reach itself is measured in
  `datalove-datafun`'s `script_exec_reactivity_tests` and
  `script_scenario_tests`; these say the engine wires it up.
- **reactive_tests** -- the step tables in `fixtures/reactive/` against the
  engine, and [script-reactivity-cases.md](script-reactivity-cases.md) against
  the tables. `BLESS=1` re-renders the doc.

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
- `/exit`, `/help`, `/reset`, `/save`, `/load`, `/edit`, `/delete` and
  `/insert` are the only commands, and help prints the word "help".
- A command whose source spans lines cannot be typed: an input starting with
  the sigil is a command on its first line. Only a replay gives one over
  several lines.
- An edit cannot change a unit from statements to an expression or back.
- Errors are plain strings with no rendered diagnostics, and type errors are
  their `Debug` form, though `ScriptCompiler` exposes the spans needed for
  better. See
  [issues.md](issues.md#user-content-the-repl-prints-errors-as-rust-values).
- Mouse capture is enabled while only key events are read, which disables
  terminal text selection and drops the mouse wheel for no benefit.
- Nothing scrolls a pane while the menu or a modal is open.
- What a unit came to -- an expression's value, a compile error -- is shown
  only in the History tab, and code selects the Environment tab. So typing
  `x` to see its value shows nothing until the tab is changed, and a red bar
  says a unit failed without saying why.
- An edit to a module no unit requires reports nothing, since only required
  modules are compiled; see
  [issues.md](issues.md#user-content-an-edit-to-a-module-no-unit-requires-reports-nothing).
