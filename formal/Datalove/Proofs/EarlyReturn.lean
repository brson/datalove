import Datalove.Fun

/-!
# Every early return fits the function

Datafun returns early from inside an expression: `e?` and the optional
operators return `none`, `e!` and the checked operators return an error. The
doc's rules allow each only where the function returns an option or a result
(`fits`, F049), so that what is returned is a value of the return type: a
`none` of an option, an error of a result.

`Fun.checkF_guarded` shows that checking enforces it: an expression that
checks, or synthesizes, is `Guarded` by what its context's return type
allows, every early return in it, at any depth, one the return type can
carry. `Fun.checkBlockF_guarded` lifts it to statements: a body that checks
with no diagnostic has every early return in every expression of it fit the
function. Both hold whatever inference says (`InferEnv`), since inference
only says what types are, and for any fuel.
-/

namespace Datalove.Fun

open Datalove.Lit (Code)

/-- The two ways of returning early from inside an expression. -/
inductive Early where
  | none_
  | er
  deriving DecidableEq

/-- What a return type allows: `none` into an option, an error into a
result. -/
def Ret.allows (r : Ret) : Early → Bool
  | .none_ => ({ ret := r, sigs := [] } : Ctx).returnsOption
  | .er => ({ ret := r, sigs := [] } : Ctx).returnsResult

theorem Ctx.returnsOption_eq (Γ : Ctx) : Γ.returnsOption = Γ.ret.allows .none_ := by
  simp [Ret.allows, Ctx.returnsOption]

theorem Ctx.returnsResult_eq (Γ : Ctx) : Γ.returnsResult = Γ.ret.allows .er := by
  simp [Ret.allows, Ctx.returnsResult]

def IdxMode.early : IdxMode → Early
  | .opt => .none_
  | .res => .er

theorem Ctx.fitsMode_eq (Γ : Ctx) (m : IdxMode) : Γ.fitsMode m = Γ.ret.allows m.early := by
  cases m <;> simp [Ctx.fitsMode, IdxMode.early, Ret.allows, Ctx.returnsOption, Ctx.returnsResult]

/-- Where a step of a place returns early: an index with `?` or `!`. -/
def Step.early : Step → Option Early
  | .index _ (some m) => some m.early
  | _ => none

/-- A step's key, if it is an index. -/
def Step.key : Step → Option Expr
  | .index k _ => some k
  | .field _ => none

/-- Where an expression returns early itself, not inside a part: a place at
each of its indexes. -/
def Expr.earlies : Expr → List Early
  | .tryOpt _ => [.none_]
  | .tryRes _ => [.er]
  | .unop .negO _ => [.none_]
  | .unop .negC _ => [.er]
  | .binop op _ _ => match needs op with
    | .result => [.er]
    | .option => [.none_]
    | .nothing => []
  | .index _ _ (some m) => [m.early]
  | .place _ steps => steps.filterMap Step.early
  | _ => []

/-- An expression's parts, one level down: a place's are its keys. -/
def Expr.parts : Expr → List Expr
  | .tuple es | .list es | .set es | .tensor _ es => es
  | .struct fs => fs.map (·.2)
  | .map es => es.flatMap fun (k, v) => [k, v]
  | .table _ rows => rows.flatten
  | .some_ e | .ok e | .er e | .data e | .error e | .term _ e | .enum e | .hinted _ e
  | .unop _ e | .tryOpt e | .tryRes e | .adapt e | .proj e _ => [e]
  | .binop _ a b | .index a b _ => [a, b]
  | .call _ args => args.map (·.2)
  | .place _ steps => steps.filterMap Step.key
  | _ => []

/-- Every early return in an expression, at any depth, is one `ok` allows. -/
inductive Guarded (ok : Early → Bool) : Expr → Prop where
  | mk : (∀ k ∈ e.earlies, ok k = true) → (∀ p ∈ e.parts, Guarded ok p) → Guarded ok e

/-! ## Running the checker -/

/-- A check that logs nothing and returns a value. -/
def Clean {α} (m : ChkM α) : Prop := ∃ a, m.run = ⟨#[], .ok a⟩

theorem run_bind {α β} (x : ChkM α) (f : α → ChkM β) :
    (x >>= f).run = (match x.run.val with
      | .ok a => ⟨x.run.log ++ (f a).run.log, (f a).run.val⟩
      | .error c => ⟨x.run.log, .error c⟩) := by
  rcases hx : x.run with ⟨l, v⟩
  have e : x = ⟨l, v⟩ := hx
  subst e
  cases v <;> rfl

theorem peel {α β} {x : ChkM α} {f : α → ChkM β} (h : Clean (x >>= f)) :
    Clean x ∧ ∃ a, Clean (f a) := by
  obtain ⟨b, h⟩ := h
  rw [run_bind] at h
  rcases hx : x.run with ⟨l, v⟩
  rw [hx] at h
  cases v with
  | error c => simp at h
  | ok a =>
    simp at h
    obtain ⟨hl, hv⟩ := h
    refine ⟨⟨a, by rw [hx, hl.1]⟩, a, b, ?_⟩
    rcases hf : (f a).run with ⟨l', v'⟩
    rw [hf] at hl hv
    simp at hl hv
    rw [hl.2, hv]

theorem throw_not_clean {α} {c : Code} : ¬ Clean (throw c : ChkM α) := by
  rintro ⟨a, h⟩
  simp [throw, throwThe, MonadExceptOf.throw, ExceptT.mk, ExceptT.run, pure] at h

theorem report_not_clean {c : Code} : ¬ Clean (report c) := by
  rintro ⟨a, h⟩
  simp [report, ExceptT.lift, ExceptT.mk, ExceptT.run, Functor.map] at h

@[simp] theorem clean_throw_bind {α β} {c : Code} {f : α → ChkM β} :
    Clean (throw c >>= f) ↔ False :=
  ⟨fun h => throw_not_clean (peel h).1, False.elim⟩

@[simp] theorem clean_throw {α} {c : Code} : Clean (throw c : ChkM α) ↔ False :=
  ⟨throw_not_clean, False.elim⟩

theorem clean_recover {m : ChkM Unit} (h : Clean (recover m)) : Clean m := by
  obtain ⟨_, h⟩ := h
  rcases hm : m.run with ⟨l, v⟩
  have e : m = ⟨l, v⟩ := hm
  subst e
  cases v with
  | ok a =>
    simp [recover, ExceptT.mk, ExceptT.run] at h
    exact ⟨a, by simp [ExceptT.run, h]⟩
  | error c => simp [recover, ExceptT.mk, ExceptT.run] at h

@[simp] theorem Ctx.enter_ret (Γ : Ctx) (e : Expr) : (Γ.enter e).ret = Γ.ret := rfl
@[simp] theorem Ctx.borrow_ret (Γ : Ctx) : Γ.borrow.ret = Γ.ret := rfl
@[simp] theorem Ctx.through_ret (Γ : Ctx) : Γ.through.ret = Γ.ret := rfl
@[simp] theorem Ctx.forArg_ret (Γ : Ctx) (m : Mode) : (Γ.forArg m).ret = Γ.ret := rfl

@[simp] theorem Ctx.enter_returnsOption (Γ : Ctx) (e : Expr) :
    (Γ.enter e).returnsOption = Γ.returnsOption := rfl
