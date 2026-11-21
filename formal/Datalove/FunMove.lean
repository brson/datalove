import Datalove.FunDrop

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
The walk keeps the drop schedule as it goes, as the Rust's analysis does:
each statement numbered, each binding given an identity, and each point a
scope is left by scheduling what is owned there; `FunDrop.lean` says how.
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
  /-- Dropped where it is owned: not a copy, not borrowed, not a const. -/
  owns : Bool := false
  /-- Which binding this is, as the Rust's `BindingId` tells two of one
  name apart. -/
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

/-- The drop schedule, as the move analysis builds it: the next statement's
index, the next binding's, the bindings a loop turned out to give a value
they were entered without (the Rust's `maybe_held`), each binding's name, and
the points scheduled so far, each a kind of point, its statement, the arm for
a `match`'s, and what it drops, each with the expression that moves it where
a return may come before the move. -/
structure DSt where
  next : Nat := 0
  uid : Nat := 0
  held : List Nat := []
  names : List (Nat × String) := []
  out : List (String × Nat × Option Nat × List (Nat × Option Nat)) := []
  /-- The index in the body's expression arena of the next expression to be
  filed (`Expr.nodes`). -/
  expr : Nat := 0
  /-- Where each binding was last moved: the index of the expression that
  took its value. -/
  movedAt : List (Nat × Nat) := []
  /-- The index the next statement had when each binding was made: every
  statement from there on may see it. -/
  madeAt : List (Nat × Nat) := []
  /-- The bindings that keep a tracking byte, as the Rust's `stays_tracked`
  has them: a `var` declared with no value, and one perhaps held. A drop of
  one reads the byte, and drops nothing where it holds nothing. -/
  tracked : List Nat := []

/-- The move analysis, keeping the drop schedule as it goes. -/
abbrev MoveM := StateT DSt (Except Lit.Code)

/-- The next statement's index, as the Rust's `alloc_stmt_id` gives it. -/
def stmtId : MoveM Nat := modifyGet fun s => (s.next, { s with next := s.next + 1 })

/-- Bind `x`, a binding of its own, owned where its type in `Γ'`, the
context the statement leaves, is not copy and it is neither borrowed nor a
const. -/
def bindOne (Γ' : Ctx) (konst : Bool) (init lent writable byRef : Bool) (σ : MSt) (x : String) :
    MoveM MSt := do
  let s ← get
  let owns := !lent && !konst && match Γ'.lookup x with
    | some (t, _) => !t.isCopy
    | none => false
  set { s with uid := s.uid + 1, names := (s.uid, x) :: s.names, madeAt := (s.uid, s.next) :: s.madeAt }
  pure ({ name := x, init, live := true, back := false, lent, writable, byRef, owns, uid := s.uid } :: σ)

/-- Names bound in order, each innermost of those before it. -/
def bindM (Γ' : Ctx) (σ : MSt) (xs : List String) (init : Bool) (writable : Bool := false)
    (konst : Bool := false) : MoveM MSt :=
  xs.foldlM (bindOne Γ' konst init false writable false) σ

/-- What of `vs` is dropped where it is: owned, and live or perhaps holding a
value (the Rust's `owned_here`). -/
def ownedIn (vs : MSt) : MoveM (List Nat) := do
  let held := (← get).held
  pure ((vs.filter fun v => v.owns && (v.live || held.contains v.uid)).map (·.uid))

/-- Schedule `drops` at a point, where `always`, or where it drops anything. -/
def record (kind : String) (stmt : Nat) (arm : Option Nat) (drops : List Nat) (always := false) : MoveM Unit :=
  if always || !drops.isEmpty then
    modify fun s => { s with out := (kind, stmt, arm, drops.map (·, none)) :: s.out }
  else pure ()

/-- Schedule, for an early return from statement `stmt` before their moves,
the bindings `moved`, each with the expression that moved it. -/
def recordNotYetMoved (stmt : Nat) (moved : List Nat) : MoveM Unit := do
  let s ← get
  if !moved.isEmpty then
    set { s with out := ("try_return_not_yet_moved", stmt, none,
      moved.map fun u => (u, s.movedAt.lookup u)) :: s.out }

/-- Where the expressions filed next begin. -/
def exprAt : MoveM Nat := do pure (← get).expr

/-- Go on filing expressions from `start`. -/
def exprFrom (start : Nat) : MoveM Unit := modify fun s => { s with expr := start }

/-- The bindings a scope made, of `τ`, its state where it ends, entered at
`σ`. -/
def scopeOwn (σ τ : MSt) : MSt := List.take (τ.length - σ.length) τ

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
      | some v =>
        if v.walked then throw "O015" else if v.lent then throw "O003" else do
          modify fun s => { s with movedAt := (v.uid, s.expr) :: s.movedAt }
          pure (σ.take x)
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
def mE (sigs : List (String × Sig)) (Γ : Ctx) (b : Bool) (σ : MSt) (e : Expr) : MoveM MSt :=
  exprAt >>= fun start =>
  (match e with
    | .var x => if b then readM σ x else moveM Γ σ x
    | .place x steps => readM σ x >>= fun σ => mSteps sigs Γ σ steps
    | .hinted _ a => mE sigs Γ (b && !buildsFromParts a) σ a
    | .adapt a => mE sigs Γ true σ a
    | .some_ a | .ok a | .er a | .data a | .error a | .term _ a =>
      mE sigs Γ false σ a
    | .enum a => mE sigs Γ b σ a
    | .binop op a c => do
      let σ ← mE sigs Γ true σ a
      let σ ← mE sigs Γ true σ c
      if op.exits then exitM σ else pure σ
    | .unop op a => do
      let σ ← mE sigs Γ true σ a
      match op with
      | .negC | .negO => exitM σ
      | _ => pure σ
    | .tryOpt a => mE sigs Γ b σ a >>= exitM
    | .tryRes a => mE sigs Γ b σ a >>= exitM
    | .proj base _ => mE sigs Γ (b || (placeOf base).isSome) σ base
    | .index base k (some _) => do
      let σ ← mE sigs Γ (b || (placeOf base).isSome) σ base
      let σ ← mE sigs Γ true σ k
      exitM σ
    | .index _ _ none => pure σ
    | .call f args => match sigs.lookup f with
      | some sig =>
        let ms := sig.params.map (·.1)
        mArgs sigs Γ σ args ms >>= fun σ =>
        (if argsExit args ms then exitM σ else pure σ) >>= fun σ =>
        pure (giveArgsM σ args ms)
      | none => throw "F089"
    | .tuple es | .list es | .set es | .tensor _ es => mAll sigs Γ σ es
    | .struct fs => mFields sigs Γ σ fs
    | .map es => mEntries sigs Γ σ es
    | .table _ rows => mRows sigs Γ σ rows
    | .none_ | .tt | .ff | .str _ | .int _ | .float | .hex _ | .atom _ => pure σ)
    >>= fun σ' => exprFrom (start + e.nodes) >>= fun _ => pure σ'

/-- A place's steps, each key in order, an index with `?` or `!` a way out. -/
def mSteps (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List Step → MoveM MSt
  | σ, [] => pure σ
  | σ, .field _ :: rest => mSteps sigs Γ σ rest
  | σ, .index k m :: rest =>
    mE sigs Γ true σ k >>= fun σ =>
    (match m with
      | some _ => exitM σ
      | none => pure σ) >>= fun σ =>
    mSteps sigs Γ σ rest

/-- A `mut` or `out` argument's place: its variable holds a value where a
part of it is reached, or where `need` asks, for a `mut` one, and is not
moved, for an `out` one too, as the Rust checks of a bare name. -/
def mPlace (sigs : List (String × Sig)) (Γ : Ctx) (σ : MSt) (e : Expr) (need : Bool) : MoveM MSt :=
  exprAt >>= fun start =>
  (match e, need with
    | .var x, need =>
      if need then readM σ x
      else match σ.find x with
        | some v => if !v.live then throw "O001" else pure σ
        | none => pure σ
    | .place x steps, _ => readM σ x >>= fun σ => mSteps sigs Γ σ steps
    | .hinted _ a, need => mPlace sigs Γ σ a need
    | .proj base _, _ => mPlace sigs Γ σ base true
    | .index base k (some _), _ => do
      let σ ← mPlace sigs Γ σ base true
      let σ ← mE sigs Γ true σ k
      exitM σ
    | _, _ => pure σ)
    >>= fun σ' => exprFrom (start + e.nodes) >>= fun _ => pure σ'

/-- A call's arguments, in order, each as its parameter takes it: one
passed `mut` or `out` a place, not a temporary the write would be lost in
(O012), then not walked (O015), then one the caller may change (O004,
O011). -/
def mArgs (sigs : List (String × Sig)) (Γ : Ctx) : MSt → List (Option Mode × Expr) → List Mode → MoveM MSt
  | σ, (_, e) :: args, m :: ms =>
    (match placeOf e with
      | some (x, _) =>
        if m == .mut || m == .out then
          unwalkedM σ x >>= fun _ => writableM σ x >>= fun _ => mPlace sigs Γ σ e (m == .mut)
        else mE sigs Γ (m != .in_) σ e
      | none => if m == .mut || m == .out then throw "O012" else mE sigs Γ (m != .in_) σ e) >>= fun σ =>
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

/-- A step of statement `id` that may return early where `leaves` says:
then what is owned once it has run is dropped there, and what it moved on
the way, which a return before the move still owns, is scheduled apart. -/
def leavingIf (id : Nat) (σ : MSt) (leaves : Bool) (m : MoveM MSt) : MoveM MSt :=
  if leaves then do
    let before ← ownedIn σ
    let σ' ← m
    let drops ← ownedIn σ'
    recordNotYetMoved id (before.filter (!drops.contains ·))
    record "before_try_return" id none drops (always := true)
    pure σ'
  else m

/-- An expression of statement `id` that the Rust's `analyze_leaving_expr`
reads, which may return early where `exprLeaves` says. -/
def leaving (id : Nat) (σ : MSt) (e : Expr) (m : MoveM MSt) : MoveM MSt :=
  leavingIf id σ (exprLeaves e) m

/-- What a `set` runs before it reaches its place: the value, then the
keys, which are filed in the expression arena before the value. -/
def mSetParts (sigs : List (String × Sig)) (Γ : Ctx) (σ : MSt) (steps : List Step) (op : Option BinOp)
    (e : Expr) : MoveM MSt := do
  let keys ← exprAt
  exprFrom (keys + Expr.nodes.keyNodes steps)
  let σ ← mE sigs Γ op.isSome σ e
  let next ← exprAt
  exprFrom keys
  let σ ← mKeys sigs Γ σ steps
  exprFrom next
  pure σ

/-- `set x … = e` or `set x … ⊕= e` of statement `id`: not to what a `for`
walks (O015), to what may be changed (O004, O011, which typechecking
refuses first), then the value, then the keys, then the place. A return from
the value, a key or a checked or optional operator drops what is owned once
the value and the keys have run, and what they moved on the way apart, for a
return before the move; one from a failed `?` or `!` on the way to the place
what is owned once the keys have run. -/
def mSet (sigs : List (String × Sig)) (Γ : Ctx) (id : Nat) (σ : MSt) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) : MoveM MSt := do
  unwalkedM σ x
  -- What typechecking lets a `set` write, a `var` or a `mut` or `out`
  -- parameter, which the analysis needs to know of what it gives a value.
  writableM σ x
  let keysLeave := steps.any fun | .index k _ => exprLeaves k | _ => false
  let σ ← leavingIf id σ (exprLeaves e || keysLeave || op.any BinOp.exits) (mSetParts sigs Γ σ steps op e)
  let σ ← mTarget σ x steps op
  (if stepsExit steps then do
      record "before_set_target_early_return" id none (← ownedIn σ) (always := true)
    else pure ())
  let σ ← (if stepsExit steps || op.any BinOp.exits then exitM σ else pure σ)
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
def joinIf (t : MSt) (tc : Bool) (e : MSt) (ec : Bool) : MoveM MSt :=
  if tc && ec && !t.sameMoves e then throw "O008"
  else pure ((if !tc && ec then e else t).initFrom (t.initMeet e))

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
    else pure (ms.foldl MSt.initMeet m)
  | [] => match arms.head?, arms.getLast? with
    | some (f, _), some (l, _) => pure (f.initFrom l)
    | _, _ => pure σ

/-- A loop's ends, `σ` what it was entered with and `σc` what its condition
left: each way back to its start, which reads the condition again, keeps
unmoved what was unmoved before the condition (O007), and its ways out
agree (O014), running out of the condition or of the elements leaving what
the condition left the first time. -/
def loopEnd (σ σc : MSt) (natural : Bool) (body : List Stmt) (r : MOut) : MoveM MSt :=
  let again := (if completes body then [σc.back r.1] else []) ++ r.2.2.map σc.back
  if !again.all σ.keepsLive then throw "O007"
  else joinAll "O014" σc ((if natural then [σc] else []) ++ r.2.1.map σc.back)

/-- Binding `u` perhaps held from here on: kept by its tracking byte, and
added to every way out of the function scheduled from where it was made on. -/
def holdFrom (u : Nat) (s : DSt) : DSt :=
  { s with
    held := if s.held.contains u then s.held else u :: s.held,
    tracked := if s.tracked.contains u then s.tracked else u :: s.tracked,
    out := s.out.map fun (k, st, a, ds) =>
      if (k == "before_return" || k == "before_try_return" || k == "before_set_target_early_return")
        && st ≥ (s.madeAt.lookup u).getD 0 && !ds.any (·.1 == u) then (k, st, a, ds ++ [(u, none)])
      else (k, st, a, ds) }

/-- What a loop turns out to perhaps hold, as the Rust's `check_loop_repeats`
finds it: an owned binding moved when the loop is entered (`σc`) and given a
value again at the body's end, where it gets there, or at a `continue`,
found there by its place in scope. Only a `var` is ever given one again. It is held
from then on, and joins each way out of the function scheduled from where it
was made on: a pass after the body gave it a value may leave by one in the
body, and the next pass of a loop around this one by one before this loop.
Its tracking byte says whether there is anything to drop. -/
def loopHeld (σc : MSt) (body : List Stmt) (r : MOut) : MoveM Unit :=
  σc.zipIdx.forM fun (v, i) =>
    let liveIn (τ : MSt) := ((σc.back τ)[i]?).any (·.live)
    if !v.live && v.owns && ((completes body && liveIn r.1) || r.2.2.any liveIn)
    then modify (holdFrom v.uid) else pure ()

mutual

/-- A statement, as `execStmt` runs it at fuel `n` in context `Γ`, its
blocks at the fuel and in the contexts it runs them in, numbered as the
Rust's `alloc_stmt_id` numbers it and scheduling what each point it leaves
a scope by drops. `base` is how many bindings were in scope where the
innermost loop's body began, which a `break` or `continue` leaves back to.
Out of fuel, the run stops, and nothing is asked. -/
def mStmt (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → MSt → Nat → Stmt → MoveM MOut
  | 0, _, σ, _, _ => pure (σ, [], [])
  | n + 1, Γ, σ, base, s => do
    let id ← stmtId
    let after := ((checkStmtF env n Γ s).run #[]).1
    match s with
    | .let_ p _ e =>
      let σ ← leaving id σ e (mE sigs Γ false σ e)
      pure (← bindM after σ (patNames p) true, [], [])
    | .var_ p _ (some e) =>
      let σ ← leaving id σ e (mE sigs Γ false σ e)
      pure (← bindM after σ (patNames p) true (writable := true), [], [])
    | .var_ p (some _) none =>
      let σ' ← bindM after σ (patNames p) false (writable := true)
      modify fun s => { s with tracked := (scopeOwn σ σ').map (·.uid) ++ s.tracked }
      pure (σ', [], [])
    | .var_ _ none none => pure (σ, [], [])
    | .set_ x steps op e => pure (← mSet sigs Γ id σ x steps op e, [], [])
    | .fun_ _ => pure (σ, [], [])
    | .ret e =>
      let σ ← exitM σ
      let σ ← (match e with
        | some e => leaving id σ e (mE sigs Γ false σ e)
        | none => pure σ)
      record "before_return" id none (← ownedIn σ) (always := true)
      pure (σ, [], [])
    | .if_ c tB eB t e =>
      let σc ← leaving id σ c (mE sigs Γ false σ c)
      let Γt := condCtx env n Γ c tB false
      let rt ← mBlock sigs env n Γt (← bindM Γt σc tB.toList true) base t
      record "then_branch_exit" id none (← ownedIn (scopeOwn σc rt.1))
      let re ← (match e with
        | some eb => do
          let Γe := condCtx env n Γ c eB true
          let re ← mBlock sigs env n Γe (← bindM Γe σc eB.toList true) base eb
          record "else_branch_exit" id none (← ownedIn (scopeOwn σc re.1))
          pure re
        | none => pure (σc, [], []))
      let σ' ← joinIf (σc.back rt.1) (completes t) (σc.back re.1) (e.all completes)
      pure (σ', rt.2.1 ++ re.2.1, rt.2.2 ++ re.2.2)
    | .loop c body =>
      let σc ← (match c with
        | some c => leaving id σ c (mE sigs Γ false σ c)
        | none => pure σ)
      let r ← mBlock sigs env n { Γ with inLoop := true } σc σc.length body
      loopHeld σc body r
      record "loop_body_end" id none (← ownedIn (scopeOwn σc r.1))
      let σ' ← loopEnd σ σc c.isSome body r
      pure (σ', [], [])
    | .brk =>
      record "before_break" id none (← ownedIn (List.take (σ.length - base) σ))
      pure (σ, [σ], [])
    | .cont =>
      record "before_continue" id none (← ownedIn (List.take (σ.length - base) σ))
      pure (σ, [], [σ])
    | .debuglog e => pure (← leaving id σ e (mE sigs Γ true σ e), [], [])
    | .call e => pure (← leaving id σ e (mE sigs Γ false σ e), [], [])
    | .const_ x _ e =>
      let σ ← leaving id σ e (mE sigs { Γ with constExpr := true } false σ e)
      pure (← bindM after σ [x] true (konst := true), [], [])
    | .type_ _ _ => pure (σ, [], [])
    | .match_ e cases dflt =>
      let σc ← leaving id σ e (mE sigs Γ false σ e)
      let r ← mCases sigs env n Γ (matchVariants env n Γ e) σc base id 0 cases
      let d ← (match dflt with
        | some db => do
          let rd ← mBlock sigs env n Γ σc base db
          record "match_arm_exit" id (some cases.length) (← ownedIn (scopeOwn σc rd.1))
          pure ([(σc.back rd.1, completes db)], rd.2.1, rd.2.2)
        | none => pure ([], [], []))
      let σ' ← joinMatch σc (r.1 ++ d.1)
      pure (σ', r.2.1 ++ d.2.1, r.2.2 ++ d.2.2)
    | .for_ names coll body =>
      let σc ← leaving id σ coll (mE sigs Γ true σ coll)
      -- What the walk freezes, found before the loop's names shadow it. The
      -- loop is left with what `σc` held, so no longer walked.
      let σw := match placeOf coll with
        | some (x, _) => σc.walk x
        | none => σc
      let Γf := forCtx env n Γ names coll
      let σb ← (names.zip (forRefs env n Γ names coll)).foldlM
        (fun σ (x, r) => bindOne Γf false true true false r σ x) σw
      let r ← mBlock sigs env n Γf σb σc.length body
      loopHeld σc body r
      record "loop_body_end" id none (← ownedIn (scopeOwn σc r.1))
      let σ' ← loopEnd σc σc true body r
      pure (σ', [], [])

/-- A `match`'s cases, each from `σ`, as `execCases` runs them: each at one
less fuel than the one before, the `i`th scheduling what its arm drops. -/
def mCases (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → List (String × Option Ty) → MSt →
    Nat → Nat → Nat → List Case → MoveM (List (MSt × Bool) × List MSt × List MSt)
  | 0, _, _, _, _, _, _, _ => pure ([], [], [])
  | _ + 1, _, _, _, _, _, _, [] => pure ([], [], [])
  | n + 1, Γ, vs, σ, base, id, i, .atom _ b :: cs => do
    let rb ← mBlock sigs env n Γ σ base b
    record "match_arm_exit" id (some i) (← ownedIn (scopeOwn σ rb.1))
    let r ← mCases sigs env n Γ vs σ base id (i + 1) cs
    pure ((σ.back rb.1, completes b) :: r.1, rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2)
  | n + 1, Γ, vs, σ, base, id, i, .term a x b :: cs => do
    let Γa := termCtx Γ vs a x
    let rb ← mBlock sigs env n Γa (← bindM Γa σ [x] true) base b
    record "match_arm_exit" id (some i) (← ownedIn (scopeOwn σ rb.1))
    let r ← mCases sigs env n Γ vs σ base id (i + 1) cs
    pure ((σ.back rb.1, completes b) :: r.1, rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2)

/-- A block's statements in order, as `execBlock` runs them: each at one
less fuel, in the context the one before it leaves (`checkStmtF` at that
fuel). -/
def mBlock (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → MSt → Nat → List Stmt → MoveM MOut
  | 0, _, σ, _, _ => pure (σ, [], [])
  | _ + 1, _, σ, _, [] => pure (σ, [], [])
  | n + 1, Γ, σ, base, s :: ss => do
    let r1 ← mStmt sigs env n Γ σ base s
    let r2 ← mBlock sigs env n ((checkStmtF env n Γ s).run #[]).1 r1.1 base ss
    pure (r2.1, r1.2.1 ++ r2.2.1, r1.2.2 ++ r2.2.2)

end

/-- A function's body, in the context `runCall` runs it in (`funCtx`, with
no type arguments) at fuel `fuel`: its parameters, the last innermost, each
given a value but an `out` one, a `ref`, `mut` or `out` one borrowed, an
`in` one owned if not copy, then the consts it sees; its end a way out
where it can complete, which drops what the function's own scope holds
there. -/
def mFun (sigs : List (String × Sig)) (env : InferEnv) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (cmarks : List (String × Bool)) : FunDecl → MoveM Unit
  | .mk name tps params _ body =>
    match sigs.lookup name with
    | none => pure ()
    | some sig => do
      let Γ := funCtx sigs aliases consts tps params sig
      let consts : MSt := cmarks.map fun (x, s) =>
        { name := x, init := s, live := true, back := false, lent := false, writable := false, byRef := false }
      let σ0 ← params.foldlM (fun σ (m, x, _) => do
          let σ ← bindOne Γ false (m != .out) (m == .ref || m == .mut || m == .out) (m == .mut || m == .out)
            (m == .ref) σ x
          pure (match σ with
            | v :: rest => { v with back := m == .out || m == .mut } :: rest
            | [] => [])) consts
      let (σ, _, _) ← mBlock sigs env fuel Γ σ0 0 body
      (if completes body then discard (exitM σ) else pure ())
      record "at_function_exit" 0 none (← ownedIn (scopeOwn consts σ))

/-- A script's statements, as `runScriptMoves` runs them: each at fuel
`fuel`, the context threaded by `checkStmt`, and each function's body as a
call runs it, with a schedule of its own. -/
def mTop (sigs : List (String × Sig)) (env : InferEnv) : Ctx → MSt → List Stmt → MoveM Unit
  | _, _, [] => pure ()
  | Γ, σ, .fun_ f :: ss => do
    StateT.lift ((mFun sigs env Γ.aliases (constsOf Γ.vars) ((constsOf Γ.vars).map (·.1, true)) f).run' {})
    mTop sigs env Γ σ ss
  | Γ, σ, s :: ss => do
    let r ← mStmt sigs env fuel Γ σ 0 s
    mTop sigs env ((checkStmt Γ s).run #[]).1 r.1 ss

/-- What a script's move analysis refuses first, if anything, following
the run that moves (`runScriptMoves`). -/
def moveScript (ss : List Stmt) : Option Lit.Code :=
  let sigs := (resolveNames ss).1
  match (mTop sigs movesEnv { ret := .ty (.result unitTy), sigs } [] ss).run' {} with
  | .ok _ => none
  | .error c => some c

/-- A script's top-level functions' drop schedules, by name, each as
`mTop` reaches the function: in the context the statements before it leave;
each with the names of the bindings that keep a tracking byte. None for a
function the move analysis refuses. -/
def scriptDrops (ss : List Stmt) : List (String × Option (List DropPoint × List String)) :=
  let sigs := (resolveNames ss).1
  let rec go (Γ : Ctx) : List Stmt → List (String × Option (List DropPoint × List String))
    | [] => []
    | .fun_ f :: rest =>
      let name := match f with | .mk n .. => n
      let r := (mFun sigs movesEnv Γ.aliases (constsOf Γ.vars) ((constsOf Γ.vars).map (·.1, true)) f).run {}
      let points := match r with
        | .ok (_, s) =>
          let nameOf (u : Nat) := (s.names.lookup u).getD "?"
          some (s.out.reverse.map (fun (k, st, a, ds) =>
            (k, st, a, ds.map fun
              | (u, some e) => s!"{nameOf u}@{e}"
              | (u, none) => nameOf u)), s.tracked.map nameOf)
        | .error _ => none
      (name, points) :: go Γ rest
    | s :: rest => go ((checkStmt Γ s).run #[]).1 rest
  go { ret := .ty (.result unitTy), sigs } ss

end Datalove.Fun
