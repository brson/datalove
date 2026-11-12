import Lean.Data.Json
import Datalove.Json
import Datalove.LitJson
import Datalove.Fun

/-!
# Datafun scripts from their serde AST

`crates/datalove-datafun-ast/src/ast_serde.rs` serializes a parsed script,
and `datalove-datalit`'s `ast_serde.rs` its type hints; this reads that JSON
as it is. A form outside the slice `Fun.lean` models is refused with
`unsupported`, and the test counts the scripts it skips.
-/

namespace Datalove.Fun

open Lean (Json)

def unsupported {α} (what : String) : Except String α := throw s!"unsupported: {what}"

def field (j : Json) (k : String) : Except String Json := j.getObjVal? k

/-- A field that may be absent or null. -/
def optField (j : Json) (k : String) : Option Json :=
  match j.getObjVal? k with
  | .ok Json.null => Option.none
  | .ok v => Option.some v
  | .error _ => Option.none

def hintNames : List (String × Hint) :=
  [("Bool", .bool), ("U8", .u8), ("I8", .i8), ("U16", .u16), ("I16", .i16),
   ("U32", .u32), ("I32", .i32), ("U64", .u64), ("I64", .i64), ("Index", .index),
   ("Offset", .offset), ("F32", .f32), ("F64", .f64), ("Int", .int), ("String", .string),
   ("Data", .data), ("Error", .error)]

/-- A datalit type hint as serde writes it: a unit variant as its name, any
other as an object with one key. -/
partial def hintFromSerde (j : Json) : Except String Hint := do
  if let .ok s := j.getStr? then
    match hintNames.lookup s with
    | Option.some h => return h
    | Option.none => throw s!"unknown hint {s}"
  let .ok (.obj kvs) := (pure j : Except String Json) | throw s!"not a hint: {j.compress}"
  let [⟨k, v⟩] := kvs.toList | throw s!"not a hint: {j.compress}"
  let named (fs : Json) : Except String HFields := do
    let mut out := HFields.nil
    for f in (← fs.getArr?).reverse do
      out := .cons (← (← field f "name").getStr?) (← hintFromSerde (← field f "type_hint")) out
    pure out
  match k with
  | "AnonTuple" => do
    let mut out := Hints.nil
    for t in (← (← field v "fields").getArr?).reverse do out := .cons (← hintFromSerde t) out
    pure (.tuple out)
  | "AnonStruct" => .struct <$> named (← field v "fields")
  | "Table" => .table <$> named (← field v "columns")
  | "List" => .list <$> hintFromSerde (← field v "element_type")
  | "Set" => .set <$> hintFromSerde (← field v "element_type")
  | "Map" => .map <$> hintFromSerde (← field v "key_type") <*> hintFromSerde (← field v "value_type")
  | "Option" => .option <$> hintFromSerde (← field v "inner_type")
  | "Result" => .result <$> hintFromSerde (← field v "inner_type")
  | "Tensor" => .tensor <$> hintFromSerde (← field v "element_type") <*> (← field v "rank").getNat?
  | "Alias" => .name <$> v.getStr?
  | "Atom" => .atom <$> (← field v "name").getStr?
  | "Term" => .term <$> (← field v "name").getStr? <*> hintFromSerde (← field v "payload")
  | "Enum" => do
    let mut out := HVariants.nil
    for var in (← (← field v "variants").getArr?).reverse do
      let n ← (← field var "name").getStr?
      out ← match optField var "payload" with
        | Option.none => pure (.atom n out)
        | Option.some p => do pure (.term n (← hintFromSerde p) out)
    pure (.enum out)
  | "Rec" => .mu <$> (← field v "name").getStr? <*> hintFromSerde (← field v "body")
  | other => unsupported s!"hint {other}"

