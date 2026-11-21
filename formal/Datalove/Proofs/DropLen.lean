import Datalove.Proofs.DropWalk

/-!
# What the walk's states are as long as

A block's walk leaves a state that extends the one it was given, its own
bindings before, and a `break` or a `continue` in a loop's body is reached
with the loop's own scope and more. A path leaves a scope by dropping as
many of its own flags as the walk drops of its bindings, so these lengths
are what keep the two lined up.
-/

namespace Datalove.Fun

theorem initFrom_length (σ τ : MSt) : (σ.initFrom τ).length = min σ.length τ.length := by
  simp [MSt.initFrom]

theorem initMeet_length (σ τ : MSt) : (σ.initMeet τ).length = min σ.length τ.length := by
  simp [MSt.initMeet]

theorem back_length {σ τ : MSt} (h : σ.length ≤ τ.length) : (σ.back τ).length = σ.length := by
  simp [MSt.back]; omega

theorem joinIf_length {t e r : MSt} {tc ec : Bool} {st st' : DSt} {k : Nat}
    (h : (joinIf t tc e ec).run st = .ok (r, st')) (ht : t.length = k) (he : e.length = k) : r.length = k := by
  unfold joinIf at h
  split at h
  · cases h
  · cases h
    split <;> simp [initFrom_length, initMeet_length, ht, he]

theorem foldl_initMeet_length : ∀ (ms : List MSt) (m : MSt) (k : Nat), m.length = k →
    (∀ x, x ∈ ms → x.length = k) → (ms.foldl MSt.initMeet m).length = k
  | [], _, _, hm, _ => hm
  | n :: ms, m, k, hm, hs => foldl_initMeet_length ms (m.initMeet n) k
      (by simp [initMeet_length, hm, hs n (.head _)]) (fun x hx => hs x (.tail _ hx))

theorem joinMatch_length {σ r : MSt} {arms : List (MSt × Bool)} {st st' : DSt} {k : Nat}
    (h : (joinMatch σ arms).run st = .ok (r, st')) (hσ : σ.length = k) (ha : ∀ a, a ∈ arms → a.1.length = k) :
    r.length = k := by
  unfold joinMatch at h
  simp only at h
  split at h
  · next m ms hm =>
    split at h
    · cases h
    · cases h
      have mem : ∀ x, x ∈ (arms.filter (·.2)).map (·.1) → x.length = k := by
        intro x hx
        obtain ⟨a, ha', e⟩ := List.mem_map.mp hx
        exact e ▸ ha a (List.mem_filter.mp ha').1
      rw [hm] at mem
      exact foldl_initMeet_length ms m k (mem m (.head _)) (fun x hx => mem x (.tail _ hx))
  · split at h
    · next f _ l _ hf hl =>
      cases h
      simp only [initFrom_length, ha _ (List.mem_of_mem_head? hf), ha _ (List.mem_of_mem_getLast? hl)]
      simp
    · cases h; exact hσ

theorem joinAll_length {code : Lit.Code} {σ r : MSt} {k : Nat} : ∀ {L : List MSt} {st st' : DSt},
    (joinAll code σ L).run st = .ok (r, st') → σ.length = k → (∀ x, x ∈ L → x.length = k) → r.length = k
  | [], _, _, h, hσ, _ => by cases h; exact hσ
  | [_], _, _, h, _, hL => by cases h; exact hL _ (.head _)
  | τ :: w :: L, st, st', h, hσ, hL => by
    rw [joinAll_cons_cons] at h
    unfold MSt.join at h
    split at h
    · next j hj =>
      split at hj
      · cases hj
        exact joinAll_length h hσ (fun x hx => by
          cases hx with
          | head => simp [initMeet_length, hL τ (.head _), hL w (.tail _ (.head _))]
          | tail _ hx => exact hL x (.tail _ (.tail _ hx)))
      · cases hj
    · cases h
termination_by L => L.length


theorem bindM_len {Γ' : Ctx} {xs : List String} {init writable konst : Bool} :
    ∀ {σ σ' : MSt} {st st' : DSt}, (bindM Γ' σ xs init writable konst).run st = .ok (σ', st') →
      σ.length ≤ σ'.length := by
  induction xs with
  | nil => intro σ σ' st st' h; cases h; exact Nat.le_refl _
  | cons x xs ih =>
    intro σ σ' st st' h
    unfold bindM at h
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    simp only [bindOne, MoveM.run_bind] at h1
    cases h1
    exact Nat.le_trans (Nat.le_succ _) (ih h2)

theorem bindFor_len {Γf : Ctx} : ∀ (xs : List (String × Bool)) {σ σ' : MSt} {st st' : DSt},
    (xs.foldlM (fun σ (x, r) => bindOne Γf false true true false r σ x) σ).run st = .ok (σ', st') →
    σ.length ≤ σ'.length
  | [], _, _, _, _, h => by cases h; exact Nat.le_refl _
  | (x, r) :: xs, σ, σ', st, st', h => by
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    simp only [bindOne, MoveM.run_bind] at h1
    cases h1
    exact Nat.le_trans (Nat.le_succ _) (bindFor_len xs h2)

theorem leavingIf_below0 {id : Nat} {σ σ' : MSt} {c : Bool} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (h : (leavingIf id σ c m).run st = .ok (σ', st')) : σ'.Below σ := by
  unfold leavingIf at h
  split at h
  · obtain ⟨_, st1, _, h2⟩ := MoveM.bind_ok h
    obtain ⟨σ1, st2, h3, h4⟩ := MoveM.bind_ok h2
    obtain ⟨_, _, _, h5⟩ := MoveM.bind_ok h4
    obtain ⟨_, _, _, h6⟩ := MoveM.bind_ok h5
    obtain ⟨_, _, _, h7⟩ := MoveM.bind_ok h6
    cases h7
    exact (hm _ _ _ h3).1
  · exact (hm st σ' st' h).1

theorem leavingIf_len {id : Nat} {σ σ' : MSt} {c : Bool} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (h : (leavingIf id σ c m).run st = .ok (σ', st')) : σ'.length = σ.length :=
  (leavingIf_below0 hm h).length

/-- What a walk leaves, as long as it needs to be: the state it was given
or longer, and every `break` and `continue` at least as long as the
innermost loop's scope. -/
def LenOk (σ : MSt) (base : Nat) (r : MOut) : Prop :=
  σ.length ≤ r.1.length ∧ (∀ τ, τ ∈ r.2.1 → base ≤ τ.length) ∧ (∀ τ, τ ∈ r.2.2 → base ≤ τ.length)

theorem mBranch_len {Γb : Ctx} {σc : MSt} {binds : List String} {id : Nat} {kind : String}
    {arm : Option Nat} {walk : MSt → MoveM MOut} {base : Nat} {st st' : DSt} {rb : MOut}
    (hb : base ≤ σc.length)
    (hwalk : ∀ σb st1 r st2, base ≤ σb.length → (walk σb).run st1 = .ok (r, st2) → LenOk σb base r)
    (h : (mBranch Γb σc binds id kind arm walk).run st = .ok (rb, st')) : LenOk σc base rb := by
  unfold mBranch at h
  obtain ⟨σb, st1, h1, h⟩ := MoveM.bind_ok h
  have l1 := bindM_len h1
  obtain ⟨r, st2, h2, h⟩ := MoveM.bind_ok h
  have l2 := hwalk σb st1 r st2 (Nat.le_trans hb l1) h2
  obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
  obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
  cases h
  exact ⟨Nat.le_trans l1 l2.1, l2.2.1, l2.2.2⟩


theorem update_length (f : MVar → MVar) (x : String) : ∀ (σ : MSt), (MSt.update f x σ).length = σ.length
  | [] => rfl
  | v :: σ => by simp only [MSt.update]; split <;> simp [update_length f x σ]

theorem mSet_len {sigs : List (String × Sig)} {Γ : Ctx} {id : Nat} {σ σ' : MSt} {x : String}
    {steps : List Step} {op : Option BinOp} {e : Expr} {st st' : DSt}
    (h : (mSet sigs Γ id σ x steps op e).run st = .ok (σ', st')) : σ'.length = σ.length := by
  unfold mSet at h
  obtain ⟨_, _, _, h⟩ := MoveM.bind_ok h
  obtain ⟨_, _, _, h⟩ := MoveM.bind_ok h
  obtain ⟨σ1, st2, h1, h⟩ := MoveM.bind_ok h
  have l1 := leavingIf_len (mSetParts_shr sigs Γ σ steps op e) h1
  obtain ⟨σ2, st3, h2, h⟩ := MoveM.bind_ok h
  have l2 := ((mTarget_shr σ1 x steps op) _ _ _ h2).1.length
  obtain ⟨_, _, _, h⟩ := MoveM.bind_ok h
  obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
  obtain ⟨σ4, st5, h4, h⟩ := MoveM.bind_ok h
  have e4 : σ4 = σ2 := by
    split at h4
    · exact (exitM_eq h4).1
    · cases h4; rfl
  simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
  obtain ⟨rfl, rfl⟩ := h
  subst e4
  split
  · rw [MSt.give, update_length, l2, l1]
  · rw [l2, l1]


theorem LenOk.simple {σ τ : MSt} {base : Nat} (h : σ.length ≤ τ.length) : LenOk σ base (τ, [], []) := by
  unfold LenOk
  refine ⟨h, ?_, ?_⟩ <;> intro _ hx <;> cases hx

section
variable (sigs : List (String × Sig)) (env : InferEnv)

theorem walk_len : ∀ n,
    (∀ Γ σ base s st r st', base ≤ σ.length → (mStmt sigs env n Γ σ base s).run st = .ok (r, st') →
      LenOk σ base r) ∧
    (∀ Γ σ base ss st r st', base ≤ σ.length → (mBlock sigs env n Γ σ base ss).run st = .ok (r, st') →
      LenOk σ base r) ∧
    (∀ Γ vs σ base id i cs st r st', base ≤ σ.length →
      (mCases sigs env n Γ vs σ base id i cs).run st = .ok (r, st') →
      (∀ a, a ∈ r.1 → a.1.length = σ.length) ∧ (∀ τ, τ ∈ r.2.1 → base ≤ τ.length) ∧
        (∀ τ, τ ∈ r.2.2 → base ≤ τ.length))
  | 0 => by
    refine ⟨fun Γ σ base s st r st' hb h => ?_, fun Γ σ base ss st r st' hb h => ?_,
      fun Γ vs σ base id i cs st r st' hb h => ?_⟩
    · simp only [mStmt] at h; cases h; exact LenOk.simple (Nat.le_refl _)
    · simp only [mBlock] at h; cases h; exact LenOk.simple (Nat.le_refl _)
    · simp only [mCases] at h; cases h
      refine ⟨?_, ?_, ?_⟩ <;> intro _ h <;> cases h
  | n + 1 => by
    obtain ⟨IS, IB, IC⟩ := walk_len n
    have br : ∀ {Γb σc binds id kind arm body st rb st'} {Γ' : Ctx} {base : Nat}, base ≤ σc.length →
        (mBranch Γb σc binds id kind arm fun σb => mBlock sigs env n Γ' σb base body).run st = .ok (rb, st') →
        LenOk σc base rb := fun hb h =>
      mBranch_len hb (fun σb st1 r st2 hb' h' => IB _ σb _ _ st1 r st2 hb' h') h
    refine ⟨fun Γ σ base s st r st' hb h => ?_, fun Γ σ base ss st r st' hb h => ?_,
      fun Γ vs σ base id i cs st r st' hb h => ?_⟩
    · unfold mStmt at h
      obtain ⟨id, st1, _, h⟩ := MoveM.bind_ok h
      cases s with
      | let_ p t e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        cases h
        exact LenOk.simple (by rw [← leavingIf_len (mE_shr sigs Γ false σ e) h2]; exact bindM_len h3)
      | var_ p t e =>
        dsimp only at h
        cases e with
        | some e =>
          obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
          obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
          cases h
          exact LenOk.simple (by rw [← leavingIf_len (mE_shr sigs Γ false σ e) h2]; exact bindM_len h3)
        | none =>
          cases t with
          | some _ =>
            obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
            obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
            cases h
            exact LenOk.simple (bindM_len h2)
          | none => cases h; exact LenOk.simple (Nat.le_refl _)
      | set_ x steps op e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        cases h
        exact LenOk.simple (Nat.le_of_eq (mSet_len h2).symm)
      | fun_ _ => cases h; exact LenOk.simple (Nat.le_refl _)
      | type_ _ _ => cases h; exact LenOk.simple (Nat.le_refl _)
      | ret e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨e2, _⟩ := exitM_eq h2
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        have l3 : σ2.length = σ1.length := by
          cases e with
          | some e => exact leavingIf_len (mE_shr sigs Γ false σ1 e) h3
          | none => cases h3; rfl
        obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st5, _, h⟩ := MoveM.bind_ok h
        cases h
        exact LenOk.simple (Nat.le_of_eq (by rw [l3, e2]))
      | brk =>
        dsimp only at h
        obtain ⟨_, st2, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
        cases h
        refine ⟨Nat.le_refl _, fun τ hτ => ?_, (fun _ h => by cases h)⟩
        cases hτ with
        | head => exact hb
        | tail _ h => cases h
      | cont =>
        dsimp only at h
        obtain ⟨_, st2, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
        cases h
        refine ⟨Nat.le_refl _, (fun _ h => by cases h), fun τ hτ => ?_⟩
        cases hτ with
        | head => exact hb
        | tail _ h => cases h
      | debuglog e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        cases h
        exact LenOk.simple (Nat.le_of_eq (leavingIf_len (mE_shr sigs Γ true σ e) h2).symm)
      | call e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        cases h
        exact LenOk.simple (Nat.le_of_eq (leavingIf_len (mE_shr sigs Γ false σ e) h2).symm)
      | const_ x t e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        cases h
        exact LenOk.simple (by rw [← leavingIf_len (mE_shr sigs _ false σ e) h2]; exact bindM_len h3)
      | if_ c tB eB t e =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have lc := leavingIf_len (mE_shr sigs Γ false σ c) h2
        have hbc : base ≤ σc.length := lc ▸ hb
        obtain ⟨rt, st3, h3, h⟩ := MoveM.bind_ok h
        have lt := br hbc h3
        obtain ⟨re, st4, h4, h⟩ := MoveM.bind_ok h
        have le : LenOk σc base re := by
          cases e with
          | some eb => dsimp only at h4; exact br hbc h4
          | none => cases h4; exact LenOk.simple (Nat.le_refl _)
        obtain ⟨σ', st5, h5, h⟩ := MoveM.bind_ok h
        have l5 := joinIf_length h5 (back_length lt.1) (back_length le.1)
        cases h
        refine ⟨Nat.le_of_eq (by rw [l5, lc]), fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact lt.2.1 τ hτ
          · exact le.2.1 τ hτ
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact lt.2.2 τ hτ
          · exact le.2.2 τ hτ
      | loop c body =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have lc : σc.length = σ.length := by
          cases c with
          | some c => exact leavingIf_len (mE_shr sigs Γ false σ c) h2
          | none => cases h2; rfl
        obtain ⟨r, st3, h3, h⟩ := MoveM.bind_ok h
        have lb := IB _ _ _ _ _ _ _ (Nat.le_refl _) h3
        obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st5, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st6, _, h⟩ := MoveM.bind_ok h
        obtain ⟨σ', st7, h7, h⟩ := MoveM.bind_ok h
        unfold loopEnd at h7
        dsimp only at h7
        have h7 := MoveM.ite_throw h7
        have l7 := joinAll_length h7 rfl (fun x hx => by
          simp only [List.mem_append, List.mem_map] at hx
          rcases hx with hx | ⟨b, hb', e⟩
          · split at hx
            · simp at hx; rw [hx]
            · cases hx
          · rw [← e]; exact back_length (lb.2.1 b hb'))
        cases h
        exact LenOk.simple (Nat.le_of_eq (by rw [l7, lc]))
      | for_ names coll body =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have lc := leavingIf_len (mE_shr sigs Γ true σ coll) h2
        obtain ⟨σb, st3, h3, h⟩ := MoveM.bind_ok h
        have lw : σc.length ≤ σb.length := by
          have := bindFor_len _ h3
          split at this
          · rw [MSt.walk, update_length] at this; exact this
          · exact this
        obtain ⟨r, st4, h4, h⟩ := MoveM.bind_ok h
        have lb := IB _ _ _ _ _ _ _ lw h4
        obtain ⟨_, st5, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st6, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st7, _, h⟩ := MoveM.bind_ok h
        obtain ⟨σ', st8, h8, h⟩ := MoveM.bind_ok h
        unfold loopEnd at h8
        dsimp only at h8
        have h8 := MoveM.ite_throw h8
        have l8 := joinAll_length h8 rfl (fun x hx => by
          simp only [List.mem_append, List.mem_map] at hx
          rcases hx with hx | ⟨b, hb', e⟩
          · simp at hx; rw [hx]
          · rw [← e]; exact back_length (lb.2.1 b hb'))
        cases h
        exact LenOk.simple (Nat.le_of_eq (by rw [l8, lc]))
      | match_ e cases dflt =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have lc := leavingIf_len (mE_shr sigs Γ false σ e) h2
        have hbc : base ≤ σc.length := lc ▸ hb
        obtain ⟨r, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨a3, b3, c3⟩ := IC _ _ _ _ _ _ _ _ _ _ hbc h3
        obtain ⟨d, st4, h4, h⟩ := MoveM.bind_ok h
        have r4 : (∀ a, a ∈ d.1 → a.1.length = σc.length) ∧ (∀ τ, τ ∈ d.2.1 → base ≤ τ.length) ∧
            (∀ τ, τ ∈ d.2.2 → base ≤ τ.length) := by
          cases dflt with
          | some db =>
            dsimp only at h4
            obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
            have l1 := br hbc g1
            cases g
            refine ⟨fun a ha => ?_, l1.2.1, l1.2.2⟩
            cases ha with
            | head => exact back_length l1.1
            | tail _ h => cases h
          | none => cases h4; refine ⟨?_, ?_, ?_⟩ <;> intro _ h <;> cases h
        obtain ⟨a4, b4, c4⟩ := r4
        obtain ⟨σ', st5, h5, h⟩ := MoveM.bind_ok h
        have l5 := joinMatch_length h5 rfl (fun a ha => by
          rcases List.mem_append.mp ha with ha | ha
          · exact a3 a ha
          · exact a4 a ha)
        cases h
        refine ⟨Nat.le_of_eq (by rw [l5, lc]), fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact b3 τ hτ
          · exact b4 τ hτ
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact c3 τ hτ
          · exact c4 τ hτ
    · cases ss with
      | nil => unfold mBlock at h; cases h; exact LenOk.simple (Nat.le_refl _)
      | cons s ss =>
        unfold mBlock at h
        obtain ⟨r1, st1, h1, h⟩ := MoveM.bind_ok h
        have l1 := IS _ _ _ _ _ _ _ hb h1
        obtain ⟨r2, st2, h2, h⟩ := MoveM.bind_ok h
        have l2 := IB _ _ _ _ _ _ _ (Nat.le_trans hb l1.1) h2
        cases h
        refine ⟨Nat.le_trans l1.1 l2.1, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact l1.2.1 τ hτ
          · exact l2.2.1 τ hτ
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact l1.2.2 τ hτ
          · exact l2.2.2 τ hτ
    · cases cs with
      | nil => unfold mCases at h; cases h; refine ⟨?_, ?_, ?_⟩ <;> intro _ h <;> cases h
      | cons c cs =>
        have tail : ∀ {rb : MOut} {st1 : DSt}, LenOk σ base rb →
            ∀ {rest : List (MSt × Bool) × List MSt × List MSt}, (do
              let r ← mCases sigs env n Γ vs σ base id (i + 1) cs
              pure ((σ.back rb.1, completes (match c with | .atom _ b => b | .term _ _ b => b)) :: r.1,
                rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2) : MoveM _).run st1 = .ok (rest, st') →
            (∀ a, a ∈ rest.1 → a.1.length = σ.length) ∧ (∀ τ, τ ∈ rest.2.1 → base ≤ τ.length) ∧
              (∀ τ, τ ∈ rest.2.2 → base ≤ τ.length) := by
          intro rb st1 lb rest g
          obtain ⟨r, s3, g3, g⟩ := MoveM.bind_ok g
          obtain ⟨a3, b3, c3⟩ := IC _ _ _ _ _ _ _ _ _ _ hb g3
          cases g
          refine ⟨fun a ha => ?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
          · cases ha with
            | head => exact back_length lb.1
            | tail _ h => exact a3 a h
          · rcases List.mem_append.mp hτ with hτ | hτ
            · exact lb.2.1 τ hτ
            · exact b3 τ hτ
          · rcases List.mem_append.mp hτ with hτ | hτ
            · exact lb.2.2 τ hτ
            · exact c3 τ hτ
        cases c with
        | atom a b =>
          unfold mCases at h
          obtain ⟨rb, st1, h1, h⟩ := MoveM.bind_ok h
          exact tail (br hb h1) h
        | term a x b =>
          unfold mCases at h
          dsimp only at h
          obtain ⟨rb, st1, h1, h⟩ := MoveM.bind_ok h
          exact tail (br hb h1) h

end


/-! ## What the walk keeps of the bindings it was given -/

/-- Two states of the same bindings in the same places. -/
inductive MSt.Ids : MSt → MSt → Prop where
  | nil : MSt.Ids [] []
  | cons {w v : MVar} {τ σ : MSt} : w.uid = v.uid → w.owns = v.owns → MSt.Ids τ σ → MSt.Ids (w :: τ) (v :: σ)

theorem MSt.Ids.refl : ∀ (σ : MSt), MSt.Ids σ σ
  | [] => .nil
  | _ :: σ => .cons rfl rfl (MSt.Ids.refl σ)

theorem MSt.Ids.trans : ∀ {a b c : MSt}, MSt.Ids a b → MSt.Ids b c → MSt.Ids a c
  | [], [], [], .nil, .nil => .nil
  | _ :: _, _ :: _, _ :: _, .cons u1 o1 t1, .cons u2 o2 t2 => .cons (u1.trans u2) (o1.trans o2) (MSt.Ids.trans t1 t2)

theorem MSt.Ids.symm : ∀ {a b : MSt}, MSt.Ids a b → MSt.Ids b a
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons u o t => .cons u.symm o.symm (MSt.Ids.symm t)

theorem MSt.Ids.length : ∀ {a b : MSt}, MSt.Ids a b → a.length = b.length
  | [], [], .nil => rfl
  | _ :: _, _ :: _, .cons _ _ t => by simp [MSt.Ids.length t]

theorem MSt.Ids.drop (k : Nat) : ∀ {a b : MSt}, MSt.Ids a b → MSt.Ids (a.drop k) (b.drop k)
  | [], [], .nil => by simp; exact .nil
  | _ :: _, _ :: _, .cons u o t => by
    cases k with
    | zero => exact .cons u o t
    | succ k => simpa using MSt.Ids.drop k t

theorem MSt.Ids.of_below : ∀ {τ σ : MSt}, τ.Below σ → MSt.Ids τ σ
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons b t => .cons b.2.1 b.2.2.1 (MSt.Ids.of_below t)

theorem MSt.Ids.update {f : MVar → MVar} (hf : ∀ v, (f v).uid = v.uid ∧ (f v).owns = v.owns) (x : String) :
    ∀ (σ : MSt), MSt.Ids (MSt.update f x σ) σ
  | [] => .nil
  | v :: σ => by
    simp only [MSt.update]
    split
    · exact .cons (hf v).1 (hf v).2 (MSt.Ids.refl σ)
    · exact .cons rfl rfl (MSt.Ids.update hf x σ)

theorem ids_zipWith {f : MVar → MVar → MVar} (hf : ∀ v w, (f v w).uid = v.uid ∧ (f v w).owns = v.owns) :
    ∀ (σ τ : MSt), σ.length = τ.length → MSt.Ids (List.zipWith f σ τ) σ
  | [], [], _ => .nil
  | v :: σ, w :: τ, h => by
    simp only [List.zipWith_cons_cons]
    exact .cons (hf v w).1 (hf v w).2 (ids_zipWith hf σ τ (by simpa using h))

/-- The suffix of `σ` as long as `k`. -/
def sfx (k : Nat) (σ : MSt) : MSt := σ.drop (σ.length - k)

theorem sfx_length {k : Nat} {σ : MSt} (h : k ≤ σ.length) : (sfx k σ).length = k := by
  simp [sfx]; omega

theorem sfx_sfx {k m : Nat} {σ : MSt} (h1 : k ≤ m) (h2 : m ≤ σ.length) : sfx k (sfx m σ) = sfx k σ := by
  simp only [sfx, List.drop_drop, List.length_drop]
  congr 1; omega

theorem sfx_self (σ : MSt) : sfx σ.length σ = σ := by simp [sfx]

theorem sfx_append {k : Nat} {new σ : MSt} (h : k ≤ σ.length) : sfx k (new ++ σ) = sfx k σ := by
  simp only [sfx, List.length_append]
  rw [show new.length + σ.length - k = new.length + (σ.length - k) by omega, List.drop_length_add_append]

theorem MSt.Ids.sfx (k : Nat) {a b : MSt} (h : MSt.Ids a b) : MSt.Ids (Datalove.Fun.sfx k a) (Datalove.Fun.sfx k b) := by
  unfold Datalove.Fun.sfx; rw [h.length]; exact h.drop _

theorem back_eq_sfx (σ τ : MSt) : σ.back τ = sfx σ.length τ := rfl


theorem bindM_shape {Γ' : Ctx} {xs : List String} {init writable konst : Bool} :
    ∀ {σ σ' : MSt} {st st' : DSt}, (bindM Γ' σ xs init writable konst).run st = .ok (σ', st') →
      ∃ new, σ' = new ++ σ := by
  induction xs with
  | nil => intro σ σ' st st' h; cases h; exact ⟨[], rfl⟩
  | cons x xs ih =>
    intro σ σ' st st' h
    unfold bindM at h
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    have e1 : ∃ v, σ1 = v :: σ := by
      simp only [bindOne, MoveM.run_bind] at h1
      cases h1; exact ⟨_, rfl⟩
    obtain ⟨v, rfl⟩ := e1
    obtain ⟨new, e⟩ := ih h2
    exact ⟨new ++ [v], by rw [e]; simp⟩

theorem bindFor_shape {Γf : Ctx} : ∀ (xs : List (String × Bool)) {σ σ' : MSt} {st st' : DSt},
    (xs.foldlM (fun σ (x, r) => bindOne Γf false true true false r σ x) σ).run st = .ok (σ', st') →
    ∃ new, σ' = new ++ σ
  | [], _, _, _, _, h => by cases h; exact ⟨[], rfl⟩
  | (x, r) :: xs, σ, σ', st, st', h => by
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    have e1 : ∃ v, σ1 = v :: σ := by
      simp only [bindOne, MoveM.run_bind] at h1
      cases h1; exact ⟨_, rfl⟩
    obtain ⟨v, rfl⟩ := e1
    obtain ⟨new, e⟩ := bindFor_shape xs h2
    exact ⟨new ++ [v], by rw [e]; simp⟩

theorem joinIf_ids {t e r : MSt} {tc ec : Bool} {st st' : DSt} {B : MSt}
    (h : (joinIf t tc e ec).run st = .ok (r, st')) (ht : MSt.Ids t B) (he : MSt.Ids e B) : MSt.Ids r B := by
  have hl : t.length = e.length := ht.length.trans he.length.symm
  unfold joinIf at h
  split at h
  · cases h
  · cases h
    split
    · exact (ids_zipWith (f := fun v w => { v with init := w.init }) (fun _ _ => ⟨rfl, rfl⟩) _ _
        (by simp [initMeet_length, hl])).trans he
    · exact (ids_zipWith (f := fun v w => { v with init := w.init }) (fun _ _ => ⟨rfl, rfl⟩) _ _
        (by simp [initMeet_length, hl])).trans ht

theorem foldl_initMeet_ids : ∀ (ms : List MSt) (m : MSt) {B : MSt}, MSt.Ids m B → (∀ x, x ∈ ms → MSt.Ids x B) →
    MSt.Ids (ms.foldl MSt.initMeet m) B
  | [], _, _, hm, _ => hm
  | n :: ms, m, _, hm, hs => foldl_initMeet_ids ms (m.initMeet n)
      ((ids_zipWith (f := fun v w => { v with init := v.init && w.init }) (fun _ _ => ⟨rfl, rfl⟩) _ _
        (hm.length.trans (hs n (.head _)).length.symm)).trans hm) (fun x hx => hs x (.tail _ hx))

theorem joinMatch_ids {σ r : MSt} {arms : List (MSt × Bool)} {st st' : DSt} {B : MSt}
    (h : (joinMatch σ arms).run st = .ok (r, st')) (hσ : MSt.Ids σ B) (ha : ∀ a, a ∈ arms → MSt.Ids a.1 B) :
    MSt.Ids r B := by
  unfold joinMatch at h
  simp only at h
  split at h
  · next m ms hm =>
    split at h
    · cases h
    · cases h
      have mem : ∀ x, x ∈ (arms.filter (·.2)).map (·.1) → MSt.Ids x B := by
        intro x hx
        obtain ⟨a, ha', e⟩ := List.mem_map.mp hx
        exact e ▸ ha a (List.mem_filter.mp ha').1
      rw [hm] at mem
      exact foldl_initMeet_ids ms m (mem m (.head _)) (fun x hx => mem x (.tail _ hx))
  · split at h
    · next f _ l _ hf hl =>
      cases h
      have hf' := ha _ (List.mem_of_mem_head? hf)
      have hl' := ha _ (List.mem_of_mem_getLast? hl)
      exact (ids_zipWith (f := fun v w => { v with init := w.init }) (fun _ _ => ⟨rfl, rfl⟩) _ _
        (hf'.length.trans hl'.length.symm)).trans hf'
    · cases h; exact hσ

theorem joinAll_ids {code : Lit.Code} {σ r : MSt} {B : MSt} : ∀ {L : List MSt} {st st' : DSt},
    (joinAll code σ L).run st = .ok (r, st') → MSt.Ids σ B → (∀ x, x ∈ L → MSt.Ids x B) → MSt.Ids r B
  | [], _, _, h, hσ, _ => by cases h; exact hσ
  | [_], _, _, h, _, hL => by cases h; exact hL _ (.head _)
  | τ :: w :: L, st, st', h, hσ, hL => by
    rw [joinAll_cons_cons] at h
    unfold MSt.join at h
    split at h
    · next j hj =>
      split at hj
      · cases hj
        exact joinAll_ids h hσ (fun x hx => by
          cases hx with
          | head =>
            exact (ids_zipWith (f := fun v w => { v with init := v.init && w.init }) (fun _ _ => ⟨rfl, rfl⟩) _ _
              ((hL τ (.head _)).length.trans (hL w (.tail _ (.head _))).length.symm)).trans (hL τ (.head _))
          | tail _ hx => exact hL x (.tail _ (.tail _ hx)))
      · cases hj
    · cases h
termination_by L => L.length


/-- What a walk keeps of the bindings it was given: them, at the bottom of
what it leaves, numbered and owned as they were; and at the bottom of every
`break` and `continue`, the innermost loop's scope as it was. -/
def ShapeOk (σ : MSt) (base : Nat) (r : MOut) : Prop :=
  MSt.Ids (sfx σ.length r.1) σ ∧ (∀ τ, τ ∈ r.2.1 → MSt.Ids (sfx base τ) (sfx base σ)) ∧
    (∀ τ, τ ∈ r.2.2 → MSt.Ids (sfx base τ) (sfx base σ))

theorem ShapeOk.simple {σ τ : MSt} {base : Nat} (h : MSt.Ids τ σ) : ShapeOk σ base (τ, [], []) := by
  unfold ShapeOk
  refine ⟨?_, ?_, ?_⟩
  · rw [← h.length, sfx_self]; exact h
  all_goals intro _ hx; cases hx

theorem mBranch_shape {Γb : Ctx} {σc : MSt} {binds : List String} {id : Nat} {kind : String}
    {arm : Option Nat} {walk : MSt → MoveM MOut} {base : Nat} {st st' : DSt} {rb : MOut}
    (hb : base ≤ σc.length)
    (hwalk : ∀ σb st1 r st2, base ≤ σb.length → (walk σb).run st1 = .ok (r, st2) → LenOk σb base r ∧ ShapeOk σb base r)
    (h : (mBranch Γb σc binds id kind arm walk).run st = .ok (rb, st')) :
    MSt.Ids (σc.back rb.1) σc ∧ (∀ τ, τ ∈ rb.2.1 → MSt.Ids (sfx base τ) (sfx base σc)) ∧
      (∀ τ, τ ∈ rb.2.2 → MSt.Ids (sfx base τ) (sfx base σc)) := by
  unfold mBranch at h
  obtain ⟨σb, st1, h1, h⟩ := MoveM.bind_ok h
  obtain ⟨new, rfl⟩ := bindM_shape h1
  obtain ⟨r, st2, h2, h⟩ := MoveM.bind_ok h
  have hbl : base ≤ (new ++ σc).length := by simp; omega
  obtain ⟨l2, s2a, s2b, s2c⟩ := hwalk _ st1 r st2 hbl h2
  obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
  obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
  cases h
  have hσ : σc.length ≤ (new ++ σc).length := by simp
  have base_eq : sfx base (new ++ σc) = sfx base σc := sfx_append hb
  refine ⟨?_, fun τ hτ => base_eq ▸ s2b τ hτ, fun τ hτ => base_eq ▸ s2c τ hτ⟩
  rw [back_eq_sfx, ← sfx_sfx hσ l2.1]
  have := s2a.sfx σc.length
  rwa [sfx_append (Nat.le_refl _), sfx_self] at this


theorem mSet_ids {sigs : List (String × Sig)} {Γ : Ctx} {id : Nat} {σ σ' : MSt} {x : String}
    {steps : List Step} {op : Option BinOp} {e : Expr} {st st' : DSt}
    (h : (mSet sigs Γ id σ x steps op e).run st = .ok (σ', st')) : MSt.Ids σ' σ := by
  unfold mSet at h
  obtain ⟨_, _, _, h⟩ := MoveM.bind_ok h
  obtain ⟨_, _, _, h⟩ := MoveM.bind_ok h
  obtain ⟨σ1, st2, h1, h⟩ := MoveM.bind_ok h
  have b1 := leavingIf_below0 (mSetParts_shr sigs Γ σ steps op e) h1
  obtain ⟨σ2, st3, h2, h⟩ := MoveM.bind_ok h
  have b2 := ((mTarget_shr σ1 x steps op) _ _ _ h2).1
  obtain ⟨_, _, _, h⟩ := MoveM.bind_ok h
  obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
  obtain ⟨σ4, st5, h4, h⟩ := MoveM.bind_ok h
  have e4 : σ4 = σ2 := by
    split at h4
    · exact (exitM_eq h4).1
    · cases h4; rfl
  simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h
  obtain ⟨rfl, rfl⟩ := h
  subst e4
  have i2 : MSt.Ids σ4 σ := (MSt.Ids.of_below b2).trans (MSt.Ids.of_below b1)
  split
  · exact (MSt.Ids.update (f := fun v => { v with init := true, live := true }) (fun _ => ⟨rfl, rfl⟩) x σ4).trans i2
  · exact i2

theorem appendShape {σ σ1 new : MSt} {base : Nat} (b : σ1.Below σ) : ShapeOk σ base (new ++ σ1, [], []) := by
  unfold ShapeOk
  refine ⟨?_, (fun _ h => by cases h), (fun _ h => by cases h)⟩
  have l := b.length
  show MSt.Ids (sfx σ.length (new ++ σ1)) σ
  rw [← l, sfx_append (Nat.le_refl _), sfx_self]
  exact MSt.Ids.of_below b

section
variable (sigs : List (String × Sig)) (env : InferEnv)

theorem walk_shape : ∀ n,
    (∀ Γ σ base s st r st', base ≤ σ.length → (mStmt sigs env n Γ σ base s).run st = .ok (r, st') →
      ShapeOk σ base r) ∧
    (∀ Γ σ base ss st r st', base ≤ σ.length → (mBlock sigs env n Γ σ base ss).run st = .ok (r, st') →
      ShapeOk σ base r) ∧
    (∀ Γ vs σ base id i cs st r st', base ≤ σ.length →
      (mCases sigs env n Γ vs σ base id i cs).run st = .ok (r, st') →
      (∀ a, a ∈ r.1 → MSt.Ids a.1 σ) ∧ (∀ τ, τ ∈ r.2.1 → MSt.Ids (sfx base τ) (sfx base σ)) ∧
        (∀ τ, τ ∈ r.2.2 → MSt.Ids (sfx base τ) (sfx base σ)))
  | 0 => by
    refine ⟨fun Γ σ base s st r st' hb h => ?_, fun Γ σ base ss st r st' hb h => ?_,
      fun Γ vs σ base id i cs st r st' hb h => ?_⟩
    · simp only [mStmt] at h; cases h; exact ShapeOk.simple (MSt.Ids.refl _)
    · simp only [mBlock] at h; cases h; exact ShapeOk.simple (MSt.Ids.refl _)
    · simp only [mCases] at h; cases h
      refine ⟨?_, ?_, ?_⟩ <;> intro _ h <;> cases h
  | n + 1 => by
    obtain ⟨IS, IB, IC⟩ := walk_shape n
    obtain ⟨LS, LB, LC⟩ := walk_len sigs env n
    have br : ∀ {Γb σc binds id kind arm body st rb st'} {Γ' : Ctx} {base : Nat}, base ≤ σc.length →
        (mBranch Γb σc binds id kind arm fun σb => mBlock sigs env n Γ' σb base body).run st = .ok (rb, st') →
        MSt.Ids (σc.back rb.1) σc ∧ (∀ τ, τ ∈ rb.2.1 → MSt.Ids (sfx base τ) (sfx base σc)) ∧
          (∀ τ, τ ∈ rb.2.2 → MSt.Ids (sfx base τ) (sfx base σc)) := fun hb h =>
      mBranch_shape hb (fun σb st1 r st2 hb' h' => ⟨LB _ σb _ _ st1 r st2 hb' h', IB _ σb _ _ st1 r st2 hb' h'⟩) h
    have below_base : ∀ {σ σc : MSt} {base : Nat}, MSt.Ids σc σ → MSt.Ids (sfx base σc) (sfx base σ) :=
      fun h => h.sfx _
    refine ⟨fun Γ σ base s st r st' hb h => ?_, fun Γ σ base ss st r st' hb h => ?_,
      fun Γ vs σ base id i cs st r st' hb h => ?_⟩
    · unfold mStmt at h
      obtain ⟨id, st1, _, h⟩ := MoveM.bind_ok h
      cases s with
      | let_ p t e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        cases h
        obtain ⟨new, rfl⟩ := bindM_shape h3
        exact appendShape (leavingIf_below0 (mE_shr sigs Γ false σ e) h2)
      | var_ p t e =>
        dsimp only at h
        cases e with
        | some e =>
          obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
          obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
          cases h
          obtain ⟨new, rfl⟩ := bindM_shape h3
          exact appendShape (leavingIf_below0 (mE_shr sigs Γ false σ e) h2)
        | none =>
          cases t with
          | some _ =>
            obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
            obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
            cases h
            obtain ⟨new, rfl⟩ := bindM_shape h2
            exact appendShape (MSt.Below.refl σ)
          | none => cases h; exact ShapeOk.simple (MSt.Ids.refl _)
      | set_ x steps op e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        cases h
        exact ShapeOk.simple (mSet_ids h2)
      | fun_ _ => cases h; exact ShapeOk.simple (MSt.Ids.refl _)
      | type_ _ _ => cases h; exact ShapeOk.simple (MSt.Ids.refl _)
      | ret e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨e2, _⟩ := exitM_eq h2
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        have i3 : MSt.Ids σ2 σ1 := by
          cases e with
          | some e => exact MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ false σ1 e) h3)
          | none => cases h3; exact MSt.Ids.refl _
        obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st5, _, h⟩ := MoveM.bind_ok h
        cases h
        exact ShapeOk.simple (e2 ▸ i3)
      | brk =>
        dsimp only at h
        obtain ⟨_, st2, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
        cases h
        unfold ShapeOk
        refine ⟨by rw [sfx_self]; exact MSt.Ids.refl _, fun τ hτ => ?_, (fun _ h => by cases h)⟩
        cases hτ with
        | head => exact MSt.Ids.refl _
        | tail _ h => cases h
      | cont =>
        dsimp only at h
        obtain ⟨_, st2, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st3, _, h⟩ := MoveM.bind_ok h
        cases h
        unfold ShapeOk
        refine ⟨by rw [sfx_self]; exact MSt.Ids.refl _, (fun _ h => by cases h), fun τ hτ => ?_⟩
        cases hτ with
        | head => exact MSt.Ids.refl _
        | tail _ h => cases h
      | debuglog e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        cases h
        exact ShapeOk.simple (MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ true σ e) h2))
      | call e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        cases h
        exact ShapeOk.simple (MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ false σ e) h2))
      | const_ x t e =>
        dsimp only at h
        obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
        obtain ⟨σ2, st3, h3, h⟩ := MoveM.bind_ok h
        cases h
        obtain ⟨new, rfl⟩ := bindM_shape h3
        exact appendShape (leavingIf_below0 (mE_shr sigs _ false σ e) h2)
      | if_ c tB eB t e =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have ic := MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ false σ c) h2)
        have hbc : base ≤ σc.length := ic.length ▸ hb
        obtain ⟨rt, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨t1, t2, t3⟩ := br hbc h3
        obtain ⟨re, st4, h4, h⟩ := MoveM.bind_ok h
        have r4 : MSt.Ids (σc.back re.1) σc ∧ (∀ τ, τ ∈ re.2.1 → MSt.Ids (sfx base τ) (sfx base σc)) ∧
            (∀ τ, τ ∈ re.2.2 → MSt.Ids (sfx base τ) (sfx base σc)) := by
          cases e with
          | some eb => dsimp only at h4; exact br hbc h4
          | none =>
            cases h4
            refine ⟨by rw [back_eq_sfx, sfx_self]; exact MSt.Ids.refl _, (fun _ h => by cases h),
              (fun _ h => by cases h)⟩
        obtain ⟨e1, e2, e3⟩ := r4
        obtain ⟨σ', st5, h5, h⟩ := MoveM.bind_ok h
        have i5 := joinIf_ids h5 t1 e1
        cases h
        unfold ShapeOk
        refine ⟨?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rw [← ic.length, ← i5.length, sfx_self]; exact i5.trans ic
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact (t2 τ hτ).trans (ic.sfx base)
          · exact (e2 τ hτ).trans (ic.sfx base)
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact (t3 τ hτ).trans (ic.sfx base)
          · exact (e3 τ hτ).trans (ic.sfx base)
      | loop c body =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have ic : MSt.Ids σc σ := by
          cases c with
          | some c => exact MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ false σ c) h2)
          | none => cases h2; exact MSt.Ids.refl _
        obtain ⟨r, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨_, b2, _⟩ := IB _ _ _ _ _ _ _ (Nat.le_refl _) h3
        obtain ⟨_, st4, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st5, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st6, _, h⟩ := MoveM.bind_ok h
        obtain ⟨σ', st7, h7, h⟩ := MoveM.bind_ok h
        unfold loopEnd at h7
        dsimp only at h7
        have h7 := MoveM.ite_throw h7
        have i7 := joinAll_ids h7 (MSt.Ids.refl σc) (fun x hx => by
          simp only [List.mem_append, List.mem_map] at hx
          rcases hx with hx | ⟨b, hb', e⟩
          · split at hx
            · simp at hx; rw [hx]; exact MSt.Ids.refl _
            · cases hx
          · rw [← e, back_eq_sfx]
            have := b2 b hb'
            rwa [sfx_self] at this)
        cases h
        exact ShapeOk.simple (i7.trans ic)
      | for_ names coll body =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have ic := MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ true σ coll) h2)
        obtain ⟨σb, st3, h3, h⟩ := MoveM.bind_ok h
        have r3 : ∃ σw, MSt.Ids σw σc ∧ ∃ new, σb = new ++ σw := by
          split at h3
          · obtain ⟨new, e⟩ := bindFor_shape _ h3
            exact ⟨_, MSt.Ids.update (f := fun v => { v with walked := true }) (fun _ => ⟨rfl, rfl⟩) _ σc, new, e⟩
          · obtain ⟨new, e⟩ := bindFor_shape _ h3
            exact ⟨σc, MSt.Ids.refl _, new, e⟩
        obtain ⟨σw, iw, new, hσb⟩ := r3
        obtain ⟨r, st4, h4, h⟩ := MoveM.bind_ok h
        have lb : σc.length ≤ σb.length := by rw [hσb, List.length_append, iw.length]; omega
        obtain ⟨_, b2, _⟩ := IB _ _ _ _ _ _ _ lb h4
        obtain ⟨_, st5, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st6, _, h⟩ := MoveM.bind_ok h
        obtain ⟨_, st7, _, h⟩ := MoveM.bind_ok h
        obtain ⟨σ', st8, h8, h⟩ := MoveM.bind_ok h
        unfold loopEnd at h8
        dsimp only at h8
        have h8 := MoveM.ite_throw h8
        have i8 := joinAll_ids h8 (MSt.Ids.refl σc) (fun x hx => by
          simp only [List.mem_append, List.mem_map] at hx
          rcases hx with hx | ⟨b, hb', e⟩
          · simp at hx; rw [hx]; exact MSt.Ids.refl _
          · rw [← e, back_eq_sfx]
            have := b2 b hb'
            have e' : sfx σc.length σw = σw := by rw [← iw.length, sfx_self]
            rw [hσb, sfx_append (Nat.le_of_eq iw.length.symm), e'] at this
            exact this.trans iw)
        cases h
        exact ShapeOk.simple (i8.trans ic)
      | match_ e cases dflt =>
        dsimp only at h
        obtain ⟨σc, st2, h2, h⟩ := MoveM.bind_ok h
        have ic := MSt.Ids.of_below (leavingIf_below0 (mE_shr sigs Γ false σ e) h2)
        have hbc : base ≤ σc.length := ic.length ▸ hb
        obtain ⟨r, st3, h3, h⟩ := MoveM.bind_ok h
        obtain ⟨a3, b3, c3⟩ := IC _ _ _ _ _ _ _ _ _ _ hbc h3
        obtain ⟨d, st4, h4, h⟩ := MoveM.bind_ok h
        have r4 : (∀ a, a ∈ d.1 → MSt.Ids a.1 σc) ∧ (∀ τ, τ ∈ d.2.1 → MSt.Ids (sfx base τ) (sfx base σc)) ∧
            (∀ τ, τ ∈ d.2.2 → MSt.Ids (sfx base τ) (sfx base σc)) := by
          cases dflt with
          | some db =>
            dsimp only at h4
            obtain ⟨rd, s1, g1, g⟩ := MoveM.bind_ok h4
            obtain ⟨d1, d2, d3⟩ := br hbc g1
            cases g
            refine ⟨fun a ha => ?_, d2, d3⟩
            cases ha with
            | head => exact d1
            | tail _ h => cases h
          | none => cases h4; refine ⟨?_, ?_, ?_⟩ <;> intro _ h <;> cases h
        obtain ⟨a4, b4, c4⟩ := r4
        obtain ⟨σ', st5, h5, h⟩ := MoveM.bind_ok h
        have i5 := joinMatch_ids h5 (MSt.Ids.refl σc) (fun a ha => by
          rcases List.mem_append.mp ha with ha | ha
          · exact a3 a ha
          · exact a4 a ha)
        cases h
        unfold ShapeOk
        refine ⟨?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rw [← ic.length, ← i5.length, sfx_self]; exact i5.trans ic
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact (b3 τ hτ).trans (ic.sfx base)
          · exact (b4 τ hτ).trans (ic.sfx base)
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact (c3 τ hτ).trans (ic.sfx base)
          · exact (c4 τ hτ).trans (ic.sfx base)
    · cases ss with
      | nil => unfold mBlock at h; cases h; exact ShapeOk.simple (MSt.Ids.refl _)
      | cons s ss =>
        unfold mBlock at h
        obtain ⟨r1, st1, h1, h⟩ := MoveM.bind_ok h
        have s1 := IS _ _ _ _ _ _ _ hb h1
        have l1 := LS _ _ _ _ _ _ _ hb h1
        obtain ⟨r2, st2, h2, h⟩ := MoveM.bind_ok h
        have hb1 : base ≤ r1.1.length := Nat.le_trans hb l1.1
        have s2 := IB _ _ _ _ _ _ _ hb1 h2
        have l2 := LB _ _ _ _ _ _ _ hb1 h2
        cases h
        have up : MSt.Ids (sfx base r1.1) (sfx base σ) := by
          rw [← sfx_sfx hb l1.1]; exact s1.1.sfx base
        unfold ShapeOk at s1 s2 ⊢
        refine ⟨?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
        · rw [← sfx_sfx l1.1 l2.1]; exact (s2.1.sfx σ.length).trans s1.1
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact s1.2.1 τ hτ
          · exact (s2.2.1 τ hτ).trans up
        · rcases List.mem_append.mp hτ with hτ | hτ
          · exact s1.2.2 τ hτ
          · exact (s2.2.2 τ hτ).trans up
    · cases cs with
      | nil => unfold mCases at h; cases h; refine ⟨?_, ?_, ?_⟩ <;> intro _ h <;> cases h
      | cons c cs =>
        have tail : ∀ {rb : MOut} {st1 : DSt}, MSt.Ids (σ.back rb.1) σ →
            (∀ τ, τ ∈ rb.2.1 → MSt.Ids (sfx base τ) (sfx base σ)) →
            (∀ τ, τ ∈ rb.2.2 → MSt.Ids (sfx base τ) (sfx base σ)) →
            ∀ {rest : List (MSt × Bool) × List MSt × List MSt}, (do
              let r ← mCases sigs env n Γ vs σ base id (i + 1) cs
              pure ((σ.back rb.1, completes (match c with | .atom _ b => b | .term _ _ b => b)) :: r.1,
                rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2) : MoveM _).run st1 = .ok (rest, st') →
            (∀ a, a ∈ rest.1 → MSt.Ids a.1 σ) ∧ (∀ τ, τ ∈ rest.2.1 → MSt.Ids (sfx base τ) (sfx base σ)) ∧
              (∀ τ, τ ∈ rest.2.2 → MSt.Ids (sfx base τ) (sfx base σ)) := by
          intro rb st1 i1 i2 i3 rest g
          obtain ⟨r, s3, g3, g⟩ := MoveM.bind_ok g
          obtain ⟨a3, b3, c3⟩ := IC _ _ _ _ _ _ _ _ _ _ hb g3
          cases g
          refine ⟨fun a ha => ?_, fun τ hτ => ?_, fun τ hτ => ?_⟩
          · cases ha with
            | head => exact i1
            | tail _ h => exact a3 a h
          · rcases List.mem_append.mp hτ with hτ | hτ
            · exact i2 τ hτ
            · exact b3 τ hτ
          · rcases List.mem_append.mp hτ with hτ | hτ
            · exact i3 τ hτ
            · exact c3 τ hτ
        cases c with
        | atom a b =>
          unfold mCases at h
          obtain ⟨rb, st1, h1, h⟩ := MoveM.bind_ok h
          obtain ⟨i1, i2, i3⟩ := br hb h1
          exact tail i1 i2 i3 h
        | term a x b =>
          unfold mCases at h
          dsimp only at h
          obtain ⟨rb, st1, h1, h⟩ := MoveM.bind_ok h
          obtain ⟨i1, i2, i3⟩ := br hb h1
          exact tail i1 i2 i3 h

end

end Datalove.Fun
