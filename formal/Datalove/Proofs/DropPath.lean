import Datalove.Proofs.DropWalk

/-!
# A path's own state, and what the walk's has to say of it

The fourth step of the drop schedule's proof (`botdocs/plan-drop-proof.md`)
needs a path's own state beside the walk's: for each variable in scope,
whether it holds a value on the way the path came. It is kept as a list of
flags lined up with the walk's state at the same point, and changed only by
what the walk says a step moves and gives (`delta`), by the bindings a step
makes, and by the scopes the path leaves.

The walk's state and a path's agree but for bindings a loop gives a value
on a later pass (`Div`): those hold a value on the path where the walk has
them moved, and they are found perhaps held, kept by a tracking byte, and
either already held where the path is or made before a loop the path is
in. `Rel` says this of each variable.
-/

namespace Datalove.Fun

/-- A binding the path holds and the walk has moved: owned, found perhaps
held and tracked by the end of the function's walk (`F`), and held already
at `st` or made before the innermost loop the path is in, whose bindings
are numbered from `U`. -/
def Div (F st : DSt) (U : Nat) (v : MVar) : Prop :=
  v.owns = true ∧ v.uid ∈ F.held ∧ v.uid ∈ F.tracked ∧ (v.uid ∈ st.held ∨ v.uid < U)

/-- The walk's state and a path's, lined up: what the walk has live the
path holds, and what the path holds the walk has live, or it diverges. -/
inductive Rel (F st : DSt) (U : Nat) : MSt → List Bool → Prop where
  | nil : Rel F st U [] []
  | cons {v : MVar} {p : Bool} {σ : MSt} {π : List Bool} :
      (v.live = true → p = true) → (p = true → v.live = true ∨ Div F st U v) → Rel F st U σ π →
      Rel F st U (v :: σ) (p :: π)

theorem Rel.length {F st : DSt} {U : Nat} : ∀ {σ : MSt} {π : List Bool}, Rel F st U σ π → π.length = σ.length
  | [], [], .nil => rfl
  | _ :: _, _ :: _, .cons _ _ t => by simp [Rel.length t]

theorem Div.mono {F st st' : DSt} {U U' : Nat} {v : MVar} (h : Div F st U v)
    (hs : ∀ u, u ∈ st.held → u ∈ st'.held) (hu : U ≤ U') : Div F st' U' v :=
  ⟨h.1, h.2.1, h.2.2.1, h.2.2.2.imp (hs _) (fun l => Nat.lt_of_lt_of_le l hu)⟩

theorem Rel.mono {F st st' : DSt} {U U' : Nat} (hs : ∀ u, u ∈ st.held → u ∈ st'.held) (hu : U ≤ U') :
    ∀ {σ : MSt} {π : List Bool}, Rel F st U σ π → Rel F st' U' σ π
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons a b t => .cons a (fun hp => (b hp).imp id (fun d => d.mono hs hu)) (Rel.mono hs hu t)

theorem Rel.drop {F st : DSt} {U : Nat} (n : Nat) : ∀ {σ : MSt} {π : List Bool}, Rel F st U σ π →
    Rel F st U (σ.drop n) (π.drop n)
  | [], [], .nil => by simp; exact .nil
  | _ :: _, _ :: _, .cons a b t => by
    cases n with
    | zero => exact .cons a b t
    | succ n => simpa using Rel.drop n t

theorem Rel.take {F st : DSt} {U : Nat} (n : Nat) : ∀ {σ : MSt} {π : List Bool}, Rel F st U σ π →
    Rel F st U (List.take n σ) (List.take n π)
  | [], [], .nil => by simp; exact .nil
  | _ :: _, _ :: _, .cons a b t => by
    cases n with
    | zero => simp; exact .nil
    | succ n => simpa using .cons a b (Rel.take n t)

theorem Rel.append {F st : DSt} {U : Nat} {σ1 σ2 : MSt} {π1 π2 : List Bool}
    (h1 : Rel F st U σ1 π1) (h2 : Rel F st U σ2 π2) : Rel F st U (σ1 ++ σ2) (π1 ++ π2) := by
  induction h1 with
  | nil => exact h2
  | cons a b _ ih => exact .cons a b ih

/-- What the path holds after a step the walk takes from `σ` to `σ'`, the
same variables: what the step moved or gave, as the walk says, and the
rest as the path had it. -/
def delta : MSt → MSt → List Bool → List Bool
  | v :: σ, w :: σ', p :: π => (if v.live == w.live then p else w.live) :: delta σ σ' π
  | _, _, _ => []

