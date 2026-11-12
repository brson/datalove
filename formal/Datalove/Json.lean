import Lean.Data.Json
import Datalove.Ty
import Datalove.Hint

/-!
# JSON encoding

How the oracle and `crates/datalove-formal-tests` write types and hints to
each other. A scalar is its name as a string; anything else is an array
whose first element names the constructor:

```
"u8"  ["tuple", [T, ...]]  ["struct", [["x", T], ...]]  ["table", [["x", T], ...]]
["list", T]  ["map", K, V]  ["set", T]  ["option", T]  ["result", T]
["tensor", T, rank]  ["atom", "A"]  ["term", "A", T]  ["enum", [["A", null], ["B", T]]]
["var", "T"]  ["rec", T]  ["recvar", i]                                  types only
["rec", "L", H]  ["name", "L"]                                          hints only
```
-/

namespace Datalove

open Lean (Json)

def scalarNames : List (String × Ty) :=
  [("bool", .bool), ("u8", .u8), ("i8", .i8), ("u16", .u16), ("i16", .i16),
   ("u32", .u32), ("i32", .i32), ("u64", .u64), ("i64", .i64),
   ("index", .index), ("offset", .offset), ("f32", .f32), ("f64", .f64),
   ("int", .int), ("string", .string), ("data", .data), ("error", .error)]

def hintScalarNames : List (String × Hint) :=
  [("bool", .bool), ("u8", .u8), ("i8", .i8), ("u16", .u16), ("i16", .i16),
   ("u32", .u32), ("i32", .i32), ("u64", .u64), ("i64", .i64),
   ("index", .index), ("offset", .offset), ("f32", .f32), ("f64", .f64),
   ("int", .int), ("string", .string), ("data", .data), ("error", .error)]

mutual

def Ty.toJson : Ty → Json
  | .tuple fs => Json.arr #["tuple", Json.arr fs.toJsonArray]
  | .struct fs => Json.arr #["struct", Json.arr fs.toJsonArray]
  | .table cs => Json.arr #["table", Json.arr cs.toJsonArray]
  | .list t => Json.arr #["list", t.toJson]
  | .map k v => Json.arr #["map", k.toJson, v.toJson]
  | .set t => Json.arr #["set", t.toJson]
  | .option t => Json.arr #["option", t.toJson]
  | .result t => Json.arr #["result", t.toJson]
  | .tensor t r => Json.arr #["tensor", t.toJson, r]
  | .atom a => Json.arr #["atom", a]
  | .term a t => Json.arr #["term", a, t.toJson]
  | .enum vs => Json.arr #["enum", Json.arr vs.toJsonArray]
  | .var n => Json.arr #["var", n]
  | .mu b => Json.arr #["rec", b.toJson]
  | .recvar i => Json.arr #["recvar", i]
  | .bool => "bool" | .u8 => "u8" | .i8 => "i8" | .u16 => "u16" | .i16 => "i16"
  | .u32 => "u32" | .i32 => "i32" | .u64 => "u64" | .i64 => "i64"
  | .index => "index" | .offset => "offset" | .f32 => "f32" | .f64 => "f64"
  | .int => "int" | .string => "string" | .data => "data" | .error => "error"

def Tys.toJsonArray : Tys → Array Json
  | .nil => #[]
  | .cons t ts => #[t.toJson] ++ ts.toJsonArray

def Fields.toJsonArray : Fields → Array Json
  | .nil => #[]
  | .cons n t fs => #[Json.arr #[n, t.toJson]] ++ fs.toJsonArray

def Variants.toJsonArray : Variants → Array Json
  | .nil => #[]
  | .atom n vs => #[Json.arr #[n, Json.null]] ++ vs.toJsonArray
  | .term n t vs => #[Json.arr #[n, t.toJson]] ++ vs.toJsonArray

end

/-- A JSON value as an array whose first element is a string tag. -/
def tagged (j : Json) : Except String (String × Array Json) := do
  let a ← j.getArr?
  if h : 0 < a.size then
    let tag ← a[0].getStr?
    pure (tag, a.extract 1 a.size)
  else throw "empty array"

def pair (j : Json) : Except String (Json × Json) := do
  let a ← j.getArr?
  if h : a.size = 2 then pure (a[0], a[1]) else throw s!"expected a pair: {j}"

