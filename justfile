default:
    just --list

test:
    cargo test --all --all-targets

test-slow:
    cargo test -p datalove-rt --features slow_tests
    cargo test -p datalove-rt-tests --features slow_tests
    cargo test -p datalove-datafun-compiler --features slow_tests
    just check-wasm

test-ci: test test-slow

# Run tests with parallelism enabled.
test-parallel:
    DATALOVE_PARALLEL=1 cargo test --all --all-targets

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

# Benchmark parallel vs sequential module parsing.
bench-parse:
    cargo test -p datalove-datafun-compiler --lib --release bench_parallel_parsing -- --nocapture --ignored

# Benchmark parallel vs sequential module parsing (debug mode).
bench-parse-debug:
    cargo test -p datalove-datafun-compiler --lib bench_parallel_parsing -- --nocapture --ignored

# Benchmark parallel vs sequential typechecking.
bench-typecheck:
    cargo test -p datalove-datafun-compiler --lib --release bench_parallel_typechecking -- --nocapture --ignored

# Benchmark parallel vs sequential typechecking (debug mode).
bench-typecheck-debug:
    cargo test -p datalove-datafun-compiler --lib bench_parallel_typechecking -- --nocapture --ignored

# Run all parallelization benchmarks.
bench-parallel:
    just bench-parse
    just bench-typecheck

test-sanitizers-all:
    just test-sanitizers-stable
    just test-sanitizers-nightly

test-san-address *ARGS='':
    env ASAN_SYMBOLIZER_PATH="$(which llvm-symbolizer-18)" ASAN_OPTIONS="symbolize=1" RUSTFLAGS="-Z sanitizer=address" cargo +nightly test --target x86_64-unknown-linux-gnu -j1 {{ARGS}}

test-san-leak *ARGS='':
    env RUSTFLAGS="-Z sanitizer=leak" \
        RUSTDOCFLAGS="-Z sanitizer=leak" \
        cargo +nightly test --all --target x86_64-unknown-linux-gnu {{ARGS}}

test-san-memory *ARGS='':
    env RUSTFLAGS="-Z sanitizer=memory" \
        RUSTDOCFLAGS="-Z sanitizer=memory" \
        cargo +nightly test --all -Zbuild-std --target x86_64-unknown-linux-gnu {{ARGS}}

test-miri-rt *ARGS='':
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt --lib -- --skip proptest {{ARGS}}

test-miri-rt-one TEST:
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt --lib {{TEST}}

test-miri-rt-tests *ARGS='':
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test int_math_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test eq_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test string_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test cmp_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test rust_api_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test anypack_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test pretty_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test eq_unique_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test cmp_total_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test list_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test tensor_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test btreeset_tests {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test btreemap_tests {{ARGS}}

test-miri-rt-tests-one TEST_FILE *ARGS='':
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test {{TEST_FILE}} {{ARGS}}

test-miri-interp *ARGS='':
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-datafun --test interp_tests {{ARGS}}

test-miri-module-interp *ARGS='':
    env MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-datafun --test module_interp_tests {{ARGS}}

test-miri-interp-all *ARGS='':
    just test-miri-interp3 {{ARGS}}
    just test-miri-module-interp3 {{ARGS}}

check:
    cargo check --all

check-wasm:
    cargo check -p datalove --target wasm32-unknown-unknown

loc:
    tokei
    echo
    fd -e dlt -e dfs -e dfm -e dls -e dlm -e world -e repl | xargs wc -l | tail -1

doc:
    cargo run -p datalove-cli -- docs

# Show module memoization test results table.
memo-table:
    python3 scripts/memo-table.py
