import Datalove.Relations

/-!
# `≡` is syntactic equality

`botdocs/datalit-typing-rules.md` says type equivalence is syntactic
equality, but for the order of an enum's variants, which the checker sorts
when it builds an enum, and for the names of `rec`s, which do not matter. In
the representation the Rust uses, with variants sorted and `rec`s by de
Bruijn index, `types_equivalent` should then be plain equality. It is:
`Ty.equiv_iff_eq`.

So `≡` is an equivalence relation, decidable, and the derived `PartialEq` on
the Rust `Type` could stand for it; the oracle tests check that the two agree
as well.
-/

namespace Datalove

mutual

theorem Ty.equiv_sound : ∀ (a b : Ty), a.equiv b = true → a = b
  | .list a, b, h | .set a, b, h | .option a, b, h | .result a, b, h | .mu a, b, h => by
    cases b <;> simp [Ty.equiv] at h ⊢; exact Ty.equiv_sound a _ h
  | .tensor a r, b, h => by
    cases b <;> simp [Ty.equiv] at h ⊢; exact ⟨Ty.equiv_sound a _ h.2, h.1⟩
  | .term n a, b, h => by
    cases b <;> simp [Ty.equiv] at h ⊢; exact ⟨h.1, Ty.equiv_sound a _ h.2⟩
  | .map k v, b, h => by
    cases b <;> simp [Ty.equiv] at h ⊢; exact ⟨Ty.equiv_sound k _ h.1, Ty.equiv_sound v _ h.2⟩
  | .tuple fs, b, h => by cases b <;> simp [Ty.equiv] at h ⊢; exact Tys.equiv_sound fs _ h
  | .struct fs, b, h | .table fs, b, h => by
    cases b <;> simp [Ty.equiv] at h ⊢; exact Fields.equiv_sound fs _ h
  | .enum vs, b, h => by cases b <;> simp [Ty.equiv] at h ⊢; exact Variants.equiv_sound vs _ h
  | .bool, b, h | .u8, b, h | .i8, b, h | .u16, b, h | .i16, b, h | .u32, b, h | .i32, b, h
  | .u64, b, h | .i64, b, h | .index, b, h | .offset, b, h | .f32, b, h | .f64, b, h
  | .int, b, h | .string, b, h | .data, b, h | .error, b, h
  | .atom _, b, h | .var _, b, h | .recvar _, b, h => by
    cases b <;> simp_all [Ty.equiv]

theorem Tys.equiv_sound : ∀ (a b : Tys), a.equiv b = true → a = b
  | .nil, b, h => by cases b <;> simp_all [Tys.equiv]
  | .cons t ts, b, h => by
    cases b <;> simp [Tys.equiv] at h ⊢; exact ⟨Ty.equiv_sound t _ h.1, Tys.equiv_sound ts _ h.2⟩

theorem Fields.equiv_sound : ∀ (a b : Fields), a.equiv b = true → a = b
  | .nil, b, h => by cases b <;> simp_all [Fields.equiv]
  | .cons n t fs, b, h => by
    cases b <;> simp [Fields.equiv] at h ⊢
    exact ⟨h.1.1, Ty.equiv_sound t _ h.1.2, Fields.equiv_sound fs _ h.2⟩

theorem Variants.equiv_sound : ∀ (a b : Variants), a.equiv b = true → a = b
  | .nil, b, h => by cases b <;> simp_all [Variants.equiv]
  | .atom n vs, b, h => by
    cases b <;> simp [Variants.equiv] at h ⊢; exact ⟨h.1, Variants.equiv_sound vs _ h.2⟩
  | .term n t vs, b, h => by
    cases b <;> simp [Variants.equiv] at h ⊢
    exact ⟨h.1.1, Ty.equiv_sound t _ h.1.2, Variants.equiv_sound vs _ h.2⟩

end

mutual

theorem Ty.equiv_refl : ∀ (a : Ty), a.equiv a = true
  | .list a | .set a | .option a | .result a | .mu a | .tensor a _ | .term _ a => by
    simp [Ty.equiv, Ty.equiv_refl a]
  | .map k v => by simp [Ty.equiv, Ty.equiv_refl k, Ty.equiv_refl v]
  | .tuple fs => by simp [Ty.equiv, Tys.equiv_refl fs]
  | .struct fs | .table fs => by simp [Ty.equiv, Fields.equiv_refl fs]
  | .enum vs => by simp [Ty.equiv, Variants.equiv_refl vs]
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset
  | .f32 | .f64 | .int | .string | .data | .error | .atom _ | .var _ | .recvar _ => by
    simp [Ty.equiv]

theorem Tys.equiv_refl : ∀ (a : Tys), a.equiv a = true
  | .nil => by simp [Tys.equiv]
  | .cons t ts => by simp [Tys.equiv, Ty.equiv_refl t, Tys.equiv_refl ts]

theorem Fields.equiv_refl : ∀ (a : Fields), a.equiv a = true
  | .nil => by simp [Fields.equiv]
  | .cons _ t fs => by simp [Fields.equiv, Ty.equiv_refl t, Fields.equiv_refl fs]

theorem Variants.equiv_refl : ∀ (a : Variants), a.equiv a = true
  | .nil => by simp [Variants.equiv]
  | .atom _ vs => by simp [Variants.equiv, Variants.equiv_refl vs]
  | .term _ t vs => by simp [Variants.equiv, Ty.equiv_refl t, Variants.equiv_refl vs]

end

/-- `types_equivalent` is equality. -/
theorem Ty.equiv_iff_eq (a b : Ty) : a.equiv b = true ↔ a = b :=
  ⟨Ty.equiv_sound a b, fun h => h ▸ Ty.equiv_refl a⟩

theorem Ty.equiv_eq_decide (a b : Ty) : a.equiv b = decide (a = b) := by
  by_cases h : a = b
  · simp [h, Ty.equiv_refl]
  · have : a.equiv b ≠ true := fun e => h (Ty.equiv_sound a b e)
    simp_all

theorem Ty.equiv_symm (a b : Ty) : a.equiv b = b.equiv a := by
  simp [Ty.equiv_eq_decide, eq_comm]

theorem Ty.equiv_trans {a b c : Ty} (h₁ : a.equiv b = true) (h₂ : b.equiv c = true) :
    a.equiv c = true := by
  rw [Ty.equiv_iff_eq] at *; exact h₁.trans h₂

end Datalove
