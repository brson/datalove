import Datalove.Proofs.DropPath

/-!
# What the walk's steps record, exactly

The points of the drop schedule's proof (`botdocs/plan-drop-proof.md`) are
right because of what the walk records at each: `ownedIn` of the variables
there, as the schedule holds them. These say exactly what each step that
records does: which point it adds, where, and with what drops.
-/

namespace Datalove.Fun

/-- What `ownedIn` finds of `vs`, where `held` is held. -/
def ownedOf (held : List Nat) (vs : MSt) : List Nat :=
  (vs.filter fun v => v.owns && (v.live || held.contains v.uid)).map (·.uid)

theorem ownedIn_run {vs : MSt} {st st' : DSt} {d : List Nat} (h : (ownedIn vs).run st = .ok (d, st')) :
    st' = st ∧ d = ownedOf st.held vs := by
  simp only [ownedIn, MoveM.run_bind] at h
  cases h; exact ⟨rfl, rfl⟩

theorem mem_ownedOf {held : List Nat} {vs : MSt} {u : Nat} :
    u ∈ ownedOf held vs ↔ ∃ v, v ∈ vs ∧ v.owns = true ∧ (v.live = true ∨ u ∈ held) ∧ v.uid = u := by
  simp only [ownedOf, List.mem_map, List.mem_filter, Bool.and_eq_true, Bool.or_eq_true, List.contains_iff_mem]
  constructor
  · rintro ⟨v, ⟨hv, ho, hl⟩, rfl⟩; exact ⟨v, hv, ho, hl, rfl⟩
  · rintro ⟨v, hv, ho, hl, rfl⟩; exact ⟨v, ⟨hv, ho, hl⟩, rfl⟩

