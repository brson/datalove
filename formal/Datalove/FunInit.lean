import Datalove.FunRun

/-!
# What holds a value (O005, O006, O009)

A `var` declared with no value, and an `out` parameter, hold nothing until
something is written to them, and the model's run stops at reading one
(`Stop.uninit`). The ownership analysis refuses a program that might: a read
of a variable that may hold nothing (O005), a return while an `out`
parameter may hold nothing (O006), and a write to a part of a variable that
may hold nothing (O009). This is that analysis, as
`datalove-datafun-ownership` makes it with its `out_param_init` state.

It follows a body in the order the model runs it, keeping for each variable
in scope, innermost first as the run keeps them, whether it holds a value on
every way to here, and whether it is an `out` parameter (`ISt`). A `var` with
no value and an `out` parameter start holding nothing; `set x = e`, and
passing `x` to a `mut` or `out` parameter, give `x` a value once the call
returns. After an `if`, a variable holds a value only if both branches gave
it one, and after a `match` only if every arm that can complete did. A loop
is left with what its condition, or running out of elements, leaves, and
with what each `break` leaves; its body is followed once, from what it is
entered with, since a pass only adds values. A return, by `ret`, at the end
of a body that can complete, or early, from a `?`, a `!`, a checked or
optional operator or an index that returns early, needs every `out`
parameter to hold a value.

An early return inside an expression is checked where it happens. The Rust
checks it against what holds a value once the whole statement's expression
has run, which a `mut` or `out` argument after the early return can change:
`o? + get(out p)` leaves without writing `p` when `o` is none, and the Rust
lets it through.

A `mut` parameter is kept as an `out` one is, as a parameter the caller
reads back: it starts with a value, so the check at a way out never fails
for it, but the proof needs to know it holds one there.

`Proofs/InitSound.lean` shows that a script this accepts, and that checks,
stops at `Stop.uninit` only while evaluating its consts, and
`Proofs/ConstSound.lean` that with F089 (`FunConsts.lean`) it does not
stop there at all.
-/

namespace Datalove.Fun

/-- A variable as the analysis keeps it: its name, whether it holds a value
on every way to here, and whether it is a parameter the caller reads back, a
`mut` or `out` one, which has to hold a value wherever the function returns. -/
abbrev IVar := String × Bool × Bool

/-- The variables in scope, innermost first, as the run keeps them. -/
abbrev ISt := List IVar

/-- Whether the innermost variable named `x` holds a value, if there is one. -/
def ISt.holds (σ : ISt) (x : String) : Option Bool := (σ.lookup x).map (·.1)

/-- The innermost variable named `x` given a value, as `setVar` sets it. -/
def ISt.give (x : String) : ISt → ISt
  | [] => []
  | (y, s, o) :: σ => if y == x then (y, true, o) :: σ else (y, s, o) :: ISt.give x σ

/-- Whether every `mut` and `out` parameter holds a value. A `mut` one
starts with one and nothing takes it away, so this asks it of the `out`
ones. -/
def ISt.outsSet (σ : ISt) : Bool := σ.all fun (_, s, o) => s || !o

/-- What two ways to one point agree holds a value. -/
def ISt.meet : ISt → ISt → ISt
  | (x, s, o) :: σ, (_, t, _) :: τ => (x, s && t, o) :: ISt.meet σ τ
  | _, _ => []

/-- `τ`, of a scope inside `σ`'s, back to `σ`'s: what it bound dropped, as
`restore` drops it. -/
def ISt.back (σ τ : ISt) : ISt := τ.drop (τ.length - σ.length)

/-- What all of the ways agree on, or `σ` with none. -/
def ISt.meetAll (σ : ISt) : List ISt → ISt
  | [] => σ
  | [τ] => τ
  | τ :: τs => τ.meet (ISt.meetAll σ τs)

/-- Names bound in order, each innermost of those before it, as `bindVals`
binds them, holding a value or not. -/
def ISt.bind (σ : ISt) (xs : List String) (s : Bool) : ISt :=
  xs.foldl (fun σ x => (x, s, false) :: σ) σ

abbrev InitM := Except Lit.Code

/-- A read of `x`, which has to hold a value (O005). -/
def readI (σ : ISt) (x : String) : InitM ISt :=
  if σ.holds x == some false then throw "O005" else pure σ

/-- A way out of the function, where every `out` parameter has to hold a
value (O006). -/
def exitI (σ : ISt) : InitM ISt :=
  if σ.outsSet then pure σ else throw "O006"

/-- Whether an operator can return early: a checked or optional one. -/
def BinOp.exits (op : BinOp) : Bool :=
  match needs op with
  | .nothing => false
  | _ => true

