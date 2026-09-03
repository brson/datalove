# Overloading or Traits

What it would take to give one name to an operation that works on several
types, why the standard library needs it, and what each way of getting there
costs.

## Contents

- [The problem](#user-content-the-problem)
- [What the compiler does now](#user-content-what-the-compiler-does-now)
- [Option A: overloading](#user-content-option-a-overloading)
- [Option B: traits and bounds](#user-content-option-b-traits-and-bounds)
- [Option C: qualified calls](#user-content-option-c-qualified-calls)
- [Comparison](#user-content-comparison)
- [Recommendation](#user-content-recommendation)
- [Open questions](#user-content-open-questions)

## The problem

The REPL wants a prelude: a set of names a session can use without ceremony,
so that a calculation reads `sqrt(2.0)` rather than two lines of `require` and
`import` first. It cannot have one.

**129 of the 212 distinct function names in `sys/std` are defined in more than
one module.** `min`, `max`, `clamp` and `is_zero` are in all thirteen numeric
modules; `abs_diff`, `div_checked` and `rem_checked` in eleven. Any prelude
naming more than one numeric type collides with itself almost immediately.

Worse, the collision is silent. `TypeContext::add_imported_function` is a
`HashMap::insert` (`tycheck/src/context.rs:534`), so the last import of a name
wins and nothing is reported:

```datalove
require module sys/std/int
require module sys/std/i32
import int.abs
import i32.abs

let x: i32 = -5
abs(x)              // i32's. No error, no warning.
```

That much is a bug on its own, whatever is done about the larger question: a
name bound twice in one scope should be a diagnostic.

## What the compiler does now

**One name, one function.** `TypeContext.functions` is
`HashMap<InternedText, TypeFunction>` (`context.rs:43`), and
`lookup_function` is a single `get`. There is no candidate set anywhere in the
front end.

**Calls resolve statically, once.** `synthesize_function_call`
(`tycheck/src/synthesize.rs:808`) looks the name up, checks arity and argument
modes, binds type parameters by walking each parameter type against its
argument, and stores a `ResolvedCallTarget` naming exactly one function AST.
By lowering time a call site has one callee, and `Instruction::Call` holds a
`CodeRef` (`ir/src/lib.rs:954`) rather than anything indirect.

That last point is worth holding onto: **anything that resolves to a single
target before lowering needs no changes below the typechecker.**

**Generics are erasure with type descriptors.** A generic function compiles
once; values of a type parameter travel in the uniform `data` representation
with a descriptor alongside, and type-directed work is done by the runtime.
`plan-generics.md` chose this deliberately, for compile time that does not grow
with instantiations and for a REPL that does not recompile when a new type
appears.

**There are no bounds, so there are no operations on a `T`.** A type parameter
is equal only to itself; a generic body may move, drop, clone, compare, print
and store a value and nothing else. `plan-generics.md` says why that was left
for later:

> Bounds become necessary when generic code needs operations that are not
> universal, and that is a later design.

This report is that later design.

**Dispatch on a type does already exist, at run time.** The runtime is
descriptor-driven throughout, and the native riders branch on descriptors:
`sys/std/rider/src/lib.rs` compares `type_tag()` against `TyTag::Data` to
decide whether an element needs boxing. What does not exist is any way for the
*language* to say which types an operation applies to.

## Option A: overloading

Give a name a set of functions and choose between them by the types of the
arguments.

```datalove
import f64.sqrt
import f32.sqrt     // no longer shadows

sqrt(2.0)           // picks one by the argument's type
```

### What changes

- `TypeContext.functions` becomes a multimap, and `add_imported_function`
  extends a set rather than overwriting.
- `synthesize_function_call` collects the candidates whose arity matches, types
  the arguments, and keeps the candidates whose parameters accept them.
- Zero candidates and more than one are both errors, with the candidate list in
  the message.

Nothing below the typechecker moves. `ResolvedCallTarget` still names one
function, so the IR, the three backends and the runtime are untouched. This is
a front-end change, and that is the whole of its appeal.

### The unusual advantage

Overloading has a bad reputation earned mostly by conversions. In C++ or Java
several candidates are viable for the same call and the language needs a
ranking over them: exact match beats promotion beats standard conversion,
with ambiguity rules where the ranking runs out.

**Datalove has no implicit numeric conversions** (botspec section 10). A `u32`
does not become an `i64` on its own, an integer does not become a float, and
`f32` does not become `f64`. So for a set of candidates differing only in their
numeric parameter types, *at most one can accept a given argument* — resolution
is exact match, and the entire ranking apparatus is unnecessary.

The feature is therefore much smaller here than the same feature elsewhere, and
that is a property of a decision already made rather than a lucky accident.

### What it costs

**It fights bidirectional inference.** Today the parameter type flows into the
argument: `sqrt(2.0)` types `2.0` as `f64` because that is what `sqrt` takes.
Choosing an overload requires the opposite order — type the argument, then pick
the callee — and a bare `2.0` on its own synthesizes `f32`. So `sqrt(2.0)`
would quietly select single precision.

That is not a reason to reject overloading, but it is a reason to change the
float literal default to `f64` first. As long as bare literals default to the
narrow type, argument-directed resolution will pick the narrow overload.

More generally, every position where a literal currently takes its type from
context has to be re-examined, since an overloaded callee supplies no context
until it is chosen.

**Candidate trials must not consume.** Arguments are linear, so typing them to
test a candidate cannot move them. There is precedent — the type-parameter
binding pass already synthesizes arguments under `ref_context` and `try_check`
already rolls back a failed attempt — but every trial has to be clean, and
errors from discarded candidates must not reach the user.

**It does nothing for generic code.** `fun mean<T>(xs: [T]): T` still cannot
add two values of `T`. Overloading resolves at concrete types; it has no
opinion about type parameters.

## Option B: traits and bounds

Say what a type parameter can do, and write the operation once.

```datalove
fun sqrt<T>(x: T): T where T is float
```

The `where { T is move }` shape appears in `mandocs/future-designs.md` under
2026/02/18, so the surface syntax has been imagined already.

### Three ways to dispatch, two of them blocked

**Dictionary passing.** The general answer, and a natural fit for erasure: a
bound becomes a table of implementations passed alongside the descriptor that
already travels with the value. It needs indirect calls. `Instruction::Call`
takes a `CodeRef` and the language has no closures, so there is no vehicle for
a dictionary today. Building one is a larger project than the bounds
themselves.

**Monomorphization.** Compile a copy per instantiation and dispatch
statically. This works, and contradicts the decision `plan-generics.md` made:
monomorphization needs to see the call sites before it can emit the callee,
which is what makes it wrong for a REPL where call sites arrive one at a time.

**Runtime dispatch on the type tag.** The runtime is already
descriptor-driven, so a bound can mean "the runtime can do this for this tag".
No new IR, no closures, no whole-program assumption. This is the one that fits.

### What the spike found

I wrote the third one to see what it costs: a `native fun num_sqrt<T>(x: T): T`
branching on `TyTag::F32` and `TyTag::F64`, wrapped by a generic
`fun any_sqrt<T>(x: T): T` in `sys/std/f64`.

It compiles and typechecks. It dies at run time:

```
failed to move a value out of its erased shape
```

Inside a generic function a `T` is erased to `data`, so the rider is handed a
boxed value and an out parameter expecting one, not an `f64`. The existing
generic natives do not pack values themselves — they hand the work to runtime
functions that have data-aware variants, as `dlr_std__list_get` does by
choosing between `dtlv_rti_list_get_local` and
`dtlv_rti_list_get_as_data_local` on the descriptor.

**So each bounded operation needs an entry in the runtime C ABI that can write
its result in the erased shape.** For the math set that is twenty-odd new
runtime functions, plus a table saying which type tags satisfy which bound. The
mechanism exists and is proven by the collections; it is the per-operation cost
that is the news.

### What it buys

Generic algorithms, which are inexpressible today at any price:

```datalove
fun sum<T>(xs: ref [T]): T where T is num
```

That is a different want from the prelude, and a larger one.

### What else it drags in

A trait is a language feature, not just a bound: whether users may declare
their own or only the built-ins are available, whether implementations are
open, what happens when two modules implement the same trait for the same type,
whether associated types exist. None of that is forced on day one — a closed
set of built-in bounds over built-in types is a coherent first step — but the
questions arrive with the syntax.

## Option C: qualified calls

Neither feature is needed to make a prelude possible; only unambiguous names
are.

The pieces are half-built. `require module` already binds a module alias
(`StmtRequireModule.module_alias`), and `a.b` already parses as a projection.
What is missing is resolving `f64.sqrt(2.0)` as a call to a module's function.
A prelude then becomes a list of `require module` lines and the stdlib's
existing one-module-per-type arrangement does the disambiguating:

```datalove
f64.sqrt(2.0)
int.abs(-5)
```

Separately, `StmtImport` is `{ module_name, item_name }` with no alias field
(`ast.rs:310`). Adding one would let a prelude rename on the way in:

```datalove
import f64.sqrt as sqrtf
```

Neither gives one name for several types, which is what was asked for. Both are
small, and both remove the reason the prelude is impossible rather than the
reason it is inconvenient.

## Comparison

| | Overloading | Traits | Qualified calls |
|---|---|---|---|
| Solves the prelude | yes | yes | yes, with a prefix |
| One name for several types | yes | yes | no |
| Generic algorithms | no | yes | no |
| Front end | multimap, candidate resolution | bounds, checking | name resolution |
| IR and backends | untouched | untouched | untouched |
| Runtime | untouched | one entry per bounded operation | untouched |
| Fights literal inference | yes | no | no |
| Whole-program assumption | no | no, with tag dispatch | no |
| Opens further design | little | traits, coherence, associated types | little |

## Recommendation

Do the small things first, then overloading, and treat traits as a separate
decision made later for a different reason.

1. **Make a duplicate binding a diagnostic.** Silent shadowing is wrong
   whatever else happens, and it is currently how the collision presents.

2. **Default float literals to `f64`.** Today a bare `2.0` is an `f32` and
   cannot be widened, which is already a trap; under argument-directed
   resolution it becomes a silent selection of the narrow overload. This has to
   land before overloading, not after.

3. **Overloading.** It is the feature the prelude actually needs, it is a
   front-end change, and the usual cost of it does not apply here because there
   are no conversions to rank. The prelude problem is a naming problem, and
   this is the naming answer.

4. **Traits when generic algorithms are wanted**, which is a real want and a
   different one. The tag-dispatch implementation is the one that fits the
   erasure design, and its price is a runtime ABI entry per bounded operation.

The two are not exclusive and the order does not trap anything: overloading
does not constrain how bounds are later resolved, and bounded generics want
candidate-set resolution at their call sites regardless.

## Open questions

- Should overload sets be per-scope or per-module? Per-scope is what the REPL
  needs, since imports accumulate over a session.
- Does a candidate set span modules, or only the imports named in one scope?
  The latter keeps resolution local and the error messages short.
- What does an ambiguous call say, given that exact matching means ambiguity
  can only arise from two identical signatures?
- For traits: are the bounds a closed set the compiler knows, or can a module
  declare one? A closed set is a much smaller feature and covers the numeric
  tower, which is the whole of the current demand.
