import Datalove.FunAlias

/-!
# Operands an expression still reads by reference (O017)

The model reads every operand where it is written, left to right
(`Fun.evalS`). The compiled code reads some of them later, through a
pointer, once the rest of the expression has run: an argument passed `ref`,
`mut` or `out`, until its call is made; an operator's operand that is not a
copy, while the other runs; an index's base, while its key runs; the place a
step reaches, while its keys run; a `set`'s key looked up with `?` or `!`
that is not a copy, while the later keys run; and a compound `set`'s value
that is not a copy, while the keys run. The two read the same where nothing
run in between writes what such an operand reads. O017 sees to that: taking
a held place `mut` or `out`, or moving it, is refused. This is O017 as the
ownership analysis decides it (`analyze_expr_moves` and `analyze_set` in
`datalove-datafun-ownership`), as botspec's section on operands across an
expression says.

The walk follows the analysis case for case, keeping what is held as it
goes, innermost first, and gives every write it meets paired with each place
held over it (`Clash`): an argument a `mut` or `out` parameter takes, and a
bare variable moved, against everything held but the arguments of its own
call, which O010 compares (`against`). A place is told apart as O010 tells
it, by its variable and its fields up to the first index (`placeKey`), and
O017 is a clash of places that overlap (`overlaps`).

`Proofs/HeldSound.lean` shows that where O017 accepts, a held operand read
late reads what it read where it was written.
-/

namespace Datalove.Fun

/-- A place an earlier operand still reads through a reference, by its key,
with the depth of the call it is an argument of, if it is one. -/
abbrev Held := (String × List Sel) × Option Nat

/-- A write met while a place was held: the place held, and the place taken
`mut` or `out`, or moved. -/
structure Clash where
  held : String × List Sel
  taken : String × List Sel

/-- A place `p` written by an argument of the call at depth `c`, if it is
one, against what is held: everything but the arguments of that same call. -/
def against (hs : List Held) (c : Option Nat) (p : String × List Sel) : List Clash :=
  (hs.filter fun (_, d) => d.isNone || d != c).map fun (q, _) => ⟨q, p⟩

/-- `hs` with `k` held on top, if there is one. -/
def hold (k : Option (String × List Sel)) (hs : List Held) : List Held :=
  match k with
  | some k => (k, none) :: hs
  | none => hs

/-- Whether an expression is of a copy type, as the checker types it in `Γ`. -/
def copyOf (Γ : Ctx) (e : Expr) : Bool :=
  match (synthF inferEnv fuel Γ e).run.val with
  | .ok t => t.isCopy
  | .error _ => false

/-- What an operand an operator borrows reads through: a place, or the place
an index's base, a `?` or `!`, or a hint reads through. -/
def borrowedKey (e : Expr) : Option (String × List Sel) :=
  (placeKey e).or (match e with
    | .hinted _ a | .tryOpt a | .tryRes a => borrowedKey a
    | .index b _ _ => borrowedKey b
    | _ => none)

/-- The modes of a function's parameters, as its signature in `Γ` gives them. -/
def paramModes (Γ : Ctx) (f : String) : List Mode :=
  match Γ.sigs.lookup f with
  | some sig => sig.params.map (·.1)
  | none => []

mutual

/-- The writes `e` makes over what is held, analyzed as the ownership
analysis does with `hs` held, `depth` calls deep, the call a bare variable
being analyzed is an argument of `direct`, and the value taken or only read
(`consumed`). A place with steps is reached through a reference to it once
its keys have run, so it is held while they do; a bare one taken is moved. -/
def clashes (Γ : Ctx) (hs : List Held) (depth : Nat) (direct : Option Nat) (consumed : Bool) :
    Expr → List Clash
  | .var x => if consumed && !copyOf Γ (.var x) then against hs direct (x, []) else []
  | .place x steps => stepClashes Γ (((x, fieldsBefore steps), none) :: hs) depth direct steps
  | .hinted _ a => clashes Γ hs depth direct consumed a
  | .proj b _ => clashes Γ hs depth direct false b
  | .index b k _ =>
    clashes Γ hs depth direct false b ++ clashes Γ (hold (borrowedKey b) hs) depth direct false k
  | .binop _ a b =>
    clashes Γ hs depth direct false a ++
      clashes Γ (hold (if copyOf Γ a then none else borrowedKey a) hs) depth direct false b
  | .unop _ a | .adapt a => clashes Γ hs depth direct false a
  | .tryOpt a | .tryRes a | .some_ a | .ok a | .er a | .data a | .error a | .term _ a =>
    clashes Γ hs depth direct true a
  | .enum a => clashes Γ hs depth direct consumed a
  | .list es | .set es | .tuple es | .tensor _ es => allClashes Γ hs depth direct es
  | .struct fs => fieldClashes Γ hs depth direct fs
  | .map es => entryClashes Γ hs depth direct es
  | .table _ rows => rowClashes Γ hs depth direct rows
  | .call f args => argClashes Γ depth hs (paramModes Γ f) args
  | _ => []

