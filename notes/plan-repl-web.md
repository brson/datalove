# Plan: Web+WASM Frontend for Datalove REPL

## Project Overview

Create a new `datalove-web` crate that provides a web+wasm frontend for the datalove repl. The architecture will have Rust WASM expose the ReplApp via a serialization protocol, with pure HTML/CSS/JS handling all DOM manipulation and rendering.

## Architecture Decisions

- **UI Approach**: Pure HTML/CSS/JavaScript - all DOM work in JS, not from Rust
- **Rust/JS Interface**: New serialization protocol for ReplApp interface
- **Deployment**: Static files only (can be served by any web server)
- **Build Tooling**: Trunk (Rust-focused bundler)
- **Executor**: WebWorkerExecutor (already exists in datalove-repl)

## Current State Analysis

### Repl Core (`datalove-repl`)

**Excellent design - very clean and thin:**
- Complete UI separation via `ReplApp` and `ReplExecutor` trait
- Already WASM-ready with serializable types
- WebWorkerExecutor implementation exists
- Worker code already written

**Public Interface:**
```rust
pub struct ReplApp<E: ReplExecutor> {
    pub fn submit_input(&mut self, input_text: String) -> UiAction;
    pub fn poll_results(&mut self) -> Vec<UiAction>;
    pub fn history(&self) -> &[HistoryEntry];
    pub fn environment(&self) -> &[(String, String, String)];
    // Menu/modal state management...
}

pub enum UiAction {
    None,
    SetMultilineInput { lines: Vec<String> },
    ClearInput,
}
```

### Ratatui Frontend (`datalove-repl-rat`)

**Thin wrapper - just widget management:**
- TextArea widget for input
- Three-panel layout (history, input, environment)
- Status indicators: ⏱ parsing, ✓ success, ✗ error, ⇒ expression result
- Color coding: Cyan names, Yellow types, Green success, Red errors
- Modals for menu and crashes

## Implementation Plan

### 1. Create `datalove-web` Crate Structure

```
crates/datalove-web/
├── Cargo.toml
├── src/
│   └── lib.rs
├── index.html
├── style.css
├── app.js
└── Trunk.toml
```

**Cargo.toml dependencies:**
- wasm-bindgen
- serde-wasm-bindgen
- datalove-repl (with WebWorkerExecutor)
- web-sys (for console logging if needed)

**Configure as cdylib:**
```toml
[lib]
crate-type = ["cdylib"]
```

### 2. Implement WASM Bindings (lib.rs)

Create `WebReplApp` struct wrapping `ReplApp<WebWorkerExecutor>`.

**Expose methods with #[wasm_bindgen]:**

```rust
#[wasm_bindgen]
pub struct WebReplApp {
    inner: ReplApp<WebWorkerExecutor>,
}

#[wasm_bindgen]
impl WebReplApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self;

    // Core interactions
    pub fn submit_input(&mut self, text: String) -> JsValue; // Returns UiAction
    pub fn poll_results(&mut self) -> JsValue; // Returns Vec<UiAction>

    // State queries (serialized to JSON)
    pub fn get_history(&self) -> JsValue;
    pub fn get_environment(&self) -> JsValue;

    // Menu/modal management
    pub fn open_menu(&mut self);
    pub fn close_menu(&mut self);
    pub fn resume_from_menu(&mut self);
    pub fn exit_from_menu(&mut self);
    pub fn menu_state(&self) -> JsValue;

    // Crash handling
    pub fn close_crash_modal(&mut self);
    pub fn crash_modal_state(&self) -> JsValue;

    // Exit state
    pub fn is_exit(&self) -> bool;
}
```

**Use serde-wasm-bindgen for all conversions:**
```rust
use serde_wasm_bindgen::to_value;

pub fn get_history(&self) -> JsValue {
    to_value(&self.inner.history()).unwrap()
}
```

### 3. Create HTML Structure (index.html)

