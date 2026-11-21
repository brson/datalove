import Datalove.Proofs.DropSched
namespace Datalove.Fun
set_option maxHeartbeats 1000000

theorem OutMade.ext' {r : MOut} {st st' : DSt} (h : OutMade r st) (x : SchedExt st st') : OutMade r st' :=
  ⟨h.1.ext x, fun τ hτ => (h.2.1 τ hτ).ext x, fun τ hτ => (h.2.2 τ hτ).ext x⟩

theorem OutMade.simple {σ : MSt} {st : DSt} (h : Made σ st) : OutMade (σ, [], []) st := by
  unfold OutMade
  refine ⟨h, ?_, ?_⟩ <;> intro _ hx <;> cases hx

/-- A step through `leavingIf`: the schedule kept as `SchedExt` says, and
its state's bindings as they were. -/
theorem leavingIf_made {id : Nat} {σ σ' : MSt} {c : Bool} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (w : st.WF) (h : (leavingIf id σ c m).run st = .ok (σ', st')) (md : Made σ st) :
    Made σ' st' ∧ SchedExt st st' := by
  have x := SchedGrows.leavingIf id σ c (SchedGrows.shr hm) st σ' st' w h
  refine ⟨Made.ext ?_ x, x⟩
  unfold leavingIf at h
  split at h
  · obtain ⟨_, st1, h1, h2⟩ := MoveM.bind_ok h
    obtain ⟨σ1, st2, h3, h4⟩ := MoveM.bind_ok h2
    have b := (hm _ σ1 st2 h3).1
    obtain ⟨_, _, _, h5⟩ := MoveM.bind_ok h4
    obtain ⟨_, _, _, h6⟩ := MoveM.bind_ok h5
    obtain ⟨_, _, _, h7⟩ := MoveM.bind_ok h6
    cases h7
    exact md.of_same (SameIds.below b)
  · exact md.of_same (SameIds.below (hm st σ' st' h).1)

theorem leaving_made {id : Nat} {σ σ' : MSt} {e : Expr} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (w : st.WF) (h : (leaving id σ e m).run st = .ok (σ', st')) (md : Made σ st) :
    Made σ' st' ∧ SchedExt st st' := leavingIf_made hm w h md

theorem exitM_eq {σ τ : MSt} {st st' : DSt} (h : (exitM σ).run st = .ok (τ, st')) : τ = σ ∧ st' = st := by
  unfold exitM at h
  cases hc : σ.outsSet
  · simp only [hc] at h; cases h
  · simp only [hc] at h; cases h; exact ⟨rfl, rfl⟩

theorem SchedExt.addTracked {st : DSt} (w : st.WF) (xs : List Nat) :
    SchedExt st { st with tracked := xs ++ st.tracked } := by
  refine ⟨Nat.le_refl _, Nat.le_refl _, fun _ h => h, fun _ h => List.mem_append_right _ h, fun _ _ h => h,
    ⟨[], st.out, rfl, ?_⟩, ⟨w.fresh, w.made, w.heldLt, fun u h => List.mem_append_right _ (w.heldTracked u h)⟩⟩
  refine Pairwise.imp (fun a b r => ?_) (Pairwise.refl st st.out)
  obtain ⟨k, s', a', f, sub, ext, p⟩ := r
  exact ⟨k, s', a', f, sub, fun x hx => (ext x hx).imp id (fun y => List.mem_append_right _ y),
    fun hk x hx hn _ => absurd hx hn⟩

theorem shr_made {σ τ : MSt} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt} (w : st.WF)
    (h : m.run st = .ok (τ, st')) (md : Made σ st) : Made τ st' ∧ SchedExt st st' := by
  have x := SchedGrows.shr hm st τ st' w h
  exact ⟨(md.of_same (SameIds.below (hm st τ st' h).1)).ext x, x⟩

theorem mSetParts_shr (sigs : List (String × Sig)) (Γ : Ctx) (σ : MSt) (steps : List Step) (op : Option BinOp)
    (e : Expr) : Shr σ (mSetParts sigs Γ σ steps op e) := by
  unfold mSetParts
  exact Shr.calm_bind KeepsSched.exprAt fun _ => Shr.calm_bind (KeepsSched.exprFrom _) fun _ =>
    Shr.bind (mE_shr sigs Γ _ σ e) fun σ1 h1 => Shr.calm_bind KeepsSched.exprAt fun _ =>
    Shr.calm_bind (KeepsSched.exprFrom _) fun _ => Shr.bind ((mKeys_shr sigs Γ σ1 steps).trans h1) fun σ2 h2 =>
    Shr.calm_bind (KeepsSched.exprFrom _) fun _ => Shr.pure' h2

theorem mSet_made {Γ : Ctx} {id : Nat} {σ σ' : MSt} {x : String} {steps : List Step} {op : Option BinOp}
    {e : Expr} {st st' : DSt} (sigs : List (String × Sig)) (w : st.WF)
    (h : (mSet sigs Γ id σ x steps op e).run st = .ok (σ', st')) (md : Made σ st) :
    Made σ' st' ∧ SchedExt st st' := by
  unfold mSet at h
  obtain ⟨_, st0, h0, h⟩ := MoveM.bind_ok h
  have x0 := SchedGrows.keepsSched (KeepsSched.unwalkedM σ x) st _ st0 w h0
  obtain ⟨_, st1, h1, h⟩ := MoveM.bind_ok h
  have x1 := x0.trans (SchedGrows.keepsSched (KeepsSched.writableM σ x) st0 _ st1 x0.wf h1)
  obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
  obtain ⟨m2, x2⟩ := leavingIf_made (mSetParts_shr sigs Γ σ steps op e) x1.wf h2 (md.ext x1)
  obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
  obtain ⟨m3', x3'⟩ := shr_made (mTarget_shr σ1 x steps op) x2.wf h3 m2
  obtain ⟨_, st3n, h3n, h⟩ := MoveM.bind_ok h
  have x3n := SchedGrows.keepsSched (KeepsSched.noteOverMoved _ _ _ _) _ _ _ x3'.wf h3n
  have x3 := x3'.trans x3n
  have m3 := m3'.ext x3n
  obtain ⟨_, st4, h4, h⟩ := MoveM.bind_ok h
  have x4 : SchedExt st3n st4 := by
    split at h4
    · exact (SchedGrows.bind (SchedGrows.ownedIn _) fun _ => SchedGrows.record _ _ _ _ _) st3n _ st4 x3.wf h4
    · cases h4; exact SchedExt.refl x3.wf
  obtain ⟨σ4, st5, h5, h⟩ := MoveM.bind_ok h
  have e5 : σ4 = σ2 ∧ st5 = st4 := by
    split at h5
    · exact exitM_eq h5
    · cases h5; exact ⟨rfl, rfl⟩
  simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
  obtain ⟨rfl, rfl⟩ := h
  obtain ⟨rfl, rfl⟩ := e5
  refine ⟨?_, x1.trans (x2.trans (x3.trans x4))⟩
  have m4 := m3.ext x4
  split
  · exact m4.of_same (SameIds.update (f := fun v => { v with init := true, live := true })
      (fun _ => ⟨rfl, rfl⟩) x σ4)
  · exact m4

theorem bindFor_made {Γf : Ctx} : ∀ (xs : List (String × Bool)) {σ σ' : MSt} {st st' : DSt}, st.WF →
    (xs.foldlM (fun σ (x, r) => bindOne Γf false true true false r σ x) σ).run st = .ok (σ', st') →
    Made σ st → Made σ' st' ∧ SchedExt st st'
  | [], σ, σ', st, st', w, h, m => by cases h; exact ⟨m, SchedExt.refl w⟩
  | (x, r) :: xs, σ, σ', st, st', w, h, m => by
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    have x1 := SchedGrows.bindOne Γf false true true false r σ x st σ1 st1 w h1
    obtain ⟨m2, x2⟩ := bindFor_made xs x1.wf h2 (bindOne_made h1 m)
    exact ⟨m2, x1.trans x2⟩

theorem mBranch_made {Γb : Ctx} {σc : MSt} {binds : List String} {id : Nat} {kind : String}
    {arm : Option Nat} {walk : MSt → MoveM MOut} {st st' : DSt} {rb : MOut} (w : st.WF) (md : Made σc st)
    (hwalk : ∀ σb st1 r st2, st1.WF → Made σb st1 → (walk σb).run st1 = .ok (r, st2) →
      SchedExt st1 st2 ∧ OutMade r st2)
    (h : (mBranch Γb σc binds id kind arm walk).run st = .ok (rb, st')) : SchedExt st st' ∧ OutMade rb st' := by
  unfold mBranch at h
  obtain ⟨σb, st1, h1, h⟩ := MoveM.bind_ok h
  obtain ⟨m1, x1⟩ := bindM_made w h1 md
  obtain ⟨r, st2, h2, h⟩ := MoveM.bind_ok h
  obtain ⟨x2, o2⟩ := hwalk σb st1 r st2 x1.wf m1 h2
  obtain ⟨_, st3, h3, h⟩ := MoveM.bind_ok h
  have x3 := SchedGrows.ownedIn _ st2 _ st3 x2.wf h3
  obtain ⟨_, st4, h4, h⟩ := MoveM.bind_ok h
  have x4 := SchedGrows.record _ _ _ _ _ st3 _ st4 x3.wf h4
  cases h
  exact ⟨x1.trans (x2.trans (x3.trans x4)), (o2.ext' x3).ext' x4⟩

section
variable (sigs : List (String × Sig)) (env : InferEnv)

theorem walk_ext : ∀ n,
    (∀ Γ σ base s st r st', st.WF → Made σ st → (mStmt sigs env n Γ σ base s).run st = .ok (r, st') →
      SchedExt st st' ∧ OutMade r st') ∧
    (∀ Γ σ base ss st r st', st.WF → Made σ st → (mBlock sigs env n Γ σ base ss).run st = .ok (r, st') →
      SchedExt st st' ∧ OutMade r st') ∧
    (∀ Γ vs σ base id i cs st r st', st.WF → Made σ st →
      (mCases sigs env n Γ vs σ base id i cs).run st = .ok (r, st') →
      SchedExt st st' ∧ (∀ a, a ∈ r.1 → Made a.1 st') ∧ (∀ τ, τ ∈ r.2.1 → Made τ st') ∧ (∀ τ, τ ∈ r.2.2 → Made τ st'))
  | 0 => by
    refine ⟨fun Γ σ base s st r st' w m h => ?_, fun Γ σ base ss st r st' w m h => ?_,
      fun Γ vs σ base id i cs st r st' w m h => ?_⟩
    · simp only [mStmt] at h; cases h; exact ⟨SchedExt.refl w, OutMade.simple m⟩
    · simp only [mBlock] at h; cases h; exact ⟨SchedExt.refl w, OutMade.simple m⟩
    · simp only [mCases] at h; cases h
      refine ⟨SchedExt.refl w, ?_, ?_, ?_⟩ <;> intro _ h <;> cases h
  | n + 1 => by
    obtain ⟨IS, IB, IC⟩ := walk_ext n
    refine ⟨fun Γ σ base s st r st' w m h => ?_, fun Γ σ base ss st r st' w m h => ?_,
      fun Γ vs σ base id i cs st r st' w m h => ?_⟩
    · unfold mStmt at h
      obtain ⟨id, st1, h1, h⟩ := MoveM.bind_ok h
      have x1 := SchedGrows.stmtId st id st1 w h1
      have m1 := m.ext x1
      cases s with
      | let_ p t e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ false σ e) x1.wf h2 m1
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨m3, x3⟩ := bindM_made x2.wf h3 m2
        cases h
        exact ⟨(x1.trans x2).trans x3, OutMade.simple m3⟩
      | var_ p t e =>
        dsimp only at h
        cases e with
        | some e =>
          obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
          obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ false σ e) x1.wf h2 m1
          obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
          obtain ⟨m3, x3⟩ := bindM_made x2.wf h3 m2
          cases h
          exact ⟨(x1.trans x2).trans x3, OutMade.simple m3⟩
        | none =>
          cases t with
          | some _ =>
            obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
            obtain ⟨m2, x2⟩ := bindM_made x1.wf h2 m1
            obtain ⟨_, st3, h3, h⟩ := MoveM.bind_ok h
            simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h3
            cases h3
            cases h
            have x3 := SchedExt.addTracked x2.wf ((scopeOwn σ σ1).map (·.uid))
            exact ⟨(x1.trans x2).trans x3, OutMade.simple (m2.ext x3)⟩
          | none => cases h; exact ⟨x1, OutMade.simple m1⟩
      | set_ x steps op e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := mSet_made sigs x1.wf h2 m1
        cases h
        exact ⟨x1.trans x2, OutMade.simple m2⟩
      | fun_ _ => cases h; exact ⟨x1, OutMade.simple m1⟩
      | type_ _ _ => cases h; exact ⟨x1, OutMade.simple m1⟩
      | ret e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨e2a, e2b⟩ := exitM_eq h2
        subst σ1; subst st2
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        have r3 : Made σ2 st3 ∧ SchedExt st1 st3 := by
          cases e with
          | some e => exact leaving_made (mE_shr sigs Γ false σ e) x1.wf h3 m1
          | none => cases h3; exact ⟨m1, SchedExt.refl x1.wf⟩
        obtain ⟨_, st4, h4, h⟩ := MoveM.bind_ok h
        have x4 := SchedGrows.ownedIn σ2 st3 _ st4 r3.2.wf h4
        obtain ⟨_, st5, h5, h⟩ := MoveM.bind_ok h
        have x5 := SchedGrows.record _ _ _ _ _ st4 _ st5 x4.wf h5
        cases h
        exact ⟨x1.trans (r3.2.trans (x4.trans x5)), OutMade.simple ((r3.1.ext x4).ext x5)⟩
      | brk =>
        dsimp only at h
        obtain ⟨_, st2, h2, h⟩ := MoveM.bind_ok h
        have x2 := SchedGrows.ownedIn _ st1 _ st2 x1.wf h2
        obtain ⟨_, st3, h3, h⟩ := MoveM.bind_ok h
        have x3 := SchedGrows.record _ _ _ _ _ st2 _ st3 x2.wf h3
        cases h
        have m3 := (m1.ext x2).ext x3
        refine ⟨x1.trans (x2.trans x3), ?_⟩
        unfold OutMade
        refine ⟨m3, fun τ hτ => ?_, (fun τ hτ => by cases hτ)⟩
        cases hτ with
        | head => exact m3
        | tail _ h => cases h
      | cont =>
        dsimp only at h
        obtain ⟨_, st2, h2, h⟩ := MoveM.bind_ok h
        have x2 := SchedGrows.ownedIn _ st1 _ st2 x1.wf h2
        obtain ⟨_, st3, h3, h⟩ := MoveM.bind_ok h
        have x3 := SchedGrows.record _ _ _ _ _ st2 _ st3 x2.wf h3
        cases h
        have m3 := (m1.ext x2).ext x3
        refine ⟨x1.trans (x2.trans x3), ?_⟩
        unfold OutMade
        refine ⟨m3, (fun τ hτ => by cases hτ), fun τ hτ => ?_⟩
        cases hτ with
        | head => exact m3
        | tail _ h => cases h
      | debuglog e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ true σ e) x1.wf h2 m1
        cases h
        exact ⟨x1.trans x2, OutMade.simple m2⟩
      | call e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ false σ e) x1.wf h2 m1
        cases h
        exact ⟨x1.trans x2, OutMade.simple m2⟩
      | const_ x t e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs _ false σ e) x1.wf h2 m1
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨m3, x3⟩ := bindM_made x2.wf h3 m2
        cases h
        exact ⟨(x1.trans x2).trans x3, OutMade.simple m3⟩
      | if_ c tB eB t e =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ false σ c) x1.wf h2 m1
        obtain ⟨rt, st6, h6, h⟩ := MoveM.bind_ok h
        obtain ⟨x6, o6⟩ := mBranch_made x2.wf m2 (fun σb st1 r st2 w m h => IB _ _ _ _ _ _ _ w m h) h6
        have mc6 : Made σc st6 := m2.ext x6
        obtain ⟨re, st7, h7, h⟩ := MoveM.bind_ok h
        have r7 : SchedExt st6 st7 ∧ OutMade re st7 := by
          cases e with
          | some eb =>
            dsimp only at h7
            exact mBranch_made x6.wf mc6 (fun σb st1 r st2 w m h => IB _ _ _ _ _ _ _ w m h) h7
          | none => cases h7; exact ⟨SchedExt.refl x6.wf, OutMade.simple mc6⟩
        obtain ⟨x7, o7⟩ := r7
        obtain ⟨σ', st8, h8, h⟩ := MoveM.bind_ok h
        obtain ⟨e8, s8⟩ := joinIf_eq h8
        simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
        obtain ⟨rfl, rfl⟩ := h
        subst e8
        have o6' := o6.ext' x7
        refine ⟨x1.trans (x2.trans (x6.trans x7)), ?_⟩
        unfold OutMade at o6' o7 ⊢
        refine ⟨?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rcases s8 with s | s
          · exact (o6'.1.of_same (SameIds.back _ _)).of_same s
          · exact (o7.1.of_same (SameIds.back _ _)).of_same s
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact o6'.2.1 τ hτ
          · exact o7.2.1 τ hτ
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact o6'.2.2 τ hτ
          · exact o7.2.2 τ hτ
      | loop c body =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have r2 : Made σc st2 ∧ SchedExt st1 st2 := by
          cases c with
          | some c => exact leaving_made (mE_shr sigs Γ false σ c) x1.wf h2 m1
          | none => cases h2; exact ⟨m1, SchedExt.refl x1.wf⟩
        obtain ⟨m2, x2⟩ := r2
        obtain ⟨r, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨x3, o3⟩ := IB _ _ _ _ _ _ _ x2.wf m2 h3
        obtain ⟨_, st4, h4, h⟩ := MoveM.bind_ok h
        have x4 := loopHeld_ext x3.wf (m2.ext x3) h4
        obtain ⟨_, st5, h5, h⟩ := MoveM.bind_ok h
        have x5 := SchedGrows.ownedIn _ st4 _ st5 x4.wf h5
        obtain ⟨_, st6, h6, h⟩ := MoveM.bind_ok h
        have x6 := SchedGrows.record _ _ _ _ _ st5 _ st6 x5.wf h6
        obtain ⟨σ', st7, h7, h⟩ := MoveM.bind_ok h
        obtain ⟨e7, s7⟩ := loopEnd_eq h7
        simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
        obtain ⟨rfl, rfl⟩ := h
        subst e7
        have o6 := ((o3.ext' x4).ext' x5).ext' x6
        refine ⟨x1.trans (x2.trans (x3.trans (x4.trans (x5.trans x6)))), ?_⟩
        refine OutMade.simple ?_
        rcases s7 with s | ⟨τ, hτ, s⟩
        · exact ((((m2.ext x3).ext x4).ext x5).ext x6).of_same s
        · exact ((o6.2.1 τ hτ).of_same (SameIds.back _ _)).of_same s
      | for_ names coll body =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ true σ coll) x1.wf h2 m1
        have mw : Made (match placeOf coll with
            | some (x, _) => σc.walk x
            | none => σc) st2 := by
          split
          · exact m2.of_same (SameIds.walk _ _)
          · exact m2
        obtain ⟨σb, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨m3, x3⟩ := bindFor_made _ x2.wf h3 mw
        obtain ⟨r, st4, h4, h⟩ := MoveM.bind_ok h
        obtain ⟨x4, o4⟩ := IB _ _ _ _ _ _ _ x3.wf m3 h4
        obtain ⟨_, st5, h5, h⟩ := MoveM.bind_ok h
        have x5 := loopHeld_ext x4.wf ((m2.ext x3).ext x4) h5
        obtain ⟨_, st6, h6, h⟩ := MoveM.bind_ok h
        have x6 := SchedGrows.ownedIn _ st5 _ st6 x5.wf h6
        obtain ⟨_, st7, h7, h⟩ := MoveM.bind_ok h
        have x7 := SchedGrows.record _ _ _ _ _ st6 _ st7 x6.wf h7
        obtain ⟨σ', st8, h8, h⟩ := MoveM.bind_ok h
        obtain ⟨e8, s8⟩ := loopEnd_eq h8
        simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
        obtain ⟨rfl, rfl⟩ := h
        subst e8
        have o7 := ((o4.ext' x5).ext' x6).ext' x7
        refine ⟨x1.trans (x2.trans (x3.trans (x4.trans (x5.trans (x6.trans x7))))), ?_⟩
        refine OutMade.simple ?_
        rcases s8 with s | ⟨τ, hτ, s⟩
        · exact (((((m2.ext x3).ext x4).ext x5).ext x6).ext x7).of_same s
        · exact ((o7.2.1 τ hτ).of_same (SameIds.back _ _)).of_same s
      | match_ e cases dflt =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨m2, x2⟩ := leaving_made (mE_shr sigs Γ false σ e) x1.wf h2 m1
        obtain ⟨r, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨x3, a3, b3, c3⟩ := IC _ _ _ _ _ _ _ _ _ _ x2.wf m2 h3
        have mc3 := m2.ext x3
        obtain ⟨d, st4, h4, h⟩ := MoveM.bind_ok h
        have r4 : SchedExt st3 st4 ∧ (∀ a, a ∈ d.1 → Made a.1 st4) ∧ (∀ τ, τ ∈ d.2.1 → Made τ st4) ∧
            (∀ τ, τ ∈ d.2.2 → Made τ st4) := by
          cases dflt with
          | some db =>
            dsimp only at h4
            obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
            obtain ⟨y1, q3⟩ := mBranch_made x3.wf mc3 (fun σb st1 r st2 w m h => IB _ _ _ _ _ _ _ w m h) g1
            cases g
            unfold OutMade at q3
            refine ⟨y1, fun a ha => ?_, q3.2.1, q3.2.2⟩
            cases ha with
            | head => exact q3.1.of_same (SameIds.back _ _)
            | tail _ h => cases h
          | none =>
            cases h4
            refine ⟨SchedExt.refl x3.wf, ?_, ?_, ?_⟩ <;> intro _ h <;> cases h
        obtain ⟨x4, a4, b4, c4⟩ := r4
        obtain ⟨σ', st5, h5, h⟩ := MoveM.bind_ok h
        obtain ⟨e5, s5⟩ := joinMatch_eq h5
        simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
        obtain ⟨rfl, rfl⟩ := h
        subst e5
        refine ⟨x1.trans (x2.trans (x3.trans x4)), ?_⟩
        unfold OutMade
        refine ⟨?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rcases s5 with s | ⟨a, ha, s⟩
          · exact (mc3.ext x4).of_same s
          · rcases List.mem_append.mp ha with ha | ha
            · exact ((a3 a ha).ext x4).of_same s
            · exact (a4 a ha).of_same s
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact (b3 τ hτ).ext x4
          · exact b4 τ hτ
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact (c3 τ hτ).ext x4
          · exact c4 τ hτ
    · cases ss with
      | nil => unfold mBlock at h; cases h; exact ⟨SchedExt.refl w, OutMade.simple m⟩
      | cons s ss =>
        unfold mBlock at h
        obtain ⟨r1, st1, h1, h⟩ := MoveM.bind_ok h
        obtain ⟨x1, o1⟩ := IS _ _ _ _ _ _ _ w m h1
        obtain ⟨r2, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨x2, o2⟩ := IB _ _ _ _ _ _ _ x1.wf o1.1 h2
        cases h
        have o1' := o1.ext' x2
        unfold OutMade at o1' o2 ⊢
        refine ⟨x1.trans x2, o2.1, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact o1'.2.1 τ hτ
          · exact o2.2.1 τ hτ
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact o1'.2.2 τ hτ
          · exact o2.2.2 τ hτ
    · cases cs with
      | nil =>
        unfold mCases at h; cases h
        refine ⟨SchedExt.refl w, ?_, ?_, ?_⟩ <;> intro _ h <;> cases h
      | cons c cs =>
        have tail : ∀ {rb : MOut} {stb : DSt}, SchedExt st stb → OutMade rb stb →
            ∀ {st'' : DSt} {rest : List (MSt × Bool) × List MSt × List MSt}, (do
              let r ← mCases sigs env n Γ vs σ base id (i + 1) cs
              pure ((σ.back rb.1, completes (match c with | .atom _ b => b | .term _ _ b => b)) :: r.1,
                rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2) : MoveM _).run stb = .ok (rest, st'') →
            SchedExt st st'' ∧ (∀ a, a ∈ rest.1 → Made a.1 st'') ∧ (∀ τ, τ ∈ rest.2.1 → Made τ st'') ∧
              (∀ τ, τ ∈ rest.2.2 → Made τ st'') := by
          intro rb stb xb ob st'' rest g
          obtain ⟨r, s3, g3, g⟩ := MoveM.bind_ok g
          obtain ⟨y3, a3, b3, c3⟩ := IC _ _ _ _ _ _ _ _ _ _ xb.wf (m.ext xb) g3
          cases g
          have ob' := ob.ext' y3
          unfold OutMade at ob'
          refine ⟨xb.trans y3, fun a ha => ?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
          · cases ha with
            | head => exact ob'.1.of_same (SameIds.back _ _)
            | tail _ h => exact a3 a h
          · rcases List.mem_append.mp hτ with hτ | hτ
            · exact ob'.2.1 τ hτ
            · exact b3 τ hτ
          · rcases List.mem_append.mp hτ with hτ | hτ
            · exact ob'.2.2 τ hτ
            · exact c3 τ hτ
        cases c with
        | atom a b =>
          unfold mCases at h
          obtain ⟨rb, st1, h1, h⟩ := MoveM.bind_ok h
          obtain ⟨x1, o1⟩ := mBranch_made w m (fun σb st1 r st2 w m h => IB _ _ _ _ _ _ _ w m h) h1
          exact tail x1 o1 h
        | term a x b =>
          unfold mCases at h
          dsimp only at h
          obtain ⟨rb, st1, h1, h⟩ := MoveM.bind_ok h
          obtain ⟨x1, o1⟩ := mBranch_made w m (fun σb st1 r st2 w m h => IB _ _ _ _ _ _ _ w m h) h1
          exact tail x1 o1 h

end

end Datalove.Fun
