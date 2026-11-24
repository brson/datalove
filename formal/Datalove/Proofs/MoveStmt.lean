import Datalove.Proofs.MoveMono

/-!
# A script the move analysis lets through never reads what was moved: statements

The second half of `Fun.runScriptMoves_move`: statements and blocks, run
by `execStmt` and `execBlock` with the run that moves (`movesEnv`). Where
the move analysis lets a statement through, its run never reads a variable
holding nothing, stops at a `break` or `continue` only with variables that
line up with a state the analysis followed to that `break` or `continue`,
and returns only with every `mut` and `out` parameter holding a value.

A loop is where moves need more than an expression does. The analysis
follows the body once, from what the condition left, and refuses a way back
to the loop's start that moved anything live at the start (O007). Every
way back un-gives nothing (`MSt.Le`), so what lines up with the end of a
pass lines up with its start again (`MAg.le`), and the next pass is the
first.
-/

namespace Datalove.Fun

open Lit (Val)

/-! ## What a statement may do -/

/-- How a statement run may stop where the analysis let it through: never at
a variable that holds nothing; returning only with every `mut` and `out`
parameter holding a value; at a `break` only where `B` says it has one, with
the variables as one of its `break`s the analysis followed has them; and at
a `continue` with the variables as one of its `continue`s has them. -/
def MSStop (O : List (String × Bool)) (brks conts : List MSt) (B : Prop) : Stop → Prop
  | .uninit | .early _ => False
  | .returned _ ρ' => MExitOk O ρ'
  | .brk ρ' => B ∧ ∃ b ∈ brks, MAg b ρ'
  | .cont ρ' => ∃ c ∈ conts, MAg c ρ'
  | _ => True

/-- What a statement run came to: variables lining up with what the analysis
has after it, and `C`; or a stop as `MSStop` says. -/
def MSRes (O : List (String × Bool)) (out : MOut) (B C : Prop) : EvalM VEnv → Prop
  | .ok ρ' => MAg out.1 ρ' ∧ C
  | .error s => MSStop O out.2.1 out.2.2 B s

theorem MSStop.of_MStop {O : List (String × Bool)} {brks conts : List MSt} {B : Prop} {s : Stop}
    (h : MStop O s) : MSStop O brks conts B s := by
  cases s <;> first | exact h | exact h.elim | trivial

