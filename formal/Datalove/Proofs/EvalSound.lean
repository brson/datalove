import Datalove.FunEval
import Datalove.Proofs.Expose
import Datalove.Proofs.InstanceCheck

/-!
# A datafun expression that checks evaluates to a value of its type

`Fun.evalS_sound`: where an expression checks quietly in a context whose
variables hold values of their types, evaluating it gives a value of its
type, or returns early with a value of the function's return type, or stops
where a call may: at what is not run yet, a variable read before it is set,
or calls nested as deep as they were given. Never an operand of the wrong
kind, and never running out of fuel, which the evaluator spends as the
checker does. It holds whatever inference says, since what inference finds
is checked before it is used, and for any calls that do what calls may
(`CallOk`), which `Proofs/CallSound.lean` shows of running a checked
program's bodies. It is of contexts with no type parameter's bound in them,
as an instance's are, so it holds of a generic through the check-once
theorem.
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
  | .atom n vs, h | .term n _ vs, h => by
    simp only [Variants.hasAtom, Variants.atomListed] at h ⊢
    by_cases hn : (n == a) = true
    · simp_all
    · simp only [hn, Bool.false_eq_true, ↓reduceIte] at h ⊢
      exact atomListed_of_hasAtom h

theorem Variants.termPayload_of_hasTerm {a : String} {t : Ty} : ∀ {vs : Variants},
    vs.hasTerm a t = true → Variants.termPayload a vs = some t
  | .nil, h => by simp [Variants.hasTerm] at h
  | .atom n vs, h => by
    simp only [Variants.hasTerm, Variants.termPayload] at h ⊢
    by_cases hn : (n == a) = true
    · simp_all
    · simp only [hn, Bool.false_eq_true, ↓reduceIte] at h ⊢
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


