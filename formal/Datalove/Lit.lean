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

Inference, unification and synthesis are `partial`: unification has no
structural measure to recurse on. Checking is total (`checkWith`), taking
synthesis and Check-Subsume as parameters, and `Proofs/LitSound.lean` proves
it sound: a literal that checks is a value of its type.
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
  /-- Datafun's `e@`, with `e` of type `t`: any type `t` adapts to, and `t`
  if nothing decides. Datalit's own inference never makes one; it is here
  because datafun's inference is datalit's, extended. -/
  | adapt (t : Ty)
  /-- Datafun's: a type inference could not read, such as an unbound name's,
  which checking reports by synthesizing what holds it. It is open, but one
  made one with a literal's open type stays that literal's (T016). -/
  | unknown
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
  | .open_, _ | .unknown, _ => true
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
  | .adapt f, .known t => f.canAdapt t
  -- A type with parts undecided: what they must be is worked out on binding.
  | .adapt _, _ => true
  | _, _ => false

/-- Two classes made one, and payloads to unify then, or `none` if no type is
of both (the doc's table of merges). -/
def mergeClasses : Class → Class → Option (Class × Option (PTy × PTy))
  | .unknown, c | c, .unknown => some (c, none)
  | .open_, c | c, .open_ => some (c, none)
  | .int, .int => some (.int, none)
  | .hex, .hex => some (.hex, none)
  | .float, .float => some (.float, none)
  | .int, .hex | .hex, .int => some (.int, none)
  | .float, .hex | .hex, .float => some (.float, none)
  | .variant a none, .variant b none => if a == b then some (.variant a none, none) else none
  | .variant a (some p), .variant b (some q) =>
    if a == b then some (.variant a (some p), some (p, q)) else none
  -- An adapted value meets another class at the type it was taken from, if
  -- the other admits it.
  | .adapt f, c | c, .adapt f => if c.admits (.known f) then some (.adapt f, none) else none
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
  -- `@` converts between types with parts only by cloning, but for an atom
  -- or term becoming an enum, so a value adapted to a type with undecided
  -- parts has those parts.
  if let .adapt f := c then
    match f, t with
    | _, .known _ => pure ()
    | .atom _, .enum _ => pure ()
    | .term a q, .enum vs => if let some p := (vs.lookup a).join then unify (.known q) p
    | _, t => unify (.known f) t

end

/-! ## Defaults -/

/-- What `resolve` throws for a variable of the `unknown` class: not a
diagnostic, but word to synthesize the expression instead. -/
def unknownCode : Code := "unknown"

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
    | .free (.adapt f) => pure f
    | .free .open_ => throw "T016"
    | .free .unknown => throw unknownCode
    | _ => panic! "a pruned variable is free"
  -- Parts in order, so that the first undecided is the one thrown.
  | .tuple ts => do
    let rs ← ts.mapM resolve
    pure (.tuple (rs.foldr .cons .nil))
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
    let rs ← vs.mapM fun (n, p) => do pure (n, ← p.mapM resolve)
    pure (.enum (rs.foldr (fun (n, p) out => match p with
      | none => .atom n out
      | some t => .term n t out) .nil))
where
  resolveFields (fs : List (String × PTy)) : InferM Fields := do
    let rs ← fs.mapM fun (n, t) => do pure (n, ← resolve t)
    pure (rs.foldr (fun (n, t) out => .cons n t out) .nil)

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

/-- What checking needs from synthesis: to synthesize a `data` or `error`
payload, and Check-Subsume for a form with no rule for the expected head.
Given as parameters, so that checking itself is a total function, structural
on the expression, which `Proofs/LitSound.lean` can reason about. -/
structure CheckEnv where
  syn : Expr → Except Code Ty
  sub : Expr → Except Code Unit

mutual

/-- `⊢ e ⇐ T`, by the doc's rule selection: a hint first (Check-Hinted), the
expected type unrolled (Check-Roll), the rule for the form and head, `none`
and `er` against anything else (T032), and Check-Subsume last. -/
def checkWith (env : CheckEnv) : Expr → Ty → Except Code Unit
  | .hinted h inner, expected => do
    let hinted ← convertHint h
    if !hinted.meet expected then throw "T040"
    checkForm env inner expected
  | e, expected => checkForm env e expected

/-- The rule for an expression's form and the expected type's head, with the
expected type unrolled. -/
def checkForm (env : CheckEnv) : Expr → Ty → Except Code Unit
  | .group inner, expected => checkWith env inner expected
  | .none, expected => match expected.expose with
    | .option _ => pure ()
    | _ => throw "T032"
  | .er p, expected => match expected.expose with
    | .result _ => checkWith env p .error
    | _ => throw "T032"
  | .some p, expected => match expected.expose with
    | .option t => checkWith env p t
    | _ => env.sub (.some p)
  | .ok p, expected => match expected.expose with
    | .result t => checkWith env p t
    | _ => env.sub (.ok p)
  | .tt, expected | .ff, expected => if Ty.bool == expected then pure () else throw "T022"
  | .str, expected => if Ty.string == expected then pure () else throw "T022"
  | .int n, expected =>
    let t := expected.expose
    if t == .int then pure ()
    else match intRange t with
      | Option.some (lo, hi, code) => if lo ≤ n && n ≤ hi then pure () else throw code
      | Option.none => env.sub (.int n)
  | .float, expected => match expected.expose with
    | .f32 | .f64 => pure ()
    | _ => env.sub .float
  | .hex n, expected =>
    let t := expected.expose
    if t == .int then pure ()
    else match hexBits t with
      | Option.some (bits, code) => if n < 2 ^ bits then pure () else throw code
      | Option.none => env.sub (.hex n)
  | .tuple es, expected => match expected.expose with
    | .tuple ts =>
      if es.length != (tysList ts).length then throw "T038"
      else checkPairs env es (tysList ts)
    | _ => env.sub (.tuple es)
  | .struct fs, expected => match expected.expose with
    | .struct ts =>
      if fs.length != (fieldsList ts).length then throw "T039"
      else checkFields env fs (fieldsList ts)
    | _ => env.sub (.struct fs)
  | .list es, expected => match expected.expose with
    | .list t => checkAll env es t
    | _ => env.sub (.list es)
  | .set es, expected => match expected.expose with
    | .set t => checkAll env es t
    | _ => env.sub (.set es)
  | .map es, expected => match expected.expose with
    | .map k v => checkEntries env es k v
    | _ => env.sub (.map es)
  | .tensor shape es, expected => match expected.expose with
    | .tensor t rank =>
      if shape.length != rank then throw "T048"
      else if es.length != shape.foldl (· * ·) 1 then throw "T049"
      else checkAll env es t
    | _ => env.sub (.tensor shape es)
  | .table header rows, expected => match expected.expose with
    | .table cols =>
      let cols := fieldsList cols
      if header.length != cols.length then throw "T054"
      else if header != cols.map (·.1) then throw "T055"
      else checkRows env rows cols
    | _ => env.sub (.table header rows)
  | .atom a, expected => match expected.expose with
    | .atom b => if a == b then pure () else throw "T057"
    | .enum vs => if Variants.atomListed a vs then pure () else throw "T057"
    | _ => env.sub (.atom a)
  | .term a p, expected => match expected.expose with
    | .term b t => if a == b then checkWith env p t else throw "T057"
    | .enum vs => match Variants.termPayload a vs with
      | Option.some t => checkWith env p t
      | Option.none => throw "T057"
    | _ => env.sub (.term a p)
  | .enum inner, expected => match expected.expose with
    | .enum _ => checkWith env inner expected
    | _ => throw "T058"
  | .data p, expected => match expected.expose with
    | .data => do let _ ← env.syn p; pure ()
    | _ => env.sub (.data p)
  | .error p, expected => match expected.expose with
    | .error => do let _ ← env.syn p; pure ()
    | _ => env.sub (.error p)
  | .hinted h e, _ => env.sub (.hinted h e)

/-- A tuple's elements against its types, in order. -/
def checkPairs (env : CheckEnv) : List Expr → List Ty → Except Code Unit
  | e :: es, t :: ts => do checkWith env e t; checkPairs env es ts
  | _, _ => pure ()

/-- A struct's fields against its type's, in order: a name out of place is
T042. -/
def checkFields (env : CheckEnv) : List (String × Expr) → List (String × Ty) → Except Code Unit
  | (n, e) :: fs, (m, t) :: ts => do
    if n != m then throw "T042"
    checkWith env e t
    checkFields env fs ts
  | _, _ => pure ()

def checkAll (env : CheckEnv) : List Expr → Ty → Except Code Unit
  | [], _ => pure ()
  | e :: es, t => do checkWith env e t; checkAll env es t

def checkEntries (env : CheckEnv) : List (Expr × Expr) → Ty → Ty → Except Code Unit
  | [], _, _ => pure ()
  | (ke, ve) :: es, k, v => do checkWith env ke k; checkWith env ve v; checkEntries env es k v

/-- A table's rows: each one element per column (else T056), each against
its column's type. -/
def checkRows (env : CheckEnv) : List (List Expr) → List (String × Ty) → Except Code Unit
  | [], _ => pure ()
  | r :: rs, cols => do
    if r.length != cols.length then throw "T056"
    checkPairs env r (cols.map (·.2))
    checkRows env rs cols

end

mutual

/-- Check-Subsume reached by a form with no rule for the head: it synthesizes
a type of another head, so it fails (T032), unless inferring what it would
be finds a malformed hint first. -/
partial def subsume (e : Expr) : Except Code Unit := do
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

/-- `⊢ e ⇐ T`, with synthesis and Check-Subsume as they are. -/
partial def check (e : Expr) (expected : Ty) : Except Code Unit :=
  checkWith ⟨synthesize, subsume⟩ e expected

end

end Datalove.Lit