**Three-panel layout matching repl-rat:**

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Datalove REPL</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <div id="app">
        <!-- History Panel -->
        <div id="history-panel">
            <div class="panel-title">History</div>
            <div id="history-content"></div>
        </div>

        <!-- Input Panel -->
        <div id="input-panel">
            <div class="panel-title">Input <span id="submit-hint">[Enter]</span></div>
            <textarea id="input-area" rows="1"></textarea>
        </div>

        <!-- Environment Panel -->
        <div id="env-panel">
            <div class="panel-title">Environment</div>
            <table id="env-table"></table>
        </div>

        <!-- Menu Modal -->
        <div id="menu-modal" class="modal hidden">
            <div class="modal-content">
                <h3>Menu</h3>
                <button id="menu-resume">Resume</button>
                <button id="menu-exit">Exit</button>
            </div>
        </div>

        <!-- Crash Modal -->
        <div id="crash-modal" class="modal hidden">
            <div class="modal-content">
                <h3>💥 Crash</h3>
                <pre id="crash-message"></pre>
                <button id="crash-close">Close</button>
            </div>
        </div>
    </div>

    <script type="module" src="app.js"></script>
</body>
</html>
```

**Trunk directives** (inline or in Trunk.toml):
- Auto-inject WASM loading
- Link worker build

### 4. Create CSS Styling (style.css)

**Match ratatui visual design:**

```css
/* Color scheme */
:root {
    --bg-color: #1a1a1a;
    --text-color: #e0e0e0;
    --border-color: #ffffff;
    --cyan: #00ffff;
    --yellow: #ffff00;
    --green: #00ff00;
    --red: #ff0000;
    --multiline-border: #ffff00;
}

/* Layout */
#app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg-color);
    color: var(--text-color);
    font-family: monospace;
}

#history-panel {
    flex: 1;
    border: 1px solid var(--border-color);
    overflow-y: auto;
    padding: 0.5rem;
}

#input-panel {
    min-height: 4rem;
    border: 1px solid var(--border-color);
    padding: 0.5rem;
}

#input-panel.multiline {
    border-color: var(--multiline-border);
    height: 33vh;
}

#env-panel {
    flex: 0 0 auto;
    max-height: 30vh;
    border: 1px solid var(--border-color);
    overflow-y: auto;
    padding: 0.5rem;
}

/* History entries */
.history-entry {
    margin-bottom: 1rem;
    padding: 0.5rem;
    border: 1px solid #444;
}

.history-prompt {
    font-weight: bold;
}

.history-result {
    margin-top: 0.25rem;
    padding-left: 1rem;
}

/* Status symbols and colors */
.status-parsing { color: var(--cyan); }
.status-success { color: var(--green); }
.status-error { color: var(--red); }

.var-name { color: var(--cyan); }
.var-type { color: var(--yellow); }
.var-value { color: var(--text-color); }

/* Modals */
.modal {
    position: fixed;
    top: 0; left: 0;
    width: 100%; height: 100%;
    background: rgba(0, 0, 0, 0.8);
    display: flex;
    align-items: center;
    justify-content: center;
}

.modal.hidden {
    display: none;
}

.modal-content {
    background: var(--bg-color);
    border: 2px solid var(--border-color);
    padding: 2rem;
    min-width: 300px;
}
```

**Additional styling:**
- Panel titles
- Scrollbar styling
- Button styling
- Input textarea styling
- Responsive considerations

### 5. Create JavaScript Application (app.js)

**Main application structure:**

```javascript
import init, { WebReplApp } from './pkg/datalove_web.js';

let app = null;

async function main() {
    // Initialize WASM
    await init();

    // Create app
    app = new WebReplApp();

    // Setup event listeners
    setupInput();
    setupKeyboard();
    setupModals();

    // Start render loop
    requestAnimationFrame(update);
}

function setupInput() {
    const input = document.getElementById('input-area');

    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.altKey && !e.shiftKey) {
            e.preventDefault();
            submitInput();
        } else if (e.key === 'Enter' && e.altKey) {
            e.preventDefault();
            submitInput();
        }
    });
}

function setupKeyboard() {
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            app.open_menu();
        }
    });
}

