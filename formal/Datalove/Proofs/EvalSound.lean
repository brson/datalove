import Datalove.FunEval
import Datalove.Proofs.Expose
import Datalove.Proofs.InstanceCheck

/-!
# A datafun expression that checks evaluates to a value of its type

`Fun.evalS_sound`: where an expression checks quietly, evaluating it from
any variables that line up with the context's, each that is set holding a
value of its type (`Agree`), gives a value of its type, the variables, which
a call writes back to, lining up still (`SoundT`); or returns early with a
value of the function's return type, the variables as they were; or stops
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

/-- A value the function may return early: one of its return type. -/
def EarlyOk (R : Ret) (r : Val) : Prop := ∃ T, R = .ty T ∧ HasType r T

/-- Why evaluating a checked expression may give no value: an early return
the function's type allows, a `Gap` the model does not run, a variable read
before it was set, which the ownership analysis refuses, or calls nested as
deep as they were given. -/
def StopOk (R : Ret) : Stop → Prop
  | .early r => EarlyOk R r
  | .unsupported _ | .uninit | .diverge => True
  | .fuel | .wrong | .retVoid | .brk _ | .cont _ | .returned _ _ => False

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

theorem unsupported {g : Gap} : Sound R P (throw (.unsupported g)) := trivial

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


/-- The variables line up with the context's, name for name, each that is set
holding a value of its type. -/
inductive Agree : List (String × Ty × Kind) → VEnv → Prop where
  | nil : Agree [] []
  | cons {x : String} {t : Ty} {k : Kind} {o : Option Val} {vars : List (String × Ty × Kind)} {ρ : VEnv} :
      (∀ v, o = some v → HasType v t) → Agree vars ρ → Agree ((x, t, k) :: vars) ((x, o) :: ρ)

theorem Agree.length {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) :
    ρ.length = vars.length := by
  induction h <;> simp_all

theorem Agree.lookup {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) (x : String) :
    ∀ t k, (vars.find? (·.1 == x)).map (·.2) = some (t, k) →
      ∃ o, ρ.lookup x = some o ∧ ∀ v, o = some v → HasType v t := by
  induction h with
  | nil => intro t k h; simp at h
  | @cons y u k' o vars ρ ho _ ih =>
    intro t k hl
    simp only [List.find?_cons, List.lookup] at hl ⊢
    by_cases e : y = x
    · subst e
      simp only [beq_self_eq_true, Option.map_some, Option.some.injEq, Prod.mk.injEq] at hl ⊢
      obtain ⟨rfl, -⟩ := hl
      exact ⟨o, rfl, ho⟩
    · have h1 : (y == x) = false := by simpa using e
      have h2 : (x == y) = false := by simpa using fun h => e h.symm
      simp only [h1, h2] at hl ⊢
      exact ih t k hl

theorem Agree.takeVar {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) (x : String) :
    Agree vars (Fun.takeVar x ρ) := by
  induction h with
  | nil => exact .nil
  | @cons y u k' o vars ρ ho h ih =>
    simp only [Fun.takeVar]
    split
    · exact .cons (fun v hv => by cases hv) h
    · exact .cons ho ih

theorem setVar_agree {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) {x : String}
    {t : Ty} {k : Kind} (hl : (vars.find? (·.1 == x)).map (·.2) = some (t, k)) {w : Val}
    (hw : HasType w t) : Agree vars (setVar x w ρ) := by
  induction h with
  | nil => simp at hl
  | @cons y u k' o vars ρ ho h ih =>
    simp only [List.find?_cons] at hl
    simp only [setVar]
    by_cases e : y = x
    · subst e
      simp only [beq_self_eq_true, Option.map_some, Option.some.injEq, Prod.mk.injEq, ↓reduceIte] at hl ⊢
      obtain ⟨rfl, -⟩ := hl
      exact .cons (fun v hv => by cases hv; exact hw) h
    · have h1 : (y == x) = false := by simpa using e
      simp only [h1, Bool.false_eq_true, ↓reduceIte] at hl ⊢
      exact .cons ho (ih hl)

/-- A context evaluation runs in: no type parameter's bound, and the
program's functions. -/
def Ctx.Runs (sigs : List (String × Sig)) (Γ : Ctx) : Prop := Γ.bounds = [] ∧ Γ.sigs = sigs

theorem Ctx.Runs.under {sigs : List (String × Sig)} {Γ : Ctx} (h : Γ.Runs sigs) (b : Expr) :
    (Γ.under b).Runs sigs := ⟨by simpa using h.1, by simpa using h.2⟩

/-- What a call gives: the function's return type, or the unit for one that
returns nothing. -/
def Sig.retTy (sig : Sig) (θ : List (String × Ty)) : Ty :=
  match sig.ret with
  | .ty r => r.inst θ
  | .void => unitTy

/-- Values for a function's parameters, one each, each there is of its
parameter's type with the type arguments substituted. -/
def ArgsFit (θ : List (String × Ty)) (vals : List (Option Val)) (ps : List (Mode × Ty)) : Prop :=
  vals.length = ps.length ∧ ∀ q ∈ vals.zip ps, ∀ v, q.1 = some v → HasType v (q.2.2.inst θ)

/-- What a call may do: give a value of the type it returns, and what each
parameter came to, of its type, or stop at what is not run yet, at a
variable read before it was set, or out of calls. -/
def CallRes (sig : Sig) (θ : List (String × Ty)) (m : EvalM (Val × List (Option Val))) : Prop :=
  match m with
  | .ok (v, after) => HasType v (sig.retTy θ) ∧ ArgsFit θ after sig.params
  | .error s => (∃ g, s = .unsupported g) ∨ s = .uninit ∨ s = .diverge

/-- Calls that do what calls may, for each function of the program called
as a checked call calls it: with ground type arguments its bounds admit, and
arguments of its parameters' types, none for an `out` one. -/
def CallOk (sigs : List (String × Sig)) (call : CallFn) : Prop :=
  ∀ f sig θ vals, sigs.lookup f = some sig → groundArgs θ = true →
    (∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) →
    ArgsFit θ vals sig.params → CallRes sig θ (call f θ vals)

theorem CallRes.sound {R : Ret} {sig : Sig} {θ : List (String × Ty)} {m : EvalM (Val × List (Option Val))}
    (h : CallRes sig θ m) : Sound R (fun r => HasType r.1 (sig.retTy θ) ∧ ArgsFit θ r.2 sig.params) m := by
  cases m with
  | ok r => exact h
  | error s => rcases h with ⟨_, rfl⟩ | rfl | rfl <;> trivial

theorem zip_inst {θ : List (String × Ty)} {vals : List (Option Val)} {sps : List (Mode × Ty)} :
    (∀ q ∈ vals.zip (sps.map fun (m, t) => (m, t.inst θ)), ∀ v, q.1 = some v → HasType v q.2.2) ↔
      (∀ q ∈ vals.zip sps, ∀ v, q.1 = some v → HasType v (q.2.2.inst θ)) := by
  rw [List.zip_map_right]
  constructor
  · intro h q hq v hv
    exact h _ (List.mem_map.mpr ⟨q, hq, rfl⟩) v hv
  · intro h q hq v hv
    obtain ⟨q', hq', rfl⟩ := List.mem_map.mp hq
    exact h q' hq' v hv

/-- Why evaluating a checked expression, which reads and writes the
variables `V`, may give no value: a return the function's type takes, with
the variables lining up with `V` still, or as `StopOk` says. -/
def StopOkT (V : List (String × Ty × Kind)) (R : Ret) : Stop → Prop
  | .returned (some r) ρ => EarlyOk R r ∧ Agree V ρ
  | .returned none _ | .early _ => False
  | s => StopOk R s

/-- What evaluating from any variables lining up with `V` gives meets `P`,
the variables lining up still, or it stops as a checked expression may. -/
def SoundT {α} (V : List (String × Ty × Kind)) (R : Ret) (P : α → Prop) (m : EvalT α) : Prop :=
  ∀ ρ, Agree V ρ → match m.run ρ with
    | .ok (a, ρ') => P a ∧ Agree V ρ'
    | .error s => StopOkT V R s

namespace SoundT

variable {α β : Type} {V : List (String × Ty × Kind)} {R : Ret} {P : α → Prop} {Q : β → Prop}

theorem pure' {a : α} (h : P a) : SoundT V R P (Pure.pure a) := fun _ hρ => ⟨h, hρ⟩

theorem bind' {x : EvalT α} {f : α → EvalT β} (hx : SoundT V R P x) (hf : ∀ a, P a → SoundT V R Q (f a)) :
    SoundT V R Q (x >>= f) := by
  intro ρ hρ
  have h := hx ρ hρ
  show match (x.run ρ >>= fun p => (f p.1).run p.2) with
    | .ok (a, ρ') => Q a ∧ Agree V ρ'
    | .error s => StopOkT V R s
  generalize x.run ρ = r at h
  cases r with
  | error s => exact h
  | ok p => exact hf p.1 h.1 p.2 h.2

theorem map' {x : EvalT α} {f : α → β} (hx : SoundT V R P x) (hf : ∀ a, P a → Q (f a)) :
    SoundT V R Q (f <$> x) := by
  rw [← bind_pure_comp]
  exact bind' hx (fun a ha => pure' (hf a ha))

theorem weaken {x : EvalT α} {P' : α → Prop} (h : SoundT V R P x) (hp : ∀ a, P a → P' a) :
    SoundT V R P' x := by
  intro ρ hρ
  have h := h ρ hρ
  generalize x.run ρ = r at h ⊢
  cases r with
  | error s => exact h
  | ok p => exact ⟨hp _ h.1, h.2⟩

theorem throw' {s : Stop} (h : StopOkT V R s) : SoundT V R P (throw s) := fun _ _ => h

theorem unsupported {g : Gap} : SoundT V R P (throw (.unsupported g)) := throw' trivial

theorem uninit : SoundT V R P (throw .uninit) := throw' trivial

/-- Moving a variable's value out, then giving `a`, or giving `a`: what
holds nothing is of every type. -/
theorem take' {a : α} (h : P a) (c : Bool) (x : String) :
    SoundT V R P (if c = true then (do modify (takeVar x); pure a : EvalT α) else pure a) := by
  intro ρ hρ
  cases c
  · exact ⟨h, hρ⟩
  · exact ⟨h, hρ.takeVar x⟩

/-- Reading the variables. -/
theorem get' {f : VEnv → EvalT α} (h : ∀ ρ, Agree V ρ → SoundT V R P (f ρ)) : SoundT V R P (get >>= f) :=
  fun ρ hρ => h ρ hρ ρ hρ

/-- Writing variables that line up. -/
theorem set' {ρ' : VEnv} (h : Agree V ρ') : SoundT V R (fun _ => True) (set ρ' : EvalT PUnit) :=
  fun _ _ => ⟨trivial, h⟩

/-- A step that reads no variable, its early return given the variables. -/
theorem here {m : EvalM α} (h : Sound R P m) : SoundT V R P (Fun.here m) := by
  intro ρ hρ
  cases m with
  | ok a => exact ⟨h, hρ⟩
  | error s =>
    cases s with
    | early r => exact ⟨h, hρ⟩
    | returned o ρ' => exact h.elim
    | _ => exact h

