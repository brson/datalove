import Datalove.FunConsts
import Datalove.Proofs.InitSound

/-!
# Evaluating a script's consts never reads what holds nothing

`Fun.constPass_init`: where the F089 check lets a script's consts through
(`constTop`), evaluating them (`constPass`) never stops at `Stop.uninit`.
With `Fun.runScript_init`, a script that checks and that the analysis of
what holds a value and the F089 check let through never stops there at all
(`Fun.runScript_noUninit`).

Evaluating a const runs on the consts' values, those evaluated before it
holding one and the rest none, and calls functions from a table in which
each function sees the same. The check analyzes the const and each function
it can reach with each const holding a value where it is one evaluated
before (`constMarks`), which is where the run's value has one
(`constMarks_IAg`), and with only those functions' signatures, so the run
calls no other (`SigsIn`). Those functions are then a program the analysis
lets through (`ProgInit`), and `execStmt_init` does the rest.
-/

namespace Datalove.Fun

open Lit (Val)

/-- A store's consts by name and type, as `constTop` keeps them. -/
def CStore.known (store : CStore) : List (String × Ty) := store.map fun p => (p.1, p.2.1)

/-- **What the check marks as holding a value has one**: where every value
in the store is one, each variable `constMarks` marks from the store's
consts lines up with what `constEnv` gives it. -/
theorem constMarks_IAg {store : CStore} (hs : ∀ p ∈ store, p.2.2.isSome = true) :
    ∀ vars : List (String × Ty × Kind),
      IAg ((constMarks vars store.known).map fun (x, b) => (x, b, false)) (constEnv vars store)
  | [] => .nil
  | (x, t, k) :: vars => by
    simp only [constMarks, constEnv, List.map_cons]
    refine .cons (fun hm => ?_) (by simpa [constMarks, constEnv] using constMarks_IAg hs vars)
    simp only [Bool.and_eq_true, beq_iff_eq, List.any_eq_true, CStore.known, List.mem_map] at hm
    obtain ⟨rfl, ⟨_, ⟨p, hp, rfl⟩, hpm⟩⟩ := hm
    simp only [beq_self_eq_true, ↓reduceIte]
    cases hf : store.find? (fun (y, u, _) => y == x && u == t) with
    | none =>
      exfalso
      have := List.find?_eq_none.mp hf p hp
      simp_all
    | some e => simpa using hs e (List.mem_of_find?_eq_some hf)

/-- Keeping the pairs whose key passes a test keeps a key's lookup where it
passes, and drops it where it does not. -/
theorem lookup_filter_key {β} (P : String → Bool) (f : String) :
    ∀ l : List (String × β), (l.filter fun p => P p.1).lookup f = if P f then l.lookup f else none
  | [] => by simp
  | (g, b) :: l => by
    by_cases hg : P g = true
    · simp only [List.filter_cons, hg, ↓reduceIte, List.lookup_cons]
      rw [lookup_filter_key P f l]
      by_cases hfg : (f == g) = true
      · have := beq_iff_eq.mp hfg; subst this; simp [hg]
      · simp [hfg]
    · simp only [List.filter_cons, hg, Bool.false_eq_true, ↓reduceIte]
      rw [lookup_filter_key P f l]
      by_cases hfg : (f == g) = true
      · have := beq_iff_eq.mp hfg; subst this; simp [hg]
      · simp [List.lookup_cons, hfg]

/-- The signatures a const reaches are among the script's. -/
theorem sigsIn_filter (sigs : List (String × Sig)) (P : String → Bool) :
    SigsIn (sigs.filter fun p => P p.1) sigs := by
  intro f sig h
  rw [lookup_filter_key] at h
  split at h
  · exact h
  · cases h

theorem funsOk_mem {A : List (String × Sig)} {known : List (String × Ty)} :
    ∀ {fds : List FunDef}, funsOk A known fds = .ok () → ∀ fd ∈ fds,
      initFun A (constMarks fd.consts (known.drop (known.length - fd.seen))) fd.params fd.body = .ok ()
  | [], _, _, h => by simp at h
  | fd :: fds, h, fd', hm => by
    simp only [funsOk] at h
    obtain ⟨u, h1, h2⟩ := bind_ok h
    rcases List.mem_cons.mp hm with rfl | hm
    · cases u; exact h1
    · exact funsOk_mem h2 fd' hm

/-- **The functions a const reaches are a program the analysis lets
through**: with the signatures of the functions it can reach, each of their
bodies is let through with each const holding a value where the table's
value has one. -/
theorem constProg {sigs : List (String × Sig)} {defs : List FunDef} {store : CStore}
    (hs : ∀ p ∈ store, p.2.2.isSome = true)
    (hm : ∀ fd ∈ defs, ∀ sig, sigs.lookup fd.name = some sig → fd.params.map (·.1) = sig.params.map (·.1))
    {R : List String}
    (hf : funsOk (sigs.filter fun p => R.contains p.1) store.known (defs.filter fun fd => R.contains fd.name) = .ok ()) :
    ProgInit (sigs.filter fun p => R.contains p.1) (funTable defs store) := by
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
  refine ⟨hm fd hfd sig hl', _, funsOk_mem hf fd (List.mem_filter.mpr ⟨hfd, hR⟩), ?_⟩
  simp only
  have hk : (store.known.drop (store.known.length - fd.seen)) = CStore.known (store.drop (store.length - fd.seen)) := by
    simp [CStore.known, List.map_drop]
  rw [hk]
  exact constMarks_IAg (fun p hp => hs p (List.mem_of_mem_drop hp)) fd.consts

