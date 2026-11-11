/-!
# Types

The datalit type language, as `crates/datalove-datalit/src/tycheck/types.rs`
represents it: one constructor per `Type` variant, recursive types by de
Bruijn index.

`Type::Rec` is `Ty.mu` here, since `Ty.rec` is the name Lean gives the
recursor. A `rec`'s name is not part of the model: the Rust `RecName`
compares equal to every other, so it is no part of the type there either.

Lists of fields and variants are their own inductives, mutual with `Ty`, so
that functions and proofs over types are plain structural recursion.
-/

namespace Datalove

mutual

/-- A datalit type. -/
inductive Ty where
  | bool | u8 | i8 | u16 | i16 | u32 | i32 | u64 | i64
  | index | offset | f32 | f64 | int | string | data | error
  | tuple (fs : Tys)
  | struct (fs : Fields)
  | list (t : Ty)
  | map (k v : Ty)
  | set (t : Ty)
  | option (t : Ty)
  | result (t : Ty)
  | tensor (t : Ty) (rank : Nat)
  | table (cs : Fields)
  | atom (a : String)
  | term (a : String) (t : Ty)
  | enum (vs : Variants)
  /-- A type parameter of the enclosing generic function. -/
  | var (n : String)
  /-- `rec X T`: `T`, in which `recvar 0` is the whole. -/
  | mu (body : Ty)
  /-- A use of an enclosing `mu`: 0 is the innermost. -/
  | recvar (i : Nat)

/-- A tuple's elements. -/
inductive Tys where
  | nil
  | cons (t : Ty) (ts : Tys)

/-- A struct's fields or a table's columns, in order. -/
inductive Fields where
  | nil
  | cons (n : String) (t : Ty) (fs : Fields)

/-- An enum's variants. The Rust keeps them sorted by name. -/
inductive Variants where
  | nil
  | atom (n : String) (vs : Variants)
  | term (n : String) (t : Ty) (vs : Variants)

end

deriving instance DecidableEq for Ty
deriving instance Repr for Ty
instance : Inhabited Ty := ⟨.bool⟩

namespace Ty

/-- The fixed-width integers. -/
def isFixedInt : Ty → Bool
  | u8 | i8 | u16 | i16 | u32 | i32 | u64 | i64 | index | offset => true
  | _ => false

def isFloat : Ty → Bool
  | f32 | f64 => true
  | _ => false

/-- The numeric types: the fixed-width integers, the floats and `int`. -/
def isNumeric : Ty → Bool
  | u8 | i8 | u16 | i16 | u32 | i32 | u64 | i64 | index | offset | f32 | f64 | int => true
  | _ => false

def isMu : Ty → Bool
  | mu _ => true
  | _ => false

end Ty

/-! ## Substitution and unfolding -/

mutual

/-- `t` with the binder `d` levels out replaced by `w`, which is closed.

As `substitute` in `types.rs`: other indices are left as they are. -/
def Ty.subst : Ty → Nat → Ty → Ty
  | .recvar i, d, w => if i = d then w else .recvar i
  | .mu b, d, w => .mu (b.subst (d + 1) w)
  | .tuple fs, d, w => .tuple (fs.subst d w)
  | .struct fs, d, w => .struct (fs.subst d w)
  | .table cs, d, w => .table (cs.subst d w)
  | .list t, d, w => .list (t.subst d w)
  | .set t, d, w => .set (t.subst d w)
  | .map k v, d, w => .map (k.subst d w) (v.subst d w)
  | .option t, d, w => .option (t.subst d w)
  | .result t, d, w => .result (t.subst d w)
  | .tensor t r, d, w => .tensor (t.subst d w) r
  | .term a t, d, w => .term a (t.subst d w)
  | .enum vs, d, w => .enum (vs.subst d w)
  | .bool, _, _ => .bool
  | .u8, _, _ => .u8
  | .i8, _, _ => .i8
  | .u16, _, _ => .u16
  | .i16, _, _ => .i16
  | .u32, _, _ => .u32
  | .i32, _, _ => .i32
  | .u64, _, _ => .u64
  | .i64, _, _ => .i64
  | .index, _, _ => .index
  | .offset, _, _ => .offset
  | .f32, _, _ => .f32
  | .f64, _, _ => .f64
  | .int, _, _ => .int
  | .string, _, _ => .string
  | .data, _, _ => .data
  | .error, _, _ => .error
  | .atom a, _, _ => .atom a
  | .var n, _, _ => .var n

def Tys.subst : Tys → Nat → Ty → Tys
  | .nil, _, _ => .nil
  | .cons t ts, d, w => .cons (t.subst d w) (ts.subst d w)

def Fields.subst : Fields → Nat → Ty → Fields
  | .nil, _, _ => .nil
  | .cons n t fs, d, w => .cons n (t.subst d w) (fs.subst d w)

def Variants.subst : Variants → Nat → Ty → Variants
  | .nil, _, _ => .nil
  | .atom n vs, d, w => .atom n (vs.subst d w)
  | .term n t vs, d, w => .term n (t.subst d w) (vs.subst d w)

end

namespace Ty

/-- One level down: the body with the whole put back where its name was.
Anything that is not a `mu` is its own unfolding. -/
def unfold : Ty → Ty
  | mu b => b.subst 0 (mu b)
  | t => t

/-- How many `mu`s a type starts with. -/
def leadingMus : Ty → Nat
  | mu b => b.leadingMus + 1
  | _ => 0

/-- Unfold `n` times while the head is a `mu`. -/
def exposeN : Nat → Ty → Ty
  | 0, t => t
  | n + 1, mu b => exposeN n (unfold (mu b))
  | _ + 1, t => t

/-- A type with any `mu` at its head unfolded, as `expose` in `types.rs`.

The Rust loops until the head is not a `rec`. On a well-formed type each
unfolding takes off one leading `mu` (`expose_not_mu` in `Proofs/Expose.lean`),
so as many unfoldings as there are leading `mu`s is the same loop. -/
def expose (t : Ty) : Ty := exposeN t.leadingMus t

end Ty

end Datalove