theorem HasType.tuple_inv {v : Val} {ts : Tys} (h : HasType v (.tuple ts)) :
    ∃ vs, v = .tuple vs ∧ vs.length = (tysList ts).length ∧ ∀ p ∈ vs.zip (tysList ts), HasType p.1 p.2 := by
  generalize ht : Ty.tuple ts = t at h
  induction h with
  | tuple hl hz => cases ht; exact ⟨_, rfl, hl, hz⟩
  | hexInt _ ih => obtain ⟨_, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.struct_inv {v : Val} {ts : Fields} (h : HasType v (.struct ts)) :
    ∃ fs, v = .struct fs ∧ fs.map (·.1) = (fieldsList ts).map (·.1) ∧
      ∀ p ∈ fs.zip (fieldsList ts), HasType p.1.2 p.2.2 := by
  generalize ht : Ty.struct ts = t at h
  induction h with
  | struct hl hz => cases ht; exact ⟨_, rfl, hl, hz⟩
  | hexInt _ ih => obtain ⟨_, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.list_inv {v : Val} {e : Ty} (h : HasType v (.list e)) :
    ∃ vs, v = .list vs ∧ ∀ w ∈ vs, HasType w e := by
  generalize ht : Ty.list e = t at h
  induction h with
  | list hw => cases ht; exact ⟨_, rfl, hw⟩
  | hexInt _ ih => obtain ⟨_, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.map_inv {v : Val} {k w : Ty} (h : HasType v (.map k w)) :
    ∃ es, v = .map es ∧ (∀ p ∈ es, HasType p.1 k) ∧ (∀ p ∈ es, HasType p.2 w) := by
  generalize ht : Ty.map k w = t at h
  induction h with
  | map hk hv => cases ht; exact ⟨_, rfl, hk, hv⟩
  | hexInt _ ih => obtain ⟨_, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem HasType.tensor_inv {v : Val} {e : Ty} {r : Nat} (h : HasType v (.tensor e r)) :
    ∃ shape vs, v = .tensor shape vs ∧ shape.length = r ∧ vs.length = shape.foldl (· * ·) 1 ∧
      ∀ w ∈ vs, HasType w e := by
  generalize ht : Ty.tensor e r = t at h
  induction h with
  | tensor hs hl hw => cases ht; exact ⟨_, _, rfl, hs, hl, hw⟩
  | hexInt _ ih => obtain ⟨_, _, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [inRange] at hr
  | hex hr => subst ht; simp [hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

end Datalove.Lit

namespace Datalove.Fun

open Lit (Val HasType inRange roll_of)

/-- Every variable in scope holds a value of its type, or none yet. -/
def EnvOk (Γ : Ctx) (ρ : VEnv) : Prop :=
  ∀ x t k, Γ.lookup x = Option.some (t, k) →
    ∃ o, ρ.lookup x = Option.some o ∧ ∀ v, o = Option.some v → HasType v t

/-- A value the function may return early: one of its return type. -/
def EarlyOk (R : Ret) (r : Val) : Prop := ∃ T, R = .ty T ∧ HasType r T

/-- Why evaluating a checked expression may give no value: an early return
the function's type allows, what is not evaluated yet, a variable read
before it was set, which the ownership analysis refuses, or calls nested as
deep as they were given. -/
def StopOk (R : Ret) : Stop → Prop
  | .early r => EarlyOk R r
  | .unsupported | .uninit | .diverge => True
  | .fuel | .wrong | .retVoid | .brk _ | .cont _ => False

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
       · rw [Quiet.pure_iff] at hf; subst hf
         split
         · exact Sound.pure' HasType.bool
         · exact Sound.unsupported
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
theorem EnvOk.forArg {Γ : Ctx} {ρ : VEnv} (h : EnvOk Γ ρ) (m : Mode) : EnvOk (Γ.forArg m) ρ := h

/-- A context evaluation runs in: no type parameter's bound, and the
program's functions. -/
def Ctx.Runs (sigs : List (String × Sig)) (Γ : Ctx) : Prop := Γ.bounds = [] ∧ Γ.sigs = sigs

/-- What a call gives: the function's return type, or the unit for one that
returns nothing. -/
def Sig.retTy (sig : Sig) (θ : List (String × Ty)) : Ty :=
  match sig.ret with
  | .ty r => r.inst θ
  | .void => unitTy

/-- What a call may do: give a value of the type it returns, or stop at what
is not run yet, at a variable read before it was set, or out of calls. -/
def CallRes (T : Ty) (m : EvalM Val) : Prop :=
  match m with
  | .ok v => HasType v T
  | .error s => s = .unsupported ∨ s = .uninit ∨ s = .diverge

/-- Calls that do what calls may, for each function of the program called
as a checked call calls it: with ground type arguments its bounds admit,
none of its parameters writing back, and arguments of its parameters'
types. -/
def CallOk (sigs : List (String × Sig)) (call : CallFn) : Prop :=
  ∀ f sig θ vals, sigs.lookup f = some sig → writesBack sig.params = false → groundArgs θ = true →
    (∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) →
    vals.length = sig.params.length → (∀ q ∈ vals.zip sig.params, HasType q.1 (q.2.2.inst θ)) →
    CallRes (sig.retTy θ) (call f θ vals)

theorem CallRes.sound {R : Ret} {T : Ty} {m : EvalM Val} (h : CallRes T m) : Sound R (HasType · T) m := by
  cases m with
  | ok v => exact h
  | error s => rcases h with rfl | rfl | rfl <;> trivial

theorem Ctx.admits_nil {Γ : Ctx} (hb : Γ.bounds = []) {b : Bound} {t : Ty} (h : Γ.admits b t = true) :
    b.admitsTy t = true := by
  cases t <;> cases b <;> simp_all [Ctx.admits, Bound.admitsTy]

theorem typeOf_quiet {m : ChkM Ty} {t : Ty} (h : Quiet m t) : typeOf m = .ok t := by
  simp only [typeOf]; rw [h]; rfl

theorem foldl_mul (l : List Nat) (a : Nat) : l.foldl (· * ·) a = a * l.foldl (· * ·) 1 := by
  induction l generalizing a with
  | nil => simp
  | cons x xs ih => simp only [List.foldl_cons]; rw [ih (a * x), ih (1 * x)]; simp [Nat.mul_assoc]

theorem struct_lookup : ∀ {fs : List (String × Val)} {ts : List (String × Ty)} {n : String} {u : Ty},
    fs.map (·.1) = ts.map (·.1) → (∀ p ∈ fs.zip ts, HasType p.1.2 p.2.2) → ts.lookup n = some u →
    ∃ w, fs.lookup n = some w ∧ HasType w u
  | [], [], _, _, _, _, h => by simp at h
  | [], _ :: _, _, _, hn, _, _ => by simp at hn
  | _ :: _, [], _, _, hn, _, _ => by simp at hn
  | (m, w) :: fs, (k, t) :: ts, n, u, hn, hz, hl => by
    simp only [List.map_cons, List.cons.injEq] at hn
    obtain ⟨rfl, hn⟩ := hn
    simp only [List.lookup] at hl ⊢
    by_cases e : n = m
    · subst e
      simp only [beq_self_eq_true, Option.some.injEq] at hl ⊢
      subst hl
      exact ⟨w, rfl, hz ((n, w), (n, t)) (by simp)⟩
    · have : (n == m) = false := by simpa using e
      simp only [this] at hl ⊢
      exact struct_lookup hn (fun p hp => hz p (by simp [hp])) hl

theorem fieldVal_sound {R : Ret} {v : Val} {t u : Ty} {s : Sel} (h : HasType v t) (hf : fieldType t s = .ok u) :
    Sound R (HasType · u) (fieldVal v s) := by
  have he := h.expose
  unfold fieldType at hf
  generalize t.expose = T at he hf
  cases T <;> cases s <;> simp at hf
  case tuple.pos ts i =>
    split at hf
    · rename_i u' hu
      simp [pure, Except.pure] at hf; subst hf
      obtain ⟨vs, rfl, hl, hz⟩ := he.tuple_inv
      obtain ⟨hi2, hu'⟩ := List.getElem?_eq_some_iff.mp hu
      have hi : i < vs.length := by rw [hl]; exact hi2
      simp only [fieldVal, List.getElem?_eq_getElem hi]
      have hmem : (vs[i], (Lit.tysList ts)[i]) ∈ vs.zip (Lit.tysList ts) := by
        rw [List.mem_iff_getElem]
        exact ⟨i, by simp only [List.length_zip]; omega, by simp⟩
      rw [hu'] at hmem
      exact Sound.pure' (hz _ hmem)
    · simp [throw, throwThe, MonadExceptOf.throw] at hf
  case struct.name fs nm =>
    split at hf
    · rename_i u' hu
      simp [pure, Except.pure] at hf; subst hf
      obtain ⟨vs, rfl, hn, hz⟩ := he.struct_inv
      obtain ⟨w, hw, hwt⟩ := struct_lookup hn hz hu
      simp only [fieldVal, hw]
      exact Sound.pure' hwt
    · simp [throw, throwThe, MonadExceptOf.throw] at hf

theorem missing_ok {Γ : Ctx} {m : IdxMode} (h : Γ.fitsMode m = true) : StopOk Γ.ret (missing m) := by
  cases m
  · exact EarlyOk.none_of h
  · exact EarlyOk.er_of h .error

/-- A map lookup finds a value of the map's value type, or none, or stops
where whether two keys are one is not known. -/
theorem findKey_sound {R : Ret} {k : Val} {w : Ty} : ∀ {es : List (Val × Val)}, (∀ p ∈ es, HasType p.2 w) →
    Sound R (fun o => ∀ v, o = some v → HasType v w) (findKey k es)
  | [], _ => fun v h => by cases h
  | (k', u) :: es, h => by
    rw [findKey]
    unfold keyEq
    cases valEq? k k' with
    | none => exact Sound.unsupported
    | some r =>
      cases r
      · exact findKey_sound (fun p hp => h p (List.mem_cons_of_mem _ hp))
      · exact Sound.pure' (fun v e => by cases e; exact h _ List.mem_cons_self)

theorem indexVal_sound {R : Ret} {m : IdxMode} {v kv : Val} {t et kt : Ty} (h : HasType v t)
    (hi : indexTypes t = .ok (et, kt)) (hk : HasType kv kt) (hm : StopOk R (missing m)) :
    Sound R (HasType · et) (indexVal m v kv) := by
  have he := h.expose
  unfold indexTypes at hi
  generalize t.expose = T at he hi
  cases T <;> simp [pure, Except.pure, throw, throwThe, MonadExceptOf.throw] at hi
  case list e =>
    obtain ⟨rfl, rfl⟩ := hi
    obtain ⟨vs, rfl, hvs⟩ := he.list_inv
    obtain ⟨i, hn, -, -⟩ := hk.int_inv (.inl rfl)
    simp only [indexVal, hn]
    split
    · exact hm
    · split
      · rename_i w hw; exact Sound.pure' (hvs w (List.mem_of_getElem? hw))
      · exact hm
  case map k w =>
    obtain ⟨rfl, rfl⟩ := hi
    obtain ⟨es, rfl, -, hvs⟩ := he.map_inv
    simp only [indexVal]
    refine Sound.bind' (findKey_sound hvs) (fun o ho => ?_)
    cases o with
    | none => exact hm
    | some v => exact Sound.pure' (ho v rfl)
  case tensor e r =>
    obtain ⟨rfl, rfl⟩ := hi
    obtain ⟨shape, vs, rfl, hs, hl, hvs⟩ := he.tensor_inv
    obtain ⟨i, hn, -, -⟩ := hk.int_inv (.inl rfl)
    simp only [indexVal, hn]
    by_cases hneg : i < 0
    · simp only [hneg, ↓reduceIte]; exact hm
    simp only [hneg, ↓reduceIte]
    have elem : ∀ (r : Nat), r ≤ 1 →
        Sound R (HasType · (if r ≤ 1 then e else .tensor e (r - 1)))
          (match vs[i.toNat]? with
            | some w => pure w
            | none => throw (missing m)) := by
      intro r hr
      simp only [hr, ↓reduceIte]
      split
      · rename_i w hw; exact Sound.pure' (hvs w (List.mem_of_getElem? hw))
      · exact hm
    rcases shape with _ | ⟨s0, _ | ⟨s1, rest⟩⟩
    · subst hs; exact elem 0 (by omega)
    · subst hs; exact elem 1 (by simp)
    · subst hs
      simp only [List.length_cons]
      have hr : ¬ (rest.length + 1 + 1 ≤ 1) := by omega
      simp only [hr, ↓reduceIte]
      split
      · rename_i hlt
        let p := shapeSize (s1 :: rest)
        have hlen : vs.length = s0 * p := by rw [hl, List.foldl_cons, foldl_mul, Nat.one_mul]; rfl
        have h1 : (i.toNat + 1) * p ≤ s0 * p := Nat.mul_le_mul_right _ hlt
        rw [Nat.succ_mul] at h1
        refine Sound.pure' (.tensor (by simp) ?_ ?_)
        · simp only [List.length_take, List.length_drop, hlen]
          show min p (s0 * p - i.toNat * p) = shapeSize (s1 :: rest)
          omega
        · intro w hw
          exact hvs w (List.mem_of_mem_drop (List.mem_of_mem_take hw))
      · exact hm

theorem litBody_checks {env : InferEnv} {n : Nat} {Γ : Ctx} {e : Expr} {t : Ty} (hl : e.isLit = true)
    (h : Quiet (synthF env (n + 1) Γ e) t) : Quiet (checkForm env n (Γ.enter e) e t) () := by
  rw [synthF_lit hl] at h
  simp only [litBody] at h
  obtain ⟨u, -, h⟩ := h.bind_inv
  obtain ⟨_, hc, h⟩ := h.bind_inv
  rw [Quiet.pure_iff] at h; subst h
  exact hc

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallOk sigs call)
include hcall
-- Only `evalCall_sound` uses `hcall` itself, the rest through it, which the
-- linter does not see across a mutual block.
set_option linter.unusedSectionVars false

mutual

/-- **A datafun expression that checks evaluates to a value of its type**,
or returns early with a value of the function's return type, or stops where
a call may: in a context with no type parameter's bound, whose variables
hold values of their types, for any inference and any sound calls. -/
theorem evalS_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Expr) (t : Ty), Quiet (synthF env n Γ e) t →
    Γ.Runs sigs → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalS env call n Γ ρ e)
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
        obtain ⟨o, hv, hvt⟩ := hρ x u k hl
        simp only [evalS, hv]
        cases o with
        | none => trivial
        | some v => exact Sound.pure' (hvt v rfl)
    case binop op a b =>
      simp only [synthF] at h
      obtain ⟨u, hp, hf⟩ := h.bind_inv
      simp only [evalS]
      exact Sound.bind' (evalPair_sound n _ ρ a b none u hp hb (hρ.enter _)) (fun r ⟨hr, ha, hb'⟩ => by
        obtain ⟨_, va, vb⟩ := r
        simp only at hr; subst hr
        exact binOp_sound (Γ := Γ.enter _) hb.1 hf ha hb')
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
    case call f args =>
      simp only [synthF] at h
      simp only [evalS]
      exact evalCall_sound n _ ρ f args none t h hb (hρ.enter _)
    case place x steps =>
      simp only [synthF] at h
      cases hl : (Γ.enter (.place x steps)).lookup x with
      | none => simp [hl] at h
      | some p =>
        obtain ⟨u, k⟩ := p
        simp only [hl] at h
        split at h
        · simp at h
        obtain ⟨o, hv, hvt⟩ := hρ x u k hl
        cases o with
        | none => simp only [evalS, hv]; trivial
        | some v =>
          simp only [evalS, hv, hl]
          exact evalSteps_sound n _ ρ u v steps t h (hvt v rfl) hb (hρ.enter _)
    case proj b sel =>
      simp only [synthF] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨w, hw, h⟩ := h.bind_inv
      split at h
      · simp at h
      rw [Quiet.pure_iff] at h; subst h
      rw [Quiet.liftE_iff] at hw
      simp only [evalS]
      exact Sound.bind' (evalS_sound n _ ρ b u hu hb (hρ.enter _)) (fun v hv => fieldVal_sound hv hw)
    case index b key mode =>
      cases mode with
      | none => simp [synthF] at h
      | some m =>
        simp only [synthF] at h
        obtain ⟨u, hu, h⟩ := h.bind_inv
        obtain ⟨⟨et, kt⟩, hi, h⟩ := h.bind_inv
        obtain ⟨_, hk, h⟩ := h.bind_inv
        rw [Quiet.liftE_iff] at hi
        rw [Quiet.recover_iff] at hk
        split at h
        · simp at h
        split at h
        · simp at h
        split at h
        · simp at h
        rename_i hfit
        rw [Quiet.pure_iff] at h; subst h
        simp only [evalS, typeOf_quiet hu]
        refine Sound.bind' (P := (u = ·)) rfl (fun _ e => ?_)
        subst e
        simp only [hi]
        refine Sound.bind' (P := ((et, kt) = ·)) rfl (fun _ e => ?_)
        subst e
        exact Sound.bind' (evalS_sound n _ ρ b u hu hb (hρ.enter _)) (fun v hv =>
          Sound.bind' (evalC_sound n _ ρ key kt hk hb (hρ.enter _)) (fun kv hkv =>
            indexVal_sound hv hi hkv (missing_ok (by simpa using hfit))))
    case unop op a =>
      have hb0 : (Γ.enter (.unop op a)).bounds = [] := hb.1
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
    Γ.Runs sigs → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalC env call n Γ ρ e t)
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
    Γ.Runs sigs → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalForm env call n Γ ρ e t)
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
          exact binOp_sound hb.1 hf ha hb')
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
    case call f args =>
      simp only [checkForm] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      split at h
      · simp at h
      rename_i hm
      simp only [evalForm]
      exact (evalCall_sound n Γ ρ f args _ u hu hb hρ).weaken
        (fun v hv => roll_of (Ty.meet_iff.mp (by simpa using hm)).symm hv.expose)
    case unop op a =>
      cases op
      case neg =>
        have hb0 : Γ.boundOf t.expose = Option.none := boundOf_nil hb.1 _
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

