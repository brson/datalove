import Datalove.FunRun

/-!
# Printing values as `debuglog` does

For the oracle's `run` query: a value of a type, as the Rust's `debuglog`
prints it, so that `fun_tests` can hold what a script logs to what the model
runs it to. What the model does not carry, a float, an
error's message, a map's, a set's, a tensor's or a table's elements, prints
as `unknown`, which matches anything there.
-/

namespace Datalove.Fun

open Lit (Val tysList fieldsList)

/-- What the model does not know of a value, which matches anything. -/
def unknown : String := "\u0001"

/-- A string as `debuglog` prints it: quoted, with a quote, a backslash, a
newline, a carriage return and a tab escaped, and anything else as it is. -/
def showString (s : String) : String :=
  "\"" ++ String.join (s.toList.map fun c => match c with
    | '"' => "\\\""
    | '\\' => "\\\\"
    | '\n' => "\\n"
    | '\r' => "\\r"
    | '\t' => "\\t"
    | c => c.toString) ++ "\""

/-- A value of type `t`, as `debuglog` prints it. -/
partial def showVal (t : Ty) (v : Val) : String :=
  match t.expose, v with
  | .bool, .bool b => toString b
  | .string, .str s => showString s
  | .tuple ts, .tuple vs =>
    "(" ++ ", ".intercalate ((vs.zip (tysList ts)).map fun (w, u) => showVal u w) ++ ")"
  | .struct fs, .struct gs =>
    "{" ++ ", ".intercalate ((gs.zip (fieldsList fs)).map fun ((n, w), (_, u)) => n ++ " = " ++ showVal u w) ++ "}"
  | .list e, .list vs => "[" ++ ", ".intercalate (vs.map (showVal e)) ++ "]"
  | .option _, .none => "none"
  | .option p, .some w => "some " ++ showVal p w
  | .result p, .ok w => "ok " ++ showVal p w
  | .result _, .er _ => "er " ++ unknown
  | .atom _, .atom a => "atom " ++ a
  | .term _ p, .term a w => "term " ++ a ++ " " ++ showVal p w
  | .enum _, .atom a => "enum { atom " ++ a ++ " }"
  | .enum vs, .term a w => match Lit.Variants.termPayload a vs with
    | some p => "enum { term " ++ a ++ " " ++ showVal p w ++ " }"
    | none => unknown
  | u, w => match numOf w with
    | some n => if u.isFloat then unknown else toString n
    | none => unknown

/-- How a run ended, by name. -/
def Stop.name : Stop → String
  | .early _ => "early"
  | .unsupported => "unsupported"
  | .uninit => "uninit"
  | .diverge => "diverge"
  | .fuel => "fuel"
  | .wrong => "wrong"
  | .retVoid => "retVoid"
  | .brk _ => "brk"
  | .cont _ => "cont"
  | .returned _ _ => "returned"

end Datalove.Fun
