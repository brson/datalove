import Datalove.FunInit

/-!
# A const reads only consts evaluated before it (F089)

A script's top-level consts are evaluated first, in order (`constPass`),
each from the consts' values alone, calling functions from a table in which
a const not evaluated yet has no value. A const may call a function declared
below it, so what a call reads is not settled by order alone: the checker
refuses a const that depends, through the functions it calls, on itself or
on a const declared after it (F089, `check_const_deps` in
`datalove-datafun-tycheck`).

This is that check, put as the analysis of what holds a value
(`FunInit.lean`) puts it. For each const, in order, it takes the functions
its expression can reach by calls (`reach`), and runs the analysis on the
const and on each of those functions with only their signatures, so a call
outside them is refused, and with each const a function sees holding a
value only where it is one evaluated before, as `constEnv` gives the run
its value. A const that reads a later one, itself, or a variable that is not
a const, directly or through a call, is refused.

The Rust looks for the consts each function names; this asks the analysis
whether one is read. The analysis reads on through code after a `ret` or
`break` as if it ran, so a const named there is read too. They differ only
where the analysis refuses a function for its own variables, where this
refuses the const and `initScript` the script; `consts_agrees` in
`fun_tests` tests that.

`Proofs/ConstSound.lean` shows that where this lets a script through, and
the analysis does, evaluating its consts never reads what holds nothing.

A module's consts are checked the same way, with what it imports in scope
(`constModuleWith`, `constWorld`); the model does not run a world, so that
is held to the Rust by `world_consts_agrees` alone.
-/

namespace Datalove.Fun

mutual

/-- The functions an expression calls by name, as the analysis meets them. -/
def callsE : Expr → List String
  | .call f args => f :: callsArgs args
  | .place _ steps => callsSteps steps
  | .hinted _ a | .adapt a | .some_ a | .ok a | .er a | .data a | .error a | .term _ a | .enum a
  | .unop _ a | .tryOpt a | .tryRes a | .proj a _ => callsE a
  | .binop _ a b | .index a b _ => callsE a ++ callsE b
  | .tuple es | .list es | .set es | .tensor _ es => callsAll es
  | .struct fs => callsFields fs
  | .map es => callsEntries es
  | .table _ rows => callsRows rows
  | .none_ | .tt | .ff | .str _ | .int _ | .float | .hex _ | .atom _ | .var _ => []

def callsSteps : List Step → List String
  | [] => []
  | .field _ :: rest => callsSteps rest
  | .index k _ :: rest => callsE k ++ callsSteps rest

def callsArgs : List (Option Mode × Expr) → List String
  | [] => []
  | (_, e) :: args => callsE e ++ callsArgs args

def callsAll : List Expr → List String
  | [] => []
  | e :: es => callsE e ++ callsAll es

def callsFields : List (String × Expr) → List String
  | [] => []
  | (_, e) :: fs => callsE e ++ callsFields fs

def callsEntries : List (Expr × Expr) → List String
  | [] => []
  | (k, v) :: es => callsE k ++ callsE v ++ callsEntries es

def callsRows : List (List Expr) → List String
  | [] => []
  | r :: rs => callsAll r ++ callsRows rs

end

mutual

/-- The functions a statement calls by name. -/
def callsS : Stmt → List String
  | .let_ _ _ e | .var_ _ _ (some e) | .debuglog e | .call e | .const_ _ _ e | .ret (some e) => callsE e
  | .set_ _ steps _ e => callsE e ++ callsSteps steps
  | .if_ c _ _ t e => callsE c ++ callsB t ++ callsElse e
  | .loop c body => (c.map callsE).getD [] ++ callsB body
  | .for_ _ coll body => callsE coll ++ callsB body
  | .match_ e cases dflt => callsE e ++ callsCases cases ++ callsElse dflt
  | .var_ _ _ none | .fun_ _ | .ret none | .brk | .cont | .type_ _ _ => []

def callsB : List Stmt → List String
  | [] => []
  | s :: ss => callsS s ++ callsB ss

def callsElse : Option (List Stmt) → List String
  | some b => callsB b
  | none => []

def callsCases : List Case → List String
  | [] => []
  | .atom _ b :: cs => callsB b ++ callsCases cs
  | .term _ _ b :: cs => callsB b ++ callsCases cs

end

