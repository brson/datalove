# Generated Programs on Every Engine

A plan to hold generated programs to the IR walker on every engine, as the
hand-written engine fixtures are, and to make the generated programs show
enough of what they compute for that to mean something. Written November
2026; done, but for a test rider (phase 4).

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

A failing seed is shrunk -- sections, blocks, then lines, dropped while it
fails the same way. Because a shrunk world is a worldfile and the oracle is
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
2. **Done.** The observing config, three settings that draw nothing
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

   Then three more settings:
   `qualified_calls` (a function whose name is already imported is called
   as `util_0.fn1(...)`, from the script and from modules, about one call a
   world in two), `compute_from_variables` (bindings, parameters among them,
   read twice as often as before, and a copy read no longer counted as a
   move), and `recursion_probability` (a module function whose parameters
   are all `in` takes `fuel: int`, returns early unless it is 1 to
   `MAX_FUEL`, 4, and calls itself once with one less; a call passes 1 to 4
   most of the time). Over 1000 seeds that is about 15800 lines of debug
   log. `WORLDGEN_ENGINES_WRITE` writes out every world checked, to be read.
   One bug in 4000 seeds, and a fixture:
   - A call dropped an `out` argument's destination as it lowered that
     argument, before the arguments after it, and one of them leaving early
     left the destination dropped and live: the IR walker read an empty slot
     in a function, and in a script unit an assertion in the IR broke
     (`1027`). The drops now come after every argument.

   That one aborted the run rather than failing a seed: the leak check
   panicked in a destructor while the first panic unwound. It reports
   instead now, while a thread panics, so the seed is caught and named.
3. **Done.** Each seed is checked in a worker process, this test run again,
   one a CPU, each sent a seed at a time on stdin, saying how it went and
   what the engines did over it on a line of its own: a seed that crashes its
   worker, or takes more than two minutes, fails alone and by name, and a new
   worker takes the next seed. That costs about a fifth of the time with four
   workers -- one worker is as quick as one thread was -- and the planted
   abort and hang that tested it each failed only their seeds.
   The oracle's counts are a `Tally` a worker sends back, the bytecode's op
   kinds and the IR left to the walker among them, which the run reports.

   A failing world is shrunk: sections, then whole blocks, then runs of
   lines halving down to one, each dropped while what is left fails the same
   way, each candidate checked in a process of its own. The same way is the
   first two lines of the failure with numbers masked, or for an engine that
   differs from the reference, which engine. The world that found `1027`
   shrank from 249 lines to 32 in ten seconds. A shrunk world is written out
   beside the original; making it a fixture, with a comment saying what it
   shows, is left to whoever reads it. The shrinker is not
   `generative_reactive_tests`', which takes apart a session's changes
   rather than a worldfile's text; the two share only the idea.

   Floors on the default seeds, now 40: the chaos dispatcher compiled
   functions and entered loops, the osr engine entered loops, fragments ran
   to the end, returned early with an error and were blocked by one that
   did, and each AOT backend ran on a tenth of the worlds. At 20 seeds no
   fragment was blocked, which is why there are 40.

   5000 seeds from 5000000 then found two bugs, and shrinking a third:
   - The generator gave a call's `ref`, `mut` and `out` parameters bindings
     at random, and one pick could take what a later one needed, which
     `can_call`'s greedy check had found for it; the generator panicked. A
     pick now leaves a binding for each after it. Its panic was outside the
     one caught, and took down the run where it wrote the world out again;
     the generator's panics are caught now too.
   - A copy value passed twice to a generic, `pair(v0, v0)`, which constant
     propagation makes of two copies, was passed by one pointer, by the JIT
     and by the IR walker calling compiled code; the callee handed the map
     insert one place as both its key and its value, and the runtime
     refused it, aborting (`1028`). Each argument after the first naming an
     operand is passed a copy.
   - The parser, given a `fun` missing its `end fun` before another, parsed
     the second as a function inside the first -- an error -- and lost the
     script's expression counters, failing a debug assertion (parser
     fixture `324`). Function scopes nest now. Found by shrink candidates.
4. **Partly done.** What the generator never wrote. `WorldGenConfig::engines`
   is the suite's config, now in worldgen, and `coverage_tests` measures it
   rather than the default, expression units included.
   - Data files: the preset turns on binding access, which writes `require
     data` with its file, and takes, revives and divisors besides.
   - Hex literals: `hex_probability`, small or anywhere in the type's range.
   - The standard library: `std_lib` reads the non-generic functions of
     fifteen `sys/std` modules from their sources, and `std_modules` has a
     script call into one to three of them and a module into one, qualified.
     `int.pow`, `int.factorial` and `string.repeat` are left out: given an
     argument in the billions, the first ran past two minutes. Worldfile
     analysis compiles the library when `AnalysisOptions::system` is given,
     as far as the sections require it, and registers its natives; the
     oracle gives it to a world that requires `sys/`. The AOT backends link
     the native component built with its riders, as `std_engine_tests` does,
     so a world of one fragment calls it too.
   - `icall`: every intrinsic but the remainders, whose zero divisor is not
     defined, as functions named `icall name`.
   - `worldgen_tests` retired: the suite's compile check is stricter than
     its typecheck, and `coverage_tests` counts what its text searches
     looked for. Determinism is a unit test, for the engines' config too.

   Found on the way: a unit that failed at run time -- a stack overflow
   from recursion a shrink had cut the base case off -- emptied the
   executor's session, and worldfile analysis compiled the units after it
   against the old one; the executor's assertion that a unit reads only
   earlier units failed (`1030`). Analysis starts a new compiler, as the
   REPL does. And a world that hung made every shrink candidate wait out
   its timeout, so a shrink is bounded by five minutes now.

   5000 seeds with the library and intrinsics then found two more:
   - The C backend's shift intrinsics wrote a bare `<<` or `>>`, undefined
     in C past the width, where the IR walker and Cranelift shift by the
     amount modulo the width (`1032`).
   - Dead code elimination, which runs where const inlining does, so in a
     function with a local `const`, removed an unused read of a copy field
     and left a drop of the whole struct, freeing what was read after
     (`1031`). The reference itself was wrong; `noconst` was right.

   `rec` types: `rec_probability` makes a type a recursive one of five
   shapes, and datalit's value generator unfolds one, its collections empty
   past the depth a type of its own can reach. A `const` of one did not
   lower: compile-time evaluation read a value back by its type and knew no
   `rec` (`1033`). A generic's ordered parameter, a set's element or a map's
   key, is never one (T065).

   Not done: a test rider, for worldfile `rider` sections and generated
   `native fun`s. The standard library's string functions are rider natives
   already, so generated worlds call riders on every engine, AOT included;
   what a test rider would add is the worldfile's own rider section, at the
   cost of a rider crate and a way to point a worldfile's rider at it.

5. **Done.** The generative reactive tests took four of these settings,
   `fallible_script`, `compute_from_variables`, `qualified_calls` and
   `std_modules`, which over 200 seeds tripled the units returning early and
   blocked; the rest change values and code rather than what units reach of
   each other. Their sessions run their calls through the chaos JIT, kept
   across the changes, against a fresh run on the bytecode, by
   `Engine::use_dispatcher`. And they check seeds in worker processes, and
   bound a shrink, as this suite does.

## Open questions

- How many seeds `just test` can afford, every applicable one linking two
  executables, one of them through `cc`.
- Whether the AOT backends should take a program the reference ended in an
  error, compared by exit status and stderr, rather than being skipped for it.
