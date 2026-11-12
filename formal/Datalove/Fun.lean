import Datalove.Lit

/-!
# Datafun's statements and expressions

A first slice of `botdocs/datafun-typing-rules.md`, written from the doc:
scripts of functions and statements, with no generics, modules, consts,
patterns, projections, indexing, `for` or `match`. A script's top level is a
body returning `!()`; a function's body sees its parameters and nothing else
the script binds.

Expressions are datalit's literal forms, which datafun types by datalit's
rules with a context added, and datafun's own: variables, operators, `?`,
`!`, `@` and calls. Inference is datalit's (`Lit.lean`), extended by the
doc's table.

The checker reports what is wrong and goes on, as the Rust does, so the
answer for a script is every diagnostic code in order. An expression that
cannot be typed reports one code and stops (`throw`); a few rules report and
go on (`report`), as a call goes on to its next argument. A failed `let` with
a hint binds its name at the hint's type; one without binds nothing.

Inference is `partial`, being `Lit`'s. Checking and synthesis are total:
they take inference as a parameter (`InferEnv`) and run on fuel, which a
check that runs out of fails. So `Proofs/EarlyReturn.lean` and
`Proofs/Completes.lean` can reason about them, and with inference given they
are the oracle for `crates/datalove-formal-tests/tests/fun_tests.rs`.
-/

namespace Datalove.Fun

open Datalove.Lit (PTy InferM fresh prune unify resolve convertHint Code intRange hexBits
  tysList fieldsList)

inductive BinOp where
  | add | sub | mul | div
  | addC | subC | mulC | divC
  | addO | subO | mulO | divO
  | lt | gt | le | ge | eq | ne
  | and | or | xor
  deriving DecidableEq, Repr, Inhabited

inductive UnOp where
  | neg | negO | negC | not
  deriving DecidableEq, Repr, Inhabited

inductive Mode where
  | in_ | out | ref | mut
  deriving DecidableEq, Repr, Inhabited

inductive Expr where
  | tt | ff
  | int (n : Int)
  | hex (n : Nat)
  | float
  | str
  | none_
  | tuple (es : List Expr)
  | struct (fs : List (String × Expr))
  | list (es : List Expr)
  | set (es : List Expr)
  | map (es : List (Expr × Expr))
  | tensor (shape : List Nat) (es : List Expr)
  | table (header : List String) (rows : List (List Expr))
  | some_ (e : Expr)
  | ok (e : Expr)
  | er (e : Expr)
  | data (e : Expr)
  | error (e : Expr)
  | atom (a : String)
  | term (a : String) (e : Expr)
  | enum (e : Expr)
  | hinted (h : Hint) (e : Expr)
  | var (x : String)
  | binop (op : BinOp) (a b : Expr)
  | unop (op : UnOp) (a : Expr)
  | tryOpt (a : Expr)
  | tryRes (a : Expr)
  | adapt (a : Expr)
  /-- A call, each argument with the mode marker written before it. -/
  | call (f : String) (args : List (Option Mode × Expr))
  deriving Inhabited

mutual

inductive Stmt where
  | let_ (x : String) (hint : Option Hint) (e : Expr)
  | var_ (x : String) (hint : Option Hint) (e : Option Expr)
  | set_ (x : String) (op : Option BinOp) (e : Expr)
  | fun_ (f : FunDecl)
  | ret (e : Option Expr)
  /-- `if c ... else ... end if`, binding the payload's name and, for a
  result, the error's, when given. -/
  | if_ (c : Expr) (thenB elseB : Option String) (t : List Stmt) (e : Option (List Stmt))
  | loop (c : Option Expr) (body : List Stmt)
  | brk
  | cont
  | debuglog (e : Expr)
  /-- `call f(...)`, perhaps with `?` or `!` on it. -/
  | call (e : Expr)
  /-- `for x̄ in ref p ... end for`. -/
  | for_ (names : List String) (coll : Expr) (body : List Stmt)
  deriving Inhabited

inductive FunDecl where
  | mk (name : String) (params : List (Mode × String × Hint)) (ret : Option Hint)
    (body : List Stmt)
  deriving Inhabited

end

/-! ## Contexts -/

inductive Kind where
  | let_ | var_
  deriving DecidableEq

/-- What `ret` returns to: a function with a type, or one with none (`void`). A
script's top level returns `!()`. -/
inductive Ret where
  | void
  | ty (t : Ty)

structure Sig where
  params : List (Mode × Ty)
  ret : Ret

