# Reactive Scripts and the Literature

How Datalove's reactive script semantics compares to FRP, reactive notebooks,
incremental computation and build systems, and what to take from each.
Written November 2026 for the "Comparing to FRP literature" section of
[the reactive script semantics draft](../../mandocs/drafts/reactive-script-semantics.md),
for review. Nothing here is decided. The sources marked *unverified* were cited
from memory and not re-read; check them before citing them in a post.

## Contents

- [The answer](#user-content-the-answer)
- [The semantics, restated](#user-content-the-semantics-restated)
- [Reactive notebooks: the direct neighbours](#user-content-reactive-notebooks-the-direct-neighbours)
- [The rules are dependence analysis](#user-content-the-rules-are-dependence-analysis)
- [Incremental computation and build systems](#user-content-incremental-computation-and-build-systems)
- [FRP](#user-content-frp)
- [What a data change did in the engine](#user-content-what-a-data-change-did-in-the-engine)
- [Data as an input binding](#user-content-data-as-an-input-binding)
- [Below the unit: incremental data](#user-content-below-the-unit-incremental-data)
- [Recommendations](#user-content-recommendations)
- [Open questions](#user-content-open-questions)
- [Sources](#user-content-sources)

## The answer

No single literature covers what Datalove does. Five come close, each from a
different side:

- **Reactive notebooks** have the same goal and the same correctness rule, and
  fail at exactly the case Datalove's backward rules handle: mutation of a
  binding another cell provided.
- **Dependence analysis** from compilers names the rules' parts: flow and anti
  dependences, and the nearest killing definition.
- **Self-adjusting computation and build systems** supply the correctness
  vocabulary ("from-scratch consistency"), early cutoff, and the alternative
  to restoring by rerunning: keeping versions.
- **FRP** describes the semantics once data is a runtime input. A script is a
  first-order, static graph over inputs that are behaviors. FRP has nothing to
  say about code edits or mutation between nodes.
- **Live programming** (managed time, Elm's hot swap with replay) is the
  precedent for code and data changing together.

The distinctive combination is static, position-ordered units with
cross-unit moves and mutation, made sound by linear types, and restored by
rerunning rather than by snapshots. The backward rules are the cost of
evaluating incrementally over mutable state, not something the semantics
needs: semantically a session is a pure function of its code and its inputs.

## The semantics, restated

From the draft and
[script-reactivity-architecture.md](../script-reactivity-architecture.md):

- A session is a list of units run in order. A unit provides bindings, and
  reads, takes, overwrites, updates or revives earlier units' bindings.
- A step starts from a modification -- add, fix, break, delete, edit, rerun --
  and ends when the session holds what a fresh run of the current code would.
- Retract restores what the old code changed. Apply restores what the new code
  will look at if a later unit changed it since. Restoring reruns the nearest
  unit that set the binding without reading it. Later units rerun on changed
  values, and break or are fixed on changed names. The rules repeat until
  nothing is added.
- The engine reruns to restore and copies nothing. Its known cost: editing the
  tail of a chain of takes reruns from the head.

## Reactive notebooks: the direct neighbours

**Rex** (Lu, Zheng, Crichton, Narayan, Raghavan, Vasilakis, "When Do Reactive
Notebooks Fail to React?", arXiv 2511.21994, 2025) is the closest work.

- It defines a reaction's correctness as the draft does: the notebook's state
  must be what a top-to-bottom run in a fresh environment gives. A reaction is
  *sound* if it gets there and *precise* if it does no redundant work.
  Precision is measured as a *rerun ratio*, cells rerun over the minimal set.
- Its Figure 1 is the draft's step 2: a cell that appends to a list another
  cell made is edited, and the provider has to rerun first. Marimo,
  Observable and IPyflow all get it wrong.
- Marimo and Observable avoid the problem by scope. A global is defined in one
  cell only, and mutation across cells is not supported. Neither warns when a
  notebook does it anyway.
- IPyflow traces mutation at runtime. It misses mutation through an alias,
  for example a helper that calls `df.drop(inplace=True)` on its argument, and
  it over-reruns by 2.8 times on the real notebooks.
- The naive baseline, rerun every cell below the edit, fails exactly the
  cases that need an upstream rerun.
- Everything treats external state (files, RNG) as out of scope.
- The suite has 87 synthetic micro-benchmarks and 38 edits to real notebooks,
  sorted into direct assignment, reassignment and mutation.

**FlowBook** (Freund, Berger, Flanagan, Jun, arXiv 2605.01560, 2026) enforces
*rerun consistency*: no location a cell reads may be written by a cell below
it. That is the situation the backward rules handle. FlowBook forbids it;
Datalove restores.

**nbsafety / IPyflow** (Macke et al., VLDB 2021) is lineage tracking by
runtime tracing plus static liveness. **Kishu** (Li et al., VLDB 2025)
restores notebook state from incremental checkpoints of changed variables,
falling back to recomputation.

**Where Datalove stands.** Rex's hardest tier, mutation, is the one no system
handles soundly. In Datalove every move and every write to another unit's
binding is visible statically: linear types for moves, `set` and `mut`/`out`
parameters in signatures for writes. IPyflow's worst failure, a write through
an alias inside a function, cannot happen. That is the strongest claim for the
comparison section. The engine is held to it by the cases under
[writes through function parameters](../script-reactivity-cases.md#user-content-writes-through-function-parameters),
which include Rex's `flu_vaccine` edit and its `aliasing_val_swap`.

## The rules are dependence analysis

| the draft's term | standard name |
|---|---|
| a later unit reruns on a changed value | flow (RAW) dependence |
| restore because a later unit took or wrote what an earlier one reads | anti (WAR) dependence |
| the nearest unit that provided, overwrote or revived it | nearest killing definition (a reaching definition can be an update) |
| an overwrite stops propagation | a kill, in gen/kill dataflow |
| a conditional or partial write counts as an update | may-def against must-def |
| collect until nothing is added | worklist fixed point |

Two consequences:

- The five accesses are combinations of use, must-def and may-def, plus
  whether the name is given away or given back. Saying so could shorten the
  "complete set of recursive rules" section and make it read as less ad hoc to PL readers.
- Compilers remove anti dependences by renaming (SSA, register renaming). The
  equivalent here is keeping old versions of bindings, which removes the need
  to restore by rerunning. Imperative self-adjusting computation does this
  with versioned references, Kishu with checkpoints, managed time with
  rollback.

## Incremental computation and build systems

- **"From-scratch consistency"** is the established name for the draft's
  rule. Hammer et al., "Incremental Computation with Names" (OOPSLA 2015),
  calls it "the fundamental correctness property of incremental computation"
  and proves it as its main theorem (B.13): incremental evaluation produces
  the same answer as re-evaluating from scratch. Acar's papers call the same
  property "consistency" (Acar, Blume, Donham, ESOP 2007; Acar, Ahmed, Blume,
  POPL 2008). Build Systems a
  la Carte (Mokhov, Mitchell, Peyton Jones, 2018 *unverified*) gives formal
  correctness definitions to cite.
- **Imperative self-adjusting computation** (Acar, Ahmed, Blume, POPL 2008)
  lifted the write-once restriction on modifiable references, keeping
  versions per write. It is the nearest formal precedent for incremental
  evaluation with mutation.
- **Early cutoff.** Salsa backdates at the compile layer. The runtime does not
  compare values, so the draft's planned "same-value" case is unimplemented
  early cutoff. Jane Street's `Incremental` (*unverified*) calls these
  cutoffs.
- **Determinism is load-bearing.** "A unit rerun to restore a binding finds
  what it found before" holds only for deterministic code. Build systems treat
  nondeterministic inputs as volatile, always changed.
- **Granularity.** Adapton and self-adjusting computation memoize below the
  unit, at calls. That bears on the draft's todo about the motivating example
  changing a function while the rules talk about bindings.

## FRP

Classic FRP (*unverified* throughout this section) is about values over time
in a fixed program: *behaviors*, which vary continuously, and *events*, which
occur and accumulate.

- **The graph is first-order and static.** It is fixed per version of the code
  and known at compile time, with no dynamic `switch`. That is early Elm's
  model (Czaplicki and Chong, "Asynchronous FRP for GUIs", PLDI 2013) and
  arrowized FRP's, not higher-order FRP's, and for the same reason: it can be
  analyzed and compiled.
- **Glitch freedom comes free.** Units run in order and a step completes before
  anything observes it.
- **Linear types do a different job.** In FRP they have been used against
  space and time leaks (Krishnaswami, ICFP 2013). Here they make mutation
  across units visible.
- **FRP nodes do not mutate each other's values.** So FRP has no counterpart
  to the backward rules.
- **Code edits are outside FRP.** The precedent for combining them with
  changing inputs is Elm's debugger, which hot-swapped code and replayed the
  recorded input events through it, giving a fresh run of the new code over
  the input history.

## What a data change did in the engine

Commit `5c7f3bcc3` added `Engine::edit_data` and the REPL's `/edit-data`, and
a step to
`crates/datalove-doc-fixtures/replays/reactive-transactions.world`
where the bank appends a coffee. Here `require data` is a compiler input: the
data is the value of a const in the unit that requires it.

- **Coffee appended.** Unit 4, `require data`, reruns; units 7, 8 and 9 rerun;
  units 1-3, 5 and 6 are untouched. `over` becomes `["dining", "groceries"]`.
  Unit 4 is not recompiled in the step table's sense: the data's type did not
  change, so its typecheck backdates, though its lowering runs again. The step
  table vocabulary has no word for that.
- **Dining budget raised.** Unit 6 recompiles; 8 and 9 rerun; and unit 7
  reruns the whole transaction loop with nothing it uses changed, because unit
  9 took `spent` and unit 8 needs it rebuilt. That is the restore cost, and
  the case a kept version of `spent` would remove.
- Both tables were written from the rules before running, and the engine
  matched them, apart from unit 4 being a rerun rather than a recompile.
- No rule changed. A data change is a rerun of the requiring unit that changes
  values; data of another type fails the unit, a break. The draft's
  modification table gained a `data change` row.

## Data as an input binding

Suppose instead the data is a binding of a special kind, which the engine's
driver sets between steps, and which is never compiled.

**The rules get simpler.**

- The type must be declared. Today an unannotated `require data` takes its type
  from the file, so data can change a type and break a unit. An input binding
  cannot, so data never reaches the "changed names" rule and never recompiles
  anything. A value of the wrong type is the driver's to refuse, or a runtime
  failure that blocks later units as an early return does.
- Restoring an input binding is reading the driver's current value again. With
  linear types a unit could take it, so the driver must keep its own copy. That
  forces a choice:
  - inputs that can be taken or mutated, so the driver hands each run a fresh
    copy and inputs take part in restores like any `var`; or
  - inputs that are read-only, like behaviors, which no unit can take,
    overwrite, update or revive. The backward rules never reach them. A unit
    that wants ownership copies one into a `var`.

  The read-only form is the better default: one copy where a unit needs
  ownership, in exchange for a class of reruns that cannot happen. With
  compile-time data the language has no way to stop a unit moving
  `transactions`; with a binding the driver owns, it does.
- A step becomes a set of modifications. Several inputs, and a code edit, can
  change between two steps. The fixed point already admits a set of seeds and
  takes each unit once, and order cannot matter, since the target is a fresh
  run over the current code and inputs. "A step starts with one modification"
  is a wording to change, not a rule to add.
- Inputs must not change during a step. The driver either queues a change or
  cancels the step and starts again. Elm's asynchronous FRP and `Incremental`'s
  stabilization face the same choice.

**The FRP comparison gets sharper.** Inputs are behaviors sampled at step
boundaries; the script is a first-order static graph re-evaluated on change;
code edits are program replacement, the live-programming half. Because a step
is a fresh run, nothing accumulates between steps: there is no `foldp`. The
session means a pure function of (code, inputs), and the backward rules exist
only because the engine reuses a heap that units mutated in place.

**Costs.** Replaying a session needs each step's input values, kept by the
driver as Elm's debugger keeps events. The compiler can no longer fold the
data or infer its type from it, but compiling stops depending on data at all.

## Below the unit: incremental data

A rerun is a whole unit. Appending one transaction reruns the whole `for t in
ref transactions` loop.

- **Behavior or event.** "The bank appends" describes an event; the semantics
  treats the input as a behavior. As a behavior, rerunning over the current
  value is the rule. As an event stream the rule becomes a fresh run over the
  input history, and an append can be folded in incrementally. An input
  binding lets the driver hand over the delta beside the value without
  changing the semantics.
- **DBSP** (Budiu, McSherry, Ryzhyk, Tannen, VLDB 2023 *unverified*) and
  **differential dataflow** (McSherry et al., CIDR 2013 *unverified*):
  collections as signed multisets, where a retraction is a negative weight,
  and linear operators incrementalize to themselves. The `spent` loop is a
  group-by-sum, which is linear, so an append could process one row. The
  draft's retract and apply resemble DRed's delete and rederive at the level of
  data.
- **Incremental lambda calculus** (Cai, Giarrusso, Rendel, Ostermann, PLDI
  2014 *unverified*) derives a program's derivative statically, for pure code,
  which Datalove's functions are. The most ambitious direction.
- **A first step.** Recognize a fold over an append-only input: a `for` over a
  `ref` collection that mutates only its own `var`. On an append, run the body
  over the new rows on the kept accumulator. Linearity says nothing else wrote
  the accumulator; if something did, the restore rule already reruns the fold.

## Recommendations

In order:

1. Decide behavior or event for inputs, and write it into the draft's
   correctness rule.
2. Decide whether data stays a compiler input or becomes an input binding, and
   if a binding, whether it is read-only.
3. Generalize a step to a set of simultaneous modifications.
4. Port Rex's mutation and reassignment micro-benchmarks to worldfiles, for an
   outside benchmark that reports soundness and rerun ratio in its terms.
5. Add differential oracle tests: a random session from `datalove worldgen`,
   a random modification, compared against a fresh run. Every fixture step is
   now compared against a fresh run (November 2026); what is missing is random
   sessions.
6. Retract by what the old code did on its last run, recorded at runtime,
   rather than by its static accesses. Apply has to stay static. This fixes
   the issue that a unit that did not run is assumed to have done what it would.
7. Value-based early cutoff at the runtime layer.
8. Later: snapshots for restores that rerun something expensive, such as the
   dining-budget step's unit 7, and fold incrementalization for append-only
   inputs.

For the draft's comparison section: the Rex and FlowBook framing, the
dependence-analysis table, "from-scratch consistency", behavior against
event, the static first-order graph, glitch freedom by ordering, and Elm's
replay as the precedent for code and data changing together.

## Open questions

- Is an input a behavior, an event stream, or either by declaration?
- May a unit take or write an input binding?
- When an input changes during a step, does the driver queue the change or
  restart the step?
- Is "the session is a pure function of code and inputs" the claim the post
  wants to lead with, with the backward rules presented as the cost of the
  evaluator?

## Sources

Read for this document:

- Lu et al., "When Do Reactive Notebooks Fail to React?",
  https://arxiv.org/abs/2511.21994
- Freund et al., "FlowBook", https://arxiv.org/abs/2605.01560
- Macke et al., "Fine-grained lineage for safer notebook interactions",
  https://dx.doi.org/10.14778/3447689.3447712
- Li et al., "Kishu: Time-Traveling for Computational Notebooks",
  https://www.vldb.org/pvldb/vol18/p970-li.pdf
- marimo reactivity guide, https://docs.marimo.io/guides/reactivity/
- Hammer, Dunfield, Headley, Labich, Foster, Hicks, Van Horn, "Incremental
  Computation with Names", OOPSLA 2015,
  https://www.cs.tufts.edu/~jfoster/papers/oopsla15.pdf
- Acar, Ahmed, Blume, "Imperative self-adjusting computation", POPL 2008,
  DOI 10.1145/1328438.1328476
- McDirmid and Edwards, "Programming with managed time", Onward! 2014,
  https://www.microsoft.com/en-us/research/publication/programming-with-managed-time/

The rest is cited from memory and marked *unverified* where it appears.