def binOpOf : String → Except String BinOp
  | "Add" => pure .add | "Sub" => pure .sub | "Mul" => pure .mul | "Div" => pure .div
  | "AddChecked" => pure .addC | "SubChecked" => pure .subC
  | "MulChecked" => pure .mulC | "DivChecked" => pure .divC
  | "AddOptional" => pure .addO | "SubOptional" => pure .subO
  | "MulOptional" => pure .mulO | "DivOptional" => pure .divO
  | "Lt" => pure .lt | "Gt" => pure .gt | "Le" => pure .le | "Ge" => pure .ge
  | "Eq" => pure .eq | "Ne" => pure .ne
  | "And" => pure .and | "Or" => pure .or | "Xor" => pure .xor
  | op => throw s!"unknown operator {op}"

def unOpOf : String → Except String UnOp
  | "Neg" => pure .neg | "NegOptional" => pure .negO | "NegResult" => pure .negC
  | "Not" => pure .not
  | op => throw s!"unknown operator {op}"

def modeOf : String → Except String Mode
  | "In" => pure .in_ | "Out" => pure .out | "Ref" => pure .ref | "Mut" => pure .mut
  | m => throw s!"unknown mode {m}"

def digits (s : String) : String := String.ofList (s.toList.filter (· != '_'))

partial def exprFromSerde (j : Json) : Except String Expr := do
  let e ← field j "expr"
  let kind ← (← field e "kind").getStr?
  let sub (k : String) : Except String Expr := do exprFromSerde (← field e k)
  let list (k : String) : Except String (List Expr) := do
    (← (← field e k).getArr?).toList.mapM exprFromSerde
  let core : Expr ← match kind with
    | "True" => pure .tt
    | "False" => pure .ff
    | "None" => pure .none_
    | "Float" => pure .float
    | "String" => pure .str
    | "Int" =>
      let s := digits (← (← field e "value").getStr?)
      match s.toInt? with
      | Option.some n => pure (.int n)
      | Option.none => throw s!"not an integer: {s}"
    | "Hex" =>
      let s := digits (← (← field e "value").getStr?)
      .hex <$> Lit.hexValue ((s.drop 2).toString)
    | "List" => .list <$> list "elements"
    | "Set" => .set <$> list "elements"
    | "AnonTuple" | "Tuple" => .tuple <$> list "elements"
    | "Tensor" => do
      let shape ← (← (← field e "shape").getArr?).toList.mapM (·.getNat?)
      pure (.tensor shape (← list "elements"))
    | "Map" => do
      let es ← (← (← field e "entries").getArr?).toList.mapM fun en => do
        pure (← exprFromSerde (← field en "key"), ← exprFromSerde (← field en "value"))
      pure (.map es)
    | "AnonStruct" => do
      let fs ← (← (← field e "fields").getArr?).toList.mapM fun f => do
        pure (← (← field f "name").getStr?, ← exprFromSerde (← field f "value"))
      pure (.struct fs)
    | "Table" => do
      let header ← (← (← field e "header").getArr?).toList.mapM (·.getStr?)
      let rows ← (← (← field e "rows").getArr?).toList.mapM fun r => do
        (← (← field r "elements").getArr?).toList.mapM exprFromSerde
      pure (.table header rows)
    | "Some" => .some_ <$> sub "payload"
    | "Ok" => .ok <$> sub "payload"
    | "Er" => .er <$> sub "payload"
    | "Data" => .data <$> sub "value"
    | "Error" => .error <$> sub "value"
    | "Atom" => .atom <$> (← field e "name").getStr?
    | "Term" => .term <$> (← field e "name").getStr? <*> sub "payload"
    | "EnumLiteral" => .enum <$> sub "variant"
    | "Hinted" => sub "inner"
    | "Place" => do
      let steps ← (← field e "steps").getArr?
      if steps.size != 0 then unsupported "projection" else .var <$> (← field e "root").getStr?
    | "BinOp" => .binop <$> binOpOf (← (← field e "op").getStr?) <*> sub "lhs" <*> sub "rhs"
    | "UnaryOp" => .unop <$> unOpOf (← (← field e "op").getStr?) <*> sub "operand"
    | "TryOption" => .tryOpt <$> sub "operand"
    | "TryResult" => .tryRes <$> sub "operand"
    | "CloneCoerce" => .adapt <$> sub "operand"
    | "FunctionCall" => do
      if (optField e "qualifier").isSome then unsupported "qualified call"
      let args ← list "args"
      let modes ← match optField e "arg_modes" with
        | Option.none => pure (args.map fun _ => Option.none)
        | Option.some ms => (← ms.getArr?).toList.mapM fun m =>
          if m.isNull then pure Option.none else do pure (Option.some (← modeOf (← m.getStr?)))
      pure (.call (← (← field e "name").getStr?) (modes.zip args))
    | other => unsupported other
  -- A literal carries its own hint; `Hinted` its, around what is inside.
  match optField e "type_hint" with
  | Option.some h => pure (.hinted (← hintFromSerde h) core)
  | Option.none => pure core

