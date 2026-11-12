import Datalove.Proofs.EarlyReturn

/-!
# A `match` that is let through covers its enum

`Fun.checkMatch_exhaustive`: a `match` whose check reports nothing takes
apart an enum (F096), names each of its variants at most once (F097), with a
case of the variant's kind (F098) of a variant the enum has (F099), and,
with no `case default`, names every variant (F100). So each variant a value
can hold reaches exactly one case, or the default. That is what lets
`completes` say a `match` completes only by an arm that does, and the
semantics in `Proofs/Completes.lean` have no way of falling through one.

A variant is found by name, as the Rust finds it, so "exactly one" rests on
an enum's variants having distinct names, which the Rust's types do and the
model does not state.
-/

namespace Datalove.Fun

open Datalove.Lit (Code)

/-- A case names a variant of the enum, of the variant's kind. -/
def CaseFits (vs : List (String × Option Ty)) : Case → Prop
  | .atom a _ => vs.lookup a = some none
  | .term a _ _ => ∃ p, vs.lookup a = some (some p)

/-- What a `match` that reports nothing has: an input that synthesizes an
enum, and cases that name its variants each at most once, each a case of
the variant's kind, every variant named where there is no default. -/
def Covers (vs : List (String × Option Ty)) (cases : List Case) (dflt : Option (List Stmt)) : Prop :=
  (cases.map Case.name).Nodup ∧ (∀ c ∈ cases, CaseFits vs c) ∧
    (dflt = none → ∀ v ∈ vs, ∃ c ∈ cases, c.name = v.1)

