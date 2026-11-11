import Datalove.Ty

/-!
# Relations between types

Each function here is a transcription of the Rust one named in its docs, kept
to the same cases in the same order, so that the oracle answers what the Rust
answers. The proofs about them are in `Datalove/Proofs`.
-/

namespace Datalove

mutual

/-- `T ≡ U`, as `types_equivalent` in `types.rs`. -/
def Ty.equiv : Ty → Ty → Bool
  | .bool, .bool | .u8, .u8 | .i8, .i8 | .u16, .u16 | .i16, .i16
  | .u32, .u32 | .i32, .i32 | .u64, .u64 | .i64, .i64
  | .index, .index | .offset, .offset | .f32, .f32 | .f64, .f64
  | .int, .int | .string, .string | .data, .data | .error, .error => true
  | .var a, .var b => a == b
  | .mu b1, .mu b2 => b1.equiv b2
  | .recvar a, .recvar b => a == b
  | .tuple f1, .tuple f2 => f1.equiv f2
  | .struct f1, .struct f2 => f1.equiv f2
  | .list a, .list b => a.equiv b
  | .map k1 v1, .map k2 v2 => k1.equiv k2 && v1.equiv v2
  | .set a, .set b => a.equiv b
  | .option a, .option b => a.equiv b
  | .result a, .result b => a.equiv b
  | .tensor a r1, .tensor b r2 => r1 == r2 && a.equiv b
  | .table c1, .table c2 => c1.equiv c2
  | .atom a, .atom b => a == b
  | .term a t1, .term b t2 => a == b && t1.equiv t2
  | .enum v1, .enum v2 => v1.equiv v2
  | _, _ => false

def Tys.equiv : Tys → Tys → Bool
  | .nil, .nil => true
  | .cons a as, .cons b bs => a.equiv b && as.equiv bs
  | _, _ => false

def Fields.equiv : Fields → Fields → Bool
  | .nil, .nil => true
  | .cons n1 a as, .cons n2 b bs => n1 == n2 && a.equiv b && as.equiv bs
  | _, _ => false

def Variants.equiv : Variants → Variants → Bool
  | .nil, .nil => true
  | .atom n1 as, .atom n2 bs => n1 == n2 && as.equiv bs
  | .term n1 a as, .term n2 b bs => n1 == n2 && a.equiv b && as.equiv bs
  | _, _ => false

end

namespace Ty

/-- `T ≈ U`, meeting at the head, as `types_equivalent_at_head`. -/
def meet (actual expected : Ty) : Bool :=
  actual.equiv expected
    || (actual.isMu && actual.expose.equiv expected)
    || (expected.isMu && actual.equiv expected.expose)

/-- Numeric widening along a chain, as `can_widen_to`. -/
def canWiden : Ty → Ty → Bool
  | .u8, .u16 | .u8, .u32 | .u8, .u64 | .u8, .int => true
  | .u16, .u32 | .u16, .u64 | .u16, .int => true
  | .u32, .u64 | .u32, .int => true
  | .u64, .int => true
  | .i8, .i16 | .i8, .i32 | .i8, .i64 | .i8, .int => true
  | .i16, .i32 | .i16, .i64 | .i16, .int => true
  | .i32, .i64 | .i32, .int => true
  | .i64, .int => true
  | .index, .int => true
  | .offset, .int => true
  | _, _ => false

end Ty

/-- Whether an enum lists the atom `a`. -/
def Variants.hasAtom (a : String) : Variants → Bool
  | .nil => false
  | .atom n vs => n == a || vs.hasAtom a
  | .term _ _ vs => vs.hasAtom a

/-- Whether an enum lists the term `a` with a payload equivalent to `t`. -/
def Variants.hasTerm (a : String) (t : Ty) : Variants → Bool
  | .nil => false
  | .atom _ vs => vs.hasTerm a t
  | .term n p vs => (n == a && t.equiv p) || vs.hasTerm a t

namespace Ty

/-- The conversions of `can_adapt_to` past meeting and widening: unsigned
to wider signed, `f32` to `f64`, and an atom or term into an enum that lists
it. -/
def adaptTable : Ty → Ty → Bool
  | .u8, .i16 | .u8, .i32 | .u8, .i64 | .u8, .int => true
  | .u16, .i32 | .u16, .i64 | .u16, .int => true
  | .u32, .i64 | .u32, .int => true
  | .f32, .f64 => true
  | .atom a, .enum vs => vs.hasAtom a
  | .term a t, .enum vs => vs.hasTerm a t
  | _, _ => false

/-- `T ⇝ U`, what `@` converts, as `can_adapt_to`. The table is consulted
with both sides exposed, so that a recursive type adapts as its unfolding
does. -/
def canAdapt (src dst : Ty) : Bool :=
  src.meet dst || src.canWiden dst || adaptTable src.expose dst.expose

end Ty

end Datalove
