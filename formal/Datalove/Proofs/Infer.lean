import Datalove.Fun

/-!
# What inference can fail with

Inference runs on fuel, so it is a total function, and what it does can be
proved. The first thing proved of it is what the comments in `Lit.lean` and
`Fun.lean` say: datalit's inference reports nothing but a malformed hint
(`Lit.infer_errors`), and datafun's reports nothing at all
(`Fun.infer_errors`), save for running out of fuel.

The unifier's `panic!`s are where that could go wrong: in Lean's logic a
panic is an error, with an empty code. Each says a variable just pruned is
free, which holds because a variable is never bound to another variable
(`Inv`), so pruning one finds a free root or a type that is not a variable.
-/

namespace Datalove.Lit

abbrev State := Array VarState

/-- What `m` does, run from a state `pre` admits: ending in a state and a
value `post` admits, or failing with a code `err` admits. -/
structure Spec {α} (pre : State → Prop) (m : InferM α) (post : α → State → Prop)
    (err : Code → Prop) : Prop where
  run : ∀ s, pre s → match m.run s with
    | .ok (a, s') => post a s'
    | .error c => err c

namespace Spec

variable {α β : Type} {pre : State → Prop} {err : Code → Prop}

theorem pure' {post : α → State → Prop} {a : α} (h : ∀ s, pre s → post a s) :
    Spec pre (Pure.pure a) post err := ⟨fun s hs => h s hs⟩

theorem bind' {mid : α → State → Prop} {post : β → State → Prop} {x : InferM α}
    {f : α → InferM β} (hx : Spec pre x mid err) (hf : ∀ a, Spec (mid a) (f a) post err) :
    Spec pre (x >>= f) post err := by
  constructor
  intro s hs
  have := hx.run s hs
  show match (StateT.bind x f).run s with
    | .ok (a, s') => post a s'
    | .error c => err c
  simp only [StateT.run, StateT.bind] at this ⊢
  revert this
  cases x s with
  | error c => intro h; exact h
  | ok r => intro h; exact (hf r.1).run r.2 h

theorem weaken {pre' : State → Prop} {post post' : α → State → Prop} {err' : Code → Prop}
    {m : InferM α} (h : Spec pre m post err) (hp : ∀ s, pre' s → pre s)
    (hq : ∀ a s, post a s → post' a s) (he : ∀ c, err c → err' c) : Spec pre' m post' err' := by
  constructor
  intro s hs
  have := h.run s (hp s hs)
  revert this
  cases m.run s with
  | error c => exact he c
  | ok r => exact hq r.1 r.2

theorem throw' {post : α → State → Prop} {c : Code} (h : err c) :
    Spec pre (throw c : InferM α) post err := ⟨fun _ _ => h⟩

theorem get' {post : State → State → Prop} (h : ∀ s, pre s → post s s) :
    Spec pre (get : InferM State) post err := ⟨fun s hs => h s hs⟩

theorem modify' {post : PUnit → State → Prop} {f : State → State}
    (h : ∀ s, pre s → post ⟨⟩ (f s)) : Spec pre (modify f : InferM PUnit) post err :=
  ⟨fun s hs => h s hs⟩

theorem set' {post : PUnit → State → Prop} {t : State}
    (h : ∀ s, pre s → post ⟨⟩ t) : Spec pre (set t : InferM PUnit) post err :=
  ⟨fun s hs => h s hs⟩

end Spec

/-! ## The invariant: a variable is never bound to a variable -/

def NotVar (t : PTy) : Prop := ∀ v, t ≠ .var v

/-- No variable is bound to another variable: binding goes to a pruned
type, which is not one, or a known type. -/
def Inv (s : State) : Prop := ∀ (i : Nat) (b : PTy), s[i]! = VarState.bound b → NotVar b

def IsFree (s : State) (v : Nat) : Prop := ∃ c, s[v]! = VarState.free c

theorem getElem!_set! (s : State) (i j : Nat) (x : VarState) :
    (s.set! i x)[j]! = if i = j ∧ i < s.size then x else s[j]! := by
  simp only [getElem!_def, Array.set!_eq_setIfInBounds, Array.getElem?_setIfInBounds]
  by_cases hij : i = j
  · subst hij; by_cases hs : i < s.size <;> simp [hs]
  · simp [hij]

theorem Inv.set_bound {s : State} (h : Inv s) {i : Nat} {b : PTy} (hb : NotVar b) :
    Inv (s.set! i (.bound b)) := by
  intro j c hc
  rw [getElem!_set!] at hc
  split at hc
  · cases hc; exact hb
  · exact h j c hc

theorem Inv.set_free {s : State} (h : Inv s) {i : Nat} {c : Class} : Inv (s.set! i (.free c)) := by
  intro j b hb
  rw [getElem!_set!] at hb
  split at hb
  · cases hb
  · exact h j b hb

theorem Inv.set_link {s : State} (h : Inv s) {i w : Nat} : Inv (s.set! i (.link w)) := by
  intro j b hb
  rw [getElem!_set!] at hb
  split at hb
  · cases hb
  · exact h j b hb

theorem getElem!_push (s : State) (x : VarState) (j : Nat) :
    (s.push x)[j]! = if j = s.size then x else s[j]! := by
  simp only [getElem!_def, Array.getElem?_push]
  by_cases hj : j = s.size <;> simp [hj]

theorem Inv.push_free {s : State} (h : Inv s) {c : Class} : Inv (s.push (.free c)) := by
  intro j b hb
  rw [getElem!_push] at hb
  split at hb
  · cases hb
  · exact h j b hb

theorem Inv.empty : Inv #[] := by
  intro j b hb
  have : (#[] : State)[j]! = default := rfl
  rw [this] at hb
  cases hb

/-! ## Finding and pruning -/

theorem findF_spec (s0 : State) : ∀ (n v : Nat),
    Spec (· = s0) (findF n v) (fun r s => s = s0 ∧ ∀ w, s0[r]! ≠ .link w) (· = "fuel")
  | 0, _ => ⟨fun _ _ => rfl⟩
  | n + 1, v => by
    constructor; intro s hs; subst hs
    simp only [findF, StateT.run, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get,
      pure, Except.pure, Except.bind]
    cases hv : s[v]! with
    | link w => exact (findF_spec s n w).run s rfl
    | free c =>
      simp only [hv, StateT.pure, Pure.pure, Except.pure]
      exact ⟨trivial, fun w hw => by cases hw⟩
    | bound b =>
      simp only [hv, StateT.pure, Pure.pure, Except.pure]
      exact ⟨trivial, fun w hw => by cases hw⟩

theorem prune_spec (s0 : State) (t : PTy) :
    Spec (fun s => s = s0 ∧ Inv s0) (prune t)
      (fun t' s => s = s0 ∧ (∀ x, t' = .var x → IsFree s0 x) ∧
        (NotVar t → t' = t) ∧ (∀ v, t = .var v → NotVar t' ∨ ∃ x, t' = .var x)) (· = "fuel") := by
  constructor
  intro s ⟨hs, inv⟩; subst hs
  cases t
  case var v =>
    have hf := (findF_spec s inferFuel v).run s rfl
    simp only [prune, find, StateT.run, bind, StateT.bind] at hf ⊢
    revert hf
    cases findF inferFuel v s with
    | error c => exact id
    | ok r =>
      obtain ⟨x, s'⟩ := r
      rintro ⟨rfl, hx⟩
      simp only [get, getThe, MonadStateOf.get, StateT.get, pure, Except.pure, Except.bind]
      cases hs : s'[x]! with
      | bound b =>
        simp only [StateT.pure, Pure.pure, Except.pure]
        exact ⟨trivial, fun y hy => absurd hy (inv x b hs y), fun h => absurd rfl (h v),
          fun _ _ => .inl (inv x b hs)⟩
      | free c =>
        simp only [StateT.pure, Pure.pure, Except.pure]
        refine ⟨trivial, fun y hy => ?_, fun h => absurd rfl (h v), fun _ _ => .inr ⟨x, rfl⟩⟩
        cases hy; exact ⟨c, hs⟩
      | link w => exact absurd hs (hx w)
  all_goals simp [prune, StateT.run, pure, StateT.pure, Except.pure]

theorem rootOf_spec (s0 : State) (t : PTy) :
    Spec (· = s0) (rootOf t) (fun _ s => s = s0) (· = "fuel") := by
  cases t
  case var v =>
    constructor; intro s hs; subst hs
    have hf := (findF_spec s inferFuel v).run s rfl
    simp only [rootOf, find, Functor.map, StateT.map, StateT.run] at hf ⊢
    revert hf
    cases findF inferFuel v s with
    | error c => exact id
    | ok r => intro h; exact h.1
  all_goals (constructor; intro s hs; subst hs; rfl)

theorem Spec.forM_inv {α} {l : List α} {f : α → InferM PUnit} {err : Code → Prop}
    (h : ∀ a ∈ l, Spec Inv (f a) (fun _ => Inv) err) : Spec Inv (l.forM f) (fun _ => Inv) err := by
  induction l with
  | nil => exact Spec.pure' (fun _ hs => hs)
  | cons a as ih =>
    simp only [List.forM]
    exact Spec.bind' (h a List.mem_cons_self)
      (fun _ => ih (fun b hb => h b (List.mem_cons_of_mem _ hb)))

theorem Spec.fact {α} {P : Prop} {m : InferM α} {post : α → State → Prop} {err : Code → Prop}
    {s0 : State} (h : P → Spec (· = s0) m post err) : Spec (fun s => s = s0 ∧ P) m post err :=
  ⟨fun s ⟨hs, hp⟩ => (h hp).run s hs⟩

theorem Spec.at {α} {pre : State → Prop} {m : InferM α} {post : α → State → Prop}
    {err : Code → Prop} (h : ∀ s0, pre s0 → Spec (· = s0) m post err) : Spec pre m post err :=
  ⟨fun s0 hs => (h s0 hs).run s0 rfl⟩

/-- What pruning `t` says of what it gives. -/
def Pruned (s0 : State) (t t' : PTy) : Prop :=
  (∀ x, t' = .var x → IsFree s0 x) ∧ (NotVar t → t' = t) ∧
    (∀ v, t = .var v → NotVar t' ∨ ∃ x, t' = .var x)

theorem prune_spec' (s0 : State) (inv : Inv s0) (t : PTy) :
    Spec (· = s0) (prune t) (fun t' s => s = s0 ∧ Pruned s0 t t') (· = "fuel") :=
  Spec.weaken (prune_spec s0 t) (fun _ h => ⟨h, inv⟩) (fun _ _ h => ⟨h.1, h.2⟩) (fun _ h => h)

mutual

theorem unifyF_spec : ∀ (n : Nat) (a b : PTy), Spec Inv (unifyF n a b) (fun _ => Inv) (· = "fuel")
  | 0, _, _ => ⟨fun _ _ => rfl⟩
  | n + 1, a, b => Spec.at fun s0 inv => by
    simp only [unifyF]
    refine Spec.bind' (rootOf_spec s0 a) (fun aRoot => ?_)
    refine Spec.bind' (rootOf_spec s0 b) (fun bRoot => ?_)
    refine Spec.bind' (prune_spec' s0 inv a) (fun a' => Spec.fact fun pa => ?_)
    refine Spec.bind' (prune_spec' s0 inv b) (fun b' => Spec.fact fun pb => ?_)
    have toInv : ∀ s, s = s0 → Inv s := fun s h => h ▸ inv
    split
    · rename_i x y
      exact Spec.weaken (unionF_spec n x y) (fun s h => ⟨toInv s h, h ▸ pa.1 x rfl, h ▸ pb.1 y rfl⟩)
        (fun _ _ h => h) (fun _ h => h)
    · rename_i x ht
      exact Spec.weaken (bindF_spec n x b')
        (fun s h => ⟨toInv s h, h ▸ pa.1 x rfl, fun v hv => ht v hv⟩) (fun _ _ h => h) (fun _ h => h)
    · rename_i y ht
      exact Spec.weaken (bindF_spec n y a')
        (fun s h => ⟨toInv s h, h ▸ pb.1 y rfl, fun v hv => ht v hv⟩) (fun _ _ h => h) (fun _ h => h)
    all_goals
      refine Spec.weaken (pre := Inv) ?_ toInv (fun _ _ h => h) (fun _ h => h)
      repeat' (first
        | with_reducible exact Spec.pure' (fun _ h => h)
        | with_reducible exact unifyPartsF_spec n _ _
        | exact Spec.modify' (fun _ h => Inv.set_bound h (fun _ hv => by cases hv))
        | with_reducible apply Spec.bind'
        | split
        | intro _)
      all_goals exact Spec.modify' (fun _ h => Inv.set_bound h (fun _ hv => by cases hv))

theorem unifyPartsF_spec : ∀ (n : Nat) (a b : PTy),
    Spec Inv (unifyPartsF n a b) (fun _ => Inv) (· = "fuel")
  | 0, _, _ => ⟨fun _ _ => rfl⟩
  | n + 1, a, b => by
    cases a <;> cases b <;> simp only [unifyPartsF]
    all_goals
      repeat' (first
        | with_reducible exact Spec.pure' (fun _ h => h)
        | with_reducible exact unifyF_spec n _ _
        | (apply Spec.forM_inv; intro _ _)
        | with_reducible apply Spec.bind'
        | split
        | intro _)

theorem unionF_spec : ∀ (n x y : Nat),
    Spec (fun s => Inv s ∧ IsFree s x ∧ IsFree s y) (unionF n x y) (fun _ => Inv) (· = "fuel")
  | 0, _, _ => ⟨fun _ _ => rfl⟩
  | n + 1, x, y => Spec.at fun s0 ⟨inv, ⟨cx, hx⟩, ⟨cy, hy⟩⟩ => by
    simp only [unionF]
    split
    · exact Spec.pure' (fun s h => h ▸ inv)
    · refine Spec.bind' (Spec.get' (post := fun st s => s = s0 ∧ st = s0) (fun s h => ⟨h, h⟩))
        (fun st => Spec.fact fun hst => ?_)
      subst hst
      simp only [hx, hy]
      split
      · exact Spec.pure' (fun s h => h ▸ inv)
      · refine Spec.bind' (Spec.modify' (post := fun _ => Inv)
          (fun s h => h ▸ Inv.set_free (Inv.set_link inv))) (fun _ => ?_)
        split
        · exact unifyF_spec n _ _
        · exact Spec.pure' (fun _ h => h)

theorem bindF_spec : ∀ (n v : Nat) (t : PTy),
    Spec (fun s => Inv s ∧ IsFree s v ∧ NotVar t) (bindF n v t) (fun _ => Inv) (· = "fuel")
  | 0, _, _ => ⟨fun _ _ => rfl⟩
  | n + 1, v, t => Spec.at fun s0 ⟨inv, ⟨c, hv⟩, nt⟩ => by
    simp only [bindF]
    refine Spec.bind' (Spec.get' (post := fun st s => s = s0 ∧ st = s0) (fun s h => ⟨h, h⟩))
      (fun st => Spec.fact fun hst => ?_)
    subst hst
    simp only [hv]
    split
    · exact Spec.pure' (fun s h => h ▸ inv)
    · refine Spec.bind' (Spec.modify' (post := fun _ => Inv)
        (fun s h => h ▸ Inv.set_bound inv nt)) (fun _ => ?_)
      repeat' (first
        | with_reducible exact Spec.pure' (fun _ h => h)
        | with_reducible exact unifyF_spec n _ _
        | with_reducible apply Spec.bind'
        | split
        | intro _)

end


/-! ## The rest of inference keeps the invariant -/

theorem fresh_spec (c : Class) {err : Code → Prop} : Spec Inv (fresh c) (fun _ => Inv) err :=
  ⟨fun _ h => Inv.push_free h⟩

theorem Spec.mapM_inv {α β} {l : List α} {f : α → InferM β} {err : Code → Prop}
    (h : ∀ a ∈ l, Spec Inv (f a) (fun _ => Inv) err) : Spec Inv (l.mapM f) (fun _ => Inv) err := by
  induction l with
  | nil => exact Spec.pure' (fun _ hs => hs)
  | cons a as ih =>
    rw [List.mapM_cons]
    exact Spec.bind' (h a List.mem_cons_self) (fun _ =>
      Spec.bind' (ih (fun b hb => h b (List.mem_cons_of_mem _ hb))) (fun _ => Spec.pure' (fun _ h => h)))

