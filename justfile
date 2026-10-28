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

# Run the tests, check benches compile since they are not tested, and check
# the runtime interface is what `gen-rti` would write.
test:
    uv run --no-project scripts/gen-rti.py --check
    cargo check --all --benches
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples

# Smoke test the sys riders built and loaded rather than linked in.
#
# A sys rider is compiled into the binary, so the interpreter takes its
# addresses directly: no cargo, no component, no dlopen. That is worth having
# and it means the path every other rider takes goes untested for exactly the
# riders this suite leans on hardest. The binary's script tests are enough to
# see that the riders build and load: they run through the driver the switch
# reaches, and `04_u32_bits` calls a native.
test-sys-riders:
    DATALOVE_BUILD_SYS_RIDERS=1 DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-cli --test script_tests

# Run tests with 64-bit collection indexes.
test-64:
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples --features index-64

# The slow configuration: the property tests and exhaustive cases, the wasm
# build and the wasm smoke test, the sys riders built from source, and
# generated programs.
test-slow:
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-rt --features slow_tests
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-rt-tests --features slow_tests
    DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-datafun-compiler --features slow_tests
    just check-wasm
    just test-wasm
    just test-sys-riders
    just test-worldgen

# Generated programs, run on the interpreter under a chaos dispatcher and
# through Cranelift AOT. CI's seed is fixed so that a failure is a change, not
# a draw; pass another to explore.
test-worldgen SEED='2000':
    WORLDGEN_DUAL_TEST=1 WORLDGEN_DUAL_SEED={{SEED}} DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test -p datalove-tests --test worldgen_dual_tests

# The programs outside the test suite, in release: the benchvs benches built
# and AOT-compiled, and the store demo's report checked against its Python
# twin on a small dataset. Needs uv and a C compiler.
test-demos:
    cd benchvs && just build
    cd demos/store && just gen 2000 && just check

# The slow configuration and the demos, which CI runs as one job.
test-extras: test-slow test-demos

# Everything CI runs, which it does one configuration per runner.
test-ci: test test-64 test-cut test-extras

# Run tests with sequential compilation, which is not part of CI.
#
# A parallel compile is the sequential one with each step's per-module queries
# computed ahead of it, so `test` runs every line this does; this is for when
# that needs checking rather than trusting. The engine suites run the
# reference and the bytecode only, with const inlining and specialization each
# turned off, which lower differently.
test-sequential:
    DATALOVE_PARALLEL=0 DATALOVE_TEST_ENGINES=bytecode,noconst,nospec DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples

# Run tests with every datalit collection literal parsed in chunks, after every
# element, which a parallel compile parses ahead of time as it would; see
# `bct::chunk`. A collection in chunks parses to what it parses to whole, so
# nothing a test checks may change. The engine suites run as for
# `test-sequential`, since chunks stop at the parse.
test-cut:
    DATALOVE_CUT_COLLECTIONS=1 DATALOVE_TEST_ENGINES=bytecode,noconst,nospec DATALOVE_LEAK_CHECK={{LEAK_CHECK}} cargo test --all --lib --bins --tests --examples

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
    just test-san-address
    just test-san-leak
    just test-san-memory

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

# Proptests run under Miri with few cases; at the default 256 a run takes hours.
test-miri-rt *ARGS='':
    env PROPTEST_CASES=8 MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt --lib {{ARGS}}

test-miri-rt-one TEST:
    env PROPTEST_CASES=8 MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt --lib {{TEST}}