@[simp] theorem Ctx.enter_returnsResult (Γ : Ctx) (e : Expr) :
    (Γ.enter e).returnsResult = Γ.returnsResult := rfl

/-- An operator that succeeds fits the function's return type. -/
theorem finishBinop_fits {Γ : Ctx} {op : BinOp} {t : Ty} (h : Clean (finishBinop Γ op t)) :
    ∀ k ∈ (Expr.binop op .tt .tt).earlies, Γ.ret.allows k = true := by
  intro k hk
  cases op <;> simp [Expr.earlies, needs] at hk <;> subst hk <;>
    simp only [finishBinop, fits, needs] at h <;>
    (first | rw [← Ctx.returnsResult_eq] | rw [← Ctx.returnsOption_eq]) <;>
    (cases hr : Γ.returnsResult <;> cases ho : Γ.returnsOption <;> simp_all <;>
      (split at h <;> simp at h))

/-- What a place's step returns early is one `ok` allows, and its key is
`Guarded`. -/
def StepOk (ok : Early → Bool) (st : Step) : Prop :=
  (∀ k, st.early = some k → ok k = true) ∧ (∀ p, st.key = some p → Guarded ok p)

theorem Guarded.leaf {ok : Early → Bool} {e : Expr} (h1 : e.earlies = []) (h2 : e.parts = []) :
    Guarded ok e :=
  .mk (by simp [h1]) (by simp [h2])

theorem Guarded.one {ok : Early → Bool} {e p : Expr} (h1 : e.earlies = []) (h2 : e.parts = [p])
    (hp : Guarded ok p) : Guarded ok e :=
  .mk (by simp [h1]) (by intro q hq; simp [h2] at hq; subst hq; exact hp)

theorem Guarded.all {ok : Early → Bool} {e : Expr} {ps : List Expr} (h1 : e.earlies = [])
    (h2 : e.parts = ps) (hp : ∀ p ∈ ps, Guarded ok p) : Guarded ok e :=
  .mk (by simp [h1]) (by rw [h2]; exact hp)

/-- A place whose steps are each `StepOk`. -/
theorem Guarded.place {ok : Early → Bool} {x : String} {steps : List Step}
    (h : ∀ st ∈ steps, StepOk ok st) : Guarded ok (.place x steps) := by
  refine .mk (fun k hk => ?_) (fun p hp => ?_)
  · simp only [Expr.earlies, List.mem_filterMap] at hk
    obtain ⟨st, hst, hk⟩ := hk
    exact (h st hst).1 k hk
  · simp only [Expr.parts, List.mem_filterMap] at hp
    obtain ⟨st, hst, hp⟩ := hp
    exact (h st hst).2 p hp

