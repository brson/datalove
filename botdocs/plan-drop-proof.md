# Proving the Drop Schedule

The formal model's drop schedule agrees with the Rust's (`drops_agree`), but
agreeing with the Rust is not being right: two implementations of one wrong
rule agree. This plan is for a proof, in `formal/`, that the schedule the
move analysis builds drops each value a function owns exactly once on every
way through the function.

## Contents

- [The problem](#user-content-the-problem)
- [What a path is](#user-content-what-a-path-is)
- [What is proved](#user-content-what-is-proved)
- [Why it holds](#user-content-why-it-holds)
- [What it leaves out](#user-content-what-it-leaves-out)
- [What setting it out found](#user-content-what-setting-it-out-found)
- [Steps](#user-content-steps)

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

Owned means not copy, not borrowed, not a const (`MVar.owns`).

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

Within an expression nothing gives a value to what holds none: an `out`
argument has to be live (O001), a `mut` one live and given a value. So
what is live only shrinks along an expression, and a return from inside one
holds what was live after it plus what it had not yet moved, which is the
`?` point's drops.

## What it leaves out

- **That the walk's moves are the run's.** A path moves what the walk says;
  that the run moves the same is `moves_agree`, tested, and the
  move-soundness proof `formal.md` lists first.
- **That an expression returns early only where `exprLeaves` says.** It is
  the Rust's own `early_return_in`, and the schedule has a point only there.
- **That lowering carries the schedule out.** The IR's drop check
  (`ir.md`, "The drop check") holds lowering to its own consistency in a
  debug build; nothing compares it with the schedule.
- **A `set` to a binding that is not a `var`.** Typechecking refuses it,
  and the move analysis does not look; the proof takes it as given.

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

## Steps

1. The path relation, over the walk's run: a statement's step taken from the
   walk's sub-results, the path's state threaded beside the walk's.
2. That along an expression what is live only shrinks (`mE` and its parts).
3. That the schedule only grows after a point is recorded: later points are
   added, and a way out of the function gains only perhaps-held bindings.
4. The invariant through each statement, by induction on the walk's fuel,
   and the two properties at each point from it.
5. The theorem for a function, and `formal.md`'s entry moved from "not
   proved" to proved.