theorem Spec.forIn_inv {α β} {l : List α} {b : β} {f : α → β → InferM (ForInStep β)}
    {err : Code → Prop} (h : ∀ a ∈ l, ∀ b, Spec Inv (f a b) (fun _ => Inv) err) :
    Spec Inv (forIn l b f) (fun _ => Inv) err := by
  induction l generalizing b with
  | nil => exact Spec.pure' (fun _ hs => hs)
  | cons a as ih =>
    rw [List.forIn_cons]
    refine Spec.bind' (h a List.mem_cons_self b) (fun r => ?_)
    cases r
    · exact Spec.pure' (fun _ h => h)
    · exact ih (fun x hx => h x (List.mem_cons_of_mem _ hx))

theorem Spec.foldlM_inv {α β} {l : List α} {b : β} {f : β → α → InferM β} {err : Code → Prop}
    (h : ∀ b, ∀ a ∈ l, Spec Inv (f b a) (fun _ => Inv) err) :
    Spec Inv (l.foldlM f b) (fun _ => Inv) err := by
  induction l generalizing b with
  | nil => exact Spec.pure' (fun _ hs => hs)
  | cons a as ih =>
    rw [List.foldlM_cons]
    exact Spec.bind' (h b a List.mem_cons_self)
      (fun _ => ih (fun b x hx => h b x (List.mem_cons_of_mem _ hx)))

