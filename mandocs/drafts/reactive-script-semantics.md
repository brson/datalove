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

todo

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




---

## Access types and terminology

The unit that declares a binding is its _provider_.
After it,
later units may perform four kinds of _accesses_ on the binding:

- _read_ it, leaving it as it was,
- _update_ it, reading the old value and writing a new one,
- _overwrite_ it, writing a new value without reading the old one,
- _take_ it, moving the value out and leaving the binding empty,
- _revive_ it, moving a new value into a previously-emptied value.

Updates and overwrites are mutations.
Takes and revives are linear moves.
Datalove has immutable `let` bindings and mutable `var` bindings.
From here we'll use simple examples with integers,
which are linear-typed bigints,
so `let f = b` moves `b` rather than copying it.

We will work through reruns caused by edits to cells
that interact with updates and takes.
Reruns due to the remaining access types will be discussed in brief,
as will reruns due to compilation failures and fixes.
For the full rules see the
[Datalove literate reactive test suite](../../botdocs/script-reactivity-cases.md).


## Experiments

<!--
We want simple and understandable bite-sized examples.
We need to get across the idea from the abstract that we
use linear anti-dependence analysis to decide reruns.
We can punt deeper rules to later examples or
to a note to see the full literate test suite.
Focus on reruns and not recompiles.
We need to use the simplest math possible.

This is the main story:

- Forward rerun (updates)
- Backward rerun (updates)
- Chained backward rerun due to multiple binding accesses (updates)
- Edit that changes the access type (updates vs ?)

Note that this is only sound because linear types.

These are refinements, could be optional:

- Updates vs overwrites
- Takes and revives
- Compile break/fix
-->


## Forward reruns

```datalove
// unit 1
let a = 10
// unit 2
let b = 1
// unit 3
let c = a + b
```

Fresh-run result: `c == 11`.

Edit unit 1:

```datalove
// edit unit 1
let a = 100
// unit 2
let b = 1
// unit 3
let c = a + 1
```

Fresh-run result: `c == 101`.
Unit 1,3 rerun. Unit 2 unchanged.


## Backward reruns

```datalove
// unit 1
var a = 100
// unit 2
set a = a + 10
// unit 3
let b = 1
// unit 4
let c = a + b
```

Fresh-run result: `c == 111`.

Edit unit 2:

```datalove
// unit 1
var a = 100
// unit 2
set a = a + 1000
// unit 3
let b = 1
// unit 4
let c = a + b
```

Fresh-run result: `c == 1101`.
Unit 1,2,4 rerun. Unit 3 unchanged.
Without the backward rerun: `c == 2101`.
<!-- any time we see a digit other than 1 or 0 in the result we know its an error -->


## Chained backwards rerun

```datalove
// unit 1
let a = 100
// unit 2
var b = 10
// unit 3
set b = b + 1
// unit 4
let c = a + b
```

Fresh-run result: `c == 111`.

Edit unit 1:

```datalove
// edit unit 1
let a = 1000
// unit 2
var b = 10
// unit 3
set b = b + 1
// unit 4
let c = a + b
```

Fresh-run result: `c == 1011`.
Unit 1,2,3,4 rerun, 2 because 3 required it transitively.
Without the backward rerun: `b == 1012`.

---




## A complete set of rules for recompiles and reruns for moves and mutations

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
first back to the units before the triggering unit `T`,
then forward to the units after `T`, and after each unit that runs again.

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
  rerun its provider,
  then, in order, the units between it and `T` that took or wrote it.
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
