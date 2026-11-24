import Datalove.FunInit
import Datalove.Proofs.RunSound

/-!
# A script the analysis lets through never reads what holds nothing

`Fun.runScript_init`: where a script checks (`checkScript`) and the
analysis of what holds a value lets it through (`initScript`), running it
stops at `Stop.uninit` only while evaluating its consts, before anything
else: past them no variable is read while it holds nothing, and no call
returns with a `mut` or `out` parameter it left holding nothing.

The analysis keeps, for each variable in scope, whether it holds a value,
and the run keeps the variables themselves, both innermost first, name by
name; they line up where every variable the analysis says holds a value
does (`IAg`). Running a step keeps every value there was (`Keeps`): a
variable is written, never emptied. So a variable the analysis reads holds
a value, whatever way the run came, and where the run leaves the function,
early or by `ret`, every `mut` and `out` parameter holds one too (`ExitOk`).

None of this needs the types: it holds of any run, of what checks and of
what does not, given calls that keep the same promise (`CallInit`).
`runCall` keeps it of a program each of whose bodies the analysis lets
through (`ProgInit`), and a script's functions are such a program, the
consts each sees holding values (`constPass_has`).

That evaluating the consts never reads one not yet evaluated is
`Proofs/ConstSound.lean`'s, which takes the functions a const can reach as
a program of their own (`ProgInit` of signatures among the run's,
`SigsIn`), each const a function sees holding a value or not.
-/

namespace Datalove.Fun

open Lit (Val)

/-- Every signature `A` has, `S` has too: the analysis's signatures, a
script's or those a const may reach, and the run's. -/
def SigsIn (A S : List (String × Sig)) : Prop :=
  ∀ f sig, A.lookup f = some sig → S.lookup f = some sig

theorem SigsIn.refl (A : List (String × Sig)) : SigsIn A A := fun _ _ h => h

theorem SigsIn.of_eq {A S S' : List (String × Sig)} (h : SigsIn A S) (e : S' = S) : SigsIn A S' := e ▸ h

/-! ## Lining up -/

/-- The analysis's variables and the run's, name by name, each the analysis
says holds a value holding one. -/
inductive IAg : ISt → VEnv → Prop where
  | nil : IAg [] []
  | cons {x : String} {s out : Bool} {o : Option Val} {σ : ISt} {ρ : VEnv} :
      (s = true → o.isSome = true) → IAg σ ρ → IAg ((x, s, out) :: σ) ((x, o) :: ρ)

/-- The variables after a step: the same ones, each that held a value
holding one still. -/
inductive Keeps : VEnv → VEnv → Prop where
  | nil : Keeps [] []
  | cons {x : String} {o o' : Option Val} {ρ ρ' : VEnv} :
      (o.isSome = true → o'.isSome = true) → Keeps ρ ρ' → Keeps ((x, o) :: ρ) ((x, o') :: ρ')

theorem Keeps.refl : ∀ (ρ : VEnv), Keeps ρ ρ
  | [] => .nil
  | _ :: ρ => .cons id (Keeps.refl ρ)

theorem Keeps.trans : ∀ {a b c : VEnv}, Keeps a b → Keeps b c → Keeps a c
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons h1 k1, .cons h2 k2 => .cons (fun h => h2 (h1 h)) (Keeps.trans k1 k2)

theorem Keeps.length : ∀ {a b : VEnv}, Keeps a b → a.length = b.length
  | _, _, .nil => rfl
  | _, _, .cons _ k => by simp [Keeps.length k]

theorem IAg.keeps : ∀ {σ : ISt} {ρ ρ' : VEnv}, IAg σ ρ → Keeps ρ ρ' → IAg σ ρ'
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons h a, .cons hk k => .cons (fun hs => hk (h hs)) (IAg.keeps a k)

theorem IAg.length : ∀ {σ : ISt} {ρ : VEnv}, IAg σ ρ → σ.length = ρ.length
  | _, _, .nil => rfl
  | _, _, .cons _ a => by simp [IAg.length a]

/-- A variable the analysis does not say holds nothing is no variable that
holds nothing. -/
theorem IAg.lookup : ∀ {σ : ISt} {ρ : VEnv} {x : String}, IAg σ ρ → σ.holds x ≠ some false →
    ρ.lookup x ≠ some none
  | _, _, _, .nil, _ => by simp
  | (y, s, _) :: σ, (_, o) :: ρ, x, .cons h a, hx => by
    simp only [ISt.holds, List.lookup] at hx ⊢
    by_cases e : x = y
    · subst e
      simp only [beq_self_eq_true, Option.map_some] at hx ⊢
      cases s with
      | false => exact absurd rfl hx
      | true => intro ho; cases ho; simp at h
    · have : (x == y) = false := by simpa using e
      simp only [this] at hx ⊢
      exact IAg.lookup a hx

theorem Keeps.lookup : ∀ {ρ ρ' : VEnv} {x : String}, Keeps ρ ρ' → ρ.lookup x ≠ some none →
    ρ'.lookup x ≠ some none
  | _, _, _, .nil, h => h
  | (y, o) :: ρ, (_, o') :: ρ', x, .cons h k, hx => by
    simp only [List.lookup] at hx ⊢
    by_cases e : x = y
    · subst e
      simp only [beq_self_eq_true] at hx ⊢
      intro ho; cases ho
      cases o with
      | none => exact hx rfl
      | some v => simp at h
    · have : (x == y) = false := by simpa using e
      simp only [this] at hx ⊢
      exact Keeps.lookup k hx

theorem setVar_keeps (x : String) (v : Val) : ∀ (ρ : VEnv), Keeps ρ (setVar x v ρ)
  | [] => .nil
  | (y, o) :: ρ => by
    simp only [setVar]
    split
    · exact .cons (fun _ => rfl) (Keeps.refl ρ)
    · exact .cons id (setVar_keeps x v ρ)

theorem IAg.give : ∀ {σ : ISt} {ρ : VEnv} (x : String) (v : Val), IAg σ ρ → IAg (σ.give x) (setVar x v ρ)
  | _, _, _, _, .nil => .nil
  | (y, s, out) :: σ, (_, o) :: ρ, x, v, .cons h a => by
    simp only [ISt.give, setVar]
    split
    · exact .cons (fun _ => rfl) a
    · exact .cons h (IAg.give x v a)

/-! ## The analysis's shape

The analysis never renames a variable or makes one an `out` parameter: a
step keeps what it had, and binds more in front, none of them `out`
parameters. -/

/-- The variables' names, and which are `out` parameters. -/
def ISt.shape (σ : ISt) : List (String × Bool) := σ.map fun v => (v.1, v.2.2)

theorem give_shape (x : String) : ∀ (σ : ISt), (σ.give x).shape = σ.shape
  | [] => rfl
  | (y, s, o) :: σ => by
    simp only [ISt.give]
    split
    · rfl
    · simp only [ISt.shape, List.map_cons] at *; exact congrArg _ (give_shape x σ)

theorem readI_eq {σ σ' : ISt} {x : String} (h : readI σ x = .ok σ') : σ' = σ ∧ σ.holds x ≠ some false := by
  unfold readI at h
  split at h
  · simp [throw, throwThe, MonadExceptOf.throw] at h
  · rename_i hn
    simp only [pure, Except.pure, Except.ok.injEq] at h
    exact ⟨h.symm, by simpa using hn⟩

theorem exitI_eq {σ σ' : ISt} (h : exitI σ = .ok σ') : σ' = σ ∧ σ.outsSet = true := by
  unfold exitI at h
  split at h
  · rename_i ho
    simp only [pure, Except.pure, Except.ok.injEq] at h
    exact ⟨h.symm, ho⟩
  · simp [throw, throwThe, MonadExceptOf.throw] at h

theorem giveArgs_shape : ∀ (σ : ISt) (args : List (Option Mode × Expr)) (ms : List Mode),
    (giveArgs σ args ms).shape = σ.shape
  | σ, (_, e) :: args, m :: ms => by
    simp only [giveArgs]
    rw [giveArgs_shape]
    split
    · split
      · exact give_shape _ σ
      · rfl
    · rfl
  | _, [], _ => rfl
  | _, _ :: _, [] => rfl

/-- A step of the analysis that keeps its shape. -/
def KeepsShape (r : InitM ISt) (σ : ISt) : Prop := ∀ σ', r = .ok σ' → σ'.shape = σ.shape

theorem KeepsShape.bind {r : InitM ISt} {f : ISt → InitM ISt} {σ : ISt} (h : KeepsShape r σ)
    (hf : ∀ σ1, σ1.shape = σ.shape → KeepsShape (f σ1) σ) : KeepsShape (r >>= f) σ := by
  intro σ' e
  cases hr : r with
  | error c => rw [hr] at e; cases e
  | ok σ1 => rw [hr] at e; exact hf σ1 (h σ1 hr) σ' e

theorem KeepsShape.pure {σ : ISt} : KeepsShape (Pure.pure σ) σ := fun _ e => by cases e; rfl

theorem KeepsShape.readI (σ : ISt) (x : String) : KeepsShape (Fun.readI σ x) σ :=
  fun _ e => by rw [(readI_eq e).1]

theorem KeepsShape.exitI (σ : ISt) : KeepsShape (Fun.exitI σ) σ :=
  fun _ e => by rw [(exitI_eq e).1]

theorem KeepsShape.trans {r : InitM ISt} {σ τ : ISt} (h : KeepsShape r τ) (e : τ.shape = σ.shape) :
    KeepsShape r σ := fun _ h' => (h _ h').trans e

section
variable (sigs : List (String × Sig))

mutual

theorem initE_shape : ∀ (σ : ISt) (e : Expr), KeepsShape (initE sigs σ e) σ
  | σ, .var x => by simp only [initE]; exact KeepsShape.readI σ x
  | σ, .place x steps => by
    simp only [initE]
    exact KeepsShape.bind (KeepsShape.readI σ x) (fun σ1 h1 => (initSteps_shape σ1 steps).trans h1)
  | σ, .hinted _ a => by simp only [initE]; exact initE_shape σ a
  | σ, .adapt a => by simp only [initE]; exact initE_shape σ a
  | σ, .some_ a => by simp only [initE]; exact initE_shape σ a
  | σ, .ok a => by simp only [initE]; exact initE_shape σ a
  | σ, .er a => by simp only [initE]; exact initE_shape σ a
  | σ, .data a => by simp only [initE]; exact initE_shape σ a
  | σ, .error a => by simp only [initE]; exact initE_shape σ a
  | σ, .term _ a => by simp only [initE]; exact initE_shape σ a
  | σ, .enum a => by simp only [initE]; exact initE_shape σ a
  | σ, .binop op a b => by
    simp only [initE]
    refine KeepsShape.bind (initE_shape σ a) (fun σ1 h1 =>
      KeepsShape.bind ((initE_shape σ1 b).trans h1) (fun σ2 h2 => ?_))
    split
    · exact (KeepsShape.exitI σ2).trans h2
    · exact KeepsShape.pure.trans h2
  | σ, .unop op a => by
    simp only [initE]
    refine KeepsShape.bind (initE_shape σ a) (fun σ1 h1 => ?_)
    split
    · exact (KeepsShape.exitI σ1).trans h1
    · exact (KeepsShape.exitI σ1).trans h1
    · exact KeepsShape.pure.trans h1
  | σ, .tryOpt a => by
    simp only [initE]
    exact KeepsShape.bind (initE_shape σ a) (fun σ1 h1 => (KeepsShape.exitI σ1).trans h1)
  | σ, .tryRes a => by
    simp only [initE]
    exact KeepsShape.bind (initE_shape σ a) (fun σ1 h1 => (KeepsShape.exitI σ1).trans h1)
  | σ, .proj b _ => by simp only [initE]; exact initE_shape σ b
  | σ, .index b k (some _) => by
    simp only [initE]
    exact KeepsShape.bind (initE_shape σ b) (fun σ1 h1 =>
      KeepsShape.bind ((initE_shape σ1 k).trans h1) (fun σ2 h2 => (KeepsShape.exitI σ2).trans h2))
  | σ, .index _ _ none => by simp only [initE]; exact KeepsShape.pure
  | σ, .call f args => by
    simp only [initE]
    split
    · refine KeepsShape.bind (initArgs_shape σ args _) (fun σ1 h1 => ?_)
      have give : ∀ (ms : List Mode) (σ2 : ISt), σ2.shape = σ.shape →
          KeepsShape (Pure.pure (giveArgs σ2 args ms) : InitM ISt) σ :=
        fun ms σ2 h2 σ3 e => by cases e; rw [giveArgs_shape]; exact h2
      split
      · exact KeepsShape.bind ((KeepsShape.exitI σ1).trans h1) (give _)
      · exact KeepsShape.bind (KeepsShape.pure.trans h1) (give _)
    · intro _ e; cases e
  | σ, .tuple es => by simp only [initE]; exact initAll_shape σ es
  | σ, .list es => by simp only [initE]; exact initAll_shape σ es
  | σ, .set es => by simp only [initE]; exact initAll_shape σ es
  | σ, .tensor _ es => by simp only [initE]; exact initAll_shape σ es
  | σ, .struct fs => by simp only [initE]; exact initFields_shape σ fs
  | σ, .map es => by simp only [initE]; exact initEntries_shape σ es
  | σ, .table _ rows => by simp only [initE]; exact initRows_shape σ rows
  | σ, .none_ | σ, .tt | σ, .ff | σ, .str _ | σ, .int _ | σ, .float | σ, .hex _ | σ, .atom _ => by
    simp only [initE]; exact KeepsShape.pure

theorem initSteps_shape : ∀ (σ : ISt) (steps : List Step), KeepsShape (initSteps sigs σ steps) σ
  | σ, [] => by simp only [initSteps]; exact KeepsShape.pure
  | σ, .field _ :: rest => by simp only [initSteps]; exact initSteps_shape σ rest
  | σ, .index k m :: rest => by
    simp only [initSteps]
    refine KeepsShape.bind (initE_shape σ k) (fun σ1 h1 => ?_)
    split
    · exact KeepsShape.bind ((KeepsShape.exitI σ1).trans h1) (fun σ2 h2 => (initSteps_shape σ2 rest).trans h2)
    · exact KeepsShape.bind (KeepsShape.pure.trans h1) (fun σ2 h2 => (initSteps_shape σ2 rest).trans h2)

theorem initPlace_shape : ∀ (σ : ISt) (e : Expr) (need : Bool), KeepsShape (initPlace sigs σ e need) σ
  | σ, .var x, need => by
    simp only [initPlace]
    split
    · exact KeepsShape.readI σ x
    · exact KeepsShape.pure
  | σ, .place x steps, _ => by
    simp only [initPlace]
    exact KeepsShape.bind (KeepsShape.readI σ x) (fun σ1 h1 => (initSteps_shape σ1 steps).trans h1)
  | σ, .hinted _ a, need => by simp only [initPlace]; exact initPlace_shape σ a need
  | σ, .proj b _, _ => by simp only [initPlace]; exact initPlace_shape σ b true
  | σ, .index b k (some _), _ => by
    simp only [initPlace]
    exact KeepsShape.bind (initPlace_shape σ b true) (fun σ1 h1 =>
      KeepsShape.bind ((initE_shape σ1 k).trans h1) (fun σ2 h2 => (KeepsShape.exitI σ2).trans h2))
  | σ, .index _ _ none, _ | σ, .none_, _ | σ, .tt, _ | σ, .ff, _ | σ, .str _, _ | σ, .int _, _
  | σ, .float, _ | σ, .hex _, _ | σ, .tuple _, _ | σ, .struct _, _ | σ, .list _, _ | σ, .set _, _
  | σ, .map _, _ | σ, .tensor _ _, _ | σ, .table _ _, _ | σ, .some_ _, _ | σ, .ok _, _ | σ, .er _, _
  | σ, .data _, _ | σ, .error _, _ | σ, .atom _, _ | σ, .term _ _, _ | σ, .enum _, _
  | σ, .binop _ _ _, _ | σ, .unop _ _, _ | σ, .tryOpt _, _ | σ, .tryRes _, _ | σ, .adapt _, _
  | σ, .call _ _, _ => by simp only [initPlace]; exact KeepsShape.pure

theorem initArgs_shape : ∀ (σ : ISt) (args : List (Option Mode × Expr)) (ms : List Mode),
    KeepsShape (initArgs sigs σ args ms) σ
  | σ, (_, e) :: args, m :: ms => by
    simp only [initArgs]
    have rest : ∀ σ1 : ISt, σ1.shape = σ.shape → KeepsShape (initArgs sigs σ1 args ms) σ :=
      fun σ1 h1 => (initArgs_shape σ1 args ms).trans h1
    split
    · split
      · exact KeepsShape.bind (initPlace_shape σ e _) rest
      · exact KeepsShape.bind (initE_shape σ e) rest
    · split
      · exact KeepsShape.bind KeepsShape.pure rest
      · exact KeepsShape.bind (initE_shape σ e) rest
  | σ, [], _ => by simp only [initArgs]; exact KeepsShape.pure
  | σ, _ :: _, [] => by simp only [initArgs]; exact KeepsShape.pure

theorem initAll_shape : ∀ (σ : ISt) (es : List Expr), KeepsShape (initAll sigs σ es) σ
  | σ, [] => by simp only [initAll]; exact KeepsShape.pure
  | σ, e :: es => by
    simp only [initAll]
    exact KeepsShape.bind (initE_shape σ e) (fun σ1 h1 => (initAll_shape σ1 es).trans h1)

theorem initFields_shape : ∀ (σ : ISt) (fs : List (String × Expr)), KeepsShape (initFields sigs σ fs) σ
  | σ, [] => by simp only [initFields]; exact KeepsShape.pure
  | σ, (_, e) :: fs => by
    simp only [initFields]
    exact KeepsShape.bind (initE_shape σ e) (fun σ1 h1 => (initFields_shape σ1 fs).trans h1)

theorem initEntries_shape : ∀ (σ : ISt) (es : List (Expr × Expr)), KeepsShape (initEntries sigs σ es) σ
  | σ, [] => by simp only [initEntries]; exact KeepsShape.pure
  | σ, (k, v) :: es => by
    simp only [initEntries]
    exact KeepsShape.bind (initE_shape σ k) (fun σ1 h1 =>
      KeepsShape.bind ((initE_shape σ1 v).trans h1) (fun σ2 h2 => (initEntries_shape σ2 es).trans h2))

theorem initRows_shape : ∀ (σ : ISt) (rows : List (List Expr)), KeepsShape (initRows sigs σ rows) σ
  | σ, [] => by simp only [initRows]; exact KeepsShape.pure
  | σ, r :: rs => by
    simp only [initRows]
    exact KeepsShape.bind (initAll_shape σ r) (fun σ1 h1 => (initRows_shape σ1 rs).trans h1)

end

end

/-! ## Steps that read no variable

The steps evaluation takes on values alone stop only where any step may:
never at a variable holding nothing, never out of a loop or a body. -/

/-- A step that stops only where any step may. -/
def Calm {α} (m : EvalM α) : Prop :=
  ∀ s, m = .error s → s ≠ .uninit ∧ (∀ ρ, s ≠ .brk ρ) ∧ (∀ ρ, s ≠ .cont ρ) ∧ (∀ o ρ, s ≠ .returned o ρ)

namespace Calm

theorem pure' {α} (a : α) : Calm (Pure.pure a : EvalM α) := fun _ e => by cases e

theorem throw' {α} {s : Stop} (h : s ≠ .uninit ∧ (∀ ρ, s ≠ .brk ρ) ∧ (∀ ρ, s ≠ .cont ρ) ∧ (∀ o ρ, s ≠ .returned o ρ)) :
    Calm (throw s : EvalM α) := fun _ e => by cases e; exact h

theorem wrong {α} : Calm (throw .wrong : EvalM α) := throw' (by simp)
theorem early {α} {r : Val} : Calm (throw (.early r) : EvalM α) := throw' (by simp)
theorem unsupported {α} {g : Gap} : Calm (throw (.unsupported g) : EvalM α) := throw' (by simp)

theorem bind' {α β} {x : EvalM α} {f : α → EvalM β} (hx : Calm x) (hf : ∀ a, Calm (f a)) : Calm (x >>= f) := by
  intro s e
  cases h : x with
  | error s2 => simp only [h, bind, Except.bind] at e; cases e; exact hx _ h
  | ok a => simp only [h, bind, Except.bind] at e; exact hf a s e

theorem map' {α β} {x : EvalM α} (f : α → β) (hx : Calm x) : Calm (f <$> x) := by
  intro s e
  cases h : x with
  | error s2 => simp only [h, Functor.map, Except.map] at e; cases e; exact hx _ h
  | ok a => simp only [h, Functor.map, Except.map] at e; cases e

end Calm

theorem missing_calm {α} (m : IdxMode) : Calm (throw (missing m) : EvalM α) := by
  cases m <;> exact Calm.early

theorem failOf_calm {α} (n : Needs) : Calm (throw (failOf n) : EvalM α) := by
  cases n <;> exact Calm.early

theorem typeOf_calm (m : ChkM Ty) : Calm (typeOf m) := by
  unfold typeOf; split
  · exact Calm.pure' _
  · exact Calm.wrong

theorem fieldVal_calm (v : Val) (s : Sel) : Calm (fieldVal v s) := by
  unfold fieldVal; repeat' split
  all_goals first | exact Calm.pure' _ | exact Calm.wrong

theorem keyEq_calm (a b : Val) : Calm (keyEq a b) := by
  unfold keyEq; split
  · exact Calm.pure' _
  · exact Calm.unsupported

theorem findKey_calm (k : Val) : ∀ es, Calm (findKey k es)
  | [] => Calm.pure' _
  | (_, _) :: es => by
    simp only [findKey]
    exact Calm.bind' (keyEq_calm _ _) (fun b => by
      cases b
      · exact findKey_calm k es
      · exact Calm.pure' _)

theorem binOp_calm (op : BinOp) (t : Ty) (va vb : Val) : Calm (binOp op t va vb) := by
  unfold binOp fallible; repeat' split
  all_goals first | exact Calm.pure' _ | exact Calm.wrong | exact Calm.unsupported | exact failOf_calm _

theorem unOp_calm (op : UnOp) (t : Ty) (v : Val) : Calm (unOp op t v) := by
  unfold unOp; repeat' split
  all_goals first | exact Calm.pure' _ | exact Calm.wrong | exact Calm.early

theorem indexVal_calm (m : IdxMode) (v k : Val) : Calm (indexVal m v k) := by
  have hmap : ∀ es, Calm (findKey k es >>= fun o => match o with
      | some w => pure w
      | none => (throw (missing m) : EvalM Val)) :=
    fun es => Calm.bind' (findKey_calm _ _) (fun o => by
      cases o <;> first | exact missing_calm m | exact Calm.pure' _)
  unfold indexVal; repeat' split
  all_goals first | exact Calm.pure' _ | exact Calm.wrong | exact missing_calm m | exact hmap _

theorem putField_calm (v : Val) (s : Sel) (w : Val) : Calm (putField v s w) := by
  unfold putField; repeat' split
  all_goals first | exact Calm.pure' _ | exact Calm.wrong

theorem replaceKey_calm (k w : Val) : ∀ es, Calm (replaceKey k w es)
  | [] => Calm.pure' _
  | (_, _) :: es => by
    simp only [replaceKey]
    exact Calm.bind' (keyEq_calm _ _) (fun b => by
      cases b
      · exact Calm.map' _ (replaceKey_calm k w es)
      · exact Calm.pure' _)

theorem putIndex_calm (v k w : Val) : Calm (putIndex v k w) := by
  unfold putIndex; repeat' split
  all_goals first | exact Calm.pure' _ | exact Calm.wrong | exact Calm.map' _ (replaceKey_calm _ _ _)

theorem insertKey_calm (k w : Val) (es : List (Val × Val)) : Calm (insertKey k w es) := by
  unfold insertKey
  exact Calm.bind' (findKey_calm _ _) (fun o => by
    cases o
    · exact Calm.pure' _
    · exact replaceKey_calm _ _ _)

/-- Writing through a variable that holds a value reads nothing else. -/
theorem writeSteps_calm (op : Option BinOp) : ∀ (t : Ty) (v : Val) (steps : List Step) (kvs : List Val) (r : Val),
    Calm (writeSteps op t (some v) steps kvs r)
  | t, v, [], kvs, r => by
    unfold writeSteps
    split
    · exact Calm.pure' _
    · split
      · exact binOp_calm _ _ _ _
      · simp_all
  | t, v, [.index _ none], [kv], r => by
    unfold writeSteps
    split
    · exact Calm.map' _ (insertKey_calm _ _ _)
    · exact Calm.wrong
    · simp_all
  | t, v, .field s :: rest, kvs, r => by
    unfold writeSteps
    split
    · exact Calm.bind' (fieldVal_calm _ _) (fun part =>
        Calm.bind' (writeSteps_calm op _ part rest kvs r) (fun w => putField_calm _ _ _))
    · simp_all
    · exact Calm.wrong
  | t, v, .index _ (some m) :: rest, kv :: kvs, r => by
    unfold writeSteps
    split
    · exact Calm.bind' (indexVal_calm _ _ _) (fun part =>
        Calm.bind' (writeSteps_calm op _ part rest kvs r) (fun w => putIndex_calm _ _ _))
    · simp_all
    · exact Calm.wrong
  | t, v, [.index _ none], [], r => by unfold writeSteps; exact Calm.wrong
  | t, v, [.index _ none], _ :: _ :: _, r => by unfold writeSteps; exact Calm.wrong
  | t, v, .index _ none :: _ :: _, kvs, r => by unfold writeSteps; exact Calm.wrong
  | t, v, .index _ (some _) :: _, [], r => by unfold writeSteps; exact Calm.wrong

/-! ## What a run may do where the analysis let it through -/

/-- A shape whose outermost part is `O`, a function's parameters and the
consts it sees, and whose innermost part, bound inside the body, has no
`out` parameter. -/
def Fr (O : List (String × Bool)) (σ : ISt) : Prop :=
  ∃ δ : List (String × Bool), (∀ p ∈ δ, p.2 = false) ∧ σ.shape = δ ++ O

theorem Fr.of_shape {O : List (String × Bool)} {σ σ' : ISt} (h : Fr O σ) (e : σ'.shape = σ.shape) : Fr O σ' := by
  obtain ⟨δ, hδ, hs⟩ := h
  exact ⟨δ, hδ, e.trans hs⟩

/-- Where the run leaves the function, every `out` parameter holds a value:
the analysis said so of variables the run's line up with. -/
def ExitOk (O : List (String × Bool)) (ρ : VEnv) : Prop :=
  ∃ σ, Fr O σ ∧ IAg σ ρ ∧ σ.outsSet = true

/-- How a run the analysis let through may stop: never at a variable that
holds nothing, never out of a loop or a body, and returning only with every
`out` parameter holding a value. -/
def IStop (O : List (String × Bool)) : Stop → Prop
  | .uninit | .early _ | .brk _ | .cont _ => False
  | .returned _ ρ => ExitOk O ρ
  | _ => True

/-- What a run from `ρ` came to: a value meeting `P` and variables that keep
every value `ρ` held and line up with `σ'`; or a stop as `IStop` says. -/
def Res (O : List (String × Bool)) (ρ : VEnv) (σ' : ISt) {α} (P : α → VEnv → Prop) :
    Except Stop (α × VEnv) → Prop
  | .ok (a, ρ') => Keeps ρ ρ' ∧ IAg σ' ρ' ∧ P a ρ'
  | .error s => IStop O s

/-- Evaluating `m` from variables that line up with `σ`, in a body whose
frame is `O`, comes to what `Res` says, `σ'` what the analysis has after. -/
def IT (O : List (String × Bool)) (σ σ' : ISt) {α} (P : α → VEnv → Prop) (m : EvalT α) : Prop :=
  Fr O σ → ∀ ρ, IAg σ ρ → Res O ρ σ' P (m.run ρ)

namespace IT

variable {O : List (String × Bool)} {σ σ1 σ2 : ISt} {α β : Type} {P : α → VEnv → Prop} {Q : β → VEnv → Prop}

theorem pure' {a : α} (h : ∀ ρ, IAg σ ρ → IAg σ1 ρ ∧ P a ρ) : IT O σ σ1 P (Pure.pure a) :=
  fun _ ρ hρ => ⟨Keeps.refl ρ, h ρ hρ⟩

theorem res_bind {ρ : VEnv} {x : EvalT α} {f : α → EvalT β} (hx : Res O ρ σ1 P (x.run ρ))
    (hf : ∀ a ρ1, P a ρ1 → IAg σ1 ρ1 → Res O ρ1 σ2 Q ((f a).run ρ1)) : Res O ρ σ2 Q ((x >>= f).run ρ) := by
  simp only [StateT.run_bind]
  generalize x.run ρ = r at hx ⊢
  cases r with
  | error s => exact hx
  | ok p =>
    obtain ⟨a, ρ1⟩ := p
    obtain ⟨k1, h1, hp⟩ := hx
    have h2 := hf a ρ1 hp h1
    simp only [bind, Except.bind]
    generalize (f a).run ρ1 = r2 at h2 ⊢
    cases r2 with
    | error s => exact h2
    | ok q => obtain ⟨b, ρ2⟩ := q; exact ⟨k1.trans h2.1, h2.2⟩

theorem res_bindK {ρ : VEnv} {x : EvalT α} {f : α → EvalT β} (hx : Res O ρ σ1 P (x.run ρ))
    (hf : ∀ a ρ1, Keeps ρ ρ1 → P a ρ1 → IAg σ1 ρ1 → Res O ρ1 σ2 Q ((f a).run ρ1)) :
    Res O ρ σ2 Q ((x >>= f).run ρ) := by
  simp only [StateT.run_bind]
  generalize x.run ρ = r at hx ⊢
  cases r with
  | error s => exact hx
  | ok p =>
    obtain ⟨a, ρ1⟩ := p
    obtain ⟨k1, h1, hp⟩ := hx
    have h2 := hf a ρ1 k1 hp h1
    simp only [bind, Except.bind]
    generalize (f a).run ρ1 = r2 at h2 ⊢
    cases r2 with
    | error s => exact h2
    | ok q => obtain ⟨b, ρ2⟩ := q; exact ⟨k1.trans h2.1, h2.2⟩

theorem bind' {x : EvalT α} {f : α → EvalT β} (hx : IT O σ σ1 P x) (hs : σ1.shape = σ.shape)
    (hf : ∀ a ρ1, P a ρ1 → Fr O σ1 → IAg σ1 ρ1 → Res O ρ1 σ2 Q ((f a).run ρ1)) : IT O σ σ2 Q (x >>= f) :=
  fun hfr ρ hρ => res_bind (hx hfr ρ hρ) (fun a ρ1 hp h1 => hf a ρ1 hp (hfr.of_shape hs) h1)

theorem bind {x : EvalT α} {f : α → EvalT β} (hx : IT O σ σ1 P x) (hs : σ1.shape = σ.shape)
    (hf : ∀ a, IT O σ1 σ2 Q (f a)) : IT O σ σ2 Q (x >>= f) :=
  bind' hx hs (fun a ρ1 _ hfr h1 => hf a hfr ρ1 h1)

theorem throw' {s : Stop} (h : IStop O s) : IT O σ σ1 P (throw s) := fun _ _ _ => h

theorem wrong : IT O σ σ1 P (throw .wrong) := throw' trivial
theorem fuel : IT O σ σ1 P (throw .fuel) := throw' trivial
theorem unsupported {g : Gap} : IT O σ σ1 P (throw (.unsupported g)) := throw' trivial

theorem get' {f : VEnv → EvalT α} (h : ∀ ρ, Fr O σ → IAg σ ρ → Res O ρ σ1 P ((f ρ).run ρ)) :
    IT O σ σ1 P (get >>= f) := by
  intro hfr ρ hρ
  simp only [StateT.run_bind, StateT.run_get, pure_bind]
  exact h ρ hfr hρ

theorem weaken {m : EvalT α} {P' : α → VEnv → Prop} (h : IT O σ σ1 P m) (hp : ∀ a ρ, P a ρ → P' a ρ) :
    IT O σ σ1 P' m := by
  intro hfr ρ hρ
  have h := h hfr ρ hρ
  generalize m.run ρ = r at h ⊢
  cases r with
  | error s => exact h
  | ok q => obtain ⟨a, ρ'⟩ := q; exact ⟨h.1, h.2.1, hp a ρ' h.2.2⟩

theorem map' {m : EvalT α} {f : α → β} (h : IT O σ σ1 P m) (hp : ∀ a ρ, P a ρ → Q (f a) ρ) :
    IT O σ σ1 Q (f <$> m) := by
  intro hfr ρ hρ
  have h := h hfr ρ hρ
  simp only [StateT.run_map]
  generalize m.run ρ = r at h ⊢
  cases r with
  | error s => exact h
  | ok q => obtain ⟨a, ρ'⟩ := q; exact ⟨h.1, h.2.1, hp a ρ' h.2.2⟩

/-- A step that reads no variable: it stops only where any step may, or
returns early where the analysis said the way out was open. -/
theorem here {m : EvalM α} (hc : Calm m) (he : ∀ r, m = .error (.early r) → σ.outsSet = true)
    (hp : ∀ a, m = .ok a → ∀ ρ, IAg σ ρ → P a ρ) : IT O σ σ P (Fun.here m) := by
  intro hfr ρ hρ
  cases hm : m with
  | ok a => simp only [Fun.here]; exact ⟨Keeps.refl ρ, hρ, hp a hm ρ hρ⟩
  | error s =>
    have hcs := hc s hm
    cases s with
    | early r =>
      simp only [Fun.here, Fun.earlyHere, StateT.run_bind, StateT.run_get, pure_bind]
      exact ⟨σ, hfr, hρ, he r hm⟩
    | uninit => exact absurd rfl hcs.1
    | brk ρ' => exact absurd rfl (hcs.2.1 ρ')
    | cont ρ' => exact absurd rfl (hcs.2.2.1 ρ')
    | returned o ρ' => exact absurd rfl (hcs.2.2.2 o ρ')
    | _ => exact trivial

/-- A step that reads no variable, run from `ρ`: what it gives, the variables
as they were. -/
theorem res_here {ρ : VEnv} {m : EvalM α} (hc : Calm m) (he : ∀ r, m = .error (.early r) → σ.outsSet = true)
    (hfr : Fr O σ) (hρ : IAg σ ρ) : Res O ρ σ (fun a ρ' => m = .ok a ∧ ρ' = ρ) ((Fun.here m).run ρ) := by
  cases hm : m with
  | ok a => simp only [Fun.here]; exact ⟨Keeps.refl ρ, hρ, rfl, rfl⟩
  | error s =>
    have hcs := hc s hm
    cases s with
    | early r =>
      simp only [Fun.here, Fun.earlyHere, StateT.run_bind, StateT.run_get, pure_bind]
      exact ⟨σ, hfr, hρ, he r hm⟩
    | uninit => exact absurd rfl hcs.1
    | brk ρ' => exact absurd rfl (hcs.2.1 ρ')
    | cont ρ' => exact absurd rfl (hcs.2.2.1 ρ')
    | returned o ρ' => exact absurd rfl (hcs.2.2.2 o ρ')
    | _ => exact trivial

