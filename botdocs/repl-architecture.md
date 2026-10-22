# REPL Architecture

How the REPL is put together, and the decisions that shaped it.

How an edit reaches the units that depend on it is
[script-reactivity-architecture.md](script-reactivity-architecture.md).

## Crates

```
datalove-repl-rat/      Widgets, rendering, and the terminal that drives them
  src/lib.rs            RatatuiApp: ReplApp plus a TextArea
  src/render.rs         The three-panel layout, cards, menu, crash and engine modals
  src/term.rs           Terminal setup, event loop, key bindings

datalove-repl/          Engine and the UI-agnostic app
  src/lib.rs            Input classification and the wire types
  src/engine.rs         Engine: compiles and evaluates; run_source
  src/app.rs            ReplApp state machine, ReplExecutor trait
  src/executor_threaded.rs  The one executor: a worker thread owning an Engine
```

`datalove-cli` calls `datalove_repl_rat::run()` for the interactive REPL and
`Engine::run_script` for `repl --script`. Both take the system library from
the caller - see below.

There was a `datalove-repl-term` crate holding the terminal half. The split
existed so a wasm frontend could reuse the widgets; that frontend was deleted
along with the rest of the dead wasm code, and `repl-rat` was never really
backend-agnostic anyway - it takes crossterm key events. The crates were
merged.

## The request/response shape

Input flows one way and results come back keyed by id:

```
submit_input(text) -> Input -> submit_parse(id) -> InputParse
                                     |
                            (if Command) submit_eval(id) -> Eval + environment
```

`ReplExecutor` abstracts where that happens; `ThreadedExecutor` puts the
engine on a worker thread so the UI stays responsive. Everything crossing the
boundary is `Serialize`/`Deserialize`.

**Results are matched to history entries by id, not by position.** The app
pushes an entry when input is submitted, which is before the previous entry's
evaluation has answered, so "the last entry" is not the entry a result
belongs to. Getting this wrong panicked the UI thread whenever someone typed
during an evaluation. Results for entries a crash reset cleared are dropped.

Executors are constructed by the caller and handed to `ReplApp`, which is
what lets the tests drive the state machine with a scripted mock.

## The engine

`Engine::new(db, sys)` takes a `SystemLibrary` rather than going looking for
one, and compiles what the session requires of it, which at startup is nothing.
The library carries the addresses of the native rider
functions alongside the sources, which become a `RiderNatives` given to both the
pipeline, for consts, and the executor, so a session can
`require module sys/std/string` and call into it. The engine keeps the pipeline,
the descriptor and the natives, so a crash reset rebuilds the compiler and
executor from them without building or loading a rider again.

**The roots follow the units.** Before every step -- an append, an edit, a
splice -- `Engine::with_compiler` narrows the pipeline's roots to what the
session's units will require once the step is done, plus the caller's
`also_roots`, and compiles. A unit adding `require module sys/std/u8` brings
`u8` into the compile, and truncating it away takes `u8` back out. When the
module set moves, the executor is given the new module registry and the
natives the new modules call before anything runs. The units already lowered
are left alone, which is only sound because a module's `IrModuleId` is given
by path and kept for the pipeline's life; when ids were graph positions, a
module joining ahead of another sent existing calls to the wrong module.

**A required module that does not compile is left out**, with every required
module that reaches it, and the rest of the roots stay (`settle_roots` in
`crates/datalove-repl/src/engine.rs`). The unit that requires it fails on its
`require`, and the engine replaces that error with the module's own. One
broken module costs only the units that require it, as one failed unit costs
only the units that read it. That is how the engine holds modules with
errors: outside the compile, with what they said kept in
`Engine::module_errors`. The engine starts whatever its modules do; the one
error no root's leaving can fix is a rider interface's, and that still stops
it starting.