theorem Spec.mapM_same {α β} {s0 : State} {l : List α} {f : α → InferM β} {err : Code → Prop}
    (h : ∀ a ∈ l, Spec (· = s0) (f a) (fun _ s => s = s0) err) :
    Spec (· = s0) (l.mapM f) (fun _ s => s = s0) err := by
  induction l with
  | nil => exact Spec.pure' (fun _ hs => hs)
  | cons a as ih =>
    rw [List.mapM_cons]
    exact Spec.bind' (h a List.mem_cons_self) (fun _ =>
      Spec.bind' (ih (fun b hb => h b (List.mem_cons_of_mem _ hb))) (fun _ => Spec.pure' (fun _ h => h)))

theorem unify_spec (a b : PTy) : Spec Inv (unify a b) (fun _ => Inv) (· = "fuel") :=
  unifyF_spec inferFuel a b

/-- What `resolve` can fail with: an open variable (T016), one inference
could not read (`unknownCode`), or running out of fuel. It reads the state
and leaves it as it was. -/
def ResolveErr (c : Code) : Prop := c = "T016" ∨ c = unknownCode ∨ c = "fuel"

theorem resolveF_spec (s0 : State) (inv : Inv s0) : ∀ (n : Nat) (t : PTy),
    Spec (· = s0) (resolveF n t) (fun _ s => s = s0) ResolveErr
  | 0, _ => ⟨fun _ _ => by rw [resolveF.eq_def]; exact (Or.inr (Or.inr rfl) : ResolveErr "fuel")⟩
  | n + 1, t => by
    simp only [resolveF]
    refine Spec.bind' (Spec.weaken (prune_spec' s0 inv t) (fun _ h => h) (fun _ _ h => h)
      (fun _ h => .inr (.inr h))) (fun t' => Spec.fact fun pt => ?_)
    have ih := resolveF_spec s0 inv n
    have ihf : ∀ fs, Spec (· = s0) (resolveF.fields n fs) (fun _ s => s = s0) ResolveErr := by
      intro fs
      simp only [resolveF.fields]
      exact Spec.bind' (Spec.mapM_same fun a _ =>
        Spec.bind' (ih a.2) (fun _ => Spec.pure' (fun _ h => h))) (fun _ => Spec.pure' (fun _ h => h))
    split
    · exact Spec.pure' (fun _ h => h)
    · rename_i v
      obtain ⟨c, hc⟩ := pt.1 v rfl
      refine Spec.bind' (Spec.get' (post := fun st s => s = s0 ∧ st = s0) (fun s h => ⟨h, h⟩))
        (fun st => Spec.fact fun hst => ?_)
      subst hst
      simp only [hc]
      rcases c with _ | _ | _ | _ | ⟨a, _ | p⟩ | f | _ <;> simp only [map_eq_pure_bind]
      all_goals first
        | exact Spec.pure' (fun _ h => h)
        | exact Spec.throw' (.inl rfl)
        | exact Spec.throw' (.inr (.inl rfl))
        | exact Spec.bind' (ih _) (fun _ => Spec.pure' (fun _ h => h))
    all_goals
      try simp only [map_eq_pure_bind, seq_eq_bind_map]
      repeat' (first
        | with_reducible exact Spec.pure' (fun _ h => h)
        | with_reducible exact ih _
        | with_reducible exact ihf _
        | (with_reducible apply Spec.mapM_same; intro _ _)
        | with_reducible apply Spec.bind'
        | split
        | unfold Option.mapM
        | rw [map_eq_pure_bind]
        | intro _)
      all_goals exact Spec.pure' (fun _ h => h)