/-- The keys of a place's steps, each only read, with the place held. -/
def stepClashes (Γ : Ctx) (hs : List Held) (depth : Nat) (direct : Option Nat) : List Step → List Clash
  | [] => []
  | .field _ :: ss => stepClashes Γ hs depth direct ss
  | .index k _ :: ss => clashes Γ hs depth direct false k ++ stepClashes Γ hs depth direct ss

/-- Elements, each taken. -/
def allClashes (Γ : Ctx) (hs : List Held) (depth : Nat) (direct : Option Nat) : List Expr → List Clash
  | [] => []
  | e :: es => clashes Γ hs depth direct true e ++ allClashes Γ hs depth direct es

def fieldClashes (Γ : Ctx) (hs : List Held) (depth : Nat) (direct : Option Nat) :
    List (String × Expr) → List Clash
  | [] => []
  | (_, e) :: fs => clashes Γ hs depth direct true e ++ fieldClashes Γ hs depth direct fs

def entryClashes (Γ : Ctx) (hs : List Held) (depth : Nat) (direct : Option Nat) :
    List (Expr × Expr) → List Clash
  | [] => []
  | (k, v) :: es =>
    clashes Γ hs depth direct true k ++ clashes Γ hs depth direct true v ++ entryClashes Γ hs depth direct es

def rowClashes (Γ : Ctx) (hs : List Held) (depth : Nat) (direct : Option Nat) :
    List (List Expr) → List Clash
  | [] => []
  | r :: rs => allClashes Γ hs depth direct r ++ rowClashes Γ hs depth direct rs

/-- A call's arguments at depth `depth`, each with its parameter's mode: one
passed `ref`, `mut` or `out` is held until the call, one passed `mut` or
`out` is a write, one passed `out` is not read, and one passed `in` is
taken. A const argument is not passed, and nothing of it is analyzed. Calls
inside an argument are one deeper. -/
def argClashes (Γ : Ctx) (depth : Nat) (hs : List Held) : List Mode → List (Option Mode × Expr) → List Clash
  | p :: ps, (m, e) :: args =>
    if p == .const_ then argClashes Γ depth hs ps args
    else
      let mode := m.getD .in_
      let byRef := mode == .ref || mode == .mut || mode == .out
      let written := mode == .mut || mode == .out
      let hs' := match placeKey e with
        | some k => if byRef then (k, some depth) :: hs else hs
        | none => hs
      let write := match placeKey e with
        | some k => if written then against hs' (some depth) k else []
        | none => []
      let read := if mode == .out then []
        else clashes Γ hs' (depth + 1) (match placeOf e with
          | some (_, []) => some depth
          | _ => none) (mode == .in_) e
      write ++ read ++ argClashes Γ depth hs' ps args
  | _, _ => []

end

/-- The keys of a `set`'s target, left to right: one with no `?` or `!`
taken, into the map it is inserted in; one looked up that is not a copy held
while the later keys run. -/
def setKeyClashes (Γ : Ctx) (hs : List Held) : List Step → List Clash
  | [] => []
  | .field _ :: ss => setKeyClashes Γ hs ss
  | .index k m :: ss =>
    clashes Γ hs 0 none m.isNone k ++
      setKeyClashes Γ (hold (if m.isSome && !copyOf Γ k then borrowedKey k else none) hs) ss

/-- The writes a `set` makes over what is held: its value first, taken by a
plain `set` and read by a compound one, which holds it while the keys run if
it is not a copy; then the keys. The place is reached only after. -/
def setClashes (Γ : Ctx) (steps : List Step) (op : Option BinOp) (e : Expr) : List Clash :=
  clashes Γ [] 0 none op.isNone e ++
    setKeyClashes Γ (hold (if op.isSome && !copyOf Γ e then borrowedKey e else none) []) steps

/-- Whether writes are O017: some place is taken while a place it overlaps is
held. -/
def heldTaken (cs : List Clash) : Bool := cs.any fun c => overlaps c.held c.taken

/-- For each top-level `debuglog`, `call` and `set` of a script, in order,
whether it is O017, each in the context the checker reaches it in. A
`debuglog` only reads its value, and a `call` takes it. -/
def heldTop : Ctx → List Stmt → List Bool
  | _, [] => []
  | Γ, s :: ss =>
    let here := match s with
      | .debuglog e => [heldTaken (clashes Γ [] 0 none false e)]
      | .call e => [heldTaken (clashes Γ [] 0 none true e)]
      | .set_ _ steps op e => [heldTaken (setClashes Γ steps op e)]
      | _ => []
    here ++ heldTop (if s matches .fun_ _ then Γ else ((checkStmt Γ s).run #[]).1) ss

/-- For each top-level `debuglog`, `call` and `set` of a script, whether it is
O017. -/
def heldScript (ss : List Stmt) : List Bool :=
  let sigs := (resolveNames ss).1
  heldTop { ret := .ty (.result unitTy), sigs } ss

end Datalove.Fun
