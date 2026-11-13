import Datalove.Lit
import Datalove.Proofs.Equiv

/-!
# A literal that checks is a value of its type

`Lit.checkWith_sound`: if a datalit expression checks against a type, the
value it denotes is a value of that type. An integer is in the type's range,
a hex literal fits its bits, a tensor has as many elements as its shape says,
a table's rows have one element per column and its header is the type's, an
atom or term is a variant of the enum it is checked against, and so on down.

What a type's values are is `HasType`, written from the tables in
`botdocs/datalit-typing-rules.md` and `mandocs/types.md`, not from the
checker: its integer ranges are stated afresh (`inRange`), not read off the
checker's `intRange`. A recursive type's values are its unfolding's.

The theorem holds for any synthesis and any Check-Subsume that never
succeeds, which the real one does not (it always ends in T032); so it holds of
`Lit.check`, which is `checkWith` given those. A `data` or `error` payload is
synthesized and then forgotten, as the type forgets it, so `Val.data` and
`Val.error` carry nothing.
-/

namespace Datalove.Lit

/-- The value a literal denotes. A hex literal is kept apart from a decimal
one: it is a bit pattern, which a float type reads as well as an integer
type. -/
inductive Val where
  | bool (b : Bool)
  | int (n : Int)
  | hex (n : Nat)
  | float
  | str
  | tuple (vs : List Val)
  | struct (fs : List (String × Val))
  | list (vs : List Val)
  | set (vs : List Val)
  | map (es : List (Val × Val))
  | tensor (shape : List Nat) (vs : List Val)
  | table (header : List String) (rows : List (List Val))
  | none
  | some (v : Val)
  | ok (v : Val)
  | er (v : Val)
  | data
  | error
  | atom (a : String)
  | term (a : String) (v : Val)

mutual

def eval : Expr → Val
  | .tt => .bool true
  | .ff => .bool false
  | .int n => .int n
  | .hex n => .hex n
  | .float => .float
  | .str => .str
  | .tuple es => .tuple (evalAll es)
  | .struct fs => .struct (evalFields fs)
  | .list es => .list (evalAll es)
  | .set es => .set (evalAll es)
  | .map es => .map (evalEntries es)
  | .tensor shape es => .tensor shape (evalAll es)
  | .table header rows => .table header (evalRows rows)
  | .none => .none
  | .some e => .some (eval e)
  | .ok e => .ok (eval e)
  | .er e => .er (eval e)
  | .data _ => .data
  | .error _ => .error
  | .atom a => .atom a
  | .term a e => .term a (eval e)
  | .enum e | .group e | .hinted _ e => eval e

def evalAll : List Expr → List Val
  | [] => []
  | e :: es => eval e :: evalAll es

def evalFields : List (String × Expr) → List (String × Val)
  | [] => []
  | (n, e) :: fs => (n, eval e) :: evalFields fs

def evalEntries : List (Expr × Expr) → List (Val × Val)
  | [] => []
  | (k, v) :: es => (eval k, eval v) :: evalEntries es

def evalRows : List (List Expr) → List (List Val)
  | [] => []
  | r :: rs => evalAll r :: evalRows rs

end

/-- `n ∈ ⟦I⟧`, the doc's table of the integers each integer type represents,
with `index` and `offset` of `indexBits` bits. False of a type that is not an
integer type. -/
def inRange : Ty → Int → Prop
  | .u8, n => 0 ≤ n ∧ n ≤ 2 ^ 8 - 1
  | .u16, n => 0 ≤ n ∧ n ≤ 2 ^ 16 - 1
  | .u32, n => 0 ≤ n ∧ n ≤ 2 ^ 32 - 1
  | .u64, n => 0 ≤ n ∧ n ≤ 2 ^ 64 - 1
  | .i8, n => -2 ^ 7 ≤ n ∧ n ≤ 2 ^ 7 - 1
  | .i16, n => -2 ^ 15 ≤ n ∧ n ≤ 2 ^ 15 - 1
  | .i32, n => -2 ^ 31 ≤ n ∧ n ≤ 2 ^ 31 - 1
  | .i64, n => -2 ^ 63 ≤ n ∧ n ≤ 2 ^ 63 - 1
  | .index, n => 0 ≤ n ∧ n ≤ 2 ^ indexBits - 1
  | .offset, n => -2 ^ (indexBits - 1) ≤ n ∧ n ≤ 2 ^ (indexBits - 1) - 1
  | .int, _ => True
  | _, _ => False

