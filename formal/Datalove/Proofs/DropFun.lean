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

/-! ## A `set`'s store

Lowering destroys what a `set`'s target holds before the store writes over
it. A tracked target asks its byte; a precise one is destroyed unless the
analysis found the `set` writing over a moved value (`noteOverMoved`, the
Rust's `set_over_moved`). That is right just when, on every path, a precise
owned binding holds a value where the walk has it live. -/

/-- An owned binding with no tracking byte holds a value on the path just
where the walk has it live. -/
theorem Rel.precise {F st : DSt} {H : Nat → Prop} : ∀ {σ : MSt} {π : List Bool}, Rel F st H σ π →
    ∀ {v : MVar} {p : Bool}, (v, p) ∈ σ.zip π → v.owns = true → v.uid ∉ F.tracked → p = v.live := by
  intro σ π h v p hv ho ht
  obtain ⟨a, b⟩ := h.mem_zip hv
  cases hp : p
  · cases hl : v.live
    · rfl
    · exact absurd (a hl) (by simp [hp])
  · rcases b hp ho with hl | d
    · exact hl.symm
    · exact absurd d.2.2.1 ht

theorem noteOverMoved_spec {id : Nat} {σ : MSt} {x : String} {steps : List Step} {st st' : DSt}
    (h : (noteOverMoved id σ x steps).run st = .ok ((), st')) :
    st'.overMoved = if steps.isEmpty && (σ.find x).any (!·.live) then id :: st.overMoved else st.overMoved := by
  unfold noteOverMoved at h
  split at h
  · next c =>
    simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
    cases h; simp [c]
  · next c =>
    cases h; simp [c]

/-- Where a `set`'s store writes, the path's state is related to the walk's
there, `σ2`, from which the walk notes the `set` as over moved: so each owned
binding without a tracking byte, the target among them, holds a value on the
path just where the walk has it live, and a precise target holds nothing just
where the walk notes the `set`. -/
theorem set_store_ok {sigs : List (String × Sig)} {Γ : Ctx} {id : Nat} {σ σ' : MSt} {x : String}
    {steps : List Step} {op : Option BinOp} {e : Expr} {st st' F : DSt} {U base : Nat} {H : Nat → Prop}
    {π : List Bool} (inv : Inv F st U H base σ π) (w : st.WF)
    (hs : (mSet sigs Γ id σ x steps op e).run st = .ok (σ', st')) :
    ∃ σ2 st3 st3n, (noteOverMoved id σ2 x steps).run st3 = .ok ((), st3n) ∧
      Rel F st3 H σ2 (delta σ σ2 π) ∧
      ∀ v p, (v, p) ∈ σ2.zip (delta σ σ2 π) → v.owns = true → v.uid ∉ F.tracked → p = v.live := by
  obtain ⟨sta, stb, σ1, st2, σ2, st3, st3n, ha, hb, h1, h2, hn, h3, _⟩ := mSet_decomp hs
  have xa := SchedGrows.keepsSched (KeepsSched.unwalkedM σ _) _ _ _ w ha
  have xb := SchedGrows.keepsSched (KeepsSched.writableM σ _) _ _ _ xa.wf hb
  have x2 := SchedGrows.leavingIf _ σ _ (SchedGrows.shr (mSetParts_shr sigs Γ σ _ _ _)) _ _ _ xb.wf h1
  have b1 := leavingIf_below (mSetParts_shr sigs Γ σ _ _ _) h1
  have x3 := SchedGrows.shr (mTarget_shr _ _ _ _) _ _ _ x2.wf h2
  have b2 := ((mTarget_shr _ _ _ _) _ _ _ h2).1
  have inv3 := inv.same (xa.trans (xb.trans (x2.trans x3))) ((MSt.Same.of_below b2).trans (MSt.Same.of_below b1))
  exact ⟨σ2, st3, st3n, hn, inv3.rel, fun v p hv ho ht => inv3.rel.precise hv ho ht⟩

end Datalove.Fun