theorem earlyHere {r : Val} (h : EarlyOk R r) : SoundT V R P (Fun.earlyHere r) := fun _ hρ => ⟨h, hρ⟩

end SoundT

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

/-! ## Writing a place -/

theorem zip_set_typed {vs : List Val} {ts : List Ty} {i : Nat} {w : Val}
    (hz : ∀ p ∈ vs.zip ts, HasType p.1 p.2) (hw : ∀ u, ts[i]? = some u → HasType w u) :
    ∀ p ∈ (vs.set i w).zip ts, HasType p.1 p.2 := by
  intro p hp
  rw [List.mem_iff_getElem] at hp
  obtain ⟨j, hj, rfl⟩ := hp
  simp only [List.length_zip, List.length_set] at hj
  simp only [List.getElem_zip, List.getElem_set]
  split
  · rename_i e; subst e; exact hw _ (List.getElem?_eq_getElem (by omega))
  · exact hz (vs[j]'(by omega), ts[j]'(by omega)) (List.mem_iff_getElem.mpr ⟨j, by simp; omega, by simp⟩)

theorem replaceFirst_typed {n : String} {w : Val} {u : Ty} :
    ∀ {fs : List (String × Val)} {tl : List (String × Ty)}, fs.map (·.1) = tl.map (·.1) →
      (∀ p ∈ fs.zip tl, HasType p.1.2 p.2.2) → tl.lookup n = some u → HasType w u →
      (putField.replaceFirst n w fs).map (·.1) = tl.map (·.1) ∧
        ∀ p ∈ (putField.replaceFirst n w fs).zip tl, HasType p.1.2 p.2.2
  | [], [], _, _, _, _ => ⟨rfl, by simp⟩
  | [], _ :: _, hn, _, _, _ => by simp at hn
  | _ :: _, [], hn, _, _, _ => by simp at hn
  | (m, v) :: fs, (k, t) :: tl, hn, hz, hl, hw => by
    simp only [List.map_cons, List.cons.injEq] at hn
    obtain ⟨rfl, hn⟩ := hn
    simp only [putField.replaceFirst]
    by_cases e : m = n
    · subst e
      simp only [List.lookup, beq_self_eq_true, Option.some.injEq] at hl
      subst hl
      simp only [beq_self_eq_true, ↓reduceIte, List.map_cons, hn, List.zip_cons_cons, List.mem_cons,
        forall_eq_or_imp, true_and]
      exact ⟨hw, fun p hp => hz p (by simp [hp])⟩
    · have h1 : (m == n) = false := by simpa using e
      have h2 : (n == m) = false := by simpa using fun h => e h.symm
      simp only [List.lookup, h2] at hl
      simp only [h1, Bool.false_eq_true, ↓reduceIte, List.map_cons, List.zip_cons_cons, List.mem_cons,
        forall_eq_or_imp]
      obtain ⟨h3, h4⟩ := replaceFirst_typed hn (fun p hp => hz p (by simp [hp])) hl hw
      exact ⟨by simp [h3], hz ((m, v), (m, t)) (by simp), h4⟩

theorem putField_sound {R : Ret} {v w : Val} {t ft : Ty} {s : Sel} (hv : HasType v t)
    (hf : fieldType t s = .ok ft) (hw : HasType w ft) : Sound R (HasType · t) (putField v s w) := by
  have he := hv.expose
  unfold fieldType at hf
  generalize hT : t.expose = T at he hf
  cases T <;> cases s <;> simp at hf
  case tuple.pos ts i =>
    split at hf
    · rename_i u hu
      simp [pure, Except.pure] at hf; subst hf
      obtain ⟨vs, rfl, hl, hz⟩ := he.tuple_inv
      have hi : i < vs.length := by rw [hl]; exact (List.getElem?_eq_some_iff.mp hu).1
      simp only [putField, hi, ↓reduceIte]
      exact Sound.pure' (roll_of hT (.tuple (by simp [hl]) (zip_set_typed hz (fun u' hu' => by
        rw [hu] at hu'; cases hu'; exact hw))))
    · simp [throw, throwThe, MonadExceptOf.throw] at hf
  case struct.name fs nm =>
    split at hf
    · rename_i u hu
      simp [pure, Except.pure] at hf; subst hf
      obtain ⟨fvs, rfl, hn, hz⟩ := he.struct_inv
      obtain ⟨h1, h2⟩ := replaceFirst_typed hn hz hu hw
      simp only [putField]
      exact Sound.pure' (roll_of hT (.struct h1 h2))
    · simp [throw, throwThe, MonadExceptOf.throw] at hf

