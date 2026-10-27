<!--

_Title_: Reactive compilation and evaluation based on analysis of bindings with linear types
_Summary_: With a simple linear type system we correctly decide which
           code units to recompile and rerun when code and input changes
           based on a simple fixed-point analysis of how bindings are used.

_Audience_: PL practitioners

_Problem_:
Datalove incrementally recompiles and reexecutes only dependent script units
when dependent code and data changes.
How do we decide which code to recompile and rerun and when?

_ABTstract_:
A reactive notebook-style script should rerun only what an edit affects,
_and_ Datalove's linear types tell the compiler exactly which
earlier bindings each unit reads, moves or mutates.
_But_ a move or mutation leaves its effect behind in an earlier unit's state.
When that unit is edited, broken or deleted,
the environment no longer matches the code.
_Therefore_ we derive which units to rerun from the few ways a unit can touch an earlier binding.
The rules are applied until nothing more needs to rerun,
and every case is checked by a literate test suite.

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




- <!-- and -->
  [What is a reactive script?](#user-content-what-is-a-reactive-script)
- <!-- and -->
  [Datalove's design for reactivity](#user-content-dataloves-design-for-reactivity)
- <!-- but..therefore -->
  [If one unit moves or mutates a binding how does that affect preceding units on replay?](#user-content-if-one-unit-moves-or-mutates-a-binding-how-does-that-affect-preceding-units-on-replay)
- <!-- therefore -->
  [A naive algebra of reruns for moves and mutations](#user-content-a-naive-algebra-of-reruns-for-moves-and-mutations)
- <!-- but..therefore -->
  [But what about edits that change what a unit does?](#user-content-but-what-about-edits-that-change-what-a-unit-does)
- <!-- therefore -->
  [A second algebra: retract and apply](#user-content-a-second-algebra-retract-and-apply)
- <!-- but..therefore -->
  [But what about edits that change which names later units can use?](#user-content-but-what-about-edits-that-change-which-names-later-units-can-use)
- <!-- therefore -->
  [A third algebra: breaks and fixes](#user-content-a-third-algebra-breaks-and-fixes)
- <!-- but..therefore -->
  [But what if a rerun unit has other bindings that must be restored?](#user-content-but-what-if-a-rerun-unit-has-other-bindings-that-must-be-restored)
- <!-- therefore -->
  [A complete recursive algebra of recompiles and reruns for moves and mutations](#user-content-a-complete-recursive-algebra-of-recompiles-and-reruns-for-moves-and-mutations)
- <!-- but..therefore -->
  [Where the static picture is approximate](#user-content-where-the-static-picture-is-approximate)
- <!-- but..therefore -->
  [Validating this design with a literate test suite](#user-content-validating-this-design-with-a-literate-test-suite)
- <!-- but..therefore -->
  [Comparing to FRP literature](#user-content-comparing-to-frp-literature)
- <!-- therefore -->
  [A live example of the Datalove scripting engine](#user-content-a-live-example-of-the-datalove-scripting-engine)




## What is a reactive script?

<!--

_And_ this is a primer on reactive scripts.

Thesis: rerun dependent code.
Antithesis: hidden state problem.

Don't want to compare to existing systems.
It's not really my goal to solve a _new_ problem, just _my_ problem.

Motivating example.

-->





## Datalove's design for reactivity

<!--

_And_ these are the properties that will help us solve the problem.

_Therefore_ about the hidden state problem.

_But_ what about I/O?
_Therefore_ here is how we model it.

Refine the motivating example to a datalove worldfile,
introducing the concept.
We'll show a version of it live at the end.

-->

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
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

let b = 100

----------
scriptunit-fragment 3
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




## If one unit moves or mutates a binding how does that affect preceding units on replay?

<!--
_But_ need a simple clear example that shows the core problem.
_Therefore_ what we decide to recompile and rerun.

_But...therefore_ modest but succinct elaboration.

Demonstrate read/take/overwrite/update
-->

Whatever we rerun,
the session must end up as a fresh run of the current code
from the top would leave it.
That's the rule every step below is held to.

After a unit declares a binding,
later units may perform four kinds of _operations_ on it:

- _read_ it, leaving it as it was,
- _take_ it, moving the value out and leaving the binding empty,
- _overwrite_ it, writing a new value without reading the old one,
- _update_ it, reading the old value and writing a new one.

A take is a move.
Overwrites and updates are the two kinds of mutation,
and only a `var` can be mutated.
Integer literals are `int`, a bigint,
which is linear,
so `let f = b` moves `b` rather than copying it.

The first four units each make one binding.
The fifth does something different to each of them.

```datalove
----------
scriptunit-fragment 1
----------

let a = 1

----------
scriptunit-fragment 2
----------

let b = 2

----------
scriptunit-fragment 3
----------

var c = 3

----------
scriptunit-fragment 4
----------

var d = 4

----------
scriptunit-fragment 5
----------

let e = a + 10
let f = b
set c = 30
set d = d * 2
```

Unit 5 _reads_ `a`, _takes_ `b`, _overwrites_ `c` and _updates_ `d`.

Each change to the session is a _step_,
and a table says what each unit does in it.
_Uses from earlier units_ names the unit each binding came from in parentheses.
_Holds after_ is what a unit's bindings hold at the end of the step,
whichever unit last changed them:
unit 3 holds `c` = 30 because unit 5 overwrote it.

**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1` | - | compiles, runs | new | `a` = 1 |
| 2 | `let b = 2` | - | compiles, runs | new | `b` moved |
| 3 | `var c = 3` | - | compiles, runs | new | `c` = 30 |
| 4 | `var d = 4` | - | compiles, runs | new | `d` = 8 |
| 5 | `let e = a + 10; let f = b; set c = 30; set d = d * 2` | reads `a` (1), takes `b` (2), overwrites `c` (3), updates `d` (4) | compiles, runs | new | `e` = 11, `f` = 2 |

Every unit compiles and runs once, in order.


**Step 2: unit 5 is edited.**

Next edit unit 5, changing `a + 10` to `a + 100`.

```datalove
----------
scriptunit-edit 5
----------

let e = a + 100
let f = b
set c = 30
set d = d * 2
```

If we simply reran this unit it would
_take_ `b` from a slot it emptied itself the last time it ran,
and _update_ `d` again from 8 to 16.

A fresh run from the top would do neither.
Units 2 and 4 have to rerun first,
so that `b` and `d` are as unit 5 first found them.
Unit 3 does not need to rerun:
unit 5 _overwrites_ `c` without looking at what it held.
Unit 1 doesn't either,
because _reading_ `a` left it as it was.

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1` | - | untouched | unit 5 only reads `a` | `a` = 1 |
| 2 | `let b = 2` | - | reruns | rerun: unit 5 takes `b`, which it took when it last ran | `b` moved |
| 3 | `var c = 3` | - | untouched | unit 5 overwrites `c` without reading it | `c` = 30 |
| 4 | `var d = 4` | - | reruns | rerun: unit 5 updates `d`, which it updated when it last ran | `d` = 8 |
| 5 | `let e = a + 100; let f = b; set c = 30; set d = d * 2` | reads `a` (1), takes `b` (2), overwrites `c` (3), updates `d` (4) | recompiles, runs | edited | `e` = 101, `f` = 2 |

**Step 3: unit 1 is edited.**

Now edit unit 1 instead,
changing `a` from 1 to 2.

```datalove
----------
scriptunit-edit 1
----------

let a = 2
```

Unit 5 reads `a`,
so it reruns.
But rerunning unit 5 puts it in the same spot as editing it did.
It would take `b` from the slot it emptied
and double `d` a second time,
so units 2 and 4 rerun before it,
just as they did in step 2.
The edited unit forced a rerun of a subsequent unit,
then that unit transitively forced a rerun of previous units.

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 2` | - | recompiles, runs | edited | `a` = 2 |
| 2 | `let b = 2` | - | reruns | rerun: unit 5 reruns and takes `b`, which it took when it last ran | `b` moved |
| 3 | `var c = 3` | - | untouched | unit 5 overwrites `c` without reading it | `c` = 30 |
| 4 | `var d = 4` | - | reruns | rerun: unit 5 reruns and updates `d`, which it updated when it last ran | `d` = 8 |
| 5 | `let e = a + 100; let f = b; set c = 30; set d = d * 2` | reads `a` (1), takes `b` (2), overwrites `c` (3), updates `d` (4) | reruns | reads `a` from unit 1 | `e` = 102, `f` = 2 |

**Step 4: unit 5 is broken.**

We can also _break_ unit 5,
editing it so it fails to compile.

```datalove
----------
scriptunit-edit 5
----------

let e = a + 100
let f = b
set c = 30
set d = d * 2

let x = y // These bindings don't exist - compile failure.
```

A fresh run would skip unit 5,
so the session should now look as if unit 5 never ran.
Every binding it changed has to be built again,
`c` included this time:
unit 5 won't run to overwrite it.

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 2` | - | untouched | unit 5 only read `a` | `a` = 2 |
| 2 | `let b = 2` | - | reruns | rerun: unit 5 took `b` when it last ran, and is retracted | `b` = 2 |
| 3 | `var c = 3` | - | reruns | rerun: unit 5 overwrote `c` when it last ran, and is retracted | `c` = 3 |
| 4 | `var d = 4` | - | reruns | rerun: unit 5 updated `d` when it last ran, and is retracted | `d` = 4 |
| 5 | `let e = a + 100; ...; let x = y` | reads `a` (1), takes `b` (2), overwrites `c` (3), updates `d` (4) | recompile fails | edited | - |

So the operation of a unit to an earlier binding decides which earlier units rerun.
If the operation is an overwrite then the nature of the modification
also matters: if an overwrite is edited the earlier unit does not need to rerun,
but if an overwrite is broken the earlier unit does need to rerun to restore the value
that will no longer be overwritten.
And a unit that reruns because an earlier one changed
needs the same as one that was edited.




## A naive algebra of reruns for moves and mutations

<!--
_Therefore_ we propose an algorithm.
-->

Each _modification_ of a unit,
so far an _edit_ or a _break_,
makes a step.
Call the modified unit the _triggering unit_.
In a step each unit is left untouched,
reruns,
recompiles and then runs,
or fails to compile.
The triggering unit always recompiles.
What happens to the earlier units
depends on what the triggering unit's code did to their bindings
the last time it ran:

| operation | break   | edit    |
|-----------|---------|---------|
| read      | nothing | nothing |
| take      | restore | restore |
| overwrite | restore | nothing |
| update    | restore | restore |

A _restore_ puts a binding back as the triggering unit first found it.
It reruns the unit that made the binding,
then, in order, the units between that one and the triggering unit
that wrote it.
Units in between that only read it are left alone.
They would recompute what they already hold.

Overwrite is the only operation that differs between a break and an edit.
An edit skips the restore for an overwrite
because the triggering unit runs again and overwrites the binding,
so what it held before doesn't matter.
A break has nothing to overwrite it so the binding must be restored.

A rerun runs a whole unit,
so a triggering unit that touches several bindings
gets the union of their rows.

Then the units after the triggering unit.
One that reads, takes or updates a binding
the triggering unit made or wrote
sees a new value,
so it reruns,
and so on down the session for whatever the reruns make or write.
A later unit that overwrites the binding
doesn't care what it held,
so does not rerun,
and the units after that one see what it wrote,
so they do not rerun.

_A rerun is an edit that doesn't change the code_,
so a unit rerun this way gets the edit column too,
and triggers the same analysis recursively
to find the full set of units that must rerun.




## But what about edits that change what a unit does?

<!--
_But_ the first algebra assumes an edit keeps the unit's operations.
_Therefore_ an edit retracts the old code and applies the new.
-->

The first algebra looks up what the triggering unit does,
and assumes an edit doesn't change that.
It can.

```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

set b = b * 2

----------
scriptunit-fragment 3
----------

set b = b + 1
```

**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 251 |
| 2 | `set b = b * 2` | updates `b` (1) | compiles, runs | new | - |
| 3 | `set b = b + 1` | updates `b` (1) | compiles, runs | new | - |

Edit unit 2 to read `b` instead of updating it.

```datalove
----------
scriptunit-edit 2
----------

let c = b + 1
```

The first algebra sees that unit 2 is a read of `b`,
which needs nothing,
so unit 2 runs alone, finds 251,
and computes an incorrect 252.
A fresh run gives 126.
Two units wrote `b` since unit 1 made it:
unit 2 itself, with code that no longer exists,
and unit 3, which runs after it.

**Step 2: unit 2 is edited to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | reruns | rerun: unit 2 updated `b` when it last ran, and the edit retracts that | `b` = 126 |
| 2 | `let c = b + 1` | reads `b` (1) | recompiles, runs | edited | `c` = 126 |
| 3 | `set b = b + 1` | updates `b` (1) | reruns | rerun: updates `b`, which unit 1 rebuilt | - |

So an edit is two modifications.
It retracts what the old code did,
and applies what the new code does.
And applying has to look past the triggering unit,
at what later units did to the binding since.




## A second algebra: retract and apply

<!--
_Therefore_ split every modification into a retract and an apply.
-->

Each modification retracts the old code, applies the new code, or both:

| modification | retract the old code | apply the new code |
|--------------|----------------------|--------------------|
| add          | -                    | yes                |
| fix          | -                    | yes                |
| break        | yes                  | -                  |
| delete       | yes                  | -                  |
| edit         | yes                  | yes                |

Adding covers appending a unit and inserting one.
Retract looks up what the old code did to each earlier binding,
and apply what the new code does:

| operation | retract | apply |
|-----------|---------|-------|
| read      | nothing | restore if a later unit took or wrote it |
| take      | restore | restore if a later unit took or wrote it |
| overwrite | restore | nothing |
| update    | restore | restore if a later unit took or wrote it |

Retract restores whatever the old code changed.
Apply restores whatever the new code is going to look at,
if something after it has changed it since.
When the new code overwrites a binding,
what it held before is dead,
so a restore the retract asks for is skipped.
The first algebra's break column is this retract column,
and its edit column is what retract and apply give
when nothing after the triggering unit touched the binding.

A restore undoes what later units did to the binding,
so the units after the triggering unit that took or wrote it rerun.
That's unit 3 in step 2.

The forward rule now has two codes to look at.
A later unit that reads, takes or updates a binding
that either the old or the new code made or wrote
may see a different value,
so it reruns.




## But what about edits that change which names later units can use?

<!--
_But_ a take or a revive changes which bindings later units may name.
_Therefore_ modifications break and fix later units.
-->

Every change so far changed values.
A take changes more:
after it,
later units can't name the binding at all.

```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

let c = a /! 10

----------
scriptunit-fragment 3
----------

let d = a + 1

----------
scriptunit-fragment 4
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

Edit unit 3 to take `a` instead of reading it.

```datalove
----------
scriptunit-edit 3
----------

let d = a
```

The old code only read `a`,
so there is nothing to retract from prior units,
and nothing after unit 3 took or wrote `a`,
so there is nothing to apply to prior units.
The impact is on subsequent units.
Unit 4 reads `a`,
and `a` was given away before unit 4 gets to it.
It hasn't been edited,
and nothing it uses was recompiled,
but it no longer compiles:

```
error[D013]: `a` was given away by an earlier unit
```

**Step 2: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | `a` is as unit 1 made it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | reads `a` before the take | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a + 2` | reads `a` (1) | recompile fails | `a` was given away by unit 3 | - |

Edit unit 3 back to a read.

```datalove
----------
scriptunit-edit 3
----------

let d = a + 1
```

Retracting the take restores `a`,
so unit 1 reruns.
And unit 4 can name `a` again,
so it is fixed.

**Step 3: unit 3 is edited back to read `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 took `a` when it last ran, and the edit retracts that | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 provided the same `a` | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | recompiles, runs | edited | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | reruns | `a` can be named again, as it could in step 1, whose compile is still memoized | `e` = 1002 |

Unit 4 is fixed without recompiling:
its compile from step 1 is still memoized.

The opposite of a take is a _revive_,
an overwrite of a `var` that an earlier unit gave away.
It gives the name back,
so later units can use the binding again.

```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

let b_moved = b

----------
scriptunit-fragment 3
----------

set b = 500

----------
scriptunit-fragment 4
----------

let c = b + 1
```

Unit 2 takes `b`, unit 3 revives it,
and unit 4 reads what unit 3 put there.
Without unit 3, unit 4 wouldn't compile.

**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 2 | `let b_moved = b` | takes `b` (1) | compiles, runs | new | `b_moved` = 125 |
| 3 | `set b = 500` | revives `b` (1) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |

Break unit 3.

```datalove
----------
scriptunit-edit 3
----------

set b = 500
let x = y
```

Retracting a revive is a restore like any other:
`b` goes back to what it held before unit 3,
which is nothing.
Unit 1 made `b`,
so it reruns,
and unit 2, between it and unit 3, takes `b` again.
Forward, it's a take added back.
Unit 4 names `b`,
which unit 2 gave away and nothing gives back,
so it breaks.

**Step 2: unit 3 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | reruns | rerun: unit 3 revived `b` when it last ran, and is retracted | `b` moved |
| 2 | `let b_moved = b` | takes `b` (1) | reruns | rerun: takes `b`, which unit 1 rebuilt | `b_moved` = 125 |
| 3 | `set b = 500; let x = y` | revives `b` (1) | recompile fails | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | recompile fails | `b` was given away by unit 2 | - |

Fix unit 3 again.
Applying a revive needs nothing,
as applying an overwrite doesn't,
and unit 4 can name `b` again.

**Step 3: unit 3 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 500 |
| 2 | `let b_moved = b` | takes `b` (1) | untouched | nothing it uses changed | `b_moved` = 125 |
| 3 | `set b = 500` | revives `b` (1) | recompiles, runs | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | `b` can be named again, as it could in step 1, whose compile is still memoized | `c` = 501 |




## A third algebra: breaks and fixes

<!--
_Therefore_ a revive joins the operations, and modifications reach later units' compiles.
-->

Like before, each modification retracts the old code, applies the new code, or both:

| modification | retract the old code | apply the new code |
|--------------|----------------------|--------------------|
| add          | -                    | yes                |
| fix          | -                    | yes                |
| break        | yes                  | -                  |
| delete       | yes                  | -                  |
| edit         | yes                  | yes                |

A revive is a fifth operation.
Backward it is an overwrite.
Forward it undoes a take.

| operation | retract | apply | later units that name the binding |
|-----------|---------|-------|-----------------------------------|
| read      | nothing | restore if a later unit took or wrote it | - |
| take      | restore | restore if a later unit took or wrote it | break; removing the take fixes them |
| overwrite | restore | nothing | - |
| update    | restore | restore if a later unit took or wrote it | - |
| revive    | restore | nothing | are fixed; removing the revive breaks them |

A restore reruns the unit that made the binding,
then, in order, the units between that one and the triggering unit
that took or wrote it.
Before revives, nothing between them could have taken it.

A break or a fix of a later unit is a modification in its own right.
The broken unit retracts what it did,
and the fixed unit applies what it does.
A modification can now cause other modifications.




## But what if a rerun unit has other bindings that must be restored?

<!--
_But_ every rerun reruns a whole unit, with all of its operations.
_Therefore_ every rerun is a modification, and the rules repeat until nothing changes.
-->

The tables work one binding at a time,
but a rerun runs a whole unit.
Whatever else it reads has to be as it found it,
and whatever else it takes or writes is done again.
Dependent bindings have to be restored,
but a restore can need a restore too.

```datalove
----------
scriptunit-fragment 1
----------

var b = 1

----------
scriptunit-fragment 2
----------

var z = 10

----------
scriptunit-fragment 3
----------

set b = 500
set z = z + 1

----------
scriptunit-fragment 4
----------

let c = b + 1

----------
scriptunit-fragment 5
----------

set b = 0
```

**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | compiles, runs | new | `b` = 0 |
| 2 | `var z = 10` | - | compiles, runs | new | `z` = 11 |
| 3 | `set b = 500; set z = z + 1` | overwrites `b` (1), updates `z` (2) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |
| 5 | `set b = 0` | overwrites `b` (1) | compiles, runs | new | - |

Edit unit 4.
It reads `b`, which unit 5 overwrote after it,
so `b` has to be restored.
The restore doesn't have to start at unit 1, which made `b`.
Any unit that set `b` without reading it,
by making, overwriting or reviving it,
rebuilds it from nothing,
so the restore starts at the nearest one, unit 3.
But unit 3 also updates `z`,
so rerunning it as it is would leave `z` at 12.
Unit 2 has to rerun first,
though unit 4 never names `z`.

**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | untouched | unit 3 overwrites `b` after it | `b` = 0 |
| 2 | `var z = 10` | - | reruns | rerun: unit 3 reruns and updates `z`, so `z` has to be as unit 3 first found it | `z` = 11 |
| 3 | `set b = 500; set z = z + 1` | overwrites `b` (1), updates `z` (2) | reruns | rerun: unit 4 reads `b`, which unit 5 overwrote after it, and unit 3 is the nearest to set it | - |
| 4 | `let c = b + 2` | reads `b` (1) | recompiles, runs | edited | `c` = 502 |
| 5 | `set b = 0` | overwrites `b` (1) | reruns | rerun: overwrites `b` after unit 4 | - |

A chain of moves walks back one link at a time.
Here `w` is made from `a2`, which was made from `a`.

```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

let a2 = a

----------
scriptunit-fragment 3
----------

var w = a2

----------
scriptunit-fragment 4
----------

let c = w + 1

----------
scriptunit-fragment 5
----------

set w = w * 2
```

**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let a2 = a` | takes `a` (1) | compiles, runs | new | `a2` moved |
| 3 | `var w = a2` | takes `a2` (2) | compiles, runs | new | `w` = 2000 |
| 4 | `let c = w + 1` | reads `w` (3) | compiles, runs | new | `c` = 1001 |
| 5 | `set w = w * 2` | updates `w` (3) | compiles, runs | new | - |

Edit unit 4, which reads `w` before unit 5 doubles it.

**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 2 reruns and takes `a`, which it took when it last ran | `a` moved |
| 2 | `let a2 = a` | takes `a` (1) | reruns | rerun: unit 3 reruns and takes `a2`, which it took when it last ran | `a2` moved |
| 3 | `var w = a2` | takes `a2` (2) | reruns | rerun: unit 4 reads `w`, which unit 5 updated after it | `w` = 2000 |
| 4 | `let c = w + 2` | reads `w` (3) | recompiles, runs | edited | `c` = 1002 |
| 5 | `set w = w * 2` | updates `w` (3) | reruns | rerun: updates `w` from unit 3, which reran | - |

Unit 3 reruns to rebuild `w`,
but it took `a2` the last time it ran,
so unit 2 reruns first,
and that took `a`, so unit 1 reruns before it.




## A complete recursive algebra of recompiles and reruns for moves and mutations

<!--
_Therefore_ we propose an algorithm and suggest it is complete.
-->

A session is a list of units, run in order.
A unit makes bindings of its own,
and performs _operations_ on the bindings of earlier units:

| operation | what it does to the binding |
|-----------|-----------------------------|
| read      | uses the value and leaves it |
| take      | moves the value out, leaving the binding empty |
| overwrite | writes a value without reading the old one |
| update    | reads the value and writes a new one |
| revive    | overwrites a binding an earlier unit took |

The operations are what the compiler sees in the code.
A write that might not happen,
because it's conditional,
or that writes only part of the binding,
counts as an update.

A _step_ starts with one _modification_ of one unit,
the _triggering unit_,
and ends when the session holds what a fresh run of the current code would.
A modification _retracts_ the unit's old code,
_applies_ its new code,
or both:

| modification | retract the old code | apply the new code |
|--------------|----------------------|--------------------|
| add          | -                    | yes                |
| fix          | -                    | yes                |
| break        | yes                  | -                  |
| delete       | yes                  | -                  |
| edit         | yes                  | yes                |
| rerun        | yes                  | yes, the same code |

A modification of a unit `T` calls for more of them.

- **Retract.**
  Restore every earlier binding the old code took, overwrote, updated or revived.
- **Apply.**
  Restore every earlier binding the new code reads, takes or updates
  that a unit after `T` took or wrote.
- **Overwrites.**
  Don't restore a binding the new code overwrites or revives.
  `T` won't look at what it held.
- **Restore.**
  To restore a binding,
  rerun the nearest unit before `T` that made, overwrote or revived it.
  That rebuilds it from nothing.
  A binding whose maker no longer runs has nothing to restore.
- **Undone work.**
  A unit that runs again rebuilds everything it makes and writes,
  which undoes whatever later units took or wrote of it,
  so those units rerun.
- **Changed values.**
  A later unit reruns if it reads, takes or updates a binding
  whose value may have changed:
  one the old or the new code made or wrote,
  before anything overwrites it again.
  A unit rerun to restore a binding finds what it found before,
  so it changes no values.
  A unit rerun for a changed value may change values in turn.
- **Changed names.**
  A later unit breaks when a name it uses is no longer available to it:
  its maker broke or was deleted,
  a take of it was added,
  or a revive of it was removed.
  It's fixed when the name is available again.

Every rerun, break and fix these rules call for
is a modification of its own unit,
and the rules apply to it in turn.
Collect them until nothing new is added,
then run the collected units in order.
Each unit joins at most once,
and there are finitely many units,
so the step ends.




## Where the static picture is approximate

<!--
_But_ the operations are what the compiler sees, not what actually ran.
_Therefore_ each case either fits the algebra conservatively or adds a rule.

From the old draft:
conditional and looping writes (conditional-write, loop-update),
partial writes (partial-write),
early returns, which block later readers like a break (return-early, return-early-before-take).
Optimizations the algebra doesn't need:
a rerun that produces the same value (same-value),
and what rerunning costs.
-->

todo




## Validating this design with a literate test suite

<!--

_But_ no proof, how do we know this is correct?
_Therefore_ all these examples and more are tests.

-->




## Comparing to FRP literature

<!--
_But_ we've entirely ignored the FRP literature,
_Therefore_ let's review some papers.

Prefer this last because I want to experiment on my own,
then do the upstream research and refine based on the literature.
-->




## A live example of the Datalove scripting engine

<!--
_Therefore_ we implemented this.
Here's a cool demo. Use original motivating example.
-->
