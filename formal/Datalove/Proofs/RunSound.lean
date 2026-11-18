import Datalove.FunRun
import Datalove.Proofs.CallSound

/-!
# A script that checks runs soundly

`Fun.runScript_sound`: where `checkScript` reports nothing, running the
script (`runScript`) logs, at each top-level `debuglog`, a value of the type
the checker gives it, and ends, or returns early with a value of the
script's type `!()`, or stops at a `Gap` the model does not run, at a
variable read before it is set, or where a loop or calls ran as long as
they were given.
It joins the theorems for expressions, statements and calls. The consts
are evaluated first, each to a value of its type (`constPass_sound`), and
every function is in the table from the start, as checked (`funTable_ok`),
by the check-once theorem (`Fun.body_checks`), its consts' values of their
types since F088 keeps a top-level const's type free of type parameters.
-/

namespace Datalove.Fun

open Lit (Val HasType)

/-! ## The checker's loop, as a recursion -/

/-- `checkScript`'s statements from `Γ`, as a recursion. -/
def checkTop (sigs : List (String × Sig)) : Ctx → List Stmt → DiagM Unit
  | _, [] => pure ()
  | Γ, .fun_ f :: ss => checkDecl sigs Γ.aliases (constsOf Γ.vars) f >>= fun _ => checkTop sigs Γ ss
  | Γ, s :: ss => checkStmt Γ s >>= fun Γ' => checkTop sigs Γ' ss

theorem checkScript_eq (ss : List Stmt) : checkScript ss =
    ((checkTop (resolveNames ss).1 { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } ss).run
      (resolveNames ss).2).2 := by
  unfold checkScript
  generalize resolveNames ss = r
  obtain ⟨sigs, diags⟩ := r
  dsimp only
  congr 2
  generalize ({ ret := .ty (.result unitTy), sigs := sigs } : Ctx) = Γ
  induction ss generalizing Γ with
  | nil => simp [checkTop]
  | cons s ss ih =>
    simp at ih
    cases s <;> simp [List.forIn_cons, checkTop, ← ih]

theorem forM_appends {α} (f : α → DiagM PUnit) (hf : ∀ a, Appends (f a)) :
    ∀ (l : List α), Appends (l.forM f)
  | [] => Appends.pure _
  | a :: l => Appends.bind (hf a) (fun _ => forM_appends f hf l)

theorem checkFun_appends (sigs : List (String × Sig)) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (f : FunDecl) : Appends (checkFun sigs aliases consts f) := by
  obtain ⟨name, tps, params, ret, body⟩ := f
  simp only [checkFun]
  cases hl : sigs.lookup name with
  | none => exact Appends.pure _
  | some sig =>
    refine Appends.bind ?_ (fun _ => Appends.bind (checkBlockF_appends _ _ _ _) (fun _ => Appends.report _ _))
    unfold reportGenericConsts
    split
    · exact Appends.pure _
    · exact forM_appends _ (fun _ => Appends.report _ _) _

theorem checkDecl_appends (sigs : List (String × Sig)) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (f : FunDecl) : Appends (checkDecl sigs aliases consts f) := by
  obtain ⟨name, tps, params, ret, body⟩ := f
  exact Appends.bind (Appends.report _ _) (fun _ => checkFun_appends _ _ _ _)

theorem checkTop_appends (sigs : List (String × Sig)) : ∀ (Γ : Ctx) (ss : List Stmt), Appends (checkTop sigs Γ ss)
  | _, [] => Appends.pure _
  | Γ, s :: ss => by
    cases s
    case fun_ f =>
      exact Appends.bind (checkDecl_appends _ _ _ _) (fun _ => checkTop_appends sigs Γ ss)
    all_goals exact Appends.bind (checkStmtF_appends _ _ _ _) (fun Γ' => checkTop_appends sigs Γ' ss)

