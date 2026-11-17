/-!
# Passing by pointer is passing by value, where nothing written is shared

The model runs a call by copy-in, copy-out (`Fun.evalCall`): each argument is
read to a value where it is written, the callee gets those values, and what
its `mut` and `out` parameters came to is written back once it returns. The
Rust passes every parameter by pointer instead, and the callee reads and
writes the caller's variables through them while it runs. The two agree only
where no parameter the callee writes is reached by another, which is what the
ownership analysis's O010 sees to and what lowering's early copy of an `in`
argument provides.

This states it of a callee abstracted to its effect on its parameters: a
sequence of steps, each setting a parameter to a function of some of them.
Under pointer passing each parameter is a location of the caller's store
(`locs`); under copy-in, copy-out it is a local seeded from that location and
written back at the end if it is writable (`ws`, the `mut` and `out` ones). If
the callee writes only writable parameters, which the checker sees to (an
`in` or `ref` parameter is not assignable), and no other parameter's location
is a writable one's (`Unshared`), the two end with the same store, and the
callee reads the same at every step (`ptr_eq_copy`, `params_agree`).

`Unshared` is O010 together with the early copy, which
`Proofs/AliasSound.lean` proves of O010 as `FunAlias.lean` models it
(`Fun.aliased_unshared`), and `aliasing_agrees` holds the Rust's O010 to
that model. A writable argument's
variable passed again `mut`, `out` or `ref` is O010. Passed again to an `in`
parameter, a copy is read by lowering into a value of its own before the call,
a location no other parameter has, and anything else is O010 too. Without it
the two differ (`shared_differs`): in `f(mut a, a)`, a callee that sets its
first parameter to 9 and then reads its second reads 9 by pointer, and the
value `a` had by copy-in.

What this does not cover is ownership: a `ref` argument whose variable a
later `in` argument gives away is freed under it, which O010 refuses too, but
values here are never freed.
-/

namespace Datalove.PassBy

/-- `σ` with `l` set to `v`. -/
def upd {α} (σ : Nat → α) (l : Nat) (v : α) : Nat → α :=
  fun l' => if l' = l then v else σ l'

/-- A callee's step: set parameter `dst` to `f` of parameters `srcs`. -/
structure Step (α : Type) where
  dst : Nat
  srcs : List Nat
  f : List α → α

/-- A callee run with its parameters passed by pointer: parameter `k` is the
caller's location `locs k`, read and written in place. -/
def runPtr {α} (locs : Nat → Nat) : List (Step α) → (Nat → α) → (Nat → α)
  | [], σ => σ
  | s :: ss, σ => runPtr locs ss (upd σ (locs s.dst) (s.f (s.srcs.map fun k => σ (locs k))))

/-- A callee run on its own copies of its parameters. -/
def runCopy {α} : List (Step α) → (Nat → α) → (Nat → α)
  | [], a => a
  | s :: ss, a => runCopy ss (upd a s.dst (s.f (s.srcs.map a)))

/-- The caller's store once the writable parameters `ws` are written back. -/
def copyOut {α} (locs : Nat → Nat) (ws : List Nat) (a : Nat → α) (σ : Nat → α) : Nat → α :=
  fun l => match ws.find? (fun k => locs k == l) with
    | some k => a k
    | none => σ l

/-- A call by copy-in, copy-out: the parameters seeded from the caller's
locations, the callee run on them, and the writable ones written back. -/
def runByValue {α} (locs : Nat → Nat) (ws : List Nat) (ss : List (Step α)) (σ : Nat → α) : Nat → α :=
  copyOut locs ws (runCopy ss (fun k => σ (locs k))) σ

/-- No parameter's location is a writable parameter's but its own. -/
def Unshared (locs : Nat → Nat) (ws : List Nat) : Prop :=
  ∀ k ∈ ws, ∀ j, locs j = locs k → j = k

/-- The callee writes only writable parameters. -/
def WritesOnly {α} (ws : List Nat) (ss : List (Step α)) : Prop :=
  ∀ s ∈ ss, s.dst ∈ ws

/-- What both runs keep the same: the parameters read the same, and a
location no writable parameter has is the caller's as it was. -/
def Inv {α} (locs : Nat → Nat) (ws : List Nat) (σ0 σ a : Nat → α) : Prop :=
  (∀ j, σ (locs j) = a j) ∧ (∀ l, (∀ k ∈ ws, locs k ≠ l) → σ l = σ0 l)

