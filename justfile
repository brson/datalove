default:
    just --list

test:
    cargo test --all
    cargo check -p datalove-repl-egui
    env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo check -p datalove-repl-egui --target=wasm32-unknown-unknown

check-wasm:
    env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo check -p datalove-repl-egui --target=wasm32-unknown-unknown

serve-wasm-repl:
    cd crates/datalove-repl-egui && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk serve --release

run-egui-repl:
    cargo run -p datalove-repl-egui