structure Ctx where
  /-- Innermost first. -/
  vars : List (String × Ty × Kind) := []
  ret : Ret
  inLoop : Bool := false
  sigs : List (String × Sig)

def Ctx.lookup (Γ : Ctx) (x : String) : Option (Ty × Kind) :=
  (Γ.vars.find? (·.1 == x)).map (·.2)

def Ctx.bind (Γ : Ctx) (x : String) (t : Ty) (k : Kind) : Ctx :=
  { Γ with vars := (x, t, k) :: Γ.vars }

def unitTy : Ty := .tuple .nil

/-! ## Classes of types -/

def isFixT (t : Ty) : Bool := t.isFixedInt
def isFloatT (t : Ty) : Bool := t.isFloat

/-- `Arith(⊕)`, by the doc's table. -/
def arithAdmits : BinOp → Ty → Bool
  | .add, t | .sub, t | .mul, t => isFloatT t || t == .int
  | .div, t => isFloatT t
  | .addC, t | .subC, t | .mulC, t | .addO, t | .subO, t | .mulO, t => isFixT t
  | .divC, t | .divO, t => isFixT t || t == .int
  | _, _ => false

/-- Whether an operator yields its operand type: the arithmetic ones. -/
def yieldsOperand : BinOp → Bool
  | .add | .sub | .mul | .div | .addC | .subC | .mulC | .divC
  | .addO | .subO | .mulO | .divO => true
  | _ => false

/-- What an operator's early return needs the function to return. -/
inductive Needs where
  | nothing | result | option

def needs : BinOp → Needs
  | .addC | .subC | .mulC | .divC => .result
  | .addO | .subO | .mulO | .divO => .option
  | _ => .nothing

def Ctx.returnsResult (Γ : Ctx) : Bool :=
  match Γ.ret with
  | .ty t => match t.expose with
    | .result _ => true
    | _ => false
  | .void => false

def Ctx.returnsOption (Γ : Ctx) : Bool :=
  match Γ.ret with
  | .ty t => match t.expose with
    | .option _ => true
    | _ => false
  | .void => false

/-- `fits(⊕, Γ)`. -/
def fits (Γ : Ctx) : Needs → Bool
  | .nothing => true
  | .result => Γ.returnsResult
  | .option => Γ.returnsOption

mutual

/-- The `Eq` class. -/
partial def isEqT : Ty → Bool
  | .bool | .string | .atom _ | .int => true
  | .tuple ts => (tysList ts).all isEqT
  | .struct fs => (fieldsList fs).all (isEqT ·.2)
  | .option t | .term _ t => isEqT t
  | .enum vs => (Lit.variantsP vs).all fun (_, p) => match p with
    | Option.none => true
    | Option.some (.known t) => isEqT t
    | Option.some _ => false
  | t => t.isNumeric

end

/-! ## Inference -/

/-- A type's outermost part, with a known type opened one level. -/
def shallow (t : PTy) : InferM PTy := do
  match ← prune t with
  | .known k => pure ((Lit.expand k.expose).getD (.known k))
  | other => pure other

mutual

