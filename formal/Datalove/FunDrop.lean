import Datalove.FunInit

/-!
# What is dropped where: the statements' numbering

The ownership analysis decides, for each point a function leaves a scope by,
which bindings still own a value and are dropped there: the end of an `if`'s
branch or a `match`'s arm, a loop body's end, a `break`, a `continue`, a
`ret`, a `?`, and the end of the body (`DropSchedule` in
`datalove-datafun-sema`). Lowering only carries the schedule out, so the
schedule is where a drop is decided right or wrong. The move analysis keeps
it as it goes (`FunMove.lean`), as the Rust's does; this is what it needs of
a body that does not depend on what is moved.

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

/-- Whether an expression can return early from the function, as the Rust's
`early_return_in` finds it: a `?` or `!`, a checked or optional operator, an
index with `?` or `!`, or a place with an index step, anywhere in it. -/
partial def exprLeaves : Expr → Bool
  | .tryOpt _ | .tryRes _ => true
  | .binop op a c => exprLeaves a || exprLeaves c || op.exits
  | .unop op a => exprLeaves a || op == .negC || op == .negO
  | .call _ args => args.any (exprLeaves ·.2)
  | .tuple es | .list es | .set es | .tensor _ es => es.any exprLeaves
  | .struct fs => fs.any (exprLeaves ·.2)
  | .map es => es.any fun (k, v) => exprLeaves k || exprLeaves v
  | .table _ rows => rows.any (·.any exprLeaves)
  | .some_ a | .ok a | .er a | .data a | .error a | .term _ a | .enum a | .hinted _ a | .adapt a
  | .proj a _ => exprLeaves a
  | .index b k m => m.isSome || exprLeaves b || exprLeaves k
  | .place _ steps => steps.any fun | .index .. => true | _ => false
  | _ => false

/-- How many expressions the parser files for `e` in its body's arena, each
after its parts (`Parser::push_expr`): one a node, but `e[k]` under `?` or
`!`, which the model takes as one form and the parser as an index under a
try, and a place, which is its root and one for each step, after the keys.
A `set`'s target files only its keys (`keyNodes`). An expression's
index in the arena, which the Rust's `ExprKey::local_index` is, is then how
many the body filed before its parts. -/
partial def Expr.nodes : Expr → Nat
  | .binop _ a c => a.nodes + c.nodes + 1
  | .unop _ a | .tryOpt a | .tryRes a | .some_ a | .ok a | .er a | .data a | .error a | .term _ a
  | .enum a | .hinted _ a | .adapt a | .proj a _ => a.nodes + 1
  | .call _ args => (args.map (·.2.nodes)).sum + 1
  | .tuple es | .list es | .set es | .tensor _ es => (es.map Expr.nodes).sum + 1
  | .struct fs => (fs.map (·.2.nodes)).sum + 1
  | .map es => (es.map fun (k, v) => k.nodes + v.nodes).sum + 1
  | .table _ rows => (rows.map fun r => (r.map Expr.nodes).sum).sum + 1
  | .index b k m => b.nodes + k.nodes + (if m.isSome then 2 else 1)
  | .place _ steps => keyNodes steps + steps.length + 1
  | _ => 1
where
  /-- The expressions a place's keys are. -/
  keyNodes : List Step → Nat
    | [] => 0
    | .field _ :: rest => keyNodes rest
    | .index k _ :: rest => k.nodes + keyNodes rest

/-- Each function a script declares at its top level, by name, with its
body's statements numbered as the Rust numbers them. -/
def scriptFunKinds (ss : List Stmt) : List (String × List String) :=
  ss.filterMap fun
    | .fun_ (.mk name _ _ _ body) => some (name, stmtKinds body)
    | _ => none

/-- A scheduled point: its kind, its statement, the arm for a `match`'s, and
the names of what it drops. -/
abbrev DropPoint := String × Nat × Option Nat × List String

end Datalove.Fun
