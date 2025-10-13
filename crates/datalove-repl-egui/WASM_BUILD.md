# Building the WASM REPL with Web Workers

The WASM build uses a Web Worker for non-blocking REPL evaluation.
This requires building two separate WASM modules:

## Build Process

### 1. Build the Worker

```bash
cd crates/datalove-repl-worker
env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release
cd ../..
```

### 2. Copy Worker Files

Or manually:
```bash
cp ../datalove-repl-worker/dist/*.js crates/datalove-repl-egui/dist/
cp ../datalove-repl-worker/dist/*.wasm crates/datalove-repl-egui/dist/
```

### 3. Build the Main App

```bash
cd crates/datalove-repl-egui
env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release
```

### 4. Serve

```bash
trunk serve
```

## Architecture

- **Main Thread** (`datalove-repl-egui`): Handles UI rendering with egui/ratatui
- **Worker Thread** (`datalove-repl-worker`): Runs the REPL engine for parse/eval operations
- **Communication**: JSON messages via `postMessage` API

The `WebWorkerExecutor` in `datalove-repl-rat` manages the worker lifecycle and message passing.
