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

/-- The drop schedule as it is built: the next statement's index, the next
binding's, the bindings a loop turned out to give a value they were entered
without (the Rust's `maybe_held`), each binding's name, and the points
scheduled so far, each a kind of point, its statement, the arm for a
`match`'s, and what it drops. -/
structure DSt where
  next : Nat := 0
  uid : Nat := 0
  held : List Nat := []
  names : List (Nat × String) := []
  out : List (String × Nat × Option Nat × List Nat) := []

abbrev DropM := StateT DSt (Except Lit.Code)

/-- The next statement's index, as the Rust's `alloc_stmt_id` gives it. -/
def stmtId : DropM Nat := modifyGet fun s => (s.next, { s with next := s.next + 1 })

/-- Bind `x`, a binding of its own, owned where its type in `Γ'`, the
context the statement leaves, is not copy and it is neither borrowed nor a
const. -/
def bindOne (Γ' : Ctx) (konst : Bool) (init lent writable byRef : Bool) (σ : MSt) (x : String) :
    DropM MSt := do
  let s ← get
  let owns := !lent && !konst && match Γ'.lookup x with
    | some (t, _) => !t.isCopy
    | none => false
  set { s with uid := s.uid + 1, names := (s.uid, x) :: s.names }
  pure ({ name := x, init, live := true, back := false, lent, writable, byRef, owns, uid := s.uid } :: σ)

def bindD (Γ' : Ctx) (σ : MSt) (xs : List String) (init : Bool) (writable : Bool := false)
    (konst : Bool := false) : DropM MSt :=
  xs.foldlM (bindOne Γ' konst init false writable false) σ

/-- What of `vs` is dropped where it is: owned, and live or perhaps holding a
value (the Rust's `owned_here`). -/
def ownedIn (vs : MSt) : DropM (List Nat) := do
  let held := (← get).held
  pure ((vs.filter fun v => v.owns && (v.live || held.contains v.uid)).map (·.uid))

/-- Schedule `drops` at a point, where `always`, or where it drops anything. -/
def record (kind : String) (stmt : Nat) (arm : Option Nat) (drops : List Nat) (always := false) : DropM Unit :=
  if always || !drops.isEmpty then
    modify fun s => { s with out := (kind, stmt, arm, drops) :: s.out }
  else pure ()

/-- The bindings a scope made, of `τ`, its state where it ends, entered at
`σ`. -/
def scopeOwn (σ τ : MSt) : MSt := List.take (τ.length - σ.length) τ

/-- What a loop turns out to perhaps hold, as the Rust's `check_loop_repeats`
finds it: a `var` moved when the loop is entered (`σc`) and given a value
again at the body's end, where it gets there, or at a `continue`. It is held
from then on, and joins each way out of the function scheduled in the loop
from its first statement on. -/
def loopHeld (σc : MSt) (body : List Stmt) (r : MOut) (first : Nat) : DropM Unit := do
  for v in σc do
    if v.live || !(v.writable && !v.lent) || !v.owns then continue
    let liveIn (τ : MSt) := ((σc.back τ).find? (·.uid == v.uid)).any (·.live)
    if (completes body && liveIn r.1) || r.2.2.any liveIn then
      modify fun s => { s with
        held := if s.held.contains v.uid then s.held else v.uid :: s.held,
        out := s.out.map fun (k, st, a, ds) =>
          if (k == "before_return" || k == "before_try_return" || k == "before_set_target_early_return")
            && st ≥ first && !ds.contains v.uid then (k, st, a, ds ++ [v.uid]) else (k, st, a, ds) }

mutual

/-- A statement, as `mStmt` follows it, scheduling what each point it
leaves a scope by drops. `base` is how many bindings were in scope where the
innermost loop's body began, which a `break` or `continue` leaves back to. -/
def dStmt (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → MSt → Nat → Stmt → DropM MOut
  | 0, _, σ, _, _ => pure (σ, [], [])
  | n + 1, Γ, σ, base, s => do
    let id ← stmtId
    let after := ((checkStmtF env n Γ s).run #[]).1
    match s with
    | .let_ p _ e =>
      let σ ← StateT.lift (mE sigs Γ false σ e)
      pure (← bindD after σ (patNames p) true, [], [])
    | .var_ p _ (some e) =>
      let σ ← StateT.lift (mE sigs Γ false σ e)
      pure (← bindD after σ (patNames p) true (writable := true), [], [])
    | .var_ p (some _) none => pure (← bindD after σ (patNames p) false (writable := true), [], [])
    | .var_ _ none none => pure (σ, [], [])
    | .set_ x steps op e => pure (← StateT.lift (mSet sigs Γ σ x steps op e), [], [])
    | .fun_ _ => pure (σ, [], [])
    | .ret e =>
      let σ ← StateT.lift (exitM σ)
      let σ ← match e with
        | some e => StateT.lift (mE sigs Γ false σ e)
        | none => pure σ
      record "before_return" id none (← ownedIn σ) (always := true)
      pure (σ, [], [])
    | .if_ c tB eB t e =>
      let σc ← StateT.lift (mE sigs Γ false σ c)
      let Γt := condCtx env n Γ c tB false
      let rt ← dBlock sigs env n Γt (← bindD Γt σc tB.toList true) base t
      record "then_branch_exit" id none (← ownedIn (scopeOwn σc rt.1))
      let re ← match e with
        | some eb => do
          let Γe := condCtx env n Γ c eB true
          let re ← dBlock sigs env n Γe (← bindD Γe σc eB.toList true) base eb
          record "else_branch_exit" id none (← ownedIn (scopeOwn σc re.1))
          pure re
        | none => pure (σc, [], [])
      let σ' ← StateT.lift (joinIf (σc.back rt.1) (completes t) (σc.back re.1) (e.all completes))
      pure (σ', rt.2.1 ++ re.2.1, rt.2.2 ++ re.2.2)
    | .loop c body =>
      let σc ← match c with
        | some c => StateT.lift (mE sigs Γ false σ c)
        | none => pure σ
      let first := (← get).next
      let r ← dBlock sigs env n { Γ with inLoop := true } σc σc.length body
      loopHeld σc body r first
      record "loop_body_end" id none (← ownedIn (scopeOwn σc r.1))
      let σ' ← StateT.lift (loopEnd σ σc c.isSome body r)
      pure (σ', [], [])
    | .brk =>
      record "before_break" id none (← ownedIn (List.take (σ.length - base) σ))
      pure (σ, [σ], [])
    | .cont =>
      record "before_continue" id none (← ownedIn (List.take (σ.length - base) σ))
      pure (σ, [], [σ])
    | .debuglog e => pure (← StateT.lift (mE sigs Γ true σ e), [], [])
    | .call e => pure (← StateT.lift (mE sigs Γ false σ e), [], [])
    | .const_ x _ e =>
      let σ ← StateT.lift (mE sigs { Γ with constExpr := true } false σ e)
      pure (← bindD after σ [x] true (konst := true), [], [])
    | .type_ _ _ => pure (σ, [], [])
    | .match_ e cases dflt =>
      let σc ← StateT.lift (mE sigs Γ false σ e)
      let r ← dCases sigs env n Γ (matchVariants env n Γ e) σc base id 0 cases
      let d ← match dflt with
        | some db => do
          let rd ← dBlock sigs env n Γ σc base db
          record "match_arm_exit" id (some cases.length) (← ownedIn (scopeOwn σc rd.1))
          pure ([(σc.back rd.1, completes db)], rd.2.1, rd.2.2)
        | none => pure ([], [], [])
      let σ' ← StateT.lift (joinMatch σc (r.1 ++ d.1))
      pure (σ', r.2.1 ++ d.2.1, r.2.2 ++ d.2.2)
    | .for_ names coll body =>
      let σc ← StateT.lift (mE sigs Γ true σ coll)
      let σw := match placeOf coll with
        | some (x, _) => σc.walk x
        | none => σc
      let Γf := forCtx env n Γ names coll
      let σb ← (names.zip (forRefs env n Γ names coll)).foldlM
        (fun σ (x, r) => bindOne Γf false true true false r σ x) σw
      let first := (← get).next
      let r ← dBlock sigs env n Γf σb σc.length body
      loopHeld σc body r first
      record "loop_body_end" id none (← ownedIn (scopeOwn σc r.1))
      let σ' ← StateT.lift (loopEnd σc σc true body r)
      pure (σ', [], [])

/-- A `match`'s cases, as `mCases` follows them, the `i`th scheduling what
its arm drops. -/
def dCases (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → List (String × Option Ty) → MSt →
    Nat → Nat → Nat → List Case → DropM (List (MSt × Bool) × List MSt × List MSt)
  | 0, _, _, _, _, _, _, _ => pure ([], [], [])
  | _ + 1, _, _, _, _, _, _, [] => pure ([], [], [])
  | n + 1, Γ, vs, σ, base, id, i, .atom _ b :: cs => do
    let rb ← dBlock sigs env n Γ σ base b
    record "match_arm_exit" id (some i) (← ownedIn (scopeOwn σ rb.1))
    let r ← dCases sigs env n Γ vs σ base id (i + 1) cs
    pure ((σ.back rb.1, completes b) :: r.1, rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2)
  | n + 1, Γ, vs, σ, base, id, i, .term a x b :: cs => do
    let Γa := termCtx Γ vs a x
    let rb ← dBlock sigs env n Γa (← bindD Γa σ [x] true) base b
    record "match_arm_exit" id (some i) (← ownedIn (scopeOwn σ rb.1))
    let r ← dCases sigs env n Γ vs σ base id (i + 1) cs
    pure ((σ.back rb.1, completes b) :: r.1, rb.2.1 ++ r.2.1, rb.2.2 ++ r.2.2)

/-- A block, as `mBlock` follows it. -/
def dBlock (sigs : List (String × Sig)) (env : InferEnv) : Nat → Ctx → MSt → Nat → List Stmt → DropM MOut
  | 0, _, σ, _, _ => pure (σ, [], [])
  | _ + 1, _, σ, _, [] => pure (σ, [], [])
  | n + 1, Γ, σ, base, s :: ss => do
    let r1 ← dStmt sigs env n Γ σ base s
    let r2 ← dBlock sigs env n ((checkStmtF env n Γ s).run #[]).1 r1.1 base ss
    pure (r2.1, r1.2.1 ++ r2.2.1, r1.2.2 ++ r2.2.2)

end

/-- A function's drop schedule, as `mFun` follows its body: its parameters
bound as there, owned if `in` and not copy, and what the function's own
scope holds where the body ends dropped there. -/
def dFun (sigs : List (String × Sig)) (env : InferEnv) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (cmarks : List (String × Bool)) : FunDecl → DropM Unit
  | .mk name tps params _ body =>
    match sigs.lookup name with
    | none => pure ()
    | some sig => do
      let Γ := funCtx sigs aliases consts tps params sig
      let consts : MSt := cmarks.map fun (x, s) =>
        { name := x, init := s, live := true, back := false, lent := false, writable := false, byRef := false }
      let σ0 ← params.foldlM (fun σ (m, x, _) => do
          let σ ← bindOne Γ false (m != .out) (m == .ref || m == .mut || m == .out) (m == .mut || m == .out)
            (m == .ref) σ x
          pure (match σ with
            | v :: rest => { v with back := m == .out || m == .mut } :: rest
            | [] => [])) consts
      let (σ, _, _) ← dBlock sigs env fuel Γ σ0 0 body
      record "at_function_exit" 0 none (← ownedIn (scopeOwn consts σ))

/-- Each function a script declares at its top level, by name, with its
body's statements numbered as the Rust numbers them. -/
def scriptFunKinds (ss : List Stmt) : List (String × List String) :=
  ss.filterMap fun
    | .fun_ (.mk name _ _ _ body) => some (name, stmtKinds body)
    | _ => none

/-- A scheduled point: its kind, its statement, the arm for a `match`'s, and
the names of what it drops. -/
abbrev DropPoint := String × Nat × Option Nat × List String

/-- A script's top-level functions' drop schedules, by name, each as
`mTop` reaches the function: in the context the statements before it leave.
None for a function the move analysis refuses. -/
def scriptDrops (ss : List Stmt) : List (String × Option (List DropPoint)) :=
  let sigs := (resolveNames ss).1
  let rec go (Γ : Ctx) : List Stmt → List (String × Option (List DropPoint))
    | [] => []
    | .fun_ f :: rest =>
      let name := match f with | .mk n .. => n
      let r := (dFun sigs movesEnv Γ.aliases (constsOf Γ.vars) ((constsOf Γ.vars).map (·.1, true)) f).run {}
      let points := match r with
        | .ok (_, s) => some (s.out.reverse.map fun (k, st, a, ds) =>
            (k, st, a, ds.map fun u => (s.names.lookup u).getD "?"))
        | .error _ => none
      (name, points) :: go Γ rest
    | s :: rest => go ((checkStmt Γ s).run #[]).1 rest
  go { ret := .ty (.result unitTy), sigs } ss

end Datalove.Fun
