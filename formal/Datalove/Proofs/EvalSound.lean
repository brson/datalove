import Datalove.FunEval
import Datalove.Proofs.Expose
import Datalove.Proofs.InstanceCheck

/-!
# A datafun expression that checks evaluates to a value of its type

`Fun.evalS_sound`: where an expression checks quietly in a context whose
variables hold values of their types, evaluating it gives a value of its
type, or returns early with a value of the function's return type, or
reaches a form the evaluator does not yet take (`Stop.unsupported`); never
an operand of the wrong kind, and never running out of fuel, which the
evaluator spends as the checker does. It holds whatever inference says,
since what inference finds is checked before it is used. It is of contexts
with no type parameter's bound in them, as an instance's are, so it holds of
a generic through the check-once theorem.
-/

namespace Datalove.Lit

theorem Ty.expose_of_not_mu {t : Ty} (h : t.isMu = false) : t.expose = t := by
  simp [Ty.expose, Ty.leadingMus_of_not_mu h, Ty.exposeN]

/-- A value of a type is a value of its unfolding. -/
theorem HasType.expose {v : Val} {t : Ty} (h : HasType v t) : HasType v t.expose := by
  induction h with
  | roll h' _ => exact h'
  | hexInt _ ih => exact .hexInt ih
  | @int t _ hr => rw [Ty.expose_of_not_mu (by cases t <;> simp_all [Ty.isMu, inRange])]; exact .int hr
  | @hex t _ hr => rw [Ty.expose_of_not_mu (by cases t <;> simp_all [Ty.isMu, hexFits])]; exact .hex hr
  | @float t hr => rw [Ty.expose_of_not_mu (by cases t <;> simp_all [Ty.isMu, Ty.isFloat])]; exact .float hr
  | bool => exact .bool
  | str => exact .str
  | tuple a b => exact .tuple a b
  | struct a b => exact .struct a b
  | list a => exact .list a
  | set a => exact .set a
  | map a b => exact .map a b
  | tensor a b c => exact .tensor a b c
  | table a b c => exact .table a b c
  | none => exact .none
  | some a => exact .some a
  | ok a => exact .ok a
  | er a => exact .er a
  | data => exact .data
  | error => exact .error
  | atom => exact .atom
  | atomEnum a => exact .atomEnum a
  | term a => exact .term a
  | termEnum a b => exact .termEnum a b

theorem HasType.bool_inv {v : Val} (h : HasType v .bool) : ∃ b, v = .bool b := by
  generalize ht : Ty.bool = t at h
  induction h with
  | bool => exact ⟨_, rfl⟩
  | hexInt _ ih => obtain ⟨_, h⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.num_inv {v : Val} {t : Ty} (h : HasType v t) (ht : t.isFixedInt = true ∨ t = .int) :
    (Fun.numOf v).isSome = true := by
  induction h with
  | int => rfl
  | hex => rfl
  | hexInt => rfl
  | @roll _ t' _ ih =>
    have : t'.isMu = false := by
      rcases ht with ht | ht
      · cases t' <;> simp_all [Ty.isFixedInt, Ty.isMu]
      · subst ht; rfl
    exact ih (by rw [Ty.expose_of_not_mu this]; exact ht)
  | @float t' hr =>
    rcases ht with ht | ht
    · cases t' <;> simp_all [Ty.isFixedInt, Ty.isFloat]
    · subst ht; simp [Ty.isFloat] at hr
  | _ => rcases ht with ht | ht <;> simp_all [Ty.isFixedInt]

theorem HasType.float_inv {v : Val} {t : Ty} (h : HasType v t) (ht : t.isFloat = true) :
    Fun.floatVal v = true := by
  induction h with
  | float => rfl
  | hex => rfl
  | hexInt => rfl
  | @roll _ t' _ ih =>
    have : t'.isMu = false := by cases t' <;> simp_all [Ty.isFloat, Ty.isMu]
    exact ih (by rw [Ty.expose_of_not_mu this]; exact ht)
  | @int t' _ hr => cases t' <;> simp_all [Ty.isFloat, inRange]
  | _ => simp_all [Ty.isFloat]

theorem HasType.option_inv {v : Val} {p : Ty} (h : HasType v (.option p)) :
    v = .none ∨ ∃ w, v = .some w ∧ HasType w p := by
  generalize ht : Ty.option p = t at h
  induction h with
  | none => exact .inl rfl
  | some hw => cases ht; exact .inr ⟨_, rfl, hw⟩
  | hexInt _ ih => rcases ih ht with h | ⟨_, h, -⟩ <;> cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.result_inv {v : Val} {p : Ty} (h : HasType v (.result p)) :
    (∃ w, v = .ok w ∧ HasType w p) ∨ (∃ w, v = .er w ∧ HasType w .error) := by
  generalize ht : Ty.result p = t at h
  induction h with
  | ok hw => cases ht; exact .inl ⟨_, rfl, hw⟩
  | er hw => exact .inr ⟨_, rfl, hw⟩
  | hexInt _ ih => rcases ih ht with ⟨_, h, -⟩ | ⟨_, h, -⟩ <;> cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.atom_inv {v : Val} {a : String} (h : HasType v (.atom a)) : v = .atom a := by
  generalize ht : Ty.atom a = t at h
  induction h with
  | atom => cases ht; rfl
  | hexInt _ ih => cases ih ht
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.term_inv {v : Val} {a : String} {p : Ty} (h : HasType v (.term a p)) :
    ∃ w, v = .term a w ∧ HasType w p := by
  generalize ht : Ty.term a p = t at h
  induction h with
  | term hw => cases ht; exact ⟨_, rfl, hw⟩
  | hexInt _ ih => obtain ⟨_, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

