import Datalove.FunCall

/-!
# Running a script

`runScript` runs a script as the Rust does: its top-level consts first, in
order, as the compiler evaluates them (`constPass`), then its top-level
statements in order, as `checkScript` checks them, keeping the value and
type of each top-level `debuglog`. Every function is in the table calls run
(`runCall`) from the start, wherever it is declared, each with the values of
the consts it sees where it is declared: the consts above it, by name and
type, the latest of each (`constEnv`, `FunDef.entry`). A const's expression
reads only consts above it, through the functions it calls too, which F089
sees to (`FunConsts.lean`); one that read a const not evaluated yet would
read it unset. A
`debuglog` inside a function is not kept.

A function goes into the table under the signature `resolveNames` gave its
name. Where the script checks, that is its own, with its type parameters and
as many parameters (`resolveNames_own`), and its body declares no type alias
(P074), which `Proofs/RunSound.lean` uses and the runner does not check.
-/

namespace Datalove.Fun

open Lit (Val)

/-- The top-level consts evaluated, the latest first: each one's name, type
and value. -/
abbrev CStore := List (String × Ty × Option Val)

/-- Variables `vars` with the values of those that are consts from `store`,
the latest of its name and type, and no value for the rest, which a const's
expression does not read (F058). -/
def constEnv (vars : List (String × Ty × Kind)) (store : CStore) : VEnv :=
  vars.map fun (x, t, k) =>
    (x, if k == .const_ then ((store.find? fun (y, u, _) => y == x && u == t).bind (·.2.2)) else none)

/-- What running a script gave: each top-level `debuglog`'s type and value,
in order, and how it ended. -/
structure RunOut where
  log : List (Ty × Val)
  result : EvalM Unit

/-- A function as declared: all of its table entry but the consts' values,
and how many top-level consts are declared above it. -/
structure FunDef where
  name : String
  aliases : Aliases
  consts : List (String × Ty × Kind)
  tps : List (String × Option Bound)
  params : List (Mode × String × Hint)
  body : List Stmt
  seen : Nat

/-- A function's table entry, with the values of the consts it sees from
`store`: those declared above it, the earliest `seen` evaluated. -/
def FunDef.entry (fd : FunDef) (store : CStore) : String × FunInfo :=
  (fd.name, { aliases := fd.aliases, consts := fd.consts,
              cvals := constEnv fd.consts (store.drop (store.length - fd.seen)),
              tps := fd.tps, params := fd.params, body := fd.body })

/-- A function declared in context `Γ` below `seen` consts, where it has a
signature. -/
def funDef (sigs : List (String × Sig)) (Γ : Ctx) (seen : Nat) : FunDecl → Option FunDef
  | .mk name tps params _ body => match sigs.lookup name with
    | some _ => some { name, aliases := Γ.aliases, consts := constsOf Γ.vars, tps, params, body, seen }
    | none => none

/-- A script's functions from `Γ`, below `seen` consts, as `checkScript`
reaches their declarations. -/
def funDefs (sigs : List (String × Sig)) : Ctx → Nat → List Stmt → List FunDef
  | _, _, [] => []
  | Γ, seen, .fun_ f :: ss => (funDef sigs Γ seen f).toList ++ funDefs sigs Γ seen ss
  | Γ, seen, s :: ss =>
    funDefs sigs ((checkStmt Γ s).run #[]).1 (if s matches .const_ .. then seen + 1 else seen) ss

/-- The table of a script's functions, its consts' values from `store`. -/
def funTable (defs : List FunDef) (store : CStore) : List (String × FunInfo) :=
  defs.map (·.entry store)

/-- How a top-level statement's stop ends the script: a return from a block
inside it is the script's early return. -/
def Stop.unreturn : Stop → Stop
  | .returned (some r) _ => .early r
  | .returned none _ => .retVoid
  | s => s

/-- A script's top-level consts from `Γ`, evaluated in order onto `store`
as `checkScript` reaches them, calls running `defs`. Each runs from the
consts' values alone, as the compiler evaluates it before anything else. -/
def constPass (sigs : List (String × Sig)) (defs : List FunDef) (k d : Nat) :
    Ctx → CStore → List Stmt → EvalM CStore
  | _, store, [] => pure store
  | Γ, store, .fun_ _ :: ss => constPass sigs defs k d Γ store ss
  | Γ, store, s :: ss =>
    let Γ' := ((checkStmt Γ s).run #[]).1
    if s matches .const_ .. then do
      let ρ ← execStmt inferEnv (runCall inferEnv sigs (funTable defs store) k d) fuel k Γ
        (constEnv Γ.vars store) s
      match Γ'.vars, ρ with
      | (x, t, _) :: _, (_, o) :: _ => constPass sigs defs k d Γ' ((x, t, o) :: store) ss
      | _, _ => throw .wrong
    else constPass sigs defs k d Γ' store ss

/-- A script's statements from `Γ` and `ρ`, as `checkScript` checks them,
loops running at most `k` times and calls nesting at most `d` deep. -/
def runTop (sigs : List (String × Sig)) (k d : Nat) :
    Ctx → VEnv → List (String × FunInfo) → List (Ty × Val) → List Stmt → RunOut
  | _, _, _, log, [] => ⟨log, pure ()⟩
  | Γ, ρ, funs, log, .fun_ _ :: ss => runTop sigs k d Γ ρ funs log ss
  | Γ, ρ, funs, log, .debuglog e :: ss =>
    let call := runCall inferEnv sigs funs k d
    match typeOf (synthF inferEnv (fuel - 1) Γ.borrow e), (evalS inferEnv call (fuel - 1) Γ.borrow e).run ρ with
    | .ok t, .ok (v, ρ') => runTop sigs k d Γ ρ' funs (log ++ [(t, v)]) ss
    | .error s, _ | _, .error s => ⟨log, throw s.unreturn⟩
  | Γ, ρ, funs, log, s :: ss =>
    match execStmt inferEnv (runCall inferEnv sigs funs k d) fuel k Γ ρ s with
    | .ok ρ' => runTop sigs k d ((checkStmt Γ s).run #[]).1 ρ' funs log ss
    | .error e => ⟨log, throw e.unreturn⟩

/-- A script run, its loops at most `k` times and its calls at most `d`
deep: its consts, then its statements. A const that stops ends it before
anything is logged. -/
def runScript (k d : Nat) (ss : List Stmt) : RunOut :=
  let sigs := (resolveNames ss).1
  let Γ : Ctx := { ret := .ty (.result unitTy), sigs }
  let defs := funDefs sigs Γ 0 ss
  match constPass sigs defs k d Γ [] ss with
  | .ok store => runTop sigs k d Γ [] (funTable defs store) [] ss
  | .error s => ⟨[], throw s.unreturn⟩

end Datalove.Fun
