import Datalove.FunExec
import Datalove.Proofs.EvalSound
import Datalove.Proofs.InstanceStmt
import Datalove.Proofs.Completes

/-!
# A block that checks keeps its variables at their types

`Fun.execBlock_sound`: where a block checks quietly, in a context with no
type parameter's bound, with its variables lined up with the context's,
each that is set holding a value of its type (`Agree`), running it leaves
them so, or breaks or continues inside a loop, or returns early with a value
of the function's return type, or with none from a function returning
nothing; or reaches what is not run yet, or reads a variable before it is
set, which the ownership analysis refuses, or runs a loop as often as it
was given, or calls as deep. Never an operand of the wrong kind, and never
out of fuel. It holds for any inference, and any calls that are sound
(`CallOk`), as `Fun.evalS_sound` does. It breaks only where the block has a
`break` of its own, and ends normally only where control can reach its end
(`completes`), which is what lets `Proofs/CallSound.lean` show a function
that returns a value returns one.
-/

namespace Datalove.Fun

open Lit (Val HasType roll_of)

/-- Values for names a pattern or `for` binds, name for name, each of its
type. -/
inductive Fits : List (String × Ty) → List (String × Option Val) → Prop where
  | nil : Fits [] []
  | cons {x : String} {t : Ty} {o : Option Val} {Δ : List (String × Ty)} {vals : List (String × Option Val)} :
      (∀ v, o = some v → HasType v t) → Fits Δ vals → Fits ((x, t) :: Δ) ((x, o) :: vals)

/-- The variables of a scope with some more bound inside it. -/
def Ext (vars : List (String × Ty × Kind)) (ρ : VEnv) : Prop :=
  ∃ δ ρ1, ρ = δ ++ ρ1 ∧ Agree vars ρ1

/-- A body's return the function's type takes: a value of its return type,
or none from a function returning nothing. -/
def RetOk (R : Ret) : Option Val → Prop
  | some r => EarlyOk R r
  | none => R = .void

/-- Why running a checked block may not end normally: a `break` only where
`B`, which says the block has one of its own. -/
def StopOkS (Γ : Ctx) (B : Prop) : Stop → Prop
  | .early _ | .retVoid => False
  | .returned o ρ => RetOk Γ.ret o ∧ Ext Γ.vars ρ
  | .brk ρ => Γ.inLoop = true ∧ Ext Γ.vars ρ ∧ B
  | .cont ρ => Γ.inLoop = true ∧ Ext Γ.vars ρ
  | .unsupported | .uninit | .diverge => True
  | .fuel | .wrong => False

/-- What running gives meets `P`, or stops as a checked block may. -/
def SoundS {α} (Γ : Ctx) (B : Prop) (P : α → Prop) (m : EvalM α) : Prop :=
  match m with
  | .ok a => P a
  | .error s => StopOkS Γ B s

namespace SoundS

variable {α β : Type} {Γ : Ctx} {B : Prop} {P : α → Prop} {Q : β → Prop}

theorem pure' {a : α} (h : P a) : SoundS Γ B P (Pure.pure a) := h

theorem bind' {x : EvalM α} {f : α → EvalM β} (hx : SoundS Γ B P x) (hf : ∀ a, P a → SoundS Γ B Q (f a)) :
    SoundS Γ B Q (x >>= f) := by
  cases x with
  | error s => exact hx
  | ok a => exact hf a hx

theorem weaken {x : EvalM α} {P' : α → Prop} (h : SoundS Γ B P x) (hp : ∀ a, P a → P' a) : SoundS Γ B P' x := by
  cases x with
  | error s => exact h
  | ok a => exact hp a h

theorem weakenB {x : EvalM α} {B' : Prop} (h : SoundS Γ B P x) (hb : B → B') : SoundS Γ B' P x := by
  cases x with
  | ok a => exact h
  | error s => cases s <;> first | exact h | exact ⟨h.1, h.2.1, hb h.2.2⟩

theorem map' {x : EvalM α} {f : α → β} (hx : SoundS Γ B P x) (hf : ∀ a, P a → Q (f a)) :
    SoundS Γ B Q (f <$> x) := by
  cases x with
  | error s => exact hx
  | ok a => exact hf a hx

end SoundS

variable {B : Prop}

theorem Agree.append {Δ vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree (Δ ++ vars) ρ) :
    ∃ δ ρ1, ρ = δ ++ ρ1 ∧ δ.length = Δ.length ∧ Agree vars ρ1 := by
  induction Δ generalizing ρ with
  | nil => exact ⟨[], ρ, rfl, rfl, h⟩
  | cons b Δ ih =>
    cases h with
    | cons ho h =>
      obtain ⟨δ, ρ1, rfl, hl, h1⟩ := ih h
      exact ⟨_ :: δ, ρ1, rfl, by simp [hl], h1⟩

theorem Ext.of_agree {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) : Ext vars ρ :=
  ⟨[], ρ, rfl, h⟩

theorem Ext.of_append {Δ vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Ext (Δ ++ vars) ρ) : Ext vars ρ := by
  obtain ⟨δ, ρ1, rfl, h1⟩ := h
  obtain ⟨δ', ρ2, rfl, -, h2⟩ := h1.append
  exact ⟨δ ++ δ', ρ2, by simp, h2⟩

