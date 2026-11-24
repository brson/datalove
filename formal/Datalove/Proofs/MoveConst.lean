import Datalove.Proofs.MoveCall
import Datalove.Proofs.ConstSound

/-!
# Evaluating a script's consts never reads what was moved

`Fun.constPassMoves_move`: where the move analysis of a script's consts, each
on the consts evaluated before it (`constMovesTop`), lets them through,
evaluating them with the run that moves (`constPassMoves`) never stops at
`Stop.uninit`. With `Fun.runScriptMoves_move`, a script the model refuses
nothing of never stops there at all (`Fun.runScriptMoves_noUninit`).

The proof is `Proofs/ConstSound.lean`'s for what holds a value, over the
move analysis: a const's initializer runs on the consts' values, those
evaluated before it holding one and the rest none, which is where the marks
the analysis starts from have one (`constMarks_MAg`), and calls functions
from a table in which each function sees the same, analyzed from the same
marks with only the reachable functions' signatures (`constProgMove`), so
that `execStmt_move` does the rest.
-/

namespace Datalove.Fun

open Lit (Val)

/-- **What the analysis marks as holding a value has one**: where every value
in the store is one, each variable `constMarks` marks from the store's consts
lines up with what `constEnv` gives it. -/
theorem constMarks_MAg {store : CStore} (hs : ∀ p ∈ store, p.2.2.isSome = true) :
    ∀ vars : List (String × Ty × Kind), MAg (constMovesMarks vars store.known) (constEnv vars store)
  | [] => .nil
  | (x, t, k) :: vars => by
    simp only [constMovesMarks, constMarks, constEnv, List.map_cons]
    refine .cons rfl (fun hm => ?_) (by simpa [constMovesMarks, constMarks, constEnv] using constMarks_MAg hs vars)
    simp only [Bool.and_true, Bool.and_eq_true, beq_iff_eq, List.any_eq_true, CStore.known, List.mem_map] at hm
    obtain ⟨rfl, ⟨_, ⟨p, hp, rfl⟩, hpm⟩⟩ := hm
    simp only [beq_self_eq_true, ↓reduceIte]
    cases hf : store.find? (fun (y, u, _) => y == x && u == t) with
    | none =>
      exfalso
      have := List.find?_eq_none.mp hf p hp
      simp_all
    | some e => simpa using hs e (List.mem_of_find?_eq_some hf)

/-- The marks are a frame with no parameter: nothing is read back. -/
theorem MFr.marks (vars : List (String × Ty × Kind)) (known : List (String × Ty)) :
    MFr [] (constMovesMarks vars known) := by
  refine ⟨⟨(constMovesMarks vars known).shape, fun p hp => ?_, by simp⟩, fun v hv hb => ?_⟩
  · simp only [MSt.shape, constMovesMarks, List.map_map, List.mem_map, Function.comp_def] at hp
    obtain ⟨⟨y, s⟩, -, rfl⟩ := hp
    rfl
  · simp only [constMovesMarks, List.mem_map] at hv
    obtain ⟨⟨y, s⟩, -, rfl⟩ := hv
    cases hb

/-! ## The functions a const reaches -/

