import Datalove.FunMove
import Datalove.FunConsts

/-!
# A const's moves, on the consts evaluated before it

A script's consts are evaluated first, in order (`constPassMoves`), each from
the consts' values alone, calling functions from a table in which a const
not evaluated yet has no value. The move analysis (`mTop`) follows a const's
initializer on the top level's variables, every const holding a value, and
each function's body with every const it sees holding one. So its letting a
script through says nothing of a const read before it is evaluated, which
F089 refuses by the analysis of what holds a value (`constTop`).

This is the move analysis as F089 runs the other: each const on the
variables in scope, holding a value where it is a const evaluated before
(`constMarks`), and each function the const can reach on the consts that
function sees, evaluated or not, with only the reachable functions'
signatures for the calls it makes (`constMovesTop`). It is the model's own:
the Rust has no such pass, F089 and its ownership pass between them refusing
what this does, which `fun_tests` holds it to. `Proofs/MoveConst.lean` shows
that evaluating the consts of a script it lets through never reads what was
moved or holds nothing.
-/

namespace Datalove.Fun

/-- `mFun`, the calls it makes looked up in `A` and the body's context, as
the checker and the run have it, built from `S`: `mFun S` is `mFunIn S S`. -/
def mFunIn (A S : List (String × Sig)) (env : InferEnv) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (cmarks : List (String × Bool)) : FunDecl → MoveM Unit
  | .mk name tps params _ body =>
    match S.lookup name with
    | none => pure ()
    | some sig => do
      let Γ := funCtx S aliases consts tps params sig
      let consts : MSt := cmarks.map fun (x, s) =>
        { name := x, init := s, live := true, back := false, lent := false, writable := false, byRef := false }
      let σ0 ← params.foldlM (fun σ (m, x, _) => do
          let σ ← bindOne Γ (m != .out) (m == .ref || m == .mut || m == .out) (m == .mut || m == .out)
            (m == .ref) σ x
          pure (match σ with
            | v :: rest => { v with back := m == .out || m == .mut } :: rest
            | [] => [])) consts
      let (σ, _, _) ← mBlock A env fuel Γ σ0 0 body
      (if completes body then discard (exitM σ) else pure ())
      record "at_function_exit" 0 none (← ownedIn (scopeOwn consts σ))

/-- The variables `constMarks` marks, as the move analysis starts from them:
each live, borrowed by none, written by none. -/
def constMovesMarks (vars : List (String × Ty × Kind)) (known : List (String × Ty)) : MSt :=
  (constMarks vars known).map fun (x, s) =>
    { name := x, init := s, live := true, back := false, lent := false, writable := false, byRef := false }

/-- The move analysis of each of `fds`, the calls it makes looked up in `A`,
each const it sees holding a value where it is one of `known` it sees, each
with a schedule of its own, and the flag of each kept (`DSt.movedHeld`). -/
def funsMovesOk (A S : List (String × Sig)) (known : List (String × Ty)) : List FunDef → MoveM Unit
  | [] => pure ()
  | fd :: fds => do
    let (_, s) ← StateT.lift ((mFunIn A S movesEnv fd.aliases fd.consts
      (constMarks fd.consts (known.drop (known.length - fd.seen))) (.mk fd.name fd.tps fd.params none fd.body)).run {})
    modify fun t => { t with movedHeld := t.movedHeld || s.movedHeld }
    funsMovesOk A S known fds

/-- The move analysis of a const `s` reached in `Γ`, the consts in `known`
evaluated: the const itself, on the variables in scope, and each function
it can reach, on the consts that function sees, with only the signatures of
those functions for the calls made. -/
def constMovesOk (sigs : List (String × Sig)) (defs : List FunDef) (Γ : Ctx) (known : List (String × Ty))
    (s : Stmt) (e : Expr) : MoveM Unit :=
  let R := reachOf defs e
  let A := sigs.filter fun (f, _) => R.contains f
  mStmt A movesEnv fuel Γ (constMovesMarks Γ.vars known) 0 s >>= fun _ =>
  funsMovesOk A sigs known (defs.filter fun fd => R.contains fd.name)

/-- A script's top-level consts from `Γ`, as `constPassMoves` evaluates them,
`known` those evaluated, the latest first. -/
def constMovesTop (sigs : List (String × Sig)) (defs : List FunDef) :
    Ctx → List (String × Ty) → List Stmt → MoveM Unit
  | _, _, [] => pure ()
  | Γ, known, .fun_ _ :: ss => constMovesTop sigs defs Γ known ss
  | Γ, known, s :: ss =>
    let Γ' := ((checkStmt Γ s).run #[]).1
    match s with
    | .const_ _ _ e =>
      constMovesOk sigs defs Γ known s e >>= fun _ =>
      match Γ'.vars with
      | (x, t, _) :: _ => constMovesTop sigs defs Γ' ((x, t) :: known) ss
      | [] => pure ()
    | _ => constMovesTop sigs defs Γ' known ss

/-- What the move analysis of a script's consts refuses first, if anything,
each on the consts evaluated before it: O005 where one reads a const not
evaluated yet, as F089 has it, and O010 standing for a call that writes back
into a variable a later argument moved (`DSt.movedHeld`), which O010 and
O017 refuse between them. -/
def constMovesScript (ss : List Stmt) : Option Lit.Code :=
  let sigs := (resolveNames ss).1
  let Γ : Ctx := { ret := .ty (.result unitTy), sigs }
  match (constMovesTop sigs (funDefs sigs Γ 0 ss) Γ [] ss).run {} with
  | .ok (_, s) => if s.movedHeld then some "O010" else none
  | .error c => some c

end Datalove.Fun
