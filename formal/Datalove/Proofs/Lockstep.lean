import Datalove.Proofs.Infer
import Datalove.Proofs.InstanceCheck
import Datalove.Proofs.Sigs

/-!
# Inference under a generic steps with inference under an instance

The check-once theorem (`Proofs/InstanceStmt.lean`) asks of inference that,
where a generic checks quietly, inference under an instance answers what it
answered under the generic, substituted (`InstEnv`). This proves it of the
real inference, `Lit.lean`'s unifier and `Fun.lean`'s rules.

Substituting type arguments touches only known types, so it leaves a
variable's number alone, and a state is substituted variable by variable.
Every decision inference makes is then one of two kinds. Either it does not
depend on what a type parameter stands for, and comes out the same under the
instance, since a type argument is ground and so keeps a type's head where
the head is not a type parameter. Or it does, and inference throws F110
(`Lit.rigidCode`) under the generic. So a run under the generic either fails
with F110, or the run under the instance makes the same steps on the
substituted state, and fails or answers the same, substituted (`Lock`). A
generic that checks quietly met no F110, which is how the theorem's
hypothesis is discharged.
-/

namespace Datalove.Lit

variable (θ : List (String × Ty))

mutual

/-- A type with variables, its known parts' type parameters substituted. -/
def PTy.inst : PTy → PTy
  | .known t => .known (t.inst θ)
  | .var v => .var v
  | .tuple ts => .tuple (PTy.instList ts)
  | .struct fs => .struct (PTy.instFields fs)
  | .table fs => .table (PTy.instFields fs)
  | .list t => .list (PTy.inst t)
  | .set t => .set (PTy.inst t)
  | .map k v => .map (PTy.inst k) (PTy.inst v)
  | .option t => .option (PTy.inst t)
  | .result t => .result (PTy.inst t)
  | .tensor t r => .tensor (PTy.inst t) r
  | .term a t => .term a (PTy.inst t)
  | .enum vs => .enum (PTy.instVariants vs)

def PTy.instList : List PTy → List PTy
  | [] => []
  | t :: ts => PTy.inst t :: PTy.instList ts

def PTy.instFields : List (String × PTy) → List (String × PTy)
  | [] => []
  | (n, t) :: fs => (n, PTy.inst t) :: PTy.instFields fs

def PTy.instVariants : List (String × Option PTy) → List (String × Option PTy)
  | [] => []
  | (n, none) :: vs => (n, none) :: PTy.instVariants vs
  | (n, some t) :: vs => (n, some (PTy.inst t)) :: PTy.instVariants vs

end

def Class.inst : Class → Class
  | .variant a p => .variant a (p.map (PTy.inst θ))
  | .adapt f => .adapt (f.inst θ)
  | c => c

def VarState.inst : VarState → VarState
  | .free c => .free (c.inst θ)
  | .bound t => .bound (t.inst θ)
  | .link v => .link v

/-- A state, each variable's type parameters substituted. -/
def State.inst (s : State) : State := s.map (VarState.inst θ)

variable {θ}

theorem PTy.instList_eq : ∀ (ts : List PTy), PTy.instList θ ts = ts.map (PTy.inst θ)
  | [] => rfl
  | t :: ts => by simp [PTy.instList, PTy.instList_eq ts]

theorem PTy.instFields_eq : ∀ (fs : List (String × PTy)),
    PTy.instFields θ fs = fs.map fun (n, t) => (n, t.inst θ)
  | [] => rfl
  | (n, t) :: fs => by simp [PTy.instFields, PTy.instFields_eq fs]

theorem PTy.instVariants_eq : ∀ (vs : List (String × Option PTy)),
    PTy.instVariants θ vs = vs.map fun (n, p) => (n, p.map (PTy.inst θ))
  | [] => rfl
  | (n, none) :: vs => by simp [PTy.instVariants, PTy.instVariants_eq vs]
  | (n, some t) :: vs => by simp [PTy.instVariants, PTy.instVariants_eq vs]

@[simp] theorem VarState.inst_default : VarState.inst θ default = default := rfl

theorem State.inst_get (s : State) (i : Nat) : (State.inst θ s)[i]! = (s[i]!).inst θ := by
  simp only [State.inst, getElem!_def, Array.getElem?_map]
  cases s[i]? <;> rfl

theorem State.inst_set (s : State) (i : Nat) (x : VarState) :
    State.inst θ (s.set! i x) = (State.inst θ s).set! i (x.inst θ) := by
  by_cases h : i < s.size <;> simp [State.inst, Array.set!, Array.setIfInBounds, h, Array.map_set]

theorem State.inst_setIfInBounds (s : State) (i : Nat) (x : VarState) :
    State.inst θ (s.setIfInBounds i x) = (State.inst θ s).setIfInBounds i (x.inst θ) :=
  State.inst_set s i x

theorem State.inst_push (s : State) (x : VarState) :
    State.inst θ (s.push x) = (State.inst θ s).push (x.inst θ) := by
  simp [State.inst, Array.map_push]

theorem State.inst_size (s : State) : (State.inst θ s).size = s.size := by
  simp [State.inst]

/-! ## Decisions that come out the same -/

/-- A type that is not a type parameter keeps its head when substituted. -/
theorem Ty.inst_not_var {t : Ty} (h : ∀ n, t ≠ .var n) : ∀ n, t.inst θ ≠ .var n := by
  cases t <;> simp_all [Ty.inst]

section
variable (g : Ground θ)
include g

theorem headVar_inst {k : Ty} (h : headVar k = false) : headVar (k.inst θ) = false := by
  unfold headVar at *
  rw [Ty.expose_inst g]
  generalize k.expose = e at h ⊢
  cases e <;> simp_all [Ty.inst]

end

theorem Class.admits_known (c : Class) (k : Ty) : c.admits (.known k) = c.admitsHead (.known k.expose) := by
  cases k <;> rfl

/-- A class, but for one adapted from a type, admits a known type by the
constructor at its head. -/
theorem Class.admitsHead_inst_known {c : Class} (hc : ∀ f, c ≠ .adapt f) {e : Ty}
    (he : ∀ n, e ≠ .var n) : (c.inst θ).admitsHead (.known (e.inst θ)) = c.admitsHead (.known e) := by
  cases c
  case variant a p =>
    cases p <;> cases e <;> simp_all [Class.inst, Class.admitsHead, Ty.inst, BEq.beq]
  all_goals cases e <;> simp_all [Class.inst, Class.admitsHead, Ty.inst, Ty.isUnsigned,
    Ty.isFixedInt, Ty.isFloat, BEq.beq]

theorem Class.admits_other (c : Class) {p : PTy} (hp : ∀ t, p ≠ .known t) :
    c.admits p = c.admitsHead p := by
  cases p <;> simp_all [Class.admits]

theorem PTy.inst_not_known {p : PTy} (hp : ∀ t, p ≠ .known t) : ∀ t, p.inst θ ≠ .known t := by
  cases p <;> simp_all [PTy.inst]

theorem Class.admitsHead_inst_other (c : Class) {p : PTy} (hp : ∀ t, p ≠ .known t) :
    (c.inst θ).admitsHead (p.inst θ) = c.admitsHead p := by
  cases c
  case variant a q =>
    cases q <;> cases p <;> simp_all [Class.inst, Class.admitsHead, PTy.inst]
  all_goals cases p <;> simp_all [Class.inst, Class.admitsHead, PTy.inst]

section
variable (g : Ground θ)
include g

theorem Class.admits_inst {c : Class} {t : PTy} (h : c.dependsOn t = false) :
    (c.inst θ).admits (t.inst θ) = c.admits t := by
  by_cases hk : ∃ k, t = .known k
  · obtain ⟨k, rfl⟩ := hk
    simp only [PTy.inst, Class.admits_known, Ty.expose_inst g]
    by_cases ho : c = .open_ ∨ c = .unknown
    · rcases ho with rfl | rfl <;> rfl
    by_cases ha : ∃ f, c = .adapt f
    · obtain ⟨f, rfl⟩ := ha
      simp only [Class.dependsOn, Bool.and_eq_false_iff, Bool.or_eq_false_iff, Bool.not_eq_false',
        Class.admits_known] at h
      simp only [Class.inst, Class.admitsHead]
      rcases h with ⟨hf, hk⟩ | h
      · rw [Ty.inst_novar θ f hf]
        rw [← Ty.expose_inst g, Ty.inst_novar θ k hk]
      · simp only [Class.admitsHead] at h
        rw [h]; exact Ty.canAdapt_inst g h
    · have hc : ∀ f, c ≠ .adapt f := fun f e => ha ⟨f, e⟩
      have hv : headVar k = false := by
        cases c <;> simp_all [Class.dependsOn]
      have he : ∀ n, k.expose ≠ .var n := by
        intro n e; simp [headVar, e] at hv
      exact Class.admitsHead_inst_known hc he
  · have hp : ∀ k, t ≠ .known k := fun k e => hk ⟨k, e⟩
    rw [Class.admits_other _ (PTy.inst_not_known hp), Class.admits_other _ hp]
    exact Class.admitsHead_inst_other c hp