theorem replaceKey_typed {R : Ret} {k w : Val} {kt vt : Ty} (hw : HasType w vt) :
    ∀ (es : List (Val × Val)), (∀ p ∈ es, HasType p.1 kt) → (∀ p ∈ es, HasType p.2 vt) →
      Sound R (fun es' => (∀ p ∈ es', HasType p.1 kt) ∧ (∀ p ∈ es', HasType p.2 vt)) (replaceKey k w es)
  | [], _, _ => ⟨by simp, by simp⟩
  | (k', u) :: es, hk, hv => by
    rw [replaceKey]
    unfold keyEq
    cases valEq? k k' with
    | none => exact Sound.unsupported
    | some r =>
      cases r
      · refine Sound.map' (replaceKey_typed hw es (fun p hp => hk p (by simp [hp]))
          (fun p hp => hv p (by simp [hp]))) (fun es' ⟨h1, h2⟩ => ⟨fun p hp => ?_, fun p hp => ?_⟩) <;>
          simp only [List.mem_cons] at hp <;> rcases hp with rfl | hp
        · exact hk (k', u) (by simp)
        · exact h1 p hp
        · exact hv (k', u) (by simp)
        · exact h2 p hp
      · refine Sound.pure' ⟨fun p hp => ?_, fun p hp => ?_⟩ <;> simp only [List.mem_cons] at hp <;>
          rcases hp with rfl | hp
        · exact hk (k', u) (by simp)
        · exact hk p (by simp [hp])
        · exact hw
        · exact hv p (by simp [hp])

theorem insertKey_typed {R : Ret} {k w : Val} {kt vt : Ty} (hk : HasType k kt) (hw : HasType w vt)
    {es : List (Val × Val)} (hks : ∀ p ∈ es, HasType p.1 kt) (hvs : ∀ p ∈ es, HasType p.2 vt) :
    Sound R (fun es' => (∀ p ∈ es', HasType p.1 kt) ∧ (∀ p ∈ es', HasType p.2 vt)) (insertKey k w es) := by
  simp only [insertKey]
  refine Sound.bind' (findKey_sound (k := k) hvs) (fun o _ => ?_)
  cases o with
  | some _ => exact replaceKey_typed hw es hks hvs
  | none =>
    refine Sound.pure' ⟨fun p hp => ?_, fun p hp => ?_⟩ <;> simp only [List.mem_append, List.mem_singleton] at hp <;>
      rcases hp with hp | rfl
    · exact hks p hp
    · exact hk
    · exact hvs p hp
    · exact hw

theorem overwrite_length : ∀ (n : Nat) (ws vs : List Val), (overwrite n ws vs).length = vs.length
  | _, _, [] => by simp [overwrite]
  | 0, [], _ :: _ => by simp [overwrite]
  | 0, _ :: ws, _ :: vs => by simp [overwrite, overwrite_length 0 ws vs]
  | n + 1, ws, _ :: vs => by simp [overwrite, overwrite_length n ws vs]

theorem mem_overwrite {u : Val} : ∀ {n : Nat} {ws vs : List Val}, u ∈ overwrite n ws vs → u ∈ ws ∨ u ∈ vs
  | _, _, [], h => by simp [overwrite] at h
  | 0, [], _ :: _, h => .inr h
  | 0, _ :: ws, _ :: vs, h => by
    simp only [overwrite, List.mem_cons] at h
    rcases h with rfl | h
    · exact .inl List.mem_cons_self
    · rcases mem_overwrite h with h | h
      · exact .inl (List.mem_cons_of_mem _ h)
      · exact .inr (List.mem_cons_of_mem _ h)
  | n + 1, ws, _ :: vs, h => by
    simp only [overwrite, List.mem_cons] at h
    rcases h with rfl | h
    · exact .inr List.mem_cons_self
    · rcases mem_overwrite h with h | h
      · exact .inl h
      · exact .inr (List.mem_cons_of_mem _ h)

theorem putIndex_sound {R : Ret} {v kv w : Val} {t et kt : Ty} (hv : HasType v t)
    (hi : indexTypes t = .ok (et, kt)) (hk : HasType kv kt) (hw : HasType w et) :
    Sound R (HasType · t) (putIndex v kv w) := by
  have he := hv.expose
  unfold indexTypes at hi
  generalize hT : t.expose = T at he hi
  cases T <;> simp [pure, Except.pure, throw, throwThe, MonadExceptOf.throw] at hi
  case list e =>
    obtain ⟨rfl, rfl⟩ := hi
    obtain ⟨vs, rfl, hvs⟩ := he.list_inv
    obtain ⟨i, hn, -, -⟩ := hk.int_inv (.inl rfl)
    simp only [putIndex, hn]
    refine Sound.pure' (roll_of hT (.list fun u hu => ?_))
    rcases List.mem_or_eq_of_mem_set hu with hu | rfl
    · exact hvs u hu
    · exact hw
  case map k' w' =>
    obtain ⟨rfl, rfl⟩ := hi
    obtain ⟨es, rfl, hks, hvs⟩ := he.map_inv
    simp only [putIndex]
    exact Sound.map' (replaceKey_typed hw es hks hvs) (fun _ ⟨h1, h2⟩ => roll_of hT (.map h1 h2))
  case tensor e r =>
    obtain ⟨rfl, rfl⟩ := hi
    obtain ⟨shape, vs, rfl, hs, hl, hvs⟩ := he.tensor_inv
    obtain ⟨i, hn, -, -⟩ := hk.int_inv (.inl rfl)
    rcases shape with _ | ⟨s0, _ | ⟨s1, rest⟩⟩
    · subst hs
      simp only [putIndex, hn]
      simp at hw
      refine Sound.pure' (roll_of hT (.tensor rfl (by simp [hl]) fun u hu => ?_))
      rcases List.mem_or_eq_of_mem_set hu with hu | rfl
      · exact hvs u hu
      · exact hw
    · subst hs
      simp only [putIndex, hn]
      simp at hw
      refine Sound.pure' (roll_of hT (.tensor rfl (by simp [hl]) fun u hu => ?_))
      rcases List.mem_or_eq_of_mem_set hu with hu | rfl
      · exact hvs u hu
      · exact hw
    · subst hs
      simp only [putIndex, hn]
      have hw' : HasType w (.tensor e (rest.length + 1)) := by simpa using hw
      obtain ⟨sh, ws, rfl, -, -, hws⟩ := hw'.tensor_inv
      refine Sound.pure' (roll_of hT (.tensor rfl (by rw [overwrite_length]; exact hl) fun u hu => ?_))
      rcases mem_overwrite hu with hu | hu
      · exact hws u (List.mem_of_mem_take hu)
      · exact hvs u hu

theorem finishBinop_quiet {Γ : Ctx} (hb : Γ.bounds = []) {op : BinOp} {t : Ty}
    (ha : arithAdmits op t (Γ.boundOf t) = true) (hf : fits Γ (needs op) = true) :
    Quiet (finishBinop Γ op t) t := by
  rw [boundOf_nil hb] at ha
  cases op <;> simp_all [finishBinop, boundOf_nil hb, arithAdmits] <;>
    (split
     · rename_i hc; rcases ha with h | h <;> simp_all
     · exact Quiet.pure _)

/-- Keys' values, one each, each of its key's type. -/
def KeysFit (kvs : List Val) (keys : List (Expr × Ty)) : Prop :=
  kvs.length = keys.length ∧ ∀ p ∈ kvs.zip keys, HasType p.1 p.2.2

/-- Writing a `set`'s value through its steps, with its keys, into what a
variable of type `t` holds gives a value of type `t`, or returns early as an
index's mode says, or stops at what is not run or not set. -/
theorem writeSteps_sound {Γ : Ctx} (hb : Γ.bounds = []) {op : Option BinOp} {r : Val} :
    ∀ (t vt : Ty) (keys : List (Expr × Ty)) (old : Option Val) (steps : List Step) (kvs : List Val),
      setPlan op t steps = some (vt, keys) → KeysFit kvs keys → HasType r vt →
      (∀ v, old = some v → HasType v t) →
      (∀ o, op = some o → arithAdmits o vt (Γ.boundOf vt) = true ∧ fits Γ (needs o) = true) →
      (∀ k m, Step.index k (some m) ∈ steps → Γ.fitsMode m = true) →
      Sound Γ.ret (HasType · t) (writeSteps op t old steps kvs r)
  | t, vt, keys, old, [], kvs, hp, _, hr, hold, hop, _ => by
    simp only [setPlan, Option.some.injEq, Prod.mk.injEq] at hp
    obtain ⟨rfl, rfl⟩ := hp
    cases op with
    | none => simp only [writeSteps]; exact Sound.pure' hr
    | some o =>
      obtain ⟨ha, hf⟩ := hop o rfl
      simp only [writeSteps]
      cases old with
      | none => exact trivial
      | some v => exact binOp_sound hb (finishBinop_quiet hb ha hf) (hold v rfl) hr
  | t, vt, keys, old, [.index k none], kvs, hp, hk, hr, hold, _, _ => by
    cases op with
    | some _ => simp [setPlan] at hp
    | none =>
      cases hx : t.expose
      case map kt vt' =>
        simp only [setPlan, hx, Option.some.injEq, Prod.mk.injEq] at hp
        obtain ⟨rfl, rfl⟩ := hp
        obtain ⟨hl, hks⟩ := hk
        match kvs, hl with
        | [kv], _ =>
          have hkv : HasType kv kt := hks (kv, (k, kt)) (by simp)
          simp only [writeSteps]
          cases old with
          | none => exact trivial
          | some o =>
            have ho := (hold o rfl).expose
            rw [hx] at ho
            obtain ⟨es, rfl, hks', hvs⟩ := ho.map_inv
            exact Sound.map' (insertKey_typed hkv hr hks' hvs) (fun _ ⟨h1, h2⟩ => roll_of hx (.map h1 h2))
      all_goals simp [setPlan, hx] at hp
  | t, vt, keys, old, .field s :: rest, kvs, hp, hk, hr, hold, hop, hm => by
    simp only [setPlan] at hp
    split at hp
    · rename_i ft hf
      simp only [writeSteps, hf]
      cases old with
      | none => exact trivial
      | some v =>
        refine Sound.bind' (fieldVal_sound (hold v rfl) hf) (fun part hpart => ?_)
        refine Sound.bind' (writeSteps_sound hb ft vt keys (some part) rest kvs hp hk hr
          (fun u hu => by cases hu; exact hpart) hop (fun k m hkm => hm k m (by simp [hkm]))) (fun w hw => ?_)
        exact putField_sound (hold v rfl) hf hw
    · simp at hp
  | t, vt, keys, old, .index _ none :: _ :: _, kvs, hp, _, _, _, _, _ => by simp [setPlan] at hp
  | t, vt, keys, old, .index k (some m) :: rest, kvs, hp, hk, hr, hold, hop, hm => by
    simp only [setPlan] at hp
    split at hp
    · rename_i et kt hi
      cases hrest : setPlan op et rest with
      | none => simp [hrest] at hp
      | some q =>
        obtain ⟨vt', ks⟩ := q
        simp only [hrest, Option.map_some, Option.some.injEq, Prod.mk.injEq] at hp
        obtain ⟨rfl, rfl⟩ := hp
        obtain ⟨hl, hks⟩ := hk
        match kvs, hl with
        | kv :: kvs', hl =>
          have hkv : HasType kv kt := hks (kv, (k, kt)) (by simp)
          have hk' : KeysFit kvs' ks := ⟨by simpa using hl, fun p hp => hks p (by simp [hp])⟩
          simp only [writeSteps, hi]
          cases old with
          | none => exact trivial
          | some v =>
            refine Sound.bind' (indexVal_sound (hold v rfl) hi hkv (missing_ok (hm k m (by simp))))
              (fun part hpart => ?_)
            refine Sound.bind' (writeSteps_sound hb et vt' ks (some part) rest kvs' hrest hk' hr
              (fun u hu => by cases hu; exact hpart) hop (fun k' m' hkm => hm k' m' (by simp [hkm])))
              (fun w hw => ?_)
            exact putIndex_sound (hold v rfl) hi hkv hw
    · simp at hp

/-- A place's steps from a variable of type `t`, which read a value of type
`u`, with their keys' values `kvs`: what `setPlan` makes of them, the keys of
their types, and each index's early return one the context takes. -/
def PathOk (Γ : Ctx) (t : Ty) (steps : List Step) (u : Ty) (kvs : List Val) : Prop :=
  ∃ keys, setPlan none t steps = some (u, keys) ∧ KeysFit kvs keys ∧
    ∀ k m, Step.index k (some m) ∈ steps → Γ.fitsMode m = true

/-- What a `mut` or `out` argument into a part of a variable was evaluated
with, for writing back to it: its variable's type, and its steps to a part
that is of the parameter's type, unfolded. -/
def ArgKeys (Γ : Ctx) (mode : Mode) (t : Ty) (e : Expr) (kvs : List Val) : Prop :=
  match placeOf e with
  | some (x, steps) => (mode == .mut || mode == .out) = true →
      ∃ u k u', Γ.lookup x = some (u, k) ∧ PathOk Γ u steps u' kvs ∧ u'.expose = t.expose
  | none => True

/-- `ArgKeys` of each argument. -/
def ArgsKeys (Γ : Ctx) : List (Option Mode × Expr) → List (Mode × Ty) → List (List Val) → Prop
  | (_, e) :: args, (mode, t) :: ps, kvs :: kvss => ArgKeys Γ mode t e kvs ∧ ArgsKeys Γ args ps kvss
  | _, _, _ => True

theorem ArgKeys.nil {Γ : Ctx} {mode : Mode} {t : Ty} {e : Expr}
    (h : placeOf e = none ∨ (mode == .mut || mode == .out) = false) : ArgKeys Γ mode t e [] := by
  unfold ArgKeys
  split
  · rename_i x steps he
    intro hm
    rcases h with h | h
    · simp [h] at he
    · simp [h] at hm
  · trivial

/-- Writing back what `mut` and `out` parameters came to, each of its
parameter's type, keeps every variable at its type: a variable's, or through
its steps, with the keys it was evaluated with, a part's, which may return
early as an index's mode says. -/
theorem writeBack_sound {env : InferEnv} : ∀ (n : Nat) (Γ : Ctx) (ρ : VEnv) (args : List (Option Mode × Expr))
    (ps : List (Mode × Ty)) (kvss : List (List Val)) (after : List (Option Val)),
    Quiet (argsF env n Γ args ps) () → Γ.bounds = [] → Agree Γ.vars ρ → ArgsKeys Γ args ps kvss →
    (∀ q ∈ after.zip ps, ∀ v, q.1 = some v → HasType v q.2.2) →
    Sound Γ.ret (Agree Γ.vars) (writeBack Γ ρ args ps kvss after)
  | 0, _, _, _, _, _, _, h, _, _, _, _ => by simp [argsF] at h
  | n + 1, Γ, ρ, (mk, e) :: args, (mode, t) :: ps, kvs :: kvss, a :: after, h, hb, hρ, hk, ha => by
    simp only [argsF] at h
    obtain ⟨⟨⟩, -, hr⟩ := h.bind_inv
    have hrest : ∀ q ∈ after.zip ps, ∀ v, q.1 = some v → HasType v q.2.2 :=
      fun q hq => ha q (by simp [hq])
    obtain ⟨hk1, hk2⟩ := hk
    simp only [writeBack]
    split
    · rename_i hmode
      unfold ArgKeys at hk1
      generalize hx : placeOf e = p at hk1 ⊢
      rcases p with _ | ⟨x, _ | ⟨st, rest⟩⟩
      · exact writeBack_sound n Γ ρ args ps kvss after hr hb hρ hk2 hrest
      · rcases a with _ | v
        · exact trivial
        · obtain ⟨u, k, u', hl, ⟨keys, hp, -, -⟩, hu⟩ := hk1 hmode
          simp only [setPlan, Option.some.injEq, Prod.mk.injEq] at hp
          obtain ⟨rfl, -⟩ := hp
          have hw : HasType v u := roll_of hu (ha (some v, (mode, t)) (by simp) v rfl).expose
          exact writeBack_sound n Γ _ args ps kvss after hr hb (setVar_agree hρ hl hw) hk2 hrest
      · rcases a with _ | v
        · exact trivial
        · obtain ⟨u, k, u', hl, ⟨keys, hp, hkf, hm⟩, hu⟩ := hk1 hmode
          obtain ⟨o, ho, hot⟩ := hρ.lookup x u k hl
          have hv : HasType v u' := roll_of hu (ha (some v, (mode, t)) (by simp) v rfl).expose
          simp only [hl, ho]
          refine Sound.bind' (writeSteps_sound hb u u' keys o (st :: rest) kvs hp hkf hv hot
            (fun _ e => by cases e) hm) (fun w hw => ?_)
          exact writeBack_sound n Γ _ args ps kvss after hr hb (setVar_agree hρ hl hw) hk2 hrest
    · exact writeBack_sound n Γ ρ args ps kvss after hr hb hρ hk2 hrest
  | _ + 1, _, ρ, [], _, _, _, _, _, hρ, _, _ => by simp only [writeBack]; exact hρ
  | _ + 1, _, ρ, _ :: _, [], _, _, _, _, hρ, _, _ => by simp only [writeBack]; exact hρ
  | _ + 1, _, ρ, _ :: _, _ :: _, [], _, _, _, hρ, _, _ => by simp only [writeBack]; exact hρ
  | _ + 1, _, ρ, _ :: _, _ :: _, _ :: _, [], _, _, hρ, _, _ => by simp only [writeBack]; exact hρ

/-- Steps that take a part as an index with neither `?` nor `!` only where a
`set` may: none, as a place being read has. -/
def NoBare (steps : List Step) : Prop := ∀ e, Step.index e none ∉ steps

theorem stepsF_nobare {env : InferEnv} : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (steps : List Step) (u : Ty),
    Quiet (stepsF env n Γ t steps) u → NoBare steps
  | 0, _, _, _, _, h => by simp [stepsF] at h
  | _ + 1, _, _, [], _, _ => by simp [NoBare]
  | n + 1, Γ, t, [st], u, h => by
    simp only [stepsF] at h
    intro e he
    simp only [List.mem_singleton] at he
    subst he
    rcases n with _ | n <;> simp [stepF] at h
  | n + 1, Γ, t, st :: st' :: rest, u, h => by
    simp only [stepsF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    intro e he
    rcases List.mem_cons.mp he with rfl | he
    · rcases n with _ | n <;> simp [stepF] at hw
    · exact stepsF_nobare n Γ w (st' :: rest) u h e he

theorem fieldType_expose {t t' : Ty} (h : t.expose = t'.expose) (s : Sel) : fieldType t s = fieldType t' s := by
  unfold fieldType; rw [h]

theorem indexTypes_expose {t t' : Ty} (h : t.expose = t'.expose) : indexTypes t = indexTypes t' := by
  unfold indexTypes; rw [h]

theorem setPlan_snoc : ∀ (t : Ty) (ss : List Step) (w : Ty) (ks : List (Expr × Ty)) (st : Step) (u : Ty)
    (ks2 : List (Expr × Ty)), NoBare ss → setPlan none t ss = some (w, ks) →
    setPlan none w [st] = some (u, ks2) → setPlan none t (ss ++ [st]) = some (u, ks ++ ks2)
  | t, [], w, ks, st, u, ks2, _, h1, h2 => by
    simp only [setPlan, Option.some.injEq, Prod.mk.injEq] at h1
    obtain ⟨rfl, rfl⟩ := h1
    simpa using h2
  | t, .field s :: rest, w, ks, st, u, ks2, hn, h1, h2 => by
    simp only [setPlan] at h1
    split at h1
    · rename_i ft hf
      have := setPlan_snoc ft rest w ks st u ks2 (fun e he => hn e (by simp [he])) h1 h2
      simp only [List.cons_append, setPlan, hf]
      exact this
    · simp at h1
  | t, .index k none :: rest, _, _, _, _, _, hn, _, _ => absurd (by simp) (hn k)
  | t, .index k (some m) :: rest, w, ks, st, u, ks2, hn, h1, h2 => by
    simp only [setPlan] at h1
    split at h1
    · rename_i et kt hi
      cases hr : setPlan none et rest with
      | none => simp [hr] at h1
      | some q =>
        obtain ⟨w', ks'⟩ := q
        simp only [hr, Option.map_some, Option.some.injEq, Prod.mk.injEq] at h1
        obtain ⟨rfl, rfl⟩ := h1
        have := setPlan_snoc et rest w' ks' st u ks2 (fun e he => hn e (by simp [he])) hr h2
        simp only [List.cons_append, setPlan, hi, this, Option.map_some, List.cons_append]
    · simp at h1

theorem PathOk.snoc {Γ : Ctx} {t w u : Ty} {ss : List Step} {st : Step} {k1 k2 : List Val}
    (hn : NoBare ss) (h1 : PathOk Γ t ss w k1) (h2 : PathOk Γ w [st] u k2) :
    PathOk Γ t (ss ++ [st]) u (k1 ++ k2) := by
  obtain ⟨keys1, hp1, ⟨hl1, hk1⟩, hm1⟩ := h1
  obtain ⟨keys2, hp2, ⟨hl2, hk2⟩, hm2⟩ := h2
  refine ⟨keys1 ++ keys2, setPlan_snoc t ss w keys1 st u keys2 hn hp1 hp2, ⟨by simp [hl1, hl2], ?_⟩, ?_⟩
  · intro p hp
    rw [List.zip_append hl1] at hp
    rcases List.mem_append.mp hp with hp | hp
    · exact hk1 p hp
    · exact hk2 p hp
  · intro k m hkm
    rcases List.mem_append.mp hkm with hkm | hkm
    · exact hm1 k m hkm
    · exact hm2 k m hkm

theorem PathOk.ret {Γ Γ' : Ctx} (hr : Γ.ret = Γ'.ret) {t u : Ty} {steps : List Step} {kvs : List Val}
    (h : PathOk Γ t steps u kvs) : PathOk Γ' t steps u kvs := by
  obtain ⟨keys, hp, hk, hm⟩ := h
  refine ⟨keys, hp, hk, fun k m hkm => ?_⟩
  have := hm k m hkm
  cases m <;> simpa only [Ctx.fitsMode, Ctx.returnsOption, Ctx.returnsResult, hr] using this

/-- What a `mut` or `out` argument's place holds, if it holds anything yet,
of type `t`, and its keys' values, through `steps` from variable `x`. -/
def PlaceOk (Γ : Ctx) (x : String) (steps : List Step) (t : Ty) (p : Option Val × List Val) : Prop :=
  (∀ v, p.1 = some v → HasType v t) ∧ NoBare steps ∧
    ∃ u k u', Γ.lookup x = some (u, k) ∧ PathOk Γ u steps u' p.2 ∧ u'.expose = t.expose

theorem PlaceOk.congr {Γ Γ' : Ctx} (hv : Γ.vars = Γ'.vars) (hr : Γ.ret = Γ'.ret) {x : String}
    {steps : List Step} {t : Ty} {p : Option Val × List Val} (h : PlaceOk Γ x steps t p) :
    PlaceOk Γ' x steps t p := by
  obtain ⟨hv', hn, u, k, u', hl, hp, hu⟩ := h
  exact ⟨hv', hn, u, k, u', by simpa [Ctx.lookup, hv] using hl, hp.ret hr, hu⟩

theorem PlaceOk.retype {Γ : Ctx} {x : String} {steps : List Step} {t t' : Ty} {p : Option Val × List Val}
    (he : t.expose = t'.expose) (h : PlaceOk Γ x steps t p) : PlaceOk Γ x steps t' p := by
  obtain ⟨hv, hn, u, k, u', hl, hp, hu⟩ := h
  exact ⟨fun v e => roll_of he.symm (hv v e).expose, hn, u, k, u', hl, hp, hu.trans he⟩

theorem PathOk.cons {Γ : Ctx} {t w u : Ty} {st : Step} {rest : List Step} {k ks : List Val}
    (hst : ∀ e, st ≠ .index e none) (h1 : PathOk Γ.through t [st] w k) (h2 : PathOk Γ w rest u ks) :
    PathOk Γ t (st :: rest) u (k ++ ks) := by
  obtain ⟨keys1, hp1, ⟨hl1, hk1⟩, hm1⟩ := h1
  obtain ⟨keys2, hp2, ⟨hl2, hk2⟩, hm2⟩ := h2
  have hm : ∀ e m, Step.index e (some m) ∈ st :: rest → Γ.fitsMode m = true := by
    intro e m hem
    rcases List.mem_cons.mp hem with rfl | hem
    · exact hm1 e m (by simp)
    · exact hm2 e m hem
  cases st with
  | field s =>
    simp only [setPlan] at hp1
    split at hp1
    · rename_i ft hf
      simp only [Option.some.injEq, Prod.mk.injEq] at hp1
      obtain ⟨rfl, rfl⟩ := hp1
      match k, hl1 with
      | [], _ =>
        refine ⟨keys2, by simp [setPlan, hf, hp2], ⟨by simpa using hl2, by simpa using hk2⟩, hm⟩
    · simp at hp1
  | index e mode =>
    cases mode with
    | none => exact absurd rfl (hst e)
    | some m =>
      simp only [setPlan] at hp1
      split at hp1
      · rename_i et kt hi
        simp only [Option.map_some, Option.some.injEq, Prod.mk.injEq] at hp1
        obtain ⟨rfl, rfl⟩ := hp1
        match k, hl1 with
        | [kv], _ =>
          have hkv : HasType kv kt := hk1 (kv, (e, kt)) (by simp)
          refine ⟨(e, kt) :: keys2, by simp [setPlan, hi, hp2], ⟨by simpa using hl2, ?_⟩, hm⟩
          intro p hp
          simp only [List.singleton_append, List.zip_cons_cons, List.mem_cons] at hp
          rcases hp with rfl | hp
          · exact hkv
          · exact hk2 p hp
      · simp at hp1

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallOk sigs call)
include hcall
-- Only `evalCall_sound` uses `hcall` itself, the rest through it, which the
-- linter does not see across a mutual block.
set_option linter.unusedSectionVars false

mutual

/-- **A datafun expression that checks evaluates to a value of its type**,
or returns early with a value of the function's return type, or stops where
a call may: in a context with no type parameter's bound, from any variables
that line up with it, which still do after, for any inference and any sound
calls. -/
theorem evalS_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), Quiet (synthF env n Γ e) t →
    Γ.Runs sigs → SoundT Γ.vars Γ.ret (HasType · t) (evalS env call n Γ e)
  | 0, _, _, _, h, _ => by simp [synthF] at h
  | n + 1, Γ, e, t, h, hb => by
    have h0 := h
    cases e
    case hinted hh inner =>
      simp only [synthF] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨_, hc, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      rw [Quiet.liftE_iff] at hu
      simp only [evalS, hu]
      exact SoundT.bind' (P := (· = u)) (SoundT.pure' rfl) (fun _ e => e ▸ evalForm_sound n _ inner u hc hb)
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
        simp only [evalS]
        refine SoundT.get' (fun ρ hρ => ?_)
        obtain ⟨o, hv, hvt⟩ := hρ.lookup x u k hl
        simp only [hv]
        cases o with
        | none => exact SoundT.uninit
        | some v => exact SoundT.take' (P := fun w => HasType w u) (a := v) (hvt v rfl) _ x
    case binop op a b =>
      simp only [synthF] at h
      obtain ⟨u, hp, hf⟩ := h.bind_inv
      simp only [evalS]
      exact SoundT.bind' (evalPair_sound n _ a b none u hp hb) (fun r ⟨hr, ha, hb'⟩ => by
        obtain ⟨_, va, vb⟩ := r
        simp only at hr; subst hr
        exact SoundT.here (binOp_sound (Γ := Γ.enter _) hb.1 hf ha hb'))
    case tryOpt a =>
      simp only [synthF] at h
      obtain ⟨u, ha, h⟩ := h.bind_inv
      split at h
      · rename_i p hp
        split at h
        · rename_i hr
          rw [Quiet.pure_iff] at h; subst h
          simp only [evalS]
          refine SoundT.bind' (evalS_sound n _ a u ha hb) (fun v hv => ?_)
          have hv' := hv.expose
          rw [hp] at hv'
          rcases hv'.option_inv with rfl | ⟨w, rfl, hw⟩
          · exact SoundT.earlyHere (EarlyOk.none_of hr)
          · exact SoundT.pure' hw
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
          refine SoundT.bind' (evalS_sound n _ a u ha hb) (fun v hv => ?_)
          have hv' := hv.expose
          rw [hp] at hv'
          rcases hv'.result_inv with ⟨w, rfl, hw⟩ | ⟨w, rfl, hw⟩
          · exact SoundT.pure' hw
          · exact SoundT.earlyHere (EarlyOk.er_of hr hw)
        · simp at h
      · simp at h
    case adapt a =>
      simp only [synthF] at h
      simp only [evalS]
      exact evalS_sound n _ a t h hb
    case call f args =>
      simp only [synthF] at h
      simp only [evalS]
      exact evalCall_sound n _ f args none t h hb
    case place x steps =>
      simp only [synthF] at h
      cases hl : (Γ.enter (.place x steps)).lookup x with
      | none => simp [hl] at h
      | some p =>
        obtain ⟨u, k⟩ := p
        simp only [hl] at h
        split at h
        · simp at h
        simp only [evalS]
        refine SoundT.get' (fun ρ hρ => ?_)
        obtain ⟨o, hv, hvt⟩ := hρ.lookup x u k hl
        cases o with
        | none => simp only [hv]; exact SoundT.uninit
        | some v =>
          simp only [hv, hl]
          exact evalSteps_sound n _ u v steps t h (hvt v rfl) hb
    case proj b sel =>
      simp only [synthF] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨w, hw, h⟩ := h.bind_inv
      split at h
      · simp at h
      rw [Quiet.pure_iff] at h; subst h
      rw [Quiet.liftE_iff] at hw
      simp only [evalS]
      have hs := evalS_sound n _ b u hu (Ctx.Runs.under (Γ := Γ.enter _) hb b)
      simp only [Ctx.under_vars, Ctx.under_ret] at hs
      exact SoundT.bind' hs (fun v hv => SoundT.here (fieldVal_sound hv hw))
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
        refine SoundT.bind' (P := (u = ·)) (SoundT.here rfl) (fun _ e => ?_)
        subst e
        simp only [hi]
        refine SoundT.bind' (P := ((et, kt) = ·)) (SoundT.pure' rfl) (fun _ e => ?_)
        subst e
        have hs := evalS_sound n _ b u hu (Ctx.Runs.under (Γ := Γ.enter _) hb b)
        simp only [Ctx.under_vars, Ctx.under_ret] at hs
        exact SoundT.bind' hs (fun v hv =>
          SoundT.bind' (evalC_sound n _ key kt hk hb) (fun kv hkv =>
            SoundT.here (indexVal_sound hv hi hkv (missing_ok (by simpa using hfit)))))
    case unop op a =>
      have hb0 : (Γ.enter (.unop op a)).bounds = [] := hb.1
      cases op
      case neg =>
        simp only [synthF] at h
        obtain ⟨u, ha, h⟩ := h.bind_inv
        simp only [evalS, typeOf_quiet ha]
        refine SoundT.bind' (P := (u = ·)) (SoundT.here rfl) (fun _ e => ?_)
        subst e
        refine SoundT.bind' (evalS_sound n _ a u ha hb) (fun v hv => ?_)
        refine SoundT.here ?_
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
        refine SoundT.bind' (P := (u = ·)) (SoundT.here rfl) (fun _ e => ?_)
        subst e
        refine SoundT.bind' (evalS_sound n _ a u ha hb) (fun v hv => ?_)
        refine SoundT.here ?_
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
        refine SoundT.bind' (P := (u = ·)) (SoundT.here rfl) (fun _ e => ?_)
        subst e
        refine SoundT.bind' (evalS_sound n _ a u ha hb) (fun v hv => ?_)
        refine SoundT.here ?_
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
        refine SoundT.bind' (P := (u = ·)) (SoundT.here rfl) (fun _ e => ?_)
        subst e
        refine SoundT.bind' (evalS_sound n _ a u ha hb) (fun v hv => ?_)
        refine SoundT.here ?_
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
      exact SoundT.bind' (P := (· = t)) (SoundT.here rfl) (fun _ e => e ▸ evalForm_sound n _ _ t hc hb)

theorem evalC_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), Quiet (checkF env n Γ e t) () →
    Γ.Runs sigs → SoundT Γ.vars Γ.ret (HasType · t) (evalC env call n Γ e t)
  | 0, _, _, _, h, _ => by simp [checkF] at h
  | n + 1, Γ, e, t, h, hb => by
    cases e
    case hinted hh inner =>
      simp only [checkF] at h
      obtain ⟨u, -, h⟩ := h.bind_inv
      split at h
      · simp at h
      simp only [evalC]
      exact evalForm_sound n _ inner t h hb
    all_goals
      simp only [checkF] at h
      simp only [evalC]
      exact evalForm_sound n _ _ t h hb

theorem evalForm_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), Quiet (checkForm env n Γ e t) () →
    Γ.Runs sigs → SoundT Γ.vars Γ.ret (HasType · t) (evalForm env call n Γ e t)
  | 0, _, _, _, h, _ => by simp [checkForm] at h
  | n + 1, Γ, e, t, h, hb => by
    cases e
    case none_ =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq; exact SoundT.pure' (roll_of hq .none)
      · simp at h
    case er p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq
        simp only [hq]
        exact SoundT.map' (evalC_sound n Γ p .error h hb) (fun v hv => roll_of hq (.er hv))
      · simp at h
    case some_ p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq
        simp only [hq]
        exact SoundT.map' (evalC_sound n Γ p q h hb) (fun v hv => roll_of hq (.some hv))
      · simp at h
    case ok p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i q hq
        simp only [hq]
        exact SoundT.map' (evalC_sound n Γ p q h hb) (fun v hv => roll_of hq (.ok hv))
      · simp at h
    case tt | ff =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; subst hc; exact SoundT.pure' .bool
      · simp at h
    case str s =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; subst hc; exact SoundT.pure' .str
      · simp at h
    case int k =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; exact SoundT.pure' (roll_of hc (.int trivial))
      · split at h
        · rename_i lo hi c hr
          split at h
          · rename_i hin
            simp only [Bool.and_eq_true, decide_eq_true_eq] at hin
            exact SoundT.pure' (roll_of rfl (.int (Lit.intRange_sound hr hin.1 hin.2)))
          · simp at h
        · simp at h
    case hex k =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hc; simp only [beq_iff_eq] at hc; exact SoundT.pure' (roll_of hc (.hex trivial))
      · split at h
        · rename_i bits c hr
          split at h
          · rename_i hin
            exact SoundT.pure' (roll_of rfl (.hex (Lit.hexBits_sound hr (by simpa using hin))))
          · simp at h
        · simp at h
    case float =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hx; exact SoundT.pure' (roll_of hx (.float rfl))
      · rename_i hx; exact SoundT.pure' (roll_of hx (.float rfl))
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
        exact SoundT.map' (evalPairs_sound n Γ es _ h hb hlen) (fun vs ⟨hl, hvs⟩ =>
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
        exact SoundT.map' (evalFields_sound n Γ fs _ h hb hlen) (fun vs ⟨hl, hvs⟩ =>
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
          | exact SoundT.map' (evalAll_sound n Γ es u h hb) (fun vs ⟨_, hvs⟩ => roll_of hx (.list hvs))
          | exact SoundT.map' (evalAll_sound n Γ es u h hb) (fun vs ⟨_, hvs⟩ => roll_of hx (.set hvs))
      · simp at h
    case map es =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i k v hx
        simp only [hx]
        exact SoundT.map' (evalEntries_sound n Γ es k v h hb) (fun ps hps =>
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
        exact SoundT.map' (evalAll_sound n Γ es u h hb) (fun vs ⟨hvl, hvs⟩ =>
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
        exact SoundT.map' (evalRows_sound n Γ rows _ h hb) (fun rs hrs =>
          roll_of hx (.table hh (fun r hr => (hrs r hr).1) (fun r hr => (hrs r hr).2)))
      · simp at h
    case atom a =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i b hx
        split at h
        · rename_i hab; simp only [beq_iff_eq] at hab; subst hab; exact SoundT.pure' (roll_of hx .atom)
        · simp at h
      · rename_i vs hx
        split at h
        · rename_i hl; exact SoundT.pure' (roll_of hx (.atomEnum hl))
        · simp at h
      · simp at h
    case term a p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i b u hx
        simp only [hx]
        split at h
        · rename_i hab; simp only [beq_iff_eq] at hab; subst hab
          exact SoundT.map' (evalC_sound n Γ p u h hb) (fun v hv => roll_of hx (.term hv))
        · simp at h
      · rename_i vs hx
        simp only [hx]
        split at h
        · rename_i u hu
          simp only [hu]
          exact SoundT.map' (evalC_sound n Γ p u h hb) (fun v hv => roll_of hx (.termEnum hu hv))
        · simp at h
      · simp at h
    case enum inner =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · exact evalC_sound n Γ inner t h hb
      · simp at h
    case data p | error p =>
      simp only [checkForm] at h; simp only [evalForm]
      split at h
      · rename_i hx
        obtain ⟨u, hu, -⟩ := h.bind_inv
        first
          | exact SoundT.bind' (evalS_sound n Γ p u hu hb) (fun _ _ => SoundT.pure' (roll_of hx .data))
          | exact SoundT.bind' (evalS_sound n Γ p u hu hb) (fun _ _ => SoundT.pure' (roll_of hx .error))
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
        exact SoundT.bind' (evalPair_sound n Γ a b _ u hp hb) (fun q ⟨hq, ha, hb'⟩ => by
          obtain ⟨_, va, vb⟩ := q
          simp only at hq; subst hq
          exact SoundT.here (binOp_sound hb.1 hf ha hb'))
      · rename_i hy
        simp only [hy, Bool.false_eq_true, ↓reduceIte]
        exact evalSub_sound n Γ _ t h hb
    case adapt a =>
      simp only [checkForm] at h; simp only [evalForm]
      obtain ⟨u, hu, h⟩ := h.bind_inv
      split at h
      · simp at h
      rename_i hc
      exact (evalS_sound n _ a u hu hb).weaken (fun v hv => hv.adapt (by simpa using hc))
    case call f args =>
      simp only [checkForm] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      split at h
      · simp at h
      rename_i hm
      simp only [evalForm]
      exact (evalCall_sound n Γ f args _ u hu hb).weaken
        (fun v hv => roll_of (Ty.meet_iff.mp (by simpa using hm)).symm hv.expose)
    case unop op a =>
      cases op
      case neg =>
        have hb0 : Γ.boundOf t.expose = Option.none := boundOf_nil hb.1 _
        have floatCase : ∀ v, HasType v t.expose →
            (isFloatT t.expose || Γ.boundOf t.expose == Option.some .float) = true →
            SoundT Γ.vars Γ.ret (HasType · t) (here (unOp .neg t.expose v)) := by
          intro v hv hf
          simp only [hb0, isFloatT] at hf
          have hfl : t.expose.isFloat = true := by simpa using hf
          refine SoundT.here ?_
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
              exact SoundT.pure' (roll_of rfl (.int (Lit.intRange_sound hr hin.1 hin.2)))
            · simp at h
          · rename_i hr
            simp only [hr]
            split at h
            · rename_i hf
              simp only [hf, ↓reduceIte]
              exact SoundT.bind' (evalC_sound n _ _ _ h hb) (fun v hv => floatCase v hv hf)
            · rename_i hf
              simp only [hf, Bool.false_eq_true, ↓reduceIte]
              exact evalSub_sound n Γ _ t h hb
        · rename_i hne
          split
          · rename_i k; exact absurd rfl (hne k)
          · split at h
            · rename_i hf
              simp only [hf, ↓reduceIte]
              exact SoundT.bind' (evalC_sound n _ _ _ h hb) (fun v hv => floatCase v hv hf)
            · rename_i hf
              simp only [hf, Bool.false_eq_true, ↓reduceIte]
              exact evalSub_sound n Γ _ t h hb
      all_goals
        simp only [checkForm] at h; simp only [evalForm]
        exact evalSub_sound n Γ _ t h hb
    all_goals
      simp only [checkForm] at h; simp only [evalForm]
      exact evalSub_sound n Γ _ t h hb

theorem evalSteps_sound : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (steps : List Step) (u : Ty),
    Quiet (stepsF env n Γ t steps) u → HasType v t → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (HasType · u) (evalSteps env call n Γ t v steps)
  | 0, _, _, _, _, _, h, _, _ => by simp [stepsF] at h
  | n + 1, Γ, t, v, [], u, h, hv, hb => by
    simp only [stepsF, Quiet.pure_iff] at h; subst h
    simp only [evalSteps]; exact SoundT.pure' hv
  | n + 1, Γ, t, v, [st], u, h, hv, hb => by
    simp only [stepsF] at h
    simp only [evalSteps]
    exact evalStep_sound n Γ t v st u h hv hb
  | n + 1, Γ, t, v, st :: st' :: rest, u, h, hv, hb => by
    simp only [stepsF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    simp only [evalSteps, typeOf_quiet hw]
    refine SoundT.bind' (P := (w = ·)) (SoundT.here rfl) (fun _ e => ?_)
    subst e
    exact SoundT.bind' (evalStep_sound n Γ.through t v st w hw hv hb) (fun x hx =>
      evalSteps_sound n Γ w x (st' :: rest) u h hx hb)

theorem evalStep_sound : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (st : Step) (u : Ty),
    Quiet (stepF env n Γ t st) u → HasType v t → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (HasType · u) (evalStep env call n Γ t v st)
  | 0, _, _, _, _, _, h, _, _ => by simp [stepF] at h
  | n + 1, Γ, t, v, .field sel, u, h, hv, hb => by
    simp only [stepF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    split at h
    · simp at h
    rw [Quiet.pure_iff] at h; subst h
    rw [Quiet.liftE_iff] at hw
    simp only [evalStep]
    exact SoundT.here (fieldVal_sound hv hw)
  | n + 1, Γ, t, v, .index k none, u, h, hv, hb => by simp [stepF] at h
  | n + 1, Γ, t, v, .index k (some m), u, h, hv, hb => by
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
    refine SoundT.bind' (P := ((et, kt) = ·)) (SoundT.pure' rfl) (fun _ e => ?_)
    subst e
    exact SoundT.bind' (evalC_sound n Γ.key k kt hk hb) (fun kv hkv =>
      SoundT.here (indexVal_sound hv hi hkv (missing_ok (by simpa using hfit))))

theorem evalPath_sound : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (steps : List Step) (u : Ty),
    Quiet (stepsF env n Γ t steps) u → HasType v t → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun p => HasType p.1 u ∧ PathOk Γ t steps u p.2) (evalPath env call n Γ t v steps)
  | 0, _, _, _, _, _, h, _, _ => by simp [stepsF] at h
  | n + 1, Γ, t, v, [], u, h, hv, hb => by
    simp only [stepsF, Quiet.pure_iff] at h; subst h
    simp only [evalPath]
    exact SoundT.pure' ⟨hv, [], by simp [setPlan], ⟨rfl, by simp⟩, by simp⟩
  | n + 1, Γ, t, v, [st], u, h, hv, hb => by
    simp only [stepsF] at h
    simp only [evalPath]
    exact evalPathStep_sound n Γ t v st u h hv hb
  | n + 1, Γ, t, v, st :: st' :: rest, u, h, hv, hb => by
    simp only [stepsF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    have hst : ∀ e, st ≠ .index e none := by
      rintro e rfl
      rcases n with _ | n <;> simp [stepF] at hw
    simp only [evalPath, typeOf_quiet hw]
    refine SoundT.bind' (P := (w = ·)) (SoundT.here rfl) (fun _ e => ?_)
    subst e
    refine SoundT.bind' (evalPathStep_sound n Γ.through t v st w hw hv hb) (fun ⟨x, k⟩ ⟨hx, hk⟩ => ?_)
    refine SoundT.bind' (evalPath_sound n Γ w x (st' :: rest) u h hx hb) (fun ⟨r, ks⟩ ⟨hr, hks⟩ => ?_)
    exact SoundT.pure' ⟨hr, PathOk.cons hst hk hks⟩

