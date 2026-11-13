import Datalove.Proofs.InstanceStmt

/-!
# A signature names only its own type parameters

`Fun.checkFun_instance` assumes it (`SigsOk`); here it is proved of what
`resolveNames` builds (`Fun.resolveNames_sigsOk`), so the theorem holds of a
script with that assumption discharged (`Fun.checkFun_instance_script`).

A hint names a type parameter only through an alias, so a converted type's
type parameters are among those of the aliases it was converted under
(`Hint.conv_allVars`). A script's aliases name none, being converted with
no type parameter in scope, and a signature is converted with its own type
parameters added, each naming itself.
-/

namespace Datalove

/-- What a conversion gives, wherever it succeeds, `P` holds of. -/
def Out {α} (P : α → Prop) (m : ConvM α) : Prop := ∀ s a s', m.run s = .ok (a, s') → P a

namespace Out

variable {α β : Type} {P : α → Prop} {Q : β → Prop}

theorem pure {a : α} (h : P a) : Out P (Pure.pure a : ConvM α) := by
  intro s b s' hb
  obtain ⟨rfl, -⟩ := pure_ok hb
  exact h

theorem bind {x : ConvM α} {f : α → ConvM β} (hx : Out P x) (hf : ∀ a, P a → Out Q (f a)) :
    Out Q (x >>= f) := by
  intro s b s'' hb
  obtain ⟨a, s', ha, hfa⟩ := bind_ok hb
  exact hf a (hx s a s' ha) s' b s'' hfa

theorem any {m : ConvM α} : Out (fun _ => True) m := fun _ _ _ _ => trivial

theorem map {x : ConvM α} {f : α → β} (hx : Out P x) (hf : ∀ a, P a → Q (f a)) : Out Q (f <$> x) := by
  rw [map_eq_pure_bind]
  exact bind hx (fun a ha => pure (hf a ha))

theorem collection {m : ConvM α} (key : Bool) (h : Out P m) : Out P (ConvM.collection key m) := by
  simp only [ConvM.collection]
  exact bind any fun _ _ => bind any fun _ _ => bind h fun r hr => bind any fun _ _ => pure hr

end Out

theorem Out.lookup {P : String → Bool} {A : Aliases} (hA : ∀ p ∈ A, p.2.allVars P = true)
    (n : String) : Out (fun t => t.allVars P = true) (ConvM.lookup A n) := by
  intro s t s' h
  simp only [ConvM.lookup] at h
  obtain ⟨g₀, s₀, hg, k⟩ := bind_ok h
  obtain ⟨e1, e2⟩ := get_ok hg
  subst e1 e2
  cases hf : List.findIdx? (fun x => x.name == n) s₀.binders with
  | none =>
    simp only [hf] at k
    cases hu : A.lookup n with
    | none => simp only [hu] at k; exact (fail_ok k).elim
    | some u =>
      simp only [hu] at k
      split at k
      · exact (fail_ok k).elim
      · split at k
        · exact (fail_ok k).elim
        · obtain ⟨rfl, -⟩ := pure_ok k
          exact hA _ (Fun.lookup_mem hu)
  | some d =>
    simp only [hf] at k
    obtain ⟨_, s₁, -, k₂⟩ := bind_ok k
    split at k₂
    · exact (fail_bind_ok k₂).elim
    · split at k₂
      · exact (fail_bind_ok k₂).elim
      · obtain ⟨rfl, -⟩ := pure_ok k₂
        simp [Ty.allVars]

/-- `Variants.allVars` as a test of each payload. -/
theorem Variants.allVars_toList (P : String → Bool) : ∀ (vs : Variants),
    vs.allVars P = vs.toList.all fun x => x.2.all (·.allVars P)
  | .nil => rfl
  | .atom _ vs => by simp [Variants.allVars, Variants.toList, Variants.allVars_toList P vs]
  | .term _ t vs => by simp [Variants.allVars, Variants.toList, Variants.allVars_toList P vs]

theorem Variants.toList_ofList : ∀ (l : List (String × Option Ty)), (Variants.ofList l).toList = l
  | [] => rfl
  | (_, none) :: l => by simp [Variants.ofList, Variants.toList, Variants.toList_ofList l]
  | (_, some _) :: l => by simp [Variants.ofList, Variants.toList, Variants.toList_ofList l]

theorem Variants.allVars_sortByName (P : String → Bool) (vs : Variants) :
    vs.sortByName.allVars P = vs.allVars P := by
  rw [Variants.allVars_toList, Variants.allVars_toList, Variants.sortByName, Variants.toList_ofList]
  exact (List.mergeSort_perm _ _).all_eq

open ConvM in
mutual

theorem Hint.conv_allVars {P : String → Bool} {A : Aliases} (hA : ∀ p ∈ A, p.2.allVars P = true) :
    ∀ (h : Hint), Out (fun t => t.allVars P = true) (h.conv A)
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset
  | .f32 | .f64 | .int | .string | .data | .error | .atom _ => by
    simp only [Hint.conv]; exact Out.pure (by simp [Ty.allVars])
  | .tuple fs => by
    simp only [Hint.conv]
    exact Out.map (Hints.conv_allVars hA fs) (fun _ h => by simpa [Ty.allVars] using h)
  | .struct fs => by
    simp only [Hint.conv]
    exact Out.map (HFields.conv_allVars hA fs) (fun _ h => by simpa [Ty.allVars] using h)
  | .table cs => by
    simp only [Hint.conv]
    exact Out.map (HFields.convColumns_allVars hA cs) (fun _ h => by simpa [Ty.allVars] using h)
  | .list t | .set t | .tensor t _ => by
    simp only [Hint.conv]
    exact Out.map (Out.collection _ (Hint.conv_allVars hA t)) (fun _ h => by simpa [Ty.allVars] using h)
  | .option t | .result t | .term _ t => by
    simp only [Hint.conv]
    exact Out.map (Hint.conv_allVars hA t) (fun _ h => by simpa [Ty.allVars] using h)
  | .map k v => by
    simp only [Hint.conv]
    exact Out.bind (Out.collection _ (Hint.conv_allVars hA k)) fun _ hk =>
      Out.bind (Out.collection _ (Hint.conv_allVars hA v)) fun _ hv =>
        Out.pure (by simp [Ty.allVars, hk, hv])
  | .enum vs => by
    simp only [Hint.conv]
    exact Out.map (HVariants.conv_allVars hA vs)
      (fun _ h => by simpa [Ty.allVars, Variants.allVars_sortByName] using h)
  | .mu _ body => by
    simp only [Hint.conv]
    exact Out.bind Out.any fun _ _ => Out.bind (Hint.conv_allVars hA body) fun _ hb =>
      Out.bind Out.any fun _ _ => Out.pure (by simpa [Ty.allVars] using hb)
  | .name n => by simp only [Hint.conv]; exact Out.lookup hA n

theorem Hints.conv_allVars {P : String → Bool} {A : Aliases} (hA : ∀ p ∈ A, p.2.allVars P = true) :
    ∀ (hs : Hints), Out (fun ts => ts.allVars P = true) (hs.conv A)
  | .nil => by simp only [Hints.conv]; exact Out.pure rfl
  | .cons t ts => by
    simp only [Hints.conv]
    exact Out.bind (Hint.conv_allVars hA t) fun _ ht => Out.bind (Hints.conv_allVars hA ts) fun _ hts =>
      Out.pure (by simp [Tys.allVars, ht, hts])

theorem HFields.conv_allVars {P : String → Bool} {A : Aliases} (hA : ∀ p ∈ A, p.2.allVars P = true) :
    ∀ (fs : HFields), Out (fun ts => ts.allVars P = true) (fs.conv A)
  | .nil => by simp only [HFields.conv]; exact Out.pure rfl
  | .cons _ t fs => by
    simp only [HFields.conv]
    exact Out.bind (Hint.conv_allVars hA t) fun _ ht => Out.bind (HFields.conv_allVars hA fs) fun _ hfs =>
      Out.pure (by simp [Fields.allVars, ht, hfs])

theorem HFields.convColumns_allVars {P : String → Bool} {A : Aliases}
    (hA : ∀ p ∈ A, p.2.allVars P = true) :
    ∀ (fs : HFields), Out (fun ts => ts.allVars P = true) (fs.convColumns A)
  | .nil => by simp only [HFields.convColumns]; exact Out.pure rfl
  | .cons _ t fs => by
    simp only [HFields.convColumns]
    exact Out.bind (Out.collection _ (Hint.conv_allVars hA t)) fun _ ht =>
      Out.bind (HFields.convColumns_allVars hA fs) fun _ hfs =>
        Out.pure (by simp [Fields.allVars, ht, hfs])

theorem HVariants.conv_allVars {P : String → Bool} {A : Aliases} (hA : ∀ p ∈ A, p.2.allVars P = true) :
    ∀ (vs : HVariants), Out (fun ts => ts.allVars P = true) (vs.conv A)
  | .nil => by simp only [HVariants.conv]; exact Out.pure rfl
  | .atom _ vs => by
    simp only [HVariants.conv]
    exact Out.map (HVariants.conv_allVars hA vs) (fun _ h => by simpa [Variants.allVars] using h)
  | .term _ t vs => by
    simp only [HVariants.conv]
    exact Out.bind (Hint.conv_allVars hA t) fun _ ht => Out.bind (HVariants.conv_allVars hA vs) fun _ hvs =>
      Out.pure (by simp [Variants.allVars, ht, hvs])

end

theorem convertIn_allVars {P : String → Bool} {A : Aliases} (hA : ∀ p ∈ A, p.2.allVars P = true)
    {h : Hint} {t : Ty} (e : Fun.convertIn A h = .ok t) : t.allVars P = true := by
  simp only [Fun.convertIn, Hint.convertWith] at e
  cases hr : (h.conv A).run {} with
  | error _ => simp [hr, Except.map] at e
  | ok r =>
    obtain ⟨t', s'⟩ := r
    simp [hr, Except.map] at e
    subst e
    exact Hint.conv_allVars hA h {} t' s' hr

end Datalove

namespace Datalove.Fun

theorem mapM_ok_mem {α β ε} {f : α → Except ε β} : ∀ {l : List α} {r : List β},
    l.mapM f = .ok r → ∀ y ∈ r, ∃ x ∈ l, f x = .ok y
  | [], r, h, y, hy => by simp [pure, Except.pure] at h; subst h; simp at hy
  | x :: l, r, h, y, hy => by
    rw [List.mapM_cons] at h
    cases hx : f x with
    | error e => simp [hx, bind, Except.bind] at h
    | ok b =>
      cases hl : l.mapM f with
      | error e => simp [hx, hl, bind, Except.bind] at h
      | ok bs =>
        simp [hx, hl, bind, Except.bind, pure, Except.pure] at h
        subst h
        simp at hy
        rcases hy with rfl | hy
        · exact ⟨x, List.mem_cons_self, hx⟩
        · obtain ⟨z, hz, hfz⟩ := mapM_ok_mem hl y hy
          exact ⟨z, List.mem_cons_of_mem _ hz, hfz⟩

/-- Aliases that name no type parameter, as a script's are. -/
def AliasOk (A : Aliases) : Prop := ∀ p ∈ A, p.2.allVars (fun _ => false) = true

theorem tparamAliases_ok {tps : List (String × Option Bound)} {A : Aliases} (hA : AliasOk A) :
    ∀ p ∈ tparamAliases tps A, p.2.allVars (fun n => tps.any (·.1 == n)) = true := by
  intro p hp
  simp only [tparamAliases, List.mem_append, List.mem_map] at hp
  rcases hp with ⟨⟨n, b⟩, hm, rfl⟩ | hp
  · simp only [Ty.allVars, List.any_eq_true, beq_iff_eq]
    exact ⟨(n, b), hm, rfl⟩
  · exact Ty.allVars_mono (fun _ h => by cases h) _ (hA p hp)

theorem sigHint_ok {A : Aliases} (hA : AliasOk A) {tps : List (String × Option Bound)} {h : Hint}
    {t : Ty} (e : sigHint A tps h = .ok t) : t.allVars (fun n => tps.any (·.1 == n)) = true := by
  simp only [sigHint] at e
  cases hc : convertIn (tparamAliases tps A) h with
  | error _ => simp [hc, bind, Except.bind] at e
  | ok u =>
    simp only [hc, bind, Except.bind] at e
    split at e
    · simp [throw, throwThe, MonadExceptOf.throw] at e
    · simp [pure, Except.pure] at e; subst e
      exact convertIn_allVars (tparamAliases_ok hA) hc

theorem sigOf_ok {A : Aliases} (hA : AliasOk A) {f : FunDecl} {sig : Sig} (h : sigOf A f = .ok sig) :
    (∀ mt ∈ sig.params, mt.2.allVars (fun n => sig.tparams.any (·.1 == n)) = true) ∧
    (∀ r, sig.ret = .ty r → r.allVars (fun n => sig.tparams.any (·.1 == n)) = true) := by
  obtain ⟨name, tps, params, ret, body⟩ := f
  simp only [sigOf] at h
  revert h
  generalize hp : (List.mapM (m := Except Lit.Code) _ params) = res
  intro h
  cases res with
  | error _ => simp [bind, Except.bind] at h
  | ok ps =>
    simp only [bind, Except.bind] at h
    have hps : ∀ mt ∈ ps, mt.2.allVars (fun n => tps.any (·.1 == n)) = true := by
      intro mt hmt
      obtain ⟨⟨m, x, hh⟩, -, he⟩ := mapM_ok_mem hp mt hmt
      cases hs : sigHint A tps hh with
      | error _ => simp [hs, bind, Except.bind] at he
      | ok t =>
        simp [hs, bind, Except.bind, pure, Except.pure] at he
        subst he
        exact sigHint_ok hA hs
    cases ret with
    | none =>
      simp [pure, Except.pure] at h; subst h
      exact ⟨hps, fun r hr => by cases hr⟩
    | some hh =>
      cases hs : sigHint A tps hh with
      | error _ => simp [hs, Functor.map, Except.map] at h
      | ok t =>
        simp [hs, Functor.map, Except.map, pure, Except.pure] at h; subst h
        exact ⟨hps, fun r hr => by cases hr; exact sigHint_ok hA hs⟩

theorem forIn_id_inv {α σ} {I : σ → Prop} : ∀ {l : List α} {b : σ} {f : α → σ → Id (ForInStep σ)},
    I b → (∀ a ∈ l, ∀ b, I b → I (f a b).run.value) → I (forIn l b f).run
  | [], b, f, hb, _ => hb
  | a :: l, b, f, hb, hf => by
    rw [List.forIn_cons]
    have h1 := hf a List.mem_cons_self b hb
    show I (match f a b with
      | ForInStep.done b => (pure b : Id σ)
      | ForInStep.yield b => forIn l b f).run
    revert h1
    cases f a b with
    | done b' => exact id
    | yield b' => exact fun h1 => forIn_id_inv h1 (fun x hx => hf x (List.mem_cons_of_mem _ hx))

theorem defineAlias_ok {A A' : Aliases} {n : String} {h : Hint} (hA : AliasOk A)
    (e : defineAlias A n h = .ok A') : AliasOk A' := by
  simp only [defineAlias] at e
  split at e
  · cases e
  · cases hc : convertIn A h with
    | error _ => simp [hc, Functor.map, Except.map] at e
    | ok t =>
      simp [hc, Functor.map, Except.map] at e
      subst e
      intro p hp
      simp only [List.mem_append, List.mem_singleton] at hp
      rcases hp with hp | rfl
      · exact hA p hp
      · exact convertIn_allVars hA hc

theorem lookup_append_one {β} (l : List (String × β)) (n f : String) (b : β) :
    (l ++ [(n, b)]).lookup f = (l.lookup f).or (if f = n then some b else none) := by
  induction l with
  | nil =>
    by_cases h : f = n
    · simp [List.lookup, h]
    · have : (f == n) = false := by simpa using h
      simp [List.lookup, h, this]
  | cons p ps ih =>
    obtain ⟨m, c⟩ := p
    by_cases h : f = m
    · subst h; simp [List.lookup]
    · have : (f == m) = false := by simpa using h
      simp only [List.cons_append, List.lookup, this, ih]

theorem SigsOk.append {sigs : List (String × Sig)} (h : SigsOk sigs) {n : String} {sig : Sig}
    (hs : (∀ mt ∈ sig.params, mt.2.allVars (fun x => sig.tparams.any (·.1 == x)) = true) ∧
      (∀ r, sig.ret = .ty r → r.allVars (fun x => sig.tparams.any (·.1 == x)) = true)) :
    SigsOk (sigs ++ [(n, sig)]) := by
  intro f sig' hl
  rw [lookup_append_one] at hl
  cases hf : sigs.lookup f with
  | some x =>
    simp only [hf, Option.some_or, Option.some.injEq] at hl
    subst hl
    exact h f x hf
  | none =>
    simp only [hf, Option.none_or] at hl
    split at hl
    · cases hl; exact hs
    · cases hl

/-- Every signature `resolveNames` builds names only its own type
parameters. -/
theorem resolveNames_sigsOk (ss : List Stmt) : SigsOk (resolveNames ss).1 := by
  have empty : SigsOk [] := fun _ _ h => by cases h
  simp only [resolveNames, Id.run, bind, pure]
  refine (forIn_id_inv
    (I := fun (st : Aliases × List (String × Sig) × Array Lit.Code) => AliasOk st.1 ∧ SigsOk st.2.1)
    ⟨fun _ h => absurd h List.not_mem_nil, empty⟩ ?_).2
  intro s _ st ⟨hA, hS⟩
  obtain ⟨A, sigs, d⟩ := st
  cases s
  case type_ n h =>
    simp only
    split
    · rename_i as hd; exact ⟨defineAlias_ok hA hd, hS⟩
    · exact ⟨hA, hS⟩
  case fun_ f =>
    simp only
    split
    · exact ⟨hA, hS⟩
    · split
      · rename_i sig hsig; exact ⟨hA, hS.append (sigOf_ok hA hsig)⟩
      · exact ⟨hA, hS⟩
  all_goals exact ⟨hA, hS⟩

/-- **Checking a generic once is enough**, of a script: `checkFun_instance`
with its signatures as `resolveNames` builds them, which name only their own
type parameters. -/
theorem checkFun_instance_script {θ : List (String × Ty)} (g : Ground θ) {env' : InferEnv}
    (H : InstEnv θ inferEnv env') (ss : List Stmt) (aliases : Aliases)
    (consts : List (String × Ty × Kind)) (name : String)
    (tps : List (String × Option Bound)) (params : List (Mode × String × Hint)) (ret : Option Hint)
    (body : List Stmt) (sig : Sig) (hsig : (resolveNames ss).1.lookup name = some sig)
    (hb : ∀ p b, (p, some b) ∈ tps → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true)
    (tf : blockTypeFree body = true)
    (quiet : SQuiet (checkFun (resolveNames ss).1 aliases consts (.mk name tps params ret body)) ()) :
    SQuiet (checkBlockF env' fuel ((funCtx (resolveNames ss).1 aliases consts tps params sig).inst θ) body
      >>= fun _ => reportIf (match sig.ret.inst θ with
        | .ty _ => completes body
        | .void => false) "F063") () :=
  checkFun_instance g H _ (resolveNames_sigsOk ss) aliases consts name tps params ret body sig hsig hb tf quiet

/-! ## Signatures are well formed -/

theorem tparamAliases_wf {tps : List (String × Option Bound)} {A : Aliases}
    (hA : ∀ p ∈ A, p.2.WF) : ∀ p ∈ tparamAliases tps A, p.2.WF := by
  intro p hp
  simp only [tparamAliases, List.mem_append, List.mem_map] at hp
  rcases hp with ⟨⟨n, _⟩, _, rfl⟩ | hp
  · rfl
  · exact hA p hp

theorem sigHint_wf {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) {tps : List (String × Option Bound)}
    {h : Hint} {t : Ty} (e : sigHint A tps h = .ok t) : t.WF := by
  simp only [sigHint] at e
  cases hc : convertIn (tparamAliases tps A) h with
  | error _ => simp [hc, bind, Except.bind] at e
  | ok u =>
    simp only [hc, bind, Except.bind] at e
    split at e
    · simp [throw, throwThe, MonadExceptOf.throw] at e
    · simp [pure, Except.pure] at e; subst e
      simp only [convertIn] at hc
      split at hc
      · rename_i v hv; cases hc; exact Hint.convertWith_wf (tparamAliases_wf hA) hv
      · cases hc

theorem sigOf_wf {A : Aliases} (hA : ∀ p ∈ A, p.2.WF) {f : FunDecl} {sig : Sig}
    (h : sigOf A f = .ok sig) : (∀ mt ∈ sig.params, mt.2.WF) ∧ (∀ r, sig.ret = .ty r → r.WF) := by
  obtain ⟨name, tps, params, ret, body⟩ := f
  simp only [sigOf] at h
  revert h
  generalize hp : (List.mapM (m := Except Lit.Code) _ params) = res
  intro h
  cases res with
  | error _ => simp [bind, Except.bind] at h
  | ok ps =>
    simp only [bind, Except.bind] at h
    have hps : ∀ mt ∈ ps, mt.2.WF := by
      intro mt hmt
      obtain ⟨⟨m, x, hh⟩, -, he⟩ := mapM_ok_mem hp mt hmt
      cases hs : sigHint A tps hh with
      | error _ => simp [hs, bind, Except.bind] at he
      | ok t =>
        simp [hs, bind, Except.bind, pure, Except.pure] at he
        subst he
        exact sigHint_wf hA hs
    cases ret with
    | none =>
      simp [pure, Except.pure] at h; subst h
      exact ⟨hps, fun r hr => by cases hr⟩
    | some hh =>
      cases hs : sigHint A tps hh with
      | error _ => simp [hs, Functor.map, Except.map] at h
      | ok t =>
        simp [hs, Functor.map, Except.map, pure, Except.pure] at h; subst h
        exact ⟨hps, fun r hr => by cases hr; exact sigHint_wf hA hs⟩

/-- Signatures whose types are well formed. -/
def SigsWF (sigs : List (String × Sig)) : Prop :=
  ∀ f sig, sigs.lookup f = some sig → (∀ mt ∈ sig.params, mt.2.WF) ∧ (∀ r, sig.ret = .ty r → r.WF)

/-- Every signature `resolveNames` builds has well-formed types: the
recursive-type conditions T062 to T066 hold of them, aliases and all. -/
theorem resolveNames_wf (ss : List Stmt) : SigsWF (resolveNames ss).1 := by
  have empty : SigsWF [] := fun _ _ h => by cases h
  simp only [resolveNames, Id.run, bind, pure]
  refine (forIn_id_inv
    (I := fun (st : Aliases × List (String × Sig) × Array Lit.Code) =>
      (∀ p ∈ st.1, p.2.WF) ∧ SigsWF st.2.1)
    ⟨fun _ h => absurd h List.not_mem_nil, empty⟩ ?_).2
  intro s hmem st ⟨hA, hS⟩
  clear hmem
  obtain ⟨A, sigs, d⟩ := st
  cases s
  case type_ n h =>
    simp only
    split
    · rename_i as hd
      refine ⟨?_, hS⟩
      simp only [defineAlias] at hd
      split at hd
      · cases hd
      · cases hc : convertIn A h with
        | error _ => simp [hc, Functor.map, Except.map] at hd
        | ok t =>
          simp [hc, Functor.map, Except.map] at hd
          subst hd
          show ∀ p ∈ A ++ [(n, t)], p.2.WF
          intro p hp
          simp only [List.mem_append, List.mem_singleton] at hp
          rcases hp with hp | rfl
          · exact hA p hp
          · try simp only [convertIn] at hc
            split at hc
            · rename_i v hv; cases hc; exact Hint.convertWith_wf hA hv
            · cases hc
    · exact ⟨hA, hS⟩
  case fun_ f =>
    simp only
    split
    · exact ⟨hA, hS⟩
    · split
      · rename_i sig hsig
        refine ⟨hA, fun g sg hg => ?_⟩
        change List.lookup g (sigs ++ [(f.name, sig)]) = some sg at hg
        rw [lookup_append_one] at hg
        cases hf : sigs.lookup g with
        | some x =>
          simp only [hf, Option.some_or, Option.some.injEq] at hg; subst hg; exact hS g x hf
        | none =>
          simp only [hf, Option.none_or] at hg
          split at hg
          · cases hg; exact sigOf_wf hA hsig
          · cases hg
      · exact ⟨hA, hS⟩
  all_goals exact ⟨hA, hS⟩

end Datalove.Fun