/-- Whether a hex literal is a value of a type: an unsigned integer type or
`index` it fits, `int`, or a float of at least its bits. -/
def hexFits : Ty → Nat → Prop
  | .u8, n => n < 2 ^ 8
  | .u16, n => n < 2 ^ 16
  | .u32, n => n < 2 ^ 32
  | .u64, n => n < 2 ^ 64
  | .index, n => n < 2 ^ indexBits
  | .f32, n => n < 2 ^ 32
  | .f64, n => n < 2 ^ 64
  | .int, _ => True
  | _, _ => False

/-- The values of a type. -/
inductive HasType : Val → Ty → Prop where
  | bool : HasType (.bool b) .bool
  | int : inRange t n → HasType (.int n) t
  | hex : hexFits t n → HasType (.hex n) t
  | float : t.isFloat = true → HasType .float t
  | str : HasType .str .string
  | tuple : vs.length = (tysList ts).length →
      (∀ p ∈ vs.zip (tysList ts), HasType p.1 p.2) → HasType (.tuple vs) (.tuple ts)
  | struct : fs.map (·.1) = (fieldsList ts).map (·.1) →
      (∀ p ∈ fs.zip (fieldsList ts), HasType p.1.2 p.2.2) → HasType (.struct fs) (.struct ts)
  | list : (∀ v ∈ vs, HasType v t) → HasType (.list vs) (.list t)
  | set : (∀ v ∈ vs, HasType v t) → HasType (.set vs) (.set t)
  | map : (∀ p ∈ es, HasType p.1 k) → (∀ p ∈ es, HasType p.2 v) → HasType (.map es) (.map k v)
  | tensor : shape.length = rank → vs.length = shape.foldl (· * ·) 1 →
      (∀ v ∈ vs, HasType v t) → HasType (.tensor shape vs) (.tensor t rank)
  | table : header = (fieldsList cs).map (·.1) →
      (∀ r ∈ rows, r.length = (fieldsList cs).length) →
      (∀ r ∈ rows, ∀ p ∈ r.zip (fieldsList cs), HasType p.1 p.2.2) →
      HasType (.table header rows) (.table cs)
  | none : HasType .none (.option t)
  | some : HasType v t → HasType (.some v) (.option t)
  | ok : HasType v t → HasType (.ok v) (.result t)
  | er : HasType v .error → HasType (.er v) (.result t)
  | data : HasType .data .data
  | error : HasType .error .error
  | atom : HasType (.atom a) (.atom a)
  | atomEnum : Variants.atomListed a vs = true → HasType (.atom a) (.enum vs)
  | term : HasType v t → HasType (.term a v) (.term a t)
  | termEnum : Variants.termPayload a vs = .some t → HasType v t → HasType (.term a v) (.enum vs)
  /-- A recursive type's values are its unfolding's. -/
  | roll : HasType v t.expose → HasType v t

/-! ## Lemmas -/

theorem throw_ne {α} {c : Code} {a : α} : (throw c : Except Code α) ≠ .ok a := by
  intro h; cases h

theorem bind_ok {α β} {x : Except Code α} {f : α → Except Code β} {b : β}
    (h : (x >>= f) = .ok b) : ∃ a, x = .ok a ∧ f a = .ok b := by
  cases x with
  | error e => cases h
  | ok a => exact ⟨a, rfl, h⟩

theorem intRange_sound {t : Ty} {lo hi : Int} {c : Code} {n : Int}
    (h : intRange t = Option.some (lo, hi, c)) (h₁ : lo ≤ n) (h₂ : n ≤ hi) : inRange t n := by
  cases t <;> simp [intRange] at h <;> obtain ⟨rfl, rfl, rfl⟩ := h <;> simp [inRange] <;> omega

theorem hexBits_sound {t : Ty} {bits : Nat} {c : Code} {n : Nat}
    (h : hexBits t = Option.some (bits, c)) (h₁ : n < 2 ^ bits) : hexFits t n := by
  cases t <;> simp [hexBits] at h <;> obtain ⟨rfl, rfl⟩ := h <;> simpa [hexFits] using h₁

