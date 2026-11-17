## How does editing a unit affect preceding units?

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
From here we'll use simple examples with integers.
Integer literals are `int`, a bigint, which is linear,
so `let f = b` moves `b` rather than copying it.

In this example the first four units each make one binding.
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
Unit 3 does not need to rerun
because unit 5 _overwrites_ `c` without looking at what it held.
Unit 1 doesn't either,
because _reading_ `a` left it as it was.

This table says what each unit does in a step.
_Uses from earlier units_ names the unit each binding came from in parentheses.
_Holds after_ is what a unit's bindings hold at the end of the step,
whichever unit last changed them.
Here unit 3 holds `c` = 30 because unit 5 overwrote it.

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
But rerunning unit 5 also accesses b, c, d, and e,
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
Call the edited or broken unit the _triggering unit_.
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

After a fresh run these leave `b` = 251.
Edit unit 2 to read `b` instead of updating it.

```datalove
----------
scriptunit-edit 2
----------

let c = b + 1
```

Which code do we need to look at to decide what to do?
The new unit 2 only reads `b`,
but the old unit 2 updated it.
Because the old updated `b` we have to rerun the unit that created `b`,
unit 1.
Because the unit that created `b` is rerun,
the units that read `b` must rerun too,
which includes the new unit 2 and unit 3.

To decide which units rerun we must consider both
the old and new code of the edited unit.

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

set b = b + 500
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

Entered, these leave `b` = 0, `z` = 11 and `c` = 502.
Edit unit 4.

```datalove
----------
scriptunit-edit 4
----------

let c = b + 2
```

It reads `b`, which unit 5 overwrote after it,
so `b` has to be restored:
unit 1 reruns,
then unit 3, which updates it.
But unit 3 also updates `z`,
so rerunning it as it is would leave `z` at 12.
Unit 2 has to rerun first,
though unit 4 never names `z`.

**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | reruns | rerun: unit 4 reads `b`, which unit 5 overwrote after it | `b` = 0 |
| 2 | `var z = 10` | - | reruns | rerun: unit 3 reruns and updates `z`, so `z` has to be as unit 3 first found it | `z` = 11 |
| 3 | `set b = b + 500; set z = z + 1` | updates `b` (1), updates `z` (2) | reruns | rerun: updates `b` after unit 1, which reran | - |
| 4 | `let c = b + 2` | reads `b` (1) | recompiles, runs | edited | `c` = 503 |
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
