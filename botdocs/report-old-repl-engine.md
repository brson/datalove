# Old REPL Engine Architecture

Documentation of the current REPL engine before replacement.

## Crate Structure

```
datalove-repl/          Core engine (UI-agnostic)
  src/
    lib.rs              Input classification, types, enums
    engine.rs           Engine, ReplHistory, script execution
    app.rs              ReplApp state machine, ReplExecutor trait
    executor_blocking.rs    Synchronous executor (WASM)
    executor_threaded.rs    Multi-threaded executor (native)

datalove-repl-rat/      Ratatui frontend
  src/
    lib.rs              RatatuiApp (ReplApp + TextArea)
    render.rs           UI rendering

datalove-repl-term/     Terminal runner
  src/
    lib.rs              Raw mode, event loop, key handling
```


## Core Types

### Input Classification

`classify_input()` in `lib.rs` determines how to handle user input:

| First keyword | Classification | Behavior |
|---------------|----------------|----------|
| `let`         | OnelineStatement | Parse, typecheck, execute immediately |
| `fun`         | MultilineStatement | Switch to multiline mode, wait for Alt+Enter |
| `/`           | ReplCommand | Handle as meta-command (/exit, /help) |
| whitespace    | Whitespace | Ignore |
| else          | Expression | Wrap in temp `let`, execute, print result |

### Command Types

```rust
enum Input {
    Input(String),        // Single line
    Multiline(String),    // Multi-line (fun definitions)
}

enum InputParse {
    Empty,                // Whitespace only
    ReadMultiline(String), // Need more lines
    Command(Command),     // Ready to execute
    CrashReset(String),   // Engine panic, reset state
}

enum Command {
    ReplCommand(ReplCommand),   // /exit, /help
    ScriptStatement(String),    // let, fun
    Expression(String),         // Bare expression
}

enum Eval {
    Nothing,              // No result (e.g., require statement)
    SuccessLet(EvalLet),  // let binding result
    SuccessExpr(EvalExpr), // Expression result
    SuccessFun(EvalFun),  // Function definition
    Error(String),        // Parse/typecheck/runtime error
    CallerInterpret(ReplCommand), // UI should handle (exit, help)
    CrashReset(String),   // Engine panic, state reset
}
```


## Engine (`engine.rs`)

### Engine Struct

```rust
pub struct Engine<'db> {
    db: &'db dyn datafun::Db,
    history: ReplHistory,
    interp_ctx: datafun::interp::InterpContext<'db>,
    module_graph: datafun::module_graph::ModuleGraph,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
}
```

### ReplHistory

Maintains incremental script state:

```rust
struct ReplHistory {
    entries: Vec<HistoryEntry>,
}

struct HistoryEntry {
    command: Command,
    last_eval: Eval,
    script_status: Option<ScriptUnitStatus>,
}

struct ScriptUnitStatus {
    script_unit: ScriptUnit,
    active: bool,  // false if typecheck failed
}
```

**Key methods:**
- `build_script()` - Creates Script from active units only
- `build_script_with_unit()` - Creates Script with a new unit appended
- `add_script_entry()` - Records a script-producing entry

### Execution Flow

1. **`parse_input()`** - Classify and parse input
   - Wraps in `catch_unwind` for panic recovery
   - Returns `InputParse`

2. **`eval()`** - Execute a Command
   - Wraps in `catch_unwind` for panic recovery
   - Dispatches to:
     - `eval_repl_command()` - Handle /exit, /help
     - `eval_script_statement()` - Execute let/fun
     - `eval_expression()` - Execute bare expression

3. **`eval_script_statement()`**
   - Creates new ScriptUnit from source
   - Builds full script (history + new unit)
   - Parses all units into single AST
   - Typechecks full script
   - Merges expr_types into interp context
   - Executes single unit via `execute_script_unit()`
   - Returns result with pretty-printed value

4. **`eval_expression()`**
   - Wraps expression as `let _expr_result = <expr>`
   - Executes like script statement
   - Removes temp variable after pretty-printing
   - Does NOT persist to history


## ReplApp (`app.rs`)

UI-agnostic application state machine:

```rust
pub struct ReplApp<E: ReplExecutor> {
    executor: E,
    next_id: u64,
    history: Vec<HistoryEntry>,
    environment: Vec<(String, String, String)>,
    multiline_mode: bool,
    menu_open: bool,
    menu_selection: usize,
    should_exit: bool,
    crash_modal: Option<String>,
    stderr_log_path: Option<PathBuf>,
}
```

**State machine:**
1. User submits input via `submit_input()`
2. App creates HistoryEntry, assigns ID, submits parse request
3. Executor processes async, returns ParseResult
4. If Command parsed, app submits eval request
5. Executor returns EvalResult with updated environment
6. App updates entry status and environment display


## ReplExecutor Trait

```rust
pub trait ReplExecutor {
    fn new() -> Self;
    fn submit_parse(&mut self, id: u64, input: Input);
    fn submit_eval(&mut self, id: u64, command: Command);
    fn try_recv_response(&mut self) -> Option<WorkerResponse>;
}
```

### BlockingExecutor

For WASM/single-threaded environments:
- Leaks `Database` for `'static` lifetime
- Queues responses in `VecDeque`
- Synchronous parse/eval

### ThreadedExecutor

For native multi-threaded:
- Spawns worker thread owning Engine
- Uses `mpsc::channel` for request/response
- Worker thread runs `worker_thread()` loop
- Non-blocking `try_recv` for responses


## Frontends

### datalove-repl-rat

Ratatui-specific wrapper:

```rust
pub struct RatatuiApp<E: ReplExecutor> {
    pub repl: ReplApp<E>,
    textarea: TextArea<'static>,
}
```

Handles:
- TextArea widget state
- Key event translation
- UI action processing (SetMultilineInput, ClearInput)

### datalove-repl-term

Terminal setup and event loop:
- Raw mode, alternate screen
- Stderr redirection to temp file (avoids corrupting terminal)
- Event polling with 10ms timeout
- Key event dispatch:
  - Ctrl+D → exit
  - Enter → submit (single-line) or newline (multiline)
  - Alt+Enter → submit (multiline)
  - Esc → menu


## Testing

`.repl` fixture files in `tests/fixtures/engine/`:
- Inputs separated by `---`
- Test runner records parse, eval, environment for each
- JSON output compared to `.out.expected`


## Design Observations

### Good Design Patterns

1. **Clean layer separation**: Engine (logic) vs App (state) vs Frontend (UI)

2. **Executor abstraction**: Same app code works sync or threaded

3. **Panic recovery**: `catch_unwind` prevents crashes from killing REPL

4. **Incremental script**: History tracks active/inactive units

5. **UI-agnostic core**: ReplApp has no rendering dependencies

6. **Request IDs**: Enables async correlation of requests/responses

### What Gets Replaced

The following in `engine.rs` depends on the old script interpreter:

- `eval_script_statement()` - calls `execute_script_unit()`
- `eval_expression()` - wraps in temp let, executes
- Pretty-printing from `script_scope.variables`
- Environment extraction from `script_scope`

The REPL architecture (Engine/ReplApp/Executor/Frontend) is solid and
should be preserved. Only the script execution internals need replacement
to use frame-based interpretation.