/-- An integer type's value: an integer in its range, written in decimal or
hex. -/
theorem HasType.int_inv {v : Val} {t : Ty} (h : HasType v t) (ht : t.isFixedInt = true ∨ t = .int) :
    ∃ n, Fun.numOf v = Option.some n ∧ inRange t n ∧ (v = .int n ∨ ∃ k : Nat, v = .hex k ∧ (k : Int) = n) := by
  induction h with
  | @int t n hr => exact ⟨n, rfl, hr, .inl rfl⟩
  | @hex t k hr =>
    refine ⟨k, rfl, ?_, .inr ⟨k, rfl, rfl⟩⟩
    rcases ht with ht | ht
    · cases t <;> simp_all [Ty.isFixedInt, hexFits, inRange, indexBits] <;> omega
    · subst ht; trivial
  | @hexInt t k _ ih =>
    obtain ⟨n, hn, hr, hv⟩ := ih ht
    simp only [Fun.numOf, Option.some.injEq] at hn
    exact ⟨n, by simp [Fun.numOf, hn], hr, .inr ⟨k, rfl, hn⟩⟩
  | @roll _ t' _ ih =>
    have : t'.isMu = false := by
      rcases ht with ht | ht
      · cases t' <;> simp_all [Ty.isFixedInt, Ty.isMu]
      · subst ht; rfl
    rw [Ty.expose_of_not_mu this] at ih
    exact ih ht
  | @float t' hr =>
    rcases ht with ht | ht
    · cases t' <;> simp [Ty.isFloat] at hr <;> simp [Ty.isFixedInt] at ht
    · subst ht; simp [Ty.isFloat] at hr
  | _ => rcases ht with ht | ht <;> simp_all [Ty.isFixedInt]

/-- An integer in a type's range, as a value of a type it may be. -/
theorem HasType.of_num {v : Val} {n : Int} {t : Ty}
    (hv : v = .int n ∨ ∃ k : Nat, v = .hex k ∧ (k : Int) = n) (hr : inRange t n) : HasType v t := by
  rcases hv with rfl | ⟨k, rfl, rfl⟩
  · exact .int hr
  · exact .hexInt (.int hr)

theorem HasType.int_not_float {n : Int} {t : Ty} (h : HasType (.int n) t) : t.isFloat = false := by
  generalize hv : Val.int n = v at h
  induction h with
  | @int t _ hr => cases t <;> simp_all [inRange, Ty.isFloat]
  | @roll _ t _ ih =>
    have := ih hv
    cases t <;> simp_all [Ty.isFloat, Ty.expose, Ty.leadingMus, Ty.exposeN]
  | _ => cases hv

theorem HasType.float_hex {k : Nat} {t : Ty} (h : HasType (.hex k) t) (ht : t = .f32) : k < 2 ^ 32 := by
  subst ht
  generalize hv : Val.hex k = v at h
  generalize ht : Ty.f32 = t at h
  induction h with
  | hex hr => cases hv; subst ht; simpa [hexFits] using hr
  | hexInt hi => cases hv; subst ht; have := hi.int_not_float; simp [Ty.isFloat] at this
  | roll _ ih => subst ht; exact ih hv (by simp [Ty.expose, Ty.leadingMus, Ty.exposeN])
  | _ => first | cases hv | cases ht

theorem Variants.atomListed_of_hasAtom {a : String} : ∀ {vs : Variants}, vs.hasAtom a = true →
    Variants.atomListed a vs = true
  | .nil, h => by simp [Variants.hasAtom] at h
  | .atom n vs, h => by
    simp only [Variants.hasAtom, Bool.or_eq_true, Variants.atomListed] at h ⊢
    exact h.imp id atomListed_of_hasAtom
  | .term _ _ vs, h => by
    simp only [Variants.hasAtom, Variants.atomListed] at h ⊢
    exact atomListed_of_hasAtom h

theorem Variants.termPayload_of_hasTerm {a : String} {t : Ty} : ∀ {vs : Variants},
    vs.hasTerm a t = true → Variants.termPayload a vs = some t
  | .nil, h => by simp [Variants.hasTerm] at h
  | .atom _ vs, h => by
    simp only [Variants.hasTerm, Variants.termPayload] at h ⊢
    exact termPayload_of_hasTerm h
  | .term n p vs, h => by
    simp only [Variants.hasTerm, Variants.termPayload] at h ⊢
    split at h
    · rename_i hn; simp only [hn, ↓reduceIte, Ty.equiv_iff_eq] at h ⊢; rw [h]
    · rename_i hn; simp only [hn, Bool.false_eq_true, ↓reduceIte]; exact termPayload_of_hasTerm h

/-- What `@` converts keeps its value: a value of a type is a value of every
type the type adapts to. -/
theorem HasType.adapt {v : Val} {s d : Ty} (h : HasType v s) (ha : s.canAdapt d = true) : HasType v d := by
  simp only [Ty.canAdapt, Bool.or_eq_true] at ha
  rcases ha with (hm | hw) | ht
  · exact roll_of (Ty.meet_iff.mp hm).symm h.expose
  · have hs : s.isFixedInt = true ∨ s = .int := by cases s <;> cases d <;> simp_all [Ty.canWiden, Ty.isFixedInt]
    obtain ⟨n, -, hr, hv⟩ := h.int_inv hs
    exact HasType.of_num hv (by cases s <;> cases d <;> simp_all [Ty.canWiden, inRange] <;> omega)
  · have he := h.expose
    refine roll_of rfl ?_
    generalize s.expose = S at he ht
    generalize d.expose = D at ht
    cases S <;> cases D <;> simp only [Ty.adaptTable] at ht <;> try simp at ht
    case f32.f64 =>
      have hf := he.float_inv rfl
      cases v <;> simp [Fun.floatVal] at hf
      · rename_i k
        have := he.float_hex rfl
        exact .hex (by simp only [hexFits]; omega)
      · exact .float rfl
    case atom.enum a vs =>
      rw [he.atom_inv]; exact .atomEnum (Variants.atomListed_of_hasAtom ht)
    case term.enum a t vs =>
      obtain ⟨w, rfl, hw⟩ := he.term_inv
      exact .termEnum (Variants.termPayload_of_hasTerm ht) hw
    all_goals
      obtain ⟨n, -, hr, hv⟩ := he.int_inv (by simp [Ty.isFixedInt])
      exact HasType.of_num hv (by simp_all [inRange] <;> omega)

