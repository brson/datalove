default:
    just --list

test:
    cargo test --all

serve-wasm-repl:
    cd crates/datalove-repl-wasm && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk serve --release
