import Datalove.FunExec
import Datalove.Proofs.InstanceStmt

/-!
# Running a function's body for a call

`runCall` is the evaluator's `CallFn` for a program: it runs the called
function's body, as an instance, under the context the check-once theorem
checks it in (`funCtx`, with the type arguments substituted), with the
parameters bound to the arguments' values and the consts in scope to
theirs. A `ret e` is what the call gives, a `ret` or a body that ends gives
the unit. Calls nest at most as deep as they are given (`Stop.diverge`). A
function not in the table, as one a script declares below the call is not
yet, is not run (`Stop.unsupported`).

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
def argVals (params : List (Mode × String × Hint)) (vals : List Val) (cvals : VEnv) : VEnv :=
  (params.zip vals).reverse.map (fun ((_, x, _), v) => (x, some v)) ++ cvals

/-- Calls into a program of functions `funs`, typed by `sigs`, nested at most
`d` deep, each loop running at most `k` times. -/
def runCall (env : InferEnv) (sigs : List (String × Sig)) (funs : List (String × FunInfo)) (k : Nat) :
    Nat → CallFn
  | 0 => fun _ _ _ => throw .diverge
  | d + 1 => fun f θ vals =>
    match sigs.lookup f, funs.lookup f with
    | some sig, some fi =>
      match execBlock env (runCall env sigs funs k d) fuel k
          ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ)
          (argVals fi.params vals fi.cvals) fi.body with
      | .ok _ => match sig.ret with
        | .void => pure unitVal
        | .ty _ => throw .wrong
      | .error (.early v) => pure v
      | .error .retVoid => pure unitVal
      | .error s => throw s
    -- A function a script declares below where it is called is not in the
    -- table yet.
    | some _, none => throw .unsupported
    | none, _ => throw .wrong

end Datalove.Fun
