import Datalove.FunInit

/-!
# What is moved (O001, O003, O004, O007, O008, O011, O012, O014, O015, O016)

Reading a variable whose type is not copy, where the ownership analysis has
the read take the value rather than borrow it (`Ctx.lends` false: a `let`'s
or a `set`'s value, an `in` argument, a `ret`, a part of what an expression
builds, a condition or `match` that binds), moves its value out, and the
variable holds nothing until something is written to it again. The ownership
analysis refuses a program that reads a moved variable (O001), moves a
borrowed binding -- a `ref`, `mut` or `out` parameter, or a `for`'s element
(O003) -- or a const (O016), or whose ways to one point disagree about
whether a variable was moved: the branches of an `if` or `match` that
complete (O008), a loop's ways out (O014), or a loop body's way back to its
start, which may not have moved what it was given (O007). Nor may a
`for`'s body change what it walks, the variable its collection is a place
in (O015): move it, pass it or a part of it `mut` or `out`, or `set` it or
a part of it, since the walk points into it. What is passed `mut` or `out`
has to be something the caller may change: a `var` or a `mut` or `out`
parameter, not a `ref` parameter or what a `for` binds by reference (O004),
nor a `let`, an `in` parameter, a const or what a condition or `match`
binds (O011), and not a temporary at all (O012). This is that
analysis, as `datalove-datafun-ownership` makes it with its `BindingState`,
beside what holds a value (`FunInit.lean`), which it takes in: a variable
declared with no value holds nothing until written (O005), and an `out`
parameter has to hold a value at every way out (O006), as there.

The two differ at a join. Where branches disagree about whether a variable
was given a value, it holds one after only if every branch gave it one; where
they disagree about whether it was moved, the program is refused.

Moves are decided where the run that moves (`runScriptMoves`) decides
them: the analysis follows each expression with the flag `Ctx.lends` has
there, entering an expression as `Ctx.enter` does, lending for an operand,
`@`, `debuglog`, a compound `set`'s value, a `for`'s collection, a `ref`,
`mut`, `out` or const argument (`Ctx.borrow`, `Ctx.forArg`), the base of a
field or index of a place (`Ctx.under`), and an index's key (`Ctx.key`).
The key a plain `set m[k] = v` inserts the Rust takes, and so does the
analysis, though the run lends it: there the analysis moves what the run
does not, which only refuses more. Each statement is followed at the fuel
and in the context the run gives it (`execStmt`, `execBlock`, `checkStmtF`
at that fuel, `funCtx` for a body), so that the two see the same types and
kinds, and `moves_agree` holds the run to the analysis as well as the
analysis to the Rust.

After an `if` or `match`, what is moved is what one branch left: the first
that completes, or, where none does, the first (`joinIf`, `joinMatch`), as
the Rust does. A loop is left with what its ways out agree on: its
condition or the end of its elements, taken to leave what the loop was
entered with, and each `break`.
-/

namespace Datalove.Fun

/-- A variable as the move analysis keeps it. -/
structure MVar where
  name : String
  /-- Given a value on every way here: false for a `var` declared with none
  and an `out` parameter, until something is written to it. -/
  init : Bool
  /-- Not moved: false once a move takes its value, and true again once
  something is written to it. -/
  live : Bool
  /-- A `mut` or `out` parameter, which the caller reads back. -/
  back : Bool
  /-- A borrowed binding, which a move may not take (O003). -/
  lent : Bool
  /-- May be passed `mut` or `out`: a `var`, or a `mut` or `out`
  parameter. -/
  writable : Bool
  /-- Bound by reference: a `ref` parameter, or a `for`'s element, key or
  value, which passed `mut` or `out` is O004 rather than O011. -/
  byRef : Bool
  /-- The collection a `for` walks, or the variable it is a part of, frozen
  for the loop's body (O015). -/
  walked : Bool := false
  /-- Dropped where it is owned: not a copy, not borrowed, not a const.
  Only the drop schedule (`FunDrop.lean`) sets it. -/
  owns : Bool := false
  /-- Which binding this is, as the Rust's `BindingId` tells two of one
  name apart. Only the drop schedule sets it. -/
  uid : Nat := 0