end Datalove.Lit

namespace Datalove.Fun

open Lit (Val HasType inRange roll_of)

/-- Every variable in scope holds a value of its type. -/
def EnvOk (Γ : Ctx) (ρ : VEnv) : Prop :=
  ∀ x t k, Γ.lookup x = Option.some (t, k) → ∃ v, ρ.lookup x = Option.some v ∧ HasType v t

/-- A value the function may return early: one of its return type. -/
def EarlyOk (R : Ret) (r : Val) : Prop := ∃ T, R = .ty T ∧ HasType r T

/-- Why evaluating a checked expression may give no value: an early return
the function's type allows, or a form this stage does not take. -/
def StopOk (R : Ret) : Stop → Prop
  | .early r => EarlyOk R r
  | .unsupported => True
  | .fuel | .wrong => False

/-- What evaluation gives meets `P`, or stops as a checked expression may. -/
def Sound {α} (R : Ret) (P : α → Prop) (m : EvalM α) : Prop :=
  match m with
  | .ok a => P a
  | .error s => StopOk R s

namespace Sound

variable {α β : Type} {R : Ret} {P : α → Prop} {Q : β → Prop}

theorem pure' {a : α} (h : P a) : Sound R P (Pure.pure a) := h

theorem bind' {x : EvalM α} {f : α → EvalM β} (hx : Sound R P x) (hf : ∀ a, P a → Sound R Q (f a)) :
    Sound R Q (x >>= f) := by
  cases x with
  | error s => exact hx
  | ok a => exact hf a hx

theorem map' {x : EvalM α} {f : α → β} (hx : Sound R P x) (hf : ∀ a, P a → Q (f a)) :
    Sound R Q (f <$> x) := by
  cases x with
  | error s => exact hx
  | ok a => exact hf a hx

theorem early {r : Val} (h : EarlyOk R r) : Sound R P (throw (.early r)) := h

theorem unsupported : Sound R P (throw .unsupported) := trivial

theorem weaken {x : EvalM α} {P' : α → Prop} (h : Sound R P x) (hp : ∀ a, P a → P' a) : Sound R P' x := by
  cases x with
  | error s => exact h
  | ok a => exact hp a h

end Sound

theorem EarlyOk.er_of {Γ : Ctx} {w : Val} (hr : Γ.returnsResult = true) (hw : HasType w .error) :
    EarlyOk Γ.ret (.er w) := by
  simp only [Ctx.returnsResult] at hr
  cases hR : Γ.ret with
  | void => simp [hR] at hr
  | ty T =>
    refine ⟨T, rfl, ?_⟩
    simp only [hR] at hr
    split at hr
    · rename_i p hp; exact roll_of hp (.er hw)
    · simp at hr

theorem EarlyOk.none_of {Γ : Ctx} (hr : Γ.returnsOption = true) : EarlyOk Γ.ret .none := by
  simp only [Ctx.returnsOption] at hr
  cases hR : Γ.ret with
  | void => simp [hR] at hr
  | ty T =>
    refine ⟨T, rfl, ?_⟩
    simp only [hR] at hr
    split at hr
    · rename_i p hp; exact roll_of hp .none
    · simp at hr

theorem inRangeB_sound {t : Ty} {r : Int} (h : inRangeB t r = true) : inRange t r := by
  simp only [inRangeB] at h
  split at h
  · rename_i lo hi c hl
    simp only [Bool.and_eq_true, decide_eq_true_eq] at h
    exact Lit.intRange_sound hl h.1 h.2
  · simp only [beq_iff_eq] at h; subst h; trivial

theorem boundOf_nil {Γ : Ctx} (hb : Γ.bounds = []) (t : Ty) : Γ.boundOf t = Option.none := by
  cases t <;> simp [Ctx.boundOf, hb]

theorem fallible_sound {R : Ret} {op : BinOp} {t : Ty} {r : Int}
    (hf : StopOk R (failOf (needs op))) : Sound R (HasType · t) (fallible op t r) := by
  simp only [fallible]
  split
  · rename_i h; exact .int (inRangeB_sound h)
  · exact hf

theorem failOf_ok {Γ : Ctx} {op : BinOp} (h : fits Γ (needs op) = true) (hn : needs op ≠ .nothing) :
    StopOk Γ.ret (failOf (needs op)) := by
  revert h hn
  cases needs op <;> simp only [fits, failOf, StopOk] <;> intro h hn
  · exact absurd rfl hn
  · exact EarlyOk.er_of h .error
  · exact EarlyOk.none_of h

