import Datalove.Proofs.Instance
import Datalove.Proofs.EarlyReturn

/-!
# Checking a generic once is enough: the checker

The instance of a context at type arguments `θ` (`Ctx.inst`) has every type
in it substituted, every alias too, a type parameter's included, so that a
hint naming one converts to its argument, and no bounds: an instance is not
generic. This file shows that an expression that checks quietly under a
generic context checks quietly under its instance, at the substituted type.
-/

namespace Datalove.Fun

open Datalove.Lit (Code unknownCode tysList fieldsList)

/-! ## Instances of contexts -/

def Ret.inst (θ : List (String × Ty)) : Ret → Ret
  | .void => .void
  | .ty t => .ty (t.inst θ)

/-- A context with type arguments `θ` substituted throughout. -/
def Ctx.inst (θ : List (String × Ty)) (Γ : Ctx) : Ctx :=
  { Γ with
    vars := Γ.vars.map fun (x, t, k) => (x, t.inst θ, k)
    ret := Γ.ret.inst θ
    aliases := Γ.aliases.inst θ
    bounds := [] }

/-- What a bound admits, in a context with no type parameters. -/
def Bound.admitsTy : Bound → Ty → Bool
  | .ord, _ => true
  | .float, t => t.isFloat
  | .fixedint, t => t.isFixedInt

/-- Type arguments the context's bounds admit, one for each bounded type
parameter. -/
def Respects (θ : List (String × Ty)) (Γ : Ctx) : Prop :=
  ∀ n b, Γ.bounds.lookup n = some b → ∃ t, θ.lookup n = some t ∧ b.admitsTy t = true

@[simp] theorem Ctx.inst_enter (θ : List (String × Ty)) (Γ : Ctx) (e : Expr) :
    (Γ.enter e).inst θ = (Γ.inst θ).enter e := rfl
@[simp] theorem Ctx.inst_borrow (θ : List (String × Ty)) (Γ : Ctx) :
    Γ.borrow.inst θ = (Γ.inst θ).borrow := rfl
@[simp] theorem Ctx.inst_through (θ : List (String × Ty)) (Γ : Ctx) :
    Γ.through.inst θ = (Γ.inst θ).through := rfl
@[simp] theorem Ctx.inst_forArg (θ : List (String × Ty)) (Γ : Ctx) (m : Mode) :
    (Γ.forArg m).inst θ = (Γ.inst θ).forArg m := rfl
@[simp] theorem Ctx.inst_bind (θ : List (String × Ty)) (Γ : Ctx) (x : String) (t : Ty) (k : Kind) :
    (Γ.bind x t k).inst θ = (Γ.inst θ).bind x (t.inst θ) k := rfl
@[simp] theorem Ctx.inst_sigs (θ : List (String × Ty)) (Γ : Ctx) : (Γ.inst θ).sigs = Γ.sigs := rfl
@[simp] theorem Ctx.inst_borrowed (θ : List (String × Ty)) (Γ : Ctx) :
    (Γ.inst θ).borrowed = Γ.borrowed := rfl
@[simp] theorem Ctx.inst_mutArg (θ : List (String × Ty)) (Γ : Ctx) :
    (Γ.inst θ).mutArg = Γ.mutArg := rfl
@[simp] theorem Ctx.inst_inLoop (θ : List (String × Ty)) (Γ : Ctx) :
    (Γ.inst θ).inLoop = Γ.inLoop := rfl
@[simp] theorem Ctx.inst_ret (θ : List (String × Ty)) (Γ : Ctx) :
    (Γ.inst θ).ret = Γ.ret.inst θ := rfl

@[simp] theorem Respects.enter {θ : List (String × Ty)} {Γ : Ctx} {e : Expr} :
    Respects θ (Γ.enter e) ↔ Respects θ Γ := Iff.rfl
@[simp] theorem Respects.borrow {θ : List (String × Ty)} {Γ : Ctx} :
    Respects θ Γ.borrow ↔ Respects θ Γ := Iff.rfl
@[simp] theorem Respects.through {θ : List (String × Ty)} {Γ : Ctx} :
    Respects θ Γ.through ↔ Respects θ Γ := Iff.rfl
@[simp] theorem Respects.forArg {θ : List (String × Ty)} {Γ : Ctx} {m : Mode} :
    Respects θ (Γ.forArg m) ↔ Respects θ Γ := Iff.rfl
@[simp] theorem Respects.bind {θ : List (String × Ty)} {Γ : Ctx} {x : String} {t : Ty} {k : Kind} :
    Respects θ (Γ.bind x t k) ↔ Respects θ Γ := Iff.rfl

theorem Ctx.lookup_inst (θ : List (String × Ty)) (Γ : Ctx) (x : String) :
    (Γ.inst θ).lookup x = (Γ.lookup x).map fun (t, k) => (t.inst θ, k) := by
  simp only [Ctx.lookup, Ctx.inst]
  induction Γ.vars with
  | nil => rfl
  | cons v vs ih =>
    obtain ⟨y, t, k⟩ := v
    by_cases e : y = x
    · subst e; simp [List.find?]
    · have hb : (y == x) = false := beq_false_of_ne e
      simp only [List.map, List.find?, hb] at ih ⊢
      exact ih

theorem Ctx.hint_inst {θ : List (String × Ty)} (g : Ground θ) {Γ : Ctx} {h : Hint} {t : Ty}
    (e : Γ.hint h = .ok t) : (Γ.inst θ).hint h = .ok (t.inst θ) := by
  simp only [Ctx.hint, convertIn, Ctx.inst] at e ⊢
  cases hc : h.convertWith Γ.aliases with
  | error _ => simp [hc] at e
  | ok u =>
    simp only [hc, Except.ok.injEq] at e
    subst e
    simp [Hint.convertWith_inst g hc]

/-! ## Quiet runs -/

/-- A check that logs nothing and gives `a`. -/
def Quiet {α} (m : ChkM α) (a : α) : Prop := m.run = ⟨#[], .ok a⟩

theorem Quiet.bind_inv {α β} {x : ChkM α} {f : α → ChkM β} {b : β} (h : Quiet (x >>= f) b) :
    ∃ a, Quiet x a ∧ Quiet (f a) b := by
  unfold Quiet at *
  rw [run_bind] at h
  rcases hx : x.run with ⟨l, v⟩
  rw [hx] at h
  cases v with
  | error c => simp at h
  | ok a =>
    simp at h
    obtain ⟨hl, hv⟩ := h
    refine ⟨a, by rw [hl.1], ?_⟩
    rcases hf : (f a).run with ⟨l', v'⟩
    rw [hf] at hl hv
    simp at hl hv
    simp [hl.2, hv]

theorem Quiet.bind {α β} {x : ChkM α} {f : α → ChkM β} {a : α} {b : β} (hx : Quiet x a)
    (hf : Quiet (f a) b) : Quiet (x >>= f) b := by
  unfold Quiet at *
  rw [run_bind, hx]
  simp [hf]

theorem Quiet.pure {α} (a : α) : Quiet (Pure.pure a : ChkM α) a := rfl

@[simp] theorem Quiet.pure_iff {α} {a b : α} : Quiet (Pure.pure a : ChkM α) b ↔ a = b := by
  simp [Quiet, Pure.pure, ExceptT.pure, ExceptT.mk, ExceptT.run]

@[simp] theorem Quiet.throw_iff {α} {c : Code} {a : α} : Quiet (throw c : ChkM α) a ↔ False := by
  simp [Quiet, throw, throwThe, MonadExceptOf.throw, ExceptT.mk, ExceptT.run]
  intro h; cases h

@[simp] theorem Quiet.throw_bind_iff {α β} {c : Code} {f : α → ChkM β} {b : β} :
    Quiet (throw c >>= f) b ↔ False := by
  constructor
  · intro h; obtain ⟨_, h, -⟩ := h.bind_inv; simp at h
  · intro h; exact h.elim

@[simp] theorem Quiet.report_iff {c : Code} {u : Unit} : Quiet (report c) u ↔ False := by
  simp [Quiet, report, ExceptT.lift, ExceptT.mk, ExceptT.run, Functor.map]

theorem Quiet.liftE_iff {α} {x : Except Code α} {a : α} : Quiet (liftE x) a ↔ x = .ok a := by
  cases x <;> simp [liftE]

theorem Quiet.recover_iff {m : ChkM Unit} {u : Unit} : Quiet (recover m) u ↔ Quiet m () := by
  unfold Quiet recover
  rcases hm : m.run with ⟨l, v⟩
  cases v <;> simp [ExceptT.mk, ExceptT.run] at * <;> rfl

theorem Quiet.unique {α} {m : ChkM α} {a b : α} (ha : Quiet m a) (hb : Quiet m b) : a = b := by
  unfold Quiet at *; rw [ha] at hb; simpa using hb

/-! ## The rules' side conditions hold of the instance -/

section
variable {θ : List (String × Ty)}

theorem Ctx.returnsOption_inst (g : Ground θ) {Γ : Ctx} (h : Γ.returnsOption = true) :
    (Γ.inst θ).returnsOption = true := by
  simp only [Ctx.returnsOption, Ctx.inst] at h ⊢
  cases hr : Γ.ret with
  | void => simp [hr] at h
  | ty t =>
    simp only [hr, Ret.inst, Ty.expose_inst g] at h ⊢
    cases he : t.expose <;> simp_all [Ty.inst]

theorem Ctx.returnsResult_inst (g : Ground θ) {Γ : Ctx} (h : Γ.returnsResult = true) :
    (Γ.inst θ).returnsResult = true := by
  simp only [Ctx.returnsResult, Ctx.inst] at h ⊢
  cases hr : Γ.ret with
  | void => simp [hr] at h
  | ty t =>
    simp only [hr, Ret.inst, Ty.expose_inst g] at h ⊢
    cases he : t.expose <;> simp_all [Ty.inst]

theorem Ctx.fitsMode_inst (g : Ground θ) {Γ : Ctx} {m : IdxMode} (h : Γ.fitsMode m = true) :
    (Γ.inst θ).fitsMode m = true := by
  cases m
  · exact Ctx.returnsOption_inst g h
  · exact Ctx.returnsResult_inst g h

theorem fits_inst (g : Ground θ) {Γ : Ctx} {k : Needs} (h : fits Γ k = true) :
    fits (Γ.inst θ) k = true := by
  cases k
  · rfl
  · exact Ctx.returnsResult_inst g h
  · exact Ctx.returnsOption_inst g h