/-- The variables in scope, innermost first, as the run keeps them. -/
abbrev MSt := List MVar

/-- The innermost variable named `x`. -/
def MSt.find (σ : MSt) (x : String) : Option MVar := σ.find? (·.name == x)

/-- The innermost variable named `x` changed by `f`. -/
def MSt.update (f : MVar → MVar) (x : String) : MSt → MSt
  | [] => []
  | v :: σ => if v.name == x then f v :: σ else v :: MSt.update f x σ

/-- `x` written: it holds a value, moved or not before. -/
def MSt.give (x : String) : MSt → MSt := MSt.update (fun v => { v with init := true, live := true }) x

/-- `x` moved. -/
def MSt.take (x : String) : MSt → MSt := MSt.update (fun v => { v with live := false }) x

/-- Names bound in order, each innermost of those before it. -/
def MSt.bind (σ : MSt) (xs : List String) (init lent : Bool) (writable : Bool := false)
    (byRef : Bool := false) : MSt :=
  xs.foldl (fun σ x => { name := x, init, live := true, back := false, lent, writable, byRef } :: σ) σ

/-- `x` walked by a `for`. -/
def MSt.walk (x : String) : MSt → MSt := MSt.update (fun v => { v with walked := true }) x

/-- Whether `x` is walked by a `for`. -/
def MSt.isWalked (σ : MSt) (x : String) : Bool := (σ.find x).any (·.walked)

/-- `τ`, of a scope inside `σ`'s, back to `σ`'s. -/
def MSt.back (σ τ : MSt) : MSt := τ.drop (τ.length - σ.length)

/-- Whether every `mut` and `out` parameter holds a value. -/
def MSt.outsSet (σ : MSt) : Bool := σ.all fun v => (v.init && v.live) || !v.back

/-- `σ`'s variables, each given a value as `τ`'s is. -/
def MSt.initFrom (σ τ : MSt) : MSt := List.zipWith (fun v w => { v with init := w.init }) σ τ

/-- `σ`'s variables, each given a value only where `τ`'s is too. -/
def MSt.initMeet (σ τ : MSt) : MSt := List.zipWith (fun v w => { v with init := v.init && w.init }) σ τ

/-- Whether `σ` and `τ` agree about what is moved. -/
def MSt.sameMoves (σ τ : MSt) : Bool := (σ.zip τ).all fun (v, w) => v.live == w.live

/-- What two ways to one point leave, where they have to agree about what
is moved, or none if they do not. -/
def MSt.join (σ τ : MSt) : Option MSt := if σ.sameMoves τ then some (σ.initMeet τ) else none

/-- Whether `τ` has moved nothing `σ` held unmoved. -/
def MSt.keepsLive (σ τ : MSt) : Bool :=
  (σ.zip τ).all fun (v, w) => !v.live || w.live

abbrev MoveM := Except Lit.Code

/-- Refuse a change to what a `for` walks (O015). -/
def unwalkedM (σ : MSt) (x : String) : MoveM Unit :=
  if σ.isWalked x then throw "O015" else pure ()

/-- Refuse passing `mut` or `out` what may not be changed: what is bound by
reference (O004), and a `let`, an `in` parameter, a const or a binding of
a condition or `match` (O011). -/
def writableM (σ : MSt) (x : String) : MoveM Unit :=
  match σ.find x with
  | some v => if v.writable then pure () else if v.byRef then throw "O004" else throw "O011"
  | none => pure ()

/-- What the ways in `L` agree on, `code` where they disagree, or `σ` with
none. -/
def joinAll (code : Lit.Code) (σ : MSt) : List MSt → MoveM MSt
  | [] => pure σ
  | τ :: L => L.foldlM (fun acc w => match acc.join w with
      | some j => pure j
      | none => throw code) τ

/-- A read of `x`, which has to hold a value: written (O005) and not moved
(O001). -/
def readM (σ : MSt) (x : String) : MoveM MSt :=
  match σ.find x with
  | some v => if !v.init then throw "O005" else if !v.live then throw "O001" else pure σ
  | none => pure σ