function setupModals() {
    document.getElementById('menu-resume').onclick = () => app.resume_from_menu();
    document.getElementById('menu-exit').onclick = () => app.exit_from_menu();
    document.getElementById('crash-close').onclick = () => app.close_crash_modal();
}

function submitInput() {
    const input = document.getElementById('input-area');
    const text = input.value;

    const action = app.submit_input(text);
    processUiAction(action);
}

function update() {
    // Poll for results
    const actions = app.poll_results();
    for (const action of actions) {
        processUiAction(action);
    }

    // Render state
    renderHistory();
    renderEnvironment();
    renderModals();

    // Check exit
    if (app.is_exit()) {
        // Handle app exit
        return;
    }

    requestAnimationFrame(update);
}

function processUiAction(action) {
    switch (action.type) {
        case 'None':
            break;
        case 'ClearInput':
            clearInput();
            break;
        case 'SetMultilineInput':
            setMultilineInput(action.lines);
            break;
    }
}

function clearInput() {
    const input = document.getElementById('input-area');
    input.value = '';
    document.getElementById('input-panel').classList.remove('multiline');
    document.getElementById('submit-hint').textContent = '[Enter]';
}

function setMultilineInput(lines) {
    const input = document.getElementById('input-area');
    input.value = lines.join('\n');
    document.getElementById('input-panel').classList.add('multiline');
    document.getElementById('submit-hint').textContent = '[Alt+Enter]';
}

function renderHistory() {
    const history = app.get_history();
    const container = document.getElementById('history-content');

    // Clear and rebuild
    container.innerHTML = '';

    for (const entry of history) {
        const div = createHistoryEntry(entry);
        container.appendChild(div);
    }

    // Auto-scroll to bottom
    container.scrollTop = container.scrollHeight;
}

function createHistoryEntry(entry) {
    const div = document.createElement('div');
    div.className = 'history-entry';

    // Add prompt
    const prompt = document.createElement('div');
    prompt.className = 'history-prompt';
    prompt.textContent = formatPrompt(entry);
    div.appendChild(prompt);

    // Add result (if available)
    if (entry.eval_result) {
        const result = document.createElement('div');
        result.className = 'history-result';
        result.innerHTML = formatResult(entry.eval_result);
        div.appendChild(result);
    } else if (entry.parse_result) {
        const result = document.createElement('div');
        result.className = 'history-result status-parsing';
        result.textContent = '⏱ parsing...';
        div.appendChild(result);
    }

    return div;
}

function formatPrompt(entry) {
    // Format input with status symbol
    // Handle multiline truncation
}

function formatResult(result) {
    // Format based on result type
    // Apply color coding
    // Return HTML string
}

function renderEnvironment() {
    const env = app.get_environment();
    const table = document.getElementById('env-table');

    table.innerHTML = '';

    if (env.length === 0) {
        const tr = table.insertRow();
        const td = tr.insertCell();
        td.colSpan = 3;
        td.textContent = '(no variables defined)';
        return;
    }

    for (const [name, type, value] of env) {
        const tr = table.insertRow();
        tr.innerHTML = `
            <td class="var-name">${name}</td>
            <td class="var-type">${type}</td>
            <td class="var-value">${value}</td>
        `;
    }
}

function renderModals() {
    const menuState = app.menu_state();
    const crashState = app.crash_modal_state();

    document.getElementById('menu-modal').classList.toggle('hidden', !menuState.visible);
    document.getElementById('crash-modal').classList.toggle('hidden', !crashState.visible);

    if (crashState.visible && crashState.message) {
        document.getElementById('crash-message').textContent = crashState.message;
    }
}

main();
```

### 6. Configure Trunk.toml

```toml
[build]
target = "index.html"
dist = "dist"
release = true

[watch]
ignore = ["dist"]

[serve]
port = 8080
address = "127.0.0.1"

[[hooks]]
stage = "post_build"
command = "echo"
command_arguments = ["Build complete!"]
```

**Additional configuration:**
- wasm-opt settings for release builds
- Worker build integration (may need custom build script)

### 7. Update Workspace Cargo.toml

Add to workspace members:
```toml
[workspace]
members = [
    # ... existing members ...
    "crates/datalove-web",
]
```

### 8. Add README (crates/datalove-web/README.md)

```markdown
# Datalove Web REPL