theorem binOp_sound {Γ : Ctx} (hb : Γ.bounds = []) {op : BinOp} {t r : Ty} {va vb : Val}
    (hf : Quiet (finishBinop Γ op t) r) (ha : HasType va t) (hb' : HasType vb t) :
    Sound Γ.ret (HasType · r) (binOp op t va vb) := by
  cases op <;> simp [finishBinop, boundOf_nil hb, binOp] at hf ⊢
  all_goals first
    | done
    | (split at hf
       · rw [Quiet.pure_iff] at hf; subst hf; exact Sound.pure' HasType.bool
       · simp at hf)
    | (split at hf
       · rename_i hc
         rw [Quiet.pure_iff] at hf; subst hf
         try simp only [beq_iff_eq] at hc
         subst hc
         obtain ⟨a, rfl⟩ := ha.bool_inv
         obtain ⟨b, rfl⟩ := hb'.bool_inv
         exact Sound.pure' HasType.bool
       · simp at hf)
    | (split at hf
       · simp at hf
       rename_i hadm
       split at hf
       · simp at hf
       rename_i hfit
       rw [Quiet.pure_iff] at hf; subst hf
       simp only [Bool.not_eq_false] at hadm hfit
       by_cases hfl : t.isFloat = true
       · have h1 := ha.float_inv hfl
         have h2 := hb'.float_inv hfl
         simp only [hfl, h1, h2, and_self, ↓reduceIte]
         exact Sound.pure' (HasType.float hfl)
       · have hint : t.isFixedInt = true ∨ t = .int := by
           cases t <;> simp_all [arithAdmits, isFloatT, isFixT, Ty.isFloat, Ty.isFixedInt]
         obtain ⟨a, hna, -, -⟩ := ha.int_inv hint
         obtain ⟨b, hnb, -, -⟩ := hb'.int_inv hint
         simp only [hfl, Bool.false_eq_true, ↓reduceIte, hna, hnb]
         first
           | (exfalso; cases t <;> simp_all [arithAdmits, isFloatT, isFixT, Ty.isFloat, Ty.isFixedInt] <;> done)
           | (have : t = .int := by
                cases t <;> simp_all [arithAdmits, isFloatT, isFixT, Ty.isFloat, Ty.isFixedInt]
              subst this; exact Sound.pure' (HasType.int trivial))
           | exact fallible_sound (failOf_ok hfit (by simp [needs]))
           | (split
              · exact failOf_ok hfit (by simp [needs])
              · exact fallible_sound (failOf_ok hfit (by simp [needs]))))


@[simp] theorem Ctx.enter_bounds (Γ : Ctx) (e : Expr) : (Γ.enter e).bounds = Γ.bounds := rfl
@[simp] theorem Ctx.borrow_bounds (Γ : Ctx) : Γ.borrow.bounds = Γ.bounds := rfl

theorem EnvOk.enter {Γ : Ctx} {ρ : VEnv} (h : EnvOk Γ ρ) (e : Expr) : EnvOk (Γ.enter e) ρ := h
theorem EnvOk.borrow {Γ : Ctx} {ρ : VEnv} (h : EnvOk Γ ρ) : EnvOk Γ.borrow ρ := h

theorem typeOf_quiet {m : ChkM Ty} {t : Ty} (h : Quiet m t) : typeOf m = .ok t := by
  simp only [typeOf]; rw [h]; rfl

theorem litBody_checks {env : InferEnv} {n : Nat} {Γ : Ctx} {e : Expr} {t : Ty} (hl : e.isLit = true)
    (h : Quiet (synthF env (n + 1) Γ e) t) : Quiet (checkForm env n (Γ.enter e) e t) () := by
  rw [synthF_lit hl] at h
  simp only [litBody] at h
  obtain ⟨u, -, h⟩ := h.bind_inv
  obtain ⟨_, hc, h⟩ := h.bind_inv
  rw [Quiet.pure_iff] at h; subst h
  exact hc

section
variable (env : InferEnv)

mutual

/-- **A datafun expression that checks evaluates to a value of its type**,
or returns early with a value of the function's return type, or reaches a
form this stage does not evaluate: in a context with no type parameter's
bound, whose variables hold values of their types, and for any inference. -/
theorem evalS_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Expr) (t : Ty), Quiet (synthF env n Γ e) t →
    Γ.bounds = [] → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalS env n Γ ρ e)
  | 0, _, _, _, _, h, _, _ => by simp [synthF] at h
  | n + 1, Γ, ρ, e, t, h, hb, hρ => by
    have h0 := h
    cases e
    case hinted hh inner =>
      simp only [synthF] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨_, hc, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      rw [Quiet.liftE_iff] at hu
      simp only [evalS, hu]
      exact Sound.bind' (P := (· = u)) rfl (fun _ e => e ▸ evalForm_sound n _ ρ inner u hc hb (hρ.enter _))
    case var x =>
      simp only [synthF] at h
      cases hl : (Γ.enter (.var x)).lookup x with
      | none => simp [hl] at h
      | some p =>
        obtain ⟨u, k⟩ := p
        simp only [hl] at h
        split at h
        · simp at h
        rw [Quiet.pure_iff] at h; subst h
        obtain ⟨v, hv, hvt⟩ := hρ x u k hl
        simp only [evalS, hv]
        exact Sound.pure' hvt
    case binop op a b =>
      simp only [synthF] at h
      obtain ⟨u, hp, hf⟩ := h.bind_inv
      simp only [evalS]
      exact Sound.bind' (evalPair_sound n _ ρ a b none u hp hb (hρ.enter _)) (fun r ⟨hr, ha, hb'⟩ => by
        obtain ⟨_, va, vb⟩ := r
        simp only at hr; subst hr
        exact binOp_sound (Γ := Γ.enter _) hb hf ha hb')
    case tryOpt a =>
      simp only [synthF] at h
      obtain ⟨u, ha, h⟩ := h.bind_inv
      split at h
      · rename_i p hp
        split at h
        · rename_i hr
          rw [Quiet.pure_iff] at h; subst h
          simp only [evalS]
          refine Sound.bind' (evalS_sound n _ ρ a u ha hb (hρ.enter _)) (fun v hv => ?_)
          have hv' := hv.expose
          rw [hp] at hv'
          rcases hv'.option_inv with rfl | ⟨w, rfl, hw⟩
          · exact Sound.early (EarlyOk.none_of hr)
          · exact Sound.pure' hw
        · simp at h
      · simp at h
    case tryRes a =>
      simp only [synthF] at h
      obtain ⟨u, ha, h⟩ := h.bind_inv
      split at h
      · rename_i p hp
        split at h
        · rename_i hr
          rw [Quiet.pure_iff] at h; subst h
          simp only [evalS]
          refine Sound.bind' (evalS_sound n _ ρ a u ha hb (hρ.enter _)) (fun v hv => ?_)
          have hv' := hv.expose
          rw [hp] at hv'
          rcases hv'.result_inv with ⟨w, rfl, hw⟩ | ⟨w, rfl, hw⟩
          · exact Sound.pure' hw
          · exact Sound.early (EarlyOk.er_of hr hw)
        · simp at h
      · simp at h
    case adapt a =>
      simp only [synthF] at h
      simp only [evalS]
      exact evalS_sound n _ ρ a t h hb (hρ.enter _).borrow
    case call | place | proj | index => simp only [evalS]; exact Sound.unsupported
    case unop op a =>
      have hb0 : (Γ.enter (.unop op a)).bounds = [] := hb
      cases op
      case neg =>
        simp only [synthF] at h
        obtain ⟨u, ha, h⟩ := h.bind_inv
        simp only [evalS, typeOf_quiet ha]
        refine Sound.bind' (P := (u = ·)) rfl (fun _ e => ?_)
        subst e
        refine Sound.bind' (evalS_sound n _ ρ a u ha hb (hρ.enter _).borrow) (fun v hv => ?_)
        split at h
        · rename_i hc
          rw [Quiet.pure_iff] at h; subst h
          simp only [boundOf_nil hb0, isFloatT] at hc
          simp only [unOp]
          by_cases hfl : u.isFloat = true
          · simp only [hfl, ↓reduceIte, hv.float_inv hfl]; exact Sound.pure' (HasType.float hfl)
          · have hu : u = Ty.int := by simp_all
            subst hu
            obtain ⟨k, hk, -, -⟩ := hv.int_inv (.inr rfl)
            simp only [hfl, Bool.false_eq_true, ↓reduceIte, hk]
            exact Sound.pure' (HasType.int trivial)
        · simp at h
      case not =>
        simp only [synthF] at h
        obtain ⟨u, ha, h⟩ := h.bind_inv
        simp only [evalS, typeOf_quiet ha]
        refine Sound.bind' (P := (u = ·)) rfl (fun _ e => ?_)
        subst e
        refine Sound.bind' (evalS_sound n _ ρ a u ha hb (hρ.enter _).borrow) (fun v hv => ?_)
        split at h
        · rename_i hc
          rw [Quiet.pure_iff] at h; subst h
          simp only [beq_iff_eq] at hc; subst hc
          obtain ⟨b, rfl⟩ := hv.bool_inv
          exact Sound.pure' HasType.bool
        · simp at h
      case negC =>
        simp only [synthF] at h
        obtain ⟨u, ha, h⟩ := h.bind_inv
        simp only [evalS, typeOf_quiet ha]
        refine Sound.bind' (P := (u = ·)) rfl (fun _ e => ?_)
        subst e
        refine Sound.bind' (evalS_sound n _ ρ a u ha hb (hρ.enter _).borrow) (fun v hv => ?_)
        split at h
        · simp at h
        rename_i hc
        split at h
        · simp at h
        rename_i hr
        rw [Quiet.pure_iff] at h; subst h
        have hfix : u.isFixedInt = true := by simpa [boundOf_nil hb0, isFixT] using hc
        have hr' : (Γ.enter (.unop .negC a)).returnsResult = true := by simpa using hr
        obtain ⟨k, hk, -, -⟩ := hv.int_inv (.inl hfix)
        simp only [unOp, hk]
        split
        · rename_i hin; exact Sound.pure' (HasType.int (inRangeB_sound hin))
        · exact Sound.early (EarlyOk.er_of hr' .error)
      case negO =>
        simp only [synthF] at h
        obtain ⟨u, ha, h⟩ := h.bind_inv
        simp only [evalS, typeOf_quiet ha]
        refine Sound.bind' (P := (u = ·)) rfl (fun _ e => ?_)
        subst e
        refine Sound.bind' (evalS_sound n _ ρ a u ha hb (hρ.enter _).borrow) (fun v hv => ?_)
        split at h
        · simp at h
        rename_i hc
        split at h
        · simp at h
        rename_i hr
        rw [Quiet.pure_iff] at h; subst h
        have hfix : u.isFixedInt = true := by simpa [boundOf_nil hb0, isFixT] using hc
        have hr' : (Γ.enter (.unop .negO a)).returnsOption = true := by simpa using hr
        obtain ⟨k, hk, -, -⟩ := hv.int_inv (.inl hfix)
        simp only [unOp, hk]
        split
        · rename_i hin; exact Sound.pure' (HasType.int (inRangeB_sound hin))
        · exact Sound.early (EarlyOk.none_of hr')
    all_goals
      have hc := litBody_checks (by rfl) h
      simp only [evalS, typeOf_quiet h0]
      exact Sound.bind' (P := (· = t)) rfl (fun _ e => e ▸ evalForm_sound n _ ρ _ t hc hb (hρ.enter _))

theorem evalC_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Expr) (t : Ty), Quiet (checkF env n Γ e t) () →
    Γ.bounds = [] → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalC env n Γ ρ e t)
  | 0, _, _, _, _, h, _, _ => by simp [checkF] at h
  | n + 1, Γ, ρ, e, t, h, hb, hρ => by
    cases e
    case hinted hh inner =>
      simp only [checkF] at h
      obtain ⟨u, -, h⟩ := h.bind_inv
      split at h
      · simp at h
      simp only [evalC]
      exact evalForm_sound n _ ρ inner t h hb (hρ.enter _)
    all_goals
      simp only [checkF] at h
      simp only [evalC]
      exact evalForm_sound n _ ρ _ t h hb (hρ.enter _)

