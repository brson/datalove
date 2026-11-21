import Datalove.Proofs.DropSpec

/-!
# What a loop's walk says of its passes

A path through a loop takes any number of passes, and the walk of the loop
looks at its body once. These are what the walk's checks and its record of
what the loop gives back say about every pass: a way back to the loop's
head keeps live what was live at the head (O007), a binding the loop was
entered without and gives a value is found perhaps held (`loopHeld`), and
the loop is left with a state as live as any of its ways out (O014).
-/

namespace Datalove.Fun

theorem mem_zipIdx_of_getElem? {α} {v : α} : ∀ {l : List α} {i k : Nat}, l[i]? = some v → (v, k + i) ∈ l.zipIdx k
  | [], _, _, h => by simp at h
  | x :: l, 0, k, h => by
    simp at h; subst h; simp [List.zipIdx_cons]
  | x :: l, i + 1, k, h => by
    simp only [List.getElem?_cons_succ] at h
    simp only [List.zipIdx_cons, List.mem_cons]
    right
    have := mem_zipIdx_of_getElem? (k := k + 1) h
    rwa [show k + 1 + i = k + (i + 1) by omega] at this

/-- `holdFrom` holds what it is given, and keeps held what was. -/
theorem holdFrom_held (u : Nat) (st : DSt) : u ∈ (holdFrom u st).held ∧ ∀ x, x ∈ st.held → x ∈ (holdFrom u st).held := by
  simp only [holdFrom]
  split
  · next h => exact ⟨by simpa using h, fun _ hx => hx⟩
  · exact ⟨.head _, fun _ hx => .tail _ hx⟩