-- Close a goal whose hypothesis `h` is a check that cannot succeed.
set_option hygiene false in
macro "impossible" : tactic => `(tactic| (simp at h; done))

theorem clean_ite_throw {α} {c : Prop} [Decidable c] {s : Code} {f : PUnit → ChkM α}
    {g : ChkM α} (h : Clean (if c then throw s >>= f else g)) : ¬ c ∧ Clean g := by
  split at h
  · simp at h
  · exact ⟨‹_›, h⟩

@[simp] theorem Ctx.enter_fitsMode (Γ : Ctx) (e : Expr) (m : IdxMode) :
    (Γ.enter e).fitsMode m = Γ.fitsMode m := rfl

mutual

theorem synthF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (e : Expr),
    Clean (synthF env n Γ e) → Guarded Γ.ret.allows e
  | 0, _, _, h => by simp [synthF] at h
  | n + 1, Γ, e, h => by
    cases e
    case hinted hh inner =>
      simp only [synthF] at h
      obtain ⟨-, _, h⟩ := peel h
      obtain ⟨hc, -⟩ := peel h
      exact .one rfl rfl (checkForm_guarded env n (Γ.enter _) inner _ hc)
    case var | tt | ff | int | hex | float | str | none_ | atom => exact .leaf rfl rfl
    case binop op a b =>
      simp only [synthF] at h
      obtain ⟨hp, _, hf⟩ := peel h
      obtain ⟨ga, gb⟩ := pairF_guarded env n (Γ.enter _) a b _ hp
      refine .mk (fun k hk => finishBinop_fits hf k (by simpa [Expr.earlies] using hk)) ?_
      intro q hq; simp [Expr.parts] at hq; rcases hq with rfl | rfl <;> assumption
    case unop op a =>
      cases op
      case neg | not =>
        simp only [synthF] at h
        obtain ⟨ha, -⟩ := peel h
        exact .one rfl rfl (synthF_guarded env n (Γ.enter _).borrow a ha)
      case negO =>
        simp only [synthF] at h
        obtain ⟨ha, _, h⟩ := peel h
        have ga := synthF_guarded env n (Γ.enter _).borrow a ha
        refine .mk ?_ (by intro q hq; simp [Expr.parts] at hq; subst hq; exact ga)
        intro k hk
        simp [Expr.earlies] at hk; subst hk
        rw [← Ctx.returnsOption_eq]
        cases ho : Γ.returnsOption <;> simp_all <;> (split at h <;> simp at h)
      case negC =>
        simp only [synthF] at h
        obtain ⟨ha, _, h⟩ := peel h
        have ga := synthF_guarded env n (Γ.enter _).borrow a ha
        refine .mk ?_ (by intro q hq; simp [Expr.parts] at hq; subst hq; exact ga)
        intro k hk
        simp [Expr.earlies] at hk; subst hk
        rw [← Ctx.returnsResult_eq]
        cases hr : Γ.returnsResult <;> simp_all <;> (split at h <;> simp at h)
    case tryOpt a =>
      simp only [synthF] at h
      obtain ⟨ha, _, h⟩ := peel h
      refine .mk ?_ (by intro q hq; simp [Expr.parts] at hq; rw [hq]
                        exact synthF_guarded env n (Γ.enter _) a ha)
      intro k hk
      simp [Expr.earlies] at hk; subst hk
      rw [← Ctx.returnsOption_eq]
      split at h
      · split at h
        · simpa using ‹_›
        · simp at h
      · simp at h
    case tryRes a =>
      simp only [synthF] at h
      obtain ⟨ha, _, h⟩ := peel h
      refine .mk ?_ (by intro q hq; simp [Expr.parts] at hq; rw [hq]
                        exact synthF_guarded env n (Γ.enter _) a ha)
      intro k hk
      simp [Expr.earlies] at hk; subst hk
      rw [← Ctx.returnsResult_eq]
      split at h
      · split at h
        · simpa using ‹_›
        · simp at h
      · simp at h
    case adapt a =>
      simp only [synthF] at h
      exact .one rfl rfl (synthF_guarded env n (Γ.enter _).borrow a h)
    case call f args =>
      simp only [synthF] at h
      split at h
      · rename_i sig _
        split at h
        · impossible
        · rename_i hl
          obtain ⟨hargs, -⟩ := peel h
          simp at hl
          refine .all rfl rfl (fun p hp => ?_)
          simp [Expr.parts] at hp
          obtain ⟨m, hm⟩ := hp
          exact argsF_guarded env n (Γ.enter _) args sig.params hargs hl (m, p) hm
      · impossible
    case place x steps =>
      simp only [synthF] at h
      split at h
      · exact .place (stepsF_guarded env n (Γ.enter _) _ steps h)
      · impossible
    case proj b s =>
      simp only [synthF] at h
      obtain ⟨hb, -⟩ := peel h
      exact .one rfl rfl (synthF_guarded env n (Γ.enter _) b hb)
    case index b k mode =>
      cases mode with
      | none => simp only [synthF] at h; impossible
      | some m =>
        simp only [synthF] at h
        obtain ⟨hb, _, h⟩ := peel h
        obtain ⟨-, ⟨t, kt⟩, h⟩ := peel h
        obtain ⟨hk, _, h⟩ := peel h
        refine .mk (fun e he => ?_) (fun p hp => ?_)
        · simp only [Expr.earlies, List.mem_singleton] at he
          subst he
          rw [← Ctx.fitsMode_eq]
          obtain ⟨-, h⟩ := clean_ite_throw h
          obtain ⟨-, h⟩ := clean_ite_throw h
          obtain ⟨hf, -⟩ := clean_ite_throw h
          simpa using hf
        · simp only [Expr.parts, List.mem_cons, List.not_mem_nil, or_false] at hp
          rcases hp with rfl | rfl
          · exact synthF_guarded env n (Γ.enter _) _ hb
          · exact checkF_guarded env n (Γ.enter _) _ _ (clean_recover hk)
    case tuple | struct | list | set | map | tensor | table | some_ | ok | er | data | error
      | term | enum =>
      simp only [synthF] at h
      split at h
      · obtain ⟨-, _, h⟩ := peel h
        obtain ⟨hc, -⟩ := peel h
        exact checkForm_guarded env n (Γ.enter _) _ _ hc
      · split at h
        · obtain ⟨-, _, h⟩ := peel h
          obtain ⟨hc, -⟩ := peel h
          exact checkForm_guarded env n (Γ.enter _) _ _ hc
        · impossible

theorem stepsF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (steps : List Step),
    Clean (stepsF env n Γ t steps) → ∀ st ∈ steps, StepOk Γ.ret.allows st
  | 0, _, _, _, h => by simp [stepsF] at h
  | _ + 1, _, _, [], _ => by simp
  | n + 1, Γ, t, [st], h => by
    simp only [stepsF] at h
    intro s hs
    simp at hs; subst hs
    exact stepF_guarded env n Γ t s h
  | n + 1, Γ, t, st :: st' :: rest, h => by
    simp only [stepsF] at h
    obtain ⟨hs, _, h⟩ := peel h
    intro s hs'
    simp only [List.mem_cons] at hs'
    rcases hs' with rfl | hs'
    · exact stepF_guarded env n Γ.through t s hs
    · exact stepsF_guarded env n Γ _ (st' :: rest) h s (by simpa using hs')

theorem stepF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (st : Step),
    Clean (stepF env n Γ t st) → StepOk Γ.ret.allows st
  | 0, _, _, _, h => by simp [stepF] at h
  | _ + 1, _, _, .field _, _ => ⟨by simp [Step.early], by simp [Step.key]⟩
  | _ + 1, _, _, .index _ none, h => by simp [stepF] at h
  | n + 1, Γ, t, .index k (some m), h => by
    simp only [stepF] at h
    obtain ⟨-, ⟨et, kt⟩, h⟩ := peel h
    obtain ⟨hk, _, h⟩ := peel h
    refine ⟨fun e he => ?_, fun p hp => ?_⟩
    · simp only [Step.early, Option.some.injEq] at he
      subst he
      rw [← Ctx.fitsMode_eq]
      obtain ⟨-, h⟩ := clean_ite_throw h
      obtain ⟨-, h⟩ := clean_ite_throw h
      obtain ⟨hf, -⟩ := clean_ite_throw h
      simpa using hf
    · simp only [Step.key, Option.some.injEq] at hp
      subst hp
      exact checkF_guarded env n Γ _ _ (clean_recover hk)

theorem pairF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (a b : Expr) (x : Option Ty)
    , Clean (pairF env n Γ a b x) →
    Guarded Γ.ret.allows a ∧ Guarded Γ.ret.allows b
  | 0, _, _, _, _, h => by simp [pairF] at h
  | n + 1, Γ, a, b, x, h => by
    simp only [pairF] at h
    obtain ⟨-, _, h⟩ := peel h
    obtain ⟨-, _, h⟩ := peel h
    split at h
    · simp at h
    · obtain ⟨ha, _, h⟩ := peel h
      obtain ⟨hb, -⟩ := peel h
      exact ⟨checkF_guarded env n Γ.borrow a _ ha, checkF_guarded env n Γ.borrow b _ hb⟩

theorem argsF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (args : List (Option Mode × Expr))
    (params : List (Mode × Ty)), Clean (argsF env n Γ args params) →
    args.length = params.length → ∀ me ∈ args, Guarded Γ.ret.allows me.2
  | 0, _, _, _, h, _ => by simp [argsF] at h
  | _ + 1, _, [], _, _, _ => by simp
  | _ + 1, _, _ :: _, [], _, hl => by simp at hl
  | n + 1, Γ, (marker, e) :: args, (mode, t) :: params, h, hl => by
    simp only [argsF] at h
    have h : Clean (checkF env n (Γ.forArg mode) e t >>= fun _ => argsF env n Γ args params) := by
      split at h <;> split at h
      all_goals first
        | exact h
        | exact absurd (peel h).1 report_not_clean
    obtain ⟨he, _, h⟩ := peel h
    intro me hme
    simp at hme
    rcases hme with rfl | hme
    · exact checkF_guarded env n (Γ.forArg mode) e _ he
    · exact argsF_guarded env n Γ args params h (by simpa using hl) me hme

theorem checkF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty)
    , Clean (checkF env n Γ e t) → Guarded Γ.ret.allows e
  | 0, _, _, _, h => by simp [checkF] at h
  | n + 1, Γ, e, t, h => by
    cases e
    case hinted hh inner =>
      simp only [checkF] at h
      obtain ⟨-, _, h⟩ := peel h
      split at h
      · simp at h
      · exact .one rfl rfl (checkForm_guarded env n (Γ.enter _) inner _ h)
    all_goals (simp only [checkF] at h; exact checkForm_guarded env n (Γ.enter _) _ _ h)

theorem checkForm_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty)
    , Clean (checkForm env n Γ e t) → Guarded Γ.ret.allows e
  | 0, _, _, _, h => by simp [checkForm] at h
  | n + 1, Γ, e, t, h => by
    cases e
    case var | tt | ff | int | hex | float | str | none_ | atom => exact .leaf rfl rfl
    case some_ p | ok p | er p | enum p =>
      simp only [checkForm] at h
      split at h
      · exact .one rfl rfl (checkF_guarded env n Γ p _ h)
      · impossible
    case data p | error p =>
      simp only [checkForm] at h
      split at h
      · obtain ⟨hp, -⟩ := peel h
        exact .one rfl rfl (synthF_guarded env n Γ p hp)
      · impossible
    case term a p =>
      simp only [checkForm] at h
      split at h
      · split at h
        · exact .one rfl rfl (checkF_guarded env n Γ p _ h)
        · impossible
      · split at h
        · exact .one rfl rfl (checkF_guarded env n Γ p _ h)
        · impossible
      · impossible
    case tuple es =>
      simp only [checkForm] at h
      split at h
      · split at h
        · impossible
        · rename_i hl
          simp at hl
          exact .all rfl rfl (pairsF_guarded env n Γ es _ h hl)
      · impossible
    case struct fs =>
      simp only [checkForm] at h
      split at h
      · split at h
        · impossible
        · rename_i hl
          simp at hl
          refine .all rfl rfl (fun p hp => ?_)
          simp [Expr.parts] at hp
          obtain ⟨m, hm⟩ := hp
          exact fieldsF_guarded env n Γ fs _ h hl (m, p) hm
      · impossible
    case list es | set es =>
      simp only [checkForm] at h
      split at h
      · exact .all rfl rfl (allF_guarded env n Γ es _ h)
      · impossible
    case map es =>
      simp only [checkForm] at h
      split at h
      · have g := entriesF_guarded env n Γ es _ _ h
        refine .all rfl rfl (fun p hp => ?_)
        simp [Expr.parts] at hp
        obtain ⟨k, v, hkv, hp⟩ := hp
        rcases hp with rfl | rfl
        · exact (g _ hkv).1
        · exact (g _ hkv).2
      · impossible
    case tensor shape es =>
      simp only [checkForm] at h
      split at h
      · split at h
        · impossible
        · split at h
          · impossible
          · exact .all rfl rfl (allF_guarded env n Γ es _ h)
      · impossible
    case table header rows =>
      simp only [checkForm] at h
      split at h
      · split at h
        · impossible
        · split at h
          · impossible
          · have g := rowsF_guarded env n Γ rows _ h
            refine .all rfl rfl (fun p hp => ?_)
            simp [Expr.parts] at hp
            obtain ⟨r, hr, hp⟩ := hp
            exact g r hr p hp
      · impossible
    case unop op a =>
      cases op
      case neg =>
        simp only [checkForm] at h
        split at h
        · exact .one rfl rfl (.leaf rfl rfl)
        · split at h
          · exact .one rfl rfl (checkF_guarded env n Γ a _ h)
          · exact subsumeF_guarded env n Γ _ _ h
      all_goals (simp only [checkForm] at h; exact subsumeF_guarded env n Γ _ _ h)
    case binop op a b =>
      simp only [checkForm] at h
      split at h
      · obtain ⟨hp, _, h⟩ := peel h
        obtain ⟨ga, gb⟩ := pairF_guarded env n Γ a b _ hp
        obtain ⟨hf, -⟩ := peel h
        refine .mk (fun k hk => finishBinop_fits hf k (by simpa [Expr.earlies] using hk)) ?_
        intro q hq; simp [Expr.parts] at hq; rcases hq with rfl | rfl <;> assumption
      · exact subsumeF_guarded env n Γ _ _ h
    case adapt a =>
      simp only [checkForm] at h
      obtain ⟨ha, -⟩ := peel h
      exact .one rfl rfl (synthF_guarded env n Γ.borrow a ha)
    case tryOpt | tryRes | call | hinted | place | proj | index =>
      simp only [checkForm] at h
      exact subsumeF_guarded env n Γ _ _ h

theorem subsumeF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty)
    , Clean (subsumeF env n Γ e t) → Guarded Γ.ret.allows e
  | 0, _, _, _, h => by simp [subsumeF] at h
  | n + 1, Γ, e, t, h => by
    simp only [subsumeF] at h
    obtain ⟨he, -⟩ := peel h
    exact synthF_guarded env n Γ e he

theorem pairsF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (ts : List Ty)
    , Clean (pairsF env n Γ es ts) → es.length = ts.length →
    ∀ e ∈ es, Guarded Γ.ret.allows e
  | 0, _, _, _, h, _ => by simp [pairsF] at h
  | _ + 1, _, [], _, _, _ => by simp
  | _ + 1, _, _ :: _, [], _, hl => by simp at hl
  | n + 1, Γ, e :: es, t :: ts, h, hl => by
    simp only [pairsF] at h
    obtain ⟨he, _, h⟩ := peel h
    intro x hx
    simp at hx
    rcases hx with rfl | hx
    · exact checkF_guarded env n Γ x _ he
    · exact pairsF_guarded env n Γ es ts h (by simpa using hl) x hx

theorem fieldsF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (fs : List (String × Expr))
    (ts : List (String × Ty)) , Clean (fieldsF env n Γ fs ts) →
    fs.length = ts.length → ∀ f ∈ fs, Guarded Γ.ret.allows f.2
  | 0, _, _, _, h, _ => by simp [fieldsF] at h
  | _ + 1, _, [], _, _, _ => by simp
  | _ + 1, _, _ :: _, [], _, hl => by simp at hl
  | n + 1, Γ, (m, e) :: fs, (k, t) :: ts, h, hl => by
    simp only [fieldsF] at h
    split at h
    · impossible
    · obtain ⟨he, _, h⟩ := peel h
      intro f hf
      simp at hf
      rcases hf with rfl | hf
      · exact checkF_guarded env n Γ e _ he
      · exact fieldsF_guarded env n Γ fs ts h (by simpa using hl) f hf

theorem allF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (t : Ty)
    , Clean (allF env n Γ es t) → ∀ e ∈ es, Guarded Γ.ret.allows e
  | 0, _, _, _, h => by simp [allF] at h
  | _ + 1, _, [], _, _ => by simp
  | n + 1, Γ, e :: es, t, h => by
    simp only [allF] at h
    obtain ⟨he, _, h⟩ := peel h
    intro x hx
    simp at hx
    rcases hx with rfl | hx
    · exact checkF_guarded env n Γ x _ he
    · exact allF_guarded env n Γ es t h x hx

theorem entriesF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (es : List (Expr × Expr))
    (k v : Ty) , Clean (entriesF env n Γ es k v) →
    ∀ kv ∈ es, Guarded Γ.ret.allows kv.1 ∧ Guarded Γ.ret.allows kv.2
  | 0, _, _, _, _, h => by simp [entriesF] at h
  | _ + 1, _, [], _, _, _ => by simp
  | n + 1, Γ, (ke, ve) :: es, k, v, h => by
    simp only [entriesF] at h
    obtain ⟨hk, _, h⟩ := peel h
    obtain ⟨hv, _, h⟩ := peel h
    intro kv hkv
    simp at hkv
    rcases hkv with rfl | hkv
    · exact ⟨checkF_guarded env n Γ _ _ hk, checkF_guarded env n Γ _ _ hv⟩
    · exact entriesF_guarded env n Γ es k v h kv hkv

theorem rowsF_guarded (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (rows : List (List Expr))
    (cols : List (String × Ty)) , Clean (rowsF env n Γ rows cols) →
    ∀ r ∈ rows, ∀ e ∈ r, Guarded Γ.ret.allows e
  | 0, _, _, _, h => by simp [rowsF] at h
  | _ + 1, _, [], _, _ => by simp
  | n + 1, Γ, r :: rs, cols, h => by
    simp only [rowsF] at h
    split at h
    · impossible
    · rename_i hl
      simp at hl
      obtain ⟨hp, _, h⟩ := peel h
      intro row hrow
      simp at hrow
      rcases hrow with rfl | hrow
      · exact pairsF_guarded env n Γ row _ hp (by simpa using hl)
      · exact rowsF_guarded env n Γ rs cols h row hrow

end

/-! ## Statements -/

/-- The expressions a statement holds itself, not in a body inside it. -/
def Stmt.exprs : Stmt → List Expr
  | .let_ _ _ e | .debuglog e | .call e => [e]
  | .set_ x steps _ e => [.place x steps, e]
  | .var_ _ _ (Option.some e) => [e]
  | .ret (Option.some e) => [e]
  | .if_ c _ _ _ _ => [c]
  | .loop (Option.some c) _ => [c]
  | .for_ _ coll _ => [coll]
  | _ => []

/-- The bodies inside a statement. -/
def Stmt.blocks : Stmt → List (List Stmt)
  | .if_ _ _ _ t (Option.some e) => [t, e]
  | .if_ _ _ _ t Option.none => [t]
  | .loop _ b | .for_ _ _ b => [b]
  | _ => []

/-- Every early return in a statement, in its expressions and in the bodies
inside it, is one `ok` allows. A function's body is not inside the statement
that declares it: it is checked under its own return type. -/
inductive GuardedS (ok : Early → Bool) : Stmt → Prop where
  | mk : (∀ e ∈ s.exprs, Guarded ok e) → (∀ b ∈ s.blocks, ∀ s' ∈ b, GuardedS ok s') →
      GuardedS ok s

/-- A step of statement checking that only adds diagnostics, whose result is
`P`, and is `Q` too when it adds none. -/
def Good {α} (m : StateM (Array Code) α) (P Q : α → Prop) : Prop :=
  ∀ d, d.size ≤ (m.run d).2.size ∧ P (m.run d).1 ∧ ((m.run d).2.size = d.size → Q (m.run d).1)

theorem good_pure {α} {a : α} {P Q : α → Prop} (hp : P a) (hq : Q a) :
    Good (pure a : StateM (Array Code) α) P Q :=
  fun _ => ⟨by simp [StateT.run, pure, StateT.pure], hp, fun _ => hq⟩

theorem good_bind {α β} {x : StateM (Array Code) α} {f : α → StateM (Array Code) β}
    {Px Qx : α → Prop} {P Q : β → Prop}
    (hx : Good x Px Qx) (hf : ∀ a, Px a → Good (f a) P (fun b => Qx a → Q b)) :
    Good (x >>= f) P Q := by
  intro d
  obtain ⟨g1, p1, q1⟩ := hx d
  obtain ⟨g2, p2, q2⟩ := hf _ p1 (x.run d).2
  have e : (x >>= f).run d = (f (x.run d).1).run (x.run d).2 := rfl
  rw [e]
  refine ⟨Nat.le_trans g1 g2, p2, fun hq => q2 (by omega) (q1 (by omega))⟩

theorem good_push {c : Code} :
    Good (modify (·.push c) : StateM (Array Code) PUnit) (fun _ => True) (fun _ => False) := by
  intro d
  have e : (modify (·.push c) : StateM (Array Code) PUnit).run d = (PUnit.unit, d.push c) := rfl
  rw [e]
  simp

theorem good_runChk {α} (m : ChkM α) :
    Good (runChk m) (fun _ => True) (fun r => ∃ a, r = Option.some a ∧ Clean m) := by
  intro d
  rcases hm : m.run with ⟨l, r⟩
  cases r with
  | ok a =>
    have e : (runChk m).run d = (Option.some a, d ++ l) := by
      simp only [runChk, hm]; rfl
    rw [e]
    refine ⟨by simp, trivial, fun hs => ⟨a, rfl, a, ?_⟩⟩
    simp at hs
    rw [hm, hs]
  | error c =>
    have e : (runChk m).run d = (Option.none, (d ++ l).push c) := by
      simp only [runChk, hm]; rfl
    rw [e]
    refine ⟨by simp, trivial, fun hs => ?_⟩
    simp at hs

theorem good_weaken {α} {m : StateM (Array Code) α} {P Q P' Q' : α → Prop} (h : Good m P Q)
    (hp : ∀ a, P a → P' a) (hq : ∀ a, P a → Q a → Q' a) : Good m P' Q' :=
  fun d => let ⟨g, p, q⟩ := h d; ⟨g, hp _ p, fun hs => hq _ p (q hs)⟩

theorem good_trivial_push {α} {c : Code} {f : PUnit → StateM (Array Code) α} {P Q : α → Prop}
    (hf : ∀ u, Good (f u) P (fun _ => True)) :
    Good (modify (·.push c) >>= f) P Q :=
  good_bind good_push (fun u _ => good_weaken (hf u) (fun _ h => h) (fun _ _ _ h => h.elim))

theorem Ctx.bindAll_ret (Γ : Ctx) (Δ : List (String × Ty)) : (Γ.bindAll Δ).ret = Γ.ret := by
  unfold Ctx.bindAll
  induction Δ generalizing Γ with
  | nil => rfl
  | cons p Δ ih => simp only [List.foldl]; rw [ih]; rfl

theorem bindOpt_ret (Γ : Ctx) (x : Option String) (t : Ty) : (bindOpt Γ x t).ret = Γ.ret := by
  cases x <;> rfl

theorem bindChecked_good (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (hint : Option Hint)
    (e : Expr) (k : Kind) :
    Good (bindChecked env n Γ x hint e k) (fun Γ' => Γ'.ret = Γ.ret)
      (fun _ => Guarded Γ.ret.allows e) := by
  cases hint with
  | some h =>
    simp only [bindChecked, convertReporting]
    refine good_bind (good_runChk _) (fun r _ => ?_)
    cases r with
    | none => exact good_pure rfl (fun hq => by simp at hq)
    | some t =>
      refine good_bind (good_runChk _) (fun r2 _ => good_pure rfl (fun hq2 _ => ?_))
      obtain ⟨_, _, hs⟩ := hq2
      exact checkF_guarded env n Γ e t hs
  | none =>
    simp only [bindChecked]
    refine good_bind (good_runChk _) (fun r _ => ?_)
    have g : (∃ a, r = some a ∧ Clean (synthF env n Γ e)) → Guarded Γ.ret.allows e :=
      fun ⟨_, _, hs⟩ => synthF_guarded env n Γ e hs
    cases r with
    | none => exact good_pure rfl (fun hq => g hq)
    | some t => exact good_pure rfl (fun hq => g hq)

theorem GuardedS.of {ok : Early → Bool} {s : Stmt} (he : ∀ e ∈ s.exprs, Guarded ok e)
    (hb : ∀ b ∈ s.blocks, ∀ s' ∈ b, GuardedS ok s') : GuardedS ok s := .mk he hb

theorem reportIf_good {c : Bool} {code : Code} :
    Good (reportIf c code) (fun _ => True) (fun _ => True) := by
  unfold reportIf
  split
  · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
  · exact good_pure trivial trivial

/-- `reportIf` that reports nothing found its condition false. -/
theorem reportIf_quiet {c : Bool} {code : Code} :
    Good (reportIf c code) (fun _ => True) (fun _ => c = false) := by
  unfold reportIf
  split
  · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
  · rename_i hc
    exact good_pure trivial (by simpa using hc)

theorem push_good {c : Code} {P : PUnit → Prop} :
    Good (modify (·.push c) : StateM (Array Code) PUnit) (fun _ => True) P :=
  good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)

theorem runChk_checkF_good (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty) :
    Good (runChk (checkF env n Γ e t)) (fun _ => True) (fun _ => Guarded Γ.ret.allows e) :=
  good_weaken (good_runChk _) (fun _ h => h)
    (fun _ _ ⟨_, _, hs⟩ => checkF_guarded env n Γ e t hs)

theorem setValue_good (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (op : Option BinOp)
    (e : Expr) :
    Good (setValue env n Γ t op e) (fun _ => True) (fun _ => Guarded Γ.ret.allows e) := by
  cases op with
  | none =>
    simp only [setValue]
    exact good_bind (runChk_checkF_good env n Γ e t) (fun _ _ => good_pure trivial id)
  | some op =>
    simp only [setValue]
    refine good_bind (runChk_checkF_good env n Γ.borrow e t) (fun _ _ => ?_)
    split
    · exact push_good
    · exact good_weaken reportIf_good (fun _ h => h) (fun _ _ _ h => h)

theorem setIndex_good (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (k : Expr)
    (mode : Option IdxMode) (op : Option BinOp) (e : Expr) :
    Good (setIndex env n Γ t k mode op e) (fun _ => True)
      (fun _ => Guarded Γ.ret.allows k ∧ Guarded Γ.ret.allows e) := by
  unfold setIndex
  split
  · exact push_good
  · split
    · refine good_bind (runChk_checkF_good env n Γ k _) (fun _ _ => ?_)
      refine good_bind (runChk_checkF_good env n Γ e _) (fun _ _ => ?_)
      exact good_pure trivial (fun he hk => ⟨hk, he⟩)
    · exact push_good
  · split
    · exact push_good
    · split
      · exact push_good
      · refine good_bind (runChk_checkF_good env n Γ k _) (fun _ _ => ?_)
        exact good_weaken (setValue_good env n Γ _ op e) (fun _ h => h)
          (fun _ _ he hk => ⟨hk, he⟩)

/-- A `set` target's steps check each key they reach, and the value. -/
theorem setSteps_good (env : InferEnv) (n : Nat) (Γ : Ctx) (op : Option BinOp) (e : Expr) :
    ∀ (t : Ty) (steps : List Step), Good (setSteps env n Γ t op e steps) (fun _ => True)
      (fun _ => (∀ st ∈ steps, ∀ p, st.key = some p → Guarded Γ.ret.allows p) ∧
        Guarded Γ.ret.allows e)
  | t, [] => by
    simp only [setSteps]
    exact good_weaken (setValue_good env n Γ t op e) (fun _ h => h) (fun _ _ h => ⟨by simp, h⟩)
  | t, [.index k mode] => by
    simp only [setSteps]
    refine good_weaken (setIndex_good env n Γ t k mode op e) (fun _ h => h) (fun _ _ h => ?_)
    refine ⟨fun st hst p hp => ?_, h.2⟩
    simp at hst; subst hst
    simp [Step.key] at hp; subst hp
    exact h.1
  | t, .field s :: rest => by
    simp only [setSteps]
    split
    · refine good_weaken (setSteps_good env n Γ op e _ rest) (fun _ h => h) (fun _ _ h => ?_)
      refine ⟨fun st hst p hp => ?_, h.2⟩
      simp at hst
      rcases hst with rfl | hst
      · simp [Step.key] at hp
      · exact h.1 st hst p hp
    · exact push_good
  | t, .index _ none :: _ :: _ => by
    simp only [setSteps]
    exact push_good
  | t, .index k (some _) :: st' :: rest => by
    simp only [setSteps]
    split
    · exact push_good
    · refine good_bind (runChk_checkF_good env n Γ k _) (fun _ _ => ?_)
      refine good_weaken (setSteps_good env n Γ op e _ (st' :: rest)) (fun _ h => h)
        (fun _ _ h hk => ⟨fun st hst p hp => ?_, h.2⟩)
      simp only [List.mem_cons] at hst
      rcases hst with rfl | hst
      · simp [Step.key] at hp; subst hp; exact hk
      · exact h.1 st (by simpa using hst) p hp

theorem reportModes_good (Γ : Ctx) : ∀ (ms : List IdxMode),
    Good (reportModes Γ ms) (fun _ => True) (fun _ => ∀ m ∈ ms, Γ.fitsMode m = true)
  | [] => good_pure trivial (by simp)
  | m :: ms => by
    simp only [reportModes]
    refine good_bind reportIf_quiet (fun _ _ => ?_)
    exact good_weaken (reportModes_good Γ ms) (fun _ h => h) (fun _ _ h hm m' hm' => by
      simp at hm'
      rcases hm' with rfl | hm'
      · simpa using hm
      · exact h m' hm')

theorem addMode_keeps {ms : List IdxMode} {m : IdxMode} (st : Step) (h : m ∈ ms) :
    m ∈ setModes.addMode ms st := by
  unfold setModes.addMode
  split
  · split <;> simp [h]
  · exact h

theorem addMode_adds (ms : List IdxMode) (st : Step) {k : Early} (h : st.early = some k) :
    ∃ m ∈ setModes.addMode ms st, m.early = k := by
  cases st with
  | field _ => simp [Step.early] at h
  | index _ mode =>
    cases mode with
    | none => simp [Step.early] at h
    | some m =>
      simp [Step.early] at h
      refine ⟨m, ?_, h⟩
      unfold setModes.addMode
      split <;> simp_all
      split <;> simp_all

theorem setModes_go : ∀ (steps : List Step) (ms : List IdxMode),
    (∀ m ∈ ms, m ∈ steps.foldl setModes.addMode ms) ∧
    (∀ st ∈ steps, ∀ k, st.early = some k → ∃ m ∈ steps.foldl setModes.addMode ms, m.early = k)
  | [], _ => ⟨fun _ h => h, by simp⟩
  | st :: rest, ms => by
    obtain ⟨keep, adds⟩ := setModes_go rest (setModes.addMode ms st)
    refine ⟨fun m hm => keep m (addMode_keeps st hm), fun st' hst k hk => ?_⟩
    simp at hst
    rcases hst with rfl | hst
    · obtain ⟨m, hm, rfl⟩ := addMode_adds ms st' hk
      exact ⟨m, keep m hm, rfl⟩
    · exact adds st' hst k hk

theorem checkSet_good (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) :
    Good (checkSet env n Γ x steps op e) (fun _ => True)
      (fun _ => ∀ e' ∈ (Stmt.set_ x steps op e).exprs, Guarded Γ.ret.allows e') := by
  unfold checkSet
  split
  · exact push_good
  · refine good_bind reportIf_good (fun _ _ => ?_)
    refine good_bind (reportModes_good Γ (setModes steps)) (fun _ _ => ?_)
    refine good_weaken (setSteps_good env n Γ op e _ steps) (fun _ h => h)
      (fun _ _ hs hm _ => ?_)
    intro e' he'
    simp only [Stmt.exprs, List.mem_cons, List.not_mem_nil, or_false] at he'
    rcases he' with rfl | rfl
    · refine .place (fun st hst => ⟨fun k hk => ?_, fun p hp => hs.1 st hst p hp⟩)
      obtain ⟨m, hm', rfl⟩ := (setModes_go steps []).2 st hst k hk
      rw [← Ctx.fitsMode_eq]
      exact hm m hm'
    · exact hs.2

theorem checkRet_good (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Option Expr) :
    Good (checkRet env n Γ e) (fun _ => True)
      (fun _ => ∀ e' ∈ (Stmt.ret e).exprs, Guarded Γ.ret.allows e') := by
  unfold checkRet
  split
  · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
  · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
  · rename_i r e' _ _
    refine good_bind (good_runChk _) (fun r' _ => good_pure trivial (fun hq => ?_))
    obtain ⟨_, _, hs⟩ := hq
    intro e'' he''
    simp [Stmt.exprs] at he''
    subst he''
    exact checkF_guarded env n Γ _ _ hs
  · exact good_pure trivial (by simp [Stmt.exprs])

theorem checkElse_good (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Option (List Stmt))
    (hb : ∀ ss, Good (checkBlockF env n Γ ss) (fun _ => True)
      (fun _ => ∀ s ∈ ss, GuardedS Γ.ret.allows s)) :
    Good (checkElse env n Γ e) (fun _ => True)
      (fun _ => ∀ b ∈ e.toList, ∀ s ∈ b, GuardedS Γ.ret.allows s) := by
  cases e with
  | none => exact good_pure trivial (by simp)
  | some e =>
    simp only [checkElse]
    exact good_weaken (hb e) (fun _ h => h) (fun _ _ h => by simpa using h)

/-- The binding `if`'s body, as `checkStmtF` writes it. -/
theorem bindingIf_good (env : InferEnv) (n : Nat) (Γ : Ctx) (c : Expr)
    (thenB elseB : Option String) (t : List Stmt) (e : Option (List Stmt))
    (blocks : ∀ Γ', Γ'.ret = Γ.ret → ∀ ss, Good (checkBlockF env n Γ' ss) (fun _ => True)
      (fun _ => ∀ s ∈ ss, GuardedS Γ.ret.allows s)) :
    Good (runChk (synthF env n Γ c) >>= fun r =>
      (match r with
      | Option.none => pure ()
      | Option.some ct =>
        match ct.expose with
        | .option p =>
          checkBlockF env n (bindOpt Γ thenB p) t >>= fun _ =>
          reportIf elseB.isSome "F094" >>= fun _ =>
          checkElse env n Γ e
        | .result p =>
          if elseB.isNone then modify (·.push "F046")
          else
            checkBlockF env n (bindOpt Γ thenB p) t >>= fun _ =>
            checkElse env n (bindOpt Γ elseB .error) e
        | _ => modify (·.push "F093")) >>= fun _ =>
      pure Γ)
      (fun Γ' => Γ'.ret = Γ.ret) (fun _ => GuardedS Γ.ret.allows (.if_ c thenB elseB t e)) := by
  refine good_bind (good_runChk _) (fun r _ => ?_)
  have bl : ∀ (ht : ∀ s ∈ t, GuardedS Γ.ret.allows s)
      (he : ∀ b ∈ e.toList, ∀ s ∈ b, GuardedS Γ.ret.allows s),
      ∀ b ∈ (Stmt.if_ c thenB elseB t e).blocks, ∀ s ∈ b, GuardedS Γ.ret.allows s := by
    intro ht he b hb s hs
    cases e with
    | none =>
      simp only [Stmt.blocks, List.mem_singleton] at hb
      subst hb; exact ht s hs
    | some e' =>
      simp only [Stmt.blocks, List.mem_cons, List.not_mem_nil, or_false] at hb
      rcases hb with rfl | rfl
      · exact ht s hs
      · exact he _ (by simp) s hs
  refine good_bind (Px := fun _ => True) (Qx := fun _ => r.isSome →
      ∀ b ∈ (Stmt.if_ c thenB elseB t e).blocks, ∀ s ∈ b, GuardedS Γ.ret.allows s) ?_
    (fun _ _ => good_pure rfl (fun hb hc => ?_))
  · cases r with
    | none => exact good_pure trivial (fun h => by simp at h)
    | some ct =>
      refine good_weaken (Q := fun _ =>
        ∀ b ∈ (Stmt.if_ c thenB elseB t e).blocks, ∀ s ∈ b, GuardedS Γ.ret.allows s) ?_
        (fun _ h => h) (fun _ _ h _ => h)
      refine good_weaken (Q := fun _ => (∀ s ∈ t, GuardedS Γ.ret.allows s) ∧
        (∀ b ∈ e.toList, ∀ s ∈ b, GuardedS Γ.ret.allows s)) ?_ (fun _ h => h)
        (fun _ _ h => bl h.1 h.2)
      dsimp only
      split
      · refine good_bind (blocks _ (bindOpt_ret _ _ _) t) (fun _ _ => ?_)
        refine good_bind reportIf_good (fun _ _ => ?_)
        exact good_weaken (checkElse_good env n Γ e (blocks _ rfl)) (fun _ h => h)
          (fun _ _ he _ ht => ⟨ht, he⟩)
      · split
        · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
        · refine good_bind (blocks _ (bindOpt_ret _ _ _) t) (fun _ _ => ?_)
          have := checkElse_good env n (bindOpt Γ elseB .error) e
            (fun ss => by
              have h := blocks (bindOpt Γ elseB .error) (bindOpt_ret _ _ _) ss
              rw [bindOpt_ret]; exact h)
          rw [bindOpt_ret] at this
          exact good_weaken this (fun _ h => h) (fun _ _ he ht => ⟨ht, he⟩)
      · exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
  · obtain ⟨_, hr, hs⟩ := hc
    exact .of (by simp [Stmt.exprs]; exact synthF_guarded env n Γ c hs) (hb (by simp [hr]))

mutual

theorem checkStmtF_good (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (s : Stmt),
    Good (checkStmtF env n Γ s) (fun Γ' => Γ'.ret = Γ.ret) (fun _ => GuardedS Γ.ret.allows s)
  | 0, Γ, s => by
    simp only [checkStmtF]
    exact good_trivial_push (fun _ => good_pure rfl trivial)
  | n + 1, Γ, s => by
    have blocks : ∀ Γ', Γ'.ret = Γ.ret → ∀ ss, Good (checkBlockF env n Γ' ss) (fun _ => True)
        (fun _ => ∀ s ∈ ss, GuardedS Γ.ret.allows s) := fun Γ' h ss => by
      have := checkBlockF_good env n Γ' ss
      rw [h] at this
      exact this
    cases s
    case let_ x hint e =>
      simp only [checkStmtF]
      exact good_weaken (bindChecked_good env n Γ x hint e .let_) (fun _ h => h)
        (fun _ _ hg => .of (by simp [Stmt.exprs, hg]) (by simp [Stmt.blocks]))
    case var_ x hint v =>
      cases v with
      | some e =>
        simp only [checkStmtF]
        exact good_weaken (bindChecked_good env n Γ x hint e .var_) (fun _ h => h)
          (fun _ _ hg => .of (by simp [Stmt.exprs, hg]) (by simp [Stmt.blocks]))
      | none =>
        cases hint with
        | some h =>
          simp only [checkStmtF, convertReporting]
          refine good_bind (good_runChk _) (fun r _ => ?_)
          cases r <;> exact good_pure rfl (fun _ => .of (by simp [Stmt.exprs]) (by simp [Stmt.blocks]))
        | none =>
          simp only [checkStmtF]
          exact good_pure rfl (.of (by simp [Stmt.exprs]) (by simp [Stmt.blocks]))
    case fun_ f =>
      simp only [checkStmtF]
      exact good_pure rfl (.of (by simp [Stmt.exprs]) (by simp [Stmt.blocks]))
    case set_ x steps op e =>
      simp only [checkStmtF]
      exact good_bind (checkSet_good env n Γ x steps op e)
        (fun _ _ => good_pure rfl (fun hq => .of hq (by simp [Stmt.blocks])))
    case ret e =>
      simp only [checkStmtF]
      exact good_bind (checkRet_good env n Γ e)
        (fun _ _ => good_pure rfl (fun hq => .of hq (by simp [Stmt.blocks])))
    case brk | cont =>
      simp only [checkStmtF]
      exact good_bind reportIf_good
        (fun _ _ => good_pure rfl (fun _ => .of (by simp [Stmt.exprs]) (by simp [Stmt.blocks])))
    case debuglog e =>
      simp only [checkStmtF]
      refine good_bind (good_runChk _) (fun r _ => good_pure rfl (fun hq => ?_))
      obtain ⟨_, _, hs⟩ := hq
      exact .of (by simp [Stmt.exprs]; exact synthF_guarded env n Γ.borrow e hs) (by simp [Stmt.blocks])
    case call e =>
      simp only [checkStmtF]
      refine good_bind (good_runChk _) (fun r _ => ?_)
      refine good_bind (Px := fun _ => True) (Qx := fun _ => True) ?_
        (fun _ _ => good_pure rfl (fun _ hq => ?_))
      · cases r with
        | none => exact good_pure trivial trivial
        | some t => exact reportIf_good
      · obtain ⟨_, _, hs⟩ := hq
        exact .of (by simp [Stmt.exprs]; exact synthF_guarded env n Γ e hs) (by simp [Stmt.blocks])
    case loop c body =>
      simp only [checkStmtF]
      refine good_bind (Px := fun _ => True) (Qx := fun _ => ∀ e ∈ (Stmt.loop c body).exprs,
          Guarded Γ.ret.allows e) ?_ (fun _ _ => ?_)
      · cases c with
        | none => exact good_pure trivial (by simp [Stmt.exprs])
        | some c =>
          refine good_bind (good_runChk _) (fun r _ => good_pure trivial (fun hq => ?_))
          obtain ⟨_, _, hs⟩ := hq
          simp [Stmt.exprs]
          exact checkF_guarded env n Γ c _ hs
      · refine good_bind (blocks { Γ with inLoop := true } rfl body) (fun _ _ => good_pure rfl (fun hb he => ?_))
        exact .of he (by simp [Stmt.blocks]; exact hb)
    case for_ names coll body =>
      simp only [checkStmtF]
      refine good_bind (Px := fun _ => True)
        (Qx := fun r => r.isSome = true ∧ Guarded Γ.ret.allows coll) ?_ (fun r _ => ?_)
      · unfold forBindings
        split
        · refine good_weaken (good_runChk _) (fun _ h => h) (fun r _ hq => ?_)
          obtain ⟨_, rfl, hs⟩ := hq
          exact ⟨rfl, synthF_guarded env n Γ.borrow _ (peel hs).1⟩
        · exact good_bind good_push (fun _ _ => good_pure trivial (fun h => h.elim))
      · refine good_bind (Px := fun _ => True) (Qx := fun _ => r.isSome →
            ∀ s ∈ body, GuardedS Γ.ret.allows s) ?_ (fun _ _ => good_pure rfl (fun hb hc => ?_))
        · cases r with
          | none => exact good_pure trivial (fun h => by simp at h)
          | some Δ =>
            have := checkBlockF_good env n ({ Γ with inLoop := true }.bindAll Δ) body
            rw [Ctx.bindAll_ret] at this
            exact good_weaken this (fun _ h => h) (fun _ _ h _ => h)
        · exact .of (by simp [Stmt.exprs]; exact hc.2) (by simp [Stmt.blocks]; exact hb hc.1)
    case if_ c a b t e =>
      cases a <;> cases b
      case none.none =>
        simp only [checkStmtF]
        refine good_bind (good_runChk _) (fun r _ => ?_)
        refine good_bind (blocks _ rfl t) (fun _ _ => ?_)
        refine good_bind (checkElse_good env n Γ e (blocks _ rfl)) (fun _ _ => ?_)
        refine good_pure rfl (fun he ht hc => ?_)
        obtain ⟨_, _, hs⟩ := hc
        refine .of (by simp [Stmt.exprs]; exact checkF_guarded env n Γ c _ hs) ?_
        intro bl hbl s' hs'
        cases e <;> simp [Stmt.blocks] at hbl <;> rcases hbl with rfl | rfl <;> simp_all
      all_goals
        simp only [checkStmtF]
        exact bindingIf_good env n Γ c _ _ t e blocks

theorem checkBlockF_good (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (ss : List Stmt),
    Good (checkBlockF env n Γ ss) (fun _ => True) (fun _ => ∀ s ∈ ss, GuardedS Γ.ret.allows s)
  | 0, Γ, ss => by
    simp only [checkBlockF]
    exact good_weaken good_push (fun _ _ => trivial) (fun _ _ h => h.elim)
  | _ + 1, Γ, [] => good_pure trivial (by simp)
  | n + 1, Γ, s :: ss => by
    simp only [checkBlockF]
    refine good_bind (checkStmtF_good env n Γ s) (fun Γ' hΓ' => ?_)
    refine good_weaken (checkBlockF_good env n Γ' ss) (fun _ _ => trivial) (fun _ _ hb hs => ?_)
    intro s' hs'
    simp at hs'
    rcases hs' with rfl | hs'
    · exact hs
    · rw [← hΓ']; exact hb s' hs'

end

/-- A function whose check reports nothing has every early return in its
body, from `?`, `!` or a checked or optional operator at any depth, one its
return type can carry: `none` only where it returns an option, an error only
where it returns a result. -/
theorem checkFun_early_returns_fit (sigs : List (String × Sig)) (name : String)
    (params : List (Mode × String × Hint)) (ret : Option Hint) (body : List Stmt)
    (sig : Sig) (hsig : sigs.lookup name = some sig) (d : Array Code)
    (quiet : ((checkFun sigs (.mk name params ret body)).run d).2.size = d.size) :
    ∀ s ∈ body, GuardedS sig.ret.allows s := by
  simp only [checkFun, hsig, checkBlock] at quiet
  have g := good_bind (checkBlockF_good inferEnv fuel
      { vars := (params.zip sig.params).reverse.map fun ((m, x, _), (_, t)) => (x, t, kindOf m),
        ret := sig.ret, sigs } body)
    (fun _ _ => good_weaken (reportIf_good (c := match sig.ret with
        | .ty _ => completes body
        | .void => false) (code := "F063"))
      (P' := fun _ => True) (Q' := fun _ => (∀ s ∈ body, GuardedS sig.ret.allows s) →
        ∀ s ∈ body, GuardedS sig.ret.allows s)
      (fun _ h => h) (fun _ _ _ hx => hx))
  exact (g d).2.2 quiet

end Datalove.Fun