theorem evalSteps_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (t : Ty) (v : Val) (steps : List Step) (u : Ty),
    Quiet (stepsF env n Γ t steps) u → HasType v t → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (HasType · u) (evalSteps env call n Γ ρ t v steps)
  | 0, _, _, _, _, _, _, h, _, _, _ => by simp [stepsF] at h
  | n + 1, Γ, ρ, t, v, [], u, h, hv, hb, hρ => by
    simp only [stepsF, Quiet.pure_iff] at h; subst h
    simp only [evalSteps]; exact Sound.pure' hv
  | n + 1, Γ, ρ, t, v, [st], u, h, hv, hb, hρ => by
    simp only [stepsF] at h
    simp only [evalSteps]
    exact evalStep_sound n Γ ρ t v st u h hv hb hρ
  | n + 1, Γ, ρ, t, v, st :: st' :: rest, u, h, hv, hb, hρ => by
    simp only [stepsF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    simp only [evalSteps, typeOf_quiet hw]
    refine Sound.bind' (P := (w = ·)) rfl (fun _ e => ?_)
    subst e
    exact Sound.bind' (evalStep_sound n Γ.through ρ t v st w hw hv hb hρ) (fun x hx =>
      evalSteps_sound n Γ ρ w x (st' :: rest) u h hx hb hρ)

theorem evalStep_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (t : Ty) (v : Val) (st : Step) (u : Ty),
    Quiet (stepF env n Γ t st) u → HasType v t → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (HasType · u) (evalStep env call n Γ ρ t v st)
  | 0, _, _, _, _, _, _, h, _, _, _ => by simp [stepF] at h
  | n + 1, Γ, ρ, t, v, .field sel, u, h, hv, hb, hρ => by
    simp only [stepF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    split at h
    · simp at h
    rw [Quiet.pure_iff] at h; subst h
    rw [Quiet.liftE_iff] at hw
    simp only [evalStep]
    exact fieldVal_sound hv hw
  | n + 1, Γ, ρ, t, v, .index k none, u, h, hv, hb, hρ => by simp [stepF] at h
  | n + 1, Γ, ρ, t, v, .index k (some m), u, h, hv, hb, hρ => by
    simp only [stepF] at h
    obtain ⟨⟨et, kt⟩, hi, h⟩ := h.bind_inv
    obtain ⟨_, hk, h⟩ := h.bind_inv
    rw [Quiet.liftE_iff] at hi
    rw [Quiet.recover_iff] at hk
    split at h
    · simp at h
    split at h
    · simp at h
    split at h
    · simp at h
    rename_i hfit
    rw [Quiet.pure_iff] at h; subst h
    simp only [evalStep, hi]
    refine Sound.bind' (P := ((et, kt) = ·)) rfl (fun _ e => ?_)
    subst e
    exact Sound.bind' (evalC_sound n Γ ρ k kt hk hb hρ) (fun kv hkv =>
      indexVal_sound hv hi hkv (missing_ok (by simpa using hfit)))

theorem evalSub_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Expr) (t : Ty), Quiet (subsumeF env n Γ e t) () →
    Γ.Runs sigs → EnvOk Γ ρ → Sound Γ.ret (HasType · t) (evalSub env call n Γ ρ e)
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
    Quiet (pairsF env n Γ es ts) () → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.length = min es.length ts.length ∧ ∀ p ∈ vs.zip ts, HasType p.1 p.2)
      (evalPairs env call n Γ ρ es ts)
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
    Quiet (fieldsF env n Γ fs ts) () → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.map (·.1) = (fs.zip ts).map (·.2.1) ∧ ∀ p ∈ vs.zip ts, HasType p.1.2 p.2.2)
      (evalFields env call n Γ ρ fs ts)
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
    Quiet (allF env n Γ es t) () → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.length = es.length ∧ ∀ v ∈ vs, HasType v t) (evalAll env call n Γ ρ es t)
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
    Quiet (entriesF env n Γ es k v) () → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun ps => ∀ p ∈ ps, HasType p.1 k ∧ HasType p.2 v) (evalEntries env call n Γ ρ es k v)
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
    Quiet (rowsF env n Γ rows cols) () → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun rs => ∀ r ∈ rs, r.length = cols.length ∧ ∀ p ∈ r.zip cols, HasType p.1 p.2.2)
      (evalRows env call n Γ ρ rows cols)
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

