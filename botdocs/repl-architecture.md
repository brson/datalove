# REPL Architecture

How the REPL is put together, and the decisions that shaped it.

The UI model it aims at is `mandocs/repl-ui.md`; the semantics it implements
are `mandocs/script-semantics.md`.

## Crates

```
datalove-repl-rat/      Widgets, rendering, and the terminal that drives them
  src/lib.rs            RatatuiApp: ReplApp plus a TextArea
  src/render.rs         The three-panel layout, cards, menu, crash modal
  src/term.rs           Terminal setup, event loop, key bindings

datalove-repl/          Engine and the UI-agnostic app
  src/lib.rs            Input classification and the wire types
  src/engine.rs         Engine: compiles and evaluates; run_source
  src/app.rs            ReplApp state machine, ReplExecutor trait
  src/executor_threaded.rs  The one executor: a worker thread owning an Engine
```

`datalove-cli` calls `datalove_repl_rat::run()` for the interactive REPL and
`Engine::run_script` for `repl --script`.

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

`Engine::new` loads the system library through
`WorkspaceDescriptor::load_default_sys()` and builds the native riders that
`sys/std/string` needs, so a session can `require module sys/std/string` and
call into it. It keeps the descriptor so a crash reset can rebuild the
compiler and executor without re-reading `sys` from disk.

Evaluation reports what a fragment defined by reading the compiled unit's
`script_context().exports` - values, slots, and functions - rather than by
re-scanning the source text. All four error phases are checked, ownership
included: an ownership error leaves lowering `Skipped`, so a use-after-move
inside a fragment used to slip through as a successful binding.

`Engine::run_source` is the one loop over a `---`-separated script. Both
`repl --script` and the fixture tests use it, so the driver and the tests
cannot drift.

## Ownership at the prompt

Every line is a script unit, which makes the linear rules that are right
inside a line hostile between lines. A unit therefore **copies out of the
bindings earlier units own** rather than taking from them, so names go on
working and a session need not be typed in dependency order. See
`compiler-guide.md` under "Ownership across units" for what that costs and
what it leaves behind (D013 for a binding a unit gave away before it ended).

Auto-adapt is off at the prompt. It repairs mistakes *within* a line, which
is the case where the whole story is visible in front of you and the error
teaches you something.

The long-term answer to a moved binding is rewind and replay - recompiling
the earlier unit with the `@` inserted and re-executing from there, which is
what `mandocs/script-semantics.md` is reaching for. It becomes necessary
rather than merely nicer when non-cloneable types land, since copying out of
an earlier unit stops being available.

## Tests

`crates/datalove-repl/tests/`:

- **engine_tests** - 21 `.repl` fixtures through `Engine::run_source`,
  snapshotting the parse, eval, and environment after every input as JSON.
  `BLESS=1` updates them; unset `RUST_BACKTRACE` first.
- **app_tests** - the state machine against a scripted `MockExecutor`, with no
  engine at all, which is how the interleavings are reachable: a result
  arriving after a later input, a crash reset, the multiline round trip.

Neither the rendering nor the key bindings have tests. `render.rs` would take
ratatui's `TestBackend`; `term.rs` has no seam, since `run()` does terminal
setup and the loop together.

## Known gaps

- Multiline input triggers only on a leading `fun`. `is_open_brace_tree` is
  hardcoded false, so an `if` or a loop cannot be typed across lines.
- No cancellation. `WorkerRequest::Shutdown` is never sent, and an infinite
  loop in evaluated code wedges the worker with no interrupt key.
- `/exit` and `/help` are the only commands, and help prints the word "help".
- Errors reach the user as `Debug` output rather than rendered diagnostics,
  though `ScriptCompiler` exposes the spans needed for better.
- Mouse capture is enabled while only key events are read, which disables
  terminal text selection for no benefit.
- History entries are two plain lines rather than the cards `repl-ui.md`
  describes, and nothing scrolls the history panel.