`ModuleErrors` keeps the failing compile's type diagnostics in their stored
form, `StoredDiagnostic`, which names its `Source`s but holds no database
lifetime, so a caller in the same process renders them against
`Engine::db`. A caller across a thread, as the interactive REPL is, has only
the messages, `ModuleErrors::broken`, for now.

`ThreadedExecutor::spawn` takes `fn() -> SystemLibrary` instead of a value:
those addresses are raw pointers and so not `Send`, and the worker thread
builds its own. `datalove-cli` passes `datalove_sys_packages::system_library`, which
is the copy embedded in the binary; see the compiler guide under "The Shipped
Binary".

Evaluation reports what a fragment defined by reading the compiled unit's
`script_context().exports` - values, slots, and functions - rather than by
re-scanning the source text. All four error phases are checked, ownership
included: an ownership error leaves lowering `Skipped`, so a use-after-move
inside a fragment used to slip through as a successful binding.

**A line that fails stays in the session** unless it fails with a runtime
error. A line that does not compile reports `Eval::Error`, one that returns
early through `!` or `?` reports `Eval::ReturnedEarly`, and one that reads from
either is compiled and not run, reporting `Eval::Blocked`. Each stays as a unit
offering no bindings, so the history and the engine's units line up one to one
for script inputs, and an edit upstream can make any of them go green. The app
shows all three as errors and clears nothing.
[script-reactivity-architecture.md](script-reactivity-architecture.md) has how
the layers keep a failed unit's names from the units after it.

A runtime error empties the session. What the frames the error unwound
owned is not destroyed a value at a time: the runtime discards all its
memory instead (`IrInterpreter::discard_runtime_state`), and every binding
the session had was in it. The executor drops its units, and the engine
starts its compiled session again and reports `Eval::ErrorReset`, which the
app treats like a crash reset. Re-running the earlier units to bring the
session back was considered and not done: a unit that fails again would
loop.

`/reset` asks for the same reset on purpose. The engine resets before it
answers, so the environment that comes back with the answer is already empty.
The app then drops only the history entries up to and including the
`/reset`. Anything typed while the reset was running was evaluated in the new
session, so it stays.

`Engine::run_source` is the one loop over a `---`-separated script. Both
`repl --script` and the fixture tests use it, so the driver and the tests
cannot drift.

## Saving and loading a session

`/save` writes the history to `repl-session.world` in the workspace root,
which is the directory the REPL was started in, the one it looks for `local`
in. Script units are `scriptunit-fragment` and `scriptunit-expr` sections,
and REPL commands are `repl-command` sections. `repl-command` exists only for
this. `repl --load <worldfile>` replays a saved history before the first
prompt.

**What is saved is the app's history, not the engine's session.** The
engine's session holds the script units, and the history also has the slash
commands and whatever a reset took out of the session. So the app writes the
file when the engine hands `/save` back to it as `CallerInterpret`. A few
things are left out:

- Anything typed after the `/save`. Those lines can still be waiting on a
  parse, while everything before it has been answered, because the engine
  answers in order.
- The line that only opened a multiline input. The input it opened holds it
  again.
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
would: a unit that fails to compile, returns early, or is blocked on one that
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
  when its operation is over, so `Engine::eval_reporting` takes a callback
  that is handed the compiler while it is still alive.

**Locations are the worldfile's lines.** Each section is a source of its own,
so a diagnostic counts its lines from the section. The parser records how many
lines of the file come before each section (`ParsedWorldfile::line_offsets`),
the CLI maps each section's `Source` to that, and the renderer is given the map
(`bct::render::LineOffsets`). It puts that many empty lines in front of the
text it hands ariadne and moves the spans to match, rather than using ariadne's
`with_display_line_offset`, which moves the line number in a frame's header and
not those in its gutter. A unit's source is made when it is compiled, so its
entry is added in the `eval_reporting` callback, from `last_source`. Sources
from one worldfile all show as that file, where the renderer used to tell them
apart by naming the second `file:1`.