theorem evalForm_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Expr) (t : Ty), Quiet (checkForm env n Γ e t) () →
    Γ.bounds = [] → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalForm env n Γ ρ e t)
  | 0, _, _, _, _, h, _, _ => by simp [checkForm] at h
  | n + 1, Γ, ρ, e, t, h, hb, hρ => by
    cases e
    case none_ =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq; exact Sound.pure' (roll_of hq .none)
      · simp at h
    case er p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq
        simp only [hq]
        exact Sound.map' (evalC_sound n Γ ρ p .error h hb hρ) (fun v hv => roll_of hq (.er hv))
      · simp at h
    case some_ p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq
        simp only [hq]
        exact Sound.map' (evalC_sound n Γ ρ p q h hb hρ) (fun v hv => roll_of hq (.some hv))
      · simp at h
    case ok p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq
        simp only [hq]
        exact Sound.map' (evalC_sound n Γ ρ p q h hb hρ) (fun v hv => roll_of hq (.ok hv))
      · simp at h
    case tt | ff =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; subst hc; exact Sound.pure' .bool
      · simp at h
    case str =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; subst hc; exact Sound.pure' .str
      · simp at h
    case int k =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; exact Sound.pure' (roll_of hc (.int trivial))
      · split at h
        · rename_i lo hi c hr
          split at h
          · rename_i hin
            simp only [Bool.and_eq_true, decide_eq_true_eq] at hin
            exact Sound.pure' (roll_of rfl (.int (Lit.intRange_sound hr hin.1 hin.2)))
          · simp at h
        · simp at h
    case hex k =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; exact Sound.pure' (roll_of hc (.hex trivial))
      · split at h
        · rename_i bits c hr
          split at h
          · rename_i hin
            exact Sound.pure' (roll_of rfl (.hex (Lit.hexBits_sound hr (by simpa using hin))))
          · simp at h
        · simp at h
    case float =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hx; exact Sound.pure' (roll_of hx (.float rfl))
      · rename_i hx; exact Sound.pure' (roll_of hx (.float rfl))
      · simp at h
    case tuple es =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i ts hx
        simp only [hx]
        split at h
        · simp at h
        rename_i hlen
        simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hlen
        exact Sound.map' (evalPairs_sound n Γ ρ es _ h hb hρ) (fun vs ⟨hl, hvs⟩ =>
          roll_of hx (.tuple (by rw [hl, hlen, Nat.min_self]) hvs))
      · simp at h
    case struct fs =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i ts hx
        simp only [hx]
        split at h
        · simp at h
        rename_i hlen
        simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hlen
        exact Sound.map' (evalFields_sound n Γ ρ fs _ h hb hρ) (fun vs ⟨hl, hvs⟩ =>
          roll_of hx (.struct (by
            rw [hl, show (fun x : (String × Expr) × (String × Ty) => x.2.1) = Prod.fst ∘ Prod.snd from rfl,
              ← List.map_map, List.map_snd_zip (by omega)]) hvs))
      · simp at h
    case list es | set es =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i u hx
        simp only [hx]
        first
          | exact Sound.map' (evalAll_sound n Γ ρ es u h hb hρ) (fun vs ⟨_, hvs⟩ => roll_of hx (.list hvs))
          | exact Sound.map' (evalAll_sound n Γ ρ es u h hb hρ) (fun vs ⟨_, hvs⟩ => roll_of hx (.set hvs))
      · simp at h
    case map es =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i k v hx
        simp only [hx]
        exact Sound.map' (evalEntries_sound n Γ ρ es k v h hb hρ) (fun ps hps =>
          roll_of hx (.map (fun p hp => (hps p hp).1) (fun p hp => (hps p hp).2)))
      · simp at h
    case tensor shape es =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i u r hx
        simp only [hx]
        split at h
        · simp at h
        rename_i hr
        split at h
        · simp at h
        rename_i hl
        simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hr hl
        exact Sound.map' (evalAll_sound n Γ ρ es u h hb hρ) (fun vs ⟨hvl, hvs⟩ =>
          roll_of hx (.tensor hr (by rw [hvl, hl]) hvs))
      · simp at h
    case table header rows =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i cols hx
        simp only [hx]
        split at h
        · simp at h
        split at h
        · simp at h
        rename_i hh
        simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hh
        exact Sound.map' (evalRows_sound n Γ ρ rows _ h hb hρ) (fun rs hrs =>
          roll_of hx (.table hh (fun r hr => (hrs r hr).1) (fun r hr => (hrs r hr).2)))
      · simp at h
    case atom a =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i b hx
        split at h
        · rename_i hab; simp only [beq_iff_eq] at hab; subst hab; exact Sound.pure' (roll_of hx .atom)
        · simp at h
      · rename_i vs hx
        split at h
        · rename_i hl; exact Sound.pure' (roll_of hx (.atomEnum hl))
        · simp at h
      · simp at h
    case term a p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i b u hx
        simp only [hx]
        split at h
        · rename_i hab; simp only [beq_iff_eq] at hab; subst hab
          exact Sound.map' (evalC_sound n Γ ρ p u h hb hρ) (fun v hv => roll_of hx (.term hv))
        · simp at h
      · rename_i vs hx
        simp only [hx]
        split at h
        · rename_i u hu
          simp only [hu]
          exact Sound.map' (evalC_sound n Γ ρ p u h hb hρ) (fun v hv => roll_of hx (.termEnum hu hv))
        · simp at h
      · simp at h
    case enum inner =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · exact evalC_sound n Γ ρ inner t h hb hρ
      · simp at h
    case data p | error p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hx
        obtain ⟨u, hu, -⟩ := h.bind_inv
        first
          | exact Sound.bind' (evalS_sound n Γ ρ p u hu hb hρ) (fun _ _ => Sound.pure' (roll_of hx .data))
          | exact Sound.bind' (evalS_sound n Γ ρ p u hu hb hρ) (fun _ _ => Sound.pure' (roll_of hx .error))
      · simp at h
    case binop op a b =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hy
        simp only [hy, ↓reduceIte]
        obtain ⟨u, hp, h⟩ := h.bind_inv
        obtain ⟨r, hf, h⟩ := h.bind_inv
        split at h
        · simp at h
        rename_i hr
        simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hr
        subst hr
        exact Sound.bind' (evalPair_sound n Γ ρ a b _ u hp hb hρ) (fun q ⟨hq, ha, hb'⟩ => by
          obtain ⟨_, va, vb⟩ := q
          simp only at hq; subst hq
          exact binOp_sound hb hf ha hb')
      · rename_i hy
        simp only [hy, Bool.false_eq_true, ↓reduceIte]
        exact evalSub_sound n Γ ρ _ t h hb hρ
    case adapt a =>
      simp only [checkForm] at h; simp only [evalForm]
      obtain ⟨u, hu, h⟩ := h.bind_inv
      split at h
      · simp at h
      rename_i hc
      exact (evalS_sound n _ ρ a u hu hb hρ.borrow).weaken (fun v hv => hv.adapt (by simpa using hc))
    case call => simp only [evalForm]; exact Sound.unsupported
    case unop op a =>
      cases op
      case neg =>
        have hb0 : Γ.boundOf t.expose = Option.none := boundOf_nil hb _
        have floatCase : ∀ v, HasType v t.expose →
            (isFloatT t.expose || Γ.boundOf t.expose == Option.some .float) = true →
            Sound Γ.ret (HasType · t) (unOp .neg t.expose v) := by
          intro v hv hf
          simp only [hb0, isFloatT] at hf
          have hfl : t.expose.isFloat = true := by simpa using hf
          simp only [unOp, hfl, ↓reduceIte, hv.float_inv hfl]
          exact Sound.pure' (roll_of rfl (.float hfl))
        simp only [checkForm] at h; simp only [evalForm]
        split at h
        · rename_i k
          split at h
          · rename_i lo hi c hr
            simp only [hr]
            split at h
            · rename_i hin
              simp only [Bool.and_eq_true, decide_eq_true_eq] at hin
              exact Sound.pure' (roll_of rfl (.int (Lit.intRange_sound hr hin.1 hin.2)))
            · simp at h
          · rename_i hr
            simp only [hr]
            split at h
            · rename_i hf
              simp only [hf, ↓reduceIte]
              exact Sound.bind' (evalC_sound n _ ρ _ _ h hb hρ.borrow) (fun v hv => floatCase v hv hf)
            · rename_i hf
              simp only [hf, Bool.false_eq_true, ↓reduceIte]
              exact evalSub_sound n Γ ρ _ t h hb hρ
        · rename_i hne
          split
          · rename_i k; exact absurd rfl (hne k)
          · split at h
            · rename_i hf
              simp only [hf, ↓reduceIte]
              exact Sound.bind' (evalC_sound n _ ρ _ _ h hb hρ.borrow) (fun v hv => floatCase v hv hf)
            · rename_i hf
              simp only [hf, Bool.false_eq_true, ↓reduceIte]
              exact evalSub_sound n Γ ρ _ t h hb hρ
      all_goals
        simp only [checkForm] at h; simp only [evalForm]
        exact evalSub_sound n Γ ρ _ t h hb hρ
    all_goals
      simp only [checkForm] at h; simp only [evalForm]
      exact evalSub_sound n Γ ρ _ t h hb hρ

