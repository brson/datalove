import Datalove.Proofs.InstanceCheck

/-!
# Checking a generic once is enough: statements and functions

The statement checker writes its diagnostics to a log. Every part of it only
appends to the log, and nothing it does depends on what the log holds
(`Appends`), so a statement that adds nothing is one whose every part added
nothing (`SQuiet.bind_inv`). That is what lets a quiet generic body be taken
apart statement by statement, and each part carried over to the instance by
`Proofs/InstanceCheck.lean`.
-/

namespace Datalove.Fun

open Datalove.Lit (Code unknownCode tysList fieldsList)

abbrev DiagM := StateM (Array Code)

/-! ## Computations that only append to the log -/

/-- A computation that only adds to the log: what it gives, and what it adds,
do not depend on what the log held. -/
structure Appends {α} (m : DiagM α) : Prop where
  run : ∀ d, m.run d = ((m.run #[]).1, d ++ (m.run #[]).2)

theorem Appends.pure {α} (a : α) : Appends (Pure.pure a : DiagM α) := by
  constructor; intro d; simp [StateT.run, Pure.pure, StateT.pure]

theorem Appends.push (c : Code) : Appends (modify (·.push c) : DiagM PUnit) := by
  constructor; intro d
  simp [StateT.run, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, Pure.pure]

theorem Appends.append (l : Array Code) : Appends (modify (· ++ l) : DiagM PUnit) := by
  constructor; intro d
  simp [StateT.run, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, Pure.pure]

theorem bind_run' {α β} (x : DiagM α) (f : α → DiagM β) (d : Array Code) :
    (x >>= f).run d = (f (x.run d).1).run (x.run d).2 := rfl

theorem Appends.bind {α β} {x : DiagM α} {f : α → DiagM β} (hx : Appends x)
    (hf : ∀ a, Appends (f a)) : Appends (x >>= f) := by
  constructor; intro d
  rw [bind_run', bind_run', hx.run d]
  dsimp only
  rw [(hf _).run (d ++ _), (hf _).run ((x.run #[]).2)]
  simp [Array.append_assoc]

theorem Appends.chk {α} (m : ChkM α) : Appends (runChk m) := by
  unfold Datalove.Fun.runChk
  refine Appends.bind (Appends.append _) (fun _ => ?_)
  split
  · exact Appends.pure _
  · exact Appends.bind (Appends.push _) (fun _ => Appends.pure _)

theorem Appends.report (c : Bool) (code : Code) : Appends (reportIf c code) := by
  unfold Datalove.Fun.reportIf; split
  · exact Appends.push _
  · exact Appends.pure _

/-! ## Quiet runs -/

/-- A computation that adds nothing to the log and gives `a`. -/
def SQuiet {α} (m : DiagM α) (a : α) : Prop := m.run #[] = (a, #[])

theorem SQuiet.bind {α β} {x : DiagM α} {f : α → DiagM β} {a : α} {b : β} (hx : SQuiet x a)
    (hf : SQuiet (f a) b) : SQuiet (x >>= f) b := by
  unfold SQuiet at *; rw [bind_run', hx]; exact hf

theorem SQuiet.bind_inv {α β} {x : DiagM α} {f : α → DiagM β} {b : β}
    (af : ∀ a, Appends (f a)) (h : SQuiet (x >>= f) b) : ∃ a, SQuiet x a ∧ SQuiet (f a) b := by
  unfold SQuiet at *
  rw [bind_run', (af _).run (x.run #[]).2] at h
  have h1 := congrArg Prod.fst h
  have h2 := congrArg Prod.snd h
  dsimp only at h1 h2
  obtain ⟨e1, e2⟩ := Array.append_eq_empty_iff.mp h2
  exact ⟨(x.run #[]).1, Prod.ext rfl e1, Prod.ext h1 e2⟩

@[simp] theorem SQuiet.pure_iff {α} {a b : α} : SQuiet (Pure.pure a : DiagM α) b ↔ a = b := by
  unfold SQuiet
  show ((a, #[]) : α × Array Code) = (b, #[]) ↔ a = b
  exact ⟨fun h => (Prod.mk.inj h).1, fun h => h ▸ rfl⟩

theorem SQuiet.pure {α} (a : α) : SQuiet (Pure.pure a : DiagM α) a := rfl

@[simp] theorem SQuiet.push_iff {c : Code} {u : PUnit} :
    SQuiet (modify (·.push c) : DiagM PUnit) u ↔ False := by
  unfold SQuiet
  show ((PUnit.unit, #[c]) : PUnit × Array Code) = (u, #[]) ↔ False
  constructor
  · intro h; have := congrArg (fun p => p.2.size) h; simp at this
  · intro h; exact h.elim

theorem SQuiet.push_bind {α} {c : Code} {f : PUnit → DiagM α} {b : α} (af : ∀ a, Appends (f a))
    (h : SQuiet (modify (·.push c) >>= f) b) : False := by
  obtain ⟨_, h, -⟩ := SQuiet.bind_inv af h
  simp at h

theorem SQuiet.reportIf_iff {c : Bool} {code : Code} {u : Unit} :
    SQuiet (reportIf c code) u ↔ c = false := by
  unfold reportIf
  cases c <;> simp

theorem SQuiet.chk_inv {α} {m : ChkM α} {r : Option α} (h : SQuiet (runChk m) r) :
    ∃ a, r = some a ∧ Quiet m a := by
  unfold SQuiet at h
  unfold Datalove.Fun.runChk at h
  unfold Quiet
  rw [bind_run'] at h
  generalize m.run = R at h ⊢
  obtain ⟨l, v⟩ := R
  have hl := congrArg (fun p => p.2.size) h
  cases v with
  | ok a =>
    have h1 := congrArg Prod.fst h
    simp [StateT.run, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet,
      Pure.pure, StateT.pure] at h1 hl
    subst h1
    exact ⟨a, rfl, by rw [hl]⟩
  | error c =>
    simp [StateT.run, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet,
      Pure.pure, StateT.pure, Bind.bind, StateT.bind] at hl

theorem SQuiet.chk {α} {m : ChkM α} {a : α} (h : Quiet m a) : SQuiet (runChk m) (some a) := by
  unfold SQuiet Datalove.Fun.runChk
  unfold Quiet at h
  rw [h]
  rfl

/-! ## Every statement only appends -/

/-- Take a computation apart into the pieces that only append. -/
macro "appends" : tactic => `(tactic| repeat' (first
  | exact Appends.pure _
  | exact Appends.push _
  | exact Appends.chk _
  | exact Appends.report _ _
  | intro _
  | apply Appends.bind
  | split))

theorem bindPat_appends (Γ : Ctx) (p : Pat) (t : Ty) (k : Kind) : Appends (bindPat Γ p t k) := by
  unfold bindPat; appends

theorem convertReporting_appends (Γ : Ctx) (h : Hint) : Appends (convertReporting Γ h) := by
  unfold convertReporting; appends

theorem bindChecked_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (p : Pat) (hint : Option Hint)
    (e : Expr) (k : Kind) : Appends (bindChecked env n Γ p hint e k) := by
  unfold bindChecked
  repeat' (first
    | exact bindPat_appends _ _ _ _
    | exact convertReporting_appends _ _
    | exact Appends.pure _
    | exact Appends.push _
    | exact Appends.chk _
    | intro _
    | apply Appends.bind
    | split)

theorem forBindings_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (names : List String) (coll : Expr) :
    Appends (forBindings env n Γ names coll) := by
  unfold forBindings; appends

theorem setValue_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (op : Option BinOp) (e : Expr) :
    Appends (setValue env n Γ t op e) := by
  unfold setValue; appends

theorem setIndex_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (k : Expr) (mode : Option IdxMode)
    (op : Option BinOp) (e : Expr) : Appends (setIndex env n Γ t k mode op e) := by
  unfold setIndex
  repeat' (first
    | exact setValue_appends _ _ _ _ _ _
    | exact Appends.pure _
    | exact Appends.push _
    | exact Appends.chk _
    | intro _
    | apply Appends.bind
    | split)

theorem setSteps_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (op : Option BinOp) (e : Expr) :
    ∀ (t : Ty) (steps : List Step), Appends (setSteps env n Γ t op e steps)
  | t, [] => by simp only [setSteps]; exact setValue_appends _ _ _ _ _ _
  | t, [.index k mode] => by simp only [setSteps]; exact setIndex_appends _ _ _ _ _ _ _ _
  | t, .field s :: rest => by
    simp only [setSteps]; split
    · exact setSteps_appends env n Γ op e _ rest
    · exact Appends.push _
  | t, .index _ none :: _ :: _ => by simp only [setSteps]; exact Appends.push _
  | t, .index k (some m) :: st :: rest => by
    simp only [setSteps]; split
    · exact Appends.push _
    · exact Appends.bind (Appends.chk _) (fun _ => setSteps_appends env n Γ op e _ (st :: rest))

theorem reportModes_appends (Γ : Ctx) : ∀ (ms : List IdxMode), Appends (reportModes Γ ms)
  | [] => Appends.pure _
  | _ :: ms => Appends.bind (Appends.report _ _) (fun _ => reportModes_appends Γ ms)

theorem checkSet_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) : Appends (checkSet env n Γ x steps op e) := by
  unfold checkSet
  repeat' (first
    | exact setSteps_appends _ _ _ _ _ _ _
    | exact reportModes_appends _ _
    | exact Appends.pure _
    | exact Appends.push _
    | exact Appends.report _ _
    | intro _
    | apply Appends.bind
    | split)

theorem checkRet_appends (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Option Expr) :
    Appends (checkRet env n Γ e) := by
  unfold checkRet; appends

/-- One step of taking a statement apart into pieces that only append. -/
macro "stmt_step" : tactic => `(tactic| first
  | with_reducible exact bindChecked_appends _ _ _ _ _ _ _
  | with_reducible exact bindPat_appends _ _ _ _
  | with_reducible exact convertReporting_appends _ _
  | with_reducible exact checkSet_appends _ _ _ _ _ _ _
  | with_reducible exact checkRet_appends _ _ _ _
  | with_reducible exact forBindings_appends _ _ _ _ _
  | with_reducible exact Appends.pure _
  | with_reducible exact Appends.push _
  | with_reducible exact Appends.chk _
  | with_reducible exact Appends.report _ _
  | intro _
  | with_reducible apply Appends.bind
  | split)

mutual

theorem checkStmtF_appends (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (s : Stmt),
    Appends (checkStmtF env n Γ s)
  | 0, Γ, _ => by
    simp only [checkStmtF]; exact Appends.bind (Appends.push _) (fun _ => Appends.pure _)
  | n + 1, Γ, s => by
    cases s
    case var_ p hint v =>
      cases v <;> (try cases hint) <;> simp only [checkStmtF] <;> repeat' (first | with_reducible exact checkBlockF_appends env n _ _ | with_reducible exact checkElse_appends env n _ _ | with_reducible exact checkCases_appends env n _ _ _ _ | stmt_step)
    case loop c body =>
      cases c <;> simp only [checkStmtF] <;> repeat' (first | with_reducible exact checkBlockF_appends env n _ _ | with_reducible exact checkElse_appends env n _ _ | with_reducible exact checkCases_appends env n _ _ _ _ | stmt_step)
    case if_ c a b t e =>
      cases a <;> cases b <;> simp only [checkStmtF] <;> repeat' (first | with_reducible exact checkBlockF_appends env n _ _ | with_reducible exact checkElse_appends env n _ _ | with_reducible exact checkCases_appends env n _ _ _ _ | stmt_step)
    all_goals (simp only [checkStmtF]; repeat' (first | with_reducible exact checkBlockF_appends env n _ _ | with_reducible exact checkElse_appends env n _ _ | with_reducible exact checkCases_appends env n _ _ _ _ | stmt_step))

theorem checkCases_appends (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (vs : List (String × Option Ty))
    (cs : List Case) (covered : List String), Appends (checkCases env n Γ vs cs covered)
  | 0, _, _, _, _ => by
    rw [checkCases.eq_def]; exact Appends.bind (Appends.push _) (fun _ => Appends.pure _)
  | _ + 1, _, _, [], _ => by rw [checkCases.eq_def]; exact Appends.pure _
  | n + 1, Γ, vs, c :: cs, covered => by
    rw [checkCases.eq_def]; dsimp only
    repeat' (first
      | with_reducible exact checkBlockF_appends env n _ _
      | with_reducible exact checkCases_appends env n _ _ _ _
      | exact Appends.pure _
      | exact Appends.push _
      | exact Appends.report _ _
      | intro _
      | apply Appends.bind
      | split)

theorem checkElse_appends (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (e : Option (List Stmt)),
    Appends (checkElse env n Γ e)
  | n, Γ, some e => by rw [checkElse.eq_def]; exact checkBlockF_appends env n Γ e
  | _, _, none => by rw [checkElse.eq_def]; exact Appends.pure _

theorem checkBlockF_appends (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (ss : List Stmt),
    Appends (checkBlockF env n Γ ss)
  | 0, _, _ => by rw [checkBlockF.eq_def]; exact Appends.push _
  | _ + 1, _, [] => by rw [checkBlockF.eq_def]; exact Appends.pure _
  | n + 1, Γ, s :: ss => by
    rw [checkBlockF.eq_def]; dsimp only
    exact Appends.bind (checkStmtF_appends env n Γ s) (fun Γ' => checkBlockF_appends env n Γ' ss)

end

/-! ## The statement helpers under an instance -/

section
variable {θ : List (String × Ty)}

mutual
/-- A statement with no type alias in it, as the parser makes a body: `type`
inside a function is P074. -/
def Stmt.typeFree : Stmt → Bool
  | .type_ _ _ => false
  | .if_ _ _ _ t e => blockTypeFree t && elseTypeFree e
  | .loop _ b => blockTypeFree b
  | .for_ _ _ b => blockTypeFree b
  | .match_ _ cs d => casesTypeFree cs && elseTypeFree d
  | _ => true

def blockTypeFree : List Stmt → Bool
  | [] => true
  | s :: ss => s.typeFree && blockTypeFree ss

def casesTypeFree : List Case → Bool
  | [] => true
  | .atom _ b :: cs | .term _ _ b :: cs => blockTypeFree b && casesTypeFree cs

def elseTypeFree : Option (List Stmt) → Bool
  | none => true
  | some b => blockTypeFree b
end

@[simp] theorem Ctx.ordered_inst (Γ : Ctx) : (Γ.inst θ).ordered = fun _ => false := by
  funext n; rfl

mutual

theorem Ty.unordered_novar (P : String → Bool) : ∀ (t : Ty), t.containsVar = false → t.unordered P = false
  | .var _, h => by simp [Ty.containsVar] at h
  | .list t, h | .set t, h | .option t, h | .result t, h | .tensor t _, h | .term _ t, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unordered]; exact Ty.unordered_novar P t h
  | .map k v, h => by
    simp only [Ty.containsVar, Bool.or_eq_false_iff] at h
    simp [Ty.unordered, Ty.unordered_novar P k h.1, Ty.unordered_novar P v h.2]
  | .tuple ts, h => by simp only [Ty.containsVar] at h; simp only [Ty.unordered]; exact Tys.unordered_novar P ts h
  | .struct fs, h | .table fs, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unordered]; exact Fields.unordered_novar P fs h
  | .enum vs, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unordered]; exact Variants.unordered_novar P vs h
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _ | .i64, _
  | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .data, _ | .error, _
  | .atom _, _ | .recvar _, _ | .mu _, _ => by simp [Ty.unordered]

theorem Tys.unordered_novar (P : String → Bool) : ∀ (ts : Tys), ts.containsVar = false → ts.unordered P = false
  | .nil, _ => rfl
  | .cons t ts, h => by
    simp only [Tys.containsVar, Bool.or_eq_false_iff] at h
    simp [Tys.unordered, Ty.unordered_novar P t h.1, Tys.unordered_novar P ts h.2]

theorem Fields.unordered_novar (P : String → Bool) : ∀ (fs : Fields), fs.containsVar = false →
    fs.unordered P = false
  | .nil, _ => rfl
  | .cons _ t fs, h => by
    simp only [Fields.containsVar, Bool.or_eq_false_iff] at h
    simp [Fields.unordered, Ty.unordered_novar P t h.1, Fields.unordered_novar P fs h.2]

theorem Variants.unordered_novar (P : String → Bool) : ∀ (vs : Variants), vs.containsVar = false →
    vs.unordered P = false
  | .nil, _ => rfl
  | .atom _ vs, h => by
    simp only [Variants.containsVar] at h; simp only [Variants.unordered]; exact Variants.unordered_novar P vs h
  | .term _ t vs, h => by
    simp only [Variants.containsVar, Bool.or_eq_false_iff] at h
    simp [Variants.unordered, Ty.unordered_novar P t h.1, Variants.unordered_novar P vs h.2]

end

mutual

theorem Ty.unorderedKey_novar (P : String → Bool) : ∀ (t : Ty), t.containsVar = false →
    t.unorderedKey P = false
  | .set t, h => by
    simp only [Ty.containsVar] at h
    simp [Ty.unorderedKey, Ty.unordered_novar P t h, Ty.unorderedKey_novar P t h]
  | .map k v, h => by
    simp only [Ty.containsVar, Bool.or_eq_false_iff] at h
    simp [Ty.unorderedKey, Ty.unordered_novar P k h.1, Ty.unorderedKey_novar P k h.1,
      Ty.unorderedKey_novar P v h.2]
  | .list t, h | .option t, h | .result t, h | .tensor t _, h | .term _ t, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unorderedKey]; exact Ty.unorderedKey_novar P t h
  | .tuple ts, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unorderedKey]; exact Tys.unorderedKey_novar P ts h
  | .struct fs, h | .table fs, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unorderedKey]; exact Fields.unorderedKey_novar P fs h
  | .enum vs, h => by
    simp only [Ty.containsVar] at h; simp only [Ty.unorderedKey]; exact Variants.unorderedKey_novar P vs h
  | .var _, _ | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _ | .u64, _
  | .i64, _ | .index, _ | .offset, _ | .f32, _ | .f64, _ | .int, _ | .string, _ | .data, _
  | .error, _ | .atom _, _ | .recvar _, _ | .mu _, _ => by simp [Ty.unorderedKey]

theorem Tys.unorderedKey_novar (P : String → Bool) : ∀ (ts : Tys), ts.containsVar = false →
    ts.unorderedKey P = false
  | .nil, _ => rfl
  | .cons t ts, h => by
    simp only [Tys.containsVar, Bool.or_eq_false_iff] at h
    simp [Tys.unorderedKey, Ty.unorderedKey_novar P t h.1, Tys.unorderedKey_novar P ts h.2]

theorem Fields.unorderedKey_novar (P : String → Bool) : ∀ (fs : Fields), fs.containsVar = false →
    fs.unorderedKey P = false
  | .nil, _ => rfl
  | .cons _ t fs, h => by
    simp only [Fields.containsVar, Bool.or_eq_false_iff] at h
    simp [Fields.unorderedKey, Ty.unorderedKey_novar P t h.1, Fields.unorderedKey_novar P fs h.2]

theorem Variants.unorderedKey_novar (P : String → Bool) : ∀ (vs : Variants), vs.containsVar = false →
    vs.unorderedKey P = false
  | .nil, _ => rfl
  | .atom _ vs, h => by
    simp only [Variants.containsVar] at h; simp only [Variants.unorderedKey]
    exact Variants.unorderedKey_novar P vs h
  | .term _ t vs, h => by
    simp only [Variants.containsVar, Bool.or_eq_false_iff] at h
    simp [Variants.unorderedKey, Ty.unorderedKey_novar P t h.1, Variants.unorderedKey_novar P vs h.2]

end

mutual

/-- A type parameter left in an instance is one with no argument, so one the
generic had not ordered either. -/
theorem Ty.unordered_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (t : Ty), (t.inst θ).unordered (fun _ => false) = true → t.unordered ok = true
  | .var n, h => by
    simp only [Ty.inst] at h
    cases hl : θ.lookup n with
    | none =>
      simp only [Ty.unordered]
      cases ho : ok n
      · rfl
      · have := hok n ho; simp [hl] at this
    | some u =>
      simp only [hl, Option.getD_some] at h
      rw [Ty.unordered_novar _ u (g.lookup hl).1] at h
      cases h
  | .list t, h | .set t, h | .option t, h | .result t, h | .tensor t _, h | .term _ t, h => by
    simp only [Ty.inst, Ty.unordered] at h ⊢; exact Ty.unordered_inst g ok hok t h
  | .map k v, h => by
    simp only [Ty.inst, Ty.unordered, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unordered_inst g ok hok k) (Ty.unordered_inst g ok hok v)
  | .tuple ts, h => by
    simp only [Ty.inst, Ty.unordered] at h ⊢; exact Tys.unordered_inst g ok hok ts h
  | .struct fs, h | .table fs, h => by
    simp only [Ty.inst, Ty.unordered] at h ⊢; exact Fields.unordered_inst g ok hok fs h
  | .enum vs, h => by
    simp only [Ty.inst, Ty.unordered] at h ⊢; exact Variants.unordered_inst g ok hok vs h
  | .bool, h | .u8, h | .i8, h | .u16, h | .i16, h | .u32, h | .i32, h | .u64, h | .i64, h
  | .index, h | .offset, h | .f32, h | .f64, h | .int, h | .string, h | .data, h | .error, h
  | .atom _, h | .recvar _, h | .mu _, h => by simp [Ty.inst, Ty.unordered] at h

theorem Tys.unordered_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (ts : Tys), (ts.inst θ).unordered (fun _ => false) = true → ts.unordered ok = true
  | .nil, h => by simp [Tys.inst, Tys.unordered] at h
  | .cons t ts, h => by
    simp only [Tys.inst, Tys.unordered, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unordered_inst g ok hok t) (Tys.unordered_inst g ok hok ts)

theorem Fields.unordered_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (fs : Fields), (fs.inst θ).unordered (fun _ => false) = true → fs.unordered ok = true
  | .nil, h => by simp [Fields.inst, Fields.unordered] at h
  | .cons _ t fs, h => by
    simp only [Fields.inst, Fields.unordered, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unordered_inst g ok hok t) (Fields.unordered_inst g ok hok fs)

theorem Variants.unordered_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (vs : Variants), (vs.inst θ).unordered (fun _ => false) = true → vs.unordered ok = true
  | .nil, h => by simp [Variants.inst, Variants.unordered] at h
  | .atom _ vs, h => by
    simp only [Variants.inst, Variants.unordered] at h ⊢; exact Variants.unordered_inst g ok hok vs h
  | .term _ t vs, h => by
    simp only [Variants.inst, Variants.unordered, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unordered_inst g ok hok t) (Variants.unordered_inst g ok hok vs)

end

mutual

theorem Ty.unorderedKey_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (t : Ty), (t.inst θ).unorderedKey (fun _ => false) = true → t.unorderedKey ok = true
  | .var n, h => by
    simp only [Ty.inst] at h
    cases hl : θ.lookup n with
    | none => simp [hl, Ty.unorderedKey] at h
    | some u =>
      simp only [hl, Option.getD_some] at h
      rw [Ty.unorderedKey_novar _ u (g.lookup hl).1] at h
      cases h
  | .set t, h => by
    simp only [Ty.inst, Ty.unorderedKey, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unordered_inst g ok hok t) (Ty.unorderedKey_inst g ok hok t)
  | .map k v, h => by
    simp only [Ty.inst, Ty.unorderedKey, Bool.or_eq_true] at h ⊢
    exact h.imp (fun h => h.imp (Ty.unordered_inst g ok hok k) (Ty.unorderedKey_inst g ok hok k))
      (Ty.unorderedKey_inst g ok hok v)
  | .list t, h | .option t, h | .result t, h | .tensor t _, h | .term _ t, h => by
    simp only [Ty.inst, Ty.unorderedKey] at h ⊢; exact Ty.unorderedKey_inst g ok hok t h
  | .tuple ts, h => by
    simp only [Ty.inst, Ty.unorderedKey] at h ⊢; exact Tys.unorderedKey_inst g ok hok ts h
  | .struct fs, h | .table fs, h => by
    simp only [Ty.inst, Ty.unorderedKey] at h ⊢; exact Fields.unorderedKey_inst g ok hok fs h
  | .enum vs, h => by
    simp only [Ty.inst, Ty.unorderedKey] at h ⊢; exact Variants.unorderedKey_inst g ok hok vs h
  | .bool, h | .u8, h | .i8, h | .u16, h | .i16, h | .u32, h | .i32, h | .u64, h | .i64, h
  | .index, h | .offset, h | .f32, h | .f64, h | .int, h | .string, h | .data, h | .error, h
  | .atom _, h | .recvar _, h | .mu _, h => by simp [Ty.inst, Ty.unorderedKey] at h

theorem Tys.unorderedKey_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (ts : Tys), (ts.inst θ).unorderedKey (fun _ => false) = true → ts.unorderedKey ok = true
  | .nil, h => by simp [Tys.inst, Tys.unorderedKey] at h
  | .cons t ts, h => by
    simp only [Tys.inst, Tys.unorderedKey, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unorderedKey_inst g ok hok t) (Tys.unorderedKey_inst g ok hok ts)

theorem Fields.unorderedKey_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (fs : Fields), (fs.inst θ).unorderedKey (fun _ => false) = true → fs.unorderedKey ok = true
  | .nil, h => by simp [Fields.inst, Fields.unorderedKey] at h
  | .cons _ t fs, h => by
    simp only [Fields.inst, Fields.unorderedKey, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unorderedKey_inst g ok hok t) (Fields.unorderedKey_inst g ok hok fs)

theorem Variants.unorderedKey_inst (g : Ground θ) (ok : String → Bool)
    (hok : ∀ n, ok n = true → (θ.lookup n).isSome = true) :
    ∀ (vs : Variants), (vs.inst θ).unorderedKey (fun _ => false) = true → vs.unorderedKey ok = true
  | .nil, h => by simp [Variants.inst, Variants.unorderedKey] at h
  | .atom _ vs, h => by
    simp only [Variants.inst, Variants.unorderedKey] at h ⊢
    exact Variants.unorderedKey_inst g ok hok vs h
  | .term _ t vs, h => by
    simp only [Variants.inst, Variants.unorderedKey, Bool.or_eq_true] at h ⊢
    exact h.imp (Ty.unorderedKey_inst g ok hok t) (Variants.unorderedKey_inst g ok hok vs)

end

/-- F061 holds of an instance where it held of the generic. -/
theorem unorderedKey_inst (g : Ground θ) {Γ : Ctx} (r : Respects θ Γ) {t : Ty}
    (h : t.unorderedKey Γ.ordered = false) : (t.inst θ).unorderedKey (Γ.inst θ).ordered = false := by
  rw [Ctx.ordered_inst]
  cases hc : (t.inst θ).unorderedKey (fun _ => false)
  · rfl
  · have hok : ∀ n, Γ.ordered n = true → (θ.lookup n).isSome = true := by
      intro n hn
      simp only [Ctx.ordered, Option.isSome_iff_exists] at hn
      obtain ⟨b, hb⟩ := hn
      obtain ⟨u, hu, -⟩ := r n b hb
      simp [hu]
    rw [Ty.unorderedKey_inst g _ hok t hc] at h
    cases h


/-- Bindings with type arguments substituted into their types. -/
def instBinds (θ : List (String × Ty)) (Δ : List (String × Ty)) : List (String × Ty) :=
  Δ.map fun (x, t) => (x, t.inst θ)

theorem Ctx.inst_bindAllAs (Γ : Ctx) (k : Kind) : ∀ (Δ : List (String × Ty)),
    (Γ.bindAllAs Δ k).inst θ = (Γ.inst θ).bindAllAs (instBinds θ Δ) k
  | [] => rfl
  | (x, t) :: Δ => by
    simp only [Ctx.bindAllAs, List.foldl, instBinds, List.map] at *
    have := Ctx.inst_bindAllAs (Γ.bind x t k) k Δ
    simp only [Ctx.bindAllAs, instBinds] at this
    rw [this]; rfl

theorem Ctx.inst_bindAll (Γ : Ctx) (Δ : List (String × Ty)) :
    (Γ.bindAll Δ).inst θ = (Γ.inst θ).bindAll (instBinds θ Δ) := by
  have := Ctx.inst_bindAllAs (θ := θ) Γ .let_ Δ
  simpa [Ctx.bindAllAs, Ctx.bindAll] using this

theorem Ctx.bindAllAs_bounds (Γ : Ctx) (k : Kind) : ∀ (Δ : List (String × Ty)),
    (Γ.bindAllAs Δ k).bounds = Γ.bounds ∧ (Γ.bindAllAs Δ k).sigs = Γ.sigs
  | [] => ⟨rfl, rfl⟩
  | (x, t) :: Δ => by
    simp only [Ctx.bindAllAs, List.foldl] at *
    exact Ctx.bindAllAs_bounds (Γ.bind x t k) k Δ

theorem Ctx.bindAll_bounds (Γ : Ctx) (Δ : List (String × Ty)) :
    (Γ.bindAll Δ).bounds = Γ.bounds ∧ (Γ.bindAll Δ).sigs = Γ.sigs := by
  have := Ctx.bindAllAs_bounds Γ .let_ Δ
  simpa [Ctx.bindAllAs, Ctx.bindAll] using this

theorem bindOpt_inst (Γ : Ctx) (x : Option String) (t : Ty) :
    (bindOpt Γ x t).inst θ = bindOpt (Γ.inst θ) x (t.inst θ) := by
  cases x <;> rfl

theorem patBindings_inst (g : Ground θ) {p : Pat} {t : Ty} {Δ : List (String × Ty)}
    (h : patBindings p t = .ok Δ) : patBindings p (t.inst θ) = .ok (instBinds θ Δ) := by
  unfold patBindings at *
  rw [Ty.expose_inst g]
  cases p with
  | name x =>
    simp only [pure, Except.pure, Except.ok.injEq] at h ⊢; subst h; rfl
  | tuple xs =>
    cases he : t.expose <;> simp only [he, Ty.inst] at h ⊢ <;> try (simp [throw, throwThe, MonadExceptOf.throw] at h; done)
    rename_i ts
    split at h
    · rename_i hl
      simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
      rw [tysList_inst]
      simp only [List.length_map, hl, ↓reduceIte, pure, Except.pure, Except.ok.injEq]
      simp only [instBinds, List.zip_map_right]
      rfl
    · simp [throw, throwThe, MonadExceptOf.throw] at h
  | struct fs =>
    cases he : t.expose <;> simp only [he, Ty.inst] at h ⊢ <;> try (simp [throw, throwThe, MonadExceptOf.throw] at h; done)
    rename_i ts
    simp only [fieldsList_inst, lookup_map_snd, Option.isNone_map, List.any_map,
      Function.comp_def]
    split at h
    · simp [throw, throwThe, MonadExceptOf.throw] at h
    · rename_i h1
      split at h
      · simp [throw, throwThe, MonadExceptOf.throw] at h
      · rename_i h2
        simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
        simp only [h1, h2, Bool.false_eq_true, ↓reduceIte, instBinds, List.map_filterMap,
          Option.map_map, Function.comp_def, pure, Except.pure]
  | atom a =>
    cases he : t.expose <;> simp only [he, Ty.inst] at h ⊢ <;> try (simp [throw, throwThe, MonadExceptOf.throw] at h; done)
    split at h
    · simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
      rename_i hab; simp [hab]; rfl
    · simp [throw, throwThe, MonadExceptOf.throw] at h
  | term a x =>
    cases he : t.expose <;> simp only [he, Ty.inst] at h ⊢ <;> try (simp [throw, throwThe, MonadExceptOf.throw] at h; done)
    split at h
    · simp only [pure, Except.pure, Except.ok.injEq] at h; subst h
      rename_i hab; simp [hab]; rfl
    · simp [throw, throwThe, MonadExceptOf.throw] at h


theorem walk_inst {c : Ty} {names : List String} {Δ : List (String × Ty)}
    (h : walk c names = .ok Δ) : walk (c.inst θ) names = .ok (instBinds θ Δ) := by
  unfold walk at *
  cases c <;> simp only [Ty.inst] at h ⊢ <;> (try (simp [throw, throwThe, MonadExceptOf.throw] at h; done))
  all_goals
    split at h <;> (try (simp [throw, throwThe, MonadExceptOf.throw] at h; done)) <;>
      (simp only [pure, Except.pure, Except.ok.injEq] at h; subst h) <;>
      (try split) <;> simp_all [instBinds, Ty.inst, pure, Except.pure]
  all_goals (split <;> first | omega | simp [Ty.inst])

theorem bindPat_inst (g : Ground θ) {Γ : Ctx} {p : Pat} {t : Ty} {k : Kind} {Γ' : Ctx}
    (h : SQuiet (bindPat Γ p t k) Γ') :
    SQuiet (bindPat (Γ.inst θ) p (t.inst θ) k) (Γ'.inst θ) ∧ Γ'.bounds = Γ.bounds ∧
      Γ'.sigs = Γ.sigs := by
  unfold bindPat at *
  cases hp : patBindings p t with
  | error c =>
    simp only [hp] at h
    exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | ok Δ =>
    simp only [hp, SQuiet.pure_iff] at h; subst h
    rw [patBindings_inst g hp]
    exact ⟨by rw [Ctx.inst_bindAllAs]; exact SQuiet.pure _, Ctx.bindAllAs_bounds Γ k Δ⟩

end

section
variable {θ : List (String × Ty)} {env env' : InferEnv}

theorem bindChecked_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx} {p : Pat}
    {hint : Option Hint} {e : Expr} {k : Kind} {Γ' : Ctx}
    (h : SQuiet (bindChecked env n Γ p hint e k) Γ') (r : Respects θ Γ) (sg : SigsOk Γ.sigs) :
    SQuiet (bindChecked env' n (Γ.inst θ) p hint e k) (Γ'.inst θ) ∧ Γ'.bounds = Γ.bounds ∧
      Γ'.sigs = Γ.sigs := by
  cases hint with
  | some hh =>
    simp only [bindChecked, convertReporting] at h ⊢
    obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hrt
    rw [Quiet.liftE_iff] at hq
    dsimp only at h
    split at h
    · exact (SQuiet.push_bind (fun _ => bindPat_appends _ _ _ _) h).elim
    · rename_i hu
      obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (fun _ => bindPat_appends _ _ _ _) h
      obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
      obtain ⟨hb, hbd⟩ := bindPat_inst g h
      refine ⟨SQuiet.bind (SQuiet.chk (Quiet.liftE_iff.mpr (Ctx.hint_inst g hq))) ?_, hbd⟩
      dsimp only
      rw [unorderedKey_inst g r (by simpa using hu)]
      simp only [Bool.false_eq_true, ↓reduceIte]
      exact SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ e t hc r sg)) hb
  | none =>
    simp only [bindChecked] at h ⊢
    obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hrt
    obtain ⟨hb, hbd⟩ := bindPat_inst g h
    exact ⟨SQuiet.bind (SQuiet.chk (synthF_inst g H n Γ e t hq r sg)) hb, hbd⟩

theorem forBindings_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx}
    {names : List String} {coll : Expr} {r0 : Option (List (String × Ty))}
    (h : SQuiet (forBindings env n Γ names coll) r0) (r : Respects θ Γ) (sg : SigsOk Γ.sigs) :
    ∃ Δ, r0 = some Δ ∧ SQuiet (forBindings env' n (Γ.inst θ) names coll) (some (instBinds θ Δ)) := by
  unfold forBindings at *
  split at h
  · rename_i hp
    obtain ⟨Δ, rfl, hq⟩ := SQuiet.chk_inv h
    obtain ⟨t, ht, hw⟩ := hq.bind_inv
    rw [Quiet.liftE_iff] at hw
    refine ⟨Δ, rfl, ?_⟩
    simp only [hp, ↓reduceIte]
    exact SQuiet.chk (Quiet.bind (synthF_inst g H n _ coll t ht r sg)
      (Quiet.liftE_iff.mpr (by rw [Ty.expose_inst g]; exact walk_inst hw)))
  · exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim

theorem setValue_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx} {t : Ty}
    {op : Option BinOp} {e : Expr} (h : SQuiet (setValue env n Γ t op e) ()) (r : Respects θ Γ)
    (sg : SigsOk Γ.sigs) : SQuiet (setValue env' n (Γ.inst θ) (t.inst θ) op e) () := by
  unfold setValue at *
  cases op with
  | none =>
    obtain ⟨_, hc, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    exact SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ e t hc r sg)) (SQuiet.pure _)
  | some op =>
    dsimp only at h ⊢
    obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    refine SQuiet.bind (SQuiet.chk (checkF_inst g H n _ e t hc r sg)) ?_
    split at h
    · simp at h
    · rename_i ha
      rw [SQuiet.reportIf_iff] at h
      have ha' := arithAdmits_inst r (by simpa using ha)
      simp only [ha', Bool.not_true, Bool.false_eq_true, ↓reduceIte, SQuiet.reportIf_iff]
      simpa using fits_inst g (Γ := Γ) (by simpa using h)

theorem setIndex_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx} {t : Ty}
    {k : Expr} {mode : Option IdxMode} {op : Option BinOp} {e : Expr}
    (h : SQuiet (setIndex env n Γ t k mode op e) ()) (r : Respects θ Γ) (sg : SigsOk Γ.sigs) :
    SQuiet (setIndex env' n (Γ.inst θ) (t.inst θ) k mode op e) () := by
  unfold setIndex at *
  cases mode with
  | none =>
    cases op with
    | some _ => simp at h
    | none =>
      dsimp only at h ⊢
      rw [Ty.expose_inst g]
      split at h
      · rename_i kt vt hm
        rw [hm]
        obtain ⟨_, hk, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
        obtain ⟨_, hv, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        obtain ⟨_, -, hk⟩ := SQuiet.chk_inv hk
        obtain ⟨_, -, hv⟩ := SQuiet.chk_inv hv
        exact SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ k kt hk r sg))
          (SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ e vt hv r sg)) (SQuiet.pure _))
      · simp at h
  | some m =>
    dsimp only at h ⊢
    cases hi : indexTypes t with
    | error c => simp only [hi] at h; simp at h
    | ok p =>
      obtain ⟨et, kt⟩ := p
      simp only [hi] at h
      rw [indexTypes_inst g hi]
      split at h
      · simp at h
      · rename_i hv
        rw [isView_inst g hi]
        simp only [hv, Bool.false_eq_true, ↓reduceIte]
        obtain ⟨_, hk, h⟩ := SQuiet.bind_inv (fun _ => setValue_appends _ _ _ _ _ _) h
        obtain ⟨_, -, hk⟩ := SQuiet.chk_inv hk
        exact SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ k kt hk r sg))
          (setValue_inst g H h r sg)

theorem setSteps_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx}
    {op : Option BinOp} {e : Expr} (r : Respects θ Γ) (sg : SigsOk Γ.sigs) :
    ∀ (t : Ty) (steps : List Step), SQuiet (setSteps env n Γ t op e steps) () →
      SQuiet (setSteps env' n (Γ.inst θ) (t.inst θ) op e steps) ()
  | t, [], h => by simp only [setSteps] at h ⊢; exact setValue_inst g H h r sg
  | t, [.index k mode], h => by simp only [setSteps] at h ⊢; exact setIndex_inst g H h r sg
  | t, .field s :: rest, h => by
    simp only [setSteps] at h ⊢
    split at h
    · rename_i u hu
      rw [fieldType_inst g hu]
      exact setSteps_inst g H r sg u rest h
    · simp at h
  | t, .index _ none :: _ :: _, h => by simp [setSteps] at h
  | t, .index k (some m) :: st :: rest, h => by
    simp only [setSteps] at h ⊢
    split at h
    · simp at h
    · rename_i et kt hi
      rw [indexTypes_inst g hi]
      obtain ⟨_, hk, h⟩ := SQuiet.bind_inv (fun _ => setSteps_appends _ _ _ _ _ _ _) h
      obtain ⟨_, -, hk⟩ := SQuiet.chk_inv hk
      exact SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ k kt hk r sg))
        (setSteps_inst g H r sg et (st :: rest) h)

theorem reportModes_inst (g : Ground θ) {Γ : Ctx} : ∀ (ms : List IdxMode),
    SQuiet (reportModes Γ ms) () → SQuiet (reportModes (Γ.inst θ) ms) ()
  | [], _ => SQuiet.pure _
  | m :: ms, h => by
    simp only [reportModes] at h ⊢
    obtain ⟨_, hm, h⟩ := SQuiet.bind_inv (fun _ => reportModes_appends _ _) h
    rw [SQuiet.reportIf_iff] at hm
    refine SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr ?_) (reportModes_inst g ms h)
    simpa using Ctx.fitsMode_inst g (Γ := Γ) (by simpa using hm)

theorem checkSet_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx} {x : String}
    {steps : List Step} {op : Option BinOp} {e : Expr}
    (h : SQuiet (checkSet env n Γ x steps op e) ()) (r : Respects θ Γ) (sg : SigsOk Γ.sigs) :
    SQuiet (checkSet env' n (Γ.inst θ) x steps op e) () := by
  unfold checkSet at *
  rw [Ctx.lookup_inst]
  cases hl : Γ.lookup x with
  | none => simp only [hl] at h; simp at h
  | some p =>
    obtain ⟨t, k⟩ := p
    simp only [hl, Option.map_some] at h ⊢
    obtain ⟨_, h1, h⟩ := SQuiet.bind_inv
      (fun _ => Appends.bind (reportModes_appends _ _) (fun _ => setSteps_appends _ _ _ _ _ _ _)) h
    obtain ⟨_, h2, h⟩ := SQuiet.bind_inv (fun _ => setSteps_appends _ _ _ _ _ _ _) h
    exact SQuiet.bind h1 (SQuiet.bind (reportModes_inst g _ h2) (setSteps_inst g H r sg t steps h))

theorem checkRet_inst (g : Ground θ) (H : InstEnv θ env env') {n : Nat} {Γ : Ctx}
    {e : Option Expr} (h : SQuiet (checkRet env n Γ e) ()) (r : Respects θ Γ) (sg : SigsOk Γ.sigs) :
    SQuiet (checkRet env' n (Γ.inst θ) e) () := by
  unfold checkRet at *
  simp only [Ctx.inst_ret]
  cases hr : Γ.ret <;> cases e <;> simp only [hr, Ret.inst] at h ⊢
  · exact SQuiet.pure _
  · simp at h
  · simp at h
  · rename_i t e
    obtain ⟨_, hc, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
    obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
    exact SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ e t hc r sg)) (SQuiet.pure _)

end

/-! ## Statements under an instance -/

/-- Show a continuation only appends: one step, or a block, `else` or `match`. -/
macro "app_cont" e:term:max n:term:max : tactic => `(tactic| (intro _; repeat' (first
  | with_reducible exact checkBlockF_appends $e $n _ _
  | with_reducible exact checkElse_appends $e $n _ _
  | with_reducible exact checkCases_appends $e $n _ _ _ _
  | stmt_step)))

theorem markConst_map (x : String) {θ : List (String × Ty)} : ∀ (vs : List (String × Ty × Kind)),
    markConst x (vs.map fun (y, t, k) => (y, t.inst θ, k)) =
      (markConst x vs).map fun (y, t, k) => (y, t.inst θ, k)
  | [] => rfl
  | (y, t, k) :: vs => by
    simp only [List.map, markConst]
    split <;> simp [markConst_map x vs]

/-- An enum's variants with type arguments substituted into their payloads. -/
def instVs (θ : List (String × Ty)) (vs : List (String × Option Ty)) : List (String × Option Ty) :=
  vs.map fun p => (p.1, p.2.map (·.inst θ))

theorem instVs_lookup (θ : List (String × Ty)) (vs : List (String × Option Ty)) (a : String) :
    (instVs θ vs).lookup a = (vs.lookup a).map (·.map (·.inst θ)) := by
  unfold instVs
  exact lookup_map_snd (fun p => p.map (·.inst θ)) a vs

@[simp] theorem Respects.bindOpt {θ : List (String × Ty)} {Γ : Ctx} {x : Option String} {t : Ty} :
    Respects θ (bindOpt Γ x t) ↔ Respects θ Γ := by cases x <;> rfl

@[simp] theorem bindOpt_sigs (Γ : Ctx) (x : Option String) (t : Ty) : (bindOpt Γ x t).sigs = Γ.sigs := by
  cases x <;> rfl

section
variable {θ : List (String × Ty)} {env env' : InferEnv}

mutual

theorem checkStmtF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx) (s : Stmt)
    (Γ' : Ctx), SQuiet (checkStmtF env n Γ s) Γ' → Respects θ Γ → SigsOk Γ.sigs → s.typeFree = true →
    SQuiet (checkStmtF env' n (Γ.inst θ) s) (Γ'.inst θ) ∧ Γ'.bounds = Γ.bounds ∧ Γ'.sigs = Γ.sigs
  | 0, _, _, _, h, _, _, _ => by
    simp only [checkStmtF] at h; exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | n + 1, Γ, s, Γ', h, r, sg, tf => by
    cases s
    case let_ p hint e =>
      simp only [checkStmtF] at h ⊢
      exact bindChecked_inst g H h r sg
    case var_ p hint v =>
      cases v with
      | some e => simp only [checkStmtF] at h ⊢; exact bindChecked_inst g H h r sg
      | none =>
        cases hint with
        | none =>
          simp only [checkStmtF, SQuiet.pure_iff] at h ⊢; subst h; exact ⟨rfl, rfl, rfl⟩
        | some hh =>
          simp only [checkStmtF, convertReporting] at h ⊢
          obtain ⟨rt, hrt, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
          obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hrt
          rw [Quiet.liftE_iff] at hq
          dsimp only at h
          obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => bindPat_appends _ _ _ _) h
          rw [SQuiet.reportIf_iff] at h1
          obtain ⟨hb, hbd⟩ := bindPat_inst g h
          refine ⟨SQuiet.bind (SQuiet.chk (Quiet.liftE_iff.mpr (Ctx.hint_inst g hq))) ?_, hbd⟩
          dsimp only
          exact SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr (unorderedKey_inst g r h1)) hb
    case set_ x steps op e =>
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, hs, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      exact ⟨SQuiet.bind (checkSet_inst g H hs r sg) (SQuiet.pure _), rfl, rfl⟩
    case fun_ f =>
      simp only [checkStmtF, SQuiet.pure_iff] at h ⊢; subst h; exact ⟨rfl, rfl, rfl⟩
    case ret e =>
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, hs, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      exact ⟨SQuiet.bind (checkRet_inst g H hs r sg) (SQuiet.pure _), rfl, rfl⟩
    case brk | cont =>
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      rw [SQuiet.reportIf_iff] at h1
      exact ⟨SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr h1) (SQuiet.pure _), rfl, rfl⟩
    case debuglog e =>
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv h1
      exact ⟨SQuiet.bind (SQuiet.chk (synthF_inst g H n _ e t hq r sg)) (SQuiet.pure _), rfl, rfl⟩
    case call e =>
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, h1, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv h1
      dsimp only at h
      obtain ⟨_, h2, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      rw [SQuiet.reportIf_iff] at h2
      refine ⟨SQuiet.bind (SQuiet.chk (synthF_inst g H n _ e t hq r sg)) ?_, rfl, rfl⟩
      dsimp only
      refine SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr ?_) (SQuiet.pure _)
      simp only [bne_eq_false_iff_eq] at h2 ⊢
      rw [h2]; rfl
    case type_ x hh => simp [Stmt.typeFree] at tf
    case const_ x hint e =>
      simp only [checkStmtF] at h ⊢
      obtain ⟨Γ1, hb, h⟩ := SQuiet.bind_inv (fun _ => by repeat' stmt_step) h
      obtain ⟨_, h88, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      obtain ⟨hb', hbd, hsg⟩ := bindChecked_inst g H (Γ := { Γ with constExpr := true }) hb r sg
      rw [SQuiet.reportIf_iff] at h88
      refine ⟨SQuiet.bind hb' ?_, hbd, hsg⟩
      have hvars : markConst x (Γ1.inst θ).vars = (markConst x Γ1.vars).map fun (y, t, k) => (y, t.inst θ, k) :=
        markConst_map x Γ1.vars
      refine SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr ?_) ?_
      · rw [hvars]
        show (match (Ctx.inst θ { Γ1 with constExpr := false, vars := markConst x Γ1.vars }).lookup x with
          | some (t, _) => t.containsVar
          | none => false) = false
        rw [Ctx.lookup_inst]
        revert h88
        cases Ctx.lookup { Γ1 with constExpr := false, vars := markConst x Γ1.vars } x with
        | none => intro _; rfl
        | some p =>
          obtain ⟨t, k⟩ := p
          intro h88
          simp only [Option.map_some] at h88 ⊢
          rw [Ty.inst_novar θ t h88]; exact h88
      · show SQuiet (Pure.pure _) _
        rw [SQuiet.pure_iff]
        simp only [Ctx.inst] at hvars ⊢
        rw [hvars]
    case loop c body =>
      simp only [Stmt.typeFree] at tf
      cases c with
      | none =>
        simp only [checkStmtF] at h ⊢
        obtain ⟨_, -, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, hb, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        exact ⟨SQuiet.bind (SQuiet.pure ()) (SQuiet.bind
          (checkBlockF_inst g H n { Γ with inLoop := true } body hb r sg tf) (SQuiet.pure _)), rfl, rfl⟩
      | some c =>
        simp only [checkStmtF] at h ⊢
        obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, hb, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        obtain ⟨_, hc, -⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hc
        obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
        exact ⟨SQuiet.bind (SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ c .bool hc r sg)) (SQuiet.pure ()))
          (SQuiet.bind (checkBlockF_inst g H n { Γ with inLoop := true } body hb r sg tf)
            (SQuiet.pure _)), rfl, rfl⟩
    case if_ c a b t e =>
      simp only [Stmt.typeFree, Bool.and_eq_true] at tf
      obtain ⟨tft, tfe⟩ := tf
      cases a <;> cases b
      case none.none =>
        simp only [checkStmtF] at h ⊢
        obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, hb, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨_, he, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        obtain ⟨_, -, hc⟩ := SQuiet.chk_inv hc
        exact ⟨SQuiet.bind (SQuiet.chk (checkF_inst g H n Γ c .bool hc r sg))
          (SQuiet.bind (checkBlockF_inst g H n Γ t hb r sg tft)
            (SQuiet.bind (checkElse_inst g H n Γ e he r sg tfe) (SQuiet.pure _))), rfl, rfl⟩
      all_goals
        simp only [checkStmtF] at h ⊢
        obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
        obtain ⟨ct, rfl, hq⟩ := SQuiet.chk_inv hc
        try dsimp only at h
        obtain ⟨_, hm, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
        rw [SQuiet.pure_iff] at h; subst h
        refine ⟨SQuiet.bind (SQuiet.chk (synthF_inst g H n Γ c ct hq r sg)) ?_, rfl, rfl⟩
        refine SQuiet.bind (a := ()) ?_ (SQuiet.pure _)
        try dsimp only
        rw [Ty.expose_inst g]
        split at hm
        · rename_i p hp
          rw [hp]
          obtain ⟨_, hb, hm⟩ := SQuiet.bind_inv (by app_cont env n) hm
          obtain ⟨_, h94, he⟩ := SQuiet.bind_inv (by app_cont env n) hm
          rw [SQuiet.reportIf_iff] at h94
          refine SQuiet.bind (a := ()) ?_ (SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr h94)
            (checkElse_inst g H n Γ e he r sg tfe))
          have := checkBlockF_inst g H n _ t hb (by simpa using r) (by simpa using sg) tft
          rwa [bindOpt_inst] at this
        · rename_i p hp
          rw [hp]
          try dsimp only
          split at hm
          · simp at hm
          · rename_i hn
            simp only [hn, Bool.false_eq_true, ↓reduceIte]
            obtain ⟨_, hb, he⟩ := SQuiet.bind_inv (by app_cont env n) hm
            refine SQuiet.bind (a := ()) ?_ ?_
            · have := checkBlockF_inst g H n _ t hb (by simpa using r) (by simpa using sg) tft
              rwa [bindOpt_inst] at this
            · have := checkElse_inst g H n _ e he (by simpa using r) (by simpa using sg) tfe
              rwa [bindOpt_inst] at this
        · simp at hm
    case match_ e cs dflt =>
      simp only [Stmt.typeFree, Bool.and_eq_true] at tf
      obtain ⟨tfc, tfd⟩ := tf
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, hc, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      obtain ⟨t, rfl, hq⟩ := SQuiet.chk_inv hc
      try dsimp only at h
      obtain ⟨_, hm, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      refine ⟨SQuiet.bind (SQuiet.chk (synthF_inst g H n Γ e t hq r sg)) ?_, rfl, rfl⟩
      refine SQuiet.bind (a := ()) ?_ (SQuiet.pure _)
      try dsimp only
      rw [Ty.expose_inst g]
      split at hm
      · rename_i vs hvs
        rw [hvs]
        try dsimp only
        obtain ⟨cov, hcs, hm⟩ := SQuiet.bind_inv (by app_cont env n) hm
        obtain ⟨_, hd, h100⟩ := SQuiet.bind_inv (by app_cont env n) hm
        rw [SQuiet.reportIf_iff] at h100
        have hcs' := checkCases_inst g H n Γ vs.toList cs [] cov hcs r sg tfc
        rw [instVs, ← Variants.toList_inst] at hcs'
        refine SQuiet.bind hcs' (SQuiet.bind (a := ()) (checkElse_inst g H n Γ dflt hd r sg tfd)
          (SQuiet.reportIf_iff.mpr ?_))
        rw [Variants.toList_inst]
        simpa [List.any_map, Function.comp_def] using h100
      · simp at hm
    case for_ names coll body =>
      simp only [Stmt.typeFree] at tf
      simp only [checkStmtF] at h ⊢
      obtain ⟨_, hf, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      obtain ⟨Δ, rfl, hf'⟩ := forBindings_inst g H hf r sg
      dsimp only at h
      obtain ⟨_, hb, h⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) h
      rw [SQuiet.pure_iff] at h; subst h
      refine ⟨SQuiet.bind hf' (SQuiet.bind (a := ()) ?_ (SQuiet.pure _)), rfl, rfl⟩
      dsimp only
      have := checkBlockF_inst g H n ({ Γ with inLoop := true }.bindAll Δ) body hb
        (by have := Ctx.bindAll_bounds { Γ with inLoop := true } Δ
            intro m b hb; rw [this.1] at hb; exact r m b hb)
        (by rw [(Ctx.bindAll_bounds _ Δ).2]; exact sg) tf
      rw [Ctx.inst_bindAll] at this
      exact this

theorem checkCases_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (vs : List (String × Option Ty)) (cs : List Case) (cov cov' : List String),
    SQuiet (checkCases env n Γ vs cs cov) cov' → Respects θ Γ → SigsOk Γ.sigs →
    casesTypeFree cs = true → SQuiet (checkCases env' n (Γ.inst θ) (instVs θ vs) cs cov) cov'
  | 0, _, _, _, _, _, h, _, _, _ => by
    rw [checkCases.eq_def] at h; exact (SQuiet.push_bind (fun _ => Appends.pure _) h).elim
  | _ + 1, _, _, [], cov, cov', h, _, _, _ => by
    rw [checkCases.eq_def] at h ⊢; exact h
  | n + 1, Γ, vs, c :: cs, cov, cov', h, r, sg, tf => by
    have tfb : blockTypeFree c.body = true ∧ casesTypeFree cs = true := by
      cases c <;> simpa [casesTypeFree, Case.body] using tf
    rw [checkCases.eq_def] at h ⊢
    dsimp only at h ⊢
    split at h
    · rename_i hcov
      simp only [hcov, ↓reduceIte]
      exact (SQuiet.push_bind (by app_cont env n) h).elim
    · rename_i hcov
      simp only [hcov, Bool.false_eq_true, ↓reduceIte]
      obtain ⟨_, hb, h⟩ := SQuiet.bind_inv (by app_cont env n) h
      refine SQuiet.bind (a := ()) ?_ (checkCases_inst g H n Γ vs cs _ cov' h r sg tfb.2)
      rw [instVs_lookup]
      split at hb
      · rename_i _ b p _ hp
        simp only [hp, Option.map_some]
        obtain ⟨_, h98, hb⟩ := SQuiet.bind_inv (by app_cont env n) hb
        rw [SQuiet.reportIf_iff] at h98
        refine SQuiet.bind (a := ()) (SQuiet.reportIf_iff.mpr (by simpa using h98)) ?_
        exact checkBlockF_inst g H n Γ b hb r sg (by simpa [Case.body] using tfb.1)
      · rename_i _ x b p _ hp
        simp only [hp, Option.map_some]
        have := checkBlockF_inst g H n _ b hb r sg (by simpa [Case.body] using tfb.1)
        exact this
      · exact (SQuiet.push_bind (by app_cont env n) hb).elim
      · exact (SQuiet.push_bind (by app_cont env n) hb).elim

theorem checkElse_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (e : Option (List Stmt)), SQuiet (checkElse env n Γ e) () → Respects θ Γ → SigsOk Γ.sigs →
    elseTypeFree e = true → SQuiet (checkElse env' n (Γ.inst θ) e) ()
  | n, Γ, some e, h, r, sg, tf => by
    rw [checkElse.eq_def] at h ⊢
    exact checkBlockF_inst g H n Γ e h r sg (by simpa [elseTypeFree] using tf)
  | _, _, none, _, _, _, _ => by rw [checkElse.eq_def]; exact SQuiet.pure _

theorem checkBlockF_inst (g : Ground θ) (H : InstEnv θ env env') : ∀ (n : Nat) (Γ : Ctx)
    (ss : List Stmt), SQuiet (checkBlockF env n Γ ss) () → Respects θ Γ → SigsOk Γ.sigs →
    blockTypeFree ss = true → SQuiet (checkBlockF env' n (Γ.inst θ) ss) ()
  | 0, _, _, h, _, _, _ => by rw [checkBlockF.eq_def] at h; simp at h
  | _ + 1, _, [], _, _, _, _ => by rw [checkBlockF.eq_def]; exact SQuiet.pure _
  | n + 1, Γ, s :: ss, h, r, sg, tf => by
    simp only [blockTypeFree, Bool.and_eq_true] at tf
    rw [checkBlockF.eq_def] at h ⊢
    dsimp only at h ⊢
    obtain ⟨Γ', hs, h⟩ := SQuiet.bind_inv (fun _ => checkBlockF_appends _ _ _ _) h
    obtain ⟨hs', hb, hsg⟩ := checkStmtF_inst g H n Γ s Γ' hs r sg tf.1
    exact SQuiet.bind hs' (checkBlockF_inst g H n Γ' ss h
      (by intro m b hm; rw [hb] at hm; exact r m b hm) (by rw [hsg]; exact sg) tf.2)

end

end

/-! ## A generic function and its instances -/

theorem lookup_mem {α β} [BEq α] [LawfulBEq α] {n : α} {b : β} :
    ∀ {l : List (α × β)}, l.lookup n = some b → (n, b) ∈ l
  | [], h => by simp at h
  | (m, c) :: l, h => by
    by_cases e : n = m
    · subst e; simp [List.lookup] at h; subst h; exact List.mem_cons_self
    · have : (n == m) = false := by simpa using e
      simp only [List.lookup, this] at h
      exact List.mem_cons_of_mem _ (lookup_mem h)

/-- The context `checkFun` checks a body under. -/
def funCtx (sigs : List (String × Sig)) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (tps : List (String × Option Bound)) (params : List (Mode × String × Hint)) (sig : Sig) : Ctx :=
  { vars := (params.zip sig.params).reverse.map (fun ((m, x, _), (_, t)) => (x, t, kindOf m)) ++ consts,
    ret := sig.ret, sigs, aliases := tparamAliases tps aliases, bounds := boundsOf tps }

/-- **Checking a generic once is enough.** A generic function whose check
reports nothing has instances whose checks report nothing: its body,
checked with each type parameter standing for a type argument the
parameter's bound admits, and with no bounds, reports nothing either, F063
included.

The type arguments are `Ground`: no type parameter and no `rec` in them.
Inference is taken as given (`InstEnv`), as it is for the other theorems:
under an instance it answers what it answered under the generic,
substituted, wherever the generic checked quietly. The signatures name only
their own type parameters (`SigsOk`), as `resolveNames` makes them, and the
body declares no type alias, which the parser refuses (P074). -/
theorem checkFun_instance {θ : List (String × Ty)} (g : Ground θ) {env' : InferEnv}
    (H : InstEnv θ inferEnv env') (sigs : List (String × Sig)) (hsigs : SigsOk sigs)
    (aliases : Aliases) (consts : List (String × Ty × Kind)) (name : String)
    (tps : List (String × Option Bound))
    (params : List (Mode × String × Hint)) (ret : Option Hint) (body : List Stmt) (sig : Sig)
    (hsig : sigs.lookup name = some sig)
    (hb : ∀ p b, (p, some b) ∈ tps → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true)
    (tf : blockTypeFree body = true)
    (quiet : SQuiet (checkFun sigs aliases consts (.mk name tps params ret body)) ()) :
    SQuiet (checkBlockF env' fuel ((funCtx sigs aliases consts tps params sig).inst θ) body >>= fun _ =>
      reportIf (match sig.ret.inst θ with
        | .ty _ => completes body
        | .void => false) "F063") () := by
  simp only [checkFun, hsig, checkBlock] at quiet
  obtain ⟨_, -, quiet⟩ := SQuiet.bind_inv
    (fun _ => Appends.bind (checkBlockF_appends _ _ _ _) (fun _ => Appends.report _ _)) quiet
  obtain ⟨_, hbody, h63⟩ := SQuiet.bind_inv (fun _ => Appends.report _ _) quiet
  have r : Respects θ (funCtx sigs aliases consts tps params sig) := by
    intro n b hl
    have hm := lookup_mem hl
    simp only [funCtx, boundsOf, List.mem_filterMap, Option.map_eq_some_iff] at hm
    obtain ⟨⟨p, b'⟩, hp, b'', hb', he⟩ := hm
    simp only [Prod.mk.injEq] at he
    obtain ⟨rfl, rfl⟩ := he
    subst hb'
    exact hb p b'' hp
  refine SQuiet.bind (checkBlockF_inst g H fuel _ body hbody r hsigs tf) ?_
  rw [SQuiet.reportIf_iff] at h63 ⊢
  cases hr : sig.ret <;> simp_all [Ret.inst]

end Datalove.Fun
