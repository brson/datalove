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
---
script-unit
---

let a = 1000

---
script-unit
---

let b = 100

---
script-unit
---

let a_moved = a
let b_moved = b
let c = a_moved /! b_moved

debuglog c
```

> Worldfiles are chunks of text delimited by headers bound between `---` lines.
> Each chunk is interpreted as determined by the header.

In this example the integers are type `int`, bigints.
They are linear types allocated on the heap,
so the assignment `let a_moved = a` moves `a` into `a_moved`, leaving `a` empty.
The `/!` operator is checked division, and returns early on divide by 0,
indicating failure.
Scripts and script units are allowed to return the type `!()`,
"result of unit".

In this example all three units compile and execute successfully.
The third logs the value "10".
Previous units both compile and execute prior to subsequent units,
as the results of execution may affect which bindings are subsequently available.




## The complications involved in (re)checking and (re)running script units


### How does one unit failing to compile affect subsequent units?

Imagine our first script unit doesn't compile.
No recompilation yet.
This is as originally written.

```datalove
---
script-unit
---

let a = 1000
let x = y // `y` doesn't exist

---
script-unit
---

let b = 100

---
script-unit
---

let a_moved = a
let b_moved = b
let c = a_moved /! b_moved

debuglog c
```

The first unit will fail in name resolution with an error.
The second will compile successfully because it references no bindings in the first.
_The second unit will then run_,
creating the binding `b` and making it available to subsequent units.
The third will fail to compile because the binding `a` does not exist,
as the unit that creates it failed to compile.

So in the first compile/run pass where the first unit failed
(the columns say what happened to each unit in this pass):

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | fail       | no    |
| 2    | success    | yes   |
| 3    | fail       | no    |

Imagine we fix the first unit's compilation error.
That allows the first and third units to both compile and run.
But we need to completely skip the second unit because it already did both
and has no dependencies on the first.

In the second pass after editing:

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | success    | yes   |
| 2    | no         | no    |
| 3    | success    | yes   |

After this second pass, because unit 3 ran,
it moved `a` and `b` out of the frames of units 1 and 2, into its own.

Now let's revert the previous edit to unit 1, making it fail to compile again.
Unit 3 depends on unit 1, so it recompiles and fails, and doesn't reexecute.
Unit 2 doesn't depend on unit 1,
so naively it neither recompiles nor reexecutes.

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | fail       | no    |
| 2    | no         | no    |
| 3    | fail       | no    |

But that's wrong.




### How is re-execution affected when subsequent units consume bindings from the re-executed unit?

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
---
script-unit
---

let a = 1000
let x = y // broken again

---
script-unit
---

// Naively we don't want to rerun this because it doesn't depend on
// unit 1...
let b = 100

---
script-unit
---

let a_moved = a
// ...but this moved `b` the last time it ran,
// and now it doesn't compile, so it can't have.
let b_moved = b
let c = a_moved /! b_moved

debuglog c
```

We have to recognize that unit 3 moved a binding out of unit 2,
and that anything that undoes or redoes unit 3 has to build the binding again,
by rerunning unit 2.

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | fail       | no    |
| 2    | no         | yes   |
| 3    | fail       | no    |

The rule is in terms of what unit 3 did _the last time it ran_,
not what it does now.
That is what the running session went through,
and the session has to end up the same as if it had been run fresh from the top.

Then when we make unit 1 compile again,
unit 2 has nothing to do:
`b` is already as unit 2 made it,
and unit 3 moves it again.

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | success    | yes   |
| 2    | no         | no    |
| 3    | success    | yes   |




### How is re-execution affected when subsequent units mutate the re-executed unit?

Here's a new variation on our running example
that mutates instead of moves.

```datalove
---
script-unit
---

let a = 1000

---
script-unit
---

var b = 125

---
script-unit
---

set b = b * 2
let c = a /! b

debuglog c
```

The third unit doubles `b` in place.
Doubling is not idempotent:
running it twice is not the same as running it once.
On first run it mutates `b` from 125 to 250 and logs the value "4".

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | success    | yes   |
| 2    | success    | yes   |
| 3    | success    | yes   |

At this point unit 2's frame contains the mutated `b`, 250.
Imagine in the second run we cause the first unit to fail as previously.

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | fail       | no    |
| 2    | no         | no    |
| 3    | fail       | no    |

Unit 3 failed to compile because it depends on unit 1.
Naively unit 2 is untouched because it doesn't.
But like with the move, unit 3 wrote to `b` the last time it ran,
and now it can't have,
so we need to rerun unit 2 to put `b` back to 125.

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | fail       | no    |
| 2    | no         | yes   |
| 3    | fail       | no    |

Now if we fix unit 1,
unit 3 runs against `b` as unit 2 made it,
doubling 125 to 250 and logging "4" again.
Had we left unit 2 alone,
unit 3 would have doubled the 250 it left behind last time,
to 500,
and logged "2".

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | success    | yes   |
| 2    | no         | no    |
| 3    | success    | yes   |

The analysis is essentially the same,
but instead of a dependency on move operations
we have a dependency on writes:
`set`, and passing a binding to a `mut` or `out` parameter.




### How does one unit returning early affect subsequent units?


