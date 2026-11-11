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

namespace ConvM

def fail (kind : ConvErrKind) (name : String) : ConvM α :=
  throw ⟨kind, name⟩

/-- Enter the body of `rec name`, as `RecScope::bind`. -/
def bind (name : String) : ConvM Unit := do
  let s ← get
  if s.inKey then fail .inKey name
  if s.binders.any (·.name == name) then fail .shadowed name
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

/-- A use of a name, as `RecScope::lookup` followed by datalit's
`NoAliases::resolve` when no `rec` binds it. -/
def lookup (name : String) : ConvM Ty := do
  let s ← get
  match s.binders.findIdx? (·.name == name) with
  | none => fail .unbound name
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

def Hint.conv : Hint → ConvM Ty
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
  | .tuple fs => .tuple <$> fs.conv
  | .struct fs => .struct <$> fs.conv
  | .list t => .list <$> collection false t.conv
  | .map k v => do
    let k ← collection true k.conv
    let v ← collection false v.conv
    pure (.map k v)
  | .set t => .set <$> collection true t.conv
  | .option t => .option <$> t.conv
  | .result t => .result <$> t.conv
  | .tensor t r => (.tensor · r) <$> collection false t.conv
  | .table cs => .table <$> cs.convColumns
  | .atom a => pure (.atom a)
  | .term a t => .term a <$> t.conv
  | .enum vs => (.enum ∘ Variants.sortByName) <$> vs.conv
  | .mu n body => do
    bind n
    let b ← body.conv
    unbind
    pure (.mu b)
  | .name n => lookup n

def Hints.conv : Hints → ConvM Tys
  | .nil => pure .nil
  | .cons t ts => do
    let t ← t.conv
    let ts ← ts.conv
    pure (.cons t ts)

def HFields.conv : HFields → ConvM Fields
  | .nil => pure .nil
  | .cons n t fs => do
    let t ← t.conv
    let fs ← fs.conv
    pure (.cons n t fs)

/-- A table's columns: each inside a collection. -/
def HFields.convColumns : HFields → ConvM Fields
  | .nil => pure .nil
  | .cons n t fs => do
    let t ← collection false t.conv
    let fs ← fs.convColumns
    pure (.cons n t fs)

def HVariants.conv : HVariants → ConvM Variants
  | .nil => pure .nil
  | .atom n vs => .atom n <$> vs.conv
  | .term n t vs => do
    let t ← t.conv
    let vs ← vs.conv
    pure (.term n t vs)

end

/-- Convert a hint to a type, or say the first thing wrong with it. -/
def Hint.convert (h : Hint) : Except ConvErr Ty :=
  (h.conv.run {}).map (·.1)

end Datalove
