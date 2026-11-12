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

Everything is `partial`, being built on `Lit`'s inference, and serves as the
oracle for `crates/datalove-formal-tests/tests/fun_tests.rs`.
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

/-- The forms datafun shares with datalit, which datalit's rules type. -/
def isLiteralForm : Expr → Bool
  | .var _ | .binop .. | .unop .. | .tryOpt _ | .tryRes _ | .adapt _ | .call .. | .hinted .. => false
  | _ => true

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

mutual

/-- `Γ ⊢ e ⇒ T`. -/
partial def synth (Γ : Ctx) (e : Expr) : ChkM Ty := do
  match e with
  | .hinted h _ =>
    let t ← liftE (convertHint h)
    check Γ e t
    pure t
  | .var x => match Γ.lookup x with
    | Option.some (t, _) => pure t
    | Option.none => throw "F001"
  | .binop op a b =>
    let t ← pair Γ a b none
    finishBinop Γ op t
  | .unop .neg a =>
    let t ← synth Γ a
    if isFloatT t || t == .int then pure t else throw "F026"
  | .unop .not a =>
    let t ← synth Γ a
    if t == .bool then pure t else throw "F026"
  | .unop .negC a =>
    let t ← synth Γ a
    if !isFixT t then throw "F026"
    if !Γ.returnsResult then throw "F049"
    pure t
  | .unop .negO a =>
    let t ← synth Γ a
    if !isFixT t then throw "F026"
    if !Γ.returnsOption then throw "F049"
    pure t
  | .tryOpt a =>
    let t ← synth Γ a
    match t.expose with
    | .option p => if Γ.returnsOption then pure p else throw "F049"
    | _ => throw "F048"
  | .tryRes a =>
    let t ← synth Γ a
    match t.expose with
    | .result p => if Γ.returnsResult then pure p else throw "F049"
    | _ => throw "F048"
  | .adapt a => synth Γ a
  | .call f args => synthCall Γ f args
  | e =>
    let t ← liftE (inferType Γ e)
    check Γ e t
    pure t

/-- `Γ ⊢ e₁ ~ e₂ ⇒ T` (Pair): the operands inferred together, each given
its default, the two equal (else F090), each checked against the type. With
an expected type (Check-BinOp), each operand is unified with it first. -/
partial def pair (Γ : Ctx) (a b : Expr) (expected : Option Ty) : ChkM Ty := do
  let r : Except Code (Ty × Ty) := (do
    let ta ← infer Γ a
    let tb ← infer Γ b
    if let Option.some t := expected then
      unify ta (.known t)
      unify tb (.known t)
    unify ta tb
    pure (← resolve ta, ← resolve tb)).run' #[]
  let (ta, tb) ← liftE r
  if ta != tb then throw "F090"
  check Γ a ta
  check Γ b ta
  pure ta

/-- An operator's type, given its operands': Syn-Arith, Syn-Ord, Syn-Eq,
Syn-Logic. -/
partial def finishBinop (Γ : Ctx) (op : BinOp) (t : Ty) : ChkM Ty := do
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

/-- Syn-Call: the function's signature, the argument count (F045), and each
argument in order: a marker that disagrees with its mode is reported (F057)
and checking goes on, and the first argument not of its parameter's type
stops the call. -/
partial def synthCall (Γ : Ctx) (f : String) (args : List (Option Mode × Expr)) : ChkM Ty := do
  let Option.some sig := Γ.sigs.lookup f | throw "F002"
  if args.length != sig.params.length then throw "F045"
  for ((marker, e), (mode, t)) in args.zip sig.params do
    let wanted := if mode == .in_ then Option.none else Option.some mode
    if marker != wanted then report "F057"
    check Γ e t
  match sig.ret with
  | .ty r => pure r
  | .void => pure unitTy

