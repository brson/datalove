import Datalove.Proofs.DropPath
import Datalove.Proofs.DropLen

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

theorem Rel.mem_zip {F st : DSt} {H : Nat → Prop} : ∀ {σ : MSt} {π : List Bool}, Rel F st H σ π →
    ∀ {v : MVar} {p : Bool}, (v, p) ∈ σ.zip π → (v.live = true → p = true) ∧ (p = true → v.owns = true → v.live = true ∨ Div F st H v)
  | [], [], .nil, _, _, h => by simp at h
  | _ :: _, _ :: _, .cons a b t, _, _, h => by
    simp only [List.zip_cons_cons, List.mem_cons, Prod.mk.injEq] at h
    rcases h with ⟨rfl, rfl⟩ | h
    · exact ⟨a, b⟩
    · exact Rel.mem_zip t h

theorem Rel.exists_zip {F st : DSt} {H : Nat → Prop} : ∀ {σ : MSt} {π : List Bool}, Rel F st H σ π →
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
theorem recEv_good {kind : String} {id : Nat} {arm : Option Nat} {vs : MSt} {πs : List Bool} {always : Bool}
    {st0 st1 st2 F : DSt} {H : Nat → Prop}
    (hr : (record kind id arm (ownedOf st1.held vs) always).run st1 = .ok ((), st2))
    (x : SchedExt st2 F) (w1 : st1.WF) (rel : Rel F st0 H vs πs)
    (cover : (always = true ∧ exitKind kind = true ∧ ∀ v, v ∈ vs → v.owns = true → F.made v.uid ≤ id) ∨
      ((∀ u, u ∈ st0.held → u ∈ st1.held) ∧ ∀ v, v ∈ vs → v.owns = true → ¬ H v.uid)) :
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
        (kind, id, arm, (ownedOf st1.held vs).map (·, none)) := by
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
      rcases hd' hp ho' with hl' | ⟨_, hF, _, hs0⟩
      · exact inE _ (mem_ownedOf.mpr ⟨v, hv, ho', .inl hl', rfl⟩)
      · rcases cover with ⟨_, hk', hmade⟩ | ⟨hheld, hU⟩
        · by_cases hs : v.uid ∈ st1.held
          · exact inE _ (mem_ownedOf.mpr ⟨v, hv, ho', .inr hs, rfl⟩)
          · exact patched (by simpa using hk') v.uid hF (hh ▸ hs) (hmade v hv ho')
        · rcases hs0 with hs | hlt
          · exact inE _ (mem_ownedOf.mpr ⟨v, hv, ho', .inr (hheld _ hs), rfl⟩)
          · exact absurd hlt (hU v hv ho')
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
    rcases cover with ⟨hal, _, _⟩ | ⟨hheld, hU⟩
    · rw [hal] at ha; exact ha rfl
    · rcases hd' hp ho' with hl' | ⟨_, _, _, hs | hlt⟩
      · exact none _ (mem_ownedOf.mpr ⟨v, hv, ho', .inl hl', rfl⟩)
      · exact none _ (mem_ownedOf.mpr ⟨v, hv, ho', .inr (hheld _ hs), rfl⟩)
      · exact absurd hlt (hU v hv ho')


/-- Owned bindings of a state, each a number of its own. -/
def OwnedNodup (σ : MSt) : Prop := List.Pairwise (fun v w => v.owns = true → w.owns = true → v.uid ≠ w.uid) σ

/-- What a return from partway along a step finds, position by position: the
step from `σ` to `σ'`, the path holding `π` before it and `πk` where it
returns. -/
theorem between_facts {F st : DSt} {H : Nat → Prop} : ∀ {σ σ' : MSt} {π πk : List Bool},
    σ'.Below σ → Rel F st H σ π → Between (delta σ σ' π) π πk →
    (∀ v p, (v, p) ∈ σ.zip πk → ∃ v', v' ∈ σ' ∧ v'.uid = v.uid ∧ v'.owns = v.owns ∧
      (p = true → v.owns = true → v.live = true ∨ Div F st H v) ∧ (v'.live = true → p = true)) ∧
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
      · exact ⟨w, .head _, bw.2.1, bw.2.2.1, fun hp ho => b (hi hp) ho, wl⟩
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
theorem tryEv_good {id : Nat} {σ σ' : MSt} {m : MoveM MSt} (hm : Shr σ m) {st0 st1 st2 F : DSt} {H : Nat → Prop}
    {π πk : List Bool} (hl : (leavingIf id σ true m).run st1 = .ok (σ', st2))
    (x : SchedExt st2 F) (w1 : st1.WF) (rel : Rel F st0 H σ π)
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
    rcases hdiv hp ho' with hvl | ⟨_, hF, _, hs | hlt⟩
    · by_cases hw : v'.live = true
      · exact List.mem_append_left _ (inB _ (hdrops ▸ mem_ownedOf.mpr ⟨v', hv', o' ▸ ho', .inl hw, u'⟩))
      · by_cases hd : v.uid ∈ drops
        · exact List.mem_append_left _ (inB _ hd)
        · have hm' := movedMem v.uid (mem_ownedOf.mpr ⟨v, hv, ho', .inl hvl, rfl⟩) hd
          have hne : moved ≠ [] := fun e => by rw [e] at hm'; simp at hm'
          refine List.mem_append_right _ (List.mem_filter.mpr ⟨nym hne _ hm', ?_⟩)
          simp only [List.any_eq_true, Bool.and_eq_true, beq_iff_eq]
          exact ⟨(v, p), hvp, rfl, hp⟩
    · by_cases hs' : v.uid ∈ st1.held
      · exact List.mem_append_left _ (inB _ (hdrops ▸ mem_ownedOf.mpr ⟨v', hv', o' ▸ ho', .inr hs', u'⟩))
      · exact List.mem_append_left _ (patched (by simp [exitKind]) v.uid hF (hh ▸ hs') (hmade v hv ho'))
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

theorem MSt.Same.symm : ∀ {a b : MSt}, a.Same b → b.Same a
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons u o t => .cons u.symm o.symm (MSt.Same.symm t)

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
theorem Rel.same {F st st' : DSt} {H : Nat → Prop} (hs : ∀ u, u ∈ st.held → u ∈ st'.held) :
    ∀ {σ σ' : MSt} {π : List Bool}, Rel F st H σ π → σ'.Same σ → Rel F st' H σ' (delta σ σ' π)
  | [], [], [], .nil, .nil => .nil
  | v :: σ, w :: σ', p :: π, .cons a b t, .cons u o bt => by
    simp only [delta]
    refine .cons ?_ ?_ (Rel.same hs t bt)
    · intro hw
      split
      · next e => simp only [beq_iff_eq] at e; exact a (e ▸ hw)
      · exact hw
    · intro hp ho
      split at hp
      · next e =>
        rcases b hp (o ▸ ho) with h | d
        · left; simp only [beq_iff_eq] at e; rw [← e]; exact h
        · right
          exact ⟨o ▸ d.1, u ▸ d.2.1, u ▸ d.2.2.1, u ▸ d.2.2.2.imp (hs _) id⟩
      · exact .inl hp

/-- What a path carries through a statement: its state related to the
walk's, every owned binding made at or before the statement, the bindings
of the innermost loop's body numbered from `U`, and every owned binding
numbered below the walk's next. -/
structure Inv (F st : DSt) (U : Nat) (H : Nat → Prop) (base : Nat) (σ : MSt) (π : List Bool) : Prop where
  rel : Rel F st H σ π
  hU : ∀ u, H u → u < U
  made : ∀ v, v ∈ σ → v.owns = true → ∃ m, st.madeAt.lookup v.uid = some m ∧ m ≤ st.next
  scope : ∀ v, v ∈ List.take (σ.length - base) σ → v.owns = true → U ≤ v.uid
  lt : ∀ v, v ∈ σ → v.owns = true → v.uid < st.uid
  ule : U ≤ st.uid
  base : base ≤ σ.length

theorem Inv.ext {F st st' : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ : MSt} {π : List Bool} (h : Inv F st U H base σ π)
    (x : SchedExt st st') : Inv F st' U H base σ π :=
  ⟨h.rel.mono x.held (fun _ h => h), h.hU,
    fun v hv ho => let ⟨m, hm, hle⟩ := h.made v hv ho; ⟨m, x.made _ m hm, Nat.le_trans hle x.next⟩,
    h.scope, fun v hv ho => Nat.lt_of_lt_of_le (h.lt v hv ho) x.uid, Nat.le_trans h.ule x.uid, h.base⟩

theorem Inv.same {F st st' : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ σ' : MSt} {π : List Bool} (h : Inv F st U H base σ π)
    (x : SchedExt st st') (s : σ'.Same σ) : Inv F st' U H base σ' (delta σ σ' π) := by
  have h' := h.ext x
  refine ⟨Rel.same x.held h.rel s, h.hU, fun v hv ho => ?_, fun v hv ho => ?_, fun v hv ho => ?_, h'.ule, ?_⟩
  · obtain ⟨w, hw, u, o⟩ := s.mem hv; exact u ▸ h'.made w hw (o ▸ ho)
  · have hl := s.length
    rw [hl] at hv
    obtain ⟨w, hw, u, o⟩ := (s.take (σ.length - base)).mem hv
    exact u ▸ h'.scope w hw (o ▸ ho)
  · obtain ⟨w, hw, u, o⟩ := s.mem hv; exact u ▸ h'.lt w hw (o ▸ ho)
  · rw [s.length]; exact h.base

/-- The made-before fact a point needs, at the statement `st.next`
numbers. -/
theorem Inv.madeBefore {F st : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ : MSt} {π : List Bool} (h : Inv F st U H base σ π)
    (x : SchedExt st F) : ∀ v, v ∈ σ → v.owns = true → F.made v.uid ≤ st.next := by
  intro v hv ho
  obtain ⟨m, hm, hle⟩ := h.made v hv ho
  simp only [DSt.made, x.made _ m hm, Option.getD_some]
  exact hle


theorem stmtId_eq {st st1 : DSt} {id : Nat} (h : stmtId.run st = .ok (id, st1)) :
    id = st.next ∧ st1 = { st with next := st.next + 1 } := by
  simp only [stmtId, modifyGet, MonadStateOf.modifyGet, StateT.modifyGet, StateT.run] at h
  cases h; exact ⟨rfl, rfl⟩

/-- What `bindM` makes: new bindings before the old, each live, numbered
from where the walk was, and made at the statement it is in. -/
theorem bindM_spec {Γ' : Ctx} {xs : List String} {init writable : Bool} :
    ∀ {σ σ' : MSt} {st st' : DSt}, st.WF → (bindM Γ' σ xs init writable).run st = .ok (σ', st') →
      ∃ new, σ' = new ++ σ ∧
        (∀ v, v ∈ new → v.live = true ∧ st.uid ≤ v.uid ∧ v.uid < st'.uid ∧
          st'.madeAt.lookup v.uid = some st.next) ∧
        st'.next = st.next ∧ st'.out = st.out ∧ st'.held = st.held ∧ st'.tracked = st.tracked ∧
        st.uid ≤ st'.uid := by
  induction xs with
  | nil =>
    intro σ σ' st st' _ h
    cases h
    refine ⟨[], rfl, ?_, rfl, rfl, rfl, rfl, Nat.le_refl _⟩
    intro v hv; cases hv
  | cons x xs ih =>
    intro σ σ' st st' w h
    unfold bindM at h
    simp only [List.foldlM] at h
    obtain ⟨σ1, st1, h1, h2⟩ := MoveM.bind_ok h
    have x1 := SchedGrows.bindOne Γ' init false writable false σ x st σ1 st1 w h1
    have x2 := SchedGrows.bindM Γ' σ1 xs init writable st1 σ' st' x1.wf h2
    have e1 : ∃ v, σ1 = v :: σ ∧ v.live = true ∧ v.uid = st.uid ∧ st1.uid = st.uid + 1 ∧
        st1.next = st.next ∧ st1.madeAt.lookup st.uid = some st.next ∧ st1.out = st.out ∧
        st1.held = st.held ∧ st1.tracked = st.tracked := by
      simp only [bindOne, MoveM.run_bind] at h1
      cases h1
      exact ⟨_, rfl, rfl, rfl, rfl, rfl, by simp [List.lookup], rfl, rfl, rfl⟩
    obtain ⟨v0, e0, l0, u0, su, sn, sm, so, sh, stk⟩ := e1
    obtain ⟨new, e, hn, nx, no, nh, nt, nu⟩ := ih x1.wf h2
    refine ⟨new ++ [v0], by rw [e, e0]; simp, fun v hv => ?_, nx.trans sn, no.trans so, nh.trans sh,
      nt.trans stk, by omega⟩
    rcases List.mem_append.mp hv with hv | hv
    · obtain ⟨l, u1, u2, mm⟩ := hn v hv
      exact ⟨l, by omega, u2, sn ▸ mm⟩
    · simp only [List.mem_singleton] at hv
      subst hv
      exact ⟨l0, u0 ▸ Nat.le_refl _, by omega, u0 ▸ x2.made _ _ sm⟩

/-- Bindings made: the path holds each, and keeps what it carries. -/
theorem Inv.bind {F st st' : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ σ' : MSt} {π : List Bool} {Γ' : Ctx}
    {xs : List String} {init writable : Bool} (h : Inv F st U H base σ π) (w : st.WF)
    (hb : (bindM Γ' σ xs init writable).run st = .ok (σ', st')) :
    Inv F st' U H base σ' (newFlags σ σ' ++ π) := by
  obtain ⟨new, e, hn, nx, _, nh, _, nu⟩ := bindM_spec w hb
  have x := SchedGrows.bindM Γ' σ xs init writable st σ' st' w hb
  have h' := h.ext x
  subst e
  have flags : newFlags σ (new ++ σ) = new.map (·.live) := by
    simp [newFlags, scopeOwn]
  rw [flags]
  have relNew : ∀ (l : MSt), (∀ v, v ∈ l → v.live = true) → Rel F st' H l (l.map (·.live)) := by
    intro l hl
    induction l with
    | nil => exact .nil
    | cons v l ih' =>
      exact .cons (fun hv => hv) (fun hp _ => .inl hp) (ih' (fun u hu => hl u (.tail _ hu)))
  refine ⟨Rel.append (relNew new (fun v hv => (hn v hv).1)) h'.rel, h.hU, fun v hv ho => ?_,
    fun v hv ho => ?_, fun v hv ho => ?_, h'.ule, by simp; exact Nat.le_trans h.base (Nat.le_add_left _ _)⟩
  · rcases List.mem_append.mp hv with hv | hv
    · exact ⟨st.next, (hn v hv).2.2.2, nx ▸ Nat.le_refl _⟩
    · exact h'.made v hv ho
  · have hl : (new ++ σ).length - base = new.length + (σ.length - base) := by
      simp; have := h.base; omega
    rw [hl, List.take_length_add_append] at hv
    rcases List.mem_append.mp hv with hv | hv
    · exact Nat.le_trans h.ule (hn v hv).2.1
    · exact h.scope v hv ho
  · rcases List.mem_append.mp hv with hv | hv
    · exact (hn v hv).2.2.1
    · exact h'.lt v hv ho


theorem leavingIf_below {id : Nat} {σ σ' : MSt} {c : Bool} {m : MoveM MSt} (hm : Shr σ m) {st st' : DSt}
    (h : (leavingIf id σ c m).run st = .ok (σ', st')) : σ'.Below σ := by
  cases c
  · exact (hm st σ' st' h).1
  · obtain ⟨stm, hmr, _⟩ := leavingIf_spec hm h
    exact (hm st σ' stm hmr).1


/-! ## Joins keep what a path through a completing branch holds -/

/-- Two states of the same bindings, each as live as the other. -/
inductive MSt.SameLive : MSt → MSt → Prop where
  | nil : MSt.SameLive [] []
  | cons {w v : MVar} {τ σ : MSt} : w.uid = v.uid → w.owns = v.owns → w.live = v.live → MSt.SameLive τ σ →
      MSt.SameLive (w :: τ) (v :: σ)

theorem MSt.SameLive.refl : ∀ (σ : MSt), σ.SameLive σ
  | [] => .nil
  | _ :: σ => .cons rfl rfl rfl (MSt.SameLive.refl σ)

theorem MSt.SameLive.trans : ∀ {a b c : MSt}, a.SameLive b → b.SameLive c → a.SameLive c
  | [], [], [], .nil, .nil => .nil
  | _ :: _, _ :: _, _ :: _, .cons u1 o1 l1 t1, .cons u2 o2 l2 t2 =>
    .cons (u1.trans u2) (o1.trans o2) (l1.trans l2) (MSt.SameLive.trans t1 t2)

theorem MSt.SameLive.length : ∀ {a b : MSt}, a.SameLive b → a.length = b.length
  | [], [], .nil => rfl
  | _ :: _, _ :: _, .cons _ _ _ t => by simp [MSt.SameLive.length t]

theorem MSt.SameLive.same : ∀ {a b : MSt}, a.SameLive b → a.Same b
  | [], [], .nil => .nil
  | _ :: _, _ :: _, .cons u o _ t => .cons u o (MSt.SameLive.same t)

theorem Rel.sameLive {F st : DSt} {H : Nat → Prop} : ∀ {σ' b : MSt} {π : List Bool}, Rel F st H b π →
    σ'.SameLive b → Rel F st H σ' π
  | [], [], [], .nil, .nil => .nil
  | _ :: _, _ :: _, _ :: _, .cons a c t, .cons u o l s =>
    .cons (fun h => a (l ▸ h)) (fun hp ho => by
      rcases c hp (o ▸ ho) with h | d
      · exact .inl (l ▸ h)
      · exact .inr ⟨o ▸ d.1, u ▸ d.2.1, u ▸ d.2.2.1, u ▸ d.2.2.2⟩) (Rel.sameLive t s)

theorem Inv.sameLive {F st : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ' b : MSt} {π : List Bool}
    (h : Inv F st U H base b π) (s : σ'.SameLive b) : Inv F st U H base σ' π := by
  have sm := s.same
  refine ⟨h.rel.sameLive s, h.hU, fun v hv ho => ?_, fun v hv ho => ?_, fun v hv ho => ?_, h.ule, ?_⟩
  · obtain ⟨w, hw, u, o⟩ := sm.mem hv; exact u ▸ h.made w hw (o ▸ ho)
  · rw [sm.length] at hv
    obtain ⟨w, hw, u, o⟩ := (sm.take _).mem hv
    exact u ▸ h.scope w hw (o ▸ ho)
  · obtain ⟨w, hw, u, o⟩ := sm.mem hv; exact u ▸ h.lt w hw (o ▸ ho)
  · rw [sm.length]; exact h.base

theorem sameLive_zipWith {f : MVar → MVar → MVar}
    (hf : ∀ v w, (f v w).uid = v.uid ∧ (f v w).owns = v.owns ∧ (f v w).live = v.live) :
    ∀ (σ τ : MSt), σ.length = τ.length → MSt.SameLive (List.zipWith f σ τ) σ
  | [], [], _ => .nil
  | v :: σ, w :: τ, h => by
    simp only [List.zipWith_cons_cons]
    exact .cons (hf v w).1 (hf v w).2.1 (hf v w).2.2 (sameLive_zipWith hf σ τ (by simpa using h))

theorem sameLive_initFrom {σ τ : MSt} (h : σ.length = τ.length) : (σ.initFrom τ).SameLive σ :=
  sameLive_zipWith (f := fun v w => { v with init := w.init }) (fun _ _ => ⟨rfl, rfl, rfl⟩) σ τ h

theorem sameLive_initMeet {σ τ : MSt} (h : σ.length = τ.length) : (σ.initMeet τ).SameLive σ :=
  sameLive_zipWith (f := fun v w => { v with init := v.init && w.init }) (fun _ _ => ⟨rfl, rfl, rfl⟩) σ τ h

theorem sameMoves_sameLive : ∀ {t e : MSt}, t.Same e → t.sameMoves e = true → t.SameLive e
  | [], [], .nil, _ => .nil
  | _ :: _, _ :: _, .cons u o s, h => by
    simp only [MSt.sameMoves, List.zip_cons_cons, List.all_cons, Bool.and_eq_true, beq_iff_eq] at h
    exact .cons u o h.1 (sameMoves_sameLive s (by simpa [MSt.sameMoves] using h.2))

theorem joinIf_live {t e r : MSt} {tc ec : Bool} {st st' : DSt}
    (h : (joinIf t tc e ec).run st = .ok (r, st')) (s : t.Same e) :
    (tc = true → r.SameLive t) ∧ (ec = true → r.SameLive e) := by
  have hl := s.length
  unfold joinIf at h
  split at h
  · cases h
  · next c =>
    cases h
    have ml : t.length = (t.initMeet e).length := by simp [initMeet_length, hl]
    have ml' : e.length = (t.initMeet e).length := by simp [initMeet_length, hl]
    refine ⟨fun htc => ?_, fun hec => ?_⟩
    · simp only [htc, Bool.not_true, Bool.false_and, Bool.false_eq_true, ite_false]
      exact sameLive_initFrom ml
    · by_cases htc : tc = true
      · simp only [htc, Bool.not_true, Bool.false_and, Bool.false_eq_true, ite_false]
        have hm : t.sameMoves e = true := by
          simp only [htc, hec, Bool.true_and, Bool.not_eq_true'] at c
          simpa using c
        exact (sameLive_initFrom ml).trans (sameMoves_sameLive s hm)
      · simp only [Bool.not_eq_true] at htc
        simp only [htc, hec, Bool.not_false, Bool.and_self, ite_true]
        exact sameLive_initFrom ml'

theorem foldl_initMeet_sameLive : ∀ (ms : List MSt) (m : MSt), (∀ x, x ∈ ms → x.length = m.length) →
    (ms.foldl MSt.initMeet m).SameLive m
  | [], m, _ => MSt.SameLive.refl m
  | n :: ms, m, hl => by
    have h1 : (m.initMeet n).SameLive m := sameLive_initMeet (hl n (.head _)).symm
    exact (foldl_initMeet_sameLive ms (m.initMeet n)
      (fun x hx => by rw [h1.length]; exact hl x (.tail _ hx))).trans h1

theorem joinMatch_live {σ r : MSt} {arms : List (MSt × Bool)} {st st' : DSt}
    (h : (joinMatch σ arms).run st = .ok (r, st')) (hs : ∀ a b, a ∈ arms → b ∈ arms → a.1.Same b.1) :
    ∀ a, a ∈ arms → a.2 = true → r.SameLive a.1 := by
  intro a ha hc
  unfold joinMatch at h
  simp only at h
  have amem : a.1 ∈ (arms.filter (·.2)).map (·.1) := List.mem_map.mpr ⟨a, List.mem_filter.mpr ⟨ha, hc⟩, rfl⟩
  split at h
  · next m ms hm =>
    split at h
    · cases h
    · next c =>
      cases h
      have mem : ∀ x, x ∈ (arms.filter (·.2)).map (·.1) → ∃ b, b ∈ arms ∧ b.1 = x := fun x hx => by
        obtain ⟨b, hb, e⟩ := List.mem_map.mp hx; exact ⟨b, (List.mem_filter.mp hb).1, e⟩
      rw [hm] at mem amem
      obtain ⟨bm, hbm, em⟩ := mem m (.head _)
      have sameTo : ∀ x, x ∈ m :: ms → x.Same m := fun x hx => by
        obtain ⟨bx, hbx, ex⟩ := mem x hx; rw [← ex, ← em]; exact hs bx bm hbx hbm
      have fold := foldl_initMeet_sameLive ms m (fun x hx => (sameTo x (.tail _ hx)).length)
      have hall : ms.all m.sameMoves = true := by simpa using c
      cases amem with
      | head => exact fold
      | tail _ hx =>
        have hm' : m.sameMoves a.1 = true := List.all_eq_true.mp hall a.1 hx
        exact fold.trans (sameMoves_sameLive (sameTo a.1 (.tail _ hx)).symm hm')
  · next hm =>
    rw [hm] at amem; cases amem


/-- Leaving the innermost `k` bindings, all inside the innermost loop: the
path drops as many flags, and keeps what it carries. -/
theorem Inv.drop {F st : DSt} {U : Nat} {H : Nat → Prop} {base : Nat} {σ : MSt} {π : List Bool}
    (h : Inv F st U H base σ π) (k : Nat) (hk : k + base ≤ σ.length) :
    Inv F st U H base (σ.drop k) (π.drop k) := by
  refine ⟨h.rel.drop k, h.hU, fun v hv ho => h.made v (List.mem_of_mem_drop hv) ho, fun v hv ho => ?_,
    fun v hv ho => h.lt v (List.mem_of_mem_drop hv) ho, h.ule, by simp; omega⟩
  rw [List.take_drop] at hv
  have hv' := List.mem_of_mem_drop hv
  have e : k + ((σ.drop k).length - base) = σ.length - base := by simp; omega
  rw [e] at hv'
  exact h.scope v hv' ho

end Datalove.Fun
