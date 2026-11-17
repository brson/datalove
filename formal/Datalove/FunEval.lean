import Datalove.Fun
import Datalove.Proofs.LitSound

/-!
# Evaluating datafun's expressions

What a datafun expression evaluates to, for `Proofs/EvalSound.lean` to show
that an expression that checks evaluates to a value of its type. Values are
datalit's (`Lit.Val`), so a type's values are what `Lit.HasType` says, as
for literals.

What an operator does depends on its operands' type, not only on their
values: `a +! b` overflows at `u8`'s bounds or at `i8`'s, and a value does
not say which. So evaluation is directed by the types the checker gives,
as a compiled program carries them. It follows the checker's functions one
for one, at the same fuel, and asks the checker for a type where an
operation needs one. The theorem it serves is then that the checker's
types are kept at run time: a value is in its type's range, an early return
is a value of the function's return type, and no operation meets an operand
of the wrong kind (`Stop.wrong`).

It evaluates literals, variables, places, hints, operators, `?`, `!`, `@`,
fields, indexes and calls, reading the variables in scope as it goes
(`EvalT`). A call goes to a `CallFn` the evaluator is given (`FunCall.lean`
runs the function's body), and writes back to its `mut` and `out` arguments'
variables, so a variable read after it in the same expression reads what it
came to. A `mut` or `out` argument into a field or element of a variable
evaluates its keys with the arguments (`evalPath`), as the Rust takes its
pointer then, and the parameter's value is written back through the same
steps with those keys' values (`writeBack`, `writeSteps`). A hinted one
is written back to the place under the hint (`unhint`). One with type
arguments that are not ground is `Stop.unsupported`. `@`
converts between types whose values are the same: a wider number, an atom or
term into an enum, a clone. A comparison, and whether two map keys are one,
are answered where the values' contents are modelled (`valEq?`): numbers,
booleans, strings, by their text, and what is built of them. A float,
`data` or `error` carries nothing here, so comparing one, or looking one up
as a map's key, is `Stop.unsupported`.
-/

namespace Datalove.Fun

open Lit (Val)

/-- Why evaluation gave no value. -/
inductive Stop where
  /-- The function returns this value early: `?`, `!`, a checked or optional
  operator that failed. -/
  | early (r : Val)
  | fuel
  /-- An operation met an operand of the wrong kind, or a form a checked
  expression never has. -/
  | wrong
  /-- A form not evaluated yet: a call that writes back or has a recursive
  type argument, or a `set` through a tensor's
  slice. -/
  | unsupported
  /-- A variable read before it was set, which the ownership analysis refuses
  (O005), not the checker. -/
  | uninit
  /-- `ret` with no value. -/
  | retVoid
  /-- `break` and `continue`, with the variables as they were. -/
  | brk (ρ : List (String × Option Val))
  | cont (ρ : List (String × Option Val))
  /-- A loop that ran as many times as it was given, or calls nested as
  deep. -/
  | diverge
  /-- A function's body returned, with a value or none, the variables as
  they were: what an early return or `ret` from a statement is once the
  block it is in has seen it, so that a call can write back what its
  `mut` and `out` parameters came to. -/
  | returned (r : Option Val) (ρ : List (String × Option Val))

abbrev EvalM := Except Stop

/-- The variables in scope, innermost first, each with its value, or none
before it is set. -/
abbrev VEnv := List (String × Option Val)

/-- What calls do: the function's name, its type arguments, and its
arguments' values, none for an `out` argument, to what it returns and what
each parameter came to, which a `mut` or `out` argument is written back
from. `FunCall.lean` builds the real one, running the function's body, from
a program. -/
abbrev CallFn := String → List (String × Ty) → List (Option Val) → EvalM (Val × List (Option Val))

/-- Type arguments with no type parameter and no `rec` in them, which the
check-once theorem takes. -/
def groundArgs (θ : List (String × Ty)) : Bool :=
  θ.all fun (_, t) => !t.containsVar && !t.containsRec


/-- An integer value, decimal or hex. -/
def numOf : Val → Option Int
  | .int n => some n
  | .hex n => some n
  | _ => none

/-- A float value: a float literal, or a hex literal's bits. -/
def floatVal : Val → Bool
  | .float | .hex _ => true
  | _ => false

/-- Whether an integer is a value of a type, as `Lit.inRange`. -/
def inRangeB (t : Ty) (r : Int) : Bool :=
  match Lit.intRange t with
  | some (lo, hi, _) => lo ≤ r && r ≤ hi
  | none => t == .int

/-- A checked operator's failure returns an error, an optional one's
nothing. -/
def failOf : Needs → Stop
  | .result => .early (.er .error)
  | _ => .early .none

/-- An integer result of an operator that can fail: the value, if it is one
of the type, or the failure. -/
def fallible (op : BinOp) (t : Ty) (r : Int) : EvalM Val :=
  if inRangeB t r then pure (.int r) else throw (failOf (needs op))

mutual

/-- Whether two values are equal, where their contents are modelled: none
for a float, `data`, `error`, a map, a set, a tensor or a table, or
what holds one. Values compared are of one type. -/
partial def valEq? : Val → Val → Option Bool
  | .bool a, .bool b => some (a == b)
  | .str a, .str b => some (a == b)
  | .atom a, .atom b => some (a == b)
  | .term a v, .term b w => if a != b then some false else valEq? v w
  | .none, .none => some true
  | .some v, .some w => valEq? v w
  | .none, .some _ | .some _, .none => some false
  | .ok v, .ok w | .er v, .er w => valEq? v w
  | .ok _, .er _ | .er _, .ok _ => some false
  | .tuple vs, .tuple ws => valsEq? vs ws
  | .list vs, .list ws => valsEq? vs ws
  | .struct fs, .struct gs => valsEq? (fs.map (·.2)) (gs.map (·.2))
  | a, b => match numOf a, numOf b with
    | some x, some y => some (x == y)
    | _, _ => none

/-- Whether two lists of values are equal, where that is known. -/
partial def valsEq? : List Val → List Val → Option Bool
  | [], [] => some true
  | v :: vs, w :: ws => match valEq? v w with
    | some true => valsEq? vs ws
    | some false => some false
    | none => none
  | _, _ => some false

end

/-- What a comparison answers, where it is known: an order of numbers, and
equality of what `valEq?` compares. -/
def compareOp? (op : BinOp) (va vb : Val) : Option Bool :=
  match op with
  | .eq => valEq? va vb
  | .ne => (valEq? va vb).map (!·)
  | .lt | .gt | .le | .ge => match numOf va, numOf vb with
    | some a, some b => some (match op with
      | .lt => decide (a < b)
      | .gt => decide (a > b)
      | .le => decide (a ≤ b)
      | _ => decide (a ≥ b))
    | _, _ => none
  | _ => none

/-- Whether two map keys are one, or `unsupported` where that is not known. -/
def keyEq (a b : Val) : EvalM Bool :=
  match valEq? a b with
  | some r => pure r
  | none => throw .unsupported

/-- The value at a key of a map's entries, the first entry's of it. -/
def findKey (k : Val) : List (Val × Val) → EvalM (Option Val)
  | [] => pure none
  | (k', w) :: es => do if ← keyEq k k' then pure (some w) else findKey k es

/-- `a ⊕ b` with operands of type `t`. -/
def binOp (op : BinOp) (t : Ty) (va vb : Val) : EvalM Val :=
  match op with
  | .lt | .gt | .le | .ge | .eq | .ne => match compareOp? op va vb with
    | some b => pure (.bool b)
    | none => throw .unsupported
  | .and | .or | .xor => match va, vb with
    | .bool a, .bool b => pure (.bool (match op with
      | .and => a && b
      | .or => a || b
      | _ => a != b))
    | _, _ => throw .wrong
  | op =>
    if t.isFloat then
      if floatVal va && floatVal vb then pure .float else throw .wrong
    else match numOf va, numOf vb with
      | some a, some b => match op with
        | .add => pure (.int (a + b))
        | .sub => pure (.int (a - b))
        | .mul => pure (.int (a * b))
        | .addC | .addO => fallible op t (a + b)
        | .subC | .subO => fallible op t (a - b)
        | .mulC | .mulO => fallible op t (a * b)
        | .divC | .divO => if b == 0 then throw (failOf (needs op)) else fallible op t (a.tdiv b)
        | _ => throw .wrong
      | _, _ => throw .wrong

/-- `-e`, `-!e`, `-?e` and `not e`, with an operand of type `t`. -/
def unOp (op : UnOp) (t : Ty) (v : Val) : EvalM Val :=
  match op, v with
  | .not, .bool b => pure (.bool !b)
  | .neg, v =>
    if t.isFloat then (if floatVal v then pure .float else throw .wrong)
    else match numOf v with
      | some n => pure (.int (-n))
      | none => throw .wrong
  | .negC, v => match numOf v with
    | some n => if inRangeB t (-n) then pure (.int (-n)) else throw (.early (.er .error))
    | none => throw .wrong
  | .negO, v => match numOf v with
    | some n => if inRangeB t (-n) then pure (.int (-n)) else throw (.early .none)
    | none => throw .wrong
  | _, _ => throw .wrong

/-- How many elements a tensor of a shape holds. -/
def shapeSize (shape : List Nat) : Nat := shape.foldl (· * ·) 1

/-- A field of a tuple or struct value. -/
def fieldVal (v : Val) (s : Sel) : EvalM Val :=
  match v, s with
  | .tuple vs, .pos i => match vs[i]? with
    | some w => pure w
    | none => throw .wrong
  | .struct fs, .name n => match fs.lookup n with
    | some w => pure w
    | none => throw .wrong
  | _, _ => throw .wrong

/-- What an index with nothing at its key returns early. -/
def missing : IdxMode → Stop
  | .opt => .early .none
  | .res => .early (.er .error)

/-- What `v[k]?` or `v[k]!` reads: a list's element, a map's value, a
tensor's element or, of rank above one, its slice along the first axis. -/
def indexVal (m : IdxMode) (v k : Val) : EvalM Val :=
  match v with
  | .list vs => match numOf k with
    | some i => if i < 0 then throw (missing m) else match vs[i.toNat]? with
      | some w => pure w
      | none => throw (missing m)
    | none => throw .wrong
  | .map es => do match ← findKey k es with
    | some w => pure w
    | none => throw (missing m)
  | .tensor shape vs => match numOf k with
    | some i => if i < 0 then throw (missing m) else match shape with
      | [] | [_] => match vs[i.toNat]? with
        | some w => pure w
        | none => throw (missing m)
      | s0 :: rest =>
        if i.toNat < s0 then
          pure (.tensor rest ((vs.drop (i.toNat * shapeSize rest)).take (shapeSize rest)))
        else throw (missing m)
    | none => throw .wrong
  | _ => throw .wrong

/-- The value of a checker's answer, or `wrong` where it has none. -/
def typeOf (m : ChkM Ty) : EvalM Ty :=
  match m.run.val with
  | .ok t => pure t
  | .error _ => throw .wrong

/-- Evaluation, reading and writing the variables in scope: a call writes
back to its `mut` and `out` arguments' variables. -/
abbrev EvalT := StateT VEnv EvalM

/-- Set the innermost variable of a name. -/
def setVar (x : String) (v : Val) : VEnv → VEnv
  | [] => []
  | (y, o) :: ρ => if y == x then (y, some v) :: ρ else (y, o) :: setVar x v ρ

/-- A tuple or struct value with a field replaced. -/
def putField (v : Val) (s : Sel) (w : Val) : EvalM Val :=
  match v, s with
  | .tuple vs, .pos i => if i < vs.length then pure (.tuple (vs.set i w)) else throw .wrong
  | .struct fs, .name n => pure (.struct (replaceFirst n w fs))
  | _, _ => throw .wrong
where
  replaceFirst (n : String) (w : Val) : List (String × Val) → List (String × Val)
    | [] => []
    | (m, u) :: fs => if m == n then (m, w) :: fs else (m, u) :: replaceFirst n w fs

/-- A map's entries with the value at a key replaced, the first entry
`indexVal` would read. -/
def replaceKey (k w : Val) : List (Val × Val) → EvalM (List (Val × Val))
  | [] => pure []
  | (k', u) :: es => do
    if ← keyEq k k' then pure ((k', w) :: es) else ((k', u) :: ·) <$> replaceKey k w es

/-- A list, map or tensor of rank one or less with its element at a key
replaced, which `indexVal` read. A tensor's slice is not written. -/
def putIndex (v k w : Val) : EvalM Val :=
  match v with
  | .list vs => match numOf k with
    | some i => pure (.list (vs.set i.toNat w))
    | none => throw .wrong
  | .map es => .map <$> replaceKey k w es
  | .tensor shape vs => match shape with
    | [] | [_] => match numOf k with
      | some i => pure (.tensor shape (vs.set i.toNat w))
      | none => throw .wrong
    | _ => throw .unsupported
  | _ => throw .wrong

/-- A map with a value at a key, replaced or added. -/
def insertKey (k w : Val) (es : List (Val × Val)) : EvalM (List (Val × Val)) := do
  match ← findKey k es with
  | some _ => replaceKey k w es
  | none => pure (es ++ [(k, w)])

/-- What a `set` target's steps need evaluated, by the types `setSteps`
walks: the type the value is written at, and each key with its type, in
order. None where the checker refuses the target, a compound `set` that
inserts into a map among it. -/
def setPlan (op : Option BinOp) : Ty → List Step → Option (Ty × List (Expr × Ty))
  | t, [] => some (t, [])
  | t, [.index k none] => match op, t.expose with
    | none, .map kt vt => some (vt, [(k, kt)])
    | _, _ => none
  | t, .field s :: rest => match fieldType t s with
    | .ok ft => setPlan op ft rest
    | .error _ => none
  | _, .index _ none :: _ => none
  | t, .index k (some _) :: rest => match indexTypes t with
    | .ok (et, kt) => (setPlan op et rest).map fun (vt, ks) => (vt, (k, kt) :: ks)
    | .error _ => none

/-- What a variable that held `old`, of type `t`, holds once a `set` writes
`r` through its steps, with its keys' values `kvs`: a compound one's
operator applied to what is there. -/
def writeSteps (op : Option BinOp) : Ty → Option Val → List Step → List Val → Val → EvalM Val
  | t, old, [], _, r => match op with
    | none => pure r
    | some op => match old with
      | some o => binOp op t o r
      | none => throw .uninit
  | _, old, [.index _ none], [kv], r => match old with
    | some (.map es) => .map <$> insertKey kv r es
    | some _ => throw .wrong
    | none => throw .uninit
  | t, old, .field s :: rest, kvs, r => match fieldType t s, old with
    | .ok ft, some v => do
      let part ← fieldVal v s
      let w ← writeSteps op ft (some part) rest kvs r
      putField v s w
    | .ok _, none => throw .uninit
    | .error _, _ => throw .wrong
  | t, old, .index _ (some m) :: rest, kv :: kvs, r => match indexTypes t, old with
    | .ok (et, _), some v => do
      let part ← indexVal m v kv
      let w ← writeSteps op et (some part) rest kvs r
      putIndex v kv w
    | .ok _, none => throw .uninit
    | .error _, _ => throw .wrong
  | _, _, _, _, _ => throw .wrong

/-- An argument with its hint taken off: a hint says what type it is and
nothing of where it is, so `mut : T / x` writes back to `x`. Only one hint
parses there. -/
def unhint : Expr → Expr
  | .hinted _ e => e
  | e => e

/-- Each `mut` and `out` argument's place set to what its parameter came to:
a variable, or a part of one through its steps, with the keys' values the
argument was evaluated with (`kvss`), as the pointer the Rust passes was
taken then, under a hint or not. -/
def writeBack (Γ : Ctx) : VEnv → List (Option Mode × Expr) → List (Mode × Ty) → List (List Val) →
    List (Option Val) → EvalM VEnv
  | ρ, (_, e) :: args, (mode, _) :: params, kvs :: kvss, a :: after =>
    if mode == .mut || mode == .out then
      match unhint e, a with
      | .var x, some v => writeBack Γ (setVar x v ρ) args params kvss after
      | .place x steps, some v => match Γ.lookup x, ρ.lookup x with
        | some (u, _), some old => do
          let w ← writeSteps none u old steps kvs v
          writeBack Γ (setVar x w ρ) args params kvss after
        | _, _ => throw .wrong
      | _, none => throw .uninit
      | _, _ => throw .unsupported
    else writeBack Γ ρ args params kvss after
  | ρ, _, _, _, _ => pure ρ

/-- An argument read by value, none for an `out` one, with no keys. -/
def argByValue (mode : Mode) (v : EvalT Val) : EvalT (Option Val × List Val) := do
  let o ← if mode == .out then pure none else some <$> v
  pure (o, [])

/-- An early return, with the variables as they are, so that a call can
write back what its parameters came to. -/
def earlyHere {α} (r : Val) : EvalT α := do throw (.returned (some r) (← get))

/-- A step that reads no variable, its early return given the variables as
they are. -/
def here {α} (m : EvalM α) : EvalT α :=
  match m with
  | .ok a => pure a
  | .error (.early r) => earlyHere r
  | .error s => throw s

mutual

/-- What `e` evaluates to, as `synthF` types it. -/
def evalS (env : InferEnv) (call : CallFn) : Nat → Ctx → Expr → EvalT Val
  | 0, _, _ => throw .fuel
  | n + 1, Γ₀, e =>
    let Γ := Γ₀.enter e
    match e with
    | .hinted h inner => do
      let t ← match Γ.hint h with
        | .ok t => pure t
        | .error _ => throw .wrong
      evalForm env call n Γ inner t
    | .var x => do match (← get).lookup x with
      | some (some v) => pure v
      | some none => throw .uninit
      | none => throw .wrong
    | .binop op a b => do
      let (t, va, vb) ← evalPair env call n Γ a b none
      here (binOp op t va vb)
    | .unop op a => do
      let t ← here (typeOf (synthF env n Γ.borrow a))
      let v ← evalS env call n Γ.borrow a
      here (unOp op t v)
    | .tryOpt a => do
      match ← evalS env call n Γ a with
      | .none => earlyHere .none
      | .some v => pure v
      | _ => throw .wrong
    | .tryRes a => do
      match ← evalS env call n Γ a with
      | .er v => earlyHere (.er v)
      | .ok v => pure v
      | _ => throw .wrong
    | .adapt a => evalS env call n Γ.borrow a
    | .call f args => evalCall env call n Γ f args none
    | .place x steps => do match (← get).lookup x, Γ.lookup x with
      | some (some v), some (t, _) => evalSteps env call n Γ t v steps
      | some none, _ => throw .uninit
      | _, _ => throw .wrong
    | .proj b s => do here (fieldVal (← evalS env call n (Γ.under b) b) s)
    | .index b k mode => match mode with
      | none => throw .wrong
      | some m => do
        let bt ← here (typeOf (synthF env n (Γ.under b) b))
        let (_, kt) ← match indexTypes bt with
          | .ok p => pure p
          | .error _ => throw .wrong
        let v ← evalS env call n (Γ.under b) b
        let kv ← evalC env call n Γ k kt
        here (indexVal m v kv)
    | e => do
      let t ← here (typeOf (synthF env (n + 1) Γ₀ e))
      evalForm env call n Γ e t

/-- What `e` evaluates to, as `checkF` checks it against `t`. -/
def evalC (env : InferEnv) (call : CallFn) : Nat → Ctx → Expr → Ty → EvalT Val
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, .hinted h inner, expected => evalForm env call n (Γ.enter (.hinted h inner)) inner expected
  | n + 1, Γ, e, expected => evalForm env call n (Γ.enter e) e expected

/-- What `e` evaluates to, as `checkForm` checks it against `expected`. -/
def evalForm (env : InferEnv) (call : CallFn) : Nat → Ctx → Expr → Ty → EvalT Val
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, e, expected =>
    match e with
    | .none_ => pure .none
    | .er p => match expected.expose with
      | .result _ => .er <$> evalC env call n Γ p .error
      | _ => throw .wrong
    | .some_ p => match expected.expose with
      | .option t => .some <$> evalC env call n Γ p t
      | _ => throw .wrong
    | .ok p => match expected.expose with
      | .result t => .ok <$> evalC env call n Γ p t
      | _ => throw .wrong
    | .tt => pure (.bool true)
    | .ff => pure (.bool false)
    | .str s => pure (.str s)
    | .int k => pure (.int k)
    | .float => pure .float
    | .hex k => pure (.hex k)
    | .tuple es => match expected.expose with
      | .tuple ts => .tuple <$> evalPairs env call n Γ es (Lit.tysList ts)
      | _ => throw .wrong
    | .struct fs => match expected.expose with
      | .struct ts => .struct <$> evalFields env call n Γ fs (Lit.fieldsList ts)
      | _ => throw .wrong
    | .list es => match expected.expose with
      | .list t => .list <$> evalAll env call n Γ es t
      | _ => throw .wrong
    | .set es => match expected.expose with
      | .set t => .set <$> evalAll env call n Γ es t
      | _ => throw .wrong
    | .map es => match expected.expose with
      | .map k v => .map <$> evalEntries env call n Γ es k v
      | _ => throw .wrong
    | .tensor shape es => match expected.expose with
      | .tensor t _ => .tensor shape <$> evalAll env call n Γ es t
      | _ => throw .wrong
    | .table header rows => match expected.expose with
      | .table cols => .table header <$> evalRows env call n Γ rows (Lit.fieldsList cols)
      | _ => throw .wrong
    | .atom a => pure (.atom a)
    | .term a p => match expected.expose with
      | .term _ t => .term a <$> evalC env call n Γ p t
      | .enum vs => match Lit.Variants.termPayload a vs with
        | some t => .term a <$> evalC env call n Γ p t
        | none => throw .wrong
      | _ => throw .wrong
    | .enum inner => evalC env call n Γ inner expected
    | .data p => do let _ ← evalS env call n Γ p; pure .data
    | .error p => do let _ ← evalS env call n Γ p; pure .error
    | .unop .neg a =>
      let t := expected.expose
      let float := isFloatT t || Γ.boundOf t == Option.some .float
      match a with
      | .int k => match Lit.intRange t with
        | some _ => pure (.int (-k))
        | none => if float then do here (unOp .neg t (← evalC env call n Γ.borrow a t)) else evalSub env call n Γ e
      | _ => if float then do here (unOp .neg t (← evalC env call n Γ.borrow a t)) else evalSub env call n Γ e
    | .binop op a b =>
      if yieldsOperand op then do
        let (t, va, vb) ← evalPair env call n Γ a b (some expected)
        here (binOp op t va vb)
      else evalSub env call n Γ e
    | .adapt a => evalS env call n Γ.borrow a
    | .call f args => evalCall env call n Γ f args (some expected)
    | .unop _ _ | .var _ | .tryOpt _ | .tryRes _ | .hinted _ _
    | .place _ _ | .proj _ _ | .index _ _ _ => evalSub env call n Γ e

/-- A call, as `callF` types it: its type arguments inferred, its arguments
evaluated against its parameters' types with them substituted, the function
called with their values, and each `mut` and `out` argument's variable set to
what its parameter came to. A call with type arguments that are not ground
is not run yet. -/
def evalCall (env : InferEnv) (call : CallFn) : Nat → Ctx → String → List (Option Mode × Expr) →
    Option Ty → EvalT Val
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, f, args, expected =>
    match Γ.sigs.lookup f, decided (env.call Γ f args expected) with
    | some sig, .ok θ =>
      if !groundArgs θ then throw .unsupported
      else do
        let ps := sig.params.map fun (m, t) => (m, t.inst θ)
        let vals ← evalArgs env call n Γ args ps
        let (r, after) ← here (call f θ (vals.map (·.1)))
        set (← here (writeBack Γ (← get) args ps (vals.map (·.2)) after))
        pure r
    | _, _ => throw .wrong

/-- A call's arguments, as `argsF` checks them, an `out` one passing no
value, each with the keys' values of a `mut` or `out` one into a part of a
variable, for writing back to (`writeBack`). Such a one is evaluated as
`checkF` reaches the place's steps, through its form, subsumption and
synthesis, under a hint or not. -/
def evalArgs (env : InferEnv) (call : CallFn) : Nat → Ctx → List (Option Mode × Expr) →
    List (Mode × Ty) → EvalT (List (Option Val × List Val))
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, (_, e) :: args, (mode, t) :: params => do
    let a ← match n, unhint e with
      | d + 4, .place x steps =>
        if mode == .mut || mode == .out then
          argAt env call (d + 1) (((Γ.forArg mode).enter (.place x steps)).enter (.place x steps)) mode x steps
        else argByValue mode (evalC env call (d + 4) (Γ.forArg mode) e t)
      | m, .place _ _ =>
        if mode == .mut || mode == .out then throw .fuel
        else argByValue mode (evalC env call m (Γ.forArg mode) e t)
      | m, _ => argByValue mode (evalC env call m (Γ.forArg mode) e t)
    let vs ← evalArgs env call n Γ args params
    pure (a :: vs)
  | _ + 1, _, _, _ => pure []

/-- A `mut` or `out` argument into a part of variable `x` through `steps`:
what is there, none for an `out` one, and the keys' values. -/
def argAt (env : InferEnv) (call : CallFn) : Nat → Ctx → Mode → String → List Step →
    EvalT (Option Val × List Val)
  | 0, _, _, _, _ => throw .fuel
  | d + 1, Γ, mode, x, steps => do
    match (← get).lookup x, Γ.lookup x with
    | some (some v), some (u, _) => do
      let (w, kvs) ← evalPath env call d Γ u v steps
      pure (if mode == .out then none else some w, kvs)
    | some none, _ => throw .uninit
    | _, _ => throw .wrong

/-- A place's steps from a value of type `t`, as `evalSteps` takes them,
with the values of their keys, in order. -/
def evalPath (env : InferEnv) (call : CallFn) : Nat → Ctx → Ty → Val → List Step → EvalT (Val × List Val)
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, v, [] => pure (v, [])
  | n + 1, Γ, t, v, [st] => evalPathStep env call n Γ t v st
  | n + 1, Γ, t, v, st :: rest => do
    let u ← here (typeOf (stepF env n Γ.through t st))
    let (w, k) ← evalPathStep env call n Γ.through t v st
    let (r, ks) ← evalPath env call n Γ u w rest
    pure (r, k ++ ks)

/-- A step from a value of type `t`, as `evalStep` takes it, with its key's
value. -/
def evalPathStep (env : InferEnv) (call : CallFn) : Nat → Ctx → Ty → Val → Step → EvalT (Val × List Val)
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, v, .field s => do pure (← here (fieldVal v s), [])
  | _ + 1, _, _, _, .index _ none => throw .wrong
  | n + 1, Γ, t, v, .index k (some m) => do
    let (_, kt) ← match indexTypes t with
      | .ok p => pure p
      | .error _ => throw .wrong
    let kv ← evalC env call n Γ k kt
    pure (← here (indexVal m v kv), [kv])

/-- A place's steps from a value of type `t`, as `stepsF` types them. -/
def evalSteps (env : InferEnv) (call : CallFn) : Nat → Ctx → Ty → Val → List Step → EvalT Val
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, v, [] => pure v
  | n + 1, Γ, t, v, [st] => evalStep env call n Γ t v st
  | n + 1, Γ, t, v, st :: rest => do
    let u ← here (typeOf (stepF env n Γ.through t st))
    let w ← evalStep env call n Γ.through t v st
    evalSteps env call n Γ u w rest

/-- A step from a value of type `t`, as `stepF` types it. -/
def evalStep (env : InferEnv) (call : CallFn) : Nat → Ctx → Ty → Val → Step → EvalT Val
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, v, .field s => here (fieldVal v s)
  | _ + 1, _, _, _, .index _ none => throw .wrong
  | n + 1, Γ, t, v, .index k (some m) => do
    let (_, kt) ← match indexTypes t with
      | .ok p => pure p
      | .error _ => throw .wrong
    let kv ← evalC env call n Γ k kt
    here (indexVal m v kv)

/-- What `e` evaluates to, as `subsumeF` synthesizes it. -/
def evalSub (env : InferEnv) (call : CallFn) : Nat → Ctx → Expr → EvalT Val
  | 0, _, _ => throw .fuel
  | n + 1, Γ, e => evalS env call n Γ e

/-- Two operands, as `pairF` types them: their type, and their values
checked against it. -/
def evalPair (env : InferEnv) (call : CallFn) : Nat → Ctx → Expr → Expr → Option Ty → EvalT (Ty × Val × Val)
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, a, b, expected => do
    let t ← here (typeOf (pairF env (n + 1) Γ a b expected))
    let va ← evalC env call n Γ.borrow a t
    let vb ← evalC env call n Γ.borrow b t
    pure (t, va, vb)

def evalPairs (env : InferEnv) (call : CallFn) : Nat → Ctx → List Expr → List Ty → EvalT (List Val)
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, e :: es, t :: ts => do
    let v ← evalC env call n Γ e t
    let vs ← evalPairs env call n Γ es ts
    pure (v :: vs)
  | _ + 1, _, _, _ => pure []

def evalFields (env : InferEnv) (call : CallFn) : Nat → Ctx → List (String × Expr) → List (String × Ty) →
    EvalT (List (String × Val))
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, (m, e) :: fs, (_, t) :: ts => do
    let v ← evalC env call n Γ e t
    let vs ← evalFields env call n Γ fs ts
    pure ((m, v) :: vs)
  | _ + 1, _, _, _ => pure []

def evalAll (env : InferEnv) (call : CallFn) : Nat → Ctx → List Expr → Ty → EvalT (List Val)
  | 0, _, _, _ => throw .fuel
  | _ + 1, _, [], _ => pure []
  | n + 1, Γ, e :: es, t => do
    let v ← evalC env call n Γ e t
    let vs ← evalAll env call n Γ es t
    pure (v :: vs)

def evalEntries (env : InferEnv) (call : CallFn) : Nat → Ctx → List (Expr × Expr) → Ty → Ty →
    EvalT (List (Val × Val))
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, [], _, _ => pure []
  | n + 1, Γ, (ke, ve) :: es, k, v => do
    let kv ← evalC env call n Γ ke k
    let vv ← evalC env call n Γ ve v
    let rest ← evalEntries env call n Γ es k v
    pure ((kv, vv) :: rest)

def evalRows (env : InferEnv) (call : CallFn) : Nat → Ctx → List (List Expr) → List (String × Ty) →
    EvalT (List (List Val))
  | 0, _, _, _ => throw .fuel
  | _ + 1, _, [], _ => pure []
  | n + 1, Γ, r :: rs, cols => do
    let vs ← evalPairs env call n Γ r (cols.map (·.2))
    let rest ← evalRows env call n Γ rs cols
    pure (vs :: rest)

end

end Datalove.Fun