theorem evalAll_length : ∀ (es : List Expr), (evalAll es).length = es.length
  | [] => rfl
  | _ :: es => by simp [evalAll, evalAll_length es]

theorem evalFields_names : ∀ (fs : List (String × Expr)), (evalFields fs).map (·.1) = fs.map (·.1)
  | [] => rfl
  | _ :: fs => by simp [evalFields, evalFields_names fs]

/-- A value of a type's unfolding, by a proof about what the unfolding is. -/
theorem roll_of {v : Val} {t u : Ty} (e : t.expose = u) (h : HasType v u) : HasType v t :=
  .roll (e ▸ h)

/-! ## Soundness -/

mutual

theorem checkWith_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (e : Expr) (t : Ty), checkWith env e t = .ok () → HasType (eval e) t
  | .hinted h inner, t, hc => by
    simp only [checkWith] at hc
    obtain ⟨_, -, hc⟩ := bind_ok hc
    split at hc
    · exact absurd hc throw_ne
    · simpa [eval] using checkForm_sound env hsub inner t hc
  | .tt, t, hc | .ff, t, hc | .int _, t, hc | .hex _, t, hc | .float, t, hc | .str, t, hc
  | .tuple _, t, hc | .struct _, t, hc | .list _, t, hc | .set _, t, hc | .map _, t, hc
  | .tensor _ _, t, hc | .table _ _, t, hc | .none, t, hc | .some _, t, hc | .ok _, t, hc
  | .er _, t, hc | .data _, t, hc | .error _, t, hc | .atom _, t, hc | .term _ _, t, hc
  | .enum _, t, hc | .group _, t, hc => checkForm_sound env hsub _ t (by simpa [checkWith] using hc)

theorem checkForm_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (e : Expr) (t : Ty), checkForm env e t = .ok () → HasType (eval e) t
  | .group inner, t, hc => by
    simp only [checkForm] at hc
    simpa [eval] using checkWith_sound env hsub inner t hc
  | .none, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht .none
    · exact absurd hc throw_ne
  | .er p, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht (.er (checkWith_sound env hsub p _ hc))
    · exact absurd hc throw_ne
  | .some p, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht (.some (checkWith_sound env hsub p _ hc))
    · exact absurd hc (hsub _)
  | .ok p, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht (.ok (checkWith_sound env hsub p _ hc))
    · exact absurd hc (hsub _)
  | .tt, t, hc | .ff, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i hb; simp at hb; subst hb; exact .bool
    · exact absurd hc throw_ne
  | .str, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i hb; simp at hb; subst hb; exact .str
    · exact absurd hc throw_ne
  | .int n, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i hi; simp at hi; exact roll_of hi (.int trivial)
    · split at hc
      · rename_i hr
        split at hc
        · rename_i hn; simp at hn; exact .roll (.int (intRange_sound hr hn.1 hn.2))
        · exact absurd hc throw_ne
      · exact absurd hc (hsub _)
  | .hex n, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i hi; simp at hi; exact roll_of hi (.hex trivial)
    · split at hc
      · rename_i hr
        split at hc
        · rename_i hn; exact .roll (.hex (hexBits_sound hr hn))
        · exact absurd hc throw_ne
      · exact absurd hc (hsub _)
  | .float, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht (.float rfl)
    · rename_i ht; exact roll_of ht (.float rfl)
    · exact absurd hc (hsub _)
  | .tuple es, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ts ht
      split at hc
      · exact absurd hc throw_ne
      · rename_i hl
        simp at hl
        exact roll_of ht (.tuple (by rw [evalAll_length]; exact hl) (checkPairs_sound env hsub es _ hc))
    · exact absurd hc (hsub _)
  | .struct fs, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ts ht
      split at hc
      · exact absurd hc throw_ne
      · rename_i hl
        simp at hl
        have := checkFields_sound env hsub fs _ hc
        exact roll_of ht (.struct (by rw [evalFields_names]; exact this.2 hl) this.1)
    · exact absurd hc (hsub _)
  | .list es, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ ht; exact roll_of ht (.list (checkAll_sound env hsub es _ hc))
    · exact absurd hc (hsub _)
  | .set es, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ ht; exact roll_of ht (.set (checkAll_sound env hsub es _ hc))
    · exact absurd hc (hsub _)
  | .map es, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ _ ht
      have := checkEntries_sound env hsub es _ _ hc
      exact roll_of ht (.map this.1 this.2)
    · exact absurd hc (hsub _)
  | .tensor shape es, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ _ ht
      split at hc
      · exact absurd hc throw_ne
      · rename_i hr
        split at hc
        · exact absurd hc throw_ne
        · rename_i hn
          simp at hr hn
          exact roll_of ht (.tensor hr (by rw [evalAll_length]; exact hn) (checkAll_sound env hsub es _ hc))
    · exact absurd hc (hsub _)
  | .table header rows, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ ht
      split at hc
      · exact absurd hc throw_ne
      · split at hc
        · exact absurd hc throw_ne
        · rename_i hh
          simp at hh
          have := checkRows_sound env hsub rows _ hc
          exact roll_of ht (.table hh this.1 this.2)
    · exact absurd hc (hsub _)
  | .atom a, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ ht
      split at hc
      · rename_i hb; simp at hb; subst hb; exact roll_of ht .atom
      · exact absurd hc throw_ne
    · rename_i _ ht
      split at hc
      · rename_i hl; exact roll_of ht (.atomEnum hl)
      · exact absurd hc throw_ne
    · exact absurd hc (hsub _)
  | .term a p, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i _ _ ht
      split at hc
      · rename_i hb; simp at hb; subst hb; exact roll_of ht (.term (checkWith_sound env hsub p _ hc))
      · exact absurd hc throw_ne
    · rename_i _ ht
      split at hc
      · rename_i _ hp; exact roll_of ht (.termEnum hp (checkWith_sound env hsub p _ hc))
      · exact absurd hc throw_ne
    · exact absurd hc (hsub _)
  | .enum inner, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · simpa [eval] using checkWith_sound env hsub inner t hc
    · exact absurd hc throw_ne
  | .data p, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht .data
    · exact absurd hc (hsub _)
  | .error p, t, hc => by
    simp only [checkForm] at hc
    split at hc
    · rename_i ht; exact roll_of ht .error
    · exact absurd hc (hsub _)
  | .hinted _ _, t, hc => by
    simp only [checkForm] at hc
    exact absurd hc (hsub _)

