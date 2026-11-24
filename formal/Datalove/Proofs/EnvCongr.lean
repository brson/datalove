import Datalove.Fun

/-!
# The checker tells inference environments apart only by their inference

`InferEnv.moves` is the run's: whether a read moves a variable's value out
(`Ctx.takes`). Nothing the checker decides reads it, and this says so: two
environments with the same `ty`, `pair` and `call` check every expression
and every statement alike. `movesEnv` and `inferEnv` are such a pair
(`FunRun.lean`), so what `checkScript` finds quiet with `inferEnv` is quiet
under the contexts the moving run and the move analysis compute with
`movesEnv`, which `Proofs/MoveInst.lean` needs.
-/

namespace Datalove.Fun

/-- Two environments the checker cannot tell apart. -/
structure InferEnv.Same (env env' : InferEnv) : Prop where
  ty : env.ty = env'.ty
  pair : env.pair = env'.pair
  call : env.call = env'.call

theorem InferEnv.Same.refl (env : InferEnv) : env.Same env := ⟨rfl, rfl, rfl⟩

theorem InferEnv.Same.symm {env env' : InferEnv} (h : env.Same env') : env'.Same env :=
  ⟨h.ty.symm, h.pair.symm, h.call.symm⟩

/-- An environment and the same with `moves` changed. -/
theorem InferEnv.Same.moves (env : InferEnv) (b : Bool) : env.Same { env with moves := b } := ⟨rfl, rfl, rfl⟩

section
variable {env env' : InferEnv} (h : InferEnv.Same env env')
include h
set_option linter.unusedSectionVars false

mutual

theorem synthF_same : ∀ (n : Nat) (Γ : Ctx) (e : Expr), synthF env n Γ e = synthF env' n Γ e
  | 0, _, _ => by simp only [synthF]
  | n + 1, Γ, e => by
    cases e <;> simp only [synthF, synthF_same n, stepsF_same n, pairF_same n, callF_same n,
      checkForm_same n, partsF_same n, checkF_same n, h.ty]

theorem stepsF_same : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (ss : List Step), stepsF env n Γ t ss = stepsF env' n Γ t ss
  | 0, _, _, _ => by simp only [stepsF]
  | n + 1, Γ, t, [] => by simp only [stepsF]
  | n + 1, Γ, t, [st] => by simp only [stepsF, stepF_same n]
  | n + 1, Γ, t, st :: st2 :: rest => by simp only [stepsF, stepF_same n, stepsF_same n]

theorem stepF_same : ∀ (n : Nat) (Γ : Ctx) (t : Ty) (st : Step), stepF env n Γ t st = stepF env' n Γ t st
  | 0, _, _, _ => by simp only [stepF]
  | n + 1, Γ, t, .field s => by simp only [stepF]
  | n + 1, Γ, t, .index k none => by simp only [stepF]
  | n + 1, Γ, t, .index k (some m) => by simp only [stepF, checkF_same n]

theorem pairF_same : ∀ (n : Nat) (Γ : Ctx) (a b : Expr) (exp : Option Ty),
    pairF env n Γ a b exp = pairF env' n Γ a b exp
  | 0, _, _, _, _ => by simp only [pairF]
  | n + 1, Γ, a, b, exp => by simp only [pairF, operandF_same n, checkF_same n, h.pair]

theorem operandF_same : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (r : Except Lit.Code Ty),
    operandF env n Γ e r = operandF env' n Γ e r
  | 0, _, _, _ => by simp only [operandF]
  | n + 1, Γ, e, .ok t => by simp only [operandF]
  | n + 1, Γ, e, .error c => by simp only [operandF, synthF_same n]

theorem partsF_same : ∀ (n : Nat) (Γ : Ctx) (e : Expr), partsF env n Γ e = partsF env' n Γ e
  | 0, _, _ => by simp only [partsF]
  | n + 1, Γ, e => by
    cases e <;> simp only [partsF, synthF_same n, synthAllF_same n, likeF_same n, entriesLikeF_same n,
      rowsLikeF_same n]

theorem synthAllF_same : ∀ (n : Nat) (Γ : Ctx) (es : List Expr), synthAllF env n Γ es = synthAllF env' n Γ es
  | 0, _, _ => by simp only [synthAllF]
  | n + 1, Γ, [] => by simp only [synthAllF]
  | n + 1, Γ, e :: es => by simp only [synthAllF, synthF_same n, synthAllF_same n]

theorem likeF_same : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (t : Ty), likeF env n Γ es t = likeF env' n Γ es t
  | 0, _, _, _ => by simp only [likeF]
  | n + 1, Γ, [], t => by simp only [likeF]
  | n + 1, Γ, e :: es, t => by simp only [likeF, synthF_same n, likeF_same n]

theorem entriesLikeF_same : ∀ (n : Nat) (Γ : Ctx) (es : List (Expr × Expr)) (kt vt : Ty),
    entriesLikeF env n Γ es kt vt = entriesLikeF env' n Γ es kt vt
  | 0, _, _, _, _ => by simp only [entriesLikeF]
  | n + 1, Γ, [], kt, vt => by simp only [entriesLikeF]
  | n + 1, Γ, (k, v) :: es, kt, vt => by simp only [entriesLikeF, synthF_same n, entriesLikeF_same n]

theorem rowsLikeF_same : ∀ (n : Nat) (Γ : Ctx) (w : Nat) (rs : List (List Expr)) (ts : List Ty),
    rowsLikeF env n Γ w rs ts = rowsLikeF env' n Γ w rs ts
  | 0, _, _, _, _ => by simp only [rowsLikeF]
  | n + 1, Γ, w, [], ts => by simp only [rowsLikeF]
  | n + 1, Γ, w, r :: rs, ts => by simp only [rowsLikeF, rowLikeF_same n, rowsLikeF_same n]

theorem rowLikeF_same : ∀ (n : Nat) (Γ : Ctx) (r : List Expr) (ts : List Ty),
    rowLikeF env n Γ r ts = rowLikeF env' n Γ r ts
  | 0, _, _, _ => by simp only [rowLikeF]
  | n + 1, Γ, e :: es, t :: ts => by simp only [rowLikeF, synthF_same n, rowLikeF_same n]
  | n + 1, Γ, [], _ => by simp only [rowLikeF]
  | n + 1, Γ, _ :: _, [] => by simp only [rowLikeF]

theorem callF_same : ∀ (n : Nat) (Γ : Ctx) (f : String) (args : List (Option Mode × Expr)) (exp : Option Ty),
    callF env n Γ f args exp = callF env' n Γ f args exp
  | 0, _, _, _, _ => by simp only [callF]
  | n + 1, Γ, f, args, exp => by simp only [callF, argsF_same n, h.call]

theorem argsF_same : ∀ (n : Nat) (Γ : Ctx) (args : List (Option Mode × Expr)) (ps : List (Mode × Ty)),
    argsF env n Γ args ps = argsF env' n Γ args ps
  | 0, _, _, _ => by simp only [argsF]
  | n + 1, Γ, (mk, e) :: args, (mode, t) :: ps => by simp only [argsF, checkF_same n, argsF_same n]
  | n + 1, Γ, [], _ => by simp only [argsF]
  | n + 1, Γ, _ :: _, [] => by simp only [argsF]

theorem checkF_same : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), checkF env n Γ e t = checkF env' n Γ e t
  | 0, _, _, _ => by simp only [checkF]
  | n + 1, Γ, e, t => by cases e <;> simp only [checkF, checkForm_same n]

