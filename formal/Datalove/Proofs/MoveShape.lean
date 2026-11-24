import Datalove.Proofs.DropShrink
import Datalove.Proofs.InitSound

/-!
# What a step of the move analysis keeps

What `Proofs/MoveSound.lean` needs to know of the move analysis's walk
(`FunMove.lean`) apart from what it refuses: a step never un-gives a
variable (`MSt.Le`), a statement binds in front of what it was given and
leaves the rest as `MSt.Le` says, inside and at each `break` and `continue`
(`MSt.Inside`), and once the walk notes a call that writes back into what a
later argument moved (`DSt.movedHeld`) it stays noted. The first is what the
proof's loop rests on: a way back to a loop's start that moved nothing live
at the start (O007) has given a value to everything the start had one, so
what lines up with the end lines up with the start.
-/

namespace Datalove.Fun

/-! ## No step un-gives a variable -/

/-- `τ` is `σ` with no variable un-given: the same names, each a parameter
the caller reads back where `σ`'s is, borrowed where `σ`'s is, each given a
value where `σ`'s is, and each borrowed one still live where `σ`'s is, since
no move takes a borrowed binding (O003). -/
inductive MSt.Le : MSt → MSt → Prop where
  | nil : MSt.Le [] []
  | cons {v w : MVar} {σ τ : MSt} : v.name = w.name → v.back = w.back → (v.init = true → w.init = true) →
      v.lent = w.lent → (v.lent = true → v.live = true → w.live = true) →
      MSt.Le σ τ → MSt.Le (v :: σ) (w :: τ)

theorem MSt.Le.refl : ∀ (σ : MSt), σ.Le σ
  | [] => .nil
  | _ :: σ => .cons rfl rfl id rfl (fun _ h => h) (MSt.Le.refl σ)

theorem MSt.Le.trans : ∀ {a b c : MSt}, a.Le b → b.Le c → a.Le c
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons n1 b1 i1 l1 v1 t1, .cons n2 b2 i2 l2 v2 t2 =>
    .cons (n1.trans n2) (b1.trans b2) (fun h => i2 (i1 h)) (l1.trans l2)
      (fun hl hv => v2 (l1 ▸ hl) (v1 hl hv)) (t1.trans t2)

theorem MSt.Le.length : ∀ {a b : MSt}, a.Le b → a.length = b.length
  | _, _, .nil => rfl
  | _, _, .cons _ _ _ _ _ t => by simp [MSt.Le.length t]

theorem MSt.Le.drop : ∀ {a b : MSt} (k : Nat), a.Le b → MSt.Le (List.drop k a) (List.drop k b)
  | _, _, 0, h => h
  | _, _, _ + 1, .nil => .nil
  | _, _, k + 1, .cons _ _ _ _ _ t => MSt.Le.drop k t

theorem MSt.Le.take : ∀ {a b : MSt} (k : Nat), a.Le b → MSt.Le (List.take k a) (List.take k b)
  | _, _, 0, _ => .nil
  | _, _, _ + 1, .nil => .nil
  | _, _, k + 1, .cons n b i l v t => .cons n b i l v (MSt.Le.take k t)

theorem MSt.Le.append : ∀ {a b c d : MSt}, a.Le b → c.Le d → (a ++ c).Le (b ++ d)
  | _, _, _, _, .nil, h => h
  | _, _, _, _, .cons n b i l v t, h => .cons n b i l v (MSt.Le.append t h)

/-- The names, and which are read back: what `MSt.Le` keeps. -/
def MSt.shape (σ : MSt) : List (String × Bool) := σ.map fun v => (v.name, v.back)

theorem MSt.Le.shape : ∀ {a b : MSt}, a.Le b → a.shape = b.shape
  | _, _, .nil => rfl
  | _, _, .cons n b _ _ _ t => by
    simp only [MSt.shape, List.map_cons, n, b]
    exact congrArg _ (MSt.Le.shape t)

theorem MSt.shape_length (σ : MSt) : σ.shape.length = σ.length := by simp [MSt.shape]

/-- Changing the innermost `x` by `f`, which keeps the variable's name,
`back` and `lent`, un-gives nothing of it, and keeps it live if it is
borrowed, un-gives nothing. -/
theorem MSt.Le.update {f : MVar → MVar} {x : String} : ∀ (σ : MSt),
    (∀ v, σ.find x = some v → (f v).name = v.name ∧ (f v).back = v.back ∧ (v.init = true → (f v).init = true) ∧
      (f v).lent = v.lent ∧ (v.lent = true → v.live = true → (f v).live = true)) →
    σ.Le (MSt.update f x σ)
  | [], _ => .nil
  | v :: σ, hf => by
    simp only [MSt.update]
    split
    · rename_i e
      have h := hf v (by simp [MSt.find, List.find?, e])
      exact .cons h.1.symm h.2.1.symm h.2.2.1 h.2.2.2.1.symm h.2.2.2.2 (MSt.Le.refl σ)
    · rename_i e
      exact .cons rfl rfl id rfl (fun _ h => h) (MSt.Le.update σ (fun w hw => hf w (by
        simp only [MSt.find, List.find?] at hw ⊢
        simp only [e]
        exact hw)))

theorem MSt.Le.give (x : String) (σ : MSt) : σ.Le (σ.give x) := by
  unfold MSt.give
  exact MSt.Le.update σ (fun _ _ => ⟨rfl, rfl, fun _ => rfl, rfl, fun _ _ => rfl⟩)

/-- A move of what is not borrowed. -/
theorem MSt.Le.take' {x : String} {σ : MSt} {v : MVar} (hv : σ.find x = some v) (hl : v.lent = false) :
    σ.Le (σ.take x) := by
  unfold MSt.take
  exact MSt.Le.update σ (fun w hw => by
    rw [hv] at hw; cases hw
    exact ⟨rfl, rfl, id, rfl, fun h => by rw [hl] at h; cases h⟩)

theorem MSt.Le.walk (x : String) (σ : MSt) : σ.Le (σ.walk x) := by
  unfold MSt.walk
  exact MSt.Le.update σ (fun _ _ => ⟨rfl, rfl, id, rfl, fun _ h => h⟩)

