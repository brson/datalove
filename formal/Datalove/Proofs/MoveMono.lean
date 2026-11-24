import Datalove.Proofs.MoveSound

/-!
# The move analysis accepts more where less is moved

Where the move analysis (`FunMove.lean`) lets an expression through from
some state, in some context, in some position, it lets it through from a
state with more live, in a context where more is copy, and in a position
lending at least as much, and what it leaves is again freer (`MSt.Freer`):
nothing it refuses depends on a variable being moved, and the flag it sets
(`DSt.movedHeld`) it sets no more.

Two uses. A plain `set m[k] = v` takes its key where the run lends it, so
the key's walk is compared with one lending it (`Proofs/MoveStmt.lean`).
And a generic function's body is analyzed in its own context, where a type
parameter is not copy, and run in an instance's, where it may be
(`Proofs/MoveInst.lean`): the instance's walk is the generic's, freer.
-/

namespace Datalove.Fun

/-! ## Freer states -/

/-- `w` is `v` with perhaps more live, and one with it in all else the
analysis asks of a variable. -/
structure MVar.Freer (v w : MVar) : Prop where
  name : v.name = w.name
  init : v.init = w.init
  live : v.live = true → w.live = true
  back : v.back = w.back
  lent : v.lent = w.lent
  writable : v.writable = w.writable
  walked : v.walked = w.walked

theorem MVar.Freer.refl (v : MVar) : v.Freer v := ⟨rfl, rfl, id, rfl, rfl, rfl, rfl⟩

theorem MVar.Freer.trans {u v w : MVar} (h1 : u.Freer v) (h2 : v.Freer w) : u.Freer w :=
  ⟨h1.name.trans h2.name, h1.init.trans h2.init, fun h => h2.live (h1.live h), h1.back.trans h2.back,
    h1.lent.trans h2.lent, h1.writable.trans h2.writable, h1.walked.trans h2.walked⟩

/-- `τ` is `σ` with perhaps more live. -/
inductive MSt.Freer : MSt → MSt → Prop where
  | nil : MSt.Freer [] []
  | cons {v w : MVar} {σ τ : MSt} : v.Freer w → MSt.Freer σ τ → MSt.Freer (v :: σ) (w :: τ)

theorem MSt.Freer.refl : ∀ (σ : MSt), σ.Freer σ
  | [] => .nil
  | v :: σ => .cons (MVar.Freer.refl v) (MSt.Freer.refl σ)

theorem MSt.Freer.trans : ∀ {a b c : MSt}, a.Freer b → b.Freer c → a.Freer c
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons h1 t1, .cons h2 t2 => .cons (h1.trans h2) (MSt.Freer.trans t1 t2)

theorem MSt.Freer.length : ∀ {a b : MSt}, a.Freer b → a.length = b.length
  | _, _, .nil => rfl
  | _, _, .cons _ t => by simp [MSt.Freer.length t]

theorem MSt.Freer.find : ∀ {σ τ : MSt}, σ.Freer τ → ∀ {x : String} {v : MVar}, σ.find x = some v →
    ∃ w, τ.find x = some w ∧ v.Freer w
  | _, _, .nil, _, _, h => by simp [MSt.find] at h
  | v :: σ, w :: τ, .cons hv t, x, u, h => by
    simp only [MSt.find, List.find?] at h ⊢
    rw [← hv.name]
    split
    · next e => rw [e] at h; cases h; exact ⟨w, rfl, hv⟩
    · next e => rw [e] at h; exact MSt.Freer.find t h

theorem MSt.Freer.find_none : ∀ {σ τ : MSt}, σ.Freer τ → ∀ {x : String}, σ.find x = none → τ.find x = none
  | _, _, .nil, _, _ => rfl
  | v :: σ, w :: τ, .cons hv t, x, h => by
    simp only [MSt.find, List.find?] at h ⊢
    rw [← hv.name]
    split
    · next e => rw [e] at h; cases h
    · next e => rw [e] at h; exact MSt.Freer.find_none t h

theorem MSt.Freer.find_back : ∀ {σ τ : MSt}, σ.Freer τ → ∀ {x : String} {w : MVar}, τ.find x = some w →
    ∃ v, σ.find x = some v ∧ v.Freer w
  | _, _, .nil, _, _, h => by simp [MSt.find] at h
  | v :: σ, w :: τ, .cons hv t, x, u, h => by
    simp only [MSt.find, List.find?] at h ⊢
    rw [hv.name]
    split
    · next e => rw [e] at h; cases h; exact ⟨v, rfl, hv⟩
    · next e => rw [e] at h; exact MSt.Freer.find_back t h

