import Datalove.Proofs.DropShrink

/-!
# A point, once scheduled, only gains what is perhaps held

The second step of the drop schedule's proof (`botdocs/plan-drop-proof.md`):
how the schedule a point was recorded with relates to the schedule the walk
of the function ends with, which is the one lowering carries out. Later
steps add points before it, and one step changes points already recorded:
a binding found perhaps held (`holdFrom`) joins every way out of the
function scheduled from where it was made on. So a point keeps what it was
recorded with, gains only bindings that keep a tracking byte, and, if it is
a way out of the function, gains every binding found perhaps held after it
was recorded and made before its statement.

`SchedExt st st'` says this of the schedule as it is at `st` and later at `st'`,
with what else only grows: the statement and binding counters, what is
held and tracked, and where each binding was made. Each step of the walk
keeps it, and it composes.
-/

namespace Datalove.Fun

/-- A scheduled point: its kind, its statement, the arm for a `match`'s, and
what it drops, each with the expression that moves it where a return may
come before the move. -/
abbrev Entry := String × Nat × Option Nat × List (Nat × Option Nat)

/-- The bindings a point drops. -/
def Entry.uids (e : Entry) : List Nat := e.2.2.2.map (·.1)

/-- A way out of the function, which a binding found perhaps held joins. -/
def exitKind (k : String) : Bool :=
  k == "before_return" || k == "before_try_return" || k == "before_set_target_early_return"

/-- Where binding `u` was made: the index the next statement had. -/
def DSt.made (st : DSt) (u : Nat) : Nat := (st.madeAt.lookup u).getD 0

/-- The schedule's bookkeeping holds together: every binding numbered so
far has a place it was made at, and only those; what is held is among them,
and is tracked. -/
structure DSt.WF (st : DSt) : Prop where
  fresh : ∀ u m, (u, m) ∈ st.madeAt → u < st.uid
  made : ∀ u, u < st.uid → ∃ m, st.madeAt.lookup u = some m
  heldLt : ∀ u, u ∈ st.held → u < st.uid
  heldTracked : ∀ u, u ∈ st.held → u ∈ st.tracked

