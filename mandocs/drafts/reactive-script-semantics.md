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
One of my [principles](#todo) in Datalove is to start with a severely
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

let a = 100

---
script-unit
---

let b = a
let c = b /! 10

debuglog c

```

> Worlfiles are chunks of text delimited by headers bound between `---` lines.
> Each chunk is interpreted as determined by the header.

In this example the integers are type `int`, bigints.
They are linear types allocated on the heap,
so the assignment `let b = a` moves `a` into `b`, leaving `a` empty.
The `/!` operator is checked division, and returns early on divide by 0,
indicating failure.




## The complications involved in (re)checking and (re)running script units


### How does one unit failing to compiler affect subsequent units?

### How does one unit returning early affect subsequent units?

### How is re-execution affected when subsequent units mutate the re-executed unit?



