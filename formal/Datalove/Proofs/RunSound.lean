import Datalove.FunRun
import Datalove.Proofs.CallSound

/-!
# A script that checks runs soundly

`Fun.runScript_sound`: where `checkScript` reports nothing, running the
script (`runScript`) logs, at each top-level `debuglog`, a value of the type
the checker gives it, and ends, or returns early with a value of the
script's type `!()`, or stops at what is not run yet, at a variable read
before it is set, or where a loop or calls ran as long as they were given.
It joins the theorems for expressions, statements and calls: each function
goes into the table as checked (`ProgOk`), by the check-once theorem
(`Fun.body_checks`), its consts' values of their types since F088 keeps a
top-level const's type free of type parameters.
-/

namespace Datalove.Fun

open Lit (Val HasType)

/-! ## The checker's loop, as a recursion -/

/-- `checkScript`'s statements from `Γ`, as a recursion. -/
def checkTop (sigs : List (String × Sig)) : Ctx → List Stmt → DiagM Unit
  | _, [] => pure ()
  | Γ, .fun_ f :: ss => checkFun sigs Γ.aliases (constsOf Γ.vars) f >>= fun _ => checkTop sigs Γ ss
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

theorem checkTop_appends (sigs : List (String × Sig)) : ∀ (Γ : Ctx) (ss : List Stmt), Appends (checkTop sigs Γ ss)
  | _, [] => Appends.pure _
  | Γ, s :: ss => by
    cases s
    case fun_ f =>
      exact Appends.bind (checkFun_appends _ _ _ _) (fun _ => checkTop_appends sigs Γ ss)
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

/-! ## Consts -/

/-- Every const in scope has a type with no type parameter in it, as F088
keeps a top-level const's. -/
def ConstsFree (vars : List (String × Ty × Kind)) : Prop :=
  ∀ p ∈ vars, p.2.2 = .const_ → p.2.1.containsVar = false

theorem Agree.filter_name (x : String) : ∀ {vars : List (String × Ty × Kind)} {ρ : VEnv}, Agree vars ρ →
    Agree (vars.filter (·.1 != x)) (ρ.filter (·.1 != x))
  | _, _, .nil => .nil
  | _, _, @Agree.cons y _ _ _ _ _ ho h => by
    simp only [List.filter_cons]
    split
    · exact .cons ho (Agree.filter_name x h)
    · exact Agree.filter_name x h

theorem constVals_agree : ∀ {vars : List (String × Ty × Kind)} {ρ : VEnv}, Agree vars ρ →
    Agree (constsOf vars) (constVals vars ρ)
  | _, _, .nil => .nil
  | _, _, @Agree.cons x t k _ _ _ ho h => by
    simp only [constsOf, constVals]
    have hr := Agree.filter_name x (constVals_agree h)
    split
    · exact .cons ho hr
    · exact hr

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

/-- The consts a function sees, with type arguments substituted, are as they
were, and their values agree. -/
theorem consts_agree {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) (hc : ConstsFree vars)
    (θ : List (String × Ty)) :
    Agree ((constsOf vars).map fun (x, t, k) => (x, t.inst θ, k)) (constVals vars ρ) := by
  have : ((constsOf vars).map fun (x, t, k) => (x, t.inst θ, k)) = constsOf vars := by
    refine (List.map_congr_left (g := id) (fun p hp => ?_)).trans (List.map_id _)
    obtain ⟨hm, hk⟩ := constsOf_mem hp
    obtain ⟨x, t, k⟩ := p
    simp [Ty.inst_novar θ t (hc _ hm hk)]
  rw [this]
  exact constVals_agree h

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
type `!()`, or stopping at what is not run yet, at a variable read before it
is set, or where a loop or calls ran as long as they were given. -/
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

theorem lookup_append_some {α β} [BEq α] {a : α} {b : β} : ∀ {l1 l2 : List (α × β)},
    (l1 ++ l2).lookup a = some b → l1.lookup a = some b ∨ l2.lookup a = some b
  | [], _, h => .inr h
  | (x, y) :: l1, l2, h => by
    simp only [List.cons_append, List.lookup] at h ⊢
    split at h
    · exact .inl h
    · exact lookup_append_some h

theorem ProgOk.append {env : InferEnv} {sigs : List (String × Sig)} {funs extra : List (String × FunInfo)}
    (h1 : ProgOk env sigs funs) (h2 : ProgOk env sigs extra) : ProgOk env sigs (funs ++ extra) := by
  intro f sig fi hs hl
  rcases lookup_append_some hl with hl | hl
  · exact h1 f sig fi hs hl
  · exact h2 f sig fi hs hl

/-- A function declaration that checks goes into the table as checked. -/
theorem funEntry_ok (ss0 : List Stmt) {Γ : Ctx} {ρ : VEnv} (hρ : Agree Γ.vars ρ) (hc : ConstsFree Γ.vars)
    {f : FunDecl} (hf : SQuiet (checkFun (resolveNames ss0).1 Γ.aliases (constsOf Γ.vars) f) ()) :
    ProgOk inferEnv (resolveNames ss0).1 (funEntry (resolveNames ss0).1 Γ ρ f).toList := by
  intro f' sig fi hs hfi
  obtain ⟨name, tps, params, ret, body⟩ := f
  simp only [funEntry] at hfi
  cases hsn : (resolveNames ss0).1.lookup name with
  | none => simp [hsn] at hfi
  | some sig0 =>
    simp only [hsn] at hfi
    split at hfi
    · rename_i hcond
      simp only [Bool.and_eq_true, beq_iff_eq] at hcond
      obtain ⟨⟨htp, hlen⟩, tf⟩ := hcond
      simp only [Option.toList_some, List.lookup] at hfi
      split at hfi
      · rename_i he
        simp only [beq_iff_eq] at he
        subst he
        simp only [Option.some.injEq] at hfi
        subst hfi
        rw [hsn] at hs
        cases hs
        exact ⟨by simpa using hlen, fun θ hg hbd =>
          ⟨body_checks ss0 Γ.aliases (constsOf Γ.vars) f' tps params ret body _ hsn htp tf hf θ hg hbd,
            consts_agree hρ hc θ⟩⟩
      · simp at hfi
    · simp at hfi

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
      obtain ⟨⟨⟩, hf, h⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) h
      simp only [runTop]
      exact runTop_sound ss0 k d ss Γ ρ _ log h hb hl hr hρ hc (hp.append (funEntry_ok ss0 hρ hc hf)) hlog
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
early with a value of its type `!()`, or stops at what is not run yet, at a
variable read before it is set, or where a loop or calls ran as long as they
were given. -/
theorem runScript_sound (ss : List Stmt) (h : checkScript ss = #[]) (k d : Nat) :
    TopOk (runScript k d ss).result ∧ ∀ p ∈ (runScript k d ss).log, HasType p.2 p.1 :=
  runTop_sound ss k d ss _ [] [] [] (checkScript_quiet h) ⟨rfl, rfl⟩ rfl rfl .nil
    (fun _ h => by simp at h) (fun _ _ _ _ h => by simp at h) (fun _ h => by simp at h)

end Datalove.Fun