theorem checkForm_same : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), checkForm env n Γ e t = checkForm env' n Γ e t
  | 0, _, _, _ => by simp only [checkForm]
  | n + 1, Γ, e, t => by
    cases e <;> simp only [checkForm, checkF_same n, pairsF_same n, fieldsF_same n, allF_same n,
      entriesF_same n, rowsF_same n, synthF_same n, pairF_same n, subsumeF_same n, callF_same n]

theorem subsumeF_same : ∀ (n : Nat) (Γ : Ctx) (e : Expr) (t : Ty), subsumeF env n Γ e t = subsumeF env' n Γ e t
  | 0, _, _, _ => by simp only [subsumeF]
  | n + 1, Γ, e, t => by simp only [subsumeF, synthF_same n]

theorem pairsF_same : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (ts : List Ty), pairsF env n Γ es ts = pairsF env' n Γ es ts
  | 0, _, _, _ => by simp only [pairsF]
  | n + 1, Γ, e :: es, t :: ts => by simp only [pairsF, checkF_same n, pairsF_same n]
  | n + 1, Γ, [], _ => by simp only [pairsF]
  | n + 1, Γ, _ :: _, [] => by simp only [pairsF]

theorem fieldsF_same : ∀ (n : Nat) (Γ : Ctx) (fs : List (String × Expr)) (ts : List (String × Ty)),
    fieldsF env n Γ fs ts = fieldsF env' n Γ fs ts
  | 0, _, _, _ => by simp only [fieldsF]
  | n + 1, Γ, (m, e) :: fs, (k, t) :: ts => by simp only [fieldsF, checkF_same n, fieldsF_same n]
  | n + 1, Γ, [], _ => by simp only [fieldsF]
  | n + 1, Γ, _ :: _, [] => by simp only [fieldsF]

theorem allF_same : ∀ (n : Nat) (Γ : Ctx) (es : List Expr) (t : Ty), allF env n Γ es t = allF env' n Γ es t
  | 0, _, _, _ => by simp only [allF]
  | n + 1, Γ, [], t => by simp only [allF]
  | n + 1, Γ, e :: es, t => by simp only [allF, checkF_same n, allF_same n]

theorem entriesF_same : ∀ (n : Nat) (Γ : Ctx) (es : List (Expr × Expr)) (k v : Ty),
    entriesF env n Γ es k v = entriesF env' n Γ es k v
  | 0, _, _, _, _ => by simp only [entriesF]
  | n + 1, Γ, [], k, v => by simp only [entriesF]
  | n + 1, Γ, (ke, ve) :: es, k, v => by simp only [entriesF, checkF_same n, entriesF_same n]

