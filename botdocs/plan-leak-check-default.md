# Turning the Leak Checker Off by Default, Without Losing It

`LeakCheckMode::from_env` returns `Panic` when `DATALOVE_LEAK_CHECK` is unset, so every
release `datalove` run tracks every allocation in a hash map. That costs 17% of an
allocation-heavy program (`botdocs/reports/report-runtime-allocator.md`). The default should
be `Ignore`. The question is what has to happen alongside so the test suite still checks it,
verifiably.

This is the investigation, not the change.

## What the default actually controls

More than a leak report. `Ignore` skips three things:

1. **Leak detection** at `shutdown`, where a non-empty `active_allocations` is reported.
2. **Double-free and invalid-pointer detection**, on every `free`: a pointer not in the map
   is `free() called on untracked pointer`.
3. **Parameter-mismatch detection**, on every `free`: the size, align and count handed to
   `free` must match what `alloc` recorded.

So flipping the default does not merely stop printing something at exit, it removes the
runtime's self-checks from shipped builds. That is a normal thing for a release build to do
and it is still worth saying out loud, because 2 and 3 catch codegen bugs rather than leaks,
and they are the ones that fire *during* a run.

## The suite does not currently verify any of it

This is the finding that matters, and it is true today, before any change:

```
DATALOVE_LEAK_CHECK=ignore just test     ->  84 suites, exit 0
```

**Everything passes with the leak checker off.** So the hundred-odd fixtures that exist to be
checked by it -- `135_nested_early_return_drops.dfs` says "There is nothing to check but the
leak detector and the answer" -- would keep passing if it silently stopped working. They
assert that nothing leaks by *not leaking*, which is unfalsifiable from the harness's side.

What does pin the checker is the unit tests in `alloc.rs`: `test_leak_detection_panic_mode_small`,
`..._large`, `..._multiple`, `..._with_backtrace` and friends. Every one of them constructs
its allocator with `new_raw_with_leak_check_mode(LeakCheckMode::Panic)`, so **they are immune
to a default flip** -- the mechanism stays tested either way. What no test covers is whether
the mode is actually on in a process that is running fixtures.

So there is an existing gap, and flipping the default does not create it. It does make it
matter.

## The environment variable is the only mechanism that works here

Three candidate mechanisms, and two of them break on something specific to this tree.

**`cfg!(debug_assertions)`** -- on in debug, off in release, no plumbing. It fails because
the runtime is compiled in two profiles at once:

| | profile |
|---|---|
| `aot.rs`, building `datalove-rt` for AOT linking | **debug**, hardcoded `target/debug`, no `--release` |
| `rider_build.rs`, building the rider component | **release**, explicit `--release` |

And as `botdocs/issues.md` records, a rider-using process contains *both* copies and they
share `AllocLocal`. With a `cfg!` default the two copies disagree about leak checking, and
which one wins depends on which happens to construct the allocator. Deterministic, but not
something anyone should have to reason about.

**A process-global default** (`AtomicU8`, set once by a harness before any runtime exists)
fails the same way and for the same reason: each copy of `datalove-rt` has its own statics,
so setting it in one does not set it in the other.

**The environment variable** is read at construction by whichever copy constructs, and both
copies read the same environment. It is the only one of the three that is consistent across
the duplication. That is a somewhat grim reason to prefer it, and it is the reason.

## One variable covers the whole matrix

No spawn site in the tree calls `.env_clear()` or sets `.env(...)` -- checked. So children
inherit, and `DATALOVE_LEAK_CHECK=panic` in front of `cargo test` reaches all three places a
runtime runs:

| where | how it gets the variable |
|---|---|
| in-process runtime in a test binary (interp, jit, rt-tests) | `cargo test` inherits from the recipe |
| AOT-built executables, via `run_executable` | `Command::output()` inherits from the test process |
| the `datalove` binary, spawned by the 6 `datalove-cli` test targets | same |

That is 36 test targets covered by one line per recipe and no code change beyond the
default. The justfile already has the idiom -- `test-parallel` is
`DATALOVE_PARALLEL=1 cargo test --all ...` -- so this reads as existing practice:

```
test:
    cargo check --all --benches
    DATALOVE_LEAK_CHECK=panic cargo test --all --lib --bins --tests --examples
```

and the same in `test-64`, `test-slow`, `test-parallel`, and whatever CI invokes.

## It costs the suite nothing

