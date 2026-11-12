import Datalove.Fun

/-!
# `completes` is sound

F063 refuses a function with a return type whose body can complete: reach its
end without returning. Whether it can is `completes`, the doc's conservative
test. `Fun.completes_sound` shows it is sound: a body `completes` says cannot
complete has no execution that finishes normally, so a function F063 lets
through always returns.

The semantics is of control flow alone. What an expression computes does not
matter here, only that any statement holding one may return early, through
`?`, `!`, the checked and optional operators or a call's failure, and that
a condition may be either way. Executions are the finite ones, an inductive
relation; a loop that runs forever has none, and finishes neither normally
nor otherwise.
-/

namespace Datalove.Fun

/-- How a statement or a body finishes. -/
inductive Outcome where
  | normal
  | brk
  | cont
  | ret
  deriving DecidableEq

/-- A statement that holds no control flow: it finishes normally, or returns
early from inside an expression. -/
def Stmt.isSimple : Stmt → Bool
  | .let_ .. | .var_ .. | .set_ .. | .debuglog _ | .call _ | .fun_ _ => true
  | _ => false

/-- Whether a statement may return early from an expression in it. -/
def Stmt.mayReturn : Stmt → Bool
  | .brk | .cont => false
  | _ => true

mutual

/-- A body runs to an outcome: each statement in turn, until one finishes
other than normally. -/
inductive Exec : List Stmt → Outcome → Prop where
  | nil : Exec [] .normal
  | cons : ExecS s .normal → Exec ss o → Exec (s :: ss) o
  | stop : ExecS s o → o ≠ .normal → Exec (s :: ss) o

inductive ExecS : Stmt → Outcome → Prop where
  | simple : s.isSimple = true → ExecS s .normal
  | early : s.mayReturn = true → ExecS s .ret
  | ret : ExecS (.ret e) .ret
  | brk : ExecS .brk .brk
  | cont : ExecS .cont .cont
  | ifThen : Exec t o → ExecS (.if_ c a b t e) o
  | ifElse : Exec e o → ExecS (.if_ c a b t (Option.some e)) o
  | ifSkip : ExecS (.if_ c a b t Option.none) .normal
  | loop : Loop c body o → ExecS (.loop c body) o
  /-- A `for` runs as a `loop while` does: it may run out at any round. -/
  | for_ : Loop (Option.some coll) body o → ExecS (.for_ names coll body) o

/-- A loop runs its body until a `break`, a return, or, for `loop while`, its
condition false. A `continue` or a normal end goes round again. -/
inductive Loop : Option Expr → List Stmt → Outcome → Prop where
  | done : Loop (Option.some c) body .normal
  | brk : Exec body .brk → Loop c body .normal
  | ret : Exec body .ret → Loop c body .ret
  | again : Exec body .normal → Loop c body o → Loop c body o
  | next : Exec body .cont → Loop c body o → Loop c body o

end

/-- A loop does not finish with a `break`: the `break` is its own. -/
theorem Loop.not_brk : ∀ {c : Option Expr} {body : List Stmt} {o : Outcome},
    Loop c body o → o ≠ .brk
  | _, _, _, .done => by simp
  | _, _, _, .brk _ => by simp
  | _, _, _, .ret _ => by simp
  | _, _, _, .again _ h => Loop.not_brk h
  | _, _, _, .next _ h => Loop.not_brk h

mutual

/-- A body that finishes with a `break` holds one belonging to it. -/
theorem Exec.hasBreak_of_brk : ∀ {ss : List Stmt}, Exec ss .brk → hasBreak ss = true
  | _, .cons _ h => by simp [hasBreak, Exec.hasBreak_of_brk h]
  | _, .stop h _ => by simp [hasBreak, ExecS.hasBreakS_of_brk h]

theorem ExecS.hasBreakS_of_brk : ∀ {s : Stmt}, ExecS s .brk → hasBreakS s = true
  | _, .brk => rfl
  | _, .ifThen h => by simp [hasBreakS, Exec.hasBreak_of_brk h]
  | _, .ifElse h => by simp [hasBreakS, hasBreakElse, Exec.hasBreak_of_brk h]
  | _, .loop h => absurd rfl h.not_brk
  | _, .for_ h => absurd rfl h.not_brk

end

/-- A bare `loop` that finishes normally holds a `break` belonging to it. -/
theorem Loop.hasBreak_of_normal : ∀ {c : Option Expr} {body : List Stmt} {o : Outcome},
    Loop c body o → c = Option.none → o = .normal → hasBreak body = true
  | _, _, _, .done, hc, _ => by cases hc
  | _, _, _, .brk hb, _, _ => Exec.hasBreak_of_brk hb
  | _, _, _, .ret _, _, ho => by cases ho
  | _, _, _, .again _ h, hc, ho => Loop.hasBreak_of_normal h hc ho
  | _, _, _, .next _ h, hc, ho => Loop.hasBreak_of_normal h hc ho

theorem completesS_of_simple {s : Stmt} (h : s.isSimple = true) : completesS s = true := by
  cases s <;> simp_all [Stmt.isSimple, completesS]

mutual

/-- A body that finishes normally is one `completes` says can. -/
theorem Exec.completes_of_normal : ∀ {ss : List Stmt}, Exec ss .normal → completes ss = true
  | _, .nil => rfl
  | _, .cons hs h => by
    simp [completes, ExecS.completesS_of_normal hs, Exec.completes_of_normal h]
  | _, .stop _ hne => absurd rfl hne

theorem ExecS.completesS_of_normal : ∀ {s : Stmt}, ExecS s .normal → completesS s = true
  | _, .simple h => completesS_of_simple h
  | _, .ifThen (e := Option.none) _ => rfl
  | _, .ifThen (e := Option.some _) h => by simp [completesS, Exec.completes_of_normal h]
  | _, .ifElse h => by simp [completesS, Exec.completes_of_normal h]
  | _, .ifSkip => rfl
  | _, .loop (c := Option.some _) _ => rfl
  | _, .loop (c := Option.none) h => by
    simp [completesS, Loop.hasBreak_of_normal h rfl rfl]
  | _, .for_ _ => rfl

end

/-- `completes` is sound: a body it says cannot complete has no execution
that finishes normally. -/
theorem completes_sound {ss : List Stmt} (h : completes ss = false) : ¬ Exec ss .normal :=
  fun e => by simp [Exec.completes_of_normal e] at h

end Datalove.Fun
