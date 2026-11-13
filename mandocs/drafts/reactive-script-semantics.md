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
Both recompilation and rerun are incremental,
performing only the work necessary to compute the changes.
It is _reactive_ to changes to both code and data.
Reruns are deterministic.
In the presence of arbitrary changes to a live coding session
Datalove produces the same results as if it had been run fresh.

I developed a simple set of recursive rules for deciding which code
must be recompiled and rerun when individual script units
are interactively modified or their inputs change.
These rules are based on analyzing variable bindings
and their subsequent usage within a linear type system.
I did this by progressively developing verifiable counter-examples
until I was satisfied of their correctness.
I did not prove correctness.



**Part 1. Background**

- [What is a reactive script?](#user-content-what-is-a-reactive-script)
- [Datalove's design for reactivity](#user-content-dataloves-design-for-reactivity)

**Part 2. Experiments**

- [If one unit moves or mutates a binding how does that affect preceding units on replay?](#user-content-if-one-unit-moves-or-mutates-a-binding-how-does-that-affect-preceding-units-on-replay)
- [But what about edits that change what a unit does?](#user-content-but-what-about-edits-that-change-what-a-unit-does)
- [But what if a rerun unit has other bindings that must be restored?](#user-content-but-what-if-a-rerun-unit-has-other-bindings-that-must-be-restored)
- [A complete set of rules for recompiles and reruns for moves and mutations](#user-content-a-complete-set-of-rules-for-recompiles-and-reruns-for-moves-and-mutations)

**Part 3. Results and Discussion**

- [Functions and modules are bindings too](#user-content-functions-and-modules-are-bindings-too)
- [Validating this design with a literate test suite](#user-content-validating-this-design-with-a-literate-test-suite)
- [Comparing to the literature](#user-content-comparing-to-the-literature)
- [A live example of the Datalove scripting engine](#user-content-a-live-example-of-the-datalove-scripting-engine)




## What is a reactive script?

<!--

_And_ this is a primer on reactive scripts.

Thesis: rerun dependent code.
Antithesis: hidden state problem?

Don't want to compare to existing systems.
It's not really my goal to solve a _new_ problem, just _my_ problem.

Motivating example.

-->

Datalove is an interactive scripting language.
The compiler and interpreter live together in a long-running engine.
Datalove scripts are reactive:
a script runs to completion,
then the engine waits for some external force to change the code or input data,
then it recompiles and reruns the script to completion again.
With careful design the engine avoids fully recompiling and rerunning the script.
As the code changes and as its input data changes,
the engine preserves its existing state and only reevaluates
what is necessary to produce a correct new result.

Here is an example script one might hack on interactively,
performing a simple data transformation.
It reads a month of bank transactions,
sorts each payee into a category,
totals the categories,
and lists the ones that went over budget.

The transactions live in a data file, `local/home/transactions.dlt`:

```datalove
[
  {day = 1, payee = "TRADER JOES #552", cents = 8412},
  {day = 2, payee = "SHELL OIL 5741", cents = 4650},
  {day = 3, payee = "BLUE BOTTLE COFFEE", cents = 675},
  {day = 5, payee = "SAFEWAY 1123", cents = 6230},
  {day = 7, payee = "TAQUERIA EL FAROLITO", cents = 2380},
  {day = 9, payee = "PG&E", cents = 11204},
  {day = 12, payee = "BLUE BOTTLE COFFEE", cents = 540},
  {day = 14, payee = "TRADER JOES #552", cents = 9177},
  {day = 18, payee = "NETFLIX.COM", cents = 1549},
  {day = 21, payee = "ZUNI CAFE", cents = 9840},
  {day = 26, payee = "BI-RITE MARKET", cents = 3315}
]
```

And the script reads them:

```datalove
require module sys/std/list
require module sys/std/map
require module sys/std/string

require data local/home/transactions: [
  {
    day: u32,
    payee: string,
    cents: int,
  }
]

fun category(ref payee: string): string
  if (
    string.starts_with(ref payee, ref "TRADER JOES")
    or string.starts_with(ref payee, ref "SAFEWAY")
  )
    ret "groceries"
  else if (
    string.contains(ref payee, ref "COFFEE")
    or string.contains(ref payee, ref "CAFE")
    or string.starts_with(ref payee, ref "TAQUERIA")
  )
    ret "dining"
  else if string.starts_with(ref payee, ref "SHELL")
    ret "car"
  else if payee == "PG&E" or payee == "NETFLIX.COM"
    ret "bills"
  end if
  ret "other"
end fun

let budget: %{string = int} = %{
  "groceries" = 25000,
  "dining" = 15000,
  "car" = 10000,
  "bills" = 15000,
  "other" = 5000
}

var spent: %{string = int} = %{}
for t in ref transactions
  let c = category(ref t.payee)
  let so_far = map.get_or(ref spent, ref c, 0)
  call map.insert(mut spent, c, so_far + t.cents)
end for

var over: [string] = []
for c, amount in ref spent
  if amount .> budget[c]!
    call list.push(mut over, c@)
  end if
end for

let output = {
  spent = spent,
  over = over
}
```

This processes input `transactions` and produces `output`.
Datalove today has no programmatic access to I/O at runtime.
The `require data` statement includes data at compile time,
bound to a variable named `transactions`,
and `output` just produces a binding observable in the runtime environment.
Let's just assume these are reasonable standins for some future way to
impose I/O on a script.

> The `%{string = int}` type is a map from `string` to `int`.
> The `!` sigil indicates sites of potential early-return errors,
> here a missing key in the `budget` map, `budget[c]!`.
> "Greater than" is spelled `.>` because Datalove
> [treats `<` and `>` as matched brackets in the lexer](../datafun.md#user-content-comparison-and-equality).
> The remaining Datalove syntax in this example is similar to other common languages.

On first run the script produces `output`:

```datalove
{
  spent = %{
    "bills" = 12753,
    "car" = 4650,
    "dining" = 13435,
    "groceries" = 23819,
    "other" = 3315
  },
  over = []
}
```

In this script I expect to trigger recompilation and/or reevaluation
when the input `transactions` changes,
or when the script code itself changes,
or when any of the modules it imports changes.

**Imagine we are developing this script experimentally in an interactive live coding environment**.

In the initial output nothing is overbudget,
but "Bi-Rite" is a grocery store,
and its transactions are currently being accumulated in the "other" category.
So I teach the `category` function about it:

```datalove
fun category(ref payee: string): string
  if (
    string.starts_with(ref payee, ref "TRADER JOES")
    or string.starts_with(ref payee, ref "SAFEWAY")
    or string.starts_with(ref payee, ref "BI-RITE")
  )
    ret "groceries"
  ...
```

When the script reevaluates,
"groceries" goes to 27134,
"other" disappears,
and groceries is now over budget:

```datalove
{
  spent = %{
    "bills" = 12753,
    "car" = 4650,
    "dining" = 13435,
    "groceries" = 27134
  },
  over = ["groceries"]
}
```

What actually needs to recompile here?
Definitely the `category` function I changed.
Definitely not the imported modules.
The rest of the script?
Since the type of `category` didn't change I expect the rest of the script doesn't need to recompile.

What actually needs to rerun here?
A less obvious question.
Probably anything that depends on the result of `category`.
Not everything does:

```datalove
let budget: %{string = int} = %{
  "groceries" = 25000,
  "dining" = 15000,
  "car" = 10000,
  "bills" = 15000,
  "other" = 5000
}

var spent: %{string = int} = %{}
for t in ref transactions
  let c = category(ref t.payee)
  let so_far = map.get_or(ref spent, ref c, 0)
  call map.insert(mut spent, c, so_far + t.cents)
end for
```

`budget` is calculated before `category` is ever mentioned.
Probably doesn't need to run again.
`spent` is built from a calculation that uses `category` so
any code that writes or reads it probably does.

Continuing, imagine the month goes on,
and my bank appends a coffee to the data file:

```datalove
  {day = 29, payee = "BLUE BOTTLE COFFEE", cents = 1890}
```

The totals for "dining" change putting me overbudget,
`over` becoming `["dining", "groceries"]`.
If I then change the code to increase the dining budget,
then `over` reevaluates back to `["groceries"]`.

What actually needs to recompile?
What actually needs to rerun?
Let's not belabor this.
You get the idea. Inputs change.
Reevaluate just what needs to reevaluate.

I'll develop the rules Datalove follows to make
these cases and more work correctly.
The rules are based on analyzing how bindings are used within a linear typesystem.
I'll end with a live demonstration of how this particular script actually reacts to modifications.




## Datalove's design for reactivity

<!--

_And_ these are the properties that will help us solve the problem.

_Therefore_ about the hidden state problem.

_But_ what about I/O?
_Therefore_ here is how we model it.

_But_ what about FRP?
_Therefore_ here is how we compare.

Refine the motivating example to a datalove worldfile,
introducing the concept.
We'll show a version of it live at the end.

-->

There are two pillars to Datalove's design that make the goal
of interactive deterministic reexecution feasible:

_Language restrictions_.
Datalove has a linear type system and its functions are side-effect free.
At the moment there is not even any way to do I/O,
which would complicate determinism greatly.
The compiler tracks data dependencies statically and precisely,
and knows exactly which code can mutate which data.

_Fast incremental recompilation_.
The compiler's analyses are memoized,
and modules are compiled in parallel.
Editing code recompiles only what depends on it.
The compiler design is aided by and informs the restrictions of the language design.

A language that doesn't do any I/O is limited in utility.
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
A single REPL input is a script unit.
Interactive Datalove is intended for the REPL and scientific notebook use cases.

In most respects script units are like a function,
except that they accept no arguments and are not required to return a result.
At run time they have the same layout as a function's stack frame.
Script units can be chained together in a list dynamically at run time.
New script units inherit the type checking and execution environment of previous script units.

In this example of a self-contained Datalove _worldfile_ we have three sequential script units.

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
let c = a_moved + b_moved
```

> Worldfiles are sections of text delimited by headers between `----------` lines.
> Each section is interpreted as determined by the header.
> There are many section types.
> Worldfiles can fully describe all input to the compiler and are the base format of the Datalove test suite.

In this example the integers are type `int`, bigints.
They are linear types allocated on the heap,
so the assignment `let a_moved = a` moves `a` into `a_moved`, leaving `a` empty.

In this example all three units compile and execute successfully.
The third binds `c` to 1100.
Each unit compiles and runs before the next.
A later unit compiles against the bindings earlier units provide,
and runs only if the units it reads from ran to their end.




## If one unit moves or mutates a binding how does that affect preceding units on replay?

<!--
_But_ need a simple clear example that shows the core problem.
_Therefore_ what we decide to recompile and rerun, as a first set of rules.

Demonstrate read/take/overwrite/update
-->

Whatever we rerun,
the session must end up as a fresh run of the current code
from the top would leave it.
That's the rule every step below is held to.

The unit that declares a binding is its _provider_.
After it,
later units may perform four kinds of _accesses_ on the binding:

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
Entering the five units is step 1:
each compiles and runs once, in order,
leaving `a` = 1, `b` taken, `c` = 30, `d` = 8, `e` = 11 and `f` = 2.

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

A table says what each unit does in a step.
_Uses from earlier units_ names the unit each binding came from in parentheses.
_Holds after_ is what a unit's bindings hold at the end of the step,
whichever unit last changed them:
unit 3 holds `c` = 30 because unit 5 overwrote it.

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1` | - | untouched | unit 5 only reads `a` | `a` = 1 |
| 2 | `let b = 2` | - | reruns | rerun: unit 5 takes `b`, which it took when it last ran | `b` taken |
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
But rerunning unit 5 puts it in the same spot as editing it did,
so units 2 and 4 rerun before it,
just as they did in step 2.
The edited unit forced a rerun of a later unit,
and that unit in turn forced reruns of earlier ones.

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 2` | - | recompiles, runs | edited | `a` = 2 |
| 2 | `let b = 2` | - | reruns | rerun: unit 5 reruns and takes `b`, which it took when it last ran | `b` taken |
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

These steps give a first set of rules.
Call the unit an edit or a break modifies the _triggering unit_.
What happens to the units before it
depends on what its code did to their bindings the last time it ran:

| access    | break   | edit    |
|-----------|---------|---------|
| read      | nothing | nothing |
| take      | restore | restore |
| overwrite | restore | nothing |
| update    | restore | restore |

A _restore_ puts a binding back as the triggering unit first found it.
It reruns the binding's provider,
then, in order, the units between that one and the triggering unit
that wrote it.
Overwrite is the only access where a break and an edit differ:
an edited unit overwrites the binding again,
and a broken one doesn't.

After the triggering unit,
a unit that reads, takes or updates a binding
the triggering unit provided or wrote
sees a new value,
so it reruns.
A unit that overwrites the binding doesn't care what it held,
so the change stops there.

A rerun is like an edit that doesn't change the code,
so each unit rerun triggers the same analysis,
until no more units need to rerun.




## But what about edits that change what a unit does?

<!--
_But_ the first rules assume an edit keeps the unit's accesses.
_Therefore_ an edit retracts the old code and applies the new,
and changes to accesses can break and fix later units.
-->

The first rules look up what the triggering unit does,
and assume an edit doesn't change that.
It can,
both to the values of earlier bindings
and to which of them later units can name.

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

Entered, these leave `b` = 251.
Edit unit 2 to read `b` instead of updating it.

```datalove
----------
scriptunit-edit 2
----------

let c = b + 1
```

The first rules see that unit 2 is a read of `b`,
which needs nothing,
so unit 2 runs alone, finds 251,
and computes an incorrect 252.
A fresh run gives 126.
Two units wrote `b` since unit 1 provided it:
unit 2 itself, with code that no longer exists,
and unit 3, which runs after it.

**Step 2: unit 2 is edited to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | reruns | rerun: unit 2 updated `b` when it last ran, and the edit retracts that | `b` = 126 |
| 2 | `let c = b + 1` | reads `b` (1) | recompiles, runs | edited | `c` = 126 |
| 3 | `set b = b + 1` | updates `b` (1) | reruns | rerun: updates `b`, which unit 1 rebuilt | - |

So an edit is two modifications.
It _retracts_ what the old code did,
and _applies_ what the new code does.
And applying has to look past the triggering unit,
at what later units did to the binding since.

A take changes more than values:
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

let c = a + 10

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
Edit unit 3 to take `a` instead of reading it.

```datalove
----------
scriptunit-edit 3
----------

let d = a
```

There is nothing to retract or apply before unit 3:
the old code only read `a`,
and nothing after unit 3 took or wrote it.
But unit 4 reads `a`,
and `a` is now given away before unit 4 gets to it.
Unit 4 hasn't been edited,
and nothing it uses was recompiled,
but it no longer compiles:

```
error[O013]: `a` was given away by an earlier unit
```

**Step 2: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | `a` is as unit 1 made it | `a` taken |
| 2 | `let c = a + 10` | reads `a` (1) | untouched | reads `a` before the take | `c` = 1010 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a + 2` | reads `a` (1) | recompile fails | `a` was given away by unit 3 | - |

Editing unit 3 back to a read retracts the take,
so unit 1 reruns to restore `a`,
and unit 4 is fixed.
It reruns without recompiling:
its compile from step 1 is still memoized.

The opposite of a take is a _revive_,
an overwrite of a `var` that an earlier unit gave away.
It gives the name back.

```datalove
var b = 125        // unit 1
let b_moved = b    // unit 2 takes `b`
set b = 500        // unit 3 revives it
let c = b + 1      // unit 4 reads what unit 3 put there
```

Break unit 3 and its revive is retracted:
`b` goes back to what it held before unit 3,
so units 1 and 2 rerun,
and unit 2 takes `b` again.
Unit 4 now names a binding that is given away,
so it breaks too.
Fix unit 3 and unit 4 is fixed with it.

So a modification retracts the old code, applies the new code, or both.
Adding or fixing a unit only applies,
breaking or deleting it only retracts,
and editing it does both.
A revive is a fifth access:
backward it is an overwrite,
and forward it undoes a take.

| access    | retract | apply | later units that name the binding |
|-----------|---------|-------|-----------------------------------|
| read      | nothing | restore if a later unit took or wrote it | - |
| take      | restore | restore if a later unit took or wrote it | break; removing the take fixes them |
| overwrite | restore | nothing | - |
| update    | restore | restore if a later unit took or wrote it | - |
| revive    | restore | nothing | are fixed; removing the revive breaks them |

Retract restores whatever the old code changed.
Apply restores whatever the new code is going to look at,
if something after it has changed it since.
The first rules' break column is the retract column,
and their edit column is what retract and apply give
when nothing after the triggering unit touched the binding.

A restore reruns the binding's provider,
then, in order, the units between that one and the triggering unit
that took or wrote it,
and then the units after the triggering unit that took or wrote it,
since the restore undid their work.
After the triggering unit,
a unit reruns if it reads, takes or updates a binding
that either the old or the new code provided or wrote.

A break or a fix of a later unit is a modification in its own right:
the broken unit retracts what it did,
and the fixed unit applies what it does.




## But what if a rerun unit has other bindings that must be restored?

<!--
_But_ every rerun reruns a whole unit, with all of its accesses.
_Therefore_ every rerun is a modification, and the rules repeat until nothing changes.
-->

The tables work one binding at a time,
but a rerun runs a whole unit.
Whatever else it reads has to be as it found it,
and whatever else it takes or writes is done again.
So a restore can need a restore too.

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

Entered, these leave `b` = 0, `z` = 11 and `c` = 501.
Edit unit 4.
It reads `b`, which unit 5 overwrote after it,
so `b` has to be restored.
The restore doesn't have to start at unit 1, which provides `b`.
Any unit that set `b` without reading it,
by providing, overwriting or reviving it,
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

Moves chain the same way, one link at a time.
If `var w = a2` has to rerun to rebuild `w`,
it takes `a2` again,
so the unit that made `a2` with `let a2 = a` reruns first,
and since that takes `a`,
the unit that made `a` reruns before it.

So every rerun is a modification of its own,
and the rules apply to it in turn,
until nothing more needs to rerun.




## A complete set of rules for recompiles and reruns for moves and mutations

<!--
_Therefore_ we propose an algorithm and suggest it is complete.
-->

A session is a list of units, run in order.
A unit provides bindings of its own,
and performs _accesses_ on the bindings of earlier units:

| access    | what it does to the binding |
|-----------|-----------------------------|
| read      | uses the value and leaves it |
| take      | moves the value out, leaving the binding empty |
| overwrite | writes a value without reading the old one |
| update    | reads the value and writes a new one |
| revive    | overwrites a binding an earlier unit took |

The accesses are what the compiler sees in the code.
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
| data change  | yes                  | yes, the same code over new data |
| reorder      | yes, in the old place | yes, in the new place |

A change to the data a unit requires is a rerun of that unit,
except that it now provides different values.
Its code is the same,
so it does to earlier bindings what it did before.
If the new data doesn't have the type the unit requires it at,
the unit no longer compiles,
and the change is a break.

A reorder is a delete and an add of the same unit,
made in one step,
so the session as it would stand between the two never runs.

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
  rerun the nearest unit before `T` that provided, overwrote or revived it.
  That rebuilds it from nothing.
  A binding whose provider no longer runs has nothing to restore.
- **Undone work.**
  A unit that runs again rebuilds everything it provides and writes,
  which undoes whatever later units took or wrote of it,
  so those units rerun.
- **Changed values.**
  A later unit reruns if it reads, takes or updates a binding
  whose value may have changed:
  one the old or the new code provided or wrote,
  before anything overwrites it again.
  A unit rerun to restore a binding finds what it found before,
  so it changes no values.
  A unit rerun for a changed value or changed data may change values in turn.
  An overwrite stops a change only while it writes what it wrote before.
  If the provider now gives the binding another type,
  or makes it a `let`,
  the overwrite compiles to something else,
  and is an edit or a break of its own.
- **Changed names.**
  A later unit breaks when a name it uses is no longer available to it:
  its provider broke or was deleted,
  a take of it was added,
  or a revive of it was removed.
  It's fixed when the name is available again.

Every rerun, break and fix these rules call for
is a modification of its own unit,
and the rules apply to it in turn.
What new code does to earlier bindings
is only known once it is compiled,
so each unit is compiled as it is collected.
Collect them until nothing new is added,
then run the collected units in order.




## Functions and modules are bindings too

All of the examples used to develop these rules so far
were based on variable bindings.
The motivating example from the intro
included modules and functions as well.

```datalove
require module sys/std/list
require module sys/std/map
require module sys/std/string

...

fun category(ref payee: string): string
  if (
    string.starts_with(ref payee, ref "TRADER JOES")
    or string.starts_with(ref payee, ref "SAFEWAY")
  )
    ret "groceries"
  else if (
    string.contains(ref payee, ref "COFFEE")
    or string.contains(ref payee, ref "CAFE")
    or string.starts_with(ref payee, ref "TAQUERIA")
  )
    ret "dining"
  else if string.starts_with(ref payee, ref "SHELL")
    ret "car"
  else if payee == "PG&E" or payee == "NETFLIX.COM"
    ret "bills"
  end if
  ret "other"
end fun

...
```

These are also bindings and follow the same analysis,
simplified since their values can't be mutated or moved.
On each step the engine just needs to track where these
are used and apply the same rules as `let` and `var`.

This does suggest a constraint for future design directions though.
It needs to continue to be easy to track the flow of data through bindings.
Any language features that hide the flow of values through bindings
will complicate the reactive architecture.
First-class references are an example.
If references to values could outlive the current statement
then the analysis would have to track their lifetimes as well.
Interior mutability is another.
I/O presents more problems.

These kinds of concerns are why one of [Datalove's principles](../principles.md)
is to establish broad architectural capabilities early,
develop the language later.
Discover the big constraints now,
and add comprehensive test suites to preserve them,
so I don't accidentally add
incompatible semantics to the surface language later.




## Validating this design with a literate test suite

<!--

_But_ no proof, how do we know this is correct?
_Therefore_ all these examples and more are tests.

-->

The examples in this document are test cases.
Their code is test case input.
The corresponding tables are the expected outcome.
This document is a live test suite.

As I wrote this experiment iteratively (with my human brain and words)
I also had LLMs validate it as a test suite while they implemented
the rules in the engine.
All three activities iterating together.
Blog-driven and test-driven development.

For each case the example script units
and the expected step tables live outside the document in the test suite,
in a single worldfile (most Datalove test cases are worldfiles).
When the test suite runs it validates the results in the suite,
then validates them against this document,
then inserts the new results back into the document,
keeping it fresh.

Here's a simple one, `edit-changes-access.world`:

```
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

----------
expect all three units are entered
----------

| unit | uses | this step | because | holds after |
|------|------|-----------|---------|-------------|
| 1 | - | compiles, runs | new | `b` = 251 |
| 2 | updates `b` (1) | compiles, runs | new | - |
| 3 | updates `b` (1) | compiles, runs | new | - |

----------
scriptunit-edit 2
----------

let c = b + 1

----------
expect unit 2 is edited to read `b`
----------

| unit | uses | this step | because | holds after |
|------|------|-----------|---------|-------------|
| 1 | - | reruns | rerun: unit 2 updated `b` when it last ran, and the edit retracts that | `b` = 126 |
| 2 | reads `b` (1) | recompiles, runs | edited | `c` = 126 |
| 3 | updates `b` (1) | reruns | rerun: updates `b` from unit 1, which reran | - |
```

The `expect` worldfile sections are inputs to the markdown tables in the document.
The `scriptunit-edit` sections indicate edits to a script unit.
They are representations of the same commands as executed by a human user of the scripting environment.
Their code is merged into the tables as the "code" column when rendered to the document.

The "uses", "this step" and "holds after" columns of the table are validated.
The "because" column is documentation for the reader.

The tables are hand-written and could be wrong though.
Every step is also held to an additional rule:
the session must end up as a fresh run of the current code would leave it.
After each step the test starts a new engine,
runs the units as they now stand from the top,
and compares how each unit ended and what it holds.
That check doesn't trust the tables at all.

I don't want to say more about the specifics of the test suite.
What I do want to say is that I love thorough testing.
Designing an entire test suite to fit a single subsystem and purpose is powerful.
They can express ideas that stock unit tests cannot.
But they take outsized effort to write and maintain.
In the past I have made elaborate and bespoke test suites when paid to do it,
when I deemed the subsystem important enough to warrant maintaining it,
and when I had the motivation to spend weeks writing it.
LLMs can write and maintain this easily.
LLMs also need thoughtful guiderails to help them maintain software in the direction I want.
This test suite contains both the specification and its validation in a form that is easy for LLMs to maintain.
This test suite is also in a form that is easy and useful for humans to read.

The content of this document presents a consice narrative for readers.
It includes the minimum description and cases to present the ideas.
The repo also contains a [complete literate test suite](../../botdocs/script-reactivity-cases.md).
They cover these cases and more.
The text around them is less scrutinized, mostly written by LLMs.




## Comparing to the literature

<!--
_But_ we've entirely ignored the FRP literature,
_Therefore_ let's review some papers.

Prefer this last because I want to experiment on my own,
then do the upstream research and refine based on the literature.
-->

This work has precedent.
The obvious is from functional reactive programming (FRP).
I have only little experience and knowledge of FRP
and did not want to change that _prior_ to doing this work.
I prefer to discover for myself.

I'm mostly familiar with FRP through its popularity
in the UI domain, React and similar.
My use case here is different.
I want the interactive coding experience to be reactive,
so when I make changes to the code or its input I see the results immediately,
and the results are deterministic.

This turns out to be significantly different from FRP
in that FRP is entirely concerned with runtime reactivity to changes in data.
I do care about that as well, and I think that for my purposes
changes in data can be treated as a subset of changes to code
(imagine that a variable binding changes value at compile time vs. runtime -
both conceptually have the same effect on the interpreter).

Datalove's reactivity is coarse-grained, script units,
where FRP research typically seeks to produce granular recomputation.
Differential dataflow for example is deeply focused on not reprocessing entire lists
during streaming workloads.
I'm not sure the extent to which Datalove will care about that,
though I think the current architecture is a reasonable foundation for future work.

Datalove's rerun rules concern themselves primarily
with repairing the effects of moves and mutations,
tractable because of its linear typesystem.
FRP nodes don't mutate each other's values so those systems
don't have backward analysis like Datalove.

So while FRP research does apply to Datalove,
and will moreso in the future when Datalove gets real I/O,
the more useful comparison today is to compiler optimization,
deterministic incremental build systems,
and live record-and-replay debuggers.
Elm has something like this.
Datalove's rerun rules are a dependence analysis,
bread and butter compiler stuff.
Dependence analysis is mostly used for optimization.
Here we are using it to determine when to rerun surrounding units.

Some of the vocabulary in my rules has established terminology:

| this post | standard name |
|---|---|
| a fresh run of the current code | from-scratch consistency |
| read | use |
| take | consume (linear types) |
| overwrite | must-def (strong update) |
| update | may-def, or read-modify-write |
| revive | reinitialization after a move |
| retract | retraction, as in Prolog and Datalog's assert and retract |
| later units rerun on a changed value | flow (read-after-write) dependence |
| restore because a later unit took or wrote what an earlier one reads | anti (write-after-read) dependence |
| the nearest unit that set the binding without reading it | nearest killing definition |
| restore by rerunning instead of keeping the old value | rematerialization |
| repeat until nothing more needs to rerun | worklist fixed point |

There is one paper that applies directly to this work,
[When Do Reactive Notebooks Fail to React?](https://arxiv.org/abs/2511.21994), by Lu et. al. 2025.
They examine existing reactive notebooks to find cases where
replay behavior is unsound compared to a fresh run.
They developed a language and testing methodology.
I adopted their technique of comparing every step to a fresh run.
The rules developed here appear to pass all their cases and more.
They did identify cases that I had not made explicit,
particularly related to mutating functions and reordering script units.
I added them to my test suite.




## A live example of the Datalove scripting engine

<!--
_Therefore_ we implemented this.
Here's a cool demo. Use original motivating example.
-->

This is the same script from the intro,
divided by hand into script units.
This is a replay captured from the actual Datalove scripting engine.
It is presented in a similar style to the actual Datalove interactive CLI
(the `datalove repl` command).

<!--
The replay is recorded from
crates/datalove-doc-fixtures/replays/reactive-transactions.world;
run `BLESS=1 cargo test -p datalove-doc-fixtures` to record it again.
-->

<div class="datalove-replay" data-replay="reactive-transactions"></div>
<script src="../assets/datalove-replay.js"></script>
<script src="../assets/reactive-transactions.replay.js"></script>
