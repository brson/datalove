import Datalove.Proofs.Sigs

/-!
# An enum names each variant once, in order

`match` finds a variant's case by name, so that a value reaches exactly one
case rests on an enum's variant names being distinct
(`Proofs/Exhaustive.lean`). The parser refuses an enum hint that names a
variant twice (D038); the conversion sorts the variants by name, so every
type built from such hints, an alias's or a signature's, names its variants
in strictly increasing order, and so each once (`Hint.conv_namesOnce`).
That order is what makes a type's equality structural: an enum's variants
written in another order convert to the same type
(`Variants.sortByName_perm`).
-/

/-- Not after, as the conversion's sort compares names, is transitive. -/
theorem String.not_lt_trans {a b c : String} (h1 : ¬b < a) (h2 : ¬c < b) : ¬c < a := by
  intro h
  rcases Std.lt_trichotomy a b with hab | rfl | hba
  · exact h2 (String.lt_trans h hab)
  · exact h2 h
  · exact h1 hba

namespace Datalove

/-- How the conversion orders two variants: the first not after the second. -/
abbrev byName (a b : String × Option Ty) : Bool := !decide (b.1 < a.1)

/-- Sorting variants whose names are distinct leaves the names strictly
increasing. -/
theorem mergeSort_byName_strict (l : List (String × Option Ty)) (hn : (l.map (·.1)).Nodup) :
    (l.mergeSort byName).Pairwise (fun a b => a.1 < b.1) := by
  have hs := List.pairwise_mergeSort (le := byName)
    (fun a b c h1 h2 => by simpa using String.not_lt_trans (by simpa using h1) (by simpa using h2))
    (fun a b => by
      by_cases h : b.1 < a.1
      · simp [String.lt_asymm h]
      · simp [h])
    l
  have hd := ((List.mergeSort_perm l byName).map (·.1)).nodup_iff.mpr hn
  rw [List.nodup_iff_pairwise_ne, List.pairwise_map] at hd
  refine (hs.and hd).imp fun {a b} ⟨h1, h2⟩ => ?_
  rcases Std.lt_trichotomy a.1 b.1 with h | h | h
  · exact h
  · exact absurd h h2
  · simp [h] at h1

/-- Sorting variants by name leaves their names strictly increasing, if no
name is written twice. -/
theorem Variants.sortByName_sorted (vs : Variants) (hn : (vs.toList.map (·.1)).Nodup) :
    (vs.sortByName.toList.map (·.1)).Pairwise (· < ·) := by
  simp only [Variants.sortByName, Variants.toList_ofList]
  rw [List.pairwise_map]
  exact mergeSort_byName_strict _ hn

/-- The order an enum's variants are written in does not matter: two lists of
the same variants, no name twice, sort to the same one. -/
theorem Variants.sortByName_perm {vs ws : Variants} (p : vs.toList.Perm ws.toList)
    (hn : (vs.toList.map (·.1)).Nodup) : vs.sortByName = ws.sortByName := by
  simp only [Variants.sortByName]
  congr 1
  refine List.Perm.eq_of_pairwise (le := fun a b => a.1 < b.1)
    (fun a b _ _ h1 h2 => absurd h2 (String.lt_asymm h1))
    (mergeSort_byName_strict _ hn) (mergeSort_byName_strict _ ((p.map _).nodup_iff.mp hn)) ?_
  exact ((List.mergeSort_perm _ _).trans p).trans (List.mergeSort_perm _ _).symm


mutual

/-- Every enum in a type names its variants in strictly increasing order, and
so each once. -/
def Ty.namesOnce : Ty → Bool
  | .enum vs => (vs.toList.map (·.1)).Pairwise (· < ·) && vs.namesOnce
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
    have hn := (Variants.sortByName_namesOnce ws).1
    simp only [Function.comp, Ty.namesOnce, Bool.and_eq_true, decide_eq_true_eq]
    exact ⟨Variants.sortByName_sorted ws (by rw [hw.2]; exact h.1), by rw [hn]; exact hw.1⟩
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
types name theirs in order, to a type that does. -/
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

/-- An enum of a type that names its variants in order names each once. -/
theorem Ty.namesOnce_nodup {vs : Variants} (h : (Ty.enum vs).namesOnce = true) :
    (vs.toList.map (·.1)).Nodup := by
  simp only [Ty.namesOnce, Bool.and_eq_true, decide_eq_true_eq] at h
  rw [List.nodup_iff_pairwise_ne]
  exact h.1.imp fun {a b} (h : a < b) (e : a = b) => String.lt_irrefl a (by rw [e] at h ⊢; exact h)

end Datalove