theorem res_get {ρ : VEnv} {f : VEnv → EvalT α} (h : Res O ρ σ1 P ((f ρ).run ρ)) :
    Res O ρ σ1 P ((get >>= f).run ρ) := by
  simp only [StateT.run_bind, StateT.run_get, pure_bind]; exact h

theorem res_set_pure {ρ ρ' : VEnv} {a : α} (k : Keeps ρ ρ') (h : IAg σ1 ρ') (hp : P a ρ') :
    Res O ρ σ1 P ((set ρ' >>= fun _ => (Pure.pure a : EvalT α)).run ρ) := by
  simp only [StateT.run_bind, StateT.run_set, StateT.run_pure, pure_bind]
  exact ⟨k, h, hp⟩

theorem earlyHere {r : Val} (h : σ.outsSet = true) : IT O σ σ1 P (Fun.earlyHere r) := by
  intro hfr ρ hρ
  simp only [Fun.earlyHere, StateT.run_bind, StateT.run_get, pure_bind]
  exact ⟨σ, hfr, hρ, h⟩

end IT

/-! ## Calls and writing back -/

/-- Calls that keep the promise: given a value for every parameter but an
`out` one, a call stops only where any step may, and returns a value for
every `mut` and `out` parameter. -/
def CallInit (sigs : List (String × Sig)) (call : CallFn) : Prop :=
  ∀ f sig θ vals, sigs.lookup f = some sig →
    (∀ q ∈ vals.zip sig.params, q.2.1 ≠ .out → q.1.isSome = true) →
    match call f θ vals with
    | .ok (_, after) => vals.length = sig.params.length ∧ after.length = sig.params.length ∧
        ∀ q ∈ after.zip sig.params, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true
    | .error s => s ≠ .uninit ∧ (∀ r, s ≠ .early r) ∧ (∀ ρ, s ≠ .brk ρ) ∧ (∀ ρ, s ≠ .cont ρ) ∧
        (∀ o ρ, s ≠ .returned o ρ)

/-- The variable of each `mut` or `out` argument written back through
steps holds a value, for writing back into. -/
def Roots (ρ : VEnv) : List (Option Mode × Expr) → List (Mode × Ty) → Prop
  | (_, e) :: args, (m, _) :: ps =>
    ((m == .mut || m == .out) = true → ∀ x st, placeOf e = some (x, st) → st ≠ [] → ρ.lookup x ≠ some none) ∧
      Roots ρ args ps
  | _, _ => True

theorem Roots.keeps {ρ ρ' : VEnv} (k : Keeps ρ ρ') : ∀ {args : List (Option Mode × Expr)} {ps : List (Mode × Ty)},
    Roots ρ args ps → Roots ρ' args ps
  | _ :: _, _ :: _, ⟨h1, h2⟩ =>
    ⟨fun hm x st hp hst => k.lookup (h1 hm x st hp hst), Roots.keeps k h2⟩
  | [], _, _ => trivial
  | _ :: _, [], _ => trivial

theorem bind_ok {ε α β} {x : Except ε α} {f : α → Except ε β} {b : β} (h : (x >>= f) = .ok b) :
    ∃ a, x = .ok a ∧ f a = .ok b := by
  cases hx : x with
  | error c => rw [hx] at h; cases h
  | ok a => rw [hx] at h; exact ⟨a, rfl, h⟩

/-- A step that never returns early. -/
def NoEarly {α} (m : EvalM α) : Prop := ∀ r, m ≠ .error (.early r)

namespace NoEarly

theorem pure' {α} (a : α) : NoEarly (Pure.pure a : EvalM α) := fun _ e => by cases e
theorem wrong {α} : NoEarly (throw .wrong : EvalM α) := fun _ e => by cases e
theorem unsupported {α} {g : Gap} : NoEarly (throw (.unsupported g) : EvalM α) := fun _ e => by cases e

theorem bind' {α β} {x : EvalM α} {f : α → EvalM β} (hx : NoEarly x) (hf : ∀ a, NoEarly (f a)) :
    NoEarly (x >>= f) := by
  intro r e
  cases h : x with
  | error s => simp only [h, bind, Except.bind] at e; cases e; exact hx r h
  | ok a => simp only [h, bind, Except.bind] at e; exact hf a r e

theorem map' {α β} {x : EvalM α} (f : α → β) (hx : NoEarly x) : NoEarly (f <$> x) := by
  intro r e
  cases h : x with
  | error s => simp only [h, Functor.map, Except.map] at e; cases e; exact hx r h
  | ok a => simp only [h, Functor.map, Except.map] at e; cases e

end NoEarly

theorem keyEq_noEarly (a b : Val) : NoEarly (keyEq a b) := by
  unfold keyEq; split
  · exact NoEarly.pure' _
  · exact NoEarly.unsupported

theorem findKey_noEarly (k : Val) : ∀ es, NoEarly (findKey k es)
  | [] => NoEarly.pure' _
  | (_, _) :: es => by
    simp only [findKey]
    exact NoEarly.bind' (keyEq_noEarly _ _) (fun b => by
      cases b
      · exact findKey_noEarly k es
      · exact NoEarly.pure' _)

theorem replaceKey_noEarly (k w : Val) : ∀ es, NoEarly (replaceKey k w es)
  | [] => NoEarly.pure' _
  | (_, _) :: es => by
    simp only [replaceKey]
    exact NoEarly.bind' (keyEq_noEarly _ _) (fun b => by
      cases b
      · exact NoEarly.map' _ (replaceKey_noEarly k w es)
      · exact NoEarly.pure' _)

theorem insertKey_noEarly (k w : Val) (es : List (Val × Val)) : NoEarly (insertKey k w es) := by
  unfold insertKey
  exact NoEarly.bind' (findKey_noEarly _ _) (fun o => by
    cases o
    · exact NoEarly.pure' _
    · exact replaceKey_noEarly _ _ _)

theorem fieldVal_noEarly (v : Val) (s : Sel) : NoEarly (fieldVal v s) := by
  unfold fieldVal; repeat' split
  all_goals first | exact NoEarly.pure' _ | exact NoEarly.wrong

theorem putField_noEarly (v : Val) (s : Sel) (w : Val) : NoEarly (putField v s w) := by
  unfold putField; repeat' split
  all_goals first | exact NoEarly.pure' _ | exact NoEarly.wrong

theorem putIndex_noEarly (v k w : Val) : NoEarly (putIndex v k w) := by
  unfold putIndex; repeat' split
  all_goals first | exact NoEarly.pure' _ | exact NoEarly.wrong | exact NoEarly.map' _ (replaceKey_noEarly _ _ _)

theorem typeOf_noEarly (m : ChkM Ty) : NoEarly (typeOf m) := by
  unfold typeOf; split
  · exact NoEarly.pure' _
  · exact NoEarly.wrong

/-- An operator that cannot fail does not return early. -/
theorem binOp_noEarly {op : BinOp} (h : op.exits = false) (t : Ty) (va vb : Val) : NoEarly (binOp op t va vb) := by
  cases op
  all_goals try (simp [BinOp.exits, needs] at h; done)
  all_goals unfold binOp; repeat' split
  all_goals first | exact NoEarly.pure' _ | exact NoEarly.wrong | exact NoEarly.unsupported | simp_all

/-- An operator other than `-!` and `-?` does not return early. -/
theorem unOp_noEarly {op : UnOp} (h : op ≠ .negC ∧ op ≠ .negO) (t : Ty) (v : Val) : NoEarly (unOp op t v) := by
  unfold unOp; repeat' split
  all_goals first | exact NoEarly.pure' _ | exact NoEarly.wrong | (exfalso; simp_all)

theorem EvalM.bind_early {α β} {x : EvalM α} {f : α → EvalM β} {r : Val} (h : (x >>= f) = .error (.early r)) :
    x = .error (.early r) ∨ ∃ a, x = .ok a ∧ f a = .error (.early r) := by
  cases hx : x with
  | error s => rw [hx] at h; cases h; exact .inl rfl
  | ok a => rw [hx] at h; exact .inr ⟨a, rfl, h⟩

/-- Writing through steps returns early only at an index with `?` or `!`. -/
theorem writeSteps_early : ∀ (t : Ty) (v : Val) (steps : List Step) (kvs : List Val) (r w : Val),
    writeSteps none t (some v) steps kvs r = .error (.early w) → stepsExit steps = true
  | t, v, [], kvs, r, w, h => by simp [writeSteps, pure, Except.pure] at h
  | t, v, [.index _ none], [kv], r, w, h => by
    unfold writeSteps at h
    split at h
    · exact absurd h (NoEarly.map' _ (insertKey_noEarly _ _ _) w)
    · simp [throw, throwThe, MonadExceptOf.throw] at h
    · simp_all
  | t, v, .field s :: rest, kvs, r, w, h => by
    unfold writeSteps at h
    split at h
    · rename_i ft hft
      rcases EvalM.bind_early h with h | ⟨part, hp, h⟩
      · exact absurd h (fieldVal_noEarly _ s w)
      rcases EvalM.bind_early h with h | ⟨w', hw, h⟩
      · have := writeSteps_early _ part rest kvs r w h
        simp only [stepsExit, List.any_cons] at this ⊢
        simpa using this
      · exact absurd h (putField_noEarly _ s w' w)
    · simp_all
    · simp [throw, throwThe, MonadExceptOf.throw] at h
  | t, v, .index _ (some m) :: rest, kv :: kvs, r, w, h => by simp [stepsExit]
  | t, v, [.index _ none], [], r, w, h => by simp [writeSteps, throw, throwThe, MonadExceptOf.throw] at h
  | t, v, [.index _ none], _ :: _ :: _, r, w, h => by simp [writeSteps, throw, throwThe, MonadExceptOf.throw] at h
  | t, v, .index _ none :: _ :: _, kvs, r, w, h => by simp [writeSteps, throw, throwThe, MonadExceptOf.throw] at h
  | t, v, .index _ (some _) :: _, [], r, w, h => by simp [stepsExit]

/-- A stop where any step may stop. -/
def CalmStop (s : Stop) : Prop :=
  s ≠ .uninit ∧ (∀ ρ, s ≠ .brk ρ) ∧ (∀ ρ, s ≠ .cont ρ) ∧ (∀ o ρ, s ≠ .returned o ρ)

/-- Writing back a call's `mut` and `out` arguments, where each variable
written through steps holds a value and the call gave every such parameter
one, gives each argument's variable a value, as the analysis has it, and
returns early only through an index with `?` or `!`. -/
theorem writeBack_init (Γ : Ctx) : ∀ (ρ : VEnv) (σ : ISt) (args : List (Option Mode × Expr)) (ps : List (Mode × Ty))
    (kvss : List (List Val)) (after : List (Option Val)),
    IAg σ ρ → Roots ρ args ps → (∀ q ∈ after.zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true) →
    after.length = ps.length → kvss.length = ps.length → ps.length ≤ args.length →
    match writeBack Γ ρ args ps kvss after with
    | .ok ρ' => Keeps ρ ρ' ∧ IAg (giveArgs σ args (ps.map (·.1))) ρ'
    | .error s => CalmStop s ∧ (∀ r, s = .early r → argsExit args (ps.map (·.1)) = true)
  | ρ, σ, (mk, e) :: args, (mode, t) :: ps, kvs :: kvss, a :: after, hρ, hr, ha, hla, hlk, hle => by
    obtain ⟨hr1, hr2⟩ := hr
    have ha' : ∀ q ∈ after.zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true :=
      fun q hq => ha q (List.mem_cons_of_mem _ hq)
    have hla' : after.length = ps.length := by simpa using hla
    have hlk' : kvss.length = ps.length := by simpa using hlk
    have hle' : ps.length ≤ args.length := by simpa using hle
    -- What is left once this argument is written back, from `ρ1`, with `σ1`.
    have rest : ∀ ρ1 σ1, Keeps ρ ρ1 → IAg σ1 ρ1 →
        (match writeBack Γ ρ1 args ps kvss after with
          | .ok ρ' => Keeps ρ1 ρ' ∧ IAg (giveArgs σ1 args (ps.map (·.1))) ρ'
          | .error s => CalmStop s ∧ (∀ r, s = .early r → argsExit args (ps.map (·.1)) = true)) →
        match writeBack Γ ρ1 args ps kvss after with
          | .ok ρ' => Keeps ρ ρ' ∧ IAg (giveArgs σ1 args (ps.map (·.1))) ρ'
          | .error s => CalmStop s ∧ (∀ r, s = .early r → (((mode == .mut || mode == .out) &&
              match placeOf e with
              | some (_, steps) => stepsExit steps
              | none => false) || argsExit args (ps.map (·.1))) = true) := by
      intro ρ1 σ1 k1 h1 ih
      generalize writeBack Γ ρ1 args ps kvss after = r at ih ⊢
      cases r with
      | ok ρ' => exact ⟨k1.trans ih.1, ih.2⟩
      | error s => exact ⟨ih.1, fun r hs => by simp [ih.2 r hs]⟩
    simp only [writeBack, giveArgs, argsExit, List.map_cons]
    by_cases hm' : mode = .mut ∨ mode = .out
    · have hm : (mode == .mut || mode == .out) = true := by simpa [Bool.or_eq_true, beq_iff_eq] using hm'
      have ha1 : a.isSome = true := ha (a, (mode, t)) List.mem_cons_self hm'
      simp only [hm, ↓reduceIte, Bool.true_and]
      cases hp : placeOf e with
      | none =>
        simp only [Bool.false_or]
        have := writeBack_init Γ ρ σ args ps kvss after hρ hr2 ha' hla' hlk' hle'
        generalize writeBack Γ ρ args ps kvss after = r at this ⊢
        cases r with
        | ok ρ' => exact this
        | error s => exact this
      | some p =>
        obtain ⟨x, st⟩ := p
        cases a with
        | none => simp at ha1
        | some v =>
          cases st with
          | nil =>
            simp only
            have k1 := setVar_keeps x v ρ
            have := rest _ _ k1 (IAg.give x v hρ)
              (writeBack_init Γ _ _ args ps kvss after (IAg.give x v hρ) (hr2.keeps k1) ha' hla' hlk' hle')
            generalize writeBack Γ (setVar x v ρ) args ps kvss after = r at this ⊢
            cases r with
            | ok ρ' => exact this
            | error s => exact ⟨this.1, fun r hs => by simpa [hp, stepsExit] using this.2 r hs⟩
          | cons s0 st' =>
            simp only
            cases hl : Γ.lookup x with
            | none => simp [CalmStop, throw, throwThe, MonadExceptOf.throw]
            | some uk =>
              obtain ⟨u, k⟩ := uk
              cases hl' : ρ.lookup x with
              | none => simp [CalmStop, throw, throwThe, MonadExceptOf.throw]
              | some old =>
                simp only
                have hold : old ≠ none := by
                  intro e0; subst e0
                  exact hr1 hm x (s0 :: st') hp (by simp) hl'
                obtain ⟨ov, rfl⟩ := Option.ne_none_iff_exists'.mp hold
                have hc := writeSteps_calm none u ov (s0 :: st') kvs v
                cases hws : writeSteps none u (some ov) (s0 :: st') kvs v with
                | error s =>
                  simp only [bind, Except.bind]
                  refine ⟨hc s hws, fun r hs => ?_⟩
                  subst hs
                  simp [writeSteps_early _ _ _ _ _ _ hws]
                | ok w =>
                  simp only [bind, Except.bind]
                  have k1 := setVar_keeps x w ρ
                  have := rest _ _ k1 (IAg.give x w hρ)
                    (writeBack_init Γ _ _ args ps kvss after (IAg.give x w hρ) (hr2.keeps k1) ha' hla' hlk' hle')
                  generalize writeBack Γ (setVar x w ρ) args ps kvss after = r at this ⊢
                  cases r with
                  | ok ρ' => exact this
                  | error s => exact ⟨this.1, fun r hs => by simpa [hm', hp] using this.2 r hs⟩
    · have hm : (mode == .mut || mode == .out) = false := by simpa [Bool.or_eq_true, beq_iff_eq] using hm'
      simp only [hm, Bool.false_and, Bool.false_or, Bool.false_eq_true, ↓reduceIte]
      exact writeBack_init Γ ρ σ args ps kvss after hρ hr2 ha' hla' hlk' hle'
  | ρ, σ, [], ps, kvss, after, hρ, _, _, _, _, hle => by
    have : ps = [] := by simpa using hle
    subst this
    simp only [writeBack, giveArgs]
    exact ⟨Keeps.refl ρ, hρ⟩
  | ρ, σ, _ :: _, [], kvss, after, hρ, _, _, _, _, _ => by
    simp only [writeBack, giveArgs, List.map_nil]
    exact ⟨Keeps.refl ρ, hρ⟩
  | ρ, σ, _ :: _, _ :: _, [], after, _, _, _, _, hlk, _ => by simp at hlk
  | ρ, σ, _ :: _, _ :: _, _ :: _, [], _, _, _, hla, _, _ => by simp at hla

theorem sigs_enter (Γ : Ctx) (e : Expr) : (Γ.enter e).sigs = Γ.sigs := rfl
theorem sigs_borrow (Γ : Ctx) : Γ.borrow.sigs = Γ.sigs := rfl
theorem sigs_forArg (Γ : Ctx) (m : Mode) : (Γ.forArg m).sigs = Γ.sigs := rfl
theorem sigs_through (Γ : Ctx) : Γ.through.sigs = Γ.sigs := rfl
theorem sigs_under (Γ : Ctx) (b : Expr) : (Γ.under b).sigs = Γ.sigs := by unfold Ctx.under; split <;> rfl

/-- What a `mut` or `out` argument's place gives: a value, where `need`
asks for one, and its variable holding a value where the argument is a part
of it, or `need` asks. -/
def IPlace (need : Bool) (e : Expr) (o : Option Val) (ρ : VEnv) : Prop :=
  (need = true → o.isSome = true) ∧
    ∀ x st, placeOf e = some (x, st) → (need = true ∨ st ≠ []) → ρ.lookup x ≠ some none

/-- What a call's arguments give: one for each parameter there is an
argument for, each a value but for an `out` parameter, and every variable
written back through steps holding a value. -/
def IArgs (args : List (Option Mode × Expr)) (ps : List (Mode × Ty)) (vs : List (Option Val × List Val))
    (ρ : VEnv) : Prop :=
  vs.length = min args.length ps.length ∧ (∀ q ∈ (vs.map (·.1)).zip ps, q.2.1 ≠ .out → q.1.isSome = true) ∧
    Roots ρ args ps

theorem initSteps_cons (sigs : List (String × Sig)) (σ : ISt) (st : Step) (rest : List Step) :
    initSteps sigs σ (st :: rest) = (initSteps sigs σ [st] >>= fun σ1 => initSteps sigs σ1 rest) := by
  cases st with
  | field s => simp [initSteps]
  | index k m => cases m <;> simp [initSteps]

theorem bind_pure_ok {α} {x : InitM α} {a : α} (h : (x >>= fun b => (pure b : InitM α)) = .ok a) : x = .ok a := by
  simpa using h

/-! ## Expressions -/

/-- An argument read by value, for any parameter but an `out` one: a value. -/
theorem argByValue_some {O : List (String × Bool)} {σ σ1 : ISt} {mode : Mode} {v : EvalT Val} (ho : mode ≠ .out)
    (hv : IT O σ σ1 (fun _ _ => True) v) (hs : σ1.shape = σ.shape) :
    IT O σ σ1 (fun a _ => a.1.isSome = true) (argByValue mode v) := by
  unfold argByValue
  simp only [show (mode == .out) = false by simpa using ho, Bool.false_eq_true, ↓reduceIte]
  exact IT.bind' (IT.map' (Q := fun o _ => o.isSome = true) hv (fun _ _ _ => rfl)) hs
    (fun o ρ1 hos hfr1 hρ1 => (IT.pure' (fun ρ hρ => ⟨hρ, hos⟩)) hfr1 ρ1 hρ1)

/-- A step evaluation takes only on the value it has in hand, with no way
out of the function, that gives its value whatever it is. -/
theorem IT.calm {O : List (String × Bool)} {α} {σ : ISt} {m : EvalM α} (hc : Calm m) (he : NoEarly m) :
    IT O σ σ (fun _ _ => True) (Fun.here m) :=
  IT.here hc (fun r hr => absurd hr (he r)) (fun _ _ _ _ => trivial)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallInit sigs call)
  (O : List (String × Bool)) (hmv : env.moves = false)
include hcall hmv
set_option linter.unusedSectionVars false

mutual

theorem evalS_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initE sigs σ e = .ok σ' → IT O σ σ' (fun _ _ => True) (evalS env call n Γ e)
  | 0, _, _, _, _, _, _ => by simp only [evalS]; exact IT.fuel
  | n + 1, Γ, e, σ, σ', hs, h => by
    have h0 := h
    cases e
    case hinted hh inner =>
      simp only [initE] at h
      simp only [evalS]
      split
      · simp only [pure_bind]
        exact evalForm_init n _ inner _ σ σ' hs h
      · exact IT.wrong
    case var x =>
      simp only [initE] at h
      obtain ⟨rfl, hx⟩ := readI_eq h
      simp only [evalS]
      refine IT.get' (fun ρ hfr hρ => ?_)
      have hl := IAg.lookup hρ hx
      split
      · simp only [hmv, Bool.false_and, Bool.false_eq_true, ↓reduceIte]
        exact IT.pure' (fun ρ' h' => ⟨h', trivial⟩) hfr ρ hρ
      · rename_i hn; exact absurd hn hl
      · exact IT.wrong hfr ρ hρ
    case binop op a b =>
      simp only [initE] at h
      obtain ⟨σ1, h1, h⟩ := bind_ok h
      obtain ⟨σ2, h2, h⟩ := bind_ok h
      have hs1 := initE_shape sigs σ a σ1 h1
      have hs2 := initE_shape sigs σ1 b σ2 h2
      simp only [evalS]
      split at h
      · rename_i hx
        obtain ⟨rfl, ho⟩ := exitI_eq h
        exact IT.bind (evalPair_init n _ a b none σ σ1 _ hs h1 h2) (hs2.trans hs1) (fun r =>
          IT.here (binOp_calm _ _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial))
      · rename_i hx
        simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
        exact IT.bind (evalPair_init n _ a b none σ σ1 _ hs h1 h2) (hs2.trans hs1) (fun r =>
          IT.calm (binOp_calm _ _ _ _) (binOp_noEarly (by simpa using hx) _ _ _))
    case unop op a =>
      simp only [initE] at h
      obtain ⟨σ1, h1, h⟩ := bind_ok h
      have hs1 := initE_shape sigs σ a σ1 h1
      simp only [evalS]
      refine IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun t => ?_)
      refine IT.bind (evalS_init n _ a σ σ1 hs h1) hs1 (fun v => ?_)
      cases op
      case negC | negO =>
        obtain ⟨rfl, ho⟩ := exitI_eq h
        exact IT.here (unOp_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial)
      all_goals
        simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
        refine IT.calm (unOp_calm _ _ _) (unOp_noEarly ?_ _ _)
        simp
    case tryOpt a =>
      simp only [initE] at h
      obtain ⟨σ1, h1, h⟩ := bind_ok h
      obtain ⟨rfl, ho⟩ := exitI_eq h
      simp only [evalS]
      refine IT.bind (evalS_init n _ a σ σ' hs h1) (initE_shape sigs σ a σ' h1) (fun v => ?_)
      split
      · exact IT.earlyHere ho
      · exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
      · exact IT.wrong
    case tryRes a =>
      simp only [initE] at h
      obtain ⟨σ1, h1, h⟩ := bind_ok h
      obtain ⟨rfl, ho⟩ := exitI_eq h
      simp only [evalS]
      refine IT.bind (evalS_init n _ a σ σ' hs h1) (initE_shape sigs σ a σ' h1) (fun v => ?_)
      split
      · exact IT.earlyHere ho
      · exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
      · exact IT.wrong
    case adapt a =>
      simp only [initE] at h
      simp only [evalS]
      exact evalS_init n _ a σ σ' hs h
    case call f args =>
      simp only [evalS]
      exact evalCall_init n _ f args none σ σ' hs h
    case place x steps =>
      simp only [initE] at h
      obtain ⟨σ1, h1, hst⟩ := bind_ok h
      obtain ⟨rfl, hx⟩ := readI_eq h1
      simp only [evalS]
      refine IT.get' (fun ρ hfr hρ => ?_)
      have hl := IAg.lookup hρ hx
      split
      · exact evalSteps_init n (Γ.enter _) _ _ steps _ _ hs hst hfr ρ hρ
      · rename_i hn; exact absurd hn hl
      · exact IT.wrong hfr ρ hρ
    case proj b sel =>
      simp only [initE] at h
      simp only [evalS]
      exact IT.bind (evalS_init n _ b σ σ' (hs.of_eq (sigs_under _ b)) h) (initE_shape sigs σ b σ' h) (fun v =>
        IT.calm (fieldVal_calm _ _) (fieldVal_noEarly _ _))
    case index b k mode =>
      cases mode with
      | none => simp only [evalS]; exact IT.wrong
      | some m =>
        simp only [initE] at h
        obtain ⟨σ1, h1, h⟩ := bind_ok h
        obtain ⟨σ2, h2, h⟩ := bind_ok h
        obtain ⟨rfl, ho⟩ := exitI_eq h
        have hs1 := initE_shape sigs σ b σ1 h1
        have hs2 := initE_shape sigs σ1 k σ' h2
        simp only [evalS]
        refine IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun bt => ?_)
        split
        · simp only [pure_bind]
          exact IT.bind (evalS_init n _ b σ σ1 (hs.of_eq (sigs_under _ b)) h1) hs1 (fun v =>
            IT.bind (evalC_init n (Γ.enter _).key k _ σ1 σ' hs h2) hs2 (fun kv =>
              IT.here (indexVal_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial)))
        · exact IT.wrong
    all_goals
      simp only [evalS]
      exact IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun t =>
        evalForm_init n _ _ t σ σ' hs h0)

theorem evalC_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initE sigs σ e = .ok σ' → IT O σ σ' (fun _ _ => True) (evalC env call n Γ e t)
  | 0, _, _, _, _, _, _, _ => by simp only [evalC]; exact IT.fuel
  | n + 1, Γ, e, t, σ, σ', hs, h => by
    cases e
    case hinted hh inner =>
      simp only [initE] at h
      simp only [evalC]
      exact evalForm_init n _ inner t σ σ' hs h
    all_goals simp only [evalC]; exact evalForm_init n _ _ t σ σ' hs h

theorem evalForm_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initE sigs σ e = .ok σ' → IT O σ σ' (fun _ _ => True) (evalForm env call n Γ e t)
  | 0, _, _, _, _, _, _, _ => by simp only [evalForm]; exact IT.fuel
  | n + 1, Γ, e, t, σ, σ', hs, h => by
    have h0 := h
    have lit : ∀ {α} (a : α), σ' = σ → IT O σ σ' (fun _ _ => True) (Pure.pure a : EvalT α) :=
      fun a e => IT.pure' (fun ρ hρ => ⟨e ▸ hρ, trivial⟩)
    cases e
    case none_ | tt | ff | str | int | float | hex | atom =>
      simp only [initE, pure, Except.pure, Except.ok.injEq] at h
      simp only [evalForm]
      exact lit _ h.symm
    case er p | some_ p | ok p =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalC_init n _ p _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case tuple es =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalPairs_init n _ es _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case struct fs =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalFields_init n _ fs _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case list es | set es =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalAll_init n _ es _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case tensor shape es =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalAll_init n _ es _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case map es =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalEntries_init n _ es _ _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case table header rows =>
      simp only [initE] at h
      simp only [evalForm]
      split
      · exact IT.map' (evalRows_init n _ rows _ σ σ' hs h) (fun _ _ _ => trivial)
      · exact IT.wrong
    case term a p =>
      simp only [initE] at h
      simp only [evalForm]
      repeat' split
      all_goals first
        | exact IT.map' (evalC_init n _ p _ σ σ' hs h) (fun _ _ _ => trivial)
        | exact IT.wrong
    case enum inner =>
      simp only [initE] at h
      simp only [evalForm]
      exact evalC_init n _ inner t σ σ' hs h
    case data p | error p =>
      simp only [initE] at h
      simp only [evalForm]
      exact IT.bind (evalS_init n _ p σ σ' hs h) (initE_shape sigs σ p σ' h) (fun _ =>
        IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))
    case unop op a =>
      cases op
      case neg =>
        simp only [initE] at h
        obtain ⟨σ1, h1, h⟩ := bind_ok h
        simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
        have hs1 := initE_shape sigs σ a σ1 h1
        simp only [evalForm]
        have viaC : IT O σ σ1 (fun _ _ => True) (do
            Fun.here (unOp .neg t.expose (← evalC env call n Γ.borrow a t.expose))) :=
          IT.bind (evalC_init n _ a _ σ σ1 hs h1) hs1 (fun v =>
            IT.calm (unOp_calm _ _ _) (unOp_noEarly (op := .neg) (by simp) _ _))
        split
        · rename_i k
          split
          · simp only [initE, pure, Except.pure, Except.ok.injEq] at h1
            exact lit _ h1.symm
          · split
            · exact viaC
            · exact evalSub_init n _ _ σ σ1 hs h0
        · split
          · exact viaC
          · exact evalSub_init n _ _ σ σ1 hs h0
      all_goals simp only [evalForm]; exact evalSub_init n _ _ σ σ' hs h0
    case binop op a b =>
      simp only [evalForm]
      split
      · simp only [initE] at h
        obtain ⟨σ1, h1, h⟩ := bind_ok h
        obtain ⟨σ2, h2, h⟩ := bind_ok h
        have hs1 := initE_shape sigs σ a σ1 h1
        have hs2 := initE_shape sigs σ1 b σ2 h2
        split at h
        · obtain ⟨rfl, ho⟩ := exitI_eq h
          exact IT.bind (evalPair_init n _ a b _ σ σ1 _ hs h1 h2) (hs2.trans hs1) (fun r =>
            IT.here (binOp_calm _ _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial))
        · rename_i hx
          simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
          exact IT.bind (evalPair_init n _ a b _ σ σ1 _ hs h1 h2) (hs2.trans hs1) (fun r =>
            IT.calm (binOp_calm _ _ _ _) (binOp_noEarly (by simpa using hx) _ _ _))
      · exact evalSub_init n _ _ σ σ' hs h0
    case adapt a =>
      simp only [initE] at h
      simp only [evalForm]
      exact evalS_init n _ a σ σ' hs h
    case call f args =>
      simp only [evalForm]
      exact evalCall_init n _ f args _ σ σ' hs h
    all_goals simp only [evalForm]; exact evalSub_init n _ _ σ σ' hs h0

theorem evalSub_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initE sigs σ e = .ok σ' → IT O σ σ' (fun _ _ => True) (evalSub env call n Γ e)
  | 0, _, _, _, _, _, _ => by simp only [evalSub]; exact IT.fuel
  | n + 1, Γ, e, σ, σ', hs, h => by simp only [evalSub]; exact evalS_init n Γ e σ σ' hs h

theorem evalCall_init : ∀ (n : Nat) (Γ : Ctx) (f : String) (args : List (Option Mode × Expr))
    (expected : Option Ty) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initE sigs σ (.call f args) = .ok σ' → IT O σ σ' (fun _ _ => True) (evalCall env call n Γ f args expected)
  | 0, _, _, _, _, _, _, _, _ => by simp only [evalCall]; exact IT.fuel
  | n + 1, Γ, f, args, expected, σ, σ', hs, h => by
    simp only [initE] at h
    cases hsig : sigs.lookup f with
    | none => simp only [hsig] at h; cases h
    | some sig =>
      simp only [hsig] at h
      simp only [evalCall, hs f sig hsig]
      obtain ⟨σ1, h1, h⟩ := bind_ok h
      have hs1 := initArgs_shape sigs σ args _ σ1 h1
      have hex : σ' = giveArgs σ1 args (sig.params.map (·.1)) ∧
          (argsExit args (sig.params.map (·.1)) = true → σ1.outsSet = true) := by
        split at h
        · obtain ⟨σ2, hA, hB⟩ := bind_ok h
          obtain ⟨e2, ho⟩ := exitI_eq hA
          subst e2
          simp only [pure, Except.pure, Except.ok.injEq] at hB
          exact ⟨hB.symm, fun _ => ho⟩
        · rename_i hx
          simp only [pure, Except.pure, bind, Except.bind, Except.ok.injEq] at h
          exact ⟨h.symm, fun e => absurd e hx⟩
      obtain ⟨rfl, hex⟩ := hex
      cases hd : decided (env.call Γ f args expected) with
      | error c => exact IT.wrong
      | ok θ =>
        by_cases hg : groundArgs θ = true
        · simp only [show (!groundArgs θ) = false by simp [hg], Bool.false_eq_true, ↓reduceIte]
          split
          · exact IT.wrong
          have hms : (sig.params.map fun (m, t) => (m, t.inst θ)).map (·.1) = sig.params.map (·.1) := by
            simp [List.map_map, Function.comp_def]
          refine IT.bind' (evalArgs_init n _ args _ σ σ1 hs (hms ▸ h1)) hs1 (fun vals ρ1 hv hfr1 hρ1 => ?_)
          obtain ⟨hlen, hvs, hroots⟩ := hv
          have hvals : ∀ q ∈ (vals.map (·.1)).zip sig.params, q.2.1 ≠ .out → q.1.isSome = true := by
            intro q hq hm
            have : (q.1, (q.2.1, q.2.2.inst θ)) ∈ (vals.map (·.1)).zip (sig.params.map fun (m, t) => (m, t.inst θ)) := by
              rw [List.zip_map_right]
              exact List.mem_map.mpr ⟨q, hq, rfl⟩
            exact hvs (q.1, (q.2.1, q.2.2.inst θ)) this hm
          have hc := hcall f sig θ (vals.map (·.1)) hsig hvals
          have hcalm : Calm (call f θ (vals.map (·.1))) := by
            intro s hs'
            rw [hs'] at hc
            exact ⟨hc.1, hc.2.2.1, hc.2.2.2.1, hc.2.2.2.2⟩
          have hne : ∀ r, call f θ (vals.map (·.1)) = .error (.early r) → σ1.outsSet = true := by
            intro r hr; rw [hr] at hc; exact absurd rfl (hc.2.1 r)
          refine IT.res_bind (IT.res_here hcalm hne hfr1 hρ1) (fun a ρ2 ⟨ha, e2⟩ hρ2 => ?_)
          subst e2
          obtain ⟨r, after⟩ := a
          rw [ha] at hc
          obtain ⟨hvl, hal, haft⟩ := hc
          simp only
          refine IT.res_get ?_
          have hps : (sig.params.map fun (m, t) => (m, t.inst θ)).length = sig.params.length := by simp
          have hle : (sig.params.map fun (m, t) => (m, t.inst θ)).length ≤ args.length := by
            rw [List.length_map] at hlen
            rw [hps]; simp at hvl; omega
          have hw := writeBack_init Γ ρ2 σ1 args (sig.params.map fun (m, t) => (m, t.inst θ))
            (vals.map (·.2)) after hρ1 hroots (fun q hq hm => by
              rw [List.zip_map_right] at hq
              obtain ⟨q', hq', rfl⟩ := List.mem_map.mp hq
              exact haft q' hq' hm) (by rw [hps]; exact hal) (by rw [hps, List.length_map]; simpa using hvl) hle
          rw [hms] at hw
          have hwc : Calm (writeBack Γ ρ2 args (sig.params.map fun (m, t) => (m, t.inst θ)) (vals.map (·.2)) after) := by
            intro s hs'
            rw [hs'] at hw
            exact hw.1
          have hwe : ∀ r, writeBack Γ ρ2 args (sig.params.map fun (m, t) => (m, t.inst θ)) (vals.map (·.2)) after
              = .error (.early r) → σ1.outsSet = true := by
            intro r hr; rw [hr] at hw; exact hex (hw.2 r rfl)
          refine IT.res_bind (IT.res_here hwc hwe hfr1 hρ1) (fun ρ3 ρ4 ⟨hw3, e4⟩ hρ4 => ?_)
          subst e4
          rw [hw3] at hw
          exact IT.res_set_pure hw.1 hw.2 trivial
        · simp only [show (!groundArgs θ) = true by simpa using hg, ↓reduceIte]
          exact IT.unsupported

theorem evalArgs_init : ∀ (n : Nat) (Γ : Ctx) (args : List (Option Mode × Expr)) (ps : List (Mode × Ty))
    (σ σ' : ISt), SigsIn sigs Γ.sigs → initArgs sigs σ args (ps.map (·.1)) = .ok σ' →
    IT O σ σ' (fun vs ρ' => IArgs args ps vs ρ') (evalArgs env call n Γ args ps)
  | 0, _, _, _, _, _, _, _ => by simp only [evalArgs]; exact IT.fuel
  | n + 1, Γ, (mk, e) :: args, (mode, t) :: ps, σ, σ', hs, h => by
    simp only [List.map_cons, initArgs] at h
    rw [evalArgs.eq_def]; dsimp only
    have tail : ∀ (x : EvalT (Option Val × List Val)) σ1, initArgs sigs σ1 args (ps.map (·.1)) = .ok σ' →
        σ1.shape = σ.shape →
        IT O σ σ1 (fun a ρ => (mode ≠ .out → a.1.isSome = true) ∧ ((mode == .mut || mode == .out) = true →
          ∀ x st, placeOf e = some (x, st) → st ≠ [] → ρ.lookup x ≠ some none)) x →
        IT O σ σ' (fun vs ρ' => IArgs ((mk, e) :: args) ((mode, t) :: ps) vs ρ')
          (x >>= fun a => evalArgs env call n Γ args ps >>= fun vs => pure (a :: vs)) := by
      intro x σ1 hr hs1 hx
      refine IT.bind' hx hs1 (fun a ρa ha hfr1 hρa => ?_)
      refine IT.res_bindK (evalArgs_init n Γ args ps σ1 σ' hs hr hfr1 ρa hρa) (fun vs ρ' k hv hρ' => ?_)
      obtain ⟨hl, hvs, hroots⟩ := hv
      refine ⟨Keeps.refl ρ', hρ', by simp [hl, Nat.succ_min_succ], fun q hq hm => ?_,
        ⟨fun hm x st hp hst => k.lookup (ha.2 hm x st hp hst), hroots⟩⟩
      simp only [List.map_cons, List.zip_cons_cons, List.mem_cons] at hq
      rcases hq with rfl | hq
      · exact ha.1 hm
      · exact hvs q hq hm
    cases hp : placeOf e with
    | some pl =>
      simp only [hp] at h
      simp only at h ⊢
      by_cases hm : (mode == .mut || mode == .out) = true
      · simp only [hm, ↓reduceIte] at h ⊢
        obtain ⟨σ1, h1, h2⟩ := bind_ok h
        refine tail _ σ1 h2 (initPlace_shape sigs σ e _ σ1 h1) ?_
        unfold placeArg
        refine IT.bind' (placeC_init n _ e t σ σ1 (mode == .mut) hs h1) (initPlace_shape sigs σ e _ σ1 h1)
          (fun r ρa hpl hfr1 hρa => ?_)
        obtain ⟨o, kvs⟩ := r
        have root : (mode == .mut || mode == .out) = true →
            ∀ x st, placeOf e = some (x, st) → st ≠ [] → ρa.lookup x ≠ some none :=
          fun _ x st hp' hst => hpl.2 x st hp' (.inr hst)
        by_cases ho : mode = .out
        · subst ho
          exact ⟨Keeps.refl ρa, hρa, fun h => absurd rfl h, root⟩
        · simp only [show (mode == .out) = false by simpa using ho, Bool.false_eq_true, ↓reduceIte]
          have hmut : (mode == .mut) = true := by
            simp only [Bool.or_eq_true, beq_iff_eq] at hm
            rcases hm with hm | hm
            · simp [hm]
            · exact absurd hm ho
          cases o with
          | none => exact absurd (hpl.1 hmut) (by simp)
          | some v => exact ⟨Keeps.refl ρa, hρa, fun _ => rfl, fun _ => root hm⟩
      · simp only [hm, Bool.false_eq_true, ↓reduceIte] at h ⊢
        obtain ⟨σ1, h1, h2⟩ := bind_ok h
        have ho : mode ≠ .out := by intro e'; subst e'; simp at hm
        refine tail _ σ1 h2 (initE_shape sigs σ e σ1 h1) ?_
        exact IT.weaken (argByValue_some ho (evalC_init n _ e t σ σ1 hs h1) (initE_shape sigs σ e σ1 h1))
          (fun a ρ ha => ⟨fun _ => ha, fun hm' => absurd hm' (by simpa using hm)⟩)
    | none =>
      simp only [hp] at h
      simp only at h ⊢
      by_cases ho : mode = .out
      · subst ho
        simp only [beq_self_eq_true, ↓reduceIte, pure_bind] at h
        refine tail _ σ h rfl ?_
        unfold argByValue
        simp only [beq_self_eq_true, ↓reduceIte, pure_bind]
        exact IT.pure' (fun ρ hρ => ⟨hρ, fun h => absurd rfl h, fun _ x st hp' => by rw [hp] at hp'; cases hp'⟩)
      · simp only [show (mode == .out) = false by simpa using ho, Bool.false_eq_true, ↓reduceIte] at h
        obtain ⟨σ1, h1, h2⟩ := bind_ok h
        refine tail _ σ1 h2 (initE_shape sigs σ e σ1 h1) ?_
        exact IT.weaken (argByValue_some ho (evalC_init n _ e t σ σ1 hs h1) (initE_shape sigs σ e σ1 h1))
          (fun a ρ ha => ⟨fun _ => ha, fun _ x st hp' => by rw [hp] at hp'; cases hp'⟩)
  | n + 1, Γ, [], ps, σ, σ', hs, h => by
    simp only [initArgs, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalArgs]
    exact IT.pure' (fun ρ hρ => ⟨hρ, by simp, by simp, trivial⟩)
  | n + 1, Γ, _ :: _, [], σ, σ', hs, h => by
    simp only [List.map_nil, initArgs, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalArgs]
    exact IT.pure' (fun ρ hρ => ⟨hρ, by simp, by simp, trivial⟩)

theorem placeC_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (σ σ' : ISt) (need : Bool), SigsIn sigs Γ.sigs →
    initPlace sigs σ e need = .ok σ' → IT O σ σ' (fun r ρ' => IPlace need e r.1 ρ') (placeC env call n Γ e t)
  | 0, _, _, _, _, _, _, _, _ => by simp only [placeC]; exact IT.fuel
  | n + 1, Γ, e, t, σ, σ', need, hs, h => by
    cases e
    case hinted hh inner =>
      simp only [initPlace] at h
      simp only [placeC]
      exact IT.weaken (placeForm_init n _ inner t σ σ' need hs h) (fun r ρ hr => by
        simpa [IPlace, placeOf] using hr)
    all_goals simp only [placeC]; exact placeForm_init n _ _ t σ σ' need hs h

theorem placeForm_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) (σ σ' : ISt) (need : Bool), SigsIn sigs Γ.sigs →
    initPlace sigs σ e need = .ok σ' → IT O σ σ' (fun r ρ' => IPlace need e r.1 ρ') (placeForm env call n Γ e t)
  | 0, _, _, _, _, _, _, _, _ => by simp only [placeForm]; exact IT.fuel
  | n + 1, Γ, e, t, σ, σ', need, hs, h => by simp only [placeForm]; exact placeSub_init n Γ e σ σ' need hs h

theorem placeSub_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (σ σ' : ISt) (need : Bool), SigsIn sigs Γ.sigs →
    initPlace sigs σ e need = .ok σ' → IT O σ σ' (fun r ρ' => IPlace need e r.1 ρ') (placeSub env call n Γ e)
  | 0, _, _, _, _, _, _, _ => by simp only [placeSub]; exact IT.fuel
  | n + 1, Γ, e, σ, σ', need, hs, h => by simp only [placeSub]; exact placeS_init n Γ e σ σ' need hs h

theorem placeS_init : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (σ σ' : ISt) (need : Bool), SigsIn sigs Γ.sigs →
    initPlace sigs σ e need = .ok σ' → IT O σ σ' (fun r ρ' => IPlace need e r.1 ρ') (placeS env call n Γ e)
  | 0, _, _, _, _, _, _, _ => by simp only [placeS]; exact IT.fuel
  | n + 1, Γ, e, σ, σ', need, hs, h => by
    cases e
    case var x =>
      simp only [initPlace] at h
      have hσ : σ' = σ ∧ (need = true → σ.holds x ≠ some false) := by
        split at h
        · obtain ⟨rfl, hx⟩ := readI_eq h; exact ⟨rfl, fun _ => hx⟩
        · rename_i hn
          simp only [pure, Except.pure, Except.ok.injEq] at h
          exact ⟨h.symm, fun e => absurd e hn⟩
      obtain ⟨rfl, hx⟩ := hσ
      simp only [placeS]
      refine IT.get' (fun ρ hfr hρ => ?_)
      split
      · rename_i o ho
        refine ⟨Keeps.refl ρ, hρ, fun hn => ?_, fun y st hp hst => ?_⟩
        · have := IAg.lookup hρ (hx hn)
          rw [ho] at this
          cases o
          · exact absurd rfl this
          · rfl
        · simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
          obtain ⟨rfl, rfl⟩ := hp
          rcases hst with hn | hst
          · exact IAg.lookup hρ (hx hn)
          · exact absurd rfl hst
      · exact IT.wrong hfr ρ hρ
    case place x steps =>
      simp only [initPlace] at h
      obtain ⟨σ1, h1, hst⟩ := bind_ok h
      obtain ⟨rfl, hx⟩ := readI_eq h1
      simp only [placeS]
      refine IT.get' (fun ρ hfr hρ => ?_)
      have hl := IAg.lookup hρ hx
      split
      · rename_i v u k hv hu
        refine IT.res_bindK (evalPath_init n (Γ.enter _) u v steps _ _ hs hst hfr ρ hρ)
          (fun r ρ' kk _ hρ' => ⟨Keeps.refl ρ', hρ', fun _ => rfl, fun y st hp _ => ?_⟩)
        simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
        obtain ⟨rfl, rfl⟩ := hp
        exact kk.lookup (by rw [hv]; simp)
      · rename_i hn; exact absurd hn hl
      · exact IT.wrong hfr ρ hρ
    case hinted hh inner =>
      simp only [initPlace] at h
      simp only [placeS]
      split
      · simp only [pure_bind]
        exact IT.weaken (placeForm_init n _ inner _ σ σ' need hs h) (fun r ρ hr => by
          simpa [IPlace, placeOf] using hr)
      · exact IT.wrong
    case proj b sel =>
      simp only [initPlace] at h
      simp only [placeS]
      refine IT.bind' (placeS_init n _ b σ σ' true (hs.of_eq (sigs_under _ b)) h) (initPlace_shape sigs σ b true σ' h)
        (fun r ρa hpl hfr1 hρa => ?_)
      obtain ⟨o, kvs⟩ := r
      cases o with
      | none => exact absurd (hpl.1 rfl) (by simp)
      | some v =>
        simp only
        refine IT.res_bindK (IT.res_here (fieldVal_calm v sel) (fun r hr => absurd hr (fieldVal_noEarly v sel r))
          hfr1 hρa) (fun w ρb kb ⟨_, eb⟩ hρb => ?_)
        subst eb
        refine ⟨Keeps.refl ρb, hρb, fun _ => rfl, fun y st hp _ => ?_⟩
        simp only [placeOf, Option.map_eq_some_iff, Prod.exists, Prod.mk.injEq] at hp
        obtain ⟨y', st', hb, rfl, rfl⟩ := hp
        exact hpl.2 y' st' hb (.inl rfl)
    case index b k mode =>
      cases mode with
      | none => simp only [placeS]; exact IT.wrong
      | some m =>
        simp only [initPlace] at h
        obtain ⟨σ1, h1, h⟩ := bind_ok h
        obtain ⟨σ2, h2, h⟩ := bind_ok h
        obtain ⟨e2, ho⟩ := exitI_eq h
        subst e2
        have hs1 := initPlace_shape sigs σ b true σ1 h1
        have hs2 := initE_shape sigs σ1 k _ h2
        simp only [placeS]
        refine IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun bt => ?_)
        split
        · simp only [pure_bind]
          refine IT.bind' (placeS_init n _ b σ σ1 true (hs.of_eq (sigs_under _ b)) h1) hs1
            (fun r ρa hpl hfr1 hρa => ?_)
          obtain ⟨o, kvs⟩ := r
          refine IT.res_bindK (evalC_init n (Γ.enter _).key k _ σ1 _ hs h2 hfr1 ρa hρa)
            (fun kv ρb kb _ hρb => ?_)
          cases o with
          | none => exact absurd (hpl.1 rfl) (by simp)
          | some v =>
            simp only
            refine IT.res_bindK (IT.res_here (indexVal_calm m v kv) (fun _ _ => ho) (hfr1.of_shape hs2) hρb)
              (fun w ρc kc ⟨_, ec⟩ hρc => ?_)
            subst ec
            refine ⟨Keeps.refl ρc, hρc, fun _ => rfl, fun y st hp _ => ?_⟩
            simp only [placeOf, Option.map_eq_some_iff, Prod.exists, Prod.mk.injEq] at hp
            obtain ⟨y', st', hb, rfl, rfl⟩ := hp
            exact kb.lookup (hpl.2 y' st' hb (.inl rfl))
        · exact IT.wrong
    all_goals simp only [placeS]; exact IT.wrong

theorem evalPath_init : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (steps : List Step) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initSteps sigs σ steps = .ok σ' →
    IT O σ σ' (fun _ _ => True) (evalPath env call n Γ t v steps)
  | 0, _, _, _, _, _, _, _, _ => by simp only [evalPath]; exact IT.fuel
  | _ + 1, _, _, v, [], σ, σ', _, h => by
    simp only [initSteps, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalPath]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, t, v, [st], σ, σ', hs, h => by
    simp only [evalPath]
    exact evalPathStep_init n Γ t v st σ σ' hs h
  | n + 1, Γ, t, v, st :: st2 :: rest, σ, σ', hs, h => by
    rw [initSteps_cons] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    have hs1 := initSteps_shape sigs σ [st] σ1 h1
    simp only [evalPath]
    exact IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun u =>
      IT.bind (evalPathStep_init n _ t v st σ σ1 hs h1) hs1 (fun p =>
        IT.bind (evalPath_init n Γ u p.1 (st2 :: rest) σ1 σ' hs h) (initSteps_shape sigs σ1 _ σ' h)
          (fun _ => IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))))

theorem evalPathStep_init : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (st : Step) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initSteps sigs σ [st] = .ok σ' →
    IT O σ σ' (fun _ _ => True) (evalPathStep env call n Γ t v st)
  | 0, _, _, _, _, _, _, _, _ => by simp only [evalPathStep]; exact IT.fuel
  | _ + 1, _, _, v, .field s, σ, σ', _, h => by
    simp only [initSteps, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalPathStep]
    exact IT.bind (IT.calm (fieldVal_calm _ _) (fieldVal_noEarly _ _)) rfl (fun _ =>
      IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))
  | _ + 1, _, _, _, .index _ none, σ, σ', _, h => by simp only [evalPathStep]; exact IT.wrong
  | n + 1, Γ, t, v, .index k (some m), σ, σ', hs, h => by
    simp only [initSteps] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    obtain ⟨σ2, h2, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    obtain ⟨rfl, ho⟩ := exitI_eq h2
    simp only [evalPathStep]
    split
    · simp only [pure_bind]
      exact IT.bind (evalC_init n _ k _ σ σ2 hs h1) (initE_shape sigs σ k σ2 h1) (fun kv =>
        IT.bind (IT.here (indexVal_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial)) rfl (fun _ =>
          IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))
    · exact IT.wrong

theorem evalSteps_init : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (steps : List Step) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initSteps sigs σ steps = .ok σ' →
    IT O σ σ' (fun _ _ => True) (evalSteps env call n Γ t v steps)
  | 0, _, _, _, _, _, _, _, _ => by simp only [evalSteps]; exact IT.fuel
  | _ + 1, _, _, v, [], σ, σ', _, h => by
    simp only [initSteps, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalSteps]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, t, v, [st], σ, σ', hs, h => by
    simp only [evalSteps]
    exact evalStep_init n Γ t v st σ σ' hs h
  | n + 1, Γ, t, v, st :: st2 :: rest, σ, σ', hs, h => by
    rw [initSteps_cons] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    have hs1 := initSteps_shape sigs σ [st] σ1 h1
    simp only [evalSteps]
    exact IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun u =>
      IT.bind (evalStep_init n _ t v st σ σ1 hs h1) hs1 (fun w =>
        evalSteps_init n Γ u w (st2 :: rest) σ1 σ' hs h))

theorem evalStep_init : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (v : Val) (st : Step) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initSteps sigs σ [st] = .ok σ' →
    IT O σ σ' (fun _ _ => True) (evalStep env call n Γ t v st)
  | 0, _, _, _, _, _, _, _, _ => by simp only [evalStep]; exact IT.fuel
  | _ + 1, _, _, v, .field s, σ, σ', _, h => by
    simp only [initSteps, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalStep]
    exact IT.calm (fieldVal_calm _ _) (fieldVal_noEarly _ _)
  | _ + 1, _, _, _, .index _ none, σ, σ', _, h => by simp only [evalStep]; exact IT.wrong
  | n + 1, Γ, t, v, .index k (some m), σ, σ', hs, h => by
    simp only [initSteps] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    obtain ⟨σ2, h2, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    obtain ⟨rfl, ho⟩ := exitI_eq h2
    simp only [evalStep]
    split
    · simp only [pure_bind]
      exact IT.bind (evalC_init n _ k _ σ σ2 hs h1) (initE_shape sigs σ k σ2 h1) (fun kv =>
        IT.here (indexVal_calm _ _ _) (fun _ _ => ho) (fun _ _ _ _ => trivial))
    · exact IT.wrong

theorem evalPair_init : ∀ (n : Nat) (Γ : Ctx) (a b : Expr) (expected : Option Ty) (σ σ1 σ' : ISt),
    SigsIn sigs Γ.sigs → initE sigs σ a = .ok σ1 → initE sigs σ1 b = .ok σ' →
    IT O σ σ' (fun _ _ => True) (evalPair env call n Γ a b expected)
  | 0, _, _, _, _, _, _, _, _, _, _ => by simp only [evalPair]; exact IT.fuel
  | n + 1, Γ, a, b, expected, σ, σ1, σ', hs, h1, h2 => by
    simp only [evalPair]
    exact IT.bind (IT.calm (typeOf_calm _) (typeOf_noEarly _)) rfl (fun t =>
      IT.bind (evalC_init n _ a t σ σ1 hs h1) (initE_shape sigs σ a σ1 h1) (fun _ =>
        IT.bind (evalC_init n _ b t σ1 σ' hs h2) (initE_shape sigs σ1 b σ' h2) (fun _ =>
          IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))))

theorem evalPairs_init : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (ts : List Ty) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initAll sigs σ es = .ok σ' → IT O σ σ' (fun _ _ => True) (evalPairs env call n Γ es ts)
  | 0, _, _, _, _, _, _, _ => by simp only [evalPairs]; exact IT.fuel
  | n + 1, Γ, e :: es, t :: ts, σ, σ', hs, h => by
    simp only [initAll] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [evalPairs]
    exact IT.bind (evalC_init n _ e t σ σ1 hs h1) (initE_shape sigs σ e σ1 h1) (fun _ =>
      IT.bind (evalPairs_init n Γ es ts σ1 σ' hs h) (initAll_shape sigs σ1 es σ' h) (fun _ =>
        IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))
  | n + 1, Γ, [], [], σ, σ', hs, h => by
    simp only [initAll, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalPairs]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, [], _ :: _, σ, σ', hs, h => by simp only [evalPairs]; exact IT.wrong
  | n + 1, Γ, _ :: _, [], σ, σ', hs, h => by simp only [evalPairs]; exact IT.wrong

theorem evalFields_init : ∀ (n : Nat) (Γ : Ctx) (fs : List (String × Expr)) (ts : List (String × Ty)) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initFields sigs σ fs = .ok σ' → IT O σ σ' (fun _ _ => True) (evalFields env call n Γ fs ts)
  | 0, _, _, _, _, _, _, _ => by simp only [evalFields]; exact IT.fuel
  | n + 1, Γ, (m, e) :: fs, (k, t) :: ts, σ, σ', hs, h => by
    simp only [initFields] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [evalFields]
    exact IT.bind (evalC_init n _ e t σ σ1 hs h1) (initE_shape sigs σ e σ1 h1) (fun _ =>
      IT.bind (evalFields_init n Γ fs ts σ1 σ' hs h) (initFields_shape sigs σ1 fs σ' h) (fun _ =>
        IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))
  | n + 1, Γ, [], [], σ, σ', hs, h => by
    simp only [initFields, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalFields]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, [], _ :: _, σ, σ', hs, h => by simp only [evalFields]; exact IT.wrong
  | n + 1, Γ, _ :: _, [], σ, σ', hs, h => by simp only [evalFields]; exact IT.wrong

theorem evalAll_init : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (t : Ty) (σ σ' : ISt), SigsIn sigs Γ.sigs →
    initAll sigs σ es = .ok σ' → IT O σ σ' (fun _ _ => True) (evalAll env call n Γ es t)
  | 0, _, _, _, _, _, _, _ => by simp only [evalAll]; exact IT.fuel
  | _ + 1, _, [], _, σ, σ', _, h => by
    simp only [initAll, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalAll]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, e :: es, t, σ, σ', hs, h => by
    simp only [initAll] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [evalAll]
    exact IT.bind (evalC_init n _ e t σ σ1 hs h1) (initE_shape sigs σ e σ1 h1) (fun _ =>
      IT.bind (evalAll_init n Γ es t σ1 σ' hs h) (initAll_shape sigs σ1 es σ' h) (fun _ =>
        IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))

theorem evalEntries_init : ∀ (n : Nat) (Γ : Ctx) (es : List (Expr × Expr)) (k v : Ty) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initEntries sigs σ es = .ok σ' → IT O σ σ' (fun _ _ => True) (evalEntries env call n Γ es k v)
  | 0, _, _, _, _, _, _, _, _ => by simp only [evalEntries]; exact IT.fuel
  | _ + 1, _, [], _, _, σ, σ', _, h => by
    simp only [initEntries, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalEntries]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, (ke, ve) :: es, k, v, σ, σ', hs, h => by
    simp only [initEntries] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    obtain ⟨σ2, h2, h⟩ := bind_ok h
    simp only [evalEntries]
    exact IT.bind (evalC_init n _ ke k σ σ1 hs h1) (initE_shape sigs σ ke σ1 h1) (fun _ =>
      IT.bind (evalC_init n _ ve v σ1 σ2 hs h2) (initE_shape sigs σ1 ve σ2 h2) (fun _ =>
        IT.bind (evalEntries_init n Γ es k v σ2 σ' hs h) (initEntries_shape sigs σ2 es σ' h) (fun _ =>
          IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩))))

theorem evalRows_init : ∀ (n : Nat) (Γ : Ctx) (rows : List (List Expr)) (cols : List (String × Ty)) (σ σ' : ISt),
    SigsIn sigs Γ.sigs → initRows sigs σ rows = .ok σ' → IT O σ σ' (fun _ _ => True) (evalRows env call n Γ rows cols)
  | 0, _, _, _, _, _, _, _ => by simp only [evalRows]; exact IT.fuel
  | _ + 1, _, [], _, σ, σ', _, h => by
    simp only [initRows, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalRows]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | n + 1, Γ, r :: rs, cols, σ, σ', hs, h => by
    simp only [initRows] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [evalRows]
    exact IT.bind (evalPairs_init n _ r _ σ σ1 hs h1) (initAll_shape sigs σ r σ1 h1) (fun _ =>
      IT.bind (evalRows_init n Γ rs cols σ1 σ' hs h) (initRows_shape sigs σ1 rs σ' h) (fun _ =>
        IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))

end

end

/-! ## The functions a body sees

Checking a statement never changes the functions in scope: the context it
gives is the one it was given, with variables, aliases or whether it is a
const's initializer changed. -/

theorem bindAllAs_sigs : ∀ (Γ : Ctx) (Δ : List (String × Ty)) (k : Kind), (Γ.bindAllAs Δ k).sigs = Γ.sigs
  | _, [], _ => rfl
  | Γ, (x, t) :: Δ, k => by
    show ((Γ.bind x t k).bindAllAs Δ k).sigs = Γ.sigs
    rw [bindAllAs_sigs]; rfl

theorem StateM.run_bind' {σ α β : Type} (m : StateM σ α) (f : α → StateM σ β) (s : σ) :
    (m >>= f).run s = (f (m.run s).1).run (m.run s).2 := rfl

theorem StateM.run_pure' {σ α : Type} (a : α) (s : σ) : (pure a : StateM σ α).run s = (a, s) := rfl

theorem StateM.run_modify' {σ : Type} (f : σ → σ) (s : σ) : (modify f : StateM σ PUnit).run s = (⟨⟩, f s) := rfl

theorem bindPat_sigs (Γ : Ctx) (p : Pat) (t : Ty) (k : Kind) (d : Array Lit.Code) :
    ((bindPat Γ p t k).run d).1.sigs = Γ.sigs := by
  unfold bindPat
  split
  · simp only [StateM.run_pure']; exact bindAllAs_sigs Γ _ k
  · simp only [StateM.run_bind', StateM.run_modify', StateM.run_pure']

theorem bindChecked_sigs (env : InferEnv) (n : Nat) (Γ : Ctx) (p : Pat) (hint : Option Hint) (e : Expr)
    (k : Kind) (d : Array Lit.Code) : ((bindChecked env n Γ p hint e k).run d).1.sigs = Γ.sigs := by
  unfold bindChecked
  cases hint with
  | some h =>
    simp only [StateM.run_bind']
    split
    · simp only [StateM.run_pure']
    · split
      · simp only [StateM.run_bind', StateM.run_modify']; rw [bindPat_sigs]
      · simp only [StateM.run_bind']; rw [bindPat_sigs]
  | none =>
    simp only [StateM.run_bind']
    repeat' split
    all_goals first | rw [bindPat_sigs] | simp only [StateM.run_pure']

theorem checkStmtF_sigs (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (s : Stmt) (d : Array Lit.Code),
    ((checkStmtF env n Γ s).run d).1.sigs = Γ.sigs
  | 0, Γ, s, d => by simp only [checkStmtF, StateM.run_bind', StateM.run_modify', StateM.run_pure']
  | n + 1, Γ, s, d => by
    cases s
    case let_ p hint e => simp only [checkStmtF]; exact bindChecked_sigs env n Γ p hint e .let_ d
    case var_ p hint v =>
      cases v with
      | some e => simp only [checkStmtF]; exact bindChecked_sigs env n Γ p hint e .var_ d
      | none =>
        cases hint with
        | none => simp only [checkStmtF, StateM.run_pure']
        | some h =>
          simp only [checkStmtF, StateM.run_bind']
          split
          · simp only [StateM.run_bind']; rw [bindPat_sigs]
          · simp only [StateM.run_pure']
    case const_ x hint e =>
      simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
      show ((bindChecked env n { Γ with constExpr := true } (.name x) hint e .const_).run d).1.sigs = Γ.sigs
      rw [bindChecked_sigs]
    case type_ x h => simp only [checkStmtF, StateM.run_pure']; split <;> rfl
    case if_ c tB eB t e =>
      cases tB <;> cases eB <;> simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
    case loop c body =>
      cases c <;> simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
    all_goals (simp only [checkStmtF]; simp only [StateM.run_bind', StateM.run_pure'])

/-! ## Scopes -/

/-- `τ` is of a scope inside `σ`'s: `σ`'s variables, with more bound in
front, none of them an `out` parameter. -/
def ISt.Inside (σ τ : ISt) : Prop := ∃ δ : List (String × Bool), (∀ p ∈ δ, p.2 = false) ∧ τ.shape = δ ++ σ.shape

theorem ISt.Inside.refl (σ : ISt) : σ.Inside σ := ⟨[], by simp, rfl⟩

theorem ISt.Inside.of_shape {σ τ : ISt} (h : τ.shape = σ.shape) : σ.Inside τ := ⟨[], by simp, h⟩

theorem ISt.Inside.trans {a b c : ISt} (h1 : a.Inside b) (h2 : b.Inside c) : a.Inside c := by
  obtain ⟨δ1, hd1, e1⟩ := h1
  obtain ⟨δ2, hd2, e2⟩ := h2
  refine ⟨δ2 ++ δ1, fun p hp => ?_, by rw [e2, e1, List.append_assoc]⟩
  rcases List.mem_append.mp hp with hp | hp
  · exact hd2 p hp
  · exact hd1 p hp

theorem ISt.Inside.bind (σ : ISt) (s : Bool) : ∀ (xs : List String), σ.Inside (σ.bind xs s)
  | [] => ISt.Inside.refl σ
  | x :: xs => by
    show σ.Inside (ISt.bind ((x, s, false) :: σ) xs s)
    exact (ISt.Inside.trans ⟨[(x, false)], by simp, rfl⟩ (ISt.Inside.bind _ s xs))

theorem Fr.inside {O : List (String × Bool)} {σ τ : ISt} (h : Fr O σ) (hi : σ.Inside τ) : Fr O τ := by
  obtain ⟨δ, hd, e⟩ := h
  obtain ⟨δ', hd', e'⟩ := hi
  refine ⟨δ' ++ δ, fun p hp => ?_, by rw [e', e, List.append_assoc]⟩
  rcases List.mem_append.mp hp with hp | hp
  · exact hd' p hp
  · exact hd p hp

theorem ISt.shape_length (σ : ISt) : σ.shape.length = σ.length := by simp [ISt.shape]

theorem ISt.back_shape {σ τ : ISt} (h : σ.Inside τ) : (σ.back τ).shape = σ.shape := by
  obtain ⟨δ, -, e⟩ := h
  have hl : τ.length = δ.length + σ.length := by
    rw [← ISt.shape_length, e, List.length_append, ISt.shape_length]
  simp only [ISt.back, ISt.shape, List.map_drop]
  have : τ.length - σ.length = δ.length := by omega
  rw [this]
  have e' : τ.map (fun v => (v.1, v.2.2)) = δ ++ σ.map (fun v => (v.1, v.2.2)) := e
  rw [e', List.drop_left]

theorem ISt.meet_shape : ∀ {σ τ : ISt}, τ.shape = σ.shape → (σ.meet τ).shape = σ.shape
  | [], _, _ => by simp [ISt.meet, ISt.shape]
  | _ :: _, [], h => by simp [ISt.shape] at h
  | (x, s, o) :: σ, (y, t, o') :: τ, h => by
    simp only [ISt.shape, List.map_cons, List.cons.injEq] at h
    simp only [ISt.meet, ISt.shape, List.map_cons]
    exact congrArg _ (ISt.meet_shape (σ := σ) (τ := τ) h.2)

theorem ISt.meetAll_shape (σ : ISt) : ∀ {L : List ISt}, (∀ τ ∈ L, τ.shape = σ.shape) → (σ.meetAll L).shape = σ.shape
  | [], _ => rfl
  | [τ], h => h τ (by simp)
  | τ :: τ' :: L, h => by
    simp only [ISt.meetAll]
    have ih := ISt.meetAll_shape σ (L := τ' :: L) (fun x hx => h x (List.mem_cons_of_mem _ hx))
    rw [ISt.meet_shape (by rw [ih, h τ List.mem_cons_self]), h τ List.mem_cons_self]

/-! ## Lining up across scopes -/

theorem IAg.drop : ∀ {σ : ISt} {ρ : VEnv} (k : Nat), IAg σ ρ → IAg (σ.drop k) (ρ.drop k)
  | _, _, 0, h => h
  | _, _, _ + 1, .nil => .nil
  | _, _, k + 1, .cons _ a => IAg.drop k a

theorem IAg.meet_left : ∀ {σ τ : ISt} {ρ : VEnv}, IAg σ ρ → τ.shape = σ.shape → IAg (σ.meet τ) ρ
  | [], _, _, .nil, _ => by simp [ISt.meet]; exact .nil
  | _ :: _, [], _, .cons _ _, h => by simp [ISt.shape] at h
  | (x, s, o) :: σ, (y, t, o') :: τ, _, .cons hs a, h => by
    simp only [ISt.shape, List.map_cons, List.cons.injEq] at h
    simp only [ISt.meet]
    exact .cons (fun e => hs (by simp_all)) (IAg.meet_left a h.2)

theorem IAg.meet_right : ∀ {σ τ : ISt} {ρ : VEnv}, IAg τ ρ → τ.shape = σ.shape → IAg (σ.meet τ) ρ
  | _, [], _, .nil, h => by
    simp only [ISt.shape, List.map_nil] at h
    have : _ = [] := List.map_eq_nil_iff.mp h.symm
    subst this; simp [ISt.meet]; exact .nil
  | [], _ :: _, _, .cons _ _, h => by simp [ISt.shape] at h
  | (x, s, o) :: σ, (y, t, o') :: τ, _, .cons ht a, h => by
    simp only [ISt.shape, List.map_cons, List.cons.injEq, Prod.mk.injEq] at h
    obtain ⟨⟨rfl, rfl⟩, h⟩ := h
    simp only [ISt.meet]
    exact .cons (fun e => ht (by simp_all)) (IAg.meet_right a h)

theorem IAg.meetAll {σ : ISt} {ρ : VEnv} : ∀ {L : List ISt} {τ : ISt}, τ ∈ L → IAg τ ρ →
    (∀ x ∈ L, x.shape = σ.shape) → IAg (σ.meetAll L) ρ
  | [], _, hm, _, _ => by simp at hm
  | [x], τ, hm, h, _ => by simp at hm; subst hm; exact h
  | x :: y :: L, τ, hm, h, hs => by
    simp only [ISt.meetAll]
    have hsL : ∀ z ∈ y :: L, z.shape = σ.shape := fun z hz => hs z (List.mem_cons_of_mem _ hz)
    rcases List.mem_cons.mp hm with rfl | hm
    · exact IAg.meet_left h (by rw [ISt.meetAll_shape σ hsL, hs τ List.mem_cons_self])
    · exact IAg.meet_right (IAg.meetAll hm h hsL) (by rw [ISt.meetAll_shape σ hsL, hs x List.mem_cons_self])

/-- Leaving a scope: what the analysis had inside, back in the scope it
began in, lines up with the run's variables back there. -/
theorem IAg.back {σ τ : ISt} {ρ ρ' : VEnv} (h : IAg τ ρ') (hl : σ.length = ρ.length) :
    IAg (σ.back τ) (restore ρ ρ') := by
  unfold ISt.back restore
  rw [IAg.length h, hl]
  exact IAg.drop _ h

theorem IAg.pushed : ∀ {σ : ISt} {ρ : VEnv} {s : Bool} (vals : List (String × Option Val)),
    IAg σ ρ → (s = true → ∀ q ∈ vals, q.2.isSome = true) → IAg (σ.bind (vals.map (·.1)) s) (bindVals ρ vals)
  | _, _, _, [], h, _ => h
  | σ, ρ, s, (x, o) :: vals, h, hv => by
    show IAg (ISt.bind ((x, s, false) :: σ) (vals.map (·.1)) s) (bindVals ((x, o) :: ρ) vals)
    exact IAg.pushed vals (.cons (fun e => hv e (x, o) List.mem_cons_self) h)
      (fun e q hq => hv e q (List.mem_cons_of_mem _ hq))

/-- The variables after a statement: those before, each that held a value
holding one still, with more bound in front. -/
def Grew (ρ ρ' : VEnv) : Prop := ∃ δ ρ1, ρ' = δ ++ ρ1 ∧ Keeps ρ ρ1

theorem Grew.refl (ρ : VEnv) : Grew ρ ρ := ⟨[], ρ, rfl, Keeps.refl ρ⟩

theorem Grew.of_keeps {ρ ρ' : VEnv} (k : Keeps ρ ρ') : Grew ρ ρ' := ⟨[], ρ', rfl, k⟩

theorem Keeps.append_left : ∀ (δ : VEnv) {ρ1 ρ' : VEnv}, Keeps (δ ++ ρ1) ρ' →
    ∃ δ' ρ1', ρ' = δ' ++ ρ1' ∧ Keeps ρ1 ρ1' ∧ δ'.length = δ.length
  | [], _, ρ', k => ⟨[], ρ', rfl, k, rfl⟩
  | _ :: δ, _, _, .cons h k => by
    obtain ⟨δ', ρ1', rfl, k', hl⟩ := Keeps.append_left δ k
    exact ⟨_ :: δ', ρ1', rfl, k', by simp [hl]⟩

theorem Grew.trans {a b c : VEnv} (h1 : Grew a b) (h2 : Grew b c) : Grew a c := by
  obtain ⟨δ1, ρ1, rfl, k1⟩ := h1
  obtain ⟨δ2, ρ2, rfl, k2⟩ := h2
  obtain ⟨δ', ρ1', rfl, k', -⟩ := Keeps.append_left δ1 k2
  exact ⟨δ2 ++ δ', ρ1', by simp, k1.trans k'⟩

theorem Grew.keeps_right {a b c : VEnv} (h1 : Grew a b) (k : Keeps b c) : Grew a c :=
  h1.trans (Grew.of_keeps k)

/-- Back in the scope it began in, what a block left keeps every value. -/
theorem Grew.restore_keeps {ρ ρ' : VEnv} (h : Grew ρ ρ') : Keeps ρ (restore ρ ρ') := by
  obtain ⟨δ, ρ1, rfl, k⟩ := h
  unfold restore
  rw [List.length_append, ← k.length]
  simp only [Nat.add_sub_cancel, List.drop_left]
  exact k

theorem bindVals_grew (ρ : VEnv) : ∀ (vals : List (String × Option Val)), Grew ρ (bindVals ρ vals)
  | [] => Grew.refl ρ
  | (x, o) :: vals => by
    show Grew ρ (bindVals ((x, o) :: ρ) vals)
    obtain ⟨δ, ρ1, e, k⟩ := bindVals_grew ((x, o) :: ρ) vals
    obtain ⟨δ', ρ1', rfl, k', -⟩ := Keeps.append_left [(x, o)] (by simpa using k)
    exact ⟨δ ++ δ', ρ1', by rw [e]; simp, k'⟩

section
variable (sigs : List (String × Sig))

theorem initKeys_shape : ∀ (σ : ISt) (steps : List Step), KeepsShape (initKeys sigs σ steps) σ
  | σ, [] => by simp only [initKeys]; exact KeepsShape.pure
  | σ, .field _ :: rest => by simp only [initKeys]; exact initKeys_shape σ rest
  | σ, .index k _ :: rest => by
    simp only [initKeys]
    exact KeepsShape.bind (initE_shape sigs σ k) (fun σ1 h1 => (initKeys_shape σ1 rest).trans h1)

theorem initTarget_shape (σ : ISt) (x : String) (steps : List Step) (op : Option BinOp) :
    KeepsShape (initTarget σ x steps op) σ := by
  unfold initTarget
  split
  · exact KeepsShape.readI σ x
  · split
    · intro σ' e; simp [throw, throwThe, MonadExceptOf.throw] at e
    · exact KeepsShape.pure

theorem initSet_shape (σ : ISt) (x : String) (steps : List Step) (op : Option BinOp) (e : Expr) :
    KeepsShape (initSet sigs σ x steps op e) σ := by
  unfold initSet
  refine KeepsShape.bind (initE_shape sigs σ e) (fun σ1 h1 =>
    KeepsShape.bind ((initKeys_shape sigs σ1 steps).trans h1) (fun σ2 h2 =>
      KeepsShape.bind ((initTarget_shape σ2 x steps op).trans h2) (fun σ3 h3 =>
        KeepsShape.bind (σ := σ) ?_ (fun σ4 h4 σ5 e => ?_))))
  · split
    · exact (KeepsShape.exitI σ3).trans h3
    · exact KeepsShape.pure.trans h3
  · cases e; split
    · rw [give_shape]; exact h4
    · exact h4

theorem ISt.Inside.of_keepsShape {r : InitM ISt} {σ σ' : ISt} (h : KeepsShape r σ) (e : r = .ok σ') : σ.Inside σ' :=
  ISt.Inside.of_shape (h σ' e)

mutual

theorem initStmt_shape : ∀ (σ : ISt) (s : Stmt) (out : IOut), initStmt sigs σ s = .ok out →
    σ.Inside out.1 ∧ ∀ b ∈ out.2, σ.Inside b
  | σ, .let_ p _ e, out, h => by
    simp only [initStmt] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨(ISt.Inside.of_keepsShape (initE_shape sigs σ e) h1).trans (ISt.Inside.bind _ _ _), by simp⟩
  | σ, .var_ p _ (some e), out, h => by
    simp only [initStmt] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨(ISt.Inside.of_keepsShape (initE_shape sigs σ e) h1).trans (ISt.Inside.bind _ _ _), by simp⟩
  | σ, .var_ p (some _) none, out, h => by
    simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.bind _ _ _, by simp⟩
  | σ, .var_ _ none none, out, h => by
    simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.refl σ, by simp⟩
  | σ, .set_ x steps op e, out, h => by
    simp only [initStmt] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.of_keepsShape (initSet_shape sigs σ x steps op e) h1, by simp⟩
  | σ, .fun_ _, out, h | σ, .cont, out, h | σ, .type_ _ _, out, h => by
    simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.refl σ, by simp⟩
  | σ, .brk, out, h => by
    simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.refl σ, by simp [ISt.Inside.refl]⟩
  | σ, .ret e, out, h => by
    simp only [initStmt] at h
    obtain ⟨σ1, h1, hr⟩ := bind_ok h
    obtain ⟨e1, -⟩ := exitI_eq h1
    cases e with
    | none =>
      simp only [pure, Except.pure, Except.ok.injEq] at hr; subst hr; subst e1
      exact ⟨ISt.Inside.refl _, by simp⟩
    | some e =>
      obtain ⟨σ2, h2, hr2⟩ := bind_ok hr
      simp only [pure, Except.pure, Except.ok.injEq] at hr2; subst hr2; subst e1
      exact ⟨ISt.Inside.of_keepsShape (initE_shape sigs _ e) h2, by simp⟩
  | σ, .debuglog e, out, h | σ, .call e, out, h => by
    simp only [initStmt] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.of_keepsShape (initE_shape sigs σ e) h1, by simp⟩
  | σ, .const_ x _ e, out, h => by
    simp only [initStmt] at h
    obtain ⟨σ1, h1, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨(ISt.Inside.of_keepsShape (initE_shape sigs σ e) h1).trans (ISt.Inside.bind _ _ _), by simp⟩
  | σ, .if_ c tB eB t e, out, h => by
    simp only [initStmt] at h
    obtain ⟨σc, hc, h⟩ := bind_ok h
    have ic := ISt.Inside.of_keepsShape (initE_shape sigs σ c) hc
    obtain ⟨rt, ht, h⟩ := bind_ok h
    obtain ⟨it, ibt⟩ := initBlock_shape _ t _ ht
    have it' := (ISt.Inside.bind σc true tB.toList).trans it
    obtain ⟨re, he, h⟩ := bind_ok h
    obtain ⟨ie, ibe⟩ := initElse_shape _ e _ he
    have ie' := (ISt.Inside.bind σc true eB.toList).trans ie
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨ic.trans (ISt.Inside.of_shape ?_), fun b hb => ?_⟩
    · rw [ISt.meet_shape (by rw [ISt.back_shape ie', ISt.back_shape it'])]
      exact ISt.back_shape it'
    · rcases List.mem_append.mp hb with hb | hb
      · exact ic.trans ((ISt.Inside.bind σc true _).trans (ibt b hb))
      · exact ic.trans ((ISt.Inside.bind σc true _).trans (ibe b hb))
  | σ, .loop c body, out, h => by
    simp only [initStmt] at h
    obtain ⟨σc, hc, h⟩ := bind_ok h
    have ic : σ.Inside σc := by
      cases c with
      | none => simp only [pure, Except.pure, Except.ok.injEq] at hc; subst hc; exact ISt.Inside.refl _
      | some c => exact ISt.Inside.of_keepsShape (initE_shape sigs σ c) hc
    obtain ⟨r, hb, h⟩ := bind_ok h
    obtain ⟨-, ibs⟩ := initBlock_shape _ body _ hb
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨ic.trans (ISt.Inside.of_shape (ISt.meetAll_shape σc (fun τ hτ => ?_))), by simp⟩
    simp only [List.mem_append, List.mem_map] at hτ
    rcases hτ with hτ | ⟨b, hb', rfl⟩
    · split at hτ
      · simp at hτ; subst hτ; rfl
      · simp at hτ
    · exact ISt.back_shape (ibs b hb')
  | σ, .match_ e cases dflt, out, h => by
    simp only [initStmt] at h
    obtain ⟨σc, hc, h⟩ := bind_ok h
    have ic := ISt.Inside.of_keepsShape (initE_shape sigs σ e) hc
    obtain ⟨r, hcs, h⟩ := bind_ok h
    obtain ⟨iends, ibs⟩ := initCases_shape _ cases _ hcs
    obtain ⟨d, hd, h⟩ := bind_ok h
    obtain ⟨idends, idbs⟩ := initDflt_shape _ dflt _ hd
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨ic.trans (ISt.Inside.of_shape (ISt.meetAll_shape σc (fun τ hτ => ?_))), fun b hb => ?_⟩
    · rcases List.mem_append.mp hτ with hτ | hτ
      · exact iends τ hτ
      · exact idends τ hτ
    · rcases List.mem_append.mp hb with hb | hb
      · exact ic.trans (ibs b hb)
      · exact ic.trans (idbs b hb)
  | σ, .for_ names coll body, out, h => by
    simp only [initStmt] at h
    obtain ⟨σc, hc, h⟩ := bind_ok h
    have ic := ISt.Inside.of_keepsShape (initE_shape sigs σ coll) hc
    obtain ⟨r, hb, h⟩ := bind_ok h
    obtain ⟨-, ibs⟩ := initBlock_shape _ body _ hb
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨ic.trans (ISt.Inside.of_shape (ISt.meetAll_shape σc (fun τ hτ => ?_))), by simp⟩
    simp only [List.mem_cons, List.mem_map] at hτ
    rcases hτ with rfl | ⟨b, hb', rfl⟩
    · rfl
    · exact ISt.back_shape ((ISt.Inside.bind σc true names).trans (ibs b hb'))

theorem initElse_shape : ∀ (σ : ISt) (e : Option (List Stmt)) (out : IOut), initElse sigs σ e = .ok out →
    σ.Inside out.1 ∧ ∀ b ∈ out.2, σ.Inside b
  | σ, some e, out, h => by simp only [initElse] at h; exact initBlock_shape σ e out h
  | σ, none, out, h => by
    simp only [initElse, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.refl σ, by simp⟩

theorem initDflt_shape : ∀ (σ : ISt) (d : Option (List Stmt)) (r : List ISt × List ISt), initDflt sigs σ d = .ok r →
    (∀ τ ∈ r.1, τ.shape = σ.shape) ∧ ∀ b ∈ r.2, σ.Inside b
  | σ, some d, r, h => by
    simp only [initDflt] at h
    obtain ⟨rb, hb, h⟩ := bind_ok h
    obtain ⟨ib, ibb⟩ := initBlock_shape σ d rb hb
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨fun τ hτ => ?_, ibb⟩
    split at hτ
    · simp at hτ; subst hτ; exact ISt.back_shape ib
    · simp at hτ
  | σ, none, r, h => by
    simp only [initDflt, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp

theorem initCases_shape : ∀ (σ : ISt) (cs : List Case) (r : List ISt × List ISt), initCases sigs σ cs = .ok r →
    (∀ τ ∈ r.1, τ.shape = σ.shape) ∧ ∀ b ∈ r.2, σ.Inside b
  | σ, [], r, h => by
    simp only [initCases, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp
  | σ, .atom _ b :: cs, r, h => by
    simp only [initCases] at h
    obtain ⟨rb, hb, h⟩ := bind_ok h
    obtain ⟨ib, ibb⟩ := initBlock_shape _ b _ hb
    obtain ⟨rc, hcs, h⟩ := bind_ok h
    obtain ⟨iends, ibs⟩ := initCases_shape _ cs _ hcs
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨fun τ hτ => ?_, fun b' hb' => ?_⟩
    · split at hτ
      · rcases List.mem_cons.mp hτ with rfl | hτ
        · exact ISt.back_shape ib
        · exact iends τ hτ
      · exact iends τ hτ
    · rcases List.mem_append.mp hb' with hb' | hb'
      · exact ibb b' hb'
      · exact ibs b' hb'
  | σ, .term _ x b :: cs, r, h => by
    simp only [initCases] at h
    obtain ⟨rb, hb, h⟩ := bind_ok h
    obtain ⟨ib, ibb⟩ := initBlock_shape _ b _ hb
    have ib' := (ISt.Inside.bind σ true [x]).trans ib
    obtain ⟨rc, hcs, h⟩ := bind_ok h
    obtain ⟨iends, ibs⟩ := initCases_shape _ cs _ hcs
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨fun τ hτ => ?_, fun b' hb' => ?_⟩
    · split at hτ
      · rcases List.mem_cons.mp hτ with rfl | hτ
        · exact ISt.back_shape ib'
        · exact iends τ hτ
      · exact iends τ hτ
    · rcases List.mem_append.mp hb' with hb' | hb'
      · exact (ISt.Inside.bind σ true [x]).trans (ibb b' hb')
      · exact ibs b' hb'

theorem initBlock_shape : ∀ (σ : ISt) (ss : List Stmt) (out : IOut), initBlock sigs σ ss = .ok out →
    σ.Inside out.1 ∧ ∀ b ∈ out.2, σ.Inside b
  | σ, [], out, h => by
    simp only [initBlock, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨ISt.Inside.refl σ, by simp⟩
  | σ, s :: ss, out, h => by
    simp only [initBlock] at h
    obtain ⟨r1, h1, h⟩ := bind_ok h
    obtain ⟨i1, ib1⟩ := initStmt_shape _ s _ h1
    obtain ⟨r2, h2, h⟩ := bind_ok h
    obtain ⟨i2, ib2⟩ := initBlock_shape _ ss _ h2
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    refine ⟨i1.trans i2, fun b hb => ?_⟩
    rcases List.mem_append.mp hb with hb | hb
    · exact ib1 b hb
    · exact i1.trans (ib2 b hb)
end

end

/-! ## Statements -/

/-- How a statement run from `ρ` may stop where the analysis let it through:
never at a variable that holds nothing; returning only with every `out`
parameter holding a value; at a `break` only where `B` says it has one, with
the variables as one of its `break`s the analysis followed has them; and at
a `break` or `continue` keeping every value `ρ` held. -/
def SStop (O : List (String × Bool)) (ρ : VEnv) (brks : List ISt) (B : Prop) : Stop → Prop
  | .uninit | .early _ => False
  | .returned _ ρ' => ExitOk O ρ'
  | .brk ρ' => B ∧ Grew ρ ρ' ∧ ∃ b ∈ brks, IAg b ρ'
  | .cont ρ' => Grew ρ ρ'
  | _ => True

/-- What a statement run from `ρ` came to: variables that keep every value
`ρ` held, with more bound, lining up with what the analysis has after it,
and `C`; or a stop as `SStop` says. -/
def SRes (O : List (String × Bool)) (ρ : VEnv) (out : IOut) (B C : Prop) : EvalM VEnv → Prop
  | .ok ρ' => Grew ρ ρ' ∧ IAg out.1 ρ' ∧ C
  | .error s => SStop O ρ out.2 B s

theorem SStop.of_IStop {O : List (String × Bool)} {ρ : VEnv} {brks : List ISt} {B : Prop} {s : Stop}
    (h : IStop O s) : SStop O ρ brks B s := by
  cases s <;> first | exact h | exact h.elim | trivial

theorem SStop.keeps {O : List (String × Bool)} {ρ ρ1 : VEnv} {brks : List ISt} {B : Prop} {s : Stop}
    (k : Keeps ρ ρ1) (h : SStop O ρ1 brks B s) : SStop O ρ brks B s := by
  cases s with
  | brk ρ' => exact ⟨h.1, (Grew.of_keeps k).trans h.2.1, h.2.2⟩
  | cont ρ' => exact (Grew.of_keeps k).trans h
  | _ => exact h

theorem SStop.weaken {O : List (String × Bool)} {ρ : VEnv} {brks brks' : List ISt} {B B' : Prop} {s : Stop}
    (h : SStop O ρ brks B s) (hb : B → B') (hl : ∀ b ∈ brks, b ∈ brks') : SStop O ρ brks' B' s := by
  cases s with
  | brk ρ' => obtain ⟨h1, h2, b, hb', h3⟩ := h; exact ⟨hb h1, h2, b, hl b hb', h3⟩
  | _ => exact h

/-- An expression run as part of a statement. -/
theorem SRes.ofRes {O : List (String × Bool)} {ρ : VEnv} {σ1 : ISt} {α} {P : α → VEnv → Prop}
    {r : Except Stop (α × VEnv)} {f : α × VEnv → EvalM VEnv} {out : IOut} {B C : Prop}
    (h : Res O ρ σ1 P r) (hf : ∀ a ρ1, Keeps ρ ρ1 → P a ρ1 → IAg σ1 ρ1 → SRes O ρ1 out B C (f (a, ρ1))) :
    SRes O ρ out B C (r >>= f) := by
  cases r with
  | error s => exact SStop.of_IStop h
  | ok p =>
    obtain ⟨a, ρ1⟩ := p
    obtain ⟨k, h1, hp⟩ := h
    have := hf a ρ1 k hp h1
    simp only [bind, Except.bind]
    generalize f (a, ρ1) = r at this ⊢
    cases r with
    | ok ρ' => exact ⟨(Grew.of_keeps k).trans this.1, this.2⟩
    | error s => exact SStop.keeps k this

theorem SRes.weaken {O : List (String × Bool)} {ρ : VEnv} {out out' : IOut} {B B' C C' : Prop} {m : EvalM VEnv}
    (h : SRes O ρ out B C m) (h1 : ∀ ρ', IAg out.1 ρ' → IAg out'.1 ρ') (hb : B → B') (hc : C → C')
    (hl : ∀ b ∈ out.2, b ∈ out'.2) : SRes O ρ out' B' C' m := by
  cases m with
  | ok ρ' => exact ⟨h.1, h1 ρ' h.2.1, hc h.2.2⟩
  | error s => exact SStop.weaken h hb hl

/-- A loop's passes, each with its condition first, where the condition and
the body run from what the loop was entered with as the analysis says: what
the loop leaves lines up with what `W` says of the ways out. -/
theorem execLoop_init {O : List (String × Bool)} {σ0 σc τ : ISt} {bs : List ISt} {W : VEnv → Prop} {C : Prop}
    {cond : VEnv → EvalM (Bool × VEnv)} {body : VEnv → EvalM VEnv} {Bb : Prop}
    (hc : ∀ ρ, IAg σ0 ρ → match cond ρ with
      | .ok (b, ρ') => Keeps ρ ρ' ∧ IAg σc ρ' ∧ (b = false → W ρ' ∧ C)
      | .error s => IStop O s)
    (hb : ∀ ρ, IAg σc ρ → SRes O ρ (τ, bs) Bb True (body ρ))
    (hbrk : ∀ ρ ρ', IAg σc ρ → Grew ρ ρ' → ∀ b ∈ bs, IAg b ρ' → Bb → W (restore ρ ρ') ∧ C)
    (hl : σc.length = σ0.length) :
    ∀ (k : Nat) (ρ : VEnv), IAg σ0 ρ → match execLoop cond body k ρ with
      | .ok ρ' => Keeps ρ ρ' ∧ W ρ' ∧ C
      | .error s => SStop O ρ [] False s
  | 0, ρ, _ => by simp only [execLoop]; trivial
  | k + 1, ρ, hρ => by
    simp only [execLoop]
    have h1 := hc ρ hρ
    generalize cond ρ = r at h1 ⊢
    cases r with
    | error s => exact SStop.of_IStop h1
    | ok p =>
      obtain ⟨b, ρ1⟩ := p
      obtain ⟨k1, hρ1, hw⟩ := h1
      simp only [bind, Except.bind]
      cases b with
      | false => exact ⟨k1, hw rfl⟩
      | true =>
        simp only [Bool.not_true, Bool.false_eq_true, ↓reduceIte]
        have h2 := hb ρ1 hρ1
        generalize body ρ1 = r2 at h2 ⊢
        have next : ∀ ρ2, Grew ρ1 ρ2 → match execLoop cond body k (restore ρ1 ρ2) with
            | .ok ρ' => Keeps ρ ρ' ∧ W ρ' ∧ C
            | .error s => SStop O ρ [] False s := by
          intro ρ2 g
          have kr := g.restore_keeps
          have := execLoop_init hc hb hbrk hl k (restore ρ1 ρ2) ((hρ.keeps k1).keeps kr)
          generalize execLoop cond body k (restore ρ1 ρ2) = r3 at this ⊢
          cases r3 with
          | ok ρ' => exact ⟨(k1.trans kr).trans this.1, this.2⟩
          | error s => exact SStop.keeps (k1.trans kr) this
        cases r2 with
        | ok ρ2 => exact next ρ2 h2.1
        | error s =>
          cases s with
          | brk ρ2 =>
            obtain ⟨hB, g, b, hbm, hb2⟩ := h2
            exact ⟨k1.trans g.restore_keeps, hbrk ρ1 ρ2 hρ1 g b hbm hb2 hB⟩
          | cont ρ2 => exact next ρ2 h2
          | returned o ρ2 => exact h2
          | uninit => exact h2.elim
          | early r => exact h2.elim
          | _ => trivial

/-- A `for`'s passes, one for each item, its names bound to the item's
values. -/
theorem execFor_init {O : List (String × Bool)} {σc τ : ISt} {bs : List ISt} {W : VEnv → Prop}
    {body : VEnv → EvalM VEnv} {names : List String} {Bb : Prop}
    (hb : ∀ ρ, IAg (σc.bind names true) ρ → SRes O ρ (τ, bs) Bb True (body ρ))
    (hend : ∀ ρ, IAg σc ρ → W ρ)
    (hbrk : ∀ ρ ρ', IAg σc ρ → Grew ρ ρ' → ∀ b ∈ bs, IAg b ρ' → W (restore ρ ρ')) :
    ∀ (items : List (List Val)) (ρ : VEnv), (∀ item ∈ items, item.length = names.length) → IAg σc ρ →
    match execFor body names items ρ with
      | .ok ρ' => Keeps ρ ρ' ∧ W ρ'
      | .error s => SStop O ρ [] False s
  | [], ρ, _, hρ => by simp only [execFor]; exact ⟨Keeps.refl ρ, hend ρ hρ⟩
  | item :: items, ρ, hlen, hρ => by
    simp only [execFor]
    have hlen1 := hlen item List.mem_cons_self
    have hb1 : IAg (σc.bind names true) (bindVals ρ ((names.zip item).map fun (x, w) => (x, some w))) := by
      have := IAg.pushed ((names.zip item).map fun (x, w) => (x, some w)) hρ (s := true)
        (fun _ q hq => by simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
      simpa [List.map_map, Function.comp_def, List.map_fst_zip, hlen1] using this
    have g0 := bindVals_grew ρ ((names.zip item).map fun (x, w) => (x, some w))
    have h2 := hb _ hb1
    generalize body (bindVals ρ ((names.zip item).map fun (x, w) => (x, some w))) = r2 at h2 ⊢
    have next : ∀ ρ2, Grew (bindVals ρ ((names.zip item).map fun (x, w) => (x, some w))) ρ2 →
        match execFor body names items (restore ρ ρ2) with
        | .ok ρ' => Keeps ρ ρ' ∧ W ρ'
        | .error s => SStop O ρ [] False s := by
      intro ρ2 g
      have g' := g0.trans g
      have kr := g'.restore_keeps
      have := execFor_init hb hend hbrk items (restore ρ ρ2)
        (fun i hi => hlen i (List.mem_cons_of_mem _ hi)) (hρ.keeps kr)
      generalize execFor body names items (restore ρ ρ2) = r3 at this ⊢
      cases r3 with
      | ok ρ' => exact ⟨kr.trans this.1, this.2⟩
      | error s => exact SStop.keeps kr this
    cases r2 with
    | ok ρ2 => exact next ρ2 h2.1
    | error s =>
      cases s with
      | brk ρ2 =>
        obtain ⟨-, g, b, hbm, hb2⟩ := h2
        have g' := g0.trans g
        exact ⟨g'.restore_keeps, hbrk ρ ρ2 hρ g' b hbm hb2⟩
      | cont ρ2 => exact next ρ2 h2
      | returned o ρ2 => exact h2
      | uninit => exact h2.elim
      | early r => exact h2.elim
      | _ => trivial

/-- The keys a place's steps index with, in order. -/
def indexKeys : List Step → List Expr
  | [] => []
  | .field _ :: rest => indexKeys rest
  | .index k _ :: rest => k :: indexKeys rest

theorem initKeys_eq (sigs : List (String × Sig)) : ∀ (σ : ISt) (steps : List Step),
    initKeys sigs σ steps = initAll sigs σ (indexKeys steps)
  | σ, [] => by simp [initKeys, initAll, indexKeys]
  | σ, .field _ :: rest => by simp only [initKeys, indexKeys]; exact initKeys_eq sigs σ rest
  | σ, .index k _ :: rest => by
    simp only [initKeys, indexKeys, initAll]
    congr 1; funext σ1; exact initKeys_eq sigs σ1 rest

theorem setPlan_keys {op : Option BinOp} : ∀ {t : Ty} {steps : List Step} {vt : Ty} {keys : List (Expr × Ty)},
    setPlan op t steps = some (vt, keys) → keys.map (·.1) = indexKeys steps
  | t, [], vt, keys, h => by simp only [setPlan, Option.some.injEq, Prod.mk.injEq] at h; obtain ⟨-, rfl⟩ := h; rfl
  | t, [.index k none], vt, keys, h => by
    simp only [setPlan] at h
    split at h
    · simp only [Option.some.injEq, Prod.mk.injEq] at h; obtain ⟨-, rfl⟩ := h; rfl
    · simp at h
  | t, .field s :: rest, vt, keys, h => by
    simp only [setPlan] at h
    split at h
    · simp only [indexKeys]; exact setPlan_keys h
    · simp at h
  | t, .index _ none :: _ :: _, vt, keys, h => by simp [setPlan] at h
  | t, .index k (some m) :: rest, vt, keys, h => by
    simp only [setPlan] at h
    split at h
    · rename_i et kt _
      cases hr : setPlan op et rest with
      | none => rw [hr] at h; simp at h
      | some q =>
        obtain ⟨vt', ks⟩ := q
        rw [hr] at h
        simp only [Option.map_some, Option.some.injEq, Prod.mk.injEq] at h
        obtain ⟨-, rfl⟩ := h
        simp [indexKeys, setPlan_keys hr]
    · simp at h

/-- Writing through steps returns early only at an index with `?` or `!`,
or at a compound operator that can fail. -/
theorem writeSteps_early' (op : Option BinOp) : ∀ (t : Ty) (v : Val) (steps : List Step) (kvs : List Val) (r w : Val),
    writeSteps op t (some v) steps kvs r = .error (.early w) → stepsExit steps = true ∨ op.any BinOp.exits = true
  | t, v, [], kvs, r, w, h => by
    unfold writeSteps at h
    split at h
    · simp [pure, Except.pure] at h
    · rename_i o
      right
      cases hx : o.exits
      · exact absurd h (binOp_noEarly hx _ _ _ w)
      · simp [hx]
  | t, v, [.index _ none], [kv], r, w, h => by
    unfold writeSteps at h
    split at h
    · exact absurd h (NoEarly.map' _ (insertKey_noEarly _ _ _) w)
    · simp [throw, throwThe, MonadExceptOf.throw] at h
    · simp_all
  | t, v, .field s :: rest, kvs, r, w, h => by
    unfold writeSteps at h
    split at h
    · rcases EvalM.bind_early h with h | ⟨part, hp, h⟩
      · exact absurd h (fieldVal_noEarly _ s w)
      rcases EvalM.bind_early h with h | ⟨w', hw, h⟩
      · rcases writeSteps_early' op _ part rest kvs r w h with h | h
        · left; simpa [stepsExit] using h
        · exact .inr h
      · exact absurd h (putField_noEarly _ s w' w)
    · simp_all
    · simp [throw, throwThe, MonadExceptOf.throw] at h
  | t, v, .index _ (some m) :: rest, kv :: kvs, r, w, h => by simp [stepsExit]
  | t, v, [.index _ none], [], r, w, h => by simp [writeSteps, throw, throwThe, MonadExceptOf.throw] at h
  | t, v, [.index _ none], _ :: _ :: _, r, w, h => by simp [writeSteps, throw, throwThe, MonadExceptOf.throw] at h
  | t, v, .index _ none :: _ :: _, kvs, r, w, h => by simp [writeSteps, throw, throwThe, MonadExceptOf.throw] at h
  | t, v, .index _ (some _) :: _, [], r, w, h => by simp [stepsExit]

theorem indexed_lengths : ∀ (vs : List Val) (items : List (List Val)), indexed vs = .ok items →
    ∀ item ∈ items, item.length = 2 := by
  intro vs items h item hi
  unfold indexed at h
  split at h
  · simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [List.mem_map] at hi
    obtain ⟨_, _, rfl⟩ := hi; rfl
  · simp [throw, throwThe, MonadExceptOf.throw] at h

/-- What a `for` walks has a value for each of its names. -/
theorem walkItems_lengths {c : Ty} {names : List String} {v : Val} {items : List (List Val)}
    (h : walkItems c names v = .ok items) : ∀ item ∈ items, item.length = names.length := by
  unfold walkItems at h
  split at h
  all_goals first
    | (simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
       intro item hi; simp only [List.mem_map] at hi; obtain ⟨_, _, rfl⟩ := hi; rfl)
    | (intro item hi; rw [indexed_lengths _ _ h item hi]; rfl)
    | (simp [throw, throwThe, MonadExceptOf.throw] at h)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallInit sigs call)
  (O : List (String × Bool)) (hmv : env.moves = false)
include hcall hmv
set_option linter.unusedSectionVars false

theorem evalKeys_init (n : Nat) (Γ : Ctx) (hs : SigsIn sigs Γ.sigs) : ∀ (keys : List (Expr × Ty)) (σ σ' : ISt),
    initAll sigs σ (keys.map (·.1)) = .ok σ' → IT O σ σ' (fun _ _ => True) (evalKeys env call n Γ keys)
  | [], σ, σ', h => by
    simp only [List.map_nil, initAll, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [evalKeys]
    exact IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)
  | (k, kt) :: keys, σ, σ', h => by
    simp only [List.map_cons, initAll] at h
    obtain ⟨σ1, h1, h2⟩ := bind_ok h
    simp only [evalKeys]
    exact IT.bind (evalC_init env sigs call hcall O hmv n Γ k kt σ σ1 hs h1) (initE_shape sigs σ k σ1 h1) (fun _ =>
      IT.bind (evalKeys_init n Γ hs keys σ1 σ' h2) (initAll_shape sigs σ1 _ σ' h2) (fun _ =>
        IT.pure' (fun ρ hρ => ⟨hρ, trivial⟩)))

theorem execBind_init (n : Nat) (Γ : Ctx) (ρ : VEnv) (p : Pat) (hint : Option Hint) (e : Expr) (σ σ1 : ISt)
    (hs : SigsIn sigs Γ.sigs) (h : initE sigs σ e = .ok σ1) (hfr : Fr O σ) (hρ : IAg σ ρ) {B C : Prop} (hC : C) :
    SRes O ρ (σ1.bind (patNames p) true, []) B C (execBind env call n Γ ρ p hint e) := by
  have fin : ∀ (a : Val) (ρ1 : VEnv), Keeps ρ ρ1 → True → IAg σ1 ρ1 →
      SRes O ρ1 (σ1.bind (patNames p) true, []) B C (do
        let vals ← patValues p a
        if vals.map (·.1) != patNames p then throw .wrong
        pure (bindVals ρ1 (vals.map fun (x, w) => (x, some w)))) := by
    intro a ρ1 _ _ hρ1
    cases hpv : patValues p a with
    | error s =>
      simp only [bind, Except.bind]
      have : s = .wrong := by
        unfold patValues at hpv; repeat' split at hpv
        all_goals simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
      subst this; trivial
    | ok vals =>
      simp only [bind, Except.bind]
      split
      · trivial
      · rename_i hn
        have hn' : vals.map (·.1) = patNames p := by simpa using hn
        refine ⟨bindVals_grew ρ1 _, ?_, hC⟩
        have := IAg.pushed (vals.map fun (x, w) => (x, some w)) hρ1 (s := true)
          (fun _ q hq => by simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
        simpa [List.map_map, Function.comp_def, hn'] using this
  unfold execBind
  cases hint with
  | none =>
    exact SRes.ofRes (evalS_init env sigs call hcall O hmv n Γ e σ σ1 hs h hfr ρ hρ) (fun a ρ1 k hp hρ1 => fin a ρ1 k hp hρ1)
  | some hh =>
    simp only
    split
    · exact SRes.ofRes (evalC_init env sigs call hcall O hmv n Γ e _ σ σ1 hs h hfr ρ hρ)
        (fun a ρ1 k hp hρ1 => fin a ρ1 k hp hρ1)
    · trivial

end

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallInit sigs call)
  (O : List (String × Bool)) (hmv : env.moves = false)
include hcall hmv
set_option linter.unusedSectionVars false

theorem execSet_init (n : Nat) (Γ : Ctx) (ρ : VEnv) (x : String) (steps : List Step) (op : Option BinOp)
    (e : Expr) (σ σ' : ISt) (hs : SigsIn sigs Γ.sigs) (h : initSet sigs σ x steps op e = .ok σ') (hfr : Fr O σ)
    (hρ : IAg σ ρ) {B C : Prop} (hC : C) : SRes O ρ (σ', []) B C (execSet env call n Γ ρ x steps op e) := by
  unfold initSet at h
  obtain ⟨σ1, h1, hA⟩ := bind_ok h
  obtain ⟨σ2, h2, hB⟩ := bind_ok hA
  obtain ⟨σ3, h3, hD⟩ := bind_ok hB
  obtain ⟨σ4, h4, hF⟩ := bind_ok hD
  simp only [pure, Except.pure, Except.ok.injEq] at hF
  have ht : σ3 = σ2 ∧ ((op.isSome = true ∨ steps ≠ []) → σ2.holds x ≠ some false) := by
    unfold initTarget at h3
    split at h3
    · obtain ⟨e3, hx⟩ := readI_eq h3; exact ⟨e3, fun _ => hx⟩
    · split at h3
      · simp [throw, throwThe, MonadExceptOf.throw] at h3
      · rename_i hn
        simp only [pure, Except.pure, Except.ok.injEq] at h3
        refine ⟨h3.symm, fun hc => ?_⟩
        rcases hc with hc | hc
        · simp at hc
        · intro hh; exact hn (by simp [hc, hh])
  obtain ⟨e3, hx⟩ := ht
  subst e3
  have hex : σ4 = σ3 ∧ ((stepsExit steps || op.any BinOp.exits) = true → σ3.outsSet = true) := by
    split at h4
    · obtain ⟨e4, ho⟩ := exitI_eq h4; exact ⟨e4, fun _ => ho⟩
    · rename_i hn
      simp only [pure, Except.pure, Except.ok.injEq] at h4
      exact ⟨h4.symm, fun hh => absurd hh hn⟩
  obtain ⟨e4, hex⟩ := hex
  subst e4
  have hs1 := initE_shape sigs σ e σ1 h1
  have hs2 := initKeys_shape sigs σ1 steps σ4 h2
  unfold execSet
  split
  · rename_i t k hl
    split
    · rename_i vt keys hp
      have hkeys : initAll sigs σ1 (keys.map (·.1)) = .ok σ4 := by
        rw [setPlan_keys hp, ← initKeys_eq]; exact h2
      have inner : ∀ Γe : Ctx, SigsIn sigs Γe.sigs → IT O σ σ4 (fun _ _ => True) (do
          let r ← evalC env call n Γe e vt
          let kvs ← evalKeys env call n Γ.key keys
          match (← get).lookup x with
          | some old => Fun.here (writeSteps op t old steps kvs r)
          | none => throw Stop.wrong) := by
        intro Γe hsΓ
        refine IT.bind (evalC_init env sigs call hcall O hmv n Γe e vt σ σ1 hsΓ h1) hs1
          (fun r => IT.bind (evalKeys_init env sigs call hcall O hmv n Γ.key hs keys σ1 σ4 hkeys) hs2 (fun kvs => ?_))
        refine IT.get' (fun ρ1 hfr1 hρ1 => ?_)
        split
        · rename_i old hold
          have hc : Calm (writeSteps op t old steps kvs r) ∧
              (∀ w, writeSteps op t old steps kvs r = .error (.early w) → σ4.outsSet = true) := by
            cases old with
            | none =>
              by_cases hq : op.isSome = true ∨ steps ≠ []
              · exact absurd hold (IAg.lookup hρ1 (hx hq))
              · simp only [not_or, Bool.not_eq_true, Option.isSome_eq_false_iff, Option.isNone_iff_eq_none,
                  ne_eq, Decidable.not_not] at hq
                obtain ⟨rfl, rfl⟩ := hq
                simp only [writeSteps]
                exact ⟨Calm.pure' _, fun w hw => by cases hw⟩
            | some v =>
              exact ⟨writeSteps_calm op t v steps kvs r, fun w hw => hex (by
                rcases writeSteps_early' op t v steps kvs r w hw with hh | hh <;> simp [hh])⟩
          exact (IT.here hc.1 hc.2 (fun _ _ _ _ => trivial)) hfr1 ρ1 hρ1
        · exact IT.wrong hfr1 ρ1 hρ1
      refine SRes.ofRes (inner _ (by cases op <;> exact hs) hfr ρ hρ) (fun w ρ' k _ hρ' => ?_)
      subst hF
      refine ⟨Grew.of_keeps (setVar_keeps x w ρ'), ?_, hC⟩
      split
      · exact IAg.give x w hρ'
      · exact hρ'.keeps (setVar_keeps x w ρ')
    · trivial
  · trivial

theorem execRet_init (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Option Expr) (σ : ISt) (out : IOut)
    (hs : SigsIn sigs Γ.sigs) (h : initStmt sigs σ (.ret e) = .ok out) (hfr : Fr O σ) (hρ : IAg σ ρ) {B C : Prop} :
    SRes O ρ out B C (execRet env call n Γ ρ e) := by
  simp only [initStmt] at h
  obtain ⟨σ1, h1, hk⟩ := bind_ok h
  obtain ⟨e1, ho⟩ := exitI_eq h1
  subst e1
  unfold execRet
  cases hr : Γ.ret with
  | ty r =>
    cases e with
    | some e' =>
      simp only at hk ⊢
      obtain ⟨σ2, h2, -⟩ := bind_ok hk
      refine SRes.ofRes (evalC_init env sigs call hcall O hmv n Γ e' r σ1 σ2 hs h2 hfr ρ hρ) (fun v ρ' k _ _ => ?_)
      exact ⟨σ1, hfr, hρ.keeps k, ho⟩
    | none => trivial
  | void =>
    cases e with
    | none => exact ⟨σ1, hfr, hρ, ho⟩
    | some _ => trivial

end

theorem SStop.grew {O : List (String × Bool)} {ρ ρ1 : VEnv} {brks : List ISt} {B : Prop} {s : Stop}
    (g : Grew ρ ρ1) (h : SStop O ρ1 brks B s) : SStop O ρ brks B s := by
  cases s with
  | brk ρ' => exact ⟨h.1, g.trans h.2.1, h.2.2⟩
  | cont ρ' => exact g.trans h
  | _ => exact h

theorem restore_restore {a b c : VEnv} (h1 : a.length ≤ b.length) (h2 : b.length ≤ c.length) :
    restore a (restore b c) = restore a c := by
  simp only [restore, List.length_drop, List.drop_drop]
  congr 1; omega

theorem Grew.length {ρ ρ' : VEnv} (g : Grew ρ ρ') : ρ.length ≤ ρ'.length := by
  obtain ⟨δ, ρ1, rfl, k⟩ := g
  simp [k.length]

theorem IAg.of_pushed {σ : ISt} {ρ : VEnv} {xs : List String} (w : Val) (h : IAg σ ρ) :
    IAg (σ.bind xs true) (bindVals ρ (xs.map (·, some w))) := by
  have := IAg.pushed (xs.map (·, some w)) h (s := true)
    (fun _ q hq => by simp only [List.mem_map] at hq; obtain ⟨_, _, rfl⟩ := hq; rfl)
  simpa [List.map_map, Function.comp_def] using this

theorem walkItems_err {c : Ty} {names : List String} {v : Val} {s : Stop} (h : walkItems c names v = .error s) :
    s = .wrong ∨ s = .unsupported .bigFor := by
  have hidx : ∀ vs, indexed vs = .error s → s = .unsupported .bigFor := by
    intro vs hi
    unfold indexed at hi
    split at hi
    · simp [pure, Except.pure] at hi
    · simp [throw, throwThe, MonadExceptOf.throw] at hi; exact hi.symm
  unfold walkItems at h
  split at h
  all_goals first
    | (exact .inr (hidx _ h))
    | (simp [throw, throwThe, MonadExceptOf.throw] at h; exact .inl h.symm)
    | (simp [pure, Except.pure] at h; done)

/-- What a `for` leaves, as a statement's result. -/
theorem SRes.ofFor {O : List (String × Bool)} {ρ : VEnv} {out : IOut} {B C : Prop} {W : VEnv → Prop}
    {m : EvalM VEnv} (h : match m with
      | .ok ρ' => Keeps ρ ρ' ∧ W ρ'
      | .error s => SStop O ρ [] False s) (hw : ∀ ρ', W ρ' → IAg out.1 ρ') (hc : C) :
    SRes O ρ out B C m := by
  cases m with
  | ok ρ' => exact ⟨Grew.of_keeps h.1, hw ρ' h.2, hc⟩
  | error s => exact SStop.weaken h (fun e => e.elim) (fun _ hb => by simp at hb)

/-- What a loop leaves, as a statement's result. -/
theorem SRes.ofLoop {O : List (String × Bool)} {ρ : VEnv} {out : IOut} {B C : Prop} {W : VEnv → Prop}
    {m : EvalM VEnv} (h : match m with
      | .ok ρ' => Keeps ρ ρ' ∧ W ρ' ∧ C
      | .error s => SStop O ρ [] False s) (hw : ∀ ρ', W ρ' → IAg out.1 ρ') (_hnil : out.2 = []) :
    SRes O ρ out B C m := by
  cases m with
  | ok ρ' => exact ⟨Grew.of_keeps h.1, hw ρ' h.2.1, h.2.2⟩
  | error s => exact SStop.weaken h (fun e => e.elim) (fun _ hb => by simp at hb)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (call : CallFn) (hcall : CallInit sigs call)
  (O : List (String × Bool)) (hmv : env.moves = false)
include hcall hmv
set_option linter.unusedSectionVars false

mutual

theorem execStmt_init : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (s : Stmt) (σ : ISt) (out : IOut), SigsIn sigs Γ.sigs →
    initStmt sigs σ s = .ok out → Fr O σ → IAg σ ρ →
    SRes O ρ out (hasBreakS s = true) (completesS s = true) (execStmt env call n k Γ ρ s)
  | 0, _, _, _, _, _, _, _, _, _, _ => by simp only [execStmt]; trivial
  | n + 1, k, Γ, ρ, s, σ, out, hs, h, hfr, hρ => by
    have hl0 := IAg.length hρ
    cases s
    case let_ p hint e =>
      simp only [initStmt] at h
      obtain ⟨σ1, h1, hA⟩ := bind_ok h
      simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
      simp only [execStmt]
      exact execBind_init env sigs call hcall O hmv n Γ ρ p hint e σ σ1 hs h1 hfr hρ rfl
    case var_ p hint v =>
      cases v with
      | some e =>
        simp only [initStmt] at h
        obtain ⟨σ1, h1, hA⟩ := bind_ok h
        simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
        simp only [execStmt]
        exact execBind_init env sigs call hcall O hmv n Γ ρ p hint e σ σ1 hs h1 hfr hρ rfl
      | none =>
        cases hint with
        | none =>
          simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
          simp only [execStmt]
          exact ⟨Grew.refl ρ, hρ, rfl⟩
        | some hh =>
          simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
          simp only [execStmt]
          split
          · split
            · rename_i Δ hΔ
              refine ⟨bindVals_grew ρ _, ?_, rfl⟩
              have := IAg.pushed (s := false) (Δ.map fun (x, _) => (x, none)) hρ (fun e => by cases e)
              simpa [List.map_map, Function.comp_def, patBindings_names hΔ] using this
            · trivial
          · trivial
    case set_ x steps op e =>
      simp only [initStmt] at h
      obtain ⟨σ1, h1, hA⟩ := bind_ok h
      simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
      simp only [execStmt]
      exact execSet_init env sigs call hcall O hmv n Γ ρ x steps op e σ σ1 hs h1 hfr hρ rfl
    case fun_ f =>
      simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
      simp only [execStmt]
      exact ⟨Grew.refl ρ, hρ, rfl⟩
    case ret e =>
      simp only [execStmt]
      exact execRet_init env sigs call hcall O hmv n Γ ρ e σ out hs h hfr hρ
    case brk =>
      simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
      simp only [execStmt]
      exact ⟨rfl, Grew.refl ρ, σ, List.mem_singleton_self σ, hρ⟩
    case cont =>
      simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
      simp only [execStmt]
      exact Grew.refl ρ
    case debuglog e =>
      simp only [initStmt] at h
      obtain ⟨σ1, h1, hA⟩ := bind_ok h
      simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
      simp only [execStmt]
      exact SRes.ofRes (evalS_init env sigs call hcall O hmv n Γ.borrow e σ σ1 hs h1 hfr ρ hρ)
        (fun _ ρ1 _ _ h1 => ⟨Grew.refl ρ1, h1, rfl⟩)
    case call e =>
      simp only [initStmt] at h
      obtain ⟨σ1, h1, hA⟩ := bind_ok h
      simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
      simp only [execStmt]
      exact SRes.ofRes (evalS_init env sigs call hcall O hmv n Γ e σ σ1 hs h1 hfr ρ hρ)
        (fun _ ρ1 _ _ h1 => ⟨Grew.refl ρ1, h1, rfl⟩)
    case const_ x hint e =>
      simp only [initStmt] at h
      obtain ⟨σ1, h1, hA⟩ := bind_ok h
      simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
      simp only [execStmt]
      exact execBind_init env sigs call hcall O hmv n { Γ with constExpr := true } ρ (.name x) hint e σ σ1 hs h1
        hfr hρ rfl
    case type_ x hh =>
      simp only [initStmt, pure, Except.pure, Except.ok.injEq] at h; subst h
      simp only [execStmt]
      exact ⟨Grew.refl ρ, hρ, rfl⟩
    case if_ c tB eB t e =>
      simp only [initStmt] at h
      obtain ⟨σc, hc, hA⟩ := bind_ok h
      obtain ⟨rt, ht, hB⟩ := bind_ok hA
      obtain ⟨re, he, hD⟩ := bind_ok hB
      simp only [pure, Except.pure, Except.ok.injEq] at hD; subst hD
      have ic := ISt.Inside.of_keepsShape (initE_shape sigs σ c) hc
      have hfc := hfr.inside ic
      obtain ⟨it, ibt⟩ := initBlock_shape sigs _ t _ ht
      have it' := (ISt.Inside.bind σc true tB.toList).trans it
      obtain ⟨ie, ibe⟩ := initElse_shape sigs _ e _ he
      have ie' := (ISt.Inside.bind σc true eB.toList).trans ie
      have shT := ISt.back_shape it'
      have shE := ISt.back_shape ie'
      have hsc := initE_shape sigs σ c σc hc
      have hlc : σc.length = σ.length := by rw [← ISt.shape_length, hsc, ISt.shape_length]
      have meetT : ∀ ρ', IAg (σc.back rt.1) ρ' → IAg ((σc.back rt.1).meet (σc.back re.1)) ρ' :=
        fun ρ' h => IAg.meet_left h (shE.trans shT.symm)
      have meetE : ∀ ρ', IAg (σc.back re.1) ρ' → IAg ((σc.back rt.1).meet (σc.back re.1)) ρ' :=
        fun ρ' h => IAg.meet_right h (shE.trans shT.symm)
      -- The `then` body, run on the condition's variables with its name bound.
      have thenCase : ∀ (Γt : Ctx) (ρ1 : VEnv) (w : Val), SigsIn sigs Γt.sigs → IAg σc ρ1 →
          SRes O ρ1 ((σc.back rt.1).meet (σc.back re.1), rt.2 ++ re.2) (hasBreakS (.if_ c tB eB t e) = true)
            (completesS (.if_ c tB eB t e) = true)
            (execBlock env call n k Γt (bindVals ρ1 (tB.toList.map (·, some w))) t >>= fun ρ' =>
              pure (restore ρ1 ρ')) := by
        intro Γt ρ1 w hst hρ1
        have hb := IAg.of_pushed (xs := tB.toList) w hρ1
        have g0 := bindVals_grew ρ1 (tB.toList.map (·, some w))
        have := execBlock_init n k Γt _ t _ rt hst ht (hfc.inside (ISt.Inside.bind σc true _)) hb
        generalize execBlock env call n k Γt _ t = m at this ⊢
        cases m with
        | ok ρ2 =>
          obtain ⟨g, h2, hct⟩ := this
          refine ⟨Grew.of_keeps (g0.trans g).restore_keeps, meetT _ (IAg.back h2 (by rw [IAg.length hρ1])), ?_⟩
          cases e <;> simp [completesS, hct]
        | error s' =>
          exact SStop.grew g0 (SStop.weaken this (fun hb => by simp [hasBreakS, hb])
            (fun b hb => List.mem_append_left _ hb))
      -- The `else`, with no name bound.
      have elseCase0 : eB = none → ∀ (ρ1 : VEnv), IAg σc ρ1 →
          SRes O ρ1 ((σc.back rt.1).meet (σc.back re.1), rt.2 ++ re.2) (hasBreakS (.if_ c tB eB t e) = true)
            (completesS (.if_ c tB eB t e) = true) (execElse env call n k Γ ρ1 e) := by
        intro heB ρ1 hρ1
        subst heB
        cases e with
        | none =>
          simp only [initElse, pure, Except.pure, Except.ok.injEq] at he; subst he
          simp only [execElse]
          refine ⟨Grew.refl ρ1, meetE _ ?_, by simp [completesS]⟩
          simpa [ISt.back, Option.toList, ISt.bind] using hρ1
        | some e' =>
          simp only [initElse] at he
          simp only [execElse]
          have := execBlock_init n k Γ ρ1 e' σc re hs (by simpa [Option.toList, ISt.bind] using he) hfc hρ1
          generalize execBlock env call n k Γ ρ1 e' = m at this ⊢
          cases m with
          | ok ρ2 =>
            obtain ⟨g, h2, hce⟩ := this
            exact ⟨Grew.of_keeps g.restore_keeps,
              meetE _ (by simpa [Option.toList, ISt.bind] using IAg.back h2 (by rw [IAg.length hρ1])),
              by simp [completesS, hce]⟩
          | error s' =>
            exact SStop.weaken this (fun hb => by simp [hasBreakS, hasBreakElse, hb])
              (fun b hb => List.mem_append_right _ hb)
      -- The `else`, with the error bound.
      have elseCase1 : ∀ (Γe : Ctx) (ρ1 : VEnv) (w : Val), SigsIn sigs Γe.sigs → IAg σc ρ1 →
          SRes O ρ1 ((σc.back rt.1).meet (σc.back re.1), rt.2 ++ re.2) (hasBreakS (.if_ c tB eB t e) = true)
            (completesS (.if_ c tB eB t e) = true)
            (execElse env call n k Γe (bindVals ρ1 (eB.toList.map (·, some w))) e >>= fun ρ' =>
              pure (restore ρ1 ρ')) := by
        intro Γe ρ1 w hse hρ1
        have hb := IAg.of_pushed (xs := eB.toList) w hρ1
        have g0 := bindVals_grew ρ1 (eB.toList.map (·, some w))
        cases e with
        | none =>
          simp only [initElse, pure, Except.pure, Except.ok.injEq] at he; subst he
          simp only [execElse, pure_bind]
          exact ⟨Grew.of_keeps g0.restore_keeps, meetE _ (IAg.back hb (by rw [IAg.length hρ1])),
            by simp [completesS]⟩
        | some e' =>
          simp only [initElse] at he
          simp only [execElse]
          have := execBlock_init n k Γe _ e' _ re hse he (hfc.inside (ISt.Inside.bind σc true _)) hb
          generalize execBlock env call n k Γe _ e' = m at this ⊢
          cases m with
          | ok ρ2 =>
            obtain ⟨g, h2, hce⟩ := this
            simp only [bind, Except.bind, pure, Except.pure]
            have hlb := g0.length
            have hl2 := g.length
            refine ⟨Grew.of_keeps ?_, ?_, by simp [completesS, hce]⟩
            · rw [restore_restore hlb hl2]; exact (g0.trans g).restore_keeps
            · rw [restore_restore hlb hl2]; exact meetE _ (IAg.back h2 (by rw [IAg.length hρ1]))
          | error s' =>
            exact SStop.grew g0 (SStop.weaken this (fun hb => by simp [hasBreakS, hasBreakElse, hb])
              (fun b hb => List.mem_append_right _ hb))
      cases tB <;> cases eB
      case none.none =>
        simp only [execStmt]
        refine SRes.ofRes (evalC_init env sigs call hcall O hmv n Γ c .bool σ σc hs hc hfr ρ hρ) (fun cv ρ1 _ _ hρ1 => ?_)
        cases hb : boolOf cv with
        | error s' =>
          have : s' = .wrong := by
            unfold boolOf at hb; split at hb <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
          subst this
          simp only [bind, Except.bind]; trivial
        | ok b =>
          simp only [bind, Except.bind]
          cases b with
          | true =>
            simp only [↓reduceIte]
            exact thenCase Γ ρ1 (.bool true) hs hρ1
          | false =>
            simp only [Bool.false_eq_true, ↓reduceIte]
            exact elseCase0 rfl ρ1 hρ1
      all_goals
        simp only [execStmt]
        cases hct : typeOf (synthF env n Γ c) with
        | error s' =>
          have : s' = .wrong := by unfold typeOf at hct; split at hct <;> simp_all [pure, Except.pure, throw,
            throwThe, MonadExceptOf.throw]
          subst this; trivial
        | ok ct =>
          simp only [bind, Except.bind]
          refine SRes.ofRes (evalS_init env sigs call hcall O hmv n Γ c σ σc hs hc hfr ρ hρ) (fun v ρ1 _ _ hρ1 => ?_)
          split
          · exact thenCase _ ρ1 _ (by simp [hs]) hρ1
          · split
            · trivial
            · exact elseCase0 (by simp_all) ρ1 hρ1
          · exact thenCase _ ρ1 _ (by simp [hs]) hρ1
          · exact elseCase1 _ ρ1 _ (by simp [hs]) hρ1
          · trivial
    case loop c body =>
      simp only [initStmt] at h
      obtain ⟨σc, hc, hA⟩ := bind_ok h
      obtain ⟨r, hr, hB⟩ := bind_ok hA
      simp only [pure, Except.pure, Except.ok.injEq] at hB; subst hB
      have ic : σ.Inside σc := by
        cases c with
        | none => simp only [pure, Except.pure, Except.ok.injEq] at hc; subst hc; exact ISt.Inside.refl _
        | some c => exact ISt.Inside.of_keepsShape (initE_shape sigs σ c) hc
      have hfc := hfr.inside ic
      obtain ⟨-, ibs⟩ := initBlock_shape sigs σc body r hr
      have hsc : σc.shape = σ.shape := by
        cases c with
        | none => simp only [pure, Except.pure, Except.ok.injEq] at hc; subst hc; rfl
        | some c => exact initE_shape sigs σ c σc hc
      have hlc : σc.length = σ.length := by rw [← ISt.shape_length, hsc, ISt.shape_length]
      have shapes : ∀ τ ∈ (if c.isSome = true then [σc] else []) ++ r.2.map σc.back, τ.shape = σc.shape := by
        intro τ hτ
        simp only [List.mem_append, List.mem_map] at hτ
        rcases hτ with hτ | ⟨b, hb, rfl⟩
        · split at hτ
          · simp at hτ; subst hτ; rfl
          · simp at hτ
        · exact ISt.back_shape (ibs b hb)
      simp only [execStmt]
      refine SRes.ofLoop (execLoop_init (O := O) (σ0 := σ) (σc := σc) (τ := r.1) (bs := r.2)
        (W := fun ρ' => IAg (σc.meetAll ((if c.isSome = true then [σc] else []) ++ r.2.map σc.back)) ρ')
        (C := completesS (.loop c body) = true) (Bb := hasBreak body = true)
        ?hc ?hb ?hbrk hlc k ρ hρ) (fun _ h => h) rfl
      case hc =>
        intro ρ0 hρ0
        cases c with
        | none =>
          simp only [pure, Except.pure, Except.ok.injEq] at hc; subst hc
          exact ⟨Keeps.refl ρ0, hρ0, fun e => by cases e⟩
        | some c =>
          simp only
          have := evalC_init env sigs call hcall O hmv n Γ c .bool σ σc hs hc hfr ρ0 hρ0
          generalize (evalC env call n Γ c .bool).run ρ0 = r0 at this ⊢
          cases r0 with
          | error s => exact this
          | ok q =>
            obtain ⟨cv, ρ1⟩ := q
            obtain ⟨k1, h1, -⟩ := this
            simp only [bind, Except.bind]
            cases hb : boolOf cv with
            | error s =>
              simp only
              have : s = .wrong := by
                unfold boolOf at hb; split at hb <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
              subst this; trivial
            | ok b =>
              simp only [pure, Except.pure]
              exact ⟨k1, h1, fun _ => ⟨IAg.meetAll (τ := σc) (by simp) h1 shapes, rfl⟩⟩
      case hb =>
        exact fun ρ0 hρ0 => (execBlock_init n k { Γ with inLoop := true } ρ0 body σc r hs hr hfc hρ0).weaken
          (fun _ h => h) id (fun _ => trivial) (fun _ h => h)
      case hbrk =>
        exact fun ρ0 ρ' hρ0 g b hb hb' hB => ⟨IAg.meetAll (τ := σc.back b)
            (by simp only [List.mem_append, List.mem_map]; exact .inr ⟨b, hb, rfl⟩)
            (IAg.back hb' (by rw [IAg.length hρ0])) shapes,
          by cases c <;> simp [completesS, hB]⟩
    case match_ e cs dflt =>
      simp only [initStmt] at h
      obtain ⟨σc, hc, hA⟩ := bind_ok h
      obtain ⟨r, hr, hB⟩ := bind_ok hA
      obtain ⟨d, hd, hD⟩ := bind_ok hB
      simp only [pure, Except.pure, Except.ok.injEq] at hD; subst hD
      have hsc := initE_shape sigs σ e σc hc
      have hfc := hfr.inside (ISt.Inside.of_shape hsc)
      obtain ⟨iends, ibs⟩ := initCases_shape sigs σc cs r hr
      obtain ⟨idends, idbs⟩ := initDflt_shape sigs σc dflt d hd
      have shapes : ∀ τ ∈ r.1 ++ d.1, τ.shape = σc.shape := fun τ hτ => by
        rcases List.mem_append.mp hτ with h' | h'
        · exact iends τ h'
        · exact idends τ h'
      simp only [execStmt]
      cases hct : typeOf (synthF env n Γ e) with
      | error s' =>
        have : s' = .wrong := by
          unfold typeOf at hct; split at hct <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
        subst this; trivial
      | ok t =>
        simp only [bind, Except.bind]
        refine SRes.ofRes (evalS_init env sigs call hcall O hmv n Γ e σ σc hs hc hfr ρ hρ) (fun v ρ1 _ _ hρ1 => ?_)
        have hl1 : σc.length = ρ1.length := IAg.length hρ1
        split
        · rename_i vs _
          split
          · rename_i a _
            have := execCases_init n k Γ ρ1 vs.toList a v cs σc r hs hr hfc hρ1
            generalize execCases env call n k Γ ρ1 vs.toList a v cs = m at this ⊢
            cases m with
            | ok o =>
              cases o with
              | some ρ' =>
                obtain ⟨g, ⟨τ, hτ, hτρ⟩, hcc⟩ := this
                exact ⟨Grew.of_keeps g.restore_keeps, IAg.meetAll (List.mem_append_left _ hτ) hτρ shapes,
                  by simp [completesS, hcc]⟩
              | none =>
                cases dflt with
                | none => trivial
                | some dd =>
                  simp only [initDflt] at hd
                  obtain ⟨rb, hrb, hE⟩ := bind_ok hd
                  simp only [pure, Except.pure, Except.ok.injEq] at hE; subst hE
                  obtain ⟨ib, -⟩ := initBlock_shape sigs σc dd rb hrb
                  simp only [Option.isNone_some, Bool.false_eq_true, ↓reduceIte, execElse]
                  have := execBlock_init n k Γ ρ1 dd σc rb hs hrb hfc hρ1
                  generalize execBlock env call n k Γ ρ1 dd = m2 at this ⊢
                  cases m2 with
                  | ok ρ2 =>
                    obtain ⟨g, h2, hcd⟩ := this
                    refine ⟨Grew.of_keeps g.restore_keeps,
                      IAg.meetAll (τ := σc.back rb.1) (List.mem_append_right _ (by simp [hcd]))
                        (IAg.back h2 hl1) shapes, by simp [completesS, completesElse, hcd]⟩
                  | error s' =>
                    exact SStop.weaken this (fun hb => by simp [hasBreakS, hasBreakElse, hb])
                      (fun b hb => List.mem_append_right _ hb)
            | error s' =>
              exact SStop.weaken this (fun hb => by simp [hasBreakS, hb]) (fun b hb => List.mem_append_left _ hb)
          · trivial
        · trivial
    case for_ names coll body =>
      simp only [initStmt] at h
      obtain ⟨σc, hc, hA⟩ := bind_ok h
      obtain ⟨r, hr, hB⟩ := bind_ok hA
      simp only [pure, Except.pure, Except.ok.injEq] at hB; subst hB
      have hsc := initE_shape sigs σ coll σc hc
      have hfc := hfr.inside (ISt.Inside.of_shape hsc)
      obtain ⟨-, ibs⟩ := initBlock_shape sigs _ body r hr
      have shapes : ∀ τ ∈ σc :: r.2.map σc.back, τ.shape = σc.shape := by
        intro τ hτ
        simp only [List.mem_cons, List.mem_map] at hτ
        rcases hτ with rfl | ⟨b, hb, rfl⟩
        · rfl
        · exact ISt.back_shape ((ISt.Inside.bind σc true names).trans (ibs b hb))
      simp only [execStmt]
      split
      · rename_i Δ _
        cases hct : typeOf (synthF env n Γ.borrow coll) with
        | error s' =>
          have : s' = .wrong := by
            unfold typeOf at hct; split at hct <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
          subst this; trivial
        | ok t =>
          simp only [bind, Except.bind]
          refine SRes.ofRes (evalS_init env sigs call hcall O hmv n Γ.borrow coll σ σc hs hc hfr ρ hρ)
            (fun v ρ1 _ _ hρ1 => ?_)
          cases hw : walkItems t.expose names v with
          | error s' =>
            rcases walkItems_err hw with rfl | rfl <;> trivial
          | ok items =>
            simp only
            exact SRes.ofFor (execFor_init (O := O) (σc := σc) (τ := r.1) (bs := r.2) (names := names)
              (W := fun ρ' => IAg (σc.meetAll (σc :: r.2.map σc.back)) ρ')
              (fun ρ0 hρ0 => (execBlock_init n k _ ρ0 body _ r (by rw [bindAll_eq, bindAllAs_sigs]; exact hs) hr
                (hfc.inside (ISt.Inside.bind σc true names)) hρ0).weaken (fun _ h => h) id (fun _ => trivial)
                (fun _ h => h))
              (fun ρ0 hρ0 => IAg.meetAll (τ := σc) (by simp) hρ0 shapes)
              (fun ρ0 ρ' hρ0 g b hb hb' => IAg.meetAll (τ := σc.back b)
                (by simp only [List.mem_cons, List.mem_map]; exact .inr ⟨b, hb, rfl⟩)
                (IAg.back hb' (by rw [IAg.length hρ0])) shapes)
              items ρ1 (walkItems_lengths hw) hρ1) (fun _ h => h) rfl
      · trivial

theorem execCases_init : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (vs : List (String × Option Ty)) (a : String)
    (v : Val) (cs : List Case) (σ : ISt) (r : List ISt × List ISt), SigsIn sigs Γ.sigs →
    initCases sigs σ cs = .ok r → Fr O σ → IAg σ ρ →
    match execCases env call n k Γ ρ vs a v cs with
    | .ok (some ρ') => Grew ρ ρ' ∧ (∃ τ ∈ r.1, IAg τ (restore ρ ρ')) ∧ casesComplete cs = true
    | .ok none => True
    | .error s => SStop O ρ r.2 (casesBreak cs = true) s
  | 0, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp only [execCases]; trivial
  | n + 1, k, Γ, ρ, vs, a, v, [], σ, r, hs, h, hfr, hρ => by simp only [execCases]; trivial
  | n + 1, k, Γ, ρ, vs, a, v, c :: cs, σ, r, hs, h, hfr, hρ => by
    have hl : σ.length = ρ.length := IAg.length hρ
    -- The cases after this one, from the same variables.
    have later : ∀ (rb : IOut) (b : List Stmt) (rc : List ISt × List ISt) (τ : ISt),
        initCases sigs σ cs = .ok rc → r = (if completes b then τ :: rc.1 else rc.1, rb.2 ++ rc.2) →
        (completes b = true ∨ casesComplete cs = true → casesComplete (c :: cs) = true) →
        (casesBreak cs = true → casesBreak (c :: cs) = true) →
        match execCases env call n k Γ ρ vs a v cs with
        | .ok (some ρ') => Grew ρ ρ' ∧ (∃ τ ∈ r.1, IAg τ (restore ρ ρ')) ∧ casesComplete (c :: cs) = true
        | .ok none => True
        | .error s => SStop O ρ r.2 (casesBreak (c :: cs) = true) s := by
      intro rb b rc τ hcs hr hcc hcb
      have := execCases_init n k Γ ρ vs a v cs σ rc hs hcs hfr hρ
      subst hr
      generalize execCases env call n k Γ ρ vs a v cs = m at this ⊢
      cases m with
      | ok o =>
        cases o with
        | some ρ' =>
          obtain ⟨g, ⟨τ', hτ', h'⟩, hc'⟩ := this
          refine ⟨g, ⟨τ', ?_, h'⟩, hcc (.inr hc')⟩
          split
          · exact List.mem_cons_of_mem _ hτ'
          · exact hτ'
        | none => trivial
      | error s => exact SStop.weaken this hcb (fun x hx => List.mem_append_right _ hx)
    have hfin : ∀ (rb : IOut) (b : List Stmt) (rc : List ISt × List ISt) (ρb : VEnv) (m : EvalM VEnv),
        r = (if completes b then σ.back rb.1 :: rc.1 else rc.1, rb.2 ++ rc.2) → Grew ρ ρb →
        (completes b = true → casesComplete (c :: cs) = true) →
        (hasBreak b = true → casesBreak (c :: cs) = true) →
        SRes O ρb rb (hasBreak b = true) (completes b = true) m →
        match (some <$> m : EvalM (Option VEnv)) with
        | .ok (some ρ') => Grew ρ ρ' ∧ (∃ τ ∈ r.1, IAg τ (restore ρ ρ')) ∧ casesComplete (c :: cs) = true
        | .ok none => True
        | .error s => SStop O ρ r.2 (casesBreak (c :: cs) = true) s := by
      intro rb b rc ρb m hr g0 hcc hcb hm
      subst hr
      cases m with
      | ok ρ' =>
        obtain ⟨g, h2, hcb'⟩ := hm
        simp only [Functor.map, Except.map]
        refine ⟨g0.trans g, ⟨σ.back rb.1, by simp [hcb'], ?_⟩, hcc hcb'⟩
        exact IAg.back h2 hl
      | error s' =>
        simp only [Functor.map, Except.map]
        exact SStop.grew g0 (SStop.weaken hm hcb (fun x hx => List.mem_append_left _ hx))
    cases c with
    | atom nm b =>
      simp only [initCases] at h
      obtain ⟨rb, hb, hA⟩ := bind_ok h
      obtain ⟨rc, hcs, hB⟩ := bind_ok hA
      simp only [pure, Except.pure, Except.ok.injEq] at hB
      rw [execCases.eq_def]; dsimp only
      by_cases hn : nm = a
      · subst hn
        simp only [Case.name, beq_self_eq_true, ↓reduceIte]
        exact hfin rb b rc ρ _ hB.symm (Grew.refl ρ) (fun hc => by simp [casesComplete, hc])
          (fun hc => by simp [casesBreak, hc]) (execBlock_init n k Γ ρ b σ rb hs hb hfr hρ)
      · simp only [Case.name, show (nm == a) = false by simpa using hn, Bool.false_eq_true, ↓reduceIte]
        exact later rb b rc (σ.back rb.1) hcs hB.symm
          (fun hc => by rcases hc with hc | hc <;> simp [casesComplete, hc])
          (fun hc => by simp [casesBreak, hc])
    | term nm x b =>
      simp only [initCases] at h
      obtain ⟨rb, hb, hA⟩ := bind_ok h
      obtain ⟨rc, hcs, hB⟩ := bind_ok hA
      simp only [pure, Except.pure, Except.ok.injEq] at hB
      rw [execCases.eq_def]; dsimp only
      by_cases hn : nm = a
      · clear later
        subst hn
        simp only [Case.name, beq_self_eq_true, ↓reduceIte]
        cases v
        case term a' w =>
          simp only
          cases hlk : vs.lookup nm with
          | none => trivial
          | some o =>
            cases o with
            | none => trivial
            | some p =>
              simp only
              have hρb : IAg (σ.bind [x] true) ((x, some w) :: ρ) := .cons (fun _ => rfl) hρ
              have g0 : Grew ρ ((x, some w) :: ρ) := ⟨[(x, some w)], ρ, rfl, Keeps.refl ρ⟩
              exact hfin rb b rc _ _ hB.symm g0 (fun hc => by simp [casesComplete, hc])
                (fun hc => by simp [casesBreak, hc])
                (execBlock_init n k (Γ.bind x p .let_) _ b _ rb hs hb (hfr.inside (ISt.Inside.bind σ true [x])) hρb)
        all_goals trivial
      · simp only [Case.name, show (nm == a) = false by simpa using hn, Bool.false_eq_true, ↓reduceIte]
        exact later rb b rc (σ.back rb.1) hcs hB.symm
          (fun hc => by rcases hc with hc | hc <;> simp [casesComplete, hc])
          (fun hc => by simp [casesBreak, hc])

theorem execBlock_init : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (ss : List Stmt) (σ : ISt) (out : IOut), SigsIn sigs Γ.sigs →
    initBlock sigs σ ss = .ok out → Fr O σ → IAg σ ρ →
    SRes O ρ out (hasBreak ss = true) (completes ss = true) (execBlock env call n k Γ ρ ss)
  | 0, _, _, _, _, _, _, _, _, _, _ => by simp only [execBlock]; trivial
  | _ + 1, _, _, ρ, [], σ, out, _, h, _, hρ => by
    simp only [initBlock, pure, Except.pure, Except.ok.injEq] at h; subst h
    simp only [execBlock]
    exact ⟨Grew.refl ρ, hρ, rfl⟩
  | n + 1, k, Γ, ρ, s :: ss, σ, out, hs, h, hfr, hρ => by
    simp only [initBlock] at h
    obtain ⟨r1, h1, hA⟩ := bind_ok h
    obtain ⟨r2, h2, hB⟩ := bind_ok hA
    simp only [pure, Except.pure, Except.ok.injEq] at hB; subst hB
    obtain ⟨i1, -⟩ := initStmt_shape sigs σ s r1 h1
    have := execStmt_init n k Γ ρ s σ r1 hs h1 hfr hρ
    simp only [execBlock]
    generalize execStmt env call n k Γ ρ s = m at this ⊢
    cases m with
    | error s' =>
      exact SStop.weaken this (fun hb => by simp [hasBreak, hb]) (fun b hb => List.mem_append_left _ hb)
    | ok ρ' =>
      obtain ⟨g, h1', hc1⟩ := this
      have := execBlock_init n k _ ρ' ss r1.1 r2 (hs.of_eq (checkStmtF_sigs env n Γ s #[])) h2 (hfr.inside i1) h1'
      simp only [bind, Except.bind]
      generalize execBlock env call n k _ ρ' ss = m2 at this ⊢
      cases m2 with
      | ok ρ'' => exact ⟨g.trans this.1, this.2.1, by simp [completes, hc1, this.2.2]⟩
      | error s' =>
        exact SStop.grew g (SStop.weaken this (fun hb => by simp [hasBreak, hb])
          (fun b hb => List.mem_append_right _ hb))

end

end

/-! ## Calls -/

theorem IAg.append : ∀ {A B : ISt} {C D : VEnv}, IAg A C → IAg B D → IAg (A ++ B) (C ++ D)
  | _, _, _, _, .nil, h => h
  | _, _, _, _, .cons h a, hb => .cons h (IAg.append a hb)

theorem IAg.reverse : ∀ {A : ISt} {C : VEnv}, IAg A C → IAg A.reverse C.reverse
  | _, _, .nil => .nil
  | _, _, .cons h a => by
    simp only [List.reverse_cons]
    exact IAg.append (IAg.reverse a) (.cons h .nil)

theorem IAg.take : ∀ {A : ISt} {C : VEnv} (n : Nat), IAg A C → IAg (A.take n) (C.take n)
  | _, _, 0, _ => by simp; exact .nil
  | _, _, _ + 1, .nil => .nil
  | _, _, n + 1, .cons h a => .cons h (IAg.take n a)

/-- What a function's body starts with lines up with the variables a call
binds: each parameter but an `out` one has a value, and so has each const. -/
theorem IAg.start : ∀ (params : List (Mode × String × Hint)) (vals : List (Option Val)),
    vals.length = params.length → (∀ q ∈ vals.zip (params.map (·.1)), q.2 ≠ .out → q.1.isSome = true) →
    IAg (params.map fun (m, x, _) => (x, m != .out, m == .out || m == .mut))
      ((params.zip vals).map fun ((_, x, _), v) => (x, v))
  | [], [], _, _ => .nil
  | (m, x, h) :: params, v :: vals, hl, hv => by
    simp only [List.map_cons, List.zip_cons_cons]
    refine .cons (fun hm => hv (v, m) (by simp) (by simpa using hm)) (IAg.start params vals (by simpa using hl)
      (fun q hq => hv q (by simp [hq])))
  | [], _ :: _, hl, _ => by simp at hl
  | _ :: _, [], hl, _ => by simp at hl

theorem IAg.consts : ∀ (consts : List String) (cvals : VEnv), cvals.map (·.1) = consts →
    (∀ q ∈ cvals, q.2.isSome = true) → IAg ((consts.map (·, true)).map fun (x, s) => (x, s, false)) cvals
  | [], [], _, _ => .nil
  | x :: consts, (y, o) :: cvals, hn, hs => by
    simp only [List.map_cons, List.cons.injEq] at hn
    obtain ⟨rfl, hn⟩ := hn
    exact .cons (fun _ => hs (y, o) List.mem_cons_self) (IAg.consts consts cvals hn
      (fun q hq => hs q (List.mem_cons_of_mem _ hq)))
  | [], _ :: _, hn, _ => by simp at hn
  | _ :: _, [], hn, _ => by simp at hn

/-- Where every `mut` and `out` parameter holds a value, so does what each
came to, as a call reads them back. -/
theorem outs_after : ∀ (A : ISt) (B : VEnv) (ps : List (Mode × Ty)), IAg A B → A.outsSet = true →
    A.map (·.2.2) = ps.map (fun p => p.1 == .out || p.1 == .mut) →
    ∀ q ∈ (B.map (·.2)).zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true
  | [], [], [], _, _, _, _, hq, _ => by simp at hq
  | (x, s, w) :: A, (_, o) :: B, (m, t) :: ps, .cons hs a, ho, hw, q, hq, hm => by
    simp only [ISt.outsSet, List.all_cons, Bool.and_eq_true] at ho
    simp only [List.map_cons, List.cons.injEq] at hw
    simp only [List.map_cons, List.zip_cons_cons, List.mem_cons] at hq
    rcases hq with rfl | hq
    · apply hs
      have : w = true := by
        rw [hw.1]; rcases hm with hm | hm <;> simp only at hm <;> simp [hm]
      simpa [this] using ho.1
    · exact outs_after A B ps a ho.2 hw.2 q hq hm
  | [], [], _ :: _, _, _, hw, _, _, _ => by simp at hw
  | _ :: _, _, [], _, _, hw, _, _, _ => by simp at hw
  | _ :: _, [], _ :: _, a, _, _, _, _, _ => by cases a
  | [], _ :: _, _, a, _, _, _, _, _ => by cases a

theorem outsSet_drop : ∀ (σ : ISt) (n : Nat), σ.outsSet = true → ISt.outsSet (σ.drop n) = true
  | _, 0, h => h
  | [], _ + 1, h => h
  | _ :: σ, n + 1, h => by
    simp only [ISt.outsSet, List.all_cons, Bool.and_eq_true] at h
    exact outsSet_drop σ n h.2

theorem outsSet_take : ∀ (σ : ISt) (n : Nat), σ.outsSet = true → ISt.outsSet (σ.take n) = true
  | _, 0, _ => rfl
  | [], _ + 1, h => h
  | v :: σ, n + 1, h => by
    simp only [ISt.outsSet, List.all_cons, Bool.and_eq_true] at h
    simp only [List.take_succ_cons, ISt.outsSet, List.all_cons, Bool.and_eq_true]
    exact ⟨h.1, outsSet_take σ n h.2⟩

theorem outsSet_reverse (σ : ISt) (h : σ.outsSet = true) : ISt.outsSet σ.reverse = true := by
  simpa [ISt.outsSet, List.all_reverse] using h

/-- A program each of whose bodies the analysis lets through: the functions
`sigs` has a signature for have its modes in the table, and the analysis
lets each body through with each const it sees holding a value or not, as
the const's value in the table has one or not. -/
def ProgInit (sigs : List (String × Sig)) (funs : List (String × FunInfo)) : Prop :=
  ∀ f sig fi, sigs.lookup f = some sig → funs.lookup f = some fi →
    fi.params.map (·.1) = sig.params.map (·.1) ∧
    ∃ marks, initFun sigs marks fi.params fi.body = .ok () ∧
      IAg (marks.map fun (x, s) => (x, s, false)) fi.cvals

/-- Where a body leaves with every `mut` and `out` parameter holding a
value, what each parameter came to, as the call reads it back, holds one. -/
theorem after_ok (params : List (Mode × String × Hint)) (consts : List (String × Bool)) (ps : List (Mode × Ty))
    (hm : params.map (·.1) = ps.map (·.1)) {τ : ISt} {ρ0 ρ' : VEnv}
    (hfr : Fr (funStart params consts).shape τ) (h : IAg τ ρ') (ho : τ.outsSet = true)
    (hl0 : (funStart params consts).length = ρ0.length) :
    (paramsAfter params (restore ρ0 ρ')).length = ps.length ∧
      ∀ q ∈ (paramsAfter params (restore ρ0 ρ')).zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true := by
  have hpl : params.length = ps.length := by simpa using congrArg List.length hm
  obtain ⟨δ, hδ, hsh⟩ := hfr
  have ins : (funStart params consts).Inside τ := ⟨δ, hδ, hsh⟩
  have hb := IAg.back (σ := funStart params consts) h hl0
  have hbs := ISt.back_shape ins
  have hbo := outsSet_drop τ (τ.length - (funStart params consts).length) ho
  have hR : (restore ρ0 ρ').length = ρ0.length := by
    rw [← IAg.length hb, ← ISt.shape_length, hbs, ISt.shape_length, hl0]
  have hp0 : params.length ≤ ρ0.length := by rw [← hl0]; simp [funStart]
  refine ⟨by simp only [paramsAfter, List.length_reverse, List.length_map, List.length_take, hR]; omega, ?_⟩
  have hA := IAg.reverse (IAg.take params.length hb)
  have hAo := outsSet_reverse _ (outsSet_take _ params.length hbo)
  have hw : (((funStart params consts).back τ).take params.length).reverse.map (·.2.2) =
      ps.map (fun p => p.1 == .out || p.1 == .mut) := by
    have e1 : ((funStart params consts).back τ).map (·.2.2) = (funStart params consts).map (·.2.2) := by
      have := congrArg (List.map (·.2)) hbs
      simpa [ISt.shape, List.map_map, Function.comp_def] using this
    rw [List.map_reverse, List.map_take, e1]
    simp only [funStart, List.map_append, List.map_map, Function.comp_def]
    rw [List.take_left' (by simp)]
    rw [List.map_reverse, List.reverse_reverse]
    have := congrArg (List.map fun m => m == Mode.out || m == Mode.mut) hm
    simpa [List.map_map, Function.comp_def] using this
  intro q hq hmq
  have := outs_after _ _ ps hA hAo hw q (by
    simpa [paramsAfter, List.map_reverse, List.map_take] using hq) hmq
  exact this

section
variable (env : InferEnv) (sigs : List (String × Sig)) (funs : List (String × FunInfo)) (k : Nat)
  (hmv : env.moves = false)
include hmv

/-- **Calls into a program the analysis lets through keep the promise**:
given a value for every parameter but an `out` one, a call never stops at a
variable that holds nothing, and returns with every `mut` and `out`
parameter holding a value. -/
theorem runCall_init {S : List (String × Sig)} (hS : SigsIn sigs S) (hp : ProgInit sigs funs) :
    ∀ d, CallInit sigs (runCall env S funs k d)
  | 0 => fun f sig θ vals _ _ => by simp only [runCall]; simp [throw, throwThe, MonadExceptOf.throw]
  | d + 1 => by
    intro f sig θ vals hs hv
    simp only [runCall, hS f sig hs]
    cases hfi : funs.lookup f with
    | none => simp [throw, throwThe, MonadExceptOf.throw]
    | some fi =>
      obtain ⟨hm, marks, hinit, hcv⟩ := hp f sig fi hs hfi
      have hpl : fi.params.length = sig.params.length := by simpa using congrArg List.length hm
      simp only
      by_cases hn : vals.length ≠ fi.params.length
      · simp [hn, throw, throwThe, MonadExceptOf.throw]
      have hlen' : vals.length = fi.params.length := by simpa using hn
      simp only [hlen', bne_self_eq_false, Bool.false_eq_true, ↓reduceIte]
      unfold initFun at hinit
      obtain ⟨r, hr, hfin⟩ := bind_ok hinit
      have hout : completes fi.body = true → r.1.outsSet = true := by
        intro hc
        rw [hc] at hfin
        simp only [↓reduceIte, discard, Functor.mapConst] at hfin
        cases he : exitI r.1 with
        | error c => rw [he] at hfin; simp [Except.map] at hfin
        | ok σ' => exact (exitI_eq he).2
      have hvals : ∀ q ∈ vals.zip (fi.params.map (·.1)), q.2 ≠ .out → q.1.isSome = true := by
        intro q hq hmq
        rw [hm, List.zip_map_right] at hq
        obtain ⟨q', hq', rfl⟩ := List.mem_map.mp hq
        exact hv q' hq' hmq
      have hρ0 : IAg (funStart fi.params (marks)) (argVals fi.params vals fi.cvals) := by
        unfold funStart argVals
        rw [List.map_reverse, List.map_reverse]
        exact IAg.append (IAg.reverse (IAg.start fi.params vals hlen' hvals)) hcv
      have hl0 := IAg.length hρ0
      have hfr0 : Fr (funStart fi.params (marks)).shape (funStart fi.params (marks)) :=
        ⟨[], by simp, rfl⟩
      have hb := execBlock_init env sigs (runCall env S funs k d) (runCall_init hS hp d)
        (funStart fi.params (marks)).shape hmv fuel k
        ((funCtx S fi.aliases fi.consts fi.tps fi.params sig).inst θ) _ fi.body _ r hS hr hfr0 hρ0
      obtain ⟨ir, -⟩ := initBlock_shape sigs _ fi.body r hr
      generalize execBlock env (runCall env S funs k d) fuel k _ _ fi.body = m at hb ⊢
      cases m with
      | ok ρ' =>
        obtain ⟨-, h1, hc⟩ := hb
        cases hret : sig.ret with
        | ty t => simp [throw, throwThe, MonadExceptOf.throw]
        | void =>
          simp only [pure, Except.pure]
          obtain ⟨ha1, ha2⟩ := after_ok fi.params (marks) sig.params hm (hfr0.inside ir) h1
            (hout hc) hl0
          exact ⟨by simp [hpl], ha1, ha2⟩
      | error s =>
        cases s with
        | returned o ρ' =>
          obtain ⟨τ, hfr, h1, ho⟩ := hb
          obtain ⟨ha1, ha2⟩ := after_ok fi.params (marks) sig.params hm hfr h1 ho hl0
          cases o with
          | some v => exact ⟨by simp [hpl], ha1, ha2⟩
          | none => exact ⟨by simp [hpl], ha1, ha2⟩
        | brk ρ' => simp [throw, throwThe, MonadExceptOf.throw]
        | cont ρ' => simp [throw, throwThe, MonadExceptOf.throw]
        | uninit => exact hb.elim
        | early r => exact hb.elim
        | _ => simp [throw, throwThe, MonadExceptOf.throw]

end

/-! ## Scripts -/

theorem unreturn_ne_uninit {s : Stop} (h : s ≠ .uninit) : s.unreturn ≠ .uninit := by
  cases s <;> simp_all [Stop.unreturn] <;> split <;> simp

/-- **The top level of a script the analysis lets through never reads what
holds nothing**: run against a program whose bodies the analysis lets
through, its statements never stop at `Stop.uninit`. -/
theorem runTop_init (sigs : List (String × Sig)) (funs : List (String × FunInfo)) (k d : Nat)
    (hp : ProgInit sigs funs) : ∀ (ss : List Stmt) (Γ Γa : Ctx) (σ : ISt) (ρ : VEnv) (log : List (Ty × Val)),
    SigsIn sigs Γ.sigs → initTop sigs Γa σ ss = .ok () → Fr [] σ → IAg σ ρ →
    (runTop sigs k d Γ ρ funs log ss).result ≠ .error .uninit
  | [], _, _, _, _, _, _, _, _, _ => by simp [runTop, pure, Except.pure]
  | s :: ss, Γ, Γa, σ, ρ, log, hs, h, hfr, hρ => by
    have hcall := runCall_init inferEnv sigs funs k rfl (SigsIn.refl sigs) hp d
    cases s
    case fun_ f =>
      obtain ⟨name, tps, params, ret, body⟩ := f
      simp only [initTop] at h
      obtain ⟨_, -, h2⟩ := bind_ok h
      simp only [runTop]
      exact runTop_init sigs funs k d hp ss Γ Γa σ ρ log hs h2 hfr hρ
    case debuglog e =>
      simp only [initTop] at h
      obtain ⟨r, h1, h2⟩ := bind_ok h
      simp only [initStmt] at h1
      obtain ⟨σ1, he, hA⟩ := bind_ok h1
      simp only [pure, Except.pure, Except.ok.injEq] at hA; subst hA
      have hev := evalS_init inferEnv sigs (runCall inferEnv sigs funs k d) hcall [] rfl (fuel - 1) Γ.borrow e σ σ1 hs he
        hfr ρ hρ
      simp only [runTop]
      cases ht : typeOf (synthF inferEnv (fuel - 1) Γ.borrow e) with
      | error s' =>
        simp only
        intro hu
        have : s' = .wrong := by
          unfold typeOf at ht; split at ht <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
        subst this
        simp [Stop.unreturn, throw, throwThe, MonadExceptOf.throw] at hu
      | ok t =>
        generalize (evalS inferEnv (runCall inferEnv sigs funs k d) (fuel - 1) Γ.borrow e).run ρ = m at hev ⊢
        cases m with
        | ok q =>
          obtain ⟨v, ρ'⟩ := q
          simp only
          exact runTop_init sigs funs k d hp ss Γ _ σ1 ρ' _ hs h2
            (hfr.inside (ISt.Inside.of_shape (initE_shape sigs σ e σ1 he))) hev.2.1
        | error s' =>
          simp only
          intro hu
          have hne : s' ≠ .uninit := by intro e; subst e; exact hev
          exact unreturn_ne_uninit hne (by simpa [throw, throwThe, MonadExceptOf.throw] using hu)
    all_goals
      simp only [initTop] at h
      obtain ⟨r, h1, h2⟩ := bind_ok h
      have hex := execStmt_init inferEnv sigs (runCall inferEnv sigs funs k d) hcall [] rfl fuel k Γ ρ _ σ r hs h1 hfr hρ
      obtain ⟨ir, -⟩ := initStmt_shape sigs σ _ r h1
      simp only [runTop]
      generalize execStmt inferEnv (runCall inferEnv sigs funs k d) fuel k Γ ρ _ = m at hex ⊢
      cases m with
      | ok ρ' =>
        simp only
        exact runTop_init sigs funs k d hp ss _ _ r.1 ρ' log (hs.of_eq (checkStmtF_sigs _ _ _ _ _)) h2
          (hfr.inside ir) hex.2.1
      | error s' =>
        simp only
        intro hu
        have hne : s' ≠ .uninit := by intro e; subst e; exact hex
        exact unreturn_ne_uninit hne (by simpa [throw, throwThe, MonadExceptOf.throw] using hu)

/-! ## A script's functions -/

theorem mapM_modes {f : Hint → Except Lit.Code Ty} : ∀ {l : List (Mode × String × Hint)} {ps : List (Mode × Ty)},
    (l.mapM fun (m, _, h) => do pure (m, ← f h)) = .ok ps → ps.map (·.1) = l.map (·.1)
  | [], ps, h => by simp only [List.mapM_nil, pure, Except.pure, Except.ok.injEq] at h; subst h; rfl
  | (m, x, hh) :: l, ps, h => by
    rw [List.mapM_cons] at h
    obtain ⟨p, hp, h⟩ := bind_ok h
    obtain ⟨rest, hr, h⟩ := bind_ok h
    simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
    obtain ⟨t, ht, hp⟩ := bind_ok hp
    simp only [pure, Except.pure, Except.ok.injEq] at hp; subst hp
    simp [mapM_modes hr]

theorem sigOf_modes {A : Aliases} {name : String} {tps : List (String × Option Bound)}
    {params : List (Mode × String × Hint)} {ret : Option Hint} {body : List Stmt} {sig : Sig}
    (h : sigOf A (.mk name tps params ret body) = .ok sig) : sig.params.map (·.1) = params.map (·.1) := by
  cases hps : (params.mapM fun (m, _, h) => do pure (m, ← sigHint A tps h)) with
  | error c =>
    simp only [sigOf] at h
    rw [hps] at h
    cases h
  | ok ps =>
    have : sig.params = ps := by
      simp only [sigOf] at h
      rw [hps] at h
      cases ret with
      | none => simp [bind, Except.bind, pure, Except.pure] at h; subst h; rfl
      | some rh =>
        simp only [bind, Except.bind] at h
        cases hr : sigHint A tps rh with
        | error c => simp [hr, Functor.map, Except.map] at h
        | ok t => simp [hr, Functor.map, Except.map, pure, Except.pure] at h; subst h; rfl
    rw [this]; exact mapM_modes hps

/-- In a script whose names resolve quietly, each function's signature has
its modes. -/
theorem decl_modes {ss : List Stmt} (hn : (resolveNames ss).2 = #[]) {name : String}
    {tps : List (String × Option Bound)} {params : List (Mode × String × Hint)} {ret : Option Hint}
    {body : List Stmt} (hm : .fun_ (.mk name tps params ret body) ∈ ss) :
    ∃ sig, (resolveNames ss).1.lookup name = some sig ∧ sig.params.map (·.1) = params.map (·.1) := by
  rw [resolveNames_foldl] at hn ⊢
  obtain ⟨A, sig, hl, hs⟩ := foldl_own ss _ hn _ hm
  exact ⟨sig, hl, sigOf_modes hs⟩

/-- Each function of a script the analysis lets through is one whose body it
lets through, with the consts it sees. -/
theorem funDefs_init (sigs : List (String × Sig)) : ∀ (ss : List Stmt) (Γ : Ctx) (σ : ISt) (seen : Nat),
    initTop sigs Γ σ ss = .ok () → ∀ fd ∈ funDefs sigs Γ seen ss,
      initFun sigs (fd.consts.map (·.1, true)) fd.params fd.body = .ok () ∧
      ∃ tps ret, Stmt.fun_ (.mk fd.name tps fd.params ret fd.body) ∈ ss
  | [], _, _, _, _ => by simp [funDefs]
  | s :: ss, Γ, σ, seen, h => by
    cases s
    case fun_ f =>
      obtain ⟨name, tps, params, ret, body⟩ := f
      simp only [initTop] at h
      obtain ⟨_, h1, h2⟩ := bind_ok h
      intro fd hfd
      simp only [funDefs, List.mem_append] at hfd
      rcases hfd with hfd | hfd
      · simp only [funDef] at hfd
        split at hfd
        · simp only [Option.toList_some, List.mem_singleton] at hfd; subst hfd
          exact ⟨h1, tps, ret, List.mem_cons_self⟩
        · simp at hfd
      · obtain ⟨hi, tps', ret', hm⟩ := funDefs_init sigs ss Γ σ seen h2 fd hfd
        exact ⟨hi, tps', ret', List.mem_cons_of_mem _ hm⟩
    all_goals
      simp only [initTop] at h
      obtain ⟨r, -, h2⟩ := bind_ok h
      intro fd hfd
      simp only [funDefs] at hfd
      obtain ⟨hi, tps', ret', hm⟩ := funDefs_init sigs ss _ r.1 _ h2 fd hfd
      exact ⟨hi, tps', ret', List.mem_cons_of_mem _ hm⟩

/-- The table of a script's functions is a program each of whose bodies the
analysis lets through, where the consts each sees hold values. -/
theorem funTable_init {ss : List Stmt} (hn : (resolveNames ss).2 = #[]) {Γ : Ctx} {σ : ISt}
    (h : initTop (resolveNames ss).1 Γ σ ss = .ok ()) {store : CStore}
    (hc : ∀ fd ∈ funDefs (resolveNames ss).1 Γ 0 ss,
      ∀ q ∈ constEnv fd.consts (store.drop (store.length - fd.seen)), q.2.isSome = true) :
    ProgInit (resolveNames ss).1 (funTable (funDefs (resolveNames ss).1 Γ 0 ss) store) := by
  intro f sig fi hs hl
  have hm := lookup_mem_of hl
  simp only [funTable, List.mem_map] at hm
  obtain ⟨fd, hfd, he⟩ := hm
  simp only [FunDef.entry, Prod.mk.injEq] at he
  obtain ⟨rfl, rfl⟩ := he
  obtain ⟨hi, tps, ret, hmem⟩ := funDefs_init _ ss Γ σ 0 h fd hfd
  obtain ⟨sig', hl', hmodes⟩ := decl_modes hn hmem
  rw [hs] at hl'; cases hl'
  refine ⟨hmodes.symm, (fd.consts.map (·.1)).map (·, true), by simpa [List.map_map, Function.comp_def] using hi,
    IAg.consts _ _ (by simp [constEnv, List.map_map, Function.comp_def]) (hc fd hfd)⟩

/-! ## The consts a function sees -/

/-- What checking a statement binds is of the kind it says: a `let` or `var`
binds no const. -/
theorem bindAllAs_vars_mem : ∀ (Γ : Ctx) (Δ : List (String × Ty)) (k : Kind) (p : String × Ty × Kind),
    p ∈ (Γ.bindAllAs Δ k).vars → p ∈ Γ.vars ∨ p.2.2 = k
  | _, [], _, _, h => .inl h
  | Γ, (x, t) :: Δ, k, p, h => by
    rcases bindAllAs_vars_mem (Γ.bind x t k) Δ k p h with h | h
    · rcases List.mem_cons.mp h with rfl | h
      · exact .inr rfl
      · exact .inl h
    · exact .inr h

theorem bindPat_vars (Γ : Ctx) (p : Pat) (t : Ty) (k : Kind) (d : Array Lit.Code) (q : String × Ty × Kind)
    (h : q ∈ ((bindPat Γ p t k).run d).1.vars) : q ∈ Γ.vars ∨ q.2.2 = k := by
  unfold bindPat at h
  split at h
  · simp only [StateM.run_pure'] at h; exact bindAllAs_vars_mem Γ _ k q h
  · simp only [StateM.run_bind', StateM.run_modify', StateM.run_pure'] at h; exact .inl h

theorem bindChecked_vars (env : InferEnv) (n : Nat) (Γ : Ctx) (p : Pat) (hint : Option Hint) (e : Expr)
    (k : Kind) (d : Array Lit.Code) (q : String × Ty × Kind)
    (h : q ∈ ((bindChecked env n Γ p hint e k).run d).1.vars) : q ∈ Γ.vars ∨ q.2.2 = k := by
  unfold bindChecked at h
  cases hint with
  | some hh =>
    simp only [StateM.run_bind'] at h
    split at h
    · simp only [StateM.run_pure'] at h; exact .inl h
    · split at h
      · simp only [StateM.run_bind', StateM.run_modify'] at h; exact bindPat_vars _ _ _ _ _ q h
      · simp only [StateM.run_bind'] at h; exact bindPat_vars _ _ _ _ _ q h
  | none =>
    simp only [StateM.run_bind'] at h
    repeat' split at h
    all_goals first | exact bindPat_vars _ _ _ _ _ q h | (simp only [StateM.run_pure'] at h; exact .inl h)

/-- A statement other than a `const` binds no const. -/
theorem checkStmtF_noConst (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (s : Stmt) (d : Array Lit.Code),
    (∀ x h e, s ≠ .const_ x h e) → ∀ q ∈ ((checkStmtF env n Γ s).run d).1.vars, q.2.2 = .const_ → q ∈ Γ.vars
  | 0, Γ, s, d, _, q, h, _ => by
    simpa only [checkStmtF, StateM.run_bind', StateM.run_modify', StateM.run_pure'] using h
  | n + 1, Γ, s, d, hs, q, h, hk => by
    cases s
    case let_ p hint e =>
      simp only [checkStmtF] at h
      rcases bindChecked_vars env n Γ p hint e .let_ d q h with h | h
      · exact h
      · rw [h] at hk; cases hk
    case var_ p hint v =>
      cases v with
      | some e =>
        simp only [checkStmtF] at h
        rcases bindChecked_vars env n Γ p hint e .var_ d q h with h | h
        · exact h
        · rw [h] at hk; cases hk
      | none =>
        cases hint with
        | none => simpa only [checkStmtF, StateM.run_pure'] using h
        | some hh =>
          simp only [checkStmtF, StateM.run_bind'] at h
          split at h
          · simp only [StateM.run_bind'] at h
            rcases bindPat_vars _ _ _ _ _ q h with h | h
            · exact h
            · rw [h] at hk; cases hk
          · simpa only [StateM.run_pure'] using h
    case const_ x hint e => exact absurd rfl (hs x hint e)
    case type_ x hh => simp only [checkStmtF, StateM.run_pure'] at h; split at h <;> exact h
    case if_ c tB eB t e =>
      cases tB <;> cases eB <;> simpa only [checkStmtF, StateM.run_bind', StateM.run_pure'] using h
    case loop c body => cases c <;> simpa only [checkStmtF, StateM.run_bind', StateM.run_pure'] using h
    all_goals simpa only [checkStmtF, StateM.run_bind', StateM.run_pure'] using h

/-- A `const` binds at most its own name, at the head of the variables. -/
theorem checkConst_vars (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (hint : Option Hint) (e : Expr)
    (d : Array Lit.Code) :
    let vs := ((checkStmtF env (n + 1) Γ (.const_ x hint e)).run d).1.vars
    vs = Γ.vars ∨ ∃ p, vs = p :: Γ.vars := by
  simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
  generalize hb : bindChecked env n { Γ with constExpr := true } (.name x) hint e .const_ = m
  rw [← hb]
  unfold bindChecked
  have hpat : ∀ (t : Ty) (k : Kind) (d' : Array Lit.Code),
      ((bindPat { Γ with constExpr := true } (.name x) t k).run d').1.vars = Γ.vars ∨
        ∃ p, ((bindPat { Γ with constExpr := true } (.name x) t k).run d').1.vars = p :: Γ.vars := by
    intro t k d'
    unfold bindPat
    simp only [patBindings, pure, Except.pure]
    exact .inr ⟨(x, t, k), rfl⟩
  cases hint with
  | some hh =>
    simp only [StateM.run_bind']
    split
    · first | exact .inl rfl | trivial
    · split
      · simp only [StateM.run_bind', StateM.run_modify']; exact hpat _ _ _
      · simp only [StateM.run_bind']; exact hpat _ _ _
  | none =>
    simp only [StateM.run_bind']
    repeat' split
    all_goals first | exact hpat _ _ _ | (left; rw [StateM.run_pure'])

/-- A `const` that runs binds its name to a value, at the head. -/
theorem execBind_head {env : InferEnv} {call : CallFn} {n : Nat} {Γ : Ctx} {ρ ρ' : VEnv} {x : String}
    {hint : Option Hint} {e : Expr} (h : execBind env call n Γ ρ (.name x) hint e = .ok ρ') :
    ∃ v ρ1, ρ' = (x, some v) :: ρ1 := by
  have fin : ∀ (r : Except Stop (Val × VEnv)), (r >>= fun q : Val × VEnv => (do
      let vals ← patValues (.name x) q.1
      if vals.map (·.1) != patNames (.name x) then throw .wrong
      pure (bindVals q.2 (vals.map fun (x, w) => (x, some w)))) : EvalM VEnv) = .ok ρ' →
      ∃ v ρ1, ρ' = (x, some v) :: ρ1 := by
    intro r hr
    cases r with
    | error s => cases hr
    | ok q =>
      obtain ⟨v, ρ1⟩ := q
      simp [patValues, patNames, bindVals, pure, Except.pure, bind, Except.bind] at hr
      exact ⟨v, ρ1, hr.symm⟩
  unfold execBind at h
  cases hint with
  | none => exact fin _ h
  | some hh =>
    simp only at h
    split at h
    · exact fin _ h
    · cases h

/-- What the store keeps for the consts in scope: a value for each. -/
def StoreHas (vars : List (String × Ty × Kind)) (store : CStore) : Prop :=
  (∀ p ∈ store, p.2.2.isSome = true) ∧ ∀ p ∈ vars, p.2.2 = .const_ → ∃ v, (p.1, p.2.1, some v) ∈ store

theorem constEnv_some {vars : List (String × Ty × Kind)} {store : CStore} (h : StoreHas vars store) :
    ∀ q ∈ constEnv (constsOf vars) store, q.2.isSome = true := by
  intro q hq
  simp only [constEnv, List.mem_map] at hq
  obtain ⟨⟨x, t, k⟩, hm, rfl⟩ := hq
  obtain ⟨hin, hk⟩ := constsOf_mem hm
  simp only at hk; subst hk
  obtain ⟨v, hv⟩ := h.2 _ hin rfl
  simp only [beq_self_eq_true, ↓reduceIte]
  cases hf : store.find? (fun (y, u, _) => y == x && u == t) with
  | none =>
    exfalso
    have := List.find?_eq_none.mp hf _ hv
    simp at this
  | some e =>
    have := h.1 e (List.mem_of_find?_eq_some hf)
    simpa using this

theorem fuel_succ : fuel = (fuel - 1) + 1 := rfl

/-- **The consts a function sees hold values**: walking a script's
statements as `constPass` evaluates its consts and `funDefs` reaches its
functions, the store holds a value for every const each function sees. -/
theorem constPass_has (sigs : List (String × Sig)) (defs : List FunDef) (k d : Nat) :
    ∀ (ss : List Stmt) (Γ : Ctx) (store final : CStore) (seen : Nat),
    store.length = seen → StoreHas Γ.vars store → constPass sigs defs k d Γ store ss = .ok final →
    (∃ δ, final = δ ++ store) ∧ ∀ fd ∈ funDefs sigs Γ seen ss,
      ∀ q ∈ constEnv fd.consts (final.drop (final.length - fd.seen)), q.2.isSome = true
  | [], Γ, store, final, seen, _, _, h => by
    simp only [constPass, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨⟨[], rfl⟩, by simp [funDefs]⟩
  | st :: ss, Γ, store, final, seen, hl, hs, h => by
   cases st
   case fun_ f =>
    simp only [constPass] at h
    obtain ⟨⟨δ, rfl⟩, ih⟩ := constPass_has sigs defs k d ss Γ store final seen hl hs h
    refine ⟨⟨δ, rfl⟩, fun fd hfd => ?_⟩
    simp only [funDefs, List.mem_append] at hfd
    rcases hfd with hfd | hfd
    · obtain ⟨name, tps, params, ret, body⟩ := f
      simp only [funDef] at hfd
      split at hfd
      · simp only [Option.toList_some, List.mem_singleton] at hfd; subst hfd
        simp only [List.length_append, hl, Nat.add_sub_cancel, List.drop_left' (rfl : δ.length = δ.length)]
        exact constEnv_some hs
      · simp at hfd
    · exact ih fd hfd
   case const_ x hint e =>
    simp only [constPass, ↓reduceIte] at h
    obtain ⟨ρ, he, h⟩ := bind_ok h
    have hx : ∃ v ρ1, ρ = (x, some v) :: ρ1 := by
      rw [fuel_succ] at he
      simp only [execStmt] at he
      exact execBind_head he
    obtain ⟨v, ρ1, rfl⟩ := hx
    have hvars := checkConst_vars inferEnv (fuel - 1) Γ x hint e #[]
    rw [← fuel_succ] at hvars
    simp only [checkStmt] at h ⊢
    generalize hG : ((checkStmtF inferEnv fuel Γ (.const_ x hint e)).run #[]).1 = Γ' at h hvars ⊢
    cases hv : Γ'.vars with
    | nil => rw [hv] at h; cases h
    | cons p rest =>
      obtain ⟨y, u, kk⟩ := p
      rw [hv] at h
      simp only at h
      have hs' : StoreHas Γ'.vars ((y, u, some v) :: store) := by
        refine ⟨fun q hq => ?_, fun q hq hk => ?_⟩
        · rcases List.mem_cons.mp hq with rfl | hq
          · rfl
          · exact hs.1 q hq
        · rcases hvars with hvs | ⟨p', hvs⟩
          · obtain ⟨w, hw⟩ := hs.2 q (hvs ▸ hq) hk
            exact ⟨w, List.mem_cons_of_mem _ hw⟩
          · rw [hvs] at hq hv
            simp only [List.cons.injEq] at hv
            obtain ⟨rfl, -⟩ := hv
            rcases List.mem_cons.mp hq with rfl | hq
            · exact ⟨v, List.mem_cons_self⟩
            · obtain ⟨w, hw⟩ := hs.2 q hq hk
              exact ⟨w, List.mem_cons_of_mem _ hw⟩
      obtain ⟨⟨δ, rfl⟩, ih⟩ := constPass_has sigs defs k d ss Γ' _ final (seen + 1) (by simp [hl]) hs' h
      refine ⟨⟨δ ++ [(y, u, some v)], by simp⟩, fun fd hfd => ?_⟩
      simp only [funDefs, checkStmt, hG] at hfd
      exact ih fd hfd
   all_goals
    simp only [constPass] at h
    obtain ⟨hδ, ih⟩ := constPass_has sigs defs k d ss _ store final seen hl
      (by exact ⟨hs.1, fun q hq hk => hs.2 q
        (checkStmtF_noConst inferEnv fuel Γ _ #[] (fun _ _ _ e => Stmt.noConfusion e) q hq hk) hk⟩) h
    exact ⟨hδ, fun fd hfd => ih fd (by simpa [funDefs] using hfd)⟩

theorem unreturn_uninit {s : Stop} (h : s.unreturn = .uninit) : s = .uninit := by
  cases s <;> simp_all [Stop.unreturn] <;> split at h <;> simp_all

/-- **A script the analysis lets through never reads what holds nothing**:
where a script checks and the analysis of what holds a value (O005, O006,
O009) lets it through, running it stops at `Stop.uninit` only where
evaluating its consts, before anything else, does. Past the consts no
variable is read while it holds nothing, and no call returns with an `out`
parameter it left holding nothing, since every way out of a body the
analysis lets through has given each one a value.

The consts are `runScript_noUninit`'s, in `Proofs/ConstSound.lean`: a
const's initializer runs on the consts' values alone (`constPass`), and
that it reads only consts already evaluated rests on F089. -/
theorem runScript_init (ss : List Stmt) (h : checkScript ss = #[]) (hi : initScript ss = none) (k d : Nat)
    (hu : (runScript k d ss).result = .error .uninit) :
    constPass (resolveNames ss).1 (funDefs (resolveNames ss).1
      { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss) k d
      { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss = .error .uninit := by
  have hn := checkScript_names h
  unfold initScript at hi
  cases ht : initTop (resolveNames ss).1 { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss with
  | error c => simp only [ht] at hi; cases hi
  | ok u =>
    cases u
    simp only [runScript] at hu
    revert hu
    cases hcp : constPass (resolveNames ss).1 (funDefs (resolveNames ss).1
        { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss) k d
        { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss with
    | ok store =>
      intro hu
      obtain ⟨-, hc⟩ := constPass_has _ _ k d ss _ [] store 0 rfl ⟨by simp, by simp⟩ hcp
      exact absurd hu (runTop_init _ _ k d (funTable_init hn ht hc) ss _ _ [] [] [] (SigsIn.refl _) ht ⟨[], by simp, rfl⟩ .nil)
    | error s =>
      intro hu
      simp only [throw, throwThe, MonadExceptOf.throw, Except.error.injEq] at hu
      rw [unreturn_uninit hu]

end Datalove.Fun
