import Datalove.Fun
import Datalove.Proofs.Equiv
import Datalove.Proofs.Convert

/-!
# Checking a generic once is enough

A generic function is checked once, with its type parameters standing for
themselves (`Ty.var`), and then run at whatever types its calls pick. That
is sound only if every instance would have checked too: the body with each
type parameter read as its type argument.

This file builds towards that theorem. Here are the facts about types it
rests on: substituting type arguments (`Ty.inst`) commutes with what the
checker does to a type, unfolding a `rec` at the head (`expose`) above all,
for type arguments that are `Ground`: no type parameter and no `rec` in them.
-/

namespace Datalove

/-- Type arguments with no type parameter and no `rec` in them. -/
def Ground (θ : List (String × Ty)) : Prop :=
  ∀ p ∈ θ, p.2.containsVar = false ∧ p.2.containsRec = false

theorem Ground.lookup {θ : List (String × Ty)} (g : Ground θ) {n : String} {t : Ty}
    (h : θ.lookup n = some t) : t.containsVar = false ∧ t.containsRec = false := by
  induction θ with
  | nil => simp at h
  | cons p ps ih =>
    obtain ⟨m, u⟩ := p
    by_cases e : m = n
    · subst e; simp [List.lookup] at h; subst h; exact g _ List.mem_cons_self
    · have : (n == m) = false := by simpa using fun h => e h.symm
      simp only [List.lookup, this] at h
      exact ih (fun q hq => g q (List.mem_cons_of_mem _ hq)) h

/-! ## Substitution of a `rec` leaves what has none alone -/

mutual

theorem Ty.subst_norec : ∀ (t : Ty) (d : Nat) (w : Ty), t.containsRec = false → t.subst d w = t
  | .list t, d, w, h | .set t, d, w, h | .option t, d, w, h | .result t, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Ty.subst_norec t d w h]
  | .tensor t _, d, w, h | .term _ t, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Ty.subst_norec t d w h]
  | .map k v, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Ty.subst_norec k d w h.1, Ty.subst_norec v d w h.2]
  | .tuple ts, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Tys.subst_norec ts d w h]
  | .struct fs, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Fields.subst_norec fs d w h]
  | .table fs, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Fields.subst_norec fs d w h]
  | .enum vs, d, w, h => by
    simp [Ty.containsRec] at h; simp [Ty.subst, Variants.subst_norec vs d w h]
  | .mu _, _, _, h | .recvar _, _, _, h => by simp [Ty.containsRec] at h
  | .bool, _, _, _ | .u8, _, _, _ | .i8, _, _, _ | .u16, _, _, _ | .i16, _, _, _
  | .u32, _, _, _ | .i32, _, _, _ | .u64, _, _, _ | .i64, _, _, _ | .index, _, _, _
  | .offset, _, _, _ | .f32, _, _, _ | .f64, _, _, _ | .int, _, _, _ | .string, _, _, _
  | .data, _, _, _ | .error, _, _, _ | .atom _, _, _, _ | .var _, _, _, _ => by simp [Ty.subst]

theorem Tys.subst_norec : ∀ (ts : Tys) (d : Nat) (w : Ty), ts.containsRec = false → ts.subst d w = ts
  | .nil, _, _, _ => by simp [Tys.subst]
  | .cons t ts, d, w, h => by
    simp [Tys.containsRec] at h
    simp [Tys.subst, Ty.subst_norec t d w h.1, Tys.subst_norec ts d w h.2]

theorem Fields.subst_norec : ∀ (fs : Fields) (d : Nat) (w : Ty), fs.containsRec = false →
    fs.subst d w = fs
  | .nil, _, _, _ => by simp [Fields.subst]
  | .cons _ t fs, d, w, h => by
    simp [Fields.containsRec] at h
    simp [Fields.subst, Ty.subst_norec t d w h.1, Fields.subst_norec fs d w h.2]

theorem Variants.subst_norec : ∀ (vs : Variants) (d : Nat) (w : Ty), vs.containsRec = false →
    vs.subst d w = vs
  | .nil, _, _, _ => by simp [Variants.subst]
  | .atom _ vs, d, w, h => by
    simp [Variants.containsRec] at h; simp [Variants.subst, Variants.subst_norec vs d w h]
  | .term _ t vs, d, w, h => by
    simp [Variants.containsRec] at h
    simp [Variants.subst, Ty.subst_norec t d w h.1, Variants.subst_norec vs d w h.2]

end

/-! ## Type arguments leave what has no type parameter alone -/

mutual

theorem Ty.inst_novar (θ : List (String × Ty)) : ∀ (t : Ty), t.containsVar = false → t.inst θ = t
  | .list t, h | .set t, h | .option t, h | .result t, h | .mu t, h => by
    simp [Ty.containsVar] at h; simp [Ty.inst, Ty.inst_novar θ t h]
  | .tensor t _, h | .term _ t, h => by
    simp [Ty.containsVar] at h; simp [Ty.inst, Ty.inst_novar θ t h]
  | .map k v, h => by
    simp [Ty.containsVar] at h; simp [Ty.inst, Ty.inst_novar θ k h.1, Ty.inst_novar θ v h.2]
  | .tuple ts, h => by simp [Ty.containsVar] at h; simp [Ty.inst, Tys.inst_novar θ ts h]
  | .struct fs, h => by simp [Ty.containsVar] at h; simp [Ty.inst, Fields.inst_novar θ fs h]
  | .table fs, h => by simp [Ty.containsVar] at h; simp [Ty.inst, Fields.inst_novar θ fs h]
  | .enum vs, h => by simp [Ty.containsVar] at h; simp [Ty.inst, Variants.inst_novar θ vs h]
  | .var _, h => by simp [Ty.containsVar] at h
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .data, _ | .error, _
  | .atom _, _ | .recvar _, _ => by simp [Ty.inst]