/-- A read of `x` that takes its value, where its type is not copy: not a
const (O016), nor what a `for` walks (O015), nor borrowed (O003). -/
def moveM (Γ : Ctx) (σ : MSt) (x : String) : MoveM MSt := do
  match Γ.lookup x with
  | some (t, .const_) => if t.isCopy then readM σ x else throw "O016"
  | some (t, _) =>
    let σ ← readM σ x
    if t.isCopy then pure σ
    else match σ.find x with
      | some v => if v.walked then throw "O015" else if v.lent then throw "O003" else pure (σ.take x)
      | none => pure σ
  | none => readM σ x

/-- A way out of the function, where every `out` parameter has to hold a
value (O006). -/
def exitM (σ : MSt) : MoveM MSt :=
  if σ.outsSet then pure σ else throw "O006"

/-- What a call's `mut` and `out` arguments' variables hold once it
returns: each a value. -/
def giveArgsM : MSt → List (Option Mode × Expr) → List Mode → MSt
  | σ, (_, e) :: args, m :: ms =>
    let σ := if m == .mut || m == .out then
        match placeOf e with
        | some (x, []) => σ.give x
        | _ => σ
      else σ
    giveArgsM σ args ms
  | σ, _, _ => σ

mutual

/-- An expression, as `evalS` runs it, from a position borrowing or not as
`b` says: what holds a value after it, or the first thing refused. -/
def mE (sigs : List (String × Sig)) (Γ : Ctx) (b : Bool) : MSt → Expr → MoveM MSt
  | σ, .var x => if b then readM σ x else moveM Γ σ x
  | σ, .place x steps => readM σ x >>= fun σ => mSteps sigs Γ σ steps
  | σ, .hinted _ a => mE sigs Γ (b && !buildsFromParts a) σ a
  | σ, .adapt a => mE sigs Γ true σ a
  | σ, .some_ a | σ, .ok a | σ, .er a | σ, .data a | σ, .error a | σ, .term _ a =>
    mE sigs Γ false σ a
  | σ, .enum a => mE sigs Γ b σ a
  | σ, .binop op a c => do
    let σ ← mE sigs Γ true σ a
    let σ ← mE sigs Γ true σ c
    if op.exits then exitM σ else pure σ
  | σ, .unop op a => do
    let σ ← mE sigs Γ true σ a
    match op with
    | .negC | .negO => exitM σ
    | _ => pure σ
  | σ, .tryOpt a => mE sigs Γ b σ a >>= exitM
  | σ, .tryRes a => mE sigs Γ b σ a >>= exitM
  | σ, .proj base _ => mE sigs Γ (b || (placeOf base).isSome) σ base
  | σ, .index base k (some _) => do
    let σ ← mE sigs Γ (b || (placeOf base).isSome) σ base
    let σ ← mE sigs Γ true σ k
    exitM σ
  | σ, .index _ _ none => pure σ
  | σ, .call f args => match sigs.lookup f with
    | some sig => do
      let ms := sig.params.map (·.1)
      let σ ← mArgs sigs Γ σ args ms
      let σ ← if argsExit args ms then exitM σ else pure σ
      pure (giveArgsM σ args ms)
    | none => throw "F089"
  | σ, .tuple es | σ, .list es | σ, .set es | σ, .tensor _ es => mAll sigs Γ σ es
  | σ, .struct fs => mFields sigs Γ σ fs
  | σ, .map es => mEntries sigs Γ σ es
  | σ, .table _ rows => mRows sigs Γ σ rows
  | σ, .none_ | σ, .tt | σ, .ff | σ, .str _ | σ, .int _ | σ, .float | σ, .hex _ | σ, .atom _ => pure σ