# Every rt-tests binary but the slow-tests proptests. `roundtrip_tests` goes
# through salsa, whose crossbeam-epoch stacked borrows rejects, so it runs
# under tree borrows; the rest under stacked borrows, which is stricter.
# The largest collection sizes are dropped under Miri; see
# `datalove_rt_tests::sizes`.
test-miri-rt-tests *ARGS='':
    env PROPTEST_CASES=8 MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --no-fail-fast \
        --test anypack_tests --test boxing_tests --test btreemap_tests --test btreeset_tests --test clone_tests --test cmp_tests --test cmp_total_tests --test cursor_tests --test destroy_tests --test eq_tests --test eq_unique_tests --test instantiate_tests --test int_math_tests --test list_generic_tests --test list_tests --test pretty_tests --test rust_api_tests --test string_tests --test table_tests --test tensor_tests \
        {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows" \
        cargo +nightly miri test -p datalove-rt-tests --test roundtrip_tests {{ARGS}}

test-miri-rt-tests-one TEST_FILE *ARGS='':
    env PROPTEST_CASES=8 MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test -p datalove-rt-tests --test {{TEST_FILE}} {{ARGS}}

# The unit tests of the other crates with unsafe code: the descriptor types,
# datalit's instantiation, and the interpreter's own.
test-miri-libs *ARGS='':
    env PROPTEST_CASES=8 MIRIFLAGS="-Zmiri-disable-isolation" \
        cargo +nightly miri test --no-fail-fast \
        -p datalove-rtdt -p datalove-datalit -p datalove-datafun-interp --lib {{ARGS}}

# The engine fixtures under Miri, on the IR walker and the bytecode: the
# bytecode's frame access is raw pointer arithmetic, which this checks as it
# runs. The runner leaves out the engines that generate code.
#
# Tree borrows, because the test harness's rayon pulls in crossbeam-epoch,
# which stacked borrows rejects and tree borrows accepts; under stacked
# borrows the run stops in crossbeam before reaching the interpreter.
test-miri-interp *ARGS='':
    env MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows" \
        cargo +nightly miri test -p datalove-datafun --test engine_tests {{ARGS}}

# The standard library fixtures under Miri, with the compiling done natively:
# the native run compiles each fixture and writes its IR to target/ir-replay,
# and the Miri run reads it back and runs it on the walker and the bytecode,
# calling the std rider's natives. Seconds a fixture, where compiling under
# Miri takes hours. Arguments filter fixtures by name. Deterministic floats,
# since Miri otherwise adds error to `exp` and the like, as platforms may.
test-miri-replay *ARGS='':
    cargo test -p datalove-datafun --test ir_replay_tests -- {{ARGS}}
    env MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-deterministic-floats" \
        cargo +nightly miri test -p datalove-datafun --test ir_replay_tests -- {{ARGS}}

# Install the `datalove` binary from this working copy.
install:
    cargo install --path crates/datalove-cli --locked

check:
    cargo check --all

check-wasm:
    cargo check -p datalove --target wasm32-unknown-unknown

# Compile the system library and run a script on the bytecode in wasm, under
# wasmi, where there are no threads: sequentially and with the parallel paths,
# which rayon runs on the calling thread there.
test-wasm:
    cargo test --release -p datalove-wasm-smoke --test wasm_smoke -- --ignored

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
# `datalove-rt`'s `c/mod.rs`. Leaving it unrun is a compile error rather than a
# quiet mismatch, since the runtime fills the table it generates; `just test`
# runs it with `--check`, which also holds each export to the rules it checks.
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

# Publish the workspace in one shot.
#
# Right for a release of crates that are all already up, which rate limits
# barely touch: a new version of an existing name gets a burst of thirty and a
# refill a minute. Wrong for a first publish and wrong for resuming a run that
# stopped part way, because cargo checks the whole set against the index
# before it uploads anything and fails on the first crate already published.
# Use `publish-drip` for those.
publish:
    cargo publish --workspace

# Publish the workspace a crate at a time, waiting out the rate limit.
#
# A new crate name is limited to one per ten minutes after a burst of five, so
# a first publish of this workspace stops after five and 429s, and cargo has
# no way to pace itself. This reads the index to see what is already up,
# excludes it, publishes what is left, and sleeps when the registry says to.
# It verifies the whole workspace once at the start and publishes
# `--no-verify` thereafter, so the waiting is not spent rebuilding. Hours,
# then: interruptible and re-runnable. See `botdocs/release.md`.
publish-drip:
    uv run --no-project scripts/publish-drip.py

benchvs:
    cd benchvs && just run

