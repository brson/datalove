import Datalove.FunMove

/-!
# What is dropped where

The ownership analysis decides, for each point a function leaves a scope by,
which bindings still own a value and are dropped there: the end of an `if`'s
branch or a `match`'s arm, a loop body's end, a `break`, a `continue`, a
`ret`, a `?`, and the end of the body (`DropSchedule` in
`datalove-datafun-sema`). Lowering only carries the schedule out, so the
schedule is where a drop is decided right or wrong.

The Rust keys each point by the index it gives the statement there, counting
every statement of the body in order, each before what it holds, dead ones
too. `stmtKinds` numbers a body as the Rust does, by the names the Rust's
`StmtKey::kind_name` gives each kind of statement, so that `drops_agree` can
first hold the two numberings to each other; the schedule is held to the
Rust's against them.
-/

namespace Datalove.Fun

/-- The name the Rust's `StmtKey::kind_name` gives a statement's kind. -/
def stmtKind : Stmt → String
  | .let_ .. => "Let"
  | .var_ .. => "Var"
  | .set_ .. => "Set"
  | .fun_ _ => "Fun"
  | .ret _ => "Ret"
  | .if_ .. => "If"
  | .loop .. => "Loop"
  | .brk => "Break"
  | .cont => "Continue"
  | .debuglog _ => "DebugLog"
  | .call _ => "ExprStatement"
  | .for_ .. => "For"
  | .const_ .. => "Const"
  | .type_ .. => "TypeAlias"
  | .match_ .. => "Match"

mutual

/-- A body's statements in the order the Rust numbers them: each before the
blocks it holds, a nested function's body not at all, which is analyzed
apart. -/
def stmtKinds : List Stmt → List String
  | [] => []
  | s :: ss => stmtKind s :: (innerKinds s ++ stmtKinds ss)

/-- The statements a statement holds, in order: an `if`'s then branch before
its else, a `match`'s cases in order and its default last. -/
def innerKinds : Stmt → List String
  | .if_ _ _ _ t e => stmtKinds t ++ elseKinds e
  | .loop _ body => stmtKinds body
  | .for_ _ _ body => stmtKinds body
  | .match_ _ cases dflt => caseKinds cases ++ elseKinds dflt
  | _ => []

def elseKinds : Option (List Stmt) → List String
  | some ss => stmtKinds ss
  | none => []

def caseKinds : List Case → List String
  | [] => []
  | .atom _ b :: cs => stmtKinds b ++ caseKinds cs
  | .term _ _ b :: cs => stmtKinds b ++ caseKinds cs

end

/-- Each function a script declares at its top level, by name, with its
body's statements numbered as the Rust numbers them. -/
def scriptFunKinds (ss : List Stmt) : List (String × List String) :=
  ss.filterMap fun
    | .fun_ (.mk name _ _ _ body) => some (name, stmtKinds body)
    | _ => none

end Datalove.Fun