theorem rowsF_same : ∀ (n : Nat) (Γ : Ctx) (rows : List (List Expr)) (cols : List (String × Ty)),
    rowsF env n Γ rows cols = rowsF env' n Γ rows cols
  | 0, _, _, _ => by simp only [rowsF]
  | n + 1, Γ, [], cols => by simp only [rowsF]
  | n + 1, Γ, r :: rs, cols => by simp only [rowsF, pairsF_same n, rowsF_same n]

end

theorem bindChecked_same (n : Nat) (Γ : Ctx) (p : Pat) (hint : Option Hint) (e : Expr) (k : Kind) :
    bindChecked env n Γ p hint e k = bindChecked env' n Γ p hint e k := by
  unfold bindChecked
  simp only [synthF_same h n, checkF_same h n]

theorem forBindings_same (n : Nat) (Γ : Ctx) (names : List String) (coll : Expr) :
    forBindings env n Γ names coll = forBindings env' n Γ names coll := by
  unfold forBindings
  simp only [synthF_same h n]

theorem setValue_same (n : Nat) (Γ : Ctx) (t : Ty) (op : Option BinOp) (e : Expr) :
    setValue env n Γ t op e = setValue env' n Γ t op e := by
  unfold setValue
  simp only [checkF_same h n]

theorem setIndex_same (n : Nat) (Γ : Ctx) (t : Ty) (k : Expr) (mode : Option IdxMode) (op : Option BinOp)
    (e : Expr) : setIndex env n Γ t k mode op e = setIndex env' n Γ t k mode op e := by
  unfold setIndex
  simp only [checkF_same h n, setValue_same h n]

theorem setSteps_same (n : Nat) (Γ : Ctx) (op : Option BinOp) (e : Expr) :
    ∀ (t : Ty) (steps : List Step), setSteps env n Γ t op e steps = setSteps env' n Γ t op e steps
  | t, [] => by simp only [setSteps, setValue_same h n]
  | t, [.index k mode] => by simp only [setSteps, setIndex_same h n]
  | t, .field s :: rest => by simp only [setSteps, setSteps_same n Γ op e _ rest]
  | t, .index _ none :: _ :: _ => by simp only [setSteps]
  | t, .index k (some m) :: st :: rest => by simp only [setSteps, checkF_same h n, setSteps_same n Γ op e _ (st :: rest)]

theorem checkSet_same (n : Nat) (Γ : Ctx) (x : String) (steps : List Step) (op : Option BinOp) (e : Expr) :
    checkSet env n Γ x steps op e = checkSet env' n Γ x steps op e := by
  unfold checkSet
  simp only [setSteps_same h n]

theorem checkRet_same (n : Nat) (Γ : Ctx) (e : Option Expr) : checkRet env n Γ e = checkRet env' n Γ e := by
  unfold checkRet
  simp only [checkF_same h n]

mutual

theorem checkStmtF_same : ∀ (n : Nat) (Γ : Ctx) (s : Stmt), checkStmtF env n Γ s = checkStmtF env' n Γ s
  | 0, _, _ => by simp only [checkStmtF]
  | n + 1, Γ, s => by
    cases s
    case var_ p hint v =>
      cases v <;> cases hint <;> simp only [checkStmtF, bindChecked_same h n]
    case if_ c tB eB t e =>
      cases tB <;> cases eB <;> simp only [checkStmtF, checkF_same h n, synthF_same h n, checkBlockF_same n,
        checkElse_same n]
    case loop c body => cases c <;> simp only [checkStmtF, checkF_same h n, checkBlockF_same n]
    all_goals simp only [checkStmtF, bindChecked_same h n, checkSet_same h n, checkRet_same h n,
      synthF_same h n, checkBlockF_same n, checkElse_same n, checkCases_same n, forBindings_same h n]

theorem checkCases_same : ∀ (n : Nat) (Γ : Ctx) (vs : List (String × Option Ty)) (cs : List Case)
    (covered : List String), checkCases env n Γ vs cs covered = checkCases env' n Γ vs cs covered
  | 0, _, _, _, _ => by simp only [checkCases]
  | n + 1, Γ, vs, [], covered => by simp only [checkCases]
  | n + 1, Γ, vs, c :: cs, covered => by
    rw [checkCases.eq_def env', checkCases.eq_def env]
    simp only [checkBlockF_same n, checkCases_same n]

theorem checkElse_same : ∀ (n : Nat) (Γ : Ctx) (e : Option (List Stmt)), checkElse env n Γ e = checkElse env' n Γ e
  | n, Γ, some e => by simp only [checkElse, checkBlockF_same n]
  | n, Γ, none => by simp only [checkElse]

theorem checkBlockF_same : ∀ (n : Nat) (Γ : Ctx) (ss : List Stmt), checkBlockF env n Γ ss = checkBlockF env' n Γ ss
  | 0, _, _ => by simp only [checkBlockF]
  | n + 1, Γ, [] => by simp only [checkBlockF]
  | n + 1, Γ, s :: ss => by simp only [checkBlockF, checkStmtF_same n, checkBlockF_same n]

end

end

end Datalove.Fun
