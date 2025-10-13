#!/usr/bin/env bash
set -euo pipefail

# Create worker-dist directory for trunk to copy
mkdir -p crates/datalove-repl-egui/worker-dist

# Copy worker files
cp crates/datalove-repl-worker/dist/*.js crates/datalove-repl-egui/worker-dist/
cp crates/datalove-repl-worker/dist/*.wasm crates/datalove-repl-egui/worker-dist/

# Find the worker JS and WASM filenames
WORKER_JS=$(ls crates/datalove-repl-worker/dist/datalove-repl-worker-*.js | head -1 | xargs basename)
WORKER_WASM=$(ls crates/datalove-repl-worker/dist/datalove-repl-worker-*_bg.wasm | head -1 | xargs basename)

# Create worker.js that imports and initializes the worker module.
cat > crates/datalove-repl-egui/worker-dist/worker.js <<EOF
import init, { start } from './$WORKER_JS';

// Initialize WASM with explicit path and start the worker.
init('./$WORKER_WASM').then(() => {
    start();
}).catch(err => {
    console.error('Failed to initialize worker:', err);
});
EOF

echo "Worker files prepared. Created worker.js that imports $WORKER_JS and loads $WORKER_WASM"
