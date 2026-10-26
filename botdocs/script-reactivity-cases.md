# Script reactivity, case by case

What an edit to a REPL session should recompile and rerun, worked through as
cases, with the rules they add up to. Every table here is rendered from a
fixture in `crates/datalove-script-engine/tests/fixtures/reactive/` and
checked against the engine by
`crates/datalove-script-engine/tests/reactive_tests.rs`, so this is a literate
test suite: what it says about the engine is true or the build fails.
Started October 2026 from `mandocs/drafts/reactive-script-semantics.md`, which
is the narrative version. How the engine does it is
[script-reactivity-architecture.md](script-reactivity-architecture.md).

## Contents

- [Reading the tables](#user-content-reading-the-tables)
- [The algebra](#user-content-the-algebra)
- [A unit stops compiling, and starts again](#user-content-a-unit-stops-compiling-and-starts-again)
- [Overwrites](#user-content-overwrites)
- [Reading before or after another unit changes a binding](#user-content-reading-before-or-after-another-unit-changes-a-binding)
- [Edits that add or remove a take](#user-content-edits-that-add-or-remove-a-take)
- [Edits that add or remove a write](#user-content-edits-that-add-or-remove-a-write)
- [Revivals](#user-content-revivals)
- [Reruns that need reruns](#user-content-reruns-that-need-reruns)
- [Values through intermediate bindings](#user-content-values-through-intermediate-bindings)
- [Conditional writes, loops and parts of bindings](#user-content-conditional-writes-loops-and-parts-of-bindings)
- [Splices](#user-content-splices)
- [Returning early](#user-content-returning-early)
- [Where the engine does more than the rules](#user-content-where-the-engine-does-more-than-the-rules)

## Reading the tables

A case is a session of script units and then steps, each step one
modification. After each step a table says, for every unit:

- **code**: its source, joined onto one line with `; `.
- **uses from earlier units**: what it does to each binding an earlier unit
  provides, and which unit is its provider: `reads`, `takes`, `overwrites`,
  `updates` or `revives` (see [the algebra](#user-content-the-algebra)).
- **this step**: what happened to it.
- **because**: why. The one column that is not checked. A `rerun:` or
  `fixup:` prefix marks a unit run, or a binding put back, to restore a
  binding for another unit.
- **holds after**: its own bindings once the step is over, `moved` for one
  given away, `-` for none.

What happens to a unit in a step is one of:

- `untouched`.
- `reruns`: ran again without compiling again.
- `compiles, runs` or `recompiles, runs`.
- `recompile fails`, or `compile fails` for a new unit.
- `compiles, returns early`: ran and left its top level through `!` or `?`.
- `compiles, is blocked`: not run, because it reads from a unit that did not
  finish.
- `deleted`.
- `` `b` dropped ``: a fixup, which puts a binding back without running
  anything.

A unit _recompiles_ when salsa runs its typecheck or its ownership analysis
again. A memo hit is not a recompile, so a unit that returns to a state it was
compiled in before only reruns. Lowering is redone for every unit the engine
reaches and is not counted.

Units keep the numbers they were entered with: a deleted unit leaves a gap, and
an inserted one takes the next number, wherever it sits. A comment under a
table lists where the engine does more work than the table asks; the test
accepts exactly that much more, and fails if the engine stops needing it, so
the comments stay true.

To add a case, write a fixture (the format is at the top of
`reactive_tests.rs`), add regions for it here --
`<!-- world NAME code -->` and `<!-- world NAME step N -->`, each closed by
`<!-- end world -->` -- and render them with
`BLESS=1 cargo test -p datalove-script-engine --test reactive_tests`. The
test fails if a fixture has a step not shown here, or this file differs from
its fixtures.

## The algebra

### Operations

What matters about a unit is what it does to bindings earlier units provide.
It uses each in one of five ways:

- _Read_ it, by borrowing: `a + 1`, `f(ref a)`.
- _Take_ it, by moving it: `let a_moved = a`, `f(a)`.
- _Overwrite_ it: write it on every way through the unit before anything reads
  what it held, early returns included. `set b = 10`, `f(out b)`.
- _Update_ it: any other write. `set b = b * 2`, `f(mut b)`, `set p.1 = x`,
  and a write under an `if` or in a loop, which may leave the old value.
- _Revive_ it: write a `var` an earlier unit gave away.

The rules apply to each binding separately.

### Modifications

A step makes one modification, and each is made of _retract_ and _apply_:

- _Edit_, an edit that compiles: retract, then apply.
- _Break_, an edit that does not compile: retract.
- _Delete_: retract.
- _Fix_, an edit that makes a unit compile that did not: apply.
- _Add_, an append or an insert: apply.

Retract reverses what the unit's previous code did to earlier bindings, and
apply runs its current code. So an Edit that turns a take into a read retracts
a take and applies a read. A unit rerun because something it uses changed is
retracted and applied with the same code. _Forward_ actions rerun the units
after it. Capitalised, Edit is the modification; a lower-case edit is any
change to a unit's code, Break and Fix included.

### The table

| operation | retract | apply | forward |
|-----------|---------|-------|---------|
| read      | nothing | rerun if a later unit took or wrote it | nothing |
| take      | rerun   | rerun if it or a later unit took or wrote it | later units that name it break; removing the take fixes them |
| overwrite | rerun   | nothing | later units that read it rerun |
| update    | rerun   | rerun if it or a later unit took or wrote it | later units that read it rerun |
| revive    | fixup: empty it | nothing | later units that name it are fixed, and rerun; removing the revival breaks them |

### Reruns and fixups

A _rerun_ restores a binding to what it held before the unit being retracted or
applied. It reruns the nearest unit before that one that sets the binding
without reading it -- its provider, or an overwrite or revival after that --
then the units between that took or wrote it, in order. Units that only
read it are not rerun: the rerun units have the same code and inputs, so a
reader would compute what it already holds.

A _fixup_ restores a binding without running anything. It is possible when the
binding held nothing, which is always so before a revival.

Retract needs a rerun only when the unit itself changed the binding:
retracting a read changes nothing, and a later unit's change would have
happened anyway. Apply needs one only for the operations that read the
binding, and only if the unit itself, last time, or a later unit changed it
since. A change by an earlier unit is just what the unit found.

### Forward

A unit after the modified one reruns if it reads a binding whose value the step
changed, whether or not it uses anything the modified unit provides, or if it
took or wrote a binding a rerun rebuilt. A take or revival added or removed
changes which names later units can use, so it breaks or fixes them, and those
are modifications with retract and apply of their own: the consequences
cascade.

Two things do not reach forward. A unit that overwrites or revives a binding
does not care how an earlier unit left it, only that it is there; if its write
was retracted, it reruns. And a take that is there before the step and after it
leaves the binding holding nothing either way.

### The closure

A rerun reruns a whole unit, so everything else that unit reads has to be as it
found it and everything else it changes is done again. So the rules run
backward and forward to a fixed point; the engine loops until nothing is added.
Binding names are enough to find it: linearity gives a value one owner at a
time, and every handoff is a take or a write some unit recorded by name. Names
are coarse, though, which costs reruns but not soundness: a write that may not
happen is an update, and a write to part of a binding is a write to all of it.

### Recompiling

The rules say what runs. What recompiles follows from salsa's memos, so it is
allowed to be conservative and it depends on history: a unit returning to a
state it was compiled in before reruns without recompiling.

## A unit stops compiling, and starts again

<!-- world compile-failure code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000
let x = y // `y` doesn't exist

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
<!-- end world -->

<!-- world compile-failure step 1 -->
**Step 1: all three units are entered, unit 1 broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | compile fails | new | - |
| 2 | `let b = 100` | - | compiles, runs | new | `b` = 100 |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | compile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

Unit 1 fails, so unit 3 cannot name `a`. Unit 2 needs nothing of unit 1 and
runs.

<!-- world compile-failure step 2 -->
**Step 2: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` moved |
| 2 | `let b = 100` | - | untouched | uses nothing of unit 1's | `b` moved |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompiles, runs | takes `a` from unit 1 | `a_moved` = 1000, `b_moved` = 100, `c` = 10 |
<!-- end world -->

Unit 2 is left alone, though unit 3 now takes its `b`.

<!-- world compile-failure step 3 -->
**Step 3: unit 1 is broken again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `let b = 100` | - | reruns | rerun: unit 3 took `b` when it last ran, and is retracted | `b` = 100 |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

Breaking unit 1 breaks unit 3, which is a retract of unit 3 too. Unit 3 took `b`
when it last ran, so unit 2 reruns to make it again. Without that, `b` would
read as moved with nothing holding it.

<!-- world compile-failure step 4 -->
**Step 4: unit 1 is fixed again.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` moved |
| 2 | `let b = 100` | - | untouched | `b` is as unit 2 made it | `b` moved |
| 3 | `let a_moved = a; let b_moved = b; let c = a_moved /! b_moved` | takes `a` (1), takes `b` (2) | recompiles, runs | takes `a` from unit 1 | `a_moved` = 1000, `b_moved` = 100, `c` = 10 |
<!-- end world -->

Fixing it again needs nothing of unit 2: `b` is as unit 2 made it.

The same with an update in place of the take:

<!-- world mutate-earlier code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

set b = b * 2
let c = a /! b
```
<!-- end world -->

<!-- world mutate-earlier step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | compiles, runs | new | `c` = 4 |
<!-- end world -->

<!-- world mutate-earlier step 2 -->
**Step 2: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 updated `b` when it last ran, and is retracted | `b` = 125 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

<!-- world mutate-earlier step 3 -->
**Step 3: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 2 made it | `b` = 250 |
| 3 | `set b = b * 2; let c = a /! b` | reads `a` (1), updates `b` (2) | recompiles, runs | reads `a` from unit 1 | `c` = 4 |
<!-- end world -->

Breaking unit 1 retracts unit 3's doubling, so unit 2 reruns and `b` is 125
again; fixing it doubles 125, not 250.

## Overwrites

<!-- world overwrite code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

set b = 500
let c = a /! b
```
<!-- end world -->

<!-- world overwrite step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 3 | `set b = 500; let c = a /! b` | reads `a` (1), overwrites `b` (2) | compiles, runs | new | `c` = 2 |
<!-- end world -->

<!-- world overwrite step 2 -->
**Step 2: unit 3 is edited to write 250.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing it uses changed | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 250 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompiles, runs | edited | `c` = 4 |
<!-- end world -->

Applying an overwrite needs nothing: it does not matter what `b` held.

<!-- world overwrite step 3 -->
**Step 3: unit 1 is broken.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000; let x = y` | - | recompile fails | edited | - |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 overwrote `b` when it last ran, and is retracted | `b` = 125 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompile fails | `a` is unbound: unit 1 failed | - |
<!-- end world -->

<!-- world overwrite step 4 -->
**Step 4: unit 1 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | recompiles, runs | edited | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 250 |
| 3 | `set b = 250; let c = a /! b` | reads `a` (1), overwrites `b` (2) | recompiles, runs | reads `a` from unit 1 | `c` = 4 |
<!-- end world -->

Retracting it does: unit 2 reruns to put `b` back. Applying it after that needs
nothing again.

A rerun starts at the nearest unit that sets the binding without reading it,
which may be an overwrite:

<!-- world rerun-from-overwrite code -->
```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

set b = 500

----------
scriptunit-fragment 3
----------

set b = b + 1

----------
scriptunit-fragment 4
----------

let c = b * 2

----------
scriptunit-fragment 5
----------

set b = 0
```
<!-- end world -->

<!-- world rerun-from-overwrite step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 0 |
| 2 | `set b = 500` | overwrites `b` (1) | compiles, runs | new | - |
| 3 | `set b = b + 1` | updates `b` (1) | compiles, runs | new | - |
| 4 | `let c = b * 2` | reads `b` (1) | compiles, runs | new | `c` = 1002 |
| 5 | `set b = 0` | overwrites `b` (1) | compiles, runs | new | - |
<!-- end world -->

<!-- world rerun-from-overwrite step 2 -->
**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | untouched | unit 2 overwrites `b` after it | `b` = 0 |
| 2 | `set b = 500` | overwrites `b` (1) | reruns | rerun: unit 4 reads `b`, which unit 5 overwrote after it, and unit 2 is the nearest to set it without reading it | - |
| 3 | `set b = b + 1` | updates `b` (1) | reruns | rerun: updates `b` from unit 2, which reran | - |
| 4 | `let c = b * 3` | reads `b` (1) | recompiles, runs | edited | `c` = 1503 |
| 5 | `set b = 0` | overwrites `b` (1) | reruns | rerun: overwrites `b` after unit 4 | - |
<!-- end world -->

Unit 4 needs `b` as 501. Unit 2 rebuilds 500 without looking at what unit 1
made, unit 3 makes it 501, and unit 1 is left alone. Unit 5 reruns after unit 4
because it overwrote what the rerun rebuilt.

## Reading before or after another unit changes a binding

<!-- world read-before-move code -->
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

let a_moved = a
```
<!-- end world -->

<!-- world read-before-move step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let a_moved = a` | takes `a` (1) | compiles, runs | new | `a_moved` = 1000 |
<!-- end world -->

<!-- world read-before-move step 2 -->
**Step 2: unit 2 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 2 reads `a`, which unit 3 took after it | `a` moved |
| 2 | `let c = a /! 100` | reads `a` (1) | recompiles, runs | edited | `c` = 10 |
| 3 | `let a_moved = a` | takes `a` (1) | reruns | rerun: takes `a` from unit 1, which reran | `a_moved` = 1000 |
<!-- end world -->

A read can need a rerun as much as a take can. Unit 3 took `a` after unit 2
read it, so unit 1 reruns before unit 2, and unit 3 reruns after to take the
new `a`.

<!-- world read-before-mutate code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

let c = a /! b

----------
scriptunit-fragment 4
----------

set b = b * 2
```
<!-- end world -->

<!-- world read-before-mutate step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 250 |
| 3 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
| 4 | `set b = b * 2` | updates `b` (2) | compiles, runs | new | - |
<!-- end world -->

<!-- world read-before-mutate step 2 -->
**Step 2: unit 3 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 reads `b`, which unit 4 updated after it | `b` = 250 |
| 3 | `let c = (a /! b) * 2` | reads `a` (1), reads `b` (2) | recompiles, runs | edited | `c` = 16 |
| 4 | `set b = b * 2` | updates `b` (2) | reruns | rerun: updates `b` from unit 2, which reran | - |
<!-- end world -->

The same with a later write. It would be the same with a later overwrite: the
kind of write decides whether applying the _writer_ needs a rerun, not a unit
before it. Unit 1 is a control: nothing changed `a`.

<!-- world read-after-mutate code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

set b = b * 2

----------
scriptunit-fragment 4
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

<!-- world read-after-mutate step 2 -->
**Step 2: unit 4 is edited.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 4 first saw it | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | untouched | unit 4 reads after it | - |
| 4 | `let c = (a /! b) * 2` | reads `a` (1), reads `b` (2) | recompiles, runs | edited | `c` = 8 |
<!-- end world -->

A change before the reader is just what it read.

## Edits that add or remove a take

<!-- world add-move code -->
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
<!-- end world -->

<!-- world add-move step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | compiles, runs | new | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | compiles, runs | new | `e` = 1002 |
<!-- end world -->

<!-- world add-move step 2 -->
**Step 2: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | `a` is as unit 1 made it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | reads `a` before the take | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a + 2` | reads `a` (1) | recompile fails | `a` was given away by unit 3 | - |
<!-- end world -->

The new take needs no rerun: `a` is still as unit 1 made it. The cost lands
forward: unit 4 can no longer name `a`.

<!-- world add-move step 3 -->
**Step 3: unit 3 is edited back to read `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 took `a` when it last ran, and the edit retracts that | `a` = 1000 |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 provided the same `a` | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | recompiles, runs | edited | `d` = 1001 |
| 4 | `let e = a + 2` | reads `a` (1) | reruns | `a` can be named again, as it could in step 1, whose compile is still memoized | `e` = 1002 |
<!-- end world -->

Removing the take is a retract, so unit 1 reruns, and unit 4 can name `a` again.
It only reruns: its compile from step 1 is still memoized.

An added take does need a rerun if a later unit already took the binding:

<!-- world add-move-later-take code -->
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

let e = a
```
<!-- end world -->

<!-- world add-move-later-take step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | compiles, runs | new | `c` = 100 |
| 3 | `let d = a + 1` | reads `a` (1) | compiles, runs | new | `d` = 1001 |
| 4 | `let e = a` | takes `a` (1) | compiles, runs | new | `e` = 1000 |
<!-- end world -->

<!-- world add-move-later-take step 2 -->
**Step 2, with unit 4 taking `a`: unit 3 is edited to take `a`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 takes `a`, which unit 4 took after it | `a` moved |
| 2 | `let c = a /! 10` | reads `a` (1) | untouched | unit 1 provided the same `a` | `c` = 100 |
| 3 | `let d = a` | takes `a` (1) | recompiles, runs | edited | `d` = 1000 |
| 4 | `let e = a` | takes `a` (1) | recompile fails | `a` was given away by unit 3 | - |
<!-- end world -->

## Edits that add or remove a write

<!-- world add-mutation code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

let d = b * 2

----------
scriptunit-fragment 4
----------

let c = a /! b
```
<!-- end world -->

<!-- world add-mutation step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 125 |
| 3 | `let d = b * 2` | reads `b` (2) | compiles, runs | new | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
<!-- end world -->

<!-- world add-mutation step 2 -->
**Step 2: unit 3 is edited to update `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | `b` is as unit 2 made it | `b` = 250 |
| 3 | `set b = b * 2` | updates `b` (2) | recompiles, runs | edited | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which now updates it | `c` = 4 |
<!-- end world -->

A write changes no names, so unit 4 compiles as before. It reruns because
the `b` it reads is now the one unit 3 left.

<!-- world add-mutation step 3 -->
**Step 3: unit 3 is edited back to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 updated `b` when it last ran, and the edit retracts that | `b` = 125 |
| 3 | `let d = b * 2` | reads `b` (2) | recompiles, runs | edited | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which no longer updates it | `c` = 8 |
<!-- end world -->

Removing the update is a retract, so unit 2 reruns. Unit 4 reruns by the forward
rule, not for the rerun's sake: the `b` it sees changed back.

Update and overwrite part when a later unit already wrote the binding:

<!-- world add-mutation-later-write code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

let d = b * 2

----------
scriptunit-fragment 4
----------

let c = a /! b

----------
scriptunit-fragment 5
----------

set b = b + 1
```
<!-- end world -->

<!-- world add-mutation-later-write step 1 -->
**Step 1: all five units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` = 1000 |
| 2 | `var b = 125` | - | compiles, runs | new | `b` = 126 |
| 3 | `let d = b * 2` | reads `b` (2) | compiles, runs | new | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | compiles, runs | new | `c` = 8 |
| 5 | `set b = b + 1` | updates `b` (2) | compiles, runs | new | - |
<!-- end world -->

<!-- world add-mutation-later-write step 2 -->
**Step 2: unit 3 is edited to update `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 updates `b`, which unit 5 updated after it | `b` = 251 |
| 3 | `set b = b * 2` | updates `b` (2) | recompiles, runs | edited | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which now updates it | `c` = 4 |
| 5 | `set b = b + 1` | updates `b` (2) | reruns | rerun: updates `b` from unit 2, which reran | - |
<!-- end world -->

<!-- world add-mutation-later-write step 3 -->
**Step 3: unit 3 is edited back to read `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | reruns | rerun: unit 3 updated `b` when it last ran, and the edit retracts that | `b` = 126 |
| 3 | `let d = b * 2` | reads `b` (2) | recompiles, runs | edited | `d` = 250 |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which no longer updates it | `c` = 8 |
| 5 | `set b = b + 1` | updates `b` (2) | reruns | rerun: updates `b` from unit 2, which reran | - |
<!-- end world -->

<!-- world add-mutation-later-write step 4 -->
**Step 4: unit 3 is edited to overwrite `b`.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | untouched | nothing changed `a` | `a` = 1000 |
| 2 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 501 |
| 3 | `set b = 500` | overwrites `b` (2) | recompiles, runs | edited | - |
| 4 | `let c = a /! b` | reads `a` (1), reads `b` (2) | reruns | reads `b` after unit 3, which now writes 500 | `c` = 2 |
| 5 | `set b = b + 1` | updates `b` (2) | reruns | rerun: updates `b` from unit 3, which reran | - |
<!-- end world -->

An added update has to read `b` as 125, so unit 2 reruns first. An added
overwrite does not read it, so unit 2 is left alone.

## Revivals

<!-- world revive code -->
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
<!-- end world -->

<!-- world revive step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 2 | `let b_moved = b` | takes `b` (1) | compiles, runs | new | `b_moved` = 125 |
| 3 | `set b = 500` | revives `b` (1) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |
<!-- end world -->

<!-- world revive step 2 -->
**Step 2: unit 3 is edited to write 600.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | untouched | unit 3 overwrites `b` without reading it | `b` = 600 |
| 2 | `let b_moved = b` | takes `b` (1) | untouched | nothing it uses changed | `b_moved` = 125 |
| 3 | `set b = 600` | revives `b` (1) | recompiles, runs | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | reads `b` after unit 3, which now writes 600 | `c` = 601 |
<!-- end world -->

A revival overwrites a binding that held nothing, so applying it needs nothing.

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

Retracting it needs only a fixup: before it, `b` held nothing, so dropping the 600
puts that back. Unit 4 can no longer name `b`.

<!-- world revive step 4 -->
**Step 4: unit 3 is fixed.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | untouched | `b` holds nothing, and unit 3 overwrites it | `b` = 600 |
| 2 | `let b_moved = b` | takes `b` (1) | untouched | nothing it uses changed | `b_moved` = 125 |
| 3 | `set b = 600` | revives `b` (1) | recompiles, runs | edited | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | `b` can be named again, as it could in step 2, whose compile is still memoized | `c` = 601 |
<!-- end world -->

Fixing it is an apply, and needs nothing either. Unit 4 is as it was compiled in
step 2, so it only reruns.

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

Whether a write revives depends on the units before it. With unit 2 no longer
taking `b`, unit 3 overwrites a `b` that holds something. Its lowered code is
the same; its ownership analysis is not. Unit 4 reads 600 either way and is
left alone.

## Reruns that need reruns

<!-- world rerun-needs-rerun code -->
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
<!-- end world -->

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

Unit 4 needs `b` as unit 3 left it, so unit 3 reruns. Unit 3 also updates `z`,
so unit 2 reruns first, though unit 4 never names `z`.

<!-- world rerun-through-moves code -->
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

Restoring `w` walks back through two takes to `a`, one rerun per link.

<!-- world forward-needs-back code -->
```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

var z = 10

----------
scriptunit-fragment 3
----------

var total = 0

----------
scriptunit-fragment 4
----------

set total = b + 1
set z = z + 1

----------
scriptunit-fragment 5
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

Unit 4 reruns forward because it reads `b`, and backward that needs `z` as
unit 2 made it.

<!-- world rerun-chain code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1

----------
scriptunit-fragment 2
----------

let b = a

----------
scriptunit-fragment 3
----------

let c = b

----------
scriptunit-fragment 4
----------

let d = c

----------
scriptunit-fragment 5
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

The cost of all this: editing the last link of a chain of takes reruns the
chain from its head, however expensive the head is.

## Values through intermediate bindings

<!-- world values-through-bindings code -->
```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

var total = 0

----------
scriptunit-fragment 3
----------

let d = b * 2

----------
scriptunit-fragment 4
----------

set total = d + 1

----------
scriptunit-fragment 5
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

The change reaches unit 5 through `d` and `total`, though unit 5 names nothing
units 1 or 3 make. Unit 2 is left alone: unit 4 overwrites `total`.

<!-- world same-value code -->
```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

let big = b .> 100

----------
scriptunit-fragment 3
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

## Conditional writes, loops and parts of bindings

<!-- world conditional-write code -->
```datalove
----------
scriptunit-fragment 1
----------

var b = 125

----------
scriptunit-fragment 2
----------

let flag = true

----------
scriptunit-fragment 3
----------

if flag
  set b = 500
end if

----------
scriptunit-fragment 4
----------

let c = b + 1
```
<!-- end world -->

<!-- world conditional-write step 1 -->
**Step 1: all four units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | compiles, runs | new | `b` = 500 |
| 2 | `let flag = true` | - | compiles, runs | new | `flag` = true |
| 3 | `if flag; set b = 500; end if` | reads `flag` (2), updates `b` (1) | compiles, runs | new | - |
| 4 | `let c = b + 1` | reads `b` (1) | compiles, runs | new | `c` = 501 |
<!-- end world -->

<!-- world conditional-write step 2 -->
**Step 2: unit 2 is edited so the write does not happen.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 125` | - | reruns | rerun: unit 3 may not write `b`, so it needs `b` as unit 1 made it | `b` = 125 |
| 2 | `let flag = false` | - | recompiles, runs | edited | `flag` = false |
| 3 | `if flag; set b = 500; end if` | reads `flag` (2), updates `b` (1) | reruns | reads `flag` from unit 2 | - |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | reads `b` after unit 3, which no longer writes it | `c` = 126 |
<!-- end world -->

A write that may not happen is an update, so when it stops happening `b` is
rebuilt.

<!-- world loop-update code -->
```datalove
----------
scriptunit-fragment 1
----------

var b = 1

----------
scriptunit-fragment 2
----------

let n = 3

----------
scriptunit-fragment 3
----------

var i = 0
loop while i .< n
  set b = b * 2
  set i = i + 1
end loop

----------
scriptunit-fragment 4
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

<!-- world loop-update step 2 -->
**Step 2: unit 2 is edited to loop once more.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var b = 1` | - | reruns | rerun: unit 3 updates `b` when it last ran, and reruns | `b` = 16 |
| 2 | `let n = 4` | - | recompiles, runs | edited | `n` = 4 |
| 3 | `var i = 0; loop while i .< n; set b = b * 2; set i = i + 1; end loop` | updates `b` (1), reads `n` (2) | reruns | reads `n` from unit 2 | `i` = 4 |
| 4 | `let c = b + 1` | reads `b` (1) | reruns | reads `b` after unit 3, which wrote a new value | `c` = 17 |
<!-- end world -->

<!-- world partial-write code -->
```datalove
----------
scriptunit-fragment 1
----------

var p = (1, "a")

----------
scriptunit-fragment 2
----------

let n = p.0 + 1

----------
scriptunit-fragment 3
----------

set p.1 = "b"
```
<!-- end world -->

<!-- world partial-write step 1 -->
**Step 1: all three units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `var p = (1, "a")` | - | compiles, runs | new | `p` = (1, "b") |
| 2 | `let n = p.0 + 1` | reads `p` (1) | compiles, runs | new | `n` = 2 |
| 3 | `set p.1 = "b"` | updates `p` (1) | compiles, runs | new | - |
<!-- end world -->

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

## Splices

<!-- world delete code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

var b = 125

----------
scriptunit-fragment 3
----------

let c = a /! b

----------
scriptunit-fragment 4
----------

let a_moved = a
set b = b * 2

----------
scriptunit-fragment 5
----------

let d = b + 1
```
<!-- end world -->

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

Deleting a unit is retracting it: unit 4 took `a` and updated `b`, so units 1 and
2 rerun, and unit 5 reruns because the `b` it reads changed. Unit 3 read both
before unit 4 changed them.

<!-- world insert code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

let a_moved = a
```
<!-- end world -->

<!-- world insert step 1 -->
**Step 1: both units are entered.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | compiles, runs | new | `a` moved |
| 2 | `let a_moved = a` | takes `a` (1) | compiles, runs | new | `a_moved` = 1000 |
<!-- end world -->

<!-- world insert step 2 -->
**Step 2: a unit reading `a` is inserted before the take.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 reads `a`, which unit 2 took after it | `a` moved |
| 3 | `let c = a + 1` | reads `a` (1) | compiles, runs | inserted | `c` = 1001 |
| 2 | `let a_moved = a` | takes `a` (1) | reruns | rerun: takes `a` from unit 1, which reran | `a_moved` = 1000 |

<!--
Engine today, more than the rule asks:
unit 2 recompiles, runs: an insert renumbers every later unit, so they are typechecked under new keys
-->
<!-- end world -->

An inserted unit is an apply with nothing to retract. Unit 3 reads `a`, which unit 2
took after it, so unit 1 reruns first and unit 2 takes it again.

## Returning early

A unit that returns early keeps what it did before the return and offers none
of its bindings. A unit that reads one of them is blocked.

<!-- world return-early code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

let d = 0

----------
scriptunit-fragment 3
----------

let a2 = a
let q = 10 /! d

----------
scriptunit-fragment 4
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

<!-- world return-early step 2 -->
**Step 2: unit 2 is edited so unit 3 finishes.**

| unit | code | uses from earlier units | this step | because | holds after |
|------|------|-------------------------|-----------|---------|-------------|
| 1 | `let a = 1000` | - | reruns | rerun: unit 3 took `a` when it last ran, and reruns | `a` moved |
| 2 | `let d = 5` | - | recompiles, runs | edited | `d` = 5 |
| 3 | `let a2 = a; let q = 10 /! d` | takes `a` (1), reads `d` (2) | reruns | reads `d` from unit 2 | `a2` = 1000, `q` = 2 |
| 4 | `let e = q + 1` | reads `q` (3) | reruns | reads `q` from unit 3, which finished | `e` = 3 |
<!-- end world -->

When unit 3 finishes, it is a forward rerun, because it reads `d`. It took `a`
when it last ran, so unit 1 reruns first, and unit 4, blocked before, runs.

<!-- world return-early-before-take code -->
```datalove
----------
scriptunit-fragment 1
----------

let a = 1000

----------
scriptunit-fragment 2
----------

let d = 0

----------
scriptunit-fragment 3
----------

let q = 10 /! d
let a2 = a

----------
scriptunit-fragment 4
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

What a unit gives away is decided by its code, so unit 4 cannot name `a` though
nothing took it at run time.

## Where the engine does more than the rules

Each is a comment under a table above.

- Retracting a revival reruns the provider and the take, where a fixup would do.
- The splices rerun every unit from the rerun point on, readers included,
  because a value is identified by its unit's index, and they start the rerun
  at the provider rather than a nearer overwrite (`rerun_start`).
- A write to part of a binding counts as a write to all of it.
- A rerun that comes out the same still reruns its readers.
- A unit that returned early is assumed to have done everything its code could.

Recompiles are not held to anything tighter than salsa's memos.