/-! ## Datalit's inference -/

/-- What datalit's inference can fail with: a malformed hint's code, or
running out of fuel. -/
def LitErr (c : Code) : Prop := c = "fuel" ∨ ∃ h, convertHint h = .error c

mutual

theorem inferF_spec : ∀ (n : Nat) (e : Expr), Spec Inv (inferF n e) (fun _ => Inv) LitErr
  | 0, _ => ⟨fun _ _ => by rw [inferF.eq_def]; exact (Or.inl rfl : LitErr "fuel")⟩
  | n + 1, e => by
    cases e
    case hinted h _ =>
      simp only [inferF, liftM]
      constructor
      intro s hs
      cases hc : convertHint h with
      | error c =>
        simp [StateT.lift, StateT.run, bind, StateT.bind, Except.bind, LitErr]
        exact .inr ⟨h, hc⟩
      | ok t =>
        simp [StateT.lift, StateT.run, bind, StateT.bind, Except.bind, pure, StateT.pure,
          Except.pure, hs]
    all_goals
      simp only [inferF]
      repeat' (first
        | with_reducible exact Spec.pure' (fun _ h => h)
        | with_reducible exact fresh_spec _
        | with_reducible exact inferF_spec n _
        | with_reducible exact togetherF_spec n _
        | (with_reducible apply Spec.mapM_inv; intro _ _)
        | (with_reducible apply Spec.forIn_inv; intro _ _ _)
        | with_reducible apply Spec.bind'
        | split
        | rw [map_eq_pure_bind]
        | intro _)
      all_goals first
        | exact Spec.pure' (fun _ h => h)
        | exact Spec.weaken (unify_spec _ _) (fun _ h => h) (fun _ _ h => h) (fun _ h => .inl h)