/-- What a step that may return early records, where it may: the point of
what it moved, if it moved anything owned, and the point of what is owned
after it. -/
theorem leavingIf_spec {id : Nat} {σ σ' : MSt} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (h : (leavingIf id σ true m).run st = .ok (σ', st')) :
    ∃ stm, m.run st = .ok (σ', stm) ∧ stm.sched = st.sched ∧
      st'.out = ("before_try_return", id, none, (ownedOf st.held σ').map (·, none)) ::
        ((if (ownedOf st.held σ).filter (fun u => !(ownedOf st.held σ').contains u) = [] then []
          else [("try_return_not_yet_moved", id, none,
            ((ownedOf st.held σ).filter (fun u => !(ownedOf st.held σ').contains u)).map
              fun u => (u, stm.movedAt.lookup u))]) ++ st.out) ∧
      st'.held = st.held ∧ st'.tracked = st.tracked ∧ st'.madeAt = st.madeAt ∧ st'.next = st.next ∧
      st'.uid = st.uid := by
  unfold leavingIf at h
  simp only [ite_true] at h
  obtain ⟨b, st1, h1, h⟩ := MoveM.bind_ok h
  obtain ⟨e1, e1'⟩ := ownedIn_run h1
  subst st1; subst b
  obtain ⟨σ1, st2, h2, h⟩ := MoveM.bind_ok h
  have c2 := (hm st σ1 st2 h2).2
  simp only [DSt.sched, Prod.mk.injEq] at c2
  obtain ⟨cn, cu, chd, cnm, co, cm, ct⟩ := c2
  obtain ⟨d, st3, h3, h⟩ := MoveM.bind_ok h
  obtain ⟨e3, e3'⟩ := ownedIn_run h3
  subst st3; subst d
  rw [chd] at h
  obtain ⟨_, st4, h4, h⟩ := MoveM.bind_ok h
  obtain ⟨_, st5, h5, h⟩ := MoveM.bind_ok h
  cases h
  refine ⟨st2, h2, ?_, ?_⟩
  · simp only [DSt.sched, Prod.mk.injEq]; exact ⟨cn, cu, chd, cnm, co, cm, ct⟩
  · unfold record at h5
    simp only [Bool.true_or, ite_true, modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet,
      StateT.run] at h5
    cases h5
    unfold recordNotYetMoved at h4
    simp only [MoveM.run_bind] at h4
    split at h4
    · next hne =>
      simp only [set, StateT.set, MonadStateOf.set, StateT.run] at h4
      cases h4
      have hne' : ¬((ownedOf st.held σ).filter (fun u => !(ownedOf st.held σ').contains u) = []) := by
        simpa [List.isEmpty_iff] using hne
      simp only [hne', ite_false, List.singleton_append]
      exact ⟨by rw [co], chd, ct, cm, cn, cu⟩
    · next he =>
      cases h4
      have he' : (ownedOf st.held σ).filter (fun u => !(ownedOf st.held σ').contains u) = [] := by
        simpa [List.isEmpty_iff] using he
      simp only [he', ite_true, List.nil_append]
      exact ⟨by rw [co], chd, ct, cm, cn, cu⟩


theorem record_eq {kind : String} {stmt : Nat} {arm : Option Nat} {ds : List Nat} {always : Bool}
    {st st' : DSt} (h : (record kind stmt arm ds always).run st = .ok ((), st')) :
    st'.out = (if always || !ds.isEmpty then (kind, stmt, arm, ds.map (·, none)) :: st.out else st.out) ∧
      st'.held = st.held ∧ st'.tracked = st.tracked ∧ st'.madeAt = st.madeAt ∧ st'.next = st.next ∧
      st'.uid = st.uid := by
  unfold record at h
  split at h
  · next c =>
    simp only [modify, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
    cases h
    simp [c]
  · next c =>
    cases h
    simp only [Bool.not_eq_true] at c
    simp [c]

theorem Rel.mem_zip {F st : DSt} {U : Nat} : ∀ {σ : MSt} {π : List Bool}, Rel F st U σ π →
    ∀ {v : MVar} {p : Bool}, (v, p) ∈ σ.zip π → (v.live = true → p = true) ∧ (p = true → v.live = true ∨ Div F st U v)
  | [], [], .nil, _, _, h => by simp at h
  | _ :: _, _ :: _, .cons a b t, _, _, h => by
    simp only [List.zip_cons_cons, List.mem_cons, Prod.mk.injEq] at h
    rcases h with ⟨rfl, rfl⟩ | h
    · exact ⟨a, b⟩
    · exact Rel.mem_zip t h

theorem Rel.exists_zip {F st : DSt} {U : Nat} : ∀ {σ : MSt} {π : List Bool}, Rel F st U σ π →
    ∀ {v : MVar}, v ∈ σ → ∃ p, (v, p) ∈ σ.zip π ∧ (v.live = true → p = true)
  | [], [], .nil, _, h => by simp at h
  | w :: _, q :: _, .cons a _ t, v, h => by
    simp only [List.mem_cons] at h
    rcases h with rfl | h
    · exact ⟨q, by simp, a⟩
    · obtain ⟨p, hp, hl⟩ := Rel.exists_zip t h
      exact ⟨p, by simp [hp], hl⟩

theorem mem_zip_left {α β} {a : α} {b : β} : ∀ {l1 : List α} {l2 : List β}, (a, b) ∈ l1.zip l2 → a ∈ l1
  | [], _, h => by simp at h
  | _ :: _, [], h => by simp at h
  | x :: l1, y :: l2, h => by
    simp only [List.zip_cons_cons, List.mem_cons, Prod.mk.injEq] at h
    rcases h with ⟨rfl, rfl⟩ | h
    · exact .head _
    · exact .tail _ (mem_zip_left h)


theorem dropsAt_none (F : DSt) : F.dropsAt none = [] := rfl

/-- A point recorded of the variables `vs`, which the path holds as `πs`
says, is right on the path: if it is a way out of the function, recorded
always, of bindings each made before its statement, or if what it leaves
was all made inside the innermost loop the path is in. -/
theorem recEv_good {kind : String} {id : Nat} {vs : MSt} {πs : List Bool} {always : Bool}
    {st0 st1 st2 F : DSt} {U : Nat}
    (hr : (record kind id none (ownedOf st1.held vs) always).run st1 = .ok ((), st2))
    (x : SchedExt st2 F) (w1 : st1.WF) (rel : Rel F st0 U vs πs) (hheld : ∀ u, u ∈ st0.held → u ∈ st1.held)
    (cover : (always = true ∧ exitKind kind = true ∧ ∀ v, v ∈ vs → v.owns = true → F.made v.uid ≤ id) ∨
      (∀ v, v ∈ vs → v.owns = true → U ≤ v.uid)) :
    (recEv st1 st2 vs πs).Good F := by
  obtain ⟨ho, hh, ht, hm, hn, hu⟩ := record_eq hr
  have trk : ∀ u, u ∈ st1.held → u ∈ F.tracked := fun u h => x.tracked u (ht ▸ w1.heldTracked u h)
  unfold Ev.Good Ev.drops recEv
  simp only [dropsAt_none, List.filter_nil, List.append_nil]
  by_cases rec : (always || !(ownedOf st1.held vs).isEmpty) = true
  · simp only [rec, ite_true] at ho
    have hl : st2.out.length = st1.out.length + 1 := by rw [ho]; simp
    simp only [hl, ite_true]
    have hk : st1.out.length < st2.out.length := by omega
    obtain ⟨e', he', ext⟩ := x.at st1.out.length hk
    have he : st2.out.reverse[st1.out.length]'(by simpa using hk) =
        (kind, id, none, (ownedOf st1.held vs).map (·, none)) := by
      simp only [ho, List.reverse_cons]
      rw [List.getElem_append_right (by simp)]
      simp
    rw [he] at ext
    obtain ⟨k1, s1, _, _, sub, extra, patched⟩ := ext
    have hd : F.dropsAt (some st1.out.length) = Entry.uids e' := by simp [DSt.dropsAt, he']
    rw [hd]
    have inE : ∀ u, u ∈ ownedOf st1.held vs → u ∈ Entry.uids e' := fun u h =>
      sub u (by simp only [Entry.uids, List.map_map]; simpa using h)
    refine ⟨fun v p hvp ho' hp => ?_, fun u hu' => ?_⟩
    · obtain ⟨_, hd'⟩ := rel.mem_zip hvp
      have hv := mem_zip_left hvp
      rcases hd' hp with hl' | ⟨_, hF, _, hs | hlt⟩
      · exact inE _ (mem_ownedOf.mpr ⟨v, hv, ho', .inl hl', rfl⟩)
      · exact inE _ (mem_ownedOf.mpr ⟨v, hv, ho', .inr (hheld _ hs), rfl⟩)
      · rcases cover with ⟨_, hk', hmade⟩ | hU
        · by_cases hs : v.uid ∈ st1.held
          · exact inE _ (mem_ownedOf.mpr ⟨v, hv, ho', .inr hs, rfl⟩)
          · exact patched (by simpa using hk') v.uid hF (hh ▸ hs) (hmade v hv ho')
        · exact absurd hlt (Nat.not_lt.mpr (hU v hv ho'))
    · rcases extra u hu' with h | h
      · simp only [Entry.uids, List.map_map] at h
        have h' : u ∈ ownedOf st1.held vs := by simpa using h
        obtain ⟨v, hv, _, hl', rfl⟩ := mem_ownedOf.mp h'
        rcases hl' with hl' | hs
        · obtain ⟨p, hp, hlp⟩ := rel.exists_zip hv
          exact .inl ⟨v, (hlp hl') ▸ hp, rfl⟩
        · exact .inr (trk _ hs)
      · exact .inr h
  · simp only [rec, Bool.false_eq_true, ite_false] at ho
    have hl : ¬ st2.out.length = st1.out.length + 1 := by rw [ho]; omega
    simp only [hl, ite_false, dropsAt_none]
    simp only [Bool.or_eq_true, Bool.not_eq_true', not_or, Bool.not_eq_false,
      List.isEmpty_iff] at rec
    obtain ⟨ha, hempty⟩ := rec
    refine ⟨fun v p hvp ho' hp => ?_, fun u hu' => by simp at hu'⟩
    exfalso
    obtain ⟨_, hd'⟩ := rel.mem_zip hvp
    have hv := mem_zip_left hvp
    have none : ∀ u, u ∉ ownedOf st1.held vs := fun u h => by rw [hempty] at h; simp at h
    rcases hd' hp with hl' | ⟨_, _, _, hs | hlt⟩
    · exact none _ (mem_ownedOf.mpr ⟨v, hv, ho', .inl hl', rfl⟩)
    · exact none _ (mem_ownedOf.mpr ⟨v, hv, ho', .inr (hheld _ hs), rfl⟩)
    · rcases cover with ⟨hal, _, _⟩ | hU
      · rw [hal] at ha; exact ha rfl
      · exact absurd hlt (Nat.not_lt.mpr (hU v hv ho'))


/-- Owned bindings of a state, each a number of its own. -/
def OwnedNodup (σ : MSt) : Prop := List.Pairwise (fun v w => v.owns = true → w.owns = true → v.uid ≠ w.uid) σ

/-- What a return from partway along a step finds, position by position: the
step from `σ` to `σ'`, the path holding `π` before it and `πk` where it
returns. -/
theorem between_facts {F st : DSt} {U : Nat} : ∀ {σ σ' : MSt} {π πk : List Bool},
    σ'.Below σ → Rel F st U σ π → Between (delta σ σ' π) π πk →
    (∀ v p, (v, p) ∈ σ.zip πk → ∃ v', v' ∈ σ' ∧ v'.uid = v.uid ∧ v'.owns = v.owns ∧
      (p = true → v.live = true ∨ Div F st U v) ∧ (v'.live = true → p = true)) ∧
    (∀ v', v' ∈ σ' → ∃ v p, (v, p) ∈ σ.zip πk ∧ v.uid = v'.uid ∧ v.owns = v'.owns ∧ v.live = (v.live) ∧
      (v'.live = true → p = true))
  | [], [], [], [], .nil, .nil, .nil => ⟨fun _ _ h => by simp at h, fun _ h => by simp at h⟩
  | v :: σ, w :: σ', q :: π, k :: πk, .cons bw bt, .cons a b t, hb => by
    simp only [delta] at hb
    cases hb with
    | cons lo hi hbt =>
    obtain ⟨f1, f2⟩ := between_facts bt t hbt
    have wl : w.live = true → k = true := by
      intro hw
      have hv := bw.2.2.2.2.2.2.2.2 hw
      apply lo
      simp [hv, hw, a hv]
    refine ⟨fun v' p h => ?_, fun v' h => ?_⟩
    · simp only [List.zip_cons_cons, List.mem_cons, Prod.mk.injEq] at h
      rcases h with ⟨rfl, rfl⟩ | h
      · exact ⟨w, .head _, bw.2.1, bw.2.2.1, fun hp => b (hi hp), wl⟩
      · obtain ⟨x, hx, u, o, c, d⟩ := f1 v' p h
        exact ⟨x, .tail _ hx, u, o, c, d⟩
    · simp only [List.mem_cons] at h
      rcases h with rfl | h
      · exact ⟨v, k, by simp, bw.2.1.symm, bw.2.2.1.symm, rfl, wl⟩
      · obtain ⟨x, p, hx, u, o, l, d⟩ := f2 v' h
        exact ⟨x, p, by simp [hx], u, o, l, d⟩


/-- A return from partway along a step that may return early, holding
`πk`, is right on the path: the bindings of the function were each made
before the step's statement. -/
theorem tryEv_good {id : Nat} {σ σ' : MSt} {m : MoveM MSt} (hm : Shr σ m) {st0 st1 st2 F : DSt} {U : Nat}
    {π πk : List Bool} (hl : (leavingIf id σ true m).run st1 = .ok (σ', st2))
    (x : SchedExt st2 F) (w1 : st1.WF) (rel : Rel F st0 U σ π) (hheld : ∀ u, u ∈ st0.held → u ∈ st1.held)
    (hmade : ∀ v, v ∈ σ → v.owns = true → F.made v.uid ≤ id) (hb : Between (delta σ σ' π) π πk) :
    (tryEv st1 st2 σ πk).Good F := by
  obtain ⟨stm, hmr, _, ho, hh, ht, _, _, _⟩ := leavingIf_spec hm hl
  have below : σ'.Below σ := (hm st1 σ' stm hmr).1
  obtain ⟨f1, f2⟩ := between_facts below rel hb
  have trk : ∀ u, u ∈ st1.held → u ∈ F.tracked := fun u h => x.tracked u (ht ▸ w1.heldTracked u h)
  -- What the step recorded, by where it is in the schedule.
  generalize hdrops : ownedOf st1.held σ' = drops at ho
  generalize hmoved : (ownedOf st1.held σ).filter (fun u => !drops.contains u) = moved at ho
  have movedMem : ∀ u, u ∈ ownedOf st1.held σ → u ∉ drops → u ∈ moved := by
    intro u h1 h2; rw [← hmoved]; simp [h1, h2]
  -- The point of what is owned after the step.
  have btr : ∃ e', F.dropsAt (tryEv st1 st2 σ πk).idx = Entry.uids e' ∧
      EntryExt st2 F e' ("before_try_return", id, none, drops.map (·, none)) := by
    have hk : st2.out.length - 1 < st2.out.length := by rw [ho]; simp
    obtain ⟨e', he', ext⟩ := x.at (st2.out.length - 1) hk
    have he : st2.out.reverse[st2.out.length - 1]'(by simpa using hk) =
        ("before_try_return", id, none, drops.map (·, none)) := by
      simp only [List.getElem_reverse]; simp [ho]
    rw [he] at ext
    exact ⟨e', by simp [tryEv, DSt.dropsAt, he'], ext⟩
  obtain ⟨eb, heb, k1, s1, _, _, sub, extra, patched⟩ := btr
  have inB : ∀ u, u ∈ drops → u ∈ Entry.uids eb := fun u h =>
    sub u (by simp only [Entry.uids, List.map_map]; simpa using h)
  -- The point of what the step moved, which a return before the move drops.
  have nym : moved ≠ [] → ∀ u, u ∈ moved → u ∈ F.dropsAt (tryEv st1 st2 σ πk).nym := by
    intro hne u hu
    have hlen : st2.out.length = st1.out.length + 2 := by rw [ho]; simp [hne]
    have hk : st1.out.length < st2.out.length := by omega
    obtain ⟨e', he', ext⟩ := x.at st1.out.length hk
    have he : st2.out.reverse[st1.out.length]'(by simpa using hk) =
        ("try_return_not_yet_moved", id, none, moved.map fun u => (u, stm.movedAt.lookup u)) := by
      simp only [ho, hne, ite_false, List.reverse_cons, List.singleton_append]
      rw [List.getElem_append_left (by simp), List.getElem_append_right (by simp)]
      simp
    rw [he] at ext
    have e'eq : e' = ("try_return_not_yet_moved", id, none, moved.map fun u => (u, stm.movedAt.lookup u)) :=
      ext.2.2.2.1 (by simp [exitKind])
    simp only [tryEv, hlen, ite_true, DSt.dropsAt, he', e'eq, Entry.uids, List.map_map]
    simpa using hu
  unfold Ev.Good Ev.drops
  rw [heb]
  refine ⟨fun v p hvp ho' hp => ?_, fun u hu => ?_⟩
  · obtain ⟨v', hv', u', o', hdiv, hlive⟩ := f1 v p hvp
    have hv := mem_zip_left hvp
    rcases hdiv hp with hvl | ⟨_, hF, _, hs | hlt⟩
    · by_cases hw : v'.live = true
      · exact List.mem_append_left _ (inB _ (hdrops ▸ mem_ownedOf.mpr ⟨v', hv', o' ▸ ho', .inl hw, u'⟩))
      · by_cases hd : v.uid ∈ drops
        · exact List.mem_append_left _ (inB _ hd)
        · have hm' := movedMem v.uid (mem_ownedOf.mpr ⟨v, hv, ho', .inl hvl, rfl⟩) hd
          have hne : moved ≠ [] := fun e => by rw [e] at hm'; simp at hm'
          refine List.mem_append_right _ (List.mem_filter.mpr ⟨nym hne _ hm', ?_⟩)
          simp only [List.any_eq_true, Bool.and_eq_true, beq_iff_eq]
          exact ⟨(v, p), hvp, rfl, hp⟩
    · exact List.mem_append_left _ (inB _ (hdrops ▸ mem_ownedOf.mpr ⟨v', hv', o' ▸ ho', .inr (hheld _ hs), u'⟩))
    · by_cases hs : v.uid ∈ st1.held
      · exact List.mem_append_left _ (inB _ (hdrops ▸ mem_ownedOf.mpr ⟨v', hv', o' ▸ ho', .inr hs, u'⟩))
      · exact List.mem_append_left _ (patched (by simp [exitKind]) v.uid hF (hh ▸ hs) (hmade v hv ho'))
  · rcases List.mem_append.mp hu with hu | hu
    · rcases extra u hu with h | h
      · simp only [Entry.uids, List.map_map] at h
        have h' : u ∈ drops := by simpa using h
        rw [← hdrops] at h'
        obtain ⟨v', hv', _, hl', rfl⟩ := mem_ownedOf.mp h'
        rcases hl' with hl' | hs
        · obtain ⟨v, p, hvp, u', _, _, d⟩ := f2 v' hv'
          exact .inl ⟨v, (d hl') ▸ hvp, u'⟩
        · exact .inr (trk _ hs)
      · exact .inr h
    · obtain ⟨_, hany⟩ := List.mem_filter.mp hu
      simp only [List.any_eq_true, Bool.and_eq_true, beq_iff_eq] at hany
      obtain ⟨⟨v, p⟩, hvp, hu', hp⟩ := hany
      exact .inl ⟨v, hp ▸ hvp, hu'⟩


/-! ## What a path keeps through a statement -/

/-- Two states of the same variables, the same bindings in the same
order. -/
inductive MSt.Same : MSt → MSt → Prop where
  | nil : MSt.Same [] []
  | cons {w v : MVar} {τ σ : MSt} : w.uid = v.uid → w.owns = v.owns → MSt.Same τ σ → MSt.Same (w :: τ) (v :: σ)

theorem MSt.Same.of_below : ∀ {τ σ : MSt}, τ.Below σ → τ.Same σ
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons b t => .cons b.2.1 b.2.2.1 (MSt.Same.of_below t)

theorem MSt.Same.trans : ∀ {a b c : MSt}, a.Same b → b.Same c → a.Same c
  | [], [], [], .nil, .nil => .nil
  | _ :: _, _ :: _, _ :: _, .cons u1 o1 t1, .cons u2 o2 t2 => .cons (u1.trans u2) (o1.trans o2) (MSt.Same.trans t1 t2)

theorem MSt.Same.length : ∀ {a b : MSt}, a.Same b → a.length = b.length
  | [], [], .nil => rfl
  | _ :: _, _ :: _, .cons _ _ t => by simp [MSt.Same.length t]

theorem MSt.Same.refl : ∀ (σ : MSt), MSt.Same σ σ
  | [] => .nil
  | _ :: σ => .cons rfl rfl (MSt.Same.refl σ)

theorem MSt.Same.update {f : MVar → MVar} (hf : ∀ v, (f v).uid = v.uid ∧ (f v).owns = v.owns) (x : String) :
    ∀ (σ : MSt), (MSt.update f x σ).Same σ
  | [] => .nil
  | v :: σ => by
    simp only [MSt.update]
    split
    · exact .cons (hf v).1 (hf v).2 (MSt.Same.refl σ)
    · exact .cons rfl rfl (MSt.Same.update hf x σ)

theorem MSt.Same.mem : ∀ {τ σ : MSt}, τ.Same σ → ∀ {w}, w ∈ τ → ∃ v, v ∈ σ ∧ w.uid = v.uid ∧ w.owns = v.owns
  | [], [], .nil, _, h => by simp at h
  | _ :: _, _ :: _, .cons u o t, _, h => by
    simp only [List.mem_cons] at h
    rcases h with rfl | h
    · exact ⟨_, .head _, u, o⟩
    · obtain ⟨v, hv, a, b⟩ := MSt.Same.mem t h; exact ⟨v, .tail _ hv, a, b⟩

theorem MSt.Same.take (n : Nat) : ∀ {τ σ : MSt}, τ.Same σ → MSt.Same (List.take n τ) (List.take n σ)
  | [], [], .nil => by simp; exact .nil
  | _ :: _, _ :: _, .cons u o t => by
    cases n with
    | zero => simp; exact .nil
    | succ n => simpa using .cons u o (MSt.Same.take n t)

/-- A step that keeps the variables, perhaps changing what holds a value:
the path's state after it as `delta` says. -/
theorem Rel.same {F st st' : DSt} {U : Nat} (hs : ∀ u, u ∈ st.held → u ∈ st'.held) :
    ∀ {σ σ' : MSt} {π : List Bool}, Rel F st U σ π → σ'.Same σ → Rel F st' U σ' (delta σ σ' π)
  | [], [], [], .nil, .nil => .nil
  | v :: σ, w :: σ', p :: π, .cons a b t, .cons u o bt => by
    simp only [delta]
    refine .cons ?_ ?_ (Rel.same hs t bt)
    · intro hw
      split
      · next e => simp only [beq_iff_eq] at e; exact a (e ▸ hw)
      · exact hw
    · intro hp
      split at hp
      · next e =>
        rcases b hp with h | d
        · left; simp only [beq_iff_eq] at e; rw [← e]; exact h
        · right
          exact ⟨o ▸ d.1, u ▸ d.2.1, u ▸ d.2.2.1, u ▸ d.2.2.2.imp (hs _) id⟩
      · exact .inl hp

/-- What a path carries through a statement: its state related to the
walk's, every owned binding made at or before the statement, the bindings
of the innermost loop's body numbered from `U`, and every owned binding
numbered below the walk's next. -/
structure Inv (F st : DSt) (U base : Nat) (σ : MSt) (π : List Bool) : Prop where
  rel : Rel F st U σ π
  made : ∀ v, v ∈ σ → v.owns = true → ∃ m, st.madeAt.lookup v.uid = some m ∧ m ≤ st.next
  scope : ∀ v, v ∈ List.take (σ.length - base) σ → v.owns = true → U ≤ v.uid
  lt : ∀ v, v ∈ σ → v.owns = true → v.uid < st.uid
  ule : U ≤ st.uid
  base : base ≤ σ.length

theorem Inv.ext {F st st' : DSt} {U base : Nat} {σ : MSt} {π : List Bool} (h : Inv F st U base σ π)
    (x : SchedExt st st') : Inv F st' U base σ π :=
  ⟨h.rel.mono x.held (Nat.le_refl _),
    fun v hv ho => let ⟨m, hm, hle⟩ := h.made v hv ho; ⟨m, x.made _ m hm, Nat.le_trans hle x.next⟩,
    h.scope, fun v hv ho => Nat.lt_of_lt_of_le (h.lt v hv ho) x.uid, Nat.le_trans h.ule x.uid, h.base⟩

theorem Inv.same {F st st' : DSt} {U base : Nat} {σ σ' : MSt} {π : List Bool} (h : Inv F st U base σ π)
    (x : SchedExt st st') (s : σ'.Same σ) : Inv F st' U base σ' (delta σ σ' π) := by
  have h' := h.ext x
  refine ⟨Rel.same x.held h.rel s, fun v hv ho => ?_, fun v hv ho => ?_, fun v hv ho => ?_, h'.ule, ?_⟩
  · obtain ⟨w, hw, u, o⟩ := s.mem hv; exact u ▸ h'.made w hw (o ▸ ho)
  · have hl := s.length
    rw [hl] at hv
    obtain ⟨w, hw, u, o⟩ := (s.take (σ.length - base)).mem hv
    exact u ▸ h'.scope w hw (o ▸ ho)
  · obtain ⟨w, hw, u, o⟩ := s.mem hv; exact u ▸ h'.lt w hw (o ▸ ho)
  · rw [s.length]; exact h.base

/-- The made-before fact a point needs, at the statement `st.next`
numbers. -/
theorem Inv.madeBefore {F st : DSt} {U base : Nat} {σ : MSt} {π : List Bool} (h : Inv F st U base σ π)
    (x : SchedExt st F) : ∀ v, v ∈ σ → v.owns = true → F.made v.uid ≤ st.next := by
  intro v hv ho
  obtain ⟨m, hm, hle⟩ := h.made v hv ho
  simp only [DSt.made, x.made _ m hm, Option.getD_some]
  exact hle

end Datalove.Fun
