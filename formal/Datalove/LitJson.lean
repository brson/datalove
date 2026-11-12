import Lean.Data.Json
import Datalove.Json
import Datalove.Lit

/-!
# JSON encoding of expressions

How `crates/datalove-formal-tests` writes datalit expressions for the oracle.
A form with no parts is its name as a string; anything else is an array
whose first element names it:

```
"true" "false" "float" "string" "none"
["int", "-12"]  ["hex", "ff"]                  value, digits only
["tuple", [E, ...]]  ["struct", [["x", E], ...]]
["list", [E, ...]]  ["set", [E, ...]]  ["map", [[K, V], ...]]
["tensor", [d, ...], [E, ...]]  ["table", ["x", ...], [[E, ...], ...]]
["some", E]  ["ok", E]  ["er", E]  ["data", E]  ["error", E]
["atom", "A"]  ["term", "A", E]  ["enum", E]  ["group", E]
["hint", H, E]                                 a hint, in the encoding of Json.lean
```
-/

namespace Datalove.Lit

open Lean (Json)

def hexValue (digits : String) : Except String Nat :=
  digits.foldl (init := .ok 0) fun acc c => do
    let n ← acc
    let d ← if '0' ≤ c && c ≤ '9' then pure (c.toNat - '0'.toNat)
      else if 'a' ≤ c && c ≤ 'f' then pure (c.toNat - 'a'.toNat + 10)
      else if 'A' ≤ c && c ≤ 'F' then pure (c.toNat - 'A'.toNat + 10)
      else throw s!"not a hex digit: {c}"
    pure (n * 16 + d)

partial def Expr.fromJson (j : Json) : Except String Expr := do
  if let .ok s := j.getStr? then
    return ← match s with
      | "true" => pure .tt
      | "false" => pure .ff
      | "float" => pure .float
      | "string" => pure .str
      | "none" => pure .none
      | _ => throw s!"unknown expression {s}"
  let (tag, args) ← tagged j
  let arg (i : Nat) : Except String Json :=
    match args[i]? with
    | Option.some a => pure a
    | Option.none => throw s!"{tag}: missing argument {i}"
  let list (j : Json) : Except String (List Expr) := do
    (← j.getArr?).toList.mapM Expr.fromJson
  match tag with
  | "int" =>
    let s ← (← arg 0).getStr?
    match s.toInt? with
    | Option.some n => pure (.int n)
    | Option.none => throw s!"not an integer: {s}"
  | "hex" => .hex <$> hexValue (← (← arg 0).getStr?)
  | "tuple" => .tuple <$> list (← arg 0)
  | "struct" => do
    let fs ← (← (← arg 0).getArr?).toList.mapM fun f => do
      let (n, e) ← pair f
      pure (← n.getStr?, ← Expr.fromJson e)
    pure (.struct fs)
  | "list" => .list <$> list (← arg 0)
  | "set" => .set <$> list (← arg 0)
  | "map" => do
    let es ← (← (← arg 0).getArr?).toList.mapM fun f => do
      let (k, v) ← pair f
      pure (← Expr.fromJson k, ← Expr.fromJson v)
    pure (.map es)
  | "tensor" => do
    let shape ← (← (← arg 0).getArr?).toList.mapM (·.getNat?)
    pure (.tensor shape (← list (← arg 1)))
  | "table" => do
    let header ← (← (← arg 0).getArr?).toList.mapM (·.getStr?)
    let rows ← (← (← arg 1).getArr?).toList.mapM list
    pure (.table header rows)
  | "some" => .some <$> Expr.fromJson (← arg 0)
  | "ok" => .ok <$> Expr.fromJson (← arg 0)
  | "er" => .er <$> Expr.fromJson (← arg 0)
  | "data" => .data <$> Expr.fromJson (← arg 0)
  | "error" => .error <$> Expr.fromJson (← arg 0)
  | "atom" => .atom <$> (← arg 0).getStr?
  | "term" => .term <$> (← arg 0).getStr? <*> Expr.fromJson (← arg 1)
  | "enum" => .enum <$> Expr.fromJson (← arg 0)
  | "group" => .group <$> Expr.fromJson (← arg 0)
  | "hint" => .hinted <$> Hint.fromJson (← arg 0) <*> Expr.fromJson (← arg 1)
  | _ => throw s!"unknown expression constructor {tag}"

end Datalove.Lit
