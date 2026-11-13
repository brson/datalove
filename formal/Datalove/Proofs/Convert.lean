import Datalove.Hint
import Datalove.WF
import Datalove.Proofs.Lift

/-!
# The conversion makes well-formed types

`Hint.convert_wf`: a type `convert_type_hint` accepts is well formed. So the
recursive-type checks the conversion makes as it goes, by name and with a
mutable scope, add up to the conditions `Ty.WF` states by index, and every
type the checker builds from a hint is one `Ty.expose_not_mu` covers.

The proof carries an invariant through the conversion: the `Scope` stays in
step with a `WfCtx` (`Same`), and a binder marked used is one the type built
mentions (`UsedBy`), which is what `unbind`'s check of T062 reads.
-/

namespace Datalove

/-! ## Running the monad -/

theorem bind_ok {α β} {m : ConvM α} {f : α → ConvM β} {s s'' : Scope} {b : β}
    (h : (m >>= f).run s = .ok (b, s'')) :
    ∃ a s', m.run s = .ok (a, s') ∧ (f a).run s' = .ok (b, s'') := by
  simp only [StateT.run, Bind.bind, StateT.bind, Except.bind] at h
  split at h
  · simp at h
  · rename_i r hr
    exact ⟨r.1, r.2, hr, h⟩

