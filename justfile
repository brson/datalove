default:
    just --list

# Leak checking, which the runtime leaves off unless asked.
#
# Set on every recipe that runs tests, and on those three CI runs in particular.
# A shipped binary defaults to off because tracking every live allocation so that
# shutdown can list what leaked costs 17% of an allocation-heavy program; the
# suite has no such cost and a hundred-odd fixtures exist to be checked by it.
#
# Children inherit it, so this one variable also reaches the executables the AOT
# backends build and the `datalove` binary the cli tests spawn.
LEAK_CHECK := "panic"

# Run the tests, and check benches compile since they are not tested.
test:
    cargo check --all --benches
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples

# Run the tests with sys riders built and loaded rather than linked in.
#
# A sys rider is compiled into the binary, so the interpreter takes its
# addresses directly: no cargo, no component, no dlopen. That is worth having
# and it means the path every other rider takes goes untested for exactly the
# riders this suite leans on hardest. Same suite, the other arrangement
# underneath.
test-sys-riders:
    DATALOVE_BUILD_SYS_RIDERS=1 DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples

# Run tests with 64-bit collection indexes.
test-64:
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples --features index-64

test-slow:
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-rt --features slow_tests
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-rt-tests --features slow_tests
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-datafun-compiler --features slow_tests
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-datafun --features slow_tests
    just check-wasm

# Everything CI runs, which it does one configuration per runner.
test-ci: test test-slow test-64 test-sys-riders

# Run tests with parallelism enabled.
test-parallel:
    DATALOVE_PARALLEL=1 DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples

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

# Run all benchmarks.
bench:
    cargo bench -p datalove-bench

# Run benchmarks matching filter.
bench-filter FILTER:
    cargo bench -p datalove-bench -- {{FILTER}}

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

# Install the `datalove` binary from this working copy.
install:
    cargo install --path crates/datalove-cli --locked

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

# Regenerate the runtime interface declarations from the runtime's definitions.
#
# Run after adding, removing or changing a `dtlv_rti_*` function in
# `datalove-rt`'s `c.rs`. Leaving it unrun is a compile error rather than a
# quiet mismatch; see `datalove-rt`'s `abi_check` module.
gen-rti:
    uv run --no-project scripts/gen-rti.py

# Check that every crate would publish, without publishing any.
#
# Manifests only. `--no-verify` skips building each packaged crate, which is
# the slow half and says nothing a normal build does not.
publish-check:
    cargo publish --dry-run --workspace --no-verify

# Build a local registry holding this workspace, for trying the release path.
#
# Vendors the third-party crates, packages ours, and puts both in one
# directory a cargo config can stand in for crates.io with. See
# `scripts/local-registry.py` for what to do with it.
#
# This is the answer to the release path being untestable until a release:
# it is testable, against crates nobody published.
local-registry DIR="target/local-registry":
    cargo vendor --versioned-dirs {{DIR}}
    cargo package --workspace --no-verify
    uv run --no-project scripts/local-registry.py {{DIR}}

# Publish the workspace.
#
# `bcts` is excluded: it is versioned on its own schedule and 0.7.0 is already
# up, which cargo would refuse rather than skip. Bump it and drop the
# exclusion when it next changes.
#
# Not atomic. Verification happens before any upload, so a failure part way is
# a network or rate-limit problem rather than a manifest one, and leaves some
# crates published. Re-running skips nothing, so pick up from what is left.
publish:
    cargo publish --workspace --exclude bcts

benchvs:
    cd benchvs && just run

