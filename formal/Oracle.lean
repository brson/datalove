import Datalove

/-!
# The oracle

Reads queries, one JSON object per line, from the file named on the command
line, and writes one answer per line to stdout, in order:

```
{"convert": H}   →  {"ok": T} or {"err": {"kind": K, "name": N}}
{"pair": [A, B]} →  {"equiv": b, "meet": b, "adapt": b}
{"type": T}      →  {"copy": b, "wf": b, "expose": T}
{"synth": E}     →  {"ok": T} or {"err": code}
{"fun": S}       →  {"codes": [code, ...]} or {"unsupported": what}
```

A script `S` is the JSON `datalove-datafun-ast`'s serde AST writes.

`synth` on a hinted expression `: T / e` is checking `e` against `T`, so it
asks about both modes.

Expressions are encoded as `Datalove/LitJson.lean` says.

`crates/datalove-formal-tests` asks the Rust the same questions and compares.
-/

open Lean (Json)
open Datalove

def answer (q : Json) : Except String Json := do
  if let .ok h := q.getObjVal? "convert" then
    let h ← Hint.fromJson h
    return match h.convert with
      | .ok t => Json.mkObj [("ok", t.toJson)]
      | .error e => Json.mkObj [("err", Json.mkObj [("kind", e.kind.toJson), ("name", e.name)])]
  if let .ok p := q.getObjVal? "pair" then
    let (a, b) ← pair p
    let a ← Ty.fromJson a
    let b ← Ty.fromJson b
    return Json.mkObj [("equiv", a.equiv b), ("meet", a.meet b), ("adapt", a.canAdapt b)]
  if let .ok t := q.getObjVal? "type" then
    let t ← Ty.fromJson t
    let wf := decide t.WF
    -- `expose` is the Rust's only on a well-formed type; elsewhere the Rust
    -- may not finish.
    let exposed := if wf then t.expose.toJson else Json.null
    return Json.mkObj [("copy", t.isCopy), ("wf", wf), ("expose", exposed)]
  if let .ok e := q.getObjVal? "synth" then
    let e ← Lit.Expr.fromJson e
    return match Lit.synthesize e with
      | .ok t => Json.mkObj [("ok", t.toJson)]
      | .error c => Json.mkObj [("err", c)]
  if let .ok e := q.getObjVal? "synth" then
    let e ← Lit.Expr.fromJson e
    return match Lit.synthesize e with
      | .ok t => Json.mkObj [("ok", t.toJson)]
      | .error c => Json.mkObj [("err", c)]
  if let .ok script := q.getObjVal? "fun" then
    return match Fun.scriptFromSerde script with
      | .ok ss => Json.mkObj [("codes", Json.arr ((Fun.checkScript ss).map Json.str))]
      | .error e => Json.mkObj [("unsupported", e)]
  if let .ok world := q.getObjVal? "world" then
    let read : Except String (List Fun.ModuleSrc) := do
      let ms ← (← world.getArr?).toList.mapM fun m => do
        Fun.moduleFromSerde (← (← m.getObjVal? "path").getStr?) (← m.getObjVal? "script")
      if !ms.all (·.body.all Fun.Stmt.isDecl) then throw "a module statement that is not a declaration"
      pure ms
    return match read >>= Fun.checkWorld with
      | .ok codes => Json.mkObj [("modules", Json.arr (codes.toArray.map fun cs => Json.arr (cs.map Json.str)))]
      | .error e => Json.mkObj [("unsupported", e)]
  throw s!"unknown query: {q.compress}"

def main (args : List String) : IO UInt32 := do
  let [path] := args
    | IO.eprintln "usage: datalove-oracle QUERIES.jsonl"; return 2
  let out ← IO.getStdout
  for line in (← IO.FS.lines path) do
    if line.trimAscii.isEmpty then continue
    match Json.parse line >>= answer with
    | .ok a => out.putStrLn a.compress
    | .error e =>
      IO.eprintln s!"bad query: {e}\n  {line}"
      return 1
  return 0