/-- A step of an expression keeps what `Rel` says: what it leaves is below
what it was given. -/
theorem Rel.step {F st st' : DSt} {U : Nat} (hs : ∀ u, u ∈ st.held → u ∈ st'.held) :
    ∀ {σ σ' : MSt} {π : List Bool}, Rel F st U σ π → σ'.Below σ → Rel F st' U σ' (delta σ σ' π)
  | [], [], [], .nil, .nil => .nil
  | v :: σ, w :: σ', p :: π, .cons a b t, .cons bw bt => by
    simp only [delta]
    refine .cons ?_ ?_ (Rel.step hs t bt)
    · intro hw
      have hv := bw.2.2.2.2.2.2.2.2 hw
      simp [hv, hw, a hv]
    · intro hp
      split at hp
      · next e =>
        rcases b hp with h | d
        · left; simp only [beq_iff_eq] at e; rw [← e]; exact h
        · right
          have u := bw.2.1; have o := bw.2.2.1
          exact ⟨o ▸ d.1, u ▸ d.2.1, u ▸ d.2.2.1, u ▸ d.2.2.2.imp (hs _) id⟩
      · exact .inl hp

/-- The path's state between two of the walk's, flag by flag: a return from
partway along an expression, which has made some of the moves the
expression makes and not the others. -/
inductive Between : List Bool → List Bool → List Bool → Prop where
  | nil : Between [] [] []
  | cons {lo hi k : Bool} {los his ks : List Bool} :
      (lo = true → k = true) → (k = true → hi = true) → Between los his ks →
      Between (lo :: los) (hi :: his) (k :: ks)


/-! ## What a path passes, and what it asks of the schedule -/

/-- A point a path passes: where it is in the schedule the function's walk
ends with, counted from the first scheduled, none where nothing was
scheduled there; for a return from partway along an expression, the point
of what the expression had moved by then, which drops only what the path
still holds; and the variables in the scopes the path leaves, each with
whether it holds a value on the path. -/
structure Ev where
  idx : Option Nat
  nym : Option Nat
  vars : List (MVar × Bool)

/-- What a scheduled point drops, by where it is in `F`. -/
def DSt.dropsAt (F : DSt) : Option Nat → List Nat
  | some i => match F.out.reverse[i]? with
    | some e => Entry.uids e
    | none => []
  | none => []

/-- What lowering drops at a point the path passes. -/
def Ev.drops (F : DSt) (ev : Ev) : List Nat :=
  F.dropsAt ev.idx ++ (F.dropsAt ev.nym).filter fun u => ev.vars.any fun (v, p) => v.uid == u && p

/-- A point is right on a path: every owned variable of the scopes it
leaves that holds a value is dropped, and everything dropped either holds
a value on the path or keeps a tracking byte, which drops nothing where it
holds nothing. -/
def Ev.Good (F : DSt) (ev : Ev) : Prop :=
  (∀ v p, (v, p) ∈ ev.vars → v.owns = true → p = true → v.uid ∈ ev.drops F) ∧
  (∀ u, u ∈ ev.drops F → (∃ v, (v, true) ∈ ev.vars ∧ v.uid = u) ∨ u ∈ F.tracked)

theorem Pairwise.getElem {R : Entry → Entry → Prop} : ∀ {as bs : List Entry}, Pairwise R as bs →
    ∀ (i : Nat) (h : i < as.length) (h' : i < bs.length), R as[i] bs[i]
  | [], [], .nil, _, h, _ => by simp at h
  | _ :: _, _ :: _, .cons r t, 0, _, _ => r
  | _ :: _, _ :: _, .cons _ t, i + 1, h, h' => Pairwise.getElem t i (by simpa using h) (by simpa using h')

/-- A point `st` has scheduled, counted from the first, is in the schedule
`st'` has, at the same place, become what `EntryExt` says. -/
theorem SchedExt.at {st st' : DSt} (x : SchedExt st st') (k : Nat) (hk : k < st.out.length) :
    ∃ e', st'.out.reverse[k]? = some e' ∧ EntryExt st st' e' (st.out.reverse[k]'(by simpa using hk)) := by
  obtain ⟨pre, mid, e, h⟩ := x.out
  have hl := Pairwise.length h
  have hk' : k < mid.length := hl ▸ hk
  refine ⟨mid.reverse[k]'(by simpa using hk'), ?_, ?_⟩
  · rw [e, List.reverse_append, List.getElem?_append_left (by simpa using hk')]
    exact List.getElem?_eq_getElem _
  · simp only [List.getElem_reverse]
    have := Pairwise.getElem h (mid.length - 1 - k) (by omega) (by omega)
    have e2 : st.out.length - 1 - k = mid.length - 1 - k := by omega
    simp only [e2]
    exact this

/-- A point just recorded is the last of what `st` scheduled. -/
theorem record_last {kind : String} {stmt : Nat} {arm : Option Nat} {drops : List Nat} {always : Bool}
    {st st' : DSt} (h : (record kind stmt arm drops always).run st = .ok ((), st'))
    (hr : always = true ∨ drops ≠ []) :
    st'.out.reverse[st.out.length]? = some (kind, stmt, arm, drops.map (·, none)) := by
  unfold record at h
  split at h
  · simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
    cases h
    simp
  · next c =>
    exfalso
    simp only [Bool.or_eq_true, Bool.not_eq_true', List.isEmpty_eq_false_iff, not_or] at c
    rcases hr with hr | hr
    · simp_all
    · simp_all

/-- How a path through a statement or a block ends: on to what follows,
with what it holds; at a `break` or a `continue`, with what it holds there;
or out of the function. -/
inductive POut where
  | norm (π : List Bool)
  | brk (π : List Bool)
  | cont (π : List Bool)
  | exit

/-- A return from partway along a step `leavingIf` scheduled, from `st` to
`st'`: the point it recorded last, the one before it if it recorded what
the step moved apart, and the variables, with what the path holds. -/
def tryEv (st st' : DSt) (σ : MSt) (πk : List Bool) : Ev :=
  { idx := some (st'.out.length - 1),
    nym := if st'.out.length = st.out.length + 2 then some st.out.length else none,
    vars := σ.zip πk }

/-- The point `record` added from `st` to `st'`, if it added one, leaving
the variables `vs`, with what the path holds. -/
def recEv (st st' : DSt) (vs : MSt) (πs : List Bool) : Ev :=
  { idx := if st'.out.length = st.out.length + 1 then some st.out.length else none,
    nym := none,
    vars := vs.zip πs }

/-- The flags of new bindings, put before what the path holds. -/
def newFlags (σ σ' : MSt) : List Bool := (scopeOwn σ σ').map (·.live)

end Datalove.Fun