Web frontend for the Datalove REPL.

## Building

Install trunk:
```bash
cargo install trunk
```

Development server:
```bash
cd crates/datalove-web
trunk serve
```

Production build:
```bash
trunk build --release
```

Output will be in `dist/` directory.

## Architecture

- Rust WASM module exposes ReplApp via serialization
- Pure HTML/CSS/JS handles all DOM manipulation
- Uses WebWorkerExecutor for non-blocking evaluation
```

### 9. Testing Plan

**Functionality tests:**
- Build with trunk (both dev and release)
- Verify worker loads correctly
- Test input submission (single-line and multiline)
- Test all UI interactions:
  - Enter to submit
  - Alt+Enter for multiline
  - Esc for menu
  - Menu buttons
- Compare behavior with repl-rat:
  - Same parsing behavior
  - Same evaluation results
  - Same error handling
  - Crash modal works

**Visual tests:**
- Verify three-panel layout
- Check color coding matches
- Test scrolling behavior
- Test multiline mode switching
- Modal display and dismissal

## Implementation Order

1. Create crate structure, Cargo.toml, workspace update
2. Implement WASM bindings (lib.rs) - minimal first
3. Create basic HTML structure
4. Create minimal CSS
5. Create app.js with core loop and input handling
6. Test basic submit/poll cycle
7. Implement history rendering
8. Implement environment rendering
9. Implement modals
10. Refine CSS styling to match repl-rat
11. Add Trunk.toml configuration
12. Write README
13. Full integration testing

## Notes

- Worker WASM build might need custom integration (check how repl-worker builds)
- May need to configure wasm-bindgen features for worker support
- Consider adding loading indicator while WASM initializes
- Error handling for WASM initialization failures
- Browser compatibility (modern browsers only, need ES modules)

## Open Questions

- How to integrate datalove-repl-worker build with Trunk?
  - May need custom build script or post-build hook
  - Worker needs to be accessible at known path
- Should we add TypeScript definitions for better JS ergonomics?
- Performance considerations for large history (virtual scrolling?)

## Current Blocker: WASM Compilation Issue

**Status**: Web frontend code is complete but cannot build due to wasm-bindgen processing error.

**Error**:
```
thread 'main' panicked at crates/wasm-interpreter/src/lib.rs:245:21:
datalove_repl::engine::parse_full_script::_::__ctor::hcfcc3ad4802a3877:
Read a negative address value from the stack. Did we run out of memory?
```

**Root Cause**: wasm-bindgen's wasm-interpreter fails when processing the compiled WASM binary. The issue occurs in Salsa-generated code for `parse_full_script`, suggesting that Salsa's compile-time initialization or const evaluation creates code paths that the wasm-interpreter cannot handle.

**Investigation**:
1. Cargo build to wasm32-unknown-unknown succeeds - WASM binary is generated
2. wasm-bindgen processing fails during optimization/validation phase
3. Error occurs regardless of whether using BlockingExecutor or WebWorkerExecutor
4. The panic is in wasm-bindgen's internal wasm-interpreter, not in our code

**Potential Solutions**:

1. **Salsa Configuration**: Check if Salsa has WASM-specific configuration or features that avoid problematic code generation
2. **Lazy Initialization**: Modify Engine/BlockingExecutor to delay Salsa database creation until first use (not during `new()`)
3. **Simpler Backend**: Create a WASM-specific backend that doesn't use Salsa, or uses a simpler evaluation strategy
4. **wasm-bindgen Options**: Try different wasm-bindgen flags (--no-demangle, --weak-refs, etc.)
5. **Compiler Flags**: Experiment with different Rust codegen options for WASM target
6. **Salsa Update**: Check if newer Salsa versions have better WASM support

**Next Steps**:
1. Investigate Salsa's WASM compatibility and configuration options
2. Try lazy initialization pattern for Engine
3. Consider creating a minimal WASM-compatible evaluator as interim solution
4. Check if datalove-datafun or datalove-datalit can compile to WASM independently
