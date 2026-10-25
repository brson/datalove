# Testing Under Miri

What runs under Miri, what it costs, and where more coverage would come from.
Written October 2026, after the recipes had rotted and were brought back.

## Contents

- [The problem](#user-content-the-problem)
- [What runs](#user-content-what-runs)
- [Making it affordable](#user-content-making-it-affordable)
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
- **Test bugs:** a destroy through a pointer from `&string`, `element_ptr`
  given the address 0x1000, a test arena moving a `Box` after lending a
  pointer into it, tydescs leaked by `table_tests`.

## Where coverage is missing

**The std rider.** Its 380 `unsafe` run only from `std_engine_tests`, which
compiles `sys/std` twice per worker before any fixture. Under Miri that
compile ran 360 million basic blocks without finishing. The rider cannot be
reached this way.

**Most of the fixture corpus**, for cost, as above.

**`datalove-rti`'s call wrappers** have no tests of their own; they run when a
rider calls them.

**The JIT and the AOT backends** are out of reach by construction: Miri does
not run generated code.

## Next steps

In order of payoff per effort.

1. **Compile natively, execute under Miri.** Compilation has no `unsafe` and
   is most of the cost of every fixture. The IR already round-trips through
   RON (`ir_serial_tests` runs deserialized IR on the interpreter). A native
   pass would write each fixture's code units and registry; a Miri pass would
   load them and run only the walker and the bytecode against the expected
   output. That makes the whole engine corpus affordable, and it is the only
   way to the std rider: the std library's IR serialized once, its fixtures
   run under Miri with the linked natives.
2. **Direct tests of the std rider's natives.** They are `extern "C"`
   functions over an `RtLocal` handle; calling them from a unit test needs
   no compiler, and runs under Miri in seconds.
3. **Tree borrows for the rt-tests as a second pass.** The two models
   catch different things; the debuglog bug is one stacked borrows passed.
4. **A scheduled sweep** of the fixture sample, since it is too slow for CI
   but finds real bugs; a `just` recipe that shards it would do.