theorem evalSub_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Expr) (t : Ty), Quiet (subsumeF env n Γ e t) () →
    Γ.bounds = [] → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalSub env n Γ ρ e)
  | 0, _, _, _, _, h, _, _ => by simp [subsumeF] at h
  | n + 1, Γ, ρ, e, t, h, hb, hρ => by
    simp only [subsumeF] at h
    obtain ⟨u, hu, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm
      simp only [evalSub]
      exact (evalS_sound n Γ ρ e u hu hb hρ).weaken
        (fun v hv => roll_of (Ty.meet_iff.mp (by simpa using hm)).symm hv.expose)

theorem evalPairs_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (es : List Expr) (ts : List Ty),
    Quiet (pairsF env n Γ es ts) () → Γ.bounds = [] → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.length = min es.length ts.length ∧ ∀ p ∈ vs.zip ts, HasType p.1 p.2)
      (evalPairs env n Γ ρ es ts)
  | 0, _, _, _, _, h, _, _ => by simp [pairsF] at h
  | n + 1, Γ, ρ, e :: es, t :: ts, h, hb, hρ => by
    simp only [pairsF] at h
    obtain ⟨_, he, h⟩ := h.bind_inv
    simp only [evalPairs]
    exact Sound.bind' (evalC_sound n Γ ρ e t he hb hρ) (fun v hv =>
      Sound.bind' (evalPairs_sound n Γ ρ es ts h hb hρ) (fun vs ⟨hl, hvs⟩ =>
        Sound.pure' ⟨by simp [hl, Nat.succ_min_succ], by
          intro p hp
          simp only [List.zip_cons_cons, List.mem_cons] at hp
          rcases hp with rfl | hp
          · exact hv
          · exact hvs p hp⟩))
  | n + 1, Γ, ρ, [], _, h, hb, hρ => by simp only [evalPairs]; exact Sound.pure' ⟨by simp, by simp⟩
  | n + 1, Γ, ρ, _ :: _, [], h, hb, hρ => by simp only [evalPairs]; exact Sound.pure' ⟨by simp, by simp⟩