/-- Changing the innermost `x` on both sides, by functions that keep one
freer than the other. -/
theorem MSt.Freer.update {f g : MVar → MVar} (hfg : ∀ v w, v.Freer w → (f v).Freer (g w)) (x : String) :
    ∀ {σ τ : MSt}, σ.Freer τ → (MSt.update f x σ).Freer (MSt.update g x τ)
  | _, _, .nil => .nil
  | v :: σ, w :: τ, .cons hv t => by
    simp only [MSt.update]
    rw [← hv.name]
    split
    · exact .cons (hfg v w hv) t
    · exact .cons hv (MSt.Freer.update hfg x t)

/-- Changing the innermost `x` on the left only, by a function that leaves
the variable freer than itself was. -/
theorem MSt.Freer.update_left {f : MVar → MVar} (hf : ∀ v w, v.Freer w → (f v).Freer w) (x : String) :
    ∀ {σ τ : MSt}, σ.Freer τ → (MSt.update f x σ).Freer τ
  | _, _, .nil => .nil
  | v :: σ, w :: τ, .cons hv t => by
    simp only [MSt.update]
    split
    · exact .cons (hf v w hv) t
    · exact .cons hv (MSt.Freer.update_left hf x t)

theorem MSt.Freer.give (x : String) {σ τ : MSt} (h : σ.Freer τ) : (σ.give x).Freer (τ.give x) := by
  unfold MSt.give
  exact MSt.Freer.update (f := fun v => { v with init := true, live := true })
    (g := fun v => { v with init := true, live := true })
    (fun _ _ hv => ⟨hv.name, rfl, fun _ => rfl, hv.back, hv.lent, hv.writable, hv.walked⟩) x h

theorem MSt.Freer.take (x : String) {σ τ : MSt} (h : σ.Freer τ) : (σ.take x).Freer (τ.take x) := by
  unfold MSt.take
  exact MSt.Freer.update (f := fun v => { v with live := false }) (g := fun v => { v with live := false })
    (fun _ _ hv => ⟨hv.name, hv.init, fun h => absurd h Bool.false_ne_true, hv.back, hv.lent, hv.writable, hv.walked⟩) x h

theorem MSt.Freer.take_left (x : String) {σ τ : MSt} (h : σ.Freer τ) : (σ.take x).Freer τ := by
  unfold MSt.take
  exact MSt.Freer.update_left (f := fun v => { v with live := false })
    (fun _ _ hv => ⟨hv.name, hv.init, fun h => absurd h Bool.false_ne_true, hv.back, hv.lent, hv.writable, hv.walked⟩) x h

theorem MSt.Freer.walk (x : String) {σ τ : MSt} (h : σ.Freer τ) : (σ.walk x).Freer (τ.walk x) := by
  unfold MSt.walk
  exact MSt.Freer.update (f := fun v => { v with walked := true }) (g := fun v => { v with walked := true })
    (fun _ _ hv => ⟨hv.name, hv.init, hv.live, hv.back, hv.lent, hv.writable, rfl⟩) x h

theorem MSt.Freer.giveArgsM : ∀ (args : List (Option Mode × Expr)) (ms : List Mode) {σ τ : MSt}, σ.Freer τ →
    (giveArgsM σ args ms).Freer (giveArgsM τ args ms)
  | (_, e) :: args, m :: ms, σ, τ, h => by
    simp only [Fun.giveArgsM]
    apply MSt.Freer.giveArgsM args ms
    split
    · split
      · exact h.give _
      · exact h
    · exact h
  | [], _, _, _, h => by simp only [Fun.giveArgsM]; exact h
  | _ :: _, [], _, _, h => by simp only [Fun.giveArgsM]; exact h

theorem MSt.outsSet_cons (v : MVar) (σ : MSt) :
    MSt.outsSet (v :: σ) = ((v.init && v.live || !v.back) && MSt.outsSet σ) := rfl

theorem MSt.Freer.outsSet : ∀ {σ τ : MSt}, σ.Freer τ → σ.outsSet = true → τ.outsSet = true
  | _, _, .nil, _ => rfl
  | v :: σ, w :: τ, .cons hv t, h => by
    rw [MSt.outsSet_cons, Bool.and_eq_true] at h ⊢
    refine ⟨?_, MSt.Freer.outsSet t h.2⟩
    rw [← hv.init, ← hv.back]
    rcases Bool.or_eq_true _ _ |>.mp h.1 with h1 | h1
    · rw [Bool.and_eq_true] at h1; simp [h1.1, hv.live h1.2]
    · simp [h1]

theorem MSt.Freer.isWalked {σ τ : MSt} (h : σ.Freer τ) (x : String) : σ.isWalked x = τ.isWalked x := by
  unfold MSt.isWalked
  cases hf : σ.find x with
  | none => rw [h.find_none hf]
  | some v =>
    obtain ⟨w, hw, hvw⟩ := h.find hf
    rw [hw, Option.any_some, Option.any_some, hvw.walked]