/-- Whether a place's steps index with `?` or `!`, which writing back through
them can return early from. -/
def stepsExit (steps : List Step) : Bool :=
  steps.any fun | .index _ (some _) => true | _ => false

/-- What a call's `mut` and `out` arguments' variables hold once it returns:
each a value, as `writeBack` writes them. A temporary has none. -/
def giveArgs : ISt → List (Option Mode × Expr) → List Mode → ISt
  | σ, (_, e) :: args, m :: ms =>
    let σ := if m == .mut || m == .out then
        match placeOf e with
        | some (x, _) => σ.give x
        | none => σ
      else σ
    giveArgs σ args ms
  | σ, _, _ => σ

/-- Whether a call writes back through an index with `?` or `!`, which can
return early once it has returned. -/
def argsExit : List (Option Mode × Expr) → List Mode → Bool
  | (_, e) :: args, m :: ms =>
    ((m == .mut || m == .out) && match placeOf e with
      | some (_, steps) => stepsExit steps
      | none => false) || argsExit args ms
  | _, _ => false

mutual

/-- An expression, as `evalS` runs it: what holds a value after it, or the
first thing refused. A call to a function `sigs` has no signature for is
refused: under a script's signatures only a script the checker refuses makes
one, and under a const's (`FunConsts.lean`) it is a call the const does not
reach. -/
def initE (sigs : List (String × Sig)) : ISt → Expr → InitM ISt
  | σ, .var x => readI σ x
  | σ, .place x steps => readI σ x >>= fun σ => initSteps sigs σ steps
  | σ, .hinted _ a => initE sigs σ a
  | σ, .adapt a => initE sigs σ a
  | σ, .some_ a => initE sigs σ a
  | σ, .ok a => initE sigs σ a
  | σ, .er a => initE sigs σ a
  | σ, .data a => initE sigs σ a
  | σ, .error a => initE sigs σ a
  | σ, .term _ a => initE sigs σ a
  | σ, .enum a => initE sigs σ a
  | σ, .binop op a b => do
    let σ ← initE sigs σ a
    let σ ← initE sigs σ b
    if op.exits then exitI σ else pure σ
  | σ, .unop op a => do
    let σ ← initE sigs σ a
    match op with
    | .negC | .negO => exitI σ
    | _ => pure σ
  | σ, .tryOpt a => initE sigs σ a >>= exitI
  | σ, .tryRes a => initE sigs σ a >>= exitI
  | σ, .proj b _ => initE sigs σ b
  | σ, .index b k (some _) => do
    let σ ← initE sigs σ b
    let σ ← initE sigs σ k
    exitI σ
  | σ, .index _ _ none => pure σ
  | σ, .call f args => match sigs.lookup f with
    | some sig => do
      let ms := sig.params.map (·.1)
      let σ ← initArgs sigs σ args ms
      let σ ← if argsExit args ms then exitI σ else pure σ
      pure (giveArgs σ args ms)
    | none => throw "F089"
  | σ, .tuple es => initAll sigs σ es
  | σ, .list es => initAll sigs σ es
  | σ, .set es => initAll sigs σ es
  | σ, .tensor _ es => initAll sigs σ es
  | σ, .struct fs => initFields sigs σ fs
  | σ, .map es => initEntries sigs σ es
  | σ, .table _ rows => initRows sigs σ rows
  | σ, .none_ | σ, .tt | σ, .ff | σ, .str _ | σ, .int _ | σ, .float | σ, .hex _ | σ, .atom _ => pure σ

/-- A place's steps, each key in order, an index with `?` or `!` a way out. -/
def initSteps (sigs : List (String × Sig)) : ISt → List Step → InitM ISt
  | σ, [] => pure σ
  | σ, .field _ :: rest => initSteps sigs σ rest
  | σ, .index k m :: rest => do
    let σ ← initE sigs σ k
    let σ ← match m with
      | some _ => exitI σ
      | none => pure σ
    initSteps sigs σ rest

/-- A `mut` or `out` argument's place, as `placeS` reaches it: its variable
holds a value where a part of it is reached, or where `need` asks, for a
`mut` one. -/
def initPlace (sigs : List (String × Sig)) : ISt → Expr → Bool → InitM ISt
  | σ, .var x, need => if need then readI σ x else pure σ
  | σ, .place x steps, _ => readI σ x >>= fun σ => initSteps sigs σ steps
  | σ, .hinted _ a, need => initPlace sigs σ a need
  | σ, .proj b _, _ => initPlace sigs σ b true
  | σ, .index b k (some _), _ => do
    let σ ← initPlace sigs σ b true
    let σ ← initE sigs σ k
    exitI σ
  | σ, _, _ => pure σ

