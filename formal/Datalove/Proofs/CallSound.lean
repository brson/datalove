import Datalove.FunCall
import Datalove.Proofs.ExecSound
import Datalove.Proofs.Lockstep

/-!
# Calls into a checked program are sound

`Fun.runCall_ok`: in a program each of whose functions' bodies checks, with
each type argument its bounds admit (`ProgOk`), calls run by `runCall` do what
`CallOk` says calls may: give a value of the function's return type, or stop
at what is not run yet, at a variable read before it is set, or out of
calls. With `Fun.evalS_sound` and `Fun.execBlock_sound`, which take any
sound calls, a checked expression or block that calls into the program runs
soundly.

A body that ends without `ret` gives the unit; for a function that returns a
value it cannot, since such a body is checked to not complete (F063), and a
block that ends normally completes (`Fun.execBlock_sound`). A body runs
outside any loop, so no `break` or `continue` escapes it.
-/

namespace Datalove.Fun

open Lit (Val HasType)

/-- A program whose functions' bodies check: each function of the table that
its signatures name has as many parameters as its signature, and a body that
checks quietly as an instance at any ground type arguments its bounds admit,
F063 included; and its consts' values are of their types there. -/
def ProgOk (env : InferEnv) (sigs : List (String × Sig)) (funs : List (String × FunInfo)) : Prop :=
  ∀ f sig fi, sigs.lookup f = some sig → funs.lookup f = some fi → fi.params.length = sig.params.length ∧
    ∀ θ, groundArgs θ = true →
      (∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) →
      SQuiet (checkBlockF env fuel ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ) fi.body
        >>= fun _ => reportIf (match sig.ret.inst θ with
          | .ty _ => completes fi.body
          | .void => false) "F063") () ∧
      Agree (fi.consts.map fun (x, t, k) => (x, t.inst θ, k)) fi.cvals

theorem Agree.append' : ∀ {a b : List (String × Ty × Kind)} {c d : VEnv}, Agree a c → Agree b d →
    Agree (a ++ b) (c ++ d)
  | _, _, _, _, .nil, h => h
  | _, _, _, _, .cons ho h, h' => .cons ho (Agree.append' h h')

theorem Agree.reverse : ∀ {a : List (String × Ty × Kind)} {c : VEnv}, Agree a c → Agree a.reverse c.reverse
  | _, _, .nil => .nil
  | _, _, .cons ho h => by
    simp only [List.reverse_cons]
    exact Agree.append' (Agree.reverse h) (.cons ho .nil)

