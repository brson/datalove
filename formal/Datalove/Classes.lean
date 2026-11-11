import Datalove.Ty

/-!
# Classes of types
-/

namespace Datalove

mutual

/-- Whether a value of the type is copied rather than moved, as
`is_copy_type` in `crates/datalove-datafun-tycheck/src/lib.rs`. -/
def Ty.isCopy : Ty → Bool
  | .var _ => false
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64
  | .index | .offset | .f32 | .f64 => true
  | .int => false
  | .string | .list _ | .map _ _ | .set _ | .tensor _ _ => false
  | .data | .error => false
  | .tuple fs => fs.isCopy
  | .struct fs => fs.isCopy
  | .option t => t.isCopy
  | .result _ => false
  | .table _ => false
  | .mu _ | .recvar _ => false
  | .atom _ => true
  | .term _ t => t.isCopy
  | .enum vs => vs.isCopy

def Tys.isCopy : Tys → Bool
  | .nil => true
  | .cons t ts => t.isCopy && ts.isCopy

def Fields.isCopy : Fields → Bool
  | .nil => true
  | .cons _ t fs => t.isCopy && fs.isCopy

def Variants.isCopy : Variants → Bool
  | .nil => true
  | .atom _ vs => vs.isCopy
  | .term _ t vs => t.isCopy && vs.isCopy

end

end Datalove
