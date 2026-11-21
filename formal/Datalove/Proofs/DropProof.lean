import Datalove.Proofs.DropLoop

/-!
# Every path through a function drops what it owns exactly once

The paths (`PStmt`, `PBlock`, `PCases`, `PLoop`, `PFor`) follow the move
analysis's walk of a body: each `if` one branch, each `match` one arm, each
loop any number of passes, leaving by its condition, the end of its
elements, a `break` or a way out of the function, and each step that may
return early (`leavingIf`) either finishing or returning, having made any
of the moves the walk says it makes. What a step moves and gives is what
the walk says of it, and the path's own state is built from those steps
alone (`delta`), never taken from the walk's joins. Each point the path
passes is an event (`Ev`), which names where the point is in the schedule
the function's walk ends with.
-/

namespace Datalove.Fun

/-- A path that does not go on to what follows. -/
def POut.notNorm : POut → Prop
  | .norm _ => False
  | _ => True

section
variable (sigs : List (String × Sig)) (env : InferEnv)

mutual

/-- The paths through a statement, at the walk's fuel, context, state and
schedule, from what the path holds. -/
inductive PStmt : Nat → Ctx → MSt → Nat → Stmt → DSt → List Bool → POut → List Ev → Prop where
  | let_fin {n Γ σ base p t e st π id st1 σ1 st2 σ2 st3} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      (bindM ((checkStmtF env n Γ (.let_ p t e)).run #[]).1 σ1 (patNames p) true).run st2 = .ok (σ2, st3) →
      PStmt (n + 1) Γ σ base (.let_ p t e) st π (.norm (newFlags σ1 σ2 ++ delta σ σ1 π)) []
  | let_exit {n Γ σ base p t e st π id st1 σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      exprLeaves e = true → Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.let_ p t e) st π .exit [tryEv st1 st2 σ πk]
  | var_fin {n Γ σ base p t e st π id st1 σ1 st2 σ2 st3} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      (bindM ((checkStmtF env n Γ (.var_ p t (some e))).run #[]).1 σ1 (patNames p) true (writable := true)).run st2
        = .ok (σ2, st3) →
      PStmt (n + 1) Γ σ base (.var_ p t (some e)) st π (.norm (newFlags σ1 σ2 ++ delta σ σ1 π)) []
  | var_exit {n Γ σ base p t e st π id st1 σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      exprLeaves e = true → Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.var_ p t (some e)) st π .exit [tryEv st1 st2 σ πk]
  | var_bare {n Γ σ base p t st π id st1 σ1 st2} :
      stmtId.run st = .ok (id, st1) →
      (bindM ((checkStmtF env n Γ (.var_ p (some t) none)).run #[]).1 σ (patNames p) false (writable := true)).run st1
        = .ok (σ1, st2) →
      PStmt (n + 1) Γ σ base (.var_ p (some t) none) st π (.norm (newFlags σ σ1 ++ π)) []
  | var_none {n Γ σ base p st π} : PStmt (n + 1) Γ σ base (.var_ p none none) st π (.norm π) []
  | fun_ {n Γ σ base f st π} : PStmt (n + 1) Γ σ base (.fun_ f) st π (.norm π) []
  | type_ {n Γ σ base x t st π} : PStmt (n + 1) Γ σ base (.type_ x t) st π (.norm π) []
  | set_fin {n Γ σ base x steps op e st π id st1 st1a st1b σ1 st2 σ2 st3} :
      stmtId.run st = .ok (id, st1) →
      (unwalkedM σ x).run st1 = .ok ((), st1a) → (writableM σ x).run st1a = .ok ((), st1b) →
      (leavingIf id σ (exprLeaves e || (steps.any fun | .index k _ => exprLeaves k | _ => false) ||
        op.any BinOp.exits) (mSetParts sigs Γ σ steps op e)).run st1b = .ok (σ1, st2) →
      (mTarget σ1 x steps op).run st2 = .ok (σ2, st3) →
      PStmt (n + 1) Γ σ base (.set_ x steps op e) st π
        (.norm (delta σ (if steps.isEmpty then σ2.give x else σ2) π)) []
  | set_try {n Γ σ base x steps op e st π id st1 st1a st1b σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (unwalkedM σ x).run st1 = .ok ((), st1a) → (writableM σ x).run st1a = .ok ((), st1b) →
      (leavingIf id σ (exprLeaves e || (steps.any fun | .index k _ => exprLeaves k | _ => false) ||
        op.any BinOp.exits) (mSetParts sigs Γ σ steps op e)).run st1b = .ok (σ1, st2) →
      (exprLeaves e || (steps.any fun | .index k _ => exprLeaves k | _ => false) || op.any BinOp.exits) = true →
      Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.set_ x steps op e) st π .exit [tryEv st1b st2 σ πk]
  | set_target {n Γ σ base x steps op e st π id st1 st1a st1b σ1 st2 σ2 st3 ds st4} :
      stmtId.run st = .ok (id, st1) →
      (unwalkedM σ x).run st1 = .ok ((), st1a) → (writableM σ x).run st1a = .ok ((), st1b) →
      (leavingIf id σ (exprLeaves e || (steps.any fun | .index k _ => exprLeaves k | _ => false) ||
        op.any BinOp.exits) (mSetParts sigs Γ σ steps op e)).run st1b = .ok (σ1, st2) →
      (mTarget σ1 x steps op).run st2 = .ok (σ2, st3) →
      stepsExit steps = true →
      (ownedIn σ2).run st3 = .ok (ds, st3) →
      (record "before_set_target_early_return" id none ds (always := true)).run st3 = .ok ((), st4) →
      PStmt (n + 1) Γ σ base (.set_ x steps op e) st π .exit [recEv st3 st4 σ2 (delta σ σ2 π)]
  | ret_try {n Γ σ base e st π id st1 σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (exitM σ).run st1 = .ok (σ, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      exprLeaves e = true → Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.ret (some e)) st π .exit [tryEv st1 st2 σ πk]
  | ret_fin {n Γ σ base e st π id st1 σ1 st2 ds st3} :
      stmtId.run st = .ok (id, st1) →
      (exitM σ).run st1 = .ok (σ, st1) →
      ((match e with
        | some e => leaving id σ e (mE sigs Γ false σ e)
        | none => pure σ) : MoveM MSt).run st1 = .ok (σ1, st2) →
      (ownedIn σ1).run st2 = .ok (ds, st2) →
      (record "before_return" id none ds (always := true)).run st2 = .ok ((), st3) →
      PStmt (n + 1) Γ σ base (.ret e) st π .exit [recEv st2 st3 σ1 (delta σ σ1 π)]
  | brk {n Γ σ base st π id st1 ds st2} :
      stmtId.run st = .ok (id, st1) →
      (ownedIn (List.take (σ.length - base) σ)).run st1 = .ok (ds, st1) →
      (record "before_break" id none ds).run st1 = .ok ((), st2) →
      PStmt (n + 1) Γ σ base .brk st π (.brk π)
        [recEv st1 st2 (List.take (σ.length - base) σ) (List.take (σ.length - base) π)]
  | cont {n Γ σ base st π id st1 ds st2} :
      stmtId.run st = .ok (id, st1) →
      (ownedIn (List.take (σ.length - base) σ)).run st1 = .ok (ds, st1) →
      (record "before_continue" id none ds).run st1 = .ok ((), st2) →
      PStmt (n + 1) Γ σ base .cont st π (.cont π)
        [recEv st1 st2 (List.take (σ.length - base) σ) (List.take (σ.length - base) π)]
  | debuglog_fin {n Γ σ base e st π id st1 σ1 st2} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ true σ e)).run st1 = .ok (σ1, st2) →
      PStmt (n + 1) Γ σ base (.debuglog e) st π (.norm (delta σ σ1 π)) []
  | debuglog_exit {n Γ σ base e st π id st1 σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ true σ e)).run st1 = .ok (σ1, st2) →
      exprLeaves e = true → Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.debuglog e) st π .exit [tryEv st1 st2 σ πk]
  | call_fin {n Γ σ base e st π id st1 σ1 st2} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      PStmt (n + 1) Γ σ base (.call e) st π (.norm (delta σ σ1 π)) []
  | call_exit {n Γ σ base e st π id st1 σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σ1, st2) →
      exprLeaves e = true → Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.call e) st π .exit [tryEv st1 st2 σ πk]
  | const_fin {n Γ σ base x t e st π id st1 σ1 st2 σ2 st3} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs { Γ with constExpr := true } false σ e)).run st1 = .ok (σ1, st2) →
      (bindM ((checkStmtF env n Γ (.const_ x t e)).run #[]).1 σ1 [x] true (konst := true)).run st2 = .ok (σ2, st3) →
      PStmt (n + 1) Γ σ base (.const_ x t e) st π (.norm (newFlags σ1 σ2 ++ delta σ σ1 π)) []
  | const_exit {n Γ σ base x t e st π id st1 σ1 st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs { Γ with constExpr := true } false σ e)).run st1 = .ok (σ1, st2) →
      exprLeaves e = true → Between (delta σ σ1 π) π πk →
      PStmt (n + 1) Γ σ base (.const_ x t e) st π .exit [tryEv st1 st2 σ πk]

  | if_exit {n Γ σ base c tB eB t e st π id st1 σc st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ c (mE sigs Γ false σ c)).run st1 = .ok (σc, st2) →
      exprLeaves c = true → Between (delta σ σc π) π πk →
      PStmt (n + 1) Γ σ base (.if_ c tB eB t e) st π .exit [tryEv st1 st2 σ πk]
  | if_then {n Γ σ base c tB eB t e st π id st1 σc st2 out evs} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ c (mE sigs Γ false σ c)).run st1 = .ok (σc, st2) →
      PBranch n (condCtx env n Γ c tB false) σc tB.toList base t id "then_branch_exit" none st2
        (delta σ σc π) out evs →
      PStmt (n + 1) Γ σ base (.if_ c tB eB t e) st π out evs
  | if_else {n Γ σ base c tB eB t eb st π id st1 σc st2 rt st3 out evs} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ c (mE sigs Γ false σ c)).run st1 = .ok (σc, st2) →
      (mBranch (condCtx env n Γ c tB false) σc tB.toList id "then_branch_exit" none fun σb =>
        mBlock sigs env n (condCtx env n Γ c tB false) σb base t).run st2 = .ok (rt, st3) →
      PBranch n (condCtx env n Γ c eB true) σc eB.toList base eb id "else_branch_exit" none st3
        (delta σ σc π) out evs →
      PStmt (n + 1) Γ σ base (.if_ c tB eB t (some eb)) st π out evs
  | if_skip {n Γ σ base c tB eB t st π id st1 σc st2} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ c (mE sigs Γ false σ c)).run st1 = .ok (σc, st2) →
      PStmt (n + 1) Γ σ base (.if_ c tB eB t none) st π (.norm (delta σ σc π)) []
  | match_exit {n Γ σ base e cases dflt st π id st1 σc st2 πk} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σc, st2) →
      exprLeaves e = true → Between (delta σ σc π) π πk →
      PStmt (n + 1) Γ σ base (.match_ e cases dflt) st π .exit [tryEv st1 st2 σ πk]
  | match_arm {n Γ σ base e cases dflt st π id st1 σc st2 out evs} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σc, st2) →
      PCases n Γ (matchVariants env n Γ e) σc base id 0 cases st2 (delta σ σc π) out evs →
      PStmt (n + 1) Γ σ base (.match_ e cases dflt) st π out evs
  | match_dflt {n Γ σ base e cases db st π id st1 σc st2 r st3 out evs} :
      stmtId.run st = .ok (id, st1) →
      (leaving id σ e (mE sigs Γ false σ e)).run st1 = .ok (σc, st2) →
      (mCases sigs env n Γ (matchVariants env n Γ e) σc base id 0 cases).run st2 = .ok (r, st3) →
      PBranch n Γ σc [] base db id "match_arm_exit" (some cases.length) st3 (delta σ σc π) out evs →
      PStmt (n + 1) Γ σ base (.match_ e cases (some db)) st π out evs
  | loop_ {n Γ σ base c body st π id st1 σc st2 r st3 st4 st5 k out evs} :
      stmtId.run st = .ok (id, st1) →
      ((match c with
        | some c => leaving id σ c (mE sigs Γ false σ c)
        | none => pure σ) : MoveM MSt).run st1 = .ok (σc, st2) →
      (mBlock sigs env n { Γ with inLoop := true } σc σc.length body).run st2 = .ok (r, st3) →
      (loopHeld σc body r).run st3 = .ok ((), st4) →
      (record "loop_body_end" id none (ownedOf st4.held (scopeOwn σc r.1))).run st4 = .ok ((), st5) →
      PIter k (fun π evs => ∃ ce πk, c = some ce ∧ exprLeaves ce = true ∧ Between (delta σ σc π) π πk ∧
          evs = [tryEv st1 st2 σ πk])
        (fun π πn => c.isSome = true ∧ πn = delta σ σc π) (fun π πb => πb = delta σ σc π)
        n { Γ with inLoop := true } σc σc body st2 r st4 st5 π out evs →
      PStmt (n + 1) Γ σ base (.loop c body) st π out evs