theorem Tys.inst_novar (θ : List (String × Ty)) : ∀ (ts : Tys), ts.containsVar = false → ts.inst θ = ts
  | .nil, _ => by simp [Tys.inst]
  | .cons t ts, h => by
    simp [Tys.containsVar] at h; simp [Tys.inst, Ty.inst_novar θ t h.1, Tys.inst_novar θ ts h.2]

theorem Fields.inst_novar (θ : List (String × Ty)) : ∀ (fs : Fields), fs.containsVar = false →
    fs.inst θ = fs
  | .nil, _ => by simp [Fields.inst]
  | .cons _ t fs, h => by
    simp [Fields.containsVar] at h
    simp [Fields.inst, Ty.inst_novar θ t h.1, Fields.inst_novar θ fs h.2]

theorem Variants.inst_novar (θ : List (String × Ty)) : ∀ (vs : Variants), vs.containsVar = false →
    vs.inst θ = vs
  | .nil, _ => by simp [Variants.inst]
  | .atom _ vs, h => by
    simp [Variants.containsVar] at h; simp [Variants.inst, Variants.inst_novar θ vs h]
  | .term _ t vs, h => by
    simp [Variants.containsVar] at h
    simp [Variants.inst, Ty.inst_novar θ t h.1, Variants.inst_novar θ vs h.2]

end

/-! ## Type arguments commute with unfolding -/

mutual

theorem Ty.inst_subst {θ : List (String × Ty)} (g : Ground θ) : ∀ (t : Ty) (d : Nat) (w : Ty),
    (t.subst d w).inst θ = (t.inst θ).subst d (w.inst θ)
  | .recvar i, d, w => by
    by_cases e : i = d <;> simp [Ty.subst, Ty.inst, e]
  | .var n, d, w => by
    simp only [Ty.subst, Ty.inst]
    cases hl : θ.lookup n with
    | none => simp [Ty.subst]
    | some t => simp [Ty.subst_norec t d _ (g.lookup hl).2]
  | .mu b, d, w => by simp [Ty.subst, Ty.inst, Ty.inst_subst g b (d + 1) w]
  | .list t, d, w | .set t, d, w | .option t, d, w | .result t, d, w => by
    simp [Ty.subst, Ty.inst, Ty.inst_subst g t d w]
  | .tensor t _, d, w | .term _ t, d, w => by simp [Ty.subst, Ty.inst, Ty.inst_subst g t d w]
  | .map k v, d, w => by simp [Ty.subst, Ty.inst, Ty.inst_subst g k d w, Ty.inst_subst g v d w]
  | .tuple ts, d, w => by simp [Ty.subst, Ty.inst, Tys.inst_subst g ts d w]
  | .struct fs, d, w => by simp [Ty.subst, Ty.inst, Fields.inst_subst g fs d w]
  | .table fs, d, w => by simp [Ty.subst, Ty.inst, Fields.inst_subst g fs d w]
  | .enum vs, d, w => by simp [Ty.subst, Ty.inst, Variants.inst_subst g vs d w]
  | .bool, _, _ | .u8, _, _ | .i8, _, _ | .u16, _, _ | .i16, _, _ | .u32, _, _ | .i32, _, _
  | .u64, _, _ | .i64, _, _ | .index, _, _ | .offset, _, _ | .f32, _, _ | .f64, _, _
  | .int, _, _ | .string, _, _ | .data, _, _ | .error, _, _ | .atom _, _, _ => by
    simp [Ty.subst, Ty.inst]

theorem Tys.inst_subst {θ : List (String × Ty)} (g : Ground θ) : ∀ (ts : Tys) (d : Nat) (w : Ty),
    (ts.subst d w).inst θ = (ts.inst θ).subst d (w.inst θ)
  | .nil, _, _ => by simp [Tys.subst, Tys.inst]
  | .cons t ts, d, w => by
    simp [Tys.subst, Tys.inst, Ty.inst_subst g t d w, Tys.inst_subst g ts d w]

theorem Fields.inst_subst {θ : List (String × Ty)} (g : Ground θ) : ∀ (fs : Fields) (d : Nat) (w : Ty),
    (fs.subst d w).inst θ = (fs.inst θ).subst d (w.inst θ)
  | .nil, _, _ => by simp [Fields.subst, Fields.inst]
  | .cons _ t fs, d, w => by
    simp [Fields.subst, Fields.inst, Ty.inst_subst g t d w, Fields.inst_subst g fs d w]