/-- A call's arguments, in order, each as its parameter takes it. -/
def initArgs (sigs : List (String × Sig)) : ISt → List (Option Mode × Expr) → List Mode → InitM ISt
  | σ, (_, e) :: args, m :: ms => do
    let σ ← match placeOf e with
      | some _ =>
        if m == .mut || m == .out then initPlace sigs σ e (m == .mut) else initE sigs σ e
      | none => if m == .out then pure σ else initE sigs σ e
    initArgs sigs σ args ms
  | σ, _, _ => pure σ

def initAll (sigs : List (String × Sig)) : ISt → List Expr → InitM ISt
  | σ, [] => pure σ
  | σ, e :: es => initE sigs σ e >>= fun σ => initAll sigs σ es

def initFields (sigs : List (String × Sig)) : ISt → List (String × Expr) → InitM ISt
  | σ, [] => pure σ
  | σ, (_, e) :: fs => initE sigs σ e >>= fun σ => initFields sigs σ fs

def initEntries (sigs : List (String × Sig)) : ISt → List (Expr × Expr) → InitM ISt
  | σ, [] => pure σ
  | σ, (k, v) :: es => do
    let σ ← initE sigs σ k
    let σ ← initE sigs σ v
    initEntries sigs σ es

def initRows (sigs : List (String × Sig)) : ISt → List (List Expr) → InitM ISt
  | σ, [] => pure σ
  | σ, r :: rs => initAll sigs σ r >>= fun σ => initRows sigs σ rs

end

/-- A `set`'s keys, in order, each a way out where it indexes with `?` or
`!`, as `execSet` evaluates them before it reaches the place. -/
def initKeys (sigs : List (String × Sig)) : ISt → List Step → InitM ISt
  | σ, [] => pure σ
  | σ, .field _ :: rest => initKeys sigs σ rest
  | σ, .index k _ :: rest => initE sigs σ k >>= fun σ => initKeys sigs σ rest

/-- What a `set` asks of the variable it writes: a compound one reads it
(O005), and one through steps writes a part of it, which has to hold a value
(O009). -/
def initTarget (σ : ISt) (x : String) (steps : List Step) : Option BinOp → InitM ISt
  | some _ => readI σ x
  | none => if !steps.isEmpty && σ.holds x == some false then throw "O009" else pure σ

/-- `set x … = e` or `set x … ⊕= e`: the value, then the keys, then the
place, which a compound operator or an index with `?` or `!` on the way to
it can return early from. -/
def initSet (sigs : List (String × Sig)) (σ : ISt) (x : String) (steps : List Step) (op : Option BinOp)
    (e : Expr) : InitM ISt :=
  initE sigs σ e >>= fun σ =>
  initKeys sigs σ steps >>= fun σ =>
  initTarget σ x steps op >>= fun σ =>
  (if stepsExit steps || op.any BinOp.exits then exitI σ else pure σ) >>= fun σ =>
  pure (if steps.isEmpty then σ.give x else σ)

/-- What a statement, a block or a `match`'s cases leave: what holds a value
where it completes, and at each of its `break`s. -/
abbrev IOut := ISt × List ISt

mutual

/-- A statement, as `execStmt` runs it. -/
def initStmt (sigs : List (String × Sig)) : ISt → Stmt → InitM IOut
  | σ, .let_ p _ e => do let σ ← initE sigs σ e; pure (σ.bind (patNames p) true, [])
  | σ, .var_ p _ (some e) => do let σ ← initE sigs σ e; pure (σ.bind (patNames p) true, [])
  | σ, .var_ p (some _) none => pure (σ.bind (patNames p) false, [])
  | σ, .var_ _ none none => pure (σ, [])
  | σ, .set_ x steps op e => do pure (← initSet sigs σ x steps op e, [])
  | σ, .fun_ _ => pure (σ, [])
  | σ, .ret e => do
    let σ ← exitI σ
    match e with
    | some e => pure (← initE sigs σ e, [])
    | none => pure (σ, [])
  | σ, .if_ c tB eB t e =>
    initE sigs σ c >>= fun σc =>
    initBlock sigs (σc.bind tB.toList true) t >>= fun rt =>
    initElse sigs (σc.bind eB.toList true) e >>= fun re =>
    pure ((σc.back rt.1).meet (σc.back re.1), rt.2 ++ re.2)
  | σ, .loop c body =>
    (match c with
      | some c => initE sigs σ c
      | none => pure σ) >>= fun σc =>
    initBlock sigs σc body >>= fun r =>
    pure (σc.meetAll ((if c.isSome then [σc] else []) ++ r.2.map σc.back), [])
  | σ, .brk => pure (σ, [σ])
  | σ, .cont => pure (σ, [])
  | σ, .debuglog e => do pure (← initE sigs σ e, [])
  | σ, .call e => do pure (← initE sigs σ e, [])
  | σ, .const_ x _ e => do let σ ← initE sigs σ e; pure (σ.bind [x] true, [])
  | σ, .type_ _ _ => pure (σ, [])
  | σ, .match_ e cases dflt =>
    initE sigs σ e >>= fun σc =>
    initCases sigs σc cases >>= fun r =>
    initDflt sigs σc dflt >>= fun d =>
    pure (σc.meetAll (r.1 ++ d.1), r.2 ++ d.2)
  | σ, .for_ names coll body =>
    initE sigs σ coll >>= fun σc =>
    initBlock sigs (σc.bind names true) body >>= fun r =>
    pure (σc.meetAll (σc :: r.2.map σc.back), [])

