import Datalove.Ty

/-!
# Type hints and their conversion

A type as written, with names, and its conversion to a `Ty`, as
`convert_type_hint` in `crates/datalove-datalit/src/tycheck/types.rs`, with
datalit's `NoAliases`: a name no `rec` binds is no type at all.

The conversion is where well-formedness of recursive types is checked
(T062 to T065), so it is modelled step for step, `RecScope` and all, and the
first error it finds is the one the Rust reports. `Datalove/WF.lean` states
the same conditions over `Ty` without names, and `Proofs/Convert.lean` shows
the conversion only produces types that meet them.

The hints the model takes are those the parser can produce: an enum names
each variant once (D038), and a struct each field once.
-/

namespace Datalove

mutual

inductive Hint where
  | bool | u8 | i8 | u16 | i16 | u32 | i32 | u64 | i64
  | index | offset | f32 | f64 | int | string | data | error
  | tuple (fs : Hints)
  | struct (fs : HFields)
  | list (t : Hint)
  | map (k v : Hint)
  | set (t : Hint)
  | option (t : Hint)
  | result (t : Hint)
  | tensor (t : Hint) (rank : Nat)
  | table (cs : HFields)
  | atom (a : String)
  | term (a : String) (t : Hint)
  | enum (vs : HVariants)
  /-- A bare name: here, only a `rec`'s. -/
  | name (n : String)
  | mu (n : String) (body : Hint)

inductive Hints where
  | nil
  | cons (t : Hint) (ts : Hints)

inductive HFields where
  | nil
  | cons (n : String) (t : Hint) (fs : HFields)

inductive HVariants where
  | nil
  | atom (n : String) (vs : HVariants)
  | term (n : String) (t : Hint) (vs : HVariants)

end

deriving instance Repr for Hint

/-- What is wrong with a hint, as `RecErrorKind` in `types.rs`, plus a name
that names nothing, which datalit reports as `CannotSynthesize`. -/
inductive ConvErrKind where
  | unused | shadowed | unguarded | inKey | unbound
  /-- A type parameter inside a `rec` (T066). -/
  | generic
  deriving DecidableEq, Repr

structure ConvErr where
  kind : ConvErrKind
  name : String
  deriving DecidableEq, Repr

/-- A `rec` binder in scope, as `RecBinder`. -/
structure Binder where
  name : String
  /-- How many collections were around the binder. -/
  colls : Nat
  used : Bool
  deriving Inhabited

/-- What is around the part being converted, as `RecScope`. -/
structure Scope where
  /-- Innermost first. -/
  binders : List Binder := []
  colls : Nat := 0
  inKey : Bool := false

abbrev ConvM := StateT Scope (Except ConvErr)

/-- The types names stand for where a hint is converted: datafun's aliases,
and none in datalit. Read only. -/
abbrev Aliases := List (String × Ty)

mutual

/-- Whether a `rec`, or a use of one, is anywhere in a type, as
`contains_rec` in `types.rs`. -/
def Ty.containsRec : Ty → Bool
  | .mu _ | .recvar _ => true
  | .tuple ts => ts.containsRec
  | .struct fs | .table fs => fs.containsRec
  | .list t | .set t | .option t | .result t | .tensor t _ | .term _ t => t.containsRec
  | .map k v => k.containsRec || v.containsRec
  | .enum vs => vs.containsRec
  | _ => false

def Tys.containsRec : Tys → Bool
  | .nil => false
  | .cons t ts => t.containsRec || ts.containsRec

def Fields.containsRec : Fields → Bool
  | .nil => false
  | .cons _ t fs => t.containsRec || fs.containsRec

def Variants.containsRec : Variants → Bool
  | .nil => false
  | .atom _ vs => vs.containsRec
  | .term _ t vs => t.containsRec || vs.containsRec

end

mutual

/-- Whether a type parameter is anywhere in a type, as `contains_var` in
`types.rs`. -/
def Ty.containsVar : Ty → Bool
  | .var _ => true
  | .mu b => b.containsVar
  | .tuple ts => ts.containsVar
  | .struct fs | .table fs => fs.containsVar
  | .list t | .set t | .option t | .result t | .tensor t _ | .term _ t => t.containsVar
  | .map k v => k.containsVar || v.containsVar
  | .enum vs => vs.containsVar
  | _ => false

def Tys.containsVar : Tys → Bool
  | .nil => false
  | .cons t ts => t.containsVar || ts.containsVar

def Fields.containsVar : Fields → Bool
  | .nil => false
  | .cons _ t fs => t.containsVar || fs.containsVar

def Variants.containsVar : Variants → Bool
  | .nil => false
  | .atom _ vs => vs.containsVar
  | .term _ t vs => t.containsVar || vs.containsVar

end

namespace ConvM

def fail (kind : ConvErrKind) (name : String) : ConvM α :=
  throw ⟨kind, name⟩

/-- Enter the body of `rec name`, as `RecScope::bind`. A name an alias has
is taken. -/
def bind (aliases : Aliases) (name : String) : ConvM Unit := do
  let s ← get
  if s.inKey then fail .inKey name
  if s.binders.any (·.name == name) || (aliases.lookup name).isSome then fail .shadowed name
  set { s with binders := ⟨name, s.colls, false⟩ :: s.binders }

/-- Leave the body of the innermost `rec`, as `RecScope::unbind`. -/
def unbind : ConvM Unit := do
  let s ← get
  match s.binders with
  | [] => panic! "unbind follows bind"
  | b :: bs =>
    set { s with binders := bs }
    if !b.used then fail .unused b.name