theorem Variants.inst_subst {θ : List (String × Ty)} (g : Ground θ) : ∀ (vs : Variants) (d : Nat) (w : Ty),
    (vs.subst d w).inst θ = (vs.inst θ).subst d (w.inst θ)
  | .nil, _, _ => by simp [Variants.subst, Variants.inst]
  | .atom _ vs, d, w => by simp [Variants.subst, Variants.inst, Variants.inst_subst g vs d w]
  | .term _ t vs, d, w => by
    simp [Variants.subst, Variants.inst, Ty.inst_subst g t d w, Variants.inst_subst g vs d w]

end

/-- A type argument is no `rec`, so substituting one keeps the leading
`rec`s. -/
theorem Ty.leadingMus_inst {θ : List (String × Ty)} (g : Ground θ) :
    ∀ (t : Ty), (t.inst θ).leadingMus = t.leadingMus
  | .mu b => by simp [Ty.inst, Ty.leadingMus, Ty.leadingMus_inst g b]
  | .var n => by
    simp only [Ty.inst]
    cases hl : θ.lookup n with
    | none => simp [Ty.leadingMus]
    | some t =>
      have := (g.lookup hl).2
      cases t <;> simp_all [Ty.leadingMus, Ty.containsRec]
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset
  | .f32 | .f64 | .int | .string | .data | .error | .atom _ | .recvar _
  | .tuple _ | .struct _ | .table _ | .list _ | .set _ | .map _ _ | .option _ | .result _
  | .tensor _ _ | .term _ _ | .enum _ => by simp [Ty.inst, Ty.leadingMus]

theorem Ty.exposeN_inst {θ : List (String × Ty)} (g : Ground θ) :
    ∀ (k : Nat) (t : Ty), Ty.exposeN k (t.inst θ) = (Ty.exposeN k t).inst θ
  | 0, t => by simp [Ty.exposeN]
  | k + 1, .mu b => by
    simp only [Ty.inst, Ty.exposeN, Ty.unfold]
    rw [← Ty.exposeN_inst g k]
    congr 1
    have := Ty.inst_subst g b 0 (.mu b)
    simp only [Ty.inst] at this
    exact this.symm
  | k + 1, .var n => by
    simp only [Ty.inst, Ty.exposeN]
    cases hl : θ.lookup n with
    | none => simp [Ty.exposeN]
    | some t =>
      have := (g.lookup hl).2
      cases t <;> simp_all [Ty.exposeN, Ty.containsRec]
  | _ + 1, .bool | _ + 1, .u8 | _ + 1, .i8 | _ + 1, .u16 | _ + 1, .i16 | _ + 1, .u32
  | _ + 1, .i32 | _ + 1, .u64 | _ + 1, .i64 | _ + 1, .index | _ + 1, .offset
  | _ + 1, .f32 | _ + 1, .f64 | _ + 1, .int | _ + 1, .string | _ + 1, .data
  | _ + 1, .error | _ + 1, .atom _ | _ + 1, .recvar _
  | _ + 1, .tuple _ | _ + 1, .struct _ | _ + 1, .table _ | _ + 1, .list _ | _ + 1, .set _
  | _ + 1, .map _ _ | _ + 1, .option _ | _ + 1, .result _ | _ + 1, .tensor _ _
  | _ + 1, .term _ _ | _ + 1, .enum _ => by simp [Ty.inst, Ty.exposeN]

/-- Substituting type arguments commutes with unfolding at the head. -/
theorem Ty.expose_inst {θ : List (String × Ty)} (g : Ground θ) (t : Ty) :
    (t.inst θ).expose = t.expose.inst θ := by
  simp only [Ty.expose, Ty.leadingMus_inst g, Ty.exposeN_inst g]

theorem Ty.meet_inst {θ : List (String × Ty)} (g : Ground θ) {a b : Ty} (h : a.meet b = true) :
    (a.inst θ).meet (b.inst θ) = true := by
  simp only [Ty.meet, Ty.equiv_iff_eq, Ty.expose_inst g] at *
  rw [h]

/-! ## Classes kept by substitution -/

mutual

theorem Ty.isCopy_inst (θ : List (String × Ty)) : ∀ (t : Ty), t.isCopy = true → (t.inst θ).isCopy = true
  | .option t, h | .term _ t, h => by simp [Ty.isCopy] at h; simp [Ty.inst, Ty.isCopy, Ty.isCopy_inst θ t h]
  | .tuple ts, h => by simp [Ty.isCopy] at h; simp [Ty.inst, Ty.isCopy, Tys.isCopy_inst θ ts h]
  | .struct fs, h => by simp [Ty.isCopy] at h; simp [Ty.inst, Ty.isCopy, Fields.isCopy_inst θ fs h]
  | .enum vs, h => by simp [Ty.isCopy] at h; simp [Ty.inst, Ty.isCopy, Variants.isCopy_inst θ vs h]
  | .var _, h | .mu _, h | .int, h | .string, h | .list _, h | .map _ _, h | .set _, h
  | .tensor _ _, h | .data, h | .error, h | .result _, h | .table _, h | .recvar _, h => by
    simp [Ty.isCopy] at h
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .atom _, _ => by simp [Ty.inst, Ty.isCopy]

theorem Tys.isCopy_inst (θ : List (String × Ty)) : ∀ (ts : Tys), ts.isCopy = true → (ts.inst θ).isCopy = true
  | .nil, _ => by simp [Tys.inst, Tys.isCopy]
  | .cons t ts, h => by
    simp [Tys.isCopy] at h
    simp [Tys.inst, Tys.isCopy, Ty.isCopy_inst θ t h.1, Tys.isCopy_inst θ ts h.2]