theorem MSStop.weaken {O : List (String × Bool)} {brks brks' conts conts' : List MSt} {B B' : Prop} {s : Stop}
    (h : MSStop O brks conts B s) (hb : B → B') (hl : ∀ b ∈ brks, b ∈ brks') (hc : ∀ c ∈ conts, c ∈ conts') :
    MSStop O brks' conts' B' s := by
  cases s with
  | brk ρ' => obtain ⟨h1, b, hb', h3⟩ := h; exact ⟨hb h1, b, hl b hb', h3⟩
  | cont ρ' => obtain ⟨c, hc', h3⟩ := h; exact ⟨c, hc c hc', h3⟩
  | _ => exact h

/-- An expression run as part of a statement. -/
theorem MSRes.ofRes {O : List (String × Bool)} {σ1 : MSt} {α} {P : α → VEnv → Prop}
    {r : Except Stop (α × VEnv)} {f : α × VEnv → EvalM VEnv} {out : MOut} {B C : Prop}
    (h : MRes O σ1 P r) (hf : ∀ a ρ1, P a ρ1 → MAg σ1 ρ1 → MSRes O out B C (f (a, ρ1))) :
    MSRes O out B C (r >>= f) := by
  cases r with
  | error s => exact MSStop.of_MStop h
  | ok p =>
    obtain ⟨a, ρ1⟩ := p
    obtain ⟨h1, hp⟩ := h
    exact hf a ρ1 hp h1

theorem MSRes.weaken {O : List (String × Bool)} {out out' : MOut} {B B' C C' : Prop} {m : EvalM VEnv}
    (h : MSRes O out B C m) (h1 : ∀ ρ', MAg out.1 ρ' → MAg out'.1 ρ') (hb : B → B') (hc : C → C')
    (hl : ∀ b ∈ out.2.1, b ∈ out'.2.1) (hk : ∀ c ∈ out.2.2, c ∈ out'.2.2) : MSRes O out' B' C' m := by
  cases m with
  | ok ρ' => exact ⟨h1 ρ' h.1, hc h.2⟩
  | error s => exact MSStop.weaken h hb hl hk

/-! ## Loops -/

/-- A loop's passes, each with its condition first, where the condition and
the body run from what the loop was entered with as the analysis says, and
every way back to the start lines up with the start again. -/
theorem execLoop_move {O : List (String × Bool)} {σ0 σc τ : MSt} {bs cs : List MSt} {W : VEnv → Prop}
    {C Cb Bb : Prop} {cond : VEnv → EvalM (Bool × VEnv)} {body : VEnv → EvalM VEnv}
    (hc : ∀ ρ, MAg σ0 ρ → match cond ρ with
      | .ok (b, ρ') => MAg σc ρ' ∧ (b = false → W ρ' ∧ C)
      | .error s => MStop O s)
    (hb : ∀ ρ, MAg σc ρ → MSRes O (τ, bs, cs) Bb Cb (body ρ))
    (hbrk : ∀ ρ ρ', MAg σc ρ → ∀ b ∈ bs, MAg b ρ' → Bb → W (restore ρ ρ') ∧ C)
    (hback : ∀ ρ ρ', MAg σc ρ → ((Cb ∧ MAg τ ρ') ∨ ∃ c ∈ cs, MAg c ρ') → MAg σ0 (restore ρ ρ')) :
    ∀ (k : Nat) (ρ : VEnv), MAg σ0 ρ → match execLoop cond body k ρ with
      | .ok ρ' => W ρ' ∧ C
      | .error s => MSStop O [] [] False s
  | 0, ρ, _ => by simp only [execLoop]; trivial
  | k + 1, ρ, hρ => by
    simp only [execLoop]
    have h1 := hc ρ hρ
    generalize cond ρ = r at h1 ⊢
    cases r with
    | error s => exact MSStop.of_MStop h1
    | ok p =>
      obtain ⟨b, ρ1⟩ := p
      obtain ⟨hρ1, hw⟩ := h1
      simp only [bind, Except.bind]
      cases b with
      | false => exact hw rfl
      | true =>
        simp only [Bool.not_true, Bool.false_eq_true, ↓reduceIte]
        have h2 := hb ρ1 hρ1
        generalize body ρ1 = r2 at h2 ⊢
        have next : ∀ ρ2, MAg σ0 (restore ρ1 ρ2) → match execLoop cond body k (restore ρ1 ρ2) with
            | .ok ρ' => W ρ' ∧ C
            | .error s => MSStop O [] [] False s :=
          fun ρ2 h0 => execLoop_move hc hb hbrk hback k (restore ρ1 ρ2) h0
        cases r2 with
        | ok ρ2 => exact next ρ2 (hback ρ1 ρ2 hρ1 (.inl ⟨h2.2, h2.1⟩))
        | error s =>
          cases s with
          | brk ρ2 =>
            obtain ⟨hB, b, hbm, hb2⟩ := h2
            exact hbrk ρ1 ρ2 hρ1 b hbm hb2 hB
          | cont ρ2 =>
            obtain ⟨c, hcm, hc2⟩ := h2
            exact next ρ2 (hback ρ1 ρ2 hρ1 (.inr ⟨c, hcm, hc2⟩))
          | returned o ρ2 => exact h2
          | uninit => exact h2.elim
          | early r => exact h2.elim
          | _ => trivial

/-- A `for`'s passes, one for each item, its names bound to the item's
values. -/
theorem execFor_move {O : List (String × Bool)} {σc τ : MSt} {bs cs : List MSt} {W : VEnv → Prop} {Cb Bb : Prop}
    {body : VEnv → EvalM VEnv} {names : List String}
    (hb : ∀ ρ item, item.length = names.length → MAg σc ρ →
      MSRes O (τ, bs, cs) Bb Cb (body (bindVals ρ ((names.zip item).map fun (x, w) => (x, some w)))))
    (hend : ∀ ρ, MAg σc ρ → W ρ)
    (hbrk : ∀ ρ ρ', MAg σc ρ → ∀ b ∈ bs, MAg b ρ' → W (restore ρ ρ'))
    (hback : ∀ ρ ρ', MAg σc ρ → ((Cb ∧ MAg τ ρ') ∨ ∃ c ∈ cs, MAg c ρ') → MAg σc (restore ρ ρ')) :
    ∀ (items : List (List Val)) (ρ : VEnv), (∀ item ∈ items, item.length = names.length) → MAg σc ρ →
    match execFor body names items ρ with
      | .ok ρ' => W ρ'
      | .error s => MSStop O [] [] False s
  | [], ρ, _, hρ => by simp only [execFor]; exact hend ρ hρ
  | item :: items, ρ, hlen, hρ => by
    simp only [execFor]
    have hlen1 := hlen item List.mem_cons_self
    have h2 := hb ρ item hlen1 hρ
    generalize body (bindVals ρ ((names.zip item).map fun (x, w) => (x, some w))) = r2 at h2 ⊢
    have next : ∀ ρ2, MAg σc (restore ρ ρ2) → match execFor body names items (restore ρ ρ2) with
        | .ok ρ' => W ρ'
        | .error s => MSStop O [] [] False s :=
      fun ρ2 h0 => execFor_move hb hend hbrk hback items (restore ρ ρ2)
        (fun i hi => hlen i (List.mem_cons_of_mem _ hi)) h0
    cases r2 with
    | ok ρ2 => exact next ρ2 (hback ρ ρ2 hρ (.inl ⟨h2.2, h2.1⟩))
    | error s =>
      cases s with
      | brk ρ2 =>
        obtain ⟨-, b, hbm, hb2⟩ := h2
        exact hbrk ρ ρ2 hρ b hbm hb2
      | cont ρ2 =>
        obtain ⟨c, hcm, hc2⟩ := h2
        exact next ρ2 (hback ρ ρ2 hρ (.inr ⟨c, hcm, hc2⟩))
      | returned o ρ2 => exact h2
      | uninit => exact h2.elim
      | early r => exact h2.elim
      | _ => trivial

/-! ## The analysis's steps around a statement -/

/-- What a `leavingIf` came to: its walk's own run, from a state with the
flag as it was given, to one with the flag as it leaves it. -/
theorem leavingIf_run {id : Nat} {σ σ' : MSt} {c : Bool} {m : MoveM MSt} {st st' : DSt}
    (h : (leavingIf id σ c m).run st = .ok (σ', st')) :
    ∃ st1 st2, m.run st1 = .ok (σ', st2) ∧ st1.movedHeld = st.movedHeld ∧ st2.movedHeld = st'.movedHeld := by
  unfold leavingIf at h
  rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
  · replace h := MoveM.bind_ok h
    obtain ⟨_, st1, h1, h⟩ := h
    have f1 : st1.movedHeld = st.movedHeld := by
      simp only [ownedIn, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, StateT.run, Pure.pure,
        StateT.pure] at h1
      cases h1; rfl
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st2, h2, h⟩ := h
    replace h := MoveM.bind_ok h
    obtain ⟨_, st3, h3, h⟩ := h
    have f3 : st3.movedHeld = st2.movedHeld := by
      simp only [ownedIn, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, StateT.run, Pure.pure,
        StateT.pure] at h3
      cases h3; rfl
    replace h := MoveM.bind_ok h
    obtain ⟨_, st4, h4, h⟩ := h
    have f4 : st4.movedHeld = st3.movedHeld := by
      simp only [recordNotYetMoved, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, StateT.run] at h4
      split at h4
      · simp only [set, MonadStateOf.set, StateT.set] at h4; cases h4; rfl
      · simp only [Pure.pure, StateT.pure] at h4; cases h4; rfl
    replace h := MoveM.bind_ok h
    obtain ⟨_, st5, h5, h⟩ := h
    have f5 : st5.movedHeld = st4.movedHeld := by
      unfold record at h5
      split at h5
      · have := MoveM.modify_run h5; subst this; rfl
      · cases h5; rfl
    obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
    exact ⟨st1, st2, h2, f1, (f5.trans (f4.trans f3)).symm⟩
  · exact ⟨st, st', h, rfl, rfl⟩

theorem leaving_run {id : Nat} {σ σ' : MSt} {e : Expr} {m : MoveM MSt} {st st' : DSt}
    (h : (leaving id σ e m).run st = .ok (σ', st')) :
    ∃ st1 st2, m.run st1 = .ok (σ', st2) ∧ st1.movedHeld = st.movedHeld ∧ st2.movedHeld = st'.movedHeld :=
  leavingIf_run h

/-- Names bound in front, as `bindM` binds them, line up with values bound in
front as `bindVals` binds them, each holding one where the analysis says. -/
theorem bindM_mag (Γ' : Ctx) (init w : Bool) : ∀ (xs : List String) (σ σ' : MSt) (st st' : DSt) (ρ : VEnv)
    (vals : List (String × Option Val)), (bindM Γ' σ xs init w).run st = .ok (σ', st') → MAg σ ρ →
    vals.map (·.1) = xs → (init = true → ∀ q ∈ vals, q.2.isSome = true) → MAg σ' (bindVals ρ vals)
  | [], σ, σ', st, st', ρ, vals, h, hρ, hn, _ => by
    simp only [bindM, List.foldlM] at h
    cases h
    have : vals = [] := List.map_eq_nil_iff.mp hn
    subst this
    exact hρ
  | x :: xs, σ, σ', st, st', ρ, [], h, hρ, hn, _ => by simp at hn
  | x :: xs, σ, σ', st, st', ρ, (y, o) :: vals, h, hρ, hn, hv => by
    simp only [bindM, List.foldlM] at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h2⟩ := h
    obtain ⟨v, rfl, hname, hinit, hlive, -⟩ := bindOne_run h1
    simp only [List.map_cons, List.cons.injEq] at hn
    show MAg σ' (bindVals ((y, o) :: ρ) vals)
    refine bindM_mag Γ' init w xs _ σ' st1 st' ((y, o) :: ρ) vals h2 ?_ hn.2
      (fun hi q hq => hv hi q (List.mem_cons_of_mem _ hq))
    refine .cons (hname.trans hn.1.symm) (fun hil => ?_) hρ
    rw [hinit, hlive, Bool.and_true] at hil
    exact hv hil (y, o) List.mem_cons_self

/-- A `for`'s names bound in front of what the walk freezes, lining up with
the item's values. -/
theorem forBinds_mag (Γf : Ctx) : ∀ (ps : List (String × Bool)) (σ σ' : MSt) (st st' : DSt) (ρ : VEnv)
    (vals : List (String × Option Val)),
    (ps.foldlM (fun σ (x, r) => bindOne Γf true true false r σ x) σ).run st = .ok (σ', st') → MAg σ ρ →
    vals.map (·.1) = ps.map (·.1) → (∀ q ∈ vals, q.2.isSome = true) → MAg σ' (bindVals ρ vals)
  | [], σ, σ', st, st', ρ, vals, h, hρ, hn, _ => by
    simp only [List.foldlM] at h
    cases h
    have : vals = [] := by simpa using hn
    subst this
    exact hρ
  | _ :: _, σ, σ', st, st', ρ, [], h, hρ, hn, _ => by simp at hn
  | (x, r) :: ps, σ, σ', st, st', ρ, (y, o) :: vals, h, hρ, hn, hv => by
    simp only [List.foldlM] at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h2⟩ := h
    obtain ⟨v, rfl, hname, hinit, hlive, -⟩ := bindOne_run h1
    simp only [List.map_cons, List.cons.injEq] at hn
    show MAg σ' (bindVals ((y, o) :: ρ) vals)
    refine forBinds_mag Γf ps _ σ' st1 st' ((y, o) :: ρ) vals h2 ?_ hn.2
      (fun q hq => hv q (List.mem_cons_of_mem _ hq))
    exact .cons (hname.trans hn.1.symm) (fun _ => hv (y, o) List.mem_cons_self) hρ

/-- What a `set`'s keys the analysis took, against the keys the run lends:
the latter's walk is let through, leaving a freer state, and keeps the flag
clear where the former did. -/
theorem mKeys_lent (sigs : List (String × Sig)) (Γ : Ctx) {σ σ' : MSt} {steps : List Step} {st st' : DSt}
    (h : (mKeys sigs Γ σ steps).run st = .ok (σ', st')) (hf : st'.movedHeld = false) :
    ∃ σ'' st'', (mKeysLent sigs Γ σ steps).run st = .ok (σ'', st'') ∧ σ'.Freer σ'' ∧ st''.movedHeld = false := by
  obtain ⟨σ'', st'', h', hfr, hfl⟩ := mKeys_mono sigs (CtxLe.refl Γ) σ σ steps (MSt.Freer.refl σ) st st σ' st' h id
  exact ⟨σ'', st'', h', hfr, flag_before hfl hf⟩

theorem MAg.freer : ∀ {σ τ : MSt} {ρ : VEnv}, σ.Freer τ → MAg τ ρ → MAg σ ρ
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons hv t, .cons hn h a =>
    .cons (hv.name.trans hn) (fun hl => h (by
      simp only [Bool.and_eq_true] at hl ⊢; exact ⟨hv.init ▸ hl.1, hv.live hl.2⟩)) (MAg.freer t a)

/-- The keys a place's steps index with, as `mKeysLent` walks them. -/
def mKeysLent' (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List Expr → MoveM MSt
  | σ, [] => pure σ
  | σ, k :: ks => mE sigs Γ true σ k >>= fun σ => mKeysLent' sigs Γ σ ks

theorem mKeysLent_eq (sigs : List (String × Sig)) (Γ : Ctx) : ∀ (σ : MSt) (steps : List Step),
    mKeysLent sigs Γ σ steps = mKeysLent' sigs Γ σ (indexKeys steps)
  | σ, [] => by simp [mKeysLent, mKeysLent', indexKeys]
  | σ, .field _ :: rest => by simp only [mKeysLent, indexKeys]; exact mKeysLent_eq sigs Γ σ rest
  | σ, .index k _ :: rest => by
    simp only [mKeysLent, indexKeys, mKeysLent']
    congr 1; funext σ1; exact mKeysLent_eq sigs Γ σ1 rest

theorem mKeysLent'_estep (sigs : List (String × Sig)) (Γ : Ctx) : ∀ (σ : MSt) (ks : List Expr),
    EStep σ (mKeysLent' sigs Γ σ ks)
  | σ, [] => by unfold mKeysLent'; exact EStep.pure σ
  | σ, k :: ks => by
    unfold mKeysLent'
    exact EStep.bind (mE_estep sigs Γ _ σ k) (fun σ1 => mKeysLent'_estep sigs Γ σ1 ks)

/-- What checking a `set`'s target gives: the keys, in order, with their types. -/
theorem typeOf_ok {m : ChkM Ty} {t : Ty} (h : typeOf m = .ok t) : m.run.val = .ok t := by
  unfold typeOf at h
  split at h
  · rename_i e; cases h; exact e
  · cases h

/-- The context a branch of an `if` runs in, as `execStmt` finds it, is the
one the analysis follows it in. -/
theorem condCtx_ok {env : InferEnv} {n : Nat} {Γ : Ctx} {c : Expr} {ct : Ty}
    (h : (synthF env n Γ c).run.val = .ok ct) (x : Option String) (err : Bool) :
    condCtx env n Γ c x err = (match ct.expose with
      | .option p => if err then Γ else bindOpt Γ x p
      | .result p => if err then bindOpt Γ x .error else bindOpt Γ x p
      | _ => Γ) := by
  unfold condCtx
  rw [h]
  rfl

theorem condCtx_none (env : InferEnv) (n : Nat) (Γ : Ctx) (c : Expr) (err : Bool) :
    condCtx env n Γ c none err = Γ := by
  unfold condCtx
  repeat' split
  all_goals rfl

theorem matchVariants_ok {env : InferEnv} {n : Nat} {Γ : Ctx} {e : Expr} {t : Ty} {vs : Variants}
    (h : (synthF env n Γ e).run.val = .ok t) (he : t.expose = .enum vs) : matchVariants env n Γ e = vs.toList := by
  unfold matchVariants
  simp only [h, he]

theorem forCtx_ok {env : InferEnv} {n : Nat} {Γ : Ctx} {names : List String} {coll : Expr}
    {Δ : List (String × Ty)} {d : Array Lit.Code} (h : (forBindings env n Γ names coll).run #[] = (some Δ, d)) :
    forCtx env n Γ names coll = { Γ with inLoop := true }.bindAll Δ := by
  unfold forCtx
  rw [h]

/-- What a `for` walks binds as many names as it has. -/
theorem walk_length {c : Ty} {names : List String} {Δ : List (String × Ty)} (h : walk c names = .ok Δ) :
    Δ.length = names.length := by
  unfold walk at h
  split at h
  all_goals first
    | (simp only [pure, Except.pure, Except.ok.injEq] at h; subst h; rfl)
    | (simp [throw, throwThe, MonadExceptOf.throw] at h)

theorem liftE_ok {α} {x : Except Lit.Code α} {a : α} (h : (liftE x : ChkM α).run.val = .ok a) : x = .ok a := by
  unfold liftE at h
  cases x with
  | ok b => cases h; rfl
  | error c => cases h

theorem forBindings_some {env : InferEnv} {n : Nat} {Γ : Ctx} {names : List String} {coll : Expr}
    {Δ : List (String × Ty)} {d : Array Lit.Code} (h : (forBindings env n Γ names coll).run #[] = (some Δ, d)) :
    ∃ t : Ty, walk t.expose names = .ok Δ := by
  unfold forBindings at h
  split at h
  · unfold runChk at h
    simp only [StateT.run_bind, StateT.run_modify, pure_bind] at h
    split at h
    · rename_i a ha
      simp only [StateT.run_pure] at h
      obtain ⟨rfl, -⟩ := h
      rw [run_bind] at ha
      split at ha
      · rename_i t ht
        simp only at ha
        exact ⟨t, liftE_ok ha⟩
      · simp at ha
    · exact (nomatch (show (Option.none : Option (List (String × Ty))) = some Δ from congrArg Prod.fst h))
  · exact (nomatch (show (Option.none : Option (List (String × Ty))) = some Δ from congrArg Prod.fst h))

theorem forRefs_length {env : InferEnv} {n : Nat} {Γ : Ctx} {names : List String} {coll : Expr}
    {Δ : List (String × Ty)} {d : Array Lit.Code} (h : (forBindings env n Γ names coll).run #[] = (some Δ, d)) :
    (forRefs env n Γ names coll).length = names.length := by
  obtain ⟨t, hw⟩ := forBindings_some h
  unfold forRefs
  rw [h]
  simp [walk_length hw]


/-- What `loopEnd` asks and leaves: every way back to the start kept live
what the start had (O007), and what the loop is left with is what its ways
out agree on. -/
theorem loopEnd_run {σ σc : MSt} {natural : Bool} {body : List Stmt} {r : MOut} {st st' : DSt} {ρ : MSt}
    (h : (loopEnd σ σc natural body r).run st = .ok (ρ, st')) :
    ((if completes body then [σc.back r.1] else []) ++ r.2.2.map σc.back).all σ.keepsLive = true ∧
      (joinAll "O014" σc ((if natural then [σc] else []) ++ r.2.1.map σc.back)).run st = .ok (ρ, st') := by
  simp only [loopEnd] at h
  rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hk, h⟩
  · cases h
  · exact ⟨by simpa using hk, h⟩

theorem MSt.keepsLive_of_all {σ : MSt} {L : List MSt} (h : L.all σ.keepsLive = true) {τ : MSt} (hτ : τ ∈ L) :
    σ.keepsLive τ = true := by
  simp only [List.all_eq_true] at h
  exact h τ hτ

theorem MFr.freer {O : List (String × Bool)} {σ τ : MSt} (h : MFr O σ) (hf : σ.Freer τ) : MFr O τ := by
  obtain ⟨⟨δ, hd, e⟩, hbl⟩ := h
  refine ⟨⟨δ, hd, ?_⟩, fun v hv hb => ?_⟩
  · rw [← e]; exact (shape_freer hf).symm
  · obtain ⟨u, hu, huv⟩ := mem_freer hf hv
    rw [← huv.lent]; exact hbl u hu (huv.back ▸ hb)
where
  shape_freer : ∀ {σ τ : MSt}, σ.Freer τ → σ.shape = τ.shape
    | _, _, .nil => rfl
    | _, _, .cons hv t => by simp only [MSt.shape, List.map_cons, hv.name, hv.back]; exact congrArg _ (shape_freer t)
  mem_freer : ∀ {σ τ : MSt}, σ.Freer τ → ∀ {v : MVar}, v ∈ τ → ∃ u ∈ σ, u.Freer v
    | _, _, .nil, _, h => by cases h
    | _, _, .cons hv t, v, h => by
      rcases List.mem_cons.mp h with rfl | h
      · exact ⟨_, List.mem_cons_self, hv⟩
      · obtain ⟨u, hu, huv⟩ := mem_freer t h
        exact ⟨u, List.mem_cons_of_mem _ hu, huv⟩

/-- What a `set`'s target asks of its variable. -/
theorem mTarget_run {σ σ' : MSt} {x : String} {steps : List Step} {op : Option BinOp} {st st' : DSt}
    (h : (mTarget σ x steps op).run st = .ok (σ', st')) :
    σ = σ' ∧ st = st' ∧ ((op.isSome = true ∨ steps ≠ []) → ∀ v, σ.find x = some v → (v.init && v.live) = true) := by
  unfold mTarget at h
  split at h
  · obtain ⟨rfl, rfl, hr⟩ := readM_run h; exact ⟨rfl, rfl, fun _ => hr⟩
  · rcases MoveM.ite_run h with ⟨he, h⟩ | ⟨he, h⟩
    · cases h
      refine ⟨rfl, rfl, fun hc => ?_⟩
      rcases hc with hc | hc
      · simp at hc
      · exact absurd (List.isEmpty_iff.mp he) hc
    · split at h
      · rename_i v hv
        rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hi, h⟩
        · cases h
        rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨hl, h⟩
        · cases h
        cases h
        refine ⟨rfl, rfl, fun _ w hw => ?_⟩
        rw [hv] at hw; cases hw
        simp only [Bool.not_eq_false'] at hi hl
        simp [hi, hl]
      · rename_i hn
        cases h
        exact ⟨rfl, rfl, fun _ w hw => by rw [hn] at hw; cases hw⟩

/-- A `match` or loop statement's result from what the loop run gave. -/
theorem MSRes.ofLoop' {O : List (String × Bool)} {out : MOut} {B C : Prop} {W : VEnv → Prop}
    {m : EvalM VEnv} (h : match m with
      | .ok ρ' => W ρ' ∧ C
      | .error s => MSStop O [] [] False s) (hw : ∀ ρ', W ρ' → MAg out.1 ρ') : MSRes O out B C m := by
  cases m with
  | ok ρ' => exact ⟨hw ρ' h.1, h.2⟩
  | error s => exact MSStop.weaken h (fun e => e.elim) (fun _ hb => by simp at hb) (fun _ hc => by simp at hc)

theorem MSRes.ofFor' {O : List (String × Bool)} {out : MOut} {B C : Prop} {W : VEnv → Prop}
    {m : EvalM VEnv} (h : match m with
      | .ok ρ' => W ρ'
      | .error s => MSStop O [] [] False s) (hw : ∀ ρ', W ρ' → MAg out.1 ρ') (hc : C) : MSRes O out B C m := by
  cases m with
  | ok ρ' => exact ⟨hw ρ' h, hc⟩
  | error s => exact MSStop.weaken h (fun e => e.elim) (fun _ hb => by simp at hb) (fun _ hc => by simp at hc)

theorem bindVals_length (ρ : VEnv) : ∀ (vals : List (String × Option Val)), (bindVals ρ vals).length = vals.length + ρ.length
  | [] => by simp [bindVals]
  | (x, o) :: vals => by
    show (bindVals ((x, o) :: ρ) vals).length = _
    rw [bindVals_length ((x, o) :: ρ) vals]
    simp; omega

theorem bindVals_eq (ρ : VEnv) : ∀ (vals : List (String × Option Val)), bindVals ρ vals = vals.reverse ++ ρ
  | [] => rfl
  | (x, o) :: vals => by
    show bindVals ((x, o) :: ρ) vals = _
    rw [bindVals_eq, List.reverse_cons, List.append_assoc]
    rfl

/-- A scope back to where it began has what it began with. -/
theorem restore_bindVals (ρ : VEnv) (vals : List (String × Option Val)) : restore ρ (bindVals ρ vals) = ρ := by
  rw [bindVals_eq, restore, List.length_append, Nat.add_sub_cancel, List.drop_left]

@[simp] theorem bindOpt_lends (Γ : Ctx) (x : Option String) (t : Ty) : (bindOpt Γ x t).lends = Γ.lends := by
  cases x <;> rfl

theorem bindAll_sigs : ∀ (Γ : Ctx) (Δ : List (String × Ty)), (Γ.bindAll Δ).sigs = Γ.sigs
  | _, [] => rfl
  | Γ, (x, t) :: Δ => by
    show ((Γ.bind x t .let_).bindAll Δ).sigs = Γ.sigs
    rw [bindAll_sigs]; rfl

theorem bindAll_lends : ∀ (Γ : Ctx) (Δ : List (String × Ty)), (Γ.bindAll Δ).lends = Γ.lends
  | _, [] => rfl
  | Γ, (x, t) :: Δ => by
    show ((Γ.bind x t .let_).bindAll Δ).lends = Γ.lends
    rw [bindAll_lends]; rfl

theorem bindAllAs_lends : ∀ (Γ : Ctx) (Δ : List (String × Ty)) (k : Kind), (Γ.bindAllAs Δ k).lends = Γ.lends
  | _, [], _ => rfl
  | Γ, (x, t) :: Δ, k => by
    show ((Γ.bind x t k).bindAllAs Δ k).lends = Γ.lends
    rw [bindAllAs_lends]; rfl

theorem bindPat_lends (Γ : Ctx) (p : Pat) (t : Ty) (k : Kind) (d : Array Lit.Code) :
    ((bindPat Γ p t k).run d).1.lends = Γ.lends := by
  unfold bindPat
  split
  · simp only [StateM.run_pure']; exact bindAllAs_lends Γ _ k
  · simp only [StateM.run_bind', StateM.run_modify', StateM.run_pure']

theorem bindChecked_lends (env : InferEnv) (n : Nat) (Γ : Ctx) (p : Pat) (hint : Option Hint) (e : Expr)
    (k : Kind) (d : Array Lit.Code) : ((bindChecked env n Γ p hint e k).run d).1.lends = Γ.lends := by
  unfold bindChecked
  cases hint with
  | some h =>
    simp only [StateM.run_bind']
    split
    · simp only [StateM.run_pure']
    · split
      · simp only [StateM.run_bind', StateM.run_modify']; rw [bindPat_lends]
      · simp only [StateM.run_bind']; rw [bindPat_lends]
  | none =>
    simp only [StateM.run_bind']
    split
    · rw [bindPat_lends]
    · split
      · split
        · rw [bindPat_lends]
        · simp only [StateM.run_pure']
      · simp only [StateM.run_pure']

/-- Checking a statement keeps whether the context lends, as it keeps its
signatures. -/
theorem lends_checkStmtF (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (s : Stmt) (d : Array Lit.Code),
    ((checkStmtF env n Γ s).run d).1.lends = Γ.lends
  | 0, Γ, s, d => by simp only [checkStmtF, StateM.run_bind', StateM.run_modify', StateM.run_pure']
  | n + 1, Γ, s, d => by
    cases s
    case let_ p hint e => simp only [checkStmtF]; exact bindChecked_lends env n Γ p hint e .let_ d
    case var_ p hint v =>
      cases v with
      | some e => simp only [checkStmtF]; exact bindChecked_lends env n Γ p hint e .var_ d
      | none =>
        cases hint with
        | none => simp only [checkStmtF, StateM.run_pure']
        | some h =>
          simp only [checkStmtF, StateM.run_bind']
          split
          · simp only [StateM.run_bind']; rw [bindPat_lends]
          · simp only [StateM.run_pure']
    case const_ x hint e =>
      simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
      show ((bindChecked env n { Γ with constExpr := true } (.name x) hint e .const_).run d).1.lends = Γ.lends
      rw [bindChecked_lends]
    case type_ x h => simp only [checkStmtF, StateM.run_pure']; split <;> rfl
    case if_ c tB eB t e =>
      cases tB <;> cases eB <;> simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
    case loop c body =>
      cases c <;> simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
    all_goals (simp only [checkStmtF]; simp only [StateM.run_bind', StateM.run_pure'])

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallMove sigs call)
  (O : List (String × Bool))
include hcall
set_option linter.unusedSectionVars false

/-- A `set`'s keys, each lent, as the run reads them. -/
theorem evalKeys_move (n : Nat) (Γ Γk Γa : Ctx) (hs : SigsIn sigs Γ.sigs) (hv : CtxLe Γa Γ)
    (hk : Γk.lends = true) (hks : Γk.sigs = Γ.sigs) (hkv : Γk.vars = Γ.vars) :
    ∀ (keys : List (Expr × Ty)) (σ σ' : MSt) (st st' : DSt),
    (mKeysLent' sigs Γa σ (keys.map (·.1))).run st = .ok (σ', st') → st'.movedHeld = false →
    MT O σ σ' (fun _ _ => True) (evalKeys env call n Γk keys)
  | [], σ, σ', st, st', h, hf => by
    simp only [List.map_nil] at h
    unfold mKeysLent' at h
    obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
    simp only [evalKeys]
    exact MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | (k, kt) :: keys, σ, σ', st, st', h, hf => by
    simp only [List.map_cons] at h
    unfold mKeysLent' at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h2⟩ := h
    have hf1 : st1.movedHeld = false := flag_before (mKeysLent'_estep sigs Γa σ1 _ st1 σ' st' h2).2 hf
    simp only [evalKeys]
    exact MT.bind (evalC_move env sigs call hcall O n Γk Γa k kt σ σ1 st st1 (hs.of_eq hks) (hv.of_vars_eq hkv)
        (by rw [hk]; exact h1) hf1) (mE_estep sigs Γa _ σ k st σ1 st1 h1).1 (fun _ =>
      MT.bind (evalKeys_move n Γ Γk Γa hs hv hk hks hkv keys σ1 σ' st1 st' h2 hf)
        (mKeysLent'_estep sigs Γa σ1 _ st1 σ' st' h2).1 (fun _ => MT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))

/-- `let` or `var` with a value. -/
theorem execBind_move (n : Nat) (Γa Γ Γ' : Ctx) (ρ : VEnv) (p : Pat) (hint : Option Hint) (e : Expr) (σ σ1 σ2 : MSt)
    (st st1 st2 st3 : DSt) (w : Bool) (hv : CtxLe Γa Γ) (hs : SigsIn sigs Γ.sigs) (hl : Γ.lends = false)
    (h : (mE sigs Γa false σ e).run st = .ok (σ1, st1)) (hf : st1.movedHeld = false)
    (hb : (bindM Γ' σ1 (patNames p) true w).run st2 = .ok (σ2, st3)) (hfr : MFr O σ) (hρ : MAg σ ρ) {B C : Prop}
    (hC : C) : MSRes O (σ2, [], []) B C (execBind env call n Γ ρ p hint e) := by
  have fin : ∀ (a : Val) (ρ1 : VEnv), MAg σ1 ρ1 →
      MSRes O (σ2, [], []) B C (do
        let vals ← patValues p a
        if vals.map (·.1) != patNames p then throw .wrong
        pure (bindVals ρ1 (vals.map fun (x, w) => (x, some w)))) := by
    intro a ρ1 hρ1
    cases hpv : patValues p a with
    | error s =>
      simp only [bind, Except.bind]
      have : s = .wrong := by
        unfold patValues at hpv; repeat' split at hpv
        all_goals simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
      subst this; trivial
    | ok vals =>
      simp only [bind, Except.bind]
      split
      · trivial
      · rename_i hn
        have hn' : vals.map (·.1) = patNames p := by simpa using hn
        refine ⟨?_, hC⟩
        exact bindM_mag Γ' true w (patNames p) σ1 σ2 st2 st3 ρ1 (vals.map fun (x, w) => (x, some w)) hb hρ1
          (by simpa [List.map_map, Function.comp_def] using hn') (fun _ q hq => by
            simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
  have hE : (mE sigs Γa Γ.lends σ e).run st = .ok (σ1, st1) := by rw [hl]; exact h
  unfold execBind
  cases hint with
  | none =>
    exact MSRes.ofRes (evalS_move env sigs call hcall O n Γ Γa e σ σ1 st st1 hs hv hE hf hfr ρ hρ)
      (fun a ρ1 _ hρ1 => fin a ρ1 hρ1)
  | some hh =>
    simp only
    split
    · exact MSRes.ofRes (evalC_move env sigs call hcall O n Γ Γa e _ σ σ1 st st1 hs hv hE hf hfr ρ hρ)
        (fun a ρ1 _ hρ1 => fin a ρ1 hρ1)
    · trivial

/-- `set x … = e`. -/
theorem execSet_move (n : Nat) (Γa Γ : Ctx) (ρ : VEnv) (id : Nat) (x : String) (steps : List Step) (op : Option BinOp)
    (e : Expr) (σ σ' : MSt) (st st' : DSt) (hv : CtxLe Γa Γ) (hs : SigsIn sigs Γ.sigs) (hl : Γ.lends = false)
    (h : (mSet sigs Γa id σ x steps op e).run st = .ok (σ', st')) (hf : st'.movedHeld = false)
    (hfr : MFr O σ) (hρ : MAg σ ρ) {B C : Prop} (hC : C) : MSRes O (σ', [], []) B C (execSet env call n Γ ρ x steps op e) := by
  unfold mSet at h
  replace h := MoveM.bind_ok h
  obtain ⟨_, stA, hA, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨_, stB, hB, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨σ1, st1, h1, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨σ2, st2, h2, h⟩ := h
  obtain ⟨rfl, rfl, htarget⟩ := mTarget_run h2
  replace h := MoveM.bind_ok h
  obtain ⟨_, st3, h3, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨_, st4, h4, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨σ3, st5, h5, h⟩ := h
  obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
  have hex : σ1 = σ3 ∧ st4 = st5 ∧ ((stepsExit steps || op.any BinOp.exits) = true → σ1.outsSet = true) := by
    rcases MoveM.ite_run h5 with ⟨hx, h5⟩ | ⟨hx, h5⟩
    · obtain ⟨rfl, rfl, ho⟩ := exitM_run h5; exact ⟨rfl, rfl, fun _ => ho⟩
    · obtain ⟨rfl, rfl⟩ := MoveM.pure_run h5; exact ⟨rfl, rfl, fun e => by rw [hx] at e; cases e⟩
  obtain ⟨rfl, rfl, hex⟩ := hex
  have hf4 : st4.movedHeld = false := hf
  have hf3 : st3.movedHeld = false := flag_before (FlagMono.ite
    (FlagMono.bind (FlagMono.ownedIn _) (fun _ => FlagMono.record _ _ _ _ _)) (FlagMono.pure _) st3 _ st4 h4) hf4
  have hf1 : st1.movedHeld = false := flag_before (FlagMono.noteOverMoved _ _ _ _ st1 _ st3 h3) hf3
  -- The parts: the value, then the keys.
  obtain ⟨stP, stQ, hP, hfP, hfQ⟩ := leavingIf_run h1
  unfold mSetParts at hP
  replace hP := MoveM.bind_ok hP
  obtain ⟨_, _, hP0, hP⟩ := hP
  obtain rfl := exprAt_run hP0
  replace hP := MoveM.bind_ok hP
  obtain ⟨_, stP1, hP1, hP⟩ := hP
  have eP1 := MoveM.modify_run hP1
  subst eP1
  replace hP := MoveM.bind_ok hP
  obtain ⟨σa, stP2, hPa, hP⟩ := hP
  replace hP := MoveM.bind_ok hP
  obtain ⟨_, stP2', hP2, hP⟩ := hP
  rw [exprAt_run hP2] at hP
  replace hP := MoveM.bind_ok hP
  obtain ⟨_, stP3, hP3, hP⟩ := hP
  have eP3 := MoveM.modify_run hP3
  subst eP3
  replace hP := MoveM.bind_ok hP
  obtain ⟨σb, stP4, hPb, hP⟩ := hP
  replace hP := MoveM.bind_ok hP
  obtain ⟨_, stP5, hP5, hP⟩ := hP
  have eP5 := MoveM.modify_run hP5
  subst eP5
  obtain ⟨rfl, rfl⟩ := MoveM.pure_run hP
  have hfb : stP4.movedHeld = false := hfQ.trans hf1
  have hfa : stP2.movedHeld = false := flag_before (mKeys_estep sigs Γa σa steps _ σb stP4 hPb).2 hfb
  have hsa := (mE_estep sigs Γa _ σ e _ σa stP2 hPa).1
  have hsb := (mKeys_estep sigs Γa σa steps _ σb stP4 hPb).1
  -- The keys as the run lends them.
  obtain ⟨σb', stK', hKl, hfrK, hfK⟩ := mKeys_lent sigs Γa hPb hfb
  rw [mKeysLent_eq] at hKl
  have hsb' := (mKeysLent'_estep sigs Γa σa _ _ σb' stK' hKl).1
  have hfrb : MFr O σb' := (hfr.of_le (hsa.trans hsb)).freer hfrK
  unfold execSet
  cases hlk : Γ.lookup x with
  | none => dsimp only; trivial
  | some tk =>
    obtain ⟨t, k⟩ := tk
    dsimp only
    cases hp : setPlan op t steps with
    | none => dsimp only; trivial
    | some vk =>
      obtain ⟨vt, keys⟩ := vk
      dsimp only
      have hkeys : keys.map (·.1) = indexKeys steps := setPlan_keys hp
      rw [← hkeys] at hKl
      have inner : ∀ Γe : Ctx, SigsIn sigs Γe.sigs → CtxLe Γa Γe → Γe.lends = op.isSome →
          MT O σ σb' (fun _ _ => True) (do
          let r ← evalC env call n Γe e vt
          let kvs ← evalKeys env call n Γ.key keys
          match (← get).lookup x with
          | some old => Fun.here (writeSteps op t old steps kvs r)
          | none => throw Stop.wrong) := by
        intro Γe hsΓ hvΓ hlΓ
        refine MT.bind (evalC_move env sigs call hcall O n Γe Γa e vt σ σa _ stP2 hsΓ hvΓ (by rw [hlΓ]; exact hPa) hfa) hsa
          (fun r => MT.bind (evalKeys_move env sigs call hcall O n Γ Γ.key Γa hs hv rfl rfl rfl keys σa σb' _ stK' hKl hfK) hsb'
            (fun kvs => ?_))
        refine MT.get' (fun ρ1 hfr1 hρ1 => ?_)
        have hρb : MAg σb ρ1 := MAg.freer hfrK hρ1
        split
        · rename_i old hold
          have hc : Calm (writeSteps op t old steps kvs r) ∧
              (∀ w, writeSteps op t old steps kvs r = .error (.early w) → σb'.outsSet = true) := by
            cases old with
            | none =>
              by_cases hq : op.isSome = true ∨ steps ≠ []
              · exact absurd hold (MAg.lookup hρb (htarget hq))
              · simp only [not_or, Bool.not_eq_true, Option.isSome_eq_false_iff, Option.isNone_iff_eq_none,
                  ne_eq, Decidable.not_not] at hq
                obtain ⟨rfl, rfl⟩ := hq
                simp only [writeSteps]
                exact ⟨Calm.pure' _, fun w hw => by cases hw⟩
            | some v =>
              exact ⟨writeSteps_calm op t v steps kvs r, fun w hw => hfrK.outsSet (hex (by
                rcases writeSteps_early' op t v steps kvs r w hw with hh | hh <;> simp [hh]))⟩
          exact (MT.here hc.1 hc.2 (fun _ _ _ _ => trivial)) hfr1 ρ1 hρ1
        · exact MT.wrong hfr1 ρ1 hρ1
      refine MSRes.ofRes (inner _ ?hsΓ ?hvΓ ?hlΓ hfr ρ hρ) (fun w ρ' _ hρ' => ?_)
      case hsΓ => cases op <;> exact hs
      case hvΓ => cases op <;> exact hv
      case hlΓ => cases op <;> first | exact hl | rfl
      have hρb : MAg σb ρ' := MAg.freer hfrK hρ'
      refine ⟨?_, hC⟩
      split
      · exact MAg.give x w hρb
      · exact MAg.set x w hρb

/-- `ret` or `ret e`. -/
theorem execRet_move (n : Nat) (Γa Γ : Ctx) (ρ : VEnv) (e : Option Expr) (id : Nat) (σ : MSt) (out : MOut)
    (st st' : DSt) (hv : CtxLe Γa Γ) (hs : SigsIn sigs Γ.sigs) (hl : Γ.lends = false)
    (h : (do
      let σ ← exitM σ
      let σ ← (match e with
        | some e => leaving id σ e (mE sigs Γa false σ e)
        | none => pure σ)
      record "before_return" id none (← ownedIn σ) (always := true)
      pure (σ, [], []) : MoveM MOut).run st = .ok (out, st'))
    (hf : st'.movedHeld = false) (hfr : MFr O σ) (hρ : MAg σ ρ) {B C : Prop} :
    MSRes O out B C (execRet env call n Γ ρ e) := by
  replace h := MoveM.bind_ok h
  obtain ⟨σ1, st1, h1, h⟩ := h
  obtain ⟨rfl, rfl, ho⟩ := exitM_run h1
  replace h := MoveM.bind_ok h
  obtain ⟨σ2, st2, h2, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨_, st3, h3, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨_, st4, h4, h⟩ := h
  obtain ⟨-, e4⟩ := MoveM.pure_run h
  subst e4
  have hf3 : st3.movedHeld = false := flag_before (FlagMono.record _ _ _ _ _ st3 _ _ h4) hf
  have hf2 : st2.movedHeld = false := flag_before (FlagMono.ownedIn _ st2 _ st3 h3) hf3
  unfold execRet
  cases hr : Γ.ret with
  | ty r =>
    cases e with
    | some e' =>
      simp only at h2 ⊢
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h2
      have hE' : (mE sigs Γa Γ.lends σ e').run sta = .ok (σ2, stb) := by rw [hl]; exact hE
      have hl2 := (mE_estep sigs Γa _ σ e' sta σ2 stb hE).1
      refine MSRes.ofRes (evalC_move env sigs call hcall O n Γ Γa e' r σ σ2 sta stb hs hv hE' (hfb.trans hf2) hfr ρ hρ)
        (fun v ρ' _ hρ' => ?_)
      exact ⟨σ2, hfr.of_le hl2, hρ', hfr.outsSet hl2 ho⟩
    | none => trivial
  | void =>
    cases e with
    | none => exact ⟨σ, hfr, hρ, ho⟩
    | some _ => trivial

/-- A branch of an `if`, an arm of a `match` or its default: its names bound
to values, then its block, inside the scope the statement is in. Where the
block ends, its variables are at least those bound. -/
theorem branch_move (n k : Nat) (Γb Γr : Ctx) (σc : MSt) (binds : List String) (id : Nat) (kind : String)
    (arm : Option Nat) (body : List Stmt) (base : Nat) (rb : MOut) (st st' : DSt) (ρ1 : VEnv)
    (vals : List (String × Option Val))
    (ih : ∀ (σb : MSt) (ρb : VEnv) (st st' : DSt) (r : MOut),
      (mBlock sigs env n Γb σb base body).run st = .ok (r, st') → st'.movedHeld = false → MFr O σb → MAg σb ρb →
      MSRes O r (hasBreak body = true) (completes body = true) (execBlock env call n k Γr ρb body))
    (h : (mBranch Γb σc binds id kind arm fun σb => mBlock sigs env n Γb σb base body).run st = .ok (rb, st'))
    (hf : st'.movedHeld = false) (hfr : MFr O σc) (hρ : MAg σc ρ1) (hn : vals.map (·.1) = binds)
    (hv : ∀ q ∈ vals, q.2.isSome = true) :
    MSRes O rb (hasBreak body = true) (completes body = true) (execBlock env call n k Γr (bindVals ρ1 vals) body) ∧
      ∀ ρ', execBlock env call n k Γr (bindVals ρ1 vals) body = .ok ρ' → (bindVals ρ1 vals).length ≤ ρ'.length := by
  unfold mBranch at h
  replace h := MoveM.bind_ok h
  obtain ⟨σb, st1, h1, h⟩ := h
  obtain ⟨ib, f1⟩ := bindM_inside Γb true false binds σc σb st st1 h1
  replace h := MoveM.bind_ok h
  obtain ⟨rb', st2, h2, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨_, st3, h3, h⟩ := h
  replace h := MoveM.bind_ok h
  obtain ⟨_, st4, h4, h⟩ := h
  obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
  have hf3 : st3.movedHeld = false := flag_before (FlagMono.record _ _ _ _ _ st3 _ st4 h4) hf
  have hf2 : st2.movedHeld = false := flag_before (FlagMono.ownedIn _ st2 _ st3 h3) hf3
  have hρb := bindM_mag Γb true false binds σc σb st st1 ρ1 vals h1 hρ hn (fun _ => hv)
  have hres := ih σb _ st1 st2 rb' h2 hf2 (hfr.inside ib) hρb
  refine ⟨hres, fun ρ' he => ?_⟩
  rw [he] at hres
  have h1' := (mBlock_sstep sigs env n Γb σb base body st1 rb' st2 h2).1.length
  rw [← MAg.length hρb, ← MAg.length hres.1]
  exact h1'

/- The analysis follows a body in the generic context, the run in an
instance of it: `θ` substituted for the type parameters, which `Ground` says
is a type with none, and inference answering under the instance what it
answered under the generic (`InstEnv`, as the check-once theorem has it;
`Lockstep.lean` proves it of the real inference). -/
variable {θ : List (String × Ty)} (g : Ground θ) (H : InstEnv θ env env)
include g H

mutual

/-- **A statement the analysis lets through never reads what was moved.**
Where the analysis follows a statement in `Γ` from `σ`, and the checker
checks it quietly there, its run in the instance `Γ.inst θ` from variables
that line up with `σ` comes to what the analysis has after it, or stops as
`MSStop` allows. -/
theorem execStmt_move : ∀ (n k : Nat) (Γ Γ' : Ctx) (ρ : VEnv) (s : Stmt) (σ : MSt) (base : Nat) (out : MOut)
    (st st' : DSt), SigsIn sigs Γ.sigs → Γ.lends = false → Respects θ Γ → SigsOk Γ.sigs → s.typeFree = true →
    SQuiet (checkStmtF env n Γ s) Γ' →
    (mStmt sigs env n Γ σ base s).run st = .ok (out, st') → st'.movedHeld = false →
    MFr O σ → MAg σ ρ → MSRes O out (hasBreakS s = true) (completesS s = true) (execStmt env call n k (Γ.inst θ) ρ s)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [execStmt]; trivial
  | n + 1, k, Γ, Γ', ρ, s, σ, base, out, st, st', hs, hl, r, sg, tf, hq, h, hf, hfr, hρ => by
    have hl0 := MAg.length hρ
    have hle : CtxLe Γ (Γ.inst θ) := CtxLe.inst θ Γ
    unfold mStmt at h
    replace h := MoveM.bind_ok h
    obtain ⟨id, st0, hid, h⟩ := h
    have hid' : st0.movedHeld = st.movedHeld := by
      simp only [stmtId, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at hid
      cases hid; rfl
    cases s with
    | let_ p hint e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
      replace h := MoveM.bind_ok h
      obtain ⟨σ2, st2, h2, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      have hf1 : st1.movedHeld = false := (bindM_inside _ _ _ _ _ _ _ _ h2).2.symm.trans hf
      simp only [execStmt]
      exact execBind_move env sigs call hcall O n Γ (Γ.inst θ) _ ρ p hint e σ σ1 σ2 sta stb st1 st2 false hle hs hl hE
        (hfb.trans hf1) h2 hfr hρ rfl
    | var_ p hint v =>
      dsimp only at h
      cases v with
      | some e =>
        replace h := MoveM.bind_ok h
        obtain ⟨σ1, st1, h1, h⟩ := h
        obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
        replace h := MoveM.bind_ok h
        obtain ⟨σ2, st2, h2, h⟩ := h
        obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
        have hf1 : st1.movedHeld = false := (bindM_inside _ _ _ _ _ _ _ _ h2).2.symm.trans hf
        simp only [execStmt]
        exact execBind_move env sigs call hcall O n Γ (Γ.inst θ) _ ρ p hint e σ σ1 σ2 sta stb st1 st2 true hle hs hl hE
          (hfb.trans hf1) h2 hfr hρ rfl
      | none =>
        cases hint with
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
          simp only [execStmt]
          exact ⟨hρ, rfl⟩
        | some hh =>
          replace h := MoveM.bind_ok h
          obtain ⟨σ1, st1, h1, h⟩ := h
          replace h := MoveM.bind_ok h
          obtain ⟨_, st2, h2, h⟩ := h
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
          simp only [execStmt]
          split
          · split
            · rename_i Δ hΔ
              refine ⟨?_, rfl⟩
              exact bindM_mag _ false true (patNames p) σ σ1 st0 st1 ρ _ h1 hρ
                (by simpa [List.map_map, Function.comp_def] using patBindings_names hΔ) (fun e => by cases e)
            · trivial
          · trivial
    | set_ x steps op e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      simp only [execStmt]
      exact execSet_move env sigs call hcall O n Γ (Γ.inst θ) ρ id x steps op e σ σ1 st0 st1 hle hs hl h1 hf hfr hρ rfl
    | fun_ f =>
      dsimp only at h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      simp only [execStmt]
      exact ⟨hρ, rfl⟩
    | ret e =>
      dsimp only at h
      simp only [execStmt]
      exact execRet_move env sigs call hcall O n Γ (Γ.inst θ) ρ e id σ out st0 st' hle hs hl h hf hfr hρ
    | brk =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st2, h2, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      simp only [execStmt]
      exact ⟨rfl, σ, List.mem_singleton_self σ, hρ⟩
    | cont =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st2, h2, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      simp only [execStmt]
      exact ⟨σ, List.mem_singleton_self σ, hρ⟩
    | debuglog e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
      simp only [execStmt]
      exact MSRes.ofRes (evalS_move env sigs call hcall O n (Γ.inst θ).borrow Γ e σ σ1 sta stb hs hle hE (hfb.trans hf)
        hfr ρ hρ) (fun _ ρ1 _ h1 => ⟨h1, rfl⟩)
    | call e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
      have hE' : (mE sigs Γ Γ.lends σ e).run sta = .ok (σ1, stb) := by rw [hl]; exact hE
      simp only [execStmt]
      exact MSRes.ofRes (evalS_move env sigs call hcall O n (Γ.inst θ) Γ e σ σ1 sta stb hs hle hE' (hfb.trans hf) hfr ρ hρ)
        (fun _ ρ1 _ h1 => ⟨h1, rfl⟩)
    | const_ x hint e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
      replace h := MoveM.bind_ok h
      obtain ⟨σ2, st2, h2, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      have hf1 : st1.movedHeld = false := (bindM_inside _ _ _ _ _ _ _ _ h2).2.symm.trans hf
      simp only [execStmt]
      exact execBind_move env sigs call hcall O n { Γ with constExpr := true } { Γ.inst θ with constExpr := true } _ ρ
        (.name x) hint e σ σ1 σ2 sta stb st1 st2 false hle hs hl hE (hfb.trans hf1) h2 hfr hρ rfl
    | type_ x hh => simp [Stmt.typeFree] at tf
    | if_ c tB eB t e =>
      simp only [Stmt.typeFree, Bool.and_eq_true] at tf
      obtain ⟨tft, tfe⟩ := tf
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      obtain ⟨sta, stb, hC, -, hfb⟩ := leaving_run h1
      have hC' : (mE sigs Γ Γ.lends σ c).run sta = .ok (σc, stb) := by rw [hl]; exact hC
      have lc := (mE_estep sigs Γ _ σ c sta σc stb hC).1
      have hfc := hfr.of_le lc
      replace h := MoveM.bind_ok h
      obtain ⟨rt, st2, h2, h⟩ := h
      obtain ⟨it, ibt, ict, f2⟩ := mBranch_sstep (fun σb => mBlock_sstep sigs env n _ σb base t) st1 rt st2 h2
      replace h := MoveM.bind_ok h
      obtain ⟨re, st3, h3, h⟩ := h
      have r3 : σc.Inside re.1 ∧ (∀ b ∈ re.2.1, σc.Inside b) ∧ (∀ c ∈ re.2.2, σc.Inside c) ∧
          (st2.movedHeld = true → st3.movedHeld = true) := by
        cases e with
        | some eb => exact mBranch_sstep (fun σb => mBlock_sstep sigs env n _ σb base eb) st2 re st3 h3
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h3
          exact ⟨MSt.Inside.refl σc, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fun hm => hm⟩
      obtain ⟨ie, ibe, ice, f3⟩ := r3
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st4, h4, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      have hf3 : st3.movedHeld = false := by
        obtain ⟨-, e4⟩ := joinIf_le it.back ie.back h4
        rw [← e4]; exact hf
      have hf2 : st2.movedHeld = false := flag_before f3 hf3
      have hf1 : st1.movedHeld = false := flag_before f2 hf2
      have hfb' : stb.movedHeld = false := hfb.trans hf1
      have hnm : (σc.back rt.1).SameNames (σc.back re.1) :=
        MSt.SameNames.of_shape (it.back.shape.symm.trans ie.back.shape)
      have hw := joinIf_weaker hnm h4
      -- The `then` body, run on the condition's variables with its name bound.
      have thenCase : ∀ (Γt : Ctx) (ρ1 : VEnv) (w : Val), Γt = condCtx env n Γ c tB false →
          SQuiet (checkBlockF env n Γt t) () → MAg σc ρ1 →
          MSRes O (σ', rt.2.1 ++ re.2.1, rt.2.2 ++ re.2.2) (hasBreakS (.if_ c tB eB t e) = true)
            (completesS (.if_ c tB eB t e) = true)
            (execBlock env call n k (Γt.inst θ) (bindVals ρ1 (tB.toList.map (·, some w))) t >>= fun ρ' =>
              pure (restore ρ1 ρ')) := by
        intro Γt ρ1 w hΓt hqt hρ1
        subst hΓt
        have hΓ : SigsIn sigs (condCtx env n Γ c tB false).sigs ∧ (condCtx env n Γ c tB false).lends = false ∧
            Respects θ (condCtx env n Γ c tB false) ∧ SigsOk (condCtx env n Γ c tB false).sigs := by
          unfold condCtx
          repeat' split
          all_goals first
            | exact ⟨hs, hl, r, sg⟩
            | (rw [bindOpt_sigs, bindOpt_lends, Respects.bindOpt]; exact ⟨hs, hl, r, sg⟩)
        obtain ⟨this, hlen⟩ := branch_move env sigs call hcall O n k _ ((condCtx env n Γ c tB false).inst θ) σc
          tB.toList id "then_branch_exit" none t base rt st1 st2 ρ1 (tB.toList.map (·, some w))
          (fun σb ρb st st' r' hr hf' hfr' hρ' =>
            execBlock_move n k _ ρb t σb base r' st st' hΓ.1 hΓ.2.1 hΓ.2.2.1 hΓ.2.2.2 tft hqt hr hf' hfr' hρ')
          h2 hf2 hfc hρ1 (by simp [List.map_map, Function.comp_def])
          (fun q hq => by simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
        generalize execBlock env call n k ((condCtx env n Γ c tB false).inst θ) _ t = m at this hlen ⊢
        cases m with
        | ok ρ2 =>
          obtain ⟨h2', hct⟩ := this
          simp only [bind, Except.bind, pure, Except.pure]
          have hl2 := hlen ρ2 rfl
          have hlb : ρ1.length ≤ (bindVals ρ1 (tB.toList.map (·, some w))).length := by
            rw [bindVals_length]; omega
          refine ⟨?_, ?_⟩
          · rw [← restore_restore hlb hl2]
            have hb1 : MAg (σc.back rt.1) (restore ρ1 (restore (bindVals ρ1 (tB.toList.map (·, some w))) ρ2)) := by
              rw [restore_restore hlb hl2]
              exact MAg.back h2' (MAg.length hρ1)
            exact MAg.weaker (hw.1 hct) hb1
          · cases e <;> simp [completesS, hct]
        | error s' =>
          exact MSStop.weaken this (fun hb => by simp [hasBreakS, hb])
            (fun b hb => List.mem_append_left _ hb) (fun c hc => List.mem_append_left _ hc)
      -- The `else`, with no name bound.
      have elseCase0 : eB = none → ∀ (ρ1 : VEnv), SQuiet (checkElse env n Γ e) () → MAg σc ρ1 →
          MSRes O (σ', rt.2.1 ++ re.2.1, rt.2.2 ++ re.2.2) (hasBreakS (.if_ c tB eB t e) = true)
            (completesS (.if_ c tB eB t e) = true) (execElse env call n k (Γ.inst θ) ρ1 e) := by
        intro heB ρ1 hqe hρ1
        subst heB
        cases e with
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h3
          simp only [execElse]
          refine ⟨MAg.weaker (hw.2 rfl) ?_, by simp [completesS]⟩
          simpa [MSt.back] using hρ1
        | some e' =>
          simp only [execElse]
          have hqe' : SQuiet (checkBlockF env n Γ e') () := by rw [checkElse.eq_def] at hqe; exact hqe
          have hΓ : condCtx env n Γ c none true = Γ := condCtx_none env n Γ c true
          rw [hΓ] at h3
          obtain ⟨this, -⟩ := branch_move env sigs call hcall O n k Γ (Γ.inst θ) σc [] id "else_branch_exit" none e' base
            re st2 st3 ρ1 []
            (fun σb ρb st st' r' hr hf' hfr' hρ' =>
              execBlock_move n k Γ ρb e' σb base r' st st' hs hl r sg (by simpa [elseTypeFree] using tfe) hqe' hr hf'
                hfr' hρ')
            h3 hf3 hfc hρ1 rfl (fun _ hq => by cases hq)
          simp only [bindVals, List.foldl_nil] at this
          generalize execBlock env call n k (Γ.inst θ) ρ1 e' = m at this ⊢
          cases m with
          | ok ρ2 =>
            obtain ⟨h2', hce⟩ := this
            exact ⟨MAg.weaker (hw.2 (by simpa [Option.all] using hce)) (MAg.back h2' (MAg.length hρ1)),
              by simp [completesS, hce]⟩
          | error s' =>
            exact MSStop.weaken this (fun hb => by simp [hasBreakS, hasBreakElse, hb])
              (fun b hb => List.mem_append_right _ hb) (fun c hc => List.mem_append_right _ hc)
      -- The `else`, with the error bound.
      have elseCase1 : ∀ (Γe : Ctx) (ρ1 : VEnv) (w : Val), Γe = condCtx env n Γ c eB true →
          SQuiet (checkElse env n Γe e) () → MAg σc ρ1 →
          MSRes O (σ', rt.2.1 ++ re.2.1, rt.2.2 ++ re.2.2) (hasBreakS (.if_ c tB eB t e) = true)
            (completesS (.if_ c tB eB t e) = true)
            (execElse env call n k (Γe.inst θ) (bindVals ρ1 (eB.toList.map (·, some w))) e >>= fun ρ' =>
              pure (restore ρ1 ρ')) := by
        intro Γe ρ1 w hΓe hqe hρ1
        subst hΓe
        cases e with
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h3
          simp only [execElse, pure_bind]
          refine ⟨MAg.weaker (hw.2 rfl) ?_, by simp [completesS]⟩
          rw [restore_bindVals]
          simpa [MSt.back] using hρ1
        | some e' =>
          simp only [execElse]
          have hqe' : SQuiet (checkBlockF env n (condCtx env n Γ c eB true) e') () := by
            rw [checkElse.eq_def] at hqe; exact hqe
          have hΓ : SigsIn sigs (condCtx env n Γ c eB true).sigs ∧ (condCtx env n Γ c eB true).lends = false ∧
              Respects θ (condCtx env n Γ c eB true) ∧ SigsOk (condCtx env n Γ c eB true).sigs := by
            unfold condCtx
            repeat' split
            all_goals first
              | exact ⟨hs, hl, r, sg⟩
              | (rw [bindOpt_sigs, bindOpt_lends, Respects.bindOpt]; exact ⟨hs, hl, r, sg⟩)
          obtain ⟨this, hlen⟩ := branch_move env sigs call hcall O n k _ ((condCtx env n Γ c eB true).inst θ) σc eB.toList
            id "else_branch_exit" none e' base re st2 st3 ρ1 (eB.toList.map (·, some w))
            (fun σb ρb st st' r' hr hf' hfr' hρ' =>
              execBlock_move n k _ ρb e' σb base r' st st' hΓ.1 hΓ.2.1 hΓ.2.2.1 hΓ.2.2.2
                (by simpa [elseTypeFree] using tfe) hqe' hr hf' hfr' hρ')
            h3 hf3 hfc hρ1 (by simp [List.map_map, Function.comp_def])
            (fun q hq => by simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
          generalize execBlock env call n k ((condCtx env n Γ c eB true).inst θ) _ e' = m at this hlen ⊢
          cases m with
          | ok ρ2 =>
            obtain ⟨h2', hce⟩ := this
            simp only [bind, Except.bind, pure, Except.pure]
            have hl2 := hlen ρ2 rfl
            have hlb : ρ1.length ≤ (bindVals ρ1 (eB.toList.map (·, some w))).length := by
              rw [bindVals_length]; omega
            refine ⟨?_, by simp [completesS, hce]⟩
            rw [restore_restore hlb hl2]
            exact MAg.weaker (hw.2 (by simpa [Option.all] using hce)) (MAg.back h2' (MAg.length hρ1))
          | error s' =>
            exact MSStop.weaken this (fun hb => by simp [hasBreakS, hasBreakElse, hb])
              (fun b hb => List.mem_append_right _ hb) (fun c hc => List.mem_append_right _ hc)
      cases tB <;> cases eB
      case none.none =>
        simp only [checkStmtF] at hq
        obtain ⟨_, -, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
        obtain ⟨_, hqb, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
        obtain ⟨_, hqe, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq
        simp only [execStmt]
        refine MSRes.ofRes (evalC_move env sigs call hcall O n (Γ.inst θ) Γ c .bool σ σc sta stb hs hle hC' hfb' hfr ρ hρ)
          (fun cv ρ1 _ hρ1 => ?_)
        cases hb : boolOf cv with
        | error s' =>
          have : s' = .wrong := by
            unfold boolOf at hb; split at hb <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
          subst this
          simp only [bind, Except.bind]; trivial
        | ok b =>
          simp only [bind, Except.bind]
          cases b with
          | true =>
            simp only [↓reduceIte]
            exact thenCase Γ ρ1 (.bool true) (condCtx_none env n Γ c false).symm hqb hρ1
          | false =>
            simp only [Bool.false_eq_true, ↓reduceIte]
            exact elseCase0 rfl ρ1 hqe hρ1
      all_goals
        simp only [checkStmtF] at hq
        obtain ⟨_, hc, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
        obtain ⟨ct, rfl, hqc⟩ := SQuiet.chk_inv hc
        try dsimp only at hq
        obtain ⟨_, hm, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq
        have hct : typeOf (synthF env n (Γ.inst θ) c) = .ok (ct.inst θ) := typeOf_quiet (synthF_inst g H n Γ c ct hqc r sg)
        have hcv : (synthF env n Γ c).run.val = .ok ct := by rw [hqc]
        simp only [execStmt]
        rw [hct]
        simp only [bind, Except.bind]
        refine MSRes.ofRes (evalS_move env sigs call hcall O n (Γ.inst θ) Γ c σ σc sta stb hs hle hC' hfb' hfr ρ hρ)
          (fun v ρ1 _ hρ1 => ?_)
        rw [Ty.expose_inst g]
        split at hm
        · -- An option: its payload bound in the `then`, nothing in the `else` (F094).
          rename_i p hp
          obtain ⟨_, hqb, hm⟩ := SQuiet.bind_inv (by app_cont env n) hm
          obtain ⟨_, h94, hqe⟩ := SQuiet.bind_inv (by app_cont env n) hm
          rw [SQuiet.reportIf_iff] at h94
          have hΓt : ∀ x, condCtx env n Γ c x false = bindOpt Γ x p := fun x => by rw [condCtx_ok hcv, hp]; rfl
          rw [hp]
          simp only [Ty.inst]
          cases v <;> try dsimp only
          all_goals first
            | trivial
            | exact absurd h94 Bool.true_ne_false
            | (rename_i w
               rw [← bindOpt_inst]
               exact thenCase _ ρ1 w (hΓt _).symm hqb hρ1)
            | (simp only [Option.isSome_none, Bool.false_eq_true, ↓reduceIte]
               exact elseCase0 rfl ρ1 hqe hρ1)
        · -- A result: its payload bound in the `then`, the error in the `else` (F046).
          rename_i p hp
          split at hm
          · simp at hm
          · rename_i hn
            obtain ⟨_, hqb, hqe⟩ := SQuiet.bind_inv (by app_cont env n) hm
            have hΓt : ∀ x, condCtx env n Γ c x false = bindOpt Γ x p := fun x => by rw [condCtx_ok hcv, hp]; rfl
            have hΓe : ∀ x, condCtx env n Γ c x true = bindOpt Γ x .error := fun x => by rw [condCtx_ok hcv, hp]; rfl
            have hbe : ∀ x, bindOpt (Γ.inst θ) x Ty.error = (bindOpt Γ x .error).inst θ := fun x => by
              rw [bindOpt_inst]; rfl
            rw [hp]
            simp only [Ty.inst]
            cases v <;> try dsimp only
            all_goals first
              | trivial
              | (rename_i w
                 rw [← bindOpt_inst]
                 exact thenCase _ ρ1 w (hΓt _).symm hqb hρ1)
              | (rename_i w
                 rw [hbe]
                 exact elseCase1 _ ρ1 w (hΓe _).symm hqe hρ1)
        · simp at hm
    | loop c body =>
      simp only [Stmt.typeFree] at tf
      have hqb : SQuiet (checkBlockF env n { Γ with inLoop := true } body) () := by
        cases c with
        | none =>
          simp only [checkStmtF] at hq
          obtain ⟨_, -, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
          obtain ⟨_, hb, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq
          exact hb
        | some c =>
          simp only [checkStmtF] at hq
          obtain ⟨_, -, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
          obtain ⟨_, hb, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq
          exact hb
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨r', st2, h2, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st3, h3, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st4, h4, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st5, h5, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st6, h6, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      obtain ⟨hO7, hJ⟩ := loopEnd_run h6
      have hf5 : st5.movedHeld = false := flag_before (FlagMono.loopEnd _ _ _ _ _ st5 _ st6 h6) hf
      have hf4 : st4.movedHeld = false := flag_before (FlagMono.record _ _ _ _ _ st4 _ st5 h5) hf5
      have hf3 : st3.movedHeld = false := flag_before (FlagMono.ownedIn _ st3 _ st4 h4) hf4
      have hf2 : st2.movedHeld = false := flag_before (FlagMono.loopHeld _ _ _ st2 _ st3 h3) hf3
      obtain ⟨ir, ibr, icr, f2⟩ := mBlock_sstep sigs env n _ σc σc.length body st1 r' st2 h2
      have hf1 : st1.movedHeld = false := flag_before f2 hf2
      have hcond : σ.Le σc ∧ (∀ c', c = some c' → ∃ sta stb, (mE sigs Γ Γ.lends σ c').run sta = .ok (σc, stb) ∧
          stb.movedHeld = false) := by
        cases c with
        | some c' =>
          obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
          refine ⟨(mE_estep sigs Γ _ σ c' sta σc stb hE).1, fun c'' hc => ?_⟩
          cases hc
          exact ⟨sta, stb, by rw [hl]; exact hE, hfb.trans hf1⟩
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h1
          exact ⟨MSt.Le.refl σ, fun _ hc => by cases hc⟩
      obtain ⟨lc, hcond⟩ := hcond
      have hfc := hfr.of_le lc
      have hlc : σc.length = σ.length := lc.length.symm
      have shapes : ∀ τ ∈ (if c.isSome = true then [σc] else []) ++ r'.2.1.map σc.back, σc.SameNames τ := by
        intro τ hτ
        simp only [List.mem_append, List.mem_map] at hτ
        rcases hτ with hτ | ⟨b, hb, rfl⟩
        · split at hτ
          · simp at hτ; rw [hτ]; exact MSt.SameNames.refl σc
          · simp at hτ
        · exact MSt.SameNames.of_shape (ibr b hb).back.shape
      have hwk := (joinAll_weaker "O014" _ σc shapes hJ).2
      simp only [execStmt]
      refine MSRes.ofLoop' (execLoop_move (O := O) (σ0 := σ) (σc := σc) (τ := r'.1) (bs := r'.2.1) (cs := r'.2.2)
        (W := fun ρ' => MAg σ' ρ') (C := completesS (.loop c body) = true) (Cb := completes body = true)
        (Bb := hasBreak body = true) ?hc ?hb ?hbrk ?hback k ρ hρ) (fun _ h => h)
      case hc =>
        intro ρ0 hρ0
        cases c with
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h1
          simp only
          exact ⟨hρ0, fun e => by cases e⟩
        | some c' =>
          obtain ⟨sta, stb, hE, hfb⟩ := hcond c' rfl
          simp only
          have := evalC_move env sigs call hcall O n (Γ.inst θ) Γ c' .bool σ σc sta stb hs hle hE hfb hfr ρ0 hρ0
          generalize (evalC env call n (Γ.inst θ) c' .bool).run ρ0 = r0 at this ⊢
          cases r0 with
          | error s => exact this
          | ok q =>
            obtain ⟨cv, ρ1⟩ := q
            obtain ⟨h1, -⟩ := this
            simp only [bind, Except.bind]
            cases hb : boolOf cv with
            | error s =>
              simp only
              have : s = .wrong := by
                unfold boolOf at hb; split at hb <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
              subst this; trivial
            | ok b =>
              simp only [pure, Except.pure]
              exact ⟨h1, fun _ => ⟨MAg.weaker (hwk σc (by simp)) h1, rfl⟩⟩
      case hb =>
        exact fun ρ0 hρ0 => execBlock_move n k { Γ with inLoop := true } ρ0 body σc σc.length r' st1 st2 hs hl r sg tf hqb
          h2 hf2 hfc hρ0
      case hbrk =>
        intro ρ0 ρ' hρ0 b hb hb' hB
        refine ⟨MAg.weaker (hwk (σc.back b) (by simp only [List.mem_append, List.mem_map]; exact .inr ⟨b, hb, rfl⟩))
          (MAg.back hb' (MAg.length hρ0)), ?_⟩
        cases c <;> simp [completesS, hB]
      case hback =>
        intro ρ0 ρ' hρ0 hr
        rcases hr with ⟨hcb, hr⟩ | ⟨cc, hcc, hr⟩
        · have hb := MAg.back hr (MAg.length hρ0)
          refine MAg.le (lc.trans ir.back) (MSt.keepsLive_of_all hO7 ?_) hb
          simp [hcb]
        · have hb := MAg.back hr (MAg.length hρ0)
          refine MAg.le (lc.trans (icr cc hcc).back) (MSt.keepsLive_of_all hO7 ?_) hb
          simp only [List.mem_append, List.mem_map]
          exact .inr ⟨cc, hcc, rfl⟩
    | match_ e cs dflt =>
      simp only [Stmt.typeFree, Bool.and_eq_true] at tf
      obtain ⟨tfc, tfd⟩ := tf
      simp only [checkStmtF] at hq
      obtain ⟨_, hc, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
      obtain ⟨t, rfl, hqt⟩ := SQuiet.chk_inv hc
      try dsimp only at hq
      obtain ⟨_, hm, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq
      split at hm
      case h_2 => simp at hm
      rename_i vs hvs
      obtain ⟨cov, hcs, hm⟩ := SQuiet.bind_inv (by app_cont env n) hm
      obtain ⟨u, hd, -⟩ := SQuiet.bind_inv (by app_cont env n) hm
      cases u
      have hct : typeOf (synthF env n (Γ.inst θ) e) = .ok (t.inst θ) := typeOf_quiet (synthF_inst g H n Γ e t hqt r sg)
      have hcv : (synthF env n Γ e).run.val = .ok t := by rw [hqt]
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
      have hE' : (mE sigs Γ Γ.lends σ e).run sta = .ok (σc, stb) := by rw [hl]; exact hE
      have lc := (mE_estep sigs Γ _ σ e sta σc stb hE).1
      have hfc := hfr.of_le lc
      replace h := MoveM.bind_ok h
      obtain ⟨r', st2, h2, h⟩ := h
      obtain ⟨ir, ibr, icr, f2⟩ := mCases_sstep sigs env n Γ _ σc base id 0 cs st1 r' st2 h2
      replace h := MoveM.bind_ok h
      obtain ⟨d, st3, h3, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st4, h4, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      have r3 : (∀ a ∈ d.1, σc.Le a.1) ∧ (∀ b ∈ d.2.1, σc.Inside b) ∧ (∀ c ∈ d.2.2, σc.Inside c) ∧
          (st2.movedHeld = true → st3.movedHeld = true) := by
        cases dflt with
        | some db =>
          replace h3 := MoveM.bind_ok h3
          obtain ⟨rd, st3', h3', h3⟩ := h3
          obtain ⟨id', ibd, icd, fd⟩ := mBranch_sstep (fun σb => mBlock_sstep sigs env n Γ σb base db) st2 rd st3' h3'
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h3
          refine ⟨fun a ha => ?_, ibd, icd, fd⟩
          simp only [List.mem_singleton] at ha
          subst ha
          exact id'.back
        | none =>
          obtain ⟨rfl, rfl⟩ := MoveM.pure_run h3
          exact ⟨fun _ ha => (nomatch ha), fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fun hm => hm⟩
      obtain ⟨ld, ibd, icd, f3⟩ := r3
      have hf3 : st3.movedHeld = false := by
        obtain ⟨-, e4⟩ := joinMatch_le (MSt.Le.refl σc) (fun a ha => by
          rcases List.mem_append.mp ha with ha | ha
          · exact ir a ha
          · exact ld a ha) h4
        rw [← e4]; exact hf
      have hf2 : st2.movedHeld = false := flag_before f3 hf3
      have hf1 : st1.movedHeld = false := flag_before f2 hf2
      have shapes : ∀ a ∈ r'.1 ++ d.1, σc.SameNames a.1 := fun a ha => by
        rcases List.mem_append.mp ha with ha | ha
        · exact MSt.SameNames.of_shape (ir a ha).shape
        · exact MSt.SameNames.of_shape (ld a ha).shape
      have hwk := joinMatch_weaker shapes h4
      have hvs' : matchVariants env n Γ e = vs.toList := matchVariants_ok hcv hvs
      rw [hvs'] at h2
      simp only [execStmt]
      rw [hct]
      simp only [bind, Except.bind]
      refine MSRes.ofRes (evalS_move env sigs call hcall O n (Γ.inst θ) Γ e σ σc sta stb hs hle hE' (hfb.trans hf1) hfr ρ hρ)
        (fun v ρ1 _ hρ1 => ?_)
      have hl1 : σc.length = ρ1.length := MAg.length hρ1
      rw [Ty.expose_inst g, hvs]
      simp only [Ty.inst]
      have e1 : (vs.inst θ).toList = instVs θ vs.toList := Variants.toList_inst θ vs
      rw [e1]
      split
      · rename_i a _
        have := execCases_move n k Γ ρ1 vs.toList a v cs σc base id 0 r' st1 st2 [] cov hs hl r sg tfc hcs h2 hf2 hfc hρ1
        generalize execCases env call n k (Γ.inst θ) ρ1 (instVs θ vs.toList) a v cs = m at this ⊢
        cases m with
        | ok o =>
          cases o with
          | some ρ' =>
            obtain ⟨τ, hτ, hτρ, hcc⟩ := this
            exact ⟨MAg.weaker (hwk τ (List.mem_append_left _ hτ)) hτρ, by simp [completesS, hcc]⟩
          | none =>
            cases dflt with
            | none => trivial
            | some dd =>
              replace h3 := MoveM.bind_ok h3
              obtain ⟨rd, st3', h3', h3⟩ := h3
              obtain ⟨rfl, rfl⟩ := MoveM.pure_run h3
              simp only [Option.isNone_some, Bool.false_eq_true, ↓reduceIte, execElse]
              have hqd : SQuiet (checkBlockF env n Γ dd) () := by rw [checkElse.eq_def] at hd; exact hd
              obtain ⟨this, -⟩ := branch_move env sigs call hcall O n k Γ (Γ.inst θ) σc [] id "match_arm_exit" (some cs.length)
                dd base rd st2 st3' ρ1 []
                (fun σb ρb st st' r' hr hf' hfr' hρ' =>
                  execBlock_move n k Γ ρb dd σb base r' st st' hs hl r sg (by simpa [elseTypeFree] using tfd) hqd hr hf'
                    hfr' hρ')
                h3' hf3 hfc hρ1 rfl (fun _ hq => by cases hq)
              simp only [bindVals, List.foldl_nil] at this
              generalize execBlock env call n k (Γ.inst θ) ρ1 dd = m2 at this ⊢
              cases m2 with
              | ok ρ2 =>
                obtain ⟨h2', hcd⟩ := this
                refine ⟨MAg.weaker (hwk (σc.back rd.1) (List.mem_append_right _ (by simp [hcd])))
                  (MAg.back h2' hl1), by simp [completesS, completesElse, hcd]⟩
              | error s' =>
                exact MSStop.weaken this (fun hb => by simp [hasBreakS, hasBreakElse, hb])
                  (fun b hb => List.mem_append_right _ hb) (fun c hc => List.mem_append_right _ hc)
        | error s' =>
          exact MSStop.weaken this (fun hb => by simp [hasBreakS, hb]) (fun b hb => List.mem_append_left _ hb)
            (fun c hc => List.mem_append_left _ hc)
      · trivial
    | for_ names coll body =>
      simp only [Stmt.typeFree] at tf
      simp only [checkStmtF] at hq
      obtain ⟨_, hfq, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
      obtain ⟨Δ, rfl, hf'⟩ := forBindings_inst g H hfq r sg
      dsimp only at hq
      obtain ⟨_, hqb, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq
      have hΓf : forCtx env n Γ names coll = { Γ with inLoop := true }.bindAll Δ := forCtx_ok hfq
      have hrl : (forRefs env n Γ names coll).length = names.length := forRefs_length hfq
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h1
      have lc := (mE_estep sigs Γ _ σ coll sta σc stb hE).1
      have hfc := hfr.of_le lc
      replace h := MoveM.bind_ok h
      obtain ⟨σb, st2, h2, h⟩ := h
      have hw : σc.Le (match placeOf coll with
          | some (x, _) => σc.walk x
          | none => σc) := by
        split
        · exact MSt.Le.walk _ σc
        · exact MSt.Le.refl σc
      obtain ⟨ib, f2⟩ := forBinds_inside _ _ _ _ _ _ h2
      replace h := MoveM.bind_ok h
      obtain ⟨r', st3, h3, h⟩ := h
      obtain ⟨ir, ibr, icr, f3⟩ := mBlock_sstep sigs env n _ σb σc.length body st2 r' st3 h3
      replace h := MoveM.bind_ok h
      obtain ⟨_, st4, h4, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st5, h5, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st6, h6, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st7, h7, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      obtain ⟨hO7, hJ⟩ := loopEnd_run h7
      have hf6 : st6.movedHeld = false := flag_before (FlagMono.loopEnd _ _ _ _ _ st6 _ st7 h7) hf
      have hf5 : st5.movedHeld = false := flag_before (FlagMono.record _ _ _ _ _ st5 _ st6 h6) hf6
      have hf4 : st4.movedHeld = false := flag_before (FlagMono.ownedIn _ st4 _ st5 h5) hf5
      have hf3 : st3.movedHeld = false := flag_before (FlagMono.loopHeld _ _ _ st3 _ st4 h4) hf4
      have hf2 : st2.movedHeld = false := flag_before f3 hf3
      have hf1 : st1.movedHeld = false := flag_before f2 hf2
      have iw : σc.Inside σb := (MSt.Inside.of_le hw).trans ib
      have shapes : ∀ τ ∈ (if true = true then [σc] else []) ++ r'.2.1.map σc.back, σc.SameNames τ := by
        intro τ hτ
        simp only [↓reduceIte, List.mem_append, List.mem_map, List.mem_singleton] at hτ
        rcases hτ with hτ | ⟨b, hb, rfl⟩
        · rw [hτ]; exact MSt.SameNames.refl σc
        · exact MSt.SameNames.of_shape (iw.trans (ibr b hb)).back.shape
      have hwk := (joinAll_weaker "O014" _ σc shapes hJ).2
      -- The body's context, generic and as the run instantiates it.
      have hΓ : SigsIn sigs (forCtx env n Γ names coll).sigs ∧ (forCtx env n Γ names coll).lends = false ∧
          Respects θ (forCtx env n Γ names coll) ∧ SigsOk (forCtx env n Γ names coll).sigs := by
        rw [hΓf, bindAll_sigs, bindAll_lends]
        refine ⟨hs, hl, fun m b hm => r m b ?_, sg⟩
        rw [(Ctx.bindAll_bounds _ Δ).1] at hm
        exact hm
      have hqf : SQuiet (checkBlockF env n (forCtx env n Γ names coll) body) () := by rw [hΓf]; exact hqb
      have hΓr : ({ Γ.inst θ with inLoop := true }).bindAll (instBinds θ Δ) = (forCtx env n Γ names coll).inst θ := by
        rw [hΓf, Ctx.inst_bindAll]; rfl
      simp only [execStmt]
      rw [show (forBindings env n (Γ.inst θ) names coll).run #[] = (some (instBinds θ Δ), #[]) from hf']
      dsimp only
      rw [hΓr]
      cases hct : typeOf (synthF env n (Γ.inst θ).borrow coll) with
      | error s' =>
        have : s' = .wrong := by
          unfold typeOf at hct; split at hct <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
        subst this; trivial
      | ok t =>
        simp only [bind, Except.bind]
        refine MSRes.ofRes (evalS_move env sigs call hcall O n (Γ.inst θ).borrow Γ coll σ σc sta stb hs hle hE
          (hfb.trans hf1) hfr ρ hρ) (fun v ρ1 _ hρ1 => ?_)
        cases hwi : walkItems t.expose names v with
        | error s' =>
          rcases walkItems_err hwi with rfl | rfl <;> trivial
        | ok items =>
          simp only
          refine MSRes.ofFor' (execFor_move (O := O) (σc := σc) (τ := r'.1) (bs := r'.2.1) (cs := r'.2.2)
            (W := fun ρ' => MAg σ' ρ') (Cb := completes body = true) (Bb := hasBreak body = true) (names := names)
            ?hb ?hend ?hbrk ?hback items ρ1 (walkItems_lengths hwi) hρ1) (fun _ h => h) rfl
          case hb =>
            intro ρ0 item hlen hρ0
            have hρw : MAg (match placeOf coll with
                | some (x, _) => σc.walk x
                | none => σc) ρ0 := by
              split
              · exact MAg.walk _ hρ0
              · exact hρ0
            have hρb := forBinds_mag _ (names.zip (forRefs env n Γ names coll)) _ σb st1 st2 ρ0
              ((names.zip item).map fun (x, w) => (x, some w)) h2 hρw
              (by simp [List.map_map, Function.comp_def, List.map_fst_zip, hrl, hlen])
              (fun q hq => by simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
            exact execBlock_move n k _ _ body σb σc.length r' st2 st3 hΓ.1 hΓ.2.1 hΓ.2.2.1 hΓ.2.2.2 tf hqf h3 hf3
              (hfc.inside iw) hρb
          case hend => exact fun ρ0 hρ0 => MAg.weaker (hwk σc (by simp)) hρ0
          case hbrk =>
            exact fun ρ0 ρ' hρ0 b hb hb' => MAg.weaker (hwk (σc.back b)
              (by simp only [↓reduceIte, List.mem_append, List.mem_map]; exact .inr ⟨b, hb, rfl⟩))
              (MAg.back hb' (MAg.length hρ0))
          case hback =>
            intro ρ0 ρ' hρ0 hr
            rcases hr with ⟨hcb, hr⟩ | ⟨cc, hcc, hr⟩
            · have hb := MAg.back hr (MAg.length hρ0)
              refine MAg.le (iw.trans ir).back (MSt.keepsLive_of_all hO7 ?_) hb
              simp [hcb]
            · have hb := MAg.back hr (MAg.length hρ0)
              refine MAg.le (iw.trans (icr cc hcc)).back (MSt.keepsLive_of_all hO7 ?_) hb
              simp only [List.mem_append, List.mem_map]
              exact .inr ⟨cc, hcc, rfl⟩

/-- A `match`'s cases: the one for the value's variant, if any, runs as a
branch does, and the run comes to what that arm left; with none, nothing
runs. -/
theorem execCases_move : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (vs : List (String × Option Ty)) (a : String)
    (v : Val) (cs : List Case) (σ : MSt) (base id i : Nat) (r : List (MSt × Bool) × List MSt × List MSt) (st st' : DSt)
    (cov cov' : List String),
    SigsIn sigs Γ.sigs → Γ.lends = false → Respects θ Γ → SigsOk Γ.sigs → casesTypeFree cs = true →
    SQuiet (checkCases env n Γ vs cs cov) cov' →
    (mCases sigs env n Γ vs σ base id i cs).run st = .ok (r, st') →
    st'.movedHeld = false → MFr O σ → MAg σ ρ →
    match execCases env call n k (Γ.inst θ) ρ (instVs θ vs) a v cs with
    | .ok (some ρ') => ∃ τ, (τ, true) ∈ r.1 ∧ MAg τ (restore ρ ρ') ∧ casesComplete cs = true
    | .ok none => True
    | .error s => MSStop O r.2.1 r.2.2 (casesBreak cs = true) s
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _ => by
    simp only [execCases]; trivial
  | n + 1, k, Γ, ρ, vs, a, v, [], σ, base, id, i, r, st, st', cov, cov', hs, hl, rr, sg, tf, hq, h, hf, hfr, hρ => by
    simp only [execCases]; trivial
  | n + 1, k, Γ, ρ, vs, a, v, c :: cs, σ, base, id, i, r, st, st', cov, cov', hs, hl, rr, sg, tf, hq, h, hf, hfr, hρ => by
    have hl0 : σ.length = ρ.length := MAg.length hρ
    have tfb : blockTypeFree c.body = true ∧ casesTypeFree cs = true := by
      cases c <;> simpa [casesTypeFree, Case.body] using tf
    rw [checkCases.eq_def] at hq
    dsimp only at hq
    split at hq
    · exact (SQuiet.push_bind (by app_cont env n) hq).elim
    rename_i hcov
    obtain ⟨_, hqb, hq⟩ := SQuiet.bind_inv (by app_cont env n) hq
    -- The cases after this one, from the same variables.
    have later : ∀ (rb : MOut) (b : List Stmt) (rc : List (MSt × Bool) × List MSt × List MSt) (τ : MSt) (st1 : DSt),
        (mCases sigs env n Γ vs σ base id (i + 1) cs).run st1 = .ok (rc, st') →
        r = ((τ, completes b) :: rc.1, rb.2.1 ++ rc.2.1, rb.2.2 ++ rc.2.2) →
        (casesComplete cs = true → casesComplete (c :: cs) = true) →
        (casesBreak cs = true → casesBreak (c :: cs) = true) →
        match execCases env call n k (Γ.inst θ) ρ (instVs θ vs) a v cs with
        | .ok (some ρ') => ∃ τ, (τ, true) ∈ r.1 ∧ MAg τ (restore ρ ρ') ∧ casesComplete (c :: cs) = true
        | .ok none => True
        | .error s => MSStop O r.2.1 r.2.2 (casesBreak (c :: cs) = true) s := by
      intro rb b rc τ st1 hcs hr hcc hcb
      have := execCases_move n k Γ ρ vs a v cs σ base id (i + 1) rc st1 st' _ cov' hs hl rr sg tfb.2 hq hcs hf hfr hρ
      subst hr
      generalize execCases env call n k (Γ.inst θ) ρ (instVs θ vs) a v cs = m at this ⊢
      cases m with
      | ok o =>
        cases o with
        | some ρ' =>
          obtain ⟨τ', hτ', h', hc'⟩ := this
          exact ⟨τ', List.mem_cons_of_mem _ hτ', h', hcc hc'⟩
        | none => trivial
      | error s =>
        exact MSStop.weaken this hcb (fun x hx => List.mem_append_right _ hx) (fun x hx => List.mem_append_right _ hx)
    have hfin : ∀ (rb : MOut) (b : List Stmt) (rc : List (MSt × Bool) × List MSt × List MSt) (ρb : VEnv)
        (vals : List (String × Option Val)) (m : EvalM VEnv),
        r = ((σ.back rb.1, completes b) :: rc.1, rb.2.1 ++ rc.2.1, rb.2.2 ++ rc.2.2) → ρb = bindVals ρ vals →
        (completes b = true → casesComplete (c :: cs) = true) →
        (hasBreak b = true → casesBreak (c :: cs) = true) →
        MSRes O rb (hasBreak b = true) (completes b = true) m →
        (∀ ρ', m = .ok ρ' → ρb.length ≤ ρ'.length) →
        match (some <$> m : EvalM (Option VEnv)) with
        | .ok (some ρ') => ∃ τ, (τ, true) ∈ r.1 ∧ MAg τ (restore ρ ρ') ∧ casesComplete (c :: cs) = true
        | .ok none => True
        | .error s => MSStop O r.2.1 r.2.2 (casesBreak (c :: cs) = true) s := by
      intro rb b rc ρb vals m hr hρb hcc hcb hm hlen
      subst hr hρb
      cases m with
      | ok ρ' =>
        obtain ⟨h2, hcb'⟩ := hm
        simp only [Functor.map, Except.map]
        refine ⟨σ.back rb.1, by simp [hcb'], ?_, hcc hcb'⟩
        exact MAg.back h2 hl0
      | error s' =>
        simp only [Functor.map, Except.map]
        exact MSStop.weaken hm hcb (fun x hx => List.mem_append_left _ hx) (fun x hx => List.mem_append_left _ hx)
    cases c with
    | atom nm b =>
      -- The arm checks quietly in `Γ`.
      have hqa : SQuiet (checkBlockF env n Γ b) () := by
        simp only [Case.name] at hqb
        cases hlk : vs.lookup nm with
        | none =>
          rw [hlk] at hqb
          simp only [Case.body] at hqb
          exact (SQuiet.push_bind (by app_cont env n) hqb).elim
        | some p =>
          rw [hlk] at hqb
          simp only at hqb
          obtain ⟨_, -, hqb⟩ := SQuiet.bind_inv (by app_cont env n) hqb
          exact hqb
      simp only [mCases] at h
      replace h := MoveM.bind_ok h
      obtain ⟨rb, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨rc, st2, h2, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      have hf1 : st1.movedHeld = false :=
        flag_before (mCases_sstep sigs env n Γ vs σ base id (i + 1) cs st1 rc st2 h2).2.2.2 hf
      rw [execCases.eq_def]; dsimp only
      by_cases hn : nm = a
      · subst hn
        simp only [Case.name, beq_self_eq_true, ↓reduceIte]
        obtain ⟨this, hlen⟩ := branch_move env sigs call hcall O n k Γ (Γ.inst θ) σ [] id "match_arm_exit" (some i) b base rb
          st st1 ρ []
          (fun σb ρb st st' r' hr hf' hfr' hρ' =>
            execBlock_move n k Γ ρb b σb base r' st st' hs hl rr sg (by simpa [Case.body] using tfb.1) hqa hr hf' hfr' hρ')
          h1 hf1 hfr hρ rfl (fun _ hq => by cases hq)
        simp only [bindVals, List.foldl_nil] at this hlen
        exact hfin rb b rc ρ [] _ rfl rfl (fun hc => by simp [casesComplete, hc]) (fun hc => by simp [casesBreak, hc])
          this hlen
      · simp only [Case.name, show (nm == a) = false by simpa using hn, Bool.false_eq_true, ↓reduceIte]
        exact later rb b rc (σ.back rb.1) st1 h2 rfl (fun hc => by simp [casesComplete, hc])
          (fun hc => by simp [casesBreak, hc])
    | term nm x b =>
      simp only [mCases] at h
      replace h := MoveM.bind_ok h
      obtain ⟨rb, st1, h1, h⟩ := h
      replace h := MoveM.bind_ok h
      obtain ⟨rc, st2, h2, h⟩ := h
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
      have hf1 : st1.movedHeld = false :=
        flag_before (mCases_sstep sigs env n Γ vs σ base id (i + 1) cs st1 rc st2 h2).2.2.2 hf
      rw [execCases.eq_def]; dsimp only
      by_cases hn : nm = a
      · clear later
        subst hn
        simp only [Case.name, beq_self_eq_true, ↓reduceIte]
        cases v
        case term a' w =>
          simp only
          rw [instVs_lookup]
          simp only [Case.name] at hqb
          cases hlk : vs.lookup nm with
          | none => rw [hlk] at hqb; exact (SQuiet.push_bind (by app_cont env n) hqb).elim
          | some o =>
            cases o with
            | none => rw [hlk] at hqb; exact (SQuiet.push_bind (by app_cont env n) hqb).elim
            | some p =>
              rw [hlk] at hqb
              simp only at hqb
              simp only [Option.map_some]
              have hΓ : termCtx Γ vs nm x = Γ.bind x p .let_ := by unfold termCtx; rw [hlk]
              rw [hΓ] at h1
              obtain ⟨this, hlen⟩ := branch_move env sigs call hcall O n k (Γ.bind x p .let_) ((Γ.bind x p .let_).inst θ) σ [x]
                id "match_arm_exit" (some i) b base rb st st1 ρ [(x, some w)]
                (fun σb ρb st st' r' hr hf' hfr' hρ' =>
                  execBlock_move n k _ ρb b σb base r' st st' hs hl rr sg (by simpa [Case.body] using tfb.1) hqb hr hf'
                    hfr' hρ')
                h1 hf1 hfr hρ rfl (fun q hq => by simp at hq; subst hq; rfl)
              simp only [bindVals, List.foldl_cons, List.foldl_nil] at this hlen
              exact hfin rb b rc ((x, some w) :: ρ) [(x, some w)] _ rfl rfl (fun hc => by simp [casesComplete, hc])
                (fun hc => by simp [casesBreak, hc]) this hlen
        all_goals trivial
      · simp only [Case.name, show (nm == a) = false by simpa using hn, Bool.false_eq_true, ↓reduceIte]
        exact later rb b rc (σ.back rb.1) st1 h2 rfl (fun hc => by simp [casesComplete, hc])
          (fun hc => by simp [casesBreak, hc])

/-- A block: its statements in order, each in the context the one before it
leaves, which the run instantiates as the analysis has it
(`checkStmtF_inst`). -/
theorem execBlock_move : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (ss : List Stmt) (σ : MSt) (base : Nat) (out : MOut)
    (st st' : DSt), SigsIn sigs Γ.sigs → Γ.lends = false → Respects θ Γ → SigsOk Γ.sigs → blockTypeFree ss = true →
    SQuiet (checkBlockF env n Γ ss) () → (mBlock sigs env n Γ σ base ss).run st = .ok (out, st') →
    st'.movedHeld = false → MFr O σ → MAg σ ρ →
    MSRes O out (hasBreak ss = true) (completes ss = true) (execBlock env call n k (Γ.inst θ) ρ ss)
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [execBlock]; trivial
  | _ + 1, _, _, ρ, [], σ, _, out, st, st', _, _, _, _, _, _, h, _, _, hρ => by
    simp only [mBlock] at h
    obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
    simp only [execBlock]
    exact ⟨hρ, rfl⟩
  | n + 1, k, Γ, ρ, s :: ss, σ, base, out, st, st', hs, hl, r, sg, tf, hq, h, hf, hfr, hρ => by
    simp only [blockTypeFree, Bool.and_eq_true] at tf
    rw [checkBlockF.eq_def] at hq
    dsimp only at hq
    obtain ⟨Γ', hs1, hq⟩ := SQuiet.bind_inv (fun _ => checkBlockF_appends _ _ _ _) hq
    obtain ⟨hs1', hbd, hsg⟩ := checkStmtF_inst g H n Γ s Γ' hs1 r sg tf.1
    have e1 : ((checkStmtF env n Γ s).run #[]).1 = Γ' := congrArg Prod.fst hs1
    have e2 : ((checkStmtF env n (Γ.inst θ) s).run #[]).1 = Γ'.inst θ := congrArg Prod.fst hs1'
    simp only [mBlock] at h
    replace h := MoveM.bind_ok h
    obtain ⟨r1, st1, h1, h⟩ := h
    replace h := MoveM.bind_ok h
    obtain ⟨r2, st2, h2, h⟩ := h
    obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
    rw [e1] at h2
    have hf1 : st1.movedHeld = false := flag_before (mBlock_sstep sigs env n _ r1.1 base ss st1 r2 st2 h2).2.2.2 hf
    obtain ⟨i1, -⟩ := mStmt_sstep sigs env n Γ σ base s st r1 st1 h1
    have := execStmt_move n k Γ Γ' ρ s σ base r1 st st1 hs hl r sg tf.1 hs1 h1 hf1 hfr hρ
    simp only [execBlock]
    rw [e2]
    generalize execStmt env call n k (Γ.inst θ) ρ s = m at this ⊢
    cases m with
    | error s' =>
      exact MSStop.weaken this (fun hb => by simp [hasBreak, hb]) (fun b hb => List.mem_append_left _ hb)
        (fun c hc => List.mem_append_left _ hc)
    | ok ρ' =>
      obtain ⟨h1', hc1⟩ := this
      have hl' : Γ'.lends = false := by rw [← e1, lends_checkStmtF]; exact hl
      have hr' : Respects θ Γ' := fun m b hm => r m b (by rw [← hbd]; exact hm)
      have := execBlock_move n k Γ' ρ' ss r1.1 base r2 st1 st2 (hs.of_eq hsg) hl' hr' (by rw [hsg]; exact sg) tf.2 hq h2 hf
        (hfr.inside i1) h1'
      simp only [bind, Except.bind]
      generalize execBlock env call n k (Γ'.inst θ) ρ' ss = m2 at this ⊢
      cases m2 with
      | ok ρ'' => exact ⟨this.1, by simp [completes, hc1, this.2]⟩
      | error s' =>
        exact MSStop.weaken this (fun hb => by simp [hasBreak, hb]) (fun b hb => List.mem_append_right _ hb)
          (fun c hc => List.mem_append_right _ hc)

end

end

end Datalove.Fun
