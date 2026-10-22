<!--

Title: An initial semantics for reactive interactive script units in Datalove
Summary:

Audience: practitioners

Problem:
Datalove incrementally recompiles and reexecutes only dependent script units
when dependent code and data changes. What does this mean in the cases?

Thesis:
Antithesis:
Synthesis:

-->

Datalove's language and compiler model have been designed
primarily as an interactive scripting environment.
It has been designed such that editing existing
code quickly recompiles previously compiled code.
It has been designed such that editing existing
code _should be able to_ quickly re-execute previously compiled code,
producing the same result.
Datalove's scripting environment is intended to be
deterministically reactive to its code and data input.

Until now though the CLI REPL has only barely functioned,
and the actual semantics of what happens
as code is recompiled and reexecuted have been left to future work.
Ok, future is now.
Let's figure it out.

- [Datalove's design for reactivity](#user-content-dataloves-design-for-reactivity)
- [How does one unit failing to compile affect subsequent units?](#user-content-how-does-one-unit-failing-to-compile-affect-subsequent-units)
- [How is re-execution affected when subsequent units consume bindings from the re-executed unit?](#user-content-how-is-re-execution-affected-when-subsequent-units-consume-bindings-from-the-re-executed-unit)
- [How is re-execution affected when subsequent units mutate the re-executed unit?](#user-content-how-is-re-execution-affected-when-subsequent-units-mutate-the-re-executed-unit)
- [What if a unit overwrites a binding without reading it?](#user-content-what-if-a-unit-overwrites-a-binding-without-reading-it)
- [Hypothesis: an algebra of rewind and replay for moves and mutations](#user-content-hypothesis-an-algebra-of-rewind-and-replay-for-moves-and-mutations)
- [What if a unit reads a binding before a later unit moves it?](#user-content-what-if-a-unit-reads-a-binding-before-a-later-unit-moves-it)
- [What if a unit reads a binding before a later unit mutates it?](#user-content-what-if-a-unit-reads-a-binding-before-a-later-unit-mutates-it)
- [What if a unit reads a binding after an earlier unit mutated it?](#user-content-what-if-a-unit-reads-a-binding-after-an-earlier-unit-mutated-it)
- [What if an edit adds or removes a move?](#user-content-what-if-an-edit-adds-or-removes-a-move)
- [What if an edit adds or removes a mutation?](#user-content-what-if-an-edit-adds-or-removes-a-mutation)
- What if a unit that moves or mutates is deleted?
- What if a unit gives a moved `var` a value again?
- What if rewinding one unit needs another rewound first?
- What if a move or mutation is conditional, or in a loop?
- What does rewinding cost?
- [How does one unit returning early affect subsequent units?](#user-content-how-does-one-unit-returning-early-affect-subsequent-units)
- Are the rules right, and what the engine does today




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

Of course a language that doesn't do any I/O is quite limited.
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

**Step 1: all three units are entered, unit 1 broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | compile fails | new | - |
| 2 | `let b = 100` | - | compiles, runs | new | `b` = 100 |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | compile fails | `a` is unbound: unit 1 failed | - |

Imagine we fix the first unit's compilation error.
That allows the first and third units to both compile and run.
But we need to completely skip the second unit because it already did both
and has no dependencies on the first.

**Step 2: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` moved |
| 2 | `let b = 100` | - | untouched | uses nothing of unit 1's | `b` moved |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompiles, runs | takes `a` from unit 1 | `a_moved` = 1000, `b_moved` = 100, `c` = 10 |

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
and that anything that undoes or redoes unit 3 has to build the binding again,
by rerunning unit 2.

**Step 3: unit 1 is broken again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `let b = 100` | - | reruns | rewind: unit 3 took `b` when it last ran, and is undone | `b` = 100 |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |

The rule is in terms of what unit 3 did _the last time it ran_,
not what it does now.
That is what the running session went through,
and the session has to end up the same as if it had been run fresh from the top.

Then when we make unit 1 compile again,
unit 2 has nothing to do:
`b` is already as unit 2 made it,
and unit 3 moves it again.

**Step 4: unit 1 is fixed again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` moved |
| 2 | `let b = 100` | - | untouched | `b` is as unit 2 made it | `b` moved |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompiles, runs | takes `a` from unit 1 | `a_moved` = 1000, `b_moved` = 100, `c` = 10 |




## How is re-execution affected when subsequent units mutate the re-executed unit?

Here's a new variation on our running example
that mutates instead of moves.

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

The third unit doubles `b` in place.
Doubling is not idempotent:
running it twice is not the same as running it once.
On first run it mutates `b` from 125 to 250, and `c` is 4.

**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | compiles, runs | new | `c` = 4 |

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

**Step 2: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | reruns | rewind: unit 3 updated `b` when it last ran, and is undone | `b` = 125 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |

Now if we fix unit 1,
unit 3 runs against `b` as unit 2 made it,
doubling 125 to 250, and `c` is 4 again.
Had we left unit 2 alone,
unit 3 would have doubled the 250 it left behind last time,
to 500,
and `c` would have been 2.

**Step 3: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 2 made it | `b` = 250 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompiles, runs | reads `a` from unit 1 | `c` = 4 |

The analysis is essentially the same,
but instead of a dependency on move operations
we have a dependency on writes:
`set`, and passing a binding to a `mut` or `out` parameter.




## What if a unit overwrites a binding without reading it?

Doubling `b` reads it before writing it,
so running the write again starting from its own result gives the wrong answer.
But not every write reads.
Here unit 3 just replaces `b`:

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

On first run unit 3 replaces 125 with 500, and `c` is 2.

**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 3 | `set b = 500; let c = a /! b` | reads `a` (1), overwrites `b` (2) | compiles, runs | new | `c` = 2 |

Now edit unit 3 to write a different value.
Unit 3 has to rerun, and it writes `b` again.
But the write doesn't care what `b` held,
so it doesn't matter that `b` is still the 500 that unit 3 left last time.
Unit 2 can be left alone,
which is where an overwrite differs from an update.

**Step 2: unit 3 is edited to write 250.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing it uses changed | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 250 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompiles, runs | edited | `c` = 4 |

Undoing the overwrite is a different story.
Break unit 1 and unit 3 stops compiling,
so the session should look like unit 3 never ran,
and `b` should be 125 again.
Nothing but unit 2 knows how to make that 125,
so it reruns,
exactly as it did for the move and for the update.

**Step 3: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | reruns | rewind: unit 3 overwrote `b` when it last ran, and is undone | `b` = 125 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |

And fixing unit 1 reruns unit 3,
which overwrites `b` again without anything to undo first.

**Step 4: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 250 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompiles, runs | reads `a` from unit 1 | `c` = 4 |

So writes come in two kinds.
An _update_ reads the binding first,
and like a move it needs the binding rebuilt both to undo it and to redo it.
An _overwrite_ doesn't read it,
and needs the binding rebuilt only to undo it.

<!--
Note: Datalove's engine today doesn't make this distinction.
It treats every write like an update, so in step 2 it reruns unit 2 as well.
Correct, but more than necessary. A candidate for the last section.
-->




## Hypothesis: an algebra of rewind and replay for moves and mutations

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

Each has effects in two directions.

_Backward_.
To _undo_ a unit, because it was deleted or stopped compiling,
or to _redo_ it, because it was edited or something it uses was,
the earlier bindings it touched have to be as they were before it first ran.
When they aren't we _rewind_:
rerun the unit that made the binding,
which builds all of that unit's bindings again,
then _replay_ the units after it that took or wrote any of them, in order.
Units that only read them don't need to replay for the rewind's sake.
The rerun unit wasn't edited,
so it builds the same values,
and a reader would compute what it already holds.
If what a reader sees does change,
because the unit being undone or redone changed it,
that's the forward rule's business, below.

_Forward_.
What a unit did to a binding is what the units after it see,
so changing what it does reaches them,
whether or not they use anything the unit itself defines.

| a unit that ... an earlier binding | undo it | redo it | units after it |
|------------|---------|----------------------------------------|--------------------------------------------|
| reads      | nothing | rewind if a unit after it took or wrote it | nothing                                 |
| takes      | rewind  | rewind                                  | can't name it, so recompile when that changes |
| overwrites | rewind  | nothing                                 | read the new value, so rerun when it changes |
| updates    | rewind  | rewind                                  | read the new value, so rerun when it changes |

Moves and mutations agree on undo,
and disagree on redo and on what they pass forward.

<!--
Note: the engine today treats take, overwrite and update all like take,
so it rewinds to redo an overwrite. Otherwise it follows this table:
a read rewinds only when the reader or a unit after it took or wrote the
binding, and a rewind replays only the units that took or wrote what it
rebuilt, not the readers. The splices (delete, insert, truncate) still
rerun every unit from the rewind point on. A candidate for the last
section.
-->




## What if a unit reads a binding before a later unit moves it?

So far the unit being redone was the one doing the moving.
But a unit that only reads can be caught out too,
if the binding it reads was taken _after_ it ran.

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

Unit 2 borrows `a`, and unit 3 takes it.

**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let a_moved = a` | takes `a` (1) | compiles, runs | new | `a_moved` = 1000 |

Now edit unit 2.
It reads `a`, which is empty:
unit 3 took it after unit 2 last ran.
Nothing unit 3 does depends on unit 2,
and unit 2 is only reading,
but unit 2 still can't run until unit 1 builds `a` again.
And once unit 1 has rerun,
unit 3 has to take the new `a`,
or `a` will be left holding a value nothing took.

**Step 2: unit 2 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rewind: unit 2 reads `a`, which unit 3 took after it | `a` moved |
| 2 | `let c = a /! 100` | reads `a` (1) | recompiles, runs | edited | `c` = 10 |
| 3 | `let a_moved = a` | takes `a` (1) | reruns | replay: takes `a` from unit 1, which reran | `a_moved` = 1000 |

So a read can need a rewind as much as a move can.
What matters is not what the edited unit does to `a`,
but whether anything changed `a` between when it was made and now.
The behavior of units that are subsequent to the edit
affects which units need to be redone.




## What if a unit reads a binding before a later unit mutates it?

The same goes for a mutation after the read.

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

Unit 3 reads `b` as 125, so `c` is 8.
Then unit 4 doubles `b` to 250.

**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
| 4 | `set b = b * 2` | updates `b` (2) | compiles, runs | new | - |

Edit unit 3 to double its result.
Rerun on its own it would read the 250 that unit 4 left
and get 8 again, rather than 16.
So unit 2 reruns to put `b` back to 125,
unit 3 runs against that,
and unit 4 doubles `b` again.

**Step 2: unit 3 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rewind: unit 3 reads `b`, which unit 4 updated after it | `b` = 250 |
| 3 | `let c = (a /! b) * 2` | reads `a` (1), reads `b` (2) | recompiles, runs | edited | `c` = 16 |
| 4 | `set b = b * 2` | updates `b` (2) | reruns | replay: updates `b` from unit 2, which reran | - |

Here it doesn't matter what kind of write unit 4 makes.
Had it been an overwrite, `set b = 500`,
unit 3 would still have read the wrong `b`,
so the rewind is the same.
The kind of write decides whether redoing _the writer_ needs a rewind,
not whether redoing a unit before it does.

Unit 1 is a useful control.
Unit 3 reads `a` too, but nothing changed `a`,
so unit 1 is left alone.




## What if a unit reads a binding after an earlier unit mutated it?

Swap units 3 and 4, so the write comes first.

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

**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | compiles, runs | new | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 4 |

Now unit 4 reads `b` after the write,
and the `b` it reads is the one unit 3 left.
That's still what's there,
so editing unit 4 needs nothing else rerun.

**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 4 first saw it | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | untouched | unit 4 reads after it | - |
| 4 | `let c = (a /! b) * 2` | reads `a` (1), reads `b` (2) | recompiles, runs | edited | `c` = 8 |

A write passes its value forward,
and a unit after it depends on it the way it would on a `let`.
A move has no such case.
If unit 3 had been `let b_moved = b`,
unit 4 wouldn't compile at all:

```
error[D013]: `b` was given away by an earlier input
```

So the rule for a reader is narrower than "anything changed the binding".
A reader needs a rewind only if the binding was changed
_after_ the reader last ran.
Changes before it are just what it read.

<!--
Note: the engine today matches this, rerunning only unit 4, in the D013
case too.
-->




## What if an edit adds or removes a move?

Every edit so far kept what the edited unit does to earlier bindings.
A unit that took `a` before the edit still took it after.
But an edit can change that too,
and turn a read into a take, or a take into a read.

Then the edit is two things at once.
It undoes what the old code did the last time it ran,
and it redoes what the new code does now.
The bindings the old code touched need what the undo rules ask for,
and the bindings the new code touches need what the redo rules ask for.
Forward, the units after it see the difference between the two.

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

Three units read `a`, and nothing takes it.

**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | compiles, runs | new | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | compiles, runs | new | `e` = 1002 |

Now edit unit 3 to take `a` instead of reading it.
The old code only read `a`, so there is nothing to undo.
The new code takes `a`,
and the redo rules say a take needs a rewind.
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
error[D013]: `a` was given away by an earlier input
```

**Step 2: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | `a` is as unit 1 made it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | reads `a` before the take | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a + 2` | reads `a` (1) | recompile fails | `a` was given away by unit 3 | - |

Unit 2 is untouched too.
It reads `a` before the take,
so what it read is still what's there.

Now edit unit 3 back to a read.
This time the old code took `a`,
and undoing a take needs a rewind:
unit 1 reruns to build `a` again.
The new code reads `a`, which is now there to read.
And unit 4 can name `a` again,
so it recompiles and runs.

**Step 3: unit 3 is edited back to read `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rewind: unit 3 took `a` when it last ran, and the edit undoes that | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 built the same `a` | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | recompiles, runs | edited | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | recompiles, runs | `a` can be named again | `e` = 1002 |

Adding the move and removing it are not mirror images.
Adding it cost nothing backward and broke a unit forward.
Removing it needed a rewind backward, and fixed that unit forward.

The added move does need a rewind
if something after it already took the binding.
Make unit 4 `let e = a`, so it is the one that takes `a`,
and then edit unit 3 to take it instead.

**Step 2, with unit 4 taking `a`: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rewind: unit 3 takes `a`, which unit 4 took after it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 built the same `a` | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a` | takes `a` (1) | recompile fails | `a` was given away by unit 3 | - |

The take has moved from unit 4 to unit 3.
The rewind is not because of anything unit 3 did.
Unit 4 emptied `a` after unit 3 last ran,
the same case as a read before a later move.
The redo rule holds if it is read that way:
a take needs a rewind when the binding is not as its maker left it,
and that is usually, but not always, because the unit took it itself.

<!--
Note: the engine today matches all three steps and the variant.
-->




## What if an edit adds or removes a mutation?

A move added or removed changes which names the units after it can use.
A mutation doesn't change any names.
It changes values.

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

Unit 3 reads `b` and doubles it into a binding of its own.
Unit 4 reads `b` as 125, so `c` is 8.

**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 125 |
| 3 | `let d = b * 2` | reads `b` (2) | compiles, runs | new | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |

Now edit unit 3 to double `b` in place.
As with the added move, nothing needs to be undone,
and `b` still holds what unit 2 made,
so the new update needs no rewind.
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

**Step 2: unit 3 is edited to update `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 2 made it | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | recompiles, runs | edited | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which now updates it | `c` = 4 |

Now edit unit 3 back to a read.
The old code updated `b`,
and undoing an update needs a rewind,
so unit 2 reruns to put `b` back to 125.
The new code reads that 125,
and `d` is 250 again.

Unit 4 reruns too, but not for the rewind's sake.
A rewind replays only the units that took or wrote what was rebuilt,
and unit 4 only reads.
It reruns because the edit changed the `b` it sees,
from the 250 unit 3 left
back to the 125 unit 2 made.
That's the forward rule.

**Step 3: unit 3 is edited back to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rewind: unit 3 updated `b` when it last ran, and the edit undoes that | `b` = 125 |
| 3 | `let d = b * 2` | reads `b` (2) | recompiles, runs | edited | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which no longer updates it | `c` = 8 |

An overwrite in place of the update, `set b = 500`,
reruns the same units in both steps.
The kinds of write differ on redo,
and a write that is new is never redone against its own result.
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
Note: the engine today matches steps 2 and 3, rerunning units 3 and 4,
then units 2, 3 and 4.
With the fifth unit, it rewinds to unit 2 for the added overwrite as well
as the added update, because it treats every write like an update.
Same gap as noted under the overwrite section.
-->




## How does one unit returning early affect subsequent units?