theorem funsMovesOk_flagMono (A S : List (String × Sig)) (known : List (String × Ty)) :
    ∀ (fds : List FunDef), FlagMono (funsMovesOk A S known fds)
  | [] => fun _ _ _ h hm => by simp only [funsMovesOk] at h; cases h; exact hm
  | fd :: fds => by
    simp only [funsMovesOk]
    refine FlagMono.bind (FlagMono.lift _) (fun p => ?_)
    obtain ⟨_, s1⟩ := p
    dsimp only
    exact FlagMono.bind (FlagMono.modify' (fun _ h => by simp [h])) (fun _ => funsMovesOk_flagMono A S known fds)

/-- Each function analyzed is let through, from the consts it sees as
evaluated, with no call writing back into what a later argument moved. -/
theorem funsMovesOk_mem {A S : List (String × Sig)} {known : List (String × Ty)} :
    ∀ {fds : List FunDef} {st st' : DSt}, (funsMovesOk A S known fds).run st = .ok ((), st') →
      st'.movedHeld = false → ∀ fd ∈ fds, ∃ s,
        (mFunIn A S movesEnv fd.aliases fd.consts (constMarks fd.consts (known.drop (known.length - fd.seen)))
          (.mk fd.name fd.tps fd.params none fd.body)).run {} = .ok ((), s) ∧ s.movedHeld = false
  | [], _, _, _, _, _, hm => by simp at hm
  | fd :: fds, st, st', h, hf, fd', hm => by
    simp only [funsMovesOk] at h
    replace h := MoveM.bind_ok h
    obtain ⟨p, st1, h1, h⟩ := h
    obtain ⟨u, s1⟩ := p
    dsimp only at h
    replace h := MoveM.bind_ok h
    obtain ⟨_, st2, h2, h⟩ := h
    have e2 := MoveM.modify_run h2
    have hf2 : st2.movedHeld = false := flag_before (funsMovesOk_flagMono A S known fds _ _ _ h) hf
    rcases List.mem_cons.mp hm with rfl | hm
    · obtain ⟨hmf, -⟩ := lift_run h1
      cases u
      refine ⟨s1, hmf, ?_⟩
      rw [e2] at hf2
      simp only [Bool.or_eq_false_iff] at hf2
      exact hf2.2
    · exact funsMovesOk_mem h hf fd' hm

/-- **The functions a const reaches are a program the move analysis lets
through**: with the signatures of the functions it can reach, each of their
bodies is let through from the consts it sees marked as evaluated, which is
where the table's value has one. -/
theorem constProgMove {sigs : List (String × Sig)} {defs : List FunDef} {store : CStore}
    (hs : ∀ p ∈ store, p.2.2.isSome = true) (hd : ∀ fd ∈ defs, FunDefMove sigs fd) {R : List String} {st st' : DSt}
    (hf : (funsMovesOk (sigs.filter fun p => R.contains p.1) sigs store.known
      (defs.filter fun fd => R.contains fd.name)).run st = .ok ((), st'))
    (hfl : st'.movedHeld = false) :
    ProgMove (sigs.filter fun p => R.contains p.1) sigs (funTable defs store) := by
  intro f sig fi hl ht
  have hl' := hl
  rw [lookup_filter_key] at hl'
  split at hl'
  case isFalse => cases hl'
  rename_i hR
  have hmem := lookup_mem_of ht
  simp only [funTable, List.mem_map] at hmem
  obtain ⟨fd, hfd, he⟩ := hmem
  simp only [FunDef.entry, Prod.mk.injEq] at he
  obtain ⟨rfl, rfl⟩ := he
  obtain ⟨hmodes, htp, tf, hqb, -⟩ := hd fd hfd sig hl'
  obtain ⟨s, hmf, hsf⟩ := funsMovesOk_mem hf hfl fd (List.mem_filter.mpr ⟨hfd, hR⟩)
  refine ⟨hmodes, htp, tf, hqb, _, none, s, ?_, hmf, hsf⟩
  have hk : (store.known.drop (store.known.length - fd.seen)) = CStore.known (store.drop (store.length - fd.seen)) := by
    simp [CStore.known, List.map_drop]
  show MAg (constMovesMarks fd.consts _) _
  rw [hk]
  exact constMarks_MAg (fun p hp => hs p (List.mem_of_mem_drop hp)) fd.consts

/-! ## The consts -/

theorem constMovesTop_flagMono (sigs : List (String × Sig)) (defs : List FunDef) :
    ∀ (ss : List Stmt) (Γ : Ctx) (known : List (String × Ty)), FlagMono (constMovesTop sigs defs Γ known ss)
  | [], _, _ => fun _ _ _ h hm => by simp only [constMovesTop] at h; cases h; exact hm
  | s :: ss, Γ, known => by
    have ih := fun Γ' known' => constMovesTop_flagMono sigs defs ss Γ' known'
    cases s
    case fun_ f => simp only [constMovesTop]; exact ih Γ known
    case const_ x hint e =>
      simp only [constMovesTop, constMovesOk]
      refine FlagMono.bind (FlagMono.bind
        (fun st a st' h hm => (mStmt_sstep _ movesEnv fuel Γ _ 0 _ st a st' h).2.2.2 hm)
        (fun _ => funsMovesOk_flagMono _ _ _ _)) (fun _ => ?_)
      split
      · exact ih _ _
      · exact FlagMono.pure _
    all_goals
      simp only [constMovesTop]
      exact ih _ known

/-- **Evaluating the consts never reads what was moved**: from a store of
consts each holding a value, where the move analysis lets the rest of a
script's consts through, each on the consts evaluated before it,
`constPassMoves` never stops at `Stop.uninit`. -/
theorem constPassMoves_move {ss0 : List Stmt} (defs : List FunDef) (k d : Nat)
    (hd : ∀ fd ∈ defs, FunDefMove (resolveNames ss0).1 fd) :
    ∀ (ss : List Stmt) (Γ : Ctx) (store : CStore) (st st' : DSt), Γ.sigs = (resolveNames ss0).1 → Γ.lends = false →
      Γ.bounds = [] → (∀ p ∈ store, p.2.2.isSome = true) → SQuiet (checkTop (resolveNames ss0).1 Γ ss) () →
      (constMovesTop (resolveNames ss0).1 defs Γ store.known ss).run st = .ok ((), st') → st'.movedHeld = false →
      constPassMoves (resolveNames ss0).1 defs k d Γ store ss ≠ .error .uninit
  | [], _, _, _, _, _, _, _, _, _, _, _ => by simp [constPassMoves, pure, Except.pure]
  | s :: ss, Γ, store, st, st', hsig, hl, hb, hs, hq, h, hf => by
    cases s
    case fun_ f =>
      simp only [constMovesTop] at h
      simp only [checkTop] at hq
      obtain ⟨_, -, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      simp only [constPassMoves]
      exact constPassMoves_move defs k d hd ss Γ store st st' hsig hl hb hs hq h hf
    case const_ x hint e =>
      simp only [constMovesTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st1, hok, h⟩ := h
      simp only [constMovesOk] at hok
      replace hok := MoveM.bind_ok hok
      obtain ⟨r, st2, h1, hfuns⟩ := hok
      generalize hR : reachOf defs e = R at h1 hfuns
      have hf1 : st1.movedHeld = false := by
        revert h
        split
        · intro h; exact flag_before (constMovesTop_flagMono _ _ ss _ _ _ _ _ h) hf
        · intro h; obtain ⟨-, rfl⟩ := MoveM.pure_run h; exact hf
      have hf2 : st2.movedHeld = false := flag_before (funsMovesOk_flagMono _ _ _ _ _ _ _ hfuns) hf1
      have hS := sigsIn_filter (resolveNames ss0).1 (fun f => R.contains f)
      have hp := constProgMove hs hd hfuns hf1
      have hcall := runCall_move _ _ (funTable defs store) k hS hp (resolveNames_sigsOk ss0) d
      simp only [checkTop] at hq
      obtain ⟨Γ', hq1, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      have hq1' := hq1
      simp only [checkStmt] at hq1'
      rw [checkStmtF_same movesEnv_same] at hq1'
      have hex := execStmt_move movesEnv _ (runCall movesEnv (resolveNames ss0).1 (funTable defs store) k d) hcall []
        Ground.nil (movesEnv_inst Ground.nil) fuel k Γ Γ' (constEnv Γ.vars store) (.const_ x hint e)
        (constMovesMarks Γ.vars store.known) 0 r st st2 (by rw [hsig]; exact hS) hl (Respects.nil hb)
        (by rw [hsig]; exact resolveNames_sigsOk ss0) rfl hq1' h1 hf2 (MFr.marks _ _) (constMarks_MAg hs Γ.vars)
      rw [Ctx.inst_nil hb] at hex
      have e1 : ((checkStmt Γ (.const_ x hint e)).run #[]).1 = Γ' := congrArg Prod.fst hq1
      simp only [constPassMoves, ↓reduceIte]
      generalize hE : execStmt movesEnv (runCall movesEnv (resolveNames ss0).1 (funTable defs store) k d) fuel k Γ
        (constEnv Γ.vars store) (.const_ x hint e) = m at hex ⊢
      cases m with
      | error s' =>
        intro hu
        simp only [bind, Except.bind, Except.error.injEq] at hu
        subst hu
        exact hex
      | ok ρ =>
        have hx : ∃ v ρ1, ρ = (x, some v) :: ρ1 := by
          rw [fuel_succ] at hE
          simp only [execStmt] at hE
          exact execBind_head hE
        obtain ⟨v, ρ1, rfl⟩ := hx
        obtain ⟨hρ, -⟩ := hex
        simp only [bind, Except.bind]
        rw [e1] at h ⊢
        have hΓ' : Γ'.sigs = (resolveNames ss0).1 := by
          rw [← e1]; exact (checkStmtF_sigs inferEnv fuel Γ _ #[]).trans hsig
        have hl' : Γ'.lends = false := by rw [← e1]; exact (lends_checkStmtF inferEnv fuel Γ _ #[]).trans hl
        have hb' : Γ'.bounds = [] := by rw [← e1]; exact (bounds_checkStmtF inferEnv fuel Γ _ #[]).trans hb
        cases hv : Γ'.vars with
        | nil => simp [throw, throwThe, MonadExceptOf.throw]
        | cons p rest =>
          obtain ⟨y, t, kk⟩ := p
          rw [hv] at h
          simp only
          refine constPassMoves_move defs k d hd ss Γ' ((y, t, some v) :: store) st1 st' hΓ' hl' hb' ?_ hq
            (by simpa [CStore.known] using h) hf
          intro q hq
          rcases List.mem_cons.mp hq with rfl | hq
          · rfl
          · exact hs q hq
    all_goals
      simp only [constMovesTop] at h
      simp only [checkTop] at hq
      obtain ⟨Γ', hq1, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      have e1 : ((checkStmt Γ _).run #[]).1 = Γ' := congrArg Prod.fst hq1
      rw [e1] at h
      simp only [constPassMoves]
      rw [e1]
      exact constPassMoves_move defs k d hd ss Γ' store st st'
        (by rw [← e1]; exact (checkStmtF_sigs inferEnv fuel Γ _ #[]).trans hsig)
        (by rw [← e1]; exact (lends_checkStmtF inferEnv fuel Γ _ #[]).trans hl)
        (by rw [← e1]; exact (bounds_checkStmtF inferEnv fuel Γ _ #[]).trans hb) hs hq h hf

/-- What the move analysis lets through of a script: the walk of its top
level, ending with no call writing back into what a later argument moved. -/
theorem moveScript_run {ss : List Stmt} (hm : moveScript ss = none) (hh : moveScriptHeld ss = false) :
    ∃ s, (mTop (resolveNames ss).1 movesEnv { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss).run {} =
      .ok ((), s) ∧ s.movedHeld = false := by
  unfold moveScript at hm
  unfold moveScriptHeld at hh
  simp only [StateT.run'] at hm
  cases ht : (mTop (resolveNames ss).1 movesEnv { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss).run {}
    with
  | error c =>
    have ht' : mTop (resolveNames ss).1 movesEnv { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss {} =
      .error c := ht
    rw [ht'] at hm
    simp [Functor.map, Except.map] at hm
  | ok p =>
    obtain ⟨u, s⟩ := p
    cases u
    simp only at hh
    rw [ht] at hh
    simp only at hh
    exact ⟨s, rfl, hh⟩

/-- **A script the model refuses nothing of never reads what was moved**:
where a script checks, the move analysis lets it through with no call
writing back into a variable a later argument moved, and the move analysis
of its consts, each on the consts evaluated before it, lets them through,
running it with the run that moves never stops at `Stop.uninit`: not while
evaluating its consts, and not after. -/
theorem runScriptMoves_noUninit (ss : List Stmt) (h : checkScript ss = #[]) (hm : moveScript ss = none)
    (hh : moveScriptHeld ss = false) (hc : constMovesScript ss = none) (k d : Nat) :
    (runScriptMoves k d ss).result ≠ .error .uninit := by
  intro hu
  have hcp := runScriptMoves_move ss h hm hh k d hu
  have hn := checkScript_names h
  have hq := checkScript_quiet h
  obtain ⟨s, ht, hs⟩ := moveScript_run hm hh
  have hd := funDefs_move hn ss _ [] 0 {} s (fun _ h => h) ht hs hq
  unfold constMovesScript at hc
  simp only at hc
  cases hct : (constMovesTop (resolveNames ss).1 (funDefs (resolveNames ss).1
      { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss)
      { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss).run {} with
  | error c => rw [hct] at hc; cases hc
  | ok p =>
    obtain ⟨u, s'⟩ := p
    cases u
    rw [hct] at hc
    simp only at hc
    split at hc
    · cases hc
    rename_i hfl
    simp only [Bool.not_eq_true] at hfl
    exact constPassMoves_move _ k d hd ss _ [] {} s' rfl rfl rfl (by simp) hq (by simpa [CStore.known] using hct) hfl hcp

end Datalove.Fun
