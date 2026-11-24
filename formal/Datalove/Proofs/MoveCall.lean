import Datalove.FunConstMoves
import Datalove.Proofs.EnvCongr
import Datalove.Proofs.MoveStmt

/-!
# A script the move analysis lets through never reads what was moved

`Fun.runScriptMoves_move`: where a script checks and the move analysis
(`moveScript`) lets it through, with no call writing back into a variable a
later argument moved (`moveScriptHeld`, what O010 and O017 refuse), running
it with the run that moves (`runScriptMoves`) never stops at `Stop.uninit`
past its consts: no variable is read while it holds nothing, moved or never
given a value, and no call returns with a `mut` or `out` parameter it left
holding nothing.

The pieces: a call into a program whose bodies the analysis lets through
keeps the promise `CallMove` makes (`runCall_move`), the body run in the
instance of the context the analysis followed it in
(`Proofs/MoveStmt.lean`); the top level's statements run in their own
context (`runTopMoves_move`); and the table a script's functions make is
such a program (`funTable_move`), each function's body let through where
`mTop` reached its declaration and checked quietly where `checkScript`
did.

The consts are evaluated apart (`constPassMoves`), on the consts' values
alone, and are not covered: a const's initializer names only consts (F058),
which the analysis of what each const reads would have to carry, as
`Proofs/ConstSound.lean` does for what holds a value.
-/

namespace Datalove.Fun

open Lit (Val)

/-! ## No type arguments -/

mutual

theorem Ty.inst_nil : ∀ (t : Ty), t.inst [] = t
  | .list t | .set t | .option t | .result t | .mu t => by simp [Ty.inst, Ty.inst_nil t]
  | .tensor t _ | .term _ t => by simp [Ty.inst, Ty.inst_nil t]
  | .map k v => by simp [Ty.inst, Ty.inst_nil k, Ty.inst_nil v]
  | .tuple ts => by simp [Ty.inst, Tys.inst_nil ts]
  | .struct fs => by simp [Ty.inst, Fields.inst_nil fs]
  | .table fs => by simp [Ty.inst, Fields.inst_nil fs]
  | .enum vs => by simp [Ty.inst, Variants.inst_nil vs]
  | .var _ => by simp [Ty.inst, List.lookup]
  | .bool | .u8 | .i8 | .u16 | .i16 | .u32 | .i32 | .u64 | .i64 | .index | .offset | .f32 | .f64 | .int
  | .string | .data | .error | .atom _ | .recvar _ => by simp [Ty.inst]

theorem Tys.inst_nil : ∀ (ts : Tys), ts.inst [] = ts
  | .nil => by simp [Tys.inst]
  | .cons t ts => by simp [Tys.inst, Ty.inst_nil t, Tys.inst_nil ts]

theorem Fields.inst_nil : ∀ (fs : Fields), fs.inst [] = fs
  | .nil => by simp [Fields.inst]
  | .cons _ t fs => by simp [Fields.inst, Ty.inst_nil t, Fields.inst_nil fs]

theorem Variants.inst_nil : ∀ (vs : Variants), vs.inst [] = vs
  | .nil => by simp [Variants.inst]
  | .atom _ vs => by simp [Variants.inst, Variants.inst_nil vs]
  | .term _ t vs => by simp [Variants.inst, Ty.inst_nil t, Variants.inst_nil vs]

end

theorem Aliases.inst_nil : ∀ (A : Aliases), A.inst [] = A
  | [] => rfl
  | (n, t) :: A => by
    have ih := Aliases.inst_nil A
    simp only [Aliases.inst] at ih ⊢
    simp [Ty.inst_nil t, ih]

theorem Ret.inst_nil (r : Ret) : r.inst [] = r := by cases r <;> simp [Ret.inst, Ty.inst_nil]

theorem vars_inst_nil : ∀ (vs : List (String × Ty × Kind)), (vs.map fun (x, t, k) => (x, t.inst [], k)) = vs
  | [] => rfl
  | (x, t, k) :: vs => by simp [Ty.inst_nil t, vars_inst_nil vs]

/-- A context with no bounds is its own instance at no type arguments: the
top level's. -/
theorem Ctx.inst_nil {Γ : Ctx} (hb : Γ.bounds = []) : Γ.inst [] = Γ := by
  obtain ⟨vars, ret, inLoop, sigs, borrowed, lends, mutArg, aliases, bounds, constExpr, inModule⟩ := Γ
  simp only at hb
  subst hb
  simp only [Ctx.inst, vars_inst_nil, Ret.inst_nil, Aliases.inst_nil]

theorem Ground.nil : Ground [] := fun _ hp => by simp at hp

theorem Respects.nil {Γ : Ctx} (hb : Γ.bounds = []) : Respects [] Γ := fun _ _ hl => by
  rw [hb] at hl; cases hl

/-! ## Inference that moves -/