theorem evalPathStep_sound : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (st : Step) (u : Ty),
    Quiet (stepF env n Γ t st) u → HasType v t → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun p => HasType p.1 u ∧ PathOk Γ t [st] u p.2) (evalPathStep env call n Γ t v st)
  | 0, _, _, _, _, _, h, _, _ => by simp [stepF] at h
  | n + 1, Γ, t, v, .field sel, u, h, hv, hb => by
    simp only [stepF] at h
    obtain ⟨w, hw, h⟩ := h.bind_inv
    split at h
    · simp at h
    rw [Quiet.pure_iff] at h; subst h
    rw [Quiet.liftE_iff] at hw
    simp only [evalPathStep]
    exact SoundT.bind' (SoundT.here (fieldVal_sound hv hw)) (fun x hx =>
      SoundT.pure' ⟨hx, [], by simp [setPlan, hw], ⟨rfl, by simp⟩, by simp⟩)
  | n + 1, Γ, t, v, .index k none, u, h, hv, hb => by simp [stepF] at h
  | n + 1, Γ, t, v, .index k (some m), u, h, hv, hb => by
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
    simp only [evalPathStep, hi]
    refine SoundT.bind' (P := ((et, kt) = ·)) (SoundT.pure' rfl) (fun _ e => ?_)
    subst e
    exact SoundT.bind' (evalC_sound n Γ.key k kt hk hb) (fun kv hkv =>
      SoundT.bind' (SoundT.here (indexVal_sound hv hi hkv (missing_ok (by simpa using hfit)))) (fun x hx =>
        SoundT.pure' ⟨hx, [(k, kt)], by simp [setPlan, hi], ⟨rfl, by simpa using hkv⟩,
          fun k' m' hkm => by simp at hkm; obtain ⟨rfl, rfl⟩ := hkm; simpa using hfit⟩))

