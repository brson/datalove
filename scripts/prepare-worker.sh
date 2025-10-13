#!/usr/bin/env bash
set -euo pipefail

# Create worker-dist directory for trunk to copy
mkdir -p crates/datalove-repl-egui/worker-dist

# Copy worker files
cp crates/datalove-repl-worker/dist/*.js crates/datalove-repl-egui/worker-dist/
cp crates/datalove-repl-worker/dist/*.wasm crates/datalove-repl-egui/worker-dist/

# Find the worker JS filename
WORKER_JS=$(ls crates/datalove-repl-worker/dist/datalove-repl-worker-*.js | head -1 | xargs basename)

# Create worker.js that imports the actual worker module
echo "importScripts('./$WORKER_JS');" > crates/datalove-repl-egui/worker-dist/worker.js

echo "Worker files prepared. Created worker.js that imports $WORKER_JS"