/-- A script that checks quietly: its names resolved quietly, then its
statements. -/
theorem checkScript_quiet {ss : List Stmt} (h : checkScript ss = #[]) :
    SQuiet (checkTop (resolveNames ss).1 { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } ss) () := by
  rw [checkScript_eq, (checkTop_appends _ _ _).run] at h
  have h2 : ((checkTop (resolveNames ss).1 { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } ss).run
      #[]).2 = #[] := by
    have := congrArg Array.size h
    simp only [Array.size_append, List.size_toArray, List.length_nil] at this
    exact Array.eq_empty_of_size_eq_zero (by omega)
  unfold SQuiet
  generalize (checkTop _ _ ss).run #[] = r at h2 ⊢
  obtain ⟨u, l⟩ := r
  simp_all

/-- A script that checks resolves its names quietly. -/
theorem checkScript_names {ss : List Stmt} (h : checkScript ss = #[]) : (resolveNames ss).2 = #[] := by
  rw [checkScript_eq, (checkTop_appends _ _ _).run] at h
  have := congrArg Array.size h
  simp only [Array.size_append, List.size_toArray, List.length_nil] at this
  exact Array.eq_empty_of_size_eq_zero (by omega)

/-! ## Consts -/

/-- Every const in scope has a type with no type parameter in it, as F088
keeps a top-level const's. -/
def ConstsFree (vars : List (String × Ty × Kind)) : Prop :=
  ∀ p ∈ vars, p.2.2 = .const_ → p.2.1.containsVar = false

theorem constsOf_mem : ∀ {vars : List (String × Ty × Kind)} {p : String × Ty × Kind}, p ∈ constsOf vars →
    p ∈ vars ∧ p.2.2 = .const_
  | [], _, h => by simp [constsOf] at h
  | (x, t, k) :: vs, p, h => by
    simp only [constsOf] at h
    split at h
    · rename_i hk
      rcases List.mem_cons.mp h with rfl | h
      · exact ⟨List.mem_cons_self, by simpa using hk⟩
      · obtain ⟨h1, h2⟩ := constsOf_mem (List.mem_filter.mp h).1
        exact ⟨List.mem_cons_of_mem _ h1, h2⟩
    · obtain ⟨h1, h2⟩ := constsOf_mem (List.mem_filter.mp h).1
      exact ⟨List.mem_cons_of_mem _ h1, h2⟩

/-- The consts evaluated hold values of their types. -/
def StoreOk (store : CStore) : Prop := ∀ p ∈ store, ∀ v, p.2.2 = some v → HasType v p.2.1

theorem StoreOk.drop {store : CStore} (h : StoreOk store) (n : Nat) : StoreOk (store.drop n) :=
  fun p hp => h p (List.mem_of_mem_drop hp)

/-- Variables whose consts' values come from the consts evaluated agree with
them. -/
theorem constEnv_agree {store : CStore} (hs : StoreOk store) :
    ∀ vars : List (String × Ty × Kind), Agree vars (constEnv vars store)
  | [] => .nil
  | (x, t, k) :: vars => by
    refine .cons (fun v hv => ?_) (constEnv_agree hs vars)
    split at hv
    · cases hf : store.find? (fun (y, u, _) => y == x && u == t) with
      | none => simp [hf] at hv
      | some p =>
        simp only [hf, Option.bind_some] at hv
        have hm := List.mem_of_find?_eq_some hf
        have hp := List.find?_some hf
        obtain ⟨y, u, o⟩ := p
        simp only [Bool.and_eq_true, beq_iff_eq] at hp
        obtain ⟨-, rfl⟩ := hp
        exact hs _ hm v hv
    · simp at hv

/-- The consts a function sees, with type arguments substituted, are as they
were, so their values agree. -/
theorem constEnv_inst {consts : List (String × Ty × Kind)} (hfree : ∀ p ∈ consts, p.2.1.containsVar = false)
    {store : CStore} (hs : StoreOk store) (θ : List (String × Ty)) :
    Agree (consts.map fun (x, t, k) => (x, t.inst θ, k)) (constEnv consts store) := by
  have : (consts.map fun (x, t, k) => (x, t.inst θ, k)) = consts := by
    refine (List.map_congr_left (g := id) (fun p hp => ?_)).trans (List.map_id _)
    obtain ⟨x, t, k⟩ := p
    simp [Ty.inst_novar θ t (hfree _ hp)]
  rw [this]
  exact constEnv_agree hs consts

theorem foldl_kinds (k : Kind) : ∀ (Δ : List (String × Ty)) (vars : List (String × Ty × Kind)) p,
    p ∈ Δ.foldl (fun vs (x, t) => (x, t, k) :: vs) vars → p ∈ vars ∨ p.2.2 = k
  | [], _, _, h => .inl h
  | (x, t) :: Δ, vars, p, h => by
    rcases foldl_kinds k Δ _ p h with h | h
    · rcases List.mem_cons.mp h with rfl | h
      · exact .inr rfl
      · exact .inl h
    · exact .inr h

theorem ConstsFree.bindAllAs {Γ : Ctx} (hc : ConstsFree Γ.vars) {k : Kind} (hk : k ≠ .const_)
    (Δ : List (String × Ty)) : ConstsFree (Γ.bindAllAs Δ k).vars := by
  intro p hp hpk
  rw [bindAllAs_vars] at hp
  rcases foldl_kinds k Δ _ p hp with h | h
  · exact hc p h hpk
  · exact absurd (h.symm.trans hpk) hk

theorem bindChecked_shape {env : InferEnv} {n : Nat} {Γ : Ctx} {p : Pat} {hint : Option Hint} {e : Expr}
    {k : Kind} {Γ' : Ctx} (h : SQuiet (bindChecked env n Γ p hint e k) Γ') :
    ∃ Δ t, patBindings p t = .ok Δ ∧ Γ' = Γ.bindAllAs Δ k := by
  cases hint with
  | some hh =>
    simp only [bindChecked, convertReporting] at h
    obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨t, rfl, -⟩ := SQuiet.chk_inv hrt
    dsimp only at h
    split at h
    · exact (SQuiet.push_bind (fun _ => bindPat_appends _ _ _ _) h).elim
    obtain ⟨_, -, h⟩ := SQuiet.bind_inv (fun _ => bindPat_appends _ _ _ _) h
    obtain ⟨Δ, hp, rfl⟩ := bindPat_sound h
    exact ⟨Δ, t, hp, rfl⟩
  | none =>
    simp only [bindChecked] at h
    obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨t, rfl, -⟩ := SQuiet.chk_inv hrt
    obtain ⟨Δ, hp, rfl⟩ := bindPat_sound h
    exact ⟨Δ, t, hp, rfl⟩

set_option hygiene false in
/-- Take a statement's check apart down to the context it gives, the one it
was given, and close the goal with `hc`. -/
local macro "peel_to_pure" : tactic => `(tactic| repeat (first
  | (rw [SQuiet.pure_iff] at h; subst h; exact hc)
  | (obtain ⟨_, -, h⟩ := SQuiet.bind_inv (by app_cont env n) h; try dsimp only at h)))

/-- A statement that checks keeps the consts in scope free of type
parameters: what it binds is not a const, or, a const, F088 sees to it. -/
theorem checkStmtF_consts {env : InferEnv} : ∀ {n : Nat} {Γ : Ctx} {s : Stmt} {Γ' : Ctx},
    SQuiet (checkStmtF env n Γ s) Γ' → ConstsFree Γ.vars → ConstsFree Γ'.vars
  | 0, _, _, _, h, _ => by
    simp only [checkStmtF] at h; exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | n + 1, Γ, s, Γ', h, hc => by
    cases s
    case let_ p hint e =>
      simp only [checkStmtF] at h
      obtain ⟨Δ, t, -, rfl⟩ := bindChecked_shape h
      exact hc.bindAllAs (by decide) Δ
    case var_ p hint v =>
      cases v with
      | some e =>
        simp only [checkStmtF] at h
        obtain ⟨Δ, t, -, rfl⟩ := bindChecked_shape h
        exact hc.bindAllAs (by decide) Δ
      | none =>
        cases hint with
        | none => simp only [checkStmtF, SQuiet.pure_iff] at h; subst h; exact hc
        | some hh =>
          simp only [checkStmtF, convertReporting] at h
          obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
          obtain ⟨t, rfl, -⟩ := SQuiet.chk_inv hrt
          dsimp only at h
          obtain ⟨_, -, h⟩ := SQuiet.bind_inv (fun _ => bindPat_appends _ _ _ _) h
          obtain ⟨Δ, -, rfl⟩ := bindPat_sound h
          exact hc.bindAllAs (by decide) Δ
    case const_ x hint e =>
      simp only [checkStmtF] at h
      obtain ⟨Γ1, hb1, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
      obtain ⟨_, h88, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      rw [SQuiet.reportIf_iff] at h88
      obtain ⟨Δ, t, hp, rfl⟩ := bindChecked_shape hb1
      simp only [patBindings, pure, Except.pure, Except.ok.injEq] at hp
      subst hp
      simp only [constGeneric, Ctx.bindAllAs, List.foldl, Ctx.bind, Ctx.lookup, List.length_cons,
        List.find?_cons, beq_self_eq_true, Option.map_some] at h88
      intro p hp hk
      rcases List.mem_cons.mp hp with rfl | hp
      · simpa using h88
      · exact hc p hp hk
    case type_ x hh =>
      simp only [checkStmtF, SQuiet.pure_iff] at h; subst h
      split <;> exact hc
    case loop c body => cases c <;> (simp only [checkStmtF] at h; peel_to_pure)
    case if_ c a b t e => cases a <;> cases b <;> (simp only [checkStmtF] at h; peel_to_pure)
    all_goals (simp only [checkStmtF] at h; peel_to_pure)

/-! ## Running -/

/-- How a script may end: normally, or returning early with a value of its
type `!()`, or stopping at a `Gap` the model does not run, at a variable
read before it is set, or where a loop or calls ran as long as they were
given. -/
def TopOk (r : EvalM Unit) : Prop :=
  match r with
  | .ok _ => True
  | .error s => StopOk (.ty (.result unitTy)) s

/-- A statement's stop at the top level, outside any loop, is one a script
may end with. -/
theorem StopOkS.top {Γ : Ctx} {B : Prop} {s : Stop} (h : StopOkS Γ B s) (hl : Γ.inLoop = false)
    (hr : Γ.ret = .ty (.result unitTy)) : StopOk (.ty (.result unitTy)) s.unreturn := by
  cases s with
  | returned o ρ =>
    cases o with
    | some r => show EarlyOk _ r; rw [← hr]; exact h.1
    | none => exact absurd ((show Γ.ret = .void from h.1).symm.trans hr) (by simp)
  | early r => exact h.elim
  | retVoid => exact h.elim
  | brk ρ => exact absurd h.1 (by simp [hl])
  | cont ρ => exact absurd h.1 (by simp [hl])
  | _ => exact h

/-- A function as declared is one `runCall` runs as checked, whatever its
consts' values: as `ProgOk` asks of its table entry, with its consts' types
free of type parameters. -/
def FunDefOk (env : InferEnv) (sigs : List (String × Sig)) (fd : FunDef) : Prop :=
  (∀ p ∈ fd.consts, p.2.1.containsVar = false) ∧
  ∀ sig, sigs.lookup fd.name = some sig → fd.params.length = sig.params.length ∧
    ∀ θ, groundArgs θ = true →
      (∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) →
      SQuiet (checkBlockF env fuel ((funCtx sigs fd.aliases fd.consts fd.tps fd.params sig).inst θ) fd.body
        >>= fun _ => reportIf (match sig.ret.inst θ with
          | .ty _ => completes fd.body
          | .void => false) "F063") ()

/-- A function declared is in the table. -/
theorem funTable_lookup {store : CStore} {f : String} :
    ∀ {defs : List FunDef}, (∃ fd ∈ defs, fd.name = f) → ∃ fi, (funTable defs store).lookup f = some fi
  | [], ⟨_, hm, _⟩ => by simp at hm
  | d :: ds, ⟨fd, hm, hn⟩ => by
    simp only [funTable, List.map_cons, FunDef.entry, List.lookup]
    by_cases e : f = d.name
    · simp [e]
    · have : fd ∈ ds := by
        rcases List.mem_cons.mp hm with rfl | hm
        · exact absurd hn.symm e
        · exact hm
      have hne : (f == d.name) = false := by simp [e]
      simp only [hne]
      exact funTable_lookup ⟨fd, this, hn⟩

/-- The table of functions as declared, their consts' values from consts
evaluated, is as `ProgOk` asks, where every function the signatures name is
declared. -/
theorem funTable_ok {env : InferEnv} {sigs : List (String × Sig)} {defs : List FunDef} {store : CStore}
    (hcov : ∀ f sig, sigs.lookup f = some sig → ∃ fd ∈ defs, fd.name = f)
    (hd : ∀ fd ∈ defs, FunDefOk env sigs fd) (hs : StoreOk store) : ProgOk env sigs (funTable defs store) := by
  refine ⟨fun f sig hsig => funTable_lookup (hcov f sig hsig), ?_⟩
  intro f sig fi hsig hl
  have hm := lookup_mem hl
  simp only [funTable, List.mem_map] at hm
  obtain ⟨fd, hfd, he⟩ := hm
  simp only [FunDef.entry, Prod.mk.injEq] at he
  obtain ⟨rfl, rfl⟩ := he
  obtain ⟨hfree, hok⟩ := hd fd hfd
  obtain ⟨hlen, hbody⟩ := hok sig hsig
  exact ⟨hlen, fun θ hg hbd => ⟨hbody θ hg hbd, constEnv_inst hfree (hs.drop _) θ⟩⟩

/-- A function declaration of a script whose names resolve quietly, that
checks, is one as checked: its signature is its own (`resolveNames_own`),
and its body declares no type alias (P074). -/
theorem funDef_ok (ss0 : List Stmt) (hn : (resolveNames ss0).2 = #[]) {Γ : Ctx} (hc : ConstsFree Γ.vars)
    {seen : Nat} {f : FunDecl} (hm : .fun_ f ∈ ss0)
    (hf : SQuiet (checkDecl (resolveNames ss0).1 Γ.aliases (constsOf Γ.vars) f) ()) :
    ∀ fd ∈ (funDef (resolveNames ss0).1 Γ seen f).toList, FunDefOk inferEnv (resolveNames ss0).1 fd := by
  intro fd hfd
  obtain ⟨name, tps, params, ret, body⟩ := f
  obtain ⟨sig0, hsn, htp, hlen⟩ := resolveNames_own hn name tps params ret body hm
  simp only [checkDecl] at hf
  obtain ⟨⟨⟩, h74, hf⟩ := SQuiet.bind_inv (fun _ => checkFun_appends _ _ _ _) hf
  rw [SQuiet.reportIf_iff] at h74
  have tf : blockTypeFree body = true := by simpa using h74
  simp only [funDef, hsn, Option.toList_some, List.mem_singleton] at hfd
  subst hfd
  refine ⟨fun p hp => ?_, fun sig hs => ?_⟩
  · obtain ⟨hm, hk⟩ := constsOf_mem hp
    exact hc p hm hk
  · simp only at hs
    rw [hsn] at hs
    cases hs
    exact ⟨hlen.symm, fun θ hg hbd =>
      body_checks ss0 Γ.aliases (constsOf Γ.vars) name tps params ret body _ hsn htp tf hf θ hg hbd⟩

/-- A function declared with a signature is among a script's functions. -/
theorem funDefs_has (sigs : List (String × Sig)) : ∀ (ss : List Stmt) (Γ : Ctx) (seen : Nat) (d : FunDecl),
    .fun_ d ∈ ss → (sigs.lookup d.name).isSome → ∃ fd ∈ funDefs sigs Γ seen ss, fd.name = d.name
  | [], _, _, _, h, _ => by simp at h
  | s :: ss, Γ, seen, d, hm, hs => by
    rcases List.mem_cons.mp hm with rfl | hm'
    · obtain ⟨name, tps, params, ret, body⟩ := d
      obtain ⟨sig, hsig⟩ := Option.isSome_iff_exists.mp hs
      simp only [FunDecl.name] at hsig
      exact ⟨{ name, aliases := Γ.aliases, consts := constsOf Γ.vars, tps, params, body, seen },
        by simp [funDefs, funDef, hsig], rfl⟩
    · cases s
      case fun_ f =>
        obtain ⟨fd, h1, h2⟩ := funDefs_has sigs ss Γ seen d hm' hs
        exact ⟨fd, by simp [funDefs, h1], h2⟩
      all_goals
        simp only [funDefs]
        exact funDefs_has sigs ss _ _ d hm' hs

/-- Every function a script's signatures name is among its functions. -/
theorem funDefs_covers (ss : List Stmt) (Γ : Ctx) (seen : Nat) :
    ∀ f sig, (resolveNames ss).1.lookup f = some sig →
      ∃ fd ∈ funDefs (resolveNames ss).1 Γ seen ss, fd.name = f := by
  intro f sig hl
  obtain ⟨d, hm, rfl⟩ := resolveNames_declared hl
  exact funDefs_has _ ss Γ seen d hm (by simp [hl])

/-- Every function of a script that checks is declared as checked. -/
theorem funDefs_ok (ss0 : List Stmt) (hn : (resolveNames ss0).2 = #[]) : ∀ (ss : List Stmt) (Γ : Ctx) (seen : Nat),
    (∀ s ∈ ss, s ∈ ss0) → SQuiet (checkTop (resolveNames ss0).1 Γ ss) () → ConstsFree Γ.vars →
    ∀ fd ∈ funDefs (resolveNames ss0).1 Γ seen ss, FunDefOk inferEnv (resolveNames ss0).1 fd
  | [], _, _, _, _, _ => by simp [funDefs]
  | s :: ss, Γ, seen, hsub, h, hc => by
    have hsub' : ∀ s ∈ ss, s ∈ ss0 := fun s' hs' => hsub s' (List.mem_cons_of_mem _ hs')
    cases s
    case fun_ f =>
      simp only [checkTop] at h
      obtain ⟨⟨⟩, hf, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      intro fd hfd
      simp only [funDefs, List.mem_append] at hfd
      rcases hfd with hfd | hfd
      · exact funDef_ok ss0 hn hc (hsub _ List.mem_cons_self) hf fd hfd
      · exact funDefs_ok ss0 hn ss Γ seen hsub' h hc fd hfd
    all_goals
      simp only [checkTop] at h
      obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      have hrun := congrArg Prod.fst hs
      simp only [funDefs]
      rw [hrun]
      exact funDefs_ok ss0 hn ss Γ' _ hsub' h (checkStmtF_consts hs hc)

/-- A const binds its name in front of the variables. -/
theorem const_vars {env : InferEnv} {n : Nat} {Γ Γ' : Ctx} {x : String} {hint : Option Hint} {e : Expr}
    (h : SQuiet (checkStmtF env (n + 1) Γ (.const_ x hint e)) Γ') : ∃ t, Γ'.vars = (x, t, .const_) :: Γ.vars := by
  simp only [checkStmtF] at h
  obtain ⟨Γ1, hb1, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
  obtain ⟨_, -, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
  rw [SQuiet.pure_iff] at h; subst h
  obtain ⟨Δ, t, hp, rfl⟩ := bindChecked_shape hb1
  simp only [patBindings, pure, Except.pure, Except.ok.injEq] at hp
  subst hp
  exact ⟨t, by simp [Ctx.bindAllAs, List.foldl, Ctx.bind]⟩

/-- A script's consts, evaluated in order from consts of their types, are
of their types, or stop as a script may. -/
theorem constPass_sound (ss0 : List Stmt) (k d : Nat) (defs : List FunDef)
    (hcov : ∀ f sig, (resolveNames ss0).1.lookup f = some sig → ∃ fd ∈ defs, fd.name = f)
    (hd : ∀ fd ∈ defs, FunDefOk inferEnv (resolveNames ss0).1 fd) :
    ∀ (ss : List Stmt) (Γ : Ctx) (store : CStore),
    SQuiet (checkTop (resolveNames ss0).1 Γ ss) () → Γ.Runs (resolveNames ss0).1 → Γ.inLoop = false →
    Γ.ret = .ty (.result unitTy) → ConstsFree Γ.vars → StoreOk store →
    match constPass (resolveNames ss0).1 defs k d Γ store ss with
    | .ok store' => StoreOk store'
    | .error s => StopOk (.ty (.result unitTy)) s.unreturn
  | [], _, store, _, _, _, _, _, hs => by simp only [constPass]; exact hs
  | s :: ss, Γ, store, h, hb, hl, hr, hc, hst => by
    have hcall := runCall_ok inferEnv _ _ (funTable_ok hcov hd hst) k d
    cases s
    case fun_ f =>
      simp only [checkTop] at h
      obtain ⟨⟨⟩, -, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      simp only [constPass]
      exact constPass_sound ss0 k d defs hcov hd ss Γ store h hb hl hr hc hst
    case const_ x hint e =>
      simp only [checkTop] at h
      obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      obtain ⟨hS, g⟩ := execStmt_sound inferEnv _ _ hcall fuel k Γ _ _ Γ' hs hb (constEnv_agree hst Γ.vars)
      have hrun := congrArg Prod.fst hs
      have hs' : SQuiet (checkStmtF inferEnv ((fuel - 1) + 1) Γ (.const_ x hint e)) Γ' := hs
      obtain ⟨t, hv⟩ := const_vars hs'
      simp only [constPass]
      rw [hrun]
      dsimp only
      rcases hres : execStmt inferEnv (runCall inferEnv (resolveNames ss0).1 (funTable defs store) k d) fuel k Γ
          (constEnv Γ.vars store) (.const_ x hint e) with s' | ρ'
      · rw [hres] at hS
        exact StopOkS.top hS hl hr
      · rw [hres] at hS
        have ha := hS.1
        rw [hv] at ha
        cases ha with
        | cons ho ha =>
          simp only [hv, bind, Except.bind]
          exact constPass_sound ss0 k d defs hcov hd ss Γ' _ h (g.runs hb) (g.2.1.trans hl) (g.1.trans hr)
            (checkStmtF_consts hs hc) (fun p hp => by
              rcases List.mem_cons.mp hp with rfl | hp
              · exact ho
              · exact hst p hp)
    all_goals
      simp only [checkTop] at h
      obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      obtain ⟨-, g⟩ := execStmt_sound inferEnv _ _ hcall fuel k Γ _ _ Γ' hs hb (constEnv_agree hst Γ.vars)
      have hrun := congrArg Prod.fst hs
      simp only [constPass]
      rw [hrun]
      exact constPass_sound ss0 k d defs hcov hd ss Γ' store h (g.runs hb) (g.2.1.trans hl) (g.1.trans hr)
        (checkStmtF_consts hs hc) hst

/-- A script's statements from a context its run agrees with run soundly. -/
theorem runTop_sound (ss0 : List Stmt) (k d : Nat) :
    ∀ (ss : List Stmt) (Γ : Ctx) (ρ : VEnv) (funs : List (String × FunInfo)) (log : List (Ty × Val)),
    SQuiet (checkTop (resolveNames ss0).1 Γ ss) () → Γ.Runs (resolveNames ss0).1 → Γ.inLoop = false →
    Γ.ret = .ty (.result unitTy) → Agree Γ.vars ρ → ConstsFree Γ.vars →
    ProgOk inferEnv (resolveNames ss0).1 funs → (∀ p ∈ log, HasType p.2 p.1) →
    TopOk (runTop (resolveNames ss0).1 k d Γ ρ funs log ss).result ∧
      ∀ p ∈ (runTop (resolveNames ss0).1 k d Γ ρ funs log ss).log, HasType p.2 p.1
  | [], _, _, _, _, _, _, _, _, _, _, _, hlog => by simp only [runTop]; exact ⟨trivial, hlog⟩
  | s :: ss, Γ, ρ, funs, log, h, hb, hl, hr, hρ, hc, hp, hlog => by
    have hcall := runCall_ok inferEnv _ funs hp k d
    cases s
    case fun_ f =>
      simp only [checkTop] at h
      obtain ⟨⟨⟩, -, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      simp only [runTop]
      exact runTop_sound ss0 k d ss Γ ρ funs log h hb hl hr hρ hc hp hlog
    case debuglog e =>
      simp only [checkTop] at h
      obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      have hs' : SQuiet (checkStmtF inferEnv ((fuel - 1) + 1) Γ (.debuglog e)) Γ' := hs
      simp only [checkStmtF] at hs'
      obtain ⟨_, h1, h2⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hs'
      rw [SQuiet.pure_iff] at h2; subst h2
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv h1
      have he := evalS_sound inferEnv _ _ hcall (fuel - 1) Γ.borrow e t hq hb ρ hρ
      simp only [runTop, typeOf_quiet hq]
      rcases hres : (evalS inferEnv (runCall inferEnv (resolveNames ss0).1 funs k d) (fuel - 1) Γ.borrow e).run ρ
        with s' | ⟨v, ρ'⟩
      · rw [hres] at he
        simp only
        refine ⟨?_, hlog⟩
        show StopOk _ s'.unreturn
        rw [← hr]
        cases s' with
        | returned o ρ'' =>
          cases o with
          | some r => exact he.1
          | none => exact he.elim
        | early r => exact he.elim
        | _ => exact he
      · rw [hres] at he
        simp only
        refine runTop_sound ss0 k d ss Γ ρ' funs _ h hb hl hr he.2 hc hp (fun p hp' => ?_)
        rcases List.mem_append.mp hp' with hp' | hp'
        · exact hlog p hp'
        · simp only [List.mem_singleton] at hp'; subst hp'; exact he.1
    all_goals
      simp only [checkTop] at h
      obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      obtain ⟨hS, g⟩ := execStmt_sound inferEnv _ _ hcall fuel k Γ ρ _ Γ' hs hb hρ
      have hrun := congrArg Prod.fst hs
      simp only [runTop]
      split
      · rename_i ρ' hres
        rw [hres] at hS
        rw [hrun]
        exact runTop_sound ss0 k d ss Γ' ρ' funs log h (g.runs hb) (g.2.1.trans hl) (g.1.trans hr) hS.1
          (checkStmtF_consts hs hc) hp hlog
      · rename_i s' hres
        rw [hres] at hS
        exact ⟨StopOkS.top hS hl hr, hlog⟩

/-- **A script that checks runs soundly**: each top-level `debuglog` logs a
value of the type the checker gives it, and the script ends, or returns
early with a value of its type `!()`, or stops at a `Gap` the model does not
run, at a variable read before it is set, or where a loop or calls ran as
long as they were given. -/
theorem runScript_sound (ss : List Stmt) (h : checkScript ss = #[]) (k d : Nat) :
    TopOk (runScript k d ss).result ∧ ∀ p ∈ (runScript k d ss).log, HasType p.2 p.1 := by
  have hq := checkScript_quiet h
  have hd := funDefs_ok ss (checkScript_names h) ss _ 0 (fun _ h => h) hq (fun _ h => by simp at h)
  have hcov := funDefs_covers ss { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0
  have hcp := constPass_sound ss k d _ hcov hd ss _ [] hq ⟨rfl, rfl⟩ rfl rfl (fun _ h => by simp at h)
    (fun _ h => by simp at h)
  simp only [runScript]
  split
  · rename_i store hres
    rw [hres] at hcp
    exact runTop_sound ss k d ss _ [] _ [] hq ⟨rfl, rfl⟩ rfl rfl .nil (fun _ h => by simp at h)
      (funTable_ok hcov hd hcp) (fun _ h => by simp at h)
  · rename_i s hres
    rw [hres] at hcp
    exact ⟨hcp, by simp⟩

end Datalove.Fun
