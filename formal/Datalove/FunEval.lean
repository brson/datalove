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
fields, indexes and calls. A call goes to a `CallFn` the evaluator is given
(`FunCall.lean` runs the function's body); one with a parameter that writes
back, `mut` or `out`, or with type arguments that are not ground, is
`Stop.unsupported`. `@`
converts between types whose values are the same: a wider number, an atom or
term into an enum, a clone. A comparison, and whether two map keys are one,
are answered where the values' contents are modelled (`valEq?`): numbers,
booleans and what is built of them. A string, a float, `data` or `error`
carries nothing here, so comparing one, or looking one up as a map's key, is
`Stop.unsupported`.
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

abbrev EvalM := Except Stop

/-- The variables in scope, innermost first, each with its value, or none
before it is set. -/
abbrev VEnv := List (String × Option Val)

/-- What calls do: the function's name, its type arguments, and its
arguments' values, to what it returns. `FunCall.lean` builds the real one,
running the function's body, from a program. -/
abbrev CallFn := String → List (String × Ty) → List Val → EvalM Val

/-- Type arguments with no type parameter and no `rec` in them, which the
check-once theorem takes. -/
def groundArgs (θ : List (String × Ty)) : Bool :=
  θ.all fun (_, t) => !t.containsVar && !t.containsRec

/-- Whether a parameter mode writes back into its argument, which calls do
not do yet. -/
def writesBack (params : List (Mode × Ty)) : Bool :=
  params.any fun (m, _) => m == .mut || m == .out


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
for a string, a float, `data`, `error`, a map, a set, a tensor or a table, or
what holds one. Values compared are of one type. -/
partial def valEq? : Val → Val → Option Bool
  | .bool a, .bool b => some (a == b)
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

mutual

/-- What `e` evaluates to, as `synthF` types it. -/
def evalS (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Expr → EvalM Val
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ₀, ρ, e =>
    let Γ := Γ₀.enter e
    match e with
    | .hinted h inner => do
      let t ← match Γ.hint h with
        | .ok t => pure t
        | .error _ => throw .wrong
      evalForm env call n Γ ρ inner t
    | .var x => match ρ.lookup x with
      | some (some v) => pure v
      | some none => throw .uninit
      | none => throw .wrong
    | .binop op a b => do
      let (t, va, vb) ← evalPair env call n Γ ρ a b none
      binOp op t va vb
    | .unop op a => do
      let t ← typeOf (synthF env n Γ.borrow a)
      let v ← evalS env call n Γ.borrow ρ a
      unOp op t v
    | .tryOpt a => do
      match ← evalS env call n Γ ρ a with
      | .none => throw (.early .none)
      | .some v => pure v
      | _ => throw .wrong
    | .tryRes a => do
      match ← evalS env call n Γ ρ a with
      | .er v => throw (.early (.er v))
      | .ok v => pure v
      | _ => throw .wrong
    | .adapt a => evalS env call n Γ.borrow ρ a
    | .call f args => evalCall env call n Γ ρ f args none
    | .place x steps => match ρ.lookup x, Γ.lookup x with
      | some (some v), some (t, _) => evalSteps env call n Γ ρ t v steps
      | some none, _ => throw .uninit
      | _, _ => throw .wrong
    | .proj b s => do fieldVal (← evalS env call n Γ ρ b) s
    | .index b k mode => match mode with
      | none => throw .wrong
      | some m => do
        let bt ← typeOf (synthF env n Γ b)
        let (_, kt) ← match indexTypes bt with
          | .ok p => pure p
          | .error _ => throw .wrong
        let v ← evalS env call n Γ ρ b
        let kv ← evalC env call n Γ ρ k kt
        indexVal m v kv
    | e => do
      let t ← typeOf (synthF env (n + 1) Γ₀ e)
      evalForm env call n Γ ρ e t

/-- What `e` evaluates to, as `checkF` checks it against `t`. -/
def evalC (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Expr → Ty → EvalM Val
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, .hinted h inner, expected => evalForm env call n (Γ.enter (.hinted h inner)) ρ inner expected
  | n + 1, Γ, ρ, e, expected => evalForm env call n (Γ.enter e) ρ e expected

/-- What `e` evaluates to, as `checkForm` checks it against `expected`. -/
def evalForm (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Expr → Ty → EvalM Val
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, e, expected =>
    match e with
    | .none_ => pure .none
    | .er p => match expected.expose with
      | .result _ => .er <$> evalC env call n Γ ρ p .error
      | _ => throw .wrong
    | .some_ p => match expected.expose with
      | .option t => .some <$> evalC env call n Γ ρ p t
      | _ => throw .wrong
    | .ok p => match expected.expose with
      | .result t => .ok <$> evalC env call n Γ ρ p t
      | _ => throw .wrong
    | .tt => pure (.bool true)
    | .ff => pure (.bool false)
    | .str => pure .str
    | .int k => pure (.int k)
    | .float => pure .float
    | .hex k => pure (.hex k)
    | .tuple es => match expected.expose with
      | .tuple ts => .tuple <$> evalPairs env call n Γ ρ es (Lit.tysList ts)
      | _ => throw .wrong
    | .struct fs => match expected.expose with
      | .struct ts => .struct <$> evalFields env call n Γ ρ fs (Lit.fieldsList ts)
      | _ => throw .wrong
    | .list es => match expected.expose with
      | .list t => .list <$> evalAll env call n Γ ρ es t
      | _ => throw .wrong
    | .set es => match expected.expose with
      | .set t => .set <$> evalAll env call n Γ ρ es t
      | _ => throw .wrong
    | .map es => match expected.expose with
      | .map k v => .map <$> evalEntries env call n Γ ρ es k v
      | _ => throw .wrong
    | .tensor shape es => match expected.expose with
      | .tensor t _ => .tensor shape <$> evalAll env call n Γ ρ es t
      | _ => throw .wrong
    | .table header rows => match expected.expose with
      | .table cols => .table header <$> evalRows env call n Γ ρ rows (Lit.fieldsList cols)
      | _ => throw .wrong
    | .atom a => pure (.atom a)
    | .term a p => match expected.expose with
      | .term _ t => .term a <$> evalC env call n Γ ρ p t
      | .enum vs => match Lit.Variants.termPayload a vs with
        | some t => .term a <$> evalC env call n Γ ρ p t
        | none => throw .wrong
      | _ => throw .wrong
    | .enum inner => evalC env call n Γ ρ inner expected
    | .data p => do let _ ← evalS env call n Γ ρ p; pure .data
    | .error p => do let _ ← evalS env call n Γ ρ p; pure .error
    | .unop .neg a =>
      let t := expected.expose
      let float := isFloatT t || Γ.boundOf t == Option.some .float
      match a with
      | .int k => match Lit.intRange t with
        | some _ => pure (.int (-k))
        | none => if float then do unOp .neg t (← evalC env call n Γ.borrow ρ a t) else evalSub env call n Γ ρ e
      | _ => if float then do unOp .neg t (← evalC env call n Γ.borrow ρ a t) else evalSub env call n Γ ρ e
    | .binop op a b =>
      if yieldsOperand op then do
        let (t, va, vb) ← evalPair env call n Γ ρ a b (some expected)
        binOp op t va vb
      else evalSub env call n Γ ρ e
    | .adapt a => evalS env call n Γ.borrow ρ a
    | .call f args => evalCall env call n Γ ρ f args (some expected)
    | .unop _ _ | .var _ | .tryOpt _ | .tryRes _ | .hinted _ _
    | .place _ _ | .proj _ _ | .index _ _ _ => evalSub env call n Γ ρ e

/-- A call, as `callF` types it: its type arguments inferred, its arguments
evaluated against its parameters' types with them substituted, and the
function called with their values. A call with a parameter that writes back
(`mut`, `out`), or with type arguments that are not ground, is not run
yet. -/
def evalCall (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → String → List (Option Mode × Expr) →
    Option Ty → EvalM Val
  | 0, _, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, f, args, expected =>
    match Γ.sigs.lookup f, decided (env.call Γ f args expected) with
    | some sig, .ok θ =>
      if writesBack sig.params || !groundArgs θ then throw .unsupported
      else do
        let vals ← evalArgs env call n Γ ρ args (sig.params.map fun (m, t) => (m, t.inst θ))
        call f θ vals
    | _, _ => throw .wrong

/-- A call's arguments, as `argsF` checks them. -/
def evalArgs (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → List (Option Mode × Expr) →
    List (Mode × Ty) → EvalM (List Val)
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, (_, e) :: args, (mode, t) :: params => do
    let v ← evalC env call n (Γ.forArg mode) ρ e t
    let vs ← evalArgs env call n Γ ρ args params
    pure (v :: vs)
  | _ + 1, _, _, _, _ => pure []

/-- A place's steps from a value of type `t`, as `stepsF` types them. -/
def evalSteps (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Ty → Val → List Step → EvalM Val
  | 0, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, _, v, [] => pure v
  | n + 1, Γ, ρ, t, v, [st] => evalStep env call n Γ ρ t v st
  | n + 1, Γ, ρ, t, v, st :: rest => do
    let u ← typeOf (stepF env n Γ.through t st)
    let w ← evalStep env call n Γ.through ρ t v st
    evalSteps env call n Γ ρ u w rest

/-- A step from a value of type `t`, as `stepF` types it. -/
def evalStep (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Ty → Val → Step → EvalM Val
  | 0, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, _, v, .field s => fieldVal v s
  | _ + 1, _, _, _, _, .index _ none => throw .wrong
  | n + 1, Γ, ρ, t, v, .index k (some m) => do
    let (_, kt) ← match indexTypes t with
      | .ok p => pure p
      | .error _ => throw .wrong
    let kv ← evalC env call n Γ ρ k kt
    indexVal m v kv

/-- What `e` evaluates to, as `subsumeF` synthesizes it. -/
def evalSub (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Expr → EvalM Val
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, e => evalS env call n Γ ρ e

/-- Two operands, as `pairF` types them: their type, and their values
checked against it. -/
def evalPair (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → Expr → Expr → Option Ty → EvalM (Ty × Val × Val)
  | 0, _, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, a, b, expected => do
    let t ← typeOf (pairF env (n + 1) Γ a b expected)
    let va ← evalC env call n Γ.borrow ρ a t
    let vb ← evalC env call n Γ.borrow ρ b t
    pure (t, va, vb)

def evalPairs (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → List Expr → List Ty → EvalM (List Val)
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, e :: es, t :: ts => do
    let v ← evalC env call n Γ ρ e t
    let vs ← evalPairs env call n Γ ρ es ts
    pure (v :: vs)
  | _ + 1, _, _, _, _ => pure []

def evalFields (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → List (String × Expr) → List (String × Ty) →
    EvalM (List (String × Val))
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, (m, e) :: fs, (_, t) :: ts => do
    let v ← evalC env call n Γ ρ e t
    let vs ← evalFields env call n Γ ρ fs ts
    pure ((m, v) :: vs)
  | _ + 1, _, _, _, _ => pure []

def evalAll (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → List Expr → Ty → EvalM (List Val)
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, [], _ => pure []
  | n + 1, Γ, ρ, e :: es, t => do
    let v ← evalC env call n Γ ρ e t
    let vs ← evalAll env call n Γ ρ es t
    pure (v :: vs)

def evalEntries (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → List (Expr × Expr) → Ty → Ty →
    EvalM (List (Val × Val))
  | 0, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, [], _, _ => pure []
  | n + 1, Γ, ρ, (ke, ve) :: es, k, v => do
    let kv ← evalC env call n Γ ρ ke k
    let vv ← evalC env call n Γ ρ ve v
    let rest ← evalEntries env call n Γ ρ es k v
    pure ((kv, vv) :: rest)

def evalRows (env : InferEnv) (call : CallFn) : Nat → Ctx → VEnv → List (List Expr) → List (String × Ty) →
    EvalM (List (List Val))
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, [], _ => pure []
  | n + 1, Γ, ρ, r :: rs, cols => do
    let vs ← evalPairs env call n Γ ρ r (cols.map (·.2))
    let rest ← evalRows env call n Γ ρ rs cols
    pure (vs :: rest)

end

end Datalove.Fun