theorem good_runChk_val {α} (m : ChkM α) :
    Good (runChk m) (fun _ => True) (fun r => ∃ a, r = Option.some a ∧ m.run = ⟨#[], .ok a⟩) := by
  intro d
  rcases hm : m.run with ⟨l, r⟩
  cases r with
  | ok a =>
    have e : (runChk m).run d = (Option.some a, d ++ l) := by
      simp only [runChk, hm]; rfl
    rw [e]
    refine ⟨by simp, trivial, fun hs => ⟨a, rfl, ?_⟩⟩
    simp at hs
    rw [hs]
  | error c =>
    have e : (runChk m).run d = (Option.none, (d ++ l).push c) := by
      simp only [runChk, hm]; rfl
    rw [e]
    refine ⟨by simp, trivial, fun hs => ?_⟩
    simp at hs

/-- The cases of a `match` that report nothing name what `Covers` asks, and
the names covered come back as the ones before and theirs. -/
theorem checkCases_quiet (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (vs : List (String × Option Ty))
    (cases : List Case) (covered : List String),
    Good (checkCases env n Γ vs cases covered) (fun _ => True)
      (fun cov => cov = covered ++ cases.map Case.name ∧ (∀ c ∈ cases, c.name ∉ covered) ∧
        (cases.map Case.name).Nodup ∧ ∀ c ∈ cases, CaseFits vs c)
  | 0, Γ, vs, cases, covered => by
    simp only [checkCases]
    exact good_trivial_push (fun _ => good_pure trivial trivial)
  | _ + 1, Γ, vs, [], covered => by
    simp only [checkCases]
    exact good_pure trivial (by simp)
  | n + 1, Γ, vs, c :: cs, covered => by
    have rest := checkCases_quiet env n Γ vs cs (covered ++ [c.name])
    rw [checkCases.eq_def]
    dsimp only
    split
    · exact good_trivial_push (fun _ => good_weaken (checkCases_quiet env n Γ vs cs covered)
        (fun _ h => h) (fun _ _ _ => trivial))
    · rename_i hnew
      refine good_bind (Px := fun _ => True) (Qx := fun _ => CaseFits vs c) ?_
        (fun _ _ => good_weaken rest (fun _ h => h) (fun cov _ hr hfit => ?_))
      · split
        · refine good_bind reportIf_quiet (fun _ _ => good_weaken (checkBlockF_good env n Γ _)
            (fun _ h => h) (fun _ _ _ hp => ?_))
          simp_all [CaseFits, Case.name]
        · refine good_weaken (checkBlockF_good env n _ _) (fun _ h => h) (fun _ _ _ => ?_)
          simp_all [CaseFits, Case.name]
        · exact good_bind good_push (fun _ _ => good_weaken (checkBlockF_good env n Γ _)
            (fun _ h => h) (fun _ _ _ h => h.elim))
        · exact good_bind good_push (fun _ _ => good_weaken (checkBlockF_good env n Γ _)
            (fun _ h => h) (fun _ _ _ h => h.elim))
      · obtain ⟨hcov, hout, hnd, hfits⟩ := hr
        have hc : c.name ∉ covered := by simpa using hnew
        refine ⟨by simp [hcov], ?_, ?_, ?_⟩
        · intro c' hc'
          simp only [List.mem_cons] at hc'
          rcases hc' with rfl | hc'
          · exact hc
          · exact fun h => hout c' hc' (by simp [h])
        · refine List.nodup_cons.mpr ⟨fun h => ?_, hnd⟩
          obtain ⟨c', hc', he⟩ := List.mem_map.mp h
          exact hout c' hc' (by simp [he])
        · intro c' hc'
          simp only [List.mem_cons] at hc'
          rcases hc' with rfl | hc'
          · exact hfit
          · exact hfits c' hc'

/-- A `match` whose check reports nothing takes apart an enum its input
synthesizes, and its cases cover it: each variant is named by exactly one
case of its kind, or there is a default. -/
theorem checkMatch_exhaustive (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Expr) (cases : List Case)
    (dflt : Option (List Stmt)) (d : Array Code)
    (quiet : ((checkStmtF env (n + 1) Γ (.match_ e cases dflt)).run d).2.size = d.size) :
    ∃ t vs, (synthF env n Γ e).run = ⟨#[], .ok t⟩ ∧ t.expose = .enum vs ∧
      Covers vs.toList cases dflt := by
  have g : Good (checkStmtF env (n + 1) Γ (.match_ e cases dflt)) (fun _ => True)
      (fun _ => ∃ t vs, (synthF env n Γ e).run = ⟨#[], .ok t⟩ ∧ t.expose = .enum vs ∧
        Covers vs.toList cases dflt) := by
    simp only [checkStmtF]
    refine good_bind (good_runChk_val _) (fun r _ => ?_)
    refine good_bind (Px := fun _ => True) (Qx := fun _ => ∀ t, r = Option.some t →
        ∃ vs, t.expose = .enum vs ∧ Covers vs.toList cases dflt) ?_
      (fun _ _ => good_pure trivial (fun hm hr => ?_))
    · cases r with
      | none => exact good_pure trivial (fun _ h => by simp at h)
      | some t =>
        dsimp only
        split
        · rename_i vs hvs
          refine good_bind (checkCases_quiet env n Γ vs.toList cases []) (fun cov _ => ?_)
          refine good_bind (good_weaken (checkElse_good env n Γ dflt
              (fun ss => checkBlockF_good env n Γ ss)) (fun _ h => h)
              (P' := fun _ => True) (Q' := fun _ => True) (fun _ _ _ => trivial))
            (fun _ _ => ?_)
          refine good_weaken reportIf_quiet (fun _ h => h) (fun _ _ hq _ hc t' ht' => ?_)
          obtain ⟨hcov, -, hnd, hfits⟩ := hc
          simp only [Option.some.injEq] at ht'
          subst ht'
          refine ⟨vs, hvs, hnd, hfits, fun hd v hv => ?_⟩
          subst hd
          simp only [Option.isNone_none, Bool.true_and, List.any_eq_false, Bool.not_eq_eq_eq_not,
            Bool.not_true] at hq
          have : v.1 ∈ cov := by simpa using hq v hv
          rw [hcov] at this
          simpa using this
        · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
    · obtain ⟨t, rfl, hrun⟩ := hr
      obtain ⟨vs, hvs, hcov⟩ := hm t rfl
      exact ⟨t, vs, hrun, hvs, hcov⟩
  exact (g d).2.2 quiet

end Datalove.Fun