partial def Ty.fromJson (j : Json) : Except String Ty := do
  if let .ok s := j.getStr? then
    match scalarNames.lookup s with
    | some t => return t
    | none => throw s!"unknown scalar type {s}"
  let (tag, args) ← tagged j
  let arg (i : Nat) : Except String Json :=
    match args[i]? with
    | some a => pure a
    | none => throw s!"{tag}: missing argument {i}"
  let named (j : Json) : Except String Fields := do
    let mut out := Fields.nil
    for f in (← j.getArr?).reverse do
      let (n, t) ← pair f
      out := .cons (← n.getStr?) (← Ty.fromJson t) out
    pure out
  match tag with
  | "tuple" => do
    let mut out := Tys.nil
    for t in (← (← arg 0).getArr?).reverse do
      out := .cons (← Ty.fromJson t) out
    pure (.tuple out)
  | "struct" => .struct <$> named (← arg 0)
  | "table" => .table <$> named (← arg 0)
  | "list" => .list <$> Ty.fromJson (← arg 0)
  | "map" => .map <$> Ty.fromJson (← arg 0) <*> Ty.fromJson (← arg 1)
  | "set" => .set <$> Ty.fromJson (← arg 0)
  | "option" => .option <$> Ty.fromJson (← arg 0)
  | "result" => .result <$> Ty.fromJson (← arg 0)
  | "tensor" => .tensor <$> Ty.fromJson (← arg 0) <*> (← arg 1).getNat?
  | "atom" => .atom <$> (← arg 0).getStr?
  | "term" => .term <$> (← arg 0).getStr? <*> Ty.fromJson (← arg 1)
  | "enum" => do
    let mut out := Variants.nil
    for v in (← (← arg 0).getArr?).reverse do
      let (n, p) ← pair v
      let n ← n.getStr?
      out ← if p.isNull then pure (.atom n out) else do pure (.term n (← Ty.fromJson p) out)
    pure (.enum out)
  | "var" => .var <$> (← arg 0).getStr?
  | "rec" => .mu <$> Ty.fromJson (← arg 0)
  | "recvar" => .recvar <$> (← arg 0).getNat?
  | _ => throw s!"unknown type constructor {tag}"

partial def Hint.fromJson (j : Json) : Except String Hint := do
  if let .ok s := j.getStr? then
    match hintScalarNames.lookup s with
    | some t => return t
    | none => throw s!"unknown scalar type {s}"
  let (tag, args) ← tagged j
  let arg (i : Nat) : Except String Json :=
    match args[i]? with
    | some a => pure a
    | none => throw s!"{tag}: missing argument {i}"
  let named (j : Json) : Except String HFields := do
    let mut out := HFields.nil
    for f in (← j.getArr?).reverse do
      let (n, t) ← pair f
      out := .cons (← n.getStr?) (← Hint.fromJson t) out
    pure out
  match tag with
  | "tuple" => do
    let mut out := Hints.nil
    for t in (← (← arg 0).getArr?).reverse do
      out := .cons (← Hint.fromJson t) out
    pure (.tuple out)
  | "struct" => .struct <$> named (← arg 0)
  | "table" => .table <$> named (← arg 0)
  | "list" => .list <$> Hint.fromJson (← arg 0)
  | "map" => .map <$> Hint.fromJson (← arg 0) <*> Hint.fromJson (← arg 1)
  | "set" => .set <$> Hint.fromJson (← arg 0)
  | "option" => .option <$> Hint.fromJson (← arg 0)
  | "result" => .result <$> Hint.fromJson (← arg 0)
  | "tensor" => .tensor <$> Hint.fromJson (← arg 0) <*> (← arg 1).getNat?
  | "atom" => .atom <$> (← arg 0).getStr?
  | "term" => .term <$> (← arg 0).getStr? <*> Hint.fromJson (← arg 1)
  | "enum" => do
    let mut out := HVariants.nil
    for v in (← (← arg 0).getArr?).reverse do
      let (n, p) ← pair v
      let n ← n.getStr?
      out ← if p.isNull then pure (.atom n out) else do pure (.term n (← Hint.fromJson p) out)
    pure (.enum out)
  | "rec" => .mu <$> (← arg 0).getStr? <*> Hint.fromJson (← arg 1)
  | "name" => .name <$> (← arg 0).getStr?
  | _ => throw s!"unknown hint constructor {tag}"

def ConvErrKind.toJson : ConvErrKind → Json
  | .unused => "unused"
  | .shadowed => "shadowed"
  | .unguarded => "unguarded"
  | .inKey => "in_key"
  | .unbound => "unbound"
  | .generic => "generic"

end Datalove
