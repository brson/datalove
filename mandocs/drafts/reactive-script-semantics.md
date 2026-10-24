<!--

Title: Reactive compilation and evaluation based on analysis of bindings with linear types
Summary: With a simple linear type system we correctly decide which
         code units to recompile and rerun when code and input changes
         based on a simple fixed-point analysis of how bindings are used.

Audience: practitioners

Problem:
Datalove incrementally recompiles and reexecutes only dependent script units
when dependent code and data changes.
How do we decide which code to recompile and rerun and when?

-->

Datalove is an interactive scripting language and environment.
The compiler quickly re-compiles the program
whenever its code changes.
The program quickly re-runs whenever its code or data changes.
We say that it is _reactive_ to changes to both code and data.
Reruns are deterministic and,
in the absence of semantic changes,
identical from run to run.

Until now though the CLI REPL has only barely functioned,
and the precise semantics of what happens
as code is recompiled and rerun have been left to future work.
Ok, future is now.
Let's figure it out.

We develop a simple recursive algebra for deciding which code
must be recompiled and rerun when individual script units
are interactively modified or their inputs change.
This algebra is based entirely on an analysis of variable bindings
and their subsequent usage within a linear type system.
We do this by progressively developing verifiable counter-examples
until we are satisfied of its correctness.
We do not prove correctness.




- <!-- and --> What is a reactive script?
- <!-- and --> Datalove's design for reactivity
- <!-- but --> Changes to data are changes to code
- <!-- but --> How does one unit failing to compile affect subsequent units?
- <!-- but --> How is re-execution affected when subsequent units move or mutate bindings from the re-run unit?
- <!-- therefore --> A naive algebra of reruns for moves and mutations
- <!-- but --> todo more buts, succinctly
- <!-- therefore --> A recursive algebra of reruns for moves and mutations
- <!-- but --> Validating this design with a literate test suite
- <!-- but --> Comparing to FRP literature
- <!-- therefore --> Future refinements and open questions







---

Old draft content.

