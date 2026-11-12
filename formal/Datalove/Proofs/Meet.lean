import Datalove.Relations
import Datalove.Proofs.Equiv

/-!
# Meeting at the head

`T ≈ U` holds when the two are equivalent once each is unfolded at the head,
so it is equality of what they expose to, and an equivalence relation:
`Ty.meet_iff`, and the three laws after it.

It was once defined as `T ≡ U`, or one side's unfolding equivalent to the
other side as it was. That is not transitive: `rec L rec M [(L, M)]` and its
one-step unfolding `rec N [(rec L rec M [(L, M)], N)]` each met their common
unfolding and not each other.
-/

namespace Datalove

namespace Ty

theorem meet_iff {a b : Ty} : a.meet b = true ↔ a.expose = b.expose := by
  simp [meet, Ty.equiv_iff_eq]

theorem meet_refl (a : Ty) : a.meet a = true := meet_iff.mpr rfl

theorem meet_symm (a b : Ty) : a.meet b = b.meet a := by
  simp [meet, Ty.equiv_symm]

theorem meet_trans {a b c : Ty} (h₁ : a.meet b = true) (h₂ : b.meet c = true) :
    a.meet c = true :=
  meet_iff.mpr ((meet_iff.mp h₁).trans (meet_iff.mp h₂))

end Ty

end Datalove
