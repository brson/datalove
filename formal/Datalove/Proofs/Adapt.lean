import Datalove.Relations
import Datalove.WF
import Datalove.Proofs.Equiv
import Datalove.Proofs.Numeric
import Datalove.Proofs.Expose

/-!
# `@` is transitive

`Ty.adapt_trans`: on well-formed types, if `a@` converts to `b` and `b@` to
`c`, then `a@` converts to `c`, given `ExposeInjective`, which meeting at the
head needs too (`Ty.meet_trans`).

It took two changes to `can_adapt_to` to make this true, both found by
trying to prove it. An atom or term did not adapt to a recursive enum, though
it adapted to the enum the recursive type unfolds to, which meets the
recursive type at the head; and a recursive type whose unfolding is a term
did not adapt to an enum listing the term. `can_adapt_to` now consults its
table of conversions with both sides exposed.

The proof leans on one fact about well-formed recursive types beside
`Ty.expose_not_mu`: one never exposes to a number (`Ty.expose_not_numeric`),
since its body uses its variable, and a number has nowhere to use it. So
widening, which looks at the types as they are, never needs to look through a
`rec`.
-/

namespace Datalove

namespace Ty

theorem expose_of_not_mu {t : Ty} (h : t.isMu = false) : t.expose = t := by
  cases t <;> simp_all [expose, exposeN, leadingMus, isMu]

/-- Types that meet at the head expose alike. -/
theorem meet_expose {x y : Ty} (wx : x.WF) (wy : y.WF) (h : x.meet y = true) :
    x.expose = y.expose := by
  rw [meet_iff] at h
  rcases h with rfl | ⟨_, rfl⟩ | ⟨_, rfl⟩
  · rfl
  · exact (expose_of_not_mu (expose_not_mu wx)).symm
  · exact expose_of_not_mu (expose_not_mu wy)

theorem subst_numeric {t : Ty} (h : t.isRecvar = false) (d : Nat) (w : Ty) :
    (t.subst d w).isNumeric = t.isNumeric := by
  cases t <;> simp_all [isRecvar, Ty.subst, isNumeric]

/-- Substitution leaves whether the core is a number alone. -/
theorem subst_core_numeric : ∀ (t : Ty) (d : Nat) (w : Ty), t.core.isRecvar = false →
    (t.subst d w).core.isNumeric = t.core.isNumeric
  | mu b, d, w, h => by
    simp only [core] at h
    simp only [Ty.subst, core]
    exact subst_core_numeric b (d + 1) w h
  | t, d, w, h => by
    cases ht : t.isMu
    · rw [core_of_not_mu ht] at h ⊢
      rw [core_of_not_mu (subst_head ht h d w).1]
      exact subst_numeric h d w
    · cases t <;> simp_all [isMu, core]
      rename_i b
      simp only [Ty.subst, core]
      exact subst_core_numeric b (d + 1) w h

theorem exposeN_numeric : ∀ (n : Nat) (t : Ty), t.leadingMus = n → t.core.isRecvar = false →
    (exposeN n t).isNumeric = t.core.isNumeric
  | 0, t, hn, _ => by
    cases t <;> simp_all [exposeN, leadingMus, core]
  | n + 1, t, hn, hc => by
    cases t <;> simp_all [leadingMus]
    rename_i b
    simp only [exposeN, unfold, core]
    have hs := subst_core b 0 (mu b) (by simpa [core] using hc)
    rw [exposeN_numeric n _ (by rw [hs.1, hn]) hs.2]
    exact subst_core_numeric b 0 (mu b) (by simpa [core] using hc)

theorem not_numeric_of_mentions {t : Ty} {d : Nat} (h : t.mentions d = true) :
    t.isNumeric = false := by
  cases t <;> simp_all [Ty.mentions, isNumeric]

theorem wfIn_mu (b : Ty) (c : WfCtx) :
    (mu b).wfIn c = (!c.inKey && b.mentions 0 && b.wfIn { c with binders := c.colls :: c.binders }) := by
  simp [Ty.wfIn]

/-- The core of a well-formed `mu` uses a variable, so it is not a number. -/
theorem wf_mu_core_not_numeric : ∀ (b : Ty) (c : WfCtx), (mu b).wfIn c = true →
    b.core.isNumeric = false
  | b, c, h => by
    rw [wfIn_mu] at h
    simp only [Bool.and_eq_true] at h
    cases hb : b.isMu
    · rw [core_of_not_mu hb]
      exact not_numeric_of_mentions h.1.2
    · cases b <;> simp_all [isMu]
      rename_i b'
      simp only [core]
      exact wf_mu_core_not_numeric b' _ h.2

/-- A well-formed recursive type does not expose to a number. -/
theorem expose_not_numeric {t : Ty} (w : t.WF) (m : t.isMu = true) : t.expose.isNumeric = false := by
  cases t <;> simp_all [isMu]
  rename_i b
  simp only [expose]
  rw [exposeN_numeric _ _ rfl (wf_core_not_recvar _ {} rfl w)]
  simp only [core]
  exact wf_mu_core_not_numeric b {} w

/-- A well-formed type that exposes to a number is that number. -/
theorem eq_expose_of_numeric {t : Ty} (w : t.WF) (h : t.expose.isNumeric = true) : t.expose = t := by
  cases m : t.isMu
  · exact expose_of_not_mu m
  · simp [expose_not_numeric w m] at h

theorem canWiden_numeric {x y : Ty} (h : x.canWiden y = true) :
    x.isNumeric = true ∧ y.isNumeric = true := by
  cases x <;> cases y <;> simp_all [canWiden, isNumeric]

