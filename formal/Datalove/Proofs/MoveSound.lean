import Datalove.Proofs.MoveShape

/-!
# A script the move analysis lets through never reads what was moved

The first half of `Fun.runScriptMoves_move` (`Proofs/MoveStmt.lean` has the
rest): expressions. Where the move analysis (`FunMove.lean`) lets an
expression through, running it with the run that moves (`movesEnv`) never
reads a variable holding nothing, moved or never given a value, and
returns early only with every `mut` and `out` parameter holding one.

The analysis keeps, for each variable in scope, whether it was given a
value on every way here and whether it has not been moved; the run keeps
the variables themselves. They line up where every variable the analysis
says holds a value, given and not moved, does (`MAg`). A read the analysis
lets through is of one it says holds a value; a read that moves, the
analysis moves too, where the run does, and the run moves only what the
analysis moves. Nothing here keeps a value the run had: a step may take one
away, and what is proved after each step is only that the analysis's state
after it lines up with the run's variables after it. That is enough for an
expression; a loop needs more, which `Proofs/MoveStmt.lean` says.

A call writes back into its `mut` and `out` arguments' variables; where a
later argument of the same call moved one, it writes into what holds
nothing. The analysis notes such a call (`DSt.movedHeld`) rather than
refusing it, since O010 and O017 refuse it between them, and the proof
takes the note never made.
-/

namespace Datalove.Fun

open Lit (Val)

/-! ## Lining up -/

/-- The analysis's variables and the run's, name by name, each the analysis
says holds a value, given and not moved, holding one. -/
inductive MAg : MSt → VEnv → Prop where
  | nil : MAg [] []
  | cons {v : MVar} {x : String} {o : Option Val} {σ : MSt} {ρ : VEnv} :
      v.name = x → ((v.init && v.live) = true → o.isSome = true) → MAg σ ρ → MAg (v :: σ) ((x, o) :: ρ)

theorem MAg.length : ∀ {σ : MSt} {ρ : VEnv}, MAg σ ρ → σ.length = ρ.length
  | _, _, .nil => rfl
  | _, _, .cons _ _ a => by simp [MAg.length a]

/-- A variable the analysis says holds a value is no variable that holds
nothing. -/
theorem MAg.lookup : ∀ {σ : MSt} {ρ : VEnv} {x : String}, MAg σ ρ →
    (∀ v, σ.find x = some v → (v.init && v.live) = true) → ρ.lookup x ≠ some none
  | _, _, _, .nil, _ => by simp
  | v :: σ, (y, o) :: ρ, x, .cons hn h a, hx => by
    simp only [MSt.find, List.find?, List.lookup] at hx ⊢
    subst hn
    by_cases e : x = v.name
    · subst e
      simp only [beq_self_eq_true] at hx ⊢
      intro ho; cases ho
      have := h (hx v rfl)
      simp at this
    · have e1 : (v.name == x) = false := by simpa using fun h => e h.symm
      have e2 : (x == v.name) = false := by simpa using e
      simp only [e1, e2] at hx ⊢
      exact MAg.lookup a hx

theorem MAg.find_some : ∀ {σ : MSt} {ρ : VEnv} {x : String} {v : MVar}, MAg σ ρ → σ.find x = some v →
    (v.init && v.live) = true → ∃ w, ρ.lookup x = some (some w)
  | σ, ρ, x, v, h, hf, hv => by
    have := MAg.lookup h (fun w hw => by rw [hf] at hw; cases hw; exact hv)
    cases hl : ρ.lookup x with
    | none =>
      exfalso
      -- The names line up, so a variable the analysis finds the run has.
      exact names_lookup h hf hl
    | some o =>
      cases o with
      | none => exact absurd hl this
      | some w => exact ⟨w, rfl⟩
where
  names_lookup : ∀ {σ : MSt} {ρ : VEnv} {x : String} {v : MVar}, MAg σ ρ → σ.find x = some v →
      ρ.lookup x = none → False
    | _, _, _, _, .nil, hf, _ => by simp [MSt.find] at hf
    | w :: σ, (y, o) :: ρ, x, v, .cons hn _ a, hf, hl => by
      simp only [MSt.find, List.find?, List.lookup] at hf hl
      subst hn
      by_cases e : x = w.name
      · subst e; simp at hl
      · have e1 : (w.name == x) = false := by simpa using fun h => e h.symm
        have e2 : (x == w.name) = false := by simpa using e
        simp only [e1, e2] at hf hl
        exact names_lookup a hf hl

theorem MAg.give : ∀ {σ : MSt} {ρ : VEnv} (x : String) (v : Val), MAg σ ρ → MAg (σ.give x) (setVar x v ρ)
  | _, _, _, _, .nil => .nil
  | w :: σ, (y, o) :: ρ, x, v, .cons hn h a => by
    simp only [MSt.give, MSt.update, setVar]
    subst hn
    by_cases e : w.name = x
    · simp only [e, beq_self_eq_true, ↓reduceIte]
      exact .cons rfl (fun _ => rfl) a
    · have e1 : (w.name == x) = false := by simpa using e
      simp only [e1, Bool.false_eq_true, ↓reduceIte]
      exact .cons rfl h (MAg.give x v a)

/-- Giving the run a value never breaks the line-up. -/
theorem MAg.set : ∀ {σ : MSt} {ρ : VEnv} (x : String) (v : Val), MAg σ ρ → MAg σ (setVar x v ρ)
  | _, _, _, _, .nil => .nil
  | w :: σ, (y, o) :: ρ, x, v, .cons hn h a => by
    simp only [setVar]
    split
    · exact .cons hn (fun _ => rfl) a
    · exact .cons hn h (MAg.set x v a)

/-- A move the analysis makes and the run makes too. -/
theorem MAg.take : ∀ {σ : MSt} {ρ : VEnv} (x : String), MAg σ ρ → MAg (σ.take x) (takeVar x ρ)
  | _, _, _, .nil => .nil
  | w :: σ, (y, o) :: ρ, x, .cons hn h a => by
    simp only [MSt.take, MSt.update, takeVar]
    subst hn
    by_cases e : w.name = x
    · simp only [e, beq_self_eq_true, ↓reduceIte]
      exact .cons rfl (fun h => by simp at h) a
    · have e1 : (w.name == x) = false := by simpa using e
      simp only [e1, Bool.false_eq_true, ↓reduceIte]
      exact .cons rfl h (MAg.take x a)

/-- A move the analysis makes and the run does not. -/
theorem MAg.take' : ∀ {σ : MSt} {ρ : VEnv} (x : String), MAg σ ρ → MAg (σ.take x) ρ
  | _, _, _, .nil => .nil
  | w :: σ, (y, o) :: ρ, x, .cons hn h a => by
    simp only [MSt.take, MSt.update]
    split
    · exact .cons hn (fun h => by simp at h) a
    · exact .cons hn h (MAg.take' x a)

theorem MAg.walk : ∀ {σ : MSt} {ρ : VEnv} (x : String), MAg σ ρ → MAg (σ.walk x) ρ
  | _, _, _, .nil => .nil
  | w :: σ, (y, o) :: ρ, x, .cons hn h a => by
    simp only [MSt.walk, MSt.update]
    split
    · exact .cons hn h a
    · exact .cons hn h (MAg.walk x a)

theorem MAg.drop : ∀ {σ : MSt} {ρ : VEnv} (k : Nat), MAg σ ρ → MAg (List.drop k σ) (List.drop k ρ)
  | _, _, 0, h => h
  | _, _, _ + 1, .nil => .nil
  | _, _, k + 1, .cons _ _ a => MAg.drop k a

theorem MAg.take_n : ∀ {σ : MSt} {ρ : VEnv} (k : Nat), MAg σ ρ → MAg (List.take k σ) (List.take k ρ)
  | _, _, 0, _ => .nil
  | _, _, _ + 1, .nil => .nil
  | _, _, k + 1, .cons hn h a => .cons hn h (MAg.take_n k a)

theorem MAg.append : ∀ {A B : MSt} {C D : VEnv}, MAg A C → MAg B D → MAg (A ++ B) (C ++ D)
  | _, _, _, _, .nil, h => h
  | _, _, _, _, .cons hn h a, hb => .cons hn h (MAg.append a hb)

theorem MAg.reverse : ∀ {A : MSt} {C : VEnv}, MAg A C → MAg A.reverse C.reverse
  | _, _, .nil => .nil
  | _, _, .cons hn h a => by
    simp only [List.reverse_cons]
    exact MAg.append (MAg.reverse a) (.cons hn h .nil)

/-- Leaving a scope: what the analysis had inside, back in the scope it
began in, lines up with the run's variables back there. -/
theorem MAg.back {σ τ : MSt} {ρ ρ' : VEnv} (h : MAg τ ρ') (hl : σ.length = ρ.length) :
    MAg (σ.back τ) (restore ρ ρ') := by
  unfold MSt.back restore
  rw [MAg.length h, hl]
  exact MAg.drop _ h

/-- Names bound in front, as `bindVals` binds them, lining up with what
`bindOne` bound where each holds a value as the analysis says. -/
theorem MAg.cons' {σ : MSt} {ρ : VEnv} {v : MVar} {x : String} {o : Option Val} (hn : v.name = x)
    (h : (v.init && v.live) = true → o.isSome = true) (a : MAg σ ρ) : MAg (v :: σ) ((x, o) :: ρ) :=
  .cons hn h a

/-- `τ` is `σ` with no variable un-given, and a way back that moved nothing
`σ` held: what lines up with `τ` lines up with `σ`. -/
theorem MAg.le : ∀ {σ τ : MSt} {ρ : VEnv}, σ.Le τ → σ.keepsLive τ = true → MAg τ ρ → MAg σ ρ
  | _, _, _, .nil, _, .nil => .nil
  | v :: σ, w :: τ, (x, o) :: ρ, .cons hn _ hi _ _ l, hk, .cons hn' h a => by
    simp only [MSt.keepsLive, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, Bool.or_eq_true,
      Bool.not_eq_true'] at hk
    refine .cons (hn.trans hn') (fun hv => h ?_) (MAg.le l (by simpa [MSt.keepsLive] using hk.2) a)
    simp only [Bool.and_eq_true] at hv ⊢
    refine ⟨hi hv.1, ?_⟩
    rcases hk.1 with hk | hk
    · rw [hv.2] at hk; cases hk
    · exact hk

/-! ## What a join leaves

After an `if`, a `match` or a loop, what the analysis has is what one way
left, with what the ways disagree about given a value taken away, and, where
the ways have to agree about what is moved, what any of them left. -/

/-- `r` is `x` with perhaps less given a value: the same names and the same
moves. -/
inductive MSt.Weaker : MSt → MSt → Prop where
  | nil : MSt.Weaker [] []
  | cons {r x : MVar} {rs xs : MSt} : r.name = x.name → r.live = x.live → (r.init = true → x.init = true) →
      MSt.Weaker rs xs → MSt.Weaker (r :: rs) (x :: xs)

theorem MSt.Weaker.refl : ∀ (σ : MSt), σ.Weaker σ
  | [] => .nil
  | _ :: σ => .cons rfl rfl id (MSt.Weaker.refl σ)

theorem MSt.Weaker.trans : ∀ {a b c : MSt}, a.Weaker b → b.Weaker c → a.Weaker c
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons n1 l1 i1 t1, .cons n2 l2 i2 t2 =>
    .cons (n1.trans n2) (l1.trans l2) (fun h => i2 (i1 h)) (t1.trans t2)

theorem MSt.Weaker.length : ∀ {a b : MSt}, a.Weaker b → a.length = b.length
  | _, _, .nil => rfl
  | _, _, .cons _ _ _ t => by simp [MSt.Weaker.length t]

theorem MAg.weaker : ∀ {r x : MSt} {ρ : VEnv}, r.Weaker x → MAg x ρ → MAg r ρ
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons hn hl hi w, .cons hn' h a =>
    .cons (hn.trans hn') (fun hv => h (by simp only [Bool.and_eq_true] at hv ⊢; exact ⟨hi hv.1, hl ▸ hv.2⟩))
      (MAg.weaker w a)

/-- The names of two states, pairwise. -/
def MSt.SameNames (a b : MSt) : Prop :=
  a.length = b.length ∧ ∀ (i : Nat) (v w : MVar), a[i]? = some v → b[i]? = some w → v.name = w.name

theorem MSt.SameNames.of_shape {a b : MSt} (h : a.shape = b.shape) : a.SameNames b := by
  have hl : a.length = b.length := by rw [← MSt.shape_length, h, MSt.shape_length]
  refine ⟨hl, fun i v w hv hw => ?_⟩
  have := congrArg (fun l => l[i]?) h
  simp only [MSt.shape, List.getElem?_map, hv, hw, Option.map_some, Option.some.injEq, Prod.mk.injEq] at this
  exact this.1

theorem MSt.SameNames.cons_iff {v w : MVar} {a b : MSt} :
    MSt.SameNames (v :: a) (w :: b) ↔ v.name = w.name ∧ a.SameNames b := by
  constructor
  · intro ⟨hl, h⟩
    refine ⟨h 0 v w rfl rfl, by simpa using hl, fun i x y hx hy => h (i + 1) x y (by simpa using hx) (by simpa using hy)⟩
  · intro ⟨hn, hl, h⟩
    refine ⟨by simp [hl], fun i x y hx hy => ?_⟩
    cases i with
    | zero => simp at hx hy; subst hx; subst hy; exact hn
    | succ i => exact h i x y (by simpa using hx) (by simpa using hy)

theorem MSt.sameMoves_refl : ∀ (σ : MSt), σ.sameMoves σ = true
  | [] => rfl
  | v :: σ => by
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_self_eq_true, true_and]
    exact MSt.sameMoves_refl σ

theorem MSt.sameMoves_symm : ∀ {a b : MSt}, a.sameMoves b = true → b.sameMoves a = true
  | [], [], _ => rfl
  | [], _ :: _, _ => rfl
  | _ :: _, [], _ => rfl
  | v :: a, w :: b, h => by
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_iff_eq] at h ⊢
    exact ⟨h.1.symm, MSt.sameMoves_symm h.2⟩

theorem MSt.sameMoves_trans : ∀ {a b c : MSt}, a.length = b.length → a.sameMoves b = true → b.sameMoves c = true →
    a.sameMoves c = true
  | [], [], _, _, _, _ => rfl
  | [], _ :: _, _, hl, _, _ => by cases hl
  | _ :: _, [], _, hl, _, _ => by cases hl
  | _ :: _, _ :: _, [], _, _, _ => rfl
  | v :: a, w :: b, u :: c, hl, h1, h2 => by
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_iff_eq] at h1 h2 ⊢
    exact ⟨h1.1.trans h2.1, MSt.sameMoves_trans (by simpa using hl) h1.2 h2.2⟩

/-- `m` is given a value only where `x` is, name by name. -/
inductive MSt.InitLe : MSt → MSt → Prop where
  | nil : MSt.InitLe [] []
  | cons {v w : MVar} {m x : MSt} : (v.init = true → w.init = true) → MSt.InitLe m x → MSt.InitLe (v :: m) (w :: x)

theorem MSt.initMeet_initLe_left : ∀ {t e : MSt}, t.length = e.length → (t.initMeet e).InitLe t
  | [], [], _ => .nil
  | v :: t, w :: e, h => by
    simp only [MSt.initMeet, List.zipWith_cons_cons]
    exact .cons (fun h => by simpa using (Bool.and_eq_true _ _ |>.mp h).1) (MSt.initMeet_initLe_left (by simpa using h))
  | [], _ :: _, h => by cases h
  | _ :: _, [], h => by cases h

theorem MSt.initMeet_initLe_right : ∀ {t e : MSt}, t.length = e.length → (t.initMeet e).InitLe e
  | [], [], _ => .nil
  | v :: t, w :: e, h => by
    simp only [MSt.initMeet, List.zipWith_cons_cons]
    exact .cons (fun h => by simpa using (Bool.and_eq_true _ _ |>.mp h).2) (MSt.initMeet_initLe_right (by simpa using h))
  | [], _ :: _, h => by cases h
  | _ :: _, [], h => by cases h

theorem MSt.SameNames.refl (σ : MSt) : σ.SameNames σ :=
  ⟨rfl, fun i v w hv hw => by rw [hv] at hw; cases hw; rfl⟩

theorem MSt.Weaker.sameMoves : ∀ {a b : MSt}, a.Weaker b → a.sameMoves b = true
  | _, _, .nil => rfl
  | _, _, .cons _ hl _ w => by
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_iff_eq]
    exact ⟨hl, MSt.Weaker.sameMoves w⟩

theorem MSt.Weaker.sameNames : ∀ {a b : MSt}, a.Weaker b → a.SameNames b
  | _, _, .nil => ⟨rfl, fun i _ _ h => by simp at h⟩
  | _, _, .cons hn _ _ w => MSt.SameNames.cons_iff.mpr ⟨hn, MSt.Weaker.sameNames w⟩

theorem MSt.SameNames.symm {a b : MSt} (h : a.SameNames b) : b.SameNames a :=
  ⟨h.1.symm, fun i v w hv hw => (h.2 i w v hw hv).symm⟩

theorem MSt.SameNames.trans {a b c : MSt} (h1 : a.SameNames b) (h2 : b.SameNames c) : a.SameNames c := by
  refine ⟨h1.1.trans h2.1, fun i v w hv hw => ?_⟩
  have : i < b.length := by rw [← h1.1]; exact (List.getElem?_eq_some_iff.mp hv).1
  obtain ⟨u, hu⟩ : ∃ u, b[i]? = some u := ⟨b[i], List.getElem?_eq_getElem this⟩
  exact (h1.2 i v u hv hu).trans (h2.2 i u w hu hw)

/-- Two ways that agree about what is moved, on names that line up, meet
weaker than either. -/
theorem MSt.Weaker.initMeet_left : ∀ {t e : MSt}, t.length = e.length → (t.initMeet e).Weaker t
  | [], [], _ => .nil
  | v :: t, w :: e, h => by
    simp only [MSt.initMeet, List.zipWith_cons_cons]
    exact .cons rfl rfl (fun h => by simpa using (Bool.and_eq_true _ _ |>.mp h).1)
      (MSt.Weaker.initMeet_left (by simpa using h))
  | [], _ :: _, h => by simp at h
  | _ :: _, [], h => by simp at h

theorem MSt.Weaker.initMeet_right : ∀ {t e : MSt}, t.SameNames e → t.sameMoves e = true → (t.initMeet e).Weaker e
  | [], [], _, _ => .nil
  | v :: t, w :: e, hn, hm => by
    obtain ⟨hn1, hn⟩ := MSt.SameNames.cons_iff.mp hn
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_iff_eq] at hm
    simp only [MSt.initMeet, List.zipWith_cons_cons]
    exact .cons hn1 hm.1 (fun h => by simpa using (Bool.and_eq_true _ _ |>.mp h).2)
      (MSt.Weaker.initMeet_right hn (by simpa [MSt.sameMoves] using hm.2))
  | [], _ :: _, hn, _ => by simp [MSt.SameNames] at hn
  | _ :: _, [], hn, _ => by simp [MSt.SameNames] at hn

