<!--

_Title_: Reactive compilation and evaluation based on analysis of bindings with linear types
_Summary_: With a simple linear type system we can correctly decide which
           code units to recompile and rerun after changes to code and input
           based on a simple analysis of how bindings are accessed.

-->



In reactive notebooks when a code cell is edited
subsequent dependent cells are rerun to recalculate the results.
The recalculated results should be equal to the results
produced by a fresh run of all code cells.

If we allow a cell to mutate a prior cell's bindings,
then after an edit we cannot simply rerun subsequent cells
to produce the same results as a fresh run.
Prior cells may be in a mutated state different from what a fresh run
would have produced at the edited cell.
Therefore we also must rerun prior cells
to restore their state before reruning subsequent cells.

Within a linear type system we can precisely identify all reads and writes of prior cells.
By analyzing five kinds of data accesses we can select a subset of prior and subsequent cells
to rerun that always produces the same result as a fresh run of all cells.


**Part 1. Background**

- [What is a reactive notebook?](#user-content-what-is-a-reactive-notebook)

**Part 2. Experiments**

- [How does editing a unit affect preceding units?](#user-content-how-does-editing-a-unit-affect-preceding-units)
- [But what about edits that change what a unit does?](#user-content-but-what-about-edits-that-change-what-a-unit-does)
- [But what if a rerun unit has other bindings that must be restored?](#user-content-but-what-if-a-rerun-unit-has-other-bindings-that-must-be-restored)

**Part 3. Results and Discussion**

- [A complete set of rules for recompiles and reruns for moves and mutations](#user-content-a-complete-set-of-rules-for-recompiles-and-reruns-for-moves-and-mutations)
- [Functions and modules are bindings too](#user-content-functions-and-modules-are-bindings-too)
- [Validating this design with a literate test suite](#user-content-validating-this-design-with-a-literate-test-suite)
- [Comparing to the literature](#user-content-comparing-to-the-literature)
- [A live example of the Datalove scripting engine](#user-content-a-live-example-of-the-datalove-scripting-engine)
- [Conclusions](#user-content-conclusions)



## What is a reactive notebook?

Scientific notebooks provide an interactive scripting environment
typically used for data exploration and scientific computing.
They are based on cells of code that may be executed independently,
or executed in sequence,
subsequent cells refering to the results of prior cells.

The well-known example of scientific notebooks is Jupyter.
In Jupyter one can edit and run arbitrary cells on demand.
Jupyter does not track dependencies between cells
and does not rerun subsequent cells after edits.
The results across the notebook can become unsynchronized
and disagree with each other:

- You edit cell 3, causing it to produce a different result,
  but cell 10 which depends on cell 3 still reflects the earlier result.
- You delete the cell that defined variable `foo` but the variable
  `foo` continues to exist in the environment at its previously-evaluated value.
- You run cells out of order and it produces a state that no fresh in-order
  run could possibly reproduce.

Reactive notebooks attempt to eliminate this confusion
by tracking data dependencies between cells and rerunning dependents after edits.
They attempt to maintain the key invariant:
_The results presented always equal what a fresh run of the whole notebook would produce_.

Common reactive notebooks accomplish this only under restricted scenarios
that cannot be validated within the type systems of their chosen programming languages.
One of those restrictions is that cells may not mutate the state of other cells.
Once this happens the dependency tracking fails and the invariant breaks.
This is the subject of
[When Do Reactive Notebooks Fail to React?](https://arxiv.org/abs/2511.21994), by Lu et. al. 2025.
They examined existing reactive notebooks to find cases where
replay behavior is unsound compared to a fresh run.

To illustrate the problem of reactivity in the face of mutations,
below is an example script one might hack on interactively.
It is written in Datalove,
an interactive scripting language with a linear type system.
It reads a month of bank transactions,
sorts each payee into a category,
totals the categories,
and lists the ones that went over budget.

The transactions live in a data file, `local/data/transactions.dlt`:

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

And the script reads them,
separated into cells,
which Datalove calls _script units_.

```datalove
// unit 1
require module sys/std/list
require module sys/std/map
require module sys/std/string

// unit 2
require data local/data/transactions: [
  {
    day: u32,
    payee: string,
    cents: int,
  }
]

// unit 3
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

// unit 4
let budget: %{string -> int} = %{
  "groceries" -> 25000,
  "dining" -> 15000,
  "car" -> 10000,
  "bills" -> 15000,
  "other" -> 5000
}

// unit 5
var spent: %{string -> int} = %{}

// unit 6
for t in ref transactions
  let c = category(ref t.payee)
  let so_far = map.get_or(ref spent, ref c, 0)
  call map.insert(mut spent, c, so_far + t.cents)
end for

// unit 7
var over: [string] = []
for c, amount in ref spent
  if amount .> budget[c]!
    call list.push(mut over, c@)
  end if
end for

// unit 8
let output = {
  spent = spent,
  over = over
}
```

Datalove today has no programmatic access to I/O at runtime.
The `require data` statement includes data at compile time,
bound to a variable named `transactions`,
and `output` just produces a binding observable in the runtime environment.
Let's just assume these are reasonable stand-ins for some future way to
impose I/O on a script.

> The `%{string -> int}` type is a map from `string` to `int`.
> The `!` sigil indicates sites of potential early-return errors,
> here a missing key in the `budget` map, `budget[c]!`.
> "Greater than" is spelled `.>` because Datalove
> [treats `<` and `>` as matched brackets in the lexer](../datafun.md#user-content-comparison-and-equality).
> The remaining Datalove syntax in this example is similar to other common languages.

Run in order, the script units produce `output`:

```datalove
{
  spent = %{
    "bills" -> 12753,
    "car" -> 4650,
    "dining" -> 13435,
    "groceries" -> 23819,
    "other" -> 3315
  },
  over = []
}
```

Nothing is over budget,
but Bi-Rite is a grocery store,
and its $33.15 is landing in "other".
So I teach unit 3 about it by editing the `category` function:

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

Which units rerun?
The naive answer is the unit that depend on the one I edited:
unit 6 calls `category` to build `spent`,
and unit 7 and 8 use what unit 6 computed.
Rerun those and the output is:

```datalove
{
  spent = %{
    "bills" -> 25506,
    "car" -> 9300,
    "dining" -> 26870,
    "groceries" -> 50953,
    "other" -> 3315
  },
  over = ["bills", "dining", "groceries"]
}
```

This result is wrong.
Everything doubled,
and I'm suddenly over budget on nearly everything.
Unit 6 doesn't rebuild `spent` from scratch, it adds to it.
The `spent` it found was the one it had already filled the last time it ran.
`spent` itself is created in an earlier unit that doesn't mention `category`.
A fresh run gives:

```datalove
{
  spent = %{
    "bills" -> 12753,
    "car" -> 4650,
    "dining" -> 13435,
    "groceries" -> 27134
  },
  over = ["groceries"]
}
```

To get the correct answer,
unit 5 has to rerun before unit 6,
receating `spent`.
The edit was to unit 3,
the unit it forced to rerun was unit 6,
and unit 6 reached backwards to mutate `spent` in unit 5.

_This is the problem at hand_.
When units are edited we must follow
the mutations to discover which other units must rerun.
The typical way to avoid this is to keep the declaration of `spent`
in the same unit as the code that fills it.
Reactive notebooks generally make that a rule.

Linear moves complicate the problem by invalidating bindings.
Unit 8 _moves_ `spent` and `over` into `output`,
leaving the two bindings empty.
Now raise the dining budget in unit 4.
Unit 7 reads `budget`, so it reruns
but `spent` doesn't exist, etc.

In both cases the units to rerun appear before the edit as well as after it.
Which ones depends on how each unit accessed the bindings of the units before it:
read them, moved them, overwrote them or updated them.
In Datalove the type checker knows these facts.
The rest of this post builds the rerun rules from those accesses.
It ends with an interactive replay of this script running in the Datalove engine,
making these same edits.




## How does editing a unit affect preceding units?

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

In the initial example the first four units each make one binding.
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

> This is a Datalove _worldfile_.
> Worldfiles are sections of text delimited by headers between `----------` lines.
> Each section is interpreted as determined by the header.
> There are many section types.
> Worldfiles can fully describe all input to the compiler and are the base format of the Datalove test suite.

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

The modification table says which halves a modification has.
The rules below say what each half calls for:
more modifications,
first back to the units before `T`,
then forward to the units after `T`, and after each unit that runs again.
The backward rules are the access table of
[the second section](#user-content-but-what-about-edits-that-change-what-a-unit-does),
except that a restore starts at the nearest unit that set the binding,
as [the third section](#user-content-but-what-if-a-rerun-unit-has-other-bindings-that-must-be-restored) found.

**Backward**, to put earlier bindings back as `T` first found them:

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

**Forward**, to the units after `T`, and after each unit that runs again:

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
This is the same key invariant maintained by reactive notebooks
and explored in the Lu, et. al paper.
After each step the test starts a new engine,
runs the units as they now stand from the top,
and compares the final state of each unit and their bindings.

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

This work has precedent in reactive notebooks,
dependence analysis in compilers and build systems,
and functional reactive programing.

I initially suspected that FRP was the nearest precedent,
but it is not.
FRP as about responding to changes to values at runtime.
This work is primarily about responding to changes to code at runtime
(though it will also apply to changes in values).
And Datalove's rerun rules concern themselves
with repairing the effects of moves and mutations.
FRP nodes don't mutate each other's values so those systems
don't have backward analysis like Datalove.

The more useful comparison today is to compiler optimization,
deterministic incremental build systems,
and live record-and-replay debuggers.
Datalove's rerun rules are a dependence analysis,
bread and butter compiler stuff.
Dependence analysis is most used for optimization.
Here it determines when to rerun surrounding units.

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




## A live example of the Datalove scripting engine

<!--
_Therefore_ we implemented this.
Here's a cool demo. Use original motivating example.
-->

This is the script from the intro,
each cell a script unit.
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




## Conclusions

These rules work because all data moves through bindings
that the compiler can follow.
That is a core constraint on the future design of the language.
Any language features that hide the flow of values through bindings
will complicate the reactive architecture.
Examples include first-class references,
interior mutability, and I/O.
