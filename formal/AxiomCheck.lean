import Lean
import Datalove

/-!
Fails the build if any declaration of the model rests on an axiom other than
Lean's three standard ones.

`lake build --wfail` already refuses a `sorry`, as a warning. This catches
what it does not: `Lean.ofReduceBool` from a `native_decide`, a `sorryAx`
that arrives some other way, or an `axiom` declared in the model. It checks
every theorem and definition in a `Datalove` module, not a chosen list, so a
new theorem is covered without being named here.
-/

open Lean Elab Command

/-- The axioms a declaration of the model may rest on. -/
def allowedAxioms : List Lean.Name := [``propext, ``Classical.choice, ``Quot.sound]

/-- Checks every declaration of a `Datalove` module against `allowedAxioms`. -/
elab "#check_model_axioms" : command => do
  let env ← getEnv
  let inModel (n : Lean.Name) : Bool :=
    match env.getModuleIdxFor? n with
    | some idx => (`Datalove).isPrefixOf env.header.moduleNames[idx.toNat]!
    | none => false
  let names := env.constants.map₁.fold (init := #[]) fun acc n ci =>
    if inModel n && !n.isInternal && (ci.isTheorem || ci.isDefinition) then acc.push n
    else acc
  let mut bad := #[]
  for n in names do
    let axs ← liftCoreM (collectAxioms n)
    let extra := axs.filter (!allowedAxioms.contains ·)
    unless extra.isEmpty do
      bad := bad.push m!"{n}: {extra.toList}"
  if bad.isEmpty then
    logInfo m!"{names.size} declarations rest on no axiom but {allowedAxioms}"
  else
    logError m!"declarations resting on other axioms:\n{MessageData.joinSep bad.toList "\n"}"

#check_model_axioms