/-- `b` given values as `m` has them, where `b` and `x` are one in names and
moves and `m` is given a value only where `x` is. -/
theorem MSt.Weaker.initFrom : ∀ {b m x : MSt}, b.SameNames x → b.sameMoves x = true → m.InitLe x →
    (b.initFrom m).Weaker x
  | [], [], [], _, _, _ => .nil
  | v :: b, u :: m, w :: x, hn, hm, .cons hi1 hi => by
    obtain ⟨hn1, hn⟩ := MSt.SameNames.cons_iff.mp hn
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_iff_eq] at hm
    simp only [MSt.initFrom, List.zipWith_cons_cons]
    exact .cons hn1 hm.1 hi1 (MSt.Weaker.initFrom hn hm.2 hi)
  | [], _, _ :: _, hn, _, _ => by simp [MSt.SameNames] at hn
  | _ :: _, _, [], hn, _, _ => by simp [MSt.SameNames] at hn
  | _ :: _, [], _ :: _, _, _, hi => by cases hi
  | [], _ :: _, [], _, _, hi => by cases hi

/-- The fold of a `match`'s or a loop's ways that agree: weaker than each. -/
theorem initMeetFold_weaker : ∀ (ms : List MSt) (m : MSt), (∀ w ∈ ms, m.SameNames w ∧ m.sameMoves w = true) →
    ∀ a, a = m ∨ a ∈ ms → (ms.foldl MSt.initMeet m).Weaker a
  | [], m, _, a, ha => by
    simp at ha; rw [ha]; exact MSt.Weaker.refl m
  | w :: ms, m, h, a, ha => by
    simp only [List.foldl_cons]
    have hw := h w List.mem_cons_self
    have hmw : (m.initMeet w).Weaker m := MSt.Weaker.initMeet_left hw.1.1
    have hww : (m.initMeet w).Weaker w := MSt.Weaker.initMeet_right hw.1 hw.2
    have hrest : ∀ u ∈ ms, (m.initMeet w).SameNames u ∧ (m.initMeet w).sameMoves u = true := by
      intro u hu
      have hu' := h u (List.mem_cons_of_mem _ hu)
      exact ⟨hmw.sameNames.trans hu'.1, MSt.sameMoves_trans hmw.length hmw.sameMoves hu'.2⟩
    rcases ha with ha | ha
    · rw [ha]; exact (initMeetFold_weaker ms _ hrest _ (.inl rfl)).trans hmw
    · rcases List.mem_cons.mp ha with ha | ha
      · rw [ha]; exact (initMeetFold_weaker ms _ hrest _ (.inl rfl)).trans hww
      · exact initMeetFold_weaker ms _ hrest a (.inr ha)