theorem Fields.isCopy_inst (θ : List (String × Ty)) : ∀ (fs : Fields), fs.isCopy = true →
    (fs.inst θ).isCopy = true
  | .nil, _ => by simp [Fields.inst, Fields.isCopy]
  | .cons _ t fs, h => by
    simp [Fields.isCopy] at h
    simp [Fields.inst, Fields.isCopy, Ty.isCopy_inst θ t h.1, Fields.isCopy_inst θ fs h.2]

theorem Variants.isCopy_inst (θ : List (String × Ty)) : ∀ (vs : Variants), vs.isCopy = true →
    (vs.inst θ).isCopy = true
  | .nil, _ => by simp [Variants.inst, Variants.isCopy]
  | .atom _ vs, h => by
    simp [Variants.isCopy] at h; simp [Variants.inst, Variants.isCopy, Variants.isCopy_inst θ vs h]
  | .term _ t vs, h => by
    simp [Variants.isCopy] at h
    simp [Variants.inst, Variants.isCopy, Ty.isCopy_inst θ t h.1, Variants.isCopy_inst θ vs h.2]

end

end Datalove

namespace Datalove

/-! ## Adaptation kept by substitution -/

theorem Variants.hasAtom_inst (θ : List (String × Ty)) (a : String) :
    ∀ (vs : Variants), vs.hasAtom a = true → (vs.inst θ).hasAtom a = true
  | .nil, h => by simp [Variants.hasAtom] at h
  | .atom n vs, h => by
    simp only [Variants.hasAtom, Variants.inst, Bool.or_eq_true] at h ⊢
    exact h.imp id (Variants.hasAtom_inst θ a vs)
  | .term _ _ vs, h => by
    simp only [Variants.hasAtom, Variants.inst] at h ⊢
    exact Variants.hasAtom_inst θ a vs h

theorem Variants.hasTerm_inst (θ : List (String × Ty)) (a : String) (t : Ty) :
    ∀ (vs : Variants), vs.hasTerm a t = true → (vs.inst θ).hasTerm a (t.inst θ) = true
  | .nil, h => by simp [Variants.hasTerm] at h
  | .atom _ vs, h => by
    simp only [Variants.hasTerm, Variants.inst] at h ⊢
    exact Variants.hasTerm_inst θ a t vs h
  | .term n p vs, h => by
    simp only [Variants.hasTerm, Variants.inst, Bool.or_eq_true, Bool.and_eq_true,
      Ty.equiv_iff_eq] at h ⊢
    rcases h with ⟨hn, rfl⟩ | h
    · exact .inl ⟨hn, rfl⟩
    · exact .inr (Variants.hasTerm_inst θ a t vs h)

theorem Ty.canWiden_novar {a b : Ty} (h : a.canWiden b = true) :
    a.containsVar = false ∧ b.containsVar = false := by
  cases a <;> cases b <;> simp_all [Ty.canWiden, Ty.containsVar]

theorem Ty.adaptTable_inst (θ : List (String × Ty)) {a b : Ty} (h : Ty.adaptTable a b = true) :
    Ty.adaptTable (a.inst θ) (b.inst θ) = true := by
  cases a <;> cases b <;> simp_all [Ty.adaptTable, Ty.inst]
  · exact Variants.hasAtom_inst θ _ _ h
  · exact Variants.hasTerm_inst θ _ _ _ h

theorem Ty.canAdapt_inst {θ : List (String × Ty)} (g : Ground θ) {a b : Ty}
    (h : a.canAdapt b = true) : (a.inst θ).canAdapt (b.inst θ) = true := by
  simp only [Ty.canAdapt, Bool.or_eq_true] at h ⊢
  rcases h with (h | h) | h
  · exact .inl (.inl (Ty.meet_inst g h))
  · obtain ⟨ha, hb⟩ := Ty.canWiden_novar h
    rw [Ty.inst_novar θ a ha, Ty.inst_novar θ b hb]
    exact .inl (.inr h)
  · rw [Ty.expose_inst g, Ty.expose_inst g]
    exact .inr (Ty.adaptTable_inst θ h)

/-! ## Numbers are their own instances -/

theorem Ty.inst_isNumeric (θ : List (String × Ty)) {t : Ty} (h : t.isNumeric = true) :
    t.inst θ = t := by
  cases t <;> simp_all [Ty.isNumeric, Ty.inst]

theorem Ty.inst_isFixedInt (θ : List (String × Ty)) {t : Ty} (h : t.isFixedInt = true) :
    t.inst θ = t := by
  cases t <;> simp_all [Ty.isFixedInt, Ty.inst]

theorem Ty.inst_isFloat (θ : List (String × Ty)) {t : Ty} (h : t.isFloat = true) :
    t.inst θ = t := by
  cases t <;> simp_all [Ty.isFloat, Ty.inst]

end Datalove

namespace Datalove.Fun

open Datalove.Lit (tysList fieldsList)

mutual

