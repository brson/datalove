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

This first stage evaluates literals, variables, hints, operators, `?`, `!`
and `@`. Calls, places, fields and indexes are `Stop.unsupported`. `@`
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
  /-- A form this stage does not evaluate: a call, a place, a field or an
  index. -/
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
    | .call _ _ | .place _ _ | .proj _ _ | .index _ _ _ => throw .unsupported
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
