import Datalove.Ty
import Datalove.Relations
import Datalove.Hint

/-!
# Literal typing

Datalit's typing rules for expressions, written from
`botdocs/datalit-typing-rules.md` rather than transcribed from the Rust: the
inference with literal classes and unification, the defaults, checking, and
synthesis. Each definition names the rule it follows. Where the doc leaves a
choice open, the comment says which was made.

The Rust (`crates/datalove-datalit/src/tycheck/`) is held to this by
`crates/datalove-formal-tests/tests/lit_tests.rs`, so a disagreement is a
place where the code and the doc differ.

The definitions are `partial`: unification has no structural measure to
recurse on. They run as the oracle; nothing is proved of them yet.
-/

namespace Datalove.Lit

/-! ## Expressions -/

/-- A datalit expression. `hinted h e` is `: h / e`, an expression carrying a
hint; `e` carries none, for two hints on one expression are written with a
group between them. -/
inductive Expr where
  | tt | ff
  /-- An integer literal, by its value. -/
  | int (n : Int)
  /-- A hex literal, by its value. -/
  | hex (n : Nat)
  | float
  | str
  | tuple (es : List Expr)
  | struct (fs : List (String × Expr))
  | list (es : List Expr)
  | set (es : List Expr)
  | map (es : List (Expr × Expr))
  | tensor (shape : List Nat) (es : List Expr)
  | table (header : List String) (rows : List (List Expr))
  | none
  | some (e : Expr)
  | ok (e : Expr)
  | er (e : Expr)
  | data (e : Expr)
  | error (e : Expr)
  | atom (a : String)
  | term (a : String) (e : Expr)
  | enum (e : Expr)
  | group (e : Expr)
  | hinted (h : Hint) (e : Expr)
  deriving Inhabited

/-- A diagnostic code, as the doc's table gives them. -/
abbrev Code := String

/-- The width of `index` and `offset`: 32 bits, the build without `index-64`. -/
def indexBits : Nat := 32

/-! ## Types with variables -/

inductive PTy where
  | known (t : Ty)
  | var (v : Nat)
  | tuple (ts : List PTy)
  | struct (fs : List (String × PTy))
  | table (fs : List (String × PTy))
  | list (t : PTy)
  | set (t : PTy)
  | map (k v : PTy)
  | option (t : PTy)
  | result (t : PTy)
  | tensor (t : PTy) (rank : Nat)
  | term (a : String) (t : PTy)
  | enum (vs : List (String × Option PTy))
  deriving Inhabited

/-- What a variable stands for, by the doc's table of classes. -/
inductive Class where
  | open_
  | int
  | hex
  | float
  /-- `atom a` when the payload is `none`, `term a τ` when it is `some τ`. -/
  | variant (a : String) (payload : Option PTy)
  deriving Inhabited

inductive VarState where
  | free (c : Class)
  | bound (t : PTy)
  | link (v : Nat)
  deriving Inhabited

abbrev InferM := StateT (Array VarState) (Except Code)

def fresh (c : Class) : InferM PTy := do
  let s ← get
  set (s.push (.free c))
  pure (.var s.size)

partial def find (v : Nat) : InferM Nat := do
  match (← get)[v]! with
  | .link w => find w
  | _ => pure v

/-- A type with a variable at the top replaced by what it is bound to. -/
def prune (t : PTy) : InferM PTy := do
  match t with
  | .var v =>
    let r ← find v
    match (← get)[r]! with
    | .bound b => pure b
    | _ => pure (.var r)
  | t => pure t

/-! ## Classes -/

def Ty.isUnsigned : Ty → Bool
  | .u8 | .u16 | .u32 | .u64 | .index => true
  | _ => false