/-- What `joinAll` leaves is weaker than each way it joined, and than `σ`
where there were none. -/
theorem joinFold_weaker (code : Lit.Code) : ∀ (L : List MSt) (acc : MSt) {st st' : DSt} {r : MSt},
    (∀ τ ∈ L, acc.SameNames τ) →
    (L.foldlM (fun (acc w : MSt) => match acc.join w with
      | some j => Pure.pure j
      | none => (throw code : MoveM MSt)) acc).run st = .ok (r, st') →
    ∀ a, a = acc ∨ a ∈ L → r.Weaker a
  | [], acc, st, st', r, _, h, a, ha => by
    simp only [List.foldlM] at h; cases h
    simp at ha; rw [ha]; exact MSt.Weaker.refl acc
  | w :: L, acc, st, st', r, hn, h, a, ha => by
    simp only [List.foldlM] at h
    replace h := MoveM.bind_ok h
    obtain ⟨j, st1, h1, h2⟩ := h
    have hj : j = acc.initMeet w ∧ acc.sameMoves w = true := by
      split at h1
      · rename_i hjoin; cases h1; exact MSt.join_eq hjoin
      · cases h1
    obtain ⟨rfl, hsm⟩ := hj
    have hw := hn w List.mem_cons_self
    have hja : (acc.initMeet w).Weaker acc := MSt.Weaker.initMeet_left hw.1
    have hjw : (acc.initMeet w).Weaker w := MSt.Weaker.initMeet_right hw hsm
    have hrest : ∀ τ ∈ L, (acc.initMeet w).SameNames τ := fun τ hτ =>
      hja.sameNames.trans (hn τ (List.mem_cons_of_mem _ hτ))
    rcases ha with ha | ha
    · rw [ha]; exact (joinFold_weaker code L _ hrest h2 _ (.inl rfl)).trans hja
    · rcases List.mem_cons.mp ha with ha | ha
      · rw [ha]; exact (joinFold_weaker code L _ hrest h2 _ (.inl rfl)).trans hjw
      · exact joinFold_weaker code L _ hrest h2 a (.inr ha)

theorem joinAll_weaker (code : Lit.Code) : ∀ (L : List MSt) (σ : MSt) {st st' : DSt} {r : MSt},
    (∀ τ ∈ L, σ.SameNames τ) → (joinAll code σ L).run st = .ok (r, st') →
    (L = [] → r = σ) ∧ ∀ a ∈ L, r.Weaker a
  | [], σ, st, st', r, _, h => by
    simp only [joinAll] at h; cases h
    exact ⟨fun _ => rfl, fun _ ha => by simp at ha⟩
  | τ :: L, σ, st, st', r, hn, h => by
    simp only [joinAll] at h
    have hτ := hn τ List.mem_cons_self
    have hrest : ∀ w ∈ L, τ.SameNames w := fun w hw => hτ.symm.trans (hn w (List.mem_cons_of_mem _ hw))
    refine ⟨fun e => (nomatch e), fun a ha => ?_⟩
    rcases List.mem_cons.mp ha with ha | ha
    · rw [ha]; exact joinFold_weaker code L τ hrest h _ (.inl rfl)
    · exact joinFold_weaker code L τ hrest h a (.inr ha)

/-- What `joinIf` leaves is weaker than what the way the run took left. -/
theorem joinIf_weaker {t e : MSt} {tc ec : Bool} {st st' : DSt} {r : MSt} (hn : t.SameNames e)
    (h : (joinIf t tc e ec).run st = .ok (r, st')) :
    (tc = true → r.Weaker t) ∧ (ec = true → r.Weaker e) := by
  unfold joinIf at h
  rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hc, h⟩
  · cases h
  · cases h
    have hl : t.length = e.length := hn.1
    by_cases hB : (!tc && ec) = true
    · simp only [hB, ↓reduceIte]
      simp only [Bool.and_eq_true, Bool.not_eq_true'] at hB
      refine ⟨fun htc => (by rw [hB.1] at htc; cases htc), fun _ => ?_⟩
      exact MSt.Weaker.initFrom (MSt.SameNames.refl e) (MSt.sameMoves_refl e) (MSt.initMeet_initLe_right hl)
    · simp only [hB, Bool.false_eq_true, ↓reduceIte]
      refine ⟨fun _ => MSt.Weaker.initFrom (MSt.SameNames.refl t) (MSt.sameMoves_refl t)
        (MSt.initMeet_initLe_left hl), fun hec => ?_⟩
      have htc : tc = true := by
        cases tc with
        | true => rfl
        | false => simp [hec] at hB
      have hsm : t.sameMoves e = true := by
        simp only [htc, hec, Bool.true_and, Bool.not_eq_false'] at hc
        exact hc
      exact MSt.Weaker.initFrom hn hsm (MSt.initMeet_initLe_right hl)

/-- What `joinMatch` leaves is weaker than what any arm that completes
left. -/
theorem joinMatch_weaker {σ : MSt} {arms : List (MSt × Bool)} {st st' : DSt} {r : MSt}
    (hn : ∀ a ∈ arms, σ.SameNames a.1) (h : (joinMatch σ arms).run st = .ok (r, st')) :
    ∀ a, (a, true) ∈ arms → r.Weaker a := by
  intro a ha
  have hmem : a ∈ (arms.filter (·.2)).map (·.1) := by
    simp only [List.mem_map, List.mem_filter]
    exact ⟨(a, true), ⟨ha, rfl⟩, rfl⟩
  have hall : ∀ w ∈ (arms.filter (·.2)).map (·.1), σ.SameNames w := by
    intro w hw
    simp only [List.mem_map, List.mem_filter] at hw
    obtain ⟨p, ⟨hp, -⟩, rfl⟩ := hw
    exact hn p hp
  simp only [joinMatch] at h
  generalize (arms.filter (·.2)).map (·.1) = merging at h hmem hall
  cases merging with
  | cons m ms =>
    dsimp only at h
    rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hc, h⟩
    · cases h
    · cases h
      simp only [Bool.not_eq_false', List.all_eq_true] at hc
      have hm := hall m List.mem_cons_self
      refine initMeetFold_weaker ms m (fun w hw => ⟨hm.symm.trans (hall w (List.mem_cons_of_mem _ hw)), hc w hw⟩) a ?_
      rcases List.mem_cons.mp hmem with rfl | hmem
      · exact .inl rfl
      · exact .inr hmem
  | nil => simp at hmem

/-! ## What a run may do where the analysis let it through -/

/-- Every parameter the caller reads back is borrowed: a `mut` or `out`
parameter is, and nothing else is read back. -/
def BackLent (σ : MSt) : Prop := ∀ v ∈ σ, v.back = true → v.lent = true

theorem BackLent.le : ∀ {σ τ : MSt}, BackLent σ → σ.Le τ → BackLent τ
  | _, _, _, .nil => fun _ h => by cases h
  | v :: σ, w :: τ, h, .cons _ hb _ hl _ t => fun u hu hback => by
    rcases List.mem_cons.mp hu with rfl | hu
    · rw [← hl]; exact h v List.mem_cons_self (hb ▸ hback)
    · exact BackLent.le (fun x hx => h x (List.mem_cons_of_mem _ hx)) t u hu hback

theorem BackLent.inside {σ τ : MSt} (h : BackLent σ) (hi : σ.Inside τ) : BackLent τ := by
  obtain ⟨δ, τ', rfl, hd, l⟩ := hi
  intro v hv hb
  rcases List.mem_append.mp hv with hv | hv
  · rw [hd v hv] at hb; cases hb
  · exact BackLent.le h l v hv hb

/-- Every `mut` and `out` parameter holding a value survives a step that
un-gives nothing, since they are borrowed and so never moved. -/
theorem outsSet_le : ∀ {σ τ : MSt}, σ.outsSet = true → σ.Le τ → BackLent σ → τ.outsSet = true
  | _, _, _, .nil, _ => rfl
  | v :: σ, w :: τ, h, .cons _ hb hi hl hv t, hbl => by
    unfold MSt.outsSet at h ⊢
    simp only [List.all_cons, Bool.and_eq_true, Bool.or_eq_true, Bool.not_eq_true'] at h ⊢
    refine ⟨?_, outsSet_le h.2 t (fun x hx => hbl x (List.mem_cons_of_mem _ hx))⟩
    rcases h.1 with h1 | h1
    · by_cases hback : v.back = true
      · left; exact ⟨hi h1.1, hv (hbl v List.mem_cons_self hback) h1.2⟩
      · right; rw [← hb]; simpa using hback
    · right; rw [← hb]; exact h1

/-- A shape whose outermost part is `O`, a function's parameters and the
consts it sees, and whose innermost part, bound inside the body, has no
parameter read back by the caller; and every parameter read back is
borrowed. -/
def MFr (O : List (String × Bool)) (σ : MSt) : Prop :=
  (∃ δ : List (String × Bool), (∀ p ∈ δ, p.2 = false) ∧ σ.shape = δ ++ O) ∧ BackLent σ

theorem MFr.of_le {O : List (String × Bool)} {σ σ' : MSt} (h : MFr O σ) (l : σ.Le σ') : MFr O σ' := by
  obtain ⟨⟨δ, hδ, hs⟩, hbl⟩ := h
  exact ⟨⟨δ, hδ, l.shape.symm.trans hs⟩, hbl.le l⟩

theorem MFr.inside {O : List (String × Bool)} {σ τ : MSt} (h : MFr O σ) (hi : σ.Inside τ) : MFr O τ := by
  obtain ⟨⟨δ, hd, e⟩, hbl⟩ := h
  refine ⟨?_, hbl.inside hi⟩
  obtain ⟨δ', τ', rfl, hd', l⟩ := hi
  refine ⟨δ'.shape ++ δ, fun p hp => ?_, ?_⟩
  · rcases List.mem_append.mp hp with hp | hp
    · simp only [MSt.shape, List.mem_map] at hp
      obtain ⟨v, hv, rfl⟩ := hp
      exact hd' v hv
    · exact hd p hp
  · simp only [MSt.shape, List.map_append, List.append_assoc]
    exact congrArg _ (l.shape.symm.trans e)

/-- Where the analysis has every `mut` and `out` parameter holding a value,
so it has after any step that un-gives nothing. -/
theorem MFr.outsSet {O : List (String × Bool)} {σ τ : MSt} (h : MFr O σ) (l : σ.Le τ) (ho : σ.outsSet = true) :
    τ.outsSet = true := outsSet_le ho l h.2

/-- Where the run leaves the function, every `mut` and `out` parameter
holds a value: the analysis said so of variables the run's line up with. -/
def MExitOk (O : List (String × Bool)) (ρ : VEnv) : Prop :=
  ∃ σ, MFr O σ ∧ MAg σ ρ ∧ σ.outsSet = true

/-- How a run the analysis let through may stop: never at a variable that
holds nothing, never out of a loop or a body, and returning only with every
`mut` and `out` parameter holding a value. -/
def MStop (O : List (String × Bool)) : Stop → Prop
  | .uninit | .early _ | .brk _ | .cont _ => False
  | .returned _ ρ => MExitOk O ρ
  | _ => True

/-- What a run from `ρ` came to: a value meeting `P` and variables that line
up with `σ'`; or a stop as `MStop` says. -/
def MRes (O : List (String × Bool)) (σ' : MSt) {α} (P : α → VEnv → Prop) : Except Stop (α × VEnv) → Prop
  | .ok (a, ρ') => MAg σ' ρ' ∧ P a ρ'
  | .error s => MStop O s

/-- Evaluating `m` from variables that line up with `σ`, in a body whose
frame is `O`, comes to what `MRes` says, `σ'` what the analysis has after. -/
def MT (O : List (String × Bool)) (σ σ' : MSt) {α} (P : α → VEnv → Prop) (m : EvalT α) : Prop :=
  MFr O σ → ∀ ρ, MAg σ ρ → MRes O σ' P (m.run ρ)

namespace MT

variable {O : List (String × Bool)} {σ σ1 σ2 : MSt} {α β : Type} {P : α → VEnv → Prop} {Q : β → VEnv → Prop}

theorem pure' {a : α} (h : ∀ ρ, MAg σ ρ → MAg σ1 ρ ∧ P a ρ) : MT O σ σ1 P (Pure.pure a) :=
  fun _ ρ hρ => h ρ hρ

theorem res_bind {x : EvalT α} {f : α → EvalT β} (hx : MRes O σ1 P (x.run ρ))
    (hf : ∀ a ρ1, P a ρ1 → MAg σ1 ρ1 → MRes O σ2 Q ((f a).run ρ1)) : MRes O σ2 Q ((x >>= f).run ρ) := by
  simp only [StateT.run_bind]
  generalize x.run ρ = r at hx ⊢
  cases r with
  | error s => exact hx
  | ok p =>
    obtain ⟨a, ρ1⟩ := p
    obtain ⟨h1, hp⟩ := hx
    exact hf a ρ1 hp h1

theorem bind' {x : EvalT α} {f : α → EvalT β} (hx : MT O σ σ1 P x) (hs : σ.Le σ1)
    (hf : ∀ a ρ1, P a ρ1 → MFr O σ1 → MAg σ1 ρ1 → MRes O σ2 Q ((f a).run ρ1)) : MT O σ σ2 Q (x >>= f) :=
  fun hfr ρ hρ => res_bind (hx hfr ρ hρ) (fun a ρ1 hp h1 => hf a ρ1 hp (hfr.of_le hs) h1)

theorem bind {x : EvalT α} {f : α → EvalT β} (hx : MT O σ σ1 P x) (hs : σ.Le σ1)
    (hf : ∀ a, MT O σ1 σ2 Q (f a)) : MT O σ σ2 Q (x >>= f) :=
  bind' hx hs (fun a ρ1 _ hfr h1 => hf a hfr ρ1 h1)

theorem throw' {s : Stop} (h : MStop O s) : MT O σ σ1 P (throw s) := fun _ _ _ => h

theorem wrong : MT O σ σ1 P (throw .wrong) := throw' trivial
theorem fuel : MT O σ σ1 P (throw .fuel) := throw' trivial
theorem unsupported {g : Gap} : MT O σ σ1 P (throw (.unsupported g)) := throw' trivial

theorem get' {f : VEnv → EvalT α} (h : ∀ ρ, MFr O σ → MAg σ ρ → MRes O σ1 P ((f ρ).run ρ)) :
    MT O σ σ1 P (get >>= f) := by
  intro hfr ρ hρ
  simp only [StateT.run_bind, StateT.run_get, pure_bind]
  exact h ρ hfr hρ

theorem weaken {m : EvalT α} {P' : α → VEnv → Prop} (h : MT O σ σ1 P m) (hp : ∀ a ρ, P a ρ → P' a ρ) :
    MT O σ σ1 P' m := by
  intro hfr ρ hρ
  have h := h hfr ρ hρ
  generalize m.run ρ = r at h ⊢
  cases r with
  | error s => exact h
  | ok q => obtain ⟨a, ρ'⟩ := q; exact ⟨h.1, hp a ρ' h.2⟩

theorem map' {m : EvalT α} {f : α → β} (h : MT O σ σ1 P m) (hp : ∀ a ρ, P a ρ → Q (f a) ρ) :
    MT O σ σ1 Q (f <$> m) := by
  intro hfr ρ hρ
  have h := h hfr ρ hρ
  simp only [StateT.run_map]
  generalize m.run ρ = r at h ⊢
  cases r with
  | error s => exact h
  | ok q => obtain ⟨a, ρ'⟩ := q; exact ⟨h.1, hp a ρ' h.2⟩

/-- A step that reads no variable: it stops only where any step may, or
returns early where the analysis said the way out was open. -/
theorem here {m : EvalM α} (hc : Calm m) (he : ∀ r, m = .error (.early r) → σ.outsSet = true)
    (hp : ∀ a, m = .ok a → ∀ ρ, MAg σ ρ → P a ρ) : MT O σ σ P (Fun.here m) := by
  intro hfr ρ hρ
  cases hm : m with
  | ok a => simp only [Fun.here]; exact ⟨hρ, hp a hm ρ hρ⟩
  | error s =>
    have hcs := hc s hm
    cases s with
    | early r =>
      simp only [Fun.here, Fun.earlyHere, StateT.run_bind, StateT.run_get, pure_bind]
      exact ⟨σ, hfr, hρ, he r hm⟩
    | uninit => exact absurd rfl hcs.1
    | brk ρ' => exact absurd rfl (hcs.2.1 ρ')
    | cont ρ' => exact absurd rfl (hcs.2.2.1 ρ')
    | returned o ρ' => exact absurd rfl (hcs.2.2.2 o ρ')
    | _ => exact trivial

/-- A step that reads no variable, run from `ρ`: what it gives, the variables
as they were. -/
theorem res_here {ρ : VEnv} {m : EvalM α} (hc : Calm m) (he : ∀ r, m = .error (.early r) → σ.outsSet = true)
    (hfr : MFr O σ) (hρ : MAg σ ρ) : MRes O σ (fun a ρ' => m = .ok a ∧ ρ' = ρ) ((Fun.here m).run ρ) := by
  cases hm : m with
  | ok a => simp only [Fun.here]; exact ⟨hρ, rfl, rfl⟩
  | error s =>
    have hcs := hc s hm
    cases s with
    | early r =>
      simp only [Fun.here, Fun.earlyHere, StateT.run_bind, StateT.run_get, pure_bind]
      exact ⟨σ, hfr, hρ, he r hm⟩
    | uninit => exact absurd rfl hcs.1
    | brk ρ' => exact absurd rfl (hcs.2.1 ρ')
    | cont ρ' => exact absurd rfl (hcs.2.2.1 ρ')
    | returned o ρ' => exact absurd rfl (hcs.2.2.2 o ρ')
    | _ => exact trivial

theorem res_get {ρ : VEnv} {f : VEnv → EvalT α} (h : MRes O σ1 P ((f ρ).run ρ)) :
    MRes O σ1 P ((get >>= f).run ρ) := by
  simp only [StateT.run_bind, StateT.run_get, pure_bind]; exact h

theorem res_set_pure {ρ ρ' : VEnv} {a : α} (h : MAg σ1 ρ') (hp : P a ρ') :
    MRes O σ1 P ((set ρ' >>= fun _ => (Pure.pure a : EvalT α)).run ρ) := by
  simp only [StateT.run_bind, StateT.run_set, StateT.run_pure, pure_bind]
  exact ⟨h, hp⟩

theorem earlyHere {r : Val} (h : σ.outsSet = true) : MT O σ σ1 P (Fun.earlyHere r) := by
  intro hfr ρ hρ
  simp only [Fun.earlyHere, StateT.run_bind, StateT.run_get, pure_bind]
  exact ⟨σ, hfr, hρ, h⟩

/-- A step evaluation takes only on the value it has in hand, with no way
out of the function, that gives its value whatever it is. -/
theorem calm {m : EvalM α} (hc : Calm m) (he : NoEarly m) : MT O σ σ (fun _ _ => True) (Fun.here m) :=
  MT.here hc (fun r hr => absurd hr (he r)) (fun _ _ _ _ => trivial)

end MT

/-! ## Steps of the analysis -/

theorem readM_run {σ σ' : MSt} {x : String} {st st' : DSt} (h : (readM σ x).run st = .ok (σ', st')) :
    σ = σ' ∧ st = st' ∧ ∀ v, σ.find x = some v → (v.init && v.live) = true := by
  unfold readM at h
  split at h
  · rename_i v hv
    rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hi, h⟩
    · cases h
    rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hl, h⟩
    · cases h
    cases h
    refine ⟨rfl, rfl, fun w hw => ?_⟩
    rw [hv] at hw; cases hw
    simp only [Bool.not_eq_false'] at hi hl
    simp [hi, hl]
  · rename_i hn
    cases h
    exact ⟨rfl, rfl, fun w hw => by rw [hn] at hw; cases hw⟩

theorem exitM_run {σ σ' : MSt} {st st' : DSt} (h : (exitM σ).run st = .ok (σ', st')) :
    σ = σ' ∧ st = st' ∧ σ.outsSet = true := by
  unfold exitM at h
  rcases MoveM.ite_run h with ⟨ho, h⟩ | ⟨-, h⟩
  · cases h; exact ⟨rfl, rfl, ho⟩
  · cases h

/-- A read that may move: `x` holds a value, and where its type in `Γa` is
not copy and it is no const, it is moved. -/
theorem moveM_run {Γa : Ctx} {σ σ' : MSt} {x : String} {st st' : DSt} (h : (moveM Γa σ x).run st = .ok (σ', st')) :
    (∀ v, σ.find x = some v → (v.init && v.live) = true) ∧ (σ' = σ ∨ σ' = σ.take x) ∧
      (∀ t k, Γa.lookup x = some (t, k) → k ≠ .const_ → t.isCopy = false → σ' = σ.take x) := by
  unfold moveM at h
  split at h
  · rename_i t hl
    rcases MoveM.ite_run h with ⟨hc, h⟩ | ⟨-, h⟩
    · obtain ⟨rfl, -, hr⟩ := readM_run h
      exact ⟨hr, .inl rfl, fun t' k' hl' hk _ => by rw [hl] at hl'; cases hl'; exact absurd rfl hk⟩
    · cases h
  · rename_i t k hl
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    obtain ⟨rfl, rfl, hr⟩ := readM_run h1
    rcases MoveM.ite_run h with ⟨hc, h⟩ | ⟨hc, h⟩
    · cases h
      exact ⟨hr, .inl rfl, fun t' k' hl' _ hnc => by rw [hl] at hl'; cases hl'; rw [hc] at hnc; cases hnc⟩
    · split at h
      · rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
        · cases h
        rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
        · cases h
        replace h := MoveM.bind_ok h
        obtain ⟨_, st2, -, h⟩ := h
        cases h
        exact ⟨hr, .inr rfl, fun _ _ _ _ _ => rfl⟩
      · rename_i hn
        cases h
        -- `x` is in `Γa` but not among the analysis's variables: nothing to
        -- say of a move the run makes, so the run must not make one; it
        -- does not, since the run's variable is then missing too.
        exact ⟨hr, .inl rfl, fun t' k' hl' _ hnc => by
          rw [hl] at hl'; cases hl'
          exact absurd_take hn⟩
  · rename_i hl
    obtain ⟨rfl, -, hr⟩ := readM_run h
    exact ⟨hr, .inl rfl, fun t k hl' => by rw [hl] at hl'; cases hl'⟩
where
  absurd_take {σ : MSt} {x : String} (hn : σ.find x = none) : σ = σ.take x := by
    unfold MSt.take
    exact (update_none hn).symm
  update_none {f : MVar → MVar} {σ : MSt} {x : String} (hn : σ.find x = none) : MSt.update f x σ = σ := by
    induction σ with
    | nil => rfl
    | cons v σ ih =>
      simp only [MSt.find, List.find?] at hn
      split at hn
      · cases hn
      · rename_i e
        simp only [MSt.update, e, Bool.false_eq_true, ↓reduceIte]
        rw [ih hn]


/-- A step un-giving nothing finds what it was given, given still. -/
theorem MSt.Le.find : ∀ {σ τ : MSt}, σ.Le τ → ∀ {x : String} {v : MVar}, σ.find x = some v →
    ∃ w, τ.find x = some w ∧ w.name = v.name ∧ (v.init = true → w.init = true)
  | _, _, .nil, _, _, h => by simp [MSt.find] at h
  | v :: σ, w :: τ, .cons hn _ hi _ _ t, x, u, h => by
    simp only [MSt.find, List.find?] at h ⊢
    rw [← hn]
    split
    · next e => rw [e] at h; cases h; exact ⟨w, rfl, hn.symm, hi⟩
    · next e => rw [e] at h; exact MSt.Le.find t h

theorem MSt.Le.find_back : ∀ {σ τ : MSt}, σ.Le τ → ∀ {x : String} {w : MVar}, τ.find x = some w →
    ∃ v, σ.find x = some v ∧ (v.init = true → w.init = true)
  | _, _, .nil, _, _, h => by simp [MSt.find] at h
  | v :: σ, w :: τ, .cons hn _ hi _ _ t, x, u, h => by
    simp only [MSt.find, List.find?] at h ⊢
    rw [hn]
    split
    · next e => rw [e] at h; cases h; exact ⟨v, rfl, hi⟩
    · next e => rw [e] at h; exact MSt.Le.find_back t h

/-- The variable of each `mut` or `out` argument written back through
steps was given a value, where it names one. -/
def RootsInit (σ : MSt) : List (Option Mode × Expr) → List Mode → Prop
  | (_, e) :: args, m :: ms =>
    ((m == .mut || m == .out) = true → ∀ x st, placeOf e = some (x, st) → st ≠ [] →
      ∀ v, σ.find x = some v → v.init = true) ∧ RootsInit σ args ms
  | _, _ => True

theorem RootsInit.le {σ τ : MSt} (l : σ.Le τ) : ∀ {args : List (Option Mode × Expr)} {ms : List Mode},
    RootsInit σ args ms → RootsInit τ args ms
  | _ :: _, _ :: _, ⟨h1, h2⟩ => ⟨fun hm x st hp hst _ hw =>
      let ⟨v, hv, hi⟩ := l.find_back hw
      hi (h1 hm x st hp hst v hv), RootsInit.le l h2⟩
  | [], _, _ => trivial
  | _ :: _, [], _ => trivial

/-- Where every root is given a value and live, and the analysis lines up
with the run, every root the call writes back through holds a value. -/
theorem roots_of {σ : MSt} {ρ : VEnv} (h : MAg σ ρ) :
    ∀ {args : List (Option Mode × Expr)} {ps : List (Mode × Ty)},
    RootsInit σ args (ps.map (·.1)) → RootsLive σ args (ps.map (·.1)) → Roots ρ args ps
  | (_, e) :: args, (m, t) :: ps, ⟨i1, i2⟩, ⟨l1, l2⟩ => by
    refine ⟨fun hm x st hp hst => ?_, roots_of h i2 l2⟩
    exact MAg.lookup h (fun v hv => by simp [i1 hm x st hp hst v hv, l1 hm x st hp v hv])
  | [], _, _, _ => trivial
  | _ :: _, [], _, _ => trivial

/-! ## The analysis's wrapping of an expression -/

theorem exprAt_run {st st' : DSt} {a : Nat} (h : exprAt.run st = .ok (a, st')) : st' = st := by
  simp only [exprAt, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, StateT.run, Pure.pure,
    StateT.pure] at h
  cases h; rfl

theorem exprFrom_pure_run {k : Nat} {σ σ' : MSt} {st st' : DSt}
    (h : (exprFrom k >>= fun _ => (Pure.pure σ : MoveM MSt)).run st = .ok (σ', st')) :
    σ' = σ ∧ st'.movedHeld = st.movedHeld := by
  simp only [exprFrom, bind, StateT.bind, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet,
    StateT.run, Pure.pure, StateT.pure] at h
  cases h; exact ⟨rfl, rfl⟩

theorem MoveM.pure_run {α} {a b : α} {st st' : DSt} (h : (Pure.pure a : MoveM α).run st = .ok (b, st')) :
    a = b ∧ st = st' := by
  cases h; exact ⟨rfl, rfl⟩

/-- An expression's walk, with what it adds: the counter of expressions set
past it. What the walk itself came to. -/
theorem wrap_run {m : MoveM MSt} {e : Expr} {σ' : MSt} {st st' : DSt}
    (h : (exprAt >>= fun start => m >>= fun σ' =>
      exprFrom (start + e.nodes) >>= fun _ => (Pure.pure σ' : MoveM MSt)).run st = .ok (σ', st')) :
    ∃ st2, m.run st = .ok (σ', st2) ∧ st2.movedHeld = st'.movedHeld := by
  replace h := MoveM.bind_ok h
  obtain ⟨start, st1, h1, h⟩ := h
  obtain rfl := exprAt_run h1
  replace h := MoveM.bind_ok h
  obtain ⟨σ'', st2, h2, h⟩ := h
  obtain ⟨rfl, hf⟩ := exprFrom_pure_run h
  exact ⟨st2, h2, hf.symm⟩

/-- Where the flag ends clear it was clear all along: a step keeps or sets
it. -/
theorem flag_before {st st' : DSt} (mono : st.movedHeld = true → st'.movedHeld = true)
    (hf : st'.movedHeld = false) : st.movedHeld = false := by
  cases h : st.movedHeld
  · rfl
  · rw [mono h] at hf; cases hf

/-- The context the analysis walks an expression in matters only by its
variables. -/
theorem Ctx.lookup_of_vars {Γa Γ : Ctx} (h : Γa.vars = Γ.vars) (x : String) : Γa.lookup x = Γ.lookup x := by
  simp [Ctx.lookup, h]

/-- `Γb` is `Γa` with each variable's type perhaps copy where `Γa`'s is not,
the same variables of the same kinds otherwise: the analysis, walking an
expression in `Γa`, takes at least where the run, in `Γb`, does. An instance
of a generic function is one of its generic context (`CtxLe.inst`). -/
def CtxLe (Γa Γb : Ctx) : Prop :=
  ∀ x, (Γa.lookup x = none → Γb.lookup x = none) ∧
    ∀ t k, Γa.lookup x = some (t, k) → ∃ t', Γb.lookup x = some (t', k) ∧ (t.isCopy = true → t'.isCopy = true)

theorem CtxLe.refl (Γ : Ctx) : CtxLe Γ Γ := fun _ => ⟨id, fun t _ h => ⟨t, h, id⟩⟩

theorem CtxLe.of_vars {Γa Γb : Ctx} (h : Γa.vars = Γb.vars) : CtxLe Γa Γb := by
  intro x
  simp only [Ctx.lookup, h]
  exact ⟨id, fun t k h => ⟨t, h, id⟩⟩

theorem CtxLe.of_vars_eq {Γa Γb Γb' : Ctx} (h : CtxLe Γa Γb) (e : Γb'.vars = Γb.vars) : CtxLe Γa Γb' := by
  intro x
  have := h x
  simp only [Ctx.lookup, e] at this ⊢
  exact this

/-- What the run looks up, the analysis looked up too, at a type copy only
where the run's is. -/
theorem CtxLe.lookup_some {Γa Γb : Ctx} (h : CtxLe Γa Γb) {x : String} {t : Ty} {k : Kind}
    (hl : Γb.lookup x = some (t, k)) : ∃ t', Γa.lookup x = some (t', k) ∧ (t'.isCopy = true → t.isCopy = true) := by
  cases hl' : Γa.lookup x with
  | none => rw [(h x).1 hl'] at hl; cases hl
  | some p =>
    obtain ⟨t', k'⟩ := p
    obtain ⟨t'', h2, hc⟩ := (h x).2 t' k' hl'
    rw [h2] at hl
    cases hl
    exact ⟨t', rfl, hc⟩

theorem CtxLe.inst (θ : List (String × Ty)) (Γ : Ctx) : CtxLe Γ (Γ.inst θ) := by
  intro x
  rw [Ctx.lookup_inst]
  refine ⟨fun h => by rw [h]; rfl, fun t k h => ⟨t.inst θ, by rw [h]; rfl, Ty.isCopy_inst θ t⟩⟩

theorem Ctx.lends_enter (Γ : Ctx) (e : Expr) : (Γ.enter e).lends = (Γ.lends && !buildsFromParts e) := rfl
theorem Ctx.lookup_enter (Γ : Ctx) (e : Expr) (x : String) : (Γ.enter e).lookup x = Γ.lookup x := rfl
theorem Ctx.vars_enter (Γ : Ctx) (e : Expr) : (Γ.enter e).vars = Γ.vars := rfl
theorem Ctx.vars_borrow (Γ : Ctx) : Γ.borrow.vars = Γ.vars := rfl
theorem Ctx.vars_key (Γ : Ctx) : Γ.key.vars = Γ.vars := rfl
theorem Ctx.vars_through (Γ : Ctx) : Γ.through.vars = Γ.vars := rfl
theorem Ctx.vars_forArg (Γ : Ctx) (m : Mode) : (Γ.forArg m).vars = Γ.vars := rfl
theorem Ctx.vars_under (Γ : Ctx) (b : Expr) : (Γ.under b).vars = Γ.vars := by unfold Ctx.under; split <;> rfl
theorem Ctx.lends_borrow (Γ : Ctx) : Γ.borrow.lends = true := rfl
theorem Ctx.lends_key (Γ : Ctx) : Γ.key.lends = true := rfl
theorem Ctx.lends_through (Γ : Ctx) : Γ.through.lends = true := rfl
theorem Ctx.lends_forArg (Γ : Ctx) (m : Mode) : (Γ.forArg m).lends = (m != .in_) := rfl
theorem Ctx.lends_under (Γ : Ctx) (b : Expr) : (Γ.under b).lends = (Γ.lends || (placeOf b).isSome) := by
  unfold Ctx.under
  split
  · rename_i h; simp [Ctx.through, h]
  · rename_i h; simp only [Bool.not_eq_true, Option.isSome_eq_false_iff, Option.isNone_iff_eq_none] at h
    simp [h]

/-- A builder's walk takes no notice of the position it is in. -/
theorem mE_enter (sigs : List (String × Sig)) (Γa : Ctx) (b : Bool) (σ : MSt) (e : Expr) :
    mE sigs Γa b σ e = mE sigs Γa (b && !buildsFromParts e) σ e := by
  cases e <;> simp [buildsFromParts] <;> (try rw [mE, mE]) <;> simp

/-! ## Expressions -/

/-- What a `mut` or `out` argument's place gives: a value, where `need` asks
for one, and its variable, where the argument is a part of it or `need`
asks, given a value and not moved where the analysis is after the place. -/
def MPlace (σ' : MSt) (need : Bool) (e : Expr) (o : Option Val) : Prop :=
  (need = true → o.isSome = true) ∧
    ∀ x st, placeOf e = some (x, st) → (need = true ∨ st ≠ []) → ∀ v, σ'.find x = some v → v.init = true

/-- What a call's arguments give: one for each parameter there is an
argument for, each a value but for an `out` parameter, and every variable
written back through steps given a value where the analysis is after the
arguments. -/
def MArgs (σ' : MSt) (args : List (Option Mode × Expr)) (ps : List (Mode × Ty)) (vs : List (Option Val × List Val)) :
    Prop :=
  vs.length = min args.length ps.length ∧ (∀ q ∈ (vs.map (·.1)).zip ps, q.2.1 ≠ .out → q.1.isSome = true) ∧
    RootsInit σ' args (ps.map (·.1))

/-- Calls that keep the promise: given a value for every parameter but an
`out` one, at type arguments its bounds admit, a call stops only where any
step may, and returns a value for every `mut` and `out` parameter. -/
def CallMove (sigs : List (String × Sig)) (call : CallFn) : Prop :=
  ∀ f sig θ vals, sigs.lookup f = some sig → groundArgs θ = true →
    (∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) →
    (∀ q ∈ vals.zip sig.params, q.2.1 ≠ .out → q.1.isSome = true) →
    match call f θ vals with
    | .ok (_, after) => vals.length = sig.params.length ∧ after.length = sig.params.length ∧
        ∀ q ∈ after.zip sig.params, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true
    | .error s => s ≠ .uninit ∧ (∀ r, s ≠ .early r) ∧ (∀ ρ, s ≠ .brk ρ) ∧ (∀ ρ, s ≠ .cont ρ) ∧
        (∀ o ρ, s ≠ .returned o ρ)

/-- Ground type arguments the checker's bounds check lets through are ones
the bounds admit. -/
theorem admitted_of_check {Γ : Ctx} {tps : List (String × Option Bound)} {θ : List (String × Ty)}
    (hg : groundArgs θ = true) (hb : boundsHold Γ tps θ = true) (hd : allDecided tps θ = true) :
    ∀ p b, (p, some b) ∈ tps → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true := by
  intro p b hp
  simp only [allDecided, List.all_eq_true] at hd
  simp only [boundsHold, List.all_eq_true] at hb
  obtain ⟨u, hu⟩ := Option.isSome_iff_exists.mp (hd _ hp)
  have hadm := hb _ hp
  simp only [hu] at hadm
  refine ⟨u, hu, ?_⟩
  have hgu : u.containsVar = false := by
    simp only [groundArgs, List.all_eq_true, Bool.and_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true] at hg
    exact (hg (p, u) (lookup_mem_of hu)).1
  cases u
  case var n => simp [Ty.containsVar] at hgu
  all_goals cases b <;> first | (simp [Bound.admitsTy]; done) | simpa [Ctx.admits, Bound.admitsTy] using hadm

/-- Writing back a call's `mut` and `out` arguments, where each variable
written through steps holds a value and the call gave every such parameter
one, gives each argument's variable a value, as the analysis has it, and
returns early only through an index with `?` or `!`. -/
theorem writeBack_move (Γ : Ctx) : ∀ (ρ : VEnv) (σ : MSt) (args : List (Option Mode × Expr)) (ps : List (Mode × Ty))
    (kvss : List (List Val)) (after : List (Option Val)),
    MAg σ ρ → Roots ρ args ps → (∀ q ∈ after.zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true) →
    after.length = ps.length → kvss.length = ps.length → ps.length ≤ args.length →
    match writeBack Γ ρ args ps kvss after with
    | .ok ρ' => MAg (giveArgsM σ args (ps.map (·.1))) ρ'
    | .error s => CalmStop s ∧ (∀ r, s = .early r → argsExit args (ps.map (·.1)) = true)
  | ρ, σ, (mk, e) :: args, (mode, t) :: ps, kvs :: kvss, a :: after, hρ, hr, ha, hla, hlk, hle => by
    obtain ⟨hr1, hr2⟩ := hr
    have ha' : ∀ q ∈ after.zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true :=
      fun q hq => ha q (List.mem_cons_of_mem _ hq)
    have hla' : after.length = ps.length := by simpa using hla
    have hlk' : kvss.length = ps.length := by simpa using hlk
    have hle' : ps.length ≤ args.length := by simpa using hle
    have rest : ∀ ρ1 σ1, MAg σ1 ρ1 → Roots ρ1 args ps →
        match writeBack Γ ρ1 args ps kvss after with
          | .ok ρ' => MAg (giveArgsM σ1 args (ps.map (·.1))) ρ'
          | .error s => CalmStop s ∧ (∀ r, s = .early r → (((mode == .mut || mode == .out) &&
              match placeOf e with
              | some (_, steps) => stepsExit steps
              | none => false) || argsExit args (ps.map (·.1))) = true) := by
      intro ρ1 σ1 h1 hr'
      have ih := writeBack_move Γ ρ1 σ1 args ps kvss after h1 hr' ha' hla' hlk' hle'
      generalize writeBack Γ ρ1 args ps kvss after = r at ih ⊢
      cases r with
      | ok ρ' => exact ih
      | error s => exact ⟨ih.1, fun r hs => by simp [ih.2 r hs]⟩
    simp only [writeBack, giveArgsM, argsExit, List.map_cons]
    by_cases hm' : mode = .mut ∨ mode = .out
    · have hm : (mode == .mut || mode == .out) = true := by simpa [Bool.or_eq_true, beq_iff_eq] using hm'
      have ha1 : a.isSome = true := ha (a, (mode, t)) List.mem_cons_self hm'
      simp only [hm, ↓reduceIte, Bool.true_and]
      cases hp : placeOf e with
      | none =>
        simp only [Bool.false_or]
        have := writeBack_move Γ ρ σ args ps kvss after hρ hr2 ha' hla' hlk' hle'
        generalize writeBack Γ ρ args ps kvss after = r at this ⊢
        cases r with
        | ok ρ' => exact this
        | error s => exact this
      | some p =>
        obtain ⟨x, st⟩ := p
        cases a with
        | none => simp at ha1
        | some v =>
          cases st with
          | nil =>
            simp only
            have := rest _ _ (MAg.give x v hρ) (roots_set hr2)
            generalize writeBack Γ (setVar x v ρ) args ps kvss after = r at this ⊢
            cases r with
            | ok ρ' => exact this
            | error s => exact ⟨this.1, fun r hs => by simpa [hp, stepsExit] using this.2 r hs⟩
          | cons s0 st' =>
            simp only
            cases hl : Γ.lookup x with
            | none => simp [CalmStop, throw, throwThe, MonadExceptOf.throw]
            | some uk =>
              obtain ⟨u, k⟩ := uk
              cases hl' : ρ.lookup x with
              | none => simp [CalmStop, throw, throwThe, MonadExceptOf.throw]
              | some old =>
                simp only
                have hold : old ≠ none := by
                  intro e0; subst e0
                  exact hr1 hm x (s0 :: st') hp (by simp) hl'
                obtain ⟨ov, rfl⟩ := Option.ne_none_iff_exists'.mp hold
                have hc := writeSteps_calm none u ov (s0 :: st') kvs v
                cases hws : writeSteps none u (some ov) (s0 :: st') kvs v with
                | error s =>
                  simp only [bind, Except.bind]
                  refine ⟨hc s hws, fun r hs => ?_⟩
                  subst hs
                  simp [writeSteps_early _ _ _ _ _ _ hws]
                | ok w =>
                  simp only [bind, Except.bind]
                  have := rest _ _ (MAg.set x w hρ) (roots_set hr2)
                  generalize writeBack Γ (setVar x w ρ) args ps kvss after = r at this ⊢
                  cases r with
                  | ok ρ' => exact this
                  | error s => exact ⟨this.1, fun r hs => by simpa [hm', hp] using this.2 r hs⟩
    · have hm : (mode == .mut || mode == .out) = false := by simpa [Bool.or_eq_true, beq_iff_eq] using hm'
      simp only [hm, Bool.false_and, Bool.false_or, Bool.false_eq_true, ↓reduceIte]
      exact writeBack_move Γ ρ σ args ps kvss after hρ hr2 ha' hla' hlk' hle'
  | ρ, σ, [], ps, kvss, after, hρ, _, _, _, _, hle => by
    have : ps = [] := by simpa using hle
    subst this
    simp only [writeBack, giveArgsM]
    exact hρ
  | ρ, σ, _ :: _, [], kvss, after, hρ, _, _, _, _, _ => by
    simp only [writeBack, giveArgsM, List.map_nil]
    exact hρ
  | ρ, σ, _ :: _, _ :: _, [], after, _, _, _, _, hlk, _ => by simp at hlk
  | ρ, σ, _ :: _, _ :: _, _ :: _, [], _, _, _, hla, _, _ => by simp at hla
where
  /-- Setting a variable keeps every root holding a value. -/
  roots_set {ρ : VEnv} {x : String} {v : Val} : ∀ {args : List (Option Mode × Expr)} {ps : List (Mode × Ty)},
      Roots ρ args ps → Roots (setVar x v ρ) args ps
    | _ :: _, _ :: _, ⟨h1, h2⟩ => ⟨fun hm y st hp hst => lookup_set (h1 hm y st hp hst), roots_set h2⟩
    | [], _, _ => trivial
    | _ :: _, [], _ => trivial
  lookup_set {ρ : VEnv} {x y : String} {v : Val} (h : ρ.lookup y ≠ some none) :
      (setVar x v ρ).lookup y ≠ some none := by
    induction ρ with
    | nil => simp [setVar]
    | cons p ρ ih =>
      obtain ⟨z, o⟩ := p
      by_cases hzx : z = x
      · subst hzx
        simp only [setVar, beq_self_eq_true, ↓reduceIte]
        by_cases hyz : y = z
        · subst hyz; simp [List.lookup]
        · simp only [List.lookup, show (y == z) = false by simpa using hyz] at h ⊢; exact h
      · simp only [setVar, show (z == x) = false by simpa using hzx, Bool.false_eq_true, ↓reduceIte]
        by_cases hyz : y = z
        · subst hyz; simp only [List.lookup, beq_self_eq_true] at h ⊢; exact h
        · simp only [List.lookup, show (y == z) = false by simpa using hyz] at h ⊢; exact ih h

theorem mSteps_cons (sigs : List (String × Sig)) (Γa : Ctx) (σ : MSt) (st : Step) (rest : List Step) :
    mSteps sigs Γa σ (st :: rest) = (mSteps sigs Γa σ [st] >>= fun σ1 => mSteps sigs Γa σ1 rest) := by
  cases st with
  | field s => simp [mSteps]
  | index k m => cases m <;> simp [mSteps]

/-- An argument read by value, for any parameter but an `out` one: a value. -/
theorem argByValue_some' {O : List (String × Bool)} {σ σ1 : MSt} {mode : Mode} {v : EvalT Val} (ho : mode ≠ .out)
    (hv : MT O σ σ1 (fun _ _ => True) v) (hs : σ.Le σ1) :
    MT O σ σ1 (fun a _ => a.1.isSome = true) (argByValue mode v) := by
  unfold argByValue
  simp only [show (mode == .out) = false by simpa using ho, Bool.false_eq_true, ↓reduceIte]
  exact MT.bind' (MT.map' (Q := fun o _ => o.isSome = true) hv (fun _ _ _ => rfl)) hs
    (fun o ρ1 hos hfr1 hρ1 => (MT.pure' (fun ρ hρ => ⟨hρ, hos⟩)) hfr1 ρ1 hρ1)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallMove sigs call)
  (O : List (String × Bool))
include hcall
set_option linter.unusedSectionVars false

mutual

theorem evalS_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (σ σ' : MSt) (st st' : DSt), SigsIn sigs Γ.sigs →
    CtxLe Γa Γ → (mE sigs Γa Γ.lends σ e).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalS env call n Γ e)
  | 0, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalS]; exact MT.fuel
  | n + 1, Γ, Γa, e, σ, σ', st, st', hs, hv, h, hf => by
    have h0 := h
    unfold mE at h
    replace h := wrap_run h
    obtain ⟨st2, h, hf2⟩ := h
    have hf2' : st2.movedHeld = false := hf2.trans hf
    cases e
    case hinted hh inner =>
      dsimp only at h
      simp only [evalS]
      split
      · simp only [pure_bind]
        exact evalForm_move n _ Γa inner _ σ σ' st st2 hs hv (fun hb => by simp [Ctx.lends_enter, buildsFromParts, hb]) h hf2'
      · exact MT.wrong
    case var x =>
      dsimp only at h
      simp only [evalS]
      refine MT.get' (fun ρ hfr hρ => ?_)
      -- What the analysis read, and whether it moved.
      have hread : (∀ v, σ.find x = some v → (v.init && v.live) = true) ∧ (σ' = σ ∨ σ' = σ.take x) ∧
          ((Γ.enter (.var x)).takes x = true → σ' = σ.take x) := by
        rcases MoveM.ite_run h with ⟨hl, h⟩ | ⟨hl, h⟩
        · obtain ⟨rfl, -, hr⟩ := readM_run h
          refine ⟨hr, .inl rfl, fun ht => ?_⟩
          unfold Ctx.takes at ht
          simp [Ctx.lends_enter, buildsFromParts, hl] at ht
        · obtain ⟨hr, hor, htake⟩ := moveM_run h
          refine ⟨hr, hor, fun ht => ?_⟩
          unfold Ctx.takes at ht
          simp only [Ctx.lends_enter, buildsFromParts, Bool.not_false, Bool.and_true, hl,
            Bool.true_and, Ctx.lookup_enter] at ht
          split at ht
          · rename_i t k hl'
            simp only [Bool.and_eq_true, Bool.not_eq_true', bne_iff_ne, ne_eq] at ht
            -- The analysis looked `x` up in `Γa`, where it is copy only if it is in `Γ`.
            obtain ⟨t', hl'', hc⟩ := CtxLe.lookup_some hv hl'
            have hc' : t'.isCopy = false := by
              cases e : t'.isCopy
              · rfl
              · rw [hc e] at ht; exact absurd ht.1 (by decide)
            exact htake t' k hl'' ht.2 hc'
          · cases ht
      obtain ⟨hr, hor, htake⟩ := hread
      have hl := MAg.lookup hρ hr
      split
      · rename_i v hv'
        by_cases hc : (env.moves && (Γ.enter (.var x)).takes x) = true
        · simp only [hc, ↓reduceIte]
          simp only [Bool.and_eq_true] at hc
          rw [htake hc.2]
          exact ⟨MAg.take x hρ, trivial⟩
        · simp only [hc, Bool.false_eq_true, ↓reduceIte]
          refine ⟨?_, trivial⟩
          rcases hor with rfl | rfl
          · exact hρ
          · exact MAg.take' x hρ
      · rename_i hn; exact absurd hn hl
      · trivial
    case binop op a b =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨σ2, st3, h2, h⟩ := h
      have hs1 := (mE_estep sigs Γa _ σ a st σ1 st1 h1).1
      have hs2 := (mE_estep sigs Γa _ σ1 b st1 σ2 st3 h2).1
      have hex : σ2 = σ' ∧ (op.exits = true → σ2.outsSet = true) ∧ st3.movedHeld = st2.movedHeld := by
        rcases MoveM.ite_run h with ⟨hx, h⟩ | ⟨hx, h⟩
        · obtain ⟨rfl, rfl, ho⟩ := exitM_run h
          exact ⟨rfl, fun _ => ho, rfl⟩
        · cases h
          exact ⟨rfl, fun e => (by rw [hx] at e; cases e), rfl⟩
      obtain ⟨rfl, ho, hf3⟩ := hex
      have hf3' : st3.movedHeld = false := hf3.trans hf2'
      have hf1' : st1.movedHeld = false := flag_before (mE_estep sigs Γa _ σ1 b st1 σ2 st3 h2).2 hf3'
      simp only [evalS]
      refine MT.bind (evalPair_move n _ Γa a b none σ σ1 σ2 st st1 st3 hs hv h1 hf1' h2 hf3') (hs1.trans hs2) (fun r => ?_)
      cases hx : op.exits
      · exact MT.calm (binOp_calm _ _ _ _) (binOp_noEarly hx _ _ _)
      · exact MT.here (binOp_calm _ _ _ _) (fun _ _ => ho hx) (fun _ _ _ _ => trivial)
    case unop op a =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      have hs1 := (mE_estep sigs Γa _ σ a st σ1 st1 h1).1
      have hex : σ1 = σ' ∧ ((op = .negC ∨ op = .negO) → σ1.outsSet = true) ∧ st1.movedHeld = st2.movedHeld := by
        split at h
        · obtain ⟨rfl, rfl, ho⟩ := exitM_run h; exact ⟨rfl, fun _ => ho, rfl⟩
        · obtain ⟨rfl, rfl, ho⟩ := exitM_run h; exact ⟨rfl, fun _ => ho, rfl⟩
        · cases h; exact ⟨rfl, fun e => (by rcases e with e | e <;> cases e <;> simp_all), rfl⟩
      obtain ⟨rfl, ho, hf1⟩ := hex
      have hf1' : st1.movedHeld = false := hf1.trans hf2'
      simp only [evalS]
      refine MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun t => ?_)
      refine MT.bind (evalS_move n _ Γa a σ σ1 st st1 hs hv h1 hf1') hs1 (fun v => ?_)
      cases op
      case negC | negO =>
        exact MT.here (unOp_calm _ _ _) (fun _ _ => ho (by simp)) (fun _ _ _ _ => trivial)
      all_goals
        refine MT.calm (unOp_calm _ _ _) (unOp_noEarly ?_ _ _)
        simp
    case tryOpt a =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨rfl, rfl, ho⟩ := exitM_run h
      have hs1 := (mE_estep sigs Γa _ σ a st σ1 st1 h1).1
      simp only [evalS]
      refine MT.bind (evalS_move n _ Γa a σ σ1 st st1 hs hv (by simpa [Ctx.lends_enter, buildsFromParts] using h1) hf2')
        hs1 (fun v => ?_)
      split
      · exact MT.earlyHere ho
      · exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
      · exact MT.wrong
    case tryRes a =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨rfl, rfl, ho⟩ := exitM_run h
      have hs1 := (mE_estep sigs Γa _ σ a st σ1 st1 h1).1
      simp only [evalS]
      refine MT.bind (evalS_move n _ Γa a σ σ1 st st1 hs hv (by simpa [Ctx.lends_enter, buildsFromParts] using h1) hf2')
        hs1 (fun v => ?_)
      split
      · exact MT.earlyHere ho
      · exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
      · exact MT.wrong
    case adapt a =>
      dsimp only at h
      simp only [evalS]
      exact evalS_move n _ Γa a σ σ' st st2 hs hv h hf2'
    case call f args =>
      simp only [evalS]
      exact evalCall_move n _ Γa f args none σ σ' st st' hs hv h0 hf
    case place x steps =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, hst⟩ := h
      obtain ⟨rfl, rfl, hx⟩ := readM_run h1
      simp only [evalS]
      refine MT.get' (fun ρ hfr hρ => ?_)
      have hl := MAg.lookup hρ hx
      split
      · exact evalSteps_move n (Γ.enter _) Γa _ _ steps _ _ st st2 hs hv hst hf2' hfr ρ hρ
      · rename_i hn; exact absurd hn hl
      · trivial
    case proj b sel =>
      dsimp only at h
      simp only [evalS]
      have hb : (mE sigs Γa ((Γ.enter (.proj b sel)).under b).lends σ b).run st = .ok (σ', st2) := by
        simpa [Ctx.lends_under, Ctx.lends_enter, buildsFromParts] using h
      exact MT.bind (evalS_move n _ Γa b σ σ' st st2 (hs.of_eq (sigs_under _ b)) (hv.of_vars_eq (Ctx.vars_under _ b)) hb hf2')
        (mE_estep sigs Γa _ σ b st σ' st2 h).1 (fun v => MT.calm (fieldVal_calm _ _) (fieldVal_noEarly _ _))
    case index b k mode =>
      cases mode with
      | none => simp only [evalS]; exact MT.wrong
      | some m =>
        dsimp only at h
        replace h := MoveM.bind_ok h
        obtain ⟨σ1, st1, h1, h⟩ := h
        replace h := MoveM.bind_ok h
        obtain ⟨σ2, st3, h2, h⟩ := h
        obtain ⟨rfl, rfl, ho⟩ := exitM_run h
        have hs1 := (mE_estep sigs Γa _ σ b st σ1 st1 h1).1
        have hs2 := (mE_estep sigs Γa _ σ1 k st1 σ2 st3 h2).1
        have hf1' : st1.movedHeld = false := flag_before (mE_estep sigs Γa _ σ1 k st1 σ2 st3 h2).2 hf2'
        have hb : (mE sigs Γa ((Γ.enter (.index b k (some m))).under b).lends σ b).run st = .ok (σ1, st1) := by
          simpa [Ctx.lends_under, Ctx.lends_enter, buildsFromParts] using h1
        simp only [evalS]
        refine MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun bt => ?_)
        split
        · simp only [pure_bind]
          exact MT.bind (evalS_move n _ Γa b σ σ1 st st1 (hs.of_eq (sigs_under _ b)) (hv.of_vars_eq (Ctx.vars_under _ b)) hb hf1') hs1 (fun v =>
            MT.bind (evalC_move n (Γ.enter _).key Γa k _ σ1 σ2 st1 st3 hs hv h2 hf2') hs2 (fun kv =>
              MT.here (indexVal_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial)))
        · exact MT.wrong
    all_goals
      simp only [evalS]
      refine MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun t => ?_)
      refine evalForm_move n _ Γa _ t σ σ' st st' hs hv (fun hb => by simp [Ctx.lends_enter, hb]) ?_ hf
      rw [mE_enter] at h0
      exact h0

theorem evalC_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (t : Ty) (σ σ' : MSt) (st st' : DSt), SigsIn sigs Γ.sigs →
    CtxLe Γa Γ → (mE sigs Γa Γ.lends σ e).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalC env call n Γ e t)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalC]; exact MT.fuel
  | n + 1, Γ, Γa, e, t, σ, σ', st, st', hs, hv, h, hf => by
    cases e
    case hinted hh inner =>
      unfold mE at h
      replace h := wrap_run h
      obtain ⟨st2, h, hf2⟩ := h
      dsimp only at h
      simp only [evalC]
      exact evalForm_move n _ Γa inner t σ σ' st st2 hs hv (fun hb => by simp [Ctx.lends_enter, buildsFromParts, hb]) h (hf2.trans hf)
    all_goals
      simp only [evalC]
      refine evalForm_move n _ Γa _ t σ σ' st st' hs hv (fun hb => by simp [Ctx.lends_enter, hb]) ?_ hf
      rw [mE_enter] at h
      exact h

theorem evalForm_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (t : Ty) (σ σ' : MSt) (st st' : DSt), SigsIn sigs Γ.sigs →
    CtxLe Γa Γ → (buildsFromParts e = true → Γ.lends = false) →
    (mE sigs Γa Γ.lends σ e).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalForm env call n Γ e t)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalForm]; exact MT.fuel
  | n + 1, Γ, Γa, e, t, σ, σ', st, st', hs, hv, hbl, h, hf => by
    have h0 := h
    have lit : ∀ {α} (a : α), σ' = σ → MT O σ σ' (fun _ _ => True) (Pure.pure a : EvalT α) :=
      fun a e => MT.pure' (fun ρ hρ => ⟨e ▸ hρ, trivial⟩)
    unfold mE at h
    replace h := wrap_run h
    obtain ⟨st2, h, hf2⟩ := h
    have hf2' : st2.movedHeld = false := hf2.trans hf
    cases e
    case none_ | tt | ff | str | int | float | hex | atom =>
      dsimp only at h
      cases h
      simp only [evalForm]
      exact lit _ rfl
    case er p | some_ p | ok p =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalC_move n _ Γa p _ σ σ' st st2 hs hv (by rw [hbl rfl]; exact h) hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case tuple es =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalPairs_move n _ Γa es _ σ σ' st st2 hs hv (hbl rfl) h hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case struct fs =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalFields_move n _ Γa fs _ σ σ' st st2 hs hv (hbl rfl) h hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case list es | set es =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalAll_move n _ Γa es _ σ σ' st st2 hs hv (hbl rfl) h hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case tensor shape es =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalAll_move n _ Γa es _ σ σ' st st2 hs hv (hbl rfl) h hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case map es =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalEntries_move n _ Γa es _ _ σ σ' st st2 hs hv (hbl rfl) h hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case table header rows =>
      dsimp only at h
      simp only [evalForm]
      split
      · exact MT.map' (evalRows_move n _ Γa rows _ σ σ' st st2 hs hv (hbl rfl) h hf2') (fun _ _ _ => trivial)
      · exact MT.wrong
    case term a p =>
      dsimp only at h
      simp only [evalForm]
      repeat' split
      all_goals first
        | exact MT.map' (evalC_move n _ Γa p _ σ σ' st st2 hs hv (by rw [hbl rfl]; exact h) hf2') (fun _ _ _ => trivial)
        | exact MT.wrong
    case enum inner =>
      dsimp only at h
      simp only [evalForm]
      exact evalC_move n _ Γa inner t σ σ' st st2 hs hv h hf2'
    case data p | error p =>
      dsimp only at h
      simp only [evalForm]
      exact MT.bind (evalS_move n _ Γa p σ σ' st st2 hs hv (by rw [hbl rfl]; exact h) hf2')
        (mE_estep sigs Γa _ σ p st σ' st2 h).1 (fun _ => MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))
    case unop op a =>
      cases op
      case neg =>
        dsimp only at h
        replace h := MoveM.bind_ok h
        obtain ⟨σ1, st1, h1, h⟩ := h
        obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
        have hs1 := (mE_estep sigs Γa _ σ a st σ1 st1 h1).1
        simp only [evalForm]
        have viaC : MT O σ σ1 (fun _ _ => True) (do
            Fun.here (unOp .neg t.expose (← evalC env call n Γ.borrow a t.expose))) :=
          MT.bind (evalC_move n _ Γa a _ σ σ1 st st1 hs hv h1 hf2') hs1 (fun v =>
            MT.calm (unOp_calm _ _ _) (unOp_noEarly (op := .neg) (by simp) _ _))
        split
        · rename_i k
          split
          · -- The analysis walked the literal, which gives `σ1 = σ`.
            unfold mE at h1
            replace h1 := wrap_run h1
            obtain ⟨_, h1, -⟩ := h1
            dsimp only at h1
            cases h1
            exact lit _ rfl
          · split
            · exact viaC
            · exact evalSub_move n _ Γa _ σ σ1 st st' hs hv h0 hf
        · split
          · exact viaC
          · exact evalSub_move n _ Γa _ σ σ1 st st' hs hv h0 hf
      all_goals simp only [evalForm]; exact evalSub_move n _ Γa _ σ σ' st st' hs hv h0 hf
    case binop op a c =>
      simp only [evalForm]
      split
      · dsimp only at h
        replace h := MoveM.bind_ok h
        obtain ⟨σ1, st1, h1, h⟩ := h
        replace h := MoveM.bind_ok h
        obtain ⟨σ2, st3, h2, h⟩ := h
        have hs1 := (mE_estep sigs Γa _ σ a st σ1 st1 h1).1
        have hs2 := (mE_estep sigs Γa _ σ1 c st1 σ2 st3 h2).1
        have hex : σ2 = σ' ∧ (op.exits = true → σ2.outsSet = true) ∧ st3.movedHeld = st2.movedHeld := by
          rcases MoveM.ite_run h with ⟨hx, h⟩ | ⟨hx, h⟩
          · obtain ⟨rfl, rfl, ho⟩ := exitM_run h
            exact ⟨rfl, fun _ => ho, rfl⟩
          · cases h
            exact ⟨rfl, fun e => (by rw [hx] at e; cases e), rfl⟩
        obtain ⟨rfl, ho, hf3⟩ := hex
        have hf3' : st3.movedHeld = false := hf3.trans hf2'
        have hf1' : st1.movedHeld = false := flag_before (mE_estep sigs Γa _ σ1 c st1 σ2 st3 h2).2 hf3'
        refine MT.bind (evalPair_move n _ Γa a c _ σ σ1 σ2 st st1 st3 hs hv h1 hf1' h2 hf3') (hs1.trans hs2) (fun r => ?_)
        cases hx : op.exits
        · exact MT.calm (binOp_calm _ _ _ _) (binOp_noEarly hx _ _ _)
        · exact MT.here (binOp_calm _ _ _ _) (fun _ _ => ho hx) (fun _ _ _ _ => trivial)
      · exact evalSub_move n _ Γa _ σ σ' st st' hs hv h0 hf
    case adapt a =>
      dsimp only at h
      simp only [evalForm]
      exact evalS_move n _ Γa a σ σ' st st2 hs hv h hf2'
    case call f args =>
      simp only [evalForm]
      exact evalCall_move n _ Γa f args _ σ σ' st st' hs hv h0 hf
    all_goals simp only [evalForm]; exact evalSub_move n _ Γa _ σ σ' st st' hs hv h0 hf

theorem evalSub_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (σ σ' : MSt) (st st' : DSt), SigsIn sigs Γ.sigs →
    CtxLe Γa Γ → (mE sigs Γa Γ.lends σ e).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalSub env call n Γ e)
  | 0, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalSub]; exact MT.fuel
  | n + 1, Γ, Γa, e, σ, σ', st, st', hs, hv, h, hf => by
    simp only [evalSub]; exact evalS_move n Γ Γa e σ σ' st st' hs hv h hf

theorem evalCall_move : ∀ (n : Nat) (Γ Γa : Ctx) (f : String) (args : List (Option Mode × Expr))
    (expected : Option Ty) (σ σ' : MSt) (st st' : DSt), SigsIn sigs Γ.sigs → CtxLe Γa Γ →
    (mE sigs Γa Γ.lends σ (.call f args)).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalCall env call n Γ f args expected)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalCall]; exact MT.fuel
  | n + 1, Γ, Γa, f, args, expected, σ, σ', st, st', hs, hv, h, hf => by
    unfold mE at h
    replace h := wrap_run h
    obtain ⟨st2, h, hf2⟩ := h
    have hf2' : st2.movedHeld = false := hf2.trans hf
    dsimp only at h
    cases hsig : sigs.lookup f with
    | none => rw [hsig] at h; dsimp only at h; cases h
    | some sig =>
      rw [hsig] at h
      dsimp only at h
      simp only [evalCall, hs f sig hsig]
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st3, hnote, h⟩ := h
      have hs1 := (mArgs_estep sigs Γa σ args _ st σ1 st1 h1).1
      have hex : σ' = giveArgsM σ1 args (sig.params.map (·.1)) ∧
          (argsExit args (sig.params.map (·.1)) = true → σ1.outsSet = true) ∧ st2.movedHeld = st3.movedHeld := by
        replace h := MoveM.bind_ok h
        obtain ⟨σ2, st4, hA, hB⟩ := h
        rcases MoveM.ite_run hA with ⟨hx, hA⟩ | ⟨hx, hA⟩
        · obtain ⟨e1, e2, ho⟩ := exitM_run hA
          cases hB
          exact ⟨by rw [e1], fun _ => ho, by rw [e2]⟩
        · cases hA; cases hB
          exact ⟨rfl, fun e => (by rw [hx] at e; cases e), rfl⟩
      obtain ⟨rfl, hex, hf3⟩ := hex
      have hf3' : st3.movedHeld = false := hf3.symm.trans hf2'
      obtain ⟨hheld, hf1'⟩ := noteHeldMoved_run hnote hf3'
      have hlive := heldMoved_false σ1 args _ hheld
      cases hd : decided (env.call Γ f args expected) with
      | error c => exact MT.wrong
      | ok θ =>
        by_cases hg : groundArgs θ = true
        · simp only [show (!groundArgs θ) = false by simp [hg], Bool.false_eq_true, ↓reduceIte]
          split
          · exact MT.wrong
          rename_i hchk
          have hchk' : (boundsHold Γ sig.tparams θ && allDecided sig.tparams θ) = true := by simpa using hchk
          have hadm := admitted_of_check hg (Bool.and_eq_true _ _ |>.mp hchk').1 (Bool.and_eq_true _ _ |>.mp hchk').2
          have hms : (sig.params.map fun (m, t) => (m, t.inst θ)).map (·.1) = sig.params.map (·.1) := by
            simp [List.map_map, Function.comp_def]
          refine MT.bind' (evalArgs_move n _ Γa args _ σ σ1 st st1 hs hv (hms ▸ h1) hf1') hs1 (fun vals ρ1 hval hfr1 hρ1 => ?_)
          obtain ⟨hlen, hvs, hroots⟩ := hval
          have hvals : ∀ q ∈ (vals.map (·.1)).zip sig.params, q.2.1 ≠ .out → q.1.isSome = true := by
            intro q hq hm
            have : (q.1, (q.2.1, q.2.2.inst θ)) ∈ (vals.map (·.1)).zip (sig.params.map fun (m, t) => (m, t.inst θ)) := by
              rw [List.zip_map_right]
              exact List.mem_map.mpr ⟨q, hq, rfl⟩
            exact hvs (q.1, (q.2.1, q.2.2.inst θ)) this hm
          have hc := hcall f sig θ (vals.map (·.1)) hsig hg hadm hvals
          have hcalm : Calm (call f θ (vals.map (·.1))) := by
            intro s hs'
            rw [hs'] at hc
            exact ⟨hc.1, hc.2.2.1, hc.2.2.2.1, hc.2.2.2.2⟩
          have hne : ∀ r, call f θ (vals.map (·.1)) = .error (.early r) → σ1.outsSet = true := by
            intro r hr; rw [hr] at hc; exact absurd rfl (hc.2.1 r)
          refine MT.res_bind (MT.res_here hcalm hne hfr1 hρ1) (fun a ρ2 ⟨ha, e2⟩ hρ2 => ?_)
          subst e2
          obtain ⟨r, after⟩ := a
          rw [ha] at hc
          obtain ⟨hvl, hal, haft⟩ := hc
          simp only
          refine MT.res_get ?_
          have hps : (sig.params.map fun (m, t) => (m, t.inst θ)).length = sig.params.length := by simp
          have hle : (sig.params.map fun (m, t) => (m, t.inst θ)).length ≤ args.length := by
            rw [List.length_map] at hlen
            rw [hps]; simp at hvl; omega
          have hroots' : Roots ρ2 args (sig.params.map fun (m, t) => (m, t.inst θ)) :=
            roots_of hρ1 (hms ▸ hroots) (hms ▸ hlive)
          have hw := writeBack_move Γ ρ2 σ1 args (sig.params.map fun (m, t) => (m, t.inst θ))
            (vals.map (·.2)) after hρ1 hroots' (fun q hq hm => by
              rw [List.zip_map_right] at hq
              obtain ⟨q', hq', rfl⟩ := List.mem_map.mp hq
              exact haft q' hq' hm) (by rw [hps]; exact hal) (by rw [hps, List.length_map]; simpa using hvl) hle
          rw [hms] at hw
          have hwc : Calm (writeBack Γ ρ2 args (sig.params.map fun (m, t) => (m, t.inst θ)) (vals.map (·.2)) after) := by
            intro s hs'
            rw [hs'] at hw
            exact hw.1
          have hwe : ∀ r, writeBack Γ ρ2 args (sig.params.map fun (m, t) => (m, t.inst θ)) (vals.map (·.2)) after
              = .error (.early r) → σ1.outsSet = true := by
            intro r hr; rw [hr] at hw; exact hex (hw.2 r rfl)
          refine MT.res_bind (MT.res_here hwc hwe hfr1 hρ1) (fun ρ3 ρ4 ⟨hw3, e4⟩ hρ4 => ?_)
          subst e4
          rw [hw3] at hw
          exact MT.res_set_pure hw trivial
        · simp only [show (!groundArgs θ) = true by simpa using hg, ↓reduceIte]
          exact MT.unsupported

theorem evalArgs_move : ∀ (n : Nat) (Γ Γa : Ctx) (args : List (Option Mode × Expr)) (ps : List (Mode × Ty))
    (σ σ' : MSt) (st st' : DSt), SigsIn sigs Γ.sigs → CtxLe Γa Γ →
    (mArgs sigs Γa σ args (ps.map (·.1))).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun vs _ => MArgs σ' args ps vs) (evalArgs env call n Γ args ps)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalArgs]; exact MT.fuel
  | n + 1, Γ, Γa, (mk, e) :: args, (mode, t) :: ps, σ, σ', st, st', hs, hv, h, hf => by
    simp only [List.map_cons] at h
    unfold mArgs at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h2⟩ := h
    have hs2 := (mArgs_estep sigs Γa σ1 args _ st1 σ' st' h2).1
    have hf1' : st1.movedHeld = false := flag_before (mArgs_estep sigs Γa σ1 args _ st1 σ' st' h2).2 hf
    rw [evalArgs.eq_def]; dsimp only
    have tail : ∀ (x : EvalT (Option Val × List Val)), σ.Le σ1 →
        MT O σ σ1 (fun a _ => (mode ≠ .out → a.1.isSome = true) ∧ ((mode == .mut || mode == .out) = true →
          ∀ y st, placeOf e = some (y, st) → st ≠ [] → ∀ v, σ1.find y = some v → v.init = true)) x →
        MT O σ σ' (fun vs _ => MArgs σ' ((mk, e) :: args) ((mode, t) :: ps) vs)
          (x >>= fun a => evalArgs env call n Γ args ps >>= fun vs => pure (a :: vs)) := by
      intro x hs1 hx
      refine MT.bind' hx hs1 (fun a ρa ha hfr1 hρa => ?_)
      refine MT.res_bind (evalArgs_move n Γ Γa args ps σ1 σ' st1 st' hs hv h2 hf hfr1 ρa hρa) (fun vs ρ' hval hρ' => ?_)
      obtain ⟨hl, hvs, hroots⟩ := hval
      refine ⟨hρ', by simp [hl, Nat.succ_min_succ], fun q hq hm => ?_,
        ⟨fun hm y st hp hst w hw => ?_, hroots⟩⟩
      · simp only [List.map_cons, List.zip_cons_cons, List.mem_cons] at hq
        rcases hq with rfl | hq
        · exact ha.1 hm
        · exact hvs q hq hm
      · obtain ⟨v, hv', hi⟩ := hs2.find_back hw
        exact hi (ha.2 hm y st hp hst v hv')
    cases hp : placeOf e with
    | some pl =>
      rw [hp] at h1
      simp only at h1 ⊢
      by_cases hm : (mode == .mut || mode == .out) = true
      · simp only [hm, ↓reduceIte] at h1 ⊢
        replace h1 := MoveM.bind_ok h1
        obtain ⟨_, stA, hA, h1⟩ := h1
        replace h1 := MoveM.bind_ok h1
        obtain ⟨_, stB, hB, h1⟩ := h1
        have hsP := (mPlace_estep sigs Γa σ e _ stB σ1 st1 h1).1
        have hfB : stB.movedHeld = false := flag_before (mPlace_estep sigs Γa σ e _ stB σ1 st1 h1).2 hf1'
        refine tail _ hsP ?_
        unfold placeArg
        refine MT.bind' (placeC_move n _ Γa e t σ σ1 stB st1 (mode == .mut) hs (hv.of_vars_eq (Ctx.vars_forArg _ _)) h1 hf1') hsP
          (fun r ρa hpl hfr1 hρa => ?_)
        obtain ⟨o, kvs⟩ := r
        have root : (mode == .mut || mode == .out) = true →
            ∀ y st, placeOf e = some (y, st) → st ≠ [] → ∀ v, σ1.find y = some v → v.init = true :=
          fun _ y st hp' hst v hv' => hpl.2 y st hp' (.inr hst) v hv'
        by_cases ho : mode = .out
        · subst ho
          exact ⟨hρa, fun h => absurd rfl h, root⟩
        · simp only [show (mode == .out) = false by simpa using ho, Bool.false_eq_true, ↓reduceIte]
          have hmut : (mode == .mut) = true := by
            simp only [Bool.or_eq_true, beq_iff_eq] at hm
            rcases hm with hm | hm
            · simp [hm]
            · exact absurd hm ho
          cases o with
          | none => exact absurd (hpl.1 hmut) (by simp)
          | some v => exact ⟨hρa, fun _ => rfl, fun _ => root hm⟩
      · simp only [hm, Bool.false_eq_true, ↓reduceIte] at h1 ⊢
        have ho : mode ≠ .out := by intro e'; subst e'; simp at hm
        have hsE := (mE_estep sigs Γa _ σ e st σ1 st1 h1).1
        refine tail _ hsE ?_
        have hE : (mE sigs Γa (Γ.forArg mode).lends σ e).run st = .ok (σ1, st1) := by
          simpa [Ctx.lends_forArg] using h1
        exact MT.weaken (argByValue_some' ho (evalC_move n _ Γa e t σ σ1 st st1 hs (hv.of_vars_eq (Ctx.vars_forArg _ _)) hE hf1') hsE)
          (fun a ρ ha => ⟨fun _ => ha, fun hm' => absurd hm' (by simpa using hm)⟩)
    | none =>
      rw [hp] at h1
      simp only at h1 ⊢
      by_cases hm : (mode == .mut || mode == .out) = true
      · simp only [hm, ↓reduceIte] at h1; cases h1
      · simp only [hm, Bool.false_eq_true, ↓reduceIte] at h1
        have ho : mode ≠ .out := by intro e'; subst e'; simp at hm
        have hsE := (mE_estep sigs Γa _ σ e st σ1 st1 h1).1
        refine tail _ hsE ?_
        have hE : (mE sigs Γa (Γ.forArg mode).lends σ e).run st = .ok (σ1, st1) := by
          simpa [Ctx.lends_forArg] using h1
        exact MT.weaken (argByValue_some' ho (evalC_move n _ Γa e t σ σ1 st st1 hs (hv.of_vars_eq (Ctx.vars_forArg _ _)) hE hf1') hsE)
          (fun a ρ ha => ⟨fun _ => ha, fun _ y st hp' => by rw [hp] at hp'; cases hp'⟩)
  | n + 1, Γ, Γa, [], ps, σ, σ', st, st', hs, hv, h, hf => by
    unfold mArgs at h; cases h
    simp only [evalArgs]
    exact MT.pure' (fun ρ hρ => ⟨hρ, by simp, by simp, trivial⟩)
  | n + 1, Γ, Γa, _ :: _, [], σ, σ', st, st', hs, hv, h, hf => by
    simp only [List.map_nil] at h
    unfold mArgs at h; cases h
    simp only [evalArgs]
    exact MT.pure' (fun ρ hρ => ⟨hρ, by simp, by simp, trivial⟩)

theorem placeC_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (t : Ty) (σ σ' : MSt) (st st' : DSt) (need : Bool),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mPlace sigs Γa σ e need).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun r _ => MPlace σ' need e r.1) (placeC env call n Γ e t)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [placeC]; exact MT.fuel
  | n + 1, Γ, Γa, e, t, σ, σ', st, st', need, hs, hv, h, hf => by
    cases e
    case hinted hh inner =>
      unfold mPlace at h
      replace h := wrap_run h
      obtain ⟨st2, h, hf2⟩ := h
      dsimp only at h
      simp only [placeC]
      exact MT.weaken (placeForm_move n _ Γa inner t σ σ' st st2 need hs hv h (hf2.trans hf)) (fun r ρ hr => by
        simpa [MPlace, placeOf] using hr)
    all_goals simp only [placeC]; exact placeForm_move n _ Γa _ t σ σ' st st' need hs hv h hf

theorem placeForm_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (t : Ty) (σ σ' : MSt) (st st' : DSt) (need : Bool),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mPlace sigs Γa σ e need).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun r _ => MPlace σ' need e r.1) (placeForm env call n Γ e t)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [placeForm]; exact MT.fuel
  | n + 1, Γ, Γa, e, t, σ, σ', st, st', need, hs, hv, h, hf => by
    simp only [placeForm]; exact placeSub_move n Γ Γa e σ σ' st st' need hs hv h hf

theorem placeSub_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (σ σ' : MSt) (st st' : DSt) (need : Bool),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mPlace sigs Γa σ e need).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun r _ => MPlace σ' need e r.1) (placeSub env call n Γ e)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [placeSub]; exact MT.fuel
  | n + 1, Γ, Γa, e, σ, σ', st, st', need, hs, hv, h, hf => by
    simp only [placeSub]; exact placeS_move n Γ Γa e σ σ' st st' need hs hv h hf

theorem placeS_move : ∀ (n : Nat) (Γ Γa : Ctx) (e : Expr) (σ σ' : MSt) (st st' : DSt) (need : Bool),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mPlace sigs Γa σ e need).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun r _ => MPlace σ' need e r.1) (placeS env call n Γ e)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [placeS]; exact MT.fuel
  | n + 1, Γ, Γa, e, σ, σ', st, st', need, hs, hv, h, hf => by
    unfold mPlace at h
    replace h := wrap_run h
    obtain ⟨st2, h, hf2⟩ := h
    have hf2' : st2.movedHeld = false := hf2.trans hf
    cases e
    case var x =>
      dsimp only at h
      have hσ : σ = σ' ∧ (need = true → ∀ v, σ.find x = some v → (v.init && v.live) = true) := by
        split at h
        · obtain ⟨rfl, -, hx⟩ := readM_run h; exact ⟨rfl, fun _ => hx⟩
        · rename_i hn
          split at h
          · rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
            · cases h
            · cases h; exact ⟨rfl, fun e => absurd e hn⟩
          · cases h; exact ⟨rfl, fun e => absurd e hn⟩
      obtain ⟨rfl, hx⟩ := hσ
      simp only [placeS]
      refine MT.get' (fun ρ hfr hρ => ?_)
      split
      · rename_i o ho
        refine ⟨hρ, fun hn => ?_, fun y st hp hst => ?_⟩
        · have := MAg.lookup hρ (hx hn)
          rw [ho] at this
          cases o
          · exact absurd rfl this
          · rfl
        · simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
          obtain ⟨rfl, rfl⟩ := hp
          rcases hst with hn | hst
          · exact fun v hv' => by simpa using (Bool.and_eq_true _ _ |>.mp (hx hn v hv')).1
          · exact absurd rfl hst
      · trivial
    case place x steps =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, hst⟩ := h
      obtain ⟨rfl, rfl, hx⟩ := readM_run h1
      have hsS := (mSteps_estep sigs Γa σ steps st σ' st2 hst).1
      simp only [placeS]
      refine MT.get' (fun ρ hfr hρ => ?_)
      have hl := MAg.lookup hρ hx
      split
      · rename_i v u k hv' hu
        refine MT.res_bind (evalPath_move n (Γ.enter _) Γa u v steps _ _ st st2 hs hv hst hf2' hfr ρ hρ)
          (fun r ρ' _ hρ' => ⟨hρ', fun _ => rfl, fun y st hp _ w hw => ?_⟩)
        simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
        obtain ⟨rfl, rfl⟩ := hp
        obtain ⟨v, hv', hi⟩ := hsS.find_back hw
        exact hi (by simpa using (Bool.and_eq_true _ _ |>.mp (hx v hv')).1)
      · rename_i hn; exact absurd hn hl
      · trivial
    case hinted hh inner =>
      dsimp only at h
      simp only [placeS]
      split
      · simp only [pure_bind]
        exact MT.weaken (placeForm_move n _ Γa inner _ σ σ' st st2 need hs hv h hf2') (fun r ρ hr => by
          simpa [MPlace, placeOf] using hr)
      · exact MT.wrong
    case proj b sel =>
      dsimp only at h
      simp only [placeS]
      have hsP := (mPlace_estep sigs Γa σ b true st σ' st2 h).1
      refine MT.bind' (placeS_move n _ Γa b σ σ' st st2 true (hs.of_eq (sigs_under _ b)) (hv.of_vars_eq (Ctx.vars_under _ b)) h hf2') hsP
        (fun r ρa hpl hfr1 hρa => ?_)
      obtain ⟨o, kvs⟩ := r
      cases o with
      | none => exact absurd (hpl.1 rfl) (by simp)
      | some v =>
        simp only
        refine MT.res_bind (MT.res_here (fieldVal_calm v sel) (fun r hr => absurd hr (fieldVal_noEarly v sel r))
          hfr1 hρa) (fun w ρb ⟨_, eb⟩ hρb => ?_)
        subst eb
        refine ⟨hρb, fun _ => rfl, fun y st hp _ w hw => ?_⟩
        simp only [placeOf, Option.map_eq_some_iff, Prod.exists, Prod.mk.injEq] at hp
        obtain ⟨y', st', hb, rfl, rfl⟩ := hp
        exact hpl.2 y' st' hb (.inl rfl) w hw
    case index b k mode =>
      cases mode with
      | none => simp only [placeS]; exact MT.wrong
      | some m =>
        dsimp only at h
        replace h := MoveM.bind_ok h
        obtain ⟨σ1, st1, h1, h⟩ := h
        replace h := MoveM.bind_ok h
        obtain ⟨σ2, st3, h2, h⟩ := h
        obtain ⟨rfl, rfl, ho⟩ := exitM_run h
        have hs1 := (mPlace_estep sigs Γa σ b true st σ1 st1 h1).1
        have hs2 := (mE_estep sigs Γa _ σ1 k st1 σ2 st3 h2).1
        have hf1' : st1.movedHeld = false := flag_before (mE_estep sigs Γa _ σ1 k st1 σ2 st3 h2).2 hf2'
        simp only [placeS]
        refine MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun bt => ?_)
        split
        · simp only [pure_bind]
          refine MT.bind' (placeS_move n _ Γa b σ σ1 st st1 true (hs.of_eq (sigs_under _ b)) (hv.of_vars_eq (Ctx.vars_under _ b)) h1 hf1') hs1
            (fun r ρa hpl hfr1 hρa => ?_)
          obtain ⟨o, kvs⟩ := r
          refine MT.res_bind (evalC_move n (Γ.enter _).key Γa k _ σ1 σ2 st1 st3 hs hv h2 hf2' hfr1 ρa hρa)
            (fun kv ρb _ hρb => ?_)
          cases o with
          | none => exact absurd (hpl.1 rfl) (by simp)
          | some v =>
            simp only
            refine MT.res_bind (MT.res_here (indexVal_calm m v kv) (fun _ _ => ho) (hfr1.of_le hs2) hρb)
              (fun w ρc ⟨_, ec⟩ hρc => ?_)
            subst ec
            refine ⟨hρc, fun _ => rfl, fun y st hp _ w hw => ?_⟩
            simp only [placeOf, Option.map_eq_some_iff, Prod.exists, Prod.mk.injEq] at hp
            obtain ⟨y', st', hb, rfl, rfl⟩ := hp
            obtain ⟨v, hv', hi⟩ := hs2.find_back hw
            exact hi (hpl.2 y' st' hb (.inl rfl) v hv')
        · exact MT.wrong
    all_goals simp only [placeS]; exact MT.wrong

theorem evalPath_move : ∀ (n : Nat) (Γ Γa : Ctx) (t : Ty) (v : Val) (steps : List Step) (σ σ' : MSt) (st st' : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mSteps sigs Γa σ steps).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalPath env call n Γ t v steps)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalPath]; exact MT.fuel
  | _ + 1, _, _, _, v, [], σ, σ', _, _, _, _, h, _ => by
    unfold mSteps at h; cases h
    simp only [evalPath]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, t, v, [st], σ, σ', s0, s1, hs, hv, h, hf => by
    simp only [evalPath]
    exact evalPathStep_move n Γ Γa t v st σ σ' s0 s1 hs hv h hf
  | n + 1, Γ, Γa, t, v, st :: st2 :: rest, σ, σ', s0, s1, hs, hv, h, hf => by
    rw [mSteps_cons] at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, s2, h1, h⟩ := h
    have hs1 := (mSteps_estep sigs Γa σ [st] s0 σ1 s2 h1).1
    have hf1 : s2.movedHeld = false := flag_before (mSteps_estep sigs Γa σ1 (st2 :: rest) s2 σ' s1 h).2 hf
    simp only [evalPath]
    exact MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun u =>
      MT.bind (evalPathStep_move n _ Γa t v st σ σ1 s0 s2 hs hv h1 hf1) hs1 (fun p =>
        MT.bind (evalPath_move n Γ Γa u p.1 (st2 :: rest) σ1 σ' s2 s1 hs hv h hf) (mSteps_estep sigs Γa σ1 _ s2 σ' s1 h).1
          (fun _ => MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))))

theorem evalPathStep_move : ∀ (n : Nat) (Γ Γa : Ctx) (t : Ty) (v : Val) (st : Step) (σ σ' : MSt) (s0 s1 : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mSteps sigs Γa σ [st]).run s0 = .ok (σ', s1) → s1.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalPathStep env call n Γ t v st)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalPathStep]; exact MT.fuel
  | _ + 1, _, _, _, v, .field s, σ, σ', _, _, _, _, h, _ => by
    unfold mSteps at h; unfold mSteps at h; cases h
    simp only [evalPathStep]
    exact MT.bind (MT.calm (fieldVal_calm _ _) (fieldVal_noEarly _ _)) (MSt.Le.refl σ) (fun _ =>
      MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))
  | _ + 1, _, _, _, _, .index _ none, σ, σ', _, _, _, _, h, _ => by simp only [evalPathStep]; exact MT.wrong
  | n + 1, Γ, Γa, t, v, .index k (some m), σ, σ', s0, s1, hs, hv, h, hf => by
    unfold mSteps at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, s2, h1, h⟩ := h
    replace h := MoveM.bind_ok h
    obtain ⟨σ2, s3, h2, h⟩ := h
    unfold mSteps at h; cases h
    obtain ⟨rfl, rfl, ho⟩ := exitM_run h2
    have hs1 := (mE_estep sigs Γa _ σ k s0 σ1 s2 h1).1
    simp only [evalPathStep]
    split
    · simp only [pure_bind]
      exact MT.bind (evalC_move n _ Γa k _ σ σ1 s0 s2 hs hv (by simpa [Ctx.lends_key] using h1) hf) hs1 (fun kv =>
        MT.bind (MT.here (indexVal_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial)) (MSt.Le.refl σ1) (fun _ =>
          MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))
    · exact MT.wrong

theorem evalSteps_move : ∀ (n : Nat) (Γ Γa : Ctx) (t : Ty) (v : Val) (steps : List Step) (σ σ' : MSt) (st st' : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mSteps sigs Γa σ steps).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalSteps env call n Γ t v steps)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalSteps]; exact MT.fuel
  | _ + 1, _, _, _, v, [], σ, σ', _, _, _, _, h, _ => by
    unfold mSteps at h; cases h
    simp only [evalSteps]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, t, v, [st], σ, σ', s0, s1, hs, hv, h, hf => by
    simp only [evalSteps]
    exact evalStep_move n Γ Γa t v st σ σ' s0 s1 hs hv h hf
  | n + 1, Γ, Γa, t, v, st :: st2 :: rest, σ, σ', s0, s1, hs, hv, h, hf => by
    rw [mSteps_cons] at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, s2, h1, h⟩ := h
    have hs1 := (mSteps_estep sigs Γa σ [st] s0 σ1 s2 h1).1
    have hf1 : s2.movedHeld = false := flag_before (mSteps_estep sigs Γa σ1 (st2 :: rest) s2 σ' s1 h).2 hf
    simp only [evalSteps]
    exact MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun u =>
      MT.bind (evalStep_move n _ Γa t v st σ σ1 s0 s2 hs hv h1 hf1) hs1 (fun w =>
        evalSteps_move n Γ Γa u w (st2 :: rest) σ1 σ' s2 s1 hs hv h hf))

theorem evalStep_move : ∀ (n : Nat) (Γ Γa : Ctx) (t : Ty) (v : Val) (st : Step) (σ σ' : MSt) (s0 s1 : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → (mSteps sigs Γa σ [st]).run s0 = .ok (σ', s1) → s1.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalStep env call n Γ t v st)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalStep]; exact MT.fuel
  | _ + 1, _, _, _, v, .field s, σ, σ', _, _, _, _, h, _ => by
    unfold mSteps at h; unfold mSteps at h; cases h
    simp only [evalStep]
    exact MT.calm (fieldVal_calm _ _) (fieldVal_noEarly _ _)
  | _ + 1, _, _, _, _, .index _ none, σ, σ', _, _, _, _, h, _ => by simp only [evalStep]; exact MT.wrong
  | n + 1, Γ, Γa, t, v, .index k (some m), σ, σ', s0, s1, hs, hv, h, hf => by
    unfold mSteps at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, s2, h1, h⟩ := h
    replace h := MoveM.bind_ok h
    obtain ⟨σ2, s3, h2, h⟩ := h
    unfold mSteps at h; cases h
    obtain ⟨rfl, rfl, ho⟩ := exitM_run h2
    have hs1 := (mE_estep sigs Γa _ σ k s0 σ1 s2 h1).1
    simp only [evalStep]
    split
    · simp only [pure_bind]
      exact MT.bind (evalC_move n _ Γa k _ σ σ1 s0 s2 hs hv (by simpa [Ctx.lends_key] using h1) hf) hs1 (fun kv =>
        MT.here (indexVal_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial))
    · exact MT.wrong

theorem evalPair_move : ∀ (n : Nat) (Γ Γa : Ctx) (a b : Expr) (expected : Option Ty) (σ σ1 σ' : MSt) (st st1 st' : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ →
    (mE sigs Γa true σ a).run st = .ok (σ1, st1) → st1.movedHeld = false →
    (mE sigs Γa true σ1 b).run st1 = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalPair env call n Γ a b expected)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalPair]; exact MT.fuel
  | n + 1, Γ, Γa, a, b, expected, σ, σ1, σ', st, st1, st', hs, hv, h1, hf1, h2, hf2 => by
    simp only [evalPair]
    exact MT.bind (MT.calm (typeOf_calm _) (typeOf_noEarly _)) (MSt.Le.refl σ) (fun t =>
      MT.bind (evalC_move n _ Γa a t σ σ1 st st1 hs hv (by simpa [Ctx.lends_borrow] using h1) hf1)
        (mE_estep sigs Γa _ σ a st σ1 st1 h1).1 (fun _ =>
        MT.bind (evalC_move n _ Γa b t σ1 σ' st1 st' hs hv (by simpa [Ctx.lends_borrow] using h2) hf2)
          (mE_estep sigs Γa _ σ1 b st1 σ' st' h2).1 (fun _ =>
          MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))))

theorem evalPairs_move : ∀ (n : Nat) (Γ Γa : Ctx) (es : List Expr) (ts : List Ty) (σ σ' : MSt) (st st' : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → Γ.lends = false → (mAll sigs Γa σ es).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalPairs env call n Γ es ts)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalPairs]; exact MT.fuel
  | n + 1, Γ, Γa, e :: es, t :: ts, σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mAll at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    have hf1 : st1.movedHeld = false := flag_before (mAll_estep sigs Γa σ1 es st1 σ' st' h).2 hf
    simp only [evalPairs]
    exact MT.bind (evalC_move n _ Γa e t σ σ1 st st1 hs hv (by rw [hl]; exact h1) hf1) (mE_estep sigs Γa _ σ e st σ1 st1 h1).1 (fun _ =>
      MT.bind (evalPairs_move n Γ Γa es ts σ1 σ' st1 st' hs hv hl h hf) (mAll_estep sigs Γa σ1 es st1 σ' st' h).1 (fun _ =>
        MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))
  | n + 1, Γ, Γa, [], [], σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mAll at h; cases h
    simp only [evalPairs]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, [], _ :: _, σ, σ', st, st', hs, hv, hl, h, hf => by simp only [evalPairs]; exact MT.wrong
  | n + 1, Γ, Γa, _ :: _, [], σ, σ', st, st', hs, hv, hl, h, hf => by simp only [evalPairs]; exact MT.wrong

theorem evalFields_move : ∀ (n : Nat) (Γ Γa : Ctx) (fs : List (String × Expr)) (ts : List (String × Ty)) (σ σ' : MSt)
    (st st' : DSt), SigsIn sigs Γ.sigs → CtxLe Γa Γ → Γ.lends = false → (mFields sigs Γa σ fs).run st = .ok (σ', st') →
    st'.movedHeld = false → MT O σ σ' (fun _ _ => True) (evalFields env call n Γ fs ts)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalFields]; exact MT.fuel
  | n + 1, Γ, Γa, (m, e) :: fs, (k, t) :: ts, σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mFields at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    have hf1 : st1.movedHeld = false := flag_before (mFields_estep sigs Γa σ1 fs st1 σ' st' h).2 hf
    simp only [evalFields]
    exact MT.bind (evalC_move n _ Γa e t σ σ1 st st1 hs hv (by rw [hl]; exact h1) hf1) (mE_estep sigs Γa _ σ e st σ1 st1 h1).1 (fun _ =>
      MT.bind (evalFields_move n Γ Γa fs ts σ1 σ' st1 st' hs hv hl h hf) (mFields_estep sigs Γa σ1 fs st1 σ' st' h).1 (fun _ =>
        MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))
  | n + 1, Γ, Γa, [], [], σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mFields at h; cases h
    simp only [evalFields]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, [], _ :: _, σ, σ', st, st', hs, hv, hl, h, hf => by simp only [evalFields]; exact MT.wrong
  | n + 1, Γ, Γa, _ :: _, [], σ, σ', st, st', hs, hv, hl, h, hf => by simp only [evalFields]; exact MT.wrong

theorem evalAll_move : ∀ (n : Nat) (Γ Γa : Ctx) (es : List Expr) (t : Ty) (σ σ' : MSt) (st st' : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → Γ.lends = false → (mAll sigs Γa σ es).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalAll env call n Γ es t)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalAll]; exact MT.fuel
  | _ + 1, _, _, [], _, σ, σ', _, _, _, _, _, h, _ => by
    unfold mAll at h; cases h
    simp only [evalAll]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, e :: es, t, σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mAll at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    have hf1 : st1.movedHeld = false := flag_before (mAll_estep sigs Γa σ1 es st1 σ' st' h).2 hf
    simp only [evalAll]
    exact MT.bind (evalC_move n _ Γa e t σ σ1 st st1 hs hv (by rw [hl]; exact h1) hf1) (mE_estep sigs Γa _ σ e st σ1 st1 h1).1 (fun _ =>
      MT.bind (evalAll_move n Γ Γa es t σ1 σ' st1 st' hs hv hl h hf) (mAll_estep sigs Γa σ1 es st1 σ' st' h).1 (fun _ =>
        MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))

theorem evalEntries_move : ∀ (n : Nat) (Γ Γa : Ctx) (es : List (Expr × Expr)) (k v : Ty) (σ σ' : MSt) (st st' : DSt),
    SigsIn sigs Γ.sigs → CtxLe Γa Γ → Γ.lends = false → (mEntries sigs Γa σ es).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalEntries env call n Γ es k v)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalEntries]; exact MT.fuel
  | _ + 1, _, _, [], _, _, σ, σ', _, _, _, _, _, h, _ => by
    unfold mEntries at h; cases h
    simp only [evalEntries]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, (ke, ve) :: es, k, v, σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mEntries at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    replace h := MoveM.bind_ok h
    obtain ⟨σ2, st2, h2, h⟩ := h
    have hf2 : st2.movedHeld = false := flag_before (mEntries_estep sigs Γa σ2 es st2 σ' st' h).2 hf
    have hf1 : st1.movedHeld = false := flag_before (mE_estep sigs Γa _ σ1 ve st1 σ2 st2 h2).2 hf2
    simp only [evalEntries]
    exact MT.bind (evalC_move n _ Γa ke k σ σ1 st st1 hs hv (by rw [hl]; exact h1) hf1) (mE_estep sigs Γa _ σ ke st σ1 st1 h1).1 (fun _ =>
      MT.bind (evalC_move n _ Γa ve v σ1 σ2 st1 st2 hs hv (by rw [hl]; exact h2) hf2) (mE_estep sigs Γa _ σ1 ve st1 σ2 st2 h2).1 (fun _ =>
        MT.bind (evalEntries_move n Γ Γa es k v σ2 σ' st2 st' hs hv hl h hf) (mEntries_estep sigs Γa σ2 es st2 σ' st' h).1 (fun _ =>
          MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))))

theorem evalRows_move : ∀ (n : Nat) (Γ Γa : Ctx) (rows : List (List Expr)) (cols : List (String × Ty)) (σ σ' : MSt)
    (st st' : DSt), SigsIn sigs Γ.sigs → CtxLe Γa Γ → Γ.lends = false → (mRows sigs Γa σ rows).run st = .ok (σ', st') →
    st'.movedHeld = false → MT O σ σ' (fun _ _ => True) (evalRows env call n Γ rows cols)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [evalRows]; exact MT.fuel
  | _ + 1, _, _, [], _, σ, σ', _, _, _, _, _, h, _ => by
    unfold mRows at h; cases h
    simp only [evalRows]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, Γa, r :: rs, cols, σ, σ', st, st', hs, hv, hl, h, hf => by
    unfold mRows at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    have hf1 : st1.movedHeld = false := flag_before (mRows_estep sigs Γa σ1 rs st1 σ' st' h).2 hf
    simp only [evalRows]
    exact MT.bind (evalPairs_move n _ Γa r _ σ σ1 st st1 hs hv hl h1 hf1) (mAll_estep sigs Γa σ r st σ1 st1 h1).1 (fun _ =>
      MT.bind (evalRows_move n Γ Γa rs cols σ1 σ' st1 st' hs hv hl h hf) (mRows_estep sigs Γa σ1 rs st1 σ' st' h).1 (fun _ =>
        MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))

end

end

end Datalove.Fun
