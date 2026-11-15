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
fields and indexes. Calls are `Stop.unsupported`. `@`
converts between types whose values are the same: a wider number, an atom or
term into an enum, a clone. A comparison's answer is not modelled
(`compareOp`), since soundness needs only that it is a `bool`.
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
  /-- A form not evaluated yet: a call. -/
  | unsupported

abbrev EvalM := Except Stop

/-- The values of the variables in scope. -/
abbrev VEnv := List (String × Val)

/-- What a comparison answers. Not modelled: it is a `bool`. -/
opaque compareOp : BinOp → Ty → Val → Val → Bool

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

/-- `a ⊕ b` with operands of type `t`. -/
def binOp (op : BinOp) (t : Ty) (va vb : Val) : EvalM Val :=
  match op with
  | .lt | .gt | .le | .ge | .eq | .ne => pure (.bool (compareOp op t va vb))
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

/-- Whether two keys are one. Not modelled: a map lookup finds some entry,
or none. -/
opaque keyEq : Val → Val → Bool

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
  | .map es => match es.find? (fun e => keyEq k e.1) with
    | some e => pure e.2
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
def evalS (env : InferEnv) : Nat → Ctx → VEnv → Expr → EvalM Val
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ₀, ρ, e =>
    let Γ := Γ₀.enter e
    match e with
    | .hinted h inner => do
      let t ← match Γ.hint h with
        | .ok t => pure t
        | .error _ => throw .wrong
      evalForm env n Γ ρ inner t
    | .var x => match ρ.lookup x with
      | some v => pure v
      | none => throw .wrong
    | .binop op a b => do
      let (t, va, vb) ← evalPair env n Γ ρ a b none
      binOp op t va vb
    | .unop op a => do
      let t ← typeOf (synthF env n Γ.borrow a)
      let v ← evalS env n Γ.borrow ρ a
      unOp op t v
    | .tryOpt a => do
      match ← evalS env n Γ ρ a with
      | .none => throw (.early .none)
      | .some v => pure v
      | _ => throw .wrong
    | .tryRes a => do
      match ← evalS env n Γ ρ a with
      | .er v => throw (.early (.er v))
      | .ok v => pure v
      | _ => throw .wrong
    | .adapt a => evalS env n Γ.borrow ρ a
    | .call _ _ => throw .unsupported
    | .place x steps => match ρ.lookup x, Γ.lookup x with
      | some v, some (t, _) => evalSteps env n Γ ρ t v steps
      | _, _ => throw .wrong
    | .proj b s => do fieldVal (← evalS env n Γ ρ b) s
    | .index b k mode => match mode with
      | none => throw .wrong
      | some m => do
        let bt ← typeOf (synthF env n Γ b)
        let (_, kt) ← match indexTypes bt with
          | .ok p => pure p
          | .error _ => throw .wrong
        let v ← evalS env n Γ ρ b
        let kv ← evalC env n Γ ρ k kt
        indexVal m v kv
    | e => do
      let t ← typeOf (synthF env (n + 1) Γ₀ e)
      evalForm env n Γ ρ e t

/-- What `e` evaluates to, as `checkF` checks it against `t`. -/
def evalC (env : InferEnv) : Nat → Ctx → VEnv → Expr → Ty → EvalM Val
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, .hinted h inner, expected => evalForm env n (Γ.enter (.hinted h inner)) ρ inner expected
  | n + 1, Γ, ρ, e, expected => evalForm env n (Γ.enter e) ρ e expected

/-- What `e` evaluates to, as `checkForm` checks it against `expected`. -/
def evalForm (env : InferEnv) : Nat → Ctx → VEnv → Expr → Ty → EvalM Val
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, e, expected =>
    match e with
    | .none_ => pure .none
    | .er p => match expected.expose with
      | .result _ => .er <$> evalC env n Γ ρ p .error
      | _ => throw .wrong
    | .some_ p => match expected.expose with
      | .option t => .some <$> evalC env n Γ ρ p t
      | _ => throw .wrong
    | .ok p => match expected.expose with
      | .result t => .ok <$> evalC env n Γ ρ p t
      | _ => throw .wrong
    | .tt => pure (.bool true)
    | .ff => pure (.bool false)
    | .str => pure .str
    | .int k => pure (.int k)
    | .float => pure .float
    | .hex k => pure (.hex k)
    | .tuple es => match expected.expose with
      | .tuple ts => .tuple <$> evalPairs env n Γ ρ es (Lit.tysList ts)
      | _ => throw .wrong
    | .struct fs => match expected.expose with
      | .struct ts => .struct <$> evalFields env n Γ ρ fs (Lit.fieldsList ts)
      | _ => throw .wrong
    | .list es => match expected.expose with
      | .list t => .list <$> evalAll env n Γ ρ es t
      | _ => throw .wrong
    | .set es => match expected.expose with
      | .set t => .set <$> evalAll env n Γ ρ es t
      | _ => throw .wrong
    | .map es => match expected.expose with
      | .map k v => .map <$> evalEntries env n Γ ρ es k v
      | _ => throw .wrong
    | .tensor shape es => match expected.expose with
      | .tensor t _ => .tensor shape <$> evalAll env n Γ ρ es t
      | _ => throw .wrong
    | .table header rows => match expected.expose with
      | .table cols => .table header <$> evalRows env n Γ ρ rows (Lit.fieldsList cols)
      | _ => throw .wrong
    | .atom a => pure (.atom a)
    | .term a p => match expected.expose with
      | .term _ t => .term a <$> evalC env n Γ ρ p t
      | .enum vs => match Lit.Variants.termPayload a vs with
        | some t => .term a <$> evalC env n Γ ρ p t
        | none => throw .wrong
      | _ => throw .wrong
    | .enum inner => evalC env n Γ ρ inner expected
    | .data p => do let _ ← evalS env n Γ ρ p; pure .data
    | .error p => do let _ ← evalS env n Γ ρ p; pure .error
    | .unop .neg a =>
      let t := expected.expose
      let float := isFloatT t || Γ.boundOf t == Option.some .float
      match a with
      | .int k => match Lit.intRange t with
        | some _ => pure (.int (-k))
        | none => if float then do unOp .neg t (← evalC env n Γ.borrow ρ a t) else evalSub env n Γ ρ e
      | _ => if float then do unOp .neg t (← evalC env n Γ.borrow ρ a t) else evalSub env n Γ ρ e
    | .binop op a b =>
      if yieldsOperand op then do
        let (t, va, vb) ← evalPair env n Γ ρ a b (some expected)
        binOp op t va vb
      else evalSub env n Γ ρ e
    | .adapt a => evalS env n Γ.borrow ρ a
    | .call _ _ => throw .unsupported
    | .unop _ _ | .var _ | .tryOpt _ | .tryRes _ | .hinted _ _
    | .place _ _ | .proj _ _ | .index _ _ _ => evalSub env n Γ ρ e

