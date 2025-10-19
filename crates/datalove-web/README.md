# Datalove Web REPL

Web frontend for the Datalove REPL.

## Architecture

- **Rust WASM module** exposes ReplApp via serialization protocol
- **Pure HTML/CSS/JavaScript** handles all DOM manipulation and rendering
- Uses `WebWorkerExecutor` for non-blocking evaluation
- Static files only - can be served by any web server

## Prerequisites

Install trunk:

```bash
cargo install trunk
```

Add wasm32 target:

```bash
rustup target add wasm32-unknown-unknown
```

## Development

Run development server with hot reload:

```bash
cd crates/datalove-web
trunk serve
```

Then open http://127.0.0.1:8080 in your browser.

## Production Build

Build optimized release version:

```bash
cd crates/datalove-web
trunk build --release
```

Output will be in `dist/` directory. Serve these static files with any web server:

```bash
# Example with Python
cd dist
python3 -m http.server 8080
```

## Usage

- Type expressions or statements and press Enter
- Use Alt+Enter to submit multiline input
- Press Esc to open the menu
- Press Esc again to close menu or crash modal

## UI Design

The web frontend matches the ratatui terminal frontend design:

- **History Panel**: Scrollable list of interactive cards showing inputs and results
- **Input Panel**: Text area for entering code (switches to multiline mode as needed)
- **Environment Panel**: Table of defined variables and functions

## Implementation

- `lib.rs`: WASM bindings wrapping ReplApp
- `index.html`: Three-panel layout structure
- `style.css`: Styling matching ratatui design
- `app.js`: JavaScript application logic and rendering
- `Trunk.toml`: Build configuration