theorem map_ok {α β} {m : ConvM α} {f : α → β} {s s' : Scope} {b : β}
    (h : (f <$> m).run s = .ok (b, s')) :
    ∃ a, m.run s = .ok (a, s') ∧ b = f a := by
  simp only [StateT.run, Functor.map, StateT.map, Bind.bind, Except.bind] at h
  split at h
  · simp at h
  · rename_i r hr
    simp [pure, Except.pure] at h
    exact ⟨r.1, by simp only [StateT.run]; rw [hr, ← h.2], h.1.symm⟩

theorem pure_ok {α} {a b : α} {s s' : Scope} (h : (pure a : ConvM α).run s = .ok (b, s')) :
    b = a ∧ s' = s := by
  simp [StateT.run, pure, StateT.pure, Except.pure] at h
  exact ⟨h.1.symm, h.2.symm⟩

/-! ## The invariant -/

/-- The well-formedness context a scope stands for. -/
def Scope.ctx (s : Scope) : WfCtx :=
  { binders := s.binders.map (·.colls), colls := s.colls, inKey := s.inKey }

/-- The scope after converting a part, as it was before but for which
binders are marked used. -/
structure Same (s s' : Scope) : Prop where
  names : s'.binders.map (·.name) = s.binders.map (·.name)
  colls : s'.binders.map (·.colls) = s.binders.map (·.colls)
  depth : s'.colls = s.colls
  key : s'.inKey = s.inKey

theorem Same.refl (s : Scope) : Same s s := ⟨rfl, rfl, rfl, rfl⟩

theorem Same.trans {s₁ s₂ s₃ : Scope} (a : Same s₁ s₂) (b : Same s₂ s₃) : Same s₁ s₃ :=
  ⟨b.names.trans a.names, b.colls.trans a.colls, b.depth.trans a.depth, b.key.trans a.key⟩

theorem Same.ctx {s s' : Scope} (h : Same s s') : s'.ctx = s.ctx := by
  simp [Scope.ctx, h.colls, h.depth, h.key]

/-- Every binder used after a part was converted was used before, or is one
the part mentions. -/
def UsedBy (s s' : Scope) (m : Nat → Bool) : Prop :=
  ∀ d, s'.binders[d]?.map (·.used) = some true →
    s.binders[d]?.map (·.used) = some true ∨ m d = true

theorem UsedBy.refl (s : Scope) (m : Nat → Bool) : UsedBy s s m :=
  fun _ h => Or.inl h

theorem UsedBy.trans {s₁ s₂ s₃ : Scope} {m₁ m₂ : Nat → Bool}
    (a : UsedBy s₁ s₂ m₁) (b : UsedBy s₂ s₃ m₂) : UsedBy s₁ s₃ (fun d => m₁ d || m₂ d) := by
  intro d h
  rcases b d h with h | h
  · rcases a d h with h | h
    · exact Or.inl h
    · exact Or.inr (by simp [h])
  · exact Or.inr (by simp [h])

theorem UsedBy.mono {s s' : Scope} {m m' : Nat → Bool} (a : UsedBy s s' m)
    (h : ∀ d, m d = true → m' d = true) : UsedBy s s' m' := by
  intro d hd
  rcases a d hd with e | e
  · exact Or.inl e
  · exact Or.inr (h d e)

/-- What converting a part establishes. -/
def Sound (s s' : Scope) (m : Nat → Bool) (wf : WfCtx → Bool) : Prop :=
  Same s s' ∧ UsedBy s s' m ∧ wf s.ctx = true

/-- Two parts in sequence. -/
theorem Sound.seq {s₁ s₂ s₃ : Scope} {m₁ m₂ : Nat → Bool} {w₁ w₂ : WfCtx → Bool}
    (a : Sound s₁ s₂ m₁ w₁) (b : Sound s₂ s₃ m₂ w₂) :
    Sound s₁ s₃ (fun d => m₁ d || m₂ d) (fun c => w₁ c && w₂ c) := by
  refine ⟨a.1.trans b.1, a.2.1.trans b.2.1, ?_⟩
  have := b.2.2
  rw [a.1.ctx] at this
  simp [a.2.2, this]

/-! ## The steps -/

/-- The scope inside a collection. -/
def Scope.enter (s : Scope) (key : Bool) : Scope :=
  { s with colls := s.colls + 1, inKey := s.inKey || key }

theorem get_ok {s s' a : Scope} (h : (get : ConvM Scope).run s = .ok (a, s')) : a = s ∧ s' = s := by
  simp [StateT.run, get, getThe, MonadStateOf.get, StateT.get, pure, Except.pure] at h
  exact ⟨h.1.symm, h.2.symm⟩

theorem set_ok {s s' x : Scope} {u : PUnit} (h : (set x : ConvM PUnit).run s = .ok (u, s')) : s' = x := by
  simp [StateT.run, set, StateT.set, pure, Except.pure] at h
  exact h.symm

theorem modify_ok {s s' : Scope} {f : Scope → Scope} {u : PUnit}
    (h : (modify f : ConvM PUnit).run s = .ok (u, s')) : s' = f s := by
  simp [StateT.run, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, pure,
    Except.pure] at h
  exact h.symm

theorem fail_ok {α} {k : ConvErrKind} {n : String} {s s' : Scope} {a : α}
    (h : (ConvM.fail k n : ConvM α).run s = .ok (a, s')) : False := by
  simp [ConvM.fail, throw, throwThe, MonadExceptOf.throw, StateT.run, StateT.lift, Except.bind,
    Bind.bind] at h

theorem fail_bind_ok {α β} {k : ConvErrKind} {n : String} {f : α → ConvM β} {s s' : Scope} {b : β}
    (h : (ConvM.fail k n >>= f).run s = .ok (b, s')) : False := by
  obtain ⟨_, _, h, -⟩ := bind_ok h
  exact fail_ok h

theorem collection_ok {α} {key : Bool} {f : ConvM α} {s s' : Scope} {a : α}
    (h : (ConvM.collection key f).run s = .ok (a, s')) :
    ∃ s₁, f.run (s.enter key) = .ok (a, s₁) ∧
      s' = { s₁ with colls := s₁.colls - 1, inKey := s.inKey } := by
  simp only [ConvM.collection] at h
  obtain ⟨g, s₀, hg, k₁⟩ := bind_ok h
  obtain ⟨eg, es⟩ := get_ok hg
  subst eg es
  obtain ⟨_, s₂, hs, k₂⟩ := bind_ok k₁
  have es₂ := set_ok hs
  subst es₂
  obtain ⟨r, s₃, hf, k₃⟩ := bind_ok k₂
  obtain ⟨_, s₄, hm, k₄⟩ := bind_ok k₃
  have es₄ := modify_ok hm
  subst es₄
  obtain ⟨er, es⟩ := pure_ok k₄
  subst er es
  exact ⟨s₃, hf, rfl⟩

/-- A part converted inside a collection. -/
theorem collection_sound {key : Bool} {f : ConvM Ty} {s s' : Scope} {t : Ty}
    (ih : ∀ s s' t, f.run s = .ok (t, s') → Sound s s' t.mentions t.wfIn)
    (h : (ConvM.collection key f).run s = .ok (t, s')) :
    Sound s s' t.mentions (fun c => t.wfIn (c.coll key)) := by
  obtain ⟨s₁, h₁, rfl⟩ := collection_ok h
  obtain ⟨same, used, wf⟩ := ih _ _ _ h₁
  refine ⟨⟨?_, ?_, ?_, rfl⟩, ?_, ?_⟩
  · exact same.names
  · exact same.colls
  · simp [same.depth, Scope.enter]
  · intro d hd; exact used d hd
  · simpa [Scope.ctx, Scope.enter, WfCtx.coll] using wf

theorem bind_name_ok {A : Aliases} {n : String} {s s' : Scope} {u : Unit}
    (h : (ConvM.bind A n).run s = .ok (u, s')) :
    s.inKey = false ∧ s' = { s with binders := ⟨n, s.colls, false⟩ :: s.binders } := by
  simp only [ConvM.bind] at h
  obtain ⟨g, s₀, hg, k₁⟩ := bind_ok h
  obtain ⟨eg, es⟩ := get_ok hg
  subst eg es
  split at k₁
  · exact (fail_bind_ok k₁).elim
  · rename_i hk
    split at k₁
    · exact (fail_bind_ok k₁).elim
    · exact ⟨by simpa using hk, set_ok k₁⟩

theorem unbind_ok {s s' : Scope} {u : Unit} {b : Binder} {bs : List Binder}
    (h : ConvM.unbind.run s = .ok (u, s')) (hb : s.binders = b :: bs) :
    b.used = true ∧ s' = { s with binders := bs } := by
  simp only [ConvM.unbind] at h
  obtain ⟨g, s₀, hg, k₁⟩ := bind_ok h
  obtain ⟨eg, es⟩ := get_ok hg
  subst eg es
  simp only [hb] at k₁
  obtain ⟨_, s₁, hs, k₂⟩ := bind_ok k₁
  have es₁ := set_ok hs
  subst es₁
  split at k₂
  · exact (fail_ok k₂).elim
  · rename_i hu
    obtain ⟨-, es₂⟩ := pure_ok k₂
    subst es₂
    exact ⟨by simpa using hu, rfl⟩

theorem markUsed_names : ∀ (l : List Binder) (k : Nat),
    (ConvM.markUsed l k).map (·.name) = l.map (·.name)
  | [], _ => rfl
  | _ :: _, 0 => by simp [ConvM.markUsed]
  | _ :: bs, k + 1 => by simp [ConvM.markUsed, markUsed_names bs k]

theorem markUsed_colls : ∀ (l : List Binder) (k : Nat),
    (ConvM.markUsed l k).map (·.colls) = l.map (·.colls)
  | [], _ => rfl
  | _ :: _, 0 => by simp [ConvM.markUsed]
  | _ :: bs, k + 1 => by simp [ConvM.markUsed, markUsed_colls bs k]

theorem markUsed_used : ∀ (l : List Binder) (k d : Nat),
    (ConvM.markUsed l k)[d]?.map (·.used) = some true →
      l[d]?.map (·.used) = some true ∨ d = k
  | [], _, _, h => by simp [ConvM.markUsed] at h
  | _ :: _, 0, 0, _ => Or.inr rfl
  | _ :: _, 0, _ + 1, h => Or.inl (by simpa [ConvM.markUsed] using h)
  | _ :: _, _ + 1, 0, h => Or.inl (by simpa [ConvM.markUsed] using h)
  | _ :: bs, k + 1, d + 1, h => by
    simp only [ConvM.markUsed, List.getElem?_cons_succ] at h
    rcases markUsed_used bs k d h with e | e
    · exact Or.inl (by simpa using e)
    · exact Or.inr (by omega)

theorem lookup_ok {n : String} {s s' : Scope} {t : Ty} (h : (ConvM.lookup [] n).run s = .ok (t, s')) :
    ∃ depth b, s.binders[depth]? = some b ∧ s.inKey = false ∧ b.colls < s.colls ∧
      t = .recvar depth ∧ s' = { s with binders := ConvM.markUsed s.binders depth } := by
  simp only [ConvM.lookup] at h
  obtain ⟨g, s₀, hg, k₁⟩ := bind_ok h
  obtain ⟨eg, es⟩ := get_ok hg
  subst eg es
  split at k₁
  · exact (fail_ok k₁).elim
  · rename_i depth hd
    obtain ⟨hlt, -, -⟩ := List.findIdx?_eq_some_iff_getElem.mp hd
    obtain ⟨_, s₁, hs, k₂⟩ := bind_ok k₁
    have es₁ := set_ok hs
    subst es₁
    split at k₂
    · exact (fail_bind_ok k₂).elim
    · rename_i hk
      split at k₂
      · exact (fail_bind_ok k₂).elim
      · rename_i hc
        obtain ⟨et, es₂⟩ := pure_ok k₂
        subst et es₂
        refine ⟨depth, s₀.binders[depth], by simp [hlt], by simpa using hk, ?_, rfl, rfl⟩
        simp [hlt] at hc
        omega

/-- A name's use under aliases: a `rec`'s, as `lookup_ok` says, or an alias's
type, which the conversion lets through only with no `rec` in it in a key,
and no type parameter in it under a `rec`. -/
theorem lookup_ok_aliases {A : Aliases} {n : String} {s s' : Scope} {t : Ty}
    (h : (ConvM.lookup A n).run s = .ok (t, s')) :
    (∃ depth b, s.binders[depth]? = some b ∧ s.inKey = false ∧ b.colls < s.colls ∧
      t = .recvar depth ∧ s' = { s with binders := ConvM.markUsed s.binders depth }) ∨
    (s' = s ∧ (t, true) ∈ A.map (fun p => (p.2, true)) ∧ (s.inKey = true → t.containsRec = false) ∧
      (s.binders ≠ [] → t.containsVar = false)) := by
  have h0 := h
  simp only [ConvM.lookup] at h
  obtain ⟨g, s₀, hg, k₁⟩ := bind_ok h
  obtain ⟨eg, es⟩ := get_ok hg
  subst eg es
  split at k₁
  · rename_i hn
    split at k₁
    · exact (fail_ok k₁).elim
    · rename_i u hu
      split at k₁
      · exact (fail_ok k₁).elim
      · rename_i hrec
        split at k₁
        · exact (fail_ok k₁).elim
        · rename_i hvar
          have h1 : s₀.inKey = true → u.containsRec = false := fun hk => by simpa [hk] using hrec
          have h2 : s₀.binders ≠ [] → u.containsVar = false := fun hb => by
            have : s₀.binders.isEmpty = false := by simpa using hb
            simpa [this] using hvar
          have hm : (u, true) ∈ A.map (fun p => (p.2, true)) := by
            simp only [List.mem_map]
            exact ⟨(n, u), lookup_mem_of hu, rfl⟩
          obtain ⟨rfl, rfl⟩ := pure_ok k₁
          exact .inr ⟨rfl, hm, h1, h2⟩
  · left
    rename_i depth hd
    obtain ⟨hlt, -, -⟩ := List.findIdx?_eq_some_iff_getElem.mp hd
    obtain ⟨_, s₁, hs, k₂⟩ := bind_ok k₁
    have es₁ := set_ok hs
    subst es₁
    split at k₂
    · exact (fail_bind_ok k₂).elim
    · rename_i hk
      split at k₂
      · exact (fail_bind_ok k₂).elim
      · rename_i hc
        obtain ⟨et, es₂⟩ := pure_ok k₂
        subst et es₂
        refine ⟨depth, s₀.binders[depth], by simp [hlt], by simpa using hk, ?_, rfl, rfl⟩
        simp [hlt] at hc
        omega

/-! ## Sorting an enum's variants -/

def optAll (c : WfCtx) : String × Option Ty → Bool
  | (_, none) => true
  | (_, some t) => t.wfIn c

def optAny (d : Nat) : String × Option Ty → Bool
  | (_, none) => false
  | (_, some t) => t.mentions d

theorem Variants.wfIn_toList : ∀ (vs : Variants) (c : WfCtx), vs.wfIn c = vs.toList.all (optAll c)
  | .nil, _ => by simp [Variants.wfIn, Variants.toList]
  | .atom _ vs, c => by simp [Variants.wfIn, Variants.toList, optAll, Variants.wfIn_toList vs c]
  | .term _ _ vs, c => by simp [Variants.wfIn, Variants.toList, optAll, Variants.wfIn_toList vs c]

theorem Variants.wfIn_ofList : ∀ (l : List (String × Option Ty)) (c : WfCtx),
    (Variants.ofList l).wfIn c = l.all (optAll c)
  | [], _ => by simp [Variants.wfIn, Variants.ofList]
  | (_, none) :: l, c => by simp [Variants.wfIn, Variants.ofList, optAll, Variants.wfIn_ofList l c]
  | (_, some _) :: l, c => by simp [Variants.wfIn, Variants.ofList, optAll, Variants.wfIn_ofList l c]

theorem Variants.mentions_toList : ∀ (vs : Variants) (d : Nat), vs.mentions d = vs.toList.any (optAny d)
  | .nil, _ => by simp [Variants.mentions, Variants.toList]
  | .atom _ vs, d => by simp [Variants.mentions, Variants.toList, optAny, Variants.mentions_toList vs d]
  | .term _ _ vs, d => by simp [Variants.mentions, Variants.toList, optAny, Variants.mentions_toList vs d]

theorem Variants.mentions_ofList : ∀ (l : List (String × Option Ty)) (d : Nat),
    (Variants.ofList l).mentions d = l.any (optAny d)
  | [], _ => by simp [Variants.mentions, Variants.ofList]
  | (_, none) :: l, d => by simp [Variants.mentions, Variants.ofList, optAny, Variants.mentions_ofList l d]
  | (_, some _) :: l, d => by simp [Variants.mentions, Variants.ofList, optAny, Variants.mentions_ofList l d]

theorem sortByName_wfIn (vs : Variants) (c : WfCtx) : vs.sortByName.wfIn c = vs.wfIn c := by
  rw [Variants.sortByName, Variants.wfIn_ofList, Variants.wfIn_toList vs]
  exact (List.mergeSort_perm _ _).all_eq

theorem sortByName_mentions (vs : Variants) (d : Nat) : vs.sortByName.mentions d = vs.mentions d := by
  rw [Variants.sortByName, Variants.mentions_ofList, Variants.mentions_toList vs]
  exact (List.mergeSort_perm _ _).any_eq

/-! ## The conversion -/

mutual

theorem Hint.conv_sound {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) : ∀ (h : Hint) (s s' : Scope) (t : Ty), (h.conv A).run s = .ok (t, s') →
    Sound s s' t.mentions t.wfIn
  | .list e, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := collection_sound (fun s s' t h => Hint.conv_sound hA e s s' t h) ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .set e, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := collection_sound (fun s s' t h => Hint.conv_sound hA e s s' t h) ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .tensor e r, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := collection_sound (fun s s' t h => Hint.conv_sound hA e s s' t h) ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .map k v, s, s', t, h => by
    obtain ⟨a, s₁, h₁, h⟩ := bind_ok h
    obtain ⟨b, s₂, h₂, h⟩ := bind_ok h
    obtain ⟨rfl, rfl⟩ := pure_ok h
    have sk := collection_sound (fun s s' t h => Hint.conv_sound hA k s s' t h) h₁
    have sv := collection_sound (fun s s' t h => Hint.conv_sound hA v s s' t h) h₂
    have := sk.seq sv
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .option e, s, s', t, h | .result e, s, s', t, h | .term _ e, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := Hint.conv_sound hA e _ _ _ ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .tuple fs, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := Hints.conv_sound hA fs _ _ _ ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .struct fs, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := HFields.conv_sound hA fs _ _ _ ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .table cs, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := HFields.convColumns_sound hA cs _ _ _ ha
    exact ⟨this.1, this.2.1.mono (by simp [Ty.mentions]), by simpa [Ty.wfIn] using this.2.2⟩
  | .enum vs, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := HVariants.conv_sound hA vs _ _ _ ha
    refine ⟨this.1, this.2.1.mono ?_, ?_⟩
    · intro d hd; simpa [Ty.mentions, sortByName_mentions] using hd
    · simpa [Ty.wfIn, sortByName_wfIn] using this.2.2
  | .mu n body, s, s', t, h => by
    obtain ⟨_, s₁, h₁, h⟩ := bind_ok h
    obtain ⟨hk, rfl⟩ := bind_name_ok h₁
    obtain ⟨b, s₂, h₂, h⟩ := bind_ok h
    obtain ⟨_, s₃, h₃, h⟩ := bind_ok h
    obtain ⟨rfl, rfl⟩ := pure_ok h
    obtain ⟨same, used, wf⟩ := Hint.conv_sound hA body _ _ _ h₂
    -- The scope after the body has the binder `bind` pushed, at its head.
    obtain ⟨b₀, bs, hb⟩ : ∃ b₀ bs, s₂.binders = b₀ :: bs := by
      have := congrArg List.length same.names
      simp at this
      cases e : s₂.binders with
      | nil => simp [e] at this
      | cons b₀ bs => exact ⟨b₀, bs, rfl⟩
    obtain ⟨hu, rfl⟩ := unbind_ok h₃ hb
    have names := same.names
    have colls := same.colls
    rw [hb] at names colls
    simp at names colls
    refine ⟨⟨names.2, colls.2, same.depth, same.key⟩, ?_, ?_⟩
    · intro d hd
      have := used (d + 1) (by simpa [hb] using hd)
      simpa [Ty.mentions] using this
    · have m0 : b.mentions 0 = true := by
        have := used 0 (by simp [hb, hu])
        simpa using this
      have wf' : b.wfIn { s.ctx with binders := s.colls :: s.ctx.binders } = true := by
        simpa [Scope.ctx] using wf
      simp [Ty.wfIn, Scope.ctx, hk, m0] at wf' ⊢
      exact wf'
  | .name n, s, s', t, h => by
    rcases lookup_ok_aliases h with ⟨depth, b, hb, hk, hc, rfl, rfl⟩ | ⟨rfl, hm, hk, hv⟩
    · refine ⟨⟨markUsed_names _ _, markUsed_colls _ _, rfl, rfl⟩, ?_, ?_⟩
      · intro d hd
        rcases markUsed_used _ _ _ hd with e | e
        · exact Or.inl e
        · exact Or.inr (by simp [Ty.mentions, e])
      · simp [Ty.wfIn, Scope.ctx, hb, hk, hc]
    · simp only [List.mem_map] at hm
      obtain ⟨p, hp, hpe⟩ := hm
      simp only [Prod.mk.injEq, and_true] at hpe
      have wf : t.WF := hpe ▸ hA p hp
      refine ⟨Same.refl _, UsedBy.refl _ _, ?_⟩
      exact wf.lift _ (fun x => hk (by simpa [Scope.ctx] using x))
        (fun x => hv (by simpa [Scope.ctx] using x))
  | .bool, s, s', t, h | .u8, s, s', t, h | .i8, s, s', t, h | .u16, s, s', t, h
  | .i16, s, s', t, h | .u32, s, s', t, h | .i32, s, s', t, h | .u64, s, s', t, h
  | .i64, s, s', t, h | .index, s, s', t, h | .offset, s, s', t, h | .f32, s, s', t, h
  | .f64, s, s', t, h | .int, s, s', t, h | .string, s, s', t, h | .data, s, s', t, h
  | .error, s, s', t, h | .atom _, s, s', t, h => by
    obtain ⟨rfl, rfl⟩ := pure_ok h
    exact ⟨Same.refl _, UsedBy.refl _ _, by simp [Ty.wfIn]⟩

theorem Hints.conv_sound {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) : ∀ (h : Hints) (s s' : Scope) (t : Tys), (h.conv A).run s = .ok (t, s') →
    Sound s s' t.mentions t.wfIn
  | .nil, s, s', t, h => by
    obtain ⟨rfl, rfl⟩ := pure_ok h
    exact ⟨Same.refl _, UsedBy.refl _ _, by simp [Tys.wfIn]⟩
  | .cons e es, s, s', t, h => by
    obtain ⟨a, s₁, h₁, h⟩ := bind_ok h
    obtain ⟨as, s₂, h₂, h⟩ := bind_ok h
    obtain ⟨rfl, rfl⟩ := pure_ok h
    have := (Hint.conv_sound hA e _ _ _ h₁).seq (Hints.conv_sound hA es _ _ _ h₂)
    exact ⟨this.1, this.2.1.mono (by simp [Tys.mentions]), by simpa [Tys.wfIn] using this.2.2⟩

theorem HFields.conv_sound {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) : ∀ (h : HFields) (s s' : Scope) (t : Fields), (h.conv A).run s = .ok (t, s') →
    Sound s s' t.mentions t.wfIn
  | .nil, s, s', t, h => by
    obtain ⟨rfl, rfl⟩ := pure_ok h
    exact ⟨Same.refl _, UsedBy.refl _ _, by simp [Fields.wfIn]⟩
  | .cons _ e es, s, s', t, h => by
    obtain ⟨a, s₁, h₁, h⟩ := bind_ok h
    obtain ⟨as, s₂, h₂, h⟩ := bind_ok h
    obtain ⟨rfl, rfl⟩ := pure_ok h
    have := (Hint.conv_sound hA e _ _ _ h₁).seq (HFields.conv_sound hA es _ _ _ h₂)
    exact ⟨this.1, this.2.1.mono (by simp [Fields.mentions]), by simpa [Fields.wfIn] using this.2.2⟩

theorem HFields.convColumns_sound {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) : ∀ (h : HFields) (s s' : Scope) (t : Fields),
    (h.convColumns A).run s = .ok (t, s') → Sound s s' t.mentions t.wfColumnsIn
  | .nil, s, s', t, h => by
    obtain ⟨rfl, rfl⟩ := pure_ok h
    exact ⟨Same.refl _, UsedBy.refl _ _, by simp [Fields.wfColumnsIn]⟩
  | .cons _ e es, s, s', t, h => by
    obtain ⟨a, s₁, h₁, h⟩ := bind_ok h
    obtain ⟨as, s₂, h₂, h⟩ := bind_ok h
    obtain ⟨rfl, rfl⟩ := pure_ok h
    have := (collection_sound (fun s s' t h => Hint.conv_sound hA e s s' t h) h₁).seq
      (HFields.convColumns_sound hA es _ _ _ h₂)
    exact ⟨this.1, this.2.1.mono (by simp [Fields.mentions]),
      by simpa [Fields.wfColumnsIn] using this.2.2⟩

theorem HVariants.conv_sound {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) : ∀ (h : HVariants) (s s' : Scope) (t : Variants),
    (h.conv A).run s = .ok (t, s') → Sound s s' t.mentions t.wfIn
  | .nil, s, s', t, h => by
    obtain ⟨rfl, rfl⟩ := pure_ok h
    exact ⟨Same.refl _, UsedBy.refl _ _, by simp [Variants.wfIn]⟩
  | .atom _ vs, s, s', t, h => by
    obtain ⟨a, ha, rfl⟩ := map_ok h
    have := HVariants.conv_sound hA vs _ _ _ ha
    exact ⟨this.1, this.2.1.mono (by simp [Variants.mentions]), by simpa [Variants.wfIn] using this.2.2⟩
  | .term _ e vs, s, s', t, h => by
    obtain ⟨a, s₁, h₁, h⟩ := bind_ok h
    obtain ⟨as, s₂, h₂, h⟩ := bind_ok h
    obtain ⟨rfl, rfl⟩ := pure_ok h
    have := (Hint.conv_sound hA e _ _ _ h₁).seq (HVariants.conv_sound hA vs _ _ _ h₂)
    exact ⟨this.1, this.2.1.mono (by simp [Variants.mentions]), by simpa [Variants.wfIn] using this.2.2⟩

end

/-- A type the conversion accepts is well formed. -/
theorem Hint.convert_wf {h : Hint} {t : Ty} (ok : h.convert = .ok t) : t.WF := by
  simp only [Hint.convert, Except.map] at ok
  split at ok
  · simp at ok
  · rename_i r hr
    simp at ok
    subst ok
    have := (Hint.conv_sound (A := []) (fun _ h => by cases h) h {} r.2 r.1
      (by simpa [StateT.run] using hr)).2.2
    simpa [Ty.WF, Scope.ctx] using this

/-- A type the conversion accepts under aliases that name well-formed types
is well formed. -/
theorem Hint.convertWith_wf {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) {h : Hint} {t : Ty}
    (ok : h.convertWith A = .ok t) : t.WF := by
  simp only [Hint.convertWith, Except.map] at ok
  split at ok
  · simp at ok
  · rename_i r hr
    simp at ok
    subst ok
    have := (Hint.conv_sound hA h {} r.2 r.1 (by simpa [StateT.run] using hr)).2.2
    simpa [Ty.WF, Scope.ctx] using this

end Datalove