/-- A place's steps from a value of type `t`, as `stepsF` types them. -/
def evalSteps (env : InferEnv) : Nat → Ctx → VEnv → Ty → Val → List Step → EvalM Val
  | 0, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, _, v, [] => pure v
  | n + 1, Γ, ρ, t, v, [st] => evalStep env n Γ ρ t v st
  | n + 1, Γ, ρ, t, v, st :: rest => do
    let u ← typeOf (stepF env n Γ.through t st)
    let w ← evalStep env n Γ.through ρ t v st
    evalSteps env n Γ ρ u w rest

/-- A step from a value of type `t`, as `stepF` types it. -/
def evalStep (env : InferEnv) : Nat → Ctx → VEnv → Ty → Val → Step → EvalM Val
  | 0, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, _, v, .field s => fieldVal v s
  | _ + 1, _, _, _, _, .index _ none => throw .wrong
  | n + 1, Γ, ρ, t, v, .index k (some m) => do
    let (_, kt) ← match indexTypes t with
      | .ok p => pure p
      | .error _ => throw .wrong
    let kv ← evalC env n Γ ρ k kt
    indexVal m v kv

/-- What `e` evaluates to, as `subsumeF` synthesizes it. -/
def evalSub (env : InferEnv) : Nat → Ctx → VEnv → Expr → EvalM Val
  | 0, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, e => evalS env n Γ ρ e

/-- Two operands, as `pairF` types them: their type, and their values
checked against it. -/
def evalPair (env : InferEnv) : Nat → Ctx → VEnv → Expr → Expr → Option Ty → EvalM (Ty × Val × Val)
  | 0, _, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, a, b, expected => do
    let t ← typeOf (pairF env (n + 1) Γ a b expected)
    let va ← evalC env n Γ.borrow ρ a t
    let vb ← evalC env n Γ.borrow ρ b t
    pure (t, va, vb)

def evalPairs (env : InferEnv) : Nat → Ctx → VEnv → List Expr → List Ty → EvalM (List Val)
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, e :: es, t :: ts => do
    let v ← evalC env n Γ ρ e t
    let vs ← evalPairs env n Γ ρ es ts
    pure (v :: vs)
  | _ + 1, _, _, _, _ => pure []

def evalFields (env : InferEnv) : Nat → Ctx → VEnv → List (String × Expr) → List (String × Ty) →
    EvalM (List (String × Val))
  | 0, _, _, _, _ => throw .fuel
  | n + 1, Γ, ρ, (m, e) :: fs, (_, t) :: ts => do
    let v ← evalC env n Γ ρ e t
    let vs ← evalFields env n Γ ρ fs ts
    pure ((m, v) :: vs)
  | _ + 1, _, _, _, _ => pure []

def evalAll (env : InferEnv) : Nat → Ctx → VEnv → List Expr → Ty → EvalM (List Val)
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, [], _ => pure []
  | n + 1, Γ, ρ, e :: es, t => do
    let v ← evalC env n Γ ρ e t
    let vs ← evalAll env n Γ ρ es t
    pure (v :: vs)

def evalEntries (env : InferEnv) : Nat → Ctx → VEnv → List (Expr × Expr) → Ty → Ty →
    EvalM (List (Val × Val))
  | 0, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, [], _, _ => pure []
  | n + 1, Γ, ρ, (ke, ve) :: es, k, v => do
    let kv ← evalC env n Γ ρ ke k
    let vv ← evalC env n Γ ρ ve v
    let rest ← evalEntries env n Γ ρ es k v
    pure ((kv, vv) :: rest)

def evalRows (env : InferEnv) : Nat → Ctx → VEnv → List (List Expr) → List (String × Ty) →
    EvalM (List (List Val))
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, [], _ => pure []
  | n + 1, Γ, ρ, r :: rs, cols => do
    let vs ← evalPairs env n Γ ρ r (cols.map (·.2))
    let rest ← evalRows env n Γ ρ rs cols
    pure (vs :: rest)

end

end Datalove.Fun