/-- **A `mut` or `out` argument's place, as `synthF` types it, is what the
walker reads**: its value, if its variable has one, of its type, and its keys'
values, through the steps it folds to (`placeOf`), from its variable. -/
theorem placeS_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (x : String) (steps : List Step),
    Quiet (synthF env n Γ e) t → placeOf e = some (x, steps) → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (PlaceOk Γ x steps t) (placeS env call n Γ e)
  | 0, _, _, _, _, _, h, _, _ => by simp [synthF] at h
  | n + 1, Γ, e, t, x, steps, h, hp, hb => by
    cases e
    case var y =>
      simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
      obtain ⟨rfl, rfl⟩ := hp
      simp only [synthF] at h
      cases hl : (Γ.enter (.var y)).lookup y with
      | none => simp [hl] at h
      | some q =>
        obtain ⟨u, k⟩ := q
        simp only [hl] at h
        split at h
        · simp at h
        rw [Quiet.pure_iff] at h; subst h
        simp only [placeS]
        refine SoundT.get' (fun ρ hρ => ?_)
        obtain ⟨o, hv, hvt⟩ := hρ.lookup y u k hl
        simp only [hv]
        exact SoundT.pure' ⟨hvt, by simp [NoBare], u, k, u, hl,
          ⟨[], by simp [setPlan], ⟨rfl, by simp⟩, by simp⟩, rfl⟩
    case place y ss =>
      simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
      obtain ⟨rfl, rfl⟩ := hp
      simp only [synthF] at h
      cases hl : (Γ.enter (.place y ss)).lookup y with
      | none => simp [hl] at h
      | some q =>
        obtain ⟨u, k⟩ := q
        simp only [hl] at h
        split at h
        · simp at h
        simp only [placeS]
        refine SoundT.get' (fun ρ hρ => ?_)
        obtain ⟨o, hv, hvt⟩ := hρ.lookup y u k hl
        cases o with
        | none => simp only [hv]; exact SoundT.uninit
        | some v =>
          simp only [hv, hl]
          refine SoundT.bind' (evalPath_sound n _ u v ss t h (hvt v rfl) hb) (fun ⟨w, kvs⟩ ⟨hw, hpath⟩ => ?_)
          exact SoundT.pure' ⟨fun w' e => by cases e; exact hw, stepsF_nobare n _ u ss t h,
            u, k, t, hl, hpath, rfl⟩
    case hinted hh inner =>
      simp only [placeOf] at hp
      simp only [synthF] at h
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨_, hc, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      rw [Quiet.liftE_iff] at hu
      simp only [placeS, hu]
      exact SoundT.bind' (P := (· = u)) (SoundT.pure' rfl)
        (fun _ e => e ▸ placeForm_sound n _ inner u x steps hc hp hb)
    case proj b sel =>
      simp only [placeOf] at hp
      cases hq : placeOf b with
      | none => simp [hq] at hp
      | some q =>
        obtain ⟨y, ss⟩ := q
        simp only [hq, Option.map_some, Option.some.injEq, Prod.mk.injEq] at hp
        obtain ⟨rfl, rfl⟩ := hp
        simp only [synthF] at h
        obtain ⟨bt, hbt, h⟩ := h.bind_inv
        obtain ⟨ft, hf, h⟩ := h.bind_inv
        split at h
        · simp at h
        rw [Quiet.pure_iff] at h; subst h
        rw [Quiet.liftE_iff] at hf
        have hs := placeS_sound n _ b bt y ss hbt hq (Ctx.Runs.under (Γ := Γ.enter _) hb b)
        simp only [Ctx.under_vars, Ctx.under_ret] at hs
        simp only [placeS]
        refine SoundT.bind' hs (fun ⟨o, kvs⟩ hpo => ?_)
        have hpo := hpo.congr (Γ' := Γ) (by simp [Ctx.enter]) (by simp [Ctx.enter])
        obtain ⟨hov, hn, u, k, u', hl, hpath, hu⟩ := hpo
        cases o with
        | none => exact SoundT.uninit
        | some v =>
          refine SoundT.bind' (SoundT.here (fieldVal_sound (hov v rfl) hf)) (fun w hw => ?_)
          refine SoundT.pure' ⟨fun w' e => by cases e; exact hw, ?_, u, k, ft, hl, ?_, rfl⟩
          · intro e he
            rcases List.mem_append.mp he with he | he
            · exact hn e he
            · simp at he
          · have h1 : PathOk Γ u' [.field sel] ft [] :=
              ⟨[], by simp [setPlan, fieldType_expose hu, hf], ⟨rfl, by simp⟩, by simp⟩
            simpa using hpath.snoc hn h1
    case index b key mode =>
      cases mode with
      | none => simp [synthF] at h
      | some m =>
        simp only [placeOf] at hp
        cases hq : placeOf b with
        | none => simp [hq] at hp
        | some q =>
          obtain ⟨y, ss⟩ := q
          simp only [hq, Option.map_some, Option.some.injEq, Prod.mk.injEq] at hp
          obtain ⟨rfl, rfl⟩ := hp
          simp only [synthF] at h
          obtain ⟨bt, hbt, h⟩ := h.bind_inv
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
          have hs := placeS_sound n _ b bt y ss hbt hq (Ctx.Runs.under (Γ := Γ.enter _) hb b)
          simp only [Ctx.under_vars, Ctx.under_ret] at hs
          simp only [placeS, typeOf_quiet hbt]
          refine SoundT.bind' (P := (bt = ·)) (SoundT.here rfl) (fun _ e => ?_)
          subst e
          simp only [hi]
          refine SoundT.bind' (P := ((et, kt) = ·)) (SoundT.pure' rfl) (fun _ e => ?_)
          subst e
          refine SoundT.bind' hs (fun ⟨o, kvs⟩ hpo => ?_)
          have hpo := hpo.congr (Γ' := Γ) (by simp [Ctx.enter]) (by simp [Ctx.enter])
          obtain ⟨hov, hn, u, k, u', hl, hpath, hu⟩ := hpo
          refine SoundT.bind' (evalC_sound n _ key kt hk hb) (fun kv hkv => ?_)
          cases o with
          | none => exact SoundT.uninit
          | some v =>
            refine SoundT.bind' (SoundT.here (indexVal_sound (hov v rfl) hi hkv
              (missing_ok (by simpa using hfit)))) (fun w hw => ?_)
            refine SoundT.pure' ⟨fun w' e => by cases e; exact hw, ?_, u, k, et, hl, ?_, rfl⟩
            · intro e he
              rcases List.mem_append.mp he with he | he
              · exact hn e he
              · simp at he
            · have h1 : PathOk Γ u' [.index key (some m)] et [kv] :=
                ⟨[(key, kt)], by simp [setPlan, indexTypes_expose hu, hi], ⟨rfl, by simpa using hkv⟩,
                  fun k' m' hkm => by simp at hkm; obtain ⟨rfl, rfl⟩ := hkm; simpa using hfit⟩
              exact hpath.snoc hn h1
    all_goals simp [placeOf] at hp