/-- A step over each of a list that holds the binding of each element a
condition picks: every one picked is held at the end. -/
theorem forM_holds {α} (key : α → Nat) (cond : α → Bool) :
    ∀ (l : List α) (st st' : DSt),
      (l.forM fun a => if cond a then modify (holdFrom (key a)) else (pure () : MoveM Unit)).run st = .ok ((), st') →
      (∀ x, x ∈ st.held → x ∈ st'.held) ∧ ∀ a, a ∈ l → cond a = true → key a ∈ st'.held
  | [], st, st', h => by
    have e : (List.forM ([] : List α) fun a => if cond a then modify (holdFrom (key a))
      else (pure () : MoveM Unit)).run st = .ok ((), st) := rfl
    rw [e] at h; cases h
    exact ⟨fun _ h => h, fun _ h => by cases h⟩
  | a :: l, st, st', h => by
    have e : (a :: l).forM (fun a => if cond a then modify (holdFrom (key a)) else (pure () : MoveM Unit)) =
        ((if cond a then modify (holdFrom (key a)) else (pure () : MoveM Unit)) >>= fun _ =>
          l.forM fun a => if cond a then modify (holdFrom (key a)) else (pure () : MoveM Unit)) := rfl
    rw [e] at h
    obtain ⟨_, st1, h1, h2⟩ := MoveM.bind_ok h
    obtain ⟨m2, p2⟩ := forM_holds key cond l st1 st' h2
    have r1 : (∀ x, x ∈ st.held → x ∈ st1.held) ∧ (cond a = true → key a ∈ st1.held) := by
      by_cases c : cond a = true
      · simp only [c, ↓reduceIte] at h1
        simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h1
        cases h1
        exact ⟨(holdFrom_held _ _).2, fun _ => (holdFrom_held _ _).1⟩
      · simp only [c, ↓reduceIte, Bool.false_eq_true] at h1; cases h1; exact ⟨fun _ h => h, fun h => absurd h c⟩
    refine ⟨fun x hx => m2 x (r1.1 x hx), fun b hb hc => ?_⟩
    cases hb with
    | head => exact m2 _ (r1.2 hc)
    | tail _ hb => exact p2 b hb hc

/-- What `loopHeld` holds: each owned binding the loop was entered
without, at its place, that the body's end, where it gets there, or a
`continue` gives back. -/
theorem loopHeld_holds {σc : MSt} {body : List Stmt} {r : MOut} {st st' : DSt}
    (h : (loopHeld σc body r).run st = .ok ((), st')) :
    (∀ x, x ∈ st.held → x ∈ st'.held) ∧
    ∀ (i : Nat) (v : MVar), σc[i]? = some v → v.live = false → v.owns = true →
      ((completes body && ((σc.back r.1)[i]?).any (·.live)) ||
        r.2.2.any fun τ => ((σc.back τ)[i]?).any (·.live)) = true → v.uid ∈ st'.held := by
  unfold loopHeld at h
  obtain ⟨m, p⟩ := forM_holds (fun vi : MVar × Nat => vi.1.uid)
    (fun vi => !vi.1.live && vi.1.owns && ((completes body && ((σc.back r.1)[vi.2]?).any (·.live)) ||
      r.2.2.any fun τ => ((σc.back τ)[vi.2]?).any (·.live))) σc.zipIdx st st' h
  refine ⟨m, fun i v hv hl ho hc => ?_⟩
  have hm := mem_zipIdx_of_getElem? (k := 0) hv
  simp only [Nat.zero_add] at hm
  exact p (v, i) hm (by simp [hl, ho, hc])


/-- What a path holds where the walk's state is the same, as a frame says
it may diverge where another does. -/
theorem Rel.conv {F st st' : DSt} {H H' : Nat → Prop} : ∀ {σ : MSt} {π : List Bool}, Rel F st H σ π →
    (∀ v, v ∈ σ → v.owns = true → (v.uid ∈ st.held ∨ H v.uid) → v.uid ∈ st'.held ∨ H' v.uid) →
    Rel F st' H' σ π
  | [], [], .nil, _ => .nil
  | _ :: _, _ :: _, .cons a b t, hc =>
    .cons a (fun hp ho => (b hp ho).imp id fun d => ⟨d.1, d.2.1, d.2.2.1, hc _ (.head _) d.1 d.2.2.2⟩)
      (Rel.conv t fun v hv => hc v (.tail _ hv))

/-- What a way back to a loop's start leaves `τ`, as the path holds it, is
what the path holds at the start, `σ`: what `σ` has live `τ` keeps live
(O007), and a binding `σ` and what the condition left, `σc`, have moved and
`τ` gives a value is perhaps held (`loopHeld`), in `K`. -/
theorem Rel.back {F st st' : DSt} {H H' : Nat → Prop} {K : List Nat}
    (hK : ∀ u, u ∈ K → u ∈ F.held ∧ u ∈ F.tracked ∧ (u ∈ st'.held ∨ H' u))
    (hc : ∀ u, (u ∈ st.held ∨ H u) → u ∈ st'.held ∨ H' u) :
    ∀ {σ σc τ : MSt} {π : List Bool}, Rel F st H τ π → MSt.Ids τ σ → σc.Below σ → MSt.keepsLive σ τ = true →
      (∀ (i : Nat) (v w : MVar), σc[i]? = some v → τ[i]? = some w → v.live = false → v.owns = true → w.live = true →
        v.uid ∈ K) →
      Rel F st' H' σ π
  | [], [], [], [], .nil, .nil, .nil, _, _ => .nil
  | v :: σ, c :: σc, w :: τ, p :: π, .cons a b t, .cons u o ids, .cons bc tb, hk, hl => by
    simp only [MSt.keepsLive, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, Bool.or_eq_true,
      Bool.not_eq_true'] at hk
    have hk' : MSt.keepsLive σ τ = true := by simpa [MSt.keepsLive] using hk.2
    refine .cons (fun hv => a (hk.1.resolve_left (by simp [hv]))) (fun hp ho => ?_)
      (Rel.back hK hc t ids tb hk' fun i v w h1 h2 => hl (i + 1) v w (by simpa using h1) (by simpa using h2))
    by_cases hv : v.live = true
    · exact .inl hv
    · right
      have hcl : c.live = false := by
        cases e : c.live
        · rfl
        · exact absurd (bc.2.2.2.2.2.2.2.2 e) hv
      have hco : c.owns = true := by rw [bc.2.2.1]; exact ho
      rcases b hp (o ▸ ho) with hw | d
      · have hu := hl 0 c w rfl rfl hcl hco hw
        rw [bc.2.1] at hu
        obtain ⟨h1, h2, h3⟩ := hK _ hu
        exact ⟨ho, h1, h2, h3⟩
      · exact ⟨ho, u ▸ d.2.1, u ▸ d.2.2.1, u ▸ hc _ d.2.2.2⟩

theorem MSt.Same.of_ids : ∀ {a b : MSt}, MSt.Ids a b → a.Same b
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons u o t => .cons u o (MSt.Same.of_ids t)

/-- What a loop's ways out agree on is as live as each. -/
theorem joinAll_live {code : Lit.Code} {σ r : MSt} : ∀ {L : List MSt} {st st' : DSt},
    (joinAll code σ L).run st = .ok (r, st') → (∀ a b, a ∈ L → b ∈ L → a.Same b) →
    ∀ τ, τ ∈ L → r.SameLive τ
  | [], _, _, _, _, _, h => by cases h
  | [_], _, _, h, _, _, hτ => by
    cases h; simp only [List.mem_singleton] at hτ; subst hτ; exact MSt.SameLive.refl _
  | τ :: w :: L, st, st', h, hs, υ, hυ => by
    rw [joinAll_cons_cons] at h
    unfold MSt.join at h
    split at h
    · next j hj =>
      split at hj
      · next c =>
        cases hj
        have tw : τ.Same w := hs τ w (.head _) (.tail _ (.head _))
        have jl : (τ.initMeet w).SameLive τ := sameLive_initMeet tw.length
        have hs' : ∀ a b, a ∈ τ.initMeet w :: L → b ∈ τ.initMeet w :: L → a.Same b := by
          intro a b ha hb
          have up : ∀ x, x ∈ τ.initMeet w :: L → ∃ y, y ∈ τ :: w :: L ∧ x.Same y := by
            intro x hx
            cases hx with
            | head => exact ⟨τ, .head _, jl.same⟩
            | tail _ hx => exact ⟨x, .tail _ (.tail _ hx), MSt.Same.refl _⟩
          obtain ⟨a', ha', sa⟩ := up a ha
          obtain ⟨b', hb', sb⟩ := up b hb
          exact sa.trans ((hs a' b' ha' hb').trans sb.symm)
        have ih := joinAll_live h hs'
        cases hυ with
        | head => exact (ih _ (.head _)).trans jl
        | tail _ hυ =>
          cases hυ with
          | head => exact ((ih _ (.head _)).trans jl).trans (sameMoves_sameLive tw c)
          | tail _ hυ => exact ih υ (.tail _ hυ)
      · cases hj
    · cases h
termination_by L => L.length

/-- What a loop's end checks: each way back keeps live what the start had
live (O007), and what it is left with is as live as each way out (O014). -/
theorem loopEnd_spec {σ σc r0 : MSt} {natural : Bool} {body : List Stmt} {r : MOut} {st st' : DSt}
    (h : (loopEnd σ σc natural body r).run st = .ok (r0, st'))
    (hs : ∀ τ, τ ∈ r.2.1 → (σc.back τ).Same σc) :
    (completes body = true → MSt.keepsLive σ (σc.back r.1) = true) ∧
    (∀ τ, τ ∈ r.2.2 → MSt.keepsLive σ (σc.back τ) = true) ∧
    (natural = true → r0.SameLive σc) ∧ (∀ τ, τ ∈ r.2.1 → r0.SameLive (σc.back τ)) := by
  unfold loopEnd at h
  dsimp only at h
  have c : (((if completes body then [σc.back r.1] else []) ++ r.2.2.map σc.back).all (MSt.keepsLive σ)) = true := by
    cases hn : (((if completes body then [σc.back r.1] else []) ++ r.2.2.map σc.back).all (MSt.keepsLive σ))
    · rw [hn] at h; cases h
    · rfl
  have h := MoveM.ite_throw h
  simp only [List.all_eq_true, List.mem_append, List.mem_map] at c
  have hs' : ∀ a b, a ∈ (if natural then [σc] else []) ++ r.2.1.map σc.back →
      b ∈ (if natural then [σc] else []) ++ r.2.1.map σc.back → a.Same b := by
    have toC : ∀ a, a ∈ (if natural then [σc] else []) ++ r.2.1.map σc.back → a.Same σc := by
      intro a ha
      simp only [List.mem_append, List.mem_map] at ha
      rcases ha with ha | ⟨τ, hτ, rfl⟩
      · split at ha
        · simp only [List.mem_singleton] at ha; subst ha; exact MSt.Same.refl _
        · cases ha
      · exact hs τ hτ
    intro a b ha hb
    exact (toC a ha).trans (toC b hb).symm
  have jl := joinAll_live h hs'
  refine ⟨fun hc => c _ (.inl (by simp [hc])), fun τ hτ => c _ (.inr ⟨τ, hτ, rfl⟩),
    fun hn => jl σc (by simp [hn]), fun τ hτ => jl _ (by simp; exact .inr ⟨τ, hτ, rfl⟩)⟩

/-- The static facts of an invariant, kept by a state of the same bindings
the path holds as `rel` says. -/
theorem Inv.reframe {F st st' : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ X : MSt} {π0 π : List Bool}
    (h : Inv F st U H base σ π0) (x : SchedExt st st') (s : X.Same σ) (rel : Rel F st' H X π) :
    Inv F st' U H base X π :=
  let h' := h.same x s
  ⟨rel, h'.hU, h'.made, h'.scope, h'.lt, h'.ule, h'.base⟩

end Datalove.Fun