/-- The functions reachable from `todo` by calls through the bodies in
`defs`, with those already found, `seen`. Each step takes one from `todo`,
and `n` steps are enough for every call written in `defs` and in `todo`. -/
def reach (defs : List FunDef) : Nat → List String → List String → List String
  | 0, seen, _ => seen
  | _ + 1, seen, [] => seen
  | n + 1, seen, f :: todo =>
    if seen.contains f then reach defs n seen todo
    else reach defs n (f :: seen) (todo ++ (defs.filter (·.name == f)).flatMap (callsB ·.body))

/-- The functions a const's expression `e` can reach by calls. -/
def reachOf (defs : List FunDef) (e : Expr) : List String :=
  reach defs ((callsE e).length + (defs.flatMap (callsB ·.body)).length + 1) [] (callsE e)

/-- Each of `vars`, holding a value where it is a const among `known`, by
name and type, as `constEnv` finds its value in a store of them. -/
def constMarks (vars : List (String × Ty × Kind)) (known : List (String × Ty)) : List (String × Bool) :=
  vars.map fun (x, t, k) => (x, k == .const_ && known.any fun (y, u) => y == x && u == t)

/-- The analysis of each of `fds`, with signatures `A`, each const it sees
holding a value where it is one of `known` it sees. -/
def funsOk (A : List (String × Sig)) (known : List (String × Ty)) : List FunDef → InitM Unit
  | [] => pure ()
  | fd :: fds =>
    initFun A (constMarks fd.consts (known.drop (known.length - fd.seen))) fd.params fd.body >>= fun _ =>
    funsOk A known fds

/-- The analysis of a const `s` reached in `Γ`, the consts in `known`
evaluated: the const itself, on the variables in scope, and each function
it can reach, on the consts that function sees, with only the signatures of
those functions. -/
def constOk (sigs : List (String × Sig)) (defs : List FunDef) (Γ : Ctx) (known : List (String × Ty))
    (s : Stmt) (e : Expr) : InitM Unit :=
  let R := reachOf defs e
  let A := sigs.filter fun (f, _) => R.contains f
  initStmt A ((constMarks Γ.vars known).map fun (x, b) => (x, b, false)) s >>= fun _ =>
  funsOk A known (defs.filter fun fd => R.contains fd.name)

/-- A script's top-level consts from `Γ`, as `constPass` evaluates them,
`known` those evaluated, the latest first. -/
def constTop (sigs : List (String × Sig)) (defs : List FunDef) :
    Ctx → List (String × Ty) → List Stmt → InitM Unit
  | _, _, [] => pure ()
  | Γ, known, .fun_ _ :: ss => constTop sigs defs Γ known ss
  | Γ, known, s :: ss =>
    let Γ' := ((checkStmt Γ s).run #[]).1
    match s with
    | .const_ _ _ e =>
      constOk sigs defs Γ known s e >>= fun _ =>
      match Γ'.vars with
      | (x, t, _) :: _ => constTop sigs defs Γ' ((x, t) :: known) ss
      | [] => pure ()
    | _ => constTop sigs defs Γ' known ss

/-- Whether a script's consts are F089: one reads, directly or through the
functions it calls, a const not evaluated before it. -/
def constScript (ss : List Stmt) : Option Lit.Code :=
  let sigs := (resolveNames ss).1
  let Γ : Ctx := { ret := .ty (.result unitTy), sigs }
  match constTop sigs (funDefs sigs Γ 0 ss) Γ [] ss with
  | .ok _ => none
  | .error _ => some "F089"

/-- Whether a module's consts are F089, its own functions' signatures and
`extra`, those it imports, in scope. A const may call an imported function,
whose module's consts are all evaluated before this module's are, so the
search follows only the module's own functions, as `check_const_deps` does,
and the analysis needs only the imported signatures. -/
def constModuleWith (extra : List (String × Sig)) (ss : List Stmt) : Option Lit.Code :=
  let sigs := (resolveNames ss).1 ++ extra
  let Γ : Ctx := { ret := .void, sigs, inModule := true }
  match constTop sigs (funDefs sigs Γ 0 ss) Γ [] ss with
  | .ok _ => none
  | .error _ => some "F089"

/-- For each module of a world, in order, what the analysis of what holds a
value refuses first, and F089 if one of its consts reads one not evaluated
before it. -/
def constWorld (ms : List ModuleSrc) : List (Option Lit.Code × Option Lit.Code) :=
  let own := ms.map fun m => (m.path, (resolveNames m.body).1)
  ms.map fun m =>
    let extra := (worldImports own m).2
    (initModuleWith extra m.body, constModuleWith extra m.body)

end Datalove.Fun