theorem togetherF_spec : ∀ (n : Nat) (es : List Expr), Spec Inv (togetherF n es) (fun _ => Inv) LitErr
  | 0, _ => ⟨fun _ _ => by rw [togetherF.eq_def]; exact (Or.inl rfl : LitErr "fuel")⟩
  | n + 1, es => by
    simp only [togetherF]
    refine Spec.bind' (fresh_spec _) (fun t => ?_)
    refine Spec.bind' (Spec.forIn_inv fun e _ _ => ?_) (fun _ => Spec.pure' (fun _ h => h))
    exact Spec.bind' (inferF_spec n e) (fun p =>
      Spec.bind' (Spec.weaken (unify_spec _ _) (fun _ h => h) (fun _ _ h => h) (fun _ h => .inl h))
        (fun _ => Spec.pure' (fun _ h => h)))

end

/-- Datalit's inference reports nothing but a malformed hint, as its comment
says, or running out of fuel. -/
theorem infer_errors {e : Expr} {c : Code} (h : (infer e).run #[] = .error c) :
    c = "fuel" ∨ ∃ hint, convertHint hint = .error c := by
  have := (inferF_spec inferFuel e).run #[] Inv.empty
  simp only [infer] at h
  rw [h] at this
  exact this

end Datalove.Lit

namespace Datalove.Fun

open Datalove.Lit (State Spec Inv fresh_spec prune_spec' unify_spec resolveF_spec inferFuel)

/-- What datafun's inference can fail with: running out of fuel, and
nothing else. -/
def FunErr (c : Lit.Code) : Prop := c = "fuel"

theorem unify_spec' (a b : Lit.PTy) : Spec Inv (Lit.unify a b) (fun _ => Inv) FunErr :=
  unify_spec a b

theorem shallow_spec (t : Lit.PTy) : Spec Inv (shallow t) (fun _ => Inv) FunErr :=
  Spec.at fun s0 inv => by
    simp only [shallow]
    refine Spec.bind' (prune_spec' s0 inv t) (fun t' => Spec.fact fun _ => ?_)
    split <;> exact Spec.pure' (fun s h => h ▸ inv)

theorem inferField_spec (b : Lit.PTy) (sel : Sel) : Spec Inv (inferField b sel) (fun _ => Inv) FunErr := by
  simp only [inferField]
  refine Spec.bind' (shallow_spec b) (fun _ => ?_)
  repeat' (first
    | exact Spec.pure' (fun _ h => h)
    | exact fresh_spec _
    | split)

theorem elementOf_spec (b : Lit.PTy) : Spec Inv (elementOf b) (fun _ => Inv) FunErr := by
  simp only [elementOf]
  refine Spec.bind' (shallow_spec b) (fun _ => ?_)
  repeat' (first
    | exact Spec.pure' (fun _ h => h)
    | exact fresh_spec _
    | split)


/-- `@` reads what its operand resolves to without keeping what resolving
did, which is nothing: resolving leaves the state as it was. -/
theorem adapt_step (t : Lit.PTy) :
    Spec Inv (do
      let s ← get
      match (Lit.resolve t).run s with
      | .ok (from_, s') => set s' >>= fun _ => Lit.fresh (.adapt from_)
      | .error _ => pure t : Lit.InferM Lit.PTy) (fun _ => Inv) FunErr := by
  refine Spec.bind' (Spec.get' (post := fun st s => s = st ∧ Inv st) (fun s h => ⟨rfl, h⟩))
    (fun st => ?_)
  constructor
  intro s ⟨hs, inv⟩
  subst hs
  have hr := (resolveF_spec s inv inferFuel t).run s rfl
  rw [show Lit.resolve t = Lit.resolveF inferFuel t from rfl]
  revert hr
  generalize (Lit.resolveF inferFuel t).run s = R
  cases R with
  | error c => intro _; exact inv
  | ok r =>
    obtain ⟨from_, s'⟩ := r
    intro hr
    simp only at hr
    subst hr
    exact (Spec.bind' (err := FunErr) (Spec.set' (post := fun _ => Inv) (fun _ _ => inv))
      (fun _ => fresh_spec _)).run s' inv