/-- The parameters, bound to arguments of their types, none for an `out`
one. -/
theorem params_agree (θ : List (String × Ty)) :
    ∀ (params : List (Mode × String × Hint)) (sps : List (Mode × Ty)) (vals : List (Option Val)),
      params.length = sps.length → vals.length = sps.length →
      (∀ q ∈ vals.zip sps, ∀ v, q.1 = some v → HasType v (q.2.2.inst θ)) →
      Agree ((params.zip sps).map fun ((m, x, _), (_, t)) => (x, t.inst θ, kindOf m))
        ((params.zip vals).map fun ((_, x, _), v) => (x, v))
  | [], _, _, _, _, _ => by simp; exact .nil
  | _ :: _, [], _, h, _, _ => by simp at h
  | _ :: _, _ :: _, [], _, h, _ => by simp at h
  | (m, x, hh) :: params, (m', t) :: sps, o :: vals, h1, h2, hv => by
    simp only [List.zip_cons_cons, List.map_cons]
    refine .cons (fun w e => hv (o, (m', t)) (by simp) w e) ?_
    exact params_agree θ params sps vals (by simpa using h1) (by simpa using h2)
      (fun q hq => hv q (by simp [hq]))

/-- A body's starting variables line up with the context it was checked in. -/
theorem argVals_agree {sigs : List (String × Sig)} {fi : FunInfo} {sig : Sig} {θ : List (String × Ty)}
    {vals : List (Option Val)} (hp : fi.params.length = sig.params.length) (hv : ArgsFit θ vals sig.params)
    (hc : Agree (fi.consts.map fun (x, t, k) => (x, t.inst θ, k)) fi.cvals) :
    Agree ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ).vars
      (argVals fi.params vals fi.cvals) := by
  simp only [Ctx.inst, funCtx, argVals, List.map_append, List.map_reverse, List.map_map]
  refine Agree.append' (Agree.reverse ?_) hc
  exact params_agree θ fi.params sig.params vals hp hv.1 hv.2

theorem Agree.take : ∀ {A B : List (String × Ty × Kind)} {ρ : VEnv}, Agree (A ++ B) ρ → Agree A (ρ.take A.length)
  | [], _, _, _ => by simp; exact .nil
  | _ :: A, _, _, h => by
    cases h with
    | cons ho h => exact .cons ho (Agree.take h)

/-- What the parameters came to, read from variables that line up with
them, is of their types. -/
theorem params_fit (θ : List (String × Ty)) :
    ∀ (params : List (Mode × String × Hint)) (sps : List (Mode × Ty)) (ys : VEnv),
      params.length = sps.length →
      Agree ((params.zip sps).map fun ((m, x, _), (_, t)) => (x, t.inst θ, kindOf m)) ys →
      ArgsFit θ (ys.map (·.2)) sps
  | [], [], _, _, h => by cases h; exact ⟨rfl, by simp⟩
  | [], _ :: _, _, h, _ => by simp at h
  | _ :: _, [], _, h, _ => by simp at h
  | (m, x, hh) :: params, (m', t) :: sps, ys, hl, h => by
    simp only [List.zip_cons_cons, List.map_cons] at h
    cases h with
    | @cons _ _ _ o _ ys ho h =>
      obtain ⟨h1, h2⟩ := params_fit θ params sps ys (by simpa using hl) h
      refine ⟨by simp [h1], fun q hq v hv => ?_⟩
      simp only [List.map_cons, List.zip_cons_cons, List.mem_cons] at hq
      rcases hq with rfl | hq
      · exact ho v hv
      · exact h2 q hq v hv

/-- What `paramsAfter` reads from a body's variables back in its own scope
is of the parameters' types. -/
theorem paramsAfter_fit {sigs : List (String × Sig)} {fi : FunInfo} {sig : Sig} {θ : List (String × Ty)}
    (hp : fi.params.length = sig.params.length) {ρ : VEnv}
    (h : Agree ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ).vars ρ) :
    ArgsFit θ (paramsAfter fi.params ρ) sig.params := by
  simp only [Ctx.inst, funCtx, List.map_append, List.map_reverse, List.map_map] at h
  have ht := Agree.take h
  simp only [List.length_reverse, List.length_map, List.length_zip, ← hp, Nat.min_self] at ht
  have hr := Agree.reverse ht
  rw [List.reverse_reverse] at hr
  unfold paramsAfter
  rw [← List.map_reverse]
  exact params_fit θ fi.params sig.params _ hp hr

/-- A block never lets a statement's early return or `ret` out as such: it
has become `Stop.returned`. -/
theorem execBlock_returned {env : InferEnv} {call : CallFn} : ∀ (n k : Nat) (Γ : Ctx) (ρ : VEnv) (ss : List Stmt)
    (s : Stop), execBlock env call n k Γ ρ ss = .error s → (∀ r, s ≠ .early r) ∧ s ≠ .retVoid
  | 0, _, _, _, _, s, h => by
    rw [execBlock.eq_def] at h; cases h; exact ⟨fun _ => by simp, by simp⟩
  | _ + 1, _, _, _, [], s, h => by rw [execBlock.eq_def] at h; cases h
  | n + 1, k, Γ, ρ, s0 :: ss, s, h => by
    rw [execBlock.eq_def] at h
    dsimp only at h
    unfold returnedAt at h
    cases he : execStmt env call n k Γ ρ s0 with
    | ok ρ' =>
      rw [he] at h
      exact execBlock_returned n k _ ρ' ss s h
    | error s1 =>
      rw [he] at h
      cases s1 <;>
        (simp only [bind, Except.bind, throw, throwThe, MonadExceptOf.throw, Except.error.injEq] at h
         subst h
         exact ⟨fun _ => by simp, by simp⟩)

theorem unitVal_typed : HasType unitVal unitTy := .tuple (by simp [Lit.tysList]) (by simp)

section
variable (env : InferEnv) (sigs : List (String × Sig)) (funs : List (String × FunInfo))

/-- **Calls into a checked program are sound.** -/
theorem runCall_ok (h : ProgOk env sigs funs) (k : Nat) : ∀ d, CallOk sigs (runCall env sigs funs k d)
  | 0 => fun _ _ _ _ _ _ _ _ => .inr (.inr rfl)
  | d + 1 => by
    intro f sig θ vals hs hg hbd hv
    cases hfi : funs.lookup f with
    | none => simp only [runCall, hs, hfi]; exact .inl rfl
    | some fi =>
    obtain ⟨hpl, hq⟩ := h f sig fi hs hfi
    obtain ⟨hq, hc⟩ := hq θ hg hbd
    obtain ⟨_, hbody, h63⟩ := SQuiet.bind_inv (fun _ => Appends.report _ _) hq
    rw [SQuiet.reportIf_iff] at h63
    have hρ := argVals_agree (sigs := sigs) hpl hv hc
    have hr := execBlock_sound env sigs (runCall env sigs funs k d) (runCall_ok h k d) fuel k _ _ fi.body
      hbody ⟨rfl, rfl⟩ hρ
    have hne := execBlock_returned (env := env) (call := runCall env sigs funs k d) fuel k
      ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ) (argVals fi.params vals fi.cvals) fi.body
    have after : ∀ ρ', Ext ((funCtx sigs fi.aliases fi.consts fi.tps fi.params sig).inst θ).vars ρ' →
        ArgsFit θ (paramsAfter fi.params (restore (argVals fi.params vals fi.cvals) ρ')) sig.params :=
      fun ρ' he => paramsAfter_fit hpl (restore_agree hρ he)
    simp only [runCall, hs, hfi]
    generalize execBlock _ _ _ _ _ _ _ = r at hr hne ⊢
    cases r with
    | ok ρ' =>
      cases hret : sig.ret with
      | void => exact ⟨by simp only [Sig.retTy, hret]; exact unitVal_typed, after ρ' hr.1⟩
      | ty t =>
        simp only [hret, Ret.inst] at h63
        rw [hr.2] at h63
        cases h63
    | error s =>
      cases s with
      | early v => exact absurd rfl ((hne _ rfl).1 v)
      | retVoid => exact absurd rfl (hne _ rfl).2
      | returned o ρ' =>
        obtain ⟨hT, hx⟩ := hr
        cases o with
        | some v =>
          obtain ⟨T, hT, hvT⟩ := hT
          simp only [Ctx.inst, funCtx] at hT
          cases hret : sig.ret with
          | void => rw [hret] at hT; cases hT
          | ty t =>
            rw [hret] at hT
            simp only [Ret.inst, Ret.ty.injEq] at hT
            subst hT
            exact ⟨by simp only [Sig.retTy, hret]; exact hvT, after ρ' hx⟩
        | none =>
          have hT : sig.ret.inst θ = .void := hT
          cases hret : sig.ret with
          | void => exact ⟨by simp only [Sig.retTy, hret]; exact unitVal_typed, after ρ' hx⟩
          | ty t => rw [hret] at hT; cases hT
      | brk ρ' => exact absurd hr.1 (by simp [Ctx.inst, funCtx])
      | cont ρ' => exact absurd hr.1 (by simp [Ctx.inst, funCtx])
      | unsupported => exact .inl rfl
      | uninit => exact .inr (.inl rfl)
      | diverge => exact .inr (.inr rfl)
      | fuel => exact hr.elim
      | wrong => exact hr.elim

end

theorem Ground.of_groundArgs {θ : List (String × Ty)} (h : groundArgs θ = true) : Ground θ := by
  intro p hp
  simp only [groundArgs, List.all_eq_true, Bool.and_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true] at h
  exact h p hp

/-- What `ProgOk` asks of a function's body follows from its declaration
checking quietly in a script, by the check-once theorem, where its body
declares no type alias and its signature's type parameters are its own. -/
theorem body_checks (ss : List Stmt) (aliases : Aliases) (consts : List (String × Ty × Kind))
    (name : String) (tps : List (String × Option Bound)) (params : List (Mode × String × Hint))
    (ret : Option Hint) (body : List Stmt) (sig : Sig) (hsig : (resolveNames ss).1.lookup name = some sig)
    (htp : sig.tparams = tps) (tf : blockTypeFree body = true)
    (quiet : SQuiet (checkFun (resolveNames ss).1 aliases consts (.mk name tps params ret body)) ())
    (θ : List (String × Ty)) (hg : groundArgs θ = true)
    (hb : ∀ p b, (p, some b) ∈ sig.tparams → ∃ t, θ.lookup p = some t ∧ b.admitsTy t = true) :
    SQuiet (checkBlockF inferEnv fuel ((funCtx (resolveNames ss).1 aliases consts tps params sig).inst θ) body
      >>= fun _ => reportIf (match sig.ret.inst θ with
        | .ty _ => completes body
        | .void => false) "F063") () :=
  checkFun_instance_real (Ground.of_groundArgs hg) ss aliases consts name tps params ret body sig hsig
    (htp ▸ hb) tf quiet

end Datalove.Fun