theorem isEqT_inst (θ : List (String × Ty)) : ∀ (t : Ty), isEqT t = true → isEqT (t.inst θ) = true
  | .option t, h | .term _ t, h => by
    simp only [isEqT] at h; simp only [Ty.inst, isEqT]; exact isEqT_inst θ t h
  | .tuple ts, h => by simp only [isEqT] at h; simp only [Ty.inst, isEqT]; exact isEqTys_inst θ ts h
  | .struct fs, h => by
    simp only [isEqT] at h; simp only [Ty.inst, isEqT]; exact isEqFields_inst θ fs h
  | .enum vs, h => by
    simp only [isEqT] at h; simp only [Ty.inst, isEqT]; exact isEqVariants_inst θ vs h
  | .var _, h | .mu _, h | .recvar _, h | .data, h | .error, h | .list _, h | .set _, h
  | .map _ _, h | .result _, h | .tensor _ _, h | .table _, h => by
    simp [isEqT, Ty.isNumeric] at h
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .atom _, _ => by
    simp [isEqT, Ty.inst, Ty.isNumeric]

theorem isEqTys_inst (θ : List (String × Ty)) : ∀ (ts : Tys), isEqTys ts = true →
    isEqTys (ts.inst θ) = true
  | .nil, _ => by simp [isEqTys, Tys.inst]
  | .cons t ts, h => by
    simp only [isEqTys, Bool.and_eq_true] at h; simp only [Tys.inst, isEqTys, Bool.and_eq_true]
    exact ⟨isEqT_inst θ t h.1, isEqTys_inst θ ts h.2⟩

theorem isEqFields_inst (θ : List (String × Ty)) : ∀ (fs : Fields), isEqFields fs = true →
    isEqFields (fs.inst θ) = true
  | .nil, _ => by simp [isEqFields, Fields.inst]
  | .cons _ t fs, h => by
    simp only [isEqFields, Bool.and_eq_true] at h; simp only [Fields.inst, isEqFields, Bool.and_eq_true]
    exact ⟨isEqT_inst θ t h.1, isEqFields_inst θ fs h.2⟩

theorem isEqVariants_inst (θ : List (String × Ty)) : ∀ (vs : Variants), isEqVariants vs = true →
    isEqVariants (vs.inst θ) = true
  | .nil, _ => by simp [isEqVariants, Variants.inst]
  | .atom _ vs, h => by
    simp only [isEqVariants] at h; simp only [Variants.inst, isEqVariants]
    exact isEqVariants_inst θ vs h
  | .term _ t vs, h => by
    simp only [isEqVariants, Bool.and_eq_true] at h
    simp only [Variants.inst, isEqVariants, Bool.and_eq_true]
    exact ⟨isEqT_inst θ t h.1, isEqVariants_inst θ vs h.2⟩

end

/-! ## The checker's helpers commute with substitution -/

theorem tysList_inst (θ : List (String × Ty)) : ∀ (ts : Tys),
    tysList (ts.inst θ) = (tysList ts).map (·.inst θ)
  | .nil => by simp [tysList, Tys.inst]
  | .cons t ts => by simp [tysList, Tys.inst, tysList_inst θ ts]

theorem fieldsList_inst (θ : List (String × Ty)) : ∀ (fs : Fields),
    fieldsList (fs.inst θ) = (fieldsList fs).map fun (n, t) => (n, t.inst θ)
  | .nil => by simp [fieldsList, Fields.inst]
  | .cons n t fs => by simp [fieldsList, Fields.inst, fieldsList_inst θ fs]

theorem lookup_map_snd {α β γ} [BEq α] (f : β → γ) (n : α) :
    ∀ (l : List (α × β)), (l.map fun (k, v) => (k, f v)).lookup n = (l.lookup n).map f
  | [] => rfl
  | (k, v) :: l => by
    simp only [List.map, List.lookup]
    cases n == k <;> simp [lookup_map_snd f n l]

theorem fieldType_inst {θ : List (String × Ty)} (g : Ground θ) {t u : Ty} {s : Sel}
    (h : fieldType t s = .ok u) : fieldType (t.inst θ) s = .ok (u.inst θ) := by
  unfold fieldType at *
  rw [Ty.expose_inst g]
  cases he : t.expose <;> cases s <;> simp only [he, Ty.inst] at h ⊢ <;>
    first | (simp at h; done) | skip
  · rename_i ts i
    rw [tysList_inst, List.getElem?_map]
    cases hi : (tysList ts)[i]? <;> simp_all [pure, Except.pure]
  · rename_i fs nm
    rw [fieldsList_inst, lookup_map_snd]
    cases hi : (fieldsList fs).lookup nm <;> simp_all [pure, Except.pure]

theorem indexTypes_inst {θ : List (String × Ty)} (g : Ground θ) {t e k : Ty}
    (h : indexTypes t = .ok (e, k)) : indexTypes (t.inst θ) = .ok (e.inst θ, k.inst θ) := by
  unfold indexTypes at *
  rw [Ty.expose_inst g]
  cases he : t.expose <;> simp only [he, Ty.inst] at h ⊢ <;> first | (simp at h; done) | skip
  all_goals (simp only [pure, Except.pure, Except.ok.injEq, Prod.mk.injEq] at h)
  · obtain ⟨rfl, rfl⟩ := h; rfl
  · obtain ⟨rfl, rfl⟩ := h; rfl
  · rename_i r
    obtain ⟨rfl, rfl⟩ := h
    by_cases hr : r ≤ 1 <;> simp [hr, Ty.inst, pure, Except.pure]