theorem Fr.unmarked (marks : List (String × Bool)) :
    Fr [] (marks.map fun (x, b) => (x, b, false)) :=
  ⟨ISt.shape (marks.map fun (x, b) => (x, b, false)), by simp [ISt.shape], by simp⟩

/-- **Evaluating the consts never reads what holds nothing**: from a store
of consts each holding a value, where the F089 check lets the rest of a
script through, `constPass` never stops at `Stop.uninit`. -/
theorem constPass_init (sigs : List (String × Sig)) (defs : List FunDef) (k d : Nat)
    (hm : ∀ fd ∈ defs, ∀ sig, sigs.lookup fd.name = some sig → fd.params.map (·.1) = sig.params.map (·.1)) :
    ∀ (ss : List Stmt) (Γ : Ctx) (store : CStore), Γ.sigs = sigs → (∀ p ∈ store, p.2.2.isSome = true) →
      constTop sigs defs Γ store.known ss = .ok () → constPass sigs defs k d Γ store ss ≠ .error .uninit
  | [], _, _, _, _, _ => by simp [constPass, pure, Except.pure]
  | s :: ss, Γ, store, hΓ, hs, h => by
    cases s
    case fun_ f =>
      simp only [constTop] at h
      simp only [constPass]
      exact constPass_init sigs defs k d hm ss Γ store hΓ hs h
    case const_ x hint e =>
      simp only [constTop] at h
      obtain ⟨u, hok, h⟩ := bind_ok h
      simp only [constOk] at hok
      obtain ⟨r, h1, hf⟩ := bind_ok hok
      generalize hR : reachOf defs e = R at h1 hf
      have hS := sigsIn_filter sigs (fun f => R.contains f)
      have hp := constProg hs hm hf
      have hcall := runCall_init inferEnv _ (funTable defs store) k hS hp d
      have hex := execStmt_init inferEnv _ (runCall inferEnv sigs (funTable defs store) k d) hcall [] fuel k Γ
        (constEnv Γ.vars store) _ _ r (hΓ ▸ hS) h1 (Fr.unmarked _) (constMarks_IAg hs Γ.vars)
      simp only [constPass, ↓reduceIte]
      generalize hE : execStmt inferEnv (runCall inferEnv sigs (funTable defs store) k d) fuel k Γ
        (constEnv Γ.vars store) (.const_ x hint e) = m at hex
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
        simp only [bind, Except.bind]
        generalize hG : ((checkStmt Γ (.const_ x hint e)).run #[]).1 = Γ' at h ⊢
        have hΓ' : Γ'.sigs = sigs := by
          rw [← hG, ← hΓ]; exact checkStmtF_sigs inferEnv fuel Γ _ #[]
        cases hv : Γ'.vars with
        | nil => simp [throw, throwThe, MonadExceptOf.throw]
        | cons p rest =>
          obtain ⟨y, t, kk⟩ := p
          rw [hv] at h
          simp only
          refine constPass_init sigs defs k d hm ss Γ' ((y, t, some v) :: store) hΓ' ?_ (by simpa [CStore.known] using h)
          intro q hq
          rcases List.mem_cons.mp hq with rfl | hq
          · rfl
          · exact hs q hq
    all_goals
      simp only [constTop] at h
      simp only [constPass]
      exact constPass_init sigs defs k d hm ss _ store
        (by rw [← hΓ]; exact checkStmtF_sigs inferEnv fuel Γ _ #[]) hs h

/-- **A script the analyses let through never reads what holds nothing**:
where a script checks, and the analysis of what holds a value (O005, O006,
O009) and the F089 check let it through, running it never stops at
`Stop.uninit`: not while evaluating its consts, and not after. -/
theorem runScript_noUninit (ss : List Stmt) (h : checkScript ss = #[]) (hi : initScript ss = none)
    (hc : constScript ss = none) (k d : Nat) : (runScript k d ss).result ≠ .error .uninit := by
  intro hu
  have hcp := runScript_init ss h hi k d hu
  have hn := checkScript_names h
  unfold initScript at hi
  cases ht : initTop (resolveNames ss).1 { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss with
  | error c => simp only [ht] at hi; cases hi
  | ok u =>
    unfold constScript at hc
    cases htop : constTop (resolveNames ss).1 (funDefs (resolveNames ss).1
        { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss)
        { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss with
    | error c => simp only [htop] at hc; cases hc
    | ok u' =>
      have hm : ∀ fd ∈ funDefs (resolveNames ss).1 { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss,
          ∀ sig, (resolveNames ss).1.lookup fd.name = some sig → fd.params.map (·.1) = sig.params.map (·.1) := by
        intro fd hfd sig hl
        cases u
        obtain ⟨-, tps, ret, hmem⟩ := funDefs_init _ ss _ [] 0 ht fd hfd
        obtain ⟨sig', hl', hmodes⟩ := decl_modes hn hmem
        rw [hl] at hl'; cases hl'
        exact hmodes.symm
      cases u'
      exact constPass_init _ _ k d hm ss _ [] rfl (by simp) (by simpa [CStore.known] using htop) hcp

end Datalove.Fun