/-- `Γ ⊢ e ⇐ T`, by the doc's rule selection. -/
partial def check (Γ : Ctx) (e : Expr) (expected : Ty) : ChkM Unit := do
  let e ← match e with
    | .hinted h inner => do
      let hinted ← liftE (convertHint h)
      if !hinted.meet expected then throw "T040"
      pure inner
    | e => pure e
  let exposed := expected.expose
  match e, exposed with
  -- Datalit's rules, with the context added.
  | .none_, .option _ => pure ()
  | .some_ p, .option t => check Γ p t
  | .ok p, .result t => check Γ p t
  | .er p, .result _ => check Γ p .error
  | .none_, _ | .er _, _ => throw "F016"
  | .tt, _ | .ff, _ => if Ty.bool == expected then pure () else throw "F016"
  | .str, _ => if Ty.string == expected then pure () else throw "F016"
  | .int n, t =>
    if t == .int then pure ()
    else match intRange t with
      | Option.some (lo, hi, code) => if lo ≤ n && n ≤ hi then pure () else throw code
      | Option.none => subsume Γ e expected
  | .float, .f32 | .float, .f64 => pure ()
  | .hex n, t =>
    if t == .int then pure ()
    else match hexBits t with
      | Option.some (bits, code) => if n < 2 ^ bits then pure () else throw code
      | Option.none => subsume Γ e expected
  | .tuple es, .tuple ts =>
    let ts := tysList ts
    if es.length != ts.length then throw "T038"
    for (e, t) in es.zip ts do check Γ e t
  | .struct fs, .struct ts =>
    let ts := fieldsList ts
    if fs.length != ts.length then throw "T039"
    for ((n, e), (m, t)) in fs.zip ts do
      if n != m then throw "T042"
      check Γ e t
  | .list es, .list t | .set es, .set t => for e in es do check Γ e t
  | .map es, .map k v => for (ke, ve) in es do check Γ ke k; check Γ ve v
  | .tensor shape es, .tensor t rank =>
    if shape.length != rank then throw "T048"
    if es.length != shape.foldl (· * ·) 1 then throw "T049"
    for e in es do check Γ e t
  | .table header rows, .table cols =>
    let cols := fieldsList cols
    if header.length != cols.length then throw "T054"
    if header != cols.map (·.1) then throw "T055"
    for row in rows do
      if row.length != cols.length then throw "T056"
      for (e, (_, t)) in row.zip cols do check Γ e t
  | .atom a, .atom b => if a == b then pure () else throw "T057"
  | .atom a, .enum vs => if Lit.Variants.atomListed a vs then pure () else throw "T057"
  | .term a p, .term b t => if a == b then check Γ p t else throw "T057"
  | .term a p, .enum vs =>
    match Lit.Variants.termPayload a vs with
    | Option.some t => check Γ p t
    | Option.none => throw "T057"
  | .enum inner, .enum _ => check Γ inner expected
  | .enum _, _ => throw "T058"
  | .data p, .data | .error p, .error => do let _ ← synth Γ p; pure ()
  -- Datafun's.
  | .unop .neg (.int n), t =>
    match intRange t with
    | Option.some (lo, hi, code) => if lo ≤ -n && -n ≤ hi then pure () else throw code
    | Option.none => if isFloatT t then check Γ (.int n) t else subsume Γ e expected
  | .unop .neg a, t => if isFloatT t then check Γ a t else subsume Γ e expected
  | .binop op a b, _ =>
    if yieldsOperand op then
      let t ← pair Γ a b (Option.some expected)
      let r ← finishBinop Γ op t
      if r != expected then throw "F016"
    else subsume Γ e expected
  | .adapt a, _ =>
    let t ← synth Γ a
    if !t.canAdapt expected then throw "F091"
  | e, _ => if isLiteralForm e then literalMismatch Γ e else subsume Γ e expected

