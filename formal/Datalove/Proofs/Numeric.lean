import Datalove.Relations
import Datalove.Proofs.Equiv

/-!
# What `@` does to numbers

`mandocs/types.md` promises numerical correctness: `@` widens an integer only
to a type that holds every value of it. Here that is a theorem about
`can_adapt_to` and the ranges of the integer types: `Ty.adapt_int_preserves`.
And it never narrows: `Ty.adapt_numeric_antisymm`.

`index` and `offset` have a width the build picks (`index-64`), so their
ranges are taken as parameters: any width at all makes the theorem hold,
since the only thing either adapts to is `int`.

`f32 ⇝ f64` is exact as well, but saying so needs a model of IEEE 754, which
this does not have.
-/

namespace Datalove

/-- The integers a value of an integer type can be, with `index` and
`offset` of `w` bits. `none` for a type that is not an integer. -/
def Ty.holdsInt (w : Nat) : Ty → Option (Int → Prop)
  | .u8 => some fun n => 0 ≤ n ∧ n < 2 ^ 8
  | .u16 => some fun n => 0 ≤ n ∧ n < 2 ^ 16
  | .u32 => some fun n => 0 ≤ n ∧ n < 2 ^ 32
  | .u64 => some fun n => 0 ≤ n ∧ n < 2 ^ 64
  | .i8 => some fun n => -2 ^ 7 ≤ n ∧ n < 2 ^ 7
  | .i16 => some fun n => -2 ^ 15 ≤ n ∧ n < 2 ^ 15
  | .i32 => some fun n => -2 ^ 31 ≤ n ∧ n < 2 ^ 31
  | .i64 => some fun n => -2 ^ 63 ≤ n ∧ n < 2 ^ 63
  | .index => some fun n => 0 ≤ n ∧ n < 2 ^ w
  | .offset => some fun n => -2 ^ (w - 1) ≤ n ∧ n < 2 ^ (w - 1)
  | .int => some fun _ => True
  | _ => none

/-- The numeric types. -/
def Ty.numerics : List Ty :=
  [.u8, .i8, .u16, .i16, .u32, .i32, .u64, .i64, .index, .offset, .f32, .f64, .int]

theorem Ty.mem_numerics {a : Ty} (h : a.isNumeric = true) : a ∈ Ty.numerics := by
  cases a <;> simp_all [Ty.isNumeric, Ty.numerics]

theorem Ty.mem_numerics_of_holdsInt {w : Nat} {a : Ty} {p : Int → Prop}
    (h : a.holdsInt w = some p) : a ∈ Ty.numerics := by
  cases a <;> simp_all [Ty.holdsInt, Ty.numerics]

/-- Every integer `@` converts to another integer type is a value of that
type: no value is lost or changed. -/
theorem Ty.adapt_int_preserves (w : Nat) (a b : Ty) (pa pb : Int → Prop)
    (ha : a.holdsInt w = some pa) (hb : b.holdsInt w = some pb)
    (h : a.canAdapt b = true) (n : Int) (hn : pa n) : pb n := by
  have ma := Ty.mem_numerics_of_holdsInt ha
  have mb := Ty.mem_numerics_of_holdsInt hb
  simp only [Ty.numerics, List.mem_cons, List.not_mem_nil, or_false] at ma mb
  rcases ma with rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl <;>
  rcases mb with rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl <;>
  simp [Ty.holdsInt] at ha hb <;> subst ha hb <;>
  simp_all [Ty.canAdapt, Ty.adaptTable, Ty.meet, Ty.canWiden, Ty.equiv, Ty.expose,
    Ty.leadingMus, Ty.exposeN] <;> omega

theorem Ty.adapt_numeric_antisymm_all :
    ∀ a ∈ Ty.numerics, ∀ b ∈ Ty.numerics,
      a.canAdapt b = true → b.canAdapt a = true → a = b := by
  decide +kernel

/-- `@` between two numeric types goes one way only, unless they are one
type: there is no narrowing. -/
theorem Ty.adapt_numeric_antisymm (a b : Ty) (ha : a.isNumeric = true) (hb : b.isNumeric = true)
    (h₁ : a.canAdapt b = true) (h₂ : b.canAdapt a = true) : a = b :=
  Ty.adapt_numeric_antisymm_all a (Ty.mem_numerics ha) b (Ty.mem_numerics hb) h₁ h₂

theorem Ty.adapt_numeric_trans_all :
    ∀ a ∈ Ty.numerics, ∀ b ∈ Ty.numerics, ∀ c ∈ Ty.numerics,
      a.canAdapt b = true → b.canAdapt c = true → a.canAdapt c = true := by
  decide +kernel

/-- Numeric `@` is transitive: the chains of `can_widen_to` and the
unsigned-to-signed table of `can_adapt_to` compose to entries of the same
tables. -/
theorem Ty.adapt_numeric_trans (a b c : Ty)
    (ha : a.isNumeric = true) (hb : b.isNumeric = true) (hc : c.isNumeric = true)
    (h₁ : a.canAdapt b = true) (h₂ : b.canAdapt c = true) : a.canAdapt c = true :=
  Ty.adapt_numeric_trans_all a (Ty.mem_numerics ha) b (Ty.mem_numerics hb) c (Ty.mem_numerics hc) h₁ h₂

end Datalove
