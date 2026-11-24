# Proving Moves Are Never Read

The formal model's move analysis agrees with the Rust's ownership pass
(`moves_agree`), and its run moves what the analysis says a read takes
(`runScriptMoves`). Agreeing is not being right: the two could share a
rule that lets a moved variable be read. This is the proof, in `formal/`,
that a script the move analysis lets through never reads what was moved:
`runScriptMoves_move`, in `Proofs/MoveCall.lean`. Written November 2026, as
the proof went in; [Where the proof is](#where-the-proof-is) maps it.

## Contents

- [The problem](#the-problem)
- [What is proved](#what-is-proved)
- [Why it holds](#why-it-holds)
- [What it leaves out](#what-it-leaves-out)
- [What setting it out found](#what-setting-it-out-found)
- [Where the proof is](#where-the-proof-is)

## The problem

The analysis (`FunMove.lean`) follows a body once, marking each variable
as given a value or not and as moved or not, and refuses a read of one that
holds nothing (O001, O005), a way back to a loop's start that moved what
was live there (O007), and branches that disagree about what is moved
(O008, O014). The run (`FunEval.lean`, under `movesEnv`) takes a variable's
value out where the analysis says the read takes it (`Ctx.takes`), and
stops at `Stop.uninit` where it reads a variable holding nothing. The init
proof (`Proofs/InitSound.lean`) shows the run never stops there for what
was never given a value, but it rests on `Keeps`: a step only ever gives a
variable a value. Under a moving run that is false, and the init proof's
theorem with it. So a proof against the move analysis's own state has to
replace it, and the state's joins, loops and generic bodies each ask for
something the init proof never needed.

## What is proved

Where a script checks (`checkScript`), the move analysis lets it through
(`moveScript`), no call of it writes back into a variable a later argument
moved (`moveScriptHeld`), and its top-level statements declare a type alias
only directly (`TopTypeFree`): `runScriptMoves` stops at `Stop.uninit` only
where evaluating the consts (`constPassMoves`) does. Past the consts, no
variable is read while it holds nothing, moved or never given a value, and
no call returns with a `mut` or `out` parameter it left holding nothing.

The pieces are the init proof's:

- **Expressions** (`evalS_move`, `Proofs/MoveSound.lean`): from variables
  that line up with the analysis's state, an expression the analysis follows
  comes to a value and variables lining up with the state after it, or stops
  as a step may.
- **Statements and blocks** (`execStmt_move`, `execBlock_move`,
  `Proofs/MoveStmt.lean`): the same, with a `break` or `continue` lining up
  with one of the states the analysis followed to it, and a return only with
  every `mut` and `out` parameter holding a value.
- **Calls** (`runCall_move`, `Proofs/MoveCall.lean`): a call into a program
  whose bodies the analysis lets through keeps the promise `CallMove` makes:
  given a value for every parameter but an `out` one, it never stops at a
  variable holding nothing and returns a value for every `mut` and `out`
  parameter.
- **The top level** (`runTopMoves_move`) and the script
  (`runScriptMoves_move`), with the table a script's functions make shown to
  be such a program (`funTable_move`).

## Why it holds

**Lining up.** A run's variables line up with a state where every variable
the analysis has given a value and not moved holds one (`MAg`). A read the
analysis lets through is of such a variable, so the run finds a value; a
read that takes it leaves the variable moved in the analysis and holding
nothing in the run, which still line up. Writes give a value to both.

**Un-giving.** The init proof's `Keeps` becomes `MSt.Le`
(`Proofs/MoveShape.lean`): a step of the analysis never takes back that a
variable was given a value, never changes a variable's name, whether a
caller reads it back, or whether it is borrowed, and keeps a borrowed
variable live. Every expression and statement is such a step (`mE_estep`,
`mStmt_sstep`), and the frame a body runs in keeps its shape (`MFr`).

**Loops.** The analysis follows a loop's body once from what the condition
left. Each way back to the start keeps live what was live before the
condition (O007, `MSt.keepsLive`), and un-gives nothing (`MSt.Le`), so
variables that line up with a way back line up with the start again
(`MAg.le`), and the next pass is the first (`execLoop_move`). A loop's
ways out agree about what is moved (O014), as an `if`'s and a `match`'s
branches do (O008), and the join is weaker than each way (`MSt.Weaker`),
so what lines up with any way lines up with the join.

**Two contexts.** A generic body is analyzed in the context the checker
checks it in (`funCtx`) and run in an instance of it (`Ctx.inst θ`), where
a type parameter may have become copy and a read that moved under the
generic borrows under the instance. The statement theorems take both
contexts. For expressions the analysis's context need only look each
variable up as copy where the run's does (`CtxLe`, by `Ty.isCopy_inst`):
where the analysis moves and the run does not, the variable holds a value
the analysis no longer counts on, which lines up. For statements the
contexts a statement binds in, an `if`'s, a `match`'s, a `for`'s and the
one it leaves, have to be the generic's instantiated, which the check-once
theorem's lemmas give (`checkStmtF_inst`, `synthF_inst`,
`forBindings_inst`) under the quiet check `checkScript` gives, with
inference under an instance answering what it answered under the generic
(`InstEnv`, which `Proofs/Lockstep.lean` proves of the real inference,
and `Proofs/EnvCongr.lean` carries to `movesEnv`, the checker never reading
`InferEnv.moves`). The top level runs in its own context, an instance at
no type arguments (`Ctx.inst_nil`).

**The key.** A plain `set m[k] = v` inserts its key: the Rust and the
analysis take it, the run lends every key. The run therefore keeps a value
the analysis has moved, which lines up, but the expression theorem follows
the analysis's own walk, which borrowed nothing. A monotonicity lemma
bridges it (`mE_mono`, `Proofs/MoveMono.lean`): an expression followed from
a state with fewer moves, in a context that is copy wherever the first was,
comes to a state with fewer moves and sets the held flag no more. The keys
as the run lends them (`mKeysLent`) are such a walk of the keys as the
analysis takes them.

**The flag.** O010 and O017 are not the move analysis's: `f(mut p.x, p)`
and `f(mut p.x, g(p))` are let through by it alone, and their calls write
back into a variable a later argument moved. The analysis records the case
instead (`DSt.movedHeld`, set by `noteHeldMoved` where a `mut` or `out`
argument's variable is not live once every argument has run), the theorem
takes the flag clear, and `moves_agree` holds the flag to the Rust: where
the Rust reports neither code, the model sets no flag.

## What it leaves out

- **The consts.** A const's initializer runs on the consts' values alone,
  where the top level's state has every binding, and that it reads only
  consts rests on F058 in the checker. `Proofs/ConstSound.lean` carries F089
  for what holds a value; the same for moves is not done, so the theorem
  stops at the const pass.
- **The flag from its own analyses.** A lemma from `FunAlias.lean`'s and
  `FunHeld.lean`'s acceptance to the flag clear would make O010 and O017
  hypotheses rather than the flag.
- **Type aliases in top-level blocks.** The parser refuses an alias in a
  block (P074) and the runner does not check it; the theorem takes
  `TopTypeFree` instead.

## What setting it out found

- **A call's type arguments were not held to its bounds.** `evalCall` ran a
  body under whatever type arguments inference gave; the body's context is
  the checker's with them substituted, which the check-once theorem speaks
  of only where the bounds admit them and every parameter is decided (F092,
  F101). The checker refuses either, so the evaluator now stops at
  `Stop.wrong` there, which `evalCall_sound` takes in.
- **A `ret`'s O006 is checked before its value runs**, so a borrowed `out`
  parameter moved by the value would pass. It cannot be moved, being
  borrowed (O003), which is why `MSt.Le` keeps a borrowed variable live and
  the frame says every parameter read back is borrowed (`BackLent`).
- **A generic's acceptance does not carry to its instance by monotonicity
  alone.** Branches that agree about what is moved under the generic may
  not under an instance followed from a state with fewer moves, unless the
  difference is exactly the variables whose type became copy, which
  monotonicity does not say. Following the run in the instance's context
  against the analysis in the generic's needs no such lemma, which is why
  the statement theorems take two contexts.

## Where the proof is

- `Proofs/EnvCongr.lean`: the checker never reads `InferEnv.moves`.
- `Proofs/MoveShape.lean`: `MSt.Le`, `MSt.Inside`, the flag's monotonicity,
  and that every expression and statement is a step.
- `Proofs/MoveSound.lean`: `MAg`, `MFr`, `CtxLe`, `CallMove`, the joins,
  and the expression theorems.
- `Proofs/MoveMono.lean`: `MSt.Freer`, `mE_mono`, `mKeysLent`.
- `Proofs/MoveStmt.lean`: loops, `MSStop`, and the statement theorems.
- `Proofs/MoveCall.lean`: `Ctx.inst_nil`, `ProgMove`, `runCall_move`,
  `runTopMoves_move`, `funTable_move`, `runScriptMoves_move`.
