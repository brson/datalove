import Datalove.Relations
import Datalove.WF
import Datalove.Proofs.Equiv

/-!
# Exposing a recursive type

`expose` in `types.rs` unfolds a `rec` at the head of a type until the head
is something else, in a loop that would not end on `rec X X`. The model
unfolds as many times as the type has leading `mu`s. `Ty.expose_not_mu`
shows that is enough on every well-formed type, so the two agree there and
the Rust loop ends: what stops `rec X X` is T064.

Then meeting at the head. `Ty.meet_trans` shows `≈` is transitive on
well-formed types if two recursive types that unfold alike are one type,
`ExposeInjective`. That is not proved here; the law tests in
`crates/datalove-formal-tests` look for a counterexample and have not found
one.
-/

namespace Datalove

namespace Ty

def isRecvar : Ty → Bool
  | recvar _ => true
  | _ => false

/-- What is under a type's leading `mu`s. -/
def core : Ty → Ty
  | mu b => b.core
  | t => t

/-- A type whose head is neither a `mu` nor a variable keeps its head under
substitution. -/
theorem subst_head {t : Ty} (hm : t.isMu = false) (hv : t.isRecvar = false) (d : Nat) (w : Ty) :
    (t.subst d w).isMu = false ∧ (t.subst d w).isRecvar = false := by
  cases t <;> simp_all [isMu, isRecvar, Ty.subst]

theorem core_of_not_mu {t : Ty} (h : t.isMu = false) : t.core = t := by
  cases t <;> simp_all [core, isMu]

theorem core_not_mu : ∀ (t : Ty), t.core.isMu = false
  | mu b => by simp only [core]; exact core_not_mu b
  | t => by
    cases ht : t.isMu
    · rw [core_of_not_mu ht]; exact ht
    · cases t <;> simp_all [isMu]
      rename_i b
      simp only [core]; exact core_not_mu b

theorem leadingMus_of_not_mu {t : Ty} (h : t.isMu = false) : t.leadingMus = 0 := by
  cases t <;> simp_all [leadingMus, isMu]

/-- Substitution into a type whose core is not a variable leaves its leading
`mu`s where they are, and its core not a variable. -/
theorem subst_core : ∀ (t : Ty) (d : Nat) (w : Ty), t.core.isRecvar = false →
    (t.subst d w).leadingMus = t.leadingMus ∧ (t.subst d w).core.isRecvar = false
  | mu b, d, w, h => by
    simp only [core] at h
    have ih := subst_core b (d + 1) w h
    simp [Ty.subst, leadingMus, core, ih]
  | t, d, w, h => by
    cases ht : t.isMu
    · rw [core_of_not_mu ht] at h
      have hs := subst_head ht h d w
      simp [leadingMus_of_not_mu ht, leadingMus_of_not_mu hs.1, core_of_not_mu hs.1, hs.2]
    · cases t <;> simp_all [isMu, core]
      rename_i b
      have ih := subst_core b (d + 1) w h
      simp [Ty.subst, leadingMus, core, ih]

theorem exposeN_not_mu : ∀ (n : Nat) (t : Ty), t.leadingMus = n → t.core.isRecvar = false →
    (exposeN n t).isMu = false
  | 0, t, hn, _ => by
    cases t <;> simp_all [exposeN, leadingMus, isMu]
  | n + 1, t, hn, hc => by
    cases t <;> simp_all [leadingMus]
    rename_i b
    simp only [exposeN, unfold]
    have hs := subst_core b 0 (mu b) (by simpa [core] using hc)
    exact exposeN_not_mu n _ (by rw [hs.1, hn]) hs.2

/-- A type well formed where no collection is around it has no variable at
its core: a variable there would recur with nothing to guard it. -/
theorem wf_core_not_recvar : ∀ (t : Ty) (c : WfCtx), c.colls = 0 → t.wfIn c = true →
    t.core.isRecvar = false
  | mu b, c, h0, hw => by
    simp only [Ty.wfIn, Bool.and_eq_true] at hw
    exact wf_core_not_recvar b { c with binders := c.colls :: c.binders } h0 hw.2
  | recvar i, c, h0, hw => by
    simp only [Ty.wfIn] at hw
    split at hw <;> simp_all
  | t, c, h0, hw => by
    cases hm : t.isMu
    · rw [core_of_not_mu hm]
      cases t <;> simp_all [isRecvar, isMu]
      simp only [Ty.wfIn] at hw
      split at hw <;> simp_all
    · cases t <;> simp_all [isMu]
      rename_i b
      simp only [Ty.wfIn, Bool.and_eq_true] at hw
      simp only [core]
      exact wf_core_not_recvar b { c with binders := c.colls :: c.binders } h0 hw.2

/-- On a well-formed type, `expose` leaves no `rec` at the head: the Rust
loop ends, and ends where the model's does. -/
theorem expose_not_mu {t : Ty} (h : t.WF) : t.expose.isMu = false :=
  exposeN_not_mu _ t rfl (wf_core_not_recvar t {} rfl h)

theorem meet_iff {a b : Ty} :
    a.meet b = true ↔ a = b ∨ (a.isMu = true ∧ a.expose = b) ∨ (b.isMu = true ∧ a = b.expose) := by
  simp [meet, Ty.equiv_iff_eq, or_assoc]

end Ty

/-- Two well-formed recursive types that unfold alike are one type.

Believed, not proved. If it fails, two different recursive types each meet
their common unfolding at the head without meeting each other, and `≈` is not
transitive. The law tests look for such a pair among the types they make. -/
def ExposeInjective : Prop :=
  ∀ x y : Ty, x.WF → y.WF → x.isMu = true → y.isMu = true → x.expose = y.expose → x = y

/-- Meeting at the head is transitive on well-formed types, given
`ExposeInjective`. -/
theorem Ty.meet_trans (inj : ExposeInjective) {a b c : Ty} (wa : a.WF) (wb : b.WF) (wc : c.WF)
    (h₁ : a.meet b = true) (h₂ : b.meet c = true) : a.meet c = true := by
  rw [Ty.meet_iff] at h₁ h₂ ⊢
  rcases h₁ with rfl | ⟨ma, rfl⟩ | ⟨mb, rfl⟩ <;> rcases h₂ with rfl | ⟨mb', rfl⟩ | ⟨mc, e⟩
  · exact Or.inl rfl
  · exact Or.inr (Or.inl ⟨mb', rfl⟩)
  · exact Or.inr (Or.inr ⟨mc, e⟩)
  · exact Or.inr (Or.inl ⟨ma, rfl⟩)
  · simp [Ty.expose_not_mu wa] at mb'
  · exact Or.inl (inj a c wa wc ma mc e)
  · exact Or.inr (Or.inr ⟨mb, rfl⟩)
  · exact Or.inl rfl
  · simp [e, Ty.expose_not_mu wc] at mb

end Datalove
