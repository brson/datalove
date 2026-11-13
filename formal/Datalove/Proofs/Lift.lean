import Datalove.WF
import Datalove.Hint

/-!
# A closed type is well formed wherever it is put

An alias names a closed, well-formed type, and the conversion puts it
wherever the alias is used: under `rec`s, inside collections, in a key. It
stays well formed there (`Ty.wfIn_lift`), so long as it has no `rec` in it if
it is in a key, and no type parameter in it if it is under a `rec`, which is
what the conversion checks of an alias (T065, T066). And being closed, it
mentions no `rec` around it (`Ty.mentions_closed`).
-/

namespace Datalove

theorem lookup_mem_of {α β} [BEq α] [LawfulBEq α] {n : α} {b : β} :
    ∀ {l : List (α × β)}, l.lookup n = some b → (n, b) ∈ l
  | [], h => by simp at h
  | (m, c) :: l, h => by
    by_cases e : n = m
    · subst e; simp [List.lookup] at h; subst h; exact List.mem_cons_self
    · have : (n == m) = false := by simpa using e
      simp only [List.lookup, this] at h
      exact List.mem_cons_of_mem _ (lookup_mem_of h)

/-- A context `c` of a type's own, put inside the context `d`. -/
def WfCtx.lift (d c : WfCtx) : WfCtx :=
  { binders := c.binders.map (· + d.colls) ++ d.binders, colls := c.colls + d.colls,
    inKey := c.inKey || d.inKey }

theorem WfCtx.lift_coll (d c : WfCtx) (key : Bool) : d.lift (c.coll key) = (d.lift c).coll key := by
  simp only [WfCtx.lift, WfCtx.coll, WfCtx.mk.injEq, true_and]
  constructor
  · omega
  · cases c.inKey <;> cases key <;> cases d.inKey <;> rfl

theorem WfCtx.lift_bind (d c : WfCtx) :
    d.lift { c with binders := c.colls :: c.binders } =
      { d.lift c with binders := (d.lift c).colls :: (d.lift c).binders } := by
  simp [WfCtx.lift]

mutual

