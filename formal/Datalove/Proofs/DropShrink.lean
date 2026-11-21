import Datalove.FunMove

/-!
# Along an expression, what is live only shrinks

The first step of the drop schedule's proof (`botdocs/plan-drop-proof.md`):
the move analysis's walk of an expression (`mE` and its parts) leaves every
variable as it found it but for whether it is live and has been given a
value, and leaves live only what was live before it. Nothing in an
expression gives a value to what holds none: an `out` argument has to be
live, and a `mut` one too, so the value a call gives them back was held
when the expression began. And the walk of an expression schedules nothing:
it keeps the statement and binding counters, the points and what is held as
they were.

So a return from partway along an expression holds what is live once it is
done, and of what was live before it, what it has not moved yet: the `?`
point's drops.
-/

namespace Datalove.Fun

/-! ## The analysis's monad -/

theorem MoveM.run_bind {α β} (x : MoveM α) (f : α → MoveM β) (st : DSt) :
    (x >>= f).run st = (x.run st) >>= fun p => (f p.1).run p.2 := rfl

theorem MoveM.run_pure {α} (a : α) (st : DSt) : (Pure.pure a : MoveM α).run st = .ok (a, st) := rfl

theorem MoveM.bind_ok {α β} {x : MoveM α} {f : α → MoveM β} {st st' : DSt} {b : β}
    (h : (x >>= f).run st = .ok (b, st')) :
    ∃ a st1, x.run st = .ok (a, st1) ∧ (f a).run st1 = .ok (b, st') := by
  rw [MoveM.run_bind] at h
  cases hx : x.run st with
  | error c => rw [hx] at h; cases h
  | ok p => rw [hx] at h; exact ⟨p.1, p.2, rfl, h⟩

theorem MoveM.throw_run {α} (c : Lit.Code) (st : DSt) :
    (throw c : MoveM α).run st = .error c := rfl

/-! ## What an expression leaves of the variables -/

/-- `w` is `v` but perhaps no longer live, or given a value. -/
def MVar.Below (w v : MVar) : Prop :=
  w.name = v.name ∧ w.uid = v.uid ∧ w.owns = v.owns ∧ w.lent = v.lent ∧ w.writable = v.writable ∧
    w.back = v.back ∧ w.byRef = v.byRef ∧ w.walked = v.walked ∧ (w.live = true → v.live = true)

theorem MVar.Below.refl (v : MVar) : v.Below v := ⟨rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl, id⟩

theorem MVar.Below.trans {u v w : MVar} (h1 : u.Below v) (h2 : v.Below w) : u.Below w :=
  ⟨h1.1.trans h2.1, h1.2.1.trans h2.2.1, h1.2.2.1.trans h2.2.2.1, h1.2.2.2.1.trans h2.2.2.2.1,
    h1.2.2.2.2.1.trans h2.2.2.2.2.1, h1.2.2.2.2.2.1.trans h2.2.2.2.2.2.1,
    h1.2.2.2.2.2.2.1.trans h2.2.2.2.2.2.2.1, h1.2.2.2.2.2.2.2.1.trans h2.2.2.2.2.2.2.2.1,
    fun h => h2.2.2.2.2.2.2.2.2 (h1.2.2.2.2.2.2.2.2 h)⟩

/-- `τ` is `σ`, each variable perhaps no longer live, or given a value. -/
inductive MSt.Below : MSt → MSt → Prop where
  | nil : MSt.Below [] []
  | cons {w v : MVar} {τ σ : MSt} : w.Below v → MSt.Below τ σ → MSt.Below (w :: τ) (v :: σ)

theorem MSt.Below.refl : ∀ (σ : MSt), σ.Below σ
  | [] => .nil
  | v :: σ => .cons (MVar.Below.refl v) (MSt.Below.refl σ)

theorem MSt.Below.trans : ∀ {a b c : MSt}, a.Below b → b.Below c → a.Below c
  | [], [], [], .nil, .nil => .nil
  | _ :: _, _ :: _, _ :: _, .cons h1 t1, .cons h2 t2 => .cons (h1.trans h2) (MSt.Below.trans t1 t2)

theorem MSt.Below.length : ∀ {a b : MSt}, a.Below b → a.length = b.length
  | [], [], .nil => rfl
  | _ :: _, _ :: _, .cons _ t => by simp [MSt.Below.length t]

/-- Changing the innermost `x` by `f`, which keeps a variable below itself,
keeps the variables below themselves. -/
theorem MSt.update_below {f : MVar → MVar} (hf : ∀ v, (f v).Below v) (x : String) :
    ∀ (σ : MSt), (MSt.update f x σ).Below σ
  | [] => .nil
  | v :: σ => by
    simp only [MSt.update]
    split
    · exact .cons (hf v) (MSt.Below.refl σ)
    · exact .cons (MVar.Below.refl v) (MSt.update_below hf x σ)

theorem MSt.take_below (x : String) (σ : MSt) : (σ.take x).Below σ :=
  MSt.update_below (f := fun v => { v with live := false })
    (fun _ => ⟨rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl, fun h => by cases h⟩) x σ

/-! ## Steps that schedule nothing -/

/-- What the drop schedule keeps besides the expression counter and where
each binding was last moved, which an expression's walk leaves as it was. -/
def DSt.sched (st : DSt) : Nat × Nat × List Nat × List (Nat × String) ×
    List (String × Nat × Option Nat × List (Nat × Option Nat)) × List (Nat × Nat) × List Nat :=
  (st.next, st.uid, st.held, st.names, st.out, st.madeAt, st.tracked)

/-- A step that leaves the schedule as it was. -/
def KeepsSched {α} (m : MoveM α) : Prop := ∀ st a st', m.run st = .ok (a, st') → st'.sched = st.sched

theorem KeepsSched.pure {α} (a : α) : KeepsSched (Pure.pure a : MoveM α) := fun _ _ _ h => by
  cases h; rfl

theorem KeepsSched.throw {α} (c : Lit.Code) : KeepsSched (throw c : MoveM α) := fun _ _ _ h => by cases h

theorem KeepsSched.bind {α β} {x : MoveM α} {f : α → MoveM β} (hx : KeepsSched x) (hf : ∀ a, KeepsSched (f a)) :
    KeepsSched (x >>= f) := fun st b st' h => by
  obtain ⟨a, st1, h1, h2⟩ := MoveM.bind_ok h
  exact (hf a st1 b st' h2).trans (hx st a st1 h1)

theorem KeepsSched.exprAt : KeepsSched Fun.exprAt := fun _ _ _ h => by cases h; rfl

theorem KeepsSched.exprFrom (n : Nat) : KeepsSched (Fun.exprFrom n) := fun _ _ _ h => by cases h; rfl

/-- An expression's walk from `σ`: what it leaves is below `σ`, and it
schedules nothing. -/
def Shr (σ : MSt) (m : MoveM MSt) : Prop :=
  ∀ st τ st', m.run st = .ok (τ, st') → τ.Below σ ∧ st'.sched = st.sched

theorem Shr.pure (σ : MSt) : Shr σ (Pure.pure σ) := fun _ _ _ h => by
  cases h; exact ⟨MSt.Below.refl _, rfl⟩

theorem Shr.pure' {σ τ : MSt} (h : τ.Below σ) : Shr σ (Pure.pure τ) := fun _ _ _ e => by
  cases e; exact ⟨h, rfl⟩

theorem Shr.throw (σ : MSt) (c : Lit.Code) : Shr σ (throw c) := fun _ _ _ h => by cases h

theorem Shr.bind {x : MoveM MSt} {f : MSt → MoveM MSt} {σ : MSt} (hx : Shr σ x)
    (hf : ∀ σ1, σ1.Below σ → Shr σ (f σ1)) : Shr σ (x >>= f) := fun st τ st' h => by
  obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
  obtain ⟨b1, c1⟩ := hx st σ1 st1 h1
  obtain ⟨b2, c2⟩ := hf σ1 b1 st1 τ st' h2
  exact ⟨b2, c2.trans c1⟩

/-- A step that schedules nothing, then one that shrinks. -/
theorem Shr.calm_bind {α} {x : MoveM α} {f : α → MoveM MSt} {σ : MSt} (hx : KeepsSched x)
    (hf : ∀ a, Shr σ (f a)) : Shr σ (x >>= f) := fun st τ st' h => by
  obtain ⟨a, st1, h1, h2⟩ := MoveM.bind_ok h
  obtain ⟨b2, c2⟩ := hf a st1 τ st' h2
  exact ⟨b2, c2.trans (hx st a st1 h1)⟩

theorem Shr.trans {σ τ : MSt} {m : MoveM MSt} (h : Shr τ m) (e : τ.Below σ) : Shr σ m :=
  fun st υ st' r => by
    obtain ⟨b, c⟩ := h st υ st' r
    exact ⟨b.trans e, c⟩

theorem Shr.readM (σ : MSt) (x : String) : Shr σ (Fun.readM σ x) := by
  unfold Fun.readM
  split
  · split
    · exact Shr.throw σ _
    · split
      · exact Shr.throw σ _
      · exact Shr.pure σ
  · exact Shr.pure σ

theorem Shr.exitM (σ : MSt) : Shr σ (Fun.exitM σ) := by
  unfold Fun.exitM
  split
  · exact Shr.pure σ
  · exact Shr.throw σ _

theorem KeepsSched.unwalkedM (σ : MSt) (x : String) : KeepsSched (Fun.unwalkedM σ x) := by
  unfold Fun.unwalkedM
  split
  · exact KeepsSched.throw _
  · exact KeepsSched.pure _

theorem KeepsSched.noteOverMoved (id : Nat) (σ : MSt) (x : String) (steps : List Step) :
    KeepsSched (Fun.noteOverMoved id σ x steps) := by
  unfold Fun.noteOverMoved
  split
  · intro st a st' h
    simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
    cases h; rfl
  · exact KeepsSched.pure _

theorem KeepsSched.writableM (σ : MSt) (x : String) : KeepsSched (Fun.writableM σ x) := by
  unfold Fun.writableM
  split
  · split
    · exact KeepsSched.pure _
    · split
      · exact KeepsSched.throw _
      · exact KeepsSched.throw _
  · exact KeepsSched.pure _

theorem Shr.moveM (Γ : Ctx) (σ : MSt) (x : String) : Shr σ (Fun.moveM Γ σ x) := by
  unfold Fun.moveM
  split
  · split
    · exact Shr.readM σ x
    · exact Shr.throw σ _
  · refine Shr.bind (Shr.readM σ x) (fun σ1 h1 => ?_)
    split
    · exact Shr.pure' h1
    · split
      · split
        · exact Shr.throw σ _
        · split
          · exact Shr.throw σ _
          · intro st τ st' e
            simp only [MoveM.run_bind] at e
            cases e
            exact ⟨(MSt.take_below x σ1).trans h1, rfl⟩
      · exact Shr.pure' h1
  · exact Shr.readM σ x


/-! ## A call gives back only what was held -/

theorem MSt.Below.find {x : String} : ∀ {τ σ : MSt}, τ.Below σ → ∀ {v : MVar}, σ.find x = some v →
    ∃ w, τ.find x = some w ∧ w.Below v
  | [], [], .nil, _, h => by simp [MSt.find] at h
  | w :: τ, v :: σ, .cons hw t, u, h => by
    simp only [MSt.find, List.find?] at h ⊢
    have hn : w.name = v.name := hw.1
    rw [hn]
    split
    · next e => rw [e] at h; cases h; exact ⟨w, rfl, hw⟩
    · next e => rw [e] at h; exact MSt.Below.find t h

/-- What a statement's walk found live, if `x` names anything: the root of a
`mut` or `out` argument. -/
def MSt.liveAt (σ : MSt) (x : String) : Prop := ∀ v, σ.find x = some v → v.live = true

theorem MSt.liveAt.up {τ σ : MSt} {x : String} (h : τ.Below σ) (l : τ.liveAt x) : σ.liveAt x := by
  intro v hv
  obtain ⟨w, hw, b⟩ := h.find hv
  exact b.2.2.2.2.2.2.2.2 (l w hw)

theorem MSt.give_below {x : String} : ∀ {τ σ : MSt}, τ.Below σ → σ.liveAt x → (τ.give x).Below σ
  | [], [], .nil, _ => .nil
  | w :: τ, v :: σ, .cons hw t, l => by
    simp only [MSt.give, MSt.update]
    have hn : w.name = v.name := hw.1
    split
    · next e =>
      have e' : (v.name == x) = true := by rw [← hn]; exact e
      have hv : v.live = true := l v (by simp only [MSt.find, List.find?, e'])
      exact .cons ⟨hw.1, hw.2.1, hw.2.2.1, hw.2.2.2.1, hw.2.2.2.2.1, hw.2.2.2.2.2.1,
        hw.2.2.2.2.2.2.1, hw.2.2.2.2.2.2.2.1, fun _ => hv⟩ t
    · next e =>
      have e' : (v.name == x) = false := by rw [← hn]; simpa using e
      refine .cons hw (MSt.give_below t (fun u hu => l u ?_))
      simp only [MSt.find, List.find?, e']; exact hu

/-- The roots of a call's `mut` and `out` arguments were live in `σ`. -/
def ArgsLive (σ : MSt) : List (Option Mode × Expr) → List Mode → Prop
  | (_, e) :: args, m :: ms =>
    ((m == .mut || m == .out) = true → ∀ x, placeOf e = some (x, []) → σ.liveAt x) ∧ ArgsLive σ args ms
  | _, _ => True

theorem ArgsLive.up {τ σ : MSt} (h : τ.Below σ) :
    ∀ (args : List (Option Mode × Expr)) (ms : List Mode), ArgsLive τ args ms → ArgsLive σ args ms
  | (_, _) :: args, _ :: ms, ⟨a, r⟩ => ⟨fun hm x hx => (a hm x hx).up h, ArgsLive.up h args ms r⟩
  | [], _, _ => trivial
  | _ :: _, [], _ => trivial

theorem giveArgsM_below : ∀ {τ σ : MSt} (args : List (Option Mode × Expr)) (ms : List Mode),
    τ.Below σ → ArgsLive σ args ms → (giveArgsM τ args ms).Below σ
  | τ, σ, (_, e) :: args, m :: ms, h, ⟨a, r⟩ => by
    simp only [giveArgsM]
    refine giveArgsM_below args ms ?_ r
    split
    · next hm =>
      split
      · next x hx => exact MSt.give_below h (a hm x hx)
      · exact h
    · exact h
  | _, _, [], _, h, _ => by simp only [giveArgsM]; exact h
  | _, _, _ :: _, [], h, _ => by simp only [giveArgsM]; exact h

theorem readM_live {σ : MSt} {x : String} {st τ st'} (h : (readM σ x).run st = .ok (τ, st')) :
    σ.liveAt x := by
  intro v hv
  unfold readM at h
  rw [hv] at h
  simp only at h
  split at h
  · cases h
  · split at h
    · cases h
    · next e => simpa using e


/-! ## The walk of an expression -/

/-- An expression's walk, with what it adds: the counter of expressions set
past it. -/
theorem Shr.wrap {σ : MSt} {e : Expr} {m : MoveM MSt} (h : Shr σ m) :
    Shr σ (Fun.exprAt >>= fun start => m >>= fun σ' =>
      Fun.exprFrom (start + e.nodes) >>= fun _ => (Pure.pure σ' : MoveM MSt)) :=
  Shr.calm_bind KeepsSched.exprAt (fun _ =>
    Shr.bind h (fun _ h1 => Shr.calm_bind (KeepsSched.exprFrom _) (fun _ => Shr.pure' h1)))

/-- `ShrA` of a call's arguments: what `Shr` says, and that each root of a
`mut` or `out` argument was live where the arguments began. -/
def ShrA (σ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode) (m : MoveM MSt) : Prop :=
  ∀ st τ st', m.run st = .ok (τ, st') → τ.Below σ ∧ st'.sched = st.sched ∧ ArgsLive σ args ms

section
variable (sigs : List (String × Sig)) (Γ : Ctx)

theorem mPlace_live : ∀ (e : Expr) (σ : MSt) (need : Bool) {x : String} {st τ st'},
    placeOf e = some (x, []) → (mPlace sigs Γ σ e need).run st = .ok (τ, st') → σ.liveAt x
  | .var y, σ, need, x, st, τ, st', hp, h => by
    simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
    obtain ⟨rfl, -⟩ := hp
    unfold mPlace at h
    obtain ⟨_, _, _, h2⟩ := MoveM.bind_ok h
    obtain ⟨_, _, h3, _⟩ := MoveM.bind_ok h2
    dsimp only at h3
    split at h3
    · exact readM_live h3
    · intro v hv
      rw [hv] at h3
      dsimp only at h3
      split at h3
      · cases h3
      · next e => simpa using e
  | .place y steps, σ, need, x, st, τ, st', hp, h => by
    simp only [placeOf, Option.some.injEq, Prod.mk.injEq] at hp
    obtain ⟨rfl, -⟩ := hp
    unfold mPlace at h
    obtain ⟨_, _, _, h2⟩ := MoveM.bind_ok h
    obtain ⟨_, _, h3, _⟩ := MoveM.bind_ok h2
    dsimp only at h3
    obtain ⟨_, _, h4, _⟩ := MoveM.bind_ok h3
    exact readM_live h4
  | .hinted _ a, σ, need, x, st, τ, st', hp, h => by
    simp only [placeOf] at hp
    unfold mPlace at h
    obtain ⟨_, _, _, h2⟩ := MoveM.bind_ok h
    obtain ⟨_, _, h3, _⟩ := MoveM.bind_ok h2
    dsimp only at h3
    exact mPlace_live a σ need hp h3
  | .proj b s, _, _, _, _, _, _, hp, _ => by
    simp only [placeOf] at hp
    cases hb : placeOf b with
    | none => rw [hb] at hp; cases hp
    | some p => rw [hb] at hp; simp at hp
  | .index b k m, _, _, _, _, _, _, hp, _ => by
    simp only [placeOf] at hp
    cases hb : placeOf b with
    | none => rw [hb] at hp; cases hp
    | some p => rw [hb] at hp; simp at hp
  | .none_, _, _, _, _, _, _, hp, _ | .tt, _, _, _, _, _, _, hp, _ | .ff, _, _, _, _, _, _, hp, _
  | .str _, _, _, _, _, _, _, hp, _ | .int _, _, _, _, _, _, _, hp, _ | .float, _, _, _, _, _, _, hp, _
  | .hex _, _, _, _, _, _, _, hp, _ | .tuple _, _, _, _, _, _, _, hp, _
  | .struct _, _, _, _, _, _, _, hp, _ | .list _, _, _, _, _, _, _, hp, _
  | .set _, _, _, _, _, _, _, hp, _ | .map _, _, _, _, _, _, _, hp, _
  | .tensor _ _, _, _, _, _, _, _, hp, _ | .table _ _, _, _, _, _, _, _, hp, _
  | .some_ _, _, _, _, _, _, _, hp, _ | .ok _, _, _, _, _, _, _, hp, _ | .er _, _, _, _, _, _, _, hp, _
  | .data _, _, _, _, _, _, _, hp, _ | .error _, _, _, _, _, _, _, hp, _
  | .atom _, _, _, _, _, _, _, hp, _ | .term _ _, _, _, _, _, _, _, hp, _
  | .enum _, _, _, _, _, _, _, hp, _ | .binop _ _ _, _, _, _, _, _, _, hp, _
  | .unop _ _, _, _, _, _, _, _, hp, _ | .tryOpt _, _, _, _, _, _, _, hp, _
  | .tryRes _, _, _, _, _, _, _, hp, _ | .adapt _, _, _, _, _, _, _, hp, _
  | .call _ _, _, _, _, _, _, _, hp, _ => by simp [placeOf] at hp


theorem ShrA.shr {σ : MSt} {args ms} {m : MoveM MSt} (h : ShrA σ args ms m) : Shr σ m :=
  fun st τ st' e => ⟨(h st τ st' e).1, (h st τ st' e).2.1⟩

mutual

theorem mE_shr : ∀ (b : Bool) (σ : MSt) (e : Expr), Shr σ (mE sigs Γ b σ e)
  | b, σ, .var x => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    split
    · exact Shr.readM σ x
    · exact Shr.moveM Γ σ x
  | b, σ, .place x steps => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    exact Shr.bind (Shr.readM σ x) (fun σ1 h1 => (mSteps_shr σ1 steps).trans h1)
  | b, σ, .hinted _ a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .adapt a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .some_ a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .ok a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .er a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .data a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .error a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .term _ a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .enum a => by unfold mE; exact Shr.wrap (mE_shr _ σ a)
  | b, σ, .binop op a c => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    refine Shr.bind (mE_shr _ σ a) (fun σ1 h1 =>
      Shr.bind ((mE_shr _ σ1 c).trans h1) (fun σ2 h2 => ?_))
    split
    · exact (Shr.exitM σ2).trans h2
    · exact (Shr.pure σ2).trans h2
  | b, σ, .unop op a => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    refine Shr.bind (mE_shr _ σ a) (fun σ1 h1 => ?_)
    split
    · exact (Shr.exitM σ1).trans h1
    · exact (Shr.exitM σ1).trans h1
    · exact (Shr.pure σ1).trans h1
  | b, σ, .tryOpt a => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    exact Shr.bind (mE_shr _ σ a) (fun σ1 h1 => (Shr.exitM σ1).trans h1)
  | b, σ, .tryRes a => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    exact Shr.bind (mE_shr _ σ a) (fun σ1 h1 => (Shr.exitM σ1).trans h1)
  | b, σ, .proj base _ => by unfold mE; exact Shr.wrap (mE_shr _ σ base)
  | b, σ, .index base k (some _) => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    exact Shr.bind (mE_shr _ σ base) (fun σ1 h1 =>
      Shr.bind ((mE_shr _ σ1 k).trans h1) (fun σ2 h2 => (Shr.exitM σ2).trans h2))
  | b, σ, .index _ _ none => by unfold mE; exact Shr.wrap (Shr.pure σ)
  | b, σ, .call f args => by
    unfold mE; refine Shr.wrap ?_; dsimp only
    split
    · intro st τ st' e
      obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok e
      obtain ⟨b1, c1, l1⟩ := mArgs_shr σ args _ st σ1 st1 h1
      obtain ⟨σ2, st2, h3, h4⟩ := MoveM.bind_ok h2
      have r2 : σ2.Below σ ∧ st2.sched = st1.sched := by
        split at h3
        · exact ((Shr.exitM σ1).trans b1) st1 σ2 st2 h3
        · exact ((Shr.pure σ1).trans b1) st1 σ2 st2 h3
      cases h4
      exact ⟨giveArgsM_below args _ r2.1 l1, r2.2.trans c1⟩
    · exact Shr.throw σ _
  | b, σ, .tuple es => by unfold mE; exact Shr.wrap (mAll_shr σ es)
  | b, σ, .list es => by unfold mE; exact Shr.wrap (mAll_shr σ es)
  | b, σ, .set es => by unfold mE; exact Shr.wrap (mAll_shr σ es)
  | b, σ, .tensor _ es => by unfold mE; exact Shr.wrap (mAll_shr σ es)
  | b, σ, .struct fs => by unfold mE; exact Shr.wrap (mFields_shr σ fs)
  | b, σ, .map es => by unfold mE; exact Shr.wrap (mEntries_shr σ es)
  | b, σ, .table _ rows => by unfold mE; exact Shr.wrap (mRows_shr σ rows)
  | b, σ, .none_ | b, σ, .tt | b, σ, .ff | b, σ, .str _ | b, σ, .int _ | b, σ, .float | b, σ, .hex _
  | b, σ, .atom _ => by unfold mE; exact Shr.wrap (Shr.pure σ)

theorem mSteps_shr : ∀ (σ : MSt) (steps : List Step), Shr σ (mSteps sigs Γ σ steps)
  | σ, [] => by unfold mSteps; exact Shr.pure σ
  | σ, .field _ :: rest => by unfold mSteps; exact mSteps_shr σ rest
  | σ, .index k m :: rest => by
    unfold mSteps
    refine Shr.bind (mE_shr _ σ k) (fun σ1 h1 => Shr.bind ?_ (fun σ2 h2 => (mSteps_shr σ2 rest).trans h2))
    split
    · exact (Shr.exitM σ1).trans h1
    · exact (Shr.pure σ1).trans h1

theorem mPlace_shr : ∀ (σ : MSt) (e : Expr) (need : Bool), Shr σ (mPlace sigs Γ σ e need)
  | σ, .var x, need => by
    unfold mPlace; refine Shr.wrap ?_; dsimp only
    split
    · exact Shr.readM σ x
    · split
      · split
        · exact Shr.throw σ _
        · exact Shr.pure σ
      · exact Shr.pure σ
  | σ, .place x steps, _ => by
    unfold mPlace; refine Shr.wrap ?_; dsimp only
    exact Shr.bind (Shr.readM σ x) (fun σ1 h1 => (mSteps_shr σ1 steps).trans h1)
  | σ, .hinted _ a, need => by unfold mPlace; exact Shr.wrap (mPlace_shr σ a need)
  | σ, .proj base _, _ => by unfold mPlace; exact Shr.wrap (mPlace_shr σ base true)
  | σ, .index base k (some _), _ => by
    unfold mPlace; refine Shr.wrap ?_; dsimp only
    exact Shr.bind (mPlace_shr σ base true) (fun σ1 h1 =>
      Shr.bind ((mE_shr _ σ1 k).trans h1) (fun σ2 h2 => (Shr.exitM σ2).trans h2))
  | σ, .index _ _ none, _ | σ, .none_, _ | σ, .tt, _ | σ, .ff, _ | σ, .str _, _ | σ, .int _, _
  | σ, .float, _ | σ, .hex _, _ | σ, .tuple _, _ | σ, .struct _, _ | σ, .list _, _ | σ, .set _, _
  | σ, .map _, _ | σ, .tensor _ _, _ | σ, .table _ _, _ | σ, .some_ _, _ | σ, .ok _, _ | σ, .er _, _
  | σ, .data _, _ | σ, .error _, _ | σ, .atom _, _ | σ, .term _ _, _ | σ, .enum _, _
  | σ, .binop _ _ _, _ | σ, .unop _ _, _ | σ, .tryOpt _, _ | σ, .tryRes _, _ | σ, .adapt _, _
  | σ, .call _ _, _ => by unfold mPlace; exact Shr.wrap (Shr.pure σ)

theorem mArgs_shr : ∀ (σ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode),
    ShrA σ args ms (mArgs sigs Γ σ args ms)
  | σ, (o, e) :: args, m :: ms => by
    unfold mArgs
    intro st τ st' h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    obtain ⟨b2, c2, l2⟩ := mArgs_shr σ1 args ms st1 τ st' h2
    have first : σ1.Below σ ∧ st1.sched = st.sched ∧
        ((m == .mut || m == .out) = true → ∀ x, placeOf e = some (x, []) → σ.liveAt x) := by
      split at h1
      · next x steps hp =>
        split at h1
        · next hm =>
          obtain ⟨_, s1, u1, u2⟩ := MoveM.bind_ok h1
          obtain ⟨_, s2, w1, w2⟩ := MoveM.bind_ok u2
          have cu := KeepsSched.unwalkedM σ x st _ s1 u1
          have cw := KeepsSched.writableM σ x s1 _ s2 w1
          obtain ⟨bp, cp⟩ := mPlace_shr σ e _ s2 σ1 st1 w2
          refine ⟨bp, cp.trans (cw.trans cu), fun _ y hy => mPlace_live sigs Γ e σ _ hy w2⟩
        · next hm =>
          obtain ⟨bp, cp⟩ := mE_shr _ σ e st σ1 st1 h1
          exact ⟨bp, cp, fun h' => absurd h' hm⟩
      · next hp =>
        split at h1
        · cases h1
        · next hm =>
          obtain ⟨bp, cp⟩ := mE_shr _ σ e st σ1 st1 h1
          exact ⟨bp, cp, fun h' => absurd h' hm⟩
    exact ⟨b2.trans first.1, c2.trans first.2.1, first.2.2, ArgsLive.up first.1 args ms l2⟩
  | σ, [], _ => by
    unfold mArgs; intro st τ st' h; cases h; exact ⟨MSt.Below.refl _, rfl, trivial⟩
  | σ, _ :: _, [] => by
    unfold mArgs; intro st τ st' h; cases h; exact ⟨MSt.Below.refl _, rfl, trivial⟩

theorem mAll_shr : ∀ (σ : MSt) (es : List Expr), Shr σ (mAll sigs Γ σ es)
  | σ, [] => by unfold mAll; exact Shr.pure σ
  | σ, e :: es => by
    unfold mAll
    exact Shr.bind (mE_shr _ σ e) (fun σ1 h1 => (mAll_shr σ1 es).trans h1)

theorem mFields_shr : ∀ (σ : MSt) (fs : List (String × Expr)), Shr σ (mFields sigs Γ σ fs)
  | σ, [] => by unfold mFields; exact Shr.pure σ
  | σ, (_, e) :: fs => by
    unfold mFields
    exact Shr.bind (mE_shr _ σ e) (fun σ1 h1 => (mFields_shr σ1 fs).trans h1)

theorem mEntries_shr : ∀ (σ : MSt) (es : List (Expr × Expr)), Shr σ (mEntries sigs Γ σ es)
  | σ, [] => by unfold mEntries; exact Shr.pure σ
  | σ, (k, v) :: es => by
    unfold mEntries
    exact Shr.bind (mE_shr _ σ k) (fun σ1 h1 =>
      Shr.bind ((mE_shr _ σ1 v).trans h1) (fun σ2 h2 => (mEntries_shr σ2 es).trans h2))

theorem mRows_shr : ∀ (σ : MSt) (rows : List (List Expr)), Shr σ (mRows sigs Γ σ rows)
  | σ, [] => by unfold mRows; exact Shr.pure σ
  | σ, r :: rs => by
    unfold mRows
    exact Shr.bind (mAll_shr σ r) (fun σ1 h1 => (mRows_shr σ1 rs).trans h1)

end


theorem mKeys_shr : ∀ (σ : MSt) (steps : List Step), Shr σ (mKeys sigs Γ σ steps)
  | σ, [] => by unfold mKeys; exact Shr.pure σ
  | σ, .field _ :: rest => by unfold mKeys; exact mKeys_shr σ rest
  | σ, .index k _ :: rest => by
    unfold mKeys
    exact Shr.bind (mE_shr sigs Γ _ σ k) (fun σ1 h1 => (mKeys_shr σ1 rest).trans h1)

theorem mTarget_shr (σ : MSt) (x : String) (steps : List Step) (op : Option BinOp) :
    Shr σ (mTarget σ x steps op) := by
  unfold mTarget
  split
  · exact Shr.readM σ x
  · split
    · exact Shr.pure σ
    · split
      · split
        · exact Shr.throw σ _
        · split
          · exact Shr.throw σ _
          · exact Shr.pure σ
      · exact Shr.pure σ

end

end Datalove.Fun