/-- A place, as `checkForm` checks it, is what the walker reads. -/
theorem placeForm_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (x : String) (steps : List Step),
    Quiet (checkForm env n Γ e t) () → placeOf e = some (x, steps) → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (PlaceOk Γ x steps t) (placeForm env call n Γ e t)
  | 0, _, _, _, _, _, h, _, _ => by simp [checkForm] at h
  | n + 1, Γ, e, t, x, steps, h, hp, hb => by
    have hs : Quiet (subsumeF env n Γ e t) () := by
      cases e <;> first | (simp [placeOf] at hp; done) | (simp only [checkForm] at h; exact h)
    simp only [placeForm]
    exact placeSub_sound n Γ e t x steps hs hp hb

/-- A place, as `subsumeF` synthesizes it, is what the walker reads. -/
theorem placeSub_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (x : String) (steps : List Step),
    Quiet (subsumeF env n Γ e t) () → placeOf e = some (x, steps) → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (PlaceOk Γ x steps t) (placeSub env call n Γ e)
  | 0, _, _, _, _, _, h, _, _ => by simp [subsumeF] at h
  | n + 1, Γ, e, t, x, steps, h, hp, hb => by
    simp only [subsumeF] at h
    obtain ⟨u, hu, h⟩ := h.bind_inv
    split at h
    · simp at h
    rename_i hm
    simp only [placeSub]
    exact SoundT.weaken (placeS_sound n Γ e u x steps hu hp hb)
      (fun _ hpo => hpo.retype (Ty.meet_iff.mp (by simpa using hm)))