/-- Point `e`, recorded by the time of `st`, become `e'` by `st'`. -/
def EntryExt (st st' : DSt) (e' e : Entry) : Prop :=
  e'.1 = e.1 ∧ e'.2.1 = e.2.1 ∧ e'.2.2.1 = e.2.2.1 ∧
  (exitKind e.1 = false → e' = e) ∧
  (∀ u, u ∈ e.uids → u ∈ e'.uids) ∧
  (∀ u, u ∈ e'.uids → u ∈ e.uids ∨ u ∈ st'.tracked) ∧
  (exitKind e.1 = true → ∀ u, u ∈ st'.held → u ∉ st.held → st'.made u ≤ e.2.1 → u ∈ e'.uids)

/-- Two lists of points, pairwise. -/
inductive Pairwise (R : Entry → Entry → Prop) : List Entry → List Entry → Prop where
  | nil : Pairwise R [] []
  | cons {a b : Entry} {as bs : List Entry} : R a b → Pairwise R as bs → Pairwise R (a :: as) (b :: bs)

theorem Pairwise.imp {R S : Entry → Entry → Prop} (h : ∀ a b, R a b → S a b) :
    ∀ {as bs : List Entry}, Pairwise R as bs → Pairwise S as bs
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons r t => .cons (h _ _ r) (Pairwise.imp h t)

theorem Pairwise.length {R : Entry → Entry → Prop} : ∀ {as bs : List Entry}, Pairwise R as bs →
    as.length = bs.length
  | [], [], .nil => rfl
  | _ :: _, _ :: _, .cons _ t => by simp [Pairwise.length t]

/-- `mid` points paired with `bs`, after `pre`: the second half of a list
paired with the whole of another. -/
theorem Pairwise.split_append {R : Entry → Entry → Prop} :
    ∀ (pre1 : List Entry) {mid bs : List Entry}, Pairwise R mid (pre1 ++ bs) →
      ∃ pre' mid', mid = pre' ++ mid' ∧ Pairwise R mid' bs
  | [], mid, _, h => ⟨[], mid, rfl, h⟩
  | _ :: pre1, _ :: mid, _, .cons _ t => by
    obtain ⟨pre', mid', e, h⟩ := Pairwise.split_append pre1 t
    exact ⟨_ :: pre', mid', by rw [e]; rfl, h⟩

/-- What only grows from `st` to `st'`, and what becomes of each point
`st` had scheduled. -/
structure SchedExt (st st' : DSt) : Prop where
  next : st.next ≤ st'.next
  uid : st.uid ≤ st'.uid
  held : ∀ u, u ∈ st.held → u ∈ st'.held
  tracked : ∀ u, u ∈ st.tracked → u ∈ st'.tracked
  made : ∀ u m, st.madeAt.lookup u = some m → st'.madeAt.lookup u = some m
  out : ∃ pre mid, st'.out = pre ++ mid ∧ Pairwise (EntryExt st st') mid st.out
  wf : st'.WF

theorem EntryExt.refl (st : DSt) (e : Entry) : EntryExt st st e e :=
  ⟨rfl, rfl, rfl, fun _ => rfl, fun _ h => h, fun _ h => .inl h, fun _ _ h1 h2 => absurd h1 h2⟩

theorem Pairwise.refl (st : DSt) : ∀ (es : List Entry), Pairwise (EntryExt st st) es es
  | [] => .nil
  | e :: es => .cons (EntryExt.refl st e) (Pairwise.refl st es)

theorem SchedExt.refl {st : DSt} (w : st.WF) : SchedExt st st :=
  ⟨Nat.le_refl _, Nat.le_refl _, fun _ h => h, fun _ h => h, fun _ _ h => h,
    ⟨[], st.out, rfl, Pairwise.refl st st.out⟩, w⟩

theorem DSt.made_eq {st st' : DSt} {u m : Nat} (h : st.madeAt.lookup u = some m)
    (e : st'.madeAt.lookup u = some m) : st'.made u = st.made u := by
  simp [DSt.made, h, e]

theorem EntryExt.trans {st st' st'' : DSt} (x1 : SchedExt st st') (x2 : SchedExt st' st'') {e e' e'' : Entry}
    (h1 : EntryExt st st' e' e) (h2 : EntryExt st' st'' e'' e') : EntryExt st st'' e'' e := by
  obtain ⟨k1, s1, a1, f1, sub1, ext1, p1⟩ := h1
  obtain ⟨k2, s2, a2, f2, sub2, ext2, p2⟩ := h2
  refine ⟨k2.trans k1, s2.trans s1, a2.trans a1, fun hk => ?_, fun u hu => sub2 u (sub1 u hu),
    fun u hu => ?_, fun hk u hu hn hm => ?_⟩
  · rw [f2 (k1 ▸ hk), f1 hk]
  · rcases ext2 u hu with h | h
    · rcases ext1 u h with h' | h'
      · exact .inl h'
      · exact .inr (x2.tracked u h')
    · exact .inr h
  · by_cases h' : u ∈ st'.held
    · obtain ⟨m, hm'⟩ := x1.wf.made u (x1.wf.heldLt u h')
      have e2 := x2.made u m hm'
      have hu' : st'.made u ≤ e.2.1 := by
        rw [← DSt.made_eq hm' e2]; exact hm
      exact sub2 u (p1 hk u h' hn hu')
    · exact p2 (k1 ▸ hk) u hu h' (s1 ▸ hm)

theorem Pairwise.trans {st st' st'' : DSt} (x1 : SchedExt st st') (x2 : SchedExt st' st'') :
    ∀ {as bs cs : List Entry}, Pairwise (EntryExt st' st'') as bs → Pairwise (EntryExt st st') bs cs →
      Pairwise (EntryExt st st'') as cs
  | [], [], [], .nil, .nil => .nil
  | _ :: _, _ :: _, _ :: _, .cons h2 t2, .cons h1 t1 =>
    .cons (EntryExt.trans x1 x2 h1 h2) (Pairwise.trans x1 x2 t2 t1)

theorem SchedExt.trans {st st' st'' : DSt} (x1 : SchedExt st st') (x2 : SchedExt st' st'') : SchedExt st st'' := by
  obtain ⟨pre1, mid1, e1, h1⟩ := x1.out
  obtain ⟨pre2, mid2, e2, h2⟩ := x2.out
  rw [e1] at h2
  obtain ⟨pre', mid', e', h'⟩ := Pairwise.split_append pre1 h2
  refine ⟨Nat.le_trans x1.next x2.next, Nat.le_trans x1.uid x2.uid, fun u h => x2.held u (x1.held u h),
    fun u h => x2.tracked u (x1.tracked u h), fun u m h => x2.made u m (x1.made u m h),
    ⟨pre2 ++ pre', mid', ?_, ?_⟩, x2.wf⟩
  · rw [e2, e', List.append_assoc]
  · exact Pairwise.trans x1 x2 h' h1


/-! ## Each step keeps it -/

/-- A step of the walk, from a schedule that holds together, keeps what
`SchedExt` says. -/
def SchedGrows {α} (m : MoveM α) : Prop := ∀ st a st', st.WF → m.run st = .ok (a, st') → SchedExt st st'

theorem SchedGrows.pure {α} (a : α) : SchedGrows (Pure.pure a : MoveM α) := fun _ _ _ w h => by
  cases h; exact SchedExt.refl w

theorem SchedGrows.throw {α} (c : Lit.Code) : SchedGrows (throw c : MoveM α) := fun _ _ _ _ h => by cases h

theorem SchedGrows.bind {α β} {x : MoveM α} {f : α → MoveM β} (hx : SchedGrows x) (hf : ∀ a, SchedGrows (f a)) :
    SchedGrows (x >>= f) := fun st b st' w h => by
  obtain ⟨a, st1, h1, h2⟩ := MoveM.bind_ok h
  have x1 := hx st a st1 w h1
  exact x1.trans (hf a st1 b st' x1.wf h2)

/-- A step that changes none of what `SchedExt` follows but perhaps the
statement counter, upwards. -/
theorem SchedExt.of_same {st st' : DSt} (w : st.WF) (n : st.next ≤ st'.next) (u : st'.uid = st.uid)
    (h : st'.held = st.held) (t : st'.tracked = st.tracked) (m : st'.madeAt = st.madeAt)
    (o : st'.out = st.out) : SchedExt st st' := by
  refine ⟨n, u ▸ Nat.le_refl _, fun _ x => h ▸ x, fun _ x => t ▸ x, fun _ _ x => m ▸ x,
    ⟨[], st.out, by rw [o]; rfl, ?_⟩, ⟨?_, ?_, ?_, ?_⟩⟩
  · refine Pairwise.imp (fun a b r => ?_) (Pairwise.refl st st.out)
    obtain ⟨k, s', a', f, sub, ext, p⟩ := r
    exact ⟨k, s', a', f, sub, fun x hx => (ext x hx).imp id (fun y => t ▸ y),
      fun hk x hx hn _ => absurd (h ▸ hx) hn⟩
  · intro x y hm; rw [u]; exact w.fresh x y (m ▸ hm)
  · intro x hx; rw [m]; exact w.made x (u ▸ hx)
  · intro x hx; rw [u]; exact w.heldLt x (h ▸ hx)
  · intro x hx; rw [t]; exact w.heldTracked x (h ▸ hx)

theorem SchedGrows.keepsSched {α} {m : MoveM α} (h : KeepsSched m) : SchedGrows m := fun st a st' w e => by
  have hs := h st a st' e
  simp only [DSt.sched, Prod.mk.injEq] at hs
  obtain ⟨n, u, he, _, o, md, t⟩ := hs
  exact SchedExt.of_same w (n ▸ Nat.le_refl _) u he t md o

theorem SchedGrows.shr {σ : MSt} {m : MoveM MSt} (h : Shr σ m) : SchedGrows m := fun st τ st' w e =>
  SchedGrows.keepsSched (fun st a st' e => (h st a st' e).2) st τ st' w e

theorem SchedGrows.stmtId : SchedGrows Fun.stmtId := fun st a st' w h => by
  simp only [Fun.stmtId, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
  cases h
  exact SchedExt.of_same w (Nat.le_succ _) rfl rfl rfl rfl rfl

theorem List.lookup_mem' : ∀ {l : List (Nat × Nat)} {u m : Nat}, l.lookup u = some m → (u, m) ∈ l
  | [], _, _, h => by simp at h
  | (a, b) :: l, u, m, h => by
    simp only [List.lookup] at h
    split at h
    · next e => cases h; have : u = a := by simpa using e
                subst this; exact .head _
    · exact .tail _ (List.lookup_mem' h)

theorem SchedGrows.bindOne (Γ' : Ctx) (konst init lent writable byRef : Bool) (σ : MSt) (x : String) :
    SchedGrows (Fun.bindOne Γ' konst init lent writable byRef σ x) := fun st a st' w h => by
  simp only [Fun.bindOne] at h
  simp only [MoveM.run_bind] at h
  cases h
  have lk : ∀ u m, st.madeAt.lookup u = some m →
      ((st.uid, st.next) :: st.madeAt).lookup u = some m := by
    intro u m hm
    have hu : u ≠ st.uid := Nat.ne_of_lt (w.fresh u m (List.lookup_mem' hm))
    simp only [List.lookup]
    have : (u == st.uid) = false := by simpa using hu
    rw [this]; exact hm
  refine ⟨Nat.le_refl _, Nat.le_succ _, fun _ x => x, fun _ x => x, lk,
    ⟨[], st.out, rfl, ?_⟩, ⟨?_, ?_, fun u hu => Nat.lt_succ_of_lt (w.heldLt u hu),
      fun u hu => w.heldTracked u hu⟩⟩
  · refine Pairwise.imp (fun a b r => ?_) (Pairwise.refl st st.out)
    obtain ⟨k, s', a', f, sub, ext, p⟩ := r
    exact ⟨k, s', a', f, sub, ext, fun hk u hu hn _ => absurd hu hn⟩
  · intro u m hm
    cases hm with
    | head => exact Nat.lt_succ_self _
    | tail _ hm => exact Nat.lt_succ_of_lt (w.fresh u m hm)
  · intro u hu
    by_cases e : u = st.uid
    · subst e; exact ⟨st.next, by simp [List.lookup]⟩
    · obtain ⟨m, hm⟩ := w.made u (Nat.lt_of_le_of_ne (Nat.le_of_lt_succ hu) e)
      exact ⟨m, lk u m hm⟩

theorem SchedGrows.bindM (Γ' : Ctx) (σ : MSt) (xs : List String) (init writable konst : Bool) :
    SchedGrows (Fun.bindM Γ' σ xs init writable konst) := by
  unfold Fun.bindM
  generalize σ = σ0
  induction xs generalizing σ0 with
  | nil => exact SchedGrows.pure σ0
  | cons x xs ih =>
    simp only [List.foldlM]
    exact SchedGrows.bind (SchedGrows.bindOne _ _ _ _ _ _ σ0 x) (fun σ1 => ih σ1)

/-- A point recorded: put before every other. -/
theorem SchedExt.prepend {st st' : DSt} (w : st.WF) (es : List Entry) (o : st'.out = es ++ st.out)
    (n : st'.next = st.next) (u : st'.uid = st.uid) (h : st'.held = st.held)
    (t : st'.tracked = st.tracked) (m : st'.madeAt = st.madeAt) : SchedExt st st' := by
  refine ⟨n ▸ Nat.le_refl _, u ▸ Nat.le_refl _, fun _ x => h ▸ x, fun _ x => t ▸ x, fun _ _ x => m ▸ x,
    ⟨es, st.out, o, ?_⟩, ⟨?_, ?_, ?_, ?_⟩⟩
  · refine Pairwise.imp (fun a b r => ?_) (Pairwise.refl st st.out)
    obtain ⟨k, s', a', f, sub, ext, p⟩ := r
    exact ⟨k, s', a', f, sub, fun x hx => (ext x hx).imp id (fun y => t ▸ y),
      fun hk x hx hn _ => absurd (h ▸ hx) hn⟩
  · intro x y hm; rw [u]; exact w.fresh x y (m ▸ hm)
  · intro x hx; rw [m]; exact w.made x (u ▸ hx)
  · intro x hx; rw [u]; exact w.heldLt x (h ▸ hx)
  · intro x hx; rw [t]; exact w.heldTracked x (h ▸ hx)

theorem SchedGrows.record (kind : String) (stmt : Nat) (arm : Option Nat) (drops : List Nat) (always : Bool) :
    SchedGrows (Fun.record kind stmt arm drops always) := fun st a st' w h => by
  unfold Fun.record at h
  split at h
  · simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
    cases h
    exact SchedExt.prepend w [_] rfl rfl rfl rfl rfl rfl
  · cases h; exact SchedExt.refl w

theorem SchedGrows.recordNotYetMoved (stmt : Nat) (moved : List Nat) :
    SchedGrows (Fun.recordNotYetMoved stmt moved) := fun st a st' w h => by
  unfold Fun.recordNotYetMoved at h
  simp only [MoveM.run_bind] at h
  split at h
  · simp only [set, StateT.set, MonadStateOf.set, StateT.run] at h
    cases h
    exact SchedExt.prepend w [_] rfl rfl rfl rfl rfl rfl
  · cases h; exact SchedExt.refl w


/-- What `holdFrom u` does to one point. -/
def patchEntry (st : DSt) (u : Nat) (e : Entry) : Entry :=
  match e with
  | (k, stm, a, ds) =>
    if (k == "before_return" || k == "before_try_return" || k == "before_set_target_early_return")
      && stm ≥ (st.madeAt.lookup u).getD 0 && !ds.any (·.1 == u) then (k, stm, a, ds ++ [(u, none)])
    else (k, stm, a, ds)

theorem holdFrom_out (u : Nat) (st : DSt) : (holdFrom u st).out = st.out.map (patchEntry st u) := by
  simp only [holdFrom]
  congr 1

theorem patchEntry_ext {st : DSt} {u : Nat} (e : Entry) :
    EntryExt st (holdFrom u st) (patchEntry st u e) e := by
  obtain ⟨k, stm, a, ds⟩ := e
  have trk : u ∈ (holdFrom u st).tracked := by
    simp only [holdFrom]; split
    · next h => simpa using h
    · exact .head _
  have hheld : ∀ x, x ∈ (holdFrom u st).held → x ∉ st.held → x = u := by
    intro x hx hn
    simp only [holdFrom] at hx
    split at hx
    · exact absurd hx hn
    · cases hx with
      | head => rfl
      | tail _ h => exact absurd h hn
  simp only [patchEntry]
  split
  · next c =>
    refine ⟨rfl, rfl, rfl, fun hk => ?_, fun x hx => ?_, fun x hx => ?_, fun _ x hx hn _ => ?_⟩
    · simp only [exitKind] at hk
      simp only [Bool.and_eq_true] at c
      rw [c.1.1] at hk; cases hk
    · simp only [Entry.uids, List.map_append, List.mem_append]; exact .inl hx
    · simp only [Entry.uids, List.map_append, List.mem_append, List.map_cons, List.map_nil,
        List.mem_cons, List.not_mem_nil, or_false] at hx
      rcases hx with h | h
      · exact .inl h
      · exact .inr (h ▸ trk)
    · rw [hheld x hx hn]
      simp [Entry.uids]
  · next c =>
    refine ⟨rfl, rfl, rfl, fun _ => rfl, fun _ hx => hx, fun _ hx => .inl hx, fun hk x hx hn hm => ?_⟩
    rw [hheld x hx hn] at hm ⊢
    simp only [exitKind] at hk
    simp only [DSt.made, holdFrom] at hm
    simp only [hk, Bool.true_and, Bool.and_eq_true, decide_eq_true_eq, Bool.not_eq_true',
      not_and, Bool.not_eq_false] at c
    have := c hm
    simp only [List.any_eq_true, beq_iff_eq] at this
    obtain ⟨p, hp, hpu⟩ := this
    simp only [Entry.uids, List.mem_map]
    exact ⟨p, hp, hpu⟩

theorem patch_pairwise (st : DSt) (u : Nat) :
    ∀ (es : List Entry), Pairwise (EntryExt st (Fun.holdFrom u st)) (es.map (patchEntry st u)) es
  | [] => .nil
  | e :: es => .cons (patchEntry_ext e) (patch_pairwise st u es)

theorem SchedExt.holdFrom {st : DSt} {u : Nat} (w : st.WF) (lt : u < st.uid) :
    SchedExt st (Fun.holdFrom u st) := by
  refine ⟨Nat.le_refl _, Nat.le_refl _, ?_, ?_, fun _ _ x => x,
    ⟨[], _, holdFrom_out u st, patch_pairwise st u _⟩,
    ⟨fun x y hm => w.fresh x y hm, fun x hx => w.made x hx, ?_, ?_⟩⟩
  · intro x hx; simp only [Fun.holdFrom]; split
    · exact hx
    · exact .tail _ hx
  · intro x hx; simp only [Fun.holdFrom]; split
    · exact hx
    · exact .tail _ hx
  · intro x hx
    simp only [Fun.holdFrom] at hx
    split at hx
    · exact w.heldLt x hx
    · cases hx with
      | head => exact lt
      | tail _ h => exact w.heldLt x h
  · intro x hx
    simp only [Fun.holdFrom] at hx ⊢
    split at hx
    · split
      · exact w.heldTracked x hx
      · exact .tail _ (w.heldTracked x hx)
    · cases hx with
      | head =>
        split
        · next h => simpa using h
        · exact .head _
      | tail _ h =>
        split
        · exact w.heldTracked x h
        · exact .tail _ (w.heldTracked x h)


theorem SchedGrows.ownedIn (vs : MSt) : SchedGrows (Fun.ownedIn vs) :=
  SchedGrows.keepsSched (fun st a st' h => by
    simp only [Fun.ownedIn, MoveM.run_bind] at h
    cases h; rfl)

/-- A step over each of a list, each keeping `SchedExt` where `P` holds of it,
which `SchedExt` keeps. -/
theorem forM_ext {f : MVar → MoveM Unit} (P : MVar → DSt → Prop)
    (hf : ∀ v st st', st.WF → P v st → (f v).run st = .ok ((), st') → SchedExt st st')
    (hP : ∀ v st st', P v st → SchedExt st st' → P v st') :
    ∀ (l : MSt) (st st' : DSt), st.WF → (∀ v, v ∈ l → P v st) → (l.forM f).run st = .ok ((), st') →
      SchedExt st st'
  | [], st, st', w, _, h => by
    have e : (List.forM ([] : MSt) f).run st = .ok ((), st) := rfl
    rw [e] at h
    cases h; exact SchedExt.refl w
  | v :: l, st, st', w, hp, h => by
    have e : List.forM (v :: l) f = (f v >>= fun _ => List.forM l f) := rfl
    rw [e] at h
    obtain ⟨_, st1, h1, h2⟩ := MoveM.bind_ok h
    have x1 := hf v st st1 w (hp v (.head _)) h1
    exact x1.trans (forM_ext P hf hP l st1 st' x1.wf (fun u hu => hP u st st1 (hp u (.tail _ hu)) x1) h2)

/-- The bindings `σ` owns were each numbered already, as the schedule has
it. -/
def Made (σ : MSt) (st : DSt) : Prop := ∀ v, v ∈ σ → v.owns = true → v.uid < st.uid

theorem Made.ext {σ : MSt} {st st' : DSt} (h : Made σ st) (x : SchedExt st st') : Made σ st' :=
  fun v hv ho => Nat.lt_of_lt_of_le (h v hv ho) x.uid

theorem loopHeld_ext {σc : MSt} {body : List Stmt} {r : MOut} {st st' : DSt} (w : st.WF) (hm : Made σc st)
    (h : (loopHeld σc body r).run st = .ok ((), st')) : SchedExt st st' := by
  unfold loopHeld at h
  refine forM_ext (fun v st => v.owns = true → v.uid < st.uid)
    (fun v st st' w p e => ?_) (fun v st st' p x ho => Nat.lt_of_lt_of_le (p ho) x.uid)
    σc st st' w (fun v hv => hm v hv) h
  dsimp only at e
  split at e
  · next c =>
    simp only [Bool.and_eq_true] at c
    simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at e
    cases e
    exact SchedExt.holdFrom w (p c.1.2)
  · cases e; exact SchedExt.refl w

theorem SchedGrows.leaving (id : Nat) (σ : MSt) (e : Expr) {m : MoveM MSt} (hm : SchedGrows m) :
    SchedGrows (Fun.leaving id σ e m) := by
  unfold Fun.leaving
  split
  · exact SchedGrows.bind (SchedGrows.ownedIn σ) fun _ => SchedGrows.bind hm fun _ =>
      SchedGrows.bind (SchedGrows.ownedIn _) fun _ => SchedGrows.bind (SchedGrows.recordNotYetMoved _ _) fun _ =>
      SchedGrows.bind (SchedGrows.record _ _ _ _ _) fun _ => SchedGrows.pure _
  · exact hm


/-! ## What the walk's states keep of the bindings -/

/-- `τ`'s bindings are some of `σ`'s, each with the same number and as
owned. -/
def SameIds (τ σ : MSt) : Prop := ∀ v, v ∈ τ → ∃ w, w ∈ σ ∧ v.uid = w.uid ∧ v.owns = w.owns

theorem SameIds.refl (σ : MSt) : SameIds σ σ := fun v h => ⟨v, h, rfl, rfl⟩

theorem SameIds.trans {a b c : MSt} (h1 : SameIds a b) (h2 : SameIds b c) : SameIds a c := fun v hv =>
  let ⟨w, hw, u1, o1⟩ := h1 v hv
  let ⟨x, hx, u2, o2⟩ := h2 w hw
  ⟨x, hx, u1.trans u2, o1.trans o2⟩

theorem Made.of_same {τ σ : MSt} {st : DSt} (h : Made σ st) (s : SameIds τ σ) : Made τ st := fun v hv ho =>
  let ⟨w, hw, u, o⟩ := s v hv
  u ▸ h w hw (o ▸ ho)

theorem SameIds.below : ∀ {τ σ : MSt}, τ.Below σ → SameIds τ σ
  | [], [], .nil => fun _ h => by cases h
  | _ :: _, _ :: _, .cons b t => fun v h => by
    cases h with
    | head => exact ⟨_, .head _, b.2.1, b.2.2.1⟩
    | tail _ h => let ⟨w, hw, u, o⟩ := SameIds.below t v h; exact ⟨w, .tail _ hw, u, o⟩

theorem SameIds.drop (n : Nat) (σ : MSt) : SameIds (σ.drop n) σ := fun v h =>
  ⟨v, List.mem_of_mem_drop h, rfl, rfl⟩

theorem SameIds.take (n : Nat) (σ : MSt) : SameIds (List.take n σ) σ := fun v h =>
  ⟨v, List.mem_of_mem_take h, rfl, rfl⟩

theorem SameIds.back (σ τ : MSt) : SameIds (σ.back τ) τ := SameIds.drop _ τ

theorem SameIds.update {f : MVar → MVar} (hf : ∀ v, (f v).uid = v.uid ∧ (f v).owns = v.owns) (x : String) :
    ∀ (σ : MSt), SameIds (MSt.update f x σ) σ
  | [] => fun _ h => by cases h
  | w :: σ => fun v h => by
    simp only [MSt.update] at h
    split at h
    · cases h with
      | head => exact ⟨w, .head _, (hf w).1, (hf w).2⟩
      | tail _ h => exact ⟨_, .tail _ h, rfl, rfl⟩
    · cases h with
      | head => exact ⟨_, .head _, rfl, rfl⟩
      | tail _ h => let ⟨x', hx, u, o⟩ := SameIds.update hf x σ v h; exact ⟨x', .tail _ hx, u, o⟩

theorem SameIds.walk (x : String) (σ : MSt) : SameIds (σ.walk x) σ :=
  SameIds.update (f := fun v => { v with walked := true }) (fun _ => ⟨rfl, rfl⟩) x σ

/-- What `zipWith` makes of two lists, where each is the first's with its
number and ownership. -/
theorem SameIds.zipWith {f : MVar → MVar → MVar} (hf : ∀ v w, (f v w).uid = v.uid ∧ (f v w).owns = v.owns) :
    ∀ (σ τ : MSt), SameIds (List.zipWith f σ τ) σ
  | [], _ => fun _ h => by simp at h
  | _ :: _, [] => fun _ h => by simp at h
  | v :: σ, w :: τ => fun u h => by
    simp only [List.zipWith_cons_cons, List.mem_cons] at h
    rcases h with h | h
    · subst h; exact ⟨v, .head _, (hf v w).1, (hf v w).2⟩
    · let ⟨x, hx, a, b⟩ := SameIds.zipWith hf σ τ u h; exact ⟨x, .tail _ hx, a, b⟩

theorem SameIds.initFrom (σ τ : MSt) : SameIds (σ.initFrom τ) σ :=
  SameIds.zipWith (f := fun v w => { v with init := w.init }) (fun _ _ => ⟨rfl, rfl⟩) σ τ

theorem SameIds.initMeet (σ τ : MSt) : SameIds (σ.initMeet τ) σ :=
  SameIds.zipWith (f := fun v w => { v with init := v.init && w.init }) (fun _ _ => ⟨rfl, rfl⟩) σ τ


theorem joinIf_eq {t e r : MSt} {tc ec : Bool} {st st' : DSt}
    (h : (joinIf t tc e ec).run st = .ok (r, st')) : st' = st ∧ (SameIds r t ∨ SameIds r e) := by
  unfold joinIf at h
  split at h
  · cases h
  · cases h
    refine ⟨rfl, ?_⟩
    split
    · exact .inr (SameIds.initFrom _ _)
    · exact .inl (SameIds.initFrom _ _)

theorem foldl_initMeet_same : ∀ (ms : List MSt) (m : MSt), SameIds (ms.foldl MSt.initMeet m) m
  | [], m => SameIds.refl m
  | n :: ms, m => (foldl_initMeet_same ms (m.initMeet n)).trans (SameIds.initMeet m n)

theorem joinMatch_eq {σ r : MSt} {arms : List (MSt × Bool)} {st st' : DSt}
    (h : (joinMatch σ arms).run st = .ok (r, st')) :
    st' = st ∧ (SameIds r σ ∨ ∃ a, a ∈ arms ∧ SameIds r a.1) := by
  unfold joinMatch at h
  simp only at h
  split at h
  · next m ms hm =>
    split at h
    · cases h
    · cases h
      refine ⟨rfl, .inr ?_⟩
      have : m ∈ (arms.filter (·.2)).map (·.1) := by rw [hm]; exact .head _
      obtain ⟨a, ha, e⟩ := List.mem_map.mp this
      exact ⟨a, (List.mem_filter.mp ha).1, e ▸ foldl_initMeet_same ms m⟩
  · split at h
    · next f _ l _ hf _ =>
      cases h
      exact ⟨rfl, .inr ⟨(f, _), List.mem_of_mem_head? hf, SameIds.initFrom _ _⟩⟩
    · cases h; exact ⟨rfl, .inl (SameIds.refl _)⟩

theorem joinAll_cons_cons (code : Lit.Code) (σ τ w : MSt) (L : List MSt) :
    joinAll code σ (τ :: w :: L) = match τ.join w with
      | some j => joinAll code σ (j :: L)
      | none => throw code := by
  simp only [joinAll, List.foldlM_cons]
  split
  · next j hj => simp only [hj, pure_bind]
  · next hj => simp only [hj, bind]; rfl

/-- A check that refuses or goes on. -/
theorem MoveM.ite_throw {α} {c : Bool} {code : Lit.Code} {m : MoveM α} {st : DSt} {p : α × DSt}
    (h : (if c = true then throw code else m).run st = .ok p) : m.run st = .ok p := by
  cases c
  · exact h
  · cases h

theorem joinAll_eq {code : Lit.Code} {σ r : MSt} : ∀ {L : List MSt} {st st' : DSt},
    (joinAll code σ L).run st = .ok (r, st') → st' = st ∧ (SameIds r σ ∨ ∃ τ, τ ∈ L ∧ SameIds r τ)
  | [], _, _, h => by cases h; exact ⟨rfl, .inl (SameIds.refl _)⟩
  | [_], _, _, h => by cases h; exact ⟨rfl, .inr ⟨_, .head _, SameIds.refl _⟩⟩
  | τ :: w :: L, st, st', h => by
    rw [joinAll_cons_cons] at h
    unfold MSt.join at h
    split at h
    · next j hj =>
      split at hj
      · cases hj
        obtain ⟨e, s⟩ := joinAll_eq h
        refine ⟨e, ?_⟩
        rcases s with s | ⟨υ, hυ, s⟩
        · exact .inl s
        · cases hυ with
          | head => exact .inr ⟨τ, .head _, s.trans (SameIds.initMeet _ _)⟩
          | tail _ hυ => exact .inr ⟨υ, .tail _ (.tail _ hυ), s⟩
      · cases hj
    · cases h
termination_by L => L.length

theorem loopEnd_eq {σ σc r0 : MSt} {natural : Bool} {body : List Stmt} {r : MOut} {st st' : DSt}
    (h : (loopEnd σ σc natural body r).run st = .ok (r0, st')) :
    st' = st ∧ (SameIds r0 σc ∨ ∃ τ, τ ∈ r.2.1 ∧ SameIds r0 (σc.back τ)) := by
  unfold loopEnd at h
  dsimp only at h
  have h := MoveM.ite_throw h
  · obtain ⟨e, s⟩ := joinAll_eq h
    refine ⟨e, ?_⟩
    rcases s with s | ⟨τ, hτ, s⟩
    · exact .inl s
    · simp only [List.mem_append, List.mem_map] at hτ
      rcases hτ with hτ | ⟨b, hb, eb⟩
      · split at hτ
        · cases hτ with
          | head => exact .inl s
          | tail _ h => cases h
        · cases hτ
      · exact .inr ⟨b, hb, eb ▸ s⟩


/-! ## The walk of a statement keeps it -/

/-- What a statement, a block or a `match`'s cases leave, each state's
owned bindings numbered already. -/
def OutMade (r : MOut) (st : DSt) : Prop :=
  Made r.1 st ∧ (∀ τ, τ ∈ r.2.1 → Made τ st) ∧ (∀ τ, τ ∈ r.2.2 → Made τ st)

theorem Made.cons {v : MVar} {σ : MSt} {st : DSt} (hv : v.owns = true → v.uid < st.uid) (h : Made σ st) :
    Made (v :: σ) st := fun w hw ho => by
  cases hw with
  | head => exact hv ho
  | tail _ hw => exact h w hw ho

theorem bindOne_made {Γ' : Ctx} {konst init lent writable byRef : Bool} {σ σ' : MSt} {x : String}
    {st st' : DSt} (h : (bindOne Γ' konst init lent writable byRef σ x).run st = .ok (σ', st'))
    (m : Made σ st) : Made σ' st' := by
  simp only [bindOne, MoveM.run_bind] at h
  cases h
  exact Made.cons (fun _ => Nat.lt_succ_self _) (fun v hv ho => Nat.lt_succ_of_lt (m v hv ho))

theorem bindM_made {Γ' : Ctx} {xs : List String} {init writable konst : Bool} :
    ∀ {σ σ' : MSt} {st st' : DSt}, st.WF → (bindM Γ' σ xs init writable konst).run st = .ok (σ', st') →
      Made σ st → Made σ' st' ∧ SchedExt st st' := by
  unfold bindM
  induction xs with
  | nil => intro σ σ' st st' w h m; cases h; exact ⟨m, SchedExt.refl w⟩
  | cons x xs ih =>
    intro σ σ' st st' w h m
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    have x1 := SchedGrows.bindOne Γ' konst init false writable false σ x st σ1 st1 w h1
    obtain ⟨m2, x2⟩ := ih x1.wf h2 (bindOne_made h1 m)
    exact ⟨m2, x1.trans x2⟩

end Datalove.Fun