theorem Class.dependsOn_inst {c : Class} {t : PTy} (h : c.dependsOn t = false) :
    (c.inst θ).dependsOn (t.inst θ) = false := by
  cases t with
  | known k =>
    cases c
    case adapt f =>
      have ha := Class.admits_inst g h
      simp only [PTy.inst, Class.inst] at ha ⊢
      simp only [Class.dependsOn, Bool.and_eq_false_iff, Bool.or_eq_false_iff, Bool.not_eq_false'] at h ⊢
      rw [ha]
      rcases h with ⟨hf, hk⟩ | h
      · exact .inl ⟨by rw [Ty.inst_novar θ f hf]; exact hf, by rw [Ty.inst_novar θ k hk]; exact hk⟩
      · exact .inr h
    all_goals simp_all [Class.dependsOn, Class.inst, PTy.inst, headVar_inst g]
  | _ => cases c <;> simp [Class.dependsOn, Class.inst, PTy.inst]

theorem Class.mergeDepends_inst {cx cy : Class} (h : cx.mergeDepends cy = false) :
    (cx.inst θ).mergeDepends (cy.inst θ) = false := by
  cases cx <;> cases cy <;> simp only [Class.mergeDepends] at h ⊢ <;>
    first | rfl | exact Class.dependsOn_inst g h

/-- Making two classes one comes out the same, substituted, where it does
not depend on what a type parameter stands for. -/
theorem mergeClasses_inst {cx cy : Class} (h : cx.mergeDepends cy = false) :
    mergeClasses (cx.inst θ) (cy.inst θ) =
      (mergeClasses cx cy).map fun (c, pl) => (c.inst θ, pl.map fun (p, q) => (p.inst θ, q.inst θ)) := by
  have ak : ∀ (c : Class) (f : Ty), c.dependsOn (.known f) = false →
      (c.inst θ).admits (.known (f.inst θ)) = c.admits (.known f) :=
    fun c f h => Class.admits_inst g (t := .known f) h
  rcases cx with _ | _ | _ | _ | ⟨a, _ | p⟩ | f | _ <;>
    rcases cy with _ | _ | _ | _ | ⟨b, _ | q⟩ | f' | _ <;>
    simp only [Class.mergeDepends] at h <;> simp [mergeClasses, Class.inst]
  all_goals (have e := ak _ _ h; simp only [Class.inst, Option.map] at e; simp [e])

end

theorem tysP_inst : ∀ (ts : Tys), PTy.instList θ (tysP ts) = tysP (ts.inst θ)
  | .nil => rfl
  | .cons t ts => by simp [tysP, PTy.instList, Tys.inst, PTy.inst, tysP_inst ts]

theorem fieldsP_inst : ∀ (fs : Fields), PTy.instFields θ (fieldsP fs) = fieldsP (fs.inst θ)
  | .nil => rfl
  | .cons n t fs => by simp [fieldsP, PTy.instFields, Fields.inst, PTy.inst, fieldsP_inst fs]

theorem variantsP_inst : ∀ (vs : Variants), PTy.instVariants θ (variantsP vs) = variantsP (vs.inst θ)
  | .nil => rfl
  | .atom n vs => by simp [variantsP, PTy.instVariants, Variants.inst, variantsP_inst vs]
  | .term n t vs => by simp [variantsP, PTy.instVariants, Variants.inst, PTy.inst, variantsP_inst vs]

/-- Opening a known type a level, where it is not a type parameter, comes out
the same substituted. -/
theorem expand_inst {k : Ty} (h : ∀ n, k ≠ .var n) :
    expand (k.inst θ) = (expand k).map (PTy.inst θ) := by
  cases k <;> simp_all [expand, Ty.inst, PTy.inst, tysP_inst, fieldsP_inst, variantsP_inst]

theorem lookup_instVariants (a : String) : ∀ (vs : List (String × Option PTy)),
    ((PTy.instVariants θ vs).lookup a).join = ((vs.lookup a).join).map (PTy.inst θ)
  | [] => rfl
  | (n, none) :: vs => by
    simp only [PTy.instVariants, List.lookup]
    cases a == n <;> simp [lookup_instVariants a vs]
  | (n, some t) :: vs => by
    simp only [PTy.instVariants, List.lookup]
    cases a == n <;> simp [lookup_instVariants a vs]

theorem termPayloadP_inst (a : String) (vs : Variants) :
    termPayloadP a (vs.inst θ) = (termPayloadP a vs).map (PTy.inst θ) := by
  simp only [termPayloadP, ← variantsP_inst, lookup_instVariants]

end Datalove.Lit

namespace Datalove.Lit

variable {θ : List (String × Ty)}

/-! ## Stepping together -/

/-- The failures after which a run under an instance need not step with the
generic's: F110, and running out of fuel, where the generic may stop at a
step the instance takes otherwise. A generic that checks quietly met
neither. -/
def Escape (c : Code) : Prop := c = rigidCode ∨ c = "fuel"

/-- `m'` steps with `m` under `θ`, its answers `f` of `m`'s: from a state
substituted, it ends where `m` does, substituted, with `f` of `m`'s answer, or
fails as `m` does, unless `m` failed with F110 or ran out of fuel. -/
def Lock {α β} (θ : List (String × Ty)) (f : α → β) (m : InferM α) (m' : InferM β) : Prop :=
  ∀ s, match m.run s with
    | .ok (a, s1) => m'.run (State.inst θ s) = .ok (f a, State.inst θ s1)
    | .error c => Escape c ∨ m'.run (State.inst θ s) = .error c

namespace Lock

variable {α β γ δ : Type} {f : α → β} {g : γ → δ}

theorem pure' {a : α} {b : β} (h : b = f a) : Lock θ f (Pure.pure a) (Pure.pure b) := by
  intro s; simp [StateT.run, Pure.pure, StateT.pure, Except.pure, h]

theorem bind' {x : InferM α} {x' : InferM β} {k : α → InferM γ} {k' : β → InferM δ}
    (hx : Lock θ f x x') (hk : ∀ a, Lock θ g (k a) (k' (f a))) : Lock θ g (x >>= k) (x' >>= k') := by
  intro s
  have h := hx s
  simp only [StateT.run, bind, StateT.bind, Except.bind] at h ⊢
  revert h
  cases x s with
  | error c =>
    intro h
    rcases h with h | h
    · exact .inl h
    · simp [h]
  | ok r =>
    intro h
    simp only [h]
    exact hk r.1 r.2

theorem throw' {c : Code} : Lock θ f (throw c : InferM α) (throw c : InferM β) := by
  intro s; exact .inr rfl

theorem rigid {m' : InferM β} : Lock θ f (throw rigidCode : InferM α) m' := by
  intro s; exact .inl (.inl rfl)

theorem fuel {m' : InferM β} : Lock θ f (throw "fuel" : InferM α) m' := by
  intro s; exact .inl (.inr rfl)

/-- A `panic!` in inference answers the default and goes on. -/
theorem panic' [Inhabited α] [Inhabited β] {msg : String} (h : f default = default) :
    Lock θ f (panic msg : InferM α) (panic msg : InferM β) := by
  intro s
  simp [panic, panicCore, default, StateT.run, pure, StateT.pure,
    Except.pure, ← h]

theorem get' : Lock θ (State.inst θ) (get : InferM State) get := by
  intro s; rfl

theorem modify' {h : State → State} {h' : State → State}
    (e : ∀ s, State.inst θ (h s) = h' (State.inst θ s)) :
    Lock θ id (modify h : InferM PUnit) (modify h') := by
  intro s
  simp only [StateT.run, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, pure,
    Except.pure, e]

theorem set' {t : State} : Lock θ id (set t : InferM PUnit) (set (State.inst θ t)) := by
  intro s; rfl

/-- Two runs that step together but answer through another map. -/
theorem map_eq {f' : α → β} {m : InferM α} {m' : InferM β} (h : Lock θ f m m') (e : ∀ a, f a = f' a) :
    Lock θ f' m m' := by
  have : f = f' := funext e
  exact this ▸ h

end Lock

theorem findF_lock : ∀ (n v : Nat), Lock θ id (findF n v) (findF n v)
  | 0, _ => Lock.fuel
  | n + 1, v => by
    intro s
    have ih := findF_lock n
    simp only [findF, StateT.run, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get,
      pure, Except.pure, Except.bind] at ih ⊢
    rw [State.inst_get]
    cases hv : s[v]! with
    | link w => simp only [VarState.inst]; exact ih w s
    | free c => simp [VarState.inst, StateT.pure, Pure.pure, Except.pure]
    | bound b => simp [VarState.inst, StateT.pure, Pure.pure, Except.pure]

theorem find_lock (v : Nat) : Lock θ id (find v) (find v) := findF_lock _ v

theorem fresh_lock (c : Class) : Lock θ id (fresh c) (fresh (c.inst θ)) := by
  intro s
  simp [fresh, StateT.run, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, set,
    MonadStateOf.set, StateT.set, pure, Except.pure, Except.bind, StateT.pure, State.inst_push,
    State.inst_size, VarState.inst]

theorem prune_lock (t : PTy) : Lock θ (PTy.inst θ) (prune t) (prune (t.inst θ)) := by
  cases t
  case var v =>
    simp only [prune, PTy.inst]
    refine Lock.bind' (find_lock v) (fun r => ?_)
    refine Lock.bind' Lock.get' (fun st => ?_)
    simp only [id, State.inst_get]
    cases st[r]! <;> simp only [VarState.inst] <;> exact Lock.pure' (by simp [PTy.inst])
  all_goals exact Lock.pure' (by simp [PTy.inst])

theorem rootOf_lock (t : PTy) : Lock θ id (rootOf t) (rootOf (t.inst θ)) := by
  cases t
  case var v =>
    simp only [rootOf, PTy.inst, map_eq_pure_bind]
    exact Lock.bind' (find_lock v) (fun r => Lock.pure' rfl)
  all_goals exact Lock.pure' rfl

end Datalove.Lit

namespace Datalove.Lit
variable {θ : List (String × Ty)}

theorem Lock.rigid_bind {β γ : Type} {f : γ → β} {k : PUnit → InferM γ} {m' : InferM β} :
    Lock θ f ((throw rigidCode : InferM PUnit) >>= k) m' := by
  intro s; exact .inl (.inl rfl)

theorem Lock.panicDecl {α β : Type} [Inhabited α] [Inhabited β] {f : α → β}
    (h : f (default : α) = (default : β)) {a b : String} {l c : Nat} {msg : String} :
    Lock θ f (@panicWithPosWithDecl (InferM α) _ a b l c msg)
      (@panicWithPosWithDecl (InferM β) _ a b l c msg) :=
  Lock.panic' h

section
variable (g : Ground θ)
include g

theorem unionF_step (n x y : Nat) (ih : ∀ p q, Lock θ id (unifyF n p q) (unifyF n (p.inst θ) (q.inst θ))) :
    Lock θ id (unionF (n + 1) x y) (unionF (n + 1) x y) := by
  simp only [unionF]
  split
  · exact Lock.pure' rfl
  refine Lock.bind' Lock.get' (fun st => ?_)
  simp only [State.inst_get]
  cases hx : st[x]! with
  | free cx =>
    cases hy : st[y]! with
    | free cy =>
      simp only [VarState.inst]
      by_cases hd : cx.mergeDepends cy = true
      · simp only [hd, ↓reduceIte]; exact Lock.rigid_bind
      · have hd' := Class.mergeDepends_inst g (Bool.eq_false_iff.mpr hd)
        simp only [hd, hd', Bool.false_eq_true, ↓reduceIte, mergeClasses_inst g (Bool.eq_false_iff.mpr hd)]
        cases mergeClasses cx cy with
        | none => exact Lock.pure' rfl
        | some r =>
          obtain ⟨c, payloads⟩ := r
          refine Lock.bind' (Lock.modify' (fun s => by rw [State.inst_set, State.inst_set]; rfl)) (fun _ => ?_)
          cases payloads with
          | none => exact Lock.pure' rfl
          | some pq => exact ih pq.1 pq.2
    | _ => simp only [VarState.inst]; exact Lock.panicDecl (f := id) rfl
  | _ => simp only [VarState.inst]; exact Lock.panicDecl (f := id) rfl

end

/-- A run that always escapes steps with anything. -/
theorem Lock.of_escape {α β} {f : α → β} {m : InferM α} {m' : InferM β}
    (h : ∀ s, ∃ c, m.run s = .error c ∧ Escape c) : Lock θ f m m' := by
  intro s
  obtain ⟨c, hc, he⟩ := h s
  rw [hc]; exact .inl he

/-- Unifying a type parameter with a type with parts is F110, or runs out of
fuel first. -/
theorem unifyF_var_escape (nm : String) {t : PTy} (hv : ∀ x, t ≠ .var x) (hk : ∀ k, t ≠ .known k) :
    ∀ n s, ∃ c, (unifyF n (.known (.var nm)) t).run s = .error c ∧ Escape c
  | 0, s => ⟨"fuel", rfl, .inr rfl⟩
  | n + 1, s => by
    refine ⟨rigidCode, ?_, .inl rfl⟩
    cases t <;> simp_all [unifyF, knownOther, rootOf, prune, StateT.run, bind, StateT.bind, Except.bind, pure,
      StateT.pure, Except.pure, throw, throwThe, MonadExceptOf.throw, StateT.lift]

section
variable (g : Ground θ)
include g

theorem bindF_step (n v : Nat) (t : PTy) (ih : ∀ p q, Lock θ id (unifyF n p q) (unifyF n (p.inst θ) (q.inst θ))) :
    Lock θ id (bindF (n + 1) v t) (bindF (n + 1) v (t.inst θ)) := by
  simp only [bindF]
  refine Lock.bind' Lock.get' (fun st => ?_)
  simp only [State.inst_get]
  cases hv : st[v]! with
  | free c =>
    simp only [VarState.inst]
    by_cases hd : c.dependsOn t = true
    · simp only [hd, ↓reduceIte]; exact Lock.rigid_bind
    have hd0 := Bool.eq_false_iff.mpr hd
    simp only [hd0, Class.dependsOn_inst g hd0, Class.admits_inst g hd0, Bool.false_eq_true, ↓reduceIte]
    split
    · exact Lock.pure' rfl
    refine Lock.bind' (Lock.modify' (fun s => by rw [State.inst_set]; rfl)) (fun _ => ?_)
    rcases c with _ | _ | _ | _ | ⟨a, _ | p⟩ | f | _
    case variant.some =>
      simp only [Class.inst, Option.map]
      have fin : ∀ q, Lock θ id (unifyF n p q >>= fun _ => (pure () : InferM Unit))
          (unifyF n (p.inst θ) (q.inst θ) >>= fun _ => pure ()) :=
        fun q => Lock.bind' (ih p q) (fun _ => Lock.pure' rfl)
      rcases t with k | _ | _ | _ | _ | _ | _ | _ | _ | _ | _ | ⟨b, q⟩ | vs
      · cases k with
        | var nm => simp [Class.dependsOn, headVar, Ty.expose, Ty.exposeN, Ty.leadingMus] at hd0
        | term b q =>
          simp only [PTy.inst, Ty.inst]
          by_cases hab : (a == b) = true
          · simp only [hab, ↓reduceIte]; exact fin (.known q)
          · simp only [hab, Bool.false_eq_true, ↓reduceIte]; exact Lock.pure' rfl
        | enum vs =>
          simp only [PTy.inst, Ty.inst, termPayloadP_inst]
          cases termPayloadP a vs with
          | none => exact Lock.pure' rfl
          | some q => exact fin q
        | _ => simp only [PTy.inst, Ty.inst]; exact Lock.pure' rfl
      all_goals try (simp only [PTy.inst]; exact Lock.pure' rfl)
      · simp only [PTy.inst]
        by_cases hab : (a == b) = true
        · simp only [hab, ↓reduceIte]; exact fin q
        · simp only [hab, Bool.false_eq_true, ↓reduceIte]; exact Lock.pure' rfl
      · simp only [PTy.inst, lookup_instVariants]
        cases (vs.lookup a).join with
        | none => exact Lock.pure' rfl
        | some q => exact fin q
    case adapt =>
      simp only [Class.inst]
      rcases t with k | x | ts | fs | fs | e | e | ⟨k', v'⟩ | e | e | ⟨e, r⟩ | ⟨b, q⟩ | vs
      · simp only [PTy.inst]; exact Lock.pure' rfl
      rotate_left 11
      · cases f with
        | var nm =>
          exact Lock.of_escape (unifyF_var_escape nm (by simp) (by simp) n)
        | atom a => simp only [PTy.inst, Ty.inst]; exact Lock.pure' rfl
        | term a q =>
          simp only [PTy.inst, Ty.inst, lookup_instVariants]
          cases (vs.lookup a).join with
          | none => exact Lock.pure' rfl
          | some p => exact ih (.known q) p
        | _ => exact ih (.known _) (.enum vs)
      all_goals exact ih (.known _) _
    all_goals exact Lock.pure' rfl
  | _ => simp only [VarState.inst]; exact Lock.panicDecl (f := id) rfl

end

section
variable (g : Ground θ)
include g

theorem Ty.unfold_inst (b : Ty) : ((Ty.mu b).inst θ).unfold = (Ty.mu b).unfold.inst θ := by
  have := Ty.inst_subst g b 0 (.mu b)
  simp only [Ty.inst] at this ⊢
  simp only [Ty.unfold]
  exact this.symm

omit g in
theorem Ty.inst_not_mu {t : Ty} (hv : ∀ n, t ≠ .var n) (hm : ∀ b, t ≠ .mu b) :
    ∀ b, t.inst θ ≠ .mu b := by
  cases t <;> simp_all [Ty.inst]

omit g in
/-- The modification `knownOther` makes of a recursive type's holder. -/
theorem holder_lock {h : Nat} {b : Ty} :
    Lock θ id (modify fun x : State => x.setIfInBounds h (VarState.bound (PTy.known (.mu b))))
      (modify fun x : State => x.setIfInBounds h (VarState.bound (PTy.known (.mu (b.inst θ))))) :=
  Lock.modify' (fun s => by rw [State.inst_setIfInBounds]; rfl)

omit g in
theorem holder_lock' {h : Nat} {b : Ty} :
    Lock θ id (modify fun x : State => x.set! h (VarState.bound (PTy.known (.mu b))))
      (modify fun x : State => x.set! h (VarState.bound (PTy.known (.mu (b.inst θ))))) :=
  Lock.modify' (fun s => by rw [State.inst_set]; rfl)

-- The leaves are closed by one loop, and which lemmas a leaf needs varies.
set_option linter.unusedSimpArgs false in
theorem knownOther_lock {parts parts' : PTy → PTy → InferM Unit}
    (hp : ∀ p q, Lock θ id (parts p q) (parts' (p.inst θ) (q.inst θ))) (k : Ty) (other : PTy)
    (root : Option Nat) :
    Lock θ id (knownOther parts k other root) (knownOther parts' (k.inst θ) (other.inst θ) root) := by
  by_cases hm : ∃ b, k = .mu b
  · obtain ⟨b, rfl⟩ := hm
    have hu := Ty.unfold_inst g b
    simp only [Ty.inst] at hu
    simp only [knownOther, Ty.inst, hu]
    generalize (Ty.mu b).unfold = u
    by_cases hv : ∃ nm, u = .var nm
    · obtain ⟨nm, rfl⟩ := hv; exact Lock.rigid_bind
    have hu' : ∀ nm, u ≠ .var nm := fun nm e => hv ⟨nm, e⟩
    have hi := Ty.inst_not_var (θ := θ) hu'
    rw [expand_inst hu']
    cases root <;> cases expand u <;> dsimp only [Option.map]
    all_goals
      repeat' (first
        | (exfalso; simp_all; done)
        | exact Lock.pure' rfl
        | exact hp _ _
        | exact Lock.bind' holder_lock (fun _ => Lock.pure' rfl)
        | exact Lock.bind' holder_lock (fun _ => hp _ _)
        | exact Lock.bind' holder_lock' (fun _ => Lock.pure' rfl)
        | exact Lock.bind' holder_lock' (fun _ => hp _ _)
        | split)
  · by_cases hv : ∃ nm, k = .var nm
    · obtain ⟨nm, rfl⟩ := hv; simp only [knownOther]; exact Lock.rigid_bind
    have hk : ∀ nm, k ≠ .var nm := fun nm e => hv ⟨nm, e⟩
    have hkm : ∀ b, k ≠ .mu b := fun b e => hm ⟨b, e⟩
    have hi := Ty.inst_not_var (θ := θ) hk
    have him := Ty.inst_not_mu (θ := θ) hk hkm
    have he := expand_inst (θ := θ) hk
    generalize Ty.inst θ k = k' at hi him he
    unfold knownOther
    repeat' (first
      | (exfalso; simp_all; done)
      | split)
    all_goals
      dsimp only
      first
        | exact Lock.pure' rfl
        | (exfalso; simp_all; done)
        | (simp_all only [Option.map_some, Option.some.injEq]; done)
        | (simp_all only [Option.map_some, Option.some.injEq]; exact hp _ _)
        | (exfalso; cases hek : expand k <;> simp_all)
end

section
variable (g : Ground θ)
include g

theorem unifyF_step (n : Nat) (a b : PTy)
    (hU : ∀ x y, Lock θ id (unionF n x y) (unionF n x y))
    (hB : ∀ v t, Lock θ id (bindF n v t) (bindF n v (t.inst θ)))
    (hP : ∀ p q, Lock θ id (unifyPartsF n p q) (unifyPartsF n (p.inst θ) (q.inst θ))) :
    Lock θ id (unifyF (n + 1) a b) (unifyF (n + 1) (a.inst θ) (b.inst θ)) := by
  simp only [unifyF]
  refine Lock.bind' (rootOf_lock a) (fun aRoot => ?_)
  refine Lock.bind' (rootOf_lock b) (fun bRoot => ?_)
  refine Lock.bind' (prune_lock a) (fun a' => ?_)
  refine Lock.bind' (prune_lock b) (fun b' => ?_)
  simp only [id]
  rcases a' with ka | x | _ | _ | _ | _ | _ | _ | _ | _ | _ | _ | _ <;>
    rcases b' with kb | y | _ | _ | _ | _ | _ | _ | _ | _ | _ | _ | _ <;>
    simp only [PTy.inst]
  all_goals first
    | exact Lock.pure' rfl
    | exact hU _ _
    | exact hB _ _
    | exact hP _ _
    | exact knownOther_lock g hP _ _ _
    | trace_state

end

theorem Lock.forM {α β} {l : List α} {f : α → InferM PUnit} {f' : β → InferM PUnit} {h : α → β}
    (hf : ∀ a ∈ l, Lock θ id (f a) (f' (h a))) : Lock θ id (l.forM f) ((l.map h).forM f') := by
  induction l with
  | nil => exact Lock.pure' rfl
  | cons a as ih =>
    simp only [List.forM, List.map]
    exact Lock.bind' (hf a List.mem_cons_self) (fun _ => ih (fun b hb => hf b (List.mem_cons_of_mem _ hb)))

theorem zip_instList (xs ys : List PTy) :
    (PTy.instList θ xs).zip (PTy.instList θ ys) = (xs.zip ys).map fun (x, y) => (x.inst θ, y.inst θ) := by
  simp [PTy.instList_eq, List.zip_map]

theorem zip_instFields (xs ys : List (String × PTy)) :
    (PTy.instFields θ xs).zip (PTy.instFields θ ys) =
      (xs.zip ys).map fun ((xn, x), (yn, y)) => ((xn, x.inst θ), (yn, y.inst θ)) := by
  simp [PTy.instFields_eq, List.zip_map]

theorem unifyPartsF_step (n : Nat) (a b : PTy)
    (hU : ∀ p q, Lock θ id (unifyF n p q) (unifyF n (p.inst θ) (q.inst θ))) :
    Lock θ id (unifyPartsF (n + 1) a b) (unifyPartsF (n + 1) (a.inst θ) (b.inst θ)) := by
  rcases a with _ | _ | xs | xs | xs | x | x | ⟨xk, xv⟩ | x | x | ⟨x, xr⟩ | ⟨xn, x⟩ | xs <;>
    rcases b with _ | _ | ys | ys | ys | y | y | ⟨yk, yv⟩ | y | y | ⟨y, yr⟩ | ⟨yn, y⟩ | ys <;>
    simp only [unifyPartsF, PTy.inst]
  all_goals first
    | exact Lock.pure' rfl
    | exact hU _ _
    | exact Lock.bind' (hU _ _) (fun _ => hU _ _)
    | (split <;> first | exact hU _ _ | exact Lock.pure' rfl)
    | (rw [zip_instList]; exact Lock.forM (fun pr _ => hU pr.1 pr.2))
    | (rw [zip_instFields]
       refine Lock.forM (fun ⟨⟨xn, x⟩, ⟨yn, y⟩⟩ _ => ?_)
       dsimp only
       by_cases h : (xn == yn) = true <;> simp only [h, ↓reduceIte, Bool.false_eq_true]
       · exact hU x y
       · exact Lock.pure' rfl)
    | (rw [PTy.instVariants_eq xs]
       refine Lock.forM (fun ⟨k, p⟩ _ => ?_)
       dsimp only
       rw [lookup_instVariants]
       cases p <;> cases (List.lookup k ys).join <;> dsimp only [Option.map] <;>
         first | exact Lock.pure' rfl | exact hU _ _)

section
variable (g : Ground θ)
include g

/-- The unifier steps with itself under a ground substitution. -/
theorem unify_locks : ∀ n,
    (∀ a b, Lock θ id (unifyF n a b) (unifyF n (a.inst θ) (b.inst θ))) ∧
    (∀ a b, Lock θ id (unifyPartsF n a b) (unifyPartsF n (a.inst θ) (b.inst θ))) ∧
    (∀ x y, Lock θ id (unionF n x y) (unionF n x y)) ∧
    (∀ v t, Lock θ id (bindF n v t) (bindF n v (t.inst θ)))
  | 0 => by
    refine ⟨fun _ _ => ?_, fun _ _ => ?_, fun _ _ => ?_, fun _ _ => ?_⟩ <;>
      simp only [unifyF, unifyPartsF, unionF, bindF] <;> exact Lock.fuel
  | n + 1 =>
    have ⟨hU, hP, hUn, hB⟩ := unify_locks n
    ⟨fun a b => unifyF_step g n a b hUn hB hP, fun a b => unifyPartsF_step n a b hU,
      fun x y => unionF_step g n x y hU, fun v t => bindF_step g n v t hU⟩

theorem unify_lock (a b : PTy) : Lock θ id (unify a b) (unify (a.inst θ) (b.inst θ)) :=
  (unify_locks g inferFuel).1 a b

end

theorem Lock.mapM {α β γ δ} {l : List α} {k : α → InferM γ} {k' : β → InferM δ} {h : α → β}
    {f : γ → δ} (hk : ∀ a ∈ l, Lock θ f (k a) (k' (h a))) :
    Lock θ (List.map f) (l.mapM k) ((l.map h).mapM k') := by
  induction l with
  | nil => exact Lock.pure' rfl
  | cons a as ih =>
    simp only [List.mapM_cons, List.map]
    exact Lock.bind' (hk a List.mem_cons_self) (fun b =>
      Lock.bind' (ih (fun c hc => hk c (List.mem_cons_of_mem _ hc))) (fun bs => Lock.pure' rfl))

theorem Lock.map' {α β γ δ} {m : InferM α} {m' : InferM β} {f : α → β} {u : α → γ} {u' : β → δ}
    {f' : γ → δ} (h : Lock θ f m m') (e : ∀ a, u' (f a) = f' (u a)) :
    Lock θ f' (u <$> m) (u' <$> m') := by
  simp only [map_eq_pure_bind]
  exact Lock.bind' h (fun a => Lock.pure' (e a))

theorem Tys.inst_foldr (rs : List Ty) :
    Tys.inst θ (rs.foldr .cons .nil) = (rs.map (Ty.inst θ)).foldr .cons .nil := by
  induction rs <;> simp_all [Tys.inst]

theorem Fields.inst_foldr (rs : List (String × Ty)) :
    Fields.inst θ (rs.foldr (fun (k, t) out => .cons k t out) .nil) =
      (rs.map fun (k, t) => (k, t.inst θ)).foldr (fun (k, t) out => .cons k t out) .nil := by
  induction rs <;> simp_all [Fields.inst]

theorem Variants.inst_foldr (rs : List (String × Option Ty)) :
    Variants.inst θ (rs.foldr (fun (k, p) out => match p with
        | none => .atom k out
        | some t => .term k t out) .nil) =
      (rs.map fun (k, p) => (k, p.map (Ty.inst θ))).foldr (fun (k, p) out => match p with
        | none => .atom k out
        | some t => .term k t out) .nil := by
  induction rs with
  | nil => rfl
  | cons r rs ih =>
    obtain ⟨k, _ | t⟩ := r <;> simp [Variants.inst, ih]

theorem resolveF_fields_lock (n : Nat)
    (ih : ∀ t, Lock θ (Ty.inst θ) (resolveF n t) (resolveF n (t.inst θ))) (fs : List (String × PTy)) :
    Lock θ (Fields.inst θ) (resolveF.fields n fs) (resolveF.fields n (PTy.instFields θ fs)) := by
  simp only [resolveF.fields, PTy.instFields_eq]
  refine Lock.bind' (Lock.mapM (fun (kt : String × PTy) _ => ?_)) (fun rs => Lock.pure' (Fields.inst_foldr rs).symm)
  exact Lock.bind' (ih kt.2) (fun u => Lock.pure' rfl)

theorem resolveF_lock : ∀ (n : Nat) (t : PTy), Lock θ (Ty.inst θ) (resolveF n t) (resolveF n (t.inst θ))
  | 0, _ => by rw [resolveF.eq_def, resolveF.eq_def]; exact Lock.fuel
  | n + 1, t => by
    have ih := resolveF_lock n
    simp only [resolveF]
    refine Lock.bind' (prune_lock t) (fun p => ?_)
    rcases p with k | v | ts | fs | fs | e | e | ⟨kk, vv⟩ | e | e | ⟨e, r⟩ | ⟨a, e⟩ | vs <;>
      simp only [PTy.inst]
    · exact Lock.pure' rfl
    · refine Lock.bind' Lock.get' (fun st => ?_)
      simp only [State.inst_get]
      cases st[v]! with
      | free c =>
        rcases c with _ | _ | _ | _ | ⟨a, _ | p⟩ | f | _ <;>
          simp only [VarState.inst, Class.inst, Option.map]
        all_goals first
          | exact Lock.pure' rfl
          | exact Lock.throw'
          | exact Lock.map' (ih p) (fun _ => rfl)
      | _ => exact Lock.panicDecl (α := Ty) (β := Ty) (f := Ty.inst θ) rfl
    · rw [PTy.instList_eq]
      exact Lock.bind' (Lock.mapM (h := PTy.inst θ) (fun a _ => ih a))
        (fun rs => Lock.pure' (by simp [Ty.inst, Tys.inst_foldr]))
    · exact Lock.map' (resolveF_fields_lock n ih fs) (fun _ => rfl)
    · exact Lock.map' (resolveF_fields_lock n ih fs) (fun _ => rfl)
    · exact Lock.map' (ih e) (fun _ => rfl)
    · exact Lock.map' (ih e) (fun _ => rfl)
    · simp only [seq_eq_bind_map, map_eq_pure_bind, bind_assoc, pure_bind]
      exact Lock.bind' (ih kk) (fun a => Lock.bind' (ih vv) (fun b => Lock.pure' rfl))
    · exact Lock.map' (ih e) (fun _ => rfl)
    · exact Lock.map' (ih e) (fun _ => rfl)
    · exact Lock.map' (ih e) (fun _ => rfl)
    · exact Lock.map' (ih e) (fun _ => rfl)
    · rw [PTy.instVariants_eq]
      refine Lock.bind' (Lock.mapM (f := fun (kp : String × Option Ty) => (kp.1, kp.2.map (Ty.inst θ)))
          (fun (kp : String × Option PTy) _ => ?_))
        (fun rs => Lock.pure' ?_)
      rotate_left
      · simp only [Ty.inst]
        congr 1
        induction rs with
        | nil => rfl
        | cons r rs ih => obtain ⟨k, _ | t⟩ := r <;> simp_all [Variants.inst]
      obtain ⟨k, p⟩ := kp
      cases p with
      | none => exact Lock.bind' (Lock.pure' (f := Option.map (Ty.inst θ)) rfl) (fun _ => Lock.pure' rfl)
      | some q =>
        simp only [Option.map, Option.mapM]
        exact Lock.bind' (Lock.map' (f' := Option.map (Ty.inst θ)) (ih q) (fun _ => rfl))
          (fun _ => Lock.pure' rfl)

end Datalove.Lit

/-! ## Hints read the same under an instance -/

namespace Datalove

variable {θ : List (String × Ty)}

/-- `m'` runs as `m` does, from the same scope to the same scope, its answer
`f` of `m`'s, and fails as `m` does, but where `m` fails for a type
parameter inside a `rec` (T066), which an instance has none of. -/
def SimE {α β} (f : α → β) (m : ConvM α) (m' : ConvM β) : Prop :=
  ∀ s, match m.run s with
    | .ok (a, s') => m'.run s = .ok (f a, s')
    | .error e => e.kind = .generic ∨ m'.run s = .error e

namespace SimE

variable {α β γ δ : Type} {f : α → β} {g : γ → δ}

theorem bind' {x : ConvM α} {x' : ConvM β} {k : α → ConvM γ} {k' : β → ConvM δ}
    (hx : SimE f x x') (hk : ∀ a, SimE g (k a) (k' (f a))) : SimE g (x >>= k) (x' >>= k') := by
  intro s
  have h := hx s
  simp only [StateT.run, bind, StateT.bind, Except.bind] at h ⊢
  revert h
  cases x s with
  | error c =>
    intro h
    rcases h with h | h
    · exact .inl h
    · simp [h]
  | ok r =>
    intro h
    simp only [h]
    exact hk r.1 r.2

theorem pure' {a : α} {b : β} (h : b = f a) : SimE f (Pure.pure a) (Pure.pure b) := by
  intro s; simp [StateT.run, Pure.pure, StateT.pure, Except.pure, h]

theorem refl (m : ConvM α) : SimE id m m := by
  intro s
  cases m.run s <;> simp

theorem map' {m : ConvM α} {m' : ConvM β} {u : α → γ} {u' : β → δ}
    (h : SimE f m m') (e : ∀ a, u' (f a) = g (u a)) : SimE g (u <$> m) (u' <$> m') := by
  simp only [map_eq_pure_bind]
  exact bind' h (fun a => pure' (e a))

theorem collection {m : ConvM α} {m' : ConvM β} (key : Bool) (h : SimE f m m') :
    SimE f (ConvM.collection key m) (ConvM.collection key m') := by
  simp only [ConvM.collection]
  exact bind' (refl _) fun s => bind' (refl _) fun _ =>
    bind' h fun r => bind' (refl _) fun _ => pure' rfl

theorem fail' {k : ConvErrKind} {n : String} : SimE f (ConvM.fail k n : ConvM α) (ConvM.fail k n : ConvM β) := by
  intro s; exact .inr rfl

end SimE

theorem SimE.generic {α β} {f : α → β} {n : String} {m' : ConvM β} :
    SimE f (ConvM.fail .generic n : ConvM α) m' := by
  intro s; exact .inl rfl

theorem ConvM.lookup_instE (g : Ground θ) (A : Aliases) (n : String) :
    SimE (·.inst θ) (ConvM.lookup A n) (ConvM.lookup (A.inst θ) n) := by
  simp only [ConvM.lookup]
  refine SimE.bind' (SimE.refl get) (fun s0 => ?_)
  simp only [id, Aliases.lookup_inst]
  cases hf : List.findIdx? (fun x => x.name == n) s0.binders with
  | none =>
    simp only
    cases hu : A.lookup n with
    | none => exact SimE.fail'
    | some u =>
      simp only [Option.map_some, Ty.containsRec_inst g]
      by_cases hk : (s0.inKey && u.containsRec) = true
      · simp only [hk, ↓reduceIte]; exact SimE.fail'
      · simp only [hk, Bool.false_eq_true, ↓reduceIte]
        by_cases hv : (!s0.binders.isEmpty && u.containsVar) = true
        · simp only [hv, ↓reduceIte]; exact SimE.generic
        · have hv' : (!s0.binders.isEmpty && (u.inst θ).containsVar) = false := by
            cases hb : s0.binders.isEmpty
            · have hv0 : u.containsVar = false := by simpa [hb] using hv
              simp [Ty.inst_novar θ u hv0, hv0]
            · simp
          simp only [hv, hv', Bool.false_eq_true, ↓reduceIte]
          exact SimE.pure' rfl
  | some d =>
    simp only
    repeat' (first
      | exact SimE.fail'
      | exact SimE.pure' rfl
      | refine SimE.bind' (SimE.refl _) (fun _ => ?_)
      | split)

theorem ConvM.bind_instE (A : Aliases) (n : String) :
    SimE id (ConvM.bind A n) (ConvM.bind (A.inst θ) n) := by
  rw [ConvM.bind_inst]; exact SimE.refl _

open ConvM in
mutual

theorem Hint.conv_instE (g : Ground θ) (A : Aliases) :
    ∀ (h : Hint), SimE (·.inst θ) (h.conv A) (h.conv (A.inst θ))
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset
  | .f32 | .f64 | .int | .string | .data | .error | .atom _ => by
    simp only [Hint.conv]; exact SimE.pure' rfl
  | .tuple fs => by simp only [Hint.conv]; exact SimE.map' (Hints.conv_instE g A fs) (fun _ => rfl)
  | .struct fs => by simp only [Hint.conv]; exact SimE.map' (HFields.conv_instE g A fs) (fun _ => rfl)
  | .table cs => by
    simp only [Hint.conv]; exact SimE.map' (HFields.convColumns_instE g A cs) (fun _ => rfl)
  | .list t => by
    simp only [Hint.conv]; exact SimE.map' (SimE.collection _ (Hint.conv_instE g A t)) (fun _ => rfl)
  | .set t => by
    simp only [Hint.conv]; exact SimE.map' (SimE.collection _ (Hint.conv_instE g A t)) (fun _ => rfl)
  | .tensor t _ => by
    simp only [Hint.conv]; exact SimE.map' (SimE.collection _ (Hint.conv_instE g A t)) (fun _ => rfl)
  | .option t => by simp only [Hint.conv]; exact SimE.map' (Hint.conv_instE g A t) (fun _ => rfl)
  | .result t => by simp only [Hint.conv]; exact SimE.map' (Hint.conv_instE g A t) (fun _ => rfl)
  | .term _ t => by simp only [Hint.conv]; exact SimE.map' (Hint.conv_instE g A t) (fun _ => rfl)
  | .map k v => by
    simp only [Hint.conv]
    exact SimE.bind' (SimE.collection _ (Hint.conv_instE g A k)) fun _ =>
      SimE.bind' (SimE.collection _ (Hint.conv_instE g A v)) fun _ => SimE.pure' rfl
  | .enum vs => by
    simp only [Hint.conv]
    exact SimE.map' (HVariants.conv_instE g A vs)
      (fun _ => by simp [Ty.inst, Variants.sortByName_inst])
  | .mu n body => by
    simp only [Hint.conv, ConvM.bind_inst]
    exact SimE.bind' (SimE.refl _) fun _ => SimE.bind' (Hint.conv_instE g A body) fun _ =>
      SimE.bind' (SimE.refl _) fun _ => SimE.pure' rfl
  | .name n => by simp only [Hint.conv]; exact ConvM.lookup_instE g A n

theorem Hints.conv_instE (g : Ground θ) (A : Aliases) :
    ∀ (hs : Hints), SimE (·.inst θ) (hs.conv A) (hs.conv (A.inst θ))
  | .nil => by simp only [Hints.conv]; exact SimE.pure' rfl
  | .cons t ts => by
    simp only [Hints.conv]
    exact SimE.bind' (Hint.conv_instE g A t) fun _ => SimE.bind' (Hints.conv_instE g A ts) fun _ =>
      SimE.pure' rfl

theorem HFields.conv_instE (g : Ground θ) (A : Aliases) :
    ∀ (fs : HFields), SimE (·.inst θ) (fs.conv A) (fs.conv (A.inst θ))
  | .nil => by simp only [HFields.conv]; exact SimE.pure' rfl
  | .cons _ t fs => by
    simp only [HFields.conv]
    exact SimE.bind' (Hint.conv_instE g A t) fun _ => SimE.bind' (HFields.conv_instE g A fs) fun _ =>
      SimE.pure' rfl

theorem HFields.convColumns_instE (g : Ground θ) (A : Aliases) :
    ∀ (fs : HFields), SimE (·.inst θ) (fs.convColumns A) (fs.convColumns (A.inst θ))
  | .nil => by simp only [HFields.convColumns]; exact SimE.pure' rfl
  | .cons _ t fs => by
    simp only [HFields.convColumns]
    exact SimE.bind' (SimE.collection _ (Hint.conv_instE g A t)) fun _ =>
      SimE.bind' (HFields.convColumns_instE g A fs) fun _ => SimE.pure' rfl

theorem HVariants.conv_instE (g : Ground θ) (A : Aliases) :
    ∀ (vs : HVariants), SimE (·.inst θ) (vs.conv A) (vs.conv (A.inst θ))
  | .nil => by simp only [HVariants.conv]; exact SimE.pure' rfl
  | .atom _ vs => by
    simp only [HVariants.conv]; exact SimE.map' (HVariants.conv_instE g A vs) (fun _ => rfl)
  | .term _ t vs => by
    simp only [HVariants.conv]
    exact SimE.bind' (Hint.conv_instE g A t) fun _ => SimE.bind' (HVariants.conv_instE g A vs) fun _ =>
      SimE.pure' rfl

end

/-- A hint reads under an instance's aliases as it does under the generic's,
its type substituted, but where the generic's has a type parameter inside a
`rec` (T066). -/
theorem Fun.convertIn_inst (g : Ground θ) (A : Aliases) (h : Hint) :
    match Fun.convertIn A h with
    | .ok t => Fun.convertIn (A.inst θ) h = .ok (t.inst θ)
    | .error c => c = "T066" ∨ Fun.convertIn (A.inst θ) h = .error c := by
  have hs := Hint.conv_instE g A h {}
  simp only [Fun.convertIn, Hint.convertWith]
  revert hs
  cases (h.conv A).run {} with
  | ok r => intro hs; simp only [hs, Except.map]
  | error e =>
    intro hs
    rcases hs with hk | hs
    · left; simp [hk, Fun.convErrCode, Lit.errCode]
    · right; simp [hs, Except.map]

end Datalove

/-! ## Datafun's inference steps together -/

namespace Datalove.Lit

variable {θ : List (String × Ty)}

/-- Bind, knowing a fact `P` of every answer the first part gives. -/
theorem Lock.bindP {α β γ δ} {f : α → β} {g : γ → δ} {P : α → Prop} {x : InferM α} {x' : InferM β}
    {k : α → InferM γ} {k' : β → InferM δ} (hx : Lock θ f x x')
    (hP : ∀ s a s', x.run s = .ok (a, s') → P a) (hk : ∀ a, P a → Lock θ g (k a) (k' (f a))) :
    Lock θ g (x >>= k) (x' >>= k') := by
  intro s
  have h := hx s
  have hp := hP s
  simp only [StateT.run, bind, StateT.bind, Except.bind] at h hp ⊢
  revert h hp
  cases x s with
  | error c =>
    intro h _
    rcases h with h | h
    · exact .inl h
    · simp [h]
  | ok r =>
    intro h hp
    simp only [h]
    exact hk r.1 (hp r.1 r.2 rfl) r.2

end Datalove.Lit

namespace Datalove.Lit

variable {θ : List (String × Ty)}

theorem Lock.forIn {α : Type} {l : List α} {f f' : α → PUnit → InferM (ForInStep PUnit)}
    (h : ∀ a ∈ l, ∀ u, Lock θ id (f a u) (f' a u)) : Lock θ id (forIn l PUnit.unit f) (forIn l PUnit.unit f') := by
  induction l with
  | nil => exact Lock.pure' rfl
  | cons a as ih =>
    simp only [List.forIn_cons]
    refine Lock.bind' (h a List.mem_cons_self _) (fun r => ?_)
    cases r with
    | done u => exact Lock.pure' rfl
    | yield u => exact ih (fun b hb => h b (List.mem_cons_of_mem _ hb))

theorem Lock.foldlM {α β : Type} {l : List α} {f : β → α → InferM β} {f' : β → α → InferM β} {u : β → β}
    (h : ∀ b a, Lock θ u (f b a) (f' (u b) a)) (b : β) :
    Lock θ u (l.foldlM f b) (l.foldlM f' (u b)) := by
  induction l generalizing b with
  | nil => exact Lock.pure' rfl
  | cons a as ih =>
    simp only [List.foldlM_cons]
    exact Lock.bind' (h b a) (fun b' => ih b')

theorem Lock.mapM_id {α γ δ : Type} {l : List α} {k : α → InferM γ} {k' : α → InferM δ} {f : γ → δ}
    (hk : ∀ a ∈ l, Lock θ f (k a) (k' a)) : Lock θ (List.map f) (l.mapM k) (l.mapM k') := by
  have := Lock.mapM (h := id) hk
  simpa using this

theorem fresh_lockP (c : Class) : Lock θ (PTy.inst θ) (fresh c) (fresh (c.inst θ)) := by
  intro s
  simp [fresh, StateT.run, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, set,
    MonadStateOf.set, StateT.set, pure, Except.pure, Except.bind, StateT.pure, State.inst_push,
    State.inst_size, VarState.inst, PTy.inst]

end Datalove.Lit

namespace Datalove.Fun

open Lit (PTy State Lock Escape Code unknownCode)

variable {θ : List (String × Ty)}

/-- What `shallow` gives as a known type is one with no parts to open. -/
def Opened (p : PTy) : Prop := ∀ k, p = .known k → Lit.expand k.expose = none ∧ Lit.headVar k = false

section
variable (g : Ground θ)
include g

theorem shallow_lock (t : PTy) : Lock θ (PTy.inst θ) (shallow t) (shallow (t.inst θ)) := by
  simp only [shallow]
  refine Lock.bind' (Lit.prune_lock t) (fun p => ?_)
  cases p
  case known k =>
    simp only [PTy.inst]
    by_cases hv : Lit.headVar k = true
    · simp only [hv, ↓reduceIte]; exact Lock.rigid_bind
    · have hv0 := Bool.eq_false_iff.mpr hv
      have he : ∀ n, k.expose ≠ .var n := by intro n e; simp [Lit.headVar, e] at hv0
      simp only [hv0, Lit.headVar_inst g hv0, Bool.false_eq_true, ↓reduceIte]
      refine Lock.pure' ?_
      rw [Ty.expose_inst g, Lit.expand_inst he]
      cases Lit.expand k.expose <;> rfl
  all_goals exact Lock.pure' rfl

omit g in
theorem expand_not_known {k : Ty} {e : PTy} (h : Lit.expand k = some e) : ∀ t, e ≠ .known t := by
  cases k <;> simp [Lit.expand] at h <;> subst h <;> simp

omit g in
theorem shallow_opened (t : PTy) : ∀ s a s', (shallow t).run s = .ok (a, s') → Opened a := by
  intro s a s' h k hk
  subst hk
  simp only [shallow, StateT.run, bind, StateT.bind, Except.bind] at h
  cases hp : Lit.prune t s with
  | error c => simp [hp] at h
  | ok r =>
    obtain ⟨p, s1⟩ := r
    simp only [hp] at h
    cases p with
    | known k' =>
      by_cases hv : Lit.headVar k' = true
      · simp [hv, throw, throwThe, MonadExceptOf.throw, StateT.lift, StateT.bind, bind, Except.bind] at h
      · have hv0 := Bool.eq_false_iff.mpr hv
        simp only [hv0, Bool.false_eq_true, ↓reduceIte, pure, StateT.pure, Except.pure,
          Except.ok.injEq, Prod.mk.injEq] at h
        cases he : Lit.expand k'.expose with
        | none => simp only [he, Option.getD_none, PTy.known.injEq] at h; obtain ⟨rfl, -⟩ := h; exact ⟨he, hv0⟩
        | some e => simp only [he, Option.getD_some] at h; exact absurd h.1 (expand_not_known he k)
    | _ => simp [pure, StateT.pure, Except.pure] at h
end

theorem fieldType_noparts {t : Ty} (he : Lit.expand t.expose = none) (s : Sel) :
    fieldType t s = .error "F068" := by
  unfold fieldType
  cases h : t.expose <;> simp_all [Lit.expand] <;> cases s <;> rfl

section
variable (g : Ground θ)
include g

omit g in
theorem fresh_lock' (c : Lit.Class) (hc : c.inst θ = c) : Lock θ (PTy.inst θ) (Lit.fresh c) (Lit.fresh c) := by
  intro s
  simp [Lit.fresh, StateT.run, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, set,
    MonadStateOf.set, StateT.set, pure, Except.pure, Except.bind, StateT.pure, Lit.State.inst_push,
    Lit.State.inst_size, Lit.VarState.inst, PTy.inst, hc]

theorem inferField_lock (b : PTy) (sel : Sel) :
    Lock θ (PTy.inst θ) (inferField b sel) (inferField (b.inst θ) sel) := by
  simp only [inferField]
  refine Lock.bindP (shallow_lock g b) (shallow_opened b) (fun p hp => ?_)
  rcases p with k | _ | ts | fs | _ | _ | _ | _ | _ | _ | _ | _ | _ <;>
    rcases sel with nm | i <;> simp only [PTy.inst]
  all_goals try exact fresh_lock' .unknown rfl
  all_goals try (
    obtain ⟨he, hv⟩ := hp k rfl
    have he' : Lit.expand (k.inst θ).expose = none := by
      have hn : ∀ n, k.expose ≠ .var n := by intro n e; simp [Lit.headVar, e] at hv
      rw [Ty.expose_inst g, Lit.expand_inst hn, he]; rfl
    simp only [fieldType_noparts he, fieldType_noparts he']
    exact fresh_lock' .unknown rfl)
  · rw [Lit.PTy.instList_eq, List.getElem?_map]
    cases ts[i]? with
    | none => exact fresh_lock' .unknown rfl
    | some t => exact Lock.pure' rfl
  · rw [Lit.PTy.instFields_eq, lookup_map_snd]
    cases fs.lookup nm with
    | none => exact fresh_lock' .unknown rfl
    | some t => exact Lock.pure' rfl

theorem elementOf_lock (b : PTy) : Lock θ (PTy.inst θ) (elementOf b) (elementOf (b.inst θ)) := by
  simp only [elementOf]
  refine Lock.bind' (shallow_lock g b) (fun p => ?_)
  rcases p with _ | _ | _ | _ | _ | _ | _ | _ | _ | _ | ⟨e, r⟩ | _ | _ <;> simp only [PTy.inst]
  all_goals first
    | exact fresh_lock' .unknown rfl
    | exact Lock.pure' rfl
    | (split <;> exact Lock.pure' rfl)

end

mutual

/-- Under a `rec` binder a well-formed type holds no type parameter. -/
theorem Ty.wf_binders_novar : ∀ (t : Ty) (c : WfCtx), c.binders ≠ [] → t.wfIn c = true →
    t.containsVar = false
  | .var _, c, hb, h => by simp [Ty.wfIn] at h; exact absurd h hb
  | .mu b, c, hb, h => by
    simp only [Ty.wfIn, Bool.and_eq_true] at h
    simp only [Ty.containsVar]
    exact Ty.wf_binders_novar b _ (by simp) h.2
  | .tuple ts, c, hb, h => by
    simp only [Ty.wfIn] at h; simp only [Ty.containsVar]; exact Tys.wf_binders_novar ts c hb h
  | .struct fs, c, hb, h => by
    simp only [Ty.wfIn] at h; simp only [Ty.containsVar]; exact Fields.wf_binders_novar fs c hb h
  | .table fs, c, hb, h => by
    simp only [Ty.wfIn] at h; simp only [Ty.containsVar]; exact Fields.wfColumns_binders_novar fs c hb h
  | .list t, c, hb, h | .set t, c, hb, h | .tensor t _, c, hb, h => by
    simp only [Ty.wfIn] at h; simp only [Ty.containsVar]
    exact Ty.wf_binders_novar t _ (by simpa [WfCtx.coll] using hb) h
  | .option t, c, hb, h | .result t, c, hb, h | .term _ t, c, hb, h => by
    simp only [Ty.wfIn] at h; simp only [Ty.containsVar]; exact Ty.wf_binders_novar t c hb h
  | .map k v, c, hb, h => by
    simp only [Ty.wfIn, Bool.and_eq_true] at h
    simp only [Ty.containsVar, Bool.or_eq_false_iff]
    exact ⟨Ty.wf_binders_novar k _ (by simpa [WfCtx.coll] using hb) h.1,
      Ty.wf_binders_novar v _ (by simpa [WfCtx.coll] using hb) h.2⟩
  | .enum vs, c, hb, h => by
    simp only [Ty.wfIn] at h; simp only [Ty.containsVar]; exact Variants.wf_binders_novar vs c hb h
  | .bool, _, _, _ | .u8, _, _, _ | .i8, _, _, _ | .u16, _, _, _ | .i16, _, _, _ | .u32, _, _, _
  | .i32, _, _, _ | .u64, _, _, _ | .i64, _, _, _ | .index, _, _, _ | .offset, _, _, _
  | .f32, _, _, _ | .f64, _, _, _ | .int, _, _, _ | .string, _, _, _ | .data, _, _, _
  | .error, _, _, _ | .atom _, _, _, _ | .recvar _, _, _, _ => rfl

theorem Tys.wf_binders_novar : ∀ (ts : Tys) (c : WfCtx), c.binders ≠ [] → ts.wfIn c = true →
    ts.containsVar = false
  | .nil, _, _, _ => rfl
  | .cons t ts, c, hb, h => by
    simp only [Tys.wfIn, Bool.and_eq_true] at h
    simp only [Tys.containsVar, Bool.or_eq_false_iff]
    exact ⟨Ty.wf_binders_novar t c hb h.1, Tys.wf_binders_novar ts c hb h.2⟩

theorem Fields.wf_binders_novar : ∀ (fs : Fields) (c : WfCtx), c.binders ≠ [] → fs.wfIn c = true →
    fs.containsVar = false
  | .nil, _, _, _ => rfl
  | .cons _ t fs, c, hb, h => by
    simp only [Fields.wfIn, Bool.and_eq_true] at h
    simp only [Fields.containsVar, Bool.or_eq_false_iff]
    exact ⟨Ty.wf_binders_novar t c hb h.1, Fields.wf_binders_novar fs c hb h.2⟩

theorem Fields.wfColumns_binders_novar : ∀ (fs : Fields) (c : WfCtx), c.binders ≠ [] →
    fs.wfColumnsIn c = true → fs.containsVar = false
  | .nil, _, _, _ => rfl
  | .cons _ t fs, c, hb, h => by
    simp only [Fields.wfColumnsIn, Bool.and_eq_true] at h
    simp only [Fields.containsVar, Bool.or_eq_false_iff]
    exact ⟨Ty.wf_binders_novar t _ (by simpa [WfCtx.coll] using hb) h.1,
      Fields.wfColumns_binders_novar fs c hb h.2⟩

theorem Variants.wf_binders_novar : ∀ (vs : Variants) (c : WfCtx), c.binders ≠ [] →
    vs.wfIn c = true → vs.containsVar = false
  | .nil, _, _, _ => rfl
  | .atom _ vs, c, hb, h => by
    simp only [Variants.wfIn] at h; simp only [Variants.containsVar]
    exact Variants.wf_binders_novar vs c hb h
  | .term _ t vs, c, hb, h => by
    simp only [Variants.wfIn, Bool.and_eq_true] at h
    simp only [Variants.containsVar, Bool.or_eq_false_iff]
    exact ⟨Ty.wf_binders_novar t c hb h.1, Variants.wf_binders_novar vs c hb h.2⟩

end

/-- A call's type parameters' variables, substituted. -/
def instPs (θ : List (String × Ty)) (ps : List (String × PTy)) : List (String × PTy) :=
  ps.map fun (p, x) => (p, x.inst θ)

mutual

/-- Instantiating a signature's well-formed type, which names only the call's
type parameters, commutes with substituting the caller's. -/
theorem instantiate_inst (ps : List (String × PTy)) : ∀ (t : Ty) (c : WfCtx), c.binders = [] →
    t.wfIn c = true → t.allVars (fun n => (ps.lookup n).isSome) = true →
    (instantiate ps t).inst θ = instantiate (instPs θ ps) t := by
  intro t c hb hw ha
  by_cases hv : t.containsVar = false
  · rw [instantiate.eq_def, instantiate.eq_def]
    simp only [hv, Bool.not_false, ↓reduceIte, PTy.inst, Ty.inst_novar θ t hv]
  have hv' : t.containsVar = true := by simpa using hv
  rw [instantiate.eq_def, instantiate.eq_def]
  simp only [hv', Bool.not_true, Bool.false_eq_true, ↓reduceIte]
  cases t
  case var n =>
    simp only [Ty.allVars] at ha
    simp only [instPs, lookup_map_snd]
    cases hl : ps.lookup n with
    | none => simp [hl] at ha
    | some x => rfl
  case mu b =>
    simp only [Ty.wfIn, Bool.and_eq_true] at hw
    have := Ty.wf_binders_novar b _ (by simp) hw.2
    simp [Ty.containsVar, this] at hv'
  case tuple ts =>
    simp only [Ty.wfIn] at hw; simp only [Ty.allVars] at ha
    simp only [PTy.inst, instTys_inst ps ts c hb hw ha]
  case struct fs =>
    simp only [Ty.wfIn] at hw; simp only [Ty.allVars] at ha
    simp only [PTy.inst, instFields_inst ps fs c hb hw ha]
  case table fs =>
    simp only [Ty.wfIn] at hw; simp only [Ty.allVars] at ha
    simp only [PTy.inst, instColumns_inst ps fs c hb hw ha]
  case list e | set e | tensor e _ =>
    simp only [Ty.wfIn] at hw; simp only [Ty.allVars] at ha
    simp only [PTy.inst, instantiate_inst ps e _ (by simpa [WfCtx.coll] using hb) hw ha]
  case option e | result e | term _ e =>
    simp only [Ty.wfIn] at hw; simp only [Ty.allVars] at ha
    simp only [PTy.inst, instantiate_inst ps e c hb hw ha]
  case map k v =>
    simp only [Ty.wfIn, Bool.and_eq_true] at hw; simp only [Ty.allVars, Bool.and_eq_true] at ha
    simp only [PTy.inst, instantiate_inst ps k _ (by simpa [WfCtx.coll] using hb) hw.1 ha.1,
      instantiate_inst ps v _ (by simpa [WfCtx.coll] using hb) hw.2 ha.2]
  case enum vs =>
    simp only [Ty.wfIn] at hw; simp only [Ty.allVars] at ha
    simp only [PTy.inst, instVariants_inst ps vs c hb hw ha]
  all_goals simp [Ty.containsVar] at hv'

theorem instTys_inst (ps : List (String × PTy)) : ∀ (ts : Tys) (c : WfCtx), c.binders = [] →
    ts.wfIn c = true → ts.allVars (fun n => (ps.lookup n).isSome) = true →
    PTy.instList θ (instTys ps ts) = instTys (instPs θ ps) ts
  | .nil, _, _, _, _ => rfl
  | .cons t ts, c, hb, hw, ha => by
    simp only [Tys.wfIn, Bool.and_eq_true] at hw; simp only [Tys.allVars, Bool.and_eq_true] at ha
    simp only [instTys, PTy.instList, instantiate_inst ps t c hb hw.1 ha.1,
      instTys_inst ps ts c hb hw.2 ha.2]

theorem instFields_inst (ps : List (String × PTy)) : ∀ (fs : Fields) (c : WfCtx), c.binders = [] →
    fs.wfIn c = true → fs.allVars (fun n => (ps.lookup n).isSome) = true →
    PTy.instFields θ (instFields ps fs) = instFields (instPs θ ps) fs
  | .nil, _, _, _, _ => rfl
  | .cons _ t fs, c, hb, hw, ha => by
    simp only [Fields.wfIn, Bool.and_eq_true] at hw; simp only [Fields.allVars, Bool.and_eq_true] at ha
    simp only [instFields, PTy.instFields, instantiate_inst ps t c hb hw.1 ha.1,
      instFields_inst ps fs c hb hw.2 ha.2]

theorem instColumns_inst (ps : List (String × PTy)) : ∀ (fs : Fields) (c : WfCtx), c.binders = [] →
    fs.wfColumnsIn c = true → fs.allVars (fun n => (ps.lookup n).isSome) = true →
    PTy.instFields θ (instFields ps fs) = instFields (instPs θ ps) fs
  | .nil, _, _, _, _ => rfl
  | .cons _ t fs, c, hb, hw, ha => by
    simp only [Fields.wfColumnsIn, Bool.and_eq_true] at hw; simp only [Fields.allVars, Bool.and_eq_true] at ha
    simp only [instFields, PTy.instFields, instantiate_inst ps t _ (by simpa [WfCtx.coll] using hb) hw.1 ha.1,
      instColumns_inst ps fs c hb hw.2 ha.2]

theorem instVariants_inst (ps : List (String × PTy)) : ∀ (vs : Variants) (c : WfCtx), c.binders = [] →
    vs.wfIn c = true → vs.allVars (fun n => (ps.lookup n).isSome) = true →
    PTy.instVariants θ (instVariants ps vs) = instVariants (instPs θ ps) vs
  | .nil, _, _, _, _ => rfl
  | .atom _ vs, c, hb, hw, ha => by
    simp only [Variants.wfIn] at hw; simp only [Variants.allVars] at ha
    simp only [instVariants, PTy.instVariants, instVariants_inst ps vs c hb hw ha]
  | .term _ t vs, c, hb, hw, ha => by
    simp only [Variants.wfIn, Bool.and_eq_true] at hw; simp only [Variants.allVars, Bool.and_eq_true] at ha
    simp only [instVariants, PTy.instVariants, instantiate_inst ps t c hb hw.1 ha.1,
      instVariants_inst ps vs c hb hw.2 ha.2]

end

theorem tparamVars_keys (tps : List (String × Option Bound)) : ∀ s ps s',
    (tps.mapM fun (p, _) => do pure (p, ← Lit.fresh .unknown) : Lit.InferM (List (String × PTy))).run s =
      .ok (ps, s') → ps.map Prod.fst = tps.map Prod.fst := by
  induction tps with
  | nil => intro s ps s' h; simp [StateT.run, pure, StateT.pure, Except.pure] at h; simp [← h.1]
  | cons tp tps ih =>
    intro s ps s' h
    simp only [List.mapM_cons, StateT.run, bind, StateT.bind, Except.bind, Lit.fresh, get, getThe,
      MonadStateOf.get, StateT.get, set, MonadStateOf.set, StateT.set, pure, StateT.pure,
      Except.pure] at h
    split at h
    · simp at h
    · rename_i r hr
      simp only [Except.ok.injEq, Prod.mk.injEq] at h
      obtain ⟨rfl, -⟩ := h
      simp [ih _ _ _ hr]

theorem lookup_isSome_of_keys {ps : List (String × PTy)} {tps : List (String × Option Bound)}
    (h : ps.map Prod.fst = tps.map Prod.fst) (n : String) :
    (ps.lookup n).isSome = tps.any (·.1 == n) := by
  induction ps generalizing tps with
  | nil => cases tps <;> simp_all
  | cons p ps ih =>
    cases tps with
    | nil => simp at h
    | cons tp tps =>
      simp only [List.map_cons, List.cons.injEq] at h
      obtain ⟨m, x⟩ := p
      simp only [List.lookup, List.any_cons]
      by_cases e : n = m
      · subst e; simp only [beq_self_eq_true, Option.isSome_some]
        simp only [Bool.true_eq, Bool.or_eq_true, beq_iff_eq]
        exact .inl h.1.symm
      · have : (n == m) = false := by simpa using e
        simp only [this, ih h.2]
        have : (tp.1 == n) = false := by rw [← h.1]; simpa using fun h => e h.symm
        simp [this]

/-- What a signature needs for its instantiation to commute: its types name
only its own type parameters and are well formed. -/
def SigFit (sig : Sig) : Prop :=
  (∀ mt ∈ sig.params, mt.2.allVars (fun n => sig.tparams.any (·.1 == n)) = true ∧ mt.2.WF) ∧
  (∀ r, sig.ret = .ty r → r.allVars (fun n => sig.tparams.any (·.1 == n)) = true ∧ r.WF)

theorem SigsOk.fit {sigs : List (String × Sig)} (h : SigsOk sigs) {f : String} {sig : Sig}
    (hl : sigs.lookup f = some sig) : SigFit sig :=
  ⟨fun mt hm => ⟨(h.1 f sig hl).1 mt hm, (h.2 f sig hl).1 mt hm⟩,
    fun r hr => ⟨(h.1 f sig hl).2 r hr, (h.2 f sig hl).2 r hr⟩⟩

theorem instantiate_fit {sig : Sig} {ps : List (String × PTy)}
    (hk : ps.map Prod.fst = sig.tparams.map Prod.fst) {t : Ty}
    (ha : t.allVars (fun n => sig.tparams.any (·.1 == n)) = true) (hw : t.WF) :
    (instantiate ps t).inst θ = instantiate (instPs θ ps) t := by
  refine instantiate_inst ps t {} rfl hw ?_
  have : (fun n => (ps.lookup n).isSome) = (fun n => sig.tparams.any (·.1 == n)) :=
    funext (lookup_isSome_of_keys hk)
  rw [this]; exact ha

theorem instFields_zip (header : List String) (cols : List PTy) :
    PTy.instFields θ (header.zip cols) = header.zip (cols.map (PTy.inst θ)) := by
  rw [PTy.instFields_eq]
  induction header generalizing cols with
  | nil => rfl
  | cons h hs ih => cases cols <;> simp_all

section
variable (g : Ground θ)
include g

theorem unify_lockP (a b : PTy) : Lock θ id (Lit.unify a b) (Lit.unify (a.inst θ) (b.inst θ)) :=
  Lit.unify_lock g a b

mutual

theorem inferF_lock (Γ : Ctx) (hs : SigsOk Γ.sigs) : ∀ (n : Nat) (e : Expr),
    Lock θ (PTy.inst θ) (inferF n Γ e) (inferF n (Γ.inst θ) e)
  | 0, _ => by simp only [inferF]; exact Lock.fuel
  | n + 1, e => by
    have ih := inferF_lock Γ hs n
    have iht := togetherF_lock Γ hs n
    cases e
    case hinted h _ =>
      simp only [inferF, Ctx.hint]
      have hc := Fun.convertIn_inst g Γ.aliases h
      change Lock θ (PTy.inst θ) _ (match convertIn (Γ.aliases.inst θ) h with
        | .ok t => pure (PTy.known t)
        | .error c => if c == "T066" then throw Lit.rigidCode else Lit.fresh .unknown)
      revert hc
      cases convertIn Γ.aliases h with
      | ok t => intro hc; rw [hc]; exact Lock.pure' rfl
      | error c =>
        intro hc
        by_cases e : c = "T066"
        · subst e; exact Lock.rigid
        · rcases hc with hc | hc
          · exact absurd hc e
          · rw [hc]
            have : (c == "T066") = false := by simpa using e
            simp only [this, Bool.false_eq_true, ↓reduceIte]
            exact fresh_lock' .unknown rfl
    case tuple es =>
      simp only [inferF]
      exact Lock.map' (Lit.Lock.mapM_id (fun a _ => ih a)) (fun rs => by simp [PTy.inst, PTy.instList_eq])
    case struct fs =>
      simp only [inferF]
      refine Lock.map' (Lit.Lock.mapM_id (f := fun p => (p.1, p.2.inst θ)) (fun ke _ => ?_))
        (fun rs => by simp [PTy.inst, PTy.instFields_eq])
      exact Lock.bind' (ih ke.2) (fun p => Lock.pure' rfl)
    case list es | set es => simp only [inferF]; exact Lock.map' (iht es) (fun _ => rfl)
    case tensor shape es => simp only [inferF]; exact Lock.map' (iht es) (fun _ => rfl)
    case map es =>
      simp only [inferF]
      exact Lock.bind' (iht _) (fun k => Lock.bind' (iht _) (fun v => Lock.pure' rfl))
    case table header rows =>
      simp only [inferF]
      exact Lock.bind' (Lit.Lock.mapM_id (fun i _ => iht _))
        (fun cols => Lock.pure' (by simp [PTy.inst, instFields_zip]))
    case some_ e | ok e => simp only [inferF]; exact Lock.map' (ih e) (fun _ => rfl)
    case none_ | er _ => simp only [inferF]; exact Lock.map' (fresh_lock' _ rfl) (fun _ => rfl)
    case term a e =>
      simp only [inferF]
      exact Lock.bind' (ih e) (fun p => Lit.fresh_lockP _)
    case var x =>
      simp only [inferF, Ctx.lookup_inst]
      cases Γ.lookup x with
      | none => exact fresh_lock' .unknown rfl
      | some tk => exact Lock.pure' rfl
    case binop op a b =>
      simp only [inferF]
      refine Lock.bind' (ih a) (fun ta => Lock.bind' (ih b) (fun tb => ?_))
      refine Lock.bind' (unify_lockP g ta tb) (fun _ => ?_)
      split <;> exact Lock.pure' rfl
    case unop op a => cases op <;> simp only [inferF] <;> first | exact Lock.pure' rfl | exact ih a
    case index b k mode =>
      cases mode
      · simp only [inferF]; exact fresh_lock' .unknown rfl
      · simp only [inferF]; exact Lock.bind' (ih b) (fun t => elementOf_lock g t)
    case place x steps =>
      simp only [inferF, Ctx.lookup_inst]
      cases Γ.lookup x with
      | none => exact fresh_lock' .unknown rfl
      | some tk =>
        refine Lit.Lock.foldlM (fun t st => ?_) (PTy.known tk.1)
        cases st with
        | field sel => exact inferField_lock g t sel
        | index _ _ => exact elementOf_lock g t
    case proj b sel =>
      simp only [inferF]
      exact Lock.bind' (ih b) (fun t => inferField_lock g t sel)
    case tryOpt a | tryRes a =>
      simp only [inferF]
      refine Lock.bind' (ih a) (fun t => Lock.bind' (shallow_lock g t) (fun p => ?_))
      cases p <;> simp only [PTy.inst] <;> first | exact Lock.pure' rfl | exact fresh_lock' .unknown rfl
    case adapt a =>
      simp only [inferF]
      refine Lock.bind' (ih a) (fun t => Lock.bind' Lock.get' (fun st => ?_))
      have hr := Lit.resolveF_lock (θ := θ) Lit.inferFuel t st
      simp only [Lit.resolve]
      revert hr
      cases (Lit.resolveF Lit.inferFuel t).run st with
      | ok r =>
        intro hr
        obtain ⟨from_, s'⟩ := r
        simp only at hr
        rw [hr]
        exact Lock.bind' Lock.set' (fun _ => Lit.fresh_lockP _)
      | error c =>
        intro hr
        by_cases hu : (c == Lit.unknownCode || c == "T016") = true
        · rcases hr with hr | hr
          · rcases hr with rfl | rfl <;> simp [Lit.rigidCode, Lit.unknownCode] at hu
          · rw [hr]; simp only [hu, ↓reduceIte]; exact Lock.pure' rfl
        · have hu' := Bool.eq_false_iff.mpr hu
          simp only [hu', Bool.false_eq_true, ↓reduceIte]
          rcases hr with hr | hr
          · rcases hr with rfl | rfl
            · exact Lock.rigid
            · exact Lock.fuel
          · rw [hr]; simp only [hu', Bool.false_eq_true, ↓reduceIte]; exact Lock.throw'
    case call f args =>
      simp only [inferF]
      change Lock θ (PTy.inst θ) _ (match Γ.sigs.lookup f with
        | some sig => (·.1) <$> inferCallF n (Γ.inst θ) sig args
        | none => Lit.fresh .unknown)
      cases hl : Γ.sigs.lookup f with
      | none => exact fresh_lock' .unknown rfl
      | some sig =>
        exact Lock.map' (inferCallF_lock Γ hs n sig args (hs.fit hl)) (fun _ => rfl)
    all_goals simp only [inferF]
    all_goals first
      | exact Lock.pure' rfl
      | exact fresh_lock' _ rfl

theorem inferCallF_lock (Γ : Ctx) (hs : SigsOk Γ.sigs) : ∀ (n : Nat) (sig : Sig)
    (args : List (Option Mode × Expr)), SigFit sig →
    Lock θ (fun p => (p.1.inst θ, instPs θ p.2)) (inferCallF n Γ sig args) (inferCallF n (Γ.inst θ) sig args)
  | 0, _, _, _ => by simp only [inferCallF]; exact Lock.fuel
  | n + 1, sig, args, hfit => by
    simp only [inferCallF]
    refine Lit.Lock.bindP (Lit.Lock.mapM_id (f := fun p => (p.1, p.2.inst θ))
      (fun tp _ => Lock.bind' (fresh_lock' .unknown rfl) (fun v => Lock.pure' rfl)))
      (tparamVars_keys sig.tparams) (fun ps hk => ?_)
    refine Lock.bind' (Lit.Lock.forIn (fun x hx u => ?_)) (fun _ => ?_)
    · obtain ⟨⟨m, a⟩, ⟨md, pt⟩⟩ := x
      have hpt : (md, pt) ∈ sig.params := (List.of_mem_zip hx).2
      obtain ⟨ha, hw⟩ := hfit.1 _ hpt
      refine Lock.bind' (inferF_lock Γ hs n a) (fun ta => ?_)
      refine Lock.bind' (f := id) ?_ (fun _ => Lock.pure' rfl)
      have e := instantiate_fit (θ := θ) hk ha hw
      have := unify_lockP g (instantiate ps pt) ta
      rw [e] at this
      exact this
    · refine Lock.pure' ?_
      cases hr : sig.ret with
      | void => rfl
      | ty r =>
        obtain ⟨ha, hw⟩ := hfit.2 r hr
        simp only [instantiate_fit hk ha hw]
        rfl

theorem togetherF_lock (Γ : Ctx) (hs : SigsOk Γ.sigs) : ∀ (n : Nat) (es : List Expr),
    Lock θ (PTy.inst θ) (togetherF n Γ es) (togetherF n (Γ.inst θ) es)
  | 0, _ => by simp only [togetherF]; exact Lock.fuel
  | n + 1, es => by
    simp only [togetherF, sharedVar]
    refine Lock.bind' (fresh_lock' _ (by split <;> rfl)) (fun t => ?_)
    refine Lock.bind' (Lit.Lock.forIn (fun e _ u => ?_)) (fun _ => Lock.pure' rfl)
    refine Lock.bind' (inferF_lock Γ hs n e) (fun ta => ?_)
    exact Lock.bind' (unify_lockP g t ta) (fun _ => Lock.pure' rfl)

end

end

theorem State.inst_empty : Lit.State.inst θ #[] = #[] := by simp [Lit.State.inst]

section
variable (g : Ground θ)
include g

/-- Inferring an expression and resolving it, under an instance, answers what
it does under the generic, substituted, where the generic does not escape. -/
theorem inferType_inst (Γ : Ctx) (hs : SigsOk Γ.sigs) (e : Expr)
    (h : ∀ c, inferType Γ e = .error c → ¬ Escape c) :
    inferType (Γ.inst θ) e = (inferType Γ e).map (Ty.inst θ) := by
  have L : Lock θ (Ty.inst θ) (infer Γ e >>= Lit.resolve) (infer (Γ.inst θ) e >>= Lit.resolve) :=
    Lock.bind' (inferF_lock g Γ hs Lit.inferFuel e) (fun p => Lit.resolveF_lock Lit.inferFuel p)
  have L0 := L #[]
  rw [State.inst_empty] at L0
  have E : ∀ Δ : Ctx, inferType Δ e = ((infer Δ e >>= Lit.resolve).run #[]).map (·.1) := fun _ => rfl
  rw [E] at h ⊢
  rw [E]
  revert h L0
  generalize (infer Γ e >>= Lit.resolve).run #[] = R
  intro h L0
  cases R with
  | ok r => simp only at L0; simp [L0, Except.map]
  | error c =>
    rcases L0 with L0 | L0
    · exact absurd L0 (h c rfl)
    · simp [L0, Except.map]

/-- The two operands of a pair, inferred together, under an instance, as
under the generic, substituted, where neither of the generic's answers
escapes. -/
theorem pairTypes_inst (Γ : Ctx) (hs : SigsOk Γ.sigs) (a b : Expr) (exp : Option Ty)
    (ha : ∀ c, (pairTypes Γ a b exp).1 = .error c → ¬ Escape c)
    (hb : ∀ c, (pairTypes Γ a b exp).2 = .error c → ¬ Escape c) :
    pairTypes (Γ.inst θ) a b (exp.map (Ty.inst θ)) =
      ((pairTypes Γ a b exp).1.map (Ty.inst θ), (pairTypes Γ a b exp).2.map (Ty.inst θ)) := by
  let joint (Δ : Ctx) (x : Option Ty) : Lit.InferM (PTy × PTy) := do
    let ta ← infer Δ a
    let tb ← infer Δ b
    if let some t := x then
      Lit.unify ta (.known t)
      Lit.unify tb (.known t)
    Lit.unify ta tb
    pure (ta, tb)
  have L : Lock θ (fun p => (p.1.inst θ, p.2.inst θ)) (joint Γ exp) (joint (Γ.inst θ) (exp.map (Ty.inst θ))) := by
    refine Lock.bind' (inferF_lock g Γ hs Lit.inferFuel a) (fun ta => ?_)
    refine Lock.bind' (inferF_lock g Γ hs Lit.inferFuel b) (fun tb => ?_)
    have tail : Lock θ (fun p => (p.1.inst θ, p.2.inst θ))
        (Lit.unify ta tb >>= fun _ => pure (ta, tb))
        (Lit.unify (ta.inst θ) (tb.inst θ) >>= fun _ => pure (ta.inst θ, tb.inst θ)) :=
      Lock.bind' (unify_lockP g ta tb) (fun _ => Lock.pure' rfl)
    cases exp with
    | none => exact tail
    | some t =>
      exact Lock.bind' (unify_lockP g ta (.known t))
        (fun _ => Lock.bind' (unify_lockP g tb (.known t)) (fun _ => tail))
  have L0 := L #[]
  rw [State.inst_empty] at L0
  simp only [pairTypes] at ha hb ⊢
  change (match (joint (Γ.inst θ) (exp.map (Ty.inst θ))).run #[] with
      | .ok ((ta, tb), s) => ((Lit.resolve ta).run' s, (Lit.resolve tb).run' s)
      | .error c => (.error c, .error c)) =
    ((match (joint Γ exp).run #[] with
      | .ok ((ta, tb), s) => ((Lit.resolve ta).run' s, (Lit.resolve tb).run' s)
      | .error c => (.error c, .error c)).1.map (Ty.inst θ),
     (match (joint Γ exp).run #[] with
      | .ok ((ta, tb), s) => ((Lit.resolve ta).run' s, (Lit.resolve tb).run' s)
      | .error c => (.error c, .error c)).2.map (Ty.inst θ))
  change (∀ c, (match (joint Γ exp).run #[] with
      | .ok ((ta, tb), s) => ((Lit.resolve ta).run' s, (Lit.resolve tb).run' s)
      | .error c => (.error c, .error c)).1 = .error c → ¬ Escape c) at ha
  change (∀ c, (match (joint Γ exp).run #[] with
      | .ok ((ta, tb), s) => ((Lit.resolve ta).run' s, (Lit.resolve tb).run' s)
      | .error c => (.error c, .error c)).2 = .error c → ¬ Escape c) at hb
  generalize (joint Γ exp).run #[] = R at ha hb L0 ⊢
  cases R with
  | error c =>
    rcases L0 with L0 | L0
    · exact absurd L0 (ha c rfl)
    · simp [L0, Except.map]
  | ok r =>
    obtain ⟨⟨ta, tb⟩, s1⟩ := r
    simp only at L0 ha hb ⊢
    rw [L0]
    have res : ∀ t, (∀ c, (Lit.resolve t).run' s1 = .error c → ¬ Escape c) →
        (Lit.resolve (t.inst θ)).run' (Lit.State.inst θ s1) = ((Lit.resolve t).run' s1).map (Ty.inst θ) := by
      intro t ht
      have R := Lit.resolveF_lock (θ := θ) Lit.inferFuel t s1
      have E : ∀ u s, (Lit.resolve u).run' s = ((Lit.resolveF Lit.inferFuel u).run s).map (·.1) :=
        fun _ _ => rfl
      rw [E] at ht
      rw [E, E]
      generalize (Lit.resolveF Lit.inferFuel t).run s1 = R0 at ht R ⊢
      cases R0 with
      | ok r => simp only at R; simp [R, Except.map]
      | error c =>
        rcases R with R | R
        · exact absurd R (ht c rfl)
        · simp [R, Except.map]
    simp only [res ta ha, res tb hb]

end

theorem run_bind_ok {α β} {x : Lit.InferM α} {f : α → Lit.InferM β} {s s' : State} {b : β}
    (h : (x >>= f).run s = .ok (b, s')) : ∃ a s1, x.run s = .ok (a, s1) ∧ (f a).run s1 = .ok (b, s') := by
  simp only [StateT.run, bind, StateT.bind, Except.bind] at h
  split at h
  · simp at h
  · rename_i r hr; exact ⟨r.1, r.2, hr, h⟩

theorem inferCallF_keys (n : Nat) (Γ : Ctx) (sig : Sig) (args : List (Option Mode × Expr)) :
    ∀ s r ps s', (inferCallF n Γ sig args).run s = .ok ((r, ps), s') → ps.map Prod.fst = sig.tparams.map Prod.fst := by
  intro s r ps s' h
  cases n with
  | zero =>
    simp [inferCallF, StateT.run, throw, throwThe, MonadExceptOf.throw, StateT.lift, Functor.map,
      Except.map] at h
  | succ n =>
    simp only [inferCallF] at h
    obtain ⟨ps0, s1, h1, h2⟩ := run_bind_ok h
    obtain ⟨_, s2, -, h3⟩ := run_bind_ok h2
    simp [StateT.run, pure, StateT.pure, Except.pure] at h3
    rw [← h3.1.2]
    exact tparamVars_keys sig.tparams s ps0 s1 h1

theorem resolve_run_inst (s1 : State) (t : PTy)
    (ht : ∀ c, (Lit.resolve t).run' s1 = .error c → ¬ Escape c) :
    (Lit.resolve (t.inst θ)).run' (Lit.State.inst θ s1) = ((Lit.resolve t).run' s1).map (Ty.inst θ) := by
  have R := Lit.resolveF_lock (θ := θ) Lit.inferFuel t s1
  have E : ∀ u s, (Lit.resolve u).run' s = ((Lit.resolveF Lit.inferFuel u).run s).map (·.1) :=
    fun _ _ => rfl
  rw [E] at ht
  rw [E, E]
  generalize (Lit.resolveF Lit.inferFuel t).run s1 = R0 at ht R ⊢
  cases R0 with
  | ok r => simp only at R; simp [R, Except.map]
  | error c =>
    rcases R with R | R
    · exact absurd R (ht c rfl)
    · simp [R, Except.map]

/-- The inference `callTypeArgs` runs for a call of `sig`. -/
def callRun (Γ : Ctx) (sig : Sig) (args : List (Option Mode × Expr)) (expected : Option Ty) :
    Lit.InferM (List (String × PTy)) := do
  let (r, ps) ← inferCall Γ sig args
  if let Option.some t := expected then Lit.unify r (.known t)
  pure ps

theorem callTypeArgs_eq (Γ : Ctx) (f : String) (args : List (Option Mode × Expr)) (exp : Option Ty) :
    callTypeArgs Γ f args exp = match Γ.sigs.lookup f with
      | none => []
      | some sig => match (callRun Γ sig args exp).run #[] with
        | .ok (ps, s) => ps.map fun (p, v) => (p, (Lit.resolve v).run' s)
        | .error c => sig.tparams.map fun (p, _) => (p, .error c) := rfl

theorem callRun_keys (Γ : Ctx) (sig : Sig) (args : List (Option Mode × Expr)) (exp : Option Ty) :
    ∀ s ps s', (callRun Γ sig args exp).run s = .ok (ps, s') → ps.map Prod.fst = sig.tparams.map Prod.fst := by
  intro s ps s' h
  simp only [callRun] at h
  obtain ⟨⟨r, ps0⟩, s1, h1, h2⟩ := run_bind_ok h
  have k := inferCallF_keys _ Γ sig args s r ps0 s1 h1
  cases exp with
  | none =>
    simp [StateT.run, pure, StateT.pure, Except.pure] at h2
    rw [← h2.1]; exact k
  | some t =>
    obtain ⟨_, s2, -, h3⟩ := run_bind_ok h2
    simp [StateT.run, pure, StateT.pure, Except.pure] at h3
    rw [← h3.1]; exact k

theorem map_resolve_inst (s1 : State) : ∀ (ps : List (String × PTy)),
    (∀ p c, (p, Except.error c) ∈ ps.map (fun (p, v) => (p, (Lit.resolve v).run' s1)) → ¬ Escape c) →
    (instPs θ ps).map (fun (p, v) => (p, (Lit.resolve v).run' (Lit.State.inst θ s1))) =
      (ps.map fun (p, v) => (p, (Lit.resolve v).run' s1)).map fun (p, r) => (p, r.map (Ty.inst θ))
  | [], _ => rfl
  | (p, v) :: ps, h => by
    simp only [instPs, List.map_cons] at h ⊢
    have h1 : ∀ c, (Lit.resolve v).run' s1 = .error c → ¬ Escape c :=
      fun c hc => h p c (by rw [← hc]; exact List.mem_cons_self)
    have h2 := map_resolve_inst s1 ps (fun q c hq => h q c (List.mem_cons_of_mem _ hq))
    simp only [instPs] at h2
    rw [resolve_run_inst s1 v h1, h2]

section
variable (g : Ground θ)
include g

theorem callTypeArgs_inst (Γ : Ctx) (hs : SigsOk Γ.sigs) (f : String) (args : List (Option Mode × Expr))
    (exp : Option Ty) (h : ∀ p c, (p, Except.error c) ∈ callTypeArgs Γ f args exp → ¬ Escape c) :
    callTypeArgs (Γ.inst θ) f args (exp.map (Ty.inst θ)) =
      (callTypeArgs Γ f args exp).map fun (p, r) => (p, r.map (Ty.inst θ)) := by
  rw [callTypeArgs_eq] at h ⊢
  rw [callTypeArgs_eq, Ctx.inst_sigs]
  cases hl : Γ.sigs.lookup f with
  | none => rfl
  | some sig =>
    simp only [hl] at h ⊢
    have L : Lock θ (instPs θ) (callRun Γ sig args exp) (callRun (Γ.inst θ) sig args (exp.map (Ty.inst θ))) := by
      simp only [callRun]
      refine Lock.bind' (inferCallF_lock g Γ hs Lit.inferFuel sig args (hs.fit hl)) (fun rp => ?_)
      obtain ⟨r, ps⟩ := rp
      cases exp with
      | none => exact Lock.pure' rfl
      | some t => exact Lock.bind' (unify_lockP g r (.known t)) (fun _ => Lock.pure' rfl)
    have L0 := L #[]
    rw [State.inst_empty] at L0
    have ki := callRun_keys (Γ.inst θ) sig args (exp.map (Ty.inst θ)) #[]
    generalize (callRun (Γ.inst θ) sig args (exp.map (Ty.inst θ))).run #[] = R' at L0 ki ⊢
    generalize (callRun Γ sig args exp).run #[] = R at h L0
    cases R with
    | ok r =>
      obtain ⟨ps, s1⟩ := r
      simp only at L0 h ⊢
      rw [L0]
      exact map_resolve_inst s1 ps h
    | error c =>
      simp only at h
      by_cases ht : sig.tparams = []
      · cases R' with
        | ok r' =>
          obtain ⟨ps', s'⟩ := r'
          have := ki ps' s' rfl
          rw [ht] at this
          simp only [List.map_nil, List.map_eq_nil_iff] at this
          simp [this, ht]
        | error c' => simp [ht]
      · obtain ⟨⟨p, b⟩, rest, hp⟩ := List.exists_cons_of_ne_nil ht
        have he : ¬ Escape c := h p c (by simp [hp])
        rcases L0 with L0 | L0
        · exact absurd L0 he
        · rw [L0]; simp [Except.map]

end

/-! ## The check-once theorem's assumption, discharged -/

theorem not_escape_unknown : ¬ Escape unknownCode := by
  simp [Lit.Escape, Lit.rigidCode, Lit.unknownCode]

theorem litBody_quiet {env : InferEnv} {n : Nat} {Γ : Ctx} {e : Expr} {t : Ty}
    (h : Quiet (litBody env n Γ e) t) : ∀ c, env.ty Γ e = .error c → c = unknownCode := by
  intro c hc
  simp only [litBody, hc] at h
  obtain ⟨a, h1, -⟩ := h.bind_inv
  by_cases hu : (c == unknownCode) = true
  · simpa using hu
  · simp [hu] at h1

theorem operandF_quiet {env : InferEnv} {n : Nat} {Γ : Ctx} {e : Expr} {r : Except Code Ty} {t : Ty}
    (h : Quiet (operandF env n Γ e r) t) : ∀ c, r = .error c → c = unknownCode := by
  intro c hc
  subst hc
  cases n with
  | zero => simp [operandF] at h
  | succ n =>
    simp only [operandF] at h
    by_cases hu : (c == unknownCode) = true
    · simpa using hu
    · simp [hu] at h

theorem decided_ok : ∀ {l : List (String × Except Code Ty)} {r : List (String × Ty)},
    decided l = .ok r → ∀ p c, (p, .error c) ∈ l → c = unknownCode
  | [], _, _, _, _, hm => by simp at hm
  | (q, .ok t) :: l, r, h, p, c, hm => by
    simp only [decided] at h
    cases hd : decided l with
    | error e => simp [hd, Functor.map, Except.map] at h
    | ok r' =>
      simp only [List.mem_cons, Prod.mk.injEq, reduceCtorEq, and_false, false_or] at hm
      exact decided_ok hd p c hm
  | (q, .error c') :: l, r, h, p, c, hm => by
    simp only [decided] at h
    by_cases hu : (c' == unknownCode) = true
    · simp only [hu, ↓reduceIte] at h
      simp only [List.mem_cons, Prod.mk.injEq, Except.error.injEq] at hm
      rcases hm with ⟨-, rfl⟩ | hm
      · simpa using hu
      · exact decided_ok h p c hm
    · simp [hu, throw, throwThe, MonadExceptOf.throw] at h

theorem callF_quiet {env : InferEnv} {n : Nat} {Γ : Ctx} {f : String} {args : List (Option Mode × Expr)}
    {exp : Option Ty} {t : Ty} (h : Quiet (callF env (n + 1) Γ f args exp) t) :
    ∀ p c, (p, .error c) ∈ env.call Γ f args exp → c = unknownCode := by
  simp only [callF] at h
  split at h
  · split at h
    · simp at h
    · obtain ⟨_, -, h⟩ := h.bind_inv
      obtain ⟨_, -, h⟩ := h.bind_inv
      obtain ⟨θ', h1, -⟩ := h.bind_inv
      exact decided_ok (Quiet.liftE_iff.mp h1)
  · simp at h

section
variable (g : Ground θ)
include g

/-- **Inference under an instance answers what it answered under the
generic, substituted**, wherever the generic checked quietly: the check-once
theorem's assumption (`InstEnv`), of the real inference. -/
theorem inferEnv_inst : InstEnv θ inferEnv inferEnv where
  ty := fun n Γ e t _ hs hl q => by
    rw [synthF_lit hl] at q
    have hq := litBody_quiet q
    rw [Ctx.inst_enter]
    rw [← Ctx.inst_enter]
    exact inferType_inst g (Γ.enter e) hs e (fun c hc => by
      rw [hq c hc]; exact not_escape_unknown)
  pair := fun n Γ a b exp t _ hs q => by
    simp only [pairF] at q
    obtain ⟨ta, h1, q⟩ := q.bind_inv
    obtain ⟨tb, h2, -⟩ := q.bind_inv
    exact pairTypes_inst g Γ hs a b exp
      (fun c hc => by rw [operandF_quiet h1 c hc]; exact not_escape_unknown)
      (fun c hc => by rw [operandF_quiet h2 c hc]; exact not_escape_unknown)
  call := fun n Γ f args exp t _ hs q => by
    have hq := callF_quiet q
    exact callTypeArgs_inst g Γ hs f args exp (fun p c hm => by
      rw [hq p c hm]; exact not_escape_unknown)

/-- **Checking a generic once is enough**, of a script, with nothing assumed
of inference: `checkFun_instance_script` with `inferEnv_inst`. -/
theorem checkFun_instance_real (ss : List Stmt) (aliases : Aliases)
    (consts : List (String × Ty × Kind)) (name : String)
    (tps : List (String × Option Bound)) (params : List (Mode × String × Hint)) (ret : Option Hint)
    (body : List Stmt) (sig : Sig) (hsig : (resolveNames ss).1.lookup name = some sig)
    (hb : ∀ p b, (p, some b) ∈ tps → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true)
    (tf : blockTypeFree body = true)
    (quiet : SQuiet (checkFun (resolveNames ss).1 aliases consts (.mk name tps params ret body)) ()) :
    SQuiet (checkBlockF inferEnv fuel ((funCtx (resolveNames ss).1 aliases consts tps params sig).inst θ) body
      >>= fun _ => reportIf (match sig.ret.inst θ with
        | .ty _ => completes body
        | .void => false) "F063") () :=
  checkFun_instance_script g (inferEnv_inst g) ss aliases consts name tps params ret body sig hsig hb tf quiet

end

end Datalove.Fun