/-- A place, as `checkF` checks it, is what the walker reads. -/
theorem placeC_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (x : String) (steps : List Step),
    Quiet (checkF env n Γ e t) () → placeOf e = some (x, steps) → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (PlaceOk Γ x steps t) (placeC env call n Γ e t)
  | 0, _, _, _, _, _, h, _, _ => by simp [checkF] at h
  | n + 1, Γ, e, t, x, steps, h, hp, hb => by
    cases e
    case hinted hh inner =>
      simp only [checkF] at h
      obtain ⟨_, -, h⟩ := h.bind_inv
      split at h
      · obtain ⟨_, h, -⟩ := h.bind_inv
        simp at h
      simp only [placeC]
      exact placeForm_sound n _ inner t x steps h (by simpa [placeOf] using hp) hb
    all_goals first
      | (simp [placeOf] at hp; done)
      | (simp only [checkF] at h; simp only [placeC]; exact placeForm_sound n _ _ t x steps h hp hb)

theorem evalSub_sound : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), Quiet (subsumeF env n Γ e t) () →
    Γ.Runs sigs → SoundT Γ.vars Γ.ret (HasType · t) (evalSub env call n Γ e)
  | 0, _, _, _, h, _ => by simp [subsumeF] at h
  | n + 1, Γ, e, t, h, hb => by
    simp only [subsumeF] at h
    obtain ⟨u, hu, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm
      simp only [evalSub]
      exact (evalS_sound n Γ e u hu hb).weaken
        (fun v hv => roll_of (Ty.meet_iff.mp (by simpa using hm)).symm hv.expose)

theorem evalPairs_sound : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (ts : List Ty),
    Quiet (pairsF env n Γ es ts) () → Γ.Runs sigs → es.length = ts.length →
    SoundT Γ.vars Γ.ret (fun vs => vs.length = min es.length ts.length ∧ ∀ p ∈ vs.zip ts, HasType p.1 p.2)
      (evalPairs env call n Γ es ts)
  | 0, _, _, _, h, _, _ => by simp [pairsF] at h
  | n + 1, Γ, e :: es, t :: ts, h, hb, hlen => by
    simp only [pairsF] at h
    obtain ⟨_, he, h⟩ := h.bind_inv
    simp only [evalPairs]
    exact SoundT.bind' (evalC_sound n Γ e t he hb) (fun v hv =>
      SoundT.bind' (evalPairs_sound n Γ es ts h hb (by simpa using hlen)) (fun vs ⟨hl, hvs⟩ =>
        SoundT.pure' ⟨by simp [hl, Nat.succ_min_succ], by
          intro p hp
          simp only [List.zip_cons_cons, List.mem_cons] at hp
          rcases hp with rfl | hp
          · exact hv
          · exact hvs p hp⟩))
  | n + 1, Γ, [], [], h, hb, _ => by simp only [evalPairs]; exact SoundT.pure' ⟨by simp, by simp⟩
  | n + 1, Γ, [], _ :: _, _, _, hlen => by simp at hlen
  | n + 1, Γ, _ :: _, [], _, _, hlen => by simp at hlen

theorem evalFields_sound : ∀ (n : Nat) (Γ : Ctx) (fs : List (String × Expr)) (ts : List (String × Ty)),
    Quiet (fieldsF env n Γ fs ts) () → Γ.Runs sigs → fs.length = ts.length →
    SoundT Γ.vars Γ.ret (fun vs => vs.map (·.1) = (fs.zip ts).map (·.2.1) ∧ ∀ p ∈ vs.zip ts, HasType p.1.2 p.2.2)
      (evalFields env call n Γ fs ts)
  | 0, _, _, _, h, _, _ => by simp [fieldsF] at h
  | n + 1, Γ, (m, e) :: fs, (k, t) :: ts, h, hb, hlen => by
    simp only [fieldsF] at h
    split at h
    · simp at h
    rename_i hmk
    obtain ⟨_, he, h⟩ := h.bind_inv
    simp only [evalFields]
    exact SoundT.bind' (evalC_sound n Γ e t he hb) (fun v hv =>
      SoundT.bind' (evalFields_sound n Γ fs ts h hb (by simpa using hlen)) (fun vs ⟨hl, hvs⟩ =>
        SoundT.pure' ⟨by simp_all, by
          intro p hp
          simp only [List.zip_cons_cons, List.mem_cons] at hp
          rcases hp with rfl | hp
          · exact hv
          · exact hvs p hp⟩))
  | n + 1, Γ, [], [], h, hb, _ => by simp only [evalFields]; exact SoundT.pure' ⟨by simp, by simp⟩
  | n + 1, Γ, [], _ :: _, _, _, hlen => by simp at hlen
  | n + 1, Γ, _ :: _, [], _, _, hlen => by simp at hlen

theorem evalAll_sound : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (t : Ty),
    Quiet (allF env n Γ es t) () → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun vs => vs.length = es.length ∧ ∀ v ∈ vs, HasType v t) (evalAll env call n Γ es t)
  | 0, _, _, _, h, _ => by simp [allF] at h
  | n + 1, Γ, [], t, h, hb => by simp only [evalAll]; exact SoundT.pure' ⟨rfl, by simp⟩
  | n + 1, Γ, e :: es, t, h, hb => by
    simp only [allF] at h
    obtain ⟨_, he, h⟩ := h.bind_inv
    simp only [evalAll]
    exact SoundT.bind' (evalC_sound n Γ e t he hb) (fun v hv =>
      SoundT.bind' (evalAll_sound n Γ es t h hb) (fun vs ⟨hl, hvs⟩ =>
        SoundT.pure' ⟨by simp [hl], by
          intro w hw
          simp only [List.mem_cons] at hw
          rcases hw with rfl | hw
          · exact hv
          · exact hvs w hw⟩))

theorem evalEntries_sound : ∀ (n : Nat) (Γ : Ctx) (es : List (Expr × Expr)) (k v : Ty),
    Quiet (entriesF env n Γ es k v) () → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun ps => ∀ p ∈ ps, HasType p.1 k ∧ HasType p.2 v) (evalEntries env call n Γ es k v)
  | 0, _, _, _, _, h, _ => by simp [entriesF] at h
  | n + 1, Γ, [], k, v, h, hb => by simp only [evalEntries]; exact SoundT.pure' (by simp)
  | n + 1, Γ, (ke, ve) :: es, k, v, h, hb => by
    simp only [entriesF] at h
    obtain ⟨_, hk, h⟩ := h.bind_inv
    obtain ⟨_, hv, h⟩ := h.bind_inv
    simp only [evalEntries]
    exact SoundT.bind' (evalC_sound n Γ ke k hk hb) (fun kv hkv =>
      SoundT.bind' (evalC_sound n Γ ve v hv hb) (fun vv hvv =>
        SoundT.bind' (evalEntries_sound n Γ es k v h hb) (fun rest hr =>
          SoundT.pure' (by
            intro p hp
            simp only [List.mem_cons] at hp
            rcases hp with rfl | hp
            · exact ⟨hkv, hvv⟩
            · exact hr p hp))))

