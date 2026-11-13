import Datalove.Proofs.Sigs

/-!
# An enum names each variant once

`match` finds a variant's case by name, so that a value reaches exactly one
case rests on an enum's variant names being distinct
(`Proofs/Exhaustive.lean`). The parser refuses an enum hint that names a
variant twice (D038); the conversion keeps that (`Hint.conv_namesOnce`), its
sort only permuting the variants, so every type built from such hints, an
alias's or a signature's, names each variant once.
-/

namespace Datalove

mutual

/-- Every enum in a type names each variant once. -/
def Ty.namesOnce : Ty → Bool
  | .enum vs => (vs.toList.map (·.1)).Nodup && vs.namesOnce
  | .mu b | .list b | .set b | .option b | .result b | .tensor b _ | .term _ b => b.namesOnce
  | .map k v => k.namesOnce && v.namesOnce
  | .tuple ts => ts.namesOnce
  | .struct fs | .table fs => fs.namesOnce
  | _ => true

def Tys.namesOnce : Tys → Bool
  | .nil => true
  | .cons t ts => t.namesOnce && ts.namesOnce

def Fields.namesOnce : Fields → Bool
  | .nil => true
  | .cons _ t fs => t.namesOnce && fs.namesOnce

def Variants.namesOnce : Variants → Bool
  | .nil => true
  | .atom _ vs => vs.namesOnce
  | .term _ t vs => t.namesOnce && vs.namesOnce

end

mutual

/-- Every enum hint names each variant once, as the parser makes them. -/
def Hint.namesOnce : Hint → Bool
  | .enum vs => vs.names.Nodup && vs.namesOnce
  | .mu _ b | .list b | .set b | .option b | .result b | .tensor b _ | .term _ b => b.namesOnce
  | .map k v => k.namesOnce && v.namesOnce
  | .tuple ts => ts.namesOnce
  | .struct fs | .table fs => fs.namesOnce
  | _ => true

def Hints.namesOnce : Hints → Bool
  | .nil => true
  | .cons t ts => t.namesOnce && ts.namesOnce

def HFields.namesOnce : HFields → Bool
  | .nil => true
  | .cons _ t fs => t.namesOnce && fs.namesOnce

def HVariants.namesOnce : HVariants → Bool
  | .nil => true
  | .atom _ vs => vs.namesOnce
  | .term _ t vs => t.namesOnce && vs.namesOnce

/-- The names of an enum hint's variants, in order. -/
def HVariants.names : HVariants → List String
  | .nil => []
  | .atom n vs => n :: vs.names
  | .term n _ vs => n :: vs.names

end

theorem Variants.namesOnce_toList : ∀ (vs : Variants),
    vs.namesOnce = vs.toList.all fun x => x.2.all (·.namesOnce)
  | .nil => rfl
  | .atom _ vs => by simp [Variants.namesOnce, Variants.toList, Variants.namesOnce_toList vs]
  | .term _ t vs => by simp [Variants.namesOnce, Variants.toList, Variants.namesOnce_toList vs]

theorem Variants.sortByName_namesOnce (vs : Variants) :
    vs.sortByName.namesOnce = vs.namesOnce ∧
      ((vs.sortByName.toList.map (·.1)).Nodup ↔ (vs.toList.map (·.1)).Nodup) := by
  simp only [Variants.sortByName, Variants.namesOnce_toList, Variants.toList_ofList]
  have p := List.mergeSort_perm vs.toList (fun a b => !decide (b.1 < a.1))
  exact ⟨p.all_eq, (p.map _).nodup_iff⟩

/-- Converting an enum hint's variants keeps their names, in order. -/
theorem HVariants.conv_names (A : Aliases) : ∀ (vs : HVariants),
    Out (fun ws => ws.toList.map (·.1) = vs.names) (vs.conv A)
  | .nil => by simp only [HVariants.conv]; exact Out.pure rfl
  | .atom n vs => by
    simp only [HVariants.conv]
    exact Out.map (HVariants.conv_names A vs) (fun _ h => by simp [Variants.toList, HVariants.names, h])
  | .term n t vs => by
    simp only [HVariants.conv]
    exact Out.bind Out.any fun _ _ => Out.bind (HVariants.conv_names A vs) fun _ h =>
      Out.pure (by simp [Variants.toList, HVariants.names, h])

