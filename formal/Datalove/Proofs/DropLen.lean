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

theorem leavingIf_len {id : Nat} {σ σ' : MSt} {c : Bool} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (h : (leavingIf id σ c m).run st = .ok (σ', st')) : σ'.length = σ.length := by
  exact (show σ'.Below σ from by
    unfold leavingIf at h
    split at h
    · obtain ⟨_, st1, _, h2⟩ := MoveM.bind_ok h
      obtain ⟨σ1, st2, h3, h4⟩ := MoveM.bind_ok h2
      obtain ⟨_, _, _, h5⟩ := MoveM.bind_ok h4
      obtain ⟨_, _, _, h6⟩ := MoveM.bind_ok h5
      obtain ⟨_, _, _, h7⟩ := MoveM.bind_ok h6
      cases h7
      exact (hm _ _ _ h3).1
    · exact (hm st σ' st' h).1).length

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

end Datalove.Fun