/-- Leaving a scope: what was bound inside dropped, what was set kept. -/
theorem restore_agree {vars : List (String × Ty × Kind)} {ρ ρ' : VEnv} (h0 : Agree vars ρ)
    (h : Ext vars ρ') : Agree vars (restore ρ ρ') := by
  obtain ⟨δ, ρ1, rfl, h1⟩ := h
  simp only [restore, List.length_append, h1.length, h0.length, Nat.add_sub_cancel, List.drop_left]
  exact h1

/-- Binding names in order, as `Ctx.bindAllAs` does. -/
theorem bindVals_agree {vars : List (String × Ty × Kind)} {ρ : VEnv} (h : Agree vars ρ) (k : Kind) :
    ∀ (Δ : List (String × Ty)) (vals : List (String × Option Val)), Fits Δ vals →
      Agree (Δ.foldl (fun vs (x, t) => (x, t, k) :: vs) vars) (bindVals ρ vals) := by
  intro Δ vals hv
  induction hv generalizing vars ρ with
  | nil => exact h
  | cons hd _ ih =>
    simp only [List.foldl_cons, bindVals]
    exact ih (.cons hd h)

theorem bindAllAs_vars (Γ : Ctx) (k : Kind) : ∀ (Δ : List (String × Ty)),
    (Γ.bindAllAs Δ k).vars = Δ.foldl (fun vs (x, t) => (x, t, k) :: vs) Γ.vars
  | [] => rfl
  | (x, t) :: Δ => by
    simp only [Ctx.bindAllAs, List.foldl_cons] at *
    have := bindAllAs_vars (Γ.bind x t k) k Δ
    simp only [Ctx.bindAllAs] at this
    exact this

theorem foldl_bind_prefix (k : Kind) : ∀ (Δ : List (String × Ty)) (vars : List (String × Ty × Kind)),
    ∃ D, Δ.foldl (fun vs (x, t) => (x, t, k) :: vs) vars = D ++ vars
  | [], vars => ⟨[], rfl⟩
  | (x, t) :: Δ, vars => by
    obtain ⟨D, hD⟩ := foldl_bind_prefix k Δ ((x, t, k) :: vars)
    exact ⟨D ++ [(x, t, k)], by simp [hD]⟩

theorem fits_zip : ∀ (xs : List String) (vs : List Val) (ts : List Ty), vs.length = ts.length →
    (∀ p ∈ vs.zip ts, HasType p.1 p.2) →
    Fits (xs.zip ts) ((xs.zip vs).map fun (x, w) => (x, some w))
  | [], _, _, _, _ => by simp; exact .nil
  | _ :: _, [], [], _, _ => by simp; exact .nil
  | _ :: _, [], _ :: _, h, _ => by simp at h
  | _ :: _, _ :: _, [], h, _ => by simp at h
  | x :: xs, v :: vs, t :: ts, hl, hz => by
    simp only [List.zip_cons_cons, List.map_cons]
    exact .cons (fun w hw => by cases hw; exact hz (v, t) (by simp))
      (fits_zip xs vs ts (by simpa using hl) (fun p hp => hz p (by simp [hp])))

/-- A struct value's field by name, where its type has one, and none where
not. -/
theorem struct_lookup_none : ∀ {fs : List (String × Val)} {ts : List (String × Ty)} {n : String},
    fs.map (·.1) = ts.map (·.1) → ts.lookup n = none → fs.lookup n = none
  | [], _, _, _, _ => rfl
  | _ :: _, [], _, hn, _ => by simp at hn
  | (m, w) :: fs, (k, t) :: ts, n, hn, hl => by
    simp only [List.map_cons, List.cons.injEq] at hn
    obtain ⟨rfl, hn⟩ := hn
    simp only [List.lookup] at hl ⊢
    by_cases e : n = m
    · subst e; simp at hl
    · have : (n == m) = false := by simpa using e
      simp only [this] at hl ⊢
      exact struct_lookup_none hn hl

theorem fits_struct {fvs : List (String × Val)} {tl : List (String × Ty)}
    (hn : fvs.map (·.1) = tl.map (·.1)) (hz : ∀ p ∈ fvs.zip tl, HasType p.1.2 p.2.2) :
    ∀ (fs : List (String × String)),
      Fits (fs.filterMap fun (f, x) => (tl.lookup f).map (x, ·))
        ((fs.filterMap fun (f, x) => (fvs.lookup f).map (x, ·)).map fun (x, w) => (x, some w))
  | [] => by simp; exact .nil
  | (f, x) :: fs => by
    simp only [List.filterMap_cons]
    cases ht : tl.lookup f with
    | none =>
      rw [struct_lookup_none hn ht]
      exact fits_struct hn hz fs
    | some u =>
      obtain ⟨w, hw, hwt⟩ := struct_lookup hn hz ht
      simp only [hw, Option.map_some, List.map_cons]
      exact .cons (fun v hv => by cases hv; exact hwt) (fits_struct hn hz fs)

/-- What a pattern takes out of a value of its type is of the types it binds. -/
theorem patValues_fits {p : Pat} {t : Ty} {Δ : List (String × Ty)} {v : Val}
    (hp : patBindings p t = .ok Δ) (hv : HasType v t) :
    ∃ vals, patValues p v = .ok vals ∧ Fits Δ (vals.map fun (x, w) => (x, some w)) := by
  have he := hv.expose
  cases p with
  | name x =>
    simp only [patBindings, pure, Except.pure, Except.ok.injEq] at hp; subst hp
    exact ⟨[(x, v)], rfl, .cons (fun w hw => by cases hw; exact hv) .nil⟩
  | tuple xs =>
    unfold patBindings at hp
    generalize t.expose = T at he hp
    cases T
    case tuple ts =>
      simp only at hp
      split at hp
      · rename_i hl
        simp only [pure, Except.pure, Except.ok.injEq] at hp; subst hp
        obtain ⟨vs, rfl, hvl, hz⟩ := he.tuple_inv
        exact ⟨_, rfl, fits_zip xs vs _ hvl hz⟩
      · simp [throw, throwThe, MonadExceptOf.throw] at hp
    all_goals simp [throw, throwThe, MonadExceptOf.throw] at hp
  | struct fs =>
    unfold patBindings at hp
    generalize t.expose = T at he hp
    cases T
    case struct ts =>
      simp only at hp
      split at hp
      · simp [throw, throwThe, MonadExceptOf.throw] at hp
      split at hp
      · simp [throw, throwThe, MonadExceptOf.throw] at hp
      simp only [pure, Except.pure, Except.ok.injEq] at hp; subst hp
      obtain ⟨fvs, rfl, hn, hz⟩ := he.struct_inv
      exact ⟨_, rfl, fits_struct hn hz fs⟩
    all_goals simp [throw, throwThe, MonadExceptOf.throw] at hp
  | atom a =>
    unfold patBindings at hp
    generalize t.expose = T at he hp
    cases T
    case atom b =>
      simp only at hp
      split at hp
      · simp only [pure, Except.pure, Except.ok.injEq] at hp; subst hp; exact ⟨[], rfl, .nil⟩
      · simp [throw, throwThe, MonadExceptOf.throw] at hp
    all_goals simp [throw, throwThe, MonadExceptOf.throw] at hp
  | term a x =>
    unfold patBindings at hp
    generalize t.expose = T at he hp
    cases T
    case term b q =>
      simp only at hp
      split at hp
      · simp only [pure, Except.pure, Except.ok.injEq] at hp; subst hp
        obtain ⟨w, rfl, hw⟩ := he.term_inv
        exact ⟨[(x, w)], rfl, .cons (fun u hu => by cases hu; exact hw) .nil⟩
      · simp [throw, throwThe, MonadExceptOf.throw] at hp
    all_goals simp [throw, throwThe, MonadExceptOf.throw] at hp

theorem slices_typed {e : Ty} {shape : List Nat} {vs : List Val}
    (hl : vs.length = shape.foldl (· * ·) 1) (hvs : ∀ w ∈ vs, HasType w e) :
    ∀ w ∈ walkItems.slices shape vs,
      HasType w (if shape.length ≤ 1 then e else .tensor e (shape.length - 1)) := by
  rcases shape with _ | ⟨s0, _ | ⟨s1, rest⟩⟩
  · simpa [walkItems.slices] using hvs
  · simpa [walkItems.slices] using hvs
  · intro w hw
    simp only [walkItems.slices, List.mem_map, List.mem_range] at hw
    obtain ⟨i, hi, rfl⟩ := hw
    simp only [List.length_cons, show ¬ (rest.length + 1 + 1 ≤ 1) by omega, ↓reduceIte]
    let p := shapeSize (s1 :: rest)
    have hlen : vs.length = s0 * p := by rw [hl, List.foldl_cons, foldl_mul, Nat.one_mul]; rfl
    have h1 : (i + 1) * p ≤ s0 * p := Nat.mul_le_mul_right _ hi
    rw [Nat.succ_mul] at h1
    refine .tensor (by simp) ?_ ?_
    · simp only [List.length_take, List.length_drop, hlen]
      show min p (s0 * p - i * p) = shapeSize (s1 :: rest)
      omega
    · intro u hu
      exact hvs u (List.mem_of_mem_drop (List.mem_of_mem_take hu))

theorem indexed_fits {i x : String} {t : Ty} {vs : List Val} (hvs : ∀ w ∈ vs, HasType w t) :
    match indexed vs with
    | .ok items => ∀ item ∈ items, Fits [(i, .index), (x, t)] (([i, x].zip item).map fun (x, w) => (x, some w))
    | .error s => s = .unsupported := by
  unfold indexed
  by_cases hlen : vs.length ≤ 2 ^ Lit.indexBits
  · simp only [hlen, ↓reduceIte, pure, Except.pure]
    intro item hm
    simp only [List.mem_map] at hm
    obtain ⟨⟨w, j⟩, hj, rfl⟩ := hm
    obtain ⟨-, hjl, -⟩ := List.mem_zipIdx hj
    have hget := List.mem_zipIdx_iff_getElem?.mp hj
    refine .cons (fun v hv => ?_) (.cons (fun v hv => ?_) .nil)
    · cases hv
      exact .int (by simp only [Lit.inRange, Lit.indexBits] at hlen ⊢; omega)
    · cases hv
      exact hvs w (List.mem_of_getElem? hget)
  · simp only [hlen, ↓reduceIte]; rfl

/-- What a `for` walks is of the types it binds. -/
theorem _root_.Datalove.Lit.HasType.set_inv {v : Val} {e : Ty} (h : HasType v (.set e)) :
    ∃ vs, v = .set vs ∧ ∀ w ∈ vs, HasType w e := by
  generalize ht : Ty.set e = t at h
  induction h with
  | set hw => cases ht; exact ⟨_, rfl, hw⟩
  | hexInt _ ih => obtain ⟨_, h, -⟩ := ih ht; cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [Lit.inRange] at hr
  | hex hr => subst ht; simp [Lit.hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem walkItems_fits {c : Ty} {names : List String} {Δ : List (String × Ty)} {v : Val}
    (hw : walk c names = .ok Δ) (hv : HasType v c) :
    match walkItems c names v with
    | .ok items => ∀ item ∈ items, Fits Δ ((names.zip item).map fun (x, w) => (x, some w))
    | .error s => s = .unsupported := by
  have one : ∀ (x : String) (t : Ty) (ws : List Val), (∀ w ∈ ws, HasType w t) →
      ∀ item ∈ ws.map ([·]), Fits [(x, t)] (([x].zip item).map fun (x, w) => (x, some w)) := by
    intro x t ws hws item hm
    simp only [List.mem_map] at hm
    obtain ⟨w, hw, rfl⟩ := hm
    exact .cons (fun u hu => by cases hu; exact hws w hw) .nil
  unfold walk at hw
  cases c
  case list t =>
    obtain ⟨vs, rfl, hvs⟩ := hv.list_inv
    rcases names with _ | ⟨x, _ | ⟨i, _ | ⟨_, _⟩⟩⟩
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
    · simp only [pure, Except.pure, Except.ok.injEq] at hw; subst hw
      exact one x t vs hvs
    · simp only [pure, Except.pure, Except.ok.injEq] at hw; subst hw
      exact indexed_fits hvs
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
  case tensor t r =>
    obtain ⟨shape, vs, rfl, hs, hl, hvs⟩ := hv.tensor_inv
    have hsl := slices_typed hl hvs
    rw [hs] at hsl
    rcases names with _ | ⟨x, _ | ⟨i, _ | ⟨_, _⟩⟩⟩
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
    · simp only [pure, Except.pure, Except.ok.injEq] at hw; subst hw
      exact one x _ _ hsl
    · simp only [pure, Except.pure, Except.ok.injEq] at hw; subst hw
      exact indexed_fits hsl
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
  case set t =>
    obtain ⟨vs, rfl, hvs⟩ := hv.set_inv
    rcases names with _ | ⟨x, _ | ⟨_, _⟩⟩
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
    · simp only [pure, Except.pure, Except.ok.injEq] at hw; subst hw
      exact one x t vs hvs
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
  case map kt vt =>
    obtain ⟨es, rfl, hk, hvs⟩ := hv.map_inv
    rcases names with _ | ⟨a, _ | ⟨b, _ | ⟨_, _⟩⟩⟩
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
    · simp only [pure, Except.pure, Except.ok.injEq] at hw; subst hw
      intro item hm
      simp only [List.mem_map] at hm
      obtain ⟨e, he, rfl⟩ := hm
      exact .cons (fun u hu => by cases hu; exact hk e he) (.cons (fun u hu => by cases hu; exact hvs e he) .nil)
    · simp [throw, throwThe, MonadExceptOf.throw] at hw
  all_goals simp [throw, throwThe, MonadExceptOf.throw] at hw

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
    · simp only [putIndex]; exact Sound.unsupported

theorem finishBinop_quiet {Γ : Ctx} (hb : Γ.bounds = []) {op : BinOp} {t : Ty}
    (ha : arithAdmits op t (Γ.boundOf t) = true) (hf : fits Γ (needs op) = true) :
    Quiet (finishBinop Γ op t) t := by
  rw [boundOf_nil hb] at ha
  cases op <;> simp_all [finishBinop, boundOf_nil hb, arithAdmits] <;>
    (split
     · rename_i hc; rcases ha with h | h <;> simp_all
     · exact Quiet.pure _)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallOk sigs call)
include hcall
set_option linter.unusedSectionVars false

theorem execSetValue_sound {n : Nat} {Γ : Ctx} {t : Ty} {old : Option Val} {op : Option BinOp}
    {e : Expr} (h : SQuiet (setValue env n Γ t op e) ()) (hold : ∀ v, old = some v → HasType v t)
    (hb : Γ.Runs sigs) : SoundT Γ.vars Γ.ret (HasType · t) (execSetValue env call n Γ t old op e) := by
  unfold setValue at h
  cases op with
  | none =>
    obtain ⟨_, hc, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    exact evalC_sound env sigs call hcall n Γ e t hc hb
  | some op =>
    dsimp only at h
    obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    split at h
    · simp at h
    rename_i ha
    rw [SQuiet.reportIf_iff] at h
    simp only [execSetValue]
    refine SoundT.bind' (evalC_sound env sigs call hcall n _ e t hc hb) (fun r hr => ?_)
    cases old with
    | none => exact SoundT.uninit
    | some o =>
      exact SoundT.here <| binOp_sound hb.1 (finishBinop_quiet hb.1 (by simpa using ha) (by simpa using h)) (hold o rfl) hr

theorem execSetIndex_sound {n : Nat} {Γ : Ctx} {t : Ty} {k : Expr} {mode : Option IdxMode}
    {op : Option BinOp} {e : Expr} {old : Option Val} (h : SQuiet (setIndex env n Γ t k mode op e) ())
    (hold : ∀ v, old = some v → HasType v t) (hb : Γ.Runs sigs)
    (hm : ∀ m, mode = some m → Γ.fitsMode m = true) :
    SoundT Γ.vars Γ.ret (HasType · t) (execSetIndex env call n Γ t k mode op e old) := by
  unfold setIndex at h
  cases mode with
  | none =>
    cases op with
    | some _ => simp at h
    | none =>
      dsimp only at h
      split at h
      · rename_i kt vt hx
        obtain ⟨_, hk, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
        obtain ⟨_, hv, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        obtain ⟨_, -, hk⟩ := SQuiet.chk_inv hk
        obtain ⟨_, -, hv⟩ := SQuiet.chk_inv hv
        simp only [execSetIndex, hx]
        refine SoundT.bind' (evalC_sound env sigs call hcall n Γ k kt hk hb) (fun kv hkv => ?_)
        refine SoundT.bind' (evalC_sound env sigs call hcall n Γ e vt hv hb) (fun nv hnv => ?_)
        cases old with
        | none => exact SoundT.uninit
        | some o =>
          have ho := (hold o rfl).expose
          rw [hx] at ho
          obtain ⟨es, rfl, hks, hvs⟩ := ho.map_inv
          exact SoundT.map' (SoundT.here (insertKey_typed hkv hnv hks hvs)) (fun _ ⟨h1, h2⟩ => roll_of hx (.map h1 h2))
      · simp at h
  | some m =>
    dsimp only at h
    cases hi : indexTypes t with
    | error c => simp only [hi] at h; simp at h
    | ok p =>
      obtain ⟨et, kt⟩ := p
      simp only [hi] at h
      split at h
      · simp at h
      rename_i hview
      obtain ⟨_, hk, h⟩ := SQuiet.bind_inv (fun _ => setValue_appends _ _ _ _ _ _) h
      obtain ⟨_, -, hk⟩ := SQuiet.chk_inv hk
      simp only [execSetIndex, hi, hview, Bool.false_eq_true, ↓reduceIte]
      refine SoundT.bind' (evalC_sound env sigs call hcall n Γ k kt hk hb) (fun kv hkv => ?_)
      cases old with
      | none => exact SoundT.uninit
      | some o =>
        refine SoundT.bind' (SoundT.here <| indexVal_sound (hold o rfl) hi hkv (missing_ok (hm m rfl))) (fun part hp => ?_)
        refine SoundT.bind' (execSetValue_sound env sigs call hcall h (fun v hv => by cases hv; exact hp) hb) (fun w hw => ?_)
        exact SoundT.here <| putIndex_sound (hold o rfl) hi hkv hw

theorem execSetSteps_sound {n : Nat} {Γ : Ctx} {op : Option BinOp} {e : Expr}
    (hb : Γ.Runs sigs) :
    ∀ (t : Ty) (old : Option Val) (steps : List Step), SQuiet (setSteps env n Γ t op e steps) () →
      (∀ v, old = some v → HasType v t) → (∀ k m, Step.index k (some m) ∈ steps → Γ.fitsMode m = true) →
      SoundT Γ.vars Γ.ret (HasType · t) (execSetSteps env call n Γ op e t old steps)
  | t, old, [], h, hold, _ => by
    simp only [setSteps] at h; simp only [execSetSteps]
    exact execSetValue_sound env sigs call hcall h hold hb
  | t, old, [.index k mode], h, hold, hm => by
    simp only [setSteps] at h; simp only [execSetSteps]
    exact execSetIndex_sound env sigs call hcall h hold hb (fun m e => hm k m (by simp [e]))
  | t, old, .field s :: rest, h, hold, hm => by
    simp only [setSteps] at h
    split at h
    · rename_i ft hf
      simp only [execSetSteps, hf]
      cases old with
      | none => exact SoundT.uninit
      | some v =>
        refine SoundT.bind' (SoundT.here <| fieldVal_sound (hold v rfl) hf) (fun part hp => ?_)
        refine SoundT.bind' (execSetSteps_sound hb ft (some part) rest h (fun u hu => by cases hu; exact hp)
          (fun k m hk => hm k m (by simp [hk]))) (fun w hw => ?_)
        exact SoundT.here <| putField_sound (hold v rfl) hf hw
    · simp at h
  | t, old, .index _ none :: _ :: _, h, _, _ => by simp [setSteps] at h
  | t, old, .index k (some m) :: st :: rest, h, hold, hm => by
    simp only [setSteps] at h
    split at h
    · simp at h
    rename_i et kt hi
    obtain ⟨_, hk, h⟩ := SQuiet.bind_inv (fun _ => setSteps_appends _ _ _ _ _ _ _) h
    obtain ⟨_, -, hk⟩ := SQuiet.chk_inv hk
    simp only [execSetSteps, hi]
    refine SoundT.bind' (evalC_sound env sigs call hcall n Γ k kt hk hb) (fun kv hkv => ?_)
    cases old with
    | none => exact SoundT.uninit
    | some v =>
      refine SoundT.bind' (SoundT.here <| indexVal_sound (hold v rfl) hi hkv (missing_ok (hm k m (by simp)))) (fun part hp => ?_)
      refine SoundT.bind' (execSetSteps_sound hb et (some part) (st :: rest) h
        (fun u hu => by cases hu; exact hp) (fun k' m' hk' => hm k' m' (by simp_all))) (fun w hw => ?_)
      exact SoundT.here <| putIndex_sound (hold v rfl) hi hkv hw

end

/-! ## Statements' contexts -/

/-- What a statement's context keeps of the one before it: the return type,
whether it is in a loop, the bounds, the functions, and the variables, with
more bound. -/
def Grows (Γ Γ' : Ctx) : Prop :=
  Γ'.ret = Γ.ret ∧ Γ'.inLoop = Γ.inLoop ∧ Γ'.bounds = Γ.bounds ∧ Γ'.sigs = Γ.sigs ∧
    ∃ D, Γ'.vars = D ++ Γ.vars

theorem Grows.refl (Γ : Ctx) : Grows Γ Γ := ⟨rfl, rfl, rfl, rfl, [], rfl⟩

theorem Grows.trans {Γ Γ1 Γ2 : Ctx} (h1 : Grows Γ Γ1) (h2 : Grows Γ1 Γ2) : Grows Γ Γ2 := by
  obtain ⟨r1, l1, b1, s1, D1, v1⟩ := h1
  obtain ⟨r2, l2, b2, s2, D2, v2⟩ := h2
  exact ⟨r2.trans r1, l2.trans l1, b2.trans b1, s2.trans s1, D2 ++ D1, by rw [v2, v1, List.append_assoc]⟩

theorem Grows.bind (Γ : Ctx) (x : String) (t : Ty) (k : Kind) : Grows Γ (Γ.bind x t k) :=
  ⟨rfl, rfl, rfl, rfl, [(x, t, k)], rfl⟩

theorem Grows.runs {sigs : List (String × Sig)} {Γ Γ' : Ctx} (g : Grows Γ Γ') (h : Γ.Runs sigs) :
    Γ'.Runs sigs :=
  ⟨g.2.2.1.trans h.1, g.2.2.2.1.trans h.2⟩

theorem Grows.bindAllAs (Γ : Ctx) (k : Kind) : ∀ (Δ : List (String × Ty)), Grows Γ (Γ.bindAllAs Δ k)
  | [] => Grows.refl Γ
  | (x, t) :: Δ => (Grows.bind Γ x t k).trans (Grows.bindAllAs (Γ.bind x t k) k Δ)

theorem Grows.bindOpt (Γ : Ctx) (x : Option String) (t : Ty) : Grows Γ (Fun.bindOpt Γ x t) := by
  cases x
  · exact Grows.refl Γ
  · exact Grows.bind _ _ _ _

theorem bindAll_eq (Γ : Ctx) (Δ : List (String × Ty)) : Γ.bindAll Δ = Γ.bindAllAs Δ .let_ := rfl

theorem StopOkS.lift {Γ Γ' : Ctx} (g : Grows Γ Γ') {s : Stop} (h : StopOkS Γ' B s) : StopOkS Γ B s := by
  obtain ⟨hr, hl, -, -, D, hv⟩ := g
  cases s with
  | brk ρ => exact ⟨by rw [← hl]; exact h.1, Ext.of_append (hv ▸ h.2.1), h.2.2⟩
  | returned o ρ => exact ⟨hr ▸ h.1, Ext.of_append (hv ▸ h.2)⟩
  | cont ρ => exact ⟨by rw [← hl]; exact h.1, Ext.of_append (hv ▸ h.2)⟩
  | _ => exact h

theorem SoundS.lift {α} {Γ Γ' : Ctx} {P : α → Prop} {m : EvalM α} (g : Grows Γ Γ') (h : SoundS Γ' B P m) :
    SoundS Γ B P m := by
  cases m with
  | ok a => exact h
  | error s => exact StopOkS.lift g h

/-- Binding values that fit what a context binds. -/
theorem bindAllAs_agree {Γ : Ctx} {ρ : VEnv} (h : Agree Γ.vars ρ) (k : Kind) {Δ : List (String × Ty)}
    {vals : List (String × Option Val)} (hv : Fits Δ vals) : Agree (Γ.bindAllAs Δ k).vars (bindVals ρ vals) := by
  rw [bindAllAs_vars]; exact bindVals_agree h k Δ vals hv

theorem fits_none : ∀ (Δ : List (String × Ty)), Fits Δ (Δ.map fun (x, _) => (x, none))
  | [] => .nil
  | _ :: Δ => .cons (fun _ h => by cases h) (fits_none Δ)

theorem bindOpt_agree {Γ : Ctx} {ρ : VEnv} (h : Agree Γ.vars ρ) (x : Option String) {t : Ty} {w : Val}
    (hw : HasType w t) : Agree (Fun.bindOpt Γ x t).vars (bindVals ρ (x.toList.map (·, some w))) := by
  cases x
  · exact h
  · exact .cons (fun v e => by cases e; exact hw) h

theorem bindPat_sound {Γ : Ctx} {p : Pat} {t : Ty} {k : Kind} {Γ' : Ctx} (h : SQuiet (bindPat Γ p t k) Γ') :
    ∃ Δ, patBindings p t = .ok Δ ∧ Γ' = Γ.bindAllAs Δ k := by
  unfold bindPat at h
  cases hp : patBindings p t with
  | error c =>
    simp only [hp] at h
    exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | ok Δ =>
    simp only [hp, SQuiet.pure_iff] at h
    exact ⟨Δ, rfl, h.symm⟩

/-- Binding a pattern to a value of the type it takes apart. -/
theorem bindPat_vals {Γ : Ctx} {ρ : VEnv} (hρ : Agree Γ.vars ρ) {p : Pat} {t : Ty} {k : Kind} {Δ : List (String × Ty)}
    (hp : patBindings p t = .ok Δ) {v : Val} (hv : HasType v t) :
    SoundS Γ B (Agree (Γ.bindAllAs Δ k).vars)
      (patValues p v >>= fun vals => pure (bindVals ρ (vals.map fun (x, w) => (x, some w)))) := by
  obtain ⟨vals, he, hf⟩ := patValues_fits hp hv
  rw [he]
  exact bindAllAs_agree hρ k hf

/-- An expression run from variables that line up, as a statement runs it:
its value and the variables it leaves, which line up still. -/
theorem SoundS.ofT {α} {Γ : Ctx} {P : α → Prop} {m : EvalT α} {ρ : VEnv} (h : SoundT Γ.vars Γ.ret P m)
    (hρ : Agree Γ.vars ρ) : SoundS Γ B (fun r => P r.1 ∧ Agree Γ.vars r.2) (m.run ρ) := by
  have h := h ρ hρ
  generalize m.run ρ = r at h ⊢
  cases r with
  | ok p => exact h
  | error s =>
    cases s with
    | returned o ρ' =>
      cases o with
      | some r => exact ⟨h.1, Ext.of_agree h.2⟩
      | none => exact h.elim
    | early r => exact h.elim
    | brk _ | cont _ | fuel | wrong | retVoid => simp only [StopOkT, StopOk] at h
    | _ => exact h

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallOk sigs call)
include hcall
set_option linter.unusedSectionVars false

theorem bindChecked_sound {n : Nat} {Γ : Ctx} {ρ : VEnv} {p : Pat} {hint : Option Hint} {e : Expr} {k : Kind}
    {Γ' : Ctx} (h : SQuiet (bindChecked env n Γ p hint e k) Γ') (hb : Γ.Runs sigs) (hρ : Agree Γ.vars ρ) :
    SoundS Γ B (Agree Γ'.vars) (execBind env call n Γ ρ p hint e) ∧ Grows Γ Γ' := by
  cases hint with
  | some hh =>
    simp only [bindChecked, convertReporting] at h
    obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hrt
    rw [Quiet.liftE_iff] at hq
    dsimp only at h
    split at h
    · exact (SQuiet.push_bind (fun _ => bindPat_appends _ _ _ _) h).elim
    obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (fun _ => bindPat_appends _ _ _ _) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    obtain ⟨Δ, hp, rfl⟩ := bindPat_sound h
    refine ⟨?_, Grows.bindAllAs Γ k Δ⟩
    simp only [execBind, hq]
    exact SoundS.bind' (SoundS.ofT (evalC_sound env sigs call hcall n Γ e t hc hb) hρ)
      (fun ⟨v, ρ'⟩ hr => bindPat_vals hr.2 hp hr.1)
  | none =>
    simp only [bindChecked] at h
    obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hrt
    obtain ⟨Δ, hp, rfl⟩ := bindPat_sound h
    refine ⟨?_, Grows.bindAllAs Γ k Δ⟩
    simp only [execBind]
    exact SoundS.bind' (SoundS.ofT (evalS_sound env sigs call hcall n Γ e t hq hb) hρ)
      (fun ⟨v, ρ'⟩ hr => bindPat_vals hr.2 hp hr.1)

end

theorem atomListed_lookup {a : String} : ∀ {vs : Variants}, Lit.Variants.atomListed a vs = true →
    vs.toList.lookup a = some none
  | .nil, h => by simp [Lit.Variants.atomListed] at h
  | .atom n vs, h => by
    simp only [Lit.Variants.atomListed] at h
    by_cases e : n = a
    · subst e; simp [Variants.toList]
    · simp only [beq_iff_eq, e, ↓reduceIte] at h
      simp only [Variants.toList, List.lookup, show (a == n) = false by simpa using Ne.symm e]
      exact atomListed_lookup h
  | .term n _ vs, h => by
    simp only [Lit.Variants.atomListed] at h
    by_cases e : n = a
    · simp [e] at h
    · simp only [beq_iff_eq, e, ↓reduceIte] at h
      simp only [Variants.toList, List.lookup, show (a == n) = false by simpa using Ne.symm e]
      exact atomListed_lookup h

theorem termPayload_lookup {a : String} {p : Ty} : ∀ {vs : Variants}, Lit.Variants.termPayload a vs = some p →
    vs.toList.lookup a = some (some p)
  | .nil, h => by simp [Lit.Variants.termPayload] at h
  | .atom n vs, h => by
    simp only [Lit.Variants.termPayload] at h
    by_cases e : n = a
    · simp [e] at h
    · simp only [beq_iff_eq, e, ↓reduceIte] at h
      simp only [Variants.toList, List.lookup, show (a == n) = false by simpa using Ne.symm e]
      exact termPayload_lookup h
  | .term n t vs, h => by
    simp only [Lit.Variants.termPayload] at h
    by_cases e : n = a
    · subst e; simp only [beq_self_eq_true, ↓reduceIte, Option.some.injEq] at h; subst h
      simp [Variants.toList]
    · simp only [beq_iff_eq, e, ↓reduceIte] at h
      simp only [Variants.toList, List.lookup, show (a == n) = false by simpa using Ne.symm e]
      exact termPayload_lookup h

theorem _root_.Datalove.Lit.HasType.enum_inv {v : Val} {vs : Variants} (h : HasType v (.enum vs)) :
    (∃ a, v = .atom a ∧ vs.toList.lookup a = some none) ∨
      (∃ a w p, v = .term a w ∧ vs.toList.lookup a = some (some p) ∧ HasType w p) := by
  generalize ht : Ty.enum vs = t at h
  induction h with
  | atomEnum hl => cases ht; exact .inl ⟨_, rfl, atomListed_lookup hl⟩
  | termEnum hp hw => cases ht; exact .inr ⟨_, _, _, rfl, termPayload_lookup hp, hw⟩
  | hexInt _ ih => rcases ih ht with ⟨_, h, -⟩ | ⟨_, _, _, h, -⟩ <;> cases h
  | roll _ ih => subst ht; exact ih rfl
  | int hr => subst ht; simp [Lit.inRange] at hr
  | hex hr => subst ht; simp [Lit.hexFits] at hr
  | float hr => subst ht; simp [Ty.isFloat] at hr
  | _ => cases ht

theorem setModes_mem : ∀ (steps : List Step) (acc : List IdxMode) (m : IdxMode),
    (m ∈ acc ∨ ∃ k, Step.index k (some m) ∈ steps) → m ∈ steps.foldl setModes.addMode acc
  | [], acc, m, h => by simpa using h
  | st :: rest, acc, m, h => by
    simp only [List.foldl_cons]
    apply setModes_mem rest
    have keep : ∀ acc', m ∈ acc' → m ∈ setModes.addMode acc' st := by
      intro acc' hm
      unfold setModes.addMode
      split
      · split <;> simp_all
      · exact hm
    rcases h with h | ⟨k, hk⟩
    · exact .inl (keep acc h)
    · rcases List.mem_cons.mp hk with e | e
      · subst e
        left
        simp only [setModes.addMode]
        split <;> simp_all
      · exact .inr ⟨k, e⟩

theorem reportModes_fits {Γ : Ctx} : ∀ {ms : List IdxMode}, SQuiet (reportModes Γ ms) () →
    ∀ m ∈ ms, Γ.fitsMode m = true
  | [], _, m, hm => by simp at hm
  | m' :: ms, h, m, hm => by
    simp only [reportModes] at h
    obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => reportModes_appends _ _) h
    rw [SQuiet.reportIf_iff] at h1
    rcases List.mem_cons.mp hm with e | e
    · subst e; simpa using h1
    · exact reportModes_fits h m e

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallOk sigs call)
include hcall
set_option linter.unusedSectionVars false

theorem execSet_sound {n : Nat} {Γ : Ctx} {ρ : VEnv} {x : String} {steps : List Step} {op : Option BinOp}
    {e : Expr} (h : SQuiet (checkSet env n Γ x steps op e) ()) (hb : Γ.Runs sigs) (hρ : Agree Γ.vars ρ) :
    SoundS Γ B (Agree Γ.vars) (execSet env call n Γ ρ x steps op e) := by
  unfold checkSet at h
  cases hl : Γ.lookup x with
  | none => simp only [hl] at h; simp at h
  | some p =>
    obtain ⟨t, k⟩ := p
    simp only [hl] at h
    obtain ⟨_, -, h⟩ := SQuiet.bind_inv
      (fun _ => Appends.bind (reportModes_appends _ _) (fun _ => setSteps_appends _ _ _ _ _ _ _)) h
    obtain ⟨_, h2, h⟩ := SQuiet.bind_inv (fun _ => setSteps_appends _ _ _ _ _ _ _) h
    obtain ⟨o, ho, hot⟩ := hρ.lookup x t k hl
    simp only [execSet, hl, ho]
    exact SoundS.bind' (SoundS.ofT (execSetSteps_sound env sigs call hcall hb t o steps h hot
      (fun k m hk => reportModes_fits h2 m (setModes_mem steps [] m (.inr ⟨k, hk⟩)))) hρ)
      (fun ⟨w, ρ'⟩ hw => setVar_agree hw.2 hl hw.1)

theorem execRet_sound {n : Nat} {Γ : Ctx} {ρ : VEnv} {e : Option Expr} {P : VEnv → Prop}
    (h : SQuiet (checkRet env n Γ e) ()) (hb : Γ.Runs sigs) (hρ : Agree Γ.vars ρ) :
    SoundS Γ B P (execRet env call n Γ ρ e) := by
  unfold checkRet at h
  cases hr : Γ.ret <;> cases e <;> simp only [hr] at h
  · simp only [execRet, hr]; exact ⟨hr, Ext.of_agree hρ⟩
  · simp at h
  · simp at h
  · rename_i t e
    obtain ⟨_, hc, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    simp only [execRet, hr]
    exact SoundS.bind' (SoundS.ofT (evalC_sound env sigs call hcall n Γ e t hc hb) hρ)
      (fun ⟨v, ρ'⟩ hv => ⟨⟨t, hr, hv.1⟩, Ext.of_agree hv.2⟩)

end

theorem execLoop_sound {Γ : Ctx} {Bb Q : Prop} {cond : VEnv → EvalM (Bool × VEnv)} {body : VEnv → EvalM VEnv}
    (hc : ∀ ρ, Agree Γ.vars ρ → SoundS Γ False (fun r => (r.1 = false → Q) ∧ Agree Γ.vars r.2) (cond ρ))
    (hb : ∀ ρ, Agree Γ.vars ρ → SoundS { Γ with inLoop := true } Bb (Ext Γ.vars) (body ρ)) :
    ∀ k ρ, Agree Γ.vars ρ → SoundS Γ B (fun ρ' => Agree Γ.vars ρ' ∧ (Q ∨ Bb)) (execLoop cond body k ρ)
  | 0, _, _ => trivial
  | k + 1, ρ, hρ => by
    rw [execLoop]
    refine SoundS.bind' (SoundS.weakenB (hc ρ hρ) False.elim) (fun ⟨c, ρ⟩ ⟨hq, hρ⟩ => ?_)
    cases c
    · exact ⟨hρ, .inl (hq rfl)⟩
    · have hbody := hb ρ hρ
      simp only [Bool.not_true, Bool.false_eq_true, ↓reduceIte]
      generalize body ρ = r at hbody ⊢
      cases r with
      | ok ρ' => exact execLoop_sound hc hb k _ (restore_agree hρ hbody)
      | error s =>
        cases s with
        | brk ρ' => exact ⟨restore_agree hρ hbody.2.1, .inr hbody.2.2⟩
        | cont ρ' => exact execLoop_sound hc hb k _ (restore_agree hρ hbody.2)
        | _ => exact hbody

theorem execFor_sound {Γ : Ctx} {Bb : Prop} {Δ : List (String × Ty)} {body : VEnv → EvalM VEnv}
    {names : List String}
    (hb : ∀ ρ vals, Agree Γ.vars ρ → Fits Δ vals →
      SoundS { Γ with inLoop := true } Bb (Ext Γ.vars) (body (bindVals ρ vals))) :
    ∀ (items : List (List Val)), (∀ item ∈ items, Fits Δ ((names.zip item).map fun (x, w) => (x, some w))) →
      ∀ ρ, Agree Γ.vars ρ → SoundS Γ B (Agree Γ.vars) (execFor body names items ρ)
  | [], _, _, hρ => hρ
  | item :: rest, hi, ρ, hρ => by
    rw [execFor]
    have hbody := hb ρ _ hρ (hi item (by simp))
    have hrest := execFor_sound hb rest (fun it h => hi it (by simp [h]))
    generalize body _ = r at hbody ⊢
    cases r with
    | ok ρ' => exact hrest _ (restore_agree hρ hbody)
    | error s =>
      cases s with
      | brk ρ' => exact restore_agree hρ hbody.2.1
      | cont ρ' => exact hrest _ (restore_agree hρ hbody.2)
      | _ => exact hbody

theorem typeOf_pure {m : ChkM Ty} {t : Ty} (h : Quiet m t) : typeOf m = pure t := typeOf_quiet h

theorem Ext.of_grows {Γ Γ' : Ctx} (g : Grows Γ Γ') {ρ : VEnv} (h : Ext Γ'.vars ρ) : Ext Γ.vars ρ := by
  obtain ⟨-, -, -, -, D, hv⟩ := g
  rw [hv] at h
  exact Ext.of_append h

/-! ## What reaches the end, and what breaks -/

theorem casesBreak_of_mem' {c : Case} {cs : List Case} (h : casesBreak cs = true) :
    casesBreak (c :: cs) = true := by
  cases c <;> simp [casesBreak, h]

theorem if_completes_then {c : Expr} {a b : Option String} {t : List Stmt} {e : Option (List Stmt)}
    (h : completes t = true) : completesS (.if_ c a b t e) = true := by
  cases e <;> simp [completesS, h]

theorem if_completes_else {c : Expr} {a b : Option String} {t : List Stmt} {e : Option (List Stmt)}
    (h : e = none ∨ completesElse e = true) : completesS (.if_ c a b t e) = true := by
  rcases h with rfl | h
  · simp [completesS]
  · cases e <;> simp_all [completesS, completesElse]

theorem lookup_some_mem {α β} [BEq α] [LawfulBEq α] {a : α} {b : β} :
    ∀ {l : List (α × β)}, l.lookup a = some b → (a, b) ∈ l
  | [], h => by simp at h
  | (x, y) :: l, h => by
    simp only [List.lookup] at h
    split at h
    · rename_i e; simp only [beq_iff_eq] at e; subst e; simp only [Option.some.injEq] at h; subst h; simp
    · exact List.mem_cons_of_mem _ (lookup_some_mem h)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallOk sigs call)
include hcall
-- The mutual theorems use `hcall` through one another and the expression
-- theorems, which the linter does not follow.
set_option linter.unusedSectionVars false

omit hcall in
theorem forBindings_sound {n : Nat} {Γ : Ctx} {names : List String} {coll : Expr}
    {r0 : Option (List (String × Ty))} (h : SQuiet (forBindings env n Γ names coll) r0) :
    ∃ Δ t, r0 = some Δ ∧ Quiet (synthF env n Γ.borrow coll) t ∧ walk t.expose names = .ok Δ := by
  unfold forBindings at h
  split at h
  · obtain ⟨Δ, rfl, hq⟩ := SQuiet.chk_inv h
    obtain ⟨t, ht, hw⟩ := hq.bind_inv
    rw [Quiet.liftE_iff] at hw
    exact ⟨Δ, t, rfl, ht, hw⟩
  · exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim

omit hcall in
/-- The names a `match`'s cases cover, as `checkCases` gives them. -/
theorem checkCases_cov : ∀ (n : Nat) (Γ : Ctx) (vs : List (String × Option Ty)) (cs : List Case)
    (cov cov' : List String), SQuiet (checkCases env n Γ vs cs cov) cov' → cov' = cov ++ cs.map (·.name)
  | 0, _, _, _, _, _, h => by
    rw [checkCases.eq_def] at h; exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | _ + 1, _, _, [], cov, cov', h => by
    rw [checkCases.eq_def] at h; simp only [SQuiet.pure_iff] at h; simp [h]
  | n + 1, Γ, vs, c :: cs, cov, cov', h => by
    rw [checkCases.eq_def] at h
    dsimp only at h
    split at h
    · exact (SQuiet.push_bind (by app_cont env n) h).elim
    obtain ⟨_, -, h⟩ := SQuiet.bind_inv (by app_cont env n) h
    rw [checkCases_cov n Γ vs cs _ cov' h]
    simp

mutual

/-- A statement that checks runs soundly, into the context the checker gives
after it: it breaks only if it has a `break` of its own, and ends normally
only if control can reach its end. -/
theorem execStmt_sound : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (s : Stmt) (Γ' : Ctx),
    SQuiet (checkStmtF env n Γ s) Γ' → Γ.Runs sigs → Agree Γ.vars ρ →
    SoundS Γ (hasBreakS s = true) (fun ρ' => Agree Γ'.vars ρ' ∧ completesS s = true)
      (execStmt env call n k Γ ρ s) ∧ Grows Γ Γ'
  | 0, _, _, _, _, _, h, _, _ => by
    simp only [checkStmtF] at h; exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | n + 1, k, Γ, ρ, s, Γ', h, hb, hρ => by
    cases s
    case let_ p hint e =>
      simp only [checkStmtF] at h; simp only [execStmt]
      obtain ⟨h1, g⟩ := bindChecked_sound env sigs call hcall h hb hρ
      exact ⟨h1.weaken (fun _ h => ⟨h, rfl⟩), g⟩
    case var_ p hint v =>
      cases v with
      | some e =>
        simp only [checkStmtF] at h; simp only [execStmt]
        obtain ⟨h1, g⟩ := bindChecked_sound env sigs call hcall h hb hρ
        exact ⟨h1.weaken (fun _ h => ⟨h, rfl⟩), g⟩
      | none =>
        cases hint with
        | none =>
          simp only [checkStmtF, SQuiet.pure_iff] at h; subst h; simp only [execStmt]
          exact ⟨⟨hρ, rfl⟩, Grows.refl Γ⟩
        | some hh =>
          simp only [checkStmtF, convertReporting] at h
          obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
          obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hrt
          rw [Quiet.liftE_iff] at hq
          dsimp only at h
          obtain ⟨_, -, h⟩ := SQuiet.bind_inv (fun _ => bindPat_appends _ _ _ _) h
          obtain ⟨Δ, hp, rfl⟩ := bindPat_sound h
          simp only [execStmt, hq, hp]
          exact ⟨⟨bindAllAs_agree hρ _ (fits_none Δ), rfl⟩, Grows.bindAllAs Γ _ Δ⟩
    case set_ x steps op e =>
      simp only [checkStmtF] at h
      obtain ⟨_, hs, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      simp only [execStmt]
      exact ⟨(execSet_sound env sigs call hcall hs hb hρ).weaken (fun _ h => ⟨h, rfl⟩), Grows.refl Γ⟩
    case fun_ f =>
      simp only [checkStmtF, SQuiet.pure_iff] at h; subst h; simp only [execStmt]
      exact ⟨⟨hρ, rfl⟩, Grows.refl Γ⟩
    case ret e =>
      simp only [checkStmtF] at h
      obtain ⟨_, hs, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      simp only [execStmt]
      exact ⟨execRet_sound env sigs call hcall hs hb hρ, Grows.refl Γ⟩
    case brk =>
      simp only [checkStmtF] at h
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      rw [SQuiet.reportIf_iff] at h1
      simp only [execStmt]
      exact ⟨⟨by simpa using h1, Ext.of_agree hρ, rfl⟩, Grows.refl Γ⟩
    case cont =>
      simp only [checkStmtF] at h
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      rw [SQuiet.reportIf_iff] at h1
      simp only [execStmt]
      exact ⟨⟨by simpa using h1, Ext.of_agree hρ⟩, Grows.refl Γ⟩
    case debuglog e =>
      simp only [checkStmtF] at h
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv h1
      simp only [execStmt]
      exact ⟨SoundS.bind' (SoundS.ofT (evalS_sound env sigs call hcall n _ e t hq hb) hρ)
        (fun ⟨_, ρ'⟩ hr => ⟨hr.2, rfl⟩), Grows.refl Γ⟩
    case call e =>
      simp only [checkStmtF] at h
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv h1
      dsimp only at h
      obtain ⟨_, -, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      simp only [execStmt]
      exact ⟨SoundS.bind' (SoundS.ofT (evalS_sound env sigs call hcall n Γ e t hq hb) hρ)
        (fun ⟨_, ρ'⟩ hr => ⟨hr.2, rfl⟩), Grows.refl Γ⟩
    case const_ x hint e =>
      simp only [checkStmtF] at h
      obtain ⟨Γ1, hb1, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
      obtain ⟨_, -, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      obtain ⟨hs, g⟩ := bindChecked_sound env sigs call hcall (Γ := { Γ with constExpr := true }) hb1 hb hρ
      simp only [execStmt]
      refine ⟨?_, g⟩
      first
        | exact hs.weaken (fun _ h => ⟨h, rfl⟩)
        | exact SoundS.bind' hs (fun _ h' => ⟨h', rfl⟩)
    case type_ x hh =>
      simp only [checkStmtF, SQuiet.pure_iff] at h; subst h
      simp only [execStmt]
      split <;> exact ⟨⟨hρ, rfl⟩, Grows.refl Γ⟩
    case loop c body =>
      cases c with
      | none =>
        simp only [checkStmtF] at h
        obtain ⟨_, -, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, hbody, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        simp only [execStmt]
        refine ⟨(execLoop_sound (Γ := Γ) (Q := False)
          (fun ρ hρ => SoundS.pure' (P := fun r : Bool × VEnv => (r.1 = false → False) ∧ Agree Γ.vars r.2)
            ⟨fun e => (by cases e), hρ⟩)
          (fun ρ hρ => (execBlock_sound n k { Γ with inLoop := true } ρ body hbody hb hρ).weaken
            (fun _ h => h.1)) k ρ hρ).weaken (fun _ h => ⟨h.1, ?_⟩), Grows.refl Γ⟩
        rcases h.2 with h | h
        · exact h.elim
        · simpa [completesS] using h
      | some c =>
        simp only [checkStmtF] at h
        obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, hbody, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        obtain ⟨_, hc, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hc
        obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
        simp only [execStmt]
        refine ⟨(execLoop_sound (Γ := Γ) (Q := True) (fun ρ hρ => SoundS.bind' (SoundS.ofT (evalC_sound env sigs call hcall n Γ c .bool hc hb) hρ) (fun ⟨v, ρ⟩ ⟨hv, hρ⟩ => ?_))
          (fun ρ hρ => (execBlock_sound n k { Γ with inLoop := true } ρ body hbody hb hρ).weaken
            (fun _ h => h.1)) k ρ hρ).weaken (fun _ h => ⟨h.1, rfl⟩), Grows.refl Γ⟩
        obtain ⟨b, rfl⟩ := hv.bool_inv
        exact SoundS.pure' ⟨fun _ => trivial, hρ⟩
    case if_ c a b t e =>
      cases a <;> cases b
      case none.none =>
        simp only [checkStmtF] at h
        obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, ht, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, he, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
        simp only [execStmt]
        refine ⟨SoundS.bind' (SoundS.ofT (evalC_sound env sigs call hcall n Γ c .bool hc hb) hρ) (fun ⟨v, ρ⟩ ⟨hv, hρ⟩ => ?_), Grows.refl Γ⟩
        obtain ⟨b, rfl⟩ := hv.bool_inv
        simp only [boolOf, pure_bind]
        cases b
        · simp only [Bool.false_eq_true, ↓reduceIte]
          exact ((execElse_sound n k Γ ρ e he hb hρ).weakenB (fun h => by simp [hasBreakS, h])).weaken
            (fun _ h => ⟨h.1, if_completes_else h.2⟩)
        · simp only [↓reduceIte]
          exact SoundS.bind' ((execBlock_sound n k Γ ρ t ht hb hρ).weakenB (fun h => by simp [hasBreakS, h]))
            (fun ρ' h' => ⟨restore_agree hρ h'.1, if_completes_then h'.2⟩)
      all_goals
        simp only [checkStmtF] at h
        obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨ct, rfl, hq⟩ := SQuiet.chk_inv hc
        try dsimp only at h
        obtain ⟨_, hm, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        refine ⟨?_, Grows.refl Γ⟩
        simp only [execStmt, typeOf_pure hq, pure_bind]
        refine SoundS.bind' (SoundS.ofT (evalS_sound env sigs call hcall n Γ c ct hq hb) hρ) (fun ⟨v, ρ⟩ ⟨hv, hρ⟩ => ?_)
        have hve := hv.expose
        try dsimp only at hm
        split at hm
        · rename_i p hp
          rw [hp] at hve
          obtain ⟨_, hbt, hm⟩ := SQuiet.bind_inv (by app_cont env n) hm
          obtain ⟨_, -, he⟩ := SQuiet.bind_inv (by app_cont env n) hm
          rcases hve.option_inv with rfl | ⟨w, rfl, hw⟩
          · simp only [hp]
            exact ((execElse_sound n k Γ ρ e he hb hρ).weakenB (fun h => by simp [hasBreakS, h])).weaken
              (fun _ h => ⟨h.1, if_completes_else h.2⟩)
          · simp only [hp]
            exact SoundS.bind' ((SoundS.lift (Grows.bindOpt Γ _ p) (execBlock_sound n k _ _ t hbt
              ((Grows.bindOpt Γ _ p).runs hb) (bindOpt_agree hρ _ hw))).weakenB
                (fun h => by simp [hasBreakS, h]))
              (fun ρ' h' => ⟨restore_agree hρ (Ext.of_grows (Grows.bindOpt Γ _ p) h'.1),
                if_completes_then h'.2⟩)
        · rename_i p hp
          rw [hp] at hve
          try dsimp only at hm
          split at hm
          · simp at hm
          obtain ⟨_, hbt, he⟩ := SQuiet.bind_inv (by app_cont env n) hm
          rcases hve.result_inv with ⟨w, rfl, hw⟩ | ⟨w, rfl, hw⟩
          · simp only [hp]
            exact SoundS.bind' ((SoundS.lift (Grows.bindOpt Γ _ p) (execBlock_sound n k _ _ t hbt
              ((Grows.bindOpt Γ _ p).runs hb) (bindOpt_agree hρ _ hw))).weakenB
                (fun h => by simp [hasBreakS, h]))
              (fun ρ' h' => ⟨restore_agree hρ (Ext.of_grows (Grows.bindOpt Γ _ p) h'.1),
                if_completes_then h'.2⟩)
          · simp only [hp]
            exact SoundS.bind' ((SoundS.lift (Grows.bindOpt Γ _ .error) (execElse_sound n k _ _ e he
              ((Grows.bindOpt Γ _ .error).runs hb) (bindOpt_agree hρ _ hw))).weakenB
                (fun h => by simp [hasBreakS, h]))
              (fun ρ' h' => ⟨restore_agree hρ (Ext.of_grows (Grows.bindOpt Γ _ .error) (Ext.of_agree h'.1)),
                if_completes_else h'.2⟩)
        · simp at hm
    case match_ e cs dflt =>
      simp only [checkStmtF] at h
      obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hc
      try dsimp only at h
      obtain ⟨_, hm, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      refine ⟨?_, Grows.refl Γ⟩
      simp only [execStmt, typeOf_pure hq, pure_bind]
      refine SoundS.bind' (SoundS.ofT (evalS_sound env sigs call hcall n Γ e t hq hb) hρ) (fun ⟨v, ρ⟩ ⟨hv, hρ⟩ => ?_)
      try dsimp only at hm
      split at hm
      · rename_i vs hvs
        try dsimp only at hm
        obtain ⟨cov, hcs, hm⟩ := SQuiet.bind_inv (by app_cont env n) hm
        obtain ⟨_, hd, h100⟩ := SQuiet.bind_inv (by app_cont env n) hm
        rw [SQuiet.reportIf_iff] at h100
        have hcov := checkCases_cov env n Γ vs.toList cs [] cov hcs
        have hve := hv.expose
        rw [hvs] at hve
        simp only [hvs]
        have finish : ∀ a, (∃ o, vs.toList.lookup a = some o) → ((v = .atom a ∧ vs.toList.lookup a = some none) ∨
            (∃ w p, v = .term a w ∧ vs.toList.lookup a = some (some p) ∧ HasType w p)) →
            SoundS Γ (hasBreakS (.match_ e cs dflt) = true)
              (fun ρ' => Agree Γ.vars ρ' ∧ completesS (.match_ e cs dflt) = true)
              (execCases env call n k Γ ρ vs.toList a v cs >>= fun o => match o with
                | some ρ' => pure (restore ρ ρ')
                | none => execElse env call n k Γ ρ dflt) := fun a ⟨o, hl⟩ ha =>
          SoundS.bind' ((execCases_sound n k Γ ρ vs.toList a v cs [] cov hcs hb hρ ha).weakenB
              (fun h => by simp [hasBreakS, h])) (fun r ⟨hsome, hnone⟩ => by
            cases r with
            | some ρ' =>
              obtain ⟨h1, h2⟩ := hsome ρ' rfl
              exact ⟨restore_agree hρ h1, by simp [completesS, h2]⟩
            | none =>
              refine ((execElse_sound n k Γ ρ dflt hd hb hρ).weakenB (fun h => by simp [hasBreakS, h])).weaken
                (fun _ h => ⟨h.1, ?_⟩)
              rcases h.2 with hn | hc
              · exfalso
                subst hn
                simp only [Option.isNone_none, Bool.true_and, List.any_eq_false, Bool.not_eq_eq_eq_not,
                  Bool.not_true, Bool.not_eq_false] at h100
                have := h100 (a, o) (lookup_some_mem hl)
                rw [hcov] at this
                simp only [List.nil_append, List.contains_iff_mem, List.mem_map] at this
                obtain ⟨c, hc, hn'⟩ := this
                exact hnone rfl c hc hn'
              · simp [completesS, hc])
        rcases hve.enum_inv with ⟨a, rfl, hl⟩ | ⟨a, w, p, rfl, hl, hw⟩
        · exact finish a ⟨_, hl⟩ (.inl ⟨rfl, hl⟩)
        · exact finish a ⟨_, hl⟩ (.inr ⟨w, p, rfl, hl, hw⟩)
      · simp at hm
    case for_ names coll body =>
      simp only [checkStmtF] at h
      obtain ⟨r0, hf, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      obtain ⟨Δ, t, rfl, ht, hw⟩ := forBindings_sound env hf
      dsimp only at h
      obtain ⟨_, hbody, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      refine ⟨?_, Grows.refl Γ⟩
      have hrun : (forBindings env n Γ names coll).run #[] = (some Δ, #[]) := hf
      simp only [execStmt, hrun, typeOf_pure ht, pure_bind]
      refine SoundS.bind' (SoundS.ofT (evalS_sound env sigs call hcall n Γ.borrow coll t ht hb) hρ) (fun ⟨v, ρ⟩ ⟨hv, hρ⟩ => ?_)
      have hi := walkItems_fits hw hv.expose
      cases hwi : walkItems t.expose names v with
      | error s => rw [hwi] at hi; subst hi; trivial
      | ok items =>
        rw [hwi] at hi
        refine SoundS.bind' (P := (· = items)) rfl (fun _ e => ?_)
        rw [e]
        refine (execFor_sound (Bb := hasBreak body = true) (Δ := Δ) (fun ρ' vals hρ' hv' => ?_) items hi ρ hρ).weaken
          (fun _ h => ⟨h, rfl⟩)
        have g := Grows.bindAllAs { Γ with inLoop := true } .let_ Δ
        have := execBlock_sound n k ({ Γ with inLoop := true }.bindAll Δ) _ body hbody
          (g.runs hb) (bindAllAs_agree (Γ := { Γ with inLoop := true }) hρ' .let_ hv')
        exact SoundS.weaken (SoundS.lift g this) (fun _ h => Ext.of_grows g h.1)

/-- The case a `match` takes runs soundly, as `checkCases` checks it; there
is none only if no case is of the variant. -/
theorem execCases_sound : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (vs : List (String × Option Ty)) (a : String)
    (v : Val) (cs : List Case) (cov cov' : List String), SQuiet (checkCases env n Γ vs cs cov) cov' →
    Γ.Runs sigs → Agree Γ.vars ρ →
    ((v = .atom a ∧ vs.lookup a = some none) ∨
      (∃ w p, v = .term a w ∧ vs.lookup a = some (some p) ∧ HasType w p)) →
    SoundS Γ (casesBreak cs = true)
      (fun o => (∀ ρ', o = some ρ' → Ext Γ.vars ρ' ∧ casesComplete cs = true) ∧
        (o = none → ∀ c ∈ cs, c.name ≠ a))
      (execCases env call n k Γ ρ vs a v cs)
  | 0, _, _, _, _, _, _, _, _, _, h, _, _, _ => by
    rw [checkCases.eq_def] at h; exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | _ + 1, _, _, _, _, _, _, [], _, _, _, _, _, _ => by
    rw [execCases.eq_def]
    exact And.intro (fun ρ' (h : (none : Option VEnv) = some ρ') => by cases h)
      (fun _ c (hc : c ∈ ([] : List Case)) => by simp at hc)
  | n + 1, k, Γ, ρ, vs, a, v, c :: cs, cov, cov', h, hb, hρ, hv => by
    rw [checkCases.eq_def] at h
    dsimp only at h
    split at h
    · exact (SQuiet.push_bind (by app_cont env n) h).elim
    obtain ⟨_, hbk, h⟩ := SQuiet.bind_inv (by app_cont env n) h
    rw [execCases.eq_def]; dsimp only
    have mem : c ∈ c :: cs := List.mem_cons_self
    by_cases hn : c.name = a
    · simp only [hn, beq_self_eq_true, ↓reduceIte]
      cases c with
      | atom name b =>
        simp only [Case.name] at hn; subst hn
        cases hl : vs.lookup name with
        | none => simp only [Case.name, hl] at hbk; exact (SQuiet.push_bind (by app_cont env n) hbk).elim
        | some p =>
          simp only [Case.name, hl] at hbk
          obtain ⟨_, -, hbk⟩ := SQuiet.bind_inv (by app_cont env n) hbk
          exact SoundS.map' ((execBlock_sound n k Γ ρ b hbk hb hρ).weakenB
              (fun h => casesBreak_of_mem mem h))
            (fun ρ' h' => ⟨fun ρ'' e => by cases e; exact ⟨h'.1, casesComplete_of_mem mem h'.2⟩,
              fun e => by cases e⟩)
      | term name x b =>
        simp only [Case.name] at hn; subst hn
        cases hl : vs.lookup name with
        | none => simp only [Case.name, hl] at hbk; exact (SQuiet.push_bind (by app_cont env n) hbk).elim
        | some o =>
          cases o with
          | none =>
            simp only [Case.name, hl] at hbk; exact (SQuiet.push_bind (by app_cont env n) hbk).elim
          | some p =>
            simp only [Case.name, hl] at hbk
            rcases hv with ⟨rfl, hl'⟩ | ⟨w, p', rfl, hl', hw⟩
            · rw [hl] at hl'; cases hl'
            · rw [hl] at hl'; cases hl'
              have g := Grows.bind Γ x p .let_
              exact SoundS.map' ((SoundS.lift g (execBlock_sound n k _ _ b hbk (g.runs hb)
                (.cons (fun u e => by cases e; exact hw) hρ))).weakenB (fun h => casesBreak_of_mem mem h))
                (fun ρ' h' => ⟨fun ρ'' e => by
                  cases e; exact ⟨Ext.of_grows g h'.1, casesComplete_of_mem mem h'.2⟩, fun e => by cases e⟩)
    · simp only [beq_false_of_ne hn, Bool.false_eq_true, ↓reduceIte]
      refine ((execCases_sound n k Γ ρ vs a v cs _ cov' h hb hρ hv).weakenB
        (fun h => casesBreak_of_mem' h)).weaken (fun o ⟨hs, hnone⟩ => ⟨fun ρ' e => ?_, fun e c' hc' => ?_⟩)
      · obtain ⟨h1, h2⟩ := hs ρ' e
        exact ⟨h1, by cases c <;> simp [casesComplete, h2]⟩
      · rcases List.mem_cons.mp hc' with rfl | hc'
        · exact hn
        · exact hnone e c' hc'

/-- An `else` that checks runs soundly, its scope left behind. -/
theorem execElse_sound : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (e : Option (List Stmt)),
    SQuiet (checkElse env n Γ e) () → Γ.Runs sigs → Agree Γ.vars ρ →
    SoundS Γ (hasBreakElse e = true) (fun ρ' => Agree Γ.vars ρ' ∧ (e = none ∨ completesElse e = true))
      (execElse env call n k Γ ρ e)
  | n, k, Γ, ρ, some e, h, hb, hρ => by
    rw [checkElse.eq_def] at h; rw [execElse.eq_def]; dsimp only
    exact SoundS.bind' (execBlock_sound n k Γ ρ e h hb hρ)
      (fun ρ' h' => ⟨restore_agree hρ h'.1, .inr h'.2⟩)
  | _, _, _, ρ, none, _, _, hρ => by rw [execElse.eq_def]; exact ⟨hρ, .inl rfl⟩

/-- A block that checks runs soundly: what it leaves is its scope's
variables with more bound, each that is set of its type; it breaks only if
it has a `break` of its own, and ends normally only if control can reach its
end. -/
theorem execBlock_sound : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (ss : List Stmt),
    SQuiet (checkBlockF env n Γ ss) () → Γ.Runs sigs → Agree Γ.vars ρ →
    SoundS Γ (hasBreak ss = true) (fun ρ' => Ext Γ.vars ρ' ∧ completes ss = true)
      (execBlock env call n k Γ ρ ss)
  | 0, _, _, _, _, h, _, _ => by rw [checkBlockF.eq_def] at h; simp at h
  | _ + 1, _, Γ, ρ, [], _, _, hρ => by
    rw [execBlock.eq_def]
    exact (⟨Ext.of_agree hρ, rfl⟩ : Ext Γ.vars ρ ∧ completes [] = true)
  | n + 1, k, Γ, ρ, s :: ss, h, hb, hρ => by
    rw [checkBlockF.eq_def] at h; dsimp only at h
    obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkBlockF_appends _ _ _ _) h
    obtain ⟨hs', g⟩ := execStmt_sound n k Γ ρ s Γ' hs hb hρ
    rw [execBlock.eq_def]; dsimp only
    have hrun : ((checkStmtF env n Γ s).run #[]).1 = Γ' := by unfold SQuiet at hs; rw [hs]
    rw [hrun]
    refine SoundS.bind' (hs'.weakenB (fun h => by simp [hasBreak, h])) (fun ρ' h' => ?_)
    exact ((SoundS.lift g (execBlock_sound n k Γ' ρ' ss h (g.runs hb) h'.1)).weakenB
      (fun h => by simp [hasBreak, h])).weaken
      (fun _ h => ⟨Ext.of_grows g h.1, by simp [completes, h'.2, h.2]⟩)

end

end

end Datalove.Fun