theorem MSt.Le.giveArgsM_of : ∀ (args : List (Option Mode × Expr)) (ms : List Mode) {σ σ' : MSt},
    σ.Le σ' → σ.Le (giveArgsM σ' args ms)
  | (_, e) :: args, m :: ms, σ, σ', h => by
    simp only [giveArgsM]
    apply MSt.Le.giveArgsM_of args ms
    split
    · split
      · exact h.trans (MSt.Le.give _ σ')
      · exact h
    · exact h
  | [], _, _, _, h => by simp only [giveArgsM]; exact h
  | _ :: _, [], _, _, h => by simp only [giveArgsM]; exact h

theorem MSt.Le.giveArgsM (σ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode) :
    σ.Le (giveArgsM σ args ms) := MSt.Le.giveArgsM_of args ms (MSt.Le.refl σ)

/-- Where two ways agree, from one point, neither un-gives. -/
theorem MSt.Le.initMeet : ∀ {σ t e : MSt}, σ.Le t → σ.Le e → σ.Le (t.initMeet e)
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons n1 b1 i1 l1 v1 t1, .cons _ _ i2 _ _ t2 => by
    simp only [MSt.initMeet, List.zipWith_cons_cons]
    exact .cons n1 b1 (fun h => by simp [i1 h, i2 h]) l1 v1 (MSt.Le.initMeet t1 t2)

theorem MSt.Le.initFrom : ∀ {σ b m : MSt}, σ.Le b → σ.Le m → σ.Le (b.initFrom m)
  | _, _, _, .nil, .nil => .nil
  | _, _, _, .cons n1 b1 _ l1 v1 t1, .cons _ _ i2 _ _ t2 => by
    simp only [MSt.initFrom, List.zipWith_cons_cons]
    exact .cons n1 b1 i2 l1 v1 (MSt.Le.initFrom t1 t2)

theorem MSt.join_eq {σ τ ρ : MSt} (h : σ.join τ = some ρ) : ρ = σ.initMeet τ ∧ σ.sameMoves τ = true := by
  unfold MSt.join at h
  split at h
  · cases h; exact ⟨rfl, by assumption⟩
  · cases h

/-- What a step may be given. -/
theorem MSt.Le.of_eq {σ τ : MSt} (h : τ = σ) : σ.Le τ := h ▸ MSt.Le.refl σ

/-- `τ`, of a scope inside `σ`'s, back to `σ`'s, un-gives nothing of what
`σ`'s scope held, where `τ` is `σ` with more bound in front. -/
theorem MSt.Le.back {σ τ δ τ' : MSt} (e : τ = δ ++ τ') (h : σ.Le τ') : σ.Le (σ.back τ) := by
  subst e
  unfold MSt.back
  rw [List.length_append, ← h.length, Nat.add_sub_cancel, List.drop_left]
  exact h

/-! ## What a statement binds -/

/-- `τ` is of a scope inside `σ`'s: `σ` with no variable un-given, and more
bound in front, none of them read back by a caller. -/
def MSt.Inside (σ τ : MSt) : Prop := ∃ δ τ', τ = δ ++ τ' ∧ (∀ v ∈ δ, v.back = false) ∧ σ.Le τ'

theorem MSt.Inside.of_le {σ τ : MSt} (h : σ.Le τ) : σ.Inside τ := ⟨[], τ, rfl, by simp, h⟩

theorem MSt.Inside.refl (σ : MSt) : σ.Inside σ := MSt.Inside.of_le (MSt.Le.refl σ)

theorem MSt.Inside.trans {a b c : MSt} (h1 : a.Inside b) (h2 : b.Inside c) : a.Inside c := by
  obtain ⟨δ1, τ1, rfl, hd1, l1⟩ := h1
  obtain ⟨δ2, τ2, rfl, hd2, l2⟩ := h2
  -- `τ2` is `δ1 ++ τ1` with nothing un-given: split it as `δ1` is split.
  have hl : (δ1 ++ τ1).length = τ2.length := l2.length
  refine ⟨δ2 ++ List.take δ1.length τ2, List.drop δ1.length τ2, by simp, fun v hv => ?_, ?_⟩
  · rcases List.mem_append.mp hv with hv | hv
    · exact hd2 v hv
    · have := MSt.Le.take δ1.length l2
      rw [List.take_left] at this
      exact shape_back_false this hd1 v hv
  · have := MSt.Le.drop δ1.length l2
    rw [List.drop_left] at this
    exact l1.trans this
where
  shape_back_false : ∀ {δ ε : MSt}, δ.Le ε → (∀ v ∈ δ, v.back = false) → ∀ v ∈ ε, v.back = false
    | _, _, .nil, _, _, h => by cases h
    | _, _, .cons _ b _ _ _ t, hd, v, hv => by
      rcases List.mem_cons.mp hv with rfl | hv
      · rw [← b]; exact hd _ List.mem_cons_self
      · exact shape_back_false t (fun w hw => hd w (List.mem_cons_of_mem _ hw)) v hv

theorem MSt.Inside.back {σ τ : MSt} (h : σ.Inside τ) : σ.Le (σ.back τ) := by
  obtain ⟨δ, τ', rfl, -, l⟩ := h
  exact MSt.Le.back rfl l

theorem MSt.Inside.length {σ τ : MSt} (h : σ.Inside τ) : σ.length ≤ τ.length := by
  obtain ⟨δ, τ', rfl, -, l⟩ := h
  simp [l.length]

theorem MSt.Inside.cons {σ τ : MSt} (h : σ.Inside τ) {v : MVar} (hb : v.back = false) : σ.Inside (v :: τ) := by
  obtain ⟨δ, τ', rfl, hd, l⟩ := h
  refine ⟨v :: δ, τ', rfl, fun w hw => ?_, l⟩
  rcases List.mem_cons.mp hw with rfl | hw
  · exact hb
  · exact hd w hw

/-- Where `σ.back τ` is read where `σ'` is of `σ`'s length. -/
theorem MSt.back_of_length {σ σ' τ : MSt} (h : σ'.length = σ.length) : σ'.back τ = σ.back τ := by
  simp [MSt.back, h]

/-- A choice in the analysis's monad that came to something: which way, and
that way's run. -/
theorem MoveM.ite_run {α} {c : Bool} {x y : MoveM α} {st : DSt} {r : α × DSt}
    (h : (if c = true then x else y).run st = .ok r) :
    (c = true ∧ x.run st = .ok r) ∨ (c = false ∧ y.run st = .ok r) := by
  cases c
  · exact .inr ⟨rfl, by simpa using h⟩
  · exact .inl ⟨rfl, by simpa using h⟩

theorem MoveM.modify_run {f : DSt → DSt} {st st' : DSt} {a : Unit}
    (h : (modify f : MoveM Unit).run st = .ok (a, st')) : st' = f st := by
  simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
  cases h; rfl

/-! ## The flag that a call writes back into what was moved -/

/-- A step that never clears `DSt.movedHeld`. -/
def FlagMono {α} (m : MoveM α) : Prop :=
  ∀ st a st', m.run st = .ok (a, st') → st.movedHeld = true → st'.movedHeld = true

namespace FlagMono

theorem pure {α} (a : α) : FlagMono (Pure.pure a : MoveM α) := fun _ _ _ h => by cases h; exact id

theorem throw {α} (c : Lit.Code) : FlagMono (throw c : MoveM α) := fun _ _ _ h => by cases h

theorem bind {α β} {x : MoveM α} {f : α → MoveM β} (hx : FlagMono x) (hf : ∀ a, FlagMono (f a)) :
    FlagMono (x >>= f) := fun st b st' h hm => by
  obtain ⟨a, st1, h1, h2⟩ := MoveM.bind_ok h
  exact hf a st1 b st' h2 (hx st a st1 h1 hm)

theorem ite {α} {c : Bool} {x y : MoveM α} (hx : FlagMono x) (hy : FlagMono y) :
    FlagMono (if c then x else y) := by
  cases c
  · simpa using hy
  · simpa using hx

/-- A `modify` that keeps the flag or sets it. -/
theorem modify' {f : DSt → DSt} (hf : ∀ s, s.movedHeld = true → (f s).movedHeld = true) :
    FlagMono (modify f : MoveM Unit) := fun st _ st' h hm => by
  simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
  cases h; exact hf st hm

theorem get' : FlagMono (get : MoveM DSt) := fun _ _ _ h => by
  simp only [get, getThe, MonadStateOf.get, StateT.get, StateT.run] at h
  cases h; exact id

theorem stmtId : FlagMono Fun.stmtId := fun _ _ _ h => by
  simp only [Fun.stmtId, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
  cases h; exact id

theorem exprAt : FlagMono Fun.exprAt := fun _ _ _ h => by
  simp only [Fun.exprAt, get, getThe, MonadStateOf.get, StateT.run, Pure.pure] at h
  cases h; exact id

theorem exprFrom (n : Nat) : FlagMono (Fun.exprFrom n) := modify' (fun _ h => h)

theorem noteHeldMoved (σ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode) :
    FlagMono (Fun.noteHeldMoved σ args ms) := by
  unfold Fun.noteHeldMoved
  exact ite (modify' (fun _ _ => rfl)) (pure _)

theorem noteOverMoved (id : Nat) (σ : MSt) (x : String) (steps : List Step) :
    FlagMono (Fun.noteOverMoved id σ x steps) := by
  unfold Fun.noteOverMoved
  exact ite (modify' (fun _ h => h)) (pure _)

theorem unwalkedM (σ : MSt) (x : String) : FlagMono (Fun.unwalkedM σ x) := by
  unfold Fun.unwalkedM
  exact ite (throw _) (pure _)

theorem writableM (σ : MSt) (x : String) : FlagMono (Fun.writableM σ x) := by
  unfold Fun.writableM
  split
  · exact ite (pure _) (ite (throw _) (throw _))
  · exact pure _

theorem readM (σ : MSt) (x : String) : FlagMono (Fun.readM σ x) := by
  unfold Fun.readM
  split
  · exact ite (throw _) (ite (throw _) (pure _))
  · exact pure _

theorem exitM (σ : MSt) : FlagMono (Fun.exitM σ) := by
  unfold Fun.exitM
  exact ite (pure _) (throw _)

theorem moveM (Γ : Ctx) (σ : MSt) (x : String) : FlagMono (Fun.moveM Γ σ x) := by
  unfold Fun.moveM
  split
  · exact ite (readM _ _) (throw _)
  · refine bind (readM _ _) (fun σ1 => ?_)
    refine ite (pure _) ?_
    split
    · exact ite (throw _) (ite (throw _) (bind (modify' (fun _ h => h)) (fun _ => pure _)))
    · exact pure _
  · exact readM _ _

theorem record (kind : String) (stmt : Nat) (arm : Option Nat) (drops : List Nat) (always : Bool) :
    FlagMono (Fun.record kind stmt arm drops always) := by
  unfold Fun.record
  exact ite (modify' (fun _ h => h)) (pure _)

theorem recordNotYetMoved (stmt : Nat) (moved : List Nat) : FlagMono (Fun.recordNotYetMoved stmt moved) := by
  intro st a st' h hm
  simp only [Fun.recordNotYetMoved, get, getThe, MonadStateOf.get, StateT.run] at h
  split at h
  · simp only [set, MonadStateOf.set] at h; cases h; exact hm
  · simp only [Pure.pure] at h; cases h; exact hm

theorem ownedIn (vs : MSt) : FlagMono (Fun.ownedIn vs) := by
  intro st a st' h hm
  simp only [Fun.ownedIn, get, getThe, MonadStateOf.get, StateT.run, Pure.pure] at h
  cases h; exact hm

theorem bindOne (Γ' : Ctx) (init lent writable byRef : Bool) (σ : MSt) (x : String) :
    FlagMono (Fun.bindOne Γ' init lent writable byRef σ x) := by
  intro st a st' h hm
  simp only [Fun.bindOne, get, getThe, MonadStateOf.get, StateT.run, set, MonadStateOf.set, Pure.pure] at h
  cases h; exact hm

theorem bindM (Γ' : Ctx) (init writable : Bool) : ∀ (xs : List String) (σ : MSt),
    FlagMono (Fun.bindM Γ' σ xs init writable)
  | [], σ => by simp only [Fun.bindM, List.foldlM]; exact pure _
  | x :: xs, σ => by
    simp only [Fun.bindM, List.foldlM]
    exact bind (bindOne Γ' init false writable false σ x) (fun σ1 => bindM Γ' init writable xs σ1)

theorem holdFrom (u : Nat) : ∀ s : DSt, s.movedHeld = true → (Fun.holdFrom u s).movedHeld = true :=
  fun _ h => h

theorem forM' {α} (f : α → MoveM Unit) (hf : ∀ a, FlagMono (f a)) : ∀ (l : List α), FlagMono (l.forM f)
  | [] => by simp only [List.forM]; exact pure _
  | a :: l => by
    simp only [List.forM]
    exact bind (hf a) (fun _ => forM' f hf l)

theorem loopHeld (σc : MSt) (body : List Stmt) (r : MOut) : FlagMono (Fun.loopHeld σc body r) := by
  unfold Fun.loopHeld
  refine forM' _ (fun p => ?_) _
  exact ite (modify' (holdFrom _)) (pure _)

theorem joinAll (code : Lit.Code) (σ : MSt) : ∀ (L : List MSt), FlagMono (Fun.joinAll code σ L)
  | [] => by simp only [Fun.joinAll]; exact pure _
  | τ :: L => by
    simp only [Fun.joinAll]
    exact fold L τ
where
  fold : ∀ (L : List MSt) (acc : MSt), FlagMono (L.foldlM (fun (acc w : MSt) => match acc.join w with
      | some j => Pure.pure j
      | none => MonadExcept.throw code) acc)
    | [], acc => by simp only [List.foldlM]; exact pure _
    | w :: L, acc => by
      simp only [List.foldlM]
      refine bind ?_ (fun j => fold L j)
      split
      · exact pure _
      · exact throw _

theorem joinIf (t : MSt) (tc : Bool) (e : MSt) (ec : Bool) : FlagMono (Fun.joinIf t tc e ec) := by
  unfold Fun.joinIf
  exact ite (throw _) (pure _)

theorem joinMatch (σ : MSt) (arms : List (MSt × Bool)) : FlagMono (Fun.joinMatch σ arms) := by
  intro st a st' h hm
  simp only [Fun.joinMatch] at h
  generalize (arms.filter (·.2)).map (·.1) = merging at h
  cases merging with
  | cons m ms =>
    dsimp only at h
    rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
    · cases h
    · cases h; exact hm
  | nil =>
    dsimp only at h
    cases hh : arms.head? with
    | none => simp only [hh] at h; cases h; exact hm
    | some p =>
      obtain ⟨f, sf⟩ := p
      cases hl : arms.getLast? with
      | none => simp only [hh, hl] at h; cases h; exact hm
      | some q =>
        obtain ⟨l, sl⟩ := q
        simp only [hh, hl] at h; cases h; exact hm

theorem loopEnd (σ σc : MSt) (natural : Bool) (body : List Stmt) (r : MOut) :
    FlagMono (Fun.loopEnd σ σc natural body r) := by
  unfold Fun.loopEnd
  exact ite (throw _) (joinAll _ _ _)

theorem lift {α} (m : MoveM α) : FlagMono (StateT.lift (m.run {}) : MoveM (α × DSt)) := by
  intro st a st' h hm
  simp only [StateT.lift, StateT.run] at h
  cases hr : m {} with
  | error c => rw [hr] at h; cases h
  | ok p => rw [hr] at h; cases h; exact hm

end FlagMono

/-! ## Expressions -/

/-- An expression's walk from `σ`: what it leaves un-gives nothing of `σ`,
and the flag is kept or set. -/
def EStep (σ : MSt) (m : MoveM MSt) : Prop :=
  ∀ st τ st', m.run st = .ok (τ, st') → σ.Le τ ∧ (st.movedHeld = true → st'.movedHeld = true)

namespace EStep

theorem pure (σ : MSt) : EStep σ (Pure.pure σ) := fun _ _ _ h => by cases h; exact ⟨MSt.Le.refl σ, id⟩

theorem pure' {σ τ : MSt} (h : σ.Le τ) : EStep σ (Pure.pure τ) := fun _ _ _ e => by cases e; exact ⟨h, id⟩

theorem throw (σ : MSt) (c : Lit.Code) : EStep σ (throw c) := fun _ _ _ h => by cases h

theorem of_flag {σ : MSt} {m : MoveM MSt} (hs : ∀ st τ st', m.run st = .ok (τ, st') → σ.Le τ) (hf : FlagMono m) :
    EStep σ m := fun st τ st' h => ⟨hs st τ st' h, hf st τ st' h⟩

theorem bind {x : MoveM MSt} {f : MSt → MoveM MSt} {σ : MSt} (hx : EStep σ x) (hf : ∀ τ, EStep τ (f τ)) :
    EStep σ (x >>= f) := fun st τ st' h => by
  obtain ⟨τ1, st1, h1, h2⟩ := MoveM.bind_ok h
  obtain ⟨l1, f1⟩ := hx st τ1 st1 h1
  obtain ⟨l2, f2⟩ := hf τ1 st1 τ st' h2
  exact ⟨l1.trans l2, fun hm => f2 (f1 hm)⟩

theorem calm_bind {α} {x : MoveM α} {f : α → MoveM MSt} {σ : MSt} (hx : FlagMono x) (hf : ∀ a, EStep σ (f a)) :
    EStep σ (x >>= f) := fun st τ st' h => by
  obtain ⟨a, st1, h1, h2⟩ := MoveM.bind_ok h
  obtain ⟨l2, f2⟩ := hf a st1 τ st' h2
  exact ⟨l2, fun hm => f2 (hx st a st1 h1 hm)⟩

theorem trans {σ τ : MSt} {m : MoveM MSt} (h : EStep τ m) (e : σ.Le τ) : EStep σ m := fun st ρ st' hr =>
  let ⟨l, f⟩ := h st ρ st' hr
  ⟨e.trans l, f⟩

theorem ite {c : Bool} {x y : MoveM MSt} {σ : MSt} (hx : EStep σ x) (hy : EStep σ y) :
    EStep σ (if c then x else y) := by
  cases c
  · simpa using hy
  · simpa using hx

theorem readM (σ : MSt) (x : String) : EStep σ (Fun.readM σ x) := by
  refine of_flag (fun st τ st' h => ?_) (FlagMono.readM σ x)
  unfold Fun.readM at h
  split at h
  · split at h
    · cases h
    · split at h
      · cases h
      · cases h; exact MSt.Le.refl σ
  · cases h; exact MSt.Le.refl σ

theorem exitM (σ : MSt) : EStep σ (Fun.exitM σ) := by
  refine of_flag (fun st τ st' h => ?_) (FlagMono.exitM σ)
  unfold Fun.exitM at h
  split at h
  · cases h; exact MSt.Le.refl σ
  · cases h

/-- A choice on whether a binding is borrowed, throwing where it is: what
the other way does holds where it is not. -/
theorem ite_lent {σ : MSt} {v : MVar} {c : Lit.Code} {y : MoveM MSt} (hy : v.lent = false → EStep σ y) :
    EStep σ (if v.lent then MonadExcept.throw c else y) := by
  cases hl : v.lent
  · simp only [Bool.false_eq_true, ↓reduceIte]; exact hy hl
  · simp only [↓reduceIte]; exact throw σ c

theorem moveM (Γ : Ctx) (σ : MSt) (x : String) : EStep σ (Fun.moveM Γ σ x) := by
  unfold Fun.moveM
  split
  · exact ite (readM σ x) (throw σ _)
  · refine bind (readM σ x) (fun σ1 => ?_)
    refine ite (pure σ1) ?_
    split
    · rename_i v hv
      refine ite (throw _ _) ?_
      refine EStep.ite_lent (fun hl => ?_)
      refine calm_bind (FlagMono.modify' (fun _ h => h)) (fun _ => ?_)
      exact pure' (MSt.Le.take' hv hl)
    · exact pure σ1
  · exact readM σ x

/-- An expression's walk, with what it adds: the counter of expressions set
past it. -/
theorem wrap {σ : MSt} {e : Expr} {m : MoveM MSt} (h : EStep σ m) :
    EStep σ (Fun.exprAt >>= fun start => m >>= fun σ' =>
      Fun.exprFrom (start + e.nodes) >>= fun _ => (Pure.pure σ' : MoveM MSt)) :=
  calm_bind FlagMono.exprAt (fun _ =>
    bind h (fun τ => calm_bind (FlagMono.exprFrom _) (fun _ => pure τ)))

end EStep

section
variable (sigs : List (String × Sig)) (Γ : Ctx)

mutual

theorem mE_estep : ∀ (b : Bool) (σ : MSt) (e : Expr), EStep σ (mE sigs Γ b σ e)
  | b, σ, .var x => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    exact EStep.ite (EStep.readM σ x) (EStep.moveM Γ σ x)
  | b, σ, .place x steps => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    exact EStep.bind (EStep.readM σ x) (fun σ1 => mSteps_estep σ1 steps)
  | b, σ, .hinted _ a => by unfold mE; exact EStep.wrap (mE_estep _ σ a)
  | b, σ, .adapt a => by unfold mE; exact EStep.wrap (mE_estep _ σ a)
  | b, σ, .some_ a | b, σ, .ok a | b, σ, .er a | b, σ, .data a | b, σ, .error a | b, σ, .term _ a => by
    unfold mE; exact EStep.wrap (mE_estep _ σ a)
  | b, σ, .enum a => by unfold mE; exact EStep.wrap (mE_estep _ σ a)
  | b, σ, .binop op a c => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    refine EStep.bind (mE_estep _ σ a) (fun σ1 => EStep.bind (mE_estep _ σ1 c) (fun σ2 => ?_))
    exact EStep.ite (EStep.exitM σ2) (EStep.pure σ2)
  | b, σ, .unop op a => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    refine EStep.bind (mE_estep _ σ a) (fun σ1 => ?_)
    split
    · exact EStep.exitM σ1
    · exact EStep.exitM σ1
    · exact EStep.pure σ1
  | b, σ, .tryOpt a => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    exact EStep.bind (mE_estep _ σ a) (fun σ1 => EStep.exitM σ1)
  | b, σ, .tryRes a => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    exact EStep.bind (mE_estep _ σ a) (fun σ1 => EStep.exitM σ1)
  | b, σ, .proj base _ => by unfold mE; exact EStep.wrap (mE_estep _ σ base)
  | b, σ, .index base k (some _) => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    exact EStep.bind (mE_estep _ σ base) (fun σ1 => EStep.bind (mE_estep _ σ1 k) (fun σ2 => EStep.exitM σ2))
  | b, σ, .index _ _ none => by unfold mE; exact EStep.wrap (EStep.pure σ)
  | b, σ, .call f args => by
    unfold mE; refine EStep.wrap ?_; dsimp only
    split
    · refine EStep.bind (mArgs_estep σ args _) (fun σ1 => ?_)
      refine EStep.calm_bind (FlagMono.noteHeldMoved σ1 args _) (fun _ => ?_)
      refine EStep.bind (EStep.ite (EStep.exitM σ1) (EStep.pure σ1)) (fun σ2 => ?_)
      exact EStep.pure' (MSt.Le.giveArgsM σ2 args _)
    · exact EStep.throw σ _
  | b, σ, .tuple es | b, σ, .list es | b, σ, .set es | b, σ, .tensor _ es => by
    unfold mE; exact EStep.wrap (mAll_estep σ es)
  | b, σ, .struct fs => by unfold mE; exact EStep.wrap (mFields_estep σ fs)
  | b, σ, .map es => by unfold mE; exact EStep.wrap (mEntries_estep σ es)
  | b, σ, .table _ rows => by unfold mE; exact EStep.wrap (mRows_estep σ rows)
  | b, σ, .none_ | b, σ, .tt | b, σ, .ff | b, σ, .str _ | b, σ, .int _ | b, σ, .float | b, σ, .hex _
  | b, σ, .atom _ => by unfold mE; exact EStep.wrap (EStep.pure σ)

theorem mSteps_estep : ∀ (σ : MSt) (steps : List Step), EStep σ (mSteps sigs Γ σ steps)
  | σ, [] => by unfold mSteps; exact EStep.pure σ
  | σ, .field _ :: rest => by unfold mSteps; exact mSteps_estep σ rest
  | σ, .index k m :: rest => by
    unfold mSteps
    refine EStep.bind (mE_estep _ σ k) (fun σ1 => ?_)
    refine EStep.bind ?_ (fun σ2 => mSteps_estep σ2 rest)
    split
    · exact EStep.exitM σ1
    · exact EStep.pure σ1

theorem mPlace_estep : ∀ (σ : MSt) (e : Expr) (need : Bool), EStep σ (mPlace sigs Γ σ e need)
  | σ, .var x, need => by
    unfold mPlace; refine EStep.wrap ?_; dsimp only
    split
    · exact EStep.readM σ x
    · split
      · exact EStep.ite (EStep.throw σ _) (EStep.pure σ)
      · exact EStep.pure σ
  | σ, .place x steps, _ => by
    unfold mPlace; refine EStep.wrap ?_; dsimp only
    exact EStep.bind (EStep.readM σ x) (fun σ1 => mSteps_estep σ1 steps)
  | σ, .hinted _ a, need => by unfold mPlace; exact EStep.wrap (mPlace_estep σ a need)
  | σ, .proj base _, _ => by unfold mPlace; exact EStep.wrap (mPlace_estep σ base true)
  | σ, .index base k (some _), _ => by
    unfold mPlace; refine EStep.wrap ?_; dsimp only
    exact EStep.bind (mPlace_estep σ base true) (fun σ1 =>
      EStep.bind (mE_estep _ σ1 k) (fun σ2 => EStep.exitM σ2))
  | σ, .index _ _ none, _ | σ, .none_, _ | σ, .tt, _ | σ, .ff, _ | σ, .str _, _ | σ, .int _, _
  | σ, .float, _ | σ, .hex _, _ | σ, .tuple _, _ | σ, .struct _, _ | σ, .list _, _ | σ, .set _, _
  | σ, .map _, _ | σ, .tensor _ _, _ | σ, .table _ _, _ | σ, .some_ _, _ | σ, .ok _, _ | σ, .er _, _
  | σ, .data _, _ | σ, .error _, _ | σ, .atom _, _ | σ, .term _ _, _ | σ, .enum _, _
  | σ, .binop _ _ _, _ | σ, .unop _ _, _ | σ, .tryOpt _, _ | σ, .tryRes _, _ | σ, .adapt _, _
  | σ, .call _ _, _ => by unfold mPlace; exact EStep.wrap (EStep.pure σ)

theorem mArgs_estep : ∀ (σ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode),
    EStep σ (mArgs sigs Γ σ args ms)
  | σ, (_, e) :: args, m :: ms => by
    unfold mArgs
    refine EStep.bind ?_ (fun σ1 => mArgs_estep σ1 args ms)
    split
    · split
      · exact EStep.calm_bind (FlagMono.unwalkedM σ _) (fun _ =>
          EStep.calm_bind (FlagMono.writableM σ _) (fun _ => mPlace_estep σ e _))
      · exact mE_estep _ σ e
    · split
      · exact EStep.throw σ _
      · exact mE_estep _ σ e
  | σ, [], _ => by unfold mArgs; exact EStep.pure σ
  | σ, _ :: _, [] => by unfold mArgs; exact EStep.pure σ

theorem mAll_estep : ∀ (σ : MSt) (es : List Expr), EStep σ (mAll sigs Γ σ es)
  | σ, [] => by unfold mAll; exact EStep.pure σ
  | σ, e :: es => by unfold mAll; exact EStep.bind (mE_estep _ σ e) (fun σ1 => mAll_estep σ1 es)

theorem mFields_estep : ∀ (σ : MSt) (fs : List (String × Expr)), EStep σ (mFields sigs Γ σ fs)
  | σ, [] => by unfold mFields; exact EStep.pure σ
  | σ, (_, e) :: fs => by unfold mFields; exact EStep.bind (mE_estep _ σ e) (fun σ1 => mFields_estep σ1 fs)

theorem mEntries_estep : ∀ (σ : MSt) (es : List (Expr × Expr)), EStep σ (mEntries sigs Γ σ es)
  | σ, [] => by unfold mEntries; exact EStep.pure σ
  | σ, (k, v) :: es => by
    unfold mEntries
    exact EStep.bind (mE_estep _ σ k) (fun σ1 => EStep.bind (mE_estep _ σ1 v) (fun σ2 => mEntries_estep σ2 es))

theorem mRows_estep : ∀ (σ : MSt) (rows : List (List Expr)), EStep σ (mRows sigs Γ σ rows)
  | σ, [] => by unfold mRows; exact EStep.pure σ
  | σ, r :: rs => by unfold mRows; exact EStep.bind (mAll_estep σ r) (fun σ1 => mRows_estep σ1 rs)

end

theorem mKeys_estep : ∀ (σ : MSt) (steps : List Step), EStep σ (mKeys sigs Γ σ steps)
  | σ, [] => by unfold mKeys; exact EStep.pure σ
  | σ, .field _ :: rest => by unfold mKeys; exact mKeys_estep σ rest
  | σ, .index k m :: rest => by
    unfold mKeys
    exact EStep.bind (mE_estep sigs Γ _ σ k) (fun σ1 => mKeys_estep σ1 rest)

theorem mTarget_estep (σ : MSt) (x : String) (steps : List Step) (op : Option BinOp) :
    EStep σ (mTarget σ x steps op) := by
  unfold mTarget
  split
  · exact EStep.readM σ x
  · refine EStep.ite (EStep.pure σ) ?_
    split
    · exact EStep.ite (EStep.throw σ _) (EStep.ite (EStep.throw σ _) (EStep.pure σ))
    · exact EStep.pure σ

theorem mSetParts_estep (σ : MSt) (steps : List Step) (op : Option BinOp) (e : Expr) :
    EStep σ (mSetParts sigs Γ σ steps op e) := by
  unfold mSetParts
  exact EStep.calm_bind FlagMono.exprAt fun _ => EStep.calm_bind (FlagMono.exprFrom _) fun _ =>
    EStep.bind (mE_estep sigs Γ _ σ e) fun σ1 => EStep.calm_bind FlagMono.exprAt fun _ =>
    EStep.calm_bind (FlagMono.exprFrom _) fun _ => EStep.bind (mKeys_estep sigs Γ σ1 steps) fun σ2 =>
    EStep.calm_bind (FlagMono.exprFrom _) fun _ => EStep.pure σ2

theorem leavingIf_estep (id : Nat) (σ : MSt) (c : Bool) {m : MoveM MSt} (hm : EStep σ m) :
    EStep σ (leavingIf id σ c m) := by
  unfold leavingIf
  refine EStep.ite ?_ hm
  exact EStep.calm_bind (FlagMono.ownedIn σ) fun _ => EStep.bind hm fun σ' =>
    EStep.calm_bind (FlagMono.ownedIn σ') fun _ => EStep.calm_bind (FlagMono.recordNotYetMoved _ _) fun _ =>
    EStep.calm_bind (FlagMono.record _ _ _ _ _) fun _ => EStep.pure σ'

theorem leaving_estep (id : Nat) (σ : MSt) (e : Expr) {m : MoveM MSt} (hm : EStep σ m) :
    EStep σ (leaving id σ e m) := leavingIf_estep id σ _ hm

theorem mSet_estep (id : Nat) (σ : MSt) (x : String) (steps : List Step) (op : Option BinOp) (e : Expr) :
    EStep σ (mSet sigs Γ id σ x steps op e) := by
  unfold mSet
  refine EStep.calm_bind (FlagMono.unwalkedM σ x) fun _ => EStep.calm_bind (FlagMono.writableM σ x) fun _ =>
    EStep.bind (leavingIf_estep id σ _ (mSetParts_estep sigs Γ σ steps op e)) fun σ1 =>
    EStep.bind (mTarget_estep σ1 x steps op) fun σ2 =>
    EStep.calm_bind (FlagMono.noteOverMoved _ _ _ _) fun _ => EStep.calm_bind ?_ fun _ =>
    EStep.bind (EStep.ite (EStep.exitM σ2) (EStep.pure σ2)) fun σ3 => ?_
  · split
    · exact FlagMono.bind (FlagMono.ownedIn _) (fun _ => FlagMono.record _ _ _ _ _)
    · exact FlagMono.pure _
  · split
    · exact EStep.pure' (MSt.Le.give x σ3)
    · exact EStep.pure σ3

end

/-! ## Statements -/

/-- Names bound in front of `σ`, as `bindM` binds them: inside `σ`'s scope,
none read back. -/
theorem bindOne_run {Γ' : Ctx} {init lent writable byRef : Bool} {σ σ' : MSt} {x : String} {st st' : DSt}
    (h : (bindOne Γ' init lent writable byRef σ x).run st = .ok (σ', st')) :
    ∃ v, σ' = v :: σ ∧ v.name = x ∧ v.init = init ∧ v.live = true ∧ v.back = false ∧ v.lent = lent ∧
      v.writable = writable ∧ v.byRef = byRef ∧ v.walked = false ∧ st'.movedHeld = st.movedHeld := by
  simp only [bindOne, bind, StateT.bind, get, getThe, MonadStateOf.get, StateT.get, StateT.run,
    set, MonadStateOf.set, StateT.set, Pure.pure, StateT.pure] at h
  cases h
  exact ⟨_, rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl⟩

theorem bindM_inside (Γ' : Ctx) (init writable : Bool) : ∀ (xs : List String) (σ σ' : MSt) (st st' : DSt),
    (bindM Γ' σ xs init writable).run st = .ok (σ', st') → σ.Inside σ' ∧ st'.movedHeld = st.movedHeld
  | [], σ, σ', st, st', h => by
    simp only [bindM, List.foldlM] at h; cases h; exact ⟨MSt.Inside.refl σ, rfl⟩
  | x :: xs, σ, σ', st, st', h => by
    simp only [bindM, List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    obtain ⟨v, rfl, -, -, -, hb, -, -, -, -, f1⟩ := bindOne_run h1
    obtain ⟨i, f⟩ := bindM_inside Γ' init writable xs _ σ' st1 st' h2
    exact ⟨(MSt.Inside.cons (MSt.Inside.refl σ) hb).trans i, f.trans f1⟩

/-- A `for`'s names bound in front of what the walk freezes: inside its
scope, none read back. -/
theorem forBinds_inside (Γf : Ctx) : ∀ (ps : List (String × Bool)) (σ σ' : MSt) (st st' : DSt),
    (ps.foldlM (fun σ (x, r) => bindOne Γf true true false r σ x) σ).run st = .ok (σ', st') →
    σ.Inside σ' ∧ (st.movedHeld = true → st'.movedHeld = true)
  | [], σ, σ', st, st', h => by simp only [List.foldlM] at h; cases h; exact ⟨MSt.Inside.refl σ, id⟩
  | (x, r) :: ps, σ, σ', st, st', h => by
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    obtain ⟨v, rfl, -, -, -, hb, -, -, -, -, f⟩ := bindOne_run h1
    obtain ⟨i, f2⟩ := forBinds_inside Γf ps _ σ' st1 st' h2
    exact ⟨(MSt.Inside.cons (MSt.Inside.refl σ) hb).trans i, fun hm => f2 (f.trans hm)⟩

/-- A statement's walk from `σ`: what it leaves, and each `break` and
`continue`, is inside `σ`'s scope, and the flag is kept or set. -/
def SStep (σ : MSt) (m : MoveM MOut) : Prop :=
  ∀ st r st', m.run st = .ok (r, st') → σ.Inside r.1 ∧ (∀ b ∈ r.2.1, σ.Inside b) ∧ (∀ c ∈ r.2.2, σ.Inside c) ∧
    (st.movedHeld = true → st'.movedHeld = true)

theorem SStep.of_estep {σ : MSt} {m : MoveM MSt} (h : EStep σ m) :
    SStep σ (m >>= fun τ => Pure.pure (τ, [], [])) := fun st r st' hr => by
  obtain ⟨τ, st1, h1, h2⟩ := MoveM.bind_ok hr
  cases h2
  obtain ⟨l, f⟩ := h st τ _ h1
  exact ⟨MSt.Inside.of_le l, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), f⟩

/-- What the ways of a `join` agree on un-gives nothing of what they were
all given. -/
theorem joinFold_le (code : Lit.Code) {σ0 : MSt} : ∀ (L : List MSt) (acc : MSt) {st st' : DSt} {r : MSt},
    σ0.Le acc → (∀ τ ∈ L, σ0.Le τ) →
    (L.foldlM (fun (acc w : MSt) => match acc.join w with
      | some j => Pure.pure j
      | none => (throw code : MoveM MSt)) acc).run st = .ok (r, st') → σ0.Le r ∧ st' = st
  | [], acc, st, st', r, ha, _, h => by simp only [List.foldlM] at h; cases h; exact ⟨ha, rfl⟩
  | w :: L, acc, st, st', r, ha, hL, h => by
    simp only [List.foldlM] at h
    obtain ⟨j, st1, h1, h2⟩ := MoveM.bind_ok h
    have hj : σ0.Le j ∧ st1 = st := by
      split at h1
      · rename_i hjoin
        cases h1
        exact ⟨(MSt.join_eq hjoin).1 ▸ MSt.Le.initMeet ha (hL w List.mem_cons_self), rfl⟩
      · cases h1
    obtain ⟨hj, rfl⟩ := hj
    exact joinFold_le code L j hj (fun τ hτ => hL τ (List.mem_cons_of_mem _ hτ)) h2

theorem joinAll_le (code : Lit.Code) {σ0 : MSt} : ∀ (L : List MSt) (σ : MSt) {st st' : DSt} {r : MSt},
    σ0.Le σ → (∀ τ ∈ L, σ0.Le τ) → (joinAll code σ L).run st = .ok (r, st') → σ0.Le r ∧ st' = st
  | [], σ, st, st', r, hσ, _, h => by simp only [joinAll] at h; cases h; exact ⟨hσ, rfl⟩
  | τ :: L, σ, st, st', r, hσ, hL, h => by
    simp only [joinAll] at h
    exact joinFold_le code L τ (hL τ List.mem_cons_self) (fun w hw => hL w (List.mem_cons_of_mem _ hw)) h

theorem joinIf_le {σ0 t e : MSt} {tc ec : Bool} {st st' : DSt} {r : MSt} (ht : σ0.Le t) (he : σ0.Le e)
    (h : (joinIf t tc e ec).run st = .ok (r, st')) : σ0.Le r ∧ st' = st := by
  unfold joinIf at h
  split at h
  · cases h
  · cases h
    refine ⟨MSt.Le.initFrom ?_ (MSt.Le.initMeet ht he), rfl⟩
    split
    · exact he
    · exact ht

theorem initMeetFold_le {σ0 : MSt} : ∀ (ms : List MSt) (m : MSt), σ0.Le m → (∀ w ∈ ms, σ0.Le w) →
    σ0.Le (ms.foldl MSt.initMeet m)
  | [], _, hm, _ => hm
  | w :: ms, _, hm, hw => initMeetFold_le ms _ (MSt.Le.initMeet hm (hw w List.mem_cons_self))
    (fun u hu => hw u (List.mem_cons_of_mem _ hu))

theorem joinMatch_le {σ0 σ : MSt} {arms : List (MSt × Bool)} {st st' : DSt} {r : MSt} (hσ : σ0.Le σ)
    (ha : ∀ a ∈ arms, σ0.Le a.1) (h : (joinMatch σ arms).run st = .ok (r, st')) : σ0.Le r ∧ st' = st := by
  simp only [joinMatch] at h
  have hmem : ∀ a ∈ (arms.filter (·.2)).map (·.1), σ0.Le a := by
    intro a hmem
    simp only [List.mem_map, List.mem_filter] at hmem
    obtain ⟨p, ⟨hp, -⟩, rfl⟩ := hmem
    exact ha p hp
  generalize (arms.filter (·.2)).map (·.1) = merging at h hmem
  cases merging with
  | cons m ms =>
    dsimp only at h
    rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
    · cases h
    · cases h
      refine ⟨?_, rfl⟩
      exact initMeetFold_le ms m (hmem m List.mem_cons_self) (fun w hw => hmem w (List.mem_cons_of_mem _ hw))
  | nil =>
    dsimp only at h
    cases hh : arms.head? with
    | none => simp only [hh] at h; cases h; exact ⟨hσ, rfl⟩
    | some p =>
      obtain ⟨f, sf⟩ := p
      cases hl : arms.getLast? with
      | none => simp only [hh, hl] at h; cases h; exact ⟨hσ, rfl⟩
      | some q =>
        obtain ⟨l, sl⟩ := q
        simp only [hh, hl] at h; cases h
        exact ⟨MSt.Le.initFrom (ha _ (List.mem_of_mem_head? hh)) (ha _ (List.mem_of_getLast? hl)), rfl⟩

theorem loopEnd_le {σ σc : MSt} {natural : Bool} {body : List Stmt} {r : MOut} {st st' : DSt} {ρ : MSt}
    (hc : σ.Le σc) (hb : ∀ b ∈ r.2.1, σc.Le (σc.back b))
    (h : (loopEnd σ σc natural body r).run st = .ok (ρ, st')) : σ.Le ρ ∧ st' = st := by
  simp only [loopEnd] at h
  rcases MoveM.ite_run h with ⟨-, h⟩ | ⟨-, h⟩
  · cases h
  · have hL : ∀ τ ∈ (if natural = true then [σc] else []) ++ r.2.1.map σc.back, σ.Le τ := by
      intro τ hτ
      simp only [List.mem_append, List.mem_map] at hτ
      rcases hτ with hτ | ⟨b, hb', rfl⟩
      · split at hτ
        · simp at hτ; subst hτ; exact hc
        · simp at hτ
      · exact hc.trans (hb b hb')
    exact joinAll_le "O014" (σ0 := σ) _ σc hc hL h

theorem mBranch_sstep {Γb : Ctx} {σc : MSt} {binds : List String} {id : Nat} {kind : String} {arm : Option Nat}
    {walk : MSt → MoveM MOut} (hw : ∀ σb, SStep σb (walk σb)) : SStep σc (mBranch Γb σc binds id kind arm walk) := by
  intro st r st' h
  unfold mBranch at h
  replace h := MoveM.bind_ok h
  obtain ⟨σb, st1, h1, h⟩ := h
  obtain ⟨ib, f1⟩ := bindM_inside Γb true false binds σc σb st st1 h1
  replace h := MoveM.bind_ok h
  obtain ⟨rb, st2, h2, h⟩ := h
  obtain ⟨i2, ib2, ic2, f2⟩ := hw σb st1 rb st2 h2
  replace h := MoveM.bind_ok h
  obtain ⟨_, st3, h3, h⟩ := h
  have f3 := FlagMono.ownedIn _ st2 _ st3 h3
  replace h := MoveM.bind_ok h
  obtain ⟨_, st4, h4, h⟩ := h
  have f4 := FlagMono.record _ _ _ _ _ st3 _ st4 h4
  cases h
  refine ⟨ib.trans i2, fun b hb => ib.trans (ib2 b hb), fun c hc => ib.trans (ic2 c hc), fun hm => ?_⟩
  exact f4 (f3 (f2 (f1.trans hm)))

section
variable (sigs : List (String × Sig)) (env : InferEnv)

mutual

theorem mStmt_sstep : ∀ (n : Nat) (Γ : Ctx) (σ : MSt) (base : Nat) (s : Stmt), SStep σ (mStmt sigs env n Γ σ base s)
  | 0, _, σ, _, _ => fun st r st' h => by
    simp only [mStmt] at h; cases h
    exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fun hm => hm⟩
  | n + 1, Γ, σ, base, s => fun st r st' h => by
    unfold mStmt at h
    replace h := MoveM.bind_ok h
    obtain ⟨id, st0, hid, h⟩ := h
    have fin : ∀ {st1 : DSt}, (st0.movedHeld = true → st1.movedHeld = true) →
      (st.movedHeld = true → st1.movedHeld = true) := fun f hm => f (FlagMono.stmtId st id st0 hid hm)
    have nil3 : ∀ {τ : MSt} {b c : MSt}, b ∈ (τ, ([] : List MSt), ([] : List MSt)).2.1 → σ.Inside b :=
      fun hb => nomatch hb
    cases s with
    | let_ p hint e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨l1, f1⟩ := leaving_estep id σ e (mE_estep sigs Γ _ σ e) st0 σ1 st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨σ2, st2, h2, h⟩ := h
      obtain ⟨i2, f2⟩ := bindM_inside _ _ _ _ σ1 σ2 st1 st2 h2
      cases h
      exact ⟨(MSt.Inside.of_le l1).trans i2, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc),
        fin (fun hm => f2.trans (f1 hm))⟩
    | var_ p hint v =>
      dsimp only at h
      cases v with
      | some e =>
        replace h := MoveM.bind_ok h
        obtain ⟨σ1, st1, h1, h⟩ := h
        obtain ⟨l1, f1⟩ := leaving_estep id σ e (mE_estep sigs Γ _ σ e) st0 σ1 st1 h1
        replace h := MoveM.bind_ok h
        obtain ⟨σ2, st2, h2, h⟩ := h
        obtain ⟨i2, f2⟩ := bindM_inside _ _ _ _ σ1 σ2 st1 st2 h2
        cases h
        exact ⟨(MSt.Inside.of_le l1).trans i2, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc),
          fin (fun hm => f2.trans (f1 hm))⟩
      | none =>
        cases hint with
        | some hh =>
          replace h := MoveM.bind_ok h
          obtain ⟨σ1, st1, h1, h⟩ := h
          obtain ⟨i1, f1⟩ := bindM_inside _ _ _ _ σ σ1 st0 st1 h1
          replace h := MoveM.bind_ok h
          obtain ⟨_, st2, h2, h⟩ := h
          have e2 := MoveM.modify_run h2
          subst e2
          cases h
          exact ⟨i1, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin (fun hm => f1.trans hm)⟩
        | none =>
          cases h
          exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin (fun hm => hm)⟩
    | set_ x steps op e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨l1, f1⟩ := mSet_estep sigs Γ id σ x steps op e st0 σ1 st1 h1
      cases h
      exact ⟨MSt.Inside.of_le l1, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin f1⟩
    | fun_ f =>
      dsimp only at h
      cases h
      exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin (fun hm => hm)⟩
    | ret e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨l1, f1⟩ := EStep.exitM σ st0 σ1 st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨σ2, st2, h2, h⟩ := h
      have r2 : σ1.Le σ2 ∧ (st1.movedHeld = true → st2.movedHeld = true) := by
        cases e with
        | some e => exact leaving_estep id σ1 e (mE_estep sigs Γ _ σ1 e) st1 σ2 st2 h2
        | none => cases h2; exact ⟨MSt.Le.refl σ1, fun hm => hm⟩
      replace h := MoveM.bind_ok h
      obtain ⟨_, st3, h3, h⟩ := h
      have f3 := FlagMono.ownedIn _ st2 _ st3 h3
      replace h := MoveM.bind_ok h
      obtain ⟨_, st4, h4, h⟩ := h
      have f4 := FlagMono.record _ _ _ _ _ st3 _ st4 h4
      cases h
      exact ⟨MSt.Inside.of_le (l1.trans r2.1), fun _ hb => (nomatch hb), fun _ hc => (nomatch hc),
        fin (fun hm => f4 (f3 (r2.2 (f1 hm))))⟩
    | if_ c tB eB t e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      obtain ⟨lc, f1⟩ := leaving_estep id σ c (mE_estep sigs Γ _ σ c) st0 σc st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨rt, st2, h2, h⟩ := h
      obtain ⟨it, ibt, ict, f2⟩ := mBranch_sstep (fun σb => mBlock_sstep n _ σb base t) st1 rt st2 h2
      replace h := MoveM.bind_ok h
      obtain ⟨re, st3, h3, h⟩ := h
      have r3 : σc.Inside re.1 ∧ (∀ b ∈ re.2.1, σc.Inside b) ∧ (∀ c ∈ re.2.2, σc.Inside c) ∧
          (st2.movedHeld = true → st3.movedHeld = true) := by
        cases e with
        | some eb => exact mBranch_sstep (fun σb => mBlock_sstep n _ σb base eb) st2 re st3 h3
        | none =>
          cases h3
          exact ⟨MSt.Inside.refl σc, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fun hm => hm⟩
      obtain ⟨ie, ibe, ice, f3⟩ := r3
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st4, h4, h⟩ := h
      obtain ⟨l4, e4⟩ := joinIf_le it.back ie.back h4
      subst e4
      cases h
      refine ⟨MSt.Inside.of_le (lc.trans l4), fun b hb => ?_, fun c hc => ?_, fin (fun hm => f3 (f2 (f1 hm)))⟩
      · rcases List.mem_append.mp hb with hb | hb
        · exact (MSt.Inside.of_le lc).trans (ibt b hb)
        · exact (MSt.Inside.of_le lc).trans (ibe b hb)
      · rcases List.mem_append.mp hc with hc | hc
        · exact (MSt.Inside.of_le lc).trans (ict c hc)
        · exact (MSt.Inside.of_le lc).trans (ice c hc)
    | loop c body =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      have r1 : σ.Le σc ∧ (st0.movedHeld = true → st1.movedHeld = true) := by
        cases c with
        | some c => exact leaving_estep id σ c (mE_estep sigs Γ _ σ c) st0 σc st1 h1
        | none => cases h1; exact ⟨MSt.Le.refl σ, fun hm => hm⟩
      obtain ⟨lc, f1⟩ := r1
      replace h := MoveM.bind_ok h
      obtain ⟨r, st2, h2, h⟩ := h
      obtain ⟨ir, ibr, icr, f2⟩ := mBlock_sstep n _ σc σc.length body st1 r st2 h2
      replace h := MoveM.bind_ok h
      obtain ⟨_, st3, h3, h⟩ := h
      have f3 := FlagMono.loopHeld σc body r st2 _ st3 h3
      replace h := MoveM.bind_ok h
      obtain ⟨_, st4, h4, h⟩ := h
      have f4 := FlagMono.ownedIn _ st3 _ st4 h4
      replace h := MoveM.bind_ok h
      obtain ⟨_, st5, h5, h⟩ := h
      have f5 := FlagMono.record _ _ _ _ _ st4 _ st5 h5
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st6, h6, h⟩ := h
      obtain ⟨l6, e6⟩ := loopEnd_le lc (fun b hb => (ibr b hb).back) h6
      subst e6
      cases h
      exact ⟨MSt.Inside.of_le l6, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc),
        fin (fun hm => f5 (f4 (f3 (f2 (f1 hm)))))⟩
    | brk =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st1, h1, h⟩ := h
      have f1 := FlagMono.ownedIn _ st0 _ st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨_, st2, h2, h⟩ := h
      have f2 := FlagMono.record _ _ _ _ _ st1 _ st2 h2
      cases h
      exact ⟨MSt.Inside.refl σ, fun b hb => (by simp only [List.mem_singleton] at hb; exact hb ▸ MSt.Inside.refl σ),
        fun _ hc => (nomatch hc), fin (fun hm => f2 (f1 hm))⟩
    | cont =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨_, st1, h1, h⟩ := h
      have f1 := FlagMono.ownedIn _ st0 _ st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨_, st2, h2, h⟩ := h
      have f2 := FlagMono.record _ _ _ _ _ st1 _ st2 h2
      cases h
      exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb),
        fun c hc => (by simp only [List.mem_singleton] at hc; exact hc ▸ MSt.Inside.refl σ), fin (fun hm => f2 (f1 hm))⟩
    | debuglog e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨l1, f1⟩ := leaving_estep id σ e (mE_estep sigs Γ _ σ e) st0 σ1 st1 h1
      cases h
      exact ⟨MSt.Inside.of_le l1, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin f1⟩
    | call e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨l1, f1⟩ := leaving_estep id σ e (mE_estep sigs Γ _ σ e) st0 σ1 st1 h1
      cases h
      exact ⟨MSt.Inside.of_le l1, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin f1⟩
    | const_ x hint e =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σ1, st1, h1, h⟩ := h
      obtain ⟨l1, f1⟩ := leaving_estep id σ e (mE_estep sigs _ _ σ e) st0 σ1 st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨σ2, st2, h2, h⟩ := h
      obtain ⟨i2, f2⟩ := bindM_inside _ _ _ _ σ1 σ2 st1 st2 h2
      cases h
      exact ⟨(MSt.Inside.of_le l1).trans i2, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc),
        fin (fun hm => f2.trans (f1 hm))⟩
    | type_ x hh =>
      dsimp only at h
      cases h
      exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fin (fun hm => hm)⟩
    | match_ e cases dflt =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      obtain ⟨lc, f1⟩ := leaving_estep id σ e (mE_estep sigs Γ _ σ e) st0 σc st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨r, st2, h2, h⟩ := h
      obtain ⟨ir, ibr, icr, f2⟩ := mCases_sstep n Γ _ σc base id 0 cases st1 r st2 h2
      replace h := MoveM.bind_ok h
      obtain ⟨d, st3, h3, h⟩ := h
      have r3 : (∀ a ∈ d.1, σc.Le a.1) ∧ (∀ b ∈ d.2.1, σc.Inside b) ∧ (∀ c ∈ d.2.2, σc.Inside c) ∧
          (st2.movedHeld = true → st3.movedHeld = true) := by
        cases dflt with
        | some db =>
          replace h3 := MoveM.bind_ok h3
          obtain ⟨rd, st3', h3', h3⟩ := h3
          obtain ⟨id', ibd, icd, fd⟩ := mBranch_sstep (fun σb => mBlock_sstep n Γ σb base db) st2 rd st3' h3'
          cases h3
          refine ⟨fun a ha => ?_, ibd, icd, fd⟩
          simp only [List.mem_singleton] at ha
          subst ha
          exact id'.back
        | none =>
          cases h3
          exact ⟨fun _ ha => (nomatch ha), fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), fun hm => hm⟩
      obtain ⟨ld, ibd, icd, f3⟩ := r3
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st4, h4, h⟩ := h
      obtain ⟨l4, e4⟩ := joinMatch_le (MSt.Le.refl σc) (fun a ha => by
        rcases List.mem_append.mp ha with ha | ha
        · exact ir a ha
        · exact ld a ha) h4
      subst e4
      cases h
      refine ⟨MSt.Inside.of_le (lc.trans l4), fun b hb => ?_, fun c hc => ?_, fin (fun hm => f3 (f2 (f1 hm)))⟩
      · rcases List.mem_append.mp hb with hb | hb
        · exact (MSt.Inside.of_le lc).trans (ibr b hb)
        · exact (MSt.Inside.of_le lc).trans (ibd b hb)
      · rcases List.mem_append.mp hc with hc | hc
        · exact (MSt.Inside.of_le lc).trans (icr c hc)
        · exact (MSt.Inside.of_le lc).trans (icd c hc)
    | for_ names coll body =>
      dsimp only at h
      replace h := MoveM.bind_ok h
      obtain ⟨σc, st1, h1, h⟩ := h
      obtain ⟨lc, f1⟩ := leaving_estep id σ coll (mE_estep sigs Γ _ σ coll) st0 σc st1 h1
      replace h := MoveM.bind_ok h
      obtain ⟨σb, st2, h2, h⟩ := h
      have hw : σc.Le (match placeOf coll with
          | some (x, _) => σc.walk x
          | none => σc) := by
        split
        · exact MSt.Le.walk _ σc
        · exact MSt.Le.refl σc
      obtain ⟨ib, f2⟩ := forBinds_inside _ _ _ _ _ _ h2
      replace h := MoveM.bind_ok h
      obtain ⟨r, st3, h3, h⟩ := h
      obtain ⟨ir, ibr, icr, f3⟩ := mBlock_sstep n _ σb σc.length body st2 r st3 h3
      replace h := MoveM.bind_ok h
      obtain ⟨_, st4, h4, h⟩ := h
      have f4 := FlagMono.loopHeld σc body r st3 _ st4 h4
      replace h := MoveM.bind_ok h
      obtain ⟨_, st5, h5, h⟩ := h
      have f5 := FlagMono.ownedIn _ st4 _ st5 h5
      replace h := MoveM.bind_ok h
      obtain ⟨_, st6, h6, h⟩ := h
      have f6 := FlagMono.record _ _ _ _ _ st5 _ st6 h6
      replace h := MoveM.bind_ok h
      obtain ⟨σ', st7, h7, h⟩ := h
      have iw : σc.Inside σb := (MSt.Inside.of_le hw).trans ib
      obtain ⟨l7, e7⟩ := loopEnd_le (MSt.Le.refl σc) (fun b hb => (iw.trans (ibr b hb)).back) h7
      subst e7
      cases h
      exact ⟨MSt.Inside.of_le (lc.trans l7), fun _ hb => (nomatch hb), fun _ hc => (nomatch hc),
        fin (fun hm => f6 (f5 (f4 (f3 (f2 (f1 hm))))))⟩

theorem mCases_sstep : ∀ (n : Nat) (Γ : Ctx) (vs : List (String × Option Ty)) (σ : MSt) (base id i : Nat)
    (cs : List Case) (st : DSt) (r : List (MSt × Bool) × List MSt × List MSt) (st' : DSt),
    (mCases sigs env n Γ vs σ base id i cs).run st = .ok (r, st') →
    (∀ a ∈ r.1, σ.Le a.1) ∧ (∀ b ∈ r.2.1, σ.Inside b) ∧ (∀ c ∈ r.2.2, σ.Inside c) ∧
      (st.movedHeld = true → st'.movedHeld = true)
  | 0, _, _, σ, _, _, _, _, st, r, st', h => by
    simp only [mCases] at h; cases h
    exact ⟨fun _ ha => (nomatch ha), fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), id⟩
  | _ + 1, _, _, σ, _, _, _, [], st, r, st', h => by
    simp only [mCases] at h; cases h
    exact ⟨fun _ ha => (nomatch ha), fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), id⟩
  | n + 1, Γ, vs, σ, base, id, i, .atom _ b :: cs, st, r, st', h => by
    simp only [mCases] at h
    replace h := MoveM.bind_ok h
    obtain ⟨rb, st1, h1, h⟩ := h
    obtain ⟨ib, ibb, icb, f1⟩ := mBranch_sstep (fun σb => mBlock_sstep n Γ σb base b) st rb st1 h1
    replace h := MoveM.bind_ok h
    obtain ⟨rc, st2, h2, h⟩ := h
    obtain ⟨lc, ibc, icc, f2⟩ := mCases_sstep n Γ vs σ base id (i + 1) cs st1 rc st2 h2
    cases h
    refine ⟨fun a ha => ?_, fun b' hb => ?_, fun c hc => ?_, fun hm => f2 (f1 hm)⟩
    · rcases List.mem_cons.mp ha with rfl | ha
      · exact ib.back
      · exact lc a ha
    · rcases List.mem_append.mp hb with hb | hb
      · exact ibb b' hb
      · exact ibc b' hb
    · rcases List.mem_append.mp hc with hc | hc
      · exact icb c hc
      · exact icc c hc
  | n + 1, Γ, vs, σ, base, id, i, .term a x b :: cs, st, r, st', h => by
    simp only [mCases] at h
    replace h := MoveM.bind_ok h
    obtain ⟨rb, st1, h1, h⟩ := h
    obtain ⟨ib, ibb, icb, f1⟩ := mBranch_sstep (fun σb => mBlock_sstep n _ σb base b) st rb st1 h1
    replace h := MoveM.bind_ok h
    obtain ⟨rc, st2, h2, h⟩ := h
    obtain ⟨lc, ibc, icc, f2⟩ := mCases_sstep n Γ vs σ base id (i + 1) cs st1 rc st2 h2
    cases h
    refine ⟨fun a ha => ?_, fun b' hb => ?_, fun c hc => ?_, fun hm => f2 (f1 hm)⟩
    · rcases List.mem_cons.mp ha with rfl | ha
      · exact ib.back
      · exact lc a ha
    · rcases List.mem_append.mp hb with hb | hb
      · exact ibb b' hb
      · exact ibc b' hb
    · rcases List.mem_append.mp hc with hc | hc
      · exact icb c hc
      · exact icc c hc

theorem mBlock_sstep : ∀ (n : Nat) (Γ : Ctx) (σ : MSt) (base : Nat) (ss : List Stmt),
    SStep σ (mBlock sigs env n Γ σ base ss)
  | 0, _, σ, _, _ => fun st r st' h => by
    simp only [mBlock] at h; cases h
    exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), id⟩
  | _ + 1, _, σ, _, [] => fun st r st' h => by
    simp only [mBlock] at h; cases h
    exact ⟨MSt.Inside.refl σ, fun _ hb => (nomatch hb), fun _ hc => (nomatch hc), id⟩
  | n + 1, Γ, σ, base, s :: ss => fun st r st' h => by
    simp only [mBlock] at h
    replace h := MoveM.bind_ok h
    obtain ⟨r1, st1, h1, h⟩ := h
    obtain ⟨i1, ib1, ic1, f1⟩ := mStmt_sstep n Γ σ base s st r1 st1 h1
    replace h := MoveM.bind_ok h
    obtain ⟨r2, st2, h2, h⟩ := h
    obtain ⟨i2, ib2, ic2, f2⟩ := mBlock_sstep n _ r1.1 base ss st1 r2 st2 h2
    cases h
    refine ⟨i1.trans i2, fun b hb => ?_, fun c hc => ?_, fun hm => f2 (f1 hm)⟩
    · rcases List.mem_append.mp hb with hb | hb
      · exact ib1 b hb
      · exact i1.trans (ib2 b hb)
    · rcases List.mem_append.mp hc with hc | hc
      · exact ic1 c hc
      · exact i1.trans (ic2 c hc)

end

end

/-! ## What a call's arguments leave live -/

/-- The variable of each `mut` or `out` argument is live, where it names
one. -/
def RootsLive (σ : MSt) : List (Option Mode × Expr) → List Mode → Prop
  | (_, e) :: args, m :: ms =>
    ((m == .mut || m == .out) = true → ∀ x st, placeOf e = some (x, st) → ∀ v, σ.find x = some v → v.live = true) ∧
      RootsLive σ args ms
  | _, _ => True

theorem heldMoved_false : ∀ (σ : MSt) (args : List (Option Mode × Expr)) (ms : List Mode),
    heldMoved σ args ms = false → RootsLive σ args ms
  | σ, (_, e) :: args, m :: ms, h => by
    simp only [heldMoved, Bool.or_eq_false_iff, Bool.and_eq_false_iff] at h
    refine ⟨fun hm x st hp v hv => ?_, heldMoved_false σ args ms h.2⟩
    rcases h.1 with h1 | h1
    · simp [h1.1, h1.2] at hm
    · rw [hp] at h1
      simpa [hv] using h1
  | _, [], _, _ => trivial
  | _, _ :: _, [], _ => trivial

/-- A call noted nowhere: where the flag ends clear, it was clear before the
call and the call's arguments left every `mut` and `out` root live. -/
theorem noteHeldMoved_run {σ : MSt} {args : List (Option Mode × Expr)} {ms : List Mode} {st st' : DSt} {u : Unit}
    (h : (noteHeldMoved σ args ms).run st = .ok (u, st')) (hf : st'.movedHeld = false) :
    heldMoved σ args ms = false ∧ st.movedHeld = false := by
  unfold noteHeldMoved at h
  split at h
  · simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
    cases h; simp at hf
  · cases h; exact ⟨by simpa using ‹¬ heldMoved σ args ms = true›, hf⟩

end Datalove.Fun