/-- The paths through a block: its statements in order, until one does not
go on. -/
inductive PBlock : Nat → Ctx → MSt → Nat → List Stmt → DSt → List Bool → POut → List Ev → Prop where
  | nil {n Γ σ base st π} : PBlock (n + 1) Γ σ base [] st π (.norm π) []
  | cons_norm {n Γ σ base s ss st π π1 evs1 r1 st1 out evs2} :
      PStmt n Γ σ base s st π (.norm π1) evs1 →
      (mStmt sigs env n Γ σ base s).run st = .ok (r1, st1) →
      PBlock n ((checkStmtF env n Γ s).run #[]).1 r1.1 base ss st1 π1 out evs2 →
      PBlock (n + 1) Γ σ base (s :: ss) st π out (evs1 ++ evs2)
  | cons_stop {n Γ σ base s ss st π out evs} :
      PStmt n Γ σ base s st π out evs → out.notNorm →
      PBlock (n + 1) Γ σ base (s :: ss) st π out evs

/-- The paths through a branch, an arm or a default (`mBranch`): its
bindings, its block, and, where the block goes on, the end of its scope. -/
inductive PBranch : Nat → Ctx → MSt → List String → Nat → List Stmt → Nat → String → Option Nat → DSt →
    List Bool → POut → List Ev → Prop where
  | norm {n Γb σc binds base body id kind arm st π σb st1 rb st2 ds st3 πe evs} :
      (bindM Γb σc binds true).run st = .ok (σb, st1) →
      (mBlock sigs env n Γb σb base body).run st1 = .ok (rb, st2) →
      (ownedIn (scopeOwn σc rb.1)).run st2 = .ok (ds, st2) →
      (record kind id arm ds).run st2 = .ok ((), st3) →
      PBlock n Γb σb base body st1 (newFlags σc σb ++ π) (.norm πe) evs →
      PBranch n Γb σc binds base body id kind arm st π (.norm (πe.drop (πe.length - σc.length)))
        (evs ++ [recEv st2 st3 (scopeOwn σc rb.1) (πe.take (πe.length - σc.length))])
  | other {n Γb σc binds base body id kind arm st π σb st1 out evs} :
      (bindM Γb σc binds true).run st = .ok (σb, st1) →
      PBlock n Γb σb base body st1 (newFlags σc σb ++ π) out evs → out.notNorm →
      PBranch n Γb σc binds base body id kind arm st π out evs

/-- The paths through a `match`'s cases: an arm, or past it to a later
one. -/
inductive PCases : Nat → Ctx → List (String × Option Ty) → MSt → Nat → Nat → Nat → List Case → DSt →
    List Bool → POut → List Ev → Prop where
  | here_atom {n Γ vs σ base id i a b cs st π out evs} :
      PBranch n Γ σ [] base b id "match_arm_exit" (some i) st π out evs →
      PCases (n + 1) Γ vs σ base id i (.atom a b :: cs) st π out evs
  | here_term {n Γ vs σ base id i a x b cs st π out evs} :
      PBranch n (termCtx Γ vs a x) σ [x] base b id "match_arm_exit" (some i) st π out evs →
      PCases (n + 1) Γ vs σ base id i (.term a x b :: cs) st π out evs
  | later_atom {n Γ vs σ base id i a b cs st π rb st1 out evs} :
      (mBranch Γ σ [] id "match_arm_exit" (some i) fun σb => mBlock sigs env n Γ σb base b).run st
        = .ok (rb, st1) →
      PCases n Γ vs σ base id (i + 1) cs st1 π out evs →
      PCases (n + 1) Γ vs σ base id i (.atom a b :: cs) st π out evs
  | later_term {n Γ vs σ base id i a x b cs st π rb st1 out evs} :
      (mBranch (termCtx Γ vs a x) σ [x] id "match_arm_exit" (some i) fun σb =>
        mBlock sigs env n (termCtx Γ vs a x) σb base b).run st = .ok (rb, st1) →
      PCases n Γ vs σ base id (i + 1) cs st1 π out evs →
      PCases (n + 1) Γ vs σ base id i (.term a x b :: cs) st π out evs

/-- The paths through a loop's passes, `k` of them: from the loop's start,
holding `π`, its head (`HX` a way out of the function there, `HN` out of
the loop, `HB` into the body); then the body, from `σb`, and the end of the
body's scope, back to the start, or a `break` out of the loop, a `continue`
back to the start, or a way out of the function. -/
inductive PIter : Nat → (List Bool → List Ev → Prop) → (List Bool → List Bool → Prop) →
    (List Bool → List Bool → Prop) → Nat → Ctx → MSt → MSt → List Stmt → DSt → MOut → DSt → DSt →
    List Bool → POut → List Ev → Prop where
  | head_exit {k HX HN HB n Γb σc σb body st2 r st4 st5 π evs} :
      HX π evs → PIter k HX HN HB n Γb σc σb body st2 r st4 st5 π .exit evs
  | natural {k HX HN HB n Γb σc σb body st2 r st4 st5 π πn} :
      HN π πn → PIter k HX HN HB n Γb σc σb body st2 r st4 st5 π (.norm πn) []
  | body_norm {k HX HN HB n Γb σc σb body st2 r st4 st5 π πb πe evs1 out evs2} :
      HB π πb → PBlock n Γb σb σc.length body st2 πb (.norm πe) evs1 →
      PIter k HX HN HB n Γb σc σb body st2 r st4 st5 (πe.drop (πe.length - σc.length)) out evs2 →
      PIter (k + 1) HX HN HB n Γb σc σb body st2 r st4 st5 π out
        (evs1 ++ recEv st4 st5 (scopeOwn σc r.1) (πe.take (πe.length - σc.length)) :: evs2)
  | body_brk {k HX HN HB n Γb σc σb body st2 r st4 st5 π πb πx evs} :
      HB π πb → PBlock n Γb σb σc.length body st2 πb (.brk πx) evs →
      PIter k HX HN HB n Γb σc σb body st2 r st4 st5 π (.norm (πx.drop (πx.length - σc.length))) evs
  | body_cont {k HX HN HB n Γb σc σb body st2 r st4 st5 π πb πx evs1 out evs2} :
      HB π πb → PBlock n Γb σb σc.length body st2 πb (.cont πx) evs1 →
      PIter k HX HN HB n Γb σc σb body st2 r st4 st5 (πx.drop (πx.length - σc.length)) out evs2 →
      PIter (k + 1) HX HN HB n Γb σc σb body st2 r st4 st5 π out (evs1 ++ evs2)
  | body_exit {k HX HN HB n Γb σc σb body st2 r st4 st5 π πb evs} :
      HB π πb → PBlock n Γb σb σc.length body st2 πb .exit evs →
      PIter k HX HN HB n Γb σc σb body st2 r st4 st5 π .exit evs

end

end

section
variable {sigs : List (String × Sig)} {env : InferEnv}
set_option maxHeartbeats 2000000

/-- A loop's passes leave it going on or out of the function, never by a
`break` or a `continue` of a loop around it. -/
theorem piter_out {HX HN HB n Γb σc σb body st2 r st4 st5} : ∀ (k : Nat) {π out evs},
    PIter sigs env k HX HN HB n Γb σc σb body st2 r st4 st5 π out evs →
    out = .exit ∨ ∃ π', out = .norm π' := by
  intro k
  induction k with
  | zero =>
    intro π out evs hp
    cases hp with
    | head_exit => exact .inl rfl
    | natural => exact .inr ⟨_, rfl⟩
    | body_brk => exact .inr ⟨_, rfl⟩
    | body_exit => exact .inl rfl
  | succ k ih =>
    intro π out evs hp
    cases hp with
    | head_exit => exact .inl rfl
    | natural => exact .inr ⟨_, rfl⟩
    | body_norm _ _ hn => exact ih hn
    | body_brk => exact .inr ⟨_, rfl⟩
    | body_cont _ _ hn => exact ih hn
    | body_exit => exact .inl rfl

mutual

/-- A path that leaves a statement by a `break`: the statement holds one
belonging to the loop it is in (`hasBreakS`). -/
theorem pstmt_breaks {n Γ σ base s st π π' evs} (hp : PStmt sigs env n Γ σ base s st π (.brk π') evs) :
    hasBreakS s = true := by
  cases hp with
  | brk => rfl
  | if_then _ _ hb => simp [hasBreakS, pbranch_breaks hb]
  | if_else _ _ _ hb => simp [hasBreakS, hasBreakElse, pbranch_breaks hb]
  | match_arm _ _ hc => simp [hasBreakS, pcases_breaks hc]
  | match_dflt _ _ _ hb => simp [hasBreakS, hasBreakElse, pbranch_breaks hb]
  | loop_ _ _ _ _ _ hi =>
    rcases piter_out _ hi with h | ⟨_, h⟩ <;> cases h