/-- A checked call gives a value of its type, or stops as a call may. -/
theorem evalCall_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (f : String) (args : List (Option Mode × Expr))
    (exp : Option Ty) (t : Ty), Quiet (callF env n Γ f args exp) t → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (HasType · t) (evalCall env call n Γ ρ f args exp)
  | 0, _, _, _, _, _, _, h, _, _ => by simp [callF] at h
  | n + 1, Γ, ρ, f, args, exp, t, h, hb, hρ => by
    simp only [callF] at h
    cases hs : Γ.sigs.lookup f with
    | none => simp only [hs] at h; simp at h
    | some sig =>
      simp only [hs] at h
      split at h
      · simp at h
      rename_i hlen
      obtain ⟨_, -, h⟩ := h.bind_inv
      obtain ⟨_, -, h⟩ := h.bind_inv
      obtain ⟨θ, hθ, h⟩ := h.bind_inv
      rw [Quiet.liftE_iff] at hθ
      obtain ⟨_, ha, h⟩ := h.bind_inv
      split at h
      · simp at h
      rename_i h92
      split at h
      · simp at h
      rename_i h101
      have ht : t = sig.retTy θ := by
        unfold Sig.retTy; split <;> (rename_i hr; simp only [hr, Quiet.pure_iff] at h; exact h.symm)
      subst ht
      simp only [evalCall, hs, hθ]
      split
      · exact Sound.unsupported
      rename_i hw
      simp only [Bool.or_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true, not_or] at hw
      refine Sound.bind' (evalArgs_sound n Γ ρ args _ ha hb hρ) (fun vals ⟨hl, hv⟩ => ?_)
      have hlen' : args.length = sig.params.length := by simpa using hlen
      have hbd : ∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true := by
        intro p b hp
        simp only [allDecided, List.all_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true,
          Bool.not_eq_false] at h101
        simp only [boundsHold, List.all_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true,
          Bool.not_eq_false] at h92
        obtain ⟨u, hu⟩ := Option.isSome_iff_exists.mp (h101 _ hp)
        have := h92 _ hp
        simp only [hu] at this
        exact ⟨u, hu, Ctx.admits_nil hb.1 this⟩
      refine (hcall f sig θ vals (hb.2 ▸ hs) (by simpa using hw.1) (by simpa using hw.2) hbd
        (by simp [hl, hlen']) (fun q hq => ?_)).sound
      have := hv (q.1, (q.2.1, q.2.2.inst θ)) (by
        rw [List.zip_map_right]
        exact List.mem_map.mpr ⟨q, hq, rfl⟩)
      exact this

/-- A call's checked arguments evaluate to values of their parameters'
types. -/
theorem evalArgs_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (args : List (Option Mode × Expr))
    (params : List (Mode × Ty)), Quiet (argsF env n Γ args params) () → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun vs => vs.length = min args.length params.length ∧ ∀ q ∈ vs.zip params, HasType q.1 q.2.2)
      (evalArgs env call n Γ ρ args params)
  | 0, _, _, _, _, h, _, _ => by simp [argsF] at h
  | n + 1, Γ, ρ, (_, e) :: args, (mode, t) :: params, h, hb, hρ => by
    simp only [argsF] at h
    obtain ⟨_, hc, h⟩ := h.bind_inv
    simp only [evalArgs]
    refine Sound.bind' (evalC_sound n _ ρ e t hc hb (hρ.forArg mode)) (fun v hv => ?_)
    refine Sound.bind' (evalArgs_sound n Γ ρ args params h hb hρ) (fun vs ⟨hl, hvs⟩ => ?_)
    refine Sound.pure' ⟨by simp [hl, Nat.succ_min_succ], ?_⟩
    intro q hq
    simp only [List.zip_cons_cons, List.mem_cons] at hq
    rcases hq with rfl | hq
    · exact hv
    · exact hvs q hq
  | n + 1, Γ, ρ, [], _, _, _, _ => by simp only [evalArgs]; exact ⟨by simp, by simp⟩
  | n + 1, Γ, ρ, _ :: _, [], _, _, _ => by simp only [evalArgs]; exact ⟨by simp, by simp⟩

theorem evalPair_sound : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (a b : Expr) (exp : Option Ty) (t : Ty),
    Quiet (pairF env n Γ a b exp) t → Γ.Runs sigs → EnvOk Γ ρ →
    Sound Γ.ret (fun r => r.1 = t ∧ HasType r.2.1 t ∧ HasType r.2.2 t) (evalPair env call n Γ ρ a b exp)
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
    exact Sound.bind' (evalC_sound n _ ρ a _ ha ⟨by simpa using hb.1, by simpa using hb.2⟩ hρ.borrow) (fun va hva =>
      Sound.bind' (evalC_sound n _ ρ b _ hb' ⟨by simpa using hb.1, by simpa using hb.2⟩ hρ.borrow) (fun vb hvb =>
        Sound.pure' ⟨rfl, hva, hvb⟩))

end

end

end Datalove.Fun
