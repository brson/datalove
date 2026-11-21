import Datalove.Proofs.DropProof

/-!
# A function's drop schedule is right on every path through it

The last step of the drop schedule's proof (`botdocs/plan-drop-proof.md`):
for a function the move analysis accepts, every point a path through its
body passes, and the end of the body where the path gets there, drops each
owned binding that holds a value on the path, and nothing else but what
keeps a tracking byte (`Ev.Good`), against the schedule the function's walk
ends with.
-/

namespace Datalove.Fun

/-- A parameter's binding, as `mFun` makes it: given a value but an `out`
one, a `ref`, `mut` or `out` one borrowed, and a `mut` or `out` one read
back by the caller. -/
def paramStep (Γ : Ctx) : MSt → Mode × String × Hint → MoveM MSt := fun σ (m, x, _) => do
  let σ ← bindOne Γ false (m != .out) (m == .ref || m == .mut || m == .out) (m == .mut || m == .out)
    (m == .ref) σ x
  pure (match σ with
    | v :: rest => { v with back := m == .out || m == .mut } :: rest
    | [] => [])

/-- The consts a function sees, as `mFun` puts them under its parameters:
each owning nothing. -/
def constVars (cmarks : List (String × Bool)) : MSt :=
  cmarks.map fun (x, s) =>
    { name := x, init := s, live := true, back := false, lent := false, writable := false, byRef := false }

/-- Where each binding was made is no later than the statement the walk is
at. -/
def MadeNext (st : DSt) : Prop := ∀ u m, st.madeAt.lookup u = some m → m ≤ st.next

theorem paramStep_spec (Γ : Ctx) (p : Mode × String × Hint) {σ σ' : MSt} {st st' : DSt} (w : st.WF)
    (mn : MadeNext st) (md : Made σ st) (h : (paramStep Γ σ p).run st = .ok (σ', st')) :
    SchedExt st st' ∧ MadeNext st' ∧ Made σ' st' := by
  obtain ⟨m, x, _⟩ := p
  simp only [paramStep] at h
  obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
  have x1 := SchedGrows.bindOne Γ false _ _ _ _ σ x st σ1 st1 w h1
  have m1 := bindOne_made h1 md
  have mn1 : MadeNext st1 := by
    simp only [bindOne, MoveM.run_bind] at h1
    cases h1
    intro u k hk
    simp only [List.lookup] at hk
    split at hk
    · cases hk; exact Nat.le_refl _
    · exact mn u k hk
  simp only [MoveM.run_pure, Except.ok.injEq, Prod.mk.injEq] at h2
  obtain ⟨rfl, rfl⟩ := h2
  refine ⟨x1, mn1, ?_⟩
  split
  · rename_i v rest
    exact Made.cons (fun ho => m1 v (.head _) ho) (fun u hu ho => m1 u (.tail _ hu) ho)
  · exact fun _ h => by cases h

theorem params_spec (Γ : Ctx) : ∀ (ps : List (Mode × String × Hint)) {σ σ' : MSt} {st st' : DSt}, st.WF →
    MadeNext st → Made σ st → (ps.foldlM (paramStep Γ) σ).run st = .ok (σ', st') →
    SchedExt st st' ∧ MadeNext st' ∧ Made σ' st'
  | [], _, _, _, _, w, mn, md, h => by cases h; exact ⟨SchedExt.refl w, mn, md⟩
  | p :: ps, σ, σ', st, st', w, mn, md, h => by
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    obtain ⟨x1, mn1, md1⟩ := paramStep_spec Γ p w mn md h1
    obtain ⟨x2, mn2, md2⟩ := params_spec Γ ps x1.wf mn1 md1 h2
    exact ⟨x1.trans x2, mn2, md2⟩

theorem empty_wf : (({} : DSt)).WF :=
  ⟨(fun _ _ h => by cases h), (fun _ h => by cases h), (fun _ h => by cases h), (fun _ h => by cases h)⟩