theorem pblock_breaks {n Γ σ base ss st π π' evs} (hp : PBlock sigs env n Γ σ base ss st π (.brk π') evs) :
    hasBreak ss = true := by
  cases hp with
  | cons_norm _ _ h2 => simp [hasBreak, pblock_breaks h2]
  | cons_stop h1 _ => simp [hasBreak, pstmt_breaks h1]

theorem pbranch_breaks {n Γb σc binds base body id kind arm st π π' evs}
    (hp : PBranch sigs env n Γb σc binds base body id kind arm st π (.brk π') evs) : hasBreak body = true := by
  cases hp with
  | other _ hb _ => exact pblock_breaks hb

theorem pcases_breaks {n Γ vs σ base id i cs st π π' evs}
    (hp : PCases sigs env n Γ vs σ base id i cs st π (.brk π') evs) : casesBreak cs = true := by
  cases hp with
  | here_atom hb => simp [casesBreak, pbranch_breaks hb]
  | here_term hb => simp [casesBreak, pbranch_breaks hb]
  | later_atom _ hc => simp [casesBreak, pcases_breaks hc]
  | later_term _ hc => simp [casesBreak, pcases_breaks hc]

end

/-- A path that goes on past a loop: it left by the loop's head, or by a
`break` its body holds. -/
theorem piter_norm {HX HN HB n Γb σc σb body st2 r st4 st5} : ∀ (k : Nat) {π π' evs},
    PIter sigs env k HX HN HB n Γb σc σb body st2 r st4 st5 π (.norm π') evs →
    (∃ π πn, HN π πn) ∨ hasBreak body = true := by
  intro k
  induction k with
  | zero =>
    intro π π' evs hp
    cases hp with
    | natural h => exact .inl ⟨_, _, h⟩
    | body_brk _ hb => exact .inr (pblock_breaks hb)
  | succ k ih =>
    intro π π' evs hp
    cases hp with
    | natural h => exact .inl ⟨_, _, h⟩
    | body_norm _ _ hn => exact ih hn
    | body_brk _ hb => exact .inr (pblock_breaks hb)
    | body_cont _ _ hn => exact ih hn

mutual

/-- A path that goes on past a statement: the statement completes, as the
analysis reads it syntactically (`completesS`). -/
theorem pstmt_completes {n Γ σ base s st π π' evs} (hp : PStmt sigs env n Γ σ base s st π (.norm π') evs) :
    completesS s = true := by
  cases hp with
  | @if_then _ _ _ _ c tB eB t e _ _ _ _ _ _ _ _ _ _ hb =>
    have := pbranch_completes hb
    cases e with
    | none => simp [completesS]
    | some eb => simp [completesS, this]
  | if_else _ _ _ hb => have := pbranch_completes hb; simp [completesS, this]
  | if_skip => simp [completesS]
  | match_arm _ _ hc => have := pcases_completes hc; simp [completesS, this]
  | match_dflt _ _ _ hb => have := pbranch_completes hb; simp [completesS, completesElse, this]
  | let_fin => simp [completesS]
  | var_fin => simp [completesS]
  | var_bare => simp [completesS]
  | var_none => simp [completesS]
  | fun_ => simp [completesS]
  | type_ => simp [completesS]
  | set_fin => simp [completesS]
  | debuglog_fin => simp [completesS]
  | call_fin => simp [completesS]
  | const_fin => simp [completesS]
  | @loop_ _ _ _ _ c body _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ hi =>
    rcases piter_norm _ hi with ⟨_, _, h, _⟩ | h
    · cases c with
      | some _ => rfl
      | none => cases h
    · cases c with
      | some _ => rfl
      | none => simpa [completesS] using h

theorem pblock_completes {n Γ σ base ss st π π' evs} (hp : PBlock sigs env n Γ σ base ss st π (.norm π') evs) :
    completes ss = true := by
  cases hp with
  | nil => rfl
  | cons_norm h1 _ h2 => simp [completes, pstmt_completes h1, pblock_completes h2]
  | cons_stop _ hn => exact hn.elim

theorem pbranch_completes {n Γb σc binds base body id kind arm st π π' evs}
    (hp : PBranch sigs env n Γb σc binds base body id kind arm st π (.norm π') evs) : completes body = true := by
  cases hp with
  | norm _ _ _ _ hb => exact pblock_completes hb
  | other _ _ hn => exact hn.elim

theorem pcases_completes {n Γ vs σ base id i cs st π π' evs}
    (hp : PCases sigs env n Γ vs σ base id i cs st π (.norm π') evs) : casesComplete cs = true := by
  cases hp with
  | here_atom hb => simp [casesComplete, pbranch_completes hb]
  | here_term hb => simp [casesComplete, pbranch_completes hb]
  | later_atom _ hc => simp [casesComplete, pcases_completes hc]
  | later_term _ hc => simp [casesComplete, pcases_completes hc]

end

end

theorem MSt.Ids.toSame : ∀ {a b : MSt}, MSt.Ids a b → a.Same b
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons u o t => .cons u o (MSt.Ids.toSame t)

theorem mem_take_mono {α} {v : α} : ∀ {l : List α} {k m : Nat}, v ∈ List.take k l → k ≤ m → v ∈ List.take m l
  | [], _, _, h, _ => by simp at h
  | x :: l, k, m, h, hkm => by
    cases k with
    | zero => simp at h
    | succ k =>
      cases m with
      | zero => omega
      | succ m =>
        simp only [List.take_succ_cons, List.mem_cons] at h ⊢
        rcases h with h | h
        · exact .inl h
        · exact .inr (mem_take_mono h (by omega))

theorem Inv.toMade {F st : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ : MSt} {π : List Bool}
    (h : Inv F st U H base σ π) : Made σ st := fun v hv ho => h.lt v hv ho

/-- What a path through a statement ends with, as the walk's result there
says: on to what follows, with what it carries; at a `break` or a
`continue`, with what it carries at one of the walk's; or out of the
function. -/
def OutOk (F st : DSt) (U : Nat) (H : Nat → Prop) (base : Nat) (r : MOut) : POut → Prop
  | .norm π => Inv F st U H base r.1 π
  | .brk π => ∃ τ, τ ∈ r.2.1 ∧ Inv F st U H base τ π
  | .cont π => ∃ τ, τ ∈ r.2.2 ∧ Inv F st U H base τ π
  | .exit => True

/-- What a path through a `match`'s cases ends with: on to what follows,
with what it carries at an arm that completes; at a `break` or a
`continue`; or out of the function. -/
def CasesOk (F st : DSt) (U : Nat) (H : Nat → Prop) (base : Nat)
    (r : List (MSt × Bool) × List MSt × List MSt) : POut → Prop
  | .norm π => ∃ a, a ∈ r.1 ∧ a.2 = true ∧ Inv F st U H base a.1 π
  | .brk π => ∃ τ, τ ∈ r.2.1 ∧ Inv F st U H base τ π
  | .cont π => ∃ τ, τ ∈ r.2.2 ∧ Inv F st U H base τ π
  | .exit => True

theorem MoveM.det {α} {m : MoveM α} {st : DSt} {a b : α × DSt} (h1 : m.run st = .ok a) (h2 : m.run st = .ok b) :
    a = b := by
  rw [h1] at h2; cases h2; rfl

/-- The steps of a `set`'s walk. -/
theorem mSet_decomp {sigs : List (String × Sig)} {Γ : Ctx} {id : Nat} {σ σ' : MSt} {x : String}
    {steps : List Step} {op : Option BinOp} {e : Expr} {st st' : DSt}
    (h : (mSet sigs Γ id σ x steps op e).run st = .ok (σ', st')) :
    ∃ sta stb σ1 st2 σ2 st3,
      (unwalkedM σ x).run st = .ok ((), sta) ∧ (writableM σ x).run sta = .ok ((), stb) ∧
      (leavingIf id σ (exprLeaves e || (steps.any fun | .index k _ => exprLeaves k | _ => false) ||
        op.any BinOp.exits) (mSetParts sigs Γ σ steps op e)).run stb = .ok (σ1, st2) ∧
      (mTarget σ1 x steps op).run st2 = .ok (σ2, st3) ∧
      ((if stepsExit steps then do
          record "before_set_target_early_return" id none (← ownedIn σ2) (always := true)
        else pure ()) : MoveM Unit).run st3 = .ok ((), st') ∧
      σ' = (if steps.isEmpty then σ2.give x else σ2) := by
  unfold mSet at h
  obtain ⟨_, sta, ha, h⟩ := MoveM.bind_ok h
  obtain ⟨_, stb, hb, h⟩ := MoveM.bind_ok h
  obtain ⟨σ1, st2, h1, h⟩ := MoveM.bind_ok h
  obtain ⟨σ2, st3, h2, h⟩ := MoveM.bind_ok h
  obtain ⟨_, st4, h3, h⟩ := MoveM.bind_ok h
  obtain ⟨σ4, st5, h4, h⟩ := MoveM.bind_ok h
  have e4 : σ4 = σ2 ∧ st5 = st4 := by
    split at h4
    · exact exitM_eq h4
    · cases h4; exact ⟨rfl, rfl⟩
  simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
  obtain ⟨rfl, rfl⟩ := h
  obtain ⟨rfl, rfl⟩ := e4
  exact ⟨sta, stb, σ1, st2, σ4, st3, ha, hb, h1, h2, h3, rfl⟩

section
variable (sigs : List (String × Sig)) (env : InferEnv)
set_option maxHeartbeats 4000000

mutual

theorem stmt_ok {n : Nat} {Γ : Ctx} {σ : MSt} {base : Nat} {s : Stmt} {st : DSt} {π : List Bool} {out : POut}
    {evs : List Ev} (hp : PStmt sigs env n Γ σ base s st π out evs) {r : MOut} {st' F : DSt} {U : Nat}
    {H : Nat → Prop} (hw : (mStmt sigs env n Γ σ base s).run st = .ok (r, st')) (w : st.WF)
    (inv : Inv F st U H base σ π) (hF : SchedExt st' F) :
    (∀ ev, ev ∈ evs → ev.Good F) ∧ OutOk F st' U H base r out := by
  cases hp with
  | let_fin hid h2 h3 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨σ2', st3', h3', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h3 h3')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ _) h2
    refine ⟨(fun _ h => by cases h), ?_⟩
    exact (inv.same (x1.trans x2) (MSt.Same.of_below b2)).bind x2.wf h3
  | let_exit hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨σ2', st3', h3', hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have x3 := SchedGrows.bindM _ _ _ _ _ _ _ _ _ x2.wf h3'
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 (x3.trans hF) x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans (x3.trans hF)))) hb
  | var_fin hid h2 h3 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨σ2', st3', h3', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h3 h3')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ _) h2
    refine ⟨(fun _ h => by cases h), ?_⟩
    exact (inv.same (x1.trans x2) (MSt.Same.of_below b2)).bind x2.wf h3
  | var_exit hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨σ2', st3', h3', hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have x3 := SchedGrows.bindM _ _ _ _ _ _ _ _ _ x2.wf h3'
    have xF := x3.trans hF
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | debuglog_fin hid h2 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ true σ _)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ true σ _) h2
    refine ⟨(fun _ h => by cases h), ?_⟩
    exact inv.same (x1.trans x2) (MSt.Same.of_below b2)
  | debuglog_exit hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ true σ _)) _ _ _ x1.wf h2
    have xF := hF
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    exact tryEv_good (mE_shr sigs Γ true σ _) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | call_fin hid h2 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ _) h2
    refine ⟨(fun _ h => by cases h), ?_⟩
    exact inv.same (x1.trans x2) (MSt.Same.of_below b2)
  | call_exit hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have xF := hF
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | const_fin hid h2 h3 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨σ2', st3', h3', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h3 h3')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs _ false σ _)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs _ false σ _) h2
    refine ⟨(fun _ h => by cases h), ?_⟩
    exact (inv.same (x1.trans x2) (MSt.Same.of_below b2)).bind x2.wf h3
  | const_exit hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨σ2', st3', h3', hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs _ false σ _)) _ _ _ x1.wf h2
    have x3 := SchedGrows.bindM _ _ _ _ _ _ _ _ _ x2.wf h3'
    have xF := x3.trans hF
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    exact tryEv_good (mE_shr sigs _ false σ _) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | var_bare hid h2 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨_, st3', h3', hw⟩ := MoveM.bind_ok hw
    simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h3'
    cases h3'
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.bindM _ _ _ _ _ _ _ _ _ x1.wf h2
    refine ⟨(fun _ h => by cases h), ?_⟩
    exact ((inv.ext x1).bind x1.wf h2).ext (SchedExt.addTracked x2.wf _)
  | var_none =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    dsimp only at hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    exact ⟨(fun _ h => by cases h), inv.ext (SchedGrows.stmtId st _ _ w hid')⟩
  | fun_ =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    dsimp only at hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    exact ⟨(fun _ h => by cases h), inv.ext (SchedGrows.stmtId st _ _ w hid')⟩
  | type_ =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    dsimp only at hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    exact ⟨(fun _ h => by cases h), inv.ext (SchedGrows.stmtId st _ _ w hid')⟩
  | brk hid h2 h3 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨ds', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨_, st3', h3', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h3 h3')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x3 := SchedGrows.record _ _ _ _ _ _ _ _ x1.wf h3
    obtain ⟨-, rfl⟩ := ownedIn_run h2
    refine ⟨fun ev hev => ?_, ⟨_, .head _, (inv.ext x1).ext x3⟩⟩
    simp only [List.mem_singleton] at hev
    subst hev
    exact recEv_good h3 hF x1.wf (inv.rel.take _)
      (.inr ⟨x1.held, fun v hv ho hH => absurd (inv.hU _ hH) (Nat.not_lt.mpr (inv.scope v hv ho))⟩)
  | cont hid h2 h3 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨ds', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨_, st3', h3', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h3 h3')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x3 := SchedGrows.record _ _ _ _ _ _ _ _ x1.wf h3
    obtain ⟨-, rfl⟩ := ownedIn_run h2
    refine ⟨fun ev hev => ?_, ⟨_, .head _, (inv.ext x1).ext x3⟩⟩
    simp only [List.mem_singleton] at hev
    subst hev
    exact recEv_good h3 hF x1.wf (inv.rel.take _)
      (.inr ⟨x1.held, fun v hv ho hH => absurd (inv.hU _ hH) (Nat.not_lt.mpr (inv.scope v hv ho))⟩)
  | @set_fin _ _ _ _ x steps op e _ _ id st1 st1a st1b σ1 st2 σ2 st3 hid ha hb h1 h2 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ', st2', hs, hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    obtain ⟨sta, stb, σ1', st2'', σ2', st3', ha', hb', h1', h2', h3', rfl⟩ := mSet_decomp hs
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det ha ha')
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hb hb')
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h1 h1')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have xa := SchedGrows.keepsSched (KeepsSched.unwalkedM σ _) _ _ _ x1.wf ha
    have xb := SchedGrows.keepsSched (KeepsSched.writableM σ _) _ _ _ xa.wf hb
    have x2 := SchedGrows.leavingIf _ σ _ (SchedGrows.shr (mSetParts_shr sigs Γ σ _ _ _)) _ _ _ xb.wf h1
    have b1 := leavingIf_below (mSetParts_shr sigs Γ σ _ _ _) h1
    have x3 := SchedGrows.shr (mTarget_shr _ _ _ _) _ _ _ x2.wf h2'
    have b2 := ((mTarget_shr _ _ _ _) _ _ _ h2').1
    have x4 : SchedExt st3' st2' := by
      by_cases hse' : stepsExit steps = true
      · simp only [hse', ite_true] at h3'
        exact (SchedGrows.bind (SchedGrows.ownedIn _) fun _ => SchedGrows.record _ _ _ _ _) _ _ _ x3.wf h3'
      · simp only [Bool.not_eq_true] at hse'
        simp only [hse', Bool.false_eq_true, ite_false] at h3'
        cases h3'; exact SchedExt.refl x3.wf
    have x1b := x1.trans (xa.trans xb)
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    refine ⟨(fun _ h => by cases h), ?_⟩
    have sm : MSt.Same (if steps.isEmpty then σ2.give x else σ2) σ := by
      have s2 : σ2.Same σ := (MSt.Same.of_below b2).trans (MSt.Same.of_below b1)
      split
      · exact (MSt.Same.update (f := fun v => { v with init := true, live := true })
          (fun _ => ⟨rfl, rfl⟩) _ σ2).trans s2
      · exact s2
    exact inv.same (x1b.trans (x2.trans (x3.trans x4))) sm
  | @set_try _ _ _ _ x steps op e _ _ id st1 st1a st1b σ1 st2 πk hid ha hb h1 hl hbt =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ', st2', hs, hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    obtain ⟨sta, stb, σ1', st2'', σ2', st3', ha', hb', h1', h2', h3', rfl⟩ := mSet_decomp hs
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det ha ha')
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hb hb')
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h1 h1')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have xa := SchedGrows.keepsSched (KeepsSched.unwalkedM σ _) _ _ _ x1.wf ha
    have xb := SchedGrows.keepsSched (KeepsSched.writableM σ _) _ _ _ xa.wf hb
    have x2 := SchedGrows.leavingIf _ σ _ (SchedGrows.shr (mSetParts_shr sigs Γ σ _ _ _)) _ _ _ xb.wf h1
    have b1 := leavingIf_below (mSetParts_shr sigs Γ σ _ _ _) h1
    have x3 := SchedGrows.shr (mTarget_shr _ _ _ _) _ _ _ x2.wf h2'
    have b2 := ((mTarget_shr _ _ _ _) _ _ _ h2').1
    have x4 : SchedExt st3' st2' := by
      by_cases hse' : stepsExit steps = true
      · simp only [hse', ite_true] at h3'
        exact (SchedGrows.bind (SchedGrows.ownedIn _) fun _ => SchedGrows.record _ _ _ _ _) _ _ _ x3.wf h3'
      · simp only [Bool.not_eq_true] at hse'
        simp only [hse', Bool.false_eq_true, ite_false] at h3'
        cases h3'; exact SchedExt.refl x3.wf
    have x1b := x1.trans (xa.trans xb)
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    rw [hl] at h1
    have xF := x3.trans (x4.trans hF)
    exact tryEv_good (mSetParts_shr sigs Γ σ _ _ _) h1 xF xb.wf inv.rel
      (inv.madeBefore (x1b.trans (x2.trans xF))) hbt
  | @set_target _ _ _ _ x steps op e _ _ id st1 st1a st1b σ1 st2 σ2 st3 ds st4 hid ha hb h1 h2 hse ho hr =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ', st2', hs, hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    obtain ⟨sta, stb, σ1', st2'', σ2', st3', ha', hb', h1', h2', h3', rfl⟩ := mSet_decomp hs
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det ha ha')
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hb hb')
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h1 h1')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have xa := SchedGrows.keepsSched (KeepsSched.unwalkedM σ _) _ _ _ x1.wf ha
    have xb := SchedGrows.keepsSched (KeepsSched.writableM σ _) _ _ _ xa.wf hb
    have x2 := SchedGrows.leavingIf _ σ _ (SchedGrows.shr (mSetParts_shr sigs Γ σ _ _ _)) _ _ _ xb.wf h1
    have b1 := leavingIf_below (mSetParts_shr sigs Γ σ _ _ _) h1
    have x3 := SchedGrows.shr (mTarget_shr _ _ _ _) _ _ _ x2.wf h2'
    have b2 := ((mTarget_shr _ _ _ _) _ _ _ h2').1
    have x4 : SchedExt st3' st2' := by
      by_cases hse' : stepsExit steps = true
      · simp only [hse', ite_true] at h3'
        exact (SchedGrows.bind (SchedGrows.ownedIn _) fun _ => SchedGrows.record _ _ _ _ _) _ _ _ x3.wf h3'
      · simp only [Bool.not_eq_true] at hse'
        simp only [hse', Bool.false_eq_true, ite_false] at h3'
        cases h3'; exact SchedExt.refl x3.wf
    have x1b := x1.trans (xa.trans xb)
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    simp only [hse, ite_true] at h3'
    obtain ⟨ds', st4', ho', hr'⟩ := MoveM.bind_ok h3'
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det ho ho')
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hr hr')
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    obtain ⟨-, rfl⟩ := ownedIn_run ho
    have s2 : σ2.Same σ := (MSt.Same.of_below b2).trans (MSt.Same.of_below b1)
    have inv3 := inv.same (x1b.trans (x2.trans x3)) s2
    refine recEv_good hr hF x3.wf inv3.rel (.inl ⟨rfl, rfl, fun v hv ho => ?_⟩)
    obtain ⟨w', hw', u', o'⟩ := s2.mem hv
    rw [u']
    exact inv.madeBefore (x1b.trans (x2.trans (x3.trans (x4.trans hF)))) w' hw' (o' ▸ ho)
  | ret_try hid hex h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ0, st1'', hex', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hex hex')
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨ds, st3', ho, hw⟩ := MoveM.bind_ok hw
    obtain ⟨_, st4', hr, hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ _)) _ _ _ x1.wf h2
    have x3 := SchedGrows.ownedIn _ _ _ _ x2.wf ho
    have x4 := SchedGrows.record _ _ _ _ _ _ _ _ x3.wf hr
    have xF := x3.trans (x4.trans hF)
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | @ret_fin _ _ _ _ e _ _ _ st1 σ1 st2 _ _ hid hex h2 ho hr =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σ0, st1'', hex', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hex hex')
    obtain ⟨σ1', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨ds', st3', ho', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det ho ho')
    obtain ⟨_, st4', hr', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hr hr')
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have rs : MSt.Same σ1 σ ∧ SchedExt st1 st2 := by
      cases e with
      | some e =>
        exact ⟨MSt.Same.of_below (leavingIf_below (mE_shr sigs Γ false σ e) h2),
          SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ e)) _ _ _ x1.wf h2⟩
      | none => cases h2; exact ⟨MSt.Same.refl _, SchedExt.refl x1.wf⟩
    obtain ⟨s1, x2⟩ := rs
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    obtain ⟨-, rfl⟩ := ownedIn_run ho
    have inv2 := inv.same (x1.trans x2) s1
    refine recEv_good hr hF x2.wf inv2.rel (.inl ⟨rfl, rfl, fun v hv ho => ?_⟩)
    obtain ⟨w', hw', u', o'⟩ := s1.mem hv
    rw [u']
    exact inv.madeBefore (x1.trans (x2.trans ((SchedGrows.record _ _ _ _ _ _ _ _ x2.wf hr).trans hF))) w' hw'
      (o' ▸ ho)
  | @loop_ _ _ _ _ c body _ _ id st1 σc st2 r st3 st4 st5 k _ _ hid h2 hb hl hrec hi =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    obtain ⟨r', st3', hb', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hb hb')
    obtain ⟨_, st4', hl', hw⟩ := MoveM.bind_ok hw
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hl hl')
    obtain ⟨ds, st4'', ho, hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := ownedIn_run ho
    obtain ⟨_, st5', hrec', hw⟩ := MoveM.bind_ok hw
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hrec hrec')
    obtain ⟨σ', st6, hend, hw⟩ := MoveM.bind_ok hw
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    have x1 := SchedGrows.stmtId st _ _ w hid
    have rs : σc.Below σ ∧ SchedExt st1 st2 := by
      cases c with
      | some ce =>
        exact ⟨leavingIf_below (mE_shr sigs Γ false σ ce) h2,
          SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ ce)) _ _ _ x1.wf h2⟩
      | none => cases h2; exact ⟨MSt.Below.refl _, SchedExt.refl x1.wf⟩
    obtain ⟨bc, x2⟩ := rs
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below bc)
    obtain ⟨x3, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ x2.wf invc.toMade hb
    have x4 := loopHeld_ext x3.wf (invc.toMade.ext x3) hl
    have x5 := SchedGrows.record _ _ _ _ _ _ _ _ x4.wf hrec
    obtain ⟨rfl, -⟩ := loopEnd_eq hend
    have xs4 := x1.trans (x2.trans (x3.trans x4))
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine iter_ok (Ub := st2.uid) hi hb hl hrec hend x2.wf hF invc.toMade invc.toMade (Nat.le_refl _)
      (by rw [sfx_self]; exact MSt.Ids.refl _) inv (xs4.trans x5) bc (fun _ _ h => h.1) ?_
      (fun π πn hr h => h.2 ▸ Rel.step (fun _ h => h) hr bc) ?_ (inv.rel.mono xs4.held (fun _ h => h))
    · rintro π evs hr ⟨ce, πk, rfl, hle, hbw, rfl⟩ ev hev
      simp only [List.mem_singleton] at hev
      subst hev
      dsimp only at h2
      unfold leaving at h2
      rw [hle] at h2
      exact tryEv_good (mE_shr sigs Γ false σ ce) h2 (x3.trans (x4.trans (x5.trans hF))) x1.wf hr
        (inv.madeBefore (x1.trans (x2.trans (x3.trans (x4.trans (x5.trans hF)))))) hbw
    · rintro π πb hr rfl
      refine ⟨(Rel.step (fun _ h => h) hr bc).conv (fun v hv ho hd => .inr ⟨invc.lt v hv ho, hd⟩),
        fun u h => h.1, invc.made, fun v hv _ => by simp at hv, invc.lt, Nat.le_refl _, Nat.le_refl _⟩
  | @if_exit _ _ _ _ c tB eB t e _ _ id st1 σc st2 πk hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ c)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ c) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨rt, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨x3, _⟩ := mBranch_made x2.wf invc.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h3
    have it := (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h3).1
    obtain ⟨re, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ MSt.Ids (σc.back re.1) σc := by
      cases e with
      | some eb =>
        dsimp only at h4
        exact ⟨(mBranch_made x3.wf (invc.toMade.ext x3)
          (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h4).1,
          (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h4).1⟩
      | none =>
        cases h4; exact ⟨SchedExt.refl x3.wf, by rw [back_eq_sfx, sfx_self]; exact MSt.Ids.refl _⟩
    obtain ⟨x4, ie⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have te : (σc.back rt.1).Same (σc.back re.1) := (it.trans ie.symm).toSame
    have jl := joinIf_live h5 te
    obtain ⟨e5, -⟩ := joinIf_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    have xF := x3.trans (x4.trans hF)
    exact tryEv_good (mE_shr sigs Γ false σ c) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | @if_then _ _ _ _ c tB eB t e _ _ id st1 σc st2 out evs hid h2 hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ c)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ c) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨rt, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨x3, _⟩ := mBranch_made x2.wf invc.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h3
    have it := (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h3).1
    obtain ⟨re, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ MSt.Ids (σc.back re.1) σc := by
      cases e with
      | some eb =>
        dsimp only at h4
        exact ⟨(mBranch_made x3.wf (invc.toMade.ext x3)
          (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h4).1,
          (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h4).1⟩
      | none =>
        cases h4; exact ⟨SchedExt.refl x3.wf, by rw [back_eq_sfx, sfx_self]; exact MSt.Ids.refl _⟩
    obtain ⟨x4, ie⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have te : (σc.back rt.1).Same (σc.back re.1) := (it.trans ie.symm).toSame
    have jl := joinIf_live h5 te
    obtain ⟨e5, -⟩ := joinIf_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    obtain ⟨g, o⟩ := branch_ok hb h3 x2.wf invc (x4.trans hF)
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => exact (o.sameLive (jl.1 (pbranch_completes hb))).ext x4
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x4⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x4⟩
    | exit => trivial
  | @if_else _ _ _ _ c tB eB t eb _ _ id st1 σc st2 rtc st3c out evs hid h2 hrt hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ c)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ c) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨rt, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨erl, est⟩ := Prod.mk.inj (MoveM.det hrt h3)
    subst rtc; subst st3c
    obtain ⟨x3, _⟩ := mBranch_made x2.wf invc.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h3
    have it := (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h3).1
    obtain ⟨re, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ MSt.Ids (σc.back re.1) σc := by
      try dsimp only at h4
      exact ⟨(mBranch_made x3.wf (invc.toMade.ext x3)
        (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h4).1,
        (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h4).1⟩
    obtain ⟨x4, ie⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have te : (σc.back rt.1).Same (σc.back re.1) := (it.trans ie.symm).toSame
    have jl := joinIf_live h5 te
    obtain ⟨e5, -⟩ := joinIf_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    try dsimp only at h4
    obtain ⟨g, o⟩ := branch_ok hb h4 x3.wf (invc.ext x3) hF
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => exact o.sameLive (jl.2 (by simpa [Option.all] using pbranch_completes hb))
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | exit => trivial
  | @if_skip _ _ _ _ c tB eB t _ _ id st1 σc st2 hid h2 =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ c)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ c) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨rt, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨x3, _⟩ := mBranch_made x2.wf invc.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) h3
    have it := (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') h3).1
    obtain ⟨re, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ MSt.Ids (σc.back re.1) σc := by
      cases h4; exact ⟨SchedExt.refl x3.wf, by rw [back_eq_sfx, sfx_self]; exact MSt.Ids.refl _⟩
    obtain ⟨x4, ie⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have te : (σc.back rt.1).Same (σc.back re.1) := (it.trans ie.symm).toSame
    have jl := joinIf_live h5 te
    obtain ⟨e5, -⟩ := joinIf_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    cases h4
    refine ⟨(fun _ h => by cases h), ?_⟩
    have s := jl.2 (by simp [Option.all])
    rw [back_eq_sfx, sfx_self] at s
    exact (invc.ext x3).sameLive s
  | @match_exit _ _ _ _ e cases dflt _ _ id st1 σc st2 πk hid h2 hl hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ e)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ e) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨r, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨x3, _⟩ := (walk_ext sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ x2.wf invc.toMade h3
    have ar := ((walk_shape sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ hbc h3).1
    obtain ⟨d, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ (∀ a, a ∈ d.1 → MSt.Ids a.1 σc) := by
      cases dflt with
      | some db =>
        dsimp only at h4
        obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
        cases g
        refine ⟨(mBranch_made x3.wf (invc.toMade.ext x3)
          (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) g1).1, fun a ha => ?_⟩
        cases ha with
        | head => exact (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') g1).1
        | tail _ h => cases h
      | none => cases h4; exact ⟨SchedExt.refl x3.wf, fun _ h => by cases h⟩
    obtain ⟨x4, ad⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have arms : ∀ a b, a ∈ r.1 ++ d.1 → b ∈ r.1 ++ d.1 → a.1.Same b.1 := by
      have all : ∀ a, a ∈ r.1 ++ d.1 → MSt.Ids a.1 σc := fun a ha => by
        rcases List.mem_append.mp ha with ha | ha
        · exact ar a ha
        · exact ad a ha
      intro a b ha hb'
      exact ((all a ha).trans (all b hb').symm).toSame
    have jl := joinMatch_live h5 arms
    obtain ⟨e5, -⟩ := joinMatch_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    obtain ⟨rfl, _⟩ := stmtId_eq hid
    refine ⟨fun ev hev => ?_, trivial⟩
    simp only [List.mem_singleton] at hev
    subst hev
    unfold leaving at h2
    rw [hl] at h2
    have xF := x3.trans (x4.trans hF)
    exact tryEv_good (mE_shr sigs Γ false σ e) h2 xF x1.wf inv.rel
      (inv.madeBefore (x1.trans (x2.trans xF))) hb
  | @match_arm _ _ _ _ e cases dflt _ _ id st1 σc st2 out evs hid h2 hc =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ e)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ e) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨r, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨x3, _⟩ := (walk_ext sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ x2.wf invc.toMade h3
    have ar := ((walk_shape sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ hbc h3).1
    obtain ⟨d, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ (∀ a, a ∈ d.1 → MSt.Ids a.1 σc) := by
      cases dflt with
      | some db =>
        dsimp only at h4
        obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
        cases g
        refine ⟨(mBranch_made x3.wf (invc.toMade.ext x3)
          (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) g1).1, fun a ha => ?_⟩
        cases ha with
        | head => exact (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') g1).1
        | tail _ h => cases h
      | none => cases h4; exact ⟨SchedExt.refl x3.wf, fun _ h => by cases h⟩
    obtain ⟨x4, ad⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have arms : ∀ a b, a ∈ r.1 ++ d.1 → b ∈ r.1 ++ d.1 → a.1.Same b.1 := by
      have all : ∀ a, a ∈ r.1 ++ d.1 → MSt.Ids a.1 σc := fun a ha => by
        rcases List.mem_append.mp ha with ha | ha
        · exact ar a ha
        · exact ad a ha
      intro a b ha hb'
      exact ((all a ha).trans (all b hb').symm).toSame
    have jl := joinMatch_live h5 arms
    obtain ⟨e5, -⟩ := joinMatch_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    obtain ⟨g, o⟩ := cases_ok hc h3 x2.wf invc (x4.trans hF)
    refine ⟨g, ?_⟩
    cases out with
    | norm _ =>
      obtain ⟨a, ha, hca, ia⟩ := o
      exact (ia.sameLive (jl a (List.mem_append_left _ ha) hca)).ext x4
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x4⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x4⟩
    | exit => trivial
  | @match_dflt _ _ _ _ e cases db _ _ id st1 σc st2 rc st3c out evs hid h2 hr hb =>
    unfold mStmt at hw
    obtain ⟨id', st1', hid', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hid hid')
    dsimp only at hw
    obtain ⟨σc', st2', h2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det h2 h2')
    have x1 := SchedGrows.stmtId st _ _ w hid
    have x2 := SchedGrows.leaving _ σ _ (SchedGrows.shr (mE_shr sigs Γ false σ e)) _ _ _ x1.wf h2
    have b2 := leavingIf_below (mE_shr sigs Γ false σ e) h2
    have invc := inv.same (x1.trans x2) (MSt.Same.of_below b2)
    have hbc : base ≤ σc.length := invc.base
    have blk : ∀ {m : Nat} {Γ' σb st1 r st2 body}, base ≤ σb.length →
        (mBlock sigs env m Γ' σb base body).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r := by
      intro m Γ' σb st1 r st2 body hb' h'
      exact ⟨(walk_len sigs env m).2.1 _ _ _ _ _ _ _ hb' h', (walk_shape sigs env m).2.1 _ _ _ _ _ _ _ hb' h'⟩
    obtain ⟨r, st3, h3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨erl, est⟩ := Prod.mk.inj (MoveM.det hr h3)
    subst rc; subst st3c
    obtain ⟨x3, _⟩ := (walk_ext sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ x2.wf invc.toMade h3
    have ar := ((walk_shape sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ hbc h3).1
    obtain ⟨d, st4, h4, hw⟩ := MoveM.bind_ok hw
    have r4 : SchedExt st3 st4 ∧ (∀ a, a ∈ d.1 → MSt.Ids a.1 σc) := by
      try dsimp only at h4
      obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
      cases g
      refine ⟨(mBranch_made x3.wf (invc.toMade.ext x3)
        (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) g1).1, fun a ha => ?_⟩
      cases ha with
      | head => exact (mBranch_shape hbc (fun σb st1 r st2 hb' h' => blk hb' h') g1).1
      | tail _ h => cases h
    obtain ⟨x4, ad⟩ := r4
    obtain ⟨σ', st5, h5, hw⟩ := MoveM.bind_ok hw
    have arms : ∀ a b, a ∈ r.1 ++ d.1 → b ∈ r.1 ++ d.1 → a.1.Same b.1 := by
      have all : ∀ a, a ∈ r.1 ++ d.1 → MSt.Ids a.1 σc := fun a ha => by
        rcases List.mem_append.mp ha with ha | ha
        · exact ar a ha
        · exact ad a ha
      intro a b ha hb'
      exact ((all a ha).trans (all b hb').symm).toSame
    have jl := joinMatch_live h5 arms
    obtain ⟨e5, -⟩ := joinMatch_eq h5
    simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at hw
    obtain ⟨rfl, rfl⟩ := hw
    subst e5
    try dsimp only at h4
    obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
    cases g
    obtain ⟨gg, o⟩ := branch_ok hb g1 x3.wf (invc.ext x3) hF
    refine ⟨gg, ?_⟩
    cases out with
    | norm _ =>
      exact o.sameLive (jl _ (List.mem_append_right _ (.head _)) (pbranch_completes hb))
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | exit => trivial
termination_by (n, 0, 0)

theorem block_ok {n : Nat} {Γ : Ctx} {σ : MSt} {base : Nat} {ss : List Stmt} {st : DSt} {π : List Bool}
    {out : POut} {evs : List Ev} (hp : PBlock sigs env n Γ σ base ss st π out evs) {r : MOut} {st' F : DSt}
    {U : Nat} {H : Nat → Prop} (hw : (mBlock sigs env n Γ σ base ss).run st = .ok (r, st')) (w : st.WF)
    (inv : Inv F st U H base σ π) (hF : SchedExt st' F) :
    (∀ ev, ev ∈ evs → ev.Good F) ∧ OutOk F st' U H base r out := by
  cases hp with
  | nil =>
    unfold mBlock at hw; cases hw
    exact ⟨(fun _ h => by cases h), inv⟩
  | cons_norm h1 hr1 h2 =>
    unfold mBlock at hw
    obtain ⟨r1', st1', hr1', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hr1 hr1')
    obtain ⟨r2, st2, hr2, hw⟩ := MoveM.bind_ok hw
    cases hw
    obtain ⟨x1, o1⟩ := (walk_ext sigs env _).1 _ _ _ _ _ _ _ w inv.toMade hr1
    obtain ⟨x2, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ x1.wf o1.1 hr2
    obtain ⟨g1, i1⟩ := stmt_ok h1 hr1 w inv (x2.trans hF)
    obtain ⟨g2, i2⟩ := block_ok h2 hr2 x1.wf i1 hF
    refine ⟨fun ev hev => ?_, ?_⟩
    · rcases List.mem_append.mp hev with h | h
      · exact g1 ev h
      · exact g2 ev h
    · cases out with
      | norm _ => exact i2
      | brk _ => obtain ⟨τ, hτ, iτ⟩ := i2; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
      | cont _ => obtain ⟨τ, hτ, iτ⟩ := i2; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
      | exit => trivial
  | cons_stop h1 hn =>
    unfold mBlock at hw
    obtain ⟨r1, st1, hr1, hw⟩ := MoveM.bind_ok hw
    obtain ⟨r2, st2, hr2, hw⟩ := MoveM.bind_ok hw
    cases hw
    obtain ⟨x1, o1⟩ := (walk_ext sigs env _).1 _ _ _ _ _ _ _ w inv.toMade hr1
    obtain ⟨x2, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ x1.wf o1.1 hr2
    obtain ⟨g1, i1⟩ := stmt_ok h1 hr1 w inv (x2.trans hF)
    refine ⟨g1, ?_⟩
    cases out with
    | norm _ => exact hn.elim
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := i1; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x2⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := i1; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x2⟩
    | exit => trivial
termination_by (n, 0, 0)

theorem branch_ok {n : Nat} {Γb : Ctx} {σc : MSt} {binds : List String} {base : Nat} {body : List Stmt}
    {id : Nat} {kind : String} {arm : Option Nat} {st : DSt} {π : List Bool} {out : POut} {evs : List Ev}
    (hp : PBranch sigs env n Γb σc binds base body id kind arm st π out evs) {rb : MOut} {st' F : DSt}
    {U : Nat} {H : Nat → Prop}
    (hw : (mBranch Γb σc binds id kind arm fun σb => mBlock sigs env n Γb σb base body).run st = .ok (rb, st'))
    (w : st.WF) (inv : Inv F st U H base σc π) (hF : SchedExt st' F) :
    (∀ ev, ev ∈ evs → ev.Good F) ∧ OutOk F st' U H base (σc.back rb.1, rb.2.1, rb.2.2) out := by
  cases hp with
  | @norm _ _ _ _ _ _ _ _ _ _ _ σb st1 rb st2 ds st3 πe evs hb1 hb2 hb3 hb4 hblock =>
    unfold mBranch at hw
    obtain ⟨σb', st1', hb1', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hb1 hb1')
    obtain ⟨rb', st2', hb2', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hb2 hb2')
    obtain ⟨ds', st3', hb3', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hb3 hb3')
    obtain ⟨_, st4', hb4', hw⟩ := MoveM.bind_ok hw
    obtain ⟨-, rfl⟩ := Prod.mk.inj (MoveM.det hb4 hb4')
    cases hw
    have x1 := SchedGrows.bindM _ _ _ _ _ _ _ _ _ w hb1
    have invb := inv.bind w hb1
    obtain ⟨xb, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ x1.wf invb.toMade hb2
    have x4 := SchedGrows.record _ _ _ _ _ _ _ _ xb.wf hb4
    obtain ⟨g, ib⟩ := block_ok hblock hb2 x1.wf invb (x4.trans hF)
    have lb := ((walk_len sigs env _).2.1 _ _ _ _ _ _ _ invb.base hb2).1
    obtain ⟨new, hσb⟩ := bindM_shape hb1
    have hcl : σc.length ≤ rb.1.length := by
      have : σc.length ≤ σb.length := by rw [hσb]; simp
      omega
    have hl : πe.length = rb.1.length := ib.rel.length
    have hk : (rb.1.length - σc.length) + base ≤ rb.1.length := by have := inv.base; omega
    obtain ⟨-, rfl⟩ := ownedIn_run hb3
    refine ⟨fun ev hev => ?_, ?_⟩
    · rcases List.mem_append.mp hev with h | h
      · exact g ev h
      · simp only [List.mem_singleton] at h
        subst h
        rw [hl]
        refine recEv_good hb4 hF xb.wf (ib.rel.take _) (.inr ⟨fun _ h => h, fun v hv ho hH => ?_⟩)
        have hv' := mem_take_mono hv (show rb.1.length - σc.length ≤ rb.1.length - base by have := inv.base; omega)
        exact absurd (ib.hU _ hH) (Nat.not_lt.mpr (ib.scope v hv' ho))
    · show Inv F _ U H base (rb.1.drop (rb.1.length - σc.length)) (πe.drop (πe.length - σc.length))
      rw [hl]
      exact (ib.drop _ hk).ext x4
  | other hb1 hblock hn =>
    unfold mBranch at hw
    obtain ⟨σb', st1', hb1', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hb1 hb1')
    obtain ⟨rb', st2, hb2, hw⟩ := MoveM.bind_ok hw
    obtain ⟨ds, st3, hb3, hw⟩ := MoveM.bind_ok hw
    obtain ⟨_, st4, hb4, hw⟩ := MoveM.bind_ok hw
    cases hw
    have x1 := SchedGrows.bindM _ _ _ _ _ _ _ _ _ w hb1
    have invb := inv.bind w hb1
    obtain ⟨xb, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ x1.wf invb.toMade hb2
    have x3 := SchedGrows.ownedIn _ _ _ _ xb.wf hb3
    have x4 := SchedGrows.record _ _ _ _ _ _ _ _ x3.wf hb4
    obtain ⟨g, ib⟩ := block_ok hblock hb2 x1.wf invb (x3.trans (x4.trans hF))
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => exact hn.elim
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := ib; exact ⟨τ, hτ, iτ.ext (x3.trans x4)⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := ib; exact ⟨τ, hτ, iτ.ext (x3.trans x4)⟩
    | exit => trivial
termination_by (n, 1, 0)

theorem iter_ok {k n : Nat} {HX : List Bool → List Ev → Prop} {HN HB : List Bool → List Bool → Prop}
    {Γb : Ctx} {σc σb : MSt} {body : List Stmt} {st2 : DSt} {r : MOut} {st4 st5 : DSt} {π : List Bool}
    {out : POut} {evs : List Ev}
    (hp : PIter sigs env k HX HN HB n Γb σc σb body st2 r st4 st5 π out evs)
    {σh σ' : MSt} {id : Nat} {nat : Bool} {st0 st3 st6 F : DSt} {U Ub : Nat} {H : Nat → Prop} {base : Nat}
    {π0 : List Bool}
    (hb : (mBlock sigs env n Γb σb σc.length body).run st2 = .ok (r, st3))
    (hl : (loopHeld σc body r).run st3 = .ok ((), st4))
    (hrec : (record "loop_body_end" id none (ownedOf st4.held (scopeOwn σc r.1))).run st4 = .ok ((), st5))
    (hend : (loopEnd σh σc nat body r).run st5 = .ok (σ', st6))
    (w : st2.WF) (hF : SchedExt st6 F) (hmb : Made σb st2) (hmc : Made σc st2)
    (hlen : σc.length ≤ σb.length) (hsb : MSt.Ids (sfx σc.length σb) σc)
    (inv0 : Inv F st0 U H base σh π0) (x0 : SchedExt st0 st6) (sc : σc.Below σh)
    (hnat : ∀ π πn, HN π πn → nat = true)
    (hX : ∀ π evs, Rel F st4 H σh π → HX π evs → ∀ ev, ev ∈ evs → ev.Good F)
    (hN : ∀ π πn, Rel F st4 H σh π → HN π πn → Rel F st4 H σc πn)
    (hB : ∀ π πb, Rel F st4 H σh π → HB π πb →
      Inv F st2 Ub (fun u => u < Ub ∧ (u ∈ st4.held ∨ H u)) σc.length σb πb)
    (hr : Rel F st4 H σh π) :
    (∀ ev, ev ∈ evs → ev.Good F) ∧ OutOk F st6 U H base (σ', [], []) out := by
  obtain ⟨x3, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w hmb hb
  have x4 := loopHeld_ext x3.wf (hmc.ext x3) hl
  have x5 := SchedGrows.record _ _ _ _ _ _ _ _ x4.wf hrec
  obtain ⟨e6, -⟩ := loopEnd_eq hend
  have hF6 := hF
  have x06 := x0
  rw [e6] at hF x0 ⊢
  have xF4 : SchedExt st4 F := x5.trans hF
  have shp := (walk_shape sigs env n).2.1 _ _ _ _ _ _ _ hlen hb
  have lenk := (walk_len sigs env n).2.1 _ _ _ _ _ _ _ hlen hb
  have brkIds : ∀ τ, τ ∈ r.2.1 → MSt.Ids (σc.back τ) σc := fun τ hτ => by
    rw [back_eq_sfx]; exact (shp.2.1 τ hτ).trans hsb
  have contIds : ∀ τ, τ ∈ r.2.2 → MSt.Ids (σc.back τ) σc := fun τ hτ => by
    rw [back_eq_sfx]; exact (shp.2.2 τ hτ).trans hsb
  have normIds : MSt.Ids (σc.back r.1) σc := by
    rw [back_eq_sfx, ← sfx_sfx hlen lenk.1]
    exact (shp.1.sfx σc.length).trans hsb
  have spec := loopEnd_spec hend (fun τ hτ => MSt.Same.of_ids (brkIds τ hτ))
  have held := loopHeld_holds hl
  have hK : ∀ u, u ∈ st4.held → u ∈ F.held ∧ u ∈ F.tracked ∧ (u ∈ st4.held ∨ H u) := fun u h =>
    ⟨xF4.held u h, xF4.tracked u (x4.wf.heldTracked u h), .inl h⟩
  have hc : ∀ u, (u ∈ st3.held ∨ (u < Ub ∧ (u ∈ st4.held ∨ H u))) → u ∈ st4.held ∨ H u := fun u h =>
    h.elim (fun h => .inl (x4.held u h)) (fun h => h.2)
  have sameH : σc.Same σh := MSt.Same.of_below sc
  have toOut : ∀ {X : MSt} {πx : List Bool}, Rel F st3 (fun u => u < Ub ∧ (u ∈ st4.held ∨ H u)) X πx →
      Rel F st5 H X πx := fun rel => rel.conv fun v _ _ h =>
    h.elim (fun h => .inl (x5.held _ (x4.held _ h))) (fun h => h.2.imp (x5.held _) (fun h => h))
  cases hp with
  | head_exit hx => exact ⟨hX _ _ hr hx, trivial⟩
  | natural hn =>
    refine ⟨(fun _ h => by cases h), ?_⟩
    have sl := spec.2.2.1 (hnat _ _ hn)
    exact inv0.reframe x0 (sl.same.trans sameH) (((hN _ _ hr hn).mono x5.held (fun _ h => h)).sameLive sl)
  | @body_norm _ _ _ _ _ _ _ _ _ _ _ _ _ _ πb πe evs1 _ evs2 hbb hblk hnext =>
    have invb := hB _ _ hr hbb
    obtain ⟨g1, ib⟩ := block_ok hblk hb w invb (x4.trans xF4)
    have hlE : πe.length = r.1.length := ib.rel.length
    have hcomp := pblock_completes hblk
    have ev : (recEv st4 st5 (scopeOwn σc r.1) (πe.take (πe.length - σc.length))).Good F := by
      rw [hlE]
      refine recEv_good hrec hF x4.wf (ib.rel.take _) (.inr ⟨held.1, fun v hv ho hH => ?_⟩)
      exact absurd (ib.hU _ hH) (Nat.not_lt.mpr (ib.scope v hv ho))
    have rb : Rel F st3 (fun u => u < Ub ∧ (u ∈ st4.held ∨ H u)) (σc.back r.1)
        (πe.drop (πe.length - σc.length)) := by
      rw [hlE]; exact ib.rel.drop _
    have hr' : Rel F st4 H σh (πe.drop (πe.length - σc.length)) :=
      Rel.back hK hc rb (normIds.trans (MSt.Ids.of_below sc)) sc (spec.1 hcomp)
        (fun i v w h1 h2 hv ho hw => held.2 i v h1 hv ho (by simp [hcomp, h2, hw]))
    obtain ⟨g2, o2⟩ := iter_ok hnext hb hl hrec hend w hF6 hmb hmc hlen hsb inv0 x06 sc hnat hX hN hB hr'
    rw [e6] at o2
    refine ⟨fun e he => ?_, o2⟩
    simp only [List.mem_append, List.mem_cons] at he
    rcases he with he | rfl | he
    · exact g1 e he
    · exact ev
    · exact g2 e he
  | @body_brk _ _ _ _ _ _ _ _ _ _ _ _ _ _ πb πx _ hbb hblk =>
    obtain ⟨g1, τ, hτ, iτ⟩ := block_ok hblk hb w (hB _ _ hr hbb) (x4.trans xF4)
    refine ⟨g1, ?_⟩
    have hlx : πx.length = τ.length := iτ.rel.length
    have rb : Rel F st3 (fun u => u < Ub ∧ (u ∈ st4.held ∨ H u)) (σc.back τ)
        (πx.drop (πx.length - σc.length)) := by
      rw [hlx]; exact iτ.rel.drop _
    have sl := spec.2.2.2 τ hτ
    exact inv0.reframe x0 (sl.same.trans ((MSt.Same.of_ids (brkIds τ hτ)).trans sameH)) ((toOut rb).sameLive sl)
  | @body_cont _ _ _ _ _ _ _ _ _ _ _ _ _ _ πb πx evs1 _ evs2 hbb hblk hnext =>
    obtain ⟨g1, τ, hτ, iτ⟩ := block_ok hblk hb w (hB _ _ hr hbb) (x4.trans xF4)
    have hlx : πx.length = τ.length := iτ.rel.length
    have rb : Rel F st3 (fun u => u < Ub ∧ (u ∈ st4.held ∨ H u)) (σc.back τ)
        (πx.drop (πx.length - σc.length)) := by
      rw [hlx]; exact iτ.rel.drop _
    have hr' : Rel F st4 H σh (πx.drop (πx.length - σc.length)) :=
      Rel.back hK hc rb ((contIds τ hτ).trans (MSt.Ids.of_below sc)) sc (spec.2.1 τ hτ)
        (fun i v w h1 h2 hv ho hw => held.2 i v h1 hv ho (by
          simp only [Bool.or_eq_true, List.any_eq_true]
          exact .inr ⟨τ, hτ, by simp [h2, hw]⟩))
    obtain ⟨g2, o2⟩ := iter_ok hnext hb hl hrec hend w hF6 hmb hmc hlen hsb inv0 x06 sc hnat hX hN hB hr'
    rw [e6] at o2
    refine ⟨fun e he => ?_, o2⟩
    rcases List.mem_append.mp he with he | he
    · exact g1 e he
    · exact g2 e he
  | body_exit hbb hblk =>
    obtain ⟨g1, -⟩ := block_ok hblk hb w (hB _ _ hr hbb) (x4.trans xF4)
    exact ⟨g1, trivial⟩
termination_by (n, 2, k)

theorem cases_ok {n : Nat} {Γ : Ctx} {vs : List (String × Option Ty)} {σ : MSt} {base id i : Nat}
    {cs : List Case} {st : DSt} {π : List Bool} {out : POut} {evs : List Ev}
    (hp : PCases sigs env n Γ vs σ base id i cs st π out evs) {r : List (MSt × Bool) × List MSt × List MSt}
    {st' F : DSt} {U : Nat} {H : Nat → Prop}
    (hw : (mCases sigs env n Γ vs σ base id i cs).run st = .ok (r, st')) (w : st.WF)
    (inv : Inv F st U H base σ π) (hF : SchedExt st' F) :
    (∀ ev, ev ∈ evs → ev.Good F) ∧ CasesOk F st' U H base r out := by
  cases hp with
  | here_atom hb =>
    unfold mCases at hw
    obtain ⟨rb, st1, hrb, hw⟩ := MoveM.bind_ok hw
    obtain ⟨r', st2, hr', hw⟩ := MoveM.bind_ok hw
    cases hw
    obtain ⟨x1, _⟩ := mBranch_made w inv.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) hrb
    obtain ⟨x2, _⟩ := (walk_ext sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ x1.wf (inv.toMade.ext x1) hr'
    obtain ⟨g, o⟩ := branch_ok hb hrb w inv (x2.trans hF)
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => exact ⟨_, .head _, pbranch_completes hb, o.ext x2⟩
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x2⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x2⟩
    | exit => trivial
  | here_term hb =>
    unfold mCases at hw
    dsimp only at hw
    obtain ⟨rb, st1, hrb, hw⟩ := MoveM.bind_ok hw
    obtain ⟨r', st2, hr', hw⟩ := MoveM.bind_ok hw
    cases hw
    obtain ⟨x1, _⟩ := mBranch_made w inv.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) hrb
    obtain ⟨x2, _⟩ := (walk_ext sigs env _).2.2 _ _ _ _ _ _ _ _ _ _ x1.wf (inv.toMade.ext x1) hr'
    obtain ⟨g, o⟩ := branch_ok hb hrb w inv (x2.trans hF)
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => exact ⟨_, .head _, pbranch_completes hb, o.ext x2⟩
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x2⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_left _ hτ, iτ.ext x2⟩
    | exit => trivial
  | later_atom hrb hc =>
    unfold mCases at hw
    obtain ⟨rb', st1', hrb', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hrb hrb')
    obtain ⟨r', st2, hr', hw⟩ := MoveM.bind_ok hw
    cases hw
    obtain ⟨x1, _⟩ := mBranch_made w inv.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) hrb
    obtain ⟨g, o⟩ := cases_ok hc hr' x1.wf (inv.ext x1) hF
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => obtain ⟨a, ha, hc', ia⟩ := o; exact ⟨a, .tail _ ha, hc', ia⟩
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | exit => trivial
  | later_term hrb hc =>
    unfold mCases at hw
    dsimp only at hw
    obtain ⟨rb', st1', hrb', hw⟩ := MoveM.bind_ok hw
    obtain ⟨rfl, rfl⟩ := Prod.mk.inj (MoveM.det hrb hrb')
    obtain ⟨r', st2, hr', hw⟩ := MoveM.bind_ok hw
    cases hw
    obtain ⟨x1, _⟩ := mBranch_made w inv.toMade
      (fun σb st1 r st2 w m h => (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ w m h) hrb
    obtain ⟨g, o⟩ := cases_ok hc hr' x1.wf (inv.ext x1) hF
    refine ⟨g, ?_⟩
    cases out with
    | norm _ => obtain ⟨a, ha, hc', ia⟩ := o; exact ⟨a, .tail _ ha, hc', ia⟩
    | brk _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | cont _ => obtain ⟨τ, hτ, iτ⟩ := o; exact ⟨τ, List.mem_append_right _ hτ, iτ⟩
    | exit => trivial
termination_by (n, 0, 0)

end

end

end Datalove.Fun