/-- `⊢ e ⇝ τ`: datalit's rules for the literal forms, and the doc's table for
datafun's. Nothing here fails but a malformed hint. -/
partial def infer (Γ : Ctx) : Expr → InferM PTy
  | .hinted h _ => do pure (.known (← StateT.lift (convertHint h)))
  | .tt | .ff => pure (.known .bool)
  | .str => pure (.known .string)
  | .int _ => fresh .int
  | .hex _ => fresh .hex
  | .float => fresh .float
  | .tuple es => .tuple <$> es.mapM (infer Γ)
  | .struct fs => .struct <$> fs.mapM fun (n, e) => do pure (n, ← infer Γ e)
  | .list es => .list <$> together Γ es
  | .set es => .set <$> together Γ es
  | .tensor shape es => (.tensor · shape.length) <$> together Γ es
  | .map es => do
    let k ← fresh .open_
    let v ← fresh .open_
    for (ke, ve) in es do
      unify k (← infer Γ ke)
      unify v (← infer Γ ve)
    pure (.map k v)
  | .table header rows => do
    let cols ← header.mapM fun _ => fresh .open_
    for row in rows do
      for (e, col) in row.zip cols do
        unify col (← infer Γ e)
    pure (.table (header.zip cols))
  | .some_ e => .option <$> infer Γ e
  | .ok e => .result <$> infer Γ e
  | .none_ => .option <$> fresh .open_
  | .er _ => .result <$> fresh .open_
  | .data _ => pure (.known .data)
  | .error _ => pure (.known .error)
  | .atom a => fresh (.variant a Option.none)
  | .term a e => do fresh (.variant a (Option.some (← infer Γ e)))
  | .enum _ => fresh .open_
  | .var x => match Γ.lookup x with
    | Option.some (t, _) => pure (.known t)
    | Option.none => StateT.lift (throw "F001")
  | .binop op a b => do
    let ta ← infer Γ a
    let tb ← infer Γ b
    unify ta tb
    if yieldsOperand op then pure ta else pure (.known .bool)
  | .unop .not _ => pure (.known .bool)
  | .unop _ a => infer Γ a
  | .tryOpt a => do
    match ← shallow (← infer Γ a) with
    | .option p => pure p
    | _ => fresh .open_
  | .tryRes a => do
    match ← shallow (← infer Γ a) with
    | .result p => pure p
    | _ => fresh .open_
  | .adapt a => do
    let t ← infer Γ a
    -- With nothing to say what it converts to, `@` keeps its operand's type.
    let s ← get
    match (resolve t).run s with
    | .ok (from_, s') => set s'; fresh (.adapt from_)
    | .error _ => pure t
  | .call f _ => match Γ.sigs.lookup f with
    | Option.some sig => match sig.ret with
      | .ty r => pure (.known r)
      | .void => pure (.known unitTy)
    | Option.none => fresh .open_

partial def together (Γ : Ctx) (es : List Expr) : InferM PTy := do
  let t ← fresh .open_
  for e in es do unify t (← infer Γ e)
  pure t

end

/-! ## Checking and synthesis -/

/-- The checker's monad: a code thrown stops the expression; a code reported
does not. -/
abbrev ChkM := ExceptT Code (StateM (Array Code))

def report (c : Code) : ChkM Unit := modify (·.push c)

def liftE {α} (x : Except Code α) : ChkM α :=
  match x with
  | .ok a => pure a
  | .error c => throw c

/-- Datalit's codes for a plain mismatch are datafun's F016. -/
def plain (c : Code) : Code := if c == "T022" || c == "T032" then "F016" else c

/-- Infer an expression and give each undecided variable its default (T016
for one with none). -/
def inferType (Γ : Ctx) (e : Expr) : Except Code Ty :=
  (do let p ← infer Γ e; resolve p).run' #[]

/-- Infer two operands together, unifying each first with an expected type
if there is one, and give each its default: the inference half of Pair. -/
def pairTypes (Γ : Ctx) (a b : Expr) (expected : Option Ty) : Except Code (Ty × Ty) :=
  (do
    let ta ← infer Γ a
    let tb ← infer Γ b
    if let Option.some t := expected then
      unify ta (.known t)
      unify tb (.known t)
    unify ta tb
    pure (← resolve ta, ← resolve tb)).run' #[]

/-- What checking takes from inference: an expression's type (`inferType`),
two operands' (`pairTypes`), and whether inferring finds a malformed hint.
Given as parameters, so that checking is a total function; inference only
says what types are, and never where an early return may be, so what
`Proofs/EarlyReturn.lean` shows of checking holds whatever it says. -/
structure InferEnv where
  ty : Ctx → Expr → Except Code Ty
  pair : Ctx → Expr → Expr → Option Ty → Except Code (Ty × Ty)
  probe : Ctx → Expr → Except Code Unit

/-- Inference as `Lit.lean` has it. -/
def inferEnv : InferEnv where
  ty := inferType
  pair := pairTypes
  probe Γ e := do let _ ← (infer Γ e).run' #[]

/-- An operator's type, given its operands': Syn-Arith, Syn-Ord, Syn-Eq,
Syn-Logic. -/
def finishBinop (Γ : Ctx) (op : BinOp) (t : Ty) : ChkM Ty := do
  match op with
  | .lt | .gt | .le | .ge =>
    if t.isNumeric then pure .bool else throw "F026"
  | .eq | .ne =>
    if isEqT t then pure .bool else throw "F026"
  | .and | .or | .xor =>
    if t == .bool then pure .bool else throw "F026"
  | op =>
    if !arithAdmits op t then throw "F026"
    if !fits Γ (needs op) then throw "F049"
    pure t

/-- A literal form with no rule for the expected head: datalit's rule
selection, under which it fails without being synthesized, as a plain
mismatch (F016), unless inferring it finds a malformed hint first. -/
def literalMismatch (env : InferEnv) (Γ : Ctx) (e : Expr) : ChkM Unit := do
  liftE (env.probe Γ e)
  throw "F016"

/-- Enough fuel for any expression or body a test writes: every call of the
checker spends one, and running out is an error, never a success. -/
def fuel : Nat := 1000000

mutual

/-- `Γ ⊢ e ⇒ T`. -/
def synthF (env : InferEnv) : Nat → Ctx → Expr → ChkM Ty
  | 0, _, _ => throw "fuel"
  | n + 1, Γ, e => match e with
    | .hinted h inner => do
      let t ← liftE (convertHint h)
      checkForm env n Γ inner t
      pure t
    | .var x => match Γ.lookup x with
      | Option.some (t, _) => pure t
      | Option.none => throw "F001"
    | .binop op a b => do
      let t ← pairF env n Γ a b Option.none
      finishBinop Γ op t
    | .unop .neg a => do
      let t ← synthF env n Γ a
      if isFloatT t || t == .int then pure t else throw "F026"
    | .unop .not a => do
      let t ← synthF env n Γ a
      if t == .bool then pure t else throw "F026"
    | .unop .negC a => do
      let t ← synthF env n Γ a
      if !isFixT t then throw "F026"
      if !Γ.returnsResult then throw "F049"
      pure t
    | .unop .negO a => do
      let t ← synthF env n Γ a
      if !isFixT t then throw "F026"
      if !Γ.returnsOption then throw "F049"
      pure t
    | .tryOpt a => do
      let t ← synthF env n Γ a
      match t.expose with
      | .option p => if Γ.returnsOption then pure p else throw "F049"
      | _ => throw "F048"
    | .tryRes a => do
      let t ← synthF env n Γ a
      match t.expose with
      | .result p => if Γ.returnsResult then pure p else throw "F049"
      | _ => throw "F048"
    | .adapt a => synthF env n Γ a
    | .call f args => do
      let Option.some sig := Γ.sigs.lookup f | throw "F002"
      if args.length != sig.params.length then throw "F045"
      argsF env n Γ args sig.params
      match sig.ret with
      | .ty r => pure r
      | .void => pure unitTy
    | e => do
      let t ← liftE (env.ty Γ e)
      checkForm env n Γ e t
      pure t

/-- Pair: the operands' types, which must be one (else F090), and each
checked against it. -/
def pairF (env : InferEnv) : Nat → Ctx → Expr → Expr → Option Ty → ChkM Ty
  | 0, _, _, _, _ => throw "fuel"
  | n + 1, Γ, a, b, expected => do
    let (ta, tb) ← liftE (env.pair Γ a b expected)
    if ta != tb then throw "F090"
    checkF env n Γ a ta
    checkF env n Γ b ta
    pure ta

/-- A call's arguments in order: a marker that disagrees with its mode is
reported (F057) and checking goes on, and the first argument not of its
parameter's type stops the call. -/
def argsF (env : InferEnv) : Nat → Ctx → List (Option Mode × Expr) → List (Mode × Ty) → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, (marker, e) :: args, (mode, t) :: params => do
    let wanted := if mode == .in_ then Option.none else Option.some mode
    if marker != wanted then report "F057"
    checkF env n Γ e t
    argsF env n Γ args params
  | _ + 1, _, _, _ => pure ()

/-- `Γ ⊢ e ⇐ T`: a hint first (Check-Hinted), then the rule for the form. -/
def checkF (env : InferEnv) : Nat → Ctx → Expr → Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, .hinted h inner, expected => do
    let hinted ← liftE (convertHint h)
    if !hinted.meet expected then throw "T040"
    checkForm env n Γ inner expected
  | n + 1, Γ, e, expected => checkForm env n Γ e expected

/-- The rule for an expression's form and the expected type's head, unrolled
(Check-Roll); a literal form with none fails (`literalMismatch`), and any
other is checked by Check-Subsume. Matched on the form first, then the head,
as the doc's rule selection reads. -/
def checkForm (env : InferEnv) : Nat → Ctx → Expr → Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, e, expected =>
    match e with
    -- Datalit's rules, with the context added.
    | .none_ => match expected.expose with
      | .option _ => pure ()
      | _ => throw "F016"
    | .er p => match expected.expose with
      | .result _ => checkF env n Γ p .error
      | _ => throw "F016"
    | .some_ p => match expected.expose with
      | .option t => checkF env n Γ p t
      | _ => literalMismatch env Γ e
    | .ok p => match expected.expose with
      | .result t => checkF env n Γ p t
      | _ => literalMismatch env Γ e
    | .tt | .ff => if Ty.bool == expected then pure () else throw "F016"
    | .str => if Ty.string == expected then pure () else throw "F016"
    | .int k =>
      let t := expected.expose
      if t == .int then pure ()
      else match intRange t with
        | Option.some (lo, hi, code) => if lo ≤ k && k ≤ hi then pure () else throw code
        | Option.none => literalMismatch env Γ e
    | .float => match expected.expose with
      | .f32 | .f64 => pure ()
      | _ => literalMismatch env Γ e
    | .hex k =>
      let t := expected.expose
      if t == .int then pure ()
      else match hexBits t with
        | Option.some (bits, code) => if k < 2 ^ bits then pure () else throw code
        | Option.none => literalMismatch env Γ e
    | .tuple es => match expected.expose with
      | .tuple ts =>
        if es.length != (tysList ts).length then throw "T038"
        else pairsF env n Γ es (tysList ts)
      | _ => literalMismatch env Γ e
    | .struct fs => match expected.expose with
      | .struct ts =>
        if fs.length != (fieldsList ts).length then throw "T039"
        else fieldsF env n Γ fs (fieldsList ts)
      | _ => literalMismatch env Γ e
    | .list es => match expected.expose with
      | .list t => allF env n Γ es t
      | _ => literalMismatch env Γ e
    | .set es => match expected.expose with
      | .set t => allF env n Γ es t
      | _ => literalMismatch env Γ e
    | .map es => match expected.expose with
      | .map k v => entriesF env n Γ es k v
      | _ => literalMismatch env Γ e
    | .tensor shape es => match expected.expose with
      | .tensor t rank =>
        if shape.length != rank then throw "T048"
        else if es.length != shape.foldl (· * ·) 1 then throw "T049"
        else allF env n Γ es t
      | _ => literalMismatch env Γ e
    | .table header rows => match expected.expose with
      | .table cols =>
        if header.length != (fieldsList cols).length then throw "T054"
        else if header != (fieldsList cols).map (·.1) then throw "T055"
        else rowsF env n Γ rows (fieldsList cols)
      | _ => literalMismatch env Γ e
    | .atom a => match expected.expose with
      | .atom b => if a == b then pure () else throw "T057"
      | .enum vs => if Lit.Variants.atomListed a vs then pure () else throw "T057"
      | _ => literalMismatch env Γ e
    | .term a p => match expected.expose with
      | .term b t => if a == b then checkF env n Γ p t else throw "T057"
      | .enum vs => match Lit.Variants.termPayload a vs with
        | Option.some t => checkF env n Γ p t
        | Option.none => throw "T057"
      | _ => literalMismatch env Γ e
    | .enum inner => match expected.expose with
      | .enum _ => checkF env n Γ inner expected
      | _ => throw "T058"
    | .data p => match expected.expose with
      | .data => do let _ ← synthF env n Γ p; pure ()
      | _ => literalMismatch env Γ e
    | .error p => match expected.expose with
      | .error => do let _ ← synthF env n Γ p; pure ()
      | _ => literalMismatch env Γ e
    -- Datafun's.
    | .unop .neg a =>
      let t := expected.expose
      match a with
      | .int k => match intRange t with
        | Option.some (lo, hi, code) => if lo ≤ -k && -k ≤ hi then pure () else throw code
        | Option.none => if isFloatT t then checkF env n Γ a t else subsumeF env n Γ e expected
      | _ => if isFloatT t then checkF env n Γ a t else subsumeF env n Γ e expected
    | .binop op a b =>
      if yieldsOperand op then do
        let t ← pairF env n Γ a b (Option.some expected)
        let r ← finishBinop Γ op t
        if r != expected then throw "F016"
      else subsumeF env n Γ e expected
    | .adapt a => do
      let t ← synthF env n Γ a
      if !t.canAdapt expected then throw "F091"
    | .unop _ _ | .var _ | .tryOpt _ | .tryRes _ | .call _ _ | .hinted _ _ =>
      subsumeF env n Γ e expected

/-- Check-Subsume: what the expression synthesizes meets the expected type at
the head, or F016. -/
def subsumeF (env : InferEnv) : Nat → Ctx → Expr → Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, e, expected => do
    let t ← synthF env n Γ e
    if !t.meet expected then throw "F016"

def pairsF (env : InferEnv) : Nat → Ctx → List Expr → List Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, e :: es, t :: ts => do checkF env n Γ e t; pairsF env n Γ es ts
  | _ + 1, _, _, _ => pure ()

def fieldsF (env : InferEnv) : Nat → Ctx → List (String × Expr) → List (String × Ty) → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, (m, e) :: fs, (k, t) :: ts => do
    if m != k then throw "T042"
    checkF env n Γ e t
    fieldsF env n Γ fs ts
  | _ + 1, _, _, _ => pure ()

def allF (env : InferEnv) : Nat → Ctx → List Expr → Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | _ + 1, _, [], _ => pure ()
  | n + 1, Γ, e :: es, t => do checkF env n Γ e t; allF env n Γ es t

def entriesF (env : InferEnv) : Nat → Ctx → List (Expr × Expr) → Ty → Ty → ChkM Unit
  | 0, _, _, _, _ => throw "fuel"
  | _ + 1, _, [], _, _ => pure ()
  | n + 1, Γ, (ke, ve) :: es, k, v => do
    checkF env n Γ ke k
    checkF env n Γ ve v
    entriesF env n Γ es k v

def rowsF (env : InferEnv) : Nat → Ctx → List (List Expr) → List (String × Ty) → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | _ + 1, _, [], _ => pure ()
  | n + 1, Γ, r :: rs, cols => do
    if r.length != cols.length then throw "T056"
    pairsF env n Γ r (cols.map (·.2))
    rowsF env n Γ rs cols

end

def synth (Γ : Ctx) (e : Expr) : ChkM Ty := synthF inferEnv fuel Γ e
def check (Γ : Ctx) (e : Expr) (t : Ty) : ChkM Unit := checkF inferEnv fuel Γ e t

/-! ## Statements -/

/-- Run an expression's check, reporting what it threw. -/
def runChk {α} (m : ChkM α) : StateM (Array Code) (Option α) := do
  -- The check runs on a fresh list, whose reports are added after, so that
  -- what a statement adds says whether its checks succeeded.
  let (r, reported) := m.run.run #[]
  modify (· ++ reported)
  match r with
  | .ok a => pure (Option.some a)
  | .error c => modify (·.push c); pure Option.none

mutual

/-- `completes(s̄)`: whether control can reach the end, by the doc's rules.
Total, so that `Proofs/Completes.lean` can show it sound. -/
def completes : List Stmt → Bool
  | [] => true
  | s :: ss => completesS s && completes ss

def completesS : Stmt → Bool
  | .ret _ | .brk | .cont => false
  | .if_ _ _ _ _ Option.none => true
  | .if_ _ _ _ t (Option.some e) => completes t || completes e
  | .loop (Option.some _) _ => true
  | .loop Option.none body => hasBreak body
  | _ => true

/-- Whether a body holds a `break` belonging to the loop it is the body of:
not one inside a nested loop. -/
def hasBreak : List Stmt → Bool
  | [] => false
  | s :: ss => hasBreakS s || hasBreak ss

def hasBreakS : Stmt → Bool
  | .brk => true
  | .if_ _ _ _ t e => hasBreak t || hasBreakElse e
  | _ => false

def hasBreakElse : Option (List Stmt) → Bool
  | Option.none => false
  | Option.some e => hasBreak e

end

def convertReporting (h : Hint) : StateM (Array Code) (Option Ty) :=
  runChk (liftE (convertHint h))

def bindOpt (Γ : Ctx) (x : Option String) (t : Ty) : Ctx :=
  match x with
  | Option.some x => Γ.bind x t .let_
  | Option.none => Γ

/-- A `let` or `var` with a value. A hint is the type the name is bound at
whatever the value does; with none, a value that does not synthesize leaves
the name unbound. -/
def bindChecked (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (hint : Option Hint) (e : Expr)
    (k : Kind) : StateM (Array Code) Ctx := do
  match hint with
  | Option.some h =>
    match ← convertReporting h with
    | Option.none => pure Γ
    | Option.some t =>
      let _ ← runChk (checkF env n Γ e t)
      pure (Γ.bind x t k)
  | Option.none =>
    match ← runChk (synthF env n Γ e) with
    | Option.some t => pure (Γ.bind x t k)
    | Option.none => pure Γ

/-- What a `for` binds walking a collection, by the doc's table: an element,
or an index and an element, of a list or tensor; an element of a set; a key
and a value of a map. Any other count of names is F109, any other type F087. -/
def walk (c : Ty) (names : List String) : Except Code (List (String × Ty)) :=
  let elem (t : Ty) (rank : Nat) : Ty := if rank ≤ 1 then t else .tensor t (rank - 1)
  match c, names with
  | .list t, [x] => pure [(x, t)]
  | .list t, [i, x] => pure [(i, .index), (x, t)]
  | .tensor t r, [x] => pure [(x, elem t r)]
  | .tensor t r, [i, x] => pure [(i, .index), (x, elem t r)]
  | .set t, [x] => pure [(x, t)]
  | .map k v, [a, b] => pure [(a, k), (b, v)]
  | .list _, _ | .tensor _ _, _ | .set _, _ | .map _ _, _ => throw "F109"
  | _, _ => throw "F087"

/-- A `for`'s bindings: its collection a place (else F108), of a type it
walks. With none, the body is left unchecked. -/
def forBindings (env : InferEnv) (n : Nat) (Γ : Ctx) (names : List String) (coll : Expr) :
    StateM (Array Code) (Option (List (String × Ty))) :=
  match coll with
  | .var _ => runChk (do
      let t ← synthF env n Γ coll
      liftE (walk t.expose names))
  | _ => modify (·.push "F108") >>= fun _ => pure Option.none

def Ctx.bindAll (Γ : Ctx) (Δ : List (String × Ty)) : Ctx :=
  Δ.foldl (fun Γ (x, t) => Γ.bind x t .let_) Γ

/-- Report `c` when `cond`. -/
def reportIf (cond : Bool) (c : Code) : StateM (Array Code) Unit :=
  if cond then modify (·.push c) else pure ()

/-- `set x = e`, or `set x ⊕= e`: the root (F054, F055), and the value
checked against it, an immutable root's included. -/
def checkSet (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (op : Option BinOp) (e : Expr) :
    StateM (Array Code) Unit :=
  match Γ.lookup x with
  | Option.none => modify (·.push "F054")
  | Option.some (t, k) =>
    reportIf (k == .let_) "F055" >>= fun _ =>
    match op with
    | Option.none => runChk (checkF env n Γ e t) >>= fun _ => pure ()
    | Option.some op =>
      runChk (do
        checkF env n Γ e t
        if !arithAdmits op t then throw "F026"
        if !fits Γ (needs op) then throw "F049") >>= fun _ => pure ()

/-- `ret` and `ret e` (F052, F053, Ret). -/
def checkRet (env : InferEnv) (n : Nat) (Γ : Ctx) (e : Option Expr) : StateM (Array Code) Unit :=
  match Γ.ret, e with
  | .void, Option.some _ => modify (·.push "F052")
  | .ty _, Option.none => modify (·.push "F053")
  | .ty r, Option.some e => runChk (checkF env n Γ e r) >>= fun _ => pure ()
  | .void, Option.none => pure ()

mutual

/-- `Γ ⊢ s ⊣ Γ'`, reporting what is wrong. Written with its binds explicit,
so that `Proofs/EarlyReturn.lean` can follow each step. -/
def checkStmtF (env : InferEnv) : Nat → Ctx → Stmt → StateM (Array Code) Ctx
  | 0, Γ, _ => modify (·.push "fuel") >>= fun _ => pure Γ
  | n + 1, Γ, s => match s with
    | .let_ x hint e => bindChecked env n Γ x hint e .let_
    | .var_ x hint (Option.some e) => bindChecked env n Γ x hint e .var_
    | .var_ x (Option.some hint) Option.none =>
      convertReporting hint >>= fun r => match r with
      | Option.some t => pure (Γ.bind x t .var_)
      | Option.none => pure Γ
    | .var_ _ Option.none Option.none => pure Γ
    | .set_ x op e => checkSet env n Γ x op e >>= fun _ => pure Γ
    | .fun_ _ => pure Γ
    | .ret e => checkRet env n Γ e >>= fun _ => pure Γ
    | .if_ c Option.none Option.none t e =>
      runChk (checkF env n Γ c .bool) >>= fun _ =>
      checkBlockF env n Γ t >>= fun _ =>
      checkElse env n Γ e >>= fun _ =>
      pure Γ
    | .if_ c thenB elseB t e =>
      runChk (synthF env n Γ c) >>= fun r =>
      (match r with
      -- A condition with no type binds nothing to check the bodies under, and
      -- they are left unchecked.
      | Option.none => pure ()
      | Option.some ct =>
        match ct.expose with
        -- An option has no error to bind: F094, after the body that binds the
        -- payload and before the one that cannot bind the error.
        | .option p =>
          checkBlockF env n (bindOpt Γ thenB p) t >>= fun _ =>
          reportIf elseB.isSome "F094" >>= fun _ =>
          checkElse env n Γ e
        -- A result needs its error bound (F046), and without it the bodies are
        -- left unchecked.
        | .result p =>
          if elseB.isNone then modify (·.push "F046")
          else
            checkBlockF env n (bindOpt Γ thenB p) t >>= fun _ =>
            checkElse env n (bindOpt Γ elseB .error) e
        -- Nothing to bind, so the bodies are left unchecked, as for a
        -- condition with no type.
        | _ => modify (·.push "F093")) >>= fun _ =>
      pure Γ
    | .loop c body =>
      (match c with
      | Option.some c => runChk (checkF env n Γ c .bool) >>= fun _ => pure ()
      | Option.none => pure ()) >>= fun _ =>
      checkBlockF env n { Γ with inLoop := true } body >>= fun _ =>
      pure Γ
    | .brk => reportIf (!Γ.inLoop) "F050" >>= fun _ => pure Γ
    | .cont => reportIf (!Γ.inLoop) "F051" >>= fun _ => pure Γ
    | .debuglog e => runChk (synthF env n Γ e) >>= fun _ => pure Γ
    | .call e =>
      runChk (synthF env n Γ e) >>= fun r =>
      (match r with
      | Option.some t => reportIf (t != unitTy) "F095"
      | Option.none => pure ()) >>= fun _ =>
      pure Γ
    | .for_ names coll body =>
      forBindings env n Γ names coll >>= fun r =>
      (match r with
      | Option.some Δ => checkBlockF env n ({ Γ with inLoop := true }.bindAll Δ) body
      -- A collection with nothing to walk binds nothing, and the body is left
      -- unchecked.
      | Option.none => pure ()) >>= fun _ =>
      pure Γ

/-- An `else` body, if there is one. -/
def checkElse (env : InferEnv) : Nat → Ctx → Option (List Stmt) → StateM (Array Code) Unit
  | n, Γ, Option.some e => checkBlockF env n Γ e
  | _, _, Option.none => pure ()

/-- A block: what it binds does not escape it. -/
def checkBlockF (env : InferEnv) : Nat → Ctx → List Stmt → StateM (Array Code) Unit
  | 0, _, _ => modify (·.push "fuel")
  | _ + 1, _, [] => pure ()
  | n + 1, Γ, s :: ss => checkStmtF env n Γ s >>= fun Γ' => checkBlockF env n Γ' ss

end

def checkStmt (Γ : Ctx) (s : Stmt) : StateM (Array Code) Ctx := checkStmtF inferEnv fuel Γ s
def checkBlock (Γ : Ctx) (ss : List Stmt) : StateM (Array Code) Unit := checkBlockF inferEnv fuel Γ ss

/-! ## Scripts -/

/-- A function's signature, or the code of what is wrong with it. -/
def sigOf : FunDecl → Except Code Sig
  | .mk _ params ret _ => do
    let ps ← params.mapM fun (m, _, h) => do pure (m, ← convertHint h)
    let r ← match ret with
      | Option.some h => Ret.ty <$> convertHint h
      | Option.none => pure .void
    pure { params := ps, ret := r }

def kindOf : Mode → Kind
  | .in_ | .ref => .let_
  | .mut | .out => .var_

def FunDecl.name : FunDecl → String
  | .mk n _ _ _ => n

/-- A function's body under its parameters (Fun), and F063 if it has a
return type and can reach its end. A function whose signature is not well
formed is not in `sigs`, and its body is not checked. -/
def checkFun (sigs : List (String × Sig)) : FunDecl → StateM (Array Code) Unit
  | .mk name params _ body =>
    match sigs.lookup name with
    | Option.none => pure ()
    | Option.some sig =>
      let vars := (params.zip sig.params).reverse.map fun ((m, x, _), (_, t)) => (x, t, kindOf m)
      checkBlock { vars, ret := sig.ret, sigs } body >>= fun _ =>
      reportIf (match sig.ret with
        | .ty _ => completes body
        | .void => false) "F063"

/-- The diagnostics of a script, in order. Signatures are collected first, so
that functions may call each other in any order; a second function of a name
is F086, and the first stays. The top level is a body returning `!()`. -/
def checkScript (ss : List Stmt) : Array Code := Id.run do
  let mut sigs : List (String × Sig) := []
  let mut diags : Array Code := #[]
  for s in ss do
    if let .fun_ f := s then
      if (sigs.lookup f.name).isSome then
        diags := diags.push "F086"
      else match sigOf f with
        | .ok sig => sigs := sigs ++ [(f.name, sig)]
        | .error c => diags := diags.push c
  let Γ : Ctx := { ret := .ty (.result unitTy), sigs }
  let run : StateM (Array Code) Unit := do
    let mut Γ := Γ
    for s in ss do
      if let .fun_ f := s then checkFun sigs f
      else Γ ← checkStmt Γ s
  (run.run diags).2

end Datalove.Fun