## Editing, not only appending

`Engine::edit_unit(i, text)` changes one line of the session and re-derives
what the change reaches, leaving the rest of the session with the lowering and
the frames it had. `Engine::edit_module` does the same from a module edit: the
units whose imports resolve into that module, and then the units that read what
those computed. `botdocs/plan-script-reactivity.md` is the whole of how the
reach is decided; two things about it belong here.

**The engine owns its database.** An edit is `set_text` on a `Source` and needs
`&mut db`, and a `ScriptCompiler` borrowing `&'db dyn Database` cannot be alive
across that, so no compiler is kept between calls -- `ScriptSession` is, and a
compiler is built over it per operation.

**A module edit has to reach the executor as well as the compiler.** A script
unit names a module function by `CodeRef::Module`, resolved against the
`Arc<ModuleFunctionRegistry>` the executor was handed, so `set_module_registry`
is what makes the new module IR the code that runs. Re-lowering the importing
unit alone leaves every value stale.

`Engine::remove_unit` and `Engine::insert_unit` splice the list and re-derive
the whole suffix. A unit of the suffix that stops compiling stays, as one
appended does.

**A module edit that does not compile is kept.** The roots are settled again,
so the module leaves the compile with the modules requiring it, and the units
reached fail saying why. The reach of a module edit is the units importing
from the edited module, from a module that requires it, or from a module the
edit broke or fixed, and the units reading what those compute. The last of
those is what the import edge alone would miss: a module that leaves the
compile because one it requires broke is in no graph to be found from. An
edit that fixes the module brings it and them back. A module no unit requires
is not compiled, so an edit to one reports nothing until something requires
it. Adding and removing modules mid-session is not offered.

## When there is no engine

Everything above assumes the engine started. When it does not, the app has to
say so, because the request/response shape hides the failure perfectly: input
is submitted, an entry is pushed as `Parsing`, and a response never comes.

That was a real bug. Startup used to run `cargo build --release` to build the
native riders; the worker unwrapped the result, so anything that stopped that
build - no cargo, a stale checkout, another cargo holding the package cache
lock - killed the thread, and every line the user typed sat at "parsing..."
with nothing on screen to say why.

So the worker reports its lifecycle. `start_engine` catches both a failed and
a panicking startup and sends `WorkerResponse::EngineDead { message }`;
success sends `EngineReady`. `ThreadedExecutor::try_recv_response` also turns
a channel disconnect into `EngineDead`, once - reporting it repeatedly would
spin `poll_results` - so a worker that dies mid-session cannot wedge the UI
either. The renderer shows "starting engine..." until ready and an "Engine
Gone" modal after, and `run()` prints the message again once the terminal is
restored, so it survives in the scrollback.

Startup no longer runs cargo, so the original cause is gone. The reporting
stays because it is the difference between an error and a hang.

## Scrolling the history

The history pane follows the newest output, which is where a session spends
nearly all of its time, so the scroll position is held as a distance *up from
the bottom* rather than down from the top. Zero needs no maintenance as
entries arrive, and submitting resets it: someone who submits wants to see
what it did, wherever they had scrolled to read.

Only the renderer knows how tall the pane is or how many lines the entries
came to, and both change every frame. It returns them as a `HistoryView`,
which is what `record_history_view` bounds the scroll against - so paging past
the oldest entry lands on it. The offset itself lives on `ReplApp`, out of the
widgets, which is what lets `app_tests` page around with no terminal at all.

## Panel focus

One of the three panels has focus, and the input panel has it at the start.
Tab moves focus down through the panels and Shift+Tab moves it up; both wrap
around. The focused panel has a white border and the others gray. The input
panel stays yellow while it has focus in multiline mode. Typing, Enter
included, only reaches the textarea while the input has focus. Esc and
Ctrl+D work from any panel. Focus lives on `ReplApp` as a `Panel`, alongside
the menu state.