@[simp] theorem Ctx.boundOf_inst (Γ : Ctx) (t : Ty) : (Γ.inst θ).boundOf t = none := by
  cases t <;> simp [Ctx.boundOf, Ctx.inst]

theorem Respects.boundOf {Γ : Ctx} (r : Respects θ Γ) {t : Ty} {b : Bound}
    (h : Γ.boundOf t = some b) : b.admitsTy (t.inst θ) = true := by
  cases t <;> simp [Ctx.boundOf] at h
  rename_i n
  obtain ⟨u, hu, ha⟩ := r n b h
  simpa [Ty.inst, hu] using ha

theorem Bound.admitsTy_float {t : Ty} (h : Bound.float.admitsTy t = true) : t.isFloat = true := h
theorem Bound.admitsTy_fixedint {t : Ty} (h : Bound.fixedint.admitsTy t = true) :
    t.isFixedInt = true := h

theorem Ty.isFloat_numeric {t : Ty} (h : t.isFloat = true) : t.isNumeric = true := by
  cases t <;> simp_all [Ty.isFloat, Ty.isNumeric]

theorem Ty.isFixedInt_numeric {t : Ty} (h : t.isFixedInt = true) : t.isNumeric = true := by
  cases t <;> simp_all [Ty.isFixedInt, Ty.isNumeric]

/-- A parameter bounded to the floats or the fixed-width integers is
numeric in every instance. -/
theorem Respects.numeric {Γ : Ctx} (r : Respects θ Γ) {t : Ty}
    (h : (Γ.boundOf t == some .float || Γ.boundOf t == some .fixedint) = true) :
    (t.inst θ).isNumeric = true := by
  simp only [Bool.or_eq_true, beq_iff_eq] at h
  rcases h with h | h
  · exact Ty.isFloat_numeric (r.boundOf h)
  · exact Ty.isFixedInt_numeric (r.boundOf h)

/-- `arithAdmits` by the classes it reads: float, fixed-width, `int`. -/
def arithFlags : BinOp → Bool → Bool → Bool → Bool
  | .add, f, _, i | .sub, f, _, i | .mul, f, _, i => f || i
  | .div, f, _, _ => f
  | .addC, _, x, _ | .subC, _, x, _ | .mulC, _, x, _ | .addO, _, x, _ | .subO, _, x, _
  | .mulO, _, x, _ => x
  | .divC, _, x, i | .divO, _, x, i => x || i
  | _, _, _, _ => false

theorem arithAdmits_flags (op : BinOp) (t : Ty) (b : Option Bound) :
    arithAdmits op t b = arithFlags op (b == some .float || isFloatT t)
      (b == some .fixedint || isFixT t) (t == .int) := by
  cases op <;> rfl

