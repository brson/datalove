default:
    just --list

test:
    cargo test --all
    cargo check -p datalove-repl-egui
    just check-wasm

# Sanitizer Testing
# =================

# Run all stable sanitizers (address + leak).
test-sanitizers-stable:
    just test-san-address
    just test-san-leak

# Run all nightly sanitizers (memory + thread).
test-sanitizers-nightly:
    just test-san-memory
    just test-san-thread

# Run all supported sanitizers.
test-sanitizers-all:
    just test-sanitizers-stable
    just test-sanitizers-nightly

# AddressSanitizer - detects memory errors (use-after-free, buffer overflows, etc).
# Works on: stable, Linux/macOS/Windows
test-san-address *ARGS='':
    env ASAN_SYMBOLIZER_PATH="$(which llvm-symbolizer-18)" ASAN_OPTIONS="symbolize=1" RUSTFLAGS="-Z sanitizer=address" cargo +nightly test --target x86_64-unknown-linux-gnu -j1 {{ARGS}}

# LeakSanitizer - detects memory leaks.
# Works on: stable, Linux/macOS
test-san-leak *ARGS='':
    env RUSTFLAGS="-Z sanitizer=leak" \
        RUSTDOCFLAGS="-Z sanitizer=leak" \
        cargo +nightly test --all --target x86_64-unknown-linux-gnu {{ARGS}}

# MemorySanitizer - detects use of uninitialized memory.
# Works on: nightly only, Linux only, requires building std from source
test-san-memory *ARGS='':
    env RUSTFLAGS="-Z sanitizer=memory" \
        RUSTDOCFLAGS="-Z sanitizer=memory" \
        cargo +nightly test --all -Zbuild-std --target x86_64-unknown-linux-gnu {{ARGS}}

# ThreadSanitizer - detects data races.
# Works on: nightly only, Linux/macOS, requires building std from source
test-san-thread *ARGS='':
    env RUSTFLAGS="-Z sanitizer=thread" \
        RUSTDOCFLAGS="-Z sanitizer=thread" \
        cargo +nightly test --all -Zbuild-std --target x86_64-unknown-linux-gnu {{ARGS}}

# HWAddressSanitizer - hardware-assisted address sanitizer.
# Works on: nightly only, ARM64 only
test-san-hwaddress *ARGS='':
    env RUSTFLAGS="-Z sanitizer=hwaddress" \
        RUSTDOCFLAGS="-Z sanitizer=hwaddress" \
        cargo +nightly test --all -Zbuild-std --target aarch64-unknown-linux-gnu {{ARGS}}

# ControlFlowIntegrity - control flow integrity checks.
# Works on: nightly only, requires LTO
test-san-cfi *ARGS='':
    env RUSTFLAGS="-Z sanitizer=cfi -Clto" \
        RUSTDOCFLAGS="-Z sanitizer=cfi" \
        cargo +nightly test --all -Zbuild-std --target x86_64-unknown-linux-gnu --release {{ARGS}}

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