- [Datalove's design for reactivity](#user-content-dataloves-design-for-reactivity)
- [How does one unit failing to compile affect subsequent units?](#user-content-how-does-one-unit-failing-to-compile-affect-subsequent-units)
- [How is re-execution affected when subsequent units consume bindings from the re-executed unit?](#user-content-how-is-re-execution-affected-when-subsequent-units-consume-bindings-from-the-re-executed-unit)
- [How is re-execution affected when subsequent units mutate the re-executed unit?](#user-content-how-is-re-execution-affected-when-subsequent-units-mutate-the-re-executed-unit)
- [What if a unit overwrites a binding without reading it?](#user-content-what-if-a-unit-overwrites-a-binding-without-reading-it)
- [Hypothesis: an algebra of reruns for moves and mutations](#user-content-hypothesis-an-algebra-of-reruns-for-moves-and-mutations)
- [What if a unit reads a binding before a later unit moves it?](#user-content-what-if-a-unit-reads-a-binding-before-a-later-unit-moves-it)
- [What if a unit reads a binding before a later unit mutates it?](#user-content-what-if-a-unit-reads-a-binding-before-a-later-unit-mutates-it)
- [What if a unit reads a binding after an earlier unit mutated it?](#user-content-what-if-a-unit-reads-a-binding-after-an-earlier-unit-mutated-it)
- [What if an edit adds or removes a move?](#user-content-what-if-an-edit-adds-or-removes-a-move)
- [What if an edit adds or removes a mutation?](#user-content-what-if-an-edit-adds-or-removes-a-mutation)
- [What if a unit that moves or mutates is deleted?](#user-content-what-if-a-unit-that-moves-or-mutates-is-deleted)
- [What if a unit gives a moved `var` a value again?](#user-content-what-if-a-unit-gives-a-moved-var-a-value-again)
- [An amended algebra of reruns and fixups](#user-content-an-amended-algebra-of-reruns-and-fixups)
- [Speculating about transitivity](#user-content-speculating-about-transitivity)
- [What if a rerun needs another rerun first?](#user-content-what-if-a-rerun-needs-another-rerun-first)
- [What if values flow through intermediate bindings?](#user-content-what-if-values-flow-through-intermediate-bindings)
- [What if a move or mutation is conditional, or in a loop?](#user-content-what-if-a-move-or-mutation-is-conditional-or-in-a-loop)
- [What if a unit writes only part of a binding?](#user-content-what-if-a-unit-writes-only-part-of-a-binding)
- [What if a rerun produces the same value?](#user-content-what-if-a-rerun-produces-the-same-value)
- [What does rerunning cost?](#user-content-what-does-rerunning-cost)
- [How does one unit returning early affect subsequent units?](#user-content-how-does-one-unit-returning-early-affect-subsequent-units)
- [Are the rules right, and what does the engine do today?](#user-content-are-the-rules-right-and-what-does-the-engine-do-today)




## Datalove's design for reactivity

There are two pillars to Datalove's design that help make the goal
of interactive deterministic reexecution feasible:

_Language restrictions_.
Datalove has a linear type system and its functions are side-effect free.
At the moment there is not even any way to do I/O,
which would complicate determinism greatly.
The compiler can track precise data dependencies statically,
and know exactly which code can mutate which data.

_Fast incremental recompilation_.
Every pass of the compiler is memoized (and parallelized).
Editing any code only recompiles its dependents.
The compiler design is aided by and informs the restrictions of the language design.

A language that doesn't do any I/O is quite limited.
Datalove will have I/O.
One of my [principles](../principles.md) in Datalove is to start with a severely
restricted language design, build out the desired compiler capabilities,
then slowly loosen those language restrictions while designing
within the established restrictions of the compiler architecture.
Even while Datalove remains a pure-functional language I have
secret plans to add simple I/O facilities in a way that will preserve
the unique properties we are building here.

Code in Datalove comes in two forms, modules and script units.
A single script unit, executed once, is just a script.
Scripts are the entry points to Datalove programs.
Multiple script units, chained together,
are the basis of interactive Datalove.
Interactive Datalove is intended for the REPL and scientific notebook use cases.
Their semantics are the subject of the work at hand.

In most respects script units are like a function,
except that they accept no arguments and are not required to return a result.
At run time they have the same layout as a function's stack frame.
Script units can be chained together in a list dynamically at run time.
New script units inherit the type checking and execution environment of previous script units.

In this example of a self-contained Datalove _worldfile_ we have two sequential script units.
They share a type checking context and execution environment.

```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

let b = 100

----------
scriptunit-fragment
----------

let a_moved = a
let b_moved = b
let c = a_moved /! b_moved
```

> Worldfiles are chunks of text delimited by headers bound between `----------` lines.
> Each chunk is interpreted as determined by the header.

In this example the integers are type `int`, bigints.
They are linear types allocated on the heap,
so the assignment `let a_moved = a` moves `a` into `a_moved`, leaving `a` empty.
The `/!` operator is checked division, and returns early on divide by 0,
indicating failure.
Scripts and script units are allowed to return the type `!()`,
"result of unit".

In this example all three units compile and execute successfully.
The third binds `c` to 10.
Previous units both compile and execute prior to subsequent units,
as the results of execution may affect which bindings are subsequently available.




## How does one unit failing to compile affect subsequent units?

Imagine our first script unit doesn't compile.
No recompilation yet.
This is as originally written.

<!-- world compile-failure code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000
let x = y // `y` doesn't exist

----------
scriptunit-fragment
----------

let b = 100

----------
scriptunit-fragment
----------

let a_moved = a
let b_moved = b
let c = a_moved /! b_moved
```
<!-- end world -->

The first unit will fail in name resolution with an error.
The second will compile successfully because it references no bindings in the first.
_The second unit will then run_,
creating the binding `b` and making it available to subsequent units.
The third will fail to compile because the binding `a` does not exist,
as the unit that creates it failed to compile.

Each step below is a table with a row per unit:
its code, joined into one line,
what it does to bindings of earlier units, and which unit made them,
what happened to it in this step,
why,
and what its own bindings hold once the step is over,
with `-` for nothing.
Each step is caused by the unit whose "because" says "edited".

<!-- world compile-failure step 1 -->
**Step 1: all three units are entered, unit 1 broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | compile fails | new | - |
| 2 | `let b = 100` | - | compiles, runs | new | `b` = 100 |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | compile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

Imagine we fix the first unit's compilation error.
That allows the first and third units to both compile and run.
But we need to completely skip the second unit because it already did both
and has no dependencies on the first.

<!-- world compile-failure step 2 -->
**Step 2: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` moved |
| 2 | `let b = 100` | - | untouched | uses nothing of unit 1's | `b` moved |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompiles, runs | takes `a` from unit 1 | `a_moved` = 1000, `b_moved` = 100, `c` = 10 |
<!-- end world -->

After this second pass, because unit 3 ran,
it moved `a` and `b` out of the frames of units 1 and 2, into its own.

Now let's revert the previous edit to unit 1, making it fail to compile again.
Unit 3 depends on unit 1, so it recompiles and fails, and doesn't reexecute.
Unit 2 doesn't depend on unit 1,
so naively it neither recompiles nor reexecutes.

**Step 3, naively: unit 1 is broken again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `let b = 100` | - | untouched | uses nothing of unit 1's | `b` moved, though nothing took it |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |

But that's wrong.




## How is re-execution affected when subsequent units consume bindings from the re-executed unit?

When unit 3 ran it moved `b` out of unit 2's frame and into its own.
Now unit 3 has failed to compile,
so what it did is gone:
its frame is destroyed, `b_moved` with it,
and it offers no bindings to anybody.

Static ownership analysis now says there is a value in `b`.
Nothing gave it away, because the only unit that did no longer compiles.
But at run time `b` is still empty.
The environment says `b` is moved,
and a new unit typed after this one that reads `b`
compiles fine and then reads a value that isn't there.

```datalove
----------
scriptunit-fragment
----------

let a = 1000
let x = y // broken again

----------
scriptunit-fragment
----------

// Naively we don't want to rerun this because it doesn't depend on
// unit 1...
let b = 100

----------
scriptunit-fragment
----------

let a_moved = a
// ...but this moved `b` the last time it ran,
// and now it doesn't compile, so it can't have.
let b_moved = b
let c = a_moved /! b_moved
```

We have to recognize that unit 3 moved a binding out of unit 2,
and that anything that retracts or applies unit 3 has to build the binding again,
by rerunning unit 2.

<!-- world compile-failure step 3 -->
**Step 3: unit 1 is broken again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `let b = 100` | - | reruns | rerun: unit 3 took `b` when it last ran, and is retracted | `b` = 100 |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

The rule is in terms of what unit 3 did _the last time it ran_,
not what it does now.
That is what the running session went through,
and the session has to end up the same as if it had been run fresh from the top.

Then when we make unit 1 compile again,
unit 2 has nothing to do:
`b` is already as unit 2 made it,
and unit 3 moves it again.

<!-- world compile-failure step 4 -->
**Step 4: unit 1 is fixed again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` moved |
| 2 | `let b = 100` | - | untouched | `b` is as unit 2 made it | `b` moved |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompiles, runs | takes `a` from unit 1 | `a_moved` = 1000, `b_moved` = 100, `c` = 10 |
<!-- end world -->




## How is re-execution affected when subsequent units mutate the re-executed unit?

Here's a new variation on our running example
that mutates instead of moves.

<!-- world mutate-earlier code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

set b = b * 2
let c = a /! b
```
<!-- end world -->

The third unit doubles `b` in place.
Doubling is not idempotent:
running it twice is not the same as running it once.
On first run it mutates `b` from 125 to 250, and `c` is 4.

<!-- world mutate-earlier step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | compiles, runs | new | `c` = 4 |
<!-- end world -->

At this point unit 2's frame contains the mutated `b`, 250.
Imagine in the second run we cause the first unit to fail as previously.

**Step 2, naively: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | untouched | uses nothing of unit 1's | `b` = 250, though nothing doubled it |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |

Unit 3 failed to compile because it depends on unit 1.
Naively unit 2 is untouched because it doesn't.
But like with the move, unit 3 wrote to `b` the last time it ran,
and now it can't have,
so we need to rerun unit 2 to put `b` back to 125.

<!-- world mutate-earlier step 2 -->
**Step 2: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 updated `b` when it last ran, and is retracted | `b` = 125 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

Now if we fix unit 1,
unit 3 runs against `b` as unit 2 made it,
doubling 125 to 250, and `c` is 4 again.
Had we left unit 2 alone,
unit 3 would have doubled the 250 it left behind last time,
to 500,
and `c` would have been 2.

<!-- world mutate-earlier step 3 -->
**Step 3: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 2 made it | `b` = 250 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompiles, runs | reads `a` from unit 1 | `c` = 4 |
<!-- end world -->

The analysis is essentially the same,
but instead of a dependency on move operations
we have a dependency on writes:
`set`, and passing a binding to a `mut` or `out` parameter.




## What if a unit overwrites a binding without reading it?

Doubling `b` reads it before writing it,
so running the write again starting from its own result gives the wrong answer.
But not every write reads.
Here unit 3 just replaces `b`:

<!-- world overwrite code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

set b = 500
let c = a /! b
```
<!-- end world -->

On first run unit 3 replaces 125 with 500, and `c` is 2.

<!-- world overwrite step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 3 | `set b = 500; let c = a /! b` | reads `a` (1), overwrites `b` (2) | compiles, runs | new | `c` = 2 |
<!-- end world -->

Now edit unit 3 to write a different value.
Unit 3 has to rerun, and it writes `b` again.
But the write doesn't care what `b` held,
so it doesn't matter that `b` is still the 500 that unit 3 left last time.
Unit 2 can be left alone,
which is where an overwrite differs from an update.

<!-- world overwrite step 2 -->
**Step 2: unit 3 is edited to write 250.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing it uses changed | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 250 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompiles, runs | edited | `c` = 4 |
<!-- end world -->

Retracting the overwrite is a different story.
Break unit 1 and unit 3 stops compiling,
so the session should look like unit 3 never ran,
and `b` should be 125 again.
Nothing but unit 2 knows how to make that 125,
so it reruns,
exactly as it did for the move and for the update.

<!-- world overwrite step 3 -->
**Step 3: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 overwrote `b` when it last ran, and is retracted | `b` = 125 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

And fixing unit 1 reruns unit 3,
which overwrites `b` again without anything to retract first.

<!-- world overwrite step 4 -->
**Step 4: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 250 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompiles, runs | reads `a` from unit 1 | `c` = 4 |
<!-- end world -->

So writes come in two kinds.
An _update_ reads the binding first,
and like a move it needs the binding rebuilt both to retract it and to apply it.
An _overwrite_ doesn't read it,
and needs the binding rebuilt only to retract it.





## Hypothesis: an algebra of reruns for moves and mutations

After those examples a pattern shows.
What matters about a unit is what it does to bindings
that belong to _earlier_ units.
There are four things it can do to one:

- _Read_ it, by borrowing: `a + 1`, `f(ref a)`.
- _Take_ it, by moving it: `let a_moved = a`, `f(a)`.
- _Overwrite_ it, without reading it first: `set b = 10`, `f(out b)`.
- _Update_ it, reading it and then writing it: `set b = b * 2`, `f(mut b)`.

A unit can do different things to different bindings,
like reading `a` and updating `b`.
The rules apply to each binding separately,
and what the unit needs is all of them together.

Each has consequences in two directions.

_Backward_.
To _retract_ a unit, because it was deleted or stopped compiling,
or to _apply_ it, because it was edited or something it uses was,
the earlier bindings it touched have to be as they were before it first ran.
When they aren't we _rerun_:
rerun the unit that made the binding,
which builds all of that unit's bindings again,
then rerun the units after it that took or wrote any of them, in order.
Units that only read them don't need to rerun for the rerun's sake.
The rerun unit wasn't edited,
so it builds the same values,
and a reader would compute what it already holds.
If what a reader sees does change,
because the unit being retracted or applied changed it,
that's the forward rule's business, below.

_Forward_.
What a unit did to a binding is what the units after it see,
so changing what it does reaches them,
whether or not they use anything the unit itself defines.

| a unit that ... an earlier binding | retract it | apply it | units after it |
|------------|---------|----------------------------------------|--------------------------------------------|
| reads      | nothing | rerun if a unit after it took or wrote it  | nothing                                 |
| takes      | rerun   | rerun                                   | can't name it, so recompile when that changes |
| overwrites | rerun   | nothing                                 | read the new value, so rerun when it changes |
| updates    | rerun   | rerun                                   | read the new value, so rerun when it changes |

Moves and mutations agree on retract,
and disagree on apply and on what they pass forward.

<!--
Note: the engine follows this table for edits, overwrites included.
It reruns to retract a revival where a fixup would do, and the splices
(delete, insert, truncate) rerun every unit from the rerun start on.
-->




## What if a unit reads a binding before a later unit moves it?

So far the unit being applied was the one doing the moving.
But a unit that only reads can be caught out too,
if the binding it reads was taken _after_ it ran.

<!-- world read-before-move code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

let c = a /! 10

----------
scriptunit-fragment
----------

let a_moved = a
```
<!-- end world -->

Unit 2 borrows `a`, and unit 3 takes it.

<!-- world read-before-move step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let a_moved = a` | takes `a` (1) | compiles, runs | new | `a_moved` = 1000 |
<!-- end world -->

Now edit unit 2.
It reads `a`, which is empty:
unit 3 took it after unit 2 last ran.
Nothing unit 3 does depends on unit 2,
and unit 2 is only reading,
but unit 2 still can't run until unit 1 builds `a` again.
And once unit 1 has rerun,
unit 3 has to take the new `a`,
or `a` will be left holding a value nothing took.

<!-- world read-before-move step 2 -->
**Step 2: unit 2 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 2 reads `a`, which unit 3 took after it | `a` moved |
| 2 | `let c = a /! 100` | reads `a` (1) | recompiles, runs | edited | `c` = 10 |
| 3 | `let a_moved = a` | takes `a` (1) | reruns | rerun: takes `a` from unit 1, which reran | `a_moved` = 1000 |
<!-- end world -->

So a read can need a rerun as much as a move can.
What matters is not what the edited unit does to `a`,
but whether anything changed `a` between when it was made and now.
The behavior of units that are subsequent to the edit
affects which units need to be redone.




## What if a unit reads a binding before a later unit mutates it?

The same goes for a mutation after the read.

<!-- world read-before-mutate code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

let c = a /! b

----------
scriptunit-fragment
----------

set b = b * 2
```
<!-- end world -->

Unit 3 reads `b` as 125, so `c` is 8.
Then unit 4 doubles `b` to 250.

<!-- world read-before-mutate step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
| 4 | `set b = b * 2` | updates `b` (2) | compiles, runs | new | - |
<!-- end world -->

Edit unit 3 to double its result.
Rerun on its own it would read the 250 that unit 4 left
and get 8 again, rather than 16.
So unit 2 reruns to put `b` back to 125,
unit 3 runs against that,
and unit 4 doubles `b` again.

<!-- world read-before-mutate step 2 -->
**Step 2: unit 3 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 reads `b`, which unit 4 updated after it | `b` = 250 |
| 3 | `let c = (a /! b) * 2` | reads `a` (1), reads `b` (2) | recompiles, runs | edited | `c` = 16 |
| 4 | `set b = b * 2` | updates `b` (2) | reruns | rerun: updates `b` from unit 2, which reran | - |
<!-- end world -->

Here it doesn't matter what kind of write unit 4 makes.
Had it been an overwrite, `set b = 500`,
unit 3 would still have read the wrong `b`,
so the rerun is the same.
The kind of write decides whether applying _the writer_ needs a rerun,
not whether applying a unit before it does.

Unit 1 is a useful control.
Unit 3 reads `a` too, but nothing changed `a`,
so unit 1 is left alone.




## What if a unit reads a binding after an earlier unit mutated it?

Swap units 3 and 4, so the write comes first.

<!-- world read-after-mutate code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

set b = b * 2

----------
scriptunit-fragment
----------

let c = a /! b
```
<!-- end world -->

<!-- world read-after-mutate step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | compiles, runs | new | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 4 |
<!-- end world -->

Now unit 4 reads `b` after the write,
and the `b` it reads is the one unit 3 left.
That's still what's there,
so editing unit 4 needs nothing else rerun.

<!-- world read-after-mutate step 2 -->
**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 4 first saw it | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | untouched | unit 4 reads after it | - |
| 4 | `let c = (a /! b) * 2` | reads `a` (1), reads `b` (2) | recompiles, runs | edited | `c` = 8 |
<!-- end world -->

A write passes its value forward,
and a unit after it depends on it the way it would on a `let`.
A move has no such case.
If unit 3 had been `let b_moved = b`,
unit 4 wouldn't compile at all:

```
error[D013]: `b` was given away by an earlier unit
```

So the rule for a reader is narrower than "anything changed the binding".
A reader needs a rerun only if the binding was changed
_after_ the reader last ran.
Changes before it are just what it read.





## What if an edit adds or removes a move?

Every edit so far kept what the edited unit does to earlier bindings.
A unit that took `a` before the edit still took it after.
But an edit can change that too,
and turn a read into a take, or a take into a read.

Then the edit is two things at once.
It retracts what the old code did the last time it ran,
and it applies what the new code does now.
The bindings the old code touched need what the retract rules ask for,
and the bindings the new code touches need what the apply rules ask for.
Forward, the units after it see the difference between the two.

<!-- world add-move code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

let c = a /! 10

----------
scriptunit-fragment
----------

let d = a + 1

----------
scriptunit-fragment
----------

let e = a + 2
```
<!-- end world -->

Three units read `a`, and nothing takes it.

<!-- world add-move step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | compiles, runs | new | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | compiles, runs | new | `e` = 1002 |
<!-- end world -->

Now edit unit 3 to take `a` instead of reading it.
The old code only read `a`, so there is nothing to retract.
The new code takes `a`,
and the apply rules say a take needs a rerun.
But it doesn't need one here.
That column assumed the unit took the binding the last time it ran too,
so the binding would be empty.
This take is new,
and `a` still holds what unit 1 made,
so unit 1 is left alone.

The cost lands forward instead.
Unit 4 reads `a`,
and `a` was given away before unit 4 gets to it.
It hasn't been edited,
and nothing it uses was rebuilt,
but it no longer compiles:

```
error[D013]: `a` was given away by an earlier unit
```

<!-- world add-move step 2 -->
**Step 2: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | `a` is as unit 1 made it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | reads `a` before the take | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a + 2` | reads `a` (1) | recompile fails | `a` was given away by unit 3 | - |
<!-- end world -->

Unit 2 is untouched too.
It reads `a` before the take,
so what it read is still what's there.

Now edit unit 3 back to a read.
This time the old code took `a`,
and retracting a take needs a rerun:
unit 1 reruns to build `a` again.
The new code reads `a`, which is now there to read.
And unit 4 can name `a` again,
so it recompiles and runs.

<!-- world add-move step 3 -->
**Step 3: unit 3 is edited back to read `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 took `a` when it last ran, and the edit retracts that | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 provided the same `a` | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | recompiles, runs | edited | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | reruns | `a` can be named again, as it could in step 1, whose compile is still memoized | `e` = 1002 |
<!-- end world -->

Adding the move and removing it are not mirror images.
Adding it cost nothing backward and broke a unit forward.
Removing it needed a rerun backward, and fixed that unit forward.

The added move does need a rerun
if something after it already took the binding.
Make unit 4 `let e = a`, so it is the one that takes `a`,
and then edit unit 3 to take it instead.

<!-- world add-move-later-take step 2 -->
**Step 2, with unit 4 taking `a`: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 takes `a`, which unit 4 took after it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 provided the same `a` | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a` | takes `a` (1) | recompile fails | `a` was given away by unit 3 | - |
<!-- end world -->

The take has moved from unit 4 to unit 3.
The rerun is not because of anything unit 3 did.
Unit 4 emptied `a` after unit 3 last ran,
the same case as a read before a later move.
The apply rule holds if it is read that way:
a take needs a rerun when the binding is not as its provider left it,
and that is usually, but not always, because the unit took it itself.





## What if an edit adds or removes a mutation?

A move added or removed changes which names the units after it can use.
A mutation doesn't change any names.
It changes values.

<!-- world add-mutation code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

let d = b * 2

----------
scriptunit-fragment
----------

let c = a /! b
```
<!-- end world -->

Unit 3 reads `b` and doubles it into a binding of its own.
Unit 4 reads `b` as 125, so `c` is 8.

<!-- world add-mutation step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 125 |
| 3 | `let d = b * 2` | reads `b` (2) | compiles, runs | new | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
<!-- end world -->

Now edit unit 3 to double `b` in place.
As with the added move, nothing needs to be retracted,
and `b` still holds what unit 2 made,
so the new update needs no rerun.
Unit 3 doubles `b` to 250.

Unit 4 is where it differs from the move.
It still compiles:
its code names `a` and `b`,
both are still there,
and their types are the same.
Its compiled code doesn't change at all.
But the `b` it reads is now the one unit 3 left,
so it has to rerun,
and `c` becomes 4.

<!-- world add-mutation step 2 -->
**Step 2: unit 3 is edited to update `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 2 made it | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | recompiles, runs | edited | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which now updates it | `c` = 4 |
<!-- end world -->

Now edit unit 3 back to a read.
The old code updated `b`,
and retracting an update needs a rerun,
so unit 2 reruns to put `b` back to 125.
The new code reads that 125,
and `d` is 250 again.

Unit 4 reruns too, but not for the rerun's sake.
A rerun reruns only the units that took or wrote what was rebuilt,
and unit 4 only reads.
It reruns because the edit changed the `b` it sees,
from the 250 unit 3 left
back to the 125 unit 2 made.
That's the forward rule.

<!-- world add-mutation step 3 -->
**Step 3: unit 3 is edited back to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 updated `b` when it last ran, and the edit retracts that | `b` = 125 |
| 3 | `let d = b * 2` | reads `b` (2) | recompiles, runs | edited | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which no longer updates it | `c` = 8 |
<!-- end world -->

An overwrite in place of the update, `set b = 500`,
reruns the same units in both steps.
The kinds of write differ on apply,
and a write that is new is never applied against its own result.
They come apart when something after already wrote the binding.
Add a fifth unit, `set b = b + 1`,
so `b` is 126 before the edit.
An update added to unit 3 has to read 125,
so unit 2 reruns first.
An overwrite added to unit 3 doesn't read `b`,
so it can write over the 126,
and unit 5 reruns forward from there.

So the move and the mutation agree backward,
and disagree forward in where the break shows.
The later units notice an added or removed move
when they are compiled,
as an error.
They notice an added or removed mutation
only when they run,
as a different value.
To find those units the engine has to know which units come after
a write to each binding.
Their code alone doesn't tell it.

<!--
Note: the fifth-unit case is checked against the engine too, in
`crates/datalove-script-engine/tests/fixtures/reactive/add-mutation-later-write.world`.
-->




## What if a unit that moves or mutates is deleted?

An edit that removes a move or a mutation retracts what the old code did
and applies what the new code does.
Deleting the unit is the same edit with nothing to apply.

<!-- world delete code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

let c = a /! b

----------
scriptunit-fragment
----------

let a_moved = a
set b = b * 2

----------
scriptunit-fragment
----------

let d = b + 1
```
<!-- end world -->

Unit 3 reads `a` and `b` before anything changes them.
Unit 4 takes `a` and doubles `b`.
Unit 5 reads `b` as unit 4 left it.

<!-- world delete step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
| 4 | `let a_moved = a; set b = b * 2` | takes `a` (1), updates `b` (2) | compiles, runs | new | `a_moved` = 1000 |
| 5 | `let d = b + 1` | reads `b` (2) | compiles, runs | new | `d` = 251 |
<!-- end world -->

Now delete unit 4.
The session should look as if unit 4 had never been entered.
That is the retract column of the table,
once for each binding unit 4 touched.
Retracting a take needs a rerun, so unit 1 reruns.
Retracting an update needs a rerun, so unit 2 reruns.

The rows below keep the numbers the units had before the deletion,
so unit 5 is still called unit 5, though it is now the fourth.

<!-- world delete step 2 -->
**Step 2: unit 4 is deleted.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 4 took `a` when it last ran, and is deleted | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 4 updated `b` when it last ran, and is deleted | `b` = 125 |
| 3 | `let c = a /! b` | reads `a` (1), reads `b` (2) | untouched | units 1 and 2 provided the same `a` and `b` | `c` = 8 |
| 4 | `let a_moved = a; set b = b * 2` | takes `a` (1), updates `b` (2) | deleted | deleted | - |
| 5 | `let d = b + 1` | reads `b` (2) | reruns | reads `b` after unit 4, which no longer updates it | `d` = 126 |

<!--
Engine today, more than the rule asks:
unit 3 reruns: a splice reruns every unit from the rerun point on, readers included
unit 5 recompiles, runs: a removal renumbers every later unit, so they are typechecked under new keys
-->
<!-- end world -->

Unit 3 read `a` and `b` before unit 4 changed them,
and the rerun builds them again as they were,
so unit 3 is left alone.
Unit 5 reruns by the forward rule,
as it did when an edit removed the mutation:
the `b` it sees has gone from 250 back to 125.

Had unit 4 overwritten `b` rather than updated it,
the step would be the same.
The two kinds of write differ only on apply,
and a deleted unit is never applied.

The other forward consequences are the ones the earlier sections found.
A later unit that used `a_moved` would fail to compile,
as it would if unit 4 had stopped compiling.
A later unit that named `a`,
and failed because unit 4 had given it away,
would compile and run.

So deleting a unit asks nothing new of the rules.
It is the same as editing it to nothing,
and the same as breaking it so it no longer compiles.
What is new is only that the units after it move up a place.
That changes their numbers, not what they compute,
so in these terms it costs nothing.





## What if a unit gives a moved `var` a value again?

A `var` that was given away holds nothing,
but it can still be assigned to.
`set` on it doesn't read it, so it is an overwrite.
It also gives the name back:
the units after it can use the binding again.
Call that _reviving_ the binding.

<!-- world revive code -->
```datalove
----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

let b_moved = b

----------
scriptunit-fragment
----------

set b = 500

----------
scriptunit-fragment
----------

let c = b + 1
```
<!-- end world -->

Unit 2 takes `b`, unit 3 revives it,
and unit 4 reads what unit 3 put there.
Without unit 3, unit 4 wouldn't compile.

<!-- world revive step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 2 | `let b_moved = b` | takes `b` (1) | compiles, runs | new | `b_moved` = 125 |
| 3 | `set b = 500` | revives `b` (1) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |
<!-- end world -->

Edit unit 3 to write a different value.
Applying an overwrite needs no rerun,
and that holds for a revival too:
it doesn't matter what `b` holds before the write.
Unit 4 reruns by the forward rule,
as it would after any overwrite.

<!-- world revive step 2 -->
**Step 2: unit 3 is edited to write 600.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 600 |
| 2 | `let b_moved = b` | takes `b` (1) | untouched | nothing it uses changed | `b_moved` = 125 |
| 3 | `set b = 600` | revives `b` (1) | recompiles, runs | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | reads `b` after unit 3, which now writes 600 | `c` = 601 |
<!-- end world -->

_Retracting is where a revival differs from every other write_.
Break unit 3,
and the session should look as if unit 3 never ran.
The retract column says an overwrite needs a rerun,
so that `b` is as it was before the write.
But before this write `b` held nothing.
Rerunning unit 1 would build 125,
and rerunning unit 2 would take it again,
all to leave `b` empty.
Dropping the 600 leaves it empty too,
and runs nothing.

Forward, it is a move added back.
Unit 4 names `b`, which unit 2 gave away,
and nothing gives it back now:

```
error[D013]: `b` was given away by an earlier unit
```

<!-- world revive step 3 -->
**Step 3: unit 3 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | _`b` dropped_ | fixup: unit 3 revived `b` when it last ran, and is retracted | `b` moved |
| 2 | `let b_moved = b` | takes `b` (1) | untouched | nothing it uses changed | `b_moved` = 125 |
| 3 | `set b = 600; let x = y` | revives `b` (1) | recompile fails | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | recompile fails | `b` was given away by unit 2 | - |

<!--
Engine today, more than the rule asks:
unit 1 reruns: treats the revival like any other write, and reruns rather than dropping `b`
unit 2 reruns: takes `b` again after unit 1 reruns
-->
<!-- end world -->

_`b` dropped_ is a new kind of action.
It is neither a recompile nor a rerun.
It's just a cleanup to the frame of that unit
to make a particular binding inaccessable again.

Fixing unit 3 again is an apply,
and needs no rerun either.
Unit 3 writes 600 into the empty `b`,
and unit 4 compiles and runs.
The session is back to how step 2 left it.

<!-- world revive step 4 -->
**Step 4: unit 3 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | untouched | `b` holds nothing, and unit 3 overwrites it | `b` = 600 |
| 2 | `let b_moved = b` | takes `b` (1) | untouched | nothing it uses changed | `b_moved` = 125 |
| 3 | `set b = 600` | revives `b` (1) | recompiles, runs | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | `b` can be named again, as it could in step 2, whose compile is still memoized | `c` = 601 |
<!-- end world -->

Whether a write revives is not up to the writer.
It depends on whether something before it gave the binding away.
Edit unit 2 to read `b` instead of taking it.
Retracting the take needs a rerun,
so unit 1 reruns and builds 125.
Unit 3 wrote `b` the last time it ran,
so it reruns,
and now it overwrites a `b` that holds something.
Its code is the same:
writing a binding drops whatever it held,
and here that is 125 rather than nothing.
Unit 4 could name `b` before the edit and after it,
and reads 600 both times,
so it is left alone.

<!-- world revive step 5 -->
**Step 5: unit 2 is edited to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | reruns | rerun: unit 2 took `b` when it last ran, and the edit retracts that | `b` = 600 |
| 2 | `let d = b + 1` | reads `b` (1) | recompiles, runs | edited | `d` = 126 |
| 3 | `set b = 600` | overwrites `b` (1) | reruns | rerun: overwrites `b` from unit 1, which reran | - |
| 4 | `let c = b + 1` | reads `b` (1) | untouched | `b` is 600 after unit 3, as before | `c` = 601 |

<!--
Engine today, more than the rule asks:
unit 3 recompiles, runs: `b` is no longer given away before it, so its ownership analysis runs again, though its lowered code is the same
-->
<!-- end world -->

So a revival is an overwrite backward and a retracted move forward,
with one difference from both.
Its retract needs no rerun,
because what it overwrote was nothing,
and nothing can be put back without running anything.
That gives the table a fifth row:

| a unit that ... an earlier binding | retract it | apply it | units after it |
|------------|---------|----------------------------------------|--------------------------------------------|
| revives    | empty it | nothing                               | can name it again, so recompile when that changes |

<!--
Note: the engine's own notes are rendered with each step above.
The write's code does not depend on whether the binding was given away:
lowering a store to an earlier unit's slot always emits `DropTracked`
(`store_external_slot` in `crates/datalove-datafun-lower/src/stmt.rs`).
-->




## An amended algebra of reruns and fixups

> Note: istm that the terms rewind and replay are essentially interchangeable here.
> Mostly we _rerun_ units to restore state.
> That strikes me as _replay_.
> The binding fixup case on revival might be considered _rewind_.
> I am going to use the terms _rerun_ and _fixup_ to avoid any semantic confusion.

The rules describe how _modifications_, editing, adding, and removing script units,
change the value of bindings in other script units and their own,
through _reruns_ and _fixups_.
They do not describe how such changes trigger recompiles
of other units.
That is treated as a function of the data dependencies in the compiler's
memoization architecture,
and needs to be correct, but may be conservative.

Every modification triggers a _step_.
The outcome of every modification depends on compiler analysis of both the current step
and the previous step.

Different modifications trigger _retract_ and/or _apply_ _actions_, and _forward_ actions.

- _Retract_ actions reverse what the triggering unit's previous-step code
  did to bindings of earlier units.
  They rerun earlier units, or fix a binding up,
  to restore those bindings to the values they held before the triggering unit.
- _Apply_ actions run the triggering unit's current-step code.
  The bindings it reads must hold what they held before it,
  so when a later unit has changed one,
  earlier units rerun first.
- _Forward_ actions rerun units subsequent to the triggering unit.
  They update bindings to their current-step values.

Both retract and apply can rerun units prior to the triggering unit.
What separates them is which version of the triggering unit's code
decides what has to be restored:
the previous step's for retract, the current step's for apply.

Each modification is made of these actions:

- _Edit_ => _retract_ + _apply_.
  An edit that successfully compiles.
- _Break_ => _retract_.
  An edit that fails to compile.
- _Delete_ => _retract_.
  Removing a unit completely.
- _Fix_ => _apply_.
  An edit that fixes a formerly broken compile.
- _Add_ => _apply_.
  An insertion or append of a unit.

A unit can perform five _operations_ on a binding of an earlier unit:
_read_ it, _take_ it, _overwrite_ it, _update_ it,
or _revive_ it, overwriting a binding that was given away.
The retract, apply, and forward consequences of a script unit
are determined by the operation
(actually the union of these operations over the bindings
the unit operates on).

_Retract and apply use the operation in different steps_.
Retract looks up what the triggering unit did in the previous step,
and apply what it does in the current step.
An edit that turns a take into a read
retracts a take and applies a read.

| operation | retract | apply | forward |
|-----------|------|------|---------|
| read      | nothing | rerun if a later unit took or wrote it | nothing |
| take      | rerun   | rerun if it or a later unit took or wrote it | later units that name it break; removing the take fixes them |
| overwrite | rerun   | nothing | later units that read it rerun |
| update    | rerun   | rerun if it or a later unit took or wrote it | later units that read it rerun |
| revive    | fixup: empty it | nothing | later units that name it are fixed, and rerun; removing the revival breaks them |

A _rerun_ restores a binding to what it held before the triggering unit.
It reruns the unit that made the binding,
then the units between that one and the triggering unit
that took or wrote it, in order.
Units that only read it are not rerun:
the rerun units have the same code and inputs as before,
so a reader would compute what it already holds.

A rerun doesn't have to start as far back as the unit that made the binding.
What it needs is the nearest unit before the triggering unit
that set the binding without reading it.
That unit's value doesn't depend on anything the binding held before,
so rerunning it rebuilds the binding from there,
and only the takes and writes between it and the triggering unit rerun after it.
The unit that made the binding is one such unit,
and an overwrite or a revival after it is a nearer one.
An update is not, because it reads what it writes.

A take is a nearer starting point too.
It leaves the binding holding nothing,
and nothing can update a binding that holds nothing,
so if the nearest such unit is a take,
the binding held nothing before the triggering unit.
Rerunning from a take means emptying the binding,
which runs nothing.

A _fixup_ is that case: it restores a binding without running anything.
It happens when the nearest unit that set the binding was a take,
which is always so when retracting a revival,
because what a revival overwrote was nothing,
and emptying the binding puts that back.

Starting nearer doesn't make the rerun simpler in kind.
The unit it starts from reruns whole,
so whatever else it reads has to be as it found it,
and whatever else it takes or writes is redone.
That is as true of the unit that made the binding,
but a nearer start is more likely to be a unit that reads other bindings.
That is where one rerun can need another first,
a question for a later section.

_Retract_ needs a rerun, or for a revival a fixup,
whenever the triggering unit changed the binding,
since it then has to be put back as it was before it.
Retracting a read changes nothing,
even if a later unit took or wrote the binding:
that later change would have happened anyway.

_Apply_ needs a rerun only for the operations that read the binding,
which are read, take and update.
The triggering unit has to find the binding as it was before it,
and it won't if the triggering unit itself, in the previous step,
or a later unit, took or wrote it.
A change made by a unit before the triggering unit is just what it found,
and stays.
An overwrite or a revival doesn't read the binding,
so its apply needs nothing.

_Forward_ actions follow from what the step changed.
Units after the triggering unit rerun if they read
a binding whose value the step changed,
or if they took or wrote a binding that a rerun rebuilt,
since what they did to it has been undone by the rebuild.
That reaches units that use nothing the triggering unit defines.
A take or a revival added or removed
changes which names the later units can use,
so it breaks or fixes them.
Those breaks and fixes are modifications in their own right,
with their own retract and apply,
so the consequences cascade.
The first example in this document is one:
breaking unit 1 broke unit 3,
and retracting unit 3's take reran unit 2.
A forward rerun of a unit that takes or writes
is a retract and an apply of the same code, and cascades the same way.

<!--
Note: the engine today differs from this section in two places.
It reruns to retract a revival where a fixup would do.
And the splices, delete among them, start a rerun at the unit that made
the binding rather than a nearer overwrite (`rerun_start` in
`crates/datalove-datafun/src/pipeline/script_compiler.rs`), and rerun
every unit from there on, readers included, because a value is identified
by its unit's index.
-->



## Speculating about transitivity

These rules are getting complex
but I think still within the realm of what I can personally understand
and reason about.
Getting close to my personal reasoning frontier though.

There's still an obvious omission.
There are missing transitive properties in both the backward and forward directions.

The root of it is that the table is per binding,
but a rerun reruns a whole unit.
Everything else that unit reads has to be as it found it,
and everything else it takes or writes gets done again.

In the backward direction,
there can be multiple bindings involved in the chain
responsible for writing a particular target binding,
and some of those other bindings might themselves
be mutated or taken by intervening units that have no direct
relation to the triggering unit.

In the forward direction
the units that rerun may themselves depend on other unrelated mutated bindings,
forcing yet more retract operations in the backward direction before they can rerun.
And as data flows through intermediate bindings subsequent readers and writers
need themselves to rerun.

It seems like there's a transitive closure of bindings and the operations
needed to recover them.
A recursive algorithm running backwards and forwards across units to a fixed point.

<!--
The engine already loops like this:
`reach_from` repeats its backward rule over every unit it reaches
until no more providers are added.
-->

Can the closure be found from binding names alone,
or does it need real dataflow?
I suspect names are enough, thanks to linearity.
A value has one owner at a time,
and every handoff is a take or a write some unit recorded by name,
so following the records follows the value.
But names are coarse.
They assume a conditional write always happens,
and a write to `p.1` counts as a write to all of `p`.
That's sound, just more reruns than needed.
It's also what keeps the nearest-overwrite refinement safe:
a write that might not happen counts as an update,
so only an overwrite that certainly ran can start a rerun.

The sections below poke at all this,
each with a test the engine passes.




## What if a rerun needs another rerun first?

The smallest case I can think of:

<!-- world rerun-needs-rerun code -->
```datalove
----------
scriptunit-fragment
----------

var b = 1

----------
scriptunit-fragment
----------

var z = 10

----------
scriptunit-fragment
----------

set b = 500
set z = z + 1

----------
scriptunit-fragment
----------

let c = b + 1

----------
scriptunit-fragment
----------

set b = 0
```
<!-- end world -->

Edit unit 4.
It needs `b` as 500, so unit 3 reruns.
But unit 3 also updates `z`,
so rerunning it as is would leave `z` at 12.
Unit 2 has to rerun first, though unit 4 never names `z`.

<!-- world rerun-needs-rerun step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | compiles, runs | new | `b` = 0 |
| 2 | `var z = 10` | - | compiles, runs | new | `z` = 11 |
| 3 | `set b = 500; set z = z + 1` | overwrites `b` (1), updates `z` (2) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |
| 5 | `set b = 0` | overwrites `b` (1) | compiles, runs | new | - |
<!-- end world -->

<!-- world rerun-needs-rerun step 2 -->
**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | untouched | unit 3 overwrites `b` after it | `b` = 0 |
| 2 | `var z = 10` | - | reruns | rerun: unit 3 reruns and updates `z`, so `z` has to be as unit 3 first found it | `z` = 11 |
| 3 | `set b = 500; set z = z + 1` | overwrites `b` (1), updates `z` (2) | reruns | rerun: unit 4 reads `b`, which unit 5 overwrote after it, and unit 3 is the nearest to set it | - |
| 4 | `let c = b + 2` | reads `b` (1) | recompiles, runs | edited | `c` = 502 |
| 5 | `set b = 0` | overwrites `b` (1) | reruns | rerun: overwrites `b` after unit 4 | - |
<!-- end world -->

The engine does rerun unit 2.
And the rerun of `b` starts at unit 3, not unit 1:
unit 3 overwrites `b`,
so it's the nearest unit that sets `b` without reading it,
and unit 1 is left alone.

A chain of moves walks back further.
Here `w` is made from `a2`, which was made from `a`.

<!-- world rerun-through-moves code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

let a2 = a

----------
scriptunit-fragment
----------

var w = a2

----------
scriptunit-fragment
----------

let c = w + 1

----------
scriptunit-fragment
----------

set w = w * 2
```
<!-- end world -->

<!-- world rerun-through-moves step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let a2 = a` | takes `a` (1) | compiles, runs | new | `a2` moved |
| 3 | `var w = a2` | takes `a2` (2) | compiles, runs | new | `w` = 2000 |
| 4 | `let c = w + 1` | reads `w` (3) | compiles, runs | new | `c` = 1001 |
| 5 | `set w = w * 2` | updates `w` (3) | compiles, runs | new | - |
<!-- end world -->

Edit unit 4, which reads `w` before unit 5 doubles it.

<!-- world rerun-through-moves step 2 -->
**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 2 reruns and takes `a`, which it took when it last ran | `a` moved |
| 2 | `let a2 = a` | takes `a` (1) | reruns | rerun: unit 3 reruns and takes `a2`, which it took when it last ran | `a2` moved |
| 3 | `var w = a2` | takes `a2` (2) | reruns | rerun: unit 4 reads `w`, which unit 5 updated after it | `w` = 2000 |
| 4 | `let c = w + 2` | reads `w` (3) | recompiles, runs | edited | `c` = 1002 |
| 5 | `set w = w * 2` | updates `w` (3) | reruns | rerun: updates `w` from unit 3, which reran | - |
<!-- end world -->

Unit 3 reruns to rebuild `w`,
but it took `a2` the last time it ran,
so unit 2 reruns first,
and that took `a`, so unit 1 reruns before it.
Each link is a take some unit recorded by name,
so following the records follows the value.




## What if values flow through intermediate bindings?

The forward direction.
Unit 3 reads `b` and computes `d`,
unit 4 writes `d` into an earlier `var`,
and unit 5 reads that `var`.

<!-- world values-through-bindings code -->
```datalove
----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

var total = 0

----------
scriptunit-fragment
----------

let d = b * 2

----------
scriptunit-fragment
----------

set total = d + 1

----------
scriptunit-fragment
----------

let shown = total * 10
```
<!-- end world -->

<!-- world values-through-bindings step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 125 |
| 2 | `var total = 0` | - | compiles, runs | new | `total` = 251 |
| 3 | `let d = b * 2` | reads `b` (1) | compiles, runs | new | `d` = 250 |
| 4 | `set total = d + 1` | overwrites `total` (2), reads `d` (3) | compiles, runs | new | - |
| 5 | `let shown = total * 10` | reads `total` (2) | compiles, runs | new | `shown` = 2510 |
<!-- end world -->

Change `b`.

<!-- world values-through-bindings step 2 -->
**Step 2: unit 1 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 50` | - | recompiles, runs | edited | `b` = 50 |
| 2 | `var total = 0` | - | untouched | unit 4 overwrites `total` without reading it | `total` = 101 |
| 3 | `let d = b * 2` | reads `b` (1) | reruns | reads `b` from unit 1 | `d` = 100 |
| 4 | `set total = d + 1` | overwrites `total` (2), reads `d` (3) | reruns | reads `d` from unit 3 | - |
| 5 | `let shown = total * 10` | reads `total` (2) | reruns | reads `total` after unit 4, which wrote a new value | `shown` = 1010 |
<!-- end world -->

The rerun reaches unit 5,
though unit 5 names nothing units 1 or 3 make:
unit 4 left `total` holding a new value.
Unit 2 is left alone,
because unit 4 overwrites `total` without reading it.

Then mix the directions.
Make unit 4 also update some unrelated binding,
so its forward rerun needs a backward rerun first.

<!-- world forward-needs-back code -->
```datalove
----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

var z = 10

----------
scriptunit-fragment
----------

var total = 0

----------
scriptunit-fragment
----------

set total = b + 1
set z = z + 1

----------
scriptunit-fragment
----------

let shown = total * 10
```
<!-- end world -->

<!-- world forward-needs-back step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 125 |
| 2 | `var z = 10` | - | compiles, runs | new | `z` = 11 |
| 3 | `var total = 0` | - | compiles, runs | new | `total` = 126 |
| 4 | `set total = b + 1; set z = z + 1` | reads `b` (1), updates `z` (2), overwrites `total` (3) | compiles, runs | new | - |
| 5 | `let shown = total * 10` | reads `total` (3) | compiles, runs | new | `shown` = 1260 |
<!-- end world -->

<!-- world forward-needs-back step 2 -->
**Step 2: unit 1 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 50` | - | recompiles, runs | edited | `b` = 50 |
| 2 | `var z = 10` | - | reruns | rerun: unit 4 reruns and updates `z`, which it updated when it last ran | `z` = 11 |
| 3 | `var total = 0` | - | untouched | unit 4 overwrites `total` without reading it | `total` = 51 |
| 4 | `set total = b + 1; set z = z + 1` | reads `b` (1), updates `z` (2), overwrites `total` (3) | reruns | reads `b` from unit 1 | - |
| 5 | `let shown = total * 10` | reads `total` (3) | reruns | reads `total` after unit 4, which wrote a new value | `shown` = 510 |
<!-- end world -->

Unit 4 reruns forward, because it reads `b`.
It updated `z` when it last ran,
so unit 2 reruns backward first.

How many rounds until it settles?
The engine doesn't count: it loops until nothing is added.
Each round only adds units,
so it settles, and the count is a cost, not a correctness question.
The chain of moves in the section before is one round per link.




## What if a move or mutation is conditional, or in a loop?

<!-- world conditional-write code -->
```datalove
----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

let flag = true

----------
scriptunit-fragment
----------

if flag
  set b = 500
end if

----------
scriptunit-fragment
----------

let c = b + 1
```
<!-- end world -->

Unit 3's write depends on a value, not just on code.

<!-- world conditional-write step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 2 | `let flag = true` | - | compiles, runs | new | `flag` = true |
| 3 | `if flag; set b = 500; end if` | reads `flag` (2), updates `b` (1) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |
<!-- end world -->

Edit unit 2 to `false`,
and unit 3 now does nothing to `b` at run time,
though its record still says it writes `b`.

<!-- world conditional-write step 2 -->
**Step 2: unit 2 is edited so the write does not happen.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | reruns | rerun: unit 3 may not write `b`, so it needs `b` as unit 1 made it | `b` = 125 |
| 2 | `let flag = false` | - | recompiles, runs | edited | `flag` = false |
| 3 | `if flag; set b = 500; end if` | reads `flag` (2), updates `b` (1) | reruns | reads `flag` from unit 2 | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | reads `b` after unit 3, which no longer writes it | `c` = 126 |
<!-- end world -->

So `b` has to be rebuilt to 125, and unit 4 rerun.
The record says unit 3 _updates_ `b`, not overwrites it:
a write that might not happen leaves the old value on some path,
so it counts as reading it.
That's why unit 3 can't be a starting point for a rerun of `b`.

A take in one branch only is already an error,
so a conditional take is out.

An update in a loop is still one update to the table.

<!-- world loop-update code -->
```datalove
----------
scriptunit-fragment
----------

var b = 1

----------
scriptunit-fragment
----------

let n = 3

----------
scriptunit-fragment
----------

var i = 0
loop while i .< n
  set b = b * 2
  set i = i + 1
end loop

----------
scriptunit-fragment
----------

let c = b + 1
```
<!-- end world -->

<!-- world loop-update step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | compiles, runs | new | `b` = 8 |
| 2 | `let n = 3` | - | compiles, runs | new | `n` = 3 |
| 3 | `var i = 0; loop while i .< n; set b = b * 2; set i = i + 1; end loop` | updates `b` (1), reads `n` (2) | compiles, runs | new | `i` = 3 |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 9 |
<!-- end world -->

Edit unit 2 to loop once more.

<!-- world loop-update step 2 -->
**Step 2: unit 2 is edited to loop once more.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | reruns | rerun: unit 3 updates `b` when it last ran, and reruns | `b` = 16 |
| 2 | `let n = 4` | - | recompiles, runs | edited | `n` = 4 |
| 3 | `var i = 0; loop while i .< n; set b = b * 2; set i = i + 1; end loop` | updates `b` (1), reads `n` (2) | reruns | reads `n` from unit 2 | `i` = 4 |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | reads `b` after unit 3, which wrote a new value | `c` = 17 |
<!-- end world -->

A loop that might run zero times counts like an `if`,
so even `set b = 500` inside one would be an update.




## What if a unit writes only part of a binding?

<!-- world partial-write code -->
```datalove
----------
scriptunit-fragment
----------

var p = (1, "a")

----------
scriptunit-fragment
----------

let n = p.0 + 1

----------
scriptunit-fragment
----------

set p.1 = "b"
```
<!-- end world -->

Unit 2 reads only `p.0`,
and unit 3 writes only `p.1`.

<!-- world partial-write step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var p = (1, "a")` | - | compiles, runs | new | `p` = (1, "b") |
| 2 | `let n = p.0 + 1` | reads `p` (1) | compiles, runs | new | `n` = 2 |
| 3 | `set p.1 = "b"` | updates `p` (1) | compiles, runs | new | - |
<!-- end world -->

Edit unit 2.
By dataflow nothing else needs to run.

<!-- world partial-write step 2 -->
**Step 2: unit 2 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var p = (1, "a")` | - | untouched | unit 2 reads only `p.0`, which unit 3 does not write | `p` = (1, "b") |
| 2 | `let n = p.0 + 2` | reads `p` (1) | recompiles, runs | edited | `n` = 3 |
| 3 | `set p.1 = "b"` | updates `p` (1) | untouched | nothing it uses changed | - |

<!--
Engine today, more than the rule asks:
unit 1 reruns: tracks bindings by name, so the write to `p.1` counts as a write to all of `p`
unit 3 reruns: writes `p` again after unit 1 reruns
-->
<!-- end world -->

By names the reader depends on the write,
and that's what the engine goes by:
a write to `p.1` counts as a write to all of `p`,
and an update at that, since the rest of `p` is kept.
So it reruns unit 1 to rebuild `p` and unit 3 to write it again.
Sound, and a cost of one rerun of the provider and the writers per such edit.
Tracking places instead of names would save it.
Not done.




## What if a rerun produces the same value?

<!-- world same-value code -->
```datalove
----------
scriptunit-fragment
----------

var b = 125

----------
scriptunit-fragment
----------

let big = b .> 100

----------
scriptunit-fragment
----------

let shown = big
```
<!-- end world -->

<!-- world same-value step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 125 |
| 2 | `let big = b .> 100` | reads `b` (1) | compiles, runs | new | `big` = true |
| 3 | `let shown = big` | reads `big` (2) | compiles, runs | new | `shown` = true |
<!-- end world -->

Change `b` from 125 to 126.
Unit 2 reruns, and `big` is still `true`,
so nothing that reads `big` needs to rerun.

<!-- world same-value step 2 -->
**Step 2: unit 1 is edited, and `big` comes out the same.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 126` | - | recompiles, runs | edited | `b` = 126 |
| 2 | `let big = b .> 100` | reads `b` (1) | reruns | reads `b` from unit 1 | `big` = true |
| 3 | `let shown = big` | reads `big` (2) | untouched | `big` is what it was | `shown` = true |

<!--
Engine today, more than the rule asks:
unit 3 reruns: does not compare a rerun unit's values with what they were
-->
<!-- end world -->

The engine reruns unit 3 anyway.
It doesn't compare a rerun unit's values with what they were.
That's salsa's backdating, applied to run-time values.
It needs values compared, which may cost more than it saves.
Worth trying on a chain where it cuts off something expensive.




## What does rerunning cost?

A rerun reruns the unit that made the binding,
and that may be the expensive one,
like a unit that loads and parses a big dataset.
The worst case is a chain of takes.

<!-- world rerun-chain code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1

----------
scriptunit-fragment
----------

let b = a

----------
scriptunit-fragment
----------

let c = b

----------
scriptunit-fragment
----------

let d = c

----------
scriptunit-fragment
----------

let e = d + 1
```
<!-- end world -->

<!-- world rerun-chain step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1` | - | compiles, runs | new | `a` moved |
| 2 | `let b = a` | takes `a` (1) | compiles, runs | new | `b` moved |
| 3 | `let c = b` | takes `b` (2) | compiles, runs | new | `c` moved |
| 4 | `let d = c` | takes `c` (3) | compiles, runs | new | `d` = 1 |
| 5 | `let e = d + 1` | reads `d` (4) | compiles, runs | new | `e` = 2 |
<!-- end world -->

Edit the last link.

<!-- world rerun-chain step 2 -->
**Step 2: unit 4, at the end of the chain, is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1` | - | reruns | rerun: unit 2 reruns and takes `a` | `a` moved |
| 2 | `let b = a` | takes `a` (1) | reruns | rerun: unit 3 reruns and takes `b` | `b` moved |
| 3 | `let c = b` | takes `b` (2) | reruns | rerun: unit 4 takes `c`, which it took when it last ran | `c` moved |
| 4 | `let d = c; let note = 0` | takes `c` (3) | recompiles, runs | edited | `d` = 1, `note` = 0 |
| 5 | `let e = d + 1` | reads `d` (4) | reruns | reads `d` from unit 4 | `e` = 2 |
<!-- end world -->

Unit 4 took `c` the last time it ran,
so unit 3 reruns to make it,
which took `b`, so unit 2 reruns,
which took `a`, so unit 1 reruns.
Editing a cheap unit at the end of a session
reruns the session from where the chain starts.

The alternative is snapshots:
keep a binding's value as each unit found it,
and restore by copying instead of rerunning.
That trades memory for time,
and it makes most of the transitivity above go away,
because a copy has no inputs.
Where's the line between the two?
Could snapshots be taken only of bindings a later unit takes or writes?




## How does one unit returning early affect subsequent units?

A unit that returns early keeps what it did before the return,
and offers none of its bindings.
A unit after it that reads one is blocked:
it compiles, and doesn't run.

Here unit 3 takes `a`, then `!`s out.

<!-- world return-early code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

let d = 0

----------
scriptunit-fragment
----------

let a2 = a
let q = 10 /! d

----------
scriptunit-fragment
----------

let e = q + 1
```
<!-- end world -->

<!-- world return-early step 1 -->
**Step 1: all four units are entered, and unit 3 returns early.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let d = 0` | - | compiles, runs | new | `d` = 0 |
| 3 | `let a2 = a; let q = 10 /! d` | takes `a` (1), reads `d` (2) | compiles, returns early | `10 /! 0` | - |
| 4 | `let e = q + 1` | reads `q` (3) | compiles, is blocked | unit 3 did not finish | - |
<!-- end world -->

Edit unit 2 so the division succeeds.

<!-- world return-early step 2 -->
**Step 2: unit 2 is edited so unit 3 finishes.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 took `a` when it last ran, and reruns | `a` moved |
| 2 | `let d = 5` | - | recompiles, runs | edited | `d` = 5 |
| 3 | `let a2 = a; let q = 10 /! d` | takes `a` (1), reads `d` (2) | reruns | reads `d` from unit 2 | `a2` = 1000, `q` = 2 |
| 4 | `let e = q + 1` | reads `q` (3) | reruns | reads `q` from unit 3, which finished | `e` = 3 |
<!-- end world -->

Is that a fix, an edit, or something else?
None of them.
Unit 3's code didn't change,
so it's a forward rerun, because it reads `d`.
It took `a` when it last ran,
so unit 1 reruns first.
And unit 4, blocked before, runs.

Now `!` out before the take.

<!-- world return-early-before-take code -->
```datalove
----------
scriptunit-fragment
----------

let a = 1000

----------
scriptunit-fragment
----------

let d = 0

----------
scriptunit-fragment
----------

let q = 10 /! d
let a2 = a

----------
scriptunit-fragment
----------

let e = a + 1
```
<!-- end world -->

<!-- world return-early-before-take step 1 -->
**Step 1: all four units are entered, and unit 3 returns early before its take.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `let d = 0` | - | compiles, runs | new | `d` = 0 |
| 3 | `let q = 10 /! d; let a2 = a` | takes `a` (1), reads `d` (2) | compiles, returns early | `10 /! 0` | - |
| 4 | `let e = a + 1` | reads `a` (1) | compile fails | `a` is given away by unit 3 | - |
<!-- end world -->

Unit 4 can't name `a`, though nothing took it at run time.
What a unit gives away is decided when it compiles,
and unit 3's code gives `a` away.

<!-- world return-early-before-take step 2 -->
**Step 2: unit 2 is edited so unit 3 finishes.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | unit 3 returned early before taking `a`, so `a` is as unit 1 made it | `a` moved |
| 2 | `let d = 5` | - | recompiles, runs | edited | `d` = 5 |
| 3 | `let q = 10 /! d; let a2 = a` | takes `a` (1), reads `d` (2) | reruns | reads `d` from unit 2 | `q` = 2, `a2` = 1000 |
| 4 | `let e = a + 1` | reads `a` (1) | untouched | still cannot name `a` | - |

<!--
Engine today, more than the rule asks:
unit 1 reruns: assumes a unit that returned early did everything its code could, so reruns unit 1 for a take that never happened
-->
<!-- end world -->

Unit 3 never took `a`,
so unit 1 needs nothing when unit 3 finishes.
But the engine assumes a unit that returned early
did everything its code could,
and reruns unit 1 for a take that never happened.
Sound, more than needed.




## Are the rules right, and what does the engine do today?

Every step table in this document is a test case.
Each lives as a fixture in `crates/datalove-script-engine/tests/fixtures/reactive/`,
is rendered into this document from there,
and is checked against the engine by `crates/datalove-script-engine/tests/reactive_tests.rs`:
which units recompile, which rerun,
what each does to earlier bindings,
and what each holds after.

Where the engine does more than a table asks,
the fixture says so,
and it shows as a comment under the table.
What's left of those:

- Retracting a revival reruns the provider and the take, where a fixup would do.
- The splices, delete and insert, rerun every unit from the rerun point on,
  readers included, and start that point at the provider.
- A write to part of a binding counts as a write to all of it.
- A rerun that comes out the same still reruns its readers.
- A unit that returned early is assumed to have done everything its code could.

And recompiles aren't held to anything tighter than salsa's memos yet.




