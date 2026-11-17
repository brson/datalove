import Datalove.FunAlias
import Datalove.Proofs.PassBy

/-!
# A call O010 accepts passes nothing written by two pointers

`Fun.aliased_apart`: of a call's arguments O010 accepts (`Fun.aliased`), no
argument a `mut` or `out` parameter takes names a place that overlaps one
another argument names and is passed by pointer, which is any but a copy
given to an `in` parameter, read into a value of its own before the call.

`Fun.aliased_unshared` puts it as `PassBy.ptr_eq_copy` asks it: where two
arguments share a location only where they name places that overlap and are
passed by pointer (`Faithful`), as places that part at a field do not, no
other argument's location is a written one's (`PassBy.Unshared`). So the
call by pointer and by copy-in, copy-out end with the same store.
-/

namespace Datalove.Fun

theorem overlaps_comm (p q : String × List Sel) : overlaps p q = overlaps q p := by
  unfold overlaps
  rw [Bool.or_comm]
  cases h : p.1 == q.1 <;> cases h' : q.1 == p.1 <;> simp_all

/-- Of arguments O010 accepts, no earlier one and later one are O010. -/
theorem aliased_pair : ∀ {l : List (Takes × Expr)}, aliased l = false →
    ∀ {i j : Nat} {a b : Takes × Expr}, i < j → l[i]? = some a → l[j]? = some b → pairAliased a b = false
  | [], _, _, _, _, _, _, ha, _ => by simp at ha
  | c :: rest, h, i, j, a, b, hij, ha, hb => by
    simp only [aliased, Bool.or_eq_false_iff, List.any_eq_false] at h
    obtain ⟨hany, hrest⟩ := h
    cases i with
    | zero =>
      simp only [List.getElem?_cons_zero, Option.some.injEq] at ha
      subst ha
      obtain ⟨j', rfl⟩ : ∃ j', j = j' + 1 := ⟨j - 1, by omega⟩
      simp only [List.getElem?_cons_succ] at hb
      simpa using hany b (List.mem_of_getElem? hb)
    | succ i =>
      obtain ⟨j', rfl⟩ : ∃ j', j = j' + 1 := ⟨j - 1, by omega⟩
      simp only [List.getElem?_cons_succ] at ha hb
      exact aliased_pair hrest (by omega) ha hb

/-- Two arguments passed by pointer, one of them written, clash. -/
theorem clash_write {t u : Takes} (ht : t ≠ .copyIn) (hu : u ≠ .copyIn) (hw : t = .write ∨ u = .write) :
    clash t u = true := by
  cases t <;> cases u <;> simp_all [clash]

/-- **A call O010 accepts passes nothing written by two pointers**: no
argument a `mut` or `out` parameter takes names a place overlapping one that
another argument passed by pointer names. -/
theorem aliased_apart {args : List (Takes × Expr)} (h : aliased args = false) {i j : Nat} (hij : i ≠ j)
    {a b : Takes × Expr} (ha : args[i]? = some a) (hb : args[j]? = some b)
    (hw : a.1 = .write) (hc : b.1 ≠ .copyIn) {p q : String × List Sel}
    (hp : placeKey a.2 = some p) (hq : placeKey b.2 = some q) : overlaps p q = false := by
  have hcl : clash a.1 b.1 = true ∧ clash b.1 a.1 = true :=
    ⟨clash_write (by simp [hw]) hc (.inl hw), clash_write hc (by simp [hw]) (.inr hw)⟩
  rcases Nat.lt_or_gt_of_ne hij with hlt | hlt
  · have := aliased_pair h hlt ha hb
    simp only [pairAliased, hp, hq, hcl.1, Bool.and_true] at this
    exact this
  · have := aliased_pair h hlt hb ha
    simp only [pairAliased, hp, hq, hcl.2, Bool.and_true] at this
    rw [overlaps_comm]; exact this

/-- The arguments a `mut` or `out` parameter takes, by position. -/
def writes (args : List (Takes × Expr)) : List Nat :=
  (List.range args.length).filter fun k => args[k]?.map (·.1) == some .write

/-- Locations of a call's arguments, as passing by pointer gives them: two
share one only where they name places that overlap and both are passed by
pointer. A copy given to an `in` parameter is read into a location of its
own, and a value that is no place is one. -/
def Faithful (args : List (Takes × Expr)) (locs : Nat → Nat) : Prop :=
  ∀ i j, locs i = locs j → i = j ∨ ∃ a b p q, args[i]? = some a ∧ args[j]? = some b ∧
    a.1 ≠ .copyIn ∧ b.1 ≠ .copyIn ∧ placeKey a.2 = some p ∧ placeKey b.2 = some q ∧ overlaps p q = true

/-- **O010 gives `PassBy` what it asks**: under locations as passing by
pointer gives them, no argument's location is a written one's but its own,
so the call by pointer and by copy-in, copy-out end with the same store
(`PassBy.ptr_eq_copy`). -/
theorem aliased_unshared {args : List (Takes × Expr)} (h : aliased args = false) {locs : Nat → Nat}
    (hf : Faithful args locs) : PassBy.Unshared locs (writes args) := by
  intro k hk j hj
  by_cases hjk : j = k
  · exact hjk
  simp only [writes, List.mem_filter, List.mem_range, beq_iff_eq] at hk
  rcases hf j k hj with e | ⟨b, a, q, p, hb, ha, hbc, -, hq, hp, ho⟩
  · exact e
  · exfalso
    have hw : a.1 = .write := by simpa [ha] using hk.2
    have := aliased_apart h (Ne.symm hjk) ha hb hw hbc hp hq
    rw [overlaps_comm] at ho
    simp_all
