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
- [The command queue](#user-content-the-command-queue)
- [The engine](#user-content-the-engine)
- [What a line does](#user-content-what-a-line-does)
- [Editing, not only appending](#user-content-editing-not-only-appending)
- [Editing at the prompt](#user-content-editing-at-the-prompt)
- [Undo and redo](#user-content-undo-and-redo)
- [Saving and loading a session](#user-content-saving-and-loading-a-session)
- [`script-world` runs through the engine](#user-content-script-world-runs-through-the-engine)
- [When there is no engine](#user-content-when-there-is-no-engine)
- [The terminal UI](#user-content-the-terminal-ui)
- [Tests](#user-content-tests)
- [Known gaps](#user-content-known-gaps)

## Crates

```
datalove-repl-rat/      Widgets, rendering, and the terminal that drives them
  help.md               The help, read into the binary as it is built
  src/lib.rs            RatatuiApp: ReplApp plus a TextArea
  src/render.rs         The three-panel layout, code, tabs, exit dialog, crash and engine modals
  src/detail.rs         The Detail tab: the selected unit, laid out
  src/ansi.rs           Text with ANSI colour escapes, as styled lines
  src/help.rs           The help, parsed from markdown and flowed for the pane's width
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
or what a REPL command came to. `Session` is where the two meet, on the
worker: it carries out units and the commands that change the session. The
rest of the commands are the app's, which carries them out itself; see
[The command queue](#user-content-the-command-queue).

`datalove-cli` calls `datalove_repl_rat::run()` for the interactive REPL and
`datalove_repl::run_script` for `repl --script`. Both take the system library,
and the workspace's own `local` library if it has one, from the caller; see
[The engine](#user-content-the-engine). `script-world` uses `Engine` directly.

## The request/response shape

Input is read in the app, and only commands cross to the engine, whose
results come back keyed by id:

```
submit_input(text) -> parse_input -> ReadMultiline: more lines
                                  -> Empty: nothing
                                  -> Command: the queue -> submit_eval(id) -> Eval, environment, session, step
                                                       -> carried out in the app
```

**Reading an input does not ask the engine.** `parse_input` is a function of
the text: the sigil and a few keywords on its first line of code say whether
it is a command, a statement, or the start of a block that reads on to its
`end`. It used to run on the worker, from when it was a method of the engine,
and an input waited for its parse to come back before its eval was sent, so a
later input's parse could overtake an earlier one's eval. Read in the app,
commands go to the engine in the order they were typed, and an input that
opens a block goes multiline as Enter is pressed.

`ReplExecutor` abstracts where evaluation happens. `ThreadedExecutor` puts the
engine on a worker thread so the UI stays responsive. Everything crossing the
boundary is `Serialize`/`Deserialize`.

**Results are matched to history entries by id, not by position.** The app
pushes an entry when input is submitted, which can be before the previous
entry's evaluation has answered. So "the last entry" is not necessarily the
entry a result belongs to, and someone typing during an evaluation would
otherwise get the result attached to the wrong line. A result always has its
entry: commands go to the engine in order and it answers in order, and the
history only forgets entries up to one whose answer reset the session, by
which time all of them have been answered.

Executors are constructed by the caller and handed to `ReplApp`, which is
what lets the tests drive the state machine with a scripted mock.

## The command queue

Some commands are the engine's: units, and the commands that change the
session, `/reset`, `/edit`, `/delete` and `/insert`. The rest are the app's:
`/undo`, `/redo`, `/save`, `/load`, `/help` and `/exit`, which read the history
and the app's units, and may send the engine commands of their own. Each runs
against what every command entered before it leaves, however fast they were
typed, and **the queue is what makes it so**. Every command entered goes into
it, typed, from the code panel, or replayed, and `ReplApp::dispatch` runs
what can run from its front, after every answer and every entry:

- A command for the engine is sent as soon as everything before it has been,
  since the engine answers in order. Units typed or replayed one after
  another are not held back for each other's answers, which at the app's
  10ms poll would cost a long `/load` seconds.
- A command for the app waits until everything sent before it is answered,
  and then runs, in the app, against the history as that leaves it. What it
  sends the engine is sent before anything after it in the queue: an undo
  takes back the change entered just before it, and the units a `/load`
  replays, which it puts at the front of the queue, go ahead of what was typed
  after the `/load`.

The app's commands once went to the worker too, for the session to hand back
as `CallerInterpret` once everything before them had been answered. That
ordered what they read, and not what they sent: by the time the answer came
back, what was typed after them was already on its way to the engine, so a
`/load` replayed its units after a line typed behind it, and an `/undo` could
not have taken back the right change.

**A command enters the history when it runs, not when it is typed**, and
drops the redo stack then, so a line typed behind an `/undo` drops the stack
that undo leaves, as it would have typed after it. What waits is shown in the
History tab, after a divider below everything that has run, the redo stack
included, and the code panel shows the units among it as on their way. An
empty input and a line that only opens a multiline input are not commands,
and enter nothing.

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

**From the code panel, Delete, Enter and Insert make the same commands.**
With a unit selected, Delete submits `/delete N`, shows the History tab where its answer
goes, and selects the unit after it, or the one before if it was the last;
focus stays on the code. Enter edits it: the unit's source goes into the
input, which takes focus, multiline if the unit spans lines, with a magenta
border and "Edit unit N" in its title. The input is the prompt otherwise: a
single line that opens a definition goes multiline, Ctrl+C twice cancels, and
a REPL command typed there runs as one. Submitting sends `/edit N <source>`,
entered in the history as that command, so a session edited from the panel
saves and replays the same as one edited at the prompt. When the edit is
submitted or cancelled, focus goes back to the code on the same unit, and the
input gets back whatever was typed in it before the edit began. Insert is the
same with an empty input, titled "Insert before unit N", and sends
`/insert before N <source>`. The new unit goes before the selected one, which
stays selected, so inserting again puts the next one after it, above the same
unit. There is no key for inserting after; appending is typing at the prompt.

An edit, a delete or an insert answers with `Eval::Edited`: what each unit it
reached came to, by number, in the session's order. A unit that fails at run
time among them empties the session, as one appended does, and the app treats
the answer as that reset.


## Undo and redo

`/undo` takes back the last change to the session and `/redo` makes it again.
They move through the history rather than adding to it, so neither is
entered, and they show the History tab, leaving focus where it was. What was
undone stays there, grayed out: it is the redo stack, always the end of the
history. Entering anything new drops it.

**Each entry keeps what undoing and redoing it takes**, as a `Change`, kept
when the engine answers the command that made it, from the app's units as the
change left them: a unit made, by an append or an insert, with the unit before
it and its source; a unit edited, with its old and new source; a unit
deleted, with the unit before it and its source. Undo turns a change into the
command that takes it back -- a delete, an edit back, or a `Restore`, which
puts a unit back with the number it had -- and redo into the one that makes it
again. Undo is last in, first out, so a change is only ever undone or redone
against the session it was made to, and the unit it names its place by is
there. An entry that made no change -- `/help`, `/save`, `/load`, a failed
command, an empty line -- has none, and goes back and forth with the change
before it. A `/reset` forgets the history before it, so nothing before one
can be undone.

The commands undo and redo send are not entered either. They go to the
engine like any other, under ids with no entry, which `ReplApp::unrecorded`
keeps until they are answered, and their answers change the app's units and
are checked against the engine's like any other. The queue runs undo and
redo once everything before them is answered, so the change they take back
or make again is known.

**Dropping the redo stack gives its numbers back.** A saved session is
replayed from the history it keeps, which numbers its units in the order they
were made, and the units the dropped changes made are not among them. So the
session numbers its next unit as that replay would: the smallest number a
dropped change made, which a `Renumber` sends the engine. Units are numbered
in order and undo is last in, first out, so those numbers are the highest
given and none of them is in the session.

## Saving and loading a session

`/save` writes the history to `repl-session.world` in the workspace root,
which is the directory the REPL was started in, the one it looks for `local`
in. Script units are `scriptunit-fragment` and `scriptunit-expr` sections,
and REPL commands are `repl-command` sections. `repl-command` exists only for
this. `repl --load <worldfile>` replays a saved history before the first
prompt, and `/load` replays `repl-session.world` in place of the session.

**What is saved is the app's history, not the engine's session.** The
engine's session holds the script units. The history also has the slash
commands and whatever a reset took out of the session. So the app writes the
file, when the queue runs the `/save`, from the history up to it. A few things
are left out:

- Anything typed after the `/save`.
- What was undone, which the `/save` dropped when it was entered.
- Whatever a reset already took out of the history.

An app given no path with `set_session_path`, as in the tests, answers `/save`
and `/load` with an error.

**A load replays through the app,** so the inputs show up in the history as
though they had been typed. `ReplApp::replay` sends each one straight to
evaluation rather than through parsing. The file already says what each input
is, and parsing a `fun` on its own would ask for more lines. A failure fails
again and a command runs again. The exceptions are `/save`, `/load` and
`/help`, which `load_session` drops: replaying a `/save` would overwrite the
file being loaded with only the part of it read so far, replaying a `/load`
would load it again, without end, and replaying a `/help` would show the help
to someone who did not ask. For `--load` the file is read before the terminal
is set up, so a bad one is reported on the ordinary screen. A worldfile with
module, data or rider sections is refused.

**`/load` resets before it replays**, so the session becomes the one saved
rather than the saved one on top of what is there. The app reads the file when
the queue runs the `/load`, and puts a `/reset` and the saved inputs at the
front of the queue. That reset is ordered with everything else the engine is
asked, and
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
is submitted, an entry is pushed as `Evaluating`, and a response never comes.
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

Each unit has a bar beside it colored for its status, how it fared the last
time it was compiled or run: green if it ran, red if it did not compile,
yellow if it returned early, magenta if it was blocked, and cyan while it
waits on the engine. A step changes the status of the units it reaches and
leaves the rest as they were, so the bars always say which units are broken.

**The bar's right half says what the last step did.** The left half, `▌`, is
every line of every unit. A step that compiled the unit -- ran its parse, its
typecheck or its ownership analysis -- fills the top right of its first line,
and one that ran it fills the bottom right of its last line. So a one-line
unit is `▌` if the step left it alone, `▛` if it compiled it and did not run
it, `▙` if it reran it and did not compile it, and `█` if it did both, and a
longer unit carries the same marks at its corners:

```
1 █ let b = 3          compiled and ran
2 ▌ let e = b          ran, and not compiled: the value it reads changed
  ▙ let g = e + 1
```

The corners are the unit's rather than each line's, because a mark on every
line makes a striped column, and top stripes and bottom stripes look alike
down a few rows. The marks are the status color, a cell having one. The last
step is the last unit, edit, delete or insert the engine carried out; a
command that changes nothing leaves the marks, and a reset leaves no units to
mark. What a step compiled comes from the engine (see
[script-reactivity-architecture.md](script-reactivity-architecture.md#user-content-what-a-step-reports)),
and `Session` turns its positions into numbers and sends it with every answer;
the app shows it as it comes, since nothing it could keep by rule would know
what salsa ran. A step is one type on both sides of the worker, `UnitStep`:
the engine's names units by position, and the REPL's by number, with a
`Blocked`'s positions turned into numbers too. An edit's `Eval::Edited` and
the answer's `step` are the same list.

**The app keeps its own copy of the session's units**, numbered as the
`Session` numbers them, and changes it as each answered command changed the
engine's: an appended unit is added and numbered, an edit, a delete or an
insert is made, and a reset empties it. The bars come from the same answers:
the unit appended, or the ones an edit reached, take what they came to. The code is these units, then the
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
answered**, not as the engine has it now. A unit shows from when it is
submitted, before it runs, so a `/reset` has to take the units before it from
when it is submitted too, or the units typed after it show appended to the session they will
never see. The history keeps the old units until the engine answers the reset,
since they are the engine's session until then, and `code_units` shows only
the units after the last reset still in it. Appended units and resets are the
only guesses the code makes: an edit, a delete or an insert shows once it is
answered. `/load` gets this for nothing: it is a `/reset`
and the saved units, replayed together.

**One unit is always selected** while the session has units, its number in
white. While the code panel has focus its source is in the yellow that marks
the tab shown too, and Up and Down move between units. A unit entered is
selected, and when the selected unit goes, the newest is. The selection is
kept by the unit's number, so units added or removed around it leave it where
it was. A reset clears it, since the new session numbers from 1 again and the
old number would land on a different unit. It is what the Detail tab
describes, and what Delete, Enter and Insert act on; see
[Editing at the prompt](#user-content-editing-at-the-prompt).

### Tabs

The bottom panel has two tabs, Environment and History; Detail while the
session has units; and Help, once help has been asked for. F2 cycles them, and
Alt+1 to Alt+4 select one. Input selects the tab it changes when submitted:
code the Environment, a REPL command the History, which is the only place what
a command did shows. A tab that goes, as Detail does at a reset, leaves the
Environment shown.

**The Detail tab describes the selected unit.** Its header is the unit's bar
beside its number and status, and, if the last step compiled or ran it, a
second line saying which, so the corners of the bar are read out beside it.
Below is what it came to, the diagnostics for one that did not compile, what
it binds and what each binding holds now, the functions it defines, and the
names it reads, provides, takes, overwrites and updates. `detail.rs` lays it
out.

What the app keeps -- the status, the marks, and each unit's last outcome,
which `SessionUnit` holds -- shows at once. The rest only the session knows,
so the app asks for it with an internal `Describe` command, answered with
`Eval::Detail(UnitDetail)`. It asks whenever the selected unit or the number
of answers that could have changed the session moves on, after dispatching the
queue, so the question goes behind whatever was sent before it and is answered
of the session that leaves. Describes take their ids from `DESCRIBE_IDS`, far
past the entries', so an entry's id is the same whether or not a describe came
between; an answer with such an id only sets the detail. Asking only reads, so
it is not counted among what an app command waits for. The detail is shown
only while it is of the selected unit; moving shows "asking the engine" until
the next answer. The diagnostics are rendered by
`ScriptCompiler::render_unit_diagnostics`: the unit's parse diagnostics, and,
if it parses, the type checker's whose labels point into it and its ownership
analysis's, as ariadne renders them for the CLI: in colour with the box-drawing
frame, by `TextStyle::Ansi`. The escapes travel back as part of the text, and
`ansi.rs` turns the SGR sequences into ratatui styles, dropping any other
escape. Each pass files
its diagnostics in a salsa accumulator from inside its query, and they are read
back from the query; ownership analysis is keyed on what the units before it
left as well as on the unit, so `unit_ownership_diagnostics` asks it again with
what the unit records hold now, which is the query the unit's compile ran.

**A unit that breaks shows its detail.** When an answer re-derives the
selected unit -- a new unit appended, or a step that ran or reported it -- and
leaves it failed, returned early or blocked, the Detail tab is shown, and the
focus stays where it was.

**Help is found by asking for it.** `/help` or F1 adds the Help tab, shows it,
and focuses the bottom panel, so the scroll keys move through it. Until then
the panel shows only what the session has. The help is `help.md`, at the root
of `datalove-repl-rat` so that it is packaged with the crate, and read into the
binary with `include_str!`. `help.rs` parses it once with comrak into blocks
of styled text -- headings, paragraphs, lists, code -- and every frame wraps
them again for the pane's width, a list item's lines after the first lined up
under its text. Markdown it does not lay out is a `todo!`, and its tests lay
the help out at several widths, so a construct added to `help.md` that the
layout does not know fails there and not in a session. A saved `/help` is not
replayed: it would show the help to someone loading the session who did not
ask.

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
`ReplApp` as a `Panel`, alongside the dialogs' state.

Typing only reaches the textarea while the input has focus. In single-line
mode Enter submits. In multiline mode Enter inserts a newline and Alt+Enter
submits. Ctrl+D exits, and F1 shows the help. Neither works while a dialog or
a modal is open: those take the keys first, the exit dialog Ctrl+C and
anything else, and a modal only Enter. Esc does nothing.

**Ctrl+C twice cancels multiline input.** In multiline mode, with the input
focused, the first Ctrl+C puts "Ctrl+C again to cancel" where the Alt+Enter
reminder was, and the second leaves multiline mode and clears what was typed.
Any other key takes the first back and does what it does. Twice, because a
definition typed over several lines is a lot to lose to one stray key. Ctrl+C
is what a shell or a REPL throws a line away with. Esc was tried first and
does not work: a terminal multiplexer holds Esc back for its `escape-time`, to
see whether an escape sequence follows, which delays it, and two Escs that
arrive in one read are parsed by crossterm as one, which loses the second.
Ctrl+C is one byte that starts no sequence. Multiline mode ends only by
submitting or cancelling; an answer to an earlier input arriving meanwhile
leaves it.

**Ctrl+C anywhere else asks whether to exit.** It opens a dialog saying
"Ctrl+C again to exit"; another Ctrl+C exits, and any other key closes it and
does nothing else, so the key that dismisses it is not typed into the input.
This replaced a menu of Resume and Exit that Esc opened, for the reason Esc
does not cancel: it is slow, and loses a key, behind a multiplexer. Ctrl+C
twice to exit is what the Node.js REPL does. After a cancel, a third Ctrl+C
asks to exit.

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
enough to show the selected unit, using where each unit was last laid out.
Its page keys scroll a page and keep the selection on screen: a selected unit
the page scrolls off gives way to the nearest unit left showing, and a page
key with nowhere left to scroll selects the first unit or the last. So paging
to either end of the code ends on its end unit, one key after the view gets
there.

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
  numbered units, and the marks say what a real step compiled and ran. A
  mismatch between the app's units and the engine's fails any of them.
- **session_tests** also undo and redo every kind of change against a real
  engine, to the session as it was before each, and drop the redo stack and
  load the session saved after it back to the same numbers.
- **command_tests** -- how `/edit`, `/delete` and `/insert` are read, sources
  over several lines, comments before a command, and the usage of each.
- **app_tests** -- the state machine against a scripted `MockExecutor`, with no
  engine at all, which is how the interleavings are reachable: a result
  arriving after a later input, a crash reset, going multiline and back, the
  engine reporting itself ready or dead, the code units and their statuses,
  the marks of what the last step compiled and ran, edits applied when
  answered, a mismatch reported, the tab input selects, scrolling each pane,
  the Detail tab coming and going and showing a broken unit, the detail asked
  for the selected unit, and what `/save` writes. The mock keeps describes
  apart from the commands it records, and fills in the session and the last step an
  answer reports, as the engine would, for appends and resets; a test that
  edits gives them explicitly.

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
  lines: `classify_input` has no case for one. See
  [issues.md](issues.md#user-content-the-repl-never-reads-a-brace-delimited-input-over-several-lines).
- No cancellation. The worker has no way to be interrupted, and an infinite
  loop in evaluated code wedges the worker with no interrupt key.
- `/exit`, `/help`, `/reset`, `/save`, `/load`, `/edit`, `/delete`,
  `/insert`, `/undo` and `/redo` are the only commands.
- Undo and redo have no key; they are typed.
- A command whose source spans lines cannot be typed: an input starting with
  the sigil is a command on its first line. Only a replay, or an edit from
  the code panel, gives one over several lines.
- An edit cannot change a unit from statements to an expression or back.
- Errors are plain strings in the History tab, and type errors are their
  `Debug` form. The Detail tab renders parse, type and ownership diagnostics,
  but a lowering error shows only as its message, having no location; see
  [issues.md](issues.md#user-content-the-repl-prints-errors-as-rust-values)
  and [issues.md](issues.md#user-content-lowering-errors-say-what-never-where).
- Mouse capture is enabled while only key events are read, which disables
  terminal text selection and drops the mouse wheel for no benefit.
- Nothing scrolls a pane while a dialog or a modal is open.
- An expression's value is shown in the History and Detail tabs, and code
  selects the Environment tab. So typing `x` to see its value shows nothing
  until the tab is changed.
- An edit to a module no unit requires reports nothing, since only required
  modules are compiled; see
  [issues.md](issues.md#user-content-an-edit-to-a-module-no-unit-requires-reports-nothing).