theorem Ty.wfIn_lift (d : WfCtx) : ∀ (t : Ty) (c : WfCtx),
    (d.inKey = true → t.containsRec = false) → (d.binders ≠ [] → t.containsVar = false) →
    t.wfIn c = true → t.wfIn (d.lift c) = true
  | .recvar i, c, hk, _, h => by
    simp only [Ty.wfIn] at h ⊢
    cases hb : c.binders[i]? with
    | none => simp [hb] at h
    | some at_ =>
      simp only [hb, Bool.and_eq_true, Bool.not_eq_true', decide_eq_true_eq] at h
      have hi : i < c.binders.length := by
        rcases Nat.lt_or_ge i c.binders.length with hi | hi
        · exact hi
        · simp [List.getElem?_eq_none hi] at hb
      have hd : d.inKey = false := by
        cases hdk : d.inKey
        · rfl
        · have := hk hdk; simp [Ty.containsRec] at this
      have hl : (c.binders.map (· + d.colls) ++ d.binders)[i]? = some (at_ + d.colls) := by
        rw [List.getElem?_append_left (by simpa using hi), List.getElem?_map, hb]; rfl
      simp only [WfCtx.lift, hl, h.1, hd, Bool.or_false, Bool.not_false, Bool.true_and,
        decide_eq_true_eq]
      omega
  | .mu b, c, hk, hv, h => by
    simp only [Ty.wfIn, Bool.and_eq_true, Bool.not_eq_true'] at h ⊢
    have hd : d.inKey = false := by
      cases hdk : d.inKey
      · rfl
      · have := hk hdk; simp [Ty.containsRec] at this
    refine ⟨⟨by simp [WfCtx.lift, h.1.1, hd], h.1.2⟩, ?_⟩
    rw [← WfCtx.lift_bind]
    exact Ty.wfIn_lift d b _ (fun x => by simp [Ty.containsRec] at hk; exact absurd x (by simp [hd]))
      (fun x => by simpa [Ty.containsVar] using hv x) h.2
  | .var n, c, _, hv, h => by
    simp only [Ty.wfIn, List.isEmpty_iff] at h ⊢
    cases hdb : d.binders with
    | nil => simp [WfCtx.lift, h, hdb]
    | cons _ _ => have := hv (by simp [hdb]); simp [Ty.containsVar] at this
  | .list t, c, hk, hv, h | .tensor t _, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    rw [← WfCtx.lift_coll]
    exact Ty.wfIn_lift d t _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .set t, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    rw [← WfCtx.lift_coll]
    exact Ty.wfIn_lift d t _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .map k v, c, hk, hv, h => by
    simp only [Ty.wfIn, Bool.and_eq_true] at h ⊢
    rw [← WfCtx.lift_coll, ← WfCtx.lift_coll]
    exact ⟨Ty.wfIn_lift d k _ (fun x => by simp [Ty.containsRec] at hk; exact (hk x).1)
        (fun x => by simp [Ty.containsVar] at hv; exact (hv x).1) h.1,
      Ty.wfIn_lift d v _ (fun x => by simp [Ty.containsRec] at hk; exact (hk x).2)
        (fun x => by simp [Ty.containsVar] at hv; exact (hv x).2) h.2⟩
  | .option t, c, hk, hv, h | .result t, c, hk, hv, h | .term _ t, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    exact Ty.wfIn_lift d t _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .tuple ts, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    exact Tys.wfIn_lift d ts _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .struct fs, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    exact Fields.wfIn_lift d fs _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .table fs, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    exact Fields.wfColumnsIn_lift d fs _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .enum vs, c, hk, hv, h => by
    simp only [Ty.wfIn] at h ⊢
    exact Variants.wfIn_lift d vs _ (fun x => by simpa [Ty.containsRec] using hk x)
      (fun x => by simpa [Ty.containsVar] using hv x) h
  | .bool, _, _, _, _ | .u8, _, _, _, _ | .i8, _, _, _, _ | .u16, _, _, _, _ | .i16, _, _, _, _
  | .u32, _, _, _, _ | .i32, _, _, _, _ | .u64, _, _, _, _ | .i64, _, _, _, _ | .index, _, _, _, _
  | .offset, _, _, _, _ | .f32, _, _, _, _ | .f64, _, _, _, _ | .int, _, _, _, _
  | .string, _, _, _, _ | .data, _, _, _, _ | .error, _, _, _, _ | .atom _, _, _, _, _ => by
    simp [Ty.wfIn]

theorem Tys.wfIn_lift (d : WfCtx) : ∀ (ts : Tys) (c : WfCtx),
    (d.inKey = true → ts.containsRec = false) → (d.binders ≠ [] → ts.containsVar = false) →
    ts.wfIn c = true → ts.wfIn (d.lift c) = true
  | .nil, _, _, _, _ => rfl
  | .cons t ts, c, hk, hv, h => by
    simp only [Tys.wfIn, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.wfIn_lift d t c (fun x => by simp [Tys.containsRec] at hk; exact (hk x).1)
        (fun x => by simp [Tys.containsVar] at hv; exact (hv x).1) h.1,
      Tys.wfIn_lift d ts c (fun x => by simp [Tys.containsRec] at hk; exact (hk x).2)
        (fun x => by simp [Tys.containsVar] at hv; exact (hv x).2) h.2⟩

theorem Fields.wfIn_lift (d : WfCtx) : ∀ (fs : Fields) (c : WfCtx),
    (d.inKey = true → fs.containsRec = false) → (d.binders ≠ [] → fs.containsVar = false) →
    fs.wfIn c = true → fs.wfIn (d.lift c) = true
  | .nil, _, _, _, _ => rfl
  | .cons _ t fs, c, hk, hv, h => by
    simp only [Fields.wfIn, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.wfIn_lift d t c (fun x => by simp [Fields.containsRec] at hk; exact (hk x).1)
        (fun x => by simp [Fields.containsVar] at hv; exact (hv x).1) h.1,
      Fields.wfIn_lift d fs c (fun x => by simp [Fields.containsRec] at hk; exact (hk x).2)
        (fun x => by simp [Fields.containsVar] at hv; exact (hv x).2) h.2⟩

theorem Fields.wfColumnsIn_lift (d : WfCtx) : ∀ (fs : Fields) (c : WfCtx),
    (d.inKey = true → fs.containsRec = false) → (d.binders ≠ [] → fs.containsVar = false) →
    fs.wfColumnsIn c = true → fs.wfColumnsIn (d.lift c) = true
  | .nil, _, _, _, _ => rfl
  | .cons _ t fs, c, hk, hv, h => by
    simp only [Fields.wfColumnsIn, Bool.and_eq_true] at h ⊢
    rw [← WfCtx.lift_coll]
    exact ⟨Ty.wfIn_lift d t _ (fun x => by simp [Fields.containsRec] at hk; exact (hk x).1)
        (fun x => by simp [Fields.containsVar] at hv; exact (hv x).1) h.1,
      Fields.wfColumnsIn_lift d fs c (fun x => by simp [Fields.containsRec] at hk; exact (hk x).2)
        (fun x => by simp [Fields.containsVar] at hv; exact (hv x).2) h.2⟩

theorem Variants.wfIn_lift (d : WfCtx) : ∀ (vs : Variants) (c : WfCtx),
    (d.inKey = true → vs.containsRec = false) → (d.binders ≠ [] → vs.containsVar = false) →
    vs.wfIn c = true → vs.wfIn (d.lift c) = true
  | .nil, _, _, _, _ => rfl
  | .atom _ vs, c, hk, hv, h => by
    simp only [Variants.wfIn] at h ⊢
    exact Variants.wfIn_lift d vs c (fun x => by simpa [Variants.containsRec] using hk x)
      (fun x => by simpa [Variants.containsVar] using hv x) h
  | .term _ t vs, c, hk, hv, h => by
    simp only [Variants.wfIn, Bool.and_eq_true] at h ⊢
    exact ⟨Ty.wfIn_lift d t c (fun x => by simp [Variants.containsRec] at hk; exact (hk x).1)
        (fun x => by simp [Variants.containsVar] at hv; exact (hv x).1) h.1,
      Variants.wfIn_lift d vs c (fun x => by simp [Variants.containsRec] at hk; exact (hk x).2)
        (fun x => by simp [Variants.containsVar] at hv; exact (hv x).2) h.2⟩

end

theorem WfCtx.lift_empty (d : WfCtx) : d.lift {} = d := by
  simp [WfCtx.lift]

/-- A closed, well-formed type is well formed in any context that it has no
`rec` in a key of, and no type parameter under a `rec` of. -/
theorem Ty.WF.lift {t : Ty} (h : t.WF) (d : WfCtx) (hk : d.inKey = true → t.containsRec = false)
    (hv : d.binders ≠ [] → t.containsVar = false) : t.wfIn d = true := by
  have := Ty.wfIn_lift d t {} hk hv h
  rwa [WfCtx.lift_empty] at this

mutual

/-- A type well formed in a context mentions no binder outside it. -/
theorem Ty.mentions_closed : ∀ (t : Ty) (c : WfCtx) (j : Nat), t.wfIn c = true →
    c.binders.length ≤ j → t.mentions j = false
  | .recvar i, c, j, h, hj => by
    simp only [Ty.wfIn] at h
    simp only [Ty.mentions, beq_eq_false_iff_ne]
    intro e; subst e
    simp [List.getElem?_eq_none hj] at h
  | .mu b, c, j, h, hj => by
    simp only [Ty.wfIn, Bool.and_eq_true] at h
    simp only [Ty.mentions]
    exact Ty.mentions_closed b _ (j + 1) h.2 (by simp; omega)
  | .list t, c, j, h, hj | .set t, c, j, h, hj | .tensor t _, c, j, h, hj => by
    simp only [Ty.wfIn] at h; simp only [Ty.mentions]
    exact Ty.mentions_closed t _ j h (by simpa [WfCtx.coll] using hj)
  | .option t, c, j, h, hj | .result t, c, j, h, hj | .term _ t, c, j, h, hj => by
    simp only [Ty.wfIn] at h; simp only [Ty.mentions]; exact Ty.mentions_closed t c j h hj
  | .map k v, c, j, h, hj => by
    simp only [Ty.wfIn, Bool.and_eq_true] at h; simp only [Ty.mentions, Bool.or_eq_false_iff]
    exact ⟨Ty.mentions_closed k _ j h.1 (by simpa [WfCtx.coll] using hj),
      Ty.mentions_closed v _ j h.2 (by simpa [WfCtx.coll] using hj)⟩
  | .tuple ts, c, j, h, hj => by
    simp only [Ty.wfIn] at h; simp only [Ty.mentions]; exact Tys.mentions_closed ts c j h hj
  | .struct fs, c, j, h, hj => by
    simp only [Ty.wfIn] at h; simp only [Ty.mentions]; exact Fields.mentions_closed fs c j h hj
  | .table fs, c, j, h, hj => by
    simp only [Ty.wfIn] at h; simp only [Ty.mentions]; exact Fields.mentions_closedColumns fs c j h hj
  | .enum vs, c, j, h, hj => by
    simp only [Ty.wfIn] at h; simp only [Ty.mentions]; exact Variants.mentions_closed vs c j h hj
  | .var _, _, _, _, _ | .bool, _, _, _, _ | .u8, _, _, _, _ | .i8, _, _, _, _ | .u16, _, _, _, _
  | .i16, _, _, _, _ | .u32, _, _, _, _ | .i32, _, _, _, _ | .u64, _, _, _, _ | .i64, _, _, _, _
  | .index, _, _, _, _ | .offset, _, _, _, _ | .f32, _, _, _, _ | .f64, _, _, _, _
  | .int, _, _, _, _ | .string, _, _, _, _ | .data, _, _, _, _ | .error, _, _, _, _
  | .atom _, _, _, _, _ => by simp [Ty.mentions]

theorem Tys.mentions_closed : ∀ (ts : Tys) (c : WfCtx) (j : Nat), ts.wfIn c = true →
    c.binders.length ≤ j → ts.mentions j = false
  | .nil, _, _, _, _ => rfl
  | .cons t ts, c, j, h, hj => by
    simp only [Tys.wfIn, Bool.and_eq_true] at h; simp only [Tys.mentions, Bool.or_eq_false_iff]
    exact ⟨Ty.mentions_closed t c j h.1 hj, Tys.mentions_closed ts c j h.2 hj⟩

theorem Fields.mentions_closed : ∀ (fs : Fields) (c : WfCtx) (j : Nat), fs.wfIn c = true →
    c.binders.length ≤ j → fs.mentions j = false
  | .nil, _, _, _, _ => rfl
  | .cons _ t fs, c, j, h, hj => by
    simp only [Fields.wfIn, Bool.and_eq_true] at h; simp only [Fields.mentions, Bool.or_eq_false_iff]
    exact ⟨Ty.mentions_closed t c j h.1 hj, Fields.mentions_closed fs c j h.2 hj⟩

theorem Fields.mentions_closedColumns : ∀ (fs : Fields) (c : WfCtx) (j : Nat),
    fs.wfColumnsIn c = true → c.binders.length ≤ j → fs.mentions j = false
  | .nil, _, _, _, _ => rfl
  | .cons _ t fs, c, j, h, hj => by
    simp only [Fields.wfColumnsIn, Bool.and_eq_true] at h
    simp only [Fields.mentions, Bool.or_eq_false_iff]
    exact ⟨Ty.mentions_closed t _ j h.1 (by simpa [WfCtx.coll] using hj),
      Fields.mentions_closedColumns fs c j h.2 hj⟩

theorem Variants.mentions_closed : ∀ (vs : Variants) (c : WfCtx) (j : Nat), vs.wfIn c = true →
    c.binders.length ≤ j → vs.mentions j = false
  | .nil, _, _, _, _ => rfl
  | .atom _ vs, c, j, h, hj => by
    simp only [Variants.wfIn] at h; simp only [Variants.mentions]
    exact Variants.mentions_closed vs c j h hj
  | .term _ t vs, c, j, h, hj => by
    simp only [Variants.wfIn, Bool.and_eq_true] at h
    simp only [Variants.mentions, Bool.or_eq_false_iff]
    exact ⟨Ty.mentions_closed t c j h.1 hj, Variants.mentions_closed vs c j h.2 hj⟩

end

end Datalove
