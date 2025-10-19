default:
    just --list

test:
    cargo test --all
    cargo check -p datalove-repl-egui
    just check-wasm

check:
    cargo check --all
    just check-wasm

check-wasm:
    env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo check -p datalove-repl-egui --target=wasm32-unknown-unknown
    env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo check -p datalove-repl-egui --target=wasm32-unknown-unknown
    cd crates/datalove-web && trunk build

build-wasm-repl:
    cd crates/datalove-repl-worker && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release
    ./scripts/prepare-worker.sh
    cd crates/datalove-repl-egui && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release

serve-wasm-repl:
    cd crates/datalove-repl-worker && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release
    ./scripts/prepare-worker.sh
    cd crates/datalove-repl-egui && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk serve --release

serve-wasm-repl2:
    cd crates/datalove-web && trunk serve --release

run-egui-repl:
    cargo run -p datalove-repl-egui

docs:
    cargo run -- docs build

