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

A `match` runs one of its cases or its default, and never falls through with
neither: that is what the exhaustiveness check guarantees of a `match` it lets
through (`Fun.checkMatch_exhaustive`, in `Proofs/Exhaustive.lean`), and what
`completes` relies on, as the Rust's `reachable.rs` does.
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
  | .let_ .. | .var_ .. | .set_ .. | .debuglog _ | .call _ | .fun_ _ | .type_ .. => true
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
  /-- A `match` runs the case of the variant its input holds... -/
  | matchCase : c ∈ cases → Exec c.body o → ExecS (.match_ e cases dflt) o
  /-- ...or its default. -/
  | matchDefault : Exec d o → ExecS (.match_ e cases (Option.some d)) o

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

theorem casesBreak_of_mem : ∀ {c : Case} {cases : List Case}, c ∈ cases →
    hasBreak c.body = true → casesBreak cases = true
  | _, [], hc, _ => by simp at hc
  | c, .atom _ b :: cs, hc, h => by
    simp only [List.mem_cons] at hc
    rcases hc with rfl | hc
    · simp [casesBreak, Case.body] at h ⊢; exact Or.inl h
    · simp [casesBreak, casesBreak_of_mem hc h]
  | c, .term _ _ b :: cs, hc, h => by
    simp only [List.mem_cons] at hc
    rcases hc with rfl | hc
    · simp [casesBreak, Case.body] at h ⊢; exact Or.inl h
    · simp [casesBreak, casesBreak_of_mem hc h]

theorem casesComplete_of_mem : ∀ {c : Case} {cases : List Case}, c ∈ cases →
    completes c.body = true → casesComplete cases = true
  | _, [], hc, _ => by simp at hc
  | c, .atom _ b :: cs, hc, h => by
    simp only [List.mem_cons] at hc
    rcases hc with rfl | hc
    · simp [casesComplete, Case.body] at h ⊢; exact Or.inl h
    · simp [casesComplete, casesComplete_of_mem hc h]
  | c, .term _ _ b :: cs, hc, h => by
    simp only [List.mem_cons] at hc
    rcases hc with rfl | hc
    · simp [casesComplete, Case.body] at h ⊢; exact Or.inl h
    · simp [casesComplete, casesComplete_of_mem hc h]

mutual

/-- A body that finishes with a `break` holds one belonging to it. Stated of
any outcome equal to a `break`, so that it recurses on the execution: a
`match`'s case is chosen from a list, and is no part of the statement. -/
theorem Exec.hasBreak_of_brk' : ∀ {ss : List Stmt} {o : Outcome}, Exec ss o → o = .brk →
    hasBreak ss = true
  | _, _, .nil, ho => by cases ho
  | _, _, .cons _ h, ho => by simp [hasBreak, Exec.hasBreak_of_brk' h ho]
  | _, _, .stop h _, ho => by simp [hasBreak, ExecS.hasBreakS_of_brk' h ho]

theorem ExecS.hasBreakS_of_brk' : ∀ {s : Stmt} {o : Outcome}, ExecS s o → o = .brk →
    hasBreakS s = true
  | _, _, .simple _, ho | _, _, .early _, ho | _, _, .ret, ho | _, _, .cont, ho
  | _, _, .ifSkip, ho => by cases ho
  | _, _, .brk, _ => rfl
  | _, _, .ifThen h, ho => by simp [hasBreakS, Exec.hasBreak_of_brk' h ho]
  | _, _, .ifElse h, ho => by simp [hasBreakS, hasBreakElse, Exec.hasBreak_of_brk' h ho]
  | _, _, .loop h, ho => absurd ho h.not_brk
  | _, _, .for_ h, ho => absurd ho h.not_brk
  | _, _, .matchCase hc h, ho => by
    simp [hasBreakS, casesBreak_of_mem hc (Exec.hasBreak_of_brk' h ho)]
  | _, _, .matchDefault h, ho => by
    simp [hasBreakS, hasBreakElse, Exec.hasBreak_of_brk' h ho]

end

theorem Exec.hasBreak_of_brk {ss : List Stmt} (h : Exec ss .brk) : hasBreak ss = true :=
  Exec.hasBreak_of_brk' h rfl

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
theorem Exec.completes_of_normal' : ∀ {ss : List Stmt} {o : Outcome}, Exec ss o → o = .normal →
    completes ss = true
  | _, _, .nil, _ => rfl
  | _, _, .cons hs h, ho => by
    simp [completes, ExecS.completesS_of_normal' hs rfl, Exec.completes_of_normal' h ho]
  | _, _, .stop _ hne, ho => absurd ho hne

theorem ExecS.completesS_of_normal' : ∀ {s : Stmt} {o : Outcome}, ExecS s o → o = .normal →
    completesS s = true
  | _, _, .simple h, _ => completesS_of_simple h
  | _, _, .early _, ho | _, _, .ret, ho | _, _, .brk, ho | _, _, .cont, ho => by cases ho
  | _, _, .ifThen (e := Option.none) _, _ => rfl
  | _, _, .ifThen (e := Option.some _) h, ho => by
    simp [completesS, Exec.completes_of_normal' h ho]
  | _, _, .ifElse h, ho => by simp [completesS, Exec.completes_of_normal' h ho]
  | _, _, .ifSkip, _ => rfl
  | _, _, .loop (c := Option.some _) _, _ => rfl
  | _, _, .loop (c := Option.none) h, ho => by
    simp [completesS, Loop.hasBreak_of_normal h rfl ho]
  | _, _, .for_ _, _ => rfl
  | _, _, .matchCase hc h, ho => by
    simp [completesS, casesComplete_of_mem hc (Exec.completes_of_normal' h ho)]
  | _, _, .matchDefault h, ho => by
    simp [completesS, completesElse, Exec.completes_of_normal' h ho]

end

/-- `completes` is sound: a body it says cannot complete has no execution
that finishes normally. -/
theorem completes_sound {ss : List Stmt} (h : completes ss = false) : ¬ Exec ss .normal :=
  fun e => by simp [Exec.completes_of_normal' e rfl] at h

end Datalove.Fun