/-- A place's steps, each key in order, an index with `?` or `!` a way out. -/
def mSteps (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List Step → MoveM MSt
  | σ, [] => pure σ
  | σ, .field _ :: rest => mSteps sigs Γ σ rest
  | σ, .index k m :: rest => do
    let σ ← mE sigs Γ true σ k
    let σ ← match m with
      | some _ => exitM σ
      | none => pure σ
    mSteps sigs Γ σ rest

/-- A `mut` or `out` argument's place: its variable holds a value where a
part of it is reached, or where `need` asks, for a `mut` one, and is not
moved, for an `out` one too, as the Rust checks of a bare name. -/
def mPlace (sigs : List (String × Sig)) (Γ : Ctx) : MSt → Expr → Bool → MoveM MSt
  | σ, .var x, need =>
    if need then readM σ x
    else match σ.find x with
      | some v => if !v.live then throw "O001" else pure σ
      | none => pure σ
  | σ, .place x steps, _ => readM σ x >>= fun σ => mSteps sigs Γ σ steps
  | σ, .hinted _ a, need => mPlace sigs Γ σ a need
  | σ, .proj base _, _ => mPlace sigs Γ σ base true
  | σ, .index base k (some _), _ => do
    let σ ← mPlace sigs Γ σ base true
    let σ ← mE sigs Γ true σ k
    exitM σ
  | σ, _, _ => pure σ

/-- A call's arguments, in order, each as its parameter takes it: one
passed `mut` or `out` a place, not a temporary the write would be lost in
(O012), then not walked (O015), then one the caller may change (O004,
O011). -/
def mArgs (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List (Option Mode × Expr) → List Mode → MoveM MSt
  | σ, (_, e) :: args, m :: ms => do
    let σ ← match placeOf e with
      | some (x, _) =>
        if m == .mut || m == .out then
          unwalkedM σ x >>= fun _ => writableM σ x >>= fun _ => mPlace sigs Γ σ e (m == .mut)
        else mE sigs Γ (m != .in_) σ e
      | none => if m == .mut || m == .out then throw "O012" else mE sigs Γ (m != .in_) σ e
    mArgs sigs Γ σ args ms
  | σ, _, _ => pure σ

def mAll (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List Expr → MoveM MSt
  | σ, [] => pure σ
  | σ, e :: es => mE sigs Γ false σ e >>= fun σ => mAll sigs Γ σ es

def mFields (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List (String × Expr) → MoveM MSt
  | σ, [] => pure σ
  | σ, (_, e) :: fs => mE sigs Γ false σ e >>= fun σ => mFields sigs Γ σ fs

def mEntries (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List (Expr × Expr) → MoveM MSt
  | σ, [] => pure σ
  | σ, (k, v) :: es => do
    let σ ← mE sigs Γ false σ k
    let σ ← mE sigs Γ false σ v
    mEntries sigs Γ σ es

def mRows (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List (List Expr) → MoveM MSt
  | σ, [] => pure σ
  | σ, r :: rs => mAll sigs Γ σ r >>= fun σ => mRows sigs Γ σ rs

end

/-- A `set`'s keys, in order: one looked up with `?` or `!` borrowed, and
the one a plain `set m[k] = v` inserts taken, into the map. -/
def mKeys (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List Step → MoveM MSt
  | σ, [] => pure σ
  | σ, .field _ :: rest => mKeys sigs Γ σ rest
  | σ, .index k m :: rest => mE sigs Γ m.isSome σ k >>= fun σ => mKeys sigs Γ σ rest

/-- What a `set` asks of the variable it writes: a compound one reads it,
and one through steps writes a part of it, which has to have been given a
value (O009) and not moved (O001). -/
def mTarget (σ : MSt) (x : String) (steps : List Step) : Option BinOp → MoveM MSt
  | some _ => readM σ x
  | none =>
    if steps.isEmpty then pure σ
    else match σ.find x with
      | some v => if !v.init then throw "O009" else if !v.live then throw "O001" else pure σ
      | none => pure σ

/-- `set x … = e` or `set x … ⊕= e`: not to what a `for` walks (O015),
then the value, then the keys, then the place. -/
def mSet (sigs : List (String × Sig)) (Γ : Ctx) (σ : MSt) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) : MoveM MSt :=
  unwalkedM σ x >>= fun _ =>
  mE sigs Γ op.isSome σ e >>= fun σ =>
  mKeys sigs Γ σ steps >>= fun σ =>
  mTarget σ x steps op >>= fun σ =>
  (if stepsExit steps || op.any BinOp.exits then exitM σ else pure σ) >>= fun σ =>
  pure (if steps.isEmpty then σ.give x else σ)

/-- The context the then branch of `if c |x| … else |y| …` runs in, or
the else branch's if `err`, as `execStmt` runs them at fuel `n`. -/
def condCtx (env : InferEnv) (n : Nat) (Γ : Ctx) (c : Expr) (x : Option String) (err : Bool) : Ctx :=
  match (synthF env n Γ c).run.val with
  | .ok ct => match ct.expose with
    | .option p => if err then Γ else bindOpt Γ x p
    | .result p => if err then bindOpt Γ x .error else bindOpt Γ x p
    | _ => Γ
  | .error _ => Γ

/-- The variants of what a `match` takes apart, as `execStmt` finds them at
fuel `n`. -/
def matchVariants (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Expr) : List (String × Option Ty) :=
  match (synthF env n Γ e).run.val with
  | .ok t => match t.expose with
    | .enum vs => vs.toList
    | _ => []
  | .error _ => []

/-- The context a `match`'s case `term a x` runs in. -/
def termCtx (Γ : Ctx) (vs : List (String × Option Ty)) (a x : String) : Ctx :=
  match vs.lookup a with
  | some (some p) => Γ.bind x p .let_
  | _ => Γ

/-- The context a `for`'s body runs in, as `execStmt` binds it at fuel
`n`. -/
def forCtx (env : InferEnv) (n : Nat) (Γ : Ctx) (names : List String) (coll : Expr) : Ctx :=
  match (forBindings env n Γ names coll).run #[] with
  | (some Δ, _) => { Γ with inLoop := true }.bindAll Δ
  | (none, _) => { Γ with inLoop := true }

/-- Whether each of a `for`'s names is bound by reference, as `execStmt`
binds them at fuel `n`: all but an index. -/
def forRefs (env : InferEnv) (n : Nat) (Γ : Ctx) (names : List String) (coll : Expr) : List Bool :=
  match (forBindings env n Γ names coll).run #[] with
  | (some Δ, _) => Δ.map fun (_, t) => match t with
    | .index => false
    | _ => true
  | (none, _) => names.map fun _ => true

/-- What a statement, a block or a `match`'s cases leave: what holds a value
where it completes, at each of its `break`s, and at each `continue`. -/
abbrev MOut := MSt × List MSt × List MSt

/-- What an `if` leaves, from what its branches leave, back in the scope
before it, and whether each completes, as the Rust's `analyze_if` has it:
where both complete they have to agree about what is moved (O008), and what
is moved after is what the then branch left, unless it alone leaves; a
variable is given a value after only if both branches gave it one. -/
def joinIf (t : MSt) (tc : Bool) (e : MSt) (ec : Bool) : MoveM MSt := do
  if tc && ec && !t.sameMoves e then throw "O008"
  pure ((if !tc && ec then e else t).initFrom (t.initMeet e))

/-- What a `match` leaves, from what each arm leaves, back in the scope
before it, and whether it completes, in order, as the Rust's
`analyze_match` has it: the arms that complete have to agree about what is
moved (O008), what is moved after is what the first of them left, and a
variable is given a value only if every one of them gave it one. With none
completing, what is moved is the first arm's and what is given a value the
last's, and with no arms, what it was entered with. -/
def joinMatch (σ : MSt) (arms : List (MSt × Bool)) : MoveM MSt := do
  let merging := (arms.filter (·.2)).map (·.1)
  match merging with
  | m :: ms =>
    if !ms.all m.sameMoves then throw "O008"
    pure (ms.foldl MSt.initMeet m)
  | [] => match arms.head?, arms.getLast? with
    | some (f, _), some (l, _) => pure (f.initFrom l)
    | _, _ => pure σ

/-- A loop's ends, `σ` what it was entered with and `σc` what its condition
left: each way back to its start, which reads the condition again, keeps
unmoved what was unmoved before the condition (O007), and its ways out
agree (O014), running out of the condition or of the elements leaving what
the condition left the first time. -/
def loopEnd (σ σc : MSt) (natural : Bool) (body : List Stmt) (r : MOut) : MoveM MSt := do
  let again := (if completes body then [σc.back r.1] else []) ++ r.2.2.map σc.back
  if !again.all σ.keepsLive then throw "O007"
  joinAll "O014" σc ((if natural then [σc] else []) ++ r.2.1.map σc.back)

mutual

/-- A statement, as `execStmt` runs it at fuel `n` in context `Γ`, its
blocks at the fuel and in the contexts it runs them in. Out of fuel, the
run stops, and nothing is asked. -/
def mStmt (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → MSt → Stmt → MoveM MOut
  | 0, _, σ, _ => pure (σ, [], [])
  | n + 1, Γ, σ, s => match s with
    | .let_ p _ e => do let σ ← mE sigs Γ false σ e; pure (σ.bind (patNames p) true false, [], [])
    | .var_ p _ (some e) => do let σ ← mE sigs Γ false σ e; pure (σ.bind (patNames p) true false true, [], [])
    | .var_ p (some _) none => pure (σ.bind (patNames p) false false true, [], [])
    | .var_ _ none none => pure (σ, [], [])
    | .set_ x steps op e => do pure (← mSet sigs Γ σ x steps op e, [], [])
    | .fun_ _ => pure (σ, [], [])
    | .ret e => do
      let σ ← exitM σ
      match e with
      | some e => pure (← mE sigs Γ false σ e, [], [])
      | none => pure (σ, [], [])
    | .if_ c tB eB t e =>
      mE sigs Γ false σ c >>= fun σc =>
      mBlock sigs env n (condCtx env n Γ c tB false) (σc.bind tB.toList true false) t >>= fun rt =>
      mElse sigs env n (condCtx env n Γ c eB true) (σc.bind eB.toList true false) e >>= fun re =>
      joinIf (σc.back rt.1) (completes t) (σc.back re.1) (e.all completes) >>= fun σ' =>
      pure (σ', rt.2.1 ++ re.2.1, rt.2.2 ++ re.2.2)
    | .loop c body =>
      (match c with
        | some c => mE sigs Γ false σ c
        | none => pure σ) >>= fun σc =>
      mBlock sigs env n { Γ with inLoop := true } σc body >>= fun r =>
      loopEnd σ σc c.isSome body r >>= fun σ' =>
      pure (σ', [], [])
    | .brk => pure (σ, [σ], [])
    | .cont => pure (σ, [], [σ])
    | .debuglog e => do pure (← mE sigs Γ true σ e, [], [])
    | .call e => do pure (← mE sigs Γ false σ e, [], [])
    | .const_ x _ e => do
      let σ ← mE sigs { Γ with constExpr := true } false σ e
      pure (σ.bind [x] true false, [], [])
    | .type_ _ _ => pure (σ, [], [])
    | .match_ e cases dflt =>
      mE sigs Γ false σ e >>= fun σc =>
      mCases sigs env n Γ (matchVariants env n Γ e) σc cases >>= fun r =>
      mDflt sigs env n Γ σc dflt >>= fun d =>
      joinMatch σc (r.1 ++ d.1) >>= fun σ' =>
      pure (σ', r.2.1 ++ d.2.1, r.2.2 ++ d.2.2)
    | .for_ names coll body =>
      mE sigs Γ true σ coll >>= fun σc =>
      -- What the walk freezes, found before the loop's names shadow it. The
      -- loop is left with what `σc` held, so no longer walked.
      let σw := match placeOf coll with
        | some (x, _) => σc.walk x
        | none => σc
      let σb := (names.zip (forRefs env n Γ names coll)).foldl
        (fun σ (x, r) => σ.bind [x] true true (byRef := r)) σw
      mBlock sigs env n (forCtx env n Γ names coll) σb body >>= fun r =>
      loopEnd σc σc true body r >>= fun σ' =>
      pure (σ', [], [])

/-- An `else`, in its own scope, or nothing, as `execElse` runs it. -/
def mElse (sigs : List (String × Sig)) (env : InferEnv) (n : Nat) (Γ : Ctx) : MSt → Option (List Stmt) → MoveM MOut
  | σ, some e => mBlock sigs env n Γ σ e
  | σ, none => pure (σ, [], [])

/-- A `match`'s default, as `execElse` runs it: what it leaves back in
`σ`'s scope and whether it completes, and its `break`s and `continue`s. -/
def mDflt (sigs : List (String × Sig)) (env : InferEnv) (n : Nat) (Γ : Ctx) : MSt → Option (List Stmt) →
    MoveM (List (MSt × Bool) × List MSt × List MSt)
  | σ, some d => mBlock sigs env n Γ σ d >>= fun r => pure ([(σ.back r.1, completes d)], r.2.1, r.2.2)
  | _, none => pure ([], [], [])

/-- A `match`'s cases, each from `σ`, as `execCases` runs them: each at one
less fuel than the one before. -/
def mCases (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → List (String × Option Ty) → MSt →
    List Case → MoveM (List (MSt × Bool) × List MSt × List MSt)
  | 0, _, _, _, _ => pure ([], [], [])
  | _ + 1, _, _, _, [] => pure ([], [], [])
  | n + 1, Γ, vs, σ, .atom _ b :: cs =>
    mBlock sigs env n Γ σ b >>= fun rb =>
    mCases sigs env n Γ vs σ cs >>= fun r =>
    pure ((σ.back rb.1, completes b) :: r.1, rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2)
  | n + 1, Γ, vs, σ, .term a x b :: cs =>
    mBlock sigs env n (termCtx Γ vs a x) (σ.bind [x] true false) b >>= fun rb =>
    mCases sigs env n Γ vs σ cs >>= fun r =>
    pure ((σ.back rb.1, completes b) :: r.1, rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2)

/-- A block's statements in order, as `execBlock` runs them: each at one
less fuel, in the context the one before it leaves (`checkStmtF` at that
fuel). -/
def mBlock (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → MSt → List Stmt → MoveM MOut
  | 0, _, σ, _ => pure (σ, [], [])
  | _ + 1, _, σ, [] => pure (σ, [], [])
  | n + 1, Γ, σ, s :: ss =>
    mStmt sigs env n Γ σ s >>= fun r1 =>
    mBlock sigs env n ((checkStmtF env n Γ s).run #[]).1 r1.1 ss >>= fun r2 =>
    pure (r2.1, r1.2.1 ++ r2.2.1, r1.2.2 ++ r2.2.2)

end

/-- A function's body, in the context `runCall` runs it in (`funCtx`, with
no type arguments) at fuel `fuel`: its parameters, the last innermost, each
given a value but an `out` one, a `ref`, `mut` or `out` one borrowed, then
the consts it sees; its end a way out where it can complete. -/
def mFun (sigs : List (String × Sig)) (env : InferEnv) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (cmarks : List (String × Bool)) : FunDecl → MoveM Unit
  | .mk name tps params _ body =>
    match sigs.lookup name with
    | none => pure ()
    | some sig => do
      let σ0 : MSt := params.reverse.map (fun (m, x, _) =>
          { name := x, init := m != .out, live := true, back := m == .out || m == .mut,
            lent := m == .ref || m == .mut || m == .out, writable := m == .mut || m == .out,
            byRef := m == .ref }) ++
        cmarks.map fun (x, s) =>
          { name := x, init := s, live := true, back := false, lent := false, writable := false,
            byRef := false }
      let (σ, _, _) ← mBlock sigs env fuel (funCtx sigs aliases consts tps params sig) σ0 body
      if completes body then discard (exitM σ)

/-- A script's statements, as `runScriptMoves` runs them: each at fuel
`fuel`, the context threaded by `checkStmt`, and each function's body as a
call runs it. -/
def mTop (sigs : List (String × Sig)) (env : InferEnv) : Ctx → MSt → List Stmt → MoveM Unit
  | _, _, [] => pure ()
  | Γ, σ, .fun_ f :: ss =>
    mFun sigs env Γ.aliases (constsOf Γ.vars) ((constsOf Γ.vars).map (·.1, true)) f >>= fun _ =>
      mTop sigs env Γ σ ss
  | Γ, σ, s :: ss =>
    mStmt sigs env fuel Γ σ s >>= fun r => mTop sigs env ((checkStmt Γ s).run #[]).1 r.1 ss

/-- What a script's move analysis refuses first, if anything, following
the run that moves (`runScriptMoves`). -/
def moveScript (ss : List Stmt) : Option Lit.Code :=
  let sigs := (resolveNames ss).1
  match mTop sigs movesEnv { ret := .ty (.result unitTy), sigs } [] ss with
  | .ok _ => none
  | .error c => some c

end Datalove.Fun
