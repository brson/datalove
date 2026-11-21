import Datalove.Proofs.DropSpec

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

end

end

/-- What a path through a statement ends with, as the walk's result there
says: on to what follows, with what it carries; at a `break` or a
`continue`, with what it carries at one of the walk's; or out of the
function. -/
def OutOk (F st : DSt) (U : Nat) (H : Nat → Prop) (base : Nat) (r : MOut) : POut → Prop
  | .norm π => Inv F st U H base r.1 π
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
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 (x3.trans hF) x1.wf inv.rel x1.held
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
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 xF x1.wf inv.rel x1.held
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
    exact tryEv_good (mE_shr sigs Γ true σ _) h2 xF x1.wf inv.rel x1.held
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
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 xF x1.wf inv.rel x1.held
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
    exact tryEv_good (mE_shr sigs _ false σ _) h2 xF x1.wf inv.rel x1.held
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
    exact recEv_good h3 hF x1.wf (inv.rel.take _) x1.held
      (.inr fun v hv ho hH => absurd (inv.hU _ hH) (Nat.not_lt.mpr (inv.scope v hv ho)))
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
    exact recEv_good h3 hF x1.wf (inv.rel.take _) x1.held
      (.inr fun v hv ho hH => absurd (inv.hU _ hH) (Nat.not_lt.mpr (inv.scope v hv ho)))
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
    exact tryEv_good (mSetParts_shr sigs Γ σ _ _ _) h1 xF xb.wf inv.rel (fun u h => xb.held u (xa.held u (x1.held u h)))
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
    refine recEv_good hr hF x3.wf inv3.rel (fun _ h => h) (.inl ⟨rfl, rfl, fun v hv ho => ?_⟩)
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
    exact tryEv_good (mE_shr sigs Γ false σ _) h2 xF x1.wf inv.rel x1.held
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
    refine recEv_good hr hF x2.wf inv2.rel (fun _ h => h) (.inl ⟨rfl, rfl, fun v hv ho => ?_⟩)
    obtain ⟨w', hw', u', o'⟩ := s1.mem hv
    rw [u']
    exact inv.madeBefore (x1.trans (x2.trans ((SchedGrows.record _ _ _ _ _ _ _ _ x2.wf hr).trans hF))) w' hw'
      (o' ▸ ho)

end

end Datalove.Fun
