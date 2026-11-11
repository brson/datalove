import Datalove.Ty

/-!
# Well-formed types

The conditions `botdocs/datalit-typing-rules.md` puts on recursive types,
stated over `Ty` by de Bruijn index rather than over names:

- **T062**: a `mu` uses its variable.
- **T063** is about names, so it has no counterpart here; see `Hint.lean`.
- **T064**: between each use of a variable and its `mu` there is a list, map,
  set, tensor or table.
- **T065**: no `mu`, and no use of one's variable, is in a map's key or a
  set's element.
- **T066**: no type parameter is inside a `mu`.

and one the rules take for granted: every variable is bound.
-/

namespace Datalove

mutual

/-- Whether `t` uses the binder `d` levels out. -/
def Ty.mentions : Ty → Nat → Bool
  | .recvar i, d => i == d
  | .mu b, d => b.mentions (d + 1)
  | .tuple fs, d => fs.mentions d
  | .struct fs, d => fs.mentions d
  | .table cs, d => cs.mentions d
  | .list t, d | .set t, d | .option t, d | .result t, d
  | .tensor t _, d | .term _ t, d => t.mentions d
  | .map k v, d => k.mentions d || v.mentions d
  | .enum vs, d => vs.mentions d
  | _, _ => false

def Tys.mentions : Tys → Nat → Bool
  | .nil, _ => false
  | .cons t ts, d => t.mentions d || ts.mentions d

def Fields.mentions : Fields → Nat → Bool
  | .nil, _ => false
  | .cons _ t fs, d => t.mentions d || fs.mentions d

def Variants.mentions : Variants → Nat → Bool
  | .nil, _ => false
  | .atom _ vs, d => vs.mentions d
  | .term _ t vs, d => t.mentions d || vs.mentions d

end

/-- Where a part of a type sits, for the well-formedness check. -/
structure WfCtx where
  /-- For each enclosing `mu`, innermost first, how many collections were
  around it. -/
  binders : List Nat := []
  /-- How many collections are around this part. -/
  colls : Nat := 0
  /-- Whether this part is in a map's key or a set's element. -/
  inKey : Bool := false

namespace WfCtx

def coll (c : WfCtx) (key : Bool) : WfCtx :=
  { c with colls := c.colls + 1, inKey := c.inKey || key }

end WfCtx

mutual

def Ty.wfIn : Ty → WfCtx → Bool
  | .recvar i, c =>
    match c.binders[i]? with
    | none => false
    | some at_ => !c.inKey && at_ < c.colls
  | .mu b, c =>
    !c.inKey && b.mentions 0 && b.wfIn { c with binders := c.colls :: c.binders }
  | .var _, c => c.binders.isEmpty
  | .tuple fs, c => fs.wfIn c
  | .struct fs, c => fs.wfIn c
  | .table cs, c => cs.wfColumnsIn c
  | .list t, c => t.wfIn (c.coll false)
  | .set t, c => t.wfIn (c.coll true)
  | .map k v, c => k.wfIn (c.coll true) && v.wfIn (c.coll false)
  | .tensor t _, c => t.wfIn (c.coll false)
  | .option t, c | .result t, c | .term _ t, c => t.wfIn c
  | .enum vs, c => vs.wfIn c
  | .bool, _ | .u8, _ | .i8, _ | .u16, _ | .i16, _ | .u32, _ | .i32, _
  | .u64, _ | .i64, _ | .index, _ | .offset, _ | .f32, _ | .f64, _
  | .int, _ | .string, _ | .data, _ | .error, _ | .atom _, _ => true

def Tys.wfIn : Tys → WfCtx → Bool
  | .nil, _ => true
  | .cons t ts, c => t.wfIn c && ts.wfIn c

def Fields.wfIn : Fields → WfCtx → Bool
  | .nil, _ => true
  | .cons _ t fs, c => t.wfIn c && fs.wfIn c

def Fields.wfColumnsIn : Fields → WfCtx → Bool
  | .nil, _ => true
  | .cons _ t fs, c => t.wfIn (c.coll false) && fs.wfColumnsIn c

def Variants.wfIn : Variants → WfCtx → Bool
  | .nil, _ => true
  | .atom _ vs, c => vs.wfIn c
  | .term _ t vs, c => t.wfIn c && vs.wfIn c

end

/-- A closed, well-formed type. -/
def Ty.WF (t : Ty) : Prop := t.wfIn {} = true

instance (t : Ty) : Decidable t.WF := inferInstanceAs (Decidable (_ = true))

end Datalove