/-- A literal form with no rule for the expected head: datalit's rule
selection, under which it fails without being synthesized, as a plain
mismatch (F016), unless inferring it finds a malformed hint first. -/
partial def literalMismatch (Γ : Ctx) (e : Expr) : ChkM Unit := do
  let _ ← liftE ((infer Γ e).run' #[])
  throw "F016"

/-- Check-Subsume: what the expression synthesizes meets the expected type at
the head, or F016. -/
partial def subsume (Γ : Ctx) (e : Expr) (expected : Ty) : ChkM Unit := do
  let t ← synth Γ e
  if !t.meet expected then throw "F016"

end

/-! ## Statements -/

/-- Run an expression's check, reporting what it threw. -/
def runChk {α} (m : ChkM α) : StateM (Array Code) (Option α) := do
  match ← m.run with
  | .ok a => pure (Option.some a)
  | .error c => modify (·.push c); pure Option.none

/-- `completes(s̄)`: whether control can reach the end, by the doc's rules. -/
partial def completes : List Stmt → Bool
  | [] => true
  | s :: ss => completes1 s && completes ss
where
  completes1 : Stmt → Bool
    | .ret _ | .brk | .cont => false
    | .if_ _ _ _ _ Option.none => true
    | .if_ _ _ _ t (Option.some e) => completes t || completes e
    | .loop (Option.some _) _ => true
    | .loop Option.none body => hasBreak body
    | _ => true
  /-- A `break` belonging to this loop: not one inside a nested loop. -/
  hasBreak : List Stmt → Bool
    | [] => false
    | .brk :: _ => true
    | .if_ _ _ _ t e :: ss => hasBreak t || (e.map hasBreak).getD false || hasBreak ss
    | _ :: ss => hasBreak ss

def convertReporting (h : Hint) : StateM (Array Code) (Option Ty) :=
  runChk (liftE (convertHint h))

mutual

/-- `Γ ⊢ s ⊣ Γ'`, reporting what is wrong. -/
partial def checkStmt (Γ : Ctx) : Stmt → StateM (Array Code) Ctx
  | .let_ x hint e => bindChecked Γ x hint e .let_
  | .var_ x hint (Option.some e) => bindChecked Γ x hint e .var_
  | .var_ x (Option.some hint) Option.none => do
    match ← convertReporting hint with
    | Option.some t => pure (Γ.bind x t .var_)
    | Option.none => pure Γ
  | .var_ _ Option.none Option.none => pure Γ
  | .set_ x op e => do
    match Γ.lookup x with
    | Option.none => modify (·.push "F054")
    | Option.some (t, k) =>
      -- An immutable root is reported, and the value is still checked.
      if k == .let_ then modify (·.push "F055")
      match op with
      | Option.none => let _ ← runChk (check Γ e t)
      | Option.some op =>
        let _ ← runChk (do
          check Γ e t
          if !arithAdmits op t then throw "F026"
          if !fits Γ (needs op) then throw "F049")
    pure Γ
  | .fun_ _ => pure Γ
  | .ret e => do
    match Γ.ret, e with
    | .void, Option.some _ => modify (·.push "F052")
    | .ty _, Option.none => modify (·.push "F053")
    | .ty r, Option.some e => let _ ← runChk (check Γ e r)
    | .void, Option.none => pure ()
    pure Γ
  | .if_ c Option.none Option.none t e => do
    let _ ← runChk (check Γ c .bool)
    checkBlock Γ t
    if let Option.some e := e then checkBlock Γ e
    pure Γ
  | .if_ c thenB elseB t e => do
    match ← runChk (synth Γ c) with
    -- A condition with no type binds nothing to check the bodies under, and
    -- they are left unchecked.
    | Option.none => pure ()
    | Option.some ct =>
      match ct.expose with
      -- An option has no error to bind: F094, after the body that binds the
      -- payload and before the one that cannot bind the error.
      | .option p =>
        checkBlock (bindOpt Γ thenB p) t
        if elseB.isSome then modify (·.push "F094")
        if let Option.some e := e then checkBlock Γ e
      -- A result needs its error bound (F046), and without it the bodies are
      -- left unchecked.
      | .result p =>
        if elseB.isNone then modify (·.push "F046")
        else
          checkBlock (bindOpt Γ thenB p) t
          if let Option.some e := e then checkBlock (bindOpt Γ elseB .error) e
      -- Nothing to bind, so the bodies are left unchecked, as for a
      -- condition with no type.
      | _ => modify (·.push "F093")
    pure Γ
  | .loop c body => do
    if let Option.some c := c then let _ ← runChk (check Γ c .bool)
    checkBlock { Γ with inLoop := true } body
    pure Γ
  | .brk => do
    if !Γ.inLoop then modify (·.push "F050")
    pure Γ
  | .cont => do
    if !Γ.inLoop then modify (·.push "F051")
    pure Γ
  | .debuglog e => do
    let _ ← runChk (synth Γ e)
    pure Γ
  | .call e => do
    if let Option.some t ← runChk (synth Γ e) then
      if t != unitTy then modify (·.push "F095")
    pure Γ

/-- A `let` or `var` with a value. A hint is the type the name is bound at
whatever the value does; with none, a value that does not synthesize leaves
the name unbound. -/
partial def bindChecked (Γ : Ctx) (x : String) (hint : Option Hint) (e : Expr) (k : Kind) :
    StateM (Array Code) Ctx := do
  match hint with
  | Option.some h =>
    match ← convertReporting h with
    | Option.none => pure Γ
    | Option.some t =>
      let _ ← runChk (check Γ e t)
      pure (Γ.bind x t k)
  | Option.none =>
    match ← runChk (synth Γ e) with
    | Option.some t => pure (Γ.bind x t k)
    | Option.none => pure Γ

partial def bindOpt (Γ : Ctx) (x : Option String) (t : Ty) : Ctx :=
  match x with
  | Option.some x => Γ.bind x t .let_
  | Option.none => Γ

/-- A block: what it binds does not escape it. -/
partial def checkBlock (Γ : Ctx) (ss : List Stmt) : StateM (Array Code) Unit := do
  let mut Γ := Γ
  for s in ss do Γ ← checkStmt Γ s

end

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
  | .mk name params _ body => do
    let Option.some sig := sigs.lookup name | pure ()
    let vars := (params.zip sig.params).reverse.map fun ((m, x, _), (_, t)) => (x, t, kindOf m)
    checkBlock { vars, ret := sig.ret, sigs } body
    if let .ty _ := sig.ret then
      if completes body then modify (·.push "F063")

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