theorem arithFlags_mono {op : BinOp} {f x i f' x' i' : Bool} (h : arithFlags op f x i = true)
    (hf : f = true → f' = true) (hx : x = true → x' = true) (hi : i = true → i' = true) :
    arithFlags op f' x' i' = true := by
  cases op <;> cases f <;> cases x <;> cases i <;> simp_all [arithFlags]

theorem arithAdmits_inst {Γ : Ctx} (r : Respects θ Γ) {op : BinOp} {t : Ty}
    (h : arithAdmits op t (Γ.boundOf t) = true) :
    arithAdmits op (t.inst θ) ((Γ.inst θ).boundOf (t.inst θ)) = true := by
  rw [Ctx.boundOf_inst]
  have hf : (Γ.boundOf t == some .float || isFloatT t) = true → isFloatT (t.inst θ) = true := by
    intro hf
    simp only [Bool.or_eq_true, beq_iff_eq] at hf
    rcases hf with hf | hf
    · exact r.boundOf hf
    · simp only [isFloatT] at hf ⊢; rw [Ty.inst_isFloat θ hf]; exact hf
  have hx : (Γ.boundOf t == some .fixedint || isFixT t) = true → isFixT (t.inst θ) = true := by
    intro hx
    simp only [Bool.or_eq_true, beq_iff_eq] at hx
    rcases hx with hx | hx
    · exact r.boundOf hx
    · simp only [isFixT] at hx ⊢; rw [Ty.inst_isFixedInt θ hx]; exact hx
  have hi : (t == .int) = true → (t.inst θ == .int) = true := by
    intro hi; simp only [beq_iff_eq] at hi; subst hi; rfl
  rw [arithAdmits_flags] at h ⊢
  exact arithFlags_mono h (hf · ) (by simpa using hx ·) hi

theorem finishBinop_inst (g : Ground θ) {Γ : Ctx} (r : Respects θ Γ) {op : BinOp} {t u : Ty}
    (h : Quiet (finishBinop Γ op t) u) : Quiet (finishBinop (Γ.inst θ) op (t.inst θ)) (u.inst θ) := by
  have bnd : ∀ c : Bool, ((Γ.boundOf t == some .float || Γ.boundOf t == some .fixedint) || c) = true →
      (c = true → (t.inst θ).isNumeric = true) → (t.inst θ).isNumeric = true := by
    intro c hc hn
    simp only [Bool.or_eq_true] at hc
    rcases hc with hc | hc
    · exact r.numeric (by simpa using hc)
    · exact hn hc
  cases op <;> simp only [finishBinop] at h ⊢
  -- The comparisons.
  case lt | gt | le | ge =>
    split at h
    · rename_i hc
      rw [Quiet.pure_iff] at h; subst h
      have hn : (t.inst θ).isNumeric = true :=
        bnd _ hc (fun hn => by rw [Ty.inst_isNumeric θ hn]; exact hn)
      simp [Ctx.boundOf_inst, hn, Ty.inst]
    · simp at h
  case eq | ne =>
    split at h
    · rename_i hc
      rw [Quiet.pure_iff] at h; subst h
      have he : isEqT (t.inst θ) = true := by
        simp only [Bool.or_eq_true] at hc
        rcases hc with hc | hc
        · have := r.numeric (by simpa using hc)
          cases ht : t.inst θ <;> simp_all [isEqT, Ty.isNumeric]
        · exact isEqT_inst θ t hc
      simp [Ctx.boundOf_inst, he, Ty.inst]
    · simp at h
  case and | or | xor =>
    split at h
    · rename_i hc
      rw [Quiet.pure_iff] at h; subst h
      simp only [beq_iff_eq] at hc; subst hc
      simp [Ty.inst]
    · simp at h
  all_goals
    split at h
    · simp at h
    · rename_i ha
      split at h
      · simp at h
      · rename_i hfit
        rw [Quiet.pure_iff] at h; subst h
        have ha' := arithAdmits_inst r (by simpa using ha)
        have hf' := fits_inst g (Γ := Γ) (by simpa using hfit)
        simp only [ha', hf', Bool.not_true, Bool.false_eq_true, ↓reduceIte]
        exact Quiet.pure _

end

/-! ## Type arguments compose -/

mutual

/-- Whether every type parameter in a type satisfies `P`. -/
def _root_.Datalove.Ty.allVars (P : String → Bool) : Ty → Bool
  | .var n => P n
  | .mu b => b.allVars P
  | .tuple ts => ts.allVars P
  | .struct fs | .table fs => fs.allVars P
  | .list t | .set t | .option t | .result t | .tensor t _ | .term _ t => t.allVars P
  | .map k v => k.allVars P && v.allVars P
  | .enum vs => vs.allVars P
  | _ => true

def _root_.Datalove.Tys.allVars (P : String → Bool) : Tys → Bool
  | .nil => true
  | .cons t ts => t.allVars P && ts.allVars P

def _root_.Datalove.Fields.allVars (P : String → Bool) : Fields → Bool
  | .nil => true
  | .cons _ t fs => t.allVars P && fs.allVars P

def _root_.Datalove.Variants.allVars (P : String → Bool) : Variants → Bool
  | .nil => true
  | .atom _ vs => vs.allVars P
  | .term _ t vs => t.allVars P && vs.allVars P

end

/-- Type arguments with others substituted into them. -/
def instArgs (θ θ' : List (String × Ty)) : List (String × Ty) :=
  θ'.map fun (p, u) => (p, u.inst θ)

theorem lookup_instArgs (θ θ' : List (String × Ty)) (n : String) :
    (instArgs θ θ').lookup n = (θ'.lookup n).map (·.inst θ) :=
  lookup_map_snd _ n θ'

mutual

theorem _root_.Datalove.Ty.inst_inst (θ θ' : List (String × Ty)) :
    ∀ (t : Ty), t.allVars (fun n => (θ'.lookup n).isSome) = true →
      (t.inst θ').inst θ = t.inst (instArgs θ θ')
  | .var n, h => by
    simp only [Ty.allVars] at h
    simp only [Ty.inst, lookup_instArgs]
    cases hl : θ'.lookup n <;> simp_all
  | .mu b, h | .list b, h | .set b, h | .option b, h | .result b, h | .tensor b _, h
  | .term _ b, h => by
    simp only [Ty.allVars] at h; simp [Ty.inst, Ty.inst_inst θ θ' b h]
  | .map k v, h => by
    simp only [Ty.allVars, Bool.and_eq_true] at h
    simp [Ty.inst, Ty.inst_inst θ θ' k h.1, Ty.inst_inst θ θ' v h.2]
  | .tuple ts, h => by simp only [Ty.allVars] at h; simp [Ty.inst, Tys.inst_inst θ θ' ts h]
  | .struct fs, h => by simp only [Ty.allVars] at h; simp [Ty.inst, Fields.inst_inst θ θ' fs h]
  | .table fs, h => by simp only [Ty.allVars] at h; simp [Ty.inst, Fields.inst_inst θ θ' fs h]
  | .enum vs, h => by simp only [Ty.allVars] at h; simp [Ty.inst, Variants.inst_inst θ θ' vs h]
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .data, _ | .error, _
  | .atom _, _ | .recvar _, _ => by simp [Ty.inst]

theorem _root_.Datalove.Tys.inst_inst (θ θ' : List (String × Ty)) :
    ∀ (ts : Tys), ts.allVars (fun n => (θ'.lookup n).isSome) = true →
      (ts.inst θ').inst θ = ts.inst (instArgs θ θ')
  | .nil, _ => by simp [Tys.inst]
  | .cons t ts, h => by
    simp only [Tys.allVars, Bool.and_eq_true] at h
    simp [Tys.inst, Ty.inst_inst θ θ' t h.1, Tys.inst_inst θ θ' ts h.2]

theorem _root_.Datalove.Fields.inst_inst (θ θ' : List (String × Ty)) :
    ∀ (fs : Fields), fs.allVars (fun n => (θ'.lookup n).isSome) = true →
      (fs.inst θ').inst θ = fs.inst (instArgs θ θ')
  | .nil, _ => by simp [Fields.inst]
  | .cons _ t fs, h => by
    simp only [Fields.allVars, Bool.and_eq_true] at h
    simp [Fields.inst, Ty.inst_inst θ θ' t h.1, Fields.inst_inst θ θ' fs h.2]

theorem _root_.Datalove.Variants.inst_inst (θ θ' : List (String × Ty)) :
    ∀ (vs : Variants), vs.allVars (fun n => (θ'.lookup n).isSome) = true →
      (vs.inst θ').inst θ = vs.inst (instArgs θ θ')
  | .nil, _ => by simp [Variants.inst]
  | .atom _ vs, h => by
    simp only [Variants.allVars] at h; simp [Variants.inst, Variants.inst_inst θ θ' vs h]
  | .term _ t vs, h => by
    simp only [Variants.allVars, Bool.and_eq_true] at h
    simp [Variants.inst, Ty.inst_inst θ θ' t h.1, Variants.inst_inst θ θ' vs h.2]

end

mutual

theorem _root_.Datalove.Ty.allVars_mono {P Q : String → Bool} (hPQ : ∀ n, P n = true → Q n = true) :
    ∀ (t : Ty), t.allVars P = true → t.allVars Q = true
  | .var n, h => by simp only [Ty.allVars] at h ⊢; exact hPQ n h
  | .mu b, h | .list b, h | .set b, h | .option b, h | .result b, h | .tensor b _, h
  | .term _ b, h => by simp only [Ty.allVars] at h ⊢; exact Ty.allVars_mono hPQ b h
  | .map k v, h => by
    simp only [Ty.allVars, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.allVars_mono hPQ k h.1, Ty.allVars_mono hPQ v h.2⟩
  | .tuple ts, h => by simp only [Ty.allVars] at h ⊢; exact Tys.allVars_mono hPQ ts h
  | .struct fs, h | .table fs, h => by
    simp only [Ty.allVars] at h ⊢; exact Fields.allVars_mono hPQ fs h
  | .enum vs, h => by simp only [Ty.allVars] at h ⊢; exact Variants.allVars_mono hPQ vs h
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .data, _ | .error, _
  | .atom _, _ | .recvar _, _ => by simp [Ty.allVars]

theorem _root_.Datalove.Tys.allVars_mono {P Q : String → Bool} (hPQ : ∀ n, P n = true → Q n = true) :
    ∀ (ts : Tys), ts.allVars P = true → ts.allVars Q = true
  | .nil, _ => by simp [Tys.allVars]
  | .cons t ts, h => by
    simp only [Tys.allVars, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.allVars_mono hPQ t h.1, Tys.allVars_mono hPQ ts h.2⟩

theorem _root_.Datalove.Fields.allVars_mono {P Q : String → Bool} (hPQ : ∀ n, P n = true → Q n = true) :
    ∀ (fs : Fields), fs.allVars P = true → fs.allVars Q = true
  | .nil, _ => by simp [Fields.allVars]
  | .cons _ t fs, h => by
    simp only [Fields.allVars, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.allVars_mono hPQ t h.1, Fields.allVars_mono hPQ fs h.2⟩

theorem _root_.Datalove.Variants.allVars_mono {P Q : String → Bool}
    (hPQ : ∀ n, P n = true → Q n = true) :
    ∀ (vs : Variants), vs.allVars P = true → vs.allVars Q = true
  | .nil, _ => by simp [Variants.allVars]
  | .atom _ vs, h => by simp only [Variants.allVars] at h ⊢; exact Variants.allVars_mono hPQ vs h
  | .term _ t vs, h => by
    simp only [Variants.allVars, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.allVars_mono hPQ t h.1, Variants.allVars_mono hPQ vs h.2⟩

end

/-- Signatures whose types name only their own type parameters. -/
def SigsOk (sigs : List (String × Sig)) : Prop :=
  ∀ f sig, sigs.lookup f = some sig →
    let own := fun n => sig.tparams.any (·.1 == n)
    (∀ mt ∈ sig.params, mt.2.allVars own = true) ∧
    (∀ r, sig.ret = .ty r → r.allVars own = true)

theorem allDecided_covers {tps : List (String × Option Bound)} {θ' : List (String × Ty)}
    (h : allDecided tps θ' = true) (n : String) (hn : tps.any (·.1 == n) = true) :
    (θ'.lookup n).isSome = true := by
  simp only [allDecided, List.all_eq_true] at h
  simp only [List.any_eq_true, beq_iff_eq] at hn
  obtain ⟨⟨p, b⟩, hm, rfl⟩ := hn
  exact h _ hm

theorem decided_inst (θ : List (String × Ty)) : ∀ (l : List (String × Except Code Ty)) {θ'},
    decided l = .ok θ' →
      decided (l.map fun (p, r) => (p, r.map (·.inst θ))) = .ok (instArgs θ θ')
  | [], θ', h => by simp [decided, pure, Except.pure] at h; subst h; rfl
  | (p, .ok t) :: rest, θ', h => by
    simp only [decided] at h
    cases hr : decided rest with
    | error c => simp [hr, Functor.map, Except.map] at h
    | ok θr =>
      simp [hr, Functor.map, Except.map] at h; subst h
      have ih := decided_inst θ rest hr
      simp only [List.map_cons]
      show decided ((p, Except.ok (t.inst θ)) :: _) = _
      simp only [decided, ih]
      rfl
  | (p, .error c) :: rest, θ', h => by
    simp only [decided] at h
    split at h
    · rename_i hc
      have ih := decided_inst θ rest h
      simp only [List.map_cons]
      show decided ((p, Except.error c) :: _) = _
      simp only [decided, hc, ↓reduceIte, ih]
    · simp [throw, throwThe, MonadExceptOf.throw] at h

theorem Ctx.admits_inst {θ : List (String × Ty)} (g : Ground θ) {Γ : Ctx} (r : Respects θ Γ)
    {b : Bound} {t : Ty} (h : Γ.admits b t = true) : (Γ.inst θ).admits b (t.inst θ) = true := by
  cases t
  case var n =>
    simp only [Ctx.admits, beq_iff_eq] at h
    obtain ⟨u, hu, ha⟩ := r n b h
    have hv := (g.lookup hu).1
    simp only [Ty.inst, hu, Option.getD_some]
    cases u <;> cases b <;> simp_all [Ctx.admits, Bound.admitsTy, Ty.containsVar]
  all_goals (cases b <;> simp_all [Ctx.admits, Ty.inst, Ty.isFloat, Ty.isFixedInt])

theorem boundsHold_inst {θ : List (String × Ty)} (g : Ground θ) {Γ : Ctx} (r : Respects θ Γ)
    {tps : List (String × Option Bound)} {θ' : List (String × Ty)}
    (h : boundsHold Γ tps θ' = true) : boundsHold (Γ.inst θ) tps (instArgs θ θ') = true := by
  simp only [boundsHold, List.all_eq_true] at h ⊢
  intro ⟨p, b⟩ hm
  have := h _ hm
  simp only [lookup_instArgs] at this ⊢
  cases b <;> cases hl : θ'.lookup p <;> simp_all
  exact Ctx.admits_inst g r this

theorem allDecided_inst {θ : List (String × Ty)} {tps : List (String × Option Bound)}
    {θ' : List (String × Ty)} (h : allDecided tps θ' = true) :
    allDecided tps (instArgs θ θ') = true := by
  simp only [allDecided, List.all_eq_true, lookup_instArgs] at h ⊢
  intro x hx; simpa using h x hx

/-! ## Small facts the rules read -/

theorem intRange_inst (θ : List (String × Ty)) {t : Ty} {x : Int × Int × Code}
    (h : Lit.intRange t = some x) : t.inst θ = t := by
  cases t <;> simp_all [Lit.intRange, Ty.inst]

theorem intRange_inst_none {θ : List (String × Ty)} {Γ : Ctx} (r : Respects θ Γ)
    {t : Ty} (hf : Γ.boundOf t = some .float) : Lit.intRange (t.inst θ) = none := by
  have := r.boundOf hf
  cases ht : t.inst θ <;> simp_all [Bound.admitsTy, Ty.isFloat, Lit.intRange]

theorem hexBits_inst (θ : List (String × Ty)) {t : Ty} {x : Nat × Code}
    (h : Lit.hexBits t = some x) : t.inst θ = t := by
  cases t <;> simp_all [Lit.hexBits, Ty.inst]

theorem atomListed_inst (θ : List (String × Ty)) (a : String) : ∀ (vs : Variants),
    Lit.Variants.atomListed a (vs.inst θ) = Lit.Variants.atomListed a vs
  | .nil => rfl
  | .atom n vs => by simp [Variants.inst, Lit.Variants.atomListed, atomListed_inst θ a vs]
  | .term _ _ vs => by simp [Variants.inst, Lit.Variants.atomListed, atomListed_inst θ a vs]

theorem termPayload_inst (θ : List (String × Ty)) (a : String) : ∀ (vs : Variants),
    Lit.Variants.termPayload a (vs.inst θ) = (Lit.Variants.termPayload a vs).map (·.inst θ)
  | .nil => rfl
  | .atom _ vs => by simp [Variants.inst, Lit.Variants.termPayload, termPayload_inst θ a vs]
  | .term n t vs => by
    simp only [Variants.inst, Lit.Variants.termPayload]
    split <;> simp [termPayload_inst θ a vs]

theorem tysOf_inst (θ : List (String × Ty)) : ∀ (ts : List Ty),
    tysOf (ts.map (·.inst θ)) = (tysOf ts).inst θ
  | [] => rfl
  | t :: ts => by simp [tysOf, Tys.inst, tysOf_inst θ ts]

theorem fieldsOf_inst (θ : List (String × Ty)) : ∀ (fs : List (String × Ty)),
    fieldsOf (fs.map fun (n, t) => (n, t.inst θ)) = (fieldsOf fs).inst θ
  | [] => rfl
  | (n, t) :: fs => by simp [fieldsOf, Fields.inst, fieldsOf_inst θ fs]

theorem fieldsOf_zip_inst (θ : List (String × Ty)) : ∀ (ns : List String) (ts : List Ty),
    fieldsOf (ns.zip (ts.map (·.inst θ))) = (fieldsOf (ns.zip ts)).inst θ
  | [], _ => rfl
  | _ :: _, [] => rfl
  | n :: ns, t :: ts => by simp [fieldsOf, Fields.inst, fieldsOf_zip_inst θ ns ts]

@[simp] theorem Ctx.enter_lookup (Γ : Ctx) (e : Expr) (x : String) :
    (Γ.enter e).lookup x = Γ.lookup x := rfl
@[simp] theorem Ctx.borrow_lookup (Γ : Ctx) (x : String) : Γ.borrow.lookup x = Γ.lookup x := rfl
@[simp] theorem Ctx.enter_boundOf (Γ : Ctx) (e : Expr) (t : Ty) :
    (Γ.enter e).boundOf t = Γ.boundOf t := rfl
@[simp] theorem Ctx.borrow_boundOf (Γ : Ctx) (t : Ty) : Γ.borrow.boundOf t = Γ.boundOf t := rfl
@[simp] theorem Ctx.enter_hint (Γ : Ctx) (e : Expr) (h : Hint) : (Γ.enter e).hint h = Γ.hint h := rfl
@[simp] theorem Ctx.enter_sigs (Γ : Ctx) (e : Expr) : (Γ.enter e).sigs = Γ.sigs := rfl
@[simp] theorem Ctx.borrow_sigs (Γ : Ctx) : Γ.borrow.sigs = Γ.sigs := rfl
@[simp] theorem Ctx.through_sigs (Γ : Ctx) : Γ.through.sigs = Γ.sigs := rfl
@[simp] theorem Ctx.forArg_sigs (Γ : Ctx) (m : Mode) : (Γ.forArg m).sigs = Γ.sigs := rfl

theorem quiet_ite_true {α} {c : Bool} {x y : ChkM α} {a : α} (hc : c = true) (hx : Quiet x a) :
    Quiet (if c = true then x else y) a := by
  simp [hc, hx]

theorem quiet_ite_false {α} {c : Bool} {x y : ChkM α} {a : α} (hc : c = false) (hy : Quiet y a) :
    Quiet (if c = true then x else y) a := by
  simp [hc, hy]

/-- The forms `synthF` types by inference: datalit's literal forms. -/
def Expr.isLit : Expr → Bool
  | .hinted _ _ | .var _ | .binop _ _ _ | .unop _ _ | .tryOpt _ | .tryRes _ | .adapt _
  | .call _ _ | .place _ _ | .proj _ _ | .index _ _ _ => false
  | _ => true

/-- What the theorem asks of inference: under an instance it answers what it
answered under the generic, substituted, wherever the generic checked
quietly. Inference is a parameter of checking, as it is for the other
theorems; the real one is `Lit.lean`'s unifier, whose answers `fun_tests`
holds the Rust to. -/
structure InstEnv (θ : List (String × Ty)) (env env' : InferEnv) : Prop where
  ty : ∀ n Γ e t, Respects θ Γ → SigsOk Γ.sigs → e.isLit = true →
    Quiet (synthF env (n + 1) Γ e) t →
    env'.ty ((Γ.enter e).inst θ) e = (env.ty (Γ.enter e) e).map (·.inst θ)
  pair : ∀ n Γ a b exp t, Respects θ Γ → SigsOk Γ.sigs → Quiet (pairF env (n + 1) Γ a b exp) t →
    env'.pair (Γ.inst θ) a b (exp.map (·.inst θ)) =
      ((env.pair Γ a b exp).1.map (·.inst θ), (env.pair Γ a b exp).2.map (·.inst θ))
  call : ∀ n Γ f args exp t, Respects θ Γ → SigsOk Γ.sigs →
    Quiet (callF env (n + 1) Γ f args exp) t →
    env'.call (Γ.inst θ) f args (exp.map (·.inst θ)) =
      (env.call Γ f args exp).map fun (p, r) => (p, r.map (·.inst θ))

/-! ## The guards the rules share -/

section
variable {θ : List (String × Ty)}

/-- What `synthF` does with a literal form, with its binds explicit. -/
def litBody (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Expr) : ChkM Ty :=
  (match env.ty Γ e with
    | .ok t => pure t
    | .error c => if c == unknownCode then partsF env n Γ e else throw c) >>= fun t =>
  checkForm env n Γ e t >>= fun _ => pure t

theorem synthF_lit {env : InferEnv} {n : Nat} {Γ : Ctx} {e : Expr} (hl : e.isLit = true) :
    synthF env (n + 1) Γ e = litBody env n (Γ.enter e) e := by
  cases e <;> simp [Expr.isLit] at hl <;>
  · simp only [synthF, litBody]
    cases env.ty _ _ with
    | ok t => simp
    | error c => by_cases hc : (c == unknownCode) = true <;> simp [hc]

theorem copyOk_inst {t : Ty} {b : Bool} (h : ¬((!t.isCopy && !b) = true)) :
    (!(t.inst θ).isCopy && !b) = false := by
  cases hc : t.isCopy
  · cases b <;> simp_all
  · simp [Ty.isCopy_inst θ t hc]

theorem negOk_inst {Γ : Ctx} (r : Respects θ Γ) {u : Ty}
    (hc : (Γ.boundOf u == some .float || isFloatT u || u == .int) = true) :
    ((Γ.inst θ).boundOf (u.inst θ) == some .float || isFloatT (u.inst θ) || u.inst θ == .int) = true := by
  rw [Ctx.boundOf_inst]
  have : isFloatT (u.inst θ) = true ∨ u.inst θ = .int := by
    simp only [Bool.or_eq_true, beq_iff_eq] at hc
    rcases hc with (hc | hc) | hc
    · exact .inl (r.boundOf hc)
    · exact .inl (by simp only [isFloatT] at hc ⊢; rw [Ty.inst_isFloat θ hc]; exact hc)
    · subst hc; exact .inr rfl
  rcases this with h | h <;> simp [h]

theorem fixOk_inst {Γ : Ctx} (r : Respects θ Γ) {u : Ty}
    (hc : (Γ.boundOf u == some .fixedint || isFixT u) = true) :
    ((Γ.inst θ).boundOf (u.inst θ) == some .fixedint || isFixT (u.inst θ)) = true := by
  rw [Ctx.boundOf_inst]
  have : isFixT (u.inst θ) = true := by
    simp only [Bool.or_eq_true, beq_iff_eq] at hc
    rcases hc with hc | hc
    · exact r.boundOf hc
    · simp only [isFixT] at hc ⊢; rw [Ty.inst_isFixedInt θ hc]; exact hc
  simp [this]

end

theorem true_of_not_not {c : Bool} (h : ¬((!c) = true)) : c = true := by cases c <;> simp_all

theorem not_false_of {c : Bool} (h : c = true) : (!c) = false := by simp [h]

theorem map_eq_bind {α β} (f : α → β) (x : ChkM α) : f <$> x = x >>= fun a => pure (f a) := rfl

theorem isFloatT_inst_nonvar (θ : List (String × Ty)) {t : Ty} (hv : ∀ m, t ≠ .var m) :
    isFloatT (t.inst θ) = isFloatT t := by
  cases t <;> simp_all [Ty.inst, isFloatT, Ty.isFloat]

theorem intRange_inst_nonvar (θ : List (String × Ty)) {t : Ty} (hv : ∀ m, t ≠ .var m) :
    Lit.intRange (t.inst θ) = Lit.intRange t := by
  cases t <;> simp_all [Ty.inst, Lit.intRange]

theorem boundOf_nonvar {Γ : Ctx} {t : Ty} (hv : ∀ m, t ≠ .var m) : Γ.boundOf t = none := by
  cases t <;> simp_all [Ctx.boundOf]

theorem Ty.expose_var_iff {s : Ty} {m : String} (h : s.expose = .var m) (hs : s.isFloat = true ∨ s = .int) :
    False := by
  rcases hs with hs | hs
  · cases s <;> simp_all [Ty.isFloat, Ty.expose, Ty.leadingMus, Ty.exposeN]
  · subst hs; simp [Ty.expose, Ty.leadingMus, Ty.exposeN] at h

/-- A negation the generic synthesizes, against a type parameter not bounded
to the floats, is no quiet check: what it negates would have to be the
parameter, which has no negation. -/
theorem neg_subsume_var {env : InferEnv} {n : Nat} {Γ : Ctx} {a : Expr} {E : Ty} {m : String}
    (h : Quiet (subsumeF env n Γ (.unop .neg a) E) ()) (he : E.expose = .var m)
    (hb : Γ.boundOf (.var m) ≠ some .float) : False := by
  cases n with
  | zero => simp [subsumeF] at h
  | succ n =>
    simp only [subsumeF] at h
    obtain ⟨s, hs, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm
      have hse : s.expose = .var m := by
        simp only [Ty.meet, Ty.equiv_iff_eq, Bool.not_eq_true', Bool.not_eq_false] at hm
        simpa [he] using hm
      cases n with
      | zero => simp [synthF] at hs
      | succ n =>
        simp only [synthF] at hs
        obtain ⟨u, -, hs⟩ := hs.bind_inv
        split at hs
        · rename_i hc
          rw [Quiet.pure_iff] at hs; subst hs
          simp only [Bool.or_eq_true, beq_iff_eq, Ctx.enter_boundOf] at hc
          rcases hc with (hc | hc) | hc
          · cases u <;> simp [Ctx.boundOf] at hc
            rename_i k
            simp [Ty.expose, Ty.leadingMus, Ty.exposeN] at hse
            subst hse
            exact hb (by simpa [Ctx.boundOf] using hc)
          · exact Ty.expose_var_iff hse (.inl hc)
          · exact Ty.expose_var_iff hse (.inr hc)
        · simp at hs

theorem tysList_length_inst (θ : List (String × Ty)) (ts : Tys) :
    (tysList (ts.inst θ)).length = (tysList ts).length := by
  simp [tysList_inst]

theorem fieldsList_names_inst (θ : List (String × Ty)) (fs : Fields) :
    (fieldsList (fs.inst θ)).map (·.1) = (fieldsList fs).map (·.1) := by
  simp [fieldsList_inst, Function.comp_def]

/-! ## Checking an expression under an instance -/

section
variable {θ : List (String × Ty)} {env env' : InferEnv}

mutual

theorem synthF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (e : Expr)
    (t : Ty), Quiet (synthF env n Γ e) t → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (synthF env' n (Γ.inst θ) e) (t.inst θ)
  | 0, _, _, _, h, _, _ => by simp [synthF] at h
  | n + 1, Γ, e, t, h, r, sg => by
    have h0 := h
    cases e
    case hinted hh inner =>
      simp only [synthF] at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨_, hc, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      rw [Quiet.liftE_iff] at hu
      exact Quiet.bind (Quiet.liftE_iff.mpr (Ctx.hint_inst g hu))
        (Quiet.bind (checkForm_inst g H n _ inner u hc r sg) (Quiet.pure _))
    case var x =>
      simp only [synthF] at h ⊢
      cases hl : (Γ.enter (.var x)).lookup x with
      | none => simp [hl] at h
      | some p =>
        obtain ⟨u, k⟩ := p
        simp only [hl, Quiet.pure_iff] at h; subst h
        have : ((Γ.enter (.var x)).inst θ).lookup x = some (u.inst θ, k) := by
          rw [Ctx.lookup_inst, hl]; rfl
        simp only [Ctx.inst_enter] at this
        simp only [this]; exact Quiet.pure _
    case binop op a b =>
      simp only [synthF] at h ⊢
      obtain ⟨u, hp, hf⟩ := h.bind_inv
      exact Quiet.bind (pairF_inst g H n _ a b none u hp r sg) (finishBinop_inst g r hf)
    case unop op a =>
      cases op
      case neg =>
        simp only [synthF] at h ⊢
        obtain ⟨u, ha, h⟩ := h.bind_inv
        split at h
        · rename_i hc
          rw [Quiet.pure_iff] at h; subst h
          exact Quiet.bind (synthF_inst g H n _ a u ha r sg)
            (quiet_ite_true (negOk_inst (Γ := Γ.enter _) r hc) (Quiet.pure _))
        · simp at h
      case not =>
        simp only [synthF] at h ⊢
        obtain ⟨u, ha, h⟩ := h.bind_inv
        split at h
        · rename_i hc
          rw [Quiet.pure_iff] at h; subst h
          simp only [beq_iff_eq] at hc; subst hc
          exact Quiet.bind (synthF_inst g H n _ a _ ha r sg) (quiet_ite_true rfl (Quiet.pure _))
        · simp at h
      case negC =>
        simp only [synthF] at h ⊢
        obtain ⟨u, ha, h⟩ := h.bind_inv
        split at h
        · simp at h
        · rename_i hc
          split at h
          · simp at h
          · rename_i hr
            rw [Quiet.pure_iff] at h; subst h
            refine Quiet.bind (synthF_inst g H n _ a u ha r sg)
              (quiet_ite_false (not_false_of (fixOk_inst (Γ := Γ.enter _) r (true_of_not_not hc)))
                (quiet_ite_false (not_false_of (Ctx.returnsResult_inst g (Γ := Γ.enter _)
                  (true_of_not_not hr))) (Quiet.pure _)))
      case negO =>
        simp only [synthF] at h ⊢
        obtain ⟨u, ha, h⟩ := h.bind_inv
        split at h
        · simp at h
        · rename_i hc
          split at h
          · simp at h
          · rename_i hr
            rw [Quiet.pure_iff] at h; subst h
            refine Quiet.bind (synthF_inst g H n _ a u ha r sg)
              (quiet_ite_false (not_false_of (fixOk_inst (Γ := Γ.enter _) r (true_of_not_not hc)))
                (quiet_ite_false (not_false_of (Ctx.returnsOption_inst g (Γ := Γ.enter _)
                  (true_of_not_not hr))) (Quiet.pure _)))
    case tryOpt a =>
      simp only [synthF] at h ⊢
      obtain ⟨u, ha, h⟩ := h.bind_inv
      split at h
      · rename_i p hexp
        split at h
        · rename_i hro
          rw [Quiet.pure_iff] at h; subst h
          refine Quiet.bind (synthF_inst g H n _ a u ha r sg) ?_
          have : (u.inst θ).expose = .option (p.inst θ) := by rw [Ty.expose_inst g, hexp]; rfl
          simp only [this]
          exact quiet_ite_true (Ctx.returnsOption_inst g (Γ := Γ.enter _) hro) (Quiet.pure _)
        · simp at h
      · simp at h
    case tryRes a =>
      simp only [synthF] at h ⊢
      obtain ⟨u, ha, h⟩ := h.bind_inv
      split at h
      · rename_i p hexp
        split at h
        · rename_i hro
          rw [Quiet.pure_iff] at h; subst h
          refine Quiet.bind (synthF_inst g H n _ a u ha r sg) ?_
          have : (u.inst θ).expose = .result (p.inst θ) := by rw [Ty.expose_inst g, hexp]; rfl
          simp only [this]
          exact quiet_ite_true (Ctx.returnsResult_inst g (Γ := Γ.enter _) hro) (Quiet.pure _)
        · simp at h
      · simp at h
    case adapt a =>
      simp only [synthF] at h ⊢
      exact synthF_inst g H n _ a t h r sg
    case call f args =>
      simp only [synthF] at h ⊢
      exact callF_inst g H n _ f args none t h r sg
    case place x steps =>
      simp only [synthF] at h ⊢
      cases hl : (Γ.enter (.place x steps)).lookup x with
      | none => simp [hl] at h
      | some p =>
        obtain ⟨u, k⟩ := p
        simp only [hl] at h
        have : ((Γ.enter (.place x steps)).inst θ).lookup x = some (u.inst θ, k) := by
          rw [Ctx.lookup_inst, hl]; rfl
        simp only [Ctx.inst_enter] at this
        simp only [this]
        exact stepsF_inst g H n _ u steps t h r sg
    case proj b sel =>
      simp only [synthF] at h ⊢
      obtain ⟨bt, hb, h⟩ := h.bind_inv
      obtain ⟨ft, hf, h⟩ := h.bind_inv
      rw [Quiet.liftE_iff] at hf
      split at h
      · simp at h
      · rename_i hcp
        rw [Quiet.pure_iff] at h; subst h
        exact Quiet.bind (synthF_inst g H n _ b bt hb r sg)
          (Quiet.bind (Quiet.liftE_iff.mpr (fieldType_inst g hf))
            (quiet_ite_false (copyOk_inst hcp) (Quiet.pure _)))
    case index b k mode =>
      cases mode
      case none => simp [synthF] at h
      case some m =>
        simp only [synthF] at h ⊢
        obtain ⟨bt, hb, h⟩ := h.bind_inv
        obtain ⟨⟨et, kt⟩, hi, h⟩ := h.bind_inv
        rw [Quiet.liftE_iff] at hi
        obtain ⟨_, hk, h⟩ := h.bind_inv
        rw [Quiet.recover_iff] at hk
        split at h
        · simp at h
        · rename_i hv
          split at h
          · simp at h
          · rename_i hcp
            split at h
            · simp at h
            · rename_i hfm
              rw [Quiet.pure_iff] at h; subst h
              refine Quiet.bind (synthF_inst g H n _ b bt hb r sg)
                (Quiet.bind (Quiet.liftE_iff.mpr (indexTypes_inst g hi)) ?_)
              refine Quiet.bind (a := ()) (Quiet.recover_iff.mpr (checkF_inst g H n _ k kt hk r sg)) ?_
              refine quiet_ite_false ?_ (quiet_ite_false (copyOk_inst hcp) (quiet_ite_false ?_ (Quiet.pure _)))
              · exact Bool.eq_false_iff.mpr (by rw [isView_inst g hi]; exact hv)
              · simpa using Ctx.fitsMode_inst g (Γ := Γ.enter (.index b k (some m))) (by simpa using hfm)
    all_goals
      rw [synthF_lit rfl] at h ⊢
      have he := H.ty n Γ _ t r sg rfl h0
      unfold litBody at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      obtain ⟨_, hc, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      refine Quiet.bind (a := u.inst θ) ?_ (Quiet.bind (checkForm_inst g H n _ _ u hc r sg) (Quiet.pure _))
      simp only [Ctx.inst_enter] at he
      rw [he]
      revert hu
      generalize env.ty _ _ = res
      intro hu
      cases res with
      | ok v =>
        simp only [Quiet.pure_iff] at hu; subst hu
        exact Quiet.pure _
      | error c =>
        by_cases hcu : (c == unknownCode) = true
        · simp only [hcu, ↓reduceIte] at hu
          simp only [Except.map, hcu, ↓reduceIte]
          exact partsF_inst g H n _ _ u hu r sg
        · simp [hcu] at hu

theorem stepsF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (t : Ty)
    (steps : List Step) (u : Ty), Quiet (stepsF env n Γ t steps) u → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (stepsF env' n (Γ.inst θ) (t.inst θ) steps) (u.inst θ)
  | 0, _, _, _, _, h, _, _ => by simp [stepsF] at h
  | _ + 1, _, t, [], u, h, _, _ => by
    simp only [stepsF, Quiet.pure_iff] at h ⊢; rw [h]
  | n + 1, Γ, t, [st], u, h, r, sg => by
    simp only [stepsF] at h ⊢
    exact stepF_inst g H n Γ t st u h r sg
  | n + 1, Γ, t, st :: st2 :: rest, u, h, r, sg => by
    simp only [stepsF] at h ⊢
    obtain ⟨v, hv, h⟩ := h.bind_inv
    exact Quiet.bind (stepF_inst g H n _ t st v hv r sg) (stepsF_inst g H n Γ v _ u h r sg)

theorem stepF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (t : Ty)
    (st : Step) (u : Ty), Quiet (stepF env n Γ t st) u → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (stepF env' n (Γ.inst θ) (t.inst θ) st) (u.inst θ)
  | 0, _, _, _, _, h, _, _ => by simp [stepF] at h
  | n + 1, Γ, t, .field sel, u, h, r, sg => by
    simp only [stepF] at h ⊢
    obtain ⟨ft, hf, h⟩ := h.bind_inv
    rw [Quiet.liftE_iff] at hf
    split at h
    · simp at h
    · rename_i hcp
      rw [Quiet.pure_iff] at h; subst h
      exact Quiet.bind (Quiet.liftE_iff.mpr (fieldType_inst g hf))
        (quiet_ite_false (copyOk_inst hcp) (Quiet.pure _))
  | n + 1, Γ, t, .index k none, u, h, r, sg => by simp [stepF] at h
  | n + 1, Γ, t, .index k (some m), u, h, r, sg => by
    simp only [stepF] at h ⊢
    obtain ⟨⟨et, kt⟩, hi, h⟩ := h.bind_inv
    rw [Quiet.liftE_iff] at hi
    obtain ⟨_, hk, h⟩ := h.bind_inv
    rw [Quiet.recover_iff] at hk
    split at h
    · simp at h
    · rename_i hv
      split at h
      · simp at h
      · rename_i hcp
        split at h
        · simp at h
        · rename_i hfm
          rw [Quiet.pure_iff] at h; subst h
          refine Quiet.bind (Quiet.liftE_iff.mpr (indexTypes_inst g hi)) ?_
          refine Quiet.bind (a := ()) (Quiet.recover_iff.mpr (checkF_inst g H n _ k kt hk r sg)) ?_
          refine quiet_ite_false ?_ (quiet_ite_false (copyOk_inst hcp) (quiet_ite_false ?_ (Quiet.pure _)))
          · exact Bool.eq_false_iff.mpr (by rw [isView_inst g hi]; exact hv)
          · simpa using Ctx.fitsMode_inst g (Γ := Γ) (by simpa using hfm)

theorem pairF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (a b : Expr)
    (exp : Option Ty) (t : Ty), Quiet (pairF env n Γ a b exp) t → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (pairF env' n (Γ.inst θ) a b (exp.map (·.inst θ))) (t.inst θ)
  | 0, _, _, _, _, _, h, _, _ => by simp [pairF] at h
  | n + 1, Γ, a, b, exp, t, h, r, sg => by
    have hp := H.pair n Γ a b exp t r sg h
    simp only [pairF] at h
    simp only [pairF, hp]
    obtain ⟨ta, ha, h⟩ := h.bind_inv
    obtain ⟨tb, hb, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hab
      simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hab
      subst hab
      obtain ⟨_, hca, h⟩ := h.bind_inv
      obtain ⟨_, hcb, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      refine Quiet.bind (operandF_inst g H n _ a _ ta ha r sg)
        (Quiet.bind (operandF_inst g H n _ b _ ta hb r sg) ?_)
      refine quiet_ite_false (by simp) ?_
      exact Quiet.bind (checkF_inst g H n _ a ta hca r sg)
        (Quiet.bind (checkF_inst g H n _ b ta hcb r sg) (Quiet.pure _))

theorem operandF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (e : Expr)
    (res : Except Code Ty) (t : Ty), Quiet (operandF env n Γ e res) t →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (operandF env' n (Γ.inst θ) e (res.map (·.inst θ))) (t.inst θ)
  | 0, _, _, _, _, h, _, _ => by simp [operandF] at h
  | n + 1, Γ, e, .ok u, t, h, r, sg => by
    simp only [operandF, Quiet.pure_iff] at h; subst h
    exact Quiet.pure _
  | n + 1, Γ, e, .error c, t, h, r, sg => by
    simp only [operandF] at h
    show Quiet (operandF env' (n + 1) (Γ.inst θ) e (.error c)) _
    simp only [operandF]
    split at h
    · rename_i hc
      simp only [hc, ↓reduceIte]
      exact synthF_inst g H n _ e t h r sg
    · simp at h

theorem partsF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (e : Expr)
    (t : Ty), Quiet (partsF env n Γ e) t → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (partsF env' n (Γ.inst θ) e) (t.inst θ)
  | 0, _, _, _, h, _, _ => by simp [partsF] at h
  | n + 1, Γ, e, t, h, r, sg => by
    cases e
    case tt | ff | str | int | hex | float | atom =>
      simp only [partsF, Quiet.pure_iff] at h ⊢; subst h; rfl
    case tuple es =>
      simp only [partsF] at h ⊢
      obtain ⟨ts, hs, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      exact Quiet.bind (synthAllF_inst g H n Γ es ts hs r sg) (by rw [tysOf_inst]; exact Quiet.pure _)
    case struct fs =>
      simp only [partsF] at h ⊢
      obtain ⟨ts, hs, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      refine Quiet.bind (synthAllF_inst g H n Γ _ ts hs r sg) ?_
      rw [fieldsOf_zip_inst]; exact Quiet.pure _
    case list es | set es =>
      cases es with
      | nil => simp [partsF] at h
      | cons x xs =>
        simp only [partsF] at h ⊢
        obtain ⟨u, hx, h⟩ := h.bind_inv
        obtain ⟨_, hl, h⟩ := h.bind_inv
        rw [Quiet.pure_iff] at h; subst h
        exact Quiet.bind (synthF_inst g H n Γ x u hx r sg)
          (Quiet.bind (likeF_inst g H n Γ xs u hl r sg) (Quiet.pure _))
    case tensor shape es =>
      cases es with
      | nil => simp [partsF] at h
      | cons x xs =>
        simp only [partsF] at h ⊢
        obtain ⟨u, hx, h⟩ := h.bind_inv
        obtain ⟨_, hl, h⟩ := h.bind_inv
        rw [Quiet.pure_iff] at h; subst h
        exact Quiet.bind (synthF_inst g H n Γ x u hx r sg)
          (Quiet.bind (likeF_inst g H n Γ xs u hl r sg) (Quiet.pure _))
    case map es =>
      cases es with
      | nil => simp [partsF] at h
      | cons kv es =>
        obtain ⟨k, v⟩ := kv
        simp only [partsF] at h ⊢
        obtain ⟨kt, hk, h⟩ := h.bind_inv
        obtain ⟨vt, hv, h⟩ := h.bind_inv
        obtain ⟨_, hl, h⟩ := h.bind_inv
        rw [Quiet.pure_iff] at h; subst h
        exact Quiet.bind (synthF_inst g H n Γ k kt hk r sg)
          (Quiet.bind (synthF_inst g H n Γ v vt hv r sg)
            (Quiet.bind (entriesLikeF_inst g H n Γ es kt vt hl r sg) (Quiet.pure _)))
    case table header rows =>
      cases rows with
      | nil =>
        cases header with
        | nil =>
          simp only [partsF, Quiet.pure_iff] at h ⊢; subst h; rfl
        | cons _ _ => simp [partsF] at h
      | cons row rows =>
        simp only [partsF] at h ⊢
        split at h
        · simp at h
        · rename_i hw
          obtain ⟨ts, hs, h⟩ := h.bind_inv
          obtain ⟨_, hl, h⟩ := h.bind_inv
          rw [Quiet.pure_iff] at h; subst h
          refine quiet_ite_false (by simpa using hw) ?_
          refine Quiet.bind (synthAllF_inst g H n Γ row ts hs r sg)
            (Quiet.bind (rowsLikeF_inst g H n Γ _ rows ts hl r sg) ?_)
          rw [fieldsOf_zip_inst]; exact Quiet.pure _
    case some_ p | ok p | term a p =>
      simp only [partsF, map_eq_bind] at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      exact Quiet.bind (synthF_inst g H n Γ p u hu r sg) (Quiet.pure _)
    case data p | error p =>
      simp only [partsF] at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      rw [Quiet.pure_iff] at h; subst h
      exact Quiet.bind (synthF_inst g H n Γ p u hu r sg) (Quiet.pure _)
    all_goals simp [partsF] at h

theorem synthAllF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List Expr) (ts : List Ty), Quiet (synthAllF env n Γ es) ts → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (synthAllF env' n (Γ.inst θ) es) (ts.map (·.inst θ))
  | 0, _, _, _, h, _, _ => by simp [synthAllF] at h
  | _ + 1, _, [], ts, h, _, _ => by
    simp only [synthAllF, Quiet.pure_iff] at h ⊢; subst h; rfl
  | n + 1, Γ, e :: es, ts, h, r, sg => by
    simp only [synthAllF] at h ⊢
    obtain ⟨t, ht, h⟩ := h.bind_inv
    obtain ⟨ts', hts, h⟩ := h.bind_inv
    rw [Quiet.pure_iff] at h; subst h
    exact Quiet.bind (synthF_inst g H n Γ e t ht r sg)
      (Quiet.bind (synthAllF_inst g H n Γ es ts' hts r sg) (Quiet.pure _))

theorem likeF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List Expr) (t : Ty), Quiet (likeF env n Γ es t) () → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (likeF env' n (Γ.inst θ) es (t.inst θ)) ()
  | 0, _, _, _, h, _, _ => by simp [likeF] at h
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | n + 1, Γ, e :: es, t, h, r, sg => by
    simp only [likeF] at h ⊢
    obtain ⟨et, he, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm
      exact Quiet.bind (synthF_inst g H n Γ e et he r sg)
        (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm))
          (likeF_inst g H n Γ es t h r sg))

theorem entriesLikeF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List (Expr × Expr)) (kt vt : Ty), Quiet (entriesLikeF env n Γ es kt vt) () →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (entriesLikeF env' n (Γ.inst θ) es (kt.inst θ) (vt.inst θ)) ()
  | 0, _, _, _, _, h, _, _ => by simp [entriesLikeF] at h
  | _ + 1, _, [], _, _, _, _, _ => by exact Quiet.pure _
  | n + 1, Γ, (k, v) :: es, kt, vt, h, r, sg => by
    simp only [entriesLikeF] at h ⊢
    obtain ⟨kt', hk, h⟩ := h.bind_inv
    obtain ⟨vt', hv, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm1
      split at h
      · simp at h
      · rename_i hm2
        exact Quiet.bind (synthF_inst g H n Γ k kt' hk r sg)
          (Quiet.bind (synthF_inst g H n Γ v vt' hv r sg)
            (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm1))
              (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm2))
                (entriesLikeF_inst g H n Γ es kt vt h r sg))))

theorem rowsLikeF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (w : Nat)
    (rows : List (List Expr)) (ts : List Ty), Quiet (rowsLikeF env n Γ w rows ts) () →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (rowsLikeF env' n (Γ.inst θ) w rows (ts.map (·.inst θ))) ()
  | 0, _, _, _, _, h, _, _ => by simp [rowsLikeF] at h
  | _ + 1, _, _, [], _, _, _, _ => by exact Quiet.pure _
  | n + 1, Γ, w, row :: rows, ts, h, r, sg => by
    simp only [rowsLikeF] at h ⊢
    split at h
    · simp at h
    · rename_i hw
      obtain ⟨_, hr, h⟩ := h.bind_inv
      exact quiet_ite_false (by simpa using hw)
        (Quiet.bind (rowLikeF_inst g H n Γ row ts hr r sg) (rowsLikeF_inst g H n Γ w rows ts h r sg))

theorem rowLikeF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List Expr) (ts : List Ty), Quiet (rowLikeF env n Γ es ts) () →
    Respects θ Γ → SigsOk Γ.sigs → Quiet (rowLikeF env' n (Γ.inst θ) es (ts.map (·.inst θ))) ()
  | 0, _, _, _, h, _, _ => by simp [rowLikeF] at h
  | n + 1, Γ, e :: es, t :: ts, h, r, sg => by
    simp only [rowLikeF] at h ⊢
    obtain ⟨et, he, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm
      exact Quiet.bind (synthF_inst g H n Γ e et he r sg)
        (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm))
          (rowLikeF_inst g H n Γ es ts h r sg))
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | _ + 1, _, _ :: _, [], _, _, _ => by exact Quiet.pure _

theorem callF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (f : String)
    (args : List (Option Mode × Expr)) (exp : Option Ty) (t : Ty), Quiet (callF env n Γ f args exp) t →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (callF env' n (Γ.inst θ) f args (exp.map (·.inst θ))) (t.inst θ)
  | 0, _, _, _, _, _, h, _, _ => by simp [callF] at h
  | n + 1, Γ, f, args, exp, t, h, r, sg => by
    have hc := H.call n Γ f args exp t r sg h
    simp only [callF] at h
    simp only [callF, hc, Ctx.inst_sigs]
    cases hs : Γ.sigs.lookup f with
    | none => simp [hs] at h
    | some sig =>
      simp only [hs] at h ⊢
      split at h
      · simp at h
      · rename_i hl
        obtain ⟨_, hm, h⟩ := h.bind_inv
        obtain ⟨θ', hd, h⟩ := h.bind_inv
        rw [Quiet.liftE_iff] at hd
        obtain ⟨_, ha, h⟩ := h.bind_inv
        split at h
        · simp at h
        · rename_i hb
          split at h
          · simp at h
          · rename_i hall
            have hall' : allDecided sig.tparams θ' = true := by simpa using hall
            have own : ∀ n, sig.tparams.any (·.1 == n) = true → (θ'.lookup n).isSome = true :=
              allDecided_covers hall'
            have hps : (sig.params.map fun (m, t) => (m, t.inst (instArgs θ θ'))) =
                (sig.params.map fun (m, t) => (m, t.inst θ')).map fun (m, t) => (m, t.inst θ) := by
              rw [List.map_map]
              apply List.map_congr_left
              intro ⟨m, u⟩ hmt
              simp only [Function.comp]
              rw [Ty.inst_inst θ θ' u (Ty.allVars_mono own u ((sg f sig hs).1 _ hmt))]
            refine quiet_ite_false (by simpa using hl) ?_
            refine Quiet.bind (a := ()) hm ?_
            refine Quiet.bind (Quiet.liftE_iff.mpr (decided_inst θ _ hd)) ?_
            rw [hps]
            refine Quiet.bind (argsF_inst g H n Γ args _ ha r sg) ?_
            refine quiet_ite_false (by simpa using boundsHold_inst g r (by simpa using hb))
              (quiet_ite_false (by simpa using allDecided_inst (θ := θ) hall') ?_)
            cases hr : sig.ret with
            | void =>
              simp only [hr, Quiet.pure_iff] at h ⊢; subst h; rfl
            | ty rt =>
              simp only [hr, Quiet.pure_iff] at h ⊢; subst h
              rw [Ty.inst_inst θ θ' rt (Ty.allVars_mono own rt ((sg f sig hs).2 rt hr))]

theorem argsF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (args : List (Option Mode × Expr)) (ps : List (Mode × Ty)), Quiet (argsF env n Γ args ps) () →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (argsF env' n (Γ.inst θ) args (ps.map fun (m, t) => (m, t.inst θ))) ()
  | 0, _, _, _, h, _, _ => by simp [argsF] at h
  | n + 1, Γ, (mk, e) :: args, (m, t) :: ps, h, r, sg => by
    simp only [argsF] at h ⊢
    obtain ⟨_, he, h⟩ := h.bind_inv
    exact Quiet.bind (checkF_inst g H n _ e t he r sg) (argsF_inst g H n Γ args ps h r sg)
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | _ + 1, _, _ :: _, [], _, _, _ => by exact Quiet.pure _

theorem checkF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (e : Expr)
    (t : Ty), Quiet (checkF env n Γ e t) () → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (checkF env' n (Γ.inst θ) e (t.inst θ)) ()
  | 0, _, _, _, h, _, _ => by simp [checkF] at h
  | n + 1, Γ, e, t, h, r, sg => by
    cases e
    case hinted hh inner =>
      simp only [checkF] at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      rw [Quiet.liftE_iff] at hu
      split at h
      · simp at h
      · rename_i hm
        exact Quiet.bind (Quiet.liftE_iff.mpr (Ctx.hint_inst g hu))
          (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm))
            (checkForm_inst g H n _ inner t h r sg))
    all_goals (simp only [checkF] at h ⊢; exact checkForm_inst g H n _ _ t h r sg)

theorem checkForm_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (e : Expr)
    (t : Ty), Quiet (checkForm env n Γ e t) () → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (checkForm env' n (Γ.inst θ) e (t.inst θ)) ()
  | 0, _, _, _, h, _, _ => by simp [checkForm] at h
  | n + 1, Γ, e, E, h, r, sg => by
    have hx : ∀ u : Ty, E.expose = u → (E.inst θ).expose = u.inst θ := fun u hu => by
      rw [Ty.expose_inst g, hu]
    cases e
    case none_ =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i p hp; rw [hx _ hp]; exact Quiet.pure _
      · simp at h
    case er p =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i u hu; rw [hx _ hu]; exact checkF_inst g H n Γ p .error h r sg
      · simp at h
    case some_ p =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i u hu; rw [hx _ hu]; exact checkF_inst g H n Γ p u h r sg
      · simp at h
    case ok p =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i u hu; rw [hx _ hu]; exact checkF_inst g H n Γ p u h r sg
      · simp at h
    case tt | ff | str =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i hc
        simp only [beq_iff_eq] at hc; subst hc
        exact quiet_ite_true (by simp [Ty.inst]) (Quiet.pure _)
      · simp at h
    case int k =>
      simp only [checkForm] at h ⊢
      have h1 := h
      rw [Ty.expose_inst g]
      split at h
      · rename_i hi
        simp only [beq_iff_eq] at hi
        rw [hi]; exact quiet_ite_true (by simp [Ty.inst]) (Quiet.pure _)
      · split at h
        · rename_i lo hi' code hr
          rw [intRange_inst θ hr]; exact h1
        · simp at h
    case hex k =>
      simp only [checkForm] at h ⊢
      have h1 := h
      rw [Ty.expose_inst g]
      split at h
      · rename_i hi
        simp only [beq_iff_eq] at hi
        rw [hi]; exact quiet_ite_true (by simp [Ty.inst]) (Quiet.pure _)
      · split at h
        · rename_i bits code hr
          rw [hexBits_inst θ hr]; exact h1
        · simp at h
    case float =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i hp; rw [hx _ hp]; exact Quiet.pure _
      · rename_i hp; rw [hx _ hp]; exact Quiet.pure _
      · simp at h
    case tuple es =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i ts hts
        rw [hx _ hts]
        split at h
        · simp at h
        · rename_i hlen
          simp only [Ty.inst]
          refine quiet_ite_false (by simpa [tysList_length_inst] using hlen) ?_
          rw [tysList_inst]; exact pairsF_inst g H n Γ es _ h r sg
      · simp at h
    case struct fs =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i ts hts
        rw [hx _ hts]
        split at h
        · simp at h
        · rename_i hlen
          simp only [Ty.inst]
          refine quiet_ite_false (by simpa [fieldsList_inst] using hlen) ?_
          rw [fieldsList_inst]; exact fieldsF_inst g H n Γ fs _ h r sg
      · simp at h
    case list es | set es =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i u hu; rw [hx _ hu]; exact allF_inst g H n Γ es u h r sg
      · simp at h
    case map es =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i k v hu; rw [hx _ hu]; exact entriesF_inst g H n Γ es k v h r sg
      · simp at h
    case tensor shape es =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i u rank hu
        rw [hx _ hu]
        split at h
        · simp at h
        · rename_i h1
          split at h
          · simp at h
          · rename_i h2
            simp only [Ty.inst]
            exact quiet_ite_false (by simpa using h1) (quiet_ite_false (by simpa using h2)
              (allF_inst g H n Γ es u h r sg))
      · simp at h
    case table header rows =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i cols hu
        rw [hx _ hu]
        split at h
        · simp at h
        · rename_i h1
          split at h
          · simp at h
          · rename_i h2
            simp only [Ty.inst]
            refine quiet_ite_false (by simpa [fieldsList_inst] using h1)
              (quiet_ite_false (by rw [fieldsList_names_inst]; simpa using h2) ?_)
            rw [fieldsList_inst]; exact rowsF_inst g H n Γ rows _ h r sg
      · simp at h
    case atom a =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i b hb; rw [hx _ hb]; exact h
      · rename_i vs hvs
        rw [hx _ hvs]
        simp only [Ty.inst, atomListed_inst]; exact h
      · simp at h
    case term a p =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i b u hb
        rw [hx _ hb]
        split at h
        · rename_i hab
          simp only [Ty.inst]
          exact quiet_ite_true hab (checkF_inst g H n Γ p u h r sg)
        · simp at h
      · rename_i vs hvs
        rw [hx _ hvs]
        simp only [Ty.inst, termPayload_inst]
        split at h
        · rename_i u hu
          simp only [hu, Option.map_some]
          exact checkF_inst g H n Γ p u h r sg
        · simp at h
      · simp at h
    case enum inner =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i vs hvs; rw [hx _ hvs]; exact checkF_inst g H n Γ inner E h r sg
      · simp at h
    case data p | error p =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i hu
        rw [hx _ hu]
        obtain ⟨u, hs, h⟩ := h.bind_inv
        exact Quiet.bind (synthF_inst g H n Γ p u hs r sg) (Quiet.pure _)
      · simp at h
    case binop op a b =>
      simp only [checkForm] at h ⊢
      split at h
      · rename_i hy
        obtain ⟨u, hp, h⟩ := h.bind_inv
        obtain ⟨rr, hf, h⟩ := h.bind_inv
        split at h
        · simp at h
        · rename_i hne
          simp only [bne_iff_ne, ne_eq, Decidable.not_not] at hne
          subst hne
          exact quiet_ite_true hy (Quiet.bind (pairF_inst g H n Γ a b (some rr) u hp r sg)
            (Quiet.bind (finishBinop_inst g r hf) (quiet_ite_false (by simp) (Quiet.pure _))))
      · rename_i hy
        exact quiet_ite_false (by simpa using hy) (subsumeF_inst g H n Γ _ E h r sg)
    case adapt a =>
      simp only [checkForm] at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      split at h
      · simp at h
      · rename_i hc
        exact Quiet.bind (synthF_inst g H n _ a u hu r sg)
          (quiet_ite_false (by simpa using Ty.canAdapt_inst g (by simpa using hc)) (Quiet.pure _))
    case call f args =>
      simp only [checkForm] at h ⊢
      obtain ⟨u, hu, h⟩ := h.bind_inv
      split at h
      · simp at h
      · rename_i hm
        exact Quiet.bind (callF_inst g H n Γ f args (some E) u hu r sg)
          (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm)) (Quiet.pure _))
    case unop op a =>
      cases op
      case neg =>
        have none_float : ((none : Option Bound) == some Bound.float) = false := rfl
        cases a
        case int k =>
          simp only [checkForm] at h ⊢
          rw [Ty.expose_inst g]
          by_cases hv : ∃ m, E.expose = .var m
          · obtain ⟨m, hm⟩ := hv
            rw [hm] at h ⊢
            simp only [Lit.intRange] at h
            by_cases hb : Γ.boundOf (.var m) = some .float
            · have hf' : isFloatT ((Ty.var m).inst θ) = true := r.boundOf hb
              have hir : Lit.intRange ((Ty.var m).inst θ) = none := by
                revert hf'; cases (Ty.var m).inst θ <;> simp [isFloatT, Ty.isFloat, Lit.intRange]
              have hc : (isFloatT (Ty.var m) || Γ.boundOf (.var m) == some .float) = true := by
                simp [hb]
              simp only [hc, ↓reduceIte] at h
              simp only [hir, hf', Bool.true_or, ↓reduceIte]
              exact checkF_inst g H n _ _ _ h r sg
            · have hc : (isFloatT (Ty.var m) || Γ.boundOf (.var m) == some .float) = false := by
                simp [isFloatT, Ty.isFloat, hb]
              simp only [hc, Bool.false_eq_true, ↓reduceIte] at h
              exact (neg_subsume_var h hm hb).elim
          · have hv' : ∀ m, E.expose ≠ .var m := fun m h => hv ⟨m, h⟩
            rw [isFloatT_inst_nonvar θ hv', intRange_inst_nonvar θ hv', Ctx.boundOf_inst]
            rw [boundOf_nonvar hv'] at h
            cases hr : Lit.intRange E.expose with
            | some x =>
              obtain ⟨lo, hi, code⟩ := x
              simp only [hr] at h ⊢; exact h
            | none =>
              simp only [hr, none_float, Bool.or_false] at h ⊢
              cases hc : isFloatT E.expose
              · simp only [hc, Bool.false_eq_true, ↓reduceIte] at h ⊢
                exact subsumeF_inst g H n Γ _ E h r sg
              · simp only [hc, ↓reduceIte] at h ⊢
                exact checkF_inst g H n _ _ _ h r sg
        all_goals
          simp only [checkForm] at h ⊢
          rw [Ty.expose_inst g]
          by_cases hv : ∃ m, E.expose = .var m
          · obtain ⟨m, hm⟩ := hv
            rw [hm] at h ⊢
            by_cases hb : Γ.boundOf (.var m) = some .float
            · have hf' : isFloatT ((Ty.var m).inst θ) = true := r.boundOf hb
              have hc : (isFloatT (Ty.var m) || Γ.boundOf (.var m) == some .float) = true := by
                simp [hb]
              simp only [hc, ↓reduceIte] at h
              simp only [hf', Bool.true_or, ↓reduceIte]
              exact checkF_inst g H n _ _ _ h r sg
            · have hc : (isFloatT (Ty.var m) || Γ.boundOf (.var m) == some .float) = false := by
                simp [isFloatT, Ty.isFloat, hb]
              simp only [hc, Bool.false_eq_true, ↓reduceIte] at h
              exact (neg_subsume_var h hm hb).elim
          · have hv' : ∀ m, E.expose ≠ .var m := fun m h => hv ⟨m, h⟩
            rw [isFloatT_inst_nonvar θ hv', Ctx.boundOf_inst]
            rw [boundOf_nonvar hv'] at h
            simp only [none_float, Bool.or_false] at h ⊢
            cases hc : isFloatT E.expose
            · simp only [hc, Bool.false_eq_true, ↓reduceIte] at h ⊢
              exact subsumeF_inst g H n Γ _ E h r sg
            · simp only [hc, ↓reduceIte] at h ⊢
              exact checkF_inst g H n _ _ _ h r sg
      all_goals (simp only [checkForm] at h ⊢; exact subsumeF_inst g H n Γ _ E h r sg)
    all_goals (simp only [checkForm] at h ⊢; exact subsumeF_inst g H n Γ _ E h r sg)

theorem subsumeF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (e : Expr)
    (t : Ty), Quiet (subsumeF env n Γ e t) () → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (subsumeF env' n (Γ.inst θ) e (t.inst θ)) ()
  | 0, _, _, _, h, _, _ => by simp [subsumeF] at h
  | n + 1, Γ, e, t, h, r, sg => by
    simp only [subsumeF] at h ⊢
    obtain ⟨u, hu, h⟩ := h.bind_inv
    split at h
    · simp at h
    · rename_i hm
      exact Quiet.bind (synthF_inst g H n Γ e u hu r sg)
        (quiet_ite_false (by simpa using Ty.meet_inst g (by simpa using hm)) (Quiet.pure _))

theorem pairsF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List Expr) (ts : List Ty), Quiet (pairsF env n Γ es ts) () →
    Respects θ Γ → SigsOk Γ.sigs → Quiet (pairsF env' n (Γ.inst θ) es (ts.map (·.inst θ))) ()
  | 0, _, _, _, h, _, _ => by simp [pairsF] at h
  | n + 1, Γ, e :: es, t :: ts, h, r, sg => by
    simp only [pairsF] at h ⊢
    obtain ⟨_, he, h⟩ := h.bind_inv
    exact Quiet.bind (checkF_inst g H n Γ e t he r sg) (pairsF_inst g H n Γ es ts h r sg)
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | _ + 1, _, _ :: _, [], _, _, _ => by exact Quiet.pure _

theorem fieldsF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (fs : List (String × Expr)) (ts : List (String × Ty)), Quiet (fieldsF env n Γ fs ts) () →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (fieldsF env' n (Γ.inst θ) fs (ts.map fun (k, t) => (k, t.inst θ))) ()
  | 0, _, _, _, h, _, _ => by simp [fieldsF] at h
  | n + 1, Γ, (m, e) :: fs, (k, t) :: ts, h, r, sg => by
    simp only [fieldsF] at h ⊢
    split at h
    · simp at h
    · rename_i hmk
      obtain ⟨_, he, h⟩ := h.bind_inv
      exact quiet_ite_false (by simpa using hmk)
        (Quiet.bind (checkF_inst g H n Γ e t he r sg) (fieldsF_inst g H n Γ fs ts h r sg))
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | _ + 1, _, _ :: _, [], _, _, _ => by exact Quiet.pure _

theorem allF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List Expr) (t : Ty), Quiet (allF env n Γ es t) () → Respects θ Γ → SigsOk Γ.sigs →
    Quiet (allF env' n (Γ.inst θ) es (t.inst θ)) ()
  | 0, _, _, _, h, _, _ => by simp [allF] at h
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | n + 1, Γ, e :: es, t, h, r, sg => by
    simp only [allF] at h ⊢
    obtain ⟨_, he, h⟩ := h.bind_inv
    exact Quiet.bind (checkF_inst g H n Γ e t he r sg) (allF_inst g H n Γ es t h r sg)

theorem entriesF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (es : List (Expr × Expr)) (k v : Ty), Quiet (entriesF env n Γ es k v) () →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (entriesF env' n (Γ.inst θ) es (k.inst θ) (v.inst θ)) ()
  | 0, _, _, _, _, h, _, _ => by simp [entriesF] at h
  | _ + 1, _, [], _, _, _, _, _ => by exact Quiet.pure _
  | n + 1, Γ, (ke, ve) :: es, k, v, h, r, sg => by
    simp only [entriesF] at h ⊢
    obtain ⟨_, hk, h⟩ := h.bind_inv
    obtain ⟨_, hv, h⟩ := h.bind_inv
    exact Quiet.bind (checkF_inst g H n Γ ke k hk r sg)
      (Quiet.bind (checkF_inst g H n Γ ve v hv r sg) (entriesF_inst g H n Γ es k v h r sg))

theorem rowsF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (rows : List (List Expr)) (cols : List (String × Ty)), Quiet (rowsF env n Γ rows cols) () →
    Respects θ Γ → SigsOk Γ.sigs →
    Quiet (rowsF env' n (Γ.inst θ) rows (cols.map fun (k, t) => (k, t.inst θ))) ()
  | 0, _, _, _, h, _, _ => by simp [rowsF] at h
  | _ + 1, _, [], _, _, _, _ => by exact Quiet.pure _
  | n + 1, Γ, row :: rows, cols, h, r, sg => by
    simp only [rowsF] at h ⊢
    split at h
    · simp at h
    · rename_i hw
      obtain ⟨_, hp, h⟩ := h.bind_inv
      refine quiet_ite_false (by simpa using hw) (Quiet.bind (a := ()) ?_ (rowsF_inst g H n Γ rows cols h r sg))
      have := pairsF_inst g H n Γ row (cols.map (·.2)) hp r sg
      simpa [List.map_map, Function.comp_def] using this

end

end

end Datalove.Fun
