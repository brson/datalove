# Testing Under Miri

What runs under Miri, what it costs, and where more coverage would come from.
Written October 2026, after the recipes had rotted and were brought back.

## Contents

- [The problem](#user-content-the-problem)
- [What runs](#user-content-what-runs)
- [Making it affordable](#user-content-making-it-affordable)
- [Compiling natively, running under Miri](#user-content-compiling-natively-running-under-miri)
- [What it found](#user-content-what-it-found)
- [Where coverage is missing](#user-content-where-coverage-is-missing)
- [Next steps](#user-content-next-steps)

## The problem

The unsafe code is concentrated: about 1300 `unsafe` in `datalove-rt`, 400
each in the interpreter, `datalove-rti` and the std rider, 150 in
`datalove-datalit`. The native suites exercise all of it, but a misaligned
write or an aliasing violation passes them silently: glibc's allocator returns
16-aligned memory whatever a `Vec<u8>` promises, and x86 does not trap on an
unaligned load. Miri checks those rules as it runs. But nobody ran the `just
test-miri-*` recipes, so they broke: every runtime test stopped at its first
`RtLocal::new`, which asks `pthread_getattr_np` for the stack bounds, and Miri
does not implement that call.

## What runs

| Recipe | What | Time |
| --- | --- | --- |
| `just test-miri-rt` | `datalove-rt` unit tests, proptests at 8 cases | 1 min |
| `just test-miri-rt-tests` | all 21 rt-tests binaries | ~30 min |
| `just test-miri-libs` | rtdt, datalit and interpreter unit tests | 3 min |
| `just test-miri-interp FILTER` | engine fixtures, walker and bytecode | see below |
| `just test-miri-replay` | all 182 std fixtures, walker and bytecode | ~2 h |

Stacked borrows is the default model and the stricter one for most code. Two
places need tree borrows instead: anything that reaches salsa or rayon, whose
crossbeam-epoch stacked borrows rejects. That is `roundtrip_tests` and every
fixture suite. Tree borrows is not uniformly weaker, though: it rejected
`debuglog_local` reading a local after lending a `*mut` to it, which stacked
borrows allows.

Under Miri, `RtLocal` has no native stack limit (`thread_stack` returns
`None`, as on non-Linux targets), so the interpreter's stack check never fires.

## Making it affordable

Miri runs code a few hundred to a few thousand times slower than native,
unevenly. Three things dominated, none of them the code under test.

**Proptests.** At the default 256 cases the rt-tests proptests run for hours.
The recipes set `PROPTEST_CASES=8`: what Miri checks is the code path, which a
few cases cover.

**Large collections.** A few B-tree tests build trees of 1000 to 5000 keys,
and some check the whole tree's shape after each removal. One of them ran 45
minutes without finishing. Under Miri, `datalove_rt_tests::sizes` and `size`
cap them at 300. The node capacities are 11 and 12, so 300 still reaches
three levels, past the 133/144/145 depth boundaries the tests are built
around.

**The fixture harness.** A profile of one engine fixture under Miri (run
with `-Zmiri-report-progress=N`, which prints the interpreted stack every N
basic blocks and works as a sampling profiler) put 18 of 23 samples in
`ExampleTestRunner` before any fixture ran: `Path` parsing and comparison of
the 2059 directory entries. `find_test_fixtures` now matches and sorts on the
file name as a string. One fixture went from 5m10s to 2m09s; most of what is
left is a fixed cost per run.

That still leaves about three CPU-minutes per fixture, across the seven
configurations Miri runs (walker, bytecode, plain bytecode, and the four
const and specialization variants), almost all of it compiling. The whole
corpus is two days of CPU. A sample is the practical unit: every 8th fixture
plus the bigint ones, 137 in all, ran as three concurrent `cargo miri test`
processes over one build (a running test holds no cargo lock) in 2h20m each.
After the fixes below, all 137 pass.

## Compiling natively, running under Miri

The compiler has no `unsafe` and is nearly all of a fixture's cost under
Miri. For the standard library it is worse than that: `std_engine_tests`
compiles `sys/std` before its first fixture, and under Miri that ran 360
million basic blocks without finishing. So the std rider, with 380 `unsafe`,
was out of reach.

`tests/ir_replay_tests.rs` in `datalove-datafun` splits the two. Run
natively, it compiles each fixture in `fixtures/std_tests` and writes its two
script units (the body, and `output`) to `target/ir-replay/std`, then replays
them natively as a check. Run under Miri, it only replays: it reads the units
back into a `ScriptExecutor::new`, which needs no compiler, registers the std
rider's natives from `datalove_rider_sys_std::symbols()` (linked in, since
Miri cannot load a library), and runs the walker, the bytecode and the plain
bytecode against the expected file. `just test-miri-replay` does both.

Two things made it fast.

- **Each fixture carries only the library functions it can reach.** The
  whole registry is 875 KB of RON, and parsing it under Miri took longer than
  any fixture. Walking `Call` and `ComptimeCall` targets from the two units
  leaves 1 to 58 KB per fixture. That the native replay passes with the
  pruned registry is the check that nothing is reached another way.
- **RON, not JSON.** JSON writes NaN and the infinities as `null`.

A fixture costs seconds to a minute under Miri this way, against hours.

Miri adds a few ULPs of random error to `exp`, `ln`, `pow` and the like,
since platforms are not held to exact results there, which changes
`96_f64_transcendental`'s output. The recipe passes
`-Zmiri-deterministic-floats`.

The same split would make the whole engine corpus affordable: its fixtures
are worldfiles of several sections rather than one script, so it needs a
little more than this test does.

## What it found

Each of these passed the native suites, including under the leak checker.

- **`set a += a` was UB in the runtime.** `int_add_assign_impl` evaluated
  `&mut *a` before reading `b.data` through the shared `b`, and `b` may be
  `a`. The function's own docs say it supports that alias.
- **`data_borrow` scratch was under-aligned.** It writes a `u64` into the
  scratch it is lent; the interpreter's CTFE and a test lent `[u8; 16]`.
  `dyn_ops::place_bits` had the same mistake on its own stack.
- **CTFE's buffers were `Vec<u8>`.** The result and early-return buffers,
  the copy of a packed `data`, and the element and row buffers that
  constants are built in, all hold typed values. Miri placed one at a 2-byte
  address and the interpreter wrote an `Int` there. They are
  `AlignedBuffer`s now; `rider_helpers::write_string_list` had the same
  problem and uses a `Vec<MaybeUninit<String>>`.
- **CTFE destroyed a buffer it never wrote.** It destroyed both the result
  and the early-return buffer, though a unit writes one. The other held
  zeroes, which is no valid `ResultTag`.
- **`debuglog_local` read its temp string around the pointer it lent.**
  Rejected by tree borrows, on every `debuglog` of every fixture.
- **Every native call passed its words as integers.** `call_c` transmuted
  each rider function to `fn(usize, ...) -> u8`, and the riders take
  pointers; integers and pointers are not ABI-compatible, and the cast
  dropped the pointers' provenance. The words are `CWord = *mut u8` now.
- **`clone_impl` held a `&mut RtLocal` across its own recursion.** Each
  recursive call borrows the runtime mutably from the raw handle, which ends
  the outer borrow, and the list, tensor and boxed arms used it afterwards.
  They borrow afresh through `local(rt)`.
- **`debuglog_local` held a `&mut RtLocal` across the pretty-print**, which
  reallocates through the handle, and read the debug mode through it
  afterwards. The replay never logged into the buffer, so a unit test now
  does.
- **The interpreter's static const pool moved a `Box` after lending a
  pointer into it.** Moving a `Box` asserts it is the only way in. The
  storage is a `Vec`.
- **A stale cached layout was caught only in debug builds.** Not Miri's
  find, but the leak's neighbour. The layout cache is keyed by function
  identity, and a replaced body is told apart only by its callers calling
  `forget_*`; the check that the cached layout still fits the body was a
  `debug_assert!`, so a missed `forget` was memory corruption in a release
  build. It is an `assert!`, and so is the check that no frame runs when
  bodies are forgotten.
- **A recursive function's bytecode and layout leaked.** A layout keeps the
  body lowered against it, and the body's call sites keep their callees'
  layouts and bodies, so recursion is a reference cycle. The native leak
  check watches the runtime's allocator, not the Rust heap, and missed it.
  The cycles are broken wherever layouts are let go: when the interpreter
  drops, and in `forget_compiled_bodies` and `forget_unit_bodies`, which an
  edit, a truncation, a module swap and every CTFE evaluation go through.
  The replay reruns each fixture's body in place so that Miri sees the
  second of those.
- **The std rider's `string_remove` and both inserts** passed
  `bytes.as_ptr()` and `bytes.as_mut_ptr()` to one `ptr::copy`; the second
  borrow ends the first.
- **Test bugs:** a destroy through a pointer from `&string`, `element_ptr`
  given the address 0x1000, a test arena moving a `Box` after lending a
  pointer into it, tydescs leaked by `table_tests`.

## Where coverage is missing

**Most of the engine fixture corpus**, for cost, until it is replayed the way
the std fixtures are.

**The std rider's natives that no fixture calls.**

**`datalove-rti`'s call wrappers** have no tests of their own; they run when a
rider calls them.

**The JIT and the AOT backends** are out of reach by construction: Miri does
not run generated code.

## Next steps

In order of payoff per effort.

1. **Replay the engine corpus** as the std fixtures are: a worldfile's
   script sections compiled natively and run under Miri, with the
   specialization variants compiled natively too.
2. **Direct tests of the std rider's natives.** They are `extern "C"`
   functions over an `RtLocal` handle; calling them from a unit test needs
   no compiler, and runs under Miri in seconds.
3. **Tree borrows for the rt-tests as a second pass.** The two models
   catch different things; the debuglog bug is one stacked borrows passed.
4. **A scheduled sweep** of the fixture sample, since it is too slow for CI
   but finds real bugs; a `just` recipe that shards it would do.
