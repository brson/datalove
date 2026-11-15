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
walks a collection once. Calls are not run yet (`Stop.unsupported`), nor is
a `set` through a tensor's slice, nor a `for` with an index over more
elements than `index` counts.
-/

namespace Datalove.Fun

open Lit (Val)

/-- The variables after a block, back to the scope it began in: what it bound
dropped, what it set kept. -/
def restore (ρ ρ' : VEnv) : VEnv := ρ'.drop (ρ'.length - ρ.length)

/-- Set the innermost variable of a name. -/
def setVar (x : String) (v : Val) : VEnv → VEnv
  | [] => []
  | (y, o) :: ρ => if y == x then (y, some v) :: ρ else (y, o) :: setVar x v ρ

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
def replaceKey (k w : Val) : List (Val × Val) → List (Val × Val)
  | [] => []
  | (k', u) :: es => if keyEq k k' then (k', w) :: es else (k', u) :: replaceKey k w es

/-- A list, map or tensor of rank one or less with its element at a key
replaced, which `indexVal` read. A tensor's slice is not written. -/
def putIndex (v k w : Val) : EvalM Val :=
  match v with
  | .list vs => match numOf k with
    | some i => pure (.list (vs.set i.toNat w))
    | none => throw .wrong
  | .map es => pure (.map (replaceKey k w es))
  | .tensor shape vs => match shape with
    | [] | [_] => match numOf k with
      | some i => pure (.tensor shape (vs.set i.toNat w))
      | none => throw .wrong
    | _ => throw .unsupported
  | _ => throw .wrong

/-- A map with a value at a key, replaced or added. -/
def insertKey (k w : Val) (es : List (Val × Val)) : List (Val × Val) :=
  if es.any (fun e => keyEq k e.1) then replaceKey k w es else es ++ [(k, w)]

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
def execLoop (cond : VEnv → EvalM Bool) (body : VEnv → EvalM VEnv) : Nat → VEnv → EvalM VEnv
  | 0, _ => throw .diverge
  | k + 1, ρ => do
    if !(← cond ρ) then return ρ
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

/-- What a `set` writes, as `setValue` types it. -/
def execSetValue (env : InferEnv) (n : Nat) (Γ : Ctx) (ρ : VEnv) (t : Ty) (old : Option Val)
    (op : Option BinOp) (e : Expr) : EvalM Val :=
  match op with
  | none => evalC env n Γ ρ e t
  | some op => do
    let r ← evalC env n Γ.borrow ρ e t
    match old with
    | some o => binOp op t o r
    | none => throw .uninit

/-- A `set` target's last step, an index, as `setIndex` types it. -/
def execSetIndex (env : InferEnv) (n : Nat) (Γ : Ctx) (ρ : VEnv) (t : Ty) (k : Expr)
    (mode : Option IdxMode) (op : Option BinOp) (e : Expr) (old : Option Val) : EvalM Val :=
  match mode, op with
  | none, some _ => throw .wrong
  | none, none => match t.expose with
    | .map kt vt => do
      let kv ← evalC env n Γ ρ k kt
      let nv ← evalC env n Γ ρ e vt
      match old with
      | some (.map es) => pure (.map (insertKey kv nv es))
      | some _ => throw .wrong
      | none => throw .uninit
    | _ => throw .wrong
  | some m, _ => match indexTypes t with
    | .error _ => throw .wrong
    | .ok (et, kt) =>
      if isView t then throw .wrong
      else do
        let kv ← evalC env n Γ ρ k kt
        match old with
        | some v => do
          let part ← indexVal m v kv
          let w ← execSetValue env n Γ ρ et (some part) op e
          putIndex v kv w
        | none => throw .uninit

/-- A `set` target's steps from a value of type `t`, as `setSteps` types
them: the value with the place written. -/
def execSetSteps (env : InferEnv) (n : Nat) (Γ : Ctx) (ρ : VEnv) (op : Option BinOp) (e : Expr) :
    Ty → Option Val → List Step → EvalM Val
  | t, old, [] => execSetValue env n Γ ρ t old op e
  | t, old, [.index k mode] => execSetIndex env n Γ ρ t k mode op e old
  | t, old, .field s :: rest => match fieldType t s with
    | .ok ft => match old with
      | some v => do
        let part ← fieldVal v s
        let w ← execSetSteps env n Γ ρ op e ft (some part) rest
        putField v s w
      | none => throw .uninit
    | .error _ => throw .wrong
  | _, _, .index _ none :: _ => throw .wrong
  | t, old, .index k (some m) :: rest => match indexTypes t with
    | .error _ => throw .wrong
    | .ok (et, kt) => do
      let kv ← evalC env n Γ ρ k kt
      match old with
      | some v => do
        let part ← indexVal m v kv
        let w ← execSetSteps env n Γ ρ op e et (some part) rest
        putIndex v kv w
      | none => throw .uninit

/-- `set x … = e`, as `checkSet` types it. -/
def execSet (env : InferEnv) (n : Nat) (Γ : Ctx) (ρ : VEnv) (x : String) (steps : List Step)
    (op : Option BinOp) (e : Expr) : EvalM VEnv :=
  match Γ.lookup x, ρ.lookup x with
  | some (t, _), some old => do
    let w ← execSetSteps env n Γ ρ op e t old steps
    pure (setVar x w ρ)
  | _, _ => throw .wrong

/-- `let` or `var` with a value, as `bindChecked` types it. -/
def execBind (env : InferEnv) (n : Nat) (Γ : Ctx) (ρ : VEnv) (p : Pat) (hint : Option Hint) (e : Expr) :
    EvalM VEnv := do
  let v ← match hint with
    | some h => match Γ.hint h with
      | .ok t => evalC env n Γ ρ e t
      | .error _ => throw .wrong
    | none => evalS env n Γ ρ e
  let vals ← patValues p v
  pure (bindVals ρ (vals.map fun (x, w) => (x, some w)))

/-- `ret` or `ret e`, as `checkRet` types it. -/
def execRet (env : InferEnv) (n : Nat) (Γ : Ctx) (ρ : VEnv) (e : Option Expr) : EvalM VEnv :=
  match Γ.ret, e with
  | .ty r, some e => do throw (.early (← evalC env n Γ ρ e r))
  | .void, none => throw .retVoid
  | _, _ => throw .wrong

/-- A condition's truth. -/
def boolOf (v : Val) : EvalM Bool :=
  match v with
  | .bool b => pure b
  | _ => throw .wrong

mutual

/-- What a statement does to the variables, as `checkStmtF` checks it, with
`k` iterations for each loop. -/
def execStmt (env : InferEnv) : Nat → Nat → Ctx → VEnv → Stmt → EvalM VEnv
  | 0, _, _, _, _ => throw .fuel
  | n + 1, k, Γ, ρ, s => match s with
    | .let_ p hint e => execBind env n Γ ρ p hint e
    | .var_ p hint (some e) => execBind env n Γ ρ p hint e
    | .var_ p (some h) none => match Γ.hint h with
      | .ok t => match patBindings p t with
        | .ok Δ => pure (bindVals ρ (Δ.map fun (x, _) => (x, none)))
        | .error _ => throw .wrong
      | .error _ => throw .wrong
    | .var_ _ none none => pure ρ
    | .set_ x steps op e => execSet env n Γ ρ x steps op e
    | .fun_ _ => pure ρ
    | .ret e => execRet env n Γ ρ e
    | .if_ c none none t e => do
      if ← boolOf (← evalC env n Γ ρ c .bool) then
        return restore ρ (← execBlock env n k Γ ρ t)
      else execElse env n k Γ ρ e
    | .if_ c thenB elseB t e => do
      let ct ← typeOf (synthF env n Γ c)
      let v ← evalS env n Γ ρ c
      match ct.expose, v with
      | .option p, .some w =>
        return restore ρ (← execBlock env n k (bindOpt Γ thenB p) (bindVals ρ (thenB.toList.map (·, some w))) t)
      | .option _, .none => execElse env n k Γ ρ e
      | .result p, .ok w =>
        return restore ρ (← execBlock env n k (bindOpt Γ thenB p) (bindVals ρ (thenB.toList.map (·, some w))) t)
      | .result _, .er w =>
        execElse env n k (bindOpt Γ elseB .error) (bindVals ρ (elseB.toList.map (·, some w))) e
          >>= fun ρ' => pure (restore ρ ρ')
      | _, _ => throw .wrong
    | .loop c body =>
      let cond : VEnv → EvalM Bool := fun ρ => match c with
        | some c => do boolOf (← evalC env n Γ ρ c .bool)
        | none => pure true
      execLoop cond (fun ρ => execBlock env n k { Γ with inLoop := true } ρ body) k ρ
    | .brk => throw (.brk ρ)
    | .cont => throw (.cont ρ)
    | .debuglog e => do let _ ← evalS env n Γ.borrow ρ e; pure ρ
    | .call e => do let _ ← evalS env n Γ ρ e; pure ρ
    | .const_ x hint e => do
      let ρ' ← execBind env n { Γ with constExpr := true } ρ (.name x) hint e
      pure ρ'
    | .type_ _ _ => pure ρ
    | .match_ e cases dflt => do
      let t ← typeOf (synthF env n Γ e)
      let v ← evalS env n Γ ρ e
      match t.expose with
      | .enum vs =>
        let name := match v with
          | .atom a => some a
          | .term a _ => some a
          | _ => none
        match name with
        | some a => do
          match ← execCases env n k Γ ρ vs.toList a v cases with
          | some ρ' => return restore ρ ρ'
          | none => execElse env n k Γ ρ dflt
        | none => throw .wrong
      | _ => throw .wrong
    | .for_ names coll body => match forBindings env n Γ names coll |>.run #[] with
      | (some Δ, _) => do
        let t ← typeOf (synthF env n Γ.borrow coll)
        let v ← evalS env n Γ.borrow ρ coll
        let items ← walkItems t.expose names v
        execFor (fun ρ => execBlock env n k ({ Γ with inLoop := true }.bindAll Δ) ρ body) names items ρ
      | (none, _) => throw .wrong

/-- The case of a `match` for variant `a`, as `checkCases` checks them, each
at one less fuel than the one before; none if there is no case for it. -/
def execCases (env : InferEnv) : Nat → Nat → Ctx → VEnv → List (String × Option Ty) → String → Val →
    List Case → EvalM (Option VEnv)
  | 0, _, _, _, _, _, _, _ => throw .fuel
  | _ + 1, _, _, _, _, _, _, [] => pure none
  | n + 1, k, Γ, ρ, vs, a, v, c :: cs =>
    if c.name == a then match c, v with
      | .atom _ b, _ => some <$> execBlock env n k Γ ρ b
      | .term _ x b, .term _ w => match vs.lookup a with
        | some (some p) => some <$> execBlock env n k (Γ.bind x p .let_) ((x, some w) :: ρ) b
        | _ => throw .wrong
      | _, _ => throw .wrong
    else execCases env n k Γ ρ vs a v cs

/-- An `else` body, if there is one, run in its own scope. -/
def execElse (env : InferEnv) : Nat → Nat → Ctx → VEnv → Option (List Stmt) → EvalM VEnv
  | n, k, Γ, ρ, some e => do return restore ρ (← execBlock env n k Γ ρ e)
  | _, _, _, ρ, none => pure ρ

/-- A block's statements in order, each in the context the one before it
leaves, as `checkBlockF` checks them. -/
def execBlock (env : InferEnv) : Nat → Nat → Ctx → VEnv → List Stmt → EvalM VEnv
  | 0, _, _, _, _ => throw .fuel
  | _ + 1, _, _, ρ, [] => pure ρ
  | n + 1, k, Γ, ρ, s :: ss => do
    let ρ' ← execStmt env n k Γ ρ s
    execBlock env n k ((checkStmtF env n Γ s).run #[]).1 ρ' ss

end

end Datalove.Fun