theorem checkPairs_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (es : List Expr) (ts : List Ty), checkPairs env es ts = .ok () →
    ∀ p ∈ (evalAll es).zip ts, HasType p.1 p.2
  | e :: es, t :: ts, hc => by
    simp only [checkPairs] at hc
    obtain ⟨_, he, hr⟩ := bind_ok hc
    intro p hp
    simp only [evalAll, List.zip_cons_cons, List.mem_cons] at hp
    rcases hp with rfl | hp
    · exact checkWith_sound env hsub e t he
    · exact checkPairs_sound env hsub es ts hr p hp
  | [], _, _ => by simp [evalAll]
  | _ :: _, [], _ => by simp [evalAll]

theorem checkFields_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (fs : List (String × Expr)) (ts : List (String × Ty)),
    checkFields env fs ts = .ok () →
    (∀ p ∈ (evalFields fs).zip ts, HasType p.1.2 p.2.2) ∧
      (fs.length = ts.length → fs.map (·.1) = ts.map (·.1))
  | (n, e) :: fs, (m, t) :: ts, hc => by
    simp only [checkFields] at hc
    split at hc
    · obtain ⟨_, hn, -⟩ := bind_ok hc
      exact absurd hn throw_ne
    · rename_i hnm
      simp at hnm
      obtain ⟨_, he, hr⟩ := bind_ok hc
      have ih := checkFields_sound env hsub fs ts hr
      refine ⟨?_, ?_⟩
      · intro p hp
        simp only [evalFields, List.zip_cons_cons, List.mem_cons] at hp
        rcases hp with rfl | hp
        · exact checkWith_sound env hsub e t he
        · exact ih.1 p hp
      · intro hl
        simp at hl
        simp [hnm, ih.2 hl]
  | [], ts, _ => by
    refine ⟨by simp [evalFields], fun hl => ?_⟩
    have : ts = [] := List.eq_nil_of_length_eq_zero hl.symm
    simp [this]
  | _ :: _, [], _ => by simp [evalFields]