/-- Where the freer state leaves a root moved, so did the other. -/
theorem heldMoved_freer : ∀ {σ τ : MSt}, σ.Freer τ → ∀ (args : List (Option Mode × Expr)) (ms : List Mode),
    heldMoved τ args ms = true → heldMoved σ args ms = true
  | σ, τ, h, (_, e) :: args, m :: ms, hm => by
    simp only [heldMoved, Bool.or_eq_true, Bool.and_eq_true] at hm ⊢
    rcases hm with ⟨hm1, hm2⟩ | hm
    · left
      refine ⟨hm1, ?_⟩
      split at hm2
      · rename_i x st hp
        cases hf : σ.find x with
        | none => rw [h.find_none hf] at hm2; simp at hm2
        | some v =>
          obtain ⟨w, hw, hvw⟩ := h.find hf
          rw [hw] at hm2
          simp only [Option.any_some, Bool.not_eq_true'] at hm2
          simp only [Option.any_some, Bool.not_eq_true']
          cases hl : v.live
          · rfl
          · rw [hvw.live hl] at hm2; cases hm2
      · cases hm2
    · right; exact heldMoved_freer h args ms hm
  | _, _, _, [], _, hm => by simp [heldMoved] at hm
  | _, _, _, _ :: _, [], hm => by simp [heldMoved] at hm

/-! ## Freer contexts -/

/-- A step of the error monad from a value. -/
theorem Except.ok_bind' {ε α β} (a : α) (f : α → Except ε β) : ((Except.ok a : Except ε α) >>= f) = f a := rfl

/-! ## Steps -/

/-- Two walks of one expression, from `σ` and a freer `σ*`: where the first
comes to `σ'`, the second comes to a freer `σ'*`, and sets the flag no more. -/
def EMono (m m' : MoveM MSt) : Prop :=
  ∀ st st' σ' st1, m.run st = .ok (σ', st1) → (st'.movedHeld = true → st.movedHeld = true) →
    ∃ σ'' st1', m'.run st' = .ok (σ'', st1') ∧ σ'.Freer σ'' ∧ (st1'.movedHeld = true → st1.movedHeld = true)

namespace EMono

theorem pure {σ τ : MSt} (h : σ.Freer τ) : EMono (Pure.pure σ) (Pure.pure τ) := fun st st' σ' st1 e hf => by
  cases e; exact ⟨τ, st', rfl, h, hf⟩

theorem throw {c : Lit.Code} {m' : MoveM MSt} : EMono (MonadExcept.throw c) m' := fun _ _ _ _ e _ => by cases e

theorem bind {x x' : MoveM MSt} {f f' : MSt → MoveM MSt} (hx : EMono x x')
    (hf : ∀ τ τ', τ.Freer τ' → EMono (f τ) (f' τ')) : EMono (x >>= f) (x' >>= f') := by
  intro st st' σ' st1 e hfl
  replace e := MoveM.bind_ok e
  obtain ⟨τ, s1, e1, e2⟩ := e
  obtain ⟨τ', s1', e1', hτ, hf1⟩ := hx st st' τ s1 e1 hfl
  obtain ⟨σ'', s2', e2', hσ, hf2⟩ := hf τ τ' hτ s1 s1' σ' st1 e2 hf1
  refine ⟨σ'', s2', ?_, hσ, hf2⟩
  rw [MoveM.run_bind, e1']
  exact e2'

/-- A step that gives no state, on both sides, each keeping the flag or
setting it no more than the other. -/
theorem calm_bind {α β} {x : MoveM α} {x' : MoveM β} {f : α → MoveM MSt} {f' : β → MoveM MSt}
    (hx : ∀ st st' a st1, x.run st = .ok (a, st1) → (st'.movedHeld = true → st.movedHeld = true) →
      ∃ b st1', x'.run st' = .ok (b, st1') ∧ (st1'.movedHeld = true → st1.movedHeld = true))
    (hf : ∀ a b, EMono (f a) (f' b)) : EMono (x >>= f) (x' >>= f') := by
  intro st st' σ' st1 e hfl
  replace e := MoveM.bind_ok e
  obtain ⟨a, s1, e1, e2⟩ := e
  obtain ⟨b, s1', e1', hf1⟩ := hx st st' a s1 e1 hfl
  obtain ⟨σ'', s2', e2', hσ, hf2⟩ := hf a b s1 s1' σ' st1 e2 hf1
  refine ⟨σ'', s2', ?_, hσ, hf2⟩
  rw [MoveM.run_bind, e1']
  exact e2'

/-- A step that gives no state and keeps the flag, on both sides. -/
theorem keep {α β} {x : MoveM α} {x' : MoveM β} {a : α} {b : β}
    (hx : ∀ st, x.run st = .ok (a, st)) (hx' : ∀ st, x'.run st = .ok (b, st)) :
    ∀ st st' a' st1, x.run st = .ok (a', st1) → (st'.movedHeld = true → st.movedHeld = true) →
      ∃ b' st1', x'.run st' = .ok (b', st1') ∧ (st1'.movedHeld = true → st1.movedHeld = true) := by
  intro st st' a' st1 e hf
  rw [hx st] at e; cases e
  exact ⟨b, st', hx' st', hf⟩

theorem ite {c : Bool} {x y x' y' : MoveM MSt} (hx : EMono x x') (hy : EMono y y') :
    EMono (if c then x else y) (if c then x' else y') := by
  cases c
  · simpa using hy
  · simpa using hx

/-- A read the analysis let through of `x` in `σ`, read in a freer state:
let through, leaving it as it was. -/
theorem readM_freer_ok {σ τ : MSt} (h : σ.Freer τ) {x : String}
    (hr : ∀ v, σ.find x = some v → (v.init && v.live) = true) (st : DSt) : (Fun.readM τ x).run st = .ok (τ, st) := by
  unfold Fun.readM
  cases hf : τ.find x with
  | none => rfl
  | some w =>
    obtain ⟨v, hv, hvw⟩ := h.find_back hf
    have := hr v hv
    simp only [Bool.and_eq_true] at this
    simp [← hvw.init, this.1, hvw.live this.2]
    rfl

theorem readM {σ τ : MSt} (h : σ.Freer τ) (x : String) : EMono (Fun.readM σ x) (Fun.readM τ x) := by
  intro st st' σ' st1 e hf
  obtain ⟨rfl, rfl, hr⟩ := readM_run e
  exact ⟨τ, st', readM_freer_ok h hr st', h, hf⟩

theorem exitM {σ τ : MSt} (h : σ.Freer τ) : EMono (Fun.exitM σ) (Fun.exitM τ) := by
  intro st st' σ' st1 e hf
  obtain ⟨rfl, rfl, ho⟩ := exitM_run e
  refine ⟨τ, st', ?_, h, hf⟩
  unfold Fun.exitM
  simp [h.outsSet ho]
  rfl

/-- A read that moves, against one that only reads: what it leaves is freer
than what the read leaves. -/
theorem moveM_readM {Γa : Ctx} {σ τ : MSt} (h : σ.Freer τ) (x : String) : EMono (Fun.moveM Γa σ x) (Fun.readM τ x) := by
  intro st st' σ' st1 e hf
  obtain ⟨hr, hor, -⟩ := moveM_run e
  refine ⟨τ, st', readM_freer_ok h hr st', ?_, fun hm => FlagMono.moveM Γa σ x st σ' st1 e (hf hm)⟩
  rcases hor with rfl | rfl
  · exact h
  · exact h.take_left x

theorem moveM {Γa Γb : Ctx} (hΓ : CtxLe Γa Γb) {σ τ : MSt} (h : σ.Freer τ) (x : String) :
    EMono (Fun.moveM Γa σ x) (Fun.moveM Γb τ x) := by
  intro st st' σ' st1 e hf
  unfold Fun.moveM at e ⊢
  cases hl : Γa.lookup x with
  | none =>
    rw [hl] at e; rw [(hΓ x).1 hl]
    exact readM h x st st' σ' st1 e hf
  | some tk =>
    obtain ⟨t, k⟩ := tk
    rw [hl] at e
    obtain ⟨t', hl', hc⟩ := (hΓ x).2 t k hl
    rw [hl']
    cases k
    case const_ =>
      dsimp only at e ⊢
      rcases MoveM.ite_run e with ⟨hcp, e⟩ | ⟨-, e⟩
      · simp only [hc hcp, ↓reduceIte]; exact readM h x st st' σ' st1 e hf
      · cases e
    all_goals
      dsimp only at e ⊢
      replace e := MoveM.bind_ok e
      obtain ⟨σ1, s1, e1, e⟩ := e
      obtain ⟨τ1, s1', e1', hτ1, hf1⟩ := readM h x st st' σ1 s1 e1 hf
      rw [MoveM.run_bind, e1', Except.ok_bind']
      dsimp only
      rcases MoveM.ite_run e with ⟨hcp, e⟩ | ⟨hcp, e⟩
      · obtain ⟨rfl, rfl⟩ := MoveM.pure_run e
        simp only [hc hcp, ↓reduceIte]
        exact ⟨τ1, s1', rfl, hτ1, hf1⟩
      · cases hcp' : t'.isCopy
        · simp only [Bool.false_eq_true, ↓reduceIte]
          cases hv : σ1.find x with
          | none =>
            rw [hv] at e
            obtain ⟨rfl, rfl⟩ := MoveM.pure_run e
            rw [hτ1.find_none hv]
            exact ⟨τ1, s1', rfl, hτ1, hf1⟩
          | some v =>
            rw [hv] at e
            obtain ⟨w, hw, hvw⟩ := hτ1.find hv
            rw [hw]
            rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨hwk, e⟩
            · cases e
            rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨hlt, e⟩
            · cases e
            replace e := MoveM.bind_ok e
            obtain ⟨_, s2, e2, e⟩ := e
            obtain ⟨rfl, rfl⟩ := MoveM.pure_run e
            have hs2 := MoveM.modify_run e2
            subst hs2
            simp only [← hvw.walked, hwk, Bool.false_eq_true, ↓reduceIte, ← hvw.lent, hlt]
            exact ⟨τ1.take x, _, rfl, hτ1.take x, hf1⟩
        · simp only [↓reduceIte]
          cases hv : σ1.find x with
          | none =>
            rw [hv] at e
            obtain ⟨rfl, rfl⟩ := MoveM.pure_run e
            exact ⟨τ1, s1', rfl, hτ1, hf1⟩
          | some v =>
            rw [hv] at e
            rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨-, e⟩
            · cases e
            rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨-, e⟩
            · cases e
            replace e := MoveM.bind_ok e
            obtain ⟨_, s2, e2, e⟩ := e
            obtain ⟨rfl, rfl⟩ := MoveM.pure_run e
            have hs2 := MoveM.modify_run e2
            subst hs2
            exact ⟨τ1, s1', rfl, hτ1.take_left x, hf1⟩

theorem exprAt_keep : ∀ st, Fun.exprAt.run st = .ok (st.expr, st) := fun _ => rfl

theorem exprFrom_pure {σ : MSt} (k : Nat) : ∀ st, (Fun.exprFrom k >>= fun _ => (Pure.pure σ : MoveM MSt)).run st =
    .ok (σ, { st with expr := k }) := fun _ => rfl

/-- The wrapping of an expression's walk: the counter of expressions set
past it, on both sides. -/
theorem wrap {m m' : MoveM MSt} {e : Expr} (h : EMono m m') :
    EMono (Fun.exprAt >>= fun start => m >>= fun σ' => Fun.exprFrom (start + e.nodes) >>= fun _ => (Pure.pure σ' : MoveM MSt))
      (Fun.exprAt >>= fun start => m' >>= fun σ' => Fun.exprFrom (start + e.nodes) >>= fun _ => (Pure.pure σ' : MoveM MSt)) := by
  intro st st' σ' st1 hr hf
  replace hr := MoveM.bind_ok hr
  obtain ⟨start, s1, e1, hr⟩ := hr
  rw [exprAt_keep] at e1
  cases e1
  replace hr := MoveM.bind_ok hr
  obtain ⟨σ1, s2, e2, hr⟩ := hr
  obtain ⟨σ1', s2', e2', hσ, hf2⟩ := h st st' σ1 s2 e2 hf
  rw [exprFrom_pure] at hr
  cases hr
  refine ⟨σ1', { s2' with expr := st'.expr + e.nodes }, ?_, hσ, hf2⟩
  rw [MoveM.run_bind, exprAt_keep, Except.ok_bind']
  dsimp only
  rw [MoveM.run_bind, e2', Except.ok_bind']
  dsimp only
  rw [exprFrom_pure]

theorem unwalkedM {σ τ : MSt} (h : σ.Freer τ) (x : String) :
    ∀ st st' a st1, (Fun.unwalkedM σ x).run st = .ok (a, st1) → (st'.movedHeld = true → st.movedHeld = true) →
      ∃ b st1', (Fun.unwalkedM τ x).run st' = .ok (b, st1') ∧ (st1'.movedHeld = true → st1.movedHeld = true) := by
  intro st st' a st1 e hf
  unfold Fun.unwalkedM at e ⊢
  rw [← h.isWalked x]
  rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨hw, e⟩
  · cases e
  · cases e
    simp only [hw, Bool.false_eq_true, ↓reduceIte]
    exact ⟨(), st', rfl, hf⟩

theorem writableM {σ τ : MSt} (h : σ.Freer τ) (x : String) :
    ∀ st st' a st1, (Fun.writableM σ x).run st = .ok (a, st1) → (st'.movedHeld = true → st.movedHeld = true) →
      ∃ b st1', (Fun.writableM τ x).run st' = .ok (b, st1') ∧ (st1'.movedHeld = true → st1.movedHeld = true) := by
  intro st st' a st1 e hf
  unfold Fun.writableM at e ⊢
  split at e
  · rename_i v hv
    obtain ⟨w, hw, hvw⟩ := h.find hv
    rw [hw]
    rcases MoveM.ite_run e with ⟨hwr, e⟩ | ⟨-, e⟩
    · cases e
      simp only [← hvw.writable, hwr, ↓reduceIte]
      exact ⟨(), st', rfl, hf⟩
    · rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨-, e⟩ <;> cases e
  · rename_i hn
    cases e
    rw [h.find_none hn]
    exact ⟨(), st', rfl, hf⟩

theorem noteHeldMoved {σ τ : MSt} (h : σ.Freer τ) (args : List (Option Mode × Expr)) (ms : List Mode) :
    ∀ st st' a st1, (Fun.noteHeldMoved σ args ms).run st = .ok (a, st1) → (st'.movedHeld = true → st.movedHeld = true) →
      ∃ b st1', (Fun.noteHeldMoved τ args ms).run st' = .ok (b, st1') ∧ (st1'.movedHeld = true → st1.movedHeld = true) := by
  intro st st' a st1 e hf
  unfold Fun.noteHeldMoved at e ⊢
  rcases MoveM.ite_run e with ⟨hm, e⟩ | ⟨hm, e⟩
  · have e' := MoveM.modify_run e
    subst e'
    cases hm' : heldMoved τ args ms
    · simp only [Bool.false_eq_true, ↓reduceIte]
      exact ⟨(), st', rfl, by simp⟩
    · simp only [↓reduceIte]
      exact ⟨(), _, rfl, by simp⟩
  · cases e
    have hm' : heldMoved τ args ms = false := by
      cases hm' : heldMoved τ args ms
      · rfl
      · rw [heldMoved_freer h args ms hm'] at hm; cases hm
    simp only [hm', Bool.false_eq_true, ↓reduceIte]
    exact ⟨(), st', rfl, hf⟩

end EMono

/-- A `set`'s keys walked with each key lent, as the run reads them. -/
def mKeysLent (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List Step → MoveM MSt
  | σ, [] => pure σ
  | σ, .field _ :: rest => mKeysLent sigs Γ σ rest
  | σ, .index k _ :: rest => mE sigs Γ true σ k >>= fun σ => mKeysLent sigs Γ σ rest

section
variable (sigs : List (String × Sig)) {Γa Γb : Ctx} (hΓ : CtxLe Γa Γb)
include hΓ
set_option linter.unusedSectionVars false

mutual

theorem mE_mono : ∀ (b b' : Bool) (σ τ : MSt) (e : Expr), (b = true → b' = true) → σ.Freer τ →
    EMono (mE sigs Γa b σ e) (mE sigs Γb b' τ e)
  | b, b', σ, τ, .var x, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    cases b
    · simp only [Bool.false_eq_true, ↓reduceIte]
      cases b'
      · simp only [Bool.false_eq_true, ↓reduceIte]; exact EMono.moveM hΓ h x
      · simp only [↓reduceIte]; exact EMono.moveM_readM h x
    · simp only [hb rfl, ↓reduceIte]
      exact EMono.readM h x
  | b, b', σ, τ, .place x steps, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    exact EMono.bind (EMono.readM h x) (fun σ1 τ1 h1 => mSteps_mono σ1 τ1 steps h1)
  | b, b', σ, τ, .hinted _ a, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    exact mE_mono _ _ σ τ a (fun hc => by
      simp only [Bool.and_eq_true] at hc ⊢; exact ⟨hb hc.1, hc.2⟩) h
  | b, b', σ, τ, .adapt a, hb, h => by unfold mE; exact EMono.wrap (mE_mono _ _ σ τ a (fun h => h) h)
  | b, b', σ, τ, .some_ a, hb, h | b, b', σ, τ, .ok a, hb, h | b, b', σ, τ, .er a, hb, h
  | b, b', σ, τ, .data a, hb, h | b, b', σ, τ, .error a, hb, h | b, b', σ, τ, .term _ a, hb, h => by
    unfold mE; exact EMono.wrap (mE_mono _ _ σ τ a (fun h => h) h)
  | b, b', σ, τ, .enum a, hb, h => by unfold mE; exact EMono.wrap (mE_mono _ _ σ τ a hb h)
  | b, b', σ, τ, .binop op a c, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    refine EMono.bind (mE_mono _ _ σ τ a (fun h => h) h) (fun σ1 τ1 h1 =>
      EMono.bind (mE_mono _ _ σ1 τ1 c (fun h => h) h1) (fun σ2 τ2 h2 => ?_))
    exact EMono.ite (EMono.exitM h2) (EMono.pure h2)
  | b, b', σ, τ, .unop op a, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    refine EMono.bind (mE_mono _ _ σ τ a (fun h => h) h) (fun σ1 τ1 h1 => ?_)
    split
    · exact EMono.exitM h1
    · exact EMono.exitM h1
    · exact EMono.pure h1
  | b, b', σ, τ, .tryOpt a, hb, h | b, b', σ, τ, .tryRes a, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    exact EMono.bind (mE_mono _ _ σ τ a hb h) (fun σ1 τ1 h1 => EMono.exitM h1)
  | b, b', σ, τ, .proj base _, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    exact mE_mono _ _ σ τ base (fun hc => by
      simp only [Bool.or_eq_true] at hc ⊢
      rcases hc with hc | hc
      · exact .inl (hb hc)
      · exact .inr hc) h
  | b, b', σ, τ, .index base k (some _), hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    refine EMono.bind (mE_mono _ _ σ τ base (fun hc => by
      simp only [Bool.or_eq_true] at hc ⊢
      rcases hc with hc | hc
      · exact .inl (hb hc)
      · exact .inr hc) h) (fun σ1 τ1 h1 => ?_)
    exact EMono.bind (mE_mono _ _ σ1 τ1 k (fun h => h) h1) (fun σ2 τ2 h2 => EMono.exitM h2)
  | b, b', σ, τ, .index _ _ none, hb, h => by unfold mE; exact EMono.wrap (EMono.pure h)
  | b, b', σ, τ, .call f args, hb, h => by
    unfold mE; refine EMono.wrap ?_; dsimp only
    split
    · refine EMono.bind (mArgs_mono σ τ args _ h) (fun σ1 τ1 h1 => ?_)
      refine EMono.calm_bind (EMono.noteHeldMoved h1 args _) (fun _ _ => ?_)
      exact EMono.bind (EMono.ite (EMono.exitM h1) (EMono.pure h1)) (fun σ2 τ2 h2 =>
        EMono.pure (MSt.Freer.giveArgsM args _ h2))
    · exact EMono.throw
  | b, b', σ, τ, .tuple es, hb, h | b, b', σ, τ, .list es, hb, h | b, b', σ, τ, .set es, hb, h
  | b, b', σ, τ, .tensor _ es, hb, h => by
    unfold mE; exact EMono.wrap (mAll_mono σ τ es h)
  | b, b', σ, τ, .struct fs, hb, h => by unfold mE; exact EMono.wrap (mFields_mono σ τ fs h)
  | b, b', σ, τ, .map es, hb, h => by unfold mE; exact EMono.wrap (mEntries_mono σ τ es h)
  | b, b', σ, τ, .table _ rows, hb, h => by unfold mE; exact EMono.wrap (mRows_mono σ τ rows h)
  | b, b', σ, τ, .none_, hb, h | b, b', σ, τ, .tt, hb, h | b, b', σ, τ, .ff, hb, h | b, b', σ, τ, .str _, hb, h
  | b, b', σ, τ, .int _, hb, h | b, b', σ, τ, .float, hb, h | b, b', σ, τ, .hex _, hb, h
  | b, b', σ, τ, .atom _, hb, h => by unfold mE; exact EMono.wrap (EMono.pure h)

theorem mSteps_mono : ∀ (σ τ : MSt) (steps : List Step), σ.Freer τ → EMono (mSteps sigs Γa σ steps) (mSteps sigs Γb τ steps)
  | σ, τ, [], h => by unfold mSteps; exact EMono.pure h
  | σ, τ, .field _ :: rest, h => by unfold mSteps; exact mSteps_mono σ τ rest h
  | σ, τ, .index k m :: rest, h => by
    unfold mSteps
    refine EMono.bind (mE_mono _ _ σ τ k (fun h => h) h) (fun σ1 τ1 h1 => ?_)
    refine EMono.bind ?_ (fun σ2 τ2 h2 => mSteps_mono σ2 τ2 rest h2)
    split
    · exact EMono.exitM h1
    · exact EMono.pure h1

theorem mPlace_mono : ∀ (σ τ : MSt) (e : Expr) (need : Bool), σ.Freer τ →
    EMono (mPlace sigs Γa σ e need) (mPlace sigs Γb τ e need)
  | σ, τ, .var x, need, h => by
    unfold mPlace; refine EMono.wrap ?_; dsimp only
    split
    · exact EMono.readM h x
    · intro st st' σ' st1 e hf
      split at e
      · rename_i v hv
        obtain ⟨w, hw, hvw⟩ := h.find hv
        rw [hw]
        rcases MoveM.ite_run e with ⟨-, e⟩ | ⟨hl, e⟩
        · cases e
        · cases e
          simp only [Bool.not_eq_false'] at hl
          simp only [hvw.live hl, Bool.not_true, Bool.false_eq_true, ↓reduceIte]
          exact ⟨τ, st', rfl, h, hf⟩
      · rename_i hn
        cases e
        rw [h.find_none hn]
        exact ⟨τ, st', rfl, h, hf⟩
  | σ, τ, .place x steps, _, h => by
    unfold mPlace; refine EMono.wrap ?_; dsimp only
    exact EMono.bind (EMono.readM h x) (fun σ1 τ1 h1 => mSteps_mono σ1 τ1 steps h1)
  | σ, τ, .hinted _ a, need, h => by unfold mPlace; exact EMono.wrap (mPlace_mono σ τ a need h)
  | σ, τ, .proj base _, _, h => by unfold mPlace; exact EMono.wrap (mPlace_mono σ τ base true h)
  | σ, τ, .index base k (some _), _, h => by
    unfold mPlace; refine EMono.wrap ?_; dsimp only
    exact EMono.bind (mPlace_mono σ τ base true h) (fun σ1 τ1 h1 =>
      EMono.bind (mE_mono _ _ σ1 τ1 k (fun h => h) h1) (fun σ2 τ2 h2 => EMono.exitM h2))
  | σ, τ, .index _ _ none, _, h | σ, τ, .none_, _, h | σ, τ, .tt, _, h | σ, τ, .ff, _, h | σ, τ, .str _, _, h
  | σ, τ, .int _, _, h | σ, τ, .float, _, h | σ, τ, .hex _, _, h | σ, τ, .tuple _, _, h | σ, τ, .struct _, _, h
  | σ, τ, .list _, _, h | σ, τ, .set _, _, h | σ, τ, .map _, _, h | σ, τ, .tensor _ _, _, h | σ, τ, .table _ _, _, h
  | σ, τ, .some_ _, _, h | σ, τ, .ok _, _, h | σ, τ, .er _, _, h | σ, τ, .data _, _, h | σ, τ, .error _, _, h
  | σ, τ, .atom _, _, h | σ, τ, .term _ _, _, h | σ, τ, .enum _, _, h | σ, τ, .binop _ _ _, _, h
  | σ, τ, .unop _ _, _, h | σ, τ, .tryOpt _, _, h | σ, τ, .tryRes _, _, h | σ, τ, .adapt _, _, h
  | σ, τ, .call _ _, _, h => by unfold mPlace; exact EMono.wrap (EMono.pure h)

theorem mArgs_mono : ∀ (σ τ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode), σ.Freer τ →
    EMono (mArgs sigs Γa σ args ms) (mArgs sigs Γb τ args ms)
  | σ, τ, (_, e) :: args, m :: ms, h => by
    unfold mArgs
    refine EMono.bind ?_ (fun σ1 τ1 h1 => mArgs_mono σ1 τ1 args ms h1)
    split
    · split
      · exact EMono.calm_bind (EMono.unwalkedM h _) (fun _ _ =>
          EMono.calm_bind (EMono.writableM h _) (fun _ _ => mPlace_mono σ τ e _ h))
      · exact mE_mono _ _ σ τ e (fun h => h) h
    · split
      · exact EMono.throw
      · exact mE_mono _ _ σ τ e (fun h => h) h
  | σ, τ, [], _, h => by unfold mArgs; exact EMono.pure h
  | σ, τ, _ :: _, [], h => by unfold mArgs; exact EMono.pure h

theorem mAll_mono : ∀ (σ τ : MSt) (es : List Expr), σ.Freer τ → EMono (mAll sigs Γa σ es) (mAll sigs Γb τ es)
  | σ, τ, [], h => by unfold mAll; exact EMono.pure h
  | σ, τ, e :: es, h => by
    unfold mAll; exact EMono.bind (mE_mono _ _ σ τ e (fun h => h) h) (fun σ1 τ1 h1 => mAll_mono σ1 τ1 es h1)

theorem mFields_mono : ∀ (σ τ : MSt) (fs : List (String × Expr)), σ.Freer τ →
    EMono (mFields sigs Γa σ fs) (mFields sigs Γb τ fs)
  | σ, τ, [], h => by unfold mFields; exact EMono.pure h
  | σ, τ, (_, e) :: fs, h => by
    unfold mFields; exact EMono.bind (mE_mono _ _ σ τ e (fun h => h) h) (fun σ1 τ1 h1 => mFields_mono σ1 τ1 fs h1)

theorem mEntries_mono : ∀ (σ τ : MSt) (es : List (Expr × Expr)), σ.Freer τ →
    EMono (mEntries sigs Γa σ es) (mEntries sigs Γb τ es)
  | σ, τ, [], h => by unfold mEntries; exact EMono.pure h
  | σ, τ, (k, v) :: es, h => by
    unfold mEntries
    exact EMono.bind (mE_mono _ _ σ τ k (fun h => h) h) (fun σ1 τ1 h1 =>
      EMono.bind (mE_mono _ _ σ1 τ1 v (fun h => h) h1) (fun σ2 τ2 h2 => mEntries_mono σ2 τ2 es h2))

theorem mRows_mono : ∀ (σ τ : MSt) (rows : List (List Expr)), σ.Freer τ →
    EMono (mRows sigs Γa σ rows) (mRows sigs Γb τ rows)
  | σ, τ, [], h => by unfold mRows; exact EMono.pure h
  | σ, τ, r :: rs, h => by
    unfold mRows; exact EMono.bind (mAll_mono σ τ r h) (fun σ1 τ1 h1 => mRows_mono σ1 τ1 rs h1)

end

/-- The keys as the analysis walks them, the inserted one taken, against the
keys all lent: the latter is let through, and leaves a freer state. -/
theorem mKeys_mono : ∀ (σ τ : MSt) (steps : List Step), σ.Freer τ →
    EMono (mKeys sigs Γa σ steps) (mKeysLent sigs Γb τ steps)
  | σ, τ, [], h => by unfold mKeys mKeysLent; exact EMono.pure h
  | σ, τ, .field _ :: rest, h => by unfold mKeys mKeysLent; exact mKeys_mono σ τ rest h
  | σ, τ, .index k m :: rest, h => by
    unfold mKeys mKeysLent
    exact EMono.bind (mE_mono sigs hΓ _ _ σ τ k (fun _ => rfl) h) (fun σ1 τ1 h1 => mKeys_mono σ1 τ1 rest h1)

end

end Datalove.Fun
