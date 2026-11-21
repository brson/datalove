# Proving the Drop Schedule

The formal model's drop schedule agrees with the Rust's (`drops_agree`), but
agreeing with the Rust is not being right: two implementations of one wrong
rule agree. This is the proof, in `formal/`, that the schedule the move
analysis builds drops each value a function owns exactly once on every way
through the function: `fun_ok`, in `Proofs/DropFun.lean`. It began as a
plan; [Where the proof is](#user-content-where-the-proof-is) maps it.

## Contents

- [The problem](#user-content-the-problem)
- [What a path is](#user-content-what-a-path-is)
- [What is proved](#user-content-what-is-proved)
- [Why it holds](#user-content-why-it-holds)
- [What it leaves out](#user-content-what-it-leaves-out)
- [What setting it out found](#user-content-what-setting-it-out-found)
- [Where the proof is](#user-content-where-the-proof-is)

## The problem

The analysis walks a body once and joins what its branches and loop passes
leave. Lowering then emits, at each point a scope is left by, the drops the
schedule names there, and nothing else decides a drop. So a drop the
schedule gets wrong is a double free or a leak on some run, whatever the
analysis said about moves. The walk sees one state per point; a run sees
whichever way it came. The proof is about the second.

## What a path is

A path follows the function's control: each `if` one branch, each `match`
one arm, each loop any number of passes, leaving by its condition, the end
of its elements, a `break`, or a way out of the function, and each
expression that can return early (`exprLeaves`) either finishing or
returning partway.

What a step moves and gives is what the walk says that statement moves and
gives: a binding live before a statement's expression and not after was
moved by it, one not live before and live after was given a value. The
path's own state is built from those steps alone, composed along the path,
never taken from the walk's joined states. Bindings are the walk's: each has
one identity however often a loop passes its declaration, and statements
keep the walk's numbers, by which the schedule names its points.

A return from inside an expression may come after any subset of the moves
the expression makes. In fact it comes after a prefix of them in evaluation
order; a subset is more paths, and the property holds of all of them.

## What is proved

For a function the move analysis accepts, on every path, at every point the
path passes -- a branch's, an arm's or a loop body's end, a `break`, a
`continue`, a `ret`, a `?` or other early return, a `set`'s failed step to
its place, and the end of the body:

- **Nothing is left holding.** Every owned binding in the scopes the point
  leaves that holds a value on the path is among the point's drops.
- **Nothing is dropped twice.** Every binding among the point's drops holds
  a value on the path, or keeps a tracking byte, whose drop finds out at run
  time: a `var` declared with no value, or one perhaps held
  (`maybe_held`).

At a `?`, the point's drops are what lowering drops: `before_try_return`,
and of `try_return_not_yet_moved` the bindings the path has not moved yet.

Owned means not copy and not borrowed (`MVar.owns`); a const declared in
the body is owned as a `let` is.

## Why it holds

Within one pass of every loop, the path's state and the walk's agree on what
is live: the walk takes one completing branch's state after an `if` or
`match`, and the branches that complete agree (O008); a loop's ways out
agree with each other and with running out (O014). The schedule at a point
is what the walk has owned and live there, so it is what the path holds.

Across passes they do not agree. A pass begins from where the last ended,
which keeps everything live that was live on entry (O007) but may hold a
`var` the loop was entered without and the body gave a value. That is
exactly a perhaps-held binding, which the walk adds to every way out of the
function scheduled from where the binding was made on, and which keeps a
tracking byte, so that a drop of it where it holds nothing does nothing. So
the invariant is: the path holds what the walk has live, and perhaps also
bindings perhaps held.

A scope's end, a `break` and a `continue` drop only bindings made inside the
scope or loop they leave. A perhaps-held binding was made before the loop
that gave it a value, so those points never need it, except where they
are scheduled after that loop, where the walk already counts it.

Within an expression nothing gives a value to what holds none, so what is
live only shrinks along an expression (`mE_shr`), and a return from inside
one holds what was live after it plus what it had not yet moved, which is
the `?` point's drops. One thing is given back: a binding passed `mut` or
`out` is the call's to write once every argument has run, so the walk has
it live after the call as before it, whatever the arguments between did.
Were a later argument to move it, the state at a `?` between the two would
be one the run never has. That no argument does is O017 and O010, which the
Rust refuses and this proof assumes; see what it leaves out.

## What it leaves out

- **That the walk's moves are the run's.** A path moves what the walk says;
  that the run moves the same is `moves_agree`, tested, and the
  move-soundness proof `formal.md` lists first.
- **That an expression returns early only where `exprLeaves` says.** It is
  the Rust's own `early_return_in`, and the schedule has a point only there.
- **That no later argument moves what an earlier one passes `mut` or
  `out`.** The walk has such a binding live after the call as before it,
  so a path returning at a `?` between the two may hold it where the run
  has moved it: in `two(mut x, both(x, maybe(c)?))` the schedule drops `x`
  at the `?`, and the run has already moved it into `both`'s pending
  argument. The move analysis alone lets the call through, so the theorem
  covers it. The Rust refuses it, O017 where a later operand moves or takes
  `mut` or `out` what an earlier one holds by reference, O010 for
  `two(mut x, x)`; the model decides both (`FunHeld.lean`, `FunAlias.lean`),
  but `fun_ok` assumes only that the move analysis accepts, so of a program
  the Rust accepts it says what it says, and of one the model's move
  analysis alone accepts it may not.
- **That lowering carries the schedule out**, past what is checked.
  Lowering emits each point's drops from the schedule's own lists, and a
  debug build panics on a scheduled binding lowering has not bound. Two
  drops of a binding are lowering's to decide. A `set`'s store destroys what
  a precise target holds unless the analysis found the `set` writing over a
  moved value (`set_over_moved`): the model notes those too
  (`noteOverMoved`), `drops_agree` holds the two to each other, and
  `set_store_ok` shows that a precise owned binding holds a value on the
  path just where the walk has it live, so just where the `set` is not
  noted. And a `?` drops a binding whose place has been lowered but not yet
  taken as one of the call's pending arguments, the half of
  `try_return_not_yet_moved` lowering keeps itself (`compiler-guide.md`).
  A const declared in a body is owned and scheduled as a `let` is, on both
  sides, and lowering drops it so; `promote_function_consts` then makes a
  static of one of a non-copy type whose value const inlining folded, and
  deletes its drops, which is the IR's drop check's and the leak check's to
  see (`plan-static-consts.md`).
  The IR's drop check (`ir.md`, "The drop check") holds the result to its
  own consistency in a debug build.
- **A `set` to a binding that is not a `var`.** The move analysis refuses
  one now (`writableM`, O011), so the proof needs nothing of typechecking
  here.
- **A `break` or `continue` outside a loop.** Typechecking refuses one. A
  path through a body that leaves by one is a path still, and its points
  are right, but the theorem says nothing of where it goes after.
- **Statements outside a function.** The theorem is per function; a
  script's top-level statements have no schedule of their own.
- **A walk out of fuel.** Paths are only those the walk took with fuel to
  spare; the analysis runs at a fuel no real body reaches.

## What setting it out found

Working out why the property holds found two places it did not, in the
Rust and in the model alike:

- A `set`'s drops for an early return were scheduled once its value had
  run, before its keys. A key whose call took a binding and then returned
  at a `?` in its next argument, or a checked or optional compound operator,
  which returns after the keys, dropped what the call had taken a second
  time (engine fixture `1048`).
- A perhaps-held binding was added to the ways out scheduled from the first
  statement of the loop that gave it a value. An outer loop the binding was
  made before reaches its earlier statements on the next pass holding the
  value, and a `ret` there leaked it (engine fixture `1049`). It is added to
  every way out from where it was made on now: wider than needed, which its
  tracking byte makes harmless, and what the proof can say in a line.

## Where the proof is

Under `formal/Datalove/Proofs/`:

- **The path relation** (`DropPath.lean`, `DropProof.lean`): a path's state
  (`delta`, `Rel`), the points it passes (`Ev`, `Ev.Good`), and the paths
  through a statement, a block, a branch, a `match`'s cases and a loop's
  passes (`PStmt`, `PBlock`, `PBranch`, `PCases`, `PIter`), each a step the
  walk took.
- **What is live only shrinks along an expression** (`DropShrink.lean`,
  `mE_shr` and its parts).
- **The schedule only grows** (`DropSched.lean`, `DropWalk.lean`:
  `SchedExt`, `walk_ext`), and the walk keeps the bindings it was given in
  place (`DropLen.lean`: `walk_len`, `walk_shape`).
- **The invariant through each statement** (`DropSpec.lean`: `Inv`;
  `DropProof.lean`: `stmt_ok`, `block_ok`, `branch_ok`, `cases_ok`,
  `iter_ok`, by induction on the walk's fuel and a loop's passes). A pass
  of a loop starts from the walk's state at the loop's start, related to
  the path's by `Rel` in a frame where a binding the loop gave a value may
  diverge; `DropLoop.lean` has what the loop's checks and `loopHeld` say of
  every pass.
- **The theorem for a function** (`DropFun.lean`: `fun_ok`), from the
  parameters' bindings, where the path holds everything and nothing
  diverges; and what a `set`'s store finds (`set_store_ok`).