theorem checkAll_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (es : List Expr) (t : Ty), checkAll env es t = .ok () →
    ∀ v ∈ evalAll es, HasType v t
  | [], _, _ => by simp [evalAll]
  | e :: es, t, hc => by
    simp only [checkAll] at hc
    obtain ⟨_, he, hr⟩ := bind_ok hc
    intro v hv
    simp only [evalAll, List.mem_cons] at hv
    rcases hv with rfl | hv
    · exact checkWith_sound env hsub e t he
    · exact checkAll_sound env hsub es t hr v hv

theorem checkEntries_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (es : List (Expr × Expr)) (k v : Ty),
    checkEntries env es k v = .ok () →
    (∀ p ∈ evalEntries es, HasType p.1 k) ∧ (∀ p ∈ evalEntries es, HasType p.2 v)
  | [], _, _, _ => by simp [evalEntries]
  | (ke, ve) :: es, k, v, hc => by
    simp only [checkEntries] at hc
    obtain ⟨_, hk, hc⟩ := bind_ok hc
    obtain ⟨_, hv, hr⟩ := bind_ok hc
    have ih := checkEntries_sound env hsub es k v hr
    refine ⟨?_, ?_⟩
    · intro p hp
      simp only [evalEntries, List.mem_cons] at hp
      rcases hp with rfl | hp
      · exact checkWith_sound env hsub ke k hk
      · exact ih.1 p hp
    · intro p hp
      simp only [evalEntries, List.mem_cons] at hp
      rcases hp with rfl | hp
      · exact checkWith_sound env hsub ve v hv
      · exact ih.2 p hp

theorem checkRows_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) : ∀ (rows : List (List Expr)) (cols : List (String × Ty)),
    checkRows env rows cols = .ok () →
    (∀ r ∈ evalRows rows, r.length = cols.length) ∧
      (∀ r ∈ evalRows rows, ∀ p ∈ r.zip cols, HasType p.1 p.2.2)
  | [], _, _ => by simp [evalRows]
  | r :: rs, cols, hc => by
    simp only [checkRows] at hc
    split at hc
    · obtain ⟨_, hw, -⟩ := bind_ok hc
      exact absurd hw throw_ne
    · rename_i hl
      simp at hl
      obtain ⟨_, hp, hr⟩ := bind_ok hc
      have pairs := checkPairs_sound env hsub r _ hp
      have ih := checkRows_sound env hsub rs cols hr
      refine ⟨?_, ?_⟩
      · intro row hrow
        simp only [evalRows, List.mem_cons] at hrow
        rcases hrow with rfl | hrow
        · rw [evalAll_length]; exact hl
        · exact ih.1 row hrow
      · intro row hrow
        simp only [evalRows, List.mem_cons] at hrow
        rcases hrow with rfl | hrow
        · intro p hp
          have : (p.1, p.2.2) ∈ (evalAll r).zip (cols.map (·.2)) := by
            rw [List.zip_map_right]
            exact List.mem_map.mpr ⟨p, hp, rfl⟩
          exact pairs _ this
        · exact ih.2 row hrow

end

/-- A literal that checks against a type is a value of it, whatever synthesis
gives a `data` payload, and whatever Check-Subsume does as long as it never
succeeds. -/
theorem check_sound (env : CheckEnv) (hsub : ∀ e, env.sub e ≠ .ok ()) {e : Expr} {t : Ty}
    (h : checkWith env e t = .ok ()) : HasType (eval e) t :=
  checkWith_sound env hsub e t h

end Datalove.Lit

namespace Datalove.Lit

/-- The real Check-Subsume never succeeds: whatever inference finds, it ends
in T032, or in what inference threw first. -/
theorem subsume_fails (e : Expr) : subsume e ≠ .ok () := by
  unfold subsume
  cases (infer e).run #[] <;> simp [bind, Except.bind, throw, throwThe, MonadExceptOf.throw]

/-- A literal the real checker accepts against a type is a value of it. -/
theorem check_sound' {e : Expr} {t : Ty} (h : check e t = .ok ()) : HasType (eval e) t :=
  check_sound _ subsume_fails h

end Datalove.Lit
