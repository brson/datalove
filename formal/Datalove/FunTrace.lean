import Datalove.Proofs.InstanceStmt

/-!
# The types a script's checking gives its bindings

For the oracle: the type `synthF` gives each `let`, `var` and `const` with a
value and no hint, in source order, a function's body where the function is
declared. Each is synthesized in the context the checker reaches it in, so
`fun_tests` can hold the types the Rust infers to the model's, which the
diagnostics alone do not show of a script that checks. Nothing here is
proved; it reads the checker.
-/

namespace Datalove.Fun

/-- What `synthF` gives, if it gives a type. -/
def synthOf (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Expr) : Option Ty :=
  match (synthF env n Γ e).run.val with
  | .ok t => Option.some t
  | .error _ => Option.none

mutual

/-- A statement's binding types, and its bodies', as `checkStmtF` reaches
them. -/
def traceStmt (env : InferEnv) : Nat → Ctx → Stmt → List (Option Ty)
  | 0, _, _ => []
  | n + 1, Γ, s => match s with
    | .let_ _ Option.none e => [synthOf env n Γ e]
    | .var_ _ Option.none (Option.some e) => [synthOf env n Γ e]
    | .const_ _ Option.none e => [synthOf env n { Γ with constExpr := true } e]
    | .if_ _ Option.none Option.none t e => traceBlock env n Γ t ++ traceElse env n Γ e
    | .if_ c a b t e => match synthOf env n Γ c with
      | Option.some ct => match ct.expose with
        | .option p => traceBlock env n (bindOpt Γ a p) t ++ traceElse env n Γ e
        | .result p => traceBlock env n (bindOpt Γ a p) t ++ traceElse env n (bindOpt Γ b .error) e
        | _ => []
      | Option.none => []
    | .loop _ body => traceBlock env n { Γ with inLoop := true } body
    | .for_ names coll body => match ((forBindings env n Γ names coll).run #[]).1 with
      | Option.some Δ => traceBlock env n ({ Γ with inLoop := true }.bindAll Δ) body
      | Option.none => []
    | .match_ e cases dflt => match synthOf env n Γ e with
      | Option.some t => match t.expose with
        | .enum vs => traceCases env n Γ vs.toList cases ++ traceElse env n Γ dflt
        | _ => []
      | Option.none => []
    | _ => []

/-- A `match`'s cases, each body in the context `checkCases` checks it in. -/
def traceCases (env : InferEnv) : Nat → Ctx → List (String × Option Ty) → List Case → List (Option Ty)
  | 0, _, _, _ => []
  | _ + 1, _, _, [] => []
  | n + 1, Γ, vs, c :: cs =>
    (match c, vs.lookup c.name with
      | .term _ x b, Option.some (Option.some p) => traceBlock env n (Γ.bind x p .let_) b
      | c, _ => traceBlock env n Γ c.body) ++ traceCases env n Γ vs cs

def traceElse (env : InferEnv) : Nat → Ctx → Option (List Stmt) → List (Option Ty)
  | n, Γ, Option.some e => traceBlock env n Γ e
  | _, _, Option.none => []

/-- A block's statements, each in the context the one before it leaves. -/
def traceBlock (env : InferEnv) : Nat → Ctx → List Stmt → List (Option Ty)
  | 0, _, _ => []
  | _ + 1, _, [] => []
  | n + 1, Γ, s :: ss => traceStmt env n Γ s ++ traceBlock env n ((checkStmtF env n Γ s).run #[]).1 ss

end

/-- A script's binding types, as `checkScript` checks it: its statements in
order, and each function's body, where it is declared, in the context
`checkFun` checks it in. -/
def traceScript (ss : List Stmt) : List (Option Ty) :=
  let sigs := (resolveNames ss).1
  let rec go (Γ : Ctx) : List Stmt → List (Option Ty)
    | [] => []
    | .fun_ (.mk name tps params _ body) :: rest =>
      (match sigs.lookup name with
        | Option.some sig =>
          traceBlock inferEnv fuel (funCtx sigs Γ.aliases (constsOf Γ.vars) tps params sig) body
        | Option.none => []) ++ go Γ rest
    | s :: rest => traceStmt inferEnv fuel Γ s ++ go ((checkStmt Γ s).run #[]).1 rest
  go { ret := .ty (.result unitTy), sigs } ss

end Datalove.Fun