theorem evalRows_sound : ∀ (n : Nat) (Γ : Ctx) (rows : List (List Expr)) (cols : List (String × Ty)),
    Quiet (rowsF env n Γ rows cols) () → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun rs => ∀ r ∈ rs, r.length = cols.length ∧ ∀ p ∈ r.zip cols, HasType p.1 p.2.2)
      (evalRows env call n Γ rows cols)
  | 0, _, _, _, h, _ => by simp [rowsF] at h
  | n + 1, Γ, [], cols, h, hb => by simp only [evalRows]; exact SoundT.pure' (by simp)
  | n + 1, Γ, r :: rs, cols, h, hb => by
    simp only [rowsF] at h
    split at h
    · simp at h
    rename_i hlen
    obtain ⟨_, hr, h⟩ := h.bind_inv
    simp only [evalRows]
    exact SoundT.bind' (evalPairs_sound n Γ r _ hr hb (by simpa using hlen)) (fun vs ⟨hl, hvs⟩ =>
      SoundT.bind' (evalRows_sound n Γ rs cols h hb) (fun rest hrest =>
        SoundT.pure' (by
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
theorem evalCall_sound : ∀ (n : Nat) (Γ : Ctx) (f : String) (args : List (Option Mode × Expr))
    (exp : Option Ty) (t : Ty), Quiet (callF env n Γ f args exp) t → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (HasType · t) (evalCall env call n Γ f args exp)
  | 0, _, _, _, _, _, h, _ => by simp [callF] at h
  | n + 1, Γ, f, args, exp, t, h, hb => by
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
      · exact SoundT.unsupported
      rename_i hg
      have hbh : boundsHold Γ sig.tparams θ = true := by simpa using h92
      have had : allDecided sig.tparams θ = true := by simpa using h101
      simp only [hbh, had, Bool.and_self, Bool.not_true, Bool.false_eq_true, ↓reduceIte]
      refine SoundT.bind' (evalArgs_sound n Γ args _ ha hb) (fun vals ⟨hl, hv, hks⟩ => ?_)
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
      refine SoundT.bind' (SoundT.here (hcall f sig θ (vals.map (·.1)) (hb.2 ▸ hs) (by simpa using hg) hbd
        ⟨by simp [hl, hlen'], zip_inst.mp hv⟩).sound) (fun ⟨r, after⟩ ⟨hr1, _, hr2⟩ => ?_)
      refine SoundT.get' (fun ρ hρ => ?_)
      refine SoundT.bind' (SoundT.here (writeBack_sound n Γ ρ args _ (vals.map (·.2)) after ha hb.1 hρ hks
        (zip_inst.mpr hr2))) (fun ρ' hρ' => ?_)
      exact SoundT.bind' (SoundT.set' hρ') (fun _ _ => SoundT.pure' hr1)

theorem argByValue_sound {V : List (String × Ty × Kind)} {R : Ret} {mode : Mode} {t : Ty} {v : EvalT Val}
    (h : SoundT V R (HasType · t) v) :
    SoundT V R (fun a : Option Val × List Val => (∀ w, a.1 = some w → HasType w t) ∧ a.2 = [])
      (argByValue mode v) := by
  simp only [argByValue]
  split
  · exact SoundT.bind' (P := fun o : Option Val => o = none) (SoundT.pure' rfl)
      (fun o ho => SoundT.pure' ⟨fun w e => (by subst ho; cases e), rfl⟩)
  · exact SoundT.bind' (SoundT.map' (Q := fun o : Option Val => ∀ w, o = some w → HasType w t) h
      (fun w hw w' e => by cases e; exact hw)) (fun o ho => SoundT.pure' ⟨ho, rfl⟩)

theorem placeArg_sound {V : List (String × Ty × Kind)} {R : Ret} {mode : Mode} {t : Ty}
    {m : EvalT (Option Val × List Val)} {P : Option Val × List Val → Prop} {Q : List Val → Prop}
    (h : SoundT V R P m) (hq : ∀ o kvs, P (o, kvs) → (∀ v, o = some v → HasType v t) ∧ Q kvs) :
    SoundT V R (fun a : Option Val × List Val => (∀ w, a.1 = some w → HasType w t) ∧ Q a.2)
      (placeArg mode m) := by
  simp only [placeArg]
  refine SoundT.bind' h (fun ⟨o, kvs⟩ hp => ?_)
  obtain ⟨ho, hk⟩ := hq o kvs hp
  split
  · exact SoundT.pure' ⟨(fun w e => by cases e), hk⟩
  · cases o with
    | none => exact SoundT.uninit
    | some v => exact SoundT.pure' ⟨(fun w e => by cases e; exact ho v rfl), hk⟩

/-- A call's checked arguments evaluate to values of their parameters'
types, none for an `out` one, with what a `mut` or `out` one into a part of a
variable is written back by (`ArgsKeys`). -/
theorem evalArgs_sound : ∀ (n : Nat) (Γ : Ctx) (args : List (Option Mode × Expr))
    (params : List (Mode × Ty)), Quiet (argsF env n Γ args params) () → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun vs => vs.length = min args.length params.length ∧
      (∀ q ∈ (vs.map (·.1)).zip params, ∀ v, q.1 = some v → HasType v q.2.2) ∧
      ArgsKeys Γ args params (vs.map (·.2))) (evalArgs env call n Γ args params)
  | 0, _, _, _, h, _ => by simp [argsF] at h
  | n + 1, Γ, (mk, e) :: args, (mode, t) :: params, h, hb => by
    simp only [argsF] at h
    obtain ⟨⟨⟩, hc, h⟩ := h.bind_inv
    rw [evalArgs.eq_def]
    dsimp only
    have rest : ∀ a : Option Val × List Val, (∀ v, a.1 = some v → HasType v t) → ArgKeys Γ mode t e a.2 →
        SoundT Γ.vars Γ.ret (fun vs => vs.length = min ((mk, e) :: args).length ((mode, t) :: params).length ∧
          (∀ q ∈ (vs.map (·.1)).zip ((mode, t) :: params), ∀ v, q.1 = some v → HasType v q.2.2) ∧
          ArgsKeys Γ ((mk, e) :: args) ((mode, t) :: params) (vs.map (·.2)))
          (evalArgs env call n Γ args params >>= fun vs => pure (a :: vs)) := by
      intro a ho hk
      refine SoundT.bind' (evalArgs_sound n Γ args params h hb) (fun vs ⟨hl, hvs, hks⟩ =>
        SoundT.pure' ⟨by simp [hl, Nat.succ_min_succ], fun q hq v hv => ?_, ⟨hk, hks⟩⟩)
      simp only [List.map_cons, List.zip_cons_cons, List.mem_cons] at hq
      rcases hq with rfl | hq
      · exact ho v hv
      · exact hvs q hq v hv
    have bindRest : ∀ x : EvalT (Option Val × List Val), SoundT Γ.vars Γ.ret (fun a : Option Val × List Val =>
        (∀ v, a.1 = some v → HasType v t) ∧ ArgKeys Γ mode t e a.2) x →
        SoundT Γ.vars Γ.ret (fun vs => vs.length = min ((mk, e) :: args).length ((mode, t) :: params).length ∧
          (∀ q ∈ (vs.map (·.1)).zip ((mode, t) :: params), ∀ v, q.1 = some v → HasType v q.2.2) ∧
          ArgsKeys Γ ((mk, e) :: args) ((mode, t) :: params) (vs.map (·.2)))
          (x >>= fun a => evalArgs env call n Γ args params >>= fun vs => pure (a :: vs)) :=
      fun x hx => SoundT.bind' hx (fun a ⟨ho, hk⟩ => rest a ho hk)
    have byValue : (placeOf e = none ∨ (mode == .mut || mode == .out) = false) →
        SoundT Γ.vars Γ.ret (fun a : Option Val × List Val =>
          (∀ v, a.1 = some v → HasType v t) ∧ ArgKeys Γ mode t e a.2)
          (argByValue mode (evalC env call n (Γ.forArg mode) e t)) := fun hw =>
      SoundT.weaken (argByValue_sound (evalC_sound n _ e t hc hb)) (fun a ⟨h1, h2⟩ =>
        ⟨h1, h2 ▸ ArgKeys.nil hw⟩)
    split
    · rename_i q hq
      split
      · rename_i hmode
        refine bindRest _ ?_
        obtain ⟨x, steps⟩ := q
        refine placeArg_sound (placeC_sound n _ e t x steps hc hq hb) (fun o kvs hpo => ?_)
        obtain ⟨ho, -, u, k, u', hl, hpath, hu⟩ := hpo
        exact ⟨ho, by unfold ArgKeys; rw [hq]; exact fun _ => ⟨u, k, u', hl, hpath, hu⟩⟩
      · rename_i hmode
        exact bindRest _ (byValue (.inr (by simpa using hmode)))
    · rename_i hq
      exact bindRest _ (byValue (.inl hq))
  | n + 1, Γ, [], _, _, _ => by simp only [evalArgs]; exact SoundT.pure' ⟨by simp, by simp, trivial⟩
  | n + 1, Γ, _ :: _, [], _, _ => by simp only [evalArgs]; exact SoundT.pure' ⟨by simp, by simp, trivial⟩

theorem evalPair_sound : ∀ (n : Nat) (Γ : Ctx) (a b : Expr) (exp : Option Ty) (t : Ty),
    Quiet (pairF env n Γ a b exp) t → Γ.Runs sigs →
    SoundT Γ.vars Γ.ret (fun r => r.1 = t ∧ HasType r.2.1 t ∧ HasType r.2.2 t) (evalPair env call n Γ a b exp)
  | 0, _, _, _, _, _, h, _ => by simp [pairF] at h
  | n + 1, Γ, a, b, exp, t, h, hb => by
    simp only [evalPair, typeOf_quiet h]
    simp only [pairF] at h
    obtain ⟨_, -, h⟩ := h.bind_inv
    obtain ⟨_, -, h⟩ := h.bind_inv
    split at h
    · simp at h
    obtain ⟨_, ha, h⟩ := h.bind_inv
    obtain ⟨_, hb', h⟩ := h.bind_inv
    rw [Quiet.pure_iff] at h; subst h
    refine SoundT.bind' (P := (· = _)) (SoundT.here rfl) (fun t' ht => ?_)
    subst ht
    exact SoundT.bind' (evalC_sound n _ a _ ha ⟨by simpa using hb.1, by simpa using hb.2⟩) (fun va hva =>
      SoundT.bind' (evalC_sound n _ b _ hb' ⟨by simpa using hb.1, by simpa using hb.2⟩) (fun vb hvb =>
        SoundT.pure' ⟨rfl, hva, hvb⟩))

end

end

end Datalove.Fun