theorem inv_run {α} (locs : Nat → Nat) (ws : List Nat) (hu : Unshared locs ws) (σ0 : Nat → α) :
    ∀ (ss : List (Step α)) (σ a : Nat → α), WritesOnly ws ss → Inv locs ws σ0 σ a →
      Inv locs ws σ0 (runPtr locs ss σ) (runCopy ss a)
  | [], _, _, _, h => h
  | s :: ss, σ, a, hw, ⟨h1, h2⟩ => by
    have hd : s.dst ∈ ws := hw s (by simp)
    have hv : s.f (s.srcs.map fun k => σ (locs k)) = s.f (s.srcs.map a) := by
      congr 1; exact List.map_congr_left (fun k _ => h1 k)
    simp only [runPtr, runCopy]
    refine inv_run locs ws hu σ0 ss _ _ (fun s' hs' => hw s' (by simp [hs'])) ⟨fun j => ?_, fun l hl => ?_⟩
    · simp only [upd]
      by_cases hj : locs j = locs s.dst
      · have := hu s.dst hd j hj
        subst this
        simp [hv]
      · have hj' : j ≠ s.dst := fun e => hj (e ▸ rfl)
        simp [hj, hj', h1 j]
    · have hl' : l ≠ locs s.dst := fun e => hl s.dst hd e.symm
      simp only [upd, hl', ite_false]
      exact h2 l hl

/-- **A callee reads the same at every step either way**: run by pointer or
on its own copies, from the same store, its parameters hold the same. -/
theorem params_agree {α} (locs : Nat → Nat) (ws : List Nat) (hu : Unshared locs ws) (ss : List (Step α))
    (hw : WritesOnly ws ss) (σ : Nat → α) (j : Nat) :
    runPtr locs ss σ (locs j) = runCopy ss (fun k => σ (locs k)) j :=
  (inv_run locs ws hu σ ss σ _ hw ⟨fun _ => rfl, fun _ _ => rfl⟩).1 j

/-- **Passing by pointer is passing by value where nothing written is
shared**: the caller's store ends the same. -/
theorem ptr_eq_copy {α} (locs : Nat → Nat) (ws : List Nat) (hu : Unshared locs ws) (ss : List (Step α))
    (hw : WritesOnly ws ss) (σ : Nat → α) :
    runPtr locs ss σ = runByValue locs ws ss σ := by
  have ⟨h1, h2⟩ := inv_run locs ws hu σ ss σ _ hw ⟨fun _ => rfl, fun _ _ => rfl⟩
  funext l
  simp only [runByValue, copyOut]
  cases hf : ws.find? (fun k => locs k == l) with
  | some k =>
    have := List.find?_some hf
    simp only [beq_iff_eq] at this
    rw [← this]; exact h1 k
  | none =>
    refine h2 l fun k hk e => ?_
    have := List.find?_eq_none.mp hf k hk
    simp [e] at this

/-- `f(mut a, a)` with `a` passed by pointer twice: a callee that sets its
first parameter to 9, then sets it to its second, reads 9 that way, and the 1
`a` was by copy-in. -/
theorem shared_differs :
    let locs : Nat → Nat := fun _ => 0
    let ss : List (Step Nat) := [⟨0, [], fun _ => 9⟩, ⟨0, [1], fun vs => vs.headD 0⟩]
    let σ : Nat → Nat := fun _ => 1
    runPtr locs ss σ 0 = 9 ∧ runByValue locs [0] ss σ 0 = 1 := by
  decide

/-- The same call with the second `a` read into a location of its own first,
as lowering does for a copy: the two agree. -/
theorem copied_agrees :
    let locs : Nat → Nat := fun k => if k = 0 then 0 else 1
    let ss : List (Step Nat) := [⟨0, [], fun _ => 9⟩, ⟨0, [1], fun vs => vs.headD 0⟩]
    let σ : Nat → Nat := fun _ => 1
    runPtr locs ss σ = runByValue locs [0] ss σ :=
  ptr_eq_copy _ [0] (fun k hk j hj => by
    simp at hk; subst hk; by_cases h : j = 0 <;> simp_all) _ (by simp [WritesOnly]) _

end Datalove.PassBy
