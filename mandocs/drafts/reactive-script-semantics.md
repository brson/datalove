<!--

Title: An initial semantics for reactive interactive script units in Datalove
Summary:

Audience: practicioners

Problem:
Datalove incrementally recompiles and reexecutes only dependent script units
when dependant code and data changes. What does this mean in the cases?

Thesis:
Antithesis:
Synthesis:

-->

Datalove's language and compiler model have been designed
primarily as an interactive scripting environment.
It has been designed such that editing existing
code quickly recompiles previously compiled code.
It has been designed such that editing existing
code _should be able to _quickly re-executes previously compiled code,
producing the same result.
Datalove's scripting enviroment is intended to be
deterministically reactive to its code and data input.

Until now though the CLI REPL has only barely functioned,
and the actual semantics of what happens
as code is recompiled and reexecuted have been left to future work.
Ok, future is now.
Let's figure it out.


## Datalove's design for reactivity

There are two pillars to Datalove's design that help make the goal
of interactive determimnistic reexecution feasible:

_Language restrictions_.
Datalove has a linear type system and its functions are side-effect free.
At the moment there is not even any way to do I/O,
which complicates determinism greatly.
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

> Worlfiles are chunks of text delimited by headers bound between `---` lines.
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


### How does one unit failing to compiler affect subsequent units?

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

So in the first compile/run pass where the first unit failed:

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

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | fail       | no    |
| 2    | no         | no    |
| 3    | fail       | no    |

Because unit 2 doesn't depend on unit 1 it neither compiles nor reexucutes.
Unit 3 recompiles and fails, so doesn't reexecute.




### How is re-execution affected when subsequent units consume bindings from the re-executed unit?

Continuing the previous example, lets make unit 1 compile again!
This time we have a complication:
unit 3 wants to reexecute,
but it already executed once in a previous revision,
moving the `b` binding out of unit 2.

```datalove
---
script-unit
---

let a = 1000

---
script-unit
---

// Naively we don't want to rerun this because it doesn't depend on
// unit 1, which now compiles...
let b = 100

---
script-unit
---

let a_moved = a
// ...but `b` no longer exists!
let b_moved = b
let c = a_moved /! b_moved

debuglog c
```

Static ownership analysis says there is a value in `b`,
but if we just rerun unit 2 there won't be.
We have to recognize that unit 3 moves bindings out of unit 2,
and also reexecute unit 2.

| unit | compiled   | ran   |
|------|------------|-------|
| 1    | success    | yes   |
| 2    | no         | yes   |
| 3    | success    | yes   |




### How is re-execution affected when subsequent units mutate the re-executed unit?



### How does one unit returning early affect subsequent units?


