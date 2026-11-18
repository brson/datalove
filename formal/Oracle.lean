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
{"run": S}       →  {"log": [line, ...], "end": how, "returned": line or null} or {"unsupported": what}
{"fun": S}       →  {"codes": [code, ...], "inits": [T or null, ...]} or {"unsupported": what}
{"alias": S}     →  {"codes": [code, ...], "calls": [b, ...]} or {"unsupported": what}
{"held": S}      →  {"codes": [code, ...], "held": [b, ...]} or {"unsupported": what}
{"init": S}      →  {"codes": [code, ...], "init": code or null} or {"unsupported": what}
{"consts": S}    →  {"codes": [code, ...], "consts": code or null} or {"unsupported": what}
```

A script `S` is the JSON `datalove-datafun-ast`'s serde AST writes. Its
`inits` are the types the checker synthesizes for each `let`, `var` and
`const` with a value and no hint, in source order (`Fun.traceScript`). A
`run` is the script run (`Fun.runScript`), each top-level `debuglog` printed
as the Rust prints it (`Fun.showVal`), what the model does not know of a
value as U+0001, and how it ended: `ok`, or the `Stop` it stopped at, with
what it returned early, which the CLI prints after its logs.

An `alias` is the script checked, and for each top-level `call`, in order,
whether its arguments are O010 (`Fun.aliasScript`). A `held` is the script
checked, and for each top-level `debuglog`, `call` and `set`, in order,
whether it is O017 (`Fun.heldScript`). An `init` is the script checked,
and what the analysis of what holds a value refuses first, O005, O006 or
O009, if anything (`Fun.initScript`). A `consts` is the script checked, and
F089 if one of its consts reads one not evaluated before it
(`Fun.constScript`).

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
      | .ok ss => Json.mkObj [("codes", Json.arr ((Fun.checkScript ss).map Json.str)),
          ("inits", Json.arr ((Fun.traceScript ss).toArray.map fun
            | .some t => t.toJson
            | .none => Json.null))]
      | .error e => Json.mkObj [("unsupported", e)]
  if let .ok script := q.getObjVal? "alias" then
    return match Fun.scriptFromSerde script with
      | .ok ss => Json.mkObj [("codes", Json.arr ((Fun.checkScript ss).map Json.str)),
          ("calls", Json.arr ((Fun.aliasScript ss).toArray.map Json.bool))]
      | .error e => Json.mkObj [("unsupported", e)]
  if let .ok script := q.getObjVal? "held" then
    return match Fun.scriptFromSerde script with
      | .ok ss => Json.mkObj [("codes", Json.arr ((Fun.checkScript ss).map Json.str)),
          ("held", Json.arr ((Fun.heldScript ss).toArray.map Json.bool))]
      | .error e => Json.mkObj [("unsupported", e)]
  if let .ok script := q.getObjVal? "init" then
    return match Fun.scriptFromSerde script with
      | .ok ss => Json.mkObj [("codes", Json.arr ((Fun.checkScript ss).map Json.str)),
          ("init", match Fun.initScript ss with
            | .some c => Json.str c
            | .none => Json.null)]
      | .error e => Json.mkObj [("unsupported", e)]
  if let .ok script := q.getObjVal? "consts" then
    return match Fun.scriptFromSerde script with
      | .ok ss => Json.mkObj [("codes", Json.arr ((Fun.checkScript ss).map Json.str)),
          ("consts", match Fun.constScript ss with
            | .some c => Json.str c
            | .none => Json.null)]
      | .error e => Json.mkObj [("unsupported", e)]
  if let .ok script := q.getObjVal? "run" then
    return match Fun.scriptFromSerde script with
      | .ok ss =>
        let out := Fun.runScript 10000 100 ss
        Json.mkObj [("log", Json.arr (out.log.toArray.map fun (t, v) => Json.str (Fun.showVal t v))),
          ("end", match out.result with
            | .ok _ => "ok"
            | .error s => s.name),
          ("returned", match out.result with
            | .error (.early r) => Json.str (Fun.showVal (.result Fun.unitTy) r)
            | _ => Json.null)]
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