theorem isView_inst {θ : List (String × Ty)} (g : Ground θ) {t e k : Ty}
    (h : indexTypes t = .ok (e, k)) : isView (t.inst θ) = isView t := by
  unfold isView
  unfold indexTypes at h
  rw [Ty.expose_inst g]
  cases he : t.expose <;> simp only [he, Ty.inst] at h ⊢ <;> simp at h ⊢

end Datalove.Fun

namespace Datalove

/-! ## A `rec` is kept, and a hint converts to its instance -/

mutual

theorem Ty.containsRec_inst {θ : List (String × Ty)} (g : Ground θ) :
    ∀ (t : Ty), (t.inst θ).containsRec = t.containsRec
  | .var n => by
    simp only [Ty.inst]
    cases hl : θ.lookup n with
    | none => simp [Ty.containsRec]
    | some t => simp [(g.lookup hl).2, Ty.containsRec]
  | .list t | .set t | .option t | .result t | .tensor t _ | .term _ t => by
    simp [Ty.inst, Ty.containsRec, Ty.containsRec_inst g t]
  | .map k v => by simp [Ty.inst, Ty.containsRec, Ty.containsRec_inst g k, Ty.containsRec_inst g v]
  | .tuple ts => by simp [Ty.inst, Ty.containsRec, Tys.containsRec_inst g ts]
  | .struct fs | .table fs => by simp [Ty.inst, Ty.containsRec, Fields.containsRec_inst g fs]
  | .enum vs => by simp [Ty.inst, Ty.containsRec, Variants.containsRec_inst g vs]
  | .mu _ | .recvar _ => by simp [Ty.inst, Ty.containsRec]
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset
  | .f32 | .f64 | .int | .string | .data | .error | .atom _ => by simp [Ty.inst, Ty.containsRec]

theorem Tys.containsRec_inst {θ : List (String × Ty)} (g : Ground θ) :
    ∀ (ts : Tys), (ts.inst θ).containsRec = ts.containsRec
  | .nil => by simp [Tys.inst, Tys.containsRec]
  | .cons t ts => by
    simp [Tys.inst, Tys.containsRec, Ty.containsRec_inst g t, Tys.containsRec_inst g ts]

theorem Fields.containsRec_inst {θ : List (String × Ty)} (g : Ground θ) :
    ∀ (fs : Fields), (fs.inst θ).containsRec = fs.containsRec
  | .nil => by simp [Fields.inst, Fields.containsRec]
  | .cons _ t fs => by
    simp [Fields.inst, Fields.containsRec, Ty.containsRec_inst g t, Fields.containsRec_inst g fs]

theorem Variants.containsRec_inst {θ : List (String × Ty)} (g : Ground θ) :
    ∀ (vs : Variants), (vs.inst θ).containsRec = vs.containsRec
  | .nil => by simp [Variants.inst, Variants.containsRec]
  | .atom _ vs => by simp [Variants.inst, Variants.containsRec, Variants.containsRec_inst g vs]
  | .term _ t vs => by
    simp [Variants.inst, Variants.containsRec, Ty.containsRec_inst g t,
      Variants.containsRec_inst g vs]

end

/-- Aliases with type arguments substituted into what they name. -/
def Aliases.inst (θ : List (String × Ty)) (A : Aliases) : Aliases :=
  A.map fun (n, t) => (n, t.inst θ)

theorem Aliases.lookup_inst (θ : List (String × Ty)) (A : Aliases) (n : String) :
    (A.inst θ).lookup n = (A.lookup n).map (·.inst θ) :=
  Fun.lookup_map_snd _ n A

/-- `m'` succeeds wherever `m` does, from the same scope to the same scope,
with what `m` gives mapped by `f`. -/
def Sim {α β} (f : α → β) (m : ConvM α) (m' : ConvM β) : Prop :=
  ∀ s a s', m.run s = .ok (a, s') → m'.run s = .ok (f a, s')

