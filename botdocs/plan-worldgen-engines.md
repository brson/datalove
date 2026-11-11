# Generated Programs on Every Engine

A plan to hold generated programs to the IR walker on every engine, as the
hand-written engine fixtures are, and to make the generated programs show
enough of what they compute for that to mean something. Written November
2026; phase 1 is done, the rest is not.

## Contents

- [The problem](#user-content-the-problem)
- [One oracle](#user-content-one-oracle)
- [Programs that show what they compute](#user-content-programs-that-show-what-they-compute)
- [Failures become fixtures](#user-content-failures-become-fixtures)
- [Floors](#user-content-floors)
- [What it retires](#user-content-what-it-retires)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## The problem

`engine_tests` runs every fixture in `fixtures/engines/` on the IR walker,
the reference, and on eleven other engines, each of which must observe what
the reference observed. Generated programs never went near it. The suites
that generated them were:

- `worldgen_tests`, which typechecked generated worldfiles, and looked at
  their text for words like `u32`.
- `worldgen_dual_tests`, which ran each on the chaos JIT and on the Cranelift
  AOT and compared the two: no reference, two engines of the twelve, only the
  first script fragment on the AOT side, a program that traps counted a
  failure even when both agreed, and run only under `WORLDGEN_DUAL_TEST=1`.
- `coverage_tests`, which says which syntax the generator writes. It is sound
  and stays.

But the harness was the smaller problem. The twenty default worlds of the
dual suite had, each, one `debuglog`, at the end of the script; function
bodies computed values and dropped them. Two of the twenty had a loop. None
recursed, none could trap (literals are small and non-negative, and `/` is
written only on floats), and none required the standard library. A pass
meant that nothing crashed and one value printed alike.

Therefore: one oracle for fixtures and generated programs both, and a
generator that writes programs worth comparing.

## One oracle

`crates/datalove-datafun/tests/engines/mod.rs` holds what `engine_tests` did
to one program: run it on the reference, run it on each other engine that
applies, and say where they first differ. `engine_tests` reads fixtures and
hands them to it; `worldgen_engine_tests` generates worldfiles and hands them
to it. A new engine added for the fixtures runs generated programs too, and
`DATALOVE_TEST_ENGINES` limits both.

A generated world is a worldfile, as a fixture is. `gen_worldfile_seeded`
writes the script cut into units, a fragment of a few statements or an
expression each, the same cut `gen_session_seeded` makes, since one is the
other written out; `WorldGenConfig::one_fragment` writes it as one fragment,
which the AOT backends need, and the suite asks for that on a share of its
seeds.

## Programs that show what they compute

A generator config for this suite, not the default, so the other suites'
seeds keep what they make:

- Before a unit ends, `debuglog` what it binds that is still alive; before a
  function returns, some of its locals.
- Compute from parameters rather than literals, so that const folding does
  not decide everything at compile time.
- Loops common, and run past the osr engine's threshold of one to three
  iterations (done in phase 1), in functions that are called: 17 of the
  20 default worlds have a loop, but the osr engine enters only two, likely
  because nothing calls the functions most of them are in.
- Recursion bounded by a fuel parameter.
- Programs that trap: a divisor that may be zero, `/!` that may overflow, an
  index that may be past the end. The reference records how the unit ended,
  and the others must end the same way.

## Failures become fixtures

A failing seed is shrunk -- units, then lines, dropped while it fails the
same way -- with the shrinker `generative_reactive_tests` has, moved where
both can use it. Because a shrunk world is a worldfile and the oracle is
shared, it goes into `fixtures/engines/` with an expected file, and is from
then on a regression test on every engine. There is no separate directory of
kept seeds.

## Floors

What a run of the default seeds exercised, checked on the default seeds
alone (`Seeds::is_default`): every unit compiled when the reference ran it;
the chaos dispatcher compiled functions; the osr engine entered loops; the
AOT backends ran on some share of worlds; some share trapped and some share
ran to the end. And a report of the bytecode ops the corpus reached, which is
what "generated is not exercised" in the compiler guide asks for.

## What it retires

- `worldgen_dual_tests`: deleted in phase 1. Both its engines are in the
  oracle, now held to the reference.
- `worldgen_tests`: its typecheck loops go once the new suite fails a world
  any unit of which does not compile, which is stricter, taking in ownership
  and lowering. Its tests that search the text go with them.

## Phases

1. **Done.** The oracle shared; `worldgen_engine_tests` on the default
   generator, its worlds cut into units; `worldgen_dual_tests` deleted. And
   one piece of phase 2, without which the suite's floors fail: every
   generated loop broke at the end of its first round, and was reached so
   rarely that twenty worlds had none, so the osr engine entered no loop in
   three hundred worlds. `counted_loop_probability` makes a loop break after
   two to six rounds, and reaches for one first; over 1000 seeds the chaos
   dispatcher then entered 36 loops and the osr engine 33. Every one of the
   1000 agreed with the reference on every engine.
2. **Partly done.** The observing config, three settings that draw nothing
   when off: `observe_bindings` (a script statement shows what it binds, a
   function what it holds before it returns), `call_every_function` (the
   script ends by calling what it has not), and `fallible_script` (the top
   level may leave early by `!`, a unit being checked as returning `!()`).
   Over 1000 seeds that is about 9300 lines of debug log, against about one a
   world before, and 366 fragments ending in an error and 102 blocked by one.
   The first runs found five bugs, each now a fixture:
   - A move in a `loop while` condition was not refused, and a second read
     of the condition found the binding gone; the IR walker panicked when it
     was an earlier unit's (`1024`).
   - `unwrap_result` and `unwrap_option` marked both sides live in a script
     frame, and a unit leaving by the error side kept garbage it destroyed at
     the session's end (`1025`).
   - A unit leaving early before it moved a `let` leaked it: the frame keeps
     what is live at the unit's end, and the early exit dropped no unit
     binding (`1025`).
   - Script lowering looked up an early return's drops by the statement's
     place among the top-level ones, which ownership numbers counting nested
     statements too: after a loop, an exit found the next statement's drops.
     This was also the Cranelift AOT "uses value from non-dominating inst"
     verifier error the dual suite had listed as known (`1025`).
   - Which units a unit reads from was taken from its IR after the passes,
     and dead code elimination took away a read whose value went unused, so a
     unit reading a failed unit's binding ran with consts inlined and was
     blocked without (`1026`).

   Still to do: computation from parameters, recursion, and qualified calls,
   since the script imports only the first function of each name and every
   module names its functions `fn0`, `fn1` and so on.
3. Shrinking, into `fixtures/engines/`; the floors. And each seed in a
   process of its own, or something like it: a crash that is not a panic --
   one early run segfaulted -- takes the whole run down without saying which
   seed it was. Panics are caught per seed already.
4. What the generator never writes, each taken off `NOT_YET_GENERATED` in
   `coverage_tests` as it lands: standard library calls, data files in
   worldfiles, `rec` types, hex literals, `icall`, a test rider. Then
   `worldgen_tests` retired.

## Open questions

- How many seeds `just test` can afford, every applicable one linking two
  executables, one of them through `cc`.
- Whether the AOT backends should take a program the reference ended in an
  error, compared by exit status and stderr, rather than being skipped for it.