mutual

theorem inferF_spec : ∀ (n : Nat) (Γ : Ctx) (e : Expr), Spec Inv (inferF n Γ e) (fun _ => Inv) FunErr
  | 0, _, _ => ⟨fun _ _ => by rw [inferF.eq_def]; exact (rfl : FunErr "fuel")⟩
  | n + 1, Γ, e => by
    cases e
    case adapt a =>
      simp only [inferF]
      exact Spec.bind' (inferF_spec n Γ a) (fun t => adapt_step t)
    case unop op a => cases op <;> simp only [inferF] <;> first
      | exact Spec.pure' (fun _ h => h)
      | exact inferF_spec n Γ a
    case index b k mode =>
      cases mode
      · simp only [inferF]; exact fresh_spec _
      · simp only [inferF]; exact Spec.bind' (inferF_spec n Γ b) (fun _ => elementOf_spec _)
    all_goals
      simp only [inferF]
      repeat' (first
        | with_reducible exact Spec.pure' (fun _ h => h)
        | with_reducible exact fresh_spec _
        | with_reducible exact inferF_spec n _ _
        | with_reducible exact togetherF_spec n _ _
        | with_reducible exact inferCallF_spec n _ _ _
        | with_reducible exact unify_spec' _ _
        | with_reducible exact shallow_spec _
        | with_reducible exact inferField_spec _ _
        | with_reducible exact elementOf_spec _
        | (with_reducible apply Spec.mapM_inv; intro _ _)
        | (with_reducible apply Spec.foldlM_inv; intro _ _ _)
        | with_reducible apply Spec.bind'
        | split
        | rw [map_eq_pure_bind]
        | intro _)
      all_goals first
        | exact Spec.pure' (fun _ h => h)
        | exact fresh_spec _
        | exact unify_spec' _ _