/-- Whether a class admits a type, by the type's head (the doc's table). A
literal's class admits a recursive type when it admits the unfolding. -/
partial def Class.admits : Class → PTy → Bool
  | .open_, _ => true
  | c, .known t@(.mu _) => c.admits (.known t.expose)
  | .int, .known t => t.isFixedInt || t == .int
  | .hex, .known t => Ty.isUnsigned t || t == .int || t.isFloat
  | .float, .known t => t.isFloat
  -- The doc: `atom a` admits `atom a` and any enum; `term a τ` admits
  -- `term a T` and any enum.
  | .variant a none, .known (.atom b) => a == b
  | .variant a (some _), .known (.term b _) => a == b
  | .variant a (some _), .term b _ => a == b
  | .variant _ _, .known (.enum _) => true
  | .variant _ _, .enum _ => true
  | _, _ => false

/-- Two classes made one, and payloads to unify then, or `none` if no type is
of both (the doc's table of merges). -/
def mergeClasses : Class → Class → Option (Class × Option (PTy × PTy))
  | .open_, c | c, .open_ => some (c, none)
  | .int, .int => some (.int, none)
  | .hex, .hex => some (.hex, none)
  | .float, .float => some (.float, none)
  | .int, .hex | .hex, .int => some (.int, none)
  | .float, .hex | .hex, .float => some (.float, none)
  | .variant a none, .variant b none => if a == b then some (.variant a none, none) else none
  | .variant a (some p), .variant b (some q) =>
    if a == b then some (.variant a (some p), some (p, q)) else none
  | _, _ => none

def tysP : Tys → List PTy
  | .nil => []
  | .cons t ts => .known t :: tysP ts

def fieldsP : Fields → List (String × PTy)
  | .nil => []
  | .cons n t fs => (n, .known t) :: fieldsP fs

def variantsP : Variants → List (String × Option PTy)
  | .nil => []
  | .atom n vs => (n, none) :: variantsP vs
  | .term n t vs => (n, some (.known t)) :: variantsP vs

/-- One level of a known type, opened so its parts can be unified. -/
def expand : Ty → Option PTy
  | .tuple fs => some (.tuple (tysP fs))
  | .struct fs => some (.struct (fieldsP fs))
  | .table cs => some (.table (fieldsP cs))
  | .list t => some (.list (.known t))
  | .set t => some (.set (.known t))
  | .map k v => some (.map (.known k) (.known v))
  | .option t => some (.option (.known t))
  | .result t => some (.result (.known t))
  | .tensor t r => some (.tensor (.known t) r)
  | .term a t => some (.term a (.known t))
  | .enum vs => some (.enum (variantsP vs))
  | _ => none

/-- The payload an enum gives its term variant `a`, if it has one. -/
def termPayloadP (a : String) (vs : Variants) : Option PTy :=
  ((variantsP vs).lookup a).join

/-! ## Unification -/

mutual

/-- `τ ≐ τ'`. A constraint that cannot hold leaves both sides as they are:
the check after inference reports it. -/
partial def unify (a b : PTy) : InferM Unit := do
  let aRoot ← rootOf a
  let bRoot ← rootOf b
  let a' ← prune a
  let b' ← prune b
  match a', b' with
  | .var x, .var y => union x y
  | .var x, t => bind x t
  | t, .var y => bind y t
  | .known _, .known _ => pure ()
  -- A variable holding a literal's parts, unified with a recursive type,
  -- takes the type, and the parts go along with the unfolding.
  | .known r@(.mu _), other =>
    if let some h := bRoot then modify (·.set! h (.bound (.known r)))
    if let some e := expand r.unfold then unifyParts e other
  | other, .known r@(.mu _) =>
    if let some h := aRoot then modify (·.set! h (.bound (.known r)))
    if let some e := expand r.unfold then unifyParts e other
  | .known k, other | other, .known k =>
    if let some e := expand k then unifyParts e other
  | x, y => unifyParts x y

partial def rootOf : PTy → InferM (Option Nat)
  | .var v => some <$> find v
  | _ => pure none

/-- Two types with no variable at the top, part by part. An enum's variants
are matched by name, as the doc says. -/
partial def unifyParts : PTy → PTy → InferM Unit
  | .tuple xs, .tuple ys => (xs.zip ys).forM fun (x, y) => unify x y
  | .struct xs, .struct ys | .table xs, .table ys =>
    (xs.zip ys).forM fun ((xn, x), (yn, y)) => if xn == yn then unify x y else pure ()
  | .list x, .list y | .set x, .set y | .option x, .option y | .result x, .result y => unify x y
  | .tensor x xr, .tensor y yr => if xr == yr then unify x y else pure ()
  | .map xk xv, .map yk yv => do unify xk yk; unify xv yv
  | .term xn x, .term yn y => if xn == yn then unify x y else pure ()
  | .enum xs, .enum ys =>
    xs.forM fun (n, x) =>
      match x, (ys.lookup n).join with
      | some x, some y => unify x y
      | _, _ => pure ()
  | _, _ => pure ()

partial def union (x y : Nat) : InferM Unit := do
  if x == y then return
  let s ← get
  let .free cx := s[x]! | panic! "a pruned variable is free"
  let .free cy := s[y]! | panic! "a pruned variable is free"
  match mergeClasses cx cy with
  | none => pure ()
  | some (c, payloads) =>
    modify fun s => (s.set! y (.link x)).set! x (.free c)
    if let some (p, q) := payloads then unify p q

/-- A variable binds to a type only if its class admits it. Binding a term
literal's variable to a type that gives the term a payload unifies the two
payloads. -/
partial def bind (v : Nat) (t : PTy) : InferM Unit := do
  let .free c := (← get)[v]! | panic! "a pruned variable is free"
  if !c.admits t then return
  modify (·.set! v (.bound t))
  if let .variant a (some p) := c then
    let given : Option PTy := match t with
      | .term b q => if a == b then some q else none
      | .known (.term b q) => if a == b then some (.known q) else none
      | .known (.enum vs) => termPayloadP a vs
      | .enum vs => (vs.lookup a).join
      | _ => none
    if let some q := given then unify p q

end

/-! ## Defaults -/

/-- The type a variable-holding type comes to, each undecided variable taking
its class's default. A variable with none is T016. -/
partial def resolve (t : PTy) : InferM Ty := do
  match ← prune t with
  | .known k => pure k
  | .var v =>
    match (← get)[v]! with
    | .free .int | .free .hex => pure .int
    | .free .float => pure .f64
    | .free (.variant a none) => pure (.atom a)
    | .free (.variant a (some p)) => .term a <$> resolve p
    | .free .open_ => throw "T016"
    | _ => panic! "a pruned variable is free"
  | .tuple ts => do
    let mut out := Tys.nil
    for t in ts.reverse do out := .cons (← resolve t) out
    pure (.tuple out)
  | .struct fs => .struct <$> resolveFields fs
  | .table fs => .table <$> resolveFields fs
  | .list t => .list <$> resolve t
  | .set t => .set <$> resolve t
  | .map k v => .map <$> resolve k <*> resolve v
  | .option t => .option <$> resolve t
  | .result t => .result <$> resolve t
  | .tensor t r => (.tensor · r) <$> resolve t
  | .term a t => .term a <$> resolve t
  | .enum vs => do
    let mut out := Variants.nil
    for (n, p) in vs.reverse do
      out ← match p with
        | none => pure (.atom n out)
        | some p => do pure (.term n (← resolve p) out)
    pure (.enum out)
where
  resolveFields (fs : List (String × PTy)) : InferM Fields := do
    let mut out := Fields.nil
    for (n, t) in fs.reverse do out := .cons n (← resolve t) out
    pure out

/-! ## Hints -/

def errCode : ConvErrKind → Code
  | .unused => "T062"
  | .shadowed => "T063"
  | .unguarded => "T064"
  | .inKey => "T065"
  | .unbound => "T059"

def convertHint (h : Hint) : Except Code Ty :=
  match h.convert with
  | .ok t => .ok t
  | .error e => .error (errCode e.kind)

/-! ## Inference -/

/-- `⊢ e ⇝ τ`, the Inf- rules. Inference reports nothing but malformed
hints. -/
partial def infer : Expr → InferM PTy
  | .hinted h _ => do pure (.known (← liftM (convertHint h)))
  | .tt | .ff => pure (.known .bool)
  | .str => pure (.known .string)
  | .int _ => fresh .int
  | .hex _ => fresh .hex
  | .float => fresh .float
  | .tuple es => .tuple <$> es.mapM infer
  | .struct fs => .struct <$> fs.mapM fun (n, e) => do pure (n, ← infer e)
  | .list es => .list <$> together es
  | .set es => .set <$> together es
  | .tensor shape es => (.tensor · shape.length) <$> together es
  | .map es => do
    let k ← fresh .open_
    let v ← fresh .open_
    for (ke, ve) in es do
      unify k (← infer ke)
      unify v (← infer ve)
    pure (.map k v)
  | .table header rows => do
    let cols ← header.mapM fun _ => fresh .open_
    for row in rows do
      for (e, col) in row.zip cols do
        unify col (← infer e)
    pure (.table (header.zip cols))
  | .some e => .option <$> infer e
  | .ok e => .result <$> infer e
  | .none => .option <$> fresh .open_
  | .er _ => .result <$> fresh .open_
  | .data _ => pure (.known .data)
  | .error _ => pure (.known .error)
  | .atom a => fresh (.variant a none)
  | .term a e => do fresh (.variant a (some (← infer e)))
  | .enum _ => fresh .open_
  | .group e => infer e
where
  liftM {α} (x : Except Code α) : InferM α := StateT.lift x
  together (es : List Expr) : InferM PTy := do
    let t ← fresh .open_
    for e in es do unify t (← infer e)
    pure t

/-! ## Checking -/

/-- The diagnostic for an integer out of range for a type, by the doc's
table: T005 to T012 in the order `u8 i8 u16 i16 u32 i32 u64 i64`, T015 for
`index` and T061 for `offset`. -/
def intRange : Ty → Option (Int × Int × Code)
  | .u8 => some (0, 2 ^ 8 - 1, "T005")
  | .i8 => some (-2 ^ 7, 2 ^ 7 - 1, "T006")
  | .u16 => some (0, 2 ^ 16 - 1, "T007")
  | .i16 => some (-2 ^ 15, 2 ^ 15 - 1, "T008")
  | .u32 => some (0, 2 ^ 32 - 1, "T009")
  | .i32 => some (-2 ^ 31, 2 ^ 31 - 1, "T010")
  | .u64 => some (0, 2 ^ 64 - 1, "T011")
  | .i64 => some (-2 ^ 63, 2 ^ 63 - 1, "T012")
  | .index => some (0, 2 ^ indexBits - 1, "T015")
  | .offset => some (-2 ^ (indexBits - 1), 2 ^ (indexBits - 1) - 1, "T061")
  | _ => none

/-- How many bits a hex literal may have against a type, and the code if it
has more: the unsigned integers by their width, `f32` and `f64` as bit
patterns (T013, T014). `none` for `int`, which takes any. -/
def hexBits : Ty → Option (Nat × Code)
  | .u8 => some (8, "T005")
  | .u16 => some (16, "T007")
  | .u32 => some (32, "T009")
  | .u64 => some (64, "T011")
  | .index => some (indexBits, "T015")
  | .f32 => some (32, "T013")
  | .f64 => some (64, "T014")
  | _ => none

def Variants.atomListed (a : String) : Variants → Bool
  | .nil => false
  | .atom n vs => n == a || Variants.atomListed a vs
  | .term _ _ vs => Variants.atomListed a vs

def Variants.termPayload (a : String) : Variants → Option Ty
  | .nil => none
  | .atom _ vs => Variants.termPayload a vs
  | .term n t vs => if n == a then some t else Variants.termPayload a vs

def tysList : Tys → List Ty
  | .nil => []
  | .cons t ts => t :: tysList ts

def fieldsList : Fields → List (String × Ty)
  | .nil => []
  | .cons n t fs => (n, t) :: fieldsList fs

mutual

/-- `⊢ e ⇐ T`, by the doc's rule selection: a hint first (Check-Hinted), the
expected type unrolled (Check-Roll), the rule for the form and head, `none`
and `er` against anything else (T032), and Check-Subsume last. -/
partial def check (e : Expr) (expected : Ty) : Except Code Unit := do
  let e ← match e with
    | .hinted h inner => do
      let hinted ← convertHint h
      if !hinted.meet expected then throw "T040"
      pure inner
    | e => pure e
  let exposed := expected.expose
  match e, exposed with
  | .group inner, _ => check inner expected
  | .none, .option _ => pure ()
  | .some p, .option t => check p t
  | .ok p, .result t => check p t
  | .er p, .result _ => check p .error
  | .none, _ | .er _, _ => throw "T032"
  | .tt, _ | .ff, _ => if Ty.bool == expected then pure () else throw "T022"
  | .str, _ => if Ty.string == expected then pure () else throw "T022"
  | .int n, t =>
    if t == .int then pure ()
    else match intRange t with
      | some (lo, hi, code) => if lo ≤ n && n ≤ hi then pure () else throw code
      | none => subsume e expected
  | .float, .f32 | .float, .f64 => pure ()
  | .hex n, t =>
    if t == .int then pure ()
    else match hexBits t with
      | some (bits, code) => if n < 2 ^ bits then pure () else throw code
      | none => subsume e expected
  | .tuple es, .tuple ts =>
    let ts := tysList ts
    if es.length != ts.length then throw "T038"
    for (e, t) in es.zip ts do check e t
  | .struct fs, .struct ts =>
    let ts := fieldsList ts
    if fs.length != ts.length then throw "T039"
    for ((n, e), (m, t)) in fs.zip ts do
      if n != m then throw "T042"
      check e t
  | .list es, .list t | .set es, .set t => for e in es do check e t
  | .map es, .map k v => for (ke, ve) in es do check ke k; check ve v
  | .tensor shape es, .tensor t rank =>
    if shape.length != rank then throw "T048"
    if es.length != shape.foldl (· * ·) 1 then throw "T049"
    for e in es do check e t
  | .table header rows, .table cols =>
    let cols := fieldsList cols
    if header.length != cols.length then throw "T054"
    for (n, (m, _)) in header.zip cols do
      if n != m then throw "T055"
    for row in rows do
      if row.length != cols.length then throw "T056"
      for (e, (_, t)) in row.zip cols do check e t
  | .atom a, .atom b => if a == b then pure () else throw "T057"
  | .atom a, .enum vs => if Variants.atomListed a vs then pure () else throw "T057"
  | .term a p, .term b t => if a == b then check p t else throw "T057"
  | .term a p, .enum vs =>
    match Variants.termPayload a vs with
    | some t => check p t
    | none => throw "T057"
  | .enum inner, .enum _ => check inner expected
  | .enum _, _ => throw "T058"
  | .data p, .data | .error p, .error => do let _ ← synthesize p; pure ()
  | e, _ => subsume e expected

/-- Check-Subsume reached by a form with no rule for the head: it synthesizes
a type of another head, so it fails (T032), unless inferring what it would
be finds a malformed hint first. -/
partial def subsume (e : Expr) (_expected : Ty) : Except Code Unit := do
  let _ ← (infer e).run #[]
  throw "T032"

/-- `⊢ e ⇒ T`: a hint is the type (Syn-TypedExpr); otherwise infer, default,
and check against the result (Syn-Infer). -/
partial def synthesize (e : Expr) : Except Code Ty := do
  match e with
  | .hinted h _ =>
    let t ← convertHint h
    check e t
    pure t
  | e =>
    let (t, _) ← (do let p ← infer e; resolve p).run #[]
    check e t
    pure t

end

end Datalove.Lit