mutual

partial def stmtFromSerde (j : Json) : Except String Stmt := do
  let kind ← (← field j "kind").getStr?
  let hint : Except String (Option Hint) := match optField j "type_hint" with
    | Option.none => pure Option.none
    | Option.some h => Option.some <$> hintFromSerde h
  match kind with
  | "Let" =>
    if (optField j "pattern").isSome then unsupported "pattern"
    .let_ <$> (← field j "name").getStr? <*> hint <*> exprFromSerde (← field j "value")
  | "Var" =>
    if (optField j "pattern").isSome then unsupported "pattern"
    let value ← match optField j "value" with
      | Option.none => pure Option.none
      | Option.some v => Option.some <$> exprFromSerde v
    pure (.var_ (← (← field j "name").getStr?) (← hint) value)
  | "Set" => do
    let target ← field j "target"
    if (← (← field target "steps").getArr?).size != 0 then unsupported "projection"
    let op ← match optField j "op" with
      | Option.none => pure Option.none
      | Option.some o => Option.some <$> binOpOf (← o.getStr?)
    pure (.set_ (← (← field target "root").getStr?) op (← exprFromSerde (← field j "value")))
  | "Fun" => .fun_ <$> funFromSerde j
  | "Ret" => .ret <$> match optField j "value" with
    | Option.none => pure Option.none
    | Option.some v => Option.some <$> exprFromSerde v
  | "If" => do
    let opt (k : String) : Except String (Option String) := match optField j k with
      | Option.none => pure Option.none
      | Option.some v => Option.some <$> v.getStr?
    let elseBody ← match optField j "else_body" with
      | Option.none => pure Option.none
      | Option.some b => Option.some <$> body b
    pure (.if_ (← exprFromSerde (← field j "condition")) (← opt "then_binding")
      (← opt "else_binding") (← body (← field j "then_body")) elseBody)
  | "Loop" => do
    let c ← match optField j "condition" with
      | Option.none => pure Option.none
      | Option.some c => Option.some <$> exprFromSerde c
    pure (.loop c (← body (← field j "body")))
  | "Break" => pure .brk
  | "Continue" => pure .cont
  | "DebugLog" => .debuglog <$> exprFromSerde (← field j "value")
  | "ExprStatement" => .call <$> exprFromSerde (← field j "expr")
  | "For" => do
    let names ← (← (← field j "bindings").getArr?).toList.mapM (·.getStr?)
    pure (.for_ names (← exprFromSerde (← field j "iteratee")) (← body (← field j "body")))
  | other => unsupported other

partial def body (j : Json) : Except String (List Stmt) := do
  (← j.getArr?).toList.mapM stmtFromSerde

partial def funFromSerde (j : Json) : Except String FunDecl := do
  if let Option.some tp := optField j "type_params" then
    if (← tp.getArr?).size != 0 then unsupported "generic"
  let params ← (← (← field j "params").getArr?).toList.mapM fun p => do
    if (← (← field p "is_comptime").getBool?) then unsupported "const parameter"
    pure (← modeOf (← (← field p "mode").getStr?), ← (← field p "name").getStr?,
      ← hintFromSerde (← field p "type_hint"))
  let ret ← match optField j "return_type" with
    | Option.none => pure Option.none
    | Option.some h => Option.some <$> hintFromSerde h
  pure (.mk (← (← field j "name").getStr?) params ret (← body (← field j "body")))

end

def scriptFromSerde (j : Json) : Except String (List Stmt) := do
  body (← field j "statements")

end Datalove.Fun