theorem evalFields_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (fs : List (String × Expr)) (ts : List (String × Ty)),
    Quiet (fieldsF env n Γ fs ts) () → Γ.bounds = [] → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.map (·.1) = (fs.zip ts).map (·.2.1) ∧ ∀ p ∈ vs.zip ts, HasType p.1.2 p.2.2)
      (evalFields env n Γ ρ fs ts)
  | 0, _, _, _, _, h, _, _ => by simp [fieldsF] at h
  | n + 1, Γ, ρ, (m, e) :: fs, (k, t) :: ts, h, hb, hρ => by
    simp only [fieldsF] at h
    split at h
    · simp at h
    rename_i hmk
    obtain ⟨_, he, h⟩ := h.bind_inv
    simp only [evalFields]
    exact Sound.bind' (evalC_sound n Γ ρ e t he hb hρ) (fun v hv =>
      Sound.bind' (evalFields_sound n Γ ρ fs ts h hb hρ) (fun vs ⟨hl, hvs⟩ =>
        Sound.pure' ⟨by simp_all, by
          intro p hp
          simp only [List.zip_cons_cons, List.mem_cons] at hp
          rcases hp with rfl | hp
          · exact hv
          · exact hvs p hp⟩))
  | n + 1, Γ, ρ, [], _, h, hb, hρ => by simp only [evalFields]; exact Sound.pure' ⟨by simp, by simp⟩
  | n + 1, Γ, ρ, _ :: _, [], h, hb, hρ => by simp only [evalFields]; exact Sound.pure' ⟨by simp, by simp⟩

