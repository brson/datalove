import Datalove.FunEval

/-!
# Running datafun's statements

What a block of statements does to its variables, for
`Proofs/ExecSound.lean` to show that a block that checks keeps every
variable at its type. As `FunEval.lean` evaluates expressions, this follows
the checker's functions one for one, asking the checker for the context
after each statement. Variables are a list, innermost first, so a block's
scope is a prefix: leaving it drops what it bound and keeps what it set
(`restore`).

A loop runs at most as many times as it is given (`Stop.diverge`); a `for`
walks a collection once. A statement's expressions run from the variables
as they are, and may change them, a call writing back to its `mut` and `out`
arguments; the statement goes on from what they leave. A `set` through a
tensor's slice is not run yet (`Stop.unsupported`), nor a `for` with an
index over more elements than `index` counts. A return, by `ret` or early
from an expression, is `Stop.returned`, with the variables, so that a call
can write back what its parameters came to.
-/

namespace Datalove.Fun

open Lit (Val)

/-- The variables after a block, back to the scope it began in: what it bound
dropped, what it set kept. -/
def restore (ρ ρ' : VEnv) : VEnv := ρ'.drop (ρ'.length - ρ.length)

/-- Bind names in order, each innermost of those before it, as
`Ctx.bindAllAs` does. -/
def bindVals (ρ : VEnv) (vals : List (String × Option Val)) : VEnv :=
  vals.foldl (fun ρ (x, o) => (x, o) :: ρ) ρ

/-- What a pattern takes out of a value, name by name, as `patBindings`
names them. -/
def patValues (p : Pat) (v : Val) : EvalM (List (String × Val)) :=
  match p, v with
  | .name x, v => pure [(x, v)]
  | .tuple xs, .tuple vs => pure (xs.zip vs)
  | .struct fs, .struct fvs => pure (fs.filterMap fun (f, x) => (fvs.lookup f).map (x, ·))
  | .atom _, _ => pure []
  | .term _ x, .term _ w => pure [(x, w)]
  | _, _ => throw .wrong

/-- A tuple or struct value with a field replaced. -/
def putField (v : Val) (s : Sel) (w : Val) : EvalM Val :=
  match v, s with
  | .tuple vs, .pos i => if i < vs.length then pure (.tuple (vs.set i w)) else throw .wrong
  | .struct fs, .name n => pure (.struct (replaceFirst n w fs))
  | _, _ => throw .wrong
where
  replaceFirst (n : String) (w : Val) : List (String × Val) → List (String × Val)
    | [] => []
    | (m, u) :: fs => if m == n then (m, w) :: fs else (m, u) :: replaceFirst n w fs

/-- A map's entries with the value at a key replaced, the first entry
`indexVal` would read. -/
def replaceKey (k w : Val) : List (Val × Val) → EvalM (List (Val × Val))
  | [] => pure []
  | (k', u) :: es => do
    if ← keyEq k k' then pure ((k', w) :: es) else ((k', u) :: ·) <$> replaceKey k w es

/-- A list, map or tensor of rank one or less with its element at a key
replaced, which `indexVal` read. A tensor's slice is not written. -/
def putIndex (v k w : Val) : EvalM Val :=
  match v with
  | .list vs => match numOf k with
    | some i => pure (.list (vs.set i.toNat w))
    | none => throw .wrong
  | .map es => .map <$> replaceKey k w es
  | .tensor shape vs => match shape with
    | [] | [_] => match numOf k with
      | some i => pure (.tensor shape (vs.set i.toNat w))
      | none => throw .wrong
    | _ => throw .unsupported
  | _ => throw .wrong

/-- A map with a value at a key, replaced or added. -/
def insertKey (k w : Val) (es : List (Val × Val)) : EvalM (List (Val × Val)) := do
  match ← findKey k es with
  | some _ => replaceKey k w es
  | none => pure (es ++ [(k, w)])

/-- Elements with their positions, as `index` values, or `unsupported` for
more than `index` counts, which no collection in memory holds. -/
def indexed (vs : List Val) : EvalM (List (List Val)) :=
  if vs.length ≤ 2 ^ Lit.indexBits then pure (vs.zipIdx.map fun (w, i) => [.int i, w])
  else throw .unsupported

/-- The elements a `for` walks, as `walk` types them: an element, or an
index and an element, of a list or tensor; an element of a set; a key and a
value of a map. -/
def walkItems (c : Ty) (names : List String) (v : Val) : EvalM (List (List Val)) :=
  match c, names, v with
  | .list _, [_], .list vs => pure (vs.map ([·]))
  | .list _, [_, _], .list vs => indexed vs
  | .tensor _ _, [_], .tensor shape vs => pure ((slices shape vs).map ([·]))
  | .tensor _ _, [_, _], .tensor shape vs => indexed (slices shape vs)
  | .set _, [_], .set vs => pure (vs.map ([·]))
  | .map _ _, [_, _], .map es => pure (es.map fun (k, w) => [k, w])
  | _, _, _ => throw .wrong
where
  /-- A tensor's elements, or its slices along the first axis. -/
  slices (shape : List Nat) (vs : List Val) : List Val :=
    match shape with
    | [] | [_] => vs
    | s0 :: rest => (List.range s0).map fun i =>
      .tensor rest ((vs.drop (i * shapeSize rest)).take (shapeSize rest))

/-- A loop's iterations, at most `k`, each with the condition first: a body
that ends, or continues, goes round again, one that breaks stops. -/
def execLoop (cond : VEnv → EvalM (Bool × VEnv)) (body : VEnv → EvalM VEnv) : Nat → VEnv → EvalM VEnv
  | 0, _ => throw .diverge
  | k + 1, ρ => do
    let (b, ρ) ← cond ρ
    if !b then return ρ
    match body ρ with
    | .ok ρ' => execLoop cond body k (restore ρ ρ')
    | .error (.brk ρ') => pure (restore ρ ρ')
    | .error (.cont ρ') => execLoop cond body k (restore ρ ρ')
    | .error s => throw s

/-- A `for`'s iterations, one for each item, its names bound to the item's
values. -/
def execFor (body : VEnv → EvalM VEnv) (names : List String) : List (List Val) → VEnv → EvalM VEnv
  | [], ρ => pure ρ
  | item :: rest, ρ =>
    match body (bindVals ρ ((names.zip item).map fun (x, w) => (x, some w))) with
    | .ok ρ' => execFor body names rest (restore ρ ρ')
    | .error (.brk ρ') => pure (restore ρ ρ')
    | .error (.cont ρ') => execFor body names rest (restore ρ ρ')
    | .error s => throw s

/-- What a `set` target's steps need evaluated, by the types `setSteps`
walks: the type the value is written at, and each key with its type, in
order. None where the checker refuses the target, a compound `set` that
inserts into a map among it. -/
def setPlan (op : Option BinOp) : Ty → List Step → Option (Ty × List (Expr × Ty))
  | t, [] => some (t, [])
  | t, [.index k none] => match op, t.expose with
    | none, .map kt vt => some (vt, [(k, kt)])
    | _, _ => none
  | t, .field s :: rest => match fieldType t s with
    | .ok ft => setPlan op ft rest
    | .error _ => none
  | _, .index _ none :: _ => none
  | t, .index k (some _) :: rest => match indexTypes t with
    | .ok (et, kt) => (setPlan op et rest).map fun (vt, ks) => (vt, (k, kt) :: ks)
    | .error _ => none

/-- A `set`'s keys, in order. -/
def evalKeys (env : InferEnv) (call : CallFn) (n : Nat) (Γ : Ctx) : List (Expr × Ty) → EvalT (List Val)
  | [] => pure []
  | (k, kt) :: ks => do
    let v ← evalC env call n Γ k kt
    let vs ← evalKeys env call n Γ ks
    pure (v :: vs)

/-- What a variable that held `old`, of type `t`, holds once a `set` writes
`r` through its steps, with its keys' values `kvs`: a compound one's
operator applied to what is there. -/
def writeSteps (op : Option BinOp) : Ty → Option Val → List Step → List Val → Val → EvalM Val
  | t, old, [], _, r => match op with
    | none => pure r
    | some op => match old with
      | some o => binOp op t o r
      | none => throw .uninit
  | _, old, [.index _ none], [kv], r => match old with
    | some (.map es) => .map <$> insertKey kv r es
    | some _ => throw .wrong
    | none => throw .uninit
  | t, old, .field s :: rest, kvs, r => match fieldType t s, old with
    | .ok ft, some v => do
      let part ← fieldVal v s
      let w ← writeSteps op ft (some part) rest kvs r
      putField v s w
    | .ok _, none => throw .uninit
    | .error _, _ => throw .wrong
  | t, old, .index _ (some m) :: rest, kv :: kvs, r => match indexTypes t, old with
    | .ok (et, _), some v => do
      let part ← indexVal m v kv
      let w ← writeSteps op et (some part) rest kvs r
      putIndex v kv w
    | .ok _, none => throw .uninit
    | .error _, _ => throw .wrong
  | _, _, _, _, _ => throw .wrong

/-- `set x … = e`, as `checkSet` types it: the value, then the keys, left to
right, and only then the place, as the variable is once they have run, which
may have changed it. -/
def execSet (env : InferEnv) (call : CallFn) (n : Nat) (Γ : Ctx) (ρ : VEnv) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) : EvalM VEnv :=
  match Γ.lookup x with
  | some (t, _) => match setPlan op t steps with
    | some (vt, keys) => do
      let (w, ρ') ← (do
        let r ← evalC env call n (match op with | none => Γ | some _ => Γ.borrow) e vt
        let kvs ← evalKeys env call n Γ keys
        match (← get).lookup x with
        | some old => here (writeSteps op t old steps kvs r)
        | none => throw Stop.wrong).run ρ
      pure (setVar x w ρ')
    | none => throw .wrong
  | none => throw .wrong

/-- `let` or `var` with a value, as `bindChecked` types it. -/
def execBind (env : InferEnv) (call : CallFn) (n : Nat) (Γ : Ctx) (ρ : VEnv) (p : Pat) (hint : Option Hint) (e : Expr) :
    EvalM VEnv := do
  let (v, ρ) ← match hint with
    | some h => match Γ.hint h with
      | .ok t => (evalC env call n Γ e t).run ρ
      | .error _ => throw .wrong
    | none => (evalS env call n Γ e).run ρ
  let vals ← patValues p v
  pure (bindVals ρ (vals.map fun (x, w) => (x, some w)))

/-- `ret` or `ret e`, as `checkRet` types it: the body's return, with the
variables as its value left them. -/
def execRet (env : InferEnv) (call : CallFn) (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Option Expr) : EvalM VEnv :=
  match Γ.ret, e with
  | .ty r, some e => do
    let (v, ρ') ← (evalC env call n Γ e r).run ρ
    throw (.returned (some v) ρ')
  | .void, none => throw (.returned none ρ)
  | _, _ => throw .wrong

/-- A condition's truth. -/
def boolOf (v : Val) : EvalM Bool :=
  match v with
  | .bool b => pure b
  | _ => throw .wrong

mutual

/-- What a statement does to the variables, as `checkStmtF` checks it, with
`k` iterations for each loop. -/
def execStmt (env : InferEnv) (call : CallFn) : Nat → Nat → Ctx → VEnv → Stmt → EvalM VEnv
  | 0, _, _, _, _ => throw .fuel
  | n + 1, k, Γ, ρ, s => match s with
    | .let_ p hint e => execBind env call n Γ ρ p hint e
    | .var_ p hint (some e) => execBind env call n Γ ρ p hint e
    | .var_ p (some h) none => match Γ.hint h with
      | .ok t => match patBindings p t with
        | .ok Δ => pure (bindVals ρ (Δ.map fun (x, _) => (x, none)))
        | .error _ => throw .wrong
      | .error _ => throw .wrong
    | .var_ _ none none => pure ρ
    | .set_ x steps op e => execSet env call n Γ ρ x steps op e
    | .fun_ _ => pure ρ
    | .ret e => execRet env call n Γ ρ e
    | .if_ c none none t e => do
      let (cv, ρ) ← (evalC env call n Γ c .bool).run ρ
      if ← boolOf cv then
        return restore ρ (← execBlock env call n k Γ ρ t)
      else execElse env call n k Γ ρ e
    | .if_ c thenB elseB t e => do
      let ct ← typeOf (synthF env n Γ c)
      let (v, ρ) ← (evalS env call n Γ c).run ρ
      match ct.expose, v with
      | .option p, .some w =>
        return restore ρ (← execBlock env call n k (bindOpt Γ thenB p) (bindVals ρ (thenB.toList.map (·, some w))) t)
      | .option _, .none => execElse env call n k Γ ρ e
      | .result p, .ok w =>
        return restore ρ (← execBlock env call n k (bindOpt Γ thenB p) (bindVals ρ (thenB.toList.map (·, some w))) t)
      | .result _, .er w =>
        execElse env call n k (bindOpt Γ elseB .error) (bindVals ρ (elseB.toList.map (·, some w))) e
          >>= fun ρ' => pure (restore ρ ρ')
      | _, _ => throw .wrong
    | .loop c body =>
      let cond : VEnv → EvalM (Bool × VEnv) := fun ρ => match c with
        | some c => do
          let (cv, ρ) ← (evalC env call n Γ c .bool).run ρ
          pure (← boolOf cv, ρ)
        | none => pure (true, ρ)
      execLoop cond (fun ρ => execBlock env call n k { Γ with inLoop := true } ρ body) k ρ
    | .brk => throw (.brk ρ)
    | .cont => throw (.cont ρ)
    | .debuglog e => do let (_, ρ) ← (evalS env call n Γ.borrow e).run ρ; pure ρ
    | .call e => do let (_, ρ) ← (evalS env call n Γ e).run ρ; pure ρ
    | .const_ x hint e => do
      let ρ' ← execBind env call n { Γ with constExpr := true } ρ (.name x) hint e
      pure ρ'
    | .type_ _ _ => pure ρ
    | .match_ e cases dflt => do
      let t ← typeOf (synthF env n Γ e)
      let (v, ρ) ← (evalS env call n Γ e).run ρ
      match t.expose with
      | .enum vs =>
        let name := match v with
          | .atom a => some a
          | .term a _ => some a
          | _ => none
        match name with
        | some a => do
          match ← execCases env call n k Γ ρ vs.toList a v cases with
          | some ρ' => return restore ρ ρ'
          | none => execElse env call n k Γ ρ dflt
        | none => throw .wrong
      | _ => throw .wrong
    | .for_ names coll body => match forBindings env n Γ names coll |>.run #[] with
      | (some Δ, _) => do
        let t ← typeOf (synthF env n Γ.borrow coll)
        let (v, ρ) ← (evalS env call n Γ.borrow coll).run ρ
        let items ← walkItems t.expose names v
        execFor (fun ρ => execBlock env call n k ({ Γ with inLoop := true }.bindAll Δ) ρ body) names items ρ
      | (none, _) => throw .wrong

/-- The case of a `match` for variant `a`, as `checkCases` checks them, each
at one less fuel than the one before; none if there is no case for it. -/
def execCases (env : InferEnv) (call : CallFn) : Nat → Nat → Ctx → VEnv → List (String × Option Ty) → String → Val →
    List Case → EvalM (Option VEnv)
  | 0, _, _, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, _, _, _, _, [] => pure none
  | n + 1, k, Γ, ρ, vs, a, v, c :: cs =>
    if c.name == a then match c, v with
      | .atom _ b, _ => some <$> execBlock env call n k Γ ρ b
      | .term _ x b, .term _ w => match vs.lookup a with
        | some (some p) => some <$> execBlock env call n k (Γ.bind x p .let_) ((x, some w) :: ρ) b
        | _ => throw .wrong
      | _, _ => throw .wrong
    else execCases env call n k Γ ρ vs a v cs

/-- An `else` body, if there is one, run in its own scope. -/
def execElse (env : InferEnv) (call : CallFn) : Nat → Nat → Ctx → VEnv → Option (List Stmt) → EvalM VEnv
  | n, k, Γ, ρ, some e => do return restore ρ (← execBlock env call n k Γ ρ e)
  | _, _, _, ρ, none => pure ρ

/-- A block's statements in order, each in the context the one before it
leaves, as `checkBlockF` checks them. -/
def execBlock (env : InferEnv) (call : CallFn) : Nat → Nat → Ctx → VEnv → List Stmt → EvalM VEnv
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, ρ, [] => pure ρ
  | n + 1, k, Γ, ρ, s :: ss => do
    let ρ' ← execStmt env call n k Γ ρ s
    execBlock env call n k ((checkStmtF env n Γ s).run #[]).1 ρ' ss

end

end Datalove.Fun