/-- Inference that answers under an instance what it answered under the
generic does so whatever the environment's other switches. -/
theorem InstEnv.same {θ : List (String × Ty)} {env env' : InferEnv} (H : InstEnv θ env env)
    (h : InferEnv.Same env env') : InstEnv θ env' env' where
  ty := fun n Γ e t r sg hl q => by
    rw [← synthF_same h] at q
    rw [← h.ty]
    exact H.ty n Γ e t r sg hl q
  pair := fun n Γ a b exp t r sg q => by
    rw [← pairF_same h] at q
    rw [← h.pair]
    exact H.pair n Γ a b exp t r sg q
  call := fun n Γ f args exp t r sg q => by
    rw [← callF_same h] at q
    rw [← h.call]
    exact H.call n Γ f args exp t r sg q

theorem movesEnv_inst {θ : List (String × Ty)} (g : Ground θ) : InstEnv θ movesEnv movesEnv :=
  InstEnv.same (inferEnv_inst g) (InferEnv.Same.moves inferEnv true)

theorem movesEnv_same : InferEnv.Same inferEnv movesEnv := InferEnv.Same.moves inferEnv true

/-! ## Contexts -/

/-- Type arguments a function's bounds admit respect the context its body is
checked in. -/
theorem respects_funCtx {θ : List (String × Ty)} (sigs : List (String × Sig)) (aliases : Aliases)
    (consts : List (String × Ty × Kind)) (tps : List (String × Option Bound)) (params : List (Mode × String × Hint))
    (sig : Sig) (hb : ∀ p b, (p, some b) ∈ tps → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) :
    Respects θ (funCtx sigs aliases consts tps params sig) := by
  intro n b hl
  have hm := lookup_mem hl
  unfold funCtx at hm
  simp only [boundsOf, List.mem_filterMap, Option.map_eq_some_iff] at hm
  obtain ⟨⟨p, b'⟩, hp, b'', hb', he⟩ := hm
  simp only [Prod.mk.injEq] at he
  obtain ⟨rfl, rfl⟩ := he
  subst hb'
  exact hb p b'' hp

theorem bindAllAs_bounds' : ∀ (Γ : Ctx) (Δ : List (String × Ty)) (k : Kind), (Γ.bindAllAs Δ k).bounds = Γ.bounds
  | _, [], _ => rfl
  | Γ, (x, t) :: Δ, k => by
    show ((Γ.bind x t k).bindAllAs Δ k).bounds = Γ.bounds
    rw [bindAllAs_bounds']; rfl

theorem bindPat_bounds (Γ : Ctx) (p : Pat) (t : Ty) (k : Kind) (d : Array Lit.Code) :
    ((bindPat Γ p t k).run d).1.bounds = Γ.bounds := by
  unfold bindPat
  split
  · simp only [StateM.run_pure']; exact bindAllAs_bounds' Γ _ k
  · simp only [StateM.run_bind', StateM.run_modify', StateM.run_pure']

theorem bindChecked_bounds (env : InferEnv) (n : Nat) (Γ : Ctx) (p : Pat) (hint : Option Hint) (e : Expr)
    (k : Kind) (d : Array Lit.Code) : ((bindChecked env n Γ p hint e k).run d).1.bounds = Γ.bounds := by
  unfold bindChecked
  cases hint with
  | some h =>
    simp only [StateM.run_bind']
    split
    · simp only [StateM.run_pure']
    · split
      · simp only [StateM.run_bind', StateM.run_modify']; rw [bindPat_bounds]
      · simp only [StateM.run_bind']; rw [bindPat_bounds]
  | none =>
    simp only [StateM.run_bind']
    split
    · rw [bindPat_bounds]
    · split
      · split
        · rw [bindPat_bounds]
        · simp only [StateM.run_pure']
      · simp only [StateM.run_pure']

/-- Checking a statement keeps the context's bounds, as it keeps its
signatures. -/
theorem bounds_checkStmtF (env : InferEnv) : ∀ (n : Nat) (Γ : Ctx) (s : Stmt) (d : Array Lit.Code),
    ((checkStmtF env n Γ s).run d).1.bounds = Γ.bounds
  | 0, Γ, s, d => by simp only [checkStmtF, StateM.run_bind', StateM.run_modify', StateM.run_pure']
  | n + 1, Γ, s, d => by
    cases s
    case let_ p hint e => simp only [checkStmtF]; exact bindChecked_bounds env n Γ p hint e .let_ d
    case var_ p hint v =>
      cases v with
      | some e => simp only [checkStmtF]; exact bindChecked_bounds env n Γ p hint e .var_ d
      | none =>
        cases hint with
        | none => simp only [checkStmtF, StateM.run_pure']
        | some h =>
          simp only [checkStmtF, StateM.run_bind']
          split
          · simp only [StateM.run_bind']; rw [bindPat_bounds]
          · simp only [StateM.run_pure']
    case const_ x hint e =>
      simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
      show ((bindChecked env n { Γ with constExpr := true } (.name x) hint e .const_).run d).1.bounds = Γ.bounds
      rw [bindChecked_bounds]
    case type_ x h => simp only [checkStmtF, StateM.run_pure']; split <;> rfl
    case if_ c tB eB t e =>
      cases tB <;> cases eB <;> simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
    case loop c body =>
      cases c <;> simp only [checkStmtF, StateM.run_bind', StateM.run_pure']
    all_goals (simp only [checkStmtF]; simp only [StateM.run_bind', StateM.run_pure'])

/-! ## What a body starts with -/

/-- The consts a function sees, each marked as holding a value, line up with
their values, each one. -/
theorem MAg.consts : ∀ (consts : List (String × Bool)) (cvals : VEnv), cvals.map (·.1) = consts.map (·.1) →
    (∀ q ∈ cvals, q.2.isSome = true) →
    MAg (consts.map fun (x, s) => ({ name := x, init := s, live := true, back := false, lent := false,
                                     writable := false, byRef := false } : MVar)) cvals
  | [], [], _, _ => .nil
  | (x, s) :: consts, (y, o) :: cvals, hn, hs => by
    simp only [List.map_cons, List.cons.injEq] at hn
    obtain ⟨rfl, hn⟩ := hn
    exact .cons rfl (fun _ => hs (y, o) List.mem_cons_self)
      (MAg.consts consts cvals hn (fun q hq => hs q (List.mem_cons_of_mem _ hq)))
  | [], _ :: _, hn, _ => by simp at hn
  | _ :: _, [], hn, _ => by simp at hn

theorem BackLent.consts (consts : List (String × Bool)) :
    BackLent (consts.map fun (x, s) => ({ name := x, init := s, live := true, back := false, lent := false,
                                          writable := false, byRef := false } : MVar)) := by
  intro v hv hb
  simp only [List.mem_map] at hv
  obtain ⟨⟨x, s⟩, -, rfl⟩ := hv
  cases hb

/-- The parameters bound as `mFun` binds them, the last innermost, line up
with the arguments as `argVals` binds them: each holds a value but an `out`
one, and each read back is borrowed. -/
theorem paramsFold_mag (Γ : Ctx) : ∀ (params : List (Mode × String × Hint)) (vals : List (Option Val))
    (σ σ0 : MSt) (ρ : VEnv) (st st' : DSt),
    (params.foldlM (fun (σ : MSt) ((m, x, _) : Mode × String × Hint) => do
        let σ ← bindOne Γ (m != Mode.out) (m == Mode.ref || m == Mode.mut || m == Mode.out)
          (m == Mode.mut || m == Mode.out) (m == Mode.ref) σ x
        pure (match σ with
          | v :: rest => { v with back := m == Mode.out || m == Mode.mut } :: rest
          | [] => [])) σ).run st = .ok (σ0, st') →
    vals.length = params.length → (∀ q ∈ vals.zip (params.map (·.1)), q.2 ≠ .out → q.1.isSome = true) →
    MAg σ ρ → BackLent σ →
    MAg σ0 ((params.zip vals).reverse.map (fun ((_, x, _), v) => (x, v)) ++ ρ) ∧ BackLent σ0 ∧
      σ0.shape = (params.reverse.map fun (m, x, _) => (x, m == .out || m == .mut)) ++ σ.shape ∧
      st'.movedHeld = st.movedHeld
  | [], [], σ, σ0, ρ, st, st', h, _, _, hρ, hbl => by
    simp only [List.foldlM_nil] at h
    obtain ⟨rfl, rfl⟩ := MoveM.pure_run h
    exact ⟨by simpa using hρ, hbl, by simp, rfl⟩
  | (m, x, hh) :: params, v :: vals, σ, σ0, ρ, st, st', h, hl, hv, hρ, hbl => by
    simp only [List.foldlM_cons] at h
    replace h := MoveM.bind_ok h
    obtain ⟨σ1, st1, h1, h⟩ := h
    try dsimp only at h1
    replace h1 := MoveM.bind_ok h1
    obtain ⟨σ', st2, hb, h1⟩ := h1
    obtain ⟨w, rfl, hname, hinit, hlive, -, hlent, -, -, -, hfl⟩ := bindOne_run hb
    obtain ⟨rfl, rfl⟩ := MoveM.pure_run h1
    dsimp only at h ⊢
    have hv1 : m ≠ .out → v.isSome = true := fun hm => hv (v, m) (by simp) hm
    have hρ1 : MAg ({ w with back := m == .out || m == .mut } :: σ) ((x, v) :: ρ) := by
      refine .cons hname (fun hil => hv1 ?_) hρ
      simp only [hinit, hlive, Bool.and_true, bne_iff_ne, ne_eq] at hil
      exact hil
    have hbl1 : BackLent ({ w with back := m == .out || m == .mut } :: σ) := by
      intro u hu hback
      rcases List.mem_cons.mp hu with rfl | hu
      · simp only at hback ⊢
        rw [hlent]
        cases m <;> simp_all
      · exact hbl u hu hback
    obtain ⟨a1, a2, a3, a4⟩ := paramsFold_mag Γ params vals _ σ0 ((x, v) :: ρ) st2 st' h (by simpa using hl)
      (fun q hq hm => hv q (by simp [hq]) hm) hρ1 hbl1
    refine ⟨?_, a2, ?_, a4.trans hfl⟩
    · simpa [List.zip_cons_cons, List.reverse_cons, List.map_append, List.append_assoc] using a1
    · rw [a3]
      simp [MSt.shape, hname, List.reverse_cons, List.map_append, List.append_assoc]
  | [], _ :: _, _, _, _, _, _, _, hl, _, _, _ => by simp at hl
  | _ :: _, [], _, _, _, _, _, _, hl, _, _, _ => by simp at hl

/-! ## What a body ends with -/

theorem outsSet_dropM : ∀ (σ : MSt) (n : Nat), σ.outsSet = true → MSt.outsSet (List.drop n σ) = true
  | _, 0, h => h
  | [], _ + 1, h => h
  | _ :: σ, n + 1, h => by
    simp only [MSt.outsSet, List.all_cons, Bool.and_eq_true] at h
    exact outsSet_dropM σ n h.2

theorem outsSet_takeM : ∀ (σ : MSt) (n : Nat), σ.outsSet = true → MSt.outsSet (List.take n σ) = true
  | _, 0, _ => rfl
  | [], _ + 1, h => h
  | v :: σ, n + 1, h => by
    simp only [MSt.outsSet, List.all_cons, Bool.and_eq_true] at h
    simp only [List.take_succ_cons, MSt.outsSet, List.all_cons, Bool.and_eq_true]
    exact ⟨h.1, outsSet_takeM σ n h.2⟩

theorem outsSet_reverseM (σ : MSt) (h : σ.outsSet = true) : MSt.outsSet σ.reverse = true := by
  simpa [MSt.outsSet, List.all_reverse] using h

/-- Where every `mut` and `out` parameter holds a value, so does what each
came to, as a call reads them back. -/
theorem outs_afterM : ∀ (A : MSt) (B : VEnv) (ps : List (Mode × Ty)), MAg A B → A.outsSet = true →
    A.map (·.back) = ps.map (fun p => p.1 == .out || p.1 == .mut) →
    ∀ q ∈ (B.map (·.2)).zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true
  | [], [], [], _, _, _, _, hq, _ => by simp at hq
  | v :: A, (_, o) :: B, (m, t) :: ps, .cons _ hs a, ho, hw, q, hq, hm => by
    simp only [MSt.outsSet, List.all_cons, Bool.and_eq_true] at ho
    simp only [List.map_cons, List.cons.injEq] at hw
    simp only [List.map_cons, List.zip_cons_cons, List.mem_cons] at hq
    rcases hq with rfl | hq
    · apply hs
      have hb : v.back = true := by
        rw [hw.1]; rcases hm with hm | hm <;> simp only at hm <;> simp [hm]
      simpa [hb] using ho.1
    · exact outs_afterM A B ps a ho.2 hw.2 q hq hm
  | [], [], _ :: _, _, _, hw, _, _, _ => by simp at hw
  | _ :: _, _, [], _, _, hw, _, _, _ => by simp at hw
  | _ :: _, [], _ :: _, a, _, _, _, _, _ => by cases a
  | [], _ :: _, _, a, _, _, _, _, _ => by cases a

/-- Back in the scope a body began in, the shape is the one it began with. -/
theorem MSt.back_shape {σ τ : MSt} {δ : List (String × Bool)} (h : τ.shape = δ ++ σ.shape) :
    (σ.back τ).shape = σ.shape := by
  have hl : τ.length = δ.length + σ.length := by
    have := congrArg List.length h
    simpa [MSt.shape_length, List.length_append] using this
  have e : (σ.back τ).shape = τ.shape.drop (τ.length - σ.length) := by
    unfold MSt.back MSt.shape
    exact List.map_drop
  rw [e, h, hl, Nat.add_sub_cancel, List.drop_left]

/-- Where a body leaves with every `mut` and `out` parameter holding a
value, what each parameter came to, as the call reads it back, holds one. -/
theorem after_okM (params : List (Mode × String × Hint)) (C : List (String × Bool)) (ps : List (Mode × Ty))
    (hm : params.map (·.1) = ps.map (·.1)) {σ0 τ : MSt} {ρ0 ρ' : VEnv}
    (hsh : σ0.shape = (params.reverse.map fun (m, x, _) => (x, m == .out || m == .mut)) ++ C)
    (hfr : MFr σ0.shape τ) (h : MAg τ ρ') (ho : τ.outsSet = true) (hl0 : σ0.length = ρ0.length) :
    (paramsAfter params (restore ρ0 ρ')).length = ps.length ∧
      ∀ q ∈ (paramsAfter params (restore ρ0 ρ')).zip ps, (q.2.1 = .mut ∨ q.2.1 = .out) → q.1.isSome = true := by
  have hpl : params.length = ps.length := by simpa using congrArg List.length hm
  obtain ⟨⟨δ, -, hs⟩, -⟩ := hfr
  have hb := MAg.back (σ := σ0) h hl0
  have hbs : (σ0.back τ).shape = σ0.shape := MSt.back_shape hs
  have hbo : (σ0.back τ).outsSet = true := outsSet_dropM τ (τ.length - σ0.length) ho
  have hR : (restore ρ0 ρ').length = ρ0.length := by
    rw [← MAg.length hb, ← MSt.shape_length, hbs, MSt.shape_length, hl0]
  have hp0 : params.length ≤ ρ0.length := by
    rw [← hl0, ← MSt.shape_length, hsh]
    simp only [List.length_append, List.length_map, List.length_reverse]
    omega
  refine ⟨by simp only [paramsAfter, List.length_reverse, List.length_map, List.length_take, hR]; omega, ?_⟩
  have hA := MAg.reverse (MAg.take_n params.length hb)
  have hAo := outsSet_reverseM _ (outsSet_takeM _ params.length hbo)
  have hw : ((List.take params.length (σ0.back τ)).reverse.map (·.back)) =
      ps.map (fun p => p.1 == .out || p.1 == .mut) := by
    have e1 : (σ0.back τ).map (·.back) = σ0.map (·.back) := by
      have := congrArg (List.map (·.2)) hbs
      simpa [MSt.shape, List.map_map, Function.comp_def] using this
    have e2 : σ0.map (·.back) = (params.reverse.map fun (m, _, _) => (m == .out || m == .mut)) ++ C.map (·.2) := by
      have := congrArg (List.map (·.2)) hsh
      simpa [MSt.shape, List.map_map, Function.comp_def] using this
    rw [List.map_reverse, List.map_take, e1, e2, List.take_left' (by simp), List.map_reverse, List.reverse_reverse]
    have := congrArg (List.map fun m => m == Mode.out || m == Mode.mut) hm
    simpa [List.map_map, Function.comp_def] using this
  intro q hq hmq
  exact outs_afterM _ _ ps hA hAo hw q (by simpa [paramsAfter, List.map_reverse, List.map_take] using hq) hmq

/-- A body's end that is a way out has every `mut` and `out` parameter
holding a value, and files nothing. -/
theorem exitM_discard {σ : MSt} {st st' : DSt} {u : Unit}
    (h : (discard (exitM σ) : MoveM Unit).run st = .ok (u, st')) : σ.outsSet = true ∧ st' = st := by
  unfold exitM at h
  simp only [discard, Functor.mapConst, Function.comp, Function.const, StateT.map, StateT.run] at h
  split at h
  · rename_i ho
    simp only [pure, StateT.pure, Except.pure, bind, Except.bind, Except.ok.injEq, Prod.mk.injEq] at h
    exact ⟨ho, h.2.symm⟩
  · simp [throw, throwThe, MonadExceptOf.throw, StateT.lift, bind, Except.bind] at h

theorem lift_run {α} {x : Except Lit.Code α} {st st' : DSt} {a : α}
    (h : (StateT.lift x : MoveM α).run st = .ok (a, st')) : x = .ok a ∧ st' = st := by
  simp only [StateT.lift, StateT.run] at h
  cases x with
  | ok b =>
    simp only [bind, Except.bind, pure, Except.pure, Except.ok.injEq, Prod.mk.injEq] at h
    obtain ⟨rfl, rfl⟩ := h
    exact ⟨rfl, rfl⟩
  | error c => simp [bind, Except.bind] at h

/-! ## Calls -/

/-- The consts a body sees as the analysis starts from them, marked as
holding a value or not. -/
def constsStart (cmarks : List (String × Bool)) : MSt :=
  cmarks.map fun (x, s) =>
    { name := x, init := s, live := true, back := false, lent := false, writable := false, byRef := false }

theorem mFun_eq_mFunIn (S : List (String × Sig)) (env : InferEnv) (aliases : Aliases)
    (consts : List (String × Ty × Kind)) (cmarks : List (String × Bool)) (d : FunDecl) :
    mFun S env aliases consts cmarks d = mFunIn S S env aliases consts cmarks d := by
  cases d; rfl

/-- A program each of whose bodies the move analysis lets through: the
functions `A` has a signature for have their modes and type parameters in
the table, declare no type alias, check quietly in the context the analysis
follows them in, built from the script's signatures `S`, and the analysis,
its calls looked up in `A`, lets each through from consts marked as holding
a value where the table's value has one, with no call writing back into what
a later argument moved. -/
def ProgMove (A S : List (String × Sig)) (funs : List (String × FunInfo)) : Prop :=
  ∀ f sig fi, A.lookup f = some sig → funs.lookup f = some fi →
    fi.params.map (·.1) = sig.params.map (·.1) ∧ sig.tparams = fi.tps ∧ blockTypeFree fi.body = true ∧
    SQuiet (checkBlockF inferEnv fuel (funCtx S fi.aliases fi.consts fi.tps fi.params sig) fi.body) () ∧
    ∃ cmarks ret s, MAg (constsStart cmarks) fi.cvals ∧
      (mFunIn A S movesEnv fi.aliases fi.consts cmarks (.mk f fi.tps fi.params ret fi.body)).run {} = .ok ((), s) ∧
      s.movedHeld = false

section
variable (A S : List (String × Sig)) (funs : List (String × FunInfo)) (k : Nat)

/-- **Calls into a program the move analysis lets through keep the
promise**: given a value for every parameter but an `out` one, at type
arguments the bounds admit, a call never stops at a variable that holds
nothing, and returns with every `mut` and `out` parameter holding a value. -/
theorem runCall_move (hS : SigsIn A S) (hp : ProgMove A S funs) (hsg : SigsOk S) :
    ∀ d, CallMove A (runCall movesEnv S funs k d)
  | 0 => fun f sig θ vals _ _ _ _ => by simp only [runCall]; simp [throw, throwThe, MonadExceptOf.throw]
  | d + 1 => by
    intro f sig θ vals hs hg hadm hv
    simp only [runCall, hS f sig hs]
    cases hfi : funs.lookup f with
    | none => simp [throw, throwThe, MonadExceptOf.throw]
    | some fi =>
      obtain ⟨hm, htp, tf, hq, cmarks, ret, s, hcm, hmf, hsf⟩ := hp f sig fi hs hfi
      have hpl : fi.params.length = sig.params.length := by simpa using congrArg List.length hm
      simp only
      by_cases hn : vals.length ≠ fi.params.length
      · simp [hn, throw, throwThe, MonadExceptOf.throw]
      have hlen' : vals.length = fi.params.length := by simpa using hn
      simp only [hlen', bne_self_eq_false, Bool.false_eq_true, ↓reduceIte]
      -- The analysis of the body: the parameters, the body, the end.
      simp only [mFunIn, hS f sig hs] at hmf
      replace hmf := MoveM.bind_ok hmf
      obtain ⟨σ0, st0, hfold, hmf⟩ := hmf
      replace hmf := MoveM.bind_ok hmf
      obtain ⟨rr, st1, hbody, hmf⟩ := hmf
      obtain ⟨σ1, bs, cs⟩ := rr
      dsimp only at hmf
      replace hmf := MoveM.bind_ok hmf
      obtain ⟨_, st2, hif, hmf⟩ := hmf
      replace hmf := MoveM.bind_ok hmf
      obtain ⟨_, st3, hown, hrec⟩ := hmf
      have hf3 : st3.movedHeld = false := flag_before (FlagMono.record _ _ _ _ _ st3 _ s hrec) hsf
      have hf2 : st2.movedHeld = false := flag_before (FlagMono.ownedIn _ st2 _ st3 hown) hf3
      have hend : (completes fi.body = true → σ1.outsSet = true) ∧ st2 = st1 := by
        rcases MoveM.ite_run hif with ⟨hc, hif⟩ | ⟨hc, hif⟩
        · obtain ⟨ho, rfl⟩ := exitM_discard hif
          exact ⟨fun _ => ho, rfl⟩
        · obtain ⟨-, rfl⟩ := MoveM.pure_run hif
          exact ⟨fun h => (by rw [hc] at h; cases h), rfl⟩
      obtain ⟨hout, rfl⟩ := hend
      -- What the body starts with lines up with the arguments.
      have hvals : ∀ q ∈ vals.zip (fi.params.map (·.1)), q.2 ≠ .out → q.1.isSome = true := by
        intro q hq hmq
        rw [hm, List.zip_map_right] at hq
        obtain ⟨q', hq', rfl⟩ := List.mem_map.mp hq
        exact hv q' hq' hmq
      obtain ⟨hρ0, hbl0, hsh0, -⟩ := paramsFold_mag (funCtx S fi.aliases fi.consts fi.tps fi.params sig) fi.params
        vals _ σ0 fi.cvals {} st0 hfold hlen' hvals hcm (BackLent.consts _)
      have hfr0 : MFr σ0.shape σ0 := ⟨⟨[], by simp, rfl⟩, hbl0⟩
      have hl0 : σ0.length = (argVals fi.params vals fi.cvals).length := MAg.length hρ0
      have hr : Respects θ (funCtx S fi.aliases fi.consts fi.tps fi.params sig) :=
        respects_funCtx S fi.aliases fi.consts fi.tps fi.params sig (by rw [← htp]; exact hadm)
      have hq' : SQuiet (checkBlockF movesEnv fuel (funCtx S fi.aliases fi.consts fi.tps fi.params sig) fi.body) () := by
        rw [← checkBlockF_same movesEnv_same]; exact hq
      have hg' := Ground.of_groundArgs hg
      have hb := execBlock_move movesEnv A (runCall movesEnv S funs k d) (runCall_move hS hp hsg d) σ0.shape hg'
        (movesEnv_inst hg') fuel k (funCtx S fi.aliases fi.consts fi.tps fi.params sig) (argVals fi.params vals fi.cvals)
        fi.body σ0 0 (σ1, bs, cs) st0 st2 hS rfl hr hsg tf hq' hbody hf2 hfr0 hρ0
      obtain ⟨ir, -⟩ := mBlock_sstep A movesEnv fuel _ σ0 0 fi.body st0 _ st2 hbody
      generalize execBlock movesEnv (runCall movesEnv S funs k d) fuel k _ _ fi.body = m at hb ⊢
      cases m with
      | ok ρ' =>
        obtain ⟨h1, hc⟩ := hb
        cases hret : sig.ret with
        | ty t => simp [throw, throwThe, MonadExceptOf.throw]
        | void =>
          simp only [pure, Except.pure]
          obtain ⟨ha1, ha2⟩ := after_okM fi.params _ sig.params hm hsh0 (hfr0.inside ir) h1 (hout hc) hl0
          exact ⟨hpl, ha1, ha2⟩
      | error s' =>
        cases s' with
        | returned o ρ' =>
          obtain ⟨τ, hfr, h1, ho⟩ := hb
          obtain ⟨ha1, ha2⟩ := after_okM fi.params _ sig.params hm hsh0 hfr h1 ho hl0
          cases o with
          | some v => exact ⟨hpl, ha1, ha2⟩
          | none => exact ⟨hpl, ha1, ha2⟩
        | brk ρ' => simp [throw, throwThe, MonadExceptOf.throw]
        | cont ρ' => simp [throw, throwThe, MonadExceptOf.throw]
        | uninit => exact hb.elim
        | early r => exact hb.elim
        | _ => simp [throw, throwThe, MonadExceptOf.throw]

end

/-! ## The top level -/

/-- Walking the top level only sets the flag. -/
theorem mTop_flagMono (sigs : List (String × Sig)) (env : InferEnv) : ∀ (ss : List Stmt) (Γ : Ctx) (σ : MSt),
    FlagMono (mTop sigs env Γ σ ss)
  | [], _, _ => fun _ _ _ h hm => by simp only [mTop] at h; cases h; exact hm
  | s :: ss, Γ, σ => by
    have ih := fun Γ' σ' => mTop_flagMono sigs env ss Γ' σ'
    cases s
    case fun_ f =>
      simp only [mTop]
      refine FlagMono.bind (FlagMono.lift _) (fun p => ?_)
      obtain ⟨_, s1⟩ := p
      dsimp only
      exact FlagMono.bind (FlagMono.modify' (fun _ h => by simp [h])) (fun _ => ih Γ σ)
    all_goals
      simp only [mTop]
      exact FlagMono.bind (fun st a st' h hm => (mStmt_sstep sigs env fuel Γ σ 0 _ st a st' h).2.2.2 hm)
        (fun r => ih _ r.1)

/-- A top-level statement declares a type alias, or declares none anywhere
in it: `topTypeFree`, which `checkScript` reports P074 for, as the parser
does. -/
def TopTypeFree (ss : List Stmt) : Prop := ∀ s ∈ ss, (∃ x h, s = .type_ x h) ∨ s.typeFree = true

theorem TopTypeFree.of_bool : ∀ {ss : List Stmt}, topTypeFree ss = true → TopTypeFree ss
  | [], _, _, h => by simp at h
  | s :: ss, h, t, ht => by
    simp only [topTypeFree, Bool.and_eq_true, Bool.or_eq_true] at h
    rcases List.mem_cons.mp ht with rfl | ht
    · rcases h.1 with h1 | h1
      · exact .inr h1
      · left
        cases t <;> simp at h1
        exact ⟨_, _, rfl⟩
    · exact TopTypeFree.of_bool h.2 t ht

/-- **The top level of a script the move analysis lets through never reads
what was moved**: run against a program whose bodies the analysis lets
through, its statements never stop at `Stop.uninit`. -/
theorem runTopMoves_move (sigs : List (String × Sig)) (funs : List (String × FunInfo)) (k d : Nat)
    (hp : ProgMove sigs sigs funs) (hsg : SigsOk sigs) :
    ∀ (ss : List Stmt) (Γ : Ctx) (σ : MSt) (ρ : VEnv) (log : List (Ty × Val)) (st st' : DSt),
    Γ.sigs = sigs → Γ.lends = false → Γ.bounds = [] → TopTypeFree ss → SQuiet (checkTop sigs Γ ss) () →
    (mTop sigs movesEnv Γ σ ss).run st = .ok ((), st') → st'.movedHeld = false → MFr [] σ → MAg σ ρ →
    (runTopMoves sigs k d Γ ρ funs log ss).result ≠ .error .uninit
  | [], _, _, _, _, _, _, _, _, _, _, _, _, _, _, _ => by simp [runTopMoves, pure, Except.pure]
  | s :: ss, Γ, σ, ρ, log, st, st', hsig, hl, hb, htf, hq, h, hf, hfr, hρ => by
    have hcall := runCall_move sigs sigs funs k (SigsIn.refl sigs) hp hsg d
    have hs : SigsIn sigs Γ.sigs := by rw [hsig]; exact SigsIn.refl sigs
    have htf' : TopTypeFree ss := fun s' hs' => htf s' (List.mem_cons_of_mem _ hs')
    cases s
    case fun_ f =>
      simp only [mTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨p, st1, -, h⟩ := h
      obtain ⟨_, s1⟩ := p
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st2, -, h⟩ := h
      simp only [checkTop] at hq
      obtain ⟨_, -, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      simp only [runTopMoves]
      exact runTopMoves_move sigs funs k d hp hsg ss Γ σ ρ log st2 st' hsig hl hb htf' hq h hf hfr hρ
    case debuglog e =>
      simp only [mTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨r, st1, h1, h⟩ := h
      rw [fuel_succ] at h1
      unfold mStmt at h1
      replace h1 := MoveM.bind_ok h1
      obtain ⟨id, st0, -, h1⟩ := h1
      dsimp only at h1
      replace h1 := MoveM.bind_ok h1
      obtain ⟨σ1, st2, h2, h1⟩ := h1
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h1
      dsimp only at h
      obtain ⟨sta, stb, hE, -, hfb⟩ := leaving_run h2
      have hf2 := flag_before (mTop_flagMono sigs movesEnv ss _ _ _ _ _ h) hf
      have hev := evalS_move movesEnv sigs (runCall movesEnv sigs funs k d) hcall [] (fuel - 1) Γ.borrow Γ e σ σ1 sta stb
        hs (CtxLe.refl _) hE (hfb.trans hf2) hfr ρ hρ
      have lσ := (mE_estep sigs Γ _ σ e sta σ1 stb hE).1
      simp only [checkTop] at hq
      obtain ⟨Γ', hq1, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      have e1 : ((checkStmt Γ (.debuglog e)).run #[]).1 = Γ' := congrArg Prod.fst hq1
      -- A `debuglog` binds nothing: the context after it is the one before.
      have hΓ' : Γ' = Γ := by
        have hq2 := hq1
        simp only [checkStmt] at hq2
        rw [fuel_succ] at hq2
        simp only [checkStmtF] at hq2
        obtain ⟨_, -, hq2⟩ := SQuiet.bind_inv (fun _ => Appends.pure _) hq2
        rw [SQuiet.pure_iff] at hq2
        exact hq2.symm
      rw [e1, hΓ'] at h
      rw [hΓ'] at hq
      simp only [runTopMoves]
      cases ht : typeOf (synthF movesEnv (fuel - 1) Γ.borrow e) with
      | error s' =>
        simp only
        intro hu
        have : s' = .wrong := by
          unfold typeOf at ht; split at ht <;> simp_all [pure, Except.pure, throw, throwThe, MonadExceptOf.throw]
        subst this
        simp [Stop.unreturn, throw, throwThe, MonadExceptOf.throw] at hu
      | ok t =>
        generalize (evalS movesEnv (runCall movesEnv sigs funs k d) (fuel - 1) Γ.borrow e).run ρ = m at hev ⊢
        cases m with
        | ok q =>
          obtain ⟨v, ρ'⟩ := q
          simp only
          exact runTopMoves_move sigs funs k d hp hsg ss Γ σ1 ρ' _ _ st' hsig hl hb htf' hq h hf (hfr.of_le lσ) hev.1
        | error s' =>
          simp only
          intro hu
          have hne : s' ≠ .uninit := by intro e; subst e; exact hev
          exact unreturn_ne_uninit hne (by simpa [throw, throwThe, MonadExceptOf.throw] using hu)
    case type_ x hh =>
      simp only [mTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨r, st1, h1, h⟩ := h
      rw [fuel_succ] at h1
      unfold mStmt at h1
      replace h1 := MoveM.bind_ok h1
      obtain ⟨id, st0, hid, h1⟩ := h1
      dsimp only at h1
      obtain ⟨rfl, rfl⟩ := MoveM.pure_run h1
      dsimp only at h
      simp only [checkTop] at hq
      obtain ⟨Γ', hq1, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      have e1 : ((checkStmt Γ (.type_ x hh)).run #[]).1 = Γ' := congrArg Prod.fst hq1
      rw [e1] at h
      simp only [runTopMoves]
      rw [fuel_succ]
      simp only [execStmt, pure, Except.pure]
      rw [e1]
      exact runTopMoves_move sigs funs k d hp hsg ss Γ' σ ρ _ _ st' (by rw [← e1]; exact (checkStmtF_sigs inferEnv fuel Γ _ #[]).trans hsig)
        (by rw [← e1]; exact (lends_checkStmtF inferEnv fuel Γ _ #[]).trans hl) (by rw [← e1]; exact (bounds_checkStmtF inferEnv fuel Γ _ #[]).trans hb) htf' hq h hf hfr hρ
    all_goals
      simp only [mTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨r, st1, h1, h⟩ := h
      simp only [checkTop] at hq
      obtain ⟨Γ', hq1, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      have e1 : ((checkStmt Γ _).run #[]).1 = Γ' := congrArg Prod.fst hq1
      have hf1 : st1.movedHeld = false := flag_before (mTop_flagMono sigs movesEnv ss _ _ _ _ _ h) hf
      obtain ⟨i1, -⟩ := mStmt_sstep sigs movesEnv fuel Γ σ 0 _ st r st1 h1
      have hq1' := hq1
      simp only [checkStmt] at hq1'
      rw [checkStmtF_same movesEnv_same] at hq1'
      rcases htf _ List.mem_cons_self with ⟨x, hh, e⟩ | htfs
      · cases e
      have hex := execStmt_move movesEnv sigs (runCall movesEnv sigs funs k d) hcall [] Ground.nil (movesEnv_inst Ground.nil)
        fuel k Γ Γ' ρ _ σ 0 r st st1 hs hl (Respects.nil hb) (by rw [hsig]; exact hsg) htfs hq1' h1 hf1 hfr hρ
      rw [Ctx.inst_nil hb] at hex
      rw [e1] at h
      simp only [runTopMoves]
      generalize execStmt movesEnv (runCall movesEnv sigs funs k d) fuel k Γ ρ _ = m at hex ⊢
      cases m with
      | ok ρ' =>
        simp only
        rw [e1]
        exact runTopMoves_move sigs funs k d hp hsg ss Γ' r.1 ρ' _ _ st' (by rw [← e1]; exact (checkStmtF_sigs inferEnv fuel Γ _ #[]).trans hsig)
          (by rw [← e1]; exact (lends_checkStmtF inferEnv fuel Γ _ #[]).trans hl) (by rw [← e1]; exact (bounds_checkStmtF inferEnv fuel Γ _ #[]).trans hb) htf' hq h hf
          (hfr.inside i1) hex.1
      | error s' =>
        simp only
        intro hu
        have hne : s' ≠ .uninit := by intro e; subst e; exact hex
        exact unreturn_ne_uninit hne (by simpa [throw, throwThe, MonadExceptOf.throw] using hu)

/-! ## A script's functions -/

/-- A function as declared is one the move analysis let through where
`mTop` reached it, and that checked quietly where `checkScript` did. -/
def FunDefMove (sigs : List (String × Sig)) (fd : FunDef) : Prop :=
  ∀ sig, sigs.lookup fd.name = some sig →
    fd.params.map (·.1) = sig.params.map (·.1) ∧ sig.tparams = fd.tps ∧ blockTypeFree fd.body = true ∧
    SQuiet (checkBlockF inferEnv fuel (funCtx sigs fd.aliases fd.consts fd.tps fd.params sig) fd.body) () ∧
    ∃ ret s, (mFun sigs movesEnv fd.aliases fd.consts (fd.consts.map (·.1, true))
      (.mk fd.name fd.tps fd.params ret fd.body)).run {} = .ok ((), s) ∧ s.movedHeld = false

theorem funDefs_move {ss0 : List Stmt} (hn : (resolveNames ss0).2 = #[]) :
    ∀ (ss : List Stmt) (Γ : Ctx) (σ : MSt) (seen : Nat) (st st' : DSt), (∀ s ∈ ss, s ∈ ss0) →
    (mTop (resolveNames ss0).1 movesEnv Γ σ ss).run st = .ok ((), st') → st'.movedHeld = false →
    SQuiet (checkTop (resolveNames ss0).1 Γ ss) () →
    ∀ fd ∈ funDefs (resolveNames ss0).1 Γ seen ss, FunDefMove (resolveNames ss0).1 fd
  | [], _, _, _, _, _, _, _, _, _ => by simp [funDefs]
  | s :: ss, Γ, σ, seen, st, st', hsub, h, hf, hq => by
    have hsub' : ∀ s' ∈ ss, s' ∈ ss0 := fun s' hs' => hsub s' (List.mem_cons_of_mem _ hs')
    cases s
    case fun_ f =>
      obtain ⟨name, tps, params, ret, body⟩ := f
      simp only [mTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨p, st1, h1, h⟩ := h
      obtain ⟨u, s1⟩ := p
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st2, h2, h⟩ := h
      have e2 := MoveM.modify_run h2
      have hf2 : st2.movedHeld = false := flag_before (mTop_flagMono _ _ ss _ _ _ _ _ h) hf
      have hs1 : s1.movedHeld = false := by
        rw [e2] at hf2
        simp only [Bool.or_eq_false_iff] at hf2
        exact hf2.2
      obtain ⟨hmf, -⟩ := lift_run h1
      simp only [checkTop] at hq
      obtain ⟨_, hqd, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      intro fd hfd
      simp only [funDefs, List.mem_append] at hfd
      rcases hfd with hfd | hfd
      · simp only [funDef] at hfd
        split at hfd
        · rename_i sig0 hsig0
          simp only [Option.toList_some, List.mem_singleton] at hfd
          subst hfd
          intro sig hs
          simp only at hs
          rw [hsig0] at hs
          cases hs
          obtain ⟨sig', hl', hmodes⟩ := decl_modes hn (hsub _ List.mem_cons_self)
          rw [hsig0] at hl'
          cases hl'
          obtain ⟨sig'', hl'', htp, -⟩ := resolveNames_own hn name tps params ret body (hsub _ List.mem_cons_self)
          rw [hsig0] at hl''
          cases hl''
          simp only [checkDecl] at hqd
          obtain ⟨⟨⟩, h74, hqf⟩ := SQuiet.bind_inv (fun _ => checkFun_appends _ _ _ _) hqd
          rw [SQuiet.reportIf_iff] at h74
          have tf : blockTypeFree body = true := by simpa using h74
          simp only [checkFun, hsig0, checkBlock] at hqf
          obtain ⟨_, -, hqf⟩ := SQuiet.bind_inv
            (fun _ => Appends.bind (checkBlockF_appends _ _ _ _) (fun _ => Appends.report _ _)) hqf
          obtain ⟨_, hbody, -⟩ := SQuiet.bind_inv (fun _ => Appends.report _ _) hqf
          cases u
          exact ⟨hmodes.symm, htp, tf, hbody, ret, s1, hmf, hs1⟩
        · simp at hfd
      · exact funDefs_move hn ss Γ σ seen st2 st' hsub' h hf hq fd hfd
    all_goals
      simp only [mTop] at h
      replace h := MoveM.bind_ok h
      obtain ⟨r, st1, -, h⟩ := h
      simp only [checkTop] at hq
      obtain ⟨Γ', hq1, hq⟩ := SQuiet.bind_inv (fun _ => checkTop_appends _ _ _) hq
      have e1 : ((checkStmt Γ _).run #[]).1 = Γ' := congrArg Prod.fst hq1
      intro fd hfd
      simp only [funDefs] at hfd
      rw [e1] at hfd h
      exact funDefs_move hn ss Γ' r.1 _ st1 st' hsub' h hf hq fd hfd

/-- The table of a script's functions is a program each of whose bodies the
move analysis lets through, where the consts each sees hold values. -/
theorem funTable_move {ss : List Stmt} (hn : (resolveNames ss).2 = #[]) {Γ : Ctx} {σ : MSt} {st st' : DSt}
    (h : (mTop (resolveNames ss).1 movesEnv Γ σ ss).run st = .ok ((), st')) (hf : st'.movedHeld = false)
    (hq : SQuiet (checkTop (resolveNames ss).1 Γ ss) ()) {store : CStore}
    (hc : ∀ fd ∈ funDefs (resolveNames ss).1 Γ 0 ss,
      ∀ q ∈ constEnv fd.consts (store.drop (store.length - fd.seen)), q.2.isSome = true) :
    ProgMove (resolveNames ss).1 (resolveNames ss).1 (funTable (funDefs (resolveNames ss).1 Γ 0 ss) store) := by
  intro f sig fi hs hl
  have hm := lookup_mem_of hl
  simp only [funTable, List.mem_map] at hm
  obtain ⟨fd, hfd, he⟩ := hm
  simp only [FunDef.entry, Prod.mk.injEq] at he
  obtain ⟨rfl, rfl⟩ := he
  obtain ⟨hmodes, htp, tf, hqb, ret, s, hmf, hsf⟩ := funDefs_move hn ss Γ σ 0 st st' (fun _ h => h) h hf hq fd hfd sig hs
  refine ⟨hmodes, htp, tf, hqb, fd.consts.map (·.1, true), ret, s, ?_, by rw [← mFun_eq_mFunIn]; exact hmf, hsf⟩
  exact MAg.consts _ _ (by simp [constEnv, List.map_map, Function.comp_def]) (hc fd hfd)

/-- **The consts a function sees hold values**, with the run that moves:
`constPass_has` of `constPassMoves`, which evaluates the same consts in the
same order. -/
theorem constPassMoves_has (sigs : List (String × Sig)) (defs : List FunDef) (k d : Nat) :
    ∀ (ss : List Stmt) (Γ : Ctx) (store final : CStore) (seen : Nat),
    store.length = seen → StoreHas Γ.vars store → constPassMoves sigs defs k d Γ store ss = .ok final →
    (∃ δ, final = δ ++ store) ∧ ∀ fd ∈ funDefs sigs Γ seen ss,
      ∀ q ∈ constEnv fd.consts (final.drop (final.length - fd.seen)), q.2.isSome = true
  | [], Γ, store, final, seen, _, _, h => by
    simp only [constPassMoves, pure, Except.pure, Except.ok.injEq] at h; subst h
    exact ⟨⟨[], rfl⟩, by simp [funDefs]⟩
  | st :: ss, Γ, store, final, seen, hl, hs, h => by
   cases st
   case fun_ f =>
    simp only [constPassMoves] at h
    obtain ⟨⟨δ, rfl⟩, ih⟩ := constPassMoves_has sigs defs k d ss Γ store final seen hl hs h
    refine ⟨⟨δ, rfl⟩, fun fd hfd => ?_⟩
    simp only [funDefs, List.mem_append] at hfd
    rcases hfd with hfd | hfd
    · obtain ⟨name, tps, params, ret, body⟩ := f
      simp only [funDef] at hfd
      split at hfd
      · simp only [Option.toList_some, List.mem_singleton] at hfd; subst hfd
        simp only [List.length_append, hl, Nat.add_sub_cancel, List.drop_left' (rfl : δ.length = δ.length)]
        exact constEnv_some hs
      · simp at hfd
    · exact ih fd hfd
   case const_ x hint e =>
    simp only [constPassMoves, ↓reduceIte] at h
    obtain ⟨ρ, he, h⟩ := bind_ok h
    have hx : ∃ v ρ1, ρ = (x, some v) :: ρ1 := by
      rw [fuel_succ] at he
      simp only [execStmt] at he
      exact execBind_head he
    obtain ⟨v, ρ1, rfl⟩ := hx
    have hvars := checkConst_vars inferEnv (fuel - 1) Γ x hint e #[]
    rw [← fuel_succ] at hvars
    simp only [checkStmt] at h ⊢
    generalize hG : ((checkStmtF inferEnv fuel Γ (.const_ x hint e)).run #[]).1 = Γ' at h hvars ⊢
    cases hv : Γ'.vars with
    | nil => rw [hv] at h; cases h
    | cons p rest =>
      obtain ⟨y, u, kk⟩ := p
      rw [hv] at h
      simp only at h
      have hs' : StoreHas Γ'.vars ((y, u, some v) :: store) := by
        refine ⟨fun q hq => ?_, fun q hq hk => ?_⟩
        · rcases List.mem_cons.mp hq with rfl | hq
          · rfl
          · exact hs.1 q hq
        · rcases hvars with hvs | ⟨p', hvs⟩
          · obtain ⟨w, hw⟩ := hs.2 q (hvs ▸ hq) hk
            exact ⟨w, List.mem_cons_of_mem _ hw⟩
          · rw [hvs] at hq hv
            simp only [List.cons.injEq] at hv
            obtain ⟨rfl, -⟩ := hv
            rcases List.mem_cons.mp hq with rfl | hq
            · exact ⟨v, List.mem_cons_self⟩
            · obtain ⟨w, hw⟩ := hs.2 q hq hk
              exact ⟨w, List.mem_cons_of_mem _ hw⟩
      obtain ⟨⟨δ, rfl⟩, ih⟩ := constPassMoves_has sigs defs k d ss Γ' _ final (seen + 1) (by simp [hl]) hs' h
      refine ⟨⟨δ ++ [(y, u, some v)], by simp⟩, fun fd hfd => ?_⟩
      simp only [funDefs, checkStmt, hG] at hfd
      exact ih fd hfd
   all_goals
    simp only [constPassMoves] at h
    obtain ⟨hδ, ih⟩ := constPassMoves_has sigs defs k d ss _ store final seen hl
      (by exact ⟨hs.1, fun q hq hk => hs.2 q
        (checkStmtF_noConst inferEnv fuel Γ _ #[] (fun _ _ _ e => Stmt.noConfusion e) q hq hk) hk⟩) h
    exact ⟨hδ, fun fd hfd => ih fd (by simpa [funDefs] using hfd)⟩

/-- **A script the move analysis lets through never reads what was moved**:
where a script checks, the move analysis lets it through, and no call writes
back into a variable a later argument moved, running it with the run that
moves stops at `Stop.uninit` only where evaluating its consts, before
anything else, does. Past the consts no variable is read while it holds
nothing, moved or never given a value, and no call returns with a `mut` or
`out` parameter it left holding nothing. -/
theorem runScriptMoves_move (ss : List Stmt) (h : checkScript ss = #[]) (hm : moveScript ss = none)
    (hh : moveScriptHeld ss = false) (k d : Nat)
    (hu : (runScriptMoves k d ss).result = .error .uninit) :
    constPassMoves (resolveNames ss).1 (funDefs (resolveNames ss).1
      { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss) k d
      { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss = .error .uninit := by
  have hn := checkScript_names h
  have hq := checkScript_quiet h
  have htf := TopTypeFree.of_bool (checkScript_parts h).2
  unfold moveScript at hm
  unfold moveScriptHeld at hh
  simp only [StateT.run'] at hm
  cases ht : (mTop (resolveNames ss).1 movesEnv { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss).run {}
    with
  | error c =>
    have ht' : mTop (resolveNames ss).1 movesEnv { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss {} =
      .error c := ht
    rw [ht'] at hm
    simp [Functor.map, Except.map] at hm
  | ok p =>
    obtain ⟨u, s⟩ := p
    cases u
    simp only at hh
    rw [ht] at hh
    simp only at hh
    simp only [runScriptMoves] at hu
    revert hu
    cases hcp : constPassMoves (resolveNames ss).1 (funDefs (resolveNames ss).1
        { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } 0 ss) k d
        { ret := .ty (.result unitTy), sigs := (resolveNames ss).1 } [] ss with
    | ok store =>
      intro hu
      obtain ⟨-, hc⟩ := constPassMoves_has _ _ k d ss _ [] store 0 rfl ⟨by simp, by simp⟩ hcp
      exact absurd hu (runTopMoves_move _ _ k d (funTable_move hn ht hh hq hc) (resolveNames_sigsOk ss) ss _ [] [] []
        {} s rfl rfl rfl htf hq ht hh ⟨⟨[], by simp, rfl⟩, fun _ hv _ => (by cases hv)⟩ .nil)
    | error s' =>
      intro hu
      simp only [throw, throwThe, MonadExceptOf.throw, Except.error.injEq] at hu
      rw [unreturn_uninit hu]

end Datalove.Fun