theorem evalAll_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (es : List Expr) (t : Ty),
    Quiet (allF env n Γ es t) () → Γ.bounds = [] → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.length = es.length ∧ ∀ v ∈ vs, HasType v t) (evalAll env n Γ ρ es t)
  | 0, _, _, _, _, h, _, _ => by simp [allF] at h
  | n + 1, Γ, ρ, [], t, h, hb, hρ => by simp only [evalAll]; exact Sound.pure' ⟨rfl, by simp⟩
  | n + 1, Γ, ρ, e :: es, t, h, hb, hρ => by
    simp only [allF] at h
    obtain ⟨_, he, h⟩ := h.bind_inv
    simp only [evalAll]
    exact Sound.bind' (evalC_sound n Γ ρ e t he hb hρ) (fun v hv =>
      Sound.bind' (evalAll_sound n Γ ρ es t h hb hρ) (fun vs ⟨hl, hvs⟩ =>
        Sound.pure' ⟨by simp [hl], by
          intro w hw
          simp only [List.mem_cons] at hw
          rcases hw with rfl | hw
          · exact hv
          · exact hvs w hw⟩))

theorem evalEntries_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (es : List (Expr × Expr)) (k v : Ty),
    Quiet (entriesF env n Γ es k v) () → Γ.bounds = [] → EnvOk Γ ρ →
    Sound Γ.ret (fun ps => ∀ p ∈ ps, HasType p.1 k ∧ HasType p.2 v) (evalEntries env n Γ ρ es k v)
  | 0, _, _, _, _, _, h, _, _ => by simp [entriesF] at h
  | n + 1, Γ, ρ, [], k, v, h, hb, hρ => by simp only [evalEntries]; exact Sound.pure' (by simp)
  | n + 1, Γ, ρ, (ke, ve) :: es, k, v, h, hb, hρ => by
    simp only [entriesF] at h
    obtain ⟨_, hk, h⟩ := h.bind_inv
    obtain ⟨_, hv, h⟩ := h.bind_inv
    simp only [evalEntries]
    exact Sound.bind' (evalC_sound n Γ ρ ke k hk hb hρ) (fun kv hkv =>
      Sound.bind' (evalC_sound n Γ ρ ve v hv hb hρ) (fun vv hvv =>
        Sound.bind' (evalEntries_sound n Γ ρ es k v h hb hρ) (fun rest hr =>
          Sound.pure' (by
            intro p hp
            simp only [List.mem_cons] at hp
            rcases hp with rfl | hp
            · exact ⟨hkv, hvv⟩
            · exact hr p hp))))

theorem evalRows_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (rows : List (List Expr)) (cols : List (String × Ty)),
    Quiet (rowsF env n Γ rows cols) () → Γ.bounds = [] → EnvOk Γ ρ →
    Sound Γ.ret (fun rs => ∀ r ∈ rs, r.length = cols.length ∧ ∀ p ∈ r.zip cols, HasType p.1 p.2.2)
      (evalRows env n Γ ρ rows cols)
  | 0, _, _, _, _, h, _, _ => by simp [rowsF] at h
  | n + 1, Γ, ρ, [], cols, h, hb, hρ => by simp only [evalRows]; exact Sound.pure' (by simp)
  | n + 1, Γ, ρ, r :: rs, cols, h, hb, hρ => by
    simp only [rowsF] at h
    split at h
    · simp at h
    rename_i hlen
    obtain ⟨_, hr, h⟩ := h.bind_inv
    simp only [evalRows]
    exact Sound.bind' (evalPairs_sound n Γ ρ r _ hr hb hρ) (fun vs ⟨hl, hvs⟩ =>
      Sound.bind' (evalRows_sound n Γ ρ rs cols h hb hρ) (fun rest hrest =>
        Sound.pure' (by
          intro w hw
          simp only [List.mem_cons] at hw
          rcases hw with rfl | hw
          · refine ⟨?_, ?_⟩
            · simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hlen
              simp [hl, hlen]
            · intro p hp
              have := hvs (p.1, p.2.2) (by
                rw [List.zip_map_right]
                exact List.mem_map.mpr ⟨p, hp, rfl⟩)
              exact this
          · exact hrest w hw)))

theorem evalPair_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (a b : Expr) (exp : Option Ty) (t : Ty),
    Quiet (pairF env n Γ a b exp) t → Γ.bounds = [] → EnvOk Γ ρ →
    Sound Γ.ret (fun r => r.1 = t ∧ HasType r.2.1 t ∧ HasType r.2.2 t) (evalPair env n Γ ρ a b exp)
  | 0, _, _, _, _, _, _, h, _, _ => by simp [pairF] at h
  | n + 1, Γ, ρ, a, b, exp, t, h, hb, hρ => by
    simp only [evalPair, typeOf_quiet h]
    simp only [pairF] at h
    obtain ⟨_, -, h⟩ := h.bind_inv
    obtain ⟨_, -, h⟩ := h.bind_inv
    split at h
    · simp at h
    obtain ⟨_, ha, h⟩ := h.bind_inv
    obtain ⟨_, hb', h⟩ := h.bind_inv
    rw [Quiet.pure_iff] at h; subst h
    refine Sound.bind' (P := (· = _)) rfl (fun t' ht => ?_)
    subst ht
    exact Sound.bind' (evalC_sound n _ ρ a _ ha (by simpa using hb) hρ.borrow) (fun va hva =>
      Sound.bind' (evalC_sound n _ ρ b _ hb' (by simpa using hb) hρ.borrow) (fun vb hvb =>
        Sound.pure' ⟨rfl, hva, hvb⟩))

end

end

end Datalove.Fun