/-- What the table relates: two numbers, or an atom or term and an enum. -/
theorem adaptTable_cases {x y : Ty} (h : adaptTable x y = true) :
    (x.isNumeric = true ∧ y.isNumeric = true) ∨ (∃ vs, y = .enum vs) := by
  cases x <;> cases y <;> simp_all [adaptTable, isNumeric]

theorem adaptTable_enum_left {vs : Variants} {y : Ty} : adaptTable (.enum vs) y = false := by
  cases y <;> simp [adaptTable]

theorem isNumeric_enum {vs : Variants} : (Ty.enum vs).isNumeric = false := by
  simp [isNumeric]

theorem not_mu_of_numeric {t : Ty} (h : t.isNumeric = true) : t.isMu = false := by
  cases t <;> simp_all [isNumeric, isMu]

theorem adaptTable_numeric_enum {x : Ty} {vs : Variants} (h : x.isNumeric = true) :
    adaptTable x (.enum vs) = false := by
  cases x <;> simp_all [adaptTable, isNumeric]

/-- A number exposes to itself. -/
theorem expose_numeric {t : Ty} (h : t.isNumeric = true) : t.expose.isNumeric = true := by
  rw [expose_of_not_mu (not_mu_of_numeric h)]; exact h

end Ty

/-- `@` is transitive on well-formed types, given `ExposeInjective`. -/
theorem Ty.adapt_trans (inj : ExposeInjective) {a b c : Ty} (wa : a.WF) (wb : b.WF) (wc : c.WF)
    (h₁ : a.canAdapt b = true) (h₂ : b.canAdapt c = true) : a.canAdapt c = true := by
  -- Every step between numbers is between the types themselves, and those
  -- compose by `adapt_numeric_trans`.
  have numeric (na : a.expose.isNumeric = true) (nb : b.expose.isNumeric = true)
      (nc : c.expose.isNumeric = true) : a.canAdapt c = true := by
    rw [Ty.eq_expose_of_numeric wa na] at na
    rw [Ty.eq_expose_of_numeric wb nb] at nb
    rw [Ty.eq_expose_of_numeric wc nc] at nc
    exact Ty.adapt_numeric_trans a b c na nb nc h₁ h₂
  have h₁' := h₁
  have h₂' := h₂
  simp only [Ty.canAdapt, Bool.or_eq_true] at h₁' h₂'
  rcases h₁' with (m₁ | w₁) | t₁ <;> rcases h₂' with (m₂ | w₂) | t₂
  -- Meeting, then meeting.
  · simp [Ty.canAdapt, Ty.meet_trans inj wa wb wc m₁ m₂]
  -- Meeting, then widening: `b` is a number, and `a` exposes as `b` does.
  · have nb := (Ty.canWiden_numeric w₂).1
    have ea := Ty.meet_expose wa wb m₁
    exact numeric (by rw [ea]; exact Ty.expose_numeric nb) (Ty.expose_numeric nb)
      (Ty.expose_numeric (Ty.canWiden_numeric w₂).2)
  -- Meeting, then the table: the table reads `b` exposed, which is `a` exposed.
  · simp [Ty.canAdapt, Ty.meet_expose wa wb m₁, t₂]
  -- Widening, then meeting: `b` is a number, and `c` exposes as `b` does.
  · have nb := (Ty.canWiden_numeric w₁).2
    have ec := Ty.meet_expose wb wc m₂
    exact numeric (Ty.expose_numeric (Ty.canWiden_numeric w₁).1) (Ty.expose_numeric nb)
      (by rw [← ec]; exact Ty.expose_numeric nb)
  -- Widening, then widening.
  · exact numeric (Ty.expose_numeric (Ty.canWiden_numeric w₁).1)
      (Ty.expose_numeric (Ty.canWiden_numeric w₁).2) (Ty.expose_numeric (Ty.canWiden_numeric w₂).2)
  -- Widening, then the table: `b` is a number, so the table relates numbers.
  · have nb := (Ty.canWiden_numeric w₁).2
    rcases Ty.adaptTable_cases t₂ with ⟨_, nc⟩ | ⟨vs, e⟩
    · exact numeric (Ty.expose_numeric (Ty.canWiden_numeric w₁).1) (Ty.expose_numeric nb) nc
    · rw [e, Ty.adaptTable_numeric_enum (Ty.expose_numeric nb)] at t₂
      exact absurd t₂ (by simp)
  -- The table, then meeting: `c` exposes as `b` does.
  · simp [Ty.canAdapt, ← Ty.meet_expose wb wc m₂, t₁]
  -- The table, then widening: `b` is a number, so the table relates numbers.
  · have nb := (Ty.canWiden_numeric w₂).1
    rcases Ty.adaptTable_cases t₁ with ⟨na, _⟩ | ⟨vs, e⟩
    · exact numeric na (Ty.expose_numeric nb) (Ty.expose_numeric (Ty.canWiden_numeric w₂).2)
    · have := Ty.expose_numeric nb
      rw [e, Ty.isNumeric_enum] at this
      exact absurd this (by simp)
  -- The table, then the table: an enum is never a source, so both steps are
  -- between numbers.
  · rcases Ty.adaptTable_cases t₁ with ⟨na, nb⟩ | ⟨vs, e⟩
    · rcases Ty.adaptTable_cases t₂ with ⟨_, nc⟩ | ⟨vs, e⟩
      · exact numeric na nb nc
      · rw [e, Ty.adaptTable_numeric_enum nb] at t₂
        exact absurd t₂ (by simp)
    · rw [e, Ty.adaptTable_enum_left] at t₂
      exact absurd t₂ (by simp)

end Datalove
