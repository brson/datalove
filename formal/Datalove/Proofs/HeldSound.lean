import Datalove.FunHeld

/-!
# Where O017 accepts, a held operand read late reads what it read early

The model reads each operand where it is written (`Fun.evalS`); the compiled
code reads a held one through a pointer once what follows it has run
(`FunHeld.lean`). What runs in between is calls, each writing back to the
places its `mut` and `out` arguments name, and moves, which leave a place
holding what the compiled code no longer owns. This abstracts each to its
effect on a store of locations: some locations changed, any other left as it
was (`Late.Write`). A read of a location none of them changes reads the same
after them as before (`Late.late_eq_early`).

`Fun.held_reads_agree` puts O017 in those terms. The walk pairs each write
it meets with every place held over it (`Fun.clashes`), and O017 accepts
where no pair overlaps (`Fun.heldTaken`). Under locations as the compiled
code gives places, two sharing one only where they overlap
(`Fun.PlaceLocs`), which is how they are told apart (`overlaps`), no write
held over a place changes a location of it, and the place read late reads
what it read early. The writes a held place meets are the walk's for it by construction;
`held_agrees` holds the walk to the Rust's O017.

Two kinds of write are not O017's to compare: arguments of one call, which
O010 compares (`Fun.aliased_unshared`), and what a place holds being freed,
since values here are never freed.
-/

namespace Datalove.Late

/-- What runs between a held operand and its read: a call writing back, or a
move. It changes the locations `locs`, each to what `f` makes of the store,
and leaves the rest as they were. -/
structure Write (α : Type) where
  locs : List Nat
  f : (Nat → α) → (Nat → α)
  frame : ∀ σ l, l ∉ locs → f σ l = σ l

/-- The store once writes have run, in order. -/
def run {α} : List (Write α) → (Nat → α) → (Nat → α)
  | [], σ => σ
  | w :: ws, σ => run ws (w.f σ)

/-- **A read made late reads what it read early**, where none of the writes
run between changes its location. -/
theorem late_eq_early {α} : ∀ (ws : List (Write α)) (σ : Nat → α) (l : Nat),
    (∀ w ∈ ws, l ∉ w.locs) → run ws σ l = σ l
  | [], _, _, _ => rfl
  | w :: ws, σ, l, h => by
    simp only [run]
    rw [late_eq_early ws (w.f σ) l (fun w' hw' => h w' (List.mem_cons_of_mem _ hw'))]
    exact w.frame σ l (h w List.mem_cons_self)

end Datalove.Late

namespace Datalove.Fun

/-- Locations as the compiled code gives places: two places share one only
where they overlap, as two that part at a field do not. -/
def PlaceLocs (loc : String × List Sel → List Nat) : Prop :=
  ∀ p q l, l ∈ loc p → l ∈ loc q → overlaps p q = true

/-- Of writes O017 accepts, none is to a place overlapping one held over it. -/
theorem heldTaken_apart {cs : List Clash} (h : heldTaken cs = false) : ∀ c ∈ cs, overlaps c.held c.taken = false := by
  intro c hc
  simp only [heldTaken, List.any_eq_false] at h
  simpa using h c hc

/-- **Where O017 accepts, a held operand read late reads what it read
early**: under locations as the compiled code gives places, writes each to
a place the walk met held over `q` leave every location of `q` as it was. -/
theorem held_reads_agree {α} {cs : List Clash} (h : heldTaken cs = false)
    {loc : String × List Sel → List Nat} (hf : PlaceLocs loc) {q : String × List Sel}
    (ws : List (Late.Write α)) (hw : ∀ w ∈ ws, ∃ c ∈ cs, c.held = q ∧ ∀ l ∈ w.locs, l ∈ loc c.taken)
    (σ : Nat → α) : ∀ l ∈ loc q, Late.run ws σ l = σ l := by
  intro l hl
  refine Late.late_eq_early ws σ l (fun w hwm hlw => ?_)
  obtain ⟨c, hc, rfl, hloc⟩ := hw w hwm
  have := hf c.held c.taken l hl (hloc l hlw)
  rw [heldTaken_apart h c hc] at this
  exact Bool.false_ne_true this

end Datalove.Fun
