default:
    just --list

test:
    cargo test --all --all-targets

# Run slow tests (proptests, backtrace tests, etc).
test-slow:
    cargo test -p datalove-rt --features slow_tests
    cargo test -p datalove-rt-tests --features slow_tests
    cargo test -p datalove-datafun-compiler --features slow_tests

# Time all tests, showing only tests that take over 1 second.
test-time:
    env RUST_TEST_TIME_UNIT=1000,10000 \
        RUST_TEST_TIME_INTEGRATION=1000,10000 \
        RUST_TEST_TIME_DOCTEST=1000,10000 \
        cargo +nightly test --all -- -Zunstable-options --report-time 2>&1 | rg '\s+<\d+\.\d+s>'

# Time all tests, showing all tests with slow tests highlighted (>1s warn, >10s critical).
test-time-all:
    env RUST_TEST_TIME_UNIT=1000,10000 \
        RUST_TEST_TIME_INTEGRATION=1000,10000 \
        RUST_TEST_TIME_DOCTEST=1000,10000 \
        cargo +nightly test --all -- -Zunstable-options --report-time

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

build-wasm-repl:
    cd crates/datalove-repl-worker && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release
    ./scripts/prepare-worker.sh
    cd crates/datalove-repl-egui && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release

serve-wasm-repl:
    cd crates/datalove-repl-worker && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk build --release
    ./scripts/prepare-worker.sh
    cd crates/datalove-repl-egui && env RUSTFLAGS='--cfg getrandom_backend="wasm_js"' trunk serve --release

loc:
    tokei
    echo
    fd -e dlt -e dfs -e dfm -e dls -e dlm -e world -e repl | xargs wc -l | tail -1
