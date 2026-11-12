import Datalove.Lit
import Datalove.Classes

/-!
# Datafun's statements and expressions

A first slice of `botdocs/datafun-typing-rules.md`, written from the doc:
scripts of functions and statements, with no generics, modules, consts,
patterns or `match`. A script's top level is a
body returning `!()`; a function's body sees its parameters and nothing else
the script binds.

Expressions are datalit's literal forms, which datafun types by datalit's
rules with a context added, and datafun's own: variables, places with field
and index steps, fields and indexes of what is not a place, operators, `?`,
`!`, `@` and calls. Inference is datalit's (`Lit.lean`), extended by the
doc's table. What it cannot read is of the `unknown` class, and a literal
form whose type comes to one is synthesized from its parts (`partsF`).

The context says whether an expression is in a borrowed position, where a
non-copy part may be read (F070, F072), and whether it is a `mut` or `out`
argument, which no view may be (F073).

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

open Datalove.Lit (PTy InferM fresh prune unify resolve convertHint Code unknownCode intRange hexBits
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

/-- A field selector: a struct's field by name, a tuple's by position. -/
inductive Sel where
  | name (n : String)
  | pos (i : Nat)
  deriving DecidableEq, Inhabited

/-- How an index returns early when there is nothing at the key: `?` with
`none`, `!` with an error. -/
inductive IdxMode where
  | opt
  | res
  deriving DecidableEq, Inhabited

mutual

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
  /-- A variable with steps into it: `v.a`, `v.0`, `v[k]?`. -/
  | place (root : String) (steps : List Step)
  /-- A field of what is not a place: `f().a`. -/
  | proj (base : Expr) (sel : Sel)
  /-- An index into what is not a place: `f()[k]?`, `f()[k]!`, or `f()[k]`
  with neither, which only a `set` target may write. -/
  | index (base key : Expr) (mode : Option IdxMode)
  deriving Inhabited

/-- A step of a place: a field, or an index with how it returns early, which
only a `set` target's last step may leave out. -/
inductive Step where
  | field (s : Sel)
  | index (k : Expr) (mode : Option IdxMode)
  deriving Inhabited

end

mutual

inductive Stmt where
  | let_ (x : String) (hint : Option Hint) (e : Expr)
  | var_ (x : String) (hint : Option Hint) (e : Option Expr)
  | set_ (x : String) (steps : List Step) (op : Option BinOp) (e : Expr)
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
  /-- Whether this is a borrowed position, where a non-copy part may be read
  without being moved out (F070, F072). -/
  borrowed : Bool := false
  /-- Whether this is an argument passed `mut` or `out`, which no tensor view
  may be (F073). -/
  mutArg : Bool := false

/-- A borrowed position: an operand, the operand of `@`, `debuglog`'s
argument, a compound `set`'s value, a `for`'s collection. -/
def Ctx.borrow (Γ : Ctx) : Ctx := { Γ with borrowed := true }

/-- An argument's position, by its parameter's mode. -/
def Ctx.forArg (Γ : Ctx) (m : Mode) : Ctx :=
  { Γ with borrowed := m != .in_, mutArg := m == .mut || m == .out }

def Ctx.lookup (Γ : Ctx) (x : String) : Option (Ty × Kind) :=
  (Γ.vars.find? (·.1 == x)).map (·.2)

def Ctx.bind (Γ : Ctx) (x : String) (t : Ty) (k : Kind) : Ctx :=
  { Γ with vars := (x, t, k) :: Γ.vars }

def unitTy : Ty := .tuple .nil

/-- A field's type (Syn-TupleProj, Syn-FieldProj): F069 for a tuple position
out of range, F067 for a struct field it lacks, F068 for anything without
fields of that kind. -/
def fieldType (t : Ty) (s : Sel) : Except Code Ty :=
  match t.expose, s with
  | .tuple ts, .pos i => match (tysList ts)[i]? with
    | Option.some t => pure t
    | Option.none => throw "F069"
  | .struct fs, .name n => match (fieldsList fs).lookup n with
    | Option.some t => pure t
    | Option.none => throw "F067"
  | _, _ => throw "F068"

/-- An index's element type and key type, by the doc's table, or F103. -/
def indexTypes (t : Ty) : Except Code (Ty × Ty) :=
  match t.expose with
  | .list e => pure (e, .index)
  | .map k v => pure (v, k)
  | .tensor e r => pure (if r ≤ 1 then e else .tensor e (r - 1), .index)
  | _ => throw "F103"

/-- Whether indexing makes a view: a tensor of rank above 1. -/
def isView (t : Ty) : Bool :=
  match t.expose with
  | .tensor _ r => r > 1
  | _ => false

/-- Whether an expression builds a value of its parts, which a borrowed
position does not reach into: the parts are moved into what is built. -/
def buildsFromParts : Expr → Bool
  | .tuple _ | .struct _ | .list _ | .set _ | .map _ | .tensor _ _ | .table _ _
  | .some_ _ | .ok _ | .er _ | .data _ | .error _ | .term _ _ => true
  | .hinted _ e => buildsFromParts e
  | _ => false

/-- Whether an expression is a place: a variable, with steps into it or not. -/
def isPlace : Expr → Bool
  | .var _ | .place _ _ => true
  | .proj b _ => isPlace b
  | _ => false

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

/-- A field's type, as far as inference can read it. -/
def inferField (b : PTy) (s : Sel) : InferM PTy := do
  match ← shallow b, s with
  | .known t, s => match fieldType t s with
    | .ok t => pure (.known t)
    | .error _ => fresh .unknown
  | .tuple ts, .pos i => match ts[i]? with
    | Option.some t => pure t
    | Option.none => fresh .unknown
  | .struct fs, .name n => match fs.lookup n with
    | Option.some t => pure t
    | Option.none => fresh .unknown
  | _, _ => fresh .unknown

/-- An element's type, as far as inference can read it. -/
def elementOf (b : PTy) : InferM PTy := do
  match ← shallow b with
  | .list e => pure e
  | .map _ v => pure v
  | .tensor e r => pure (if r ≤ 1 then e else .tensor e (r - 1))
  | _ => fresh .unknown

/-- The variable a collection's elements share: open, for the collection to
be reported, when there are none. -/
def sharedVar (empty : Bool) : InferM PTy :=
  fresh (if empty then .open_ else .unknown)

mutual

/-- `⊢ e ⇝ τ`: datalit's rules for the literal forms, and the doc's table for
datafun's. Nothing here fails: what cannot be read is `unknown`. -/
partial def infer (Γ : Ctx) : Expr → InferM PTy
  | .hinted h _ => match convertHint h with
    | .ok t => pure (.known t)
    | .error _ => fresh .unknown
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
    let k ← together Γ (es.map (·.1))
    let v ← together Γ (es.map (·.2))
    pure (.map k v)
  | .table header rows => do
    let cols ← (List.range header.length).mapM fun i =>
      together Γ (rows.filterMap (·[i]?))
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
    | Option.none => fresh .unknown
  | .binop op a b => do
    let ta ← infer Γ a
    let tb ← infer Γ b
    unify ta tb
    if yieldsOperand op then pure ta else pure (.known .bool)
  | .unop .not _ => pure (.known .bool)
  | .unop _ a => infer Γ a
  | .index b _ (Option.some _) => do elementOf (← infer Γ b)
  | .index _ _ Option.none => fresh .unknown
  | .place x steps => match Γ.lookup x with
    | Option.none => fresh .unknown
    | Option.some (t, _) =>
      steps.foldlM (init := .known t) fun t st => match st with
        | .field s => inferField t s
        | .index _ _ => elementOf t
  | .proj b s => do inferField (← infer Γ b) s
  | .tryOpt a => do
    match ← shallow (← infer Γ a) with
    | .option p => pure p
    | _ => fresh .unknown
  | .tryRes a => do
    match ← shallow (← infer Γ a) with
    | .result p => pure p
    | _ => fresh .unknown
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
    | Option.none => fresh .unknown

/-- Expressions that must share a type, and that type. -/
partial def together (Γ : Ctx) (es : List Expr) : InferM PTy := do
  let t ← sharedVar es.isEmpty
  for e in es do unify t (← infer Γ e)
  pure t

end

/-! ## Checking and synthesis -/

/-- A computation that adds to a log of diagnostics and never takes from it:
what a composite logs is what its parts log, in order. -/
structure Log (α : Type) where
  log : Array Code
  val : α

instance : Monad Log where
  pure a := ⟨#[], a⟩
  bind x f := let y := f x.val; ⟨x.log ++ y.log, y.val⟩

/-- The checker's monad: a code thrown stops the expression; a code reported
does not. -/
abbrev ChkM := ExceptT Code Log

def report (c : Code) : ChkM Unit := ExceptT.lift ⟨#[c], ()⟩

def liftE {α} (x : Except Code α) : ChkM α :=
  match x with
  | .ok a => pure a
  | .error c => throw c

/-- A check whose failure is reported and gone on from. -/
def recover (m : ChkM Unit) : ChkM Unit :=
  ExceptT.mk ⟨m.run.log ++ (match m.run.val with
    | .ok _ => #[]
    | .error c => #[c]), .ok ()⟩

/-- Infer an expression and give each undecided variable its default (T016
for one with none, `unknownCode` for one inference could not read). -/
def inferType (Γ : Ctx) (e : Expr) : Except Code Ty :=
  (do let p ← infer Γ e; resolve p).run' #[]

/-- Infer two operands together, unifying each first with an expected type
if there is one, and give each its default: the inference half of Pair. -/
def pairTypes (Γ : Ctx) (a b : Expr) (expected : Option Ty) : Except Code Ty × Except Code Ty :=
  let joint : InferM (PTy × PTy) := do
    let ta ← infer Γ a
    let tb ← infer Γ b
    if let Option.some t := expected then
      unify ta (.known t)
      unify tb (.known t)
    unify ta tb
    pure (ta, tb)
  match joint.run #[] with
  | .ok ((ta, tb), s) => ((resolve ta).run' s, (resolve tb).run' s)
  | .error c => (.error c, .error c)

/-- What checking takes from inference: an expression's type (`inferType`)
and two operands' (`pairTypes`). Given as parameters, so that checking is a
total function; inference only says what types are, and never where an early
return may be, so what `Proofs/EarlyReturn.lean` shows of checking holds
whatever it says. -/
structure InferEnv where
  ty : Ctx → Expr → Except Code Ty
  pair : Ctx → Expr → Expr → Option Ty → Except Code Ty × Except Code Ty

/-- Inference as `Lit.lean` has it. -/
def inferEnv : InferEnv where
  ty := inferType
  pair := pairTypes

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

/-- Whether the function returns what an index's `?` or `!` returns early. -/
def Ctx.fitsMode (Γ : Ctx) : IdxMode → Bool
  | .opt => Γ.returnsOption
  | .res => Γ.returnsResult

/-- The context an expression is checked in: a borrowed position ends at one
that builds a value of its parts. -/
def Ctx.enter (Γ : Ctx) (e : Expr) : Ctx :=
  { Γ with borrowed := Γ.borrowed && !buildsFromParts e }

/-- The context of a place's step that is not its last, which only leads to
what is read. -/
def Ctx.through (Γ : Ctx) : Ctx := { Γ with borrowed := true, mutArg := false }

def tysOf : List Ty → Tys
  | [] => .nil
  | t :: ts => .cons t (tysOf ts)

def fieldsOf : List (String × Ty) → Fields
  | [] => .nil
  | (n, t) :: fs => .cons n t (fieldsOf fs)

/-- Enough fuel for any expression or body a test writes: every call of the
checker spends one, and running out is an error, never a success. -/
def fuel : Nat := 1000000

mutual

/-- `Γ ⊢ e ⇒ T`. -/
def synthF (env : InferEnv) : Nat → Ctx → Expr → ChkM Ty
  | 0, _, _ => throw "fuel"
  | n + 1, Γ₀, e =>
    let Γ := Γ₀.enter e
    match e with
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
      let t ← synthF env n Γ.borrow a
      if isFloatT t || t == .int then pure t else throw "F026"
    | .unop .not a => do
      let t ← synthF env n Γ.borrow a
      if t == .bool then pure t else throw "F026"
    | .unop .negC a => do
      let t ← synthF env n Γ.borrow a
      if !isFixT t then throw "F026"
      if !Γ.returnsResult then throw "F049"
      pure t
    | .unop .negO a => do
      let t ← synthF env n Γ.borrow a
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
    | .adapt a => synthF env n Γ.borrow a
    | .call f args => do
      let Option.some sig := Γ.sigs.lookup f | throw "F002"
      if args.length != sig.params.length then throw "F045"
      argsF env n Γ args sig.params
      match sig.ret with
      | .ty r => pure r
      | .void => pure unitTy
    | .place x steps => match Γ.lookup x with
      | Option.some (t, _) => stepsF env n Γ t steps
      | Option.none => throw "F001"
    -- Syn-FieldProj. A field of a temporary is free to move out; one of a
    -- place is moved out of what holds it (F070).
    | .proj b s => do
      let bt ← synthF env n Γ b
      let t ← liftE (fieldType bt s)
      if isPlace b && !t.isCopy && !Γ.borrowed then throw "F070"
      pure t
    -- Syn-Index, for what is not a place. Its element is a temporary's, and
    -- may be moved out (no F072).
    | .index b k mode => match mode with
      | Option.none => throw "F104"
      | Option.some m => do
        let bt ← synthF env n Γ b
        let (t, kt) ← liftE (indexTypes bt)
        recover (checkF env n Γ k kt)
        if Γ.mutArg && isView bt then throw "F073"
        if !Γ.fitsMode m then throw "F049"
        pure t
    -- A literal form: its type is what inference finds for its parts
    -- together, and where it could not read one, what synthesizing them
    -- finds (`partsF`).
    | e => do
      let t ← match env.ty Γ e with
        | .ok t => pure t
        | .error c => if c == unknownCode then partsF env n Γ e else throw c
      checkForm env n Γ e t
      pure t

/-- A place's steps from a type, each but the last in a borrowed position
that no view is wrong in (`Ctx.through`): only the last is what is read. -/
def stepsF (env : InferEnv) : Nat → Ctx → Ty → List Step → ChkM Ty
  | 0, _, _, _ => throw "fuel"
  | _ + 1, _, t, [] => pure t
  | n + 1, Γ, t, [st] => stepF env n Γ t st
  | n + 1, Γ, t, st :: rest => do
    let t ← stepF env n Γ.through t st
    stepsF env n Γ t rest

/-- A step of a place being read: a field (Syn-FieldProj, Syn-TupleProj), or
an index (Syn-Index), whose key is reported and gone on from. A part not
copied is moved out of what holds it, which only a borrowed position may
leave in place (F070, F072). -/
def stepF (env : InferEnv) : Nat → Ctx → Ty → Step → ChkM Ty
  | 0, _, _, _ => throw "fuel"
  | _ + 1, Γ, t, .field s => do
    let ft ← liftE (fieldType t s)
    if !ft.isCopy && !Γ.borrowed then throw "F070"
    pure ft
  | _ + 1, _, _, .index _ Option.none => throw "F104"
  | n + 1, Γ, t, .index k (Option.some m) => do
    let (et, kt) ← liftE (indexTypes t)
    recover (checkF env n Γ k kt)
    if Γ.mutArg && isView t then throw "F073"
    if !et.isCopy && !Γ.borrowed then throw "F072"
    if !Γ.fitsMode m then throw "F049"
    pure et

/-- Pair: the operands' types, which must be one (else F090), and each
checked against it, as operands are, borrowed. -/
def pairF (env : InferEnv) : Nat → Ctx → Expr → Expr → Option Ty → ChkM Ty
  | 0, _, _, _, _ => throw "fuel"
  | n + 1, Γ, a, b, expected => do
    let (ra, rb) := env.pair Γ a b expected
    let ta ← operandF env n Γ a ra
    let tb ← operandF env n Γ b rb
    if ta != tb then throw "F090"
    checkF env n Γ.borrow a ta
    checkF env n Γ.borrow b ta
    pure ta

/-- An operand's inferred type, or what synthesizing it finds where inference
could not read it. -/
def operandF (env : InferEnv) : Nat → Ctx → Expr → Except Code Ty → ChkM Ty
  | 0, _, _, _ => throw "fuel"
  | _ + 1, _, _, .ok t => pure t
  | n + 1, Γ, e, .error c => if c == unknownCode then synthF env n Γ.borrow e else throw c

/-- A literal form's type from its parts, synthesized in order, each of a
collection's like its first (F016): what the Rust's `synthesize_unhinted`
does with one inference could not read. A form that leaves a type open is
T016, as are the forms that never get here. -/
def partsF (env : InferEnv) : Nat → Ctx → Expr → ChkM Ty
  | 0, _, _ => throw "fuel"
  | n + 1, Γ, e => match e with
    | .tt | .ff => pure .bool
    | .str => pure .string
    | .int _ | .hex _ => pure .int
    | .float => pure .f64
    | .atom a => pure (.atom a)
    | .tuple es => do
      let ts ← synthAllF env n Γ es
      pure (.tuple (tysOf ts))
    | .struct fs => do
      let ts ← synthAllF env n Γ (fs.map (·.2))
      pure (.struct (fieldsOf ((fs.map (·.1)).zip ts)))
    | .list (x :: xs) => do
      let t ← synthF env n Γ x
      likeF env n Γ xs t
      pure (.list t)
    | .set (x :: xs) => do
      let t ← synthF env n Γ x
      likeF env n Γ xs t
      pure (.set t)
    | .tensor shape (x :: xs) => do
      let t ← synthF env n Γ x
      likeF env n Γ xs t
      pure (.tensor t shape.length)
    | .map ((k, v) :: es) => do
      let kt ← synthF env n Γ k
      let vt ← synthF env n Γ v
      entriesLikeF env n Γ es kt vt
      pure (.map kt vt)
    | .table header (r :: rs) => do
      if r.length != header.length then throw "D030"
      let ts ← synthAllF env n Γ r
      rowsLikeF env n Γ header.length rs ts
      pure (.table (fieldsOf (header.zip ts)))
    | .table [] [] => pure (.table .nil)
    | .some_ p => .option <$> synthF env n Γ p
    | .ok p => .result <$> synthF env n Γ p
    | .term a p => .term a <$> synthF env n Γ p
    | .data p => do let _ ← synthF env n Γ p; pure .data
    | .error p => do let _ ← synthF env n Γ p; pure .error
    | _ => throw "T016"

def synthAllF (env : InferEnv) : Nat → Ctx → List Expr → ChkM (List Ty)
  | 0, _, _ => throw "fuel"
  | _ + 1, _, [] => pure []
  | n + 1, Γ, e :: es => do
    let t ← synthF env n Γ e
    let ts ← synthAllF env n Γ es
    pure (t :: ts)

/-- Each expression synthesized, and like `t`. -/
def likeF (env : InferEnv) : Nat → Ctx → List Expr → Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | _ + 1, _, [], _ => pure ()
  | n + 1, Γ, e :: es, t => do
    let et ← synthF env n Γ e
    if !et.meet t then throw "F016"
    likeF env n Γ es t

def entriesLikeF (env : InferEnv) : Nat → Ctx → List (Expr × Expr) → Ty → Ty → ChkM Unit
  | 0, _, _, _, _ => throw "fuel"
  | _ + 1, _, [], _, _ => pure ()
  | n + 1, Γ, (k, v) :: es, kt, vt => do
    let kt' ← synthF env n Γ k
    let vt' ← synthF env n Γ v
    if !kt'.meet kt then throw "F016"
    if !vt'.meet vt then throw "F016"
    entriesLikeF env n Γ es kt vt

def rowsLikeF (env : InferEnv) : Nat → Ctx → Nat → List (List Expr) → List Ty → ChkM Unit
  | 0, _, _, _, _ => throw "fuel"
  | _ + 1, _, _, [], _ => pure ()
  | n + 1, Γ, width, r :: rs, ts => do
    if r.length != width then throw "D030"
    rowLikeF env n Γ r ts
    rowsLikeF env n Γ width rs ts

def rowLikeF (env : InferEnv) : Nat → Ctx → List Expr → List Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, e :: es, t :: ts => do
    let et ← synthF env n Γ e
    if !et.meet t then throw "F016"
    rowLikeF env n Γ es ts
  | _ + 1, _, _, _ => pure ()

/-- A call's arguments in order, each in the position its parameter's mode
makes it: a marker that disagrees with its mode is reported (F057) and
checking goes on, and the first argument not of its parameter's type stops
the call. -/
def argsF (env : InferEnv) : Nat → Ctx → List (Option Mode × Expr) → List (Mode × Ty) → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, (marker, e) :: args, (mode, t) :: params => do
    let wanted := if mode == .in_ then Option.none else Option.some mode
    if marker != wanted then report "F057"
    checkF env n (Γ.forArg mode) e t
    argsF env n Γ args params
  | _ + 1, _, _, _ => pure ()

/-- `Γ ⊢ e ⇐ T`: a hint first (Check-Hinted), then the rule for the form. -/
def checkF (env : InferEnv) : Nat → Ctx → Expr → Ty → ChkM Unit
  | 0, _, _, _ => throw "fuel"
  | n + 1, Γ, .hinted h inner, expected => do
    let hinted ← liftE (convertHint h)
    if !hinted.meet expected then throw "T040"
    checkForm env n (Γ.enter (.hinted h inner)) inner expected
  | n + 1, Γ, e, expected => checkForm env n (Γ.enter e) e expected

/-- The rule for an expression's form and the expected type's head, unrolled
(Check-Roll); a literal form with none is a mismatch (F016), and any other is
checked by Check-Subsume. Matched on the form first, then the head, as the
doc's rule selection reads. -/
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
      | _ => throw "F016"
    | .ok p => match expected.expose with
      | .result t => checkF env n Γ p t
      | _ => throw "F016"
    | .tt | .ff => if Ty.bool == expected then pure () else throw "F016"
    | .str => if Ty.string == expected then pure () else throw "F016"
    | .int k =>
      let t := expected.expose
      if t == .int then pure ()
      else match intRange t with
        | Option.some (lo, hi, code) => if lo ≤ k && k ≤ hi then pure () else throw code
        | Option.none => throw "F016"
    | .float => match expected.expose with
      | .f32 | .f64 => pure ()
      | _ => throw "F016"
    | .hex k =>
      let t := expected.expose
      if t == .int then pure ()
      else match hexBits t with
        | Option.some (bits, code) => if k < 2 ^ bits then pure () else throw code
        | Option.none => throw "F016"
    | .tuple es => match expected.expose with
      | .tuple ts =>
        if es.length != (tysList ts).length then throw "T038"
        else pairsF env n Γ es (tysList ts)
      | _ => throw "F016"
    | .struct fs => match expected.expose with
      | .struct ts =>
        if fs.length != (fieldsList ts).length then throw "T039"
        else fieldsF env n Γ fs (fieldsList ts)
      | _ => throw "F016"
    | .list es => match expected.expose with
      | .list t => allF env n Γ es t
      | _ => throw "F016"
    | .set es => match expected.expose with
      | .set t => allF env n Γ es t
      | _ => throw "F016"
    | .map es => match expected.expose with
      | .map k v => entriesF env n Γ es k v
      | _ => throw "F016"
    | .tensor shape es => match expected.expose with
      | .tensor t rank =>
        if shape.length != rank then throw "T048"
        else if es.length != shape.foldl (· * ·) 1 then throw "T049"
        else allF env n Γ es t
      | _ => throw "F016"
    | .table header rows => match expected.expose with
      | .table cols =>
        if header.length != (fieldsList cols).length then throw "T054"
        else if header != (fieldsList cols).map (·.1) then throw "T055"
        else rowsF env n Γ rows (fieldsList cols)
      | _ => throw "F016"
    | .atom a => match expected.expose with
      | .atom b => if a == b then pure () else throw "T057"
      | .enum vs => if Lit.Variants.atomListed a vs then pure () else throw "T057"
      | _ => throw "F016"
    | .term a p => match expected.expose with
      | .term b t => if a == b then checkF env n Γ p t else throw "T057"
      | .enum vs => match Lit.Variants.termPayload a vs with
        | Option.some t => checkF env n Γ p t
        | Option.none => throw "T057"
      | _ => throw "F016"
    | .enum inner => match expected.expose with
      | .enum _ => checkF env n Γ inner expected
      | _ => throw "T058"
    | .data p => match expected.expose with
      | .data => do let _ ← synthF env n Γ p; pure ()
      | _ => throw "F016"
    | .error p => match expected.expose with
      | .error => do let _ ← synthF env n Γ p; pure ()
      | _ => throw "F016"
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
      let t ← synthF env n Γ.borrow a
      if !t.canAdapt expected then throw "F091"
    | .unop _ _ | .var _ | .tryOpt _ | .tryRes _ | .call _ _ | .hinted _ _
    | .place _ _ | .proj _ _ | .index _ _ _ =>
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
def runChk {α} (m : ChkM α) : StateM (Array Code) (Option α) :=
  let r := m.run
  modify (· ++ r.log) >>= fun _ =>
  match r.val with
  | .ok a => pure (Option.some a)
  | .error c => modify (·.push c) >>= fun _ => pure Option.none

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

/-- A `for`'s bindings: its collection a place (else F108), read borrowed, of
a type it walks. With none, the body is left unchecked. -/
def forBindings (env : InferEnv) (n : Nat) (Γ : Ctx) (names : List String) (coll : Expr) :
    StateM (Array Code) (Option (List (String × Ty))) :=
  if isPlace coll then
    runChk (do
      let t ← synthF env n Γ.borrow coll
      liftE (walk t.expose names))
  else modify (·.push "F108") >>= fun _ => pure Option.none

def Ctx.bindAll (Γ : Ctx) (Δ : List (String × Ty)) : Ctx :=
  Δ.foldl (fun Γ (x, t) => Γ.bind x t .let_) Γ

/-- Report `c` when `cond`. -/
def reportIf (cond : Bool) (c : Code) : StateM (Array Code) Unit :=
  if cond then modify (·.push c) else pure ()

/-- What a `set` writes: the value checked against the place's type, and for
a compound `set x ⊕= e`, read as an operand is, borrowed, and the operator
one the type has (F026) and the function fits (F049). Each is reported and
gone on from. -/
def setValue (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (op : Option BinOp) (e : Expr) :
    StateM (Array Code) Unit :=
  match op with
  | Option.none => runChk (checkF env n Γ e t) >>= fun _ => pure ()
  | Option.some op =>
    runChk (checkF env n Γ.borrow e t) >>= fun _ =>
    if !arithAdmits op t then modify (·.push "F026")
    else reportIf (!fits Γ (needs op)) "F049"

/-- A `set` target's last step, an index: with no `?` or `!`, an insertion,
which only a map takes, and only with a plain `set` (F074); with one, the
element's place, which a view's is not (F073). -/
def setIndex (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (k : Expr) (mode : Option IdxMode)
    (op : Option BinOp) (e : Expr) : StateM (Array Code) Unit :=
  match mode, op with
  | Option.none, Option.some _ => modify (·.push "F074")
  | Option.none, Option.none => match t.expose with
    | .map kt vt =>
      runChk (checkF env n Γ k kt) >>= fun _ =>
      runChk (checkF env n Γ e vt) >>= fun _ => pure ()
    | _ => modify (·.push "F074")
  | Option.some _, _ => match indexTypes t with
    | .error c => modify (·.push c)
    | .ok (et, kt) =>
      if isView t then modify (·.push "F073")
      else
        runChk (checkF env n Γ k kt) >>= fun _ =>
        setValue env n Γ et op e

/-- A `set` target's steps from a type. A field the type does not have stops
the statement, as an index partway along with no `?` or `!` does (F074); a
key is reported and gone on from. -/
def setSteps (env : InferEnv) (n : Nat) (Γ : Ctx) (t : Ty) (op : Option BinOp) (e : Expr) :
    List Step → StateM (Array Code) Unit
  | [] => setValue env n Γ t op e
  | [.index k mode] => setIndex env n Γ t k mode op e
  | .field s :: rest => match fieldType t s with
    | .ok t => setSteps env n Γ t op e rest
    | .error c => modify (·.push c)
  | .index _ Option.none :: _ => modify (·.push "F074")
  | .index k (Option.some _) :: rest => match indexTypes t with
    | .error c => modify (·.push c)
    | .ok (et, kt) =>
      runChk (checkF env n Γ k kt) >>= fun _ =>
      setSteps env n Γ et op e rest

/-- The ways a `set` target's indexes return early, each once, in the order
they are first written. -/
def setModes (steps : List Step) : List IdxMode :=
  steps.foldl addMode []
where
  addMode (ms : List IdxMode) : Step → List IdxMode
    | .index _ (Option.some m) => if ms.contains m then ms else ms ++ [m]
    | _ => ms

/-- Report each way of returning early the function does not fit (F049). -/
def reportModes (Γ : Ctx) : List IdxMode → StateM (Array Code) Unit
  | [] => pure ()
  | m :: ms => reportIf (!Γ.fitsMode m) "F049" >>= fun _ => reportModes Γ ms

/-- `set p = e`, or `set p ⊕= e`: the root (F054, F055), the early returns its
indexes make (F049), and its steps to the place written, an immutable root's
included. -/
def checkSet (env : InferEnv) (n : Nat) (Γ : Ctx) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) : StateM (Array Code) Unit :=
  match Γ.lookup x with
  | Option.none => modify (·.push "F054")
  | Option.some (t, k) =>
    reportIf (k == .let_) "F055" >>= fun _ =>
    reportModes Γ (setModes steps) >>= fun _ =>
    setSteps env n Γ t op e steps

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
    | .set_ x steps op e => checkSet env n Γ x steps op e >>= fun _ => pure Γ
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
    | .debuglog e => runChk (synthF env n Γ.borrow e) >>= fun _ => pure Γ
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