/-- An `else`, in its own scope, or nothing, which leaves things as they
were. -/
def initElse (sigs : List (String × Sig)) : ISt → Option (List Stmt) → InitM IOut
  | σ, some e => initBlock sigs σ e
  | σ, none => pure (σ, [])

/-- A `match`'s default, as `initCases` takes a case: what it leaves, back in
`σ`'s scope, where it can complete, and its `break`s. -/
def initDflt (sigs : List (String × Sig)) : ISt → Option (List Stmt) → InitM (List ISt × List ISt)
  | σ, some d => initBlock sigs σ d >>= fun r => pure (if completes d then [σ.back r.1] else [], r.2)
  | _, none => pure ([], [])

/-- A `match`'s cases, each from `σ`: what each that can complete leaves,
back in `σ`'s scope, and every `break` in them. -/
def initCases (sigs : List (String × Sig)) : ISt → List Case → InitM (List ISt × List ISt)
  | _, [] => pure ([], [])
  | σ, .atom _ b :: cs =>
    initBlock sigs σ b >>= fun rb =>
    initCases sigs σ cs >>= fun r =>
    pure (if completes b then σ.back rb.1 :: r.1 else r.1, rb.2 ++ r.2)
  | σ, .term _ x b :: cs =>
    initBlock sigs (σ.bind [x] true) b >>= fun rb =>
    initCases sigs σ cs >>= fun r =>
    pure (if completes b then σ.back rb.1 :: r.1 else r.1, rb.2 ++ r.2)

/-- A block's statements in order. -/
def initBlock (sigs : List (String × Sig)) : ISt → List Stmt → InitM IOut
  | σ, [] => pure (σ, [])
  | σ, s :: ss =>
    initStmt sigs σ s >>= fun r1 =>
    initBlock sigs r1.1 ss >>= fun r2 =>
    pure (r2.1, r1.2 ++ r2.2)

end

/-- What a function's body starts with: its parameters, the last innermost,
each holding a value but an `out` one, then the consts it sees, as
`argVals` binds them, each holding a value or not. -/
def funStart (params : List (Mode × String × Hint)) (consts : List (String × Bool)) : ISt :=
  params.reverse.map (fun (m, x, _) => (x, m != .out, m == .out || m == .mut)) ++
    consts.map fun (x, s) => (x, s, false)

/-- A function's body: what it refuses, if anything. Its end is a way out
where it can complete. -/
def initFun (sigs : List (String × Sig)) (consts : List (String × Bool)) (params : List (Mode × String × Hint))
    (body : List Stmt) : InitM Unit := do
  let (σ, _) ← initBlock sigs (funStart params consts) body
  if completes body then discard (exitI σ)

/-- A script's statements, as `runTop` runs them: each function's body in
the context the checker reaches its declaration in, with the consts it sees
there, and every other statement on the top level's variables. -/
def initTop (sigs : List (String × Sig)) : Ctx → ISt → List Stmt → InitM Unit
  | _, _, [] => pure ()
  | Γ, σ, .fun_ (.mk _ _ params _ body) :: ss =>
    initFun sigs ((constsOf Γ.vars).map (·.1, true)) params body >>= fun _ => initTop sigs Γ σ ss
  | Γ, σ, s :: ss =>
    initStmt sigs σ s >>= fun r => initTop sigs ((checkStmt Γ s).run #[]).1 r.1 ss

/-- What a script's analysis refuses first, if anything: O005, O006 or O009. -/
def initScript (ss : List Stmt) : Option Lit.Code :=
  let sigs := (resolveNames ss).1
  match initTop sigs { ret := .ty (.result unitTy), sigs } [] ss with
  | .ok _ => none
  | .error c => some c

end Datalove.Fun