open ConvM in
mutual

theorem Hint.conv_namesOnce {A : Aliases} (hA : ∀ p ∈ A, p.2.namesOnce = true) :
    ∀ (h : Hint), h.namesOnce = true → Out (fun t => t.namesOnce = true) (h.conv A)
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .data, _ | .error, _
  | .atom _, _ => by simp only [Hint.conv]; exact Out.pure (by simp [Ty.namesOnce])
  | .tuple fs, h => by
    simp only [Hint.conv]
    exact Out.map (Hints.conv_namesOnce hA fs (by simpa [Hint.namesOnce] using h))
      (fun _ h => by simpa [Ty.namesOnce] using h)
  | .struct fs, h => by
    simp only [Hint.conv]
    exact Out.map (HFields.conv_namesOnce hA fs (by simpa [Hint.namesOnce] using h))
      (fun _ h => by simpa [Ty.namesOnce] using h)
  | .table cs, h => by
    simp only [Hint.conv]
    exact Out.map (HFields.convColumns_namesOnce hA cs (by simpa [Hint.namesOnce] using h))
      (fun _ h => by simpa [Ty.namesOnce] using h)
  | .list t, h | .set t, h | .tensor t _, h => by
    simp only [Hint.conv]
    exact Out.map (Out.collection _ (Hint.conv_namesOnce hA t (by simpa [Hint.namesOnce] using h)))
      (fun _ h => by simpa [Ty.namesOnce] using h)
  | .option t, h | .result t, h | .term _ t, h => by
    simp only [Hint.conv]
    exact Out.map (Hint.conv_namesOnce hA t (by simpa [Hint.namesOnce] using h))
      (fun _ h => by simpa [Ty.namesOnce] using h)
  | .map k v, h => by
    simp only [Hint.namesOnce, Bool.and_eq_true] at h
    simp only [Hint.conv]
    exact Out.bind (Out.collection _ (Hint.conv_namesOnce hA k h.1)) fun _ hk =>
      Out.bind (Out.collection _ (Hint.conv_namesOnce hA v h.2)) fun _ hv =>
        Out.pure (by simp [Ty.namesOnce, hk, hv])
  | .enum vs, h => by
    simp only [Hint.namesOnce, Bool.and_eq_true, decide_eq_true_eq] at h
    simp only [Hint.conv]
    refine Out.map (P := fun ws => ws.namesOnce = true ∧ ws.toList.map (·.1) = vs.names)
      (fun s a s' ha => ⟨HVariants.conv_namesOnce hA vs h.2 s a s' ha,
        HVariants.conv_names A vs s a s' ha⟩) (fun ws hw => ?_)
    obtain ⟨hn, hs⟩ := Variants.sortByName_namesOnce ws
    simp only [Function.comp, Ty.namesOnce, Bool.and_eq_true, decide_eq_true_eq]
    exact ⟨hs.mpr (by rw [hw.2]; exact h.1), by rw [hn]; exact hw.1⟩
  | .mu _ body, h => by
    simp only [Hint.conv]
    exact Out.bind Out.any fun _ _ =>
      Out.bind (Hint.conv_namesOnce hA body (by simpa [Hint.namesOnce] using h)) fun _ hb =>
        Out.bind Out.any fun _ _ => Out.pure (by simpa [Ty.namesOnce] using hb)
  | .name n, _ => by
    simp only [Hint.conv]
    intro s t s' ht
    rcases lookup_ok_aliases ht with ⟨_, _, _, _, _, rfl, -⟩ | ⟨-, hm, -, -⟩
    · simp [Ty.namesOnce]
    · simp only [List.mem_map, Prod.mk.injEq, and_true] at hm
      obtain ⟨p, hp, rfl⟩ := hm
      exact hA p hp

theorem Hints.conv_namesOnce {A : Aliases} (hA : ∀ p ∈ A, p.2.namesOnce = true) :
    ∀ (hs : Hints), hs.namesOnce = true → Out (fun ts => ts.namesOnce = true) (hs.conv A)
  | .nil, _ => by simp only [Hints.conv]; exact Out.pure rfl
  | .cons t ts, h => by
    simp only [Hints.namesOnce, Bool.and_eq_true] at h
    simp only [Hints.conv]
    exact Out.bind (Hint.conv_namesOnce hA t h.1) fun _ ht =>
      Out.bind (Hints.conv_namesOnce hA ts h.2) fun _ hts => Out.pure (by simp [Tys.namesOnce, ht, hts])

theorem HFields.conv_namesOnce {A : Aliases} (hA : ∀ p ∈ A, p.2.namesOnce = true) :
    ∀ (fs : HFields), fs.namesOnce = true → Out (fun ts => ts.namesOnce = true) (fs.conv A)
  | .nil, _ => by simp only [HFields.conv]; exact Out.pure rfl
  | .cons _ t fs, h => by
    simp only [HFields.namesOnce, Bool.and_eq_true] at h
    simp only [HFields.conv]
    exact Out.bind (Hint.conv_namesOnce hA t h.1) fun _ ht =>
      Out.bind (HFields.conv_namesOnce hA fs h.2) fun _ hfs =>
        Out.pure (by simp [Fields.namesOnce, ht, hfs])

theorem HFields.convColumns_namesOnce {A : Aliases} (hA : ∀ p ∈ A, p.2.namesOnce = true) :
    ∀ (fs : HFields), fs.namesOnce = true → Out (fun ts => ts.namesOnce = true) (fs.convColumns A)
  | .nil, _ => by simp only [HFields.convColumns]; exact Out.pure rfl
  | .cons _ t fs, h => by
    simp only [HFields.namesOnce, Bool.and_eq_true] at h
    simp only [HFields.convColumns]
    exact Out.bind (Out.collection _ (Hint.conv_namesOnce hA t h.1)) fun _ ht =>
      Out.bind (HFields.convColumns_namesOnce hA fs h.2) fun _ hfs =>
        Out.pure (by simp [Fields.namesOnce, ht, hfs])

theorem HVariants.conv_namesOnce {A : Aliases} (hA : ∀ p ∈ A, p.2.namesOnce = true) :
    ∀ (vs : HVariants), vs.namesOnce = true → Out (fun ws => ws.namesOnce = true) (vs.conv A)
  | .nil, _ => by simp only [HVariants.conv]; exact Out.pure rfl
  | .atom _ vs, h => by
    simp only [HVariants.namesOnce] at h
    simp only [HVariants.conv]
    exact Out.map (HVariants.conv_namesOnce hA vs h) (fun _ h => by simpa [Variants.namesOnce] using h)
  | .term _ t vs, h => by
    simp only [HVariants.namesOnce, Bool.and_eq_true] at h
    simp only [HVariants.conv]
    exact Out.bind (Hint.conv_namesOnce hA t h.1) fun _ ht =>
      Out.bind (HVariants.conv_namesOnce hA vs h.2) fun _ hvs =>
        Out.pure (by simp [Variants.namesOnce, ht, hvs])

end

/-- A hint whose enums name each variant once converts, under aliases whose
types do the same, to a type that does. -/
theorem Hint.convertWith_namesOnce {A : Aliases} (hA : ∀ p ∈ A, p.2.namesOnce = true) {h : Hint}
    (hh : h.namesOnce = true) {t : Ty} (ok : h.convertWith A = .ok t) : t.namesOnce = true := by
  simp only [Hint.convertWith] at ok
  cases hr : (h.conv A).run {} with
  | error _ => simp [hr, Except.map] at ok
  | ok r =>
    obtain ⟨t', s'⟩ := r
    simp [hr, Except.map] at ok
    subst ok
    exact Hint.conv_namesOnce hA h hh {} t' s' hr

end Datalove