Worth knowing before deciding how much of the suite to turn it on for: on `std_all_tests`,
the allocation-heaviest suite, tracking is free.

| | run 1 | run 2 |
|---|---|---|
| default (`Panic`) | 157.5s | 164.5s |
| `DATALOVE_LEAK_CHECK=ignore` | 158.3s | 165.3s |

No difference. The suite is dominated by compilation and `cc`, not by runtime allocation. The
17% figure is about an allocation-heavy datalove *program*, which no fixture is. So there is
no performance argument for turning it on selectively -- turn it on everywhere it can go.

## What is left to decide

**1. How to make it verifiable, which is the only real work here.**

You cannot have both "a bare `cargo test` passes with the variable unset" and "the suite
fails if the variable goes missing". Those are the same condition with opposite signs. So:

- **Assert it.** The harnesses that exercise the runtime check the effective mode at startup
  and fail with "leak checking is off; run `just test` or set `DATALOVE_LEAK_CHECK=panic`".
  A bare `cargo test -p datalove-datafun` then fails until you set it. Given CLAUDE.md
  already says to run `just test` before calling a task done, that fits the project's
  existing contract, and it is the only option that actually catches a lost variable.
- **Document it and accept the gap.** Only `just test` and CI have leak checking, nothing
  enforces that, and a future edit to the justfile can quietly remove it.

I would assert it, in one place rather than in every harness: a single test in
`datalove-rt-tests` that allocates through `dtlv_rti_init`, shuts down, and expects the
panic **with the mode taken from the environment** rather than passed explicitly. That test
is the canary. It fails if the recipe loses the variable, it does not need a language-level
leak to exist, and it does not duplicate the `alloc.rs` unit tests, which test the mechanism
rather than the wiring.

**2. Whether the cli tests should have it on at all.**

You raised this, and blanket inheritance gets it backwards. The six `datalove-cli` test
targets exist to test the shipping binary, and after this change the shipping binary has
leak checking **off**. If `just test` sets the variable globally, those tests exercise a
configuration no user will run -- which is precisely the kind of divergence a
spawn-the-real-binary test is there to catch.

So either they clear it for their children (`.env_remove("DATALOVE_LEAK_CHECK")` at the
spawn sites, which is where a helper would earn its place -- `script_tests.rs` builds its
`Command` inline and the other five presumably do too), or the variable is set per-recipe
rather than for the whole run. The first is more honest about what each suite is for.

Note the tension: turning it off for those suites means the CLI paths lose the free
double-free and parameter-mismatch checking that the AOT and interpreter suites keep. That is
an argument for the cli tests being few and about argument handling and output, with the
semantics covered elsewhere -- which looks like what they are, but it was not checked.

**3. Nothing needs to change about `run_executable`.**

Inheriting the parent's variable is the right behaviour and is already what happens. A
tidier version would have it pass on the parent's *effective* mode explicitly, so that a
child inherits the decision rather than the environment, but that is not needed for any of
the above and would be a change to a function the cli also calls.

## Done

The default is `Ignore` and the justfile sets `DATALOVE_LEAK_CHECK=panic` on `test`,
`test-64`, `test-slow` and `test-parallel`.

**The framing in "What the default actually controls" above was right, and the conclusion
drawn from it was wrong.** Yes, `Ignore` gives up the double-free, untracked-pointer and
parameter-mismatch checks as well as the leak report. But those checks read the same
`active_allocations` the report enumerates, so they cannot be kept without keeping the map,
and the map is the entire 13-17%. Moving them into the production path was tried, measured at
no saving at all, and undone.

The reason they belong behind the switch is about the language: **datalove is safe.** A
program cannot reach `free` with the wrong size, or free the same pointer twice, by being
written badly -- only by the compiler lowering a drop wrongly. That is a compiler bug, the
test suite is where it should be caught, and the justfile is what turns the checks on there.
A shipped binary re-checking the compiler on every allocation is the wrong place to spend
17%.

Still not done, and unchanged by any of this:

- **Verifiability.** `DATALOVE_LEAK_CHECK=ignore just test` still passes all 84 suites, so
  nothing fails if the justfile loses the variable. The canary test in option 1 above would
  fix it and was not written.
- **The cli tests.** They inherit `panic` from the recipe, so they exercise a configuration
  no user runs. Since the checks are once again the thing the variable controls, this is back
  to mattering as much as it did when you raised it.