/-- Mark the binder at `depth` used. -/
def markUsed : List Binder → Nat → List Binder
  | [], _ => []
  | b :: bs, 0 => { b with used := true } :: bs
  | b :: bs, n + 1 => b :: markUsed bs n

/-- A use of a name, as `RecScope::lookup`, or when no `rec` binds it, the
alias's type, as `resolve` and `check_named`: a type with a `rec` in it is
no map's key or set's element, and one with a type parameter in it is not
inside a `rec`. -/
def lookup (aliases : Aliases) (name : String) : ConvM Ty := do
  let s ← get
  match s.binders.findIdx? (·.name == name) with
  | none => match aliases.lookup name with
    | none => fail .unbound name
    | some t =>
      if s.inKey && t.containsRec then fail .inKey name
      else if !s.binders.isEmpty && t.containsVar then fail .generic name
      else pure t
  | some depth =>
    let b := s.binders[depth]!
    set { s with binders := markUsed s.binders depth }
    if s.inKey then fail .inKey name
    if s.colls ≤ b.colls then fail .unguarded name
    pure (.recvar depth)

/-- Run `f` inside a collection, in an ordered position if `key`, as
`RecScope::collection`. -/
def collection (key : Bool) (f : ConvM α) : ConvM α := do
  let s ← get
  let wasInKey := s.inKey
  set { s with colls := s.colls + 1, inKey := s.inKey || key }
  let r ← f
  modify fun s => { s with colls := s.colls - 1, inKey := wasInKey }
  pure r

end ConvM

/-- Enum variants as a list, for sorting. -/
def Variants.toList : Variants → List (String × Option Ty)
  | .nil => []
  | .atom n vs => (n, none) :: vs.toList
  | .term n t vs => (n, some t) :: vs.toList

def Variants.ofList : List (String × Option Ty) → Variants
  | [] => .nil
  | (n, none) :: vs => .atom n (Variants.ofList vs)
  | (n, some t) :: vs => .term n t (Variants.ofList vs)

/-- Variants sorted by name, as `convert_in` sorts them. -/
def Variants.sortByName (vs : Variants) : Variants :=
  Variants.ofList (vs.toList.mergeSort fun a b => !(b.1 < a.1))

open ConvM in
mutual

def Hint.conv (aliases : Aliases) : Hint → ConvM Ty
  | .bool => pure .bool
  | .u8 => pure .u8
  | .i8 => pure .i8
  | .u16 => pure .u16
  | .i16 => pure .i16
  | .u32 => pure .u32
  | .i32 => pure .i32
  | .u64 => pure .u64
  | .i64 => pure .i64
  | .index => pure .index
  | .offset => pure .offset
  | .f32 => pure .f32
  | .f64 => pure .f64
  | .int => pure .int
  | .string => pure .string
  | .data => pure .data
  | .error => pure .error
  | .tuple fs => .tuple <$> fs.conv aliases
  | .struct fs => .struct <$> fs.conv aliases
  | .list t => .list <$> collection false (t.conv aliases)
  | .map k v => do
    let k ← collection true (k.conv aliases)
    let v ← collection false (v.conv aliases)
    pure (.map k v)
  | .set t => .set <$> collection true (t.conv aliases)
  | .option t => .option <$> t.conv aliases
  | .result t => .result <$> t.conv aliases
  | .tensor t r => (.tensor · r) <$> collection false (t.conv aliases)
  | .table cs => .table <$> cs.convColumns aliases
  | .atom a => pure (.atom a)
  | .term a t => .term a <$> t.conv aliases
  | .enum vs => (.enum ∘ Variants.sortByName) <$> vs.conv aliases
  | .mu n body => do
    bind aliases n
    let b ← body.conv aliases
    unbind
    pure (.mu b)
  | .name n => lookup aliases n

def Hints.conv (aliases : Aliases) : Hints → ConvM Tys
  | .nil => pure .nil
  | .cons t ts => do
    let t ← t.conv aliases
    let ts ← ts.conv aliases
    pure (.cons t ts)

def HFields.conv (aliases : Aliases) : HFields → ConvM Fields
  | .nil => pure .nil
  | .cons n t fs => do
    let t ← t.conv aliases
    let fs ← fs.conv aliases
    pure (.cons n t fs)

/-- A table's columns: each inside a collection. -/
def HFields.convColumns (aliases : Aliases) : HFields → ConvM Fields
  | .nil => pure .nil
  | .cons n t fs => do
    let t ← collection false (t.conv aliases)
    let fs ← fs.convColumns aliases
    pure (.cons n t fs)

def HVariants.conv (aliases : Aliases) : HVariants → ConvM Variants
  | .nil => pure .nil
  | .atom n vs => .atom n <$> vs.conv aliases
  | .term n t vs => do
    let t ← t.conv aliases
    let vs ← vs.conv aliases
    pure (.term n t vs)

end

/-- Convert a hint to a type, or say the first thing wrong with it. -/
def Hint.convert (h : Hint) : Except ConvErr Ty :=
  ((h.conv []).run {}).map (·.1)

/-- Convert a hint where names stand for the aliases' types. -/
def Hint.convertWith (aliases : Aliases) (h : Hint) : Except ConvErr Ty :=
  ((h.conv aliases).run {}).map (·.1)

end Datalove
