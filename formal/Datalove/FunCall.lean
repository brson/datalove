import Datalove.FunExec
import Datalove.Proofs.InstanceStmt

/-!
# Running a function's body for a call

`runCall` is the evaluator's `CallFn` for a program: it runs the called
function's body, as an instance, under the context the check-once theorem
checks it in (`funCtx`, with the type arguments substituted), with the
parameters bound to the arguments' values and the consts in scope to
theirs. A `ret e` is what the call gives, a `ret` or a body that ends gives
the unit, and with it what each parameter came to, for the caller to write
back a `mut` or `out` argument from (`paramsAfter`). Calls nest at most as
deep as they are given (`Stop.diverge`). A function with a signature and
no entry in the table is `Stop.wrong`: every function a script's signatures
name is in its table (`funTable_covers`).

It takes `funCtx` and `Ctx.inst` from the proofs, so that the context a body
runs in is by definition the one it was checked in.
-/

namespace Datalove.Fun

open Lit (Val)

/-- A function of the program as calls run it: its declaration's parts, and
the consts in scope where it is declared, with their values. -/
structure FunInfo where
  aliases : Aliases
  consts : List (String × Ty × Kind)
  cvals : VEnv
  tps : List (String × Option Bound)
  params : List (Mode × String × Hint)
  body : List Stmt

/-- The unit, what a function returning nothing gives. -/
def unitVal : Val := .tuple []

/-- The variables a body starts with, as `funCtx` lists them: the parameters,
the last innermost, bound to the arguments, then the consts. -/
def argVals (params : List (Mode × String × Hint)) (vals : List (Option Val)) (cvals : VEnv) : VEnv :=
  (params.zip vals).reverse.map (fun ((_, x, _), v) => (x, v)) ++ cvals

/-- What each parameter came to, in order, from the variables a body ended
with back in its own scope. -/
def paramsAfter (params : List (Mode × String × Hint)) (ρ : VEnv) : List (Option Val) :=
  ((ρ.take params.length).map (·.2)).reverse

/-- Calls into a program of functions `funs`, typed by `sigs`, nested at most
`d` deep, each loop running at most `k` times. -/
def runCall (env : InferEnv) (sigs : List (String × Sig)) (funs : List (String × FunInfo)) (k : Nat) :
    Nat → CallFn
  | 0 => fun _ _ _ => throw .diverge
  | d + 1 => fun f θ vals =>
    match sigs.lookup f, funs.lookup f with
    | some sig, some fi =>
      let ρ := argVals fi.params vals fi.cvals
      match execBlock env (runCall env sigs funs k d) fuel k
          ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ) ρ fi.body with
      | .ok ρ' => match sig.ret with
        | .void => pure (unitVal, paramsAfter fi.params (restore ρ ρ'))
        | .ty _ => throw .wrong
      | .error (.returned (some v) ρ') => pure (v, paramsAfter fi.params (restore ρ ρ'))
      | .error (.returned none ρ') => pure (unitVal, paramsAfter fi.params (restore ρ ρ'))
      | .error s => throw s
    | some _, none => throw .wrong
    | none, _ => throw .wrong

end Datalove.Fun
