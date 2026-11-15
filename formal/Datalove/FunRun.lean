import Datalove.FunCall

/-!
# Running a script

`runScript` runs a script's top-level statements in order, as `checkScript`
checks them, keeping the value and type of each top-level `debuglog`. Its
functions go into the table calls run (`runCall`) as their declarations are
reached, each with the values of the consts it sees there (`constVals`, as
`constsOf` picks the consts), so a call of a function declared below it is
not run yet (`Stop.unsupported`). A `debuglog` inside a function is not kept.

A function goes into the table only where its signature's type parameters
are its declaration's, its parameters are as many as its signature's, and
its body declares no type alias. `resolveNames` makes the first two so of
any script and the parser the third, which `Proofs/RunSound.lean` does not
yet show; a function that broke them would not be run.
-/

namespace Datalove.Fun

open Lit (Val)

/-- The values of the consts in `vars`, as `constsOf` picks them: a const not
shadowed by a binding inside it, with the value it is bound to in `ρ`. -/
def constVals : List (String × Ty × Kind) → VEnv → VEnv
  | (x, _, k) :: vs, (_, o) :: ρ =>
    let rest := (constVals vs ρ).filter (·.1 != x)
    if k == .const_ then (x, o) :: rest else rest
  | _, _ => []

/-- What running a script gave: each top-level `debuglog`'s type and value,
in order, and how it ended. -/
structure RunOut where
  log : List (Ty × Val)
  result : EvalM Unit

/-- A function's table entry, where its declaration is one `runCall` can run
as checked. -/
def funEntry (sigs : List (String × Sig)) (Γ : Ctx) (ρ : VEnv) : FunDecl → Option (String × FunInfo)
  | .mk name tps params _ body => match sigs.lookup name with
    | some sig =>
      if sig.tparams == tps && params.length == sig.params.length && blockTypeFree body then
        some (name, { aliases := Γ.aliases, consts := constsOf Γ.vars, cvals := constVals Γ.vars ρ,
                      tps, params, body })
      else none
    | none => none

/-- How a top-level statement's stop ends the script: a return from a block
inside it is the script's early return. -/
def Stop.unreturn : Stop → Stop
  | .returned (some r) _ => .early r
  | .returned none _ => .retVoid
  | s => s

/-- A script's statements from `Γ` and `ρ`, as `checkScript` checks them,
loops running at most `k` times and calls nesting at most `d` deep. -/
def runTop (sigs : List (String × Sig)) (k d : Nat) :
    Ctx → VEnv → List (String × FunInfo) → List (Ty × Val) → List Stmt → RunOut
  | _, _, _, log, [] => ⟨log, pure ()⟩
  | Γ, ρ, funs, log, .fun_ f :: ss =>
    runTop sigs k d Γ ρ (funs ++ (funEntry sigs Γ ρ f).toList) log ss
  | Γ, ρ, funs, log, .debuglog e :: ss =>
    let call := runCall inferEnv sigs funs k d
    match typeOf (synthF inferEnv (fuel - 1) Γ.borrow e), evalS inferEnv call (fuel - 1) Γ.borrow ρ e with
    | .ok t, .ok v => runTop sigs k d Γ ρ funs (log ++ [(t, v)]) ss
    | .error s, _ | _, .error s => ⟨log, throw s⟩
  | Γ, ρ, funs, log, s :: ss =>
    match execStmt inferEnv (runCall inferEnv sigs funs k d) fuel k Γ ρ s with
    | .ok ρ' => runTop sigs k d ((checkStmt Γ s).run #[]).1 ρ' funs log ss
    | .error e => ⟨log, throw e.unreturn⟩

/-- A script run, its loops at most `k` times and its calls at most `d`
deep. -/
def runScript (k d : Nat) (ss : List Stmt) : RunOut :=
  let sigs := (resolveNames ss).1
  runTop sigs k d { ret := .ty (.result unitTy), sigs } [] [] [] ss

end Datalove.Fun
