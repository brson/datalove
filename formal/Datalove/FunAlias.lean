import Datalove.FunRun

/-!
# Arguments of one call that alias (O010)

The Rust passes every argument by pointer, and the model by copy-in,
copy-out; `Proofs/PassBy.lean` shows the two agree where no argument the
callee writes is reached by another passed by pointer. O010 is what sees to
that, and this is O010 as the ownership analysis decides it
(`check_argument_aliasing` in `datalove-datafun-ownership`), as botspec's
table of a call's arguments says.

Two arguments are compared where each names a place, by the variable it
starts from and its fields up to the first index, which may reach any
element (`placeKey`): they overlap where they start from one variable and
the fields of one begin the other's. A hint leaves a place the place
(`placeOf`): `(: T / p).a` is `p.a`, as the Rust's parser folds it. Of two
that overlap, the earlier and the later are refused
by how their parameters take them (`clash`): a copy given to an `in`
parameter never, as it is read into a value of its own before the call;
otherwise a `mut` or `out` one with anything, and a `ref` one with a later
one given away.

`Proofs/AliasSound.lean` shows that a call O010 accepts gives `PassBy` what
it asks.
-/

namespace Datalove.Fun

/-- How a parameter takes its argument, as O010 tells them apart. -/
inductive Takes where
  /-- `in`, of a copy type: read into a value of its own before the call. -/
  | copyIn
  /-- `in`, of any other type: given away. -/
  | moveIn
  | ref
  /-- `mut` or `out`. -/
  | write
  deriving DecidableEq, Repr

/-- How a parameter of mode `m` takes an argument, a copy or not. A const
parameter takes it as `in` does. -/
def Takes.of (m : Mode) (copy : Bool) : Takes :=
  match m with
  | .in_ | .const_ => if copy then .copyIn else .moveIn
  | .ref => .ref
  | .mut | .out => .write

/-- A place's fields up to its first index. -/
def fieldsBefore : List Step → List Sel
  | .field s :: ss => s :: fieldsBefore ss
  | _ => []

/-- The place an argument names, as O010 tells places apart: the variable it
starts from and its fields up to the first index. -/
def placeKey (e : Expr) : Option (String × List Sel) :=
  (placeOf e).map fun (x, ss) => (x, fieldsBefore ss)

/-- Two places overlap where they start from one variable and the fields of
one begin the other's. -/
def overlaps (p q : String × List Sel) : Bool :=
  p.1 == q.1 && (p.2.isPrefixOf q.2 || q.2.isPrefixOf p.2)

/-- Whether an earlier argument taken as `t` and a later one taken as `u`,
naming places that overlap, are O010. -/
def clash : Takes → Takes → Bool
  | .copyIn, _ | _, .copyIn => false
  | .write, _ | _, .write => true
  | .ref, .moveIn => true
  | _, _ => false

/-- Whether an earlier argument and a later one are O010. -/
def pairAliased (a b : Takes × Expr) : Bool :=
  match placeKey a.2, placeKey b.2 with
  | some p, some q => overlaps p q && clash a.1 b.1
  | _, _ => false

/-- Whether a call's arguments, each with how its parameter takes it, are
O010: some earlier one and later one are. -/
def aliased : List (Takes × Expr) → Bool
  | [] => false
  | a :: rest => rest.any (pairAliased a) || aliased rest

/-- A call's arguments with how its parameters take them, in context `Γ`:
each argument a copy or not by the type it synthesizes. -/
def callTakes (Γ : Ctx) (f : String) (args : List (Option Mode × Expr)) : List (Takes × Expr) :=
  match Γ.sigs.lookup f with
  | some sig => (sig.params.zip args).map fun ((m, _), (_, e)) =>
      (Takes.of m (match (synthF inferEnv fuel Γ e).run.val with
        | .ok t => t.isCopy
        | .error _ => false), e)
  | none => []

/-- For each top-level `call` of a script, in order, whether its arguments
are O010, each in the context the checker reaches it in. -/
def aliasTop : Ctx → List Stmt → List Bool
  | _, [] => []
  | Γ, s :: ss =>
    let here := match s with
      | .call (.call f args) => [aliased (callTakes Γ f args)]
      | _ => []
    here ++ aliasTop (if s matches .fun_ _ then Γ else ((checkStmt Γ s).run #[]).1) ss

/-- For each top-level `call` of a script, whether its arguments are O010. -/
def aliasScript (ss : List Stmt) : List Bool :=
  let sigs := (resolveNames ss).1
  aliasTop { ret := .ty (.result unitTy), sigs } ss

end Datalove.Fun