The scroll keys are Up/Down, which move a line, and PageUp/PageDown, which
move a page. `ReplApp::scroll_target` decides where they go. When History or
Environment has focus, that pane takes the plain keys and the other pane takes
them with Ctrl. The single-line input has no use for them, so it lends the
plain keys to History and the Ctrl ones to Environment. Multiline input keeps
all of them, Ctrl included, for moving its cursor, so neither pane scrolls
from there until focus moves.

The environment scrolls down from its first row, where the history counts up
from its bottom. Both are a `ScrollState`, bounded by the limit the renderer
last reported and clamped when a key is pressed rather than on the next
frame.

## Ownership at the prompt

Every line is a script unit, and a unit follows the same linear rules
between lines as inside one: `let b = a` moves `a` out of the unit that bound
it, and a later line that names `a` is D013. Naming a binding as a bare
expression shows it without taking it. `compiler-guide.md` under "Ownership
across units" has how this is checked.

This used to be softened: a unit copied out of earlier units' bindings rather
than taking from them, so that names went on working. That was taken out to
keep script units as close to the language as possible, and because copying
stops being available once there are types that cannot be cloned. The
re-derivation of an edit no longer depends on it either -- re-running a unit
rewinds to the earlier bindings it took or wrote; see
[script-reactivity-architecture.md](script-reactivity-architecture.md).

## Tests

`crates/datalove-repl/tests/`:

- **engine_tests** - the `.repl` fixtures through `Engine::run_source`,
  snapshotting the parse, eval, and environment after every input as JSON.
  They run against `datalove_sys_packages::system_library()`, so the suite covers
  the library and the linked riders the binary actually ships.
  `BLESS=1` updates them; unset `RUST_BACKTRACE` first.
- **engine_edit_tests** - the edit path: `edit_unit`, `edit_module` and the
  splices, the units each reaches, and the environment afterwards; and units
  that did not compile, parse, or run to their end staying and going green. Separate from
  `engine_tests` because that one is `harness = false` and has a `main` of its
  own, so `#[test]` functions cannot live beside it. The reach itself is
  measured in `datalove-datafun`'s `script_exec_reactivity_tests` and
  `script_scenario_tests`; these say the engine wires it up.
- **session_tests** - `/save` and `--load` through the app and a real
  engine on its worker thread: a saved history replays to the same outcomes
  and environment, and saving it again writes the same file.
- **app_tests** - the state machine against a scripted `MockExecutor`, with no
  engine at all, which is how the interleavings are reachable: a result
  arriving after a later input, a crash reset, the multiline round trip, the
  engine reporting itself ready or dead, paging through the history, and
  what `/save` writes.

Neither the rendering nor the key bindings have tests. `render.rs` would take
ratatui's `TestBackend`; `term.rs` has no seam, since `run()` does terminal
setup and the loop together.

## Known gaps

- Multiline input triggers only on a leading `fun`. `is_open_brace_tree` is
  hardcoded false, so an `if` or a loop cannot be typed across lines.
- No cancellation. `WorkerRequest::Shutdown` is never sent, and an infinite
  loop in evaluated code wedges the worker with no interrupt key.
- `/exit`, `/help`, `/reset` and `/save` are the only commands, and help
  prints the word "help".
- Errors reach the user as `Debug` output rather than rendered diagnostics,
  though `ScriptCompiler` exposes the spans needed for better.
- Mouse capture is enabled while only key events are read, which disables
  terminal text selection for no benefit.
- History entries are two plain lines rather than the cards `repl-ui.md`
  describes.
- An edit to a module no unit requires reports nothing, since only required
  modules are compiled; see
  [issues.md](issues.md#user-content-an-edit-to-a-module-no-unit-requires-reports-nothing).
- PageUp and PageDown are the only way to scroll the history. The mouse wheel
  is captured and dropped, and the pane cannot be scrolled while the menu or a
  modal is open.