/-- Where a function's body starts, the path holds every binding, none of
which is perhaps held. -/
theorem Inv.start {F st : DSt} {σ : MSt} (w : st.WF) (mn : MadeNext st) (md : Made σ st) :
    Inv F st 0 (fun _ => False) 0 σ (σ.map (·.live)) := by
  have rel : ∀ (l : MSt), Rel F st (fun _ => False) l (l.map (·.live)) := by
    intro l
    induction l with
    | nil => exact .nil
    | cons v l ih => exact .cons (fun hv => hv) (fun hp _ => .inl hp) ih
  refine ⟨rel σ, fun _ h => h.elim, fun v hv ho => ?_, fun _ _ _ => Nat.zero_le _, md, Nat.zero_le _,
    Nat.zero_le _⟩
  obtain ⟨m, hm⟩ := w.made v.uid (md v hv ho)
  exact ⟨m, hm, mn _ _ hm⟩

theorem discard_exitM {σ : MSt} {st st' : DSt} {u : Unit} (h : (discard (exitM σ)).run st = .ok (u, st')) :
    st' = st := by
  unfold exitM at h
  split at h
  · cases h; rfl
  · cases h

/-- The drop schedule of a function the move analysis accepts is right on
every path through its body: at every point the path passes, and at the
body's end where the path gets there. -/
theorem fun_ok {sigs : List (String × Sig)} {env : InferEnv} {aliases : Aliases}
    {consts : List (String × Ty × Kind)} {cmarks : List (String × Bool)} {name : String}
    {tps : List (String × Option Bound)} {params : List (Mode × String × Hint)} {rt : Option Hint}
    {body : List Stmt} {sig : Sig} {F : DSt} (hs : sigs.lookup name = some sig)
    (hrun : (mFun sigs env aliases consts cmarks (.mk name tps params rt body)).run {} = .ok ((), F)) :
    ∃ σ0 st0, (params.foldlM (paramStep (funCtx sigs aliases consts tps params sig)) (constVars cmarks)).run {}
        = .ok (σ0, st0) ∧
      ∀ out evs, PBlock sigs env fuel (funCtx sigs aliases consts tps params sig) σ0 0 body st0
          (σ0.map (·.live)) out evs →
        (∀ ev, ev ∈ evs → ev.Good F) ∧
        ∀ πe, out = .norm πe → ∃ r st1,
          (mBlock sigs env fuel (funCtx sigs aliases consts tps params sig) σ0 0 body).run st0 = .ok (r, st1) ∧
          (recEv st1 F (scopeOwn (constVars cmarks) r.1)
            (πe.take (πe.length - (constVars cmarks).length))).Good F := by
  unfold mFun at hrun
  simp only [hs] at hrun
  obtain ⟨σ0, st0, h0, hrun⟩ := MoveM.bind_ok hrun
  obtain ⟨r, st1, hb, hrun⟩ := MoveM.bind_ok hrun
  obtain ⟨σ, b1, b2⟩ := r
  dsimp only at hrun
  obtain ⟨_, st2, he, hrun2⟩ := MoveM.bind_ok hrun
  have e2 : st2 = st1 := by
    split at he
    · exact discard_exitM he
    · cases he; rfl
  subst e2
  obtain ⟨ds, st3, ho, hrun3⟩ := MoveM.bind_ok hrun2
  obtain ⟨rfl, rfl⟩ := ownedIn_run ho
  have md0 : Made (constVars cmarks) ({} : DSt) := by
    intro v hv ho
    simp only [constVars, List.mem_map] at hv
    obtain ⟨_, _, rfl⟩ := hv
    cases ho
  obtain ⟨x0, mn0, mdσ⟩ := params_spec _ params empty_wf (fun _ _ h => by cases h) md0 h0
  refine ⟨σ0, st0, h0, fun out evs hp => ?_⟩
  have inv := Inv.start (F := F) x0.wf mn0 mdσ
  obtain ⟨x1, _⟩ := (walk_ext sigs env _).2.1 _ _ _ _ _ _ _ x0.wf mdσ hb
  have xr := SchedGrows.record _ _ _ _ _ _ _ _ x1.wf hrun3
  obtain ⟨g, o⟩ := block_ok sigs env hp hb x0.wf inv xr
  refine ⟨g, fun πe hπe => ⟨_, _, hb, ?_⟩⟩
  subst hπe
  have hl : πe.length = σ.length := o.rel.length
  rw [hl]
  exact recEv_good hrun3 (SchedExt.refl xr.wf) x1.wf (o.rel.take _) (.inr ⟨fun _ h => h, fun _ _ _ h => h⟩)

end Datalove.Fun