theorem inferCallF_spec : ∀ (n : Nat) (Γ : Ctx) (sig : Sig) (args : List (Option Mode × Expr)),
    Spec Inv (inferCallF n Γ sig args) (fun _ => Inv) FunErr
  | 0, _, _, _ => ⟨fun _ _ => by rw [inferCallF.eq_def]; exact (rfl : FunErr "fuel")⟩
  | n + 1, Γ, sig, args => by
    simp only [inferCallF]
    refine Spec.bind' (Spec.mapM_inv fun _ _ => Spec.bind' (fresh_spec _)
      (fun _ => Spec.pure' (fun _ h => h))) (fun ps => ?_)
    refine Spec.bind' (Spec.forIn_inv fun x _ _ => ?_) (fun _ => Spec.pure' (fun _ h => h))
    exact Spec.bind' (inferF_spec n Γ _) (fun _ =>
      Spec.bind' (unify_spec' _ _) (fun _ => Spec.pure' (fun _ h => h)))

theorem togetherF_spec : ∀ (n : Nat) (Γ : Ctx) (es : List Expr),
    Spec Inv (togetherF n Γ es) (fun _ => Inv) FunErr
  | 0, _, _ => ⟨fun _ _ => by rw [togetherF.eq_def]; exact (rfl : FunErr "fuel")⟩
  | n + 1, Γ, es => by
    simp only [togetherF, sharedVar]
    refine Spec.bind' (fresh_spec _) (fun t => ?_)
    refine Spec.bind' (Spec.forIn_inv fun e _ _ => ?_) (fun _ => Spec.pure' (fun _ h => h))
    exact Spec.bind' (inferF_spec n Γ e) (fun _ =>
      Spec.bind' (unify_spec' _ _) (fun _ => Spec.pure' (fun _ h => h)))

end

/-- Datafun's inference fails at nothing, as its comment says, save running
out of fuel: what it cannot read is `unknown`. -/
theorem infer_errors {Γ : Ctx} {e : Expr} {c : Lit.Code} (h : (infer Γ e).run #[] = .error c) :
    c = "fuel" := by
  have := (inferF_spec inferFuel Γ e).run #[] Lit.Inv.empty
  simp only [infer] at h
  rw [h] at this
  exact this

end Datalove.Fun