theorem bind_run {α β} (m : ConvM α) (k : α → ConvM β) {s s' : Scope} {a : α}
    (h : m.run s = .ok (a, s')) : (m >>= k).run s = (k a).run s' := by
  simp only [StateT.run, Bind.bind, StateT.bind, Except.bind] at h ⊢
  rw [h]

theorem Sim.refl {α} (m : ConvM α) : Sim id m m := fun _ _ _ h => h

theorem Sim.bind {α β γ δ} {f : α → β} {g : γ → δ} {m : ConvM α} {m' : ConvM β}
    {k : α → ConvM γ} {k' : β → ConvM δ}
    (h : Sim f m m') (hk : ∀ a, Sim g (k a) (k' (f a))) : Sim g (m >>= k) (m' >>= k') := by
  intro s c s'' hc
  obtain ⟨a, s', ha, hka⟩ := bind_ok hc
  rw [bind_run _ _ (h s a s' ha)]
  exact hk a s' c s'' hka

theorem Sim.pure {α β} {f : α → β} {a : α} {b : β} (e : b = f a) :
    Sim f (Pure.pure a) (Pure.pure b) := by
  intro s c s' hc
  obtain ⟨rfl, rfl⟩ := pure_ok hc
  subst e
  rfl

theorem Sim.map {α β γ δ} {f : α → β} {g : γ → δ} {u : α → γ} {u' : β → δ}
    {m : ConvM α} {m' : ConvM β} (h : Sim f m m') (hu : ∀ a, u' (f a) = g (u a)) :
    Sim g (u <$> m) (u' <$> m') := by
  rw [map_eq_pure_bind, map_eq_pure_bind]
  exact Sim.bind h (fun a => Sim.pure (hu a))

theorem Sim.collection {α β} {f : α → β} {m : ConvM α} {m' : ConvM β} (key : Bool)
    (h : Sim f m m') : Sim f (ConvM.collection key m) (ConvM.collection key m') := by
  simp only [ConvM.collection]
  exact Sim.bind (Sim.refl _) fun s => Sim.bind (Sim.refl _) fun _ =>
    Sim.bind h fun r => Sim.bind (Sim.refl _) fun _ => Sim.pure rfl

theorem ConvM.bind_inst (θ : List (String × Ty)) (A : Aliases) (n : String) :
    ConvM.bind (A.inst θ) n = ConvM.bind A n := by
  simp only [ConvM.bind, Aliases.lookup_inst, Option.isSome_map]

theorem ConvM.lookup_inst {θ : List (String × Ty)} (g : Ground θ) (A : Aliases) (n : String) :
    Sim (·.inst θ) (ConvM.lookup A n) (ConvM.lookup (A.inst θ) n) := by
  intro s t s' h
  simp only [ConvM.lookup] at h ⊢
  obtain ⟨g₀, s₀, hg, k⟩ := bind_ok h
  obtain ⟨e1, e2⟩ := get_ok hg
  subst e1 e2
  rw [bind_run _ _ (show (get : ConvM Scope).run s₀ = .ok (s₀, s₀) from rfl)]
  cases hf : List.findIdx? (fun x => x.name == n) s₀.binders with
  | none =>
    simp only [hf, Aliases.lookup_inst] at k ⊢
    cases hu : A.lookup n with
    | none => simp only [hu] at k; exact (fail_ok k).elim
    | some u =>
      simp only [hu, Option.map_some] at k ⊢
      split at k
      · exact (fail_ok k).elim
      · rename_i hrec
        split at k
        · exact (fail_ok k).elim
        · rename_i hvar
          have hvar' : (!s₀.binders.isEmpty && (u.inst θ).containsVar) = false := by
            cases hb : s₀.binders.isEmpty
            · have hv : u.containsVar = false := by simpa [hb] using hvar
              simp [Ty.inst_novar θ u hv, hv]
            · simp
          have hrec' : (s₀.inKey && (u.inst θ).containsRec) = false :=
            Bool.eq_false_iff.mpr (by rw [Ty.containsRec_inst g]; exact hrec)
          obtain ⟨rfl, rfl⟩ := pure_ok k
          simp only [hrec', hvar', Bool.false_eq_true, ↓reduceIte]
          rfl
  | some d =>
    simp only [hf] at k ⊢
    have k0 := k
    obtain ⟨_, s₁, hs, k₂⟩ := bind_ok k
    split at k₂
    · exact (fail_bind_ok k₂).elim
    · split at k₂
      · exact (fail_bind_ok k₂).elim
      · obtain ⟨rfl, -⟩ := pure_ok k₂
        exact k0

theorem Variants.toList_inst (θ : List (String × Ty)) : ∀ (vs : Variants),
    (vs.inst θ).toList = vs.toList.map fun p => (p.1, p.2.map (·.inst θ))
  | .nil => rfl
  | .atom n vs => by simp [Variants.inst, Variants.toList, Variants.toList_inst θ vs]
  | .term n t vs => by simp [Variants.inst, Variants.toList, Variants.toList_inst θ vs]

theorem Variants.ofList_inst (θ : List (String × Ty)) : ∀ (l : List (String × Option Ty)),
    Variants.ofList (l.map fun p => (p.1, p.2.map (·.inst θ))) = (Variants.ofList l).inst θ
  | [] => rfl
  | (n, none) :: l => by simp [Variants.ofList, Variants.inst, Variants.ofList_inst θ l]
  | (n, some t) :: l => by simp [Variants.ofList, Variants.inst, Variants.ofList_inst θ l]

theorem Variants.sortByName_inst (θ : List (String × Ty)) (vs : Variants) :
    (vs.inst θ).sortByName = vs.sortByName.inst θ := by
  simp only [Variants.sortByName, Variants.toList_inst]
  rw [← Variants.ofList_inst]
  congr 1
  exact (List.map_mergeSort (r := fun (a b : String × Option Ty) => !decide (b.1 < a.1))
    (s := fun (a b : String × Option Ty) => !decide (b.1 < a.1))
    (f := fun (p : String × Option Ty) => (p.1, p.2.map (·.inst θ)))
    (fun _ _ _ _ => rfl)).symm

open ConvM in
mutual

theorem Hint.conv_inst {θ : List (String × Ty)} (g : Ground θ) (A : Aliases) :
    ∀ (h : Hint), Sim (·.inst θ) (h.conv A) (h.conv (A.inst θ))
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset
  | .f32 | .f64 | .int | .string | .data | .error | .atom _ => by
    simp only [Hint.conv]; exact Sim.pure rfl
  | .tuple fs => by simp only [Hint.conv]; exact Sim.map (Hints.conv_inst g A fs) (fun _ => rfl)
  | .struct fs => by simp only [Hint.conv]; exact Sim.map (HFields.conv_inst g A fs) (fun _ => rfl)
  | .table cs => by
    simp only [Hint.conv]; exact Sim.map (HFields.convColumns_inst g A cs) (fun _ => rfl)
  | .list t => by
    simp only [Hint.conv]; exact Sim.map (Sim.collection _ (Hint.conv_inst g A t)) (fun _ => rfl)
  | .set t => by
    simp only [Hint.conv]; exact Sim.map (Sim.collection _ (Hint.conv_inst g A t)) (fun _ => rfl)
  | .tensor t _ => by
    simp only [Hint.conv]; exact Sim.map (Sim.collection _ (Hint.conv_inst g A t)) (fun _ => rfl)
  | .option t => by simp only [Hint.conv]; exact Sim.map (Hint.conv_inst g A t) (fun _ => rfl)
  | .result t => by simp only [Hint.conv]; exact Sim.map (Hint.conv_inst g A t) (fun _ => rfl)
  | .term _ t => by simp only [Hint.conv]; exact Sim.map (Hint.conv_inst g A t) (fun _ => rfl)
  | .map k v => by
    simp only [Hint.conv]
    exact Sim.bind (Sim.collection _ (Hint.conv_inst g A k)) fun _ =>
      Sim.bind (Sim.collection _ (Hint.conv_inst g A v)) fun _ => Sim.pure rfl
  | .enum vs => by
    simp only [Hint.conv]
    exact Sim.map (HVariants.conv_inst g A vs)
      (fun _ => by simp [Ty.inst, Variants.sortByName_inst])
  | .mu n body => by
    simp only [Hint.conv, ConvM.bind_inst]
    exact Sim.bind (Sim.refl _) fun _ => Sim.bind (Hint.conv_inst g A body) fun _ =>
      Sim.bind (Sim.refl _) fun _ => Sim.pure rfl
  | .name n => by simp only [Hint.conv]; exact ConvM.lookup_inst g A n

theorem Hints.conv_inst {θ : List (String × Ty)} (g : Ground θ) (A : Aliases) :
    ∀ (hs : Hints), Sim (·.inst θ) (hs.conv A) (hs.conv (A.inst θ))
  | .nil => by simp only [Hints.conv]; exact Sim.pure rfl
  | .cons t ts => by
    simp only [Hints.conv]
    exact Sim.bind (Hint.conv_inst g A t) fun _ => Sim.bind (Hints.conv_inst g A ts) fun _ =>
      Sim.pure rfl

theorem HFields.conv_inst {θ : List (String × Ty)} (g : Ground θ) (A : Aliases) :
    ∀ (fs : HFields), Sim (·.inst θ) (fs.conv A) (fs.conv (A.inst θ))
  | .nil => by simp only [HFields.conv]; exact Sim.pure rfl
  | .cons _ t fs => by
    simp only [HFields.conv]
    exact Sim.bind (Hint.conv_inst g A t) fun _ => Sim.bind (HFields.conv_inst g A fs) fun _ =>
      Sim.pure rfl

theorem HFields.convColumns_inst {θ : List (String × Ty)} (g : Ground θ) (A : Aliases) :
    ∀ (fs : HFields), Sim (·.inst θ) (fs.convColumns A) (fs.convColumns (A.inst θ))
  | .nil => by simp only [HFields.convColumns]; exact Sim.pure rfl
  | .cons _ t fs => by
    simp only [HFields.convColumns]
    exact Sim.bind (Sim.collection _ (Hint.conv_inst g A t)) fun _ =>
      Sim.bind (HFields.convColumns_inst g A fs) fun _ => Sim.pure rfl

theorem HVariants.conv_inst {θ : List (String × Ty)} (g : Ground θ) (A : Aliases) :
    ∀ (vs : HVariants), Sim (·.inst θ) (vs.conv A) (vs.conv (A.inst θ))
  | .nil => by simp only [HVariants.conv]; exact Sim.pure rfl
  | .atom _ vs => by
    simp only [HVariants.conv]; exact Sim.map (HVariants.conv_inst g A vs) (fun _ => rfl)
  | .term _ t vs => by
    simp only [HVariants.conv]
    exact Sim.bind (Hint.conv_inst g A t) fun _ => Sim.bind (HVariants.conv_inst g A vs) fun _ =>
      Sim.pure rfl

end

/-- A hint that converts under some aliases converts, under them with type
arguments substituted, to its type with them substituted. -/
theorem Hint.convertWith_inst {θ : List (String × Ty)} (g : Ground θ) {A : Aliases} {h : Hint}
    {t : Ty} (e : h.convertWith A = .ok t) : h.convertWith (A.inst θ) = .ok (t.inst θ) := by
  simp only [Hint.convertWith] at e ⊢
  cases hr : (h.conv A).run {} with
  | error _ => simp [hr, Except.map] at e
  | ok r =>
    obtain ⟨t', s'⟩ := r
    simp only [hr, Except.map, Except.ok.injEq] at e
    subst e
    simp [Hint.conv_inst g A h {} t' s' hr, Except.map]

end Datalove
