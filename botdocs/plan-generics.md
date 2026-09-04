# Generics and Specialization

How datalove should implement type parameters, and what to do about const parameter
specialization now that it has been built and measured.

The short version: generics by erasure and type descriptors, which the runtime already
does everywhere; monomorphization later and only for code the JIT has already decided is
hot. Const parameters are a separate feature that should drop union-branch for plain
monomorphization.

## Contents

- [Why not the const parameter machinery](#user-content-why-not-the-const-parameter-machinery)
- [What already exists](#user-content-what-already-exists)
- [The design](#user-content-the-design)
- [Cost](#user-content-cost)
- [Surface syntax](#user-content-surface-syntax)
- [Const parameters after this](#user-content-const-parameters-after-this)
- [What a first attempt found](#user-content-what-a-first-attempt-found)
- [Where this stands](#user-content-where-this-stands) -- what works, and the
  current limitations
- [Order of work](#user-content-order-of-work)
- [Prior art](#user-content-prior-art)

## Why not the const parameter machinery

Const parameter specialization looks like a foundation for generics and is not one. The
reason is worth stating precisely, because it is the impasse earlier work reached without
naming.

Union-branch keeps a single function signature by *removing* the const parameter and
adding a tag:

```
fun repeat(const n: i32, s: string): string
->  fun repeat(tag: i32, s: string): string     // one signature, N branches
```

Every branch agrees on the signature because the parameter that varied is gone. A type
parameter cannot be removed that way, because it is not only a parameter, it is the type
of other parameters and of the return:

```
fun swap(const T: type, x: T, y: T): (T, T)
->  fun swap(tag: i32, x: ???, y: ???): ???     // nothing to write here
```

`const-param-specialization.md` reaches this point and offers three answers: a union
sized to the largest instantiation, a pointer plus a size descriptor, or boxing. All
three are the same answer, which is *adopt a uniform representation*, and that is a
different mechanism with nothing to do with tags. Monomorphizing a value keeps the
signature; monomorphizing a type changes it. The machinery stops there.

The second reason not to build on it: union-branch does not pay for itself even for const
values. `build_dispatch_blocks` clones every body block once per instantiation, so the
output is monomorphization plus a `Switch` on a discriminant that every call site passes
as a literal, and fusing the instantiations into one symbol defeats the per-function call
counting in `optimizing.rs`. It is dominated by the simpler thing.

What *is* reusable is CTFE: the const binding graph, `ConstValue`, and evaluating const
expressions on the real interpreter. That is a genuine prerequisite for const generics
and for any type-level evaluation.

## What already exists

The pieces generics need are mostly built, in the runtime rather than the compiler.

**The runtime is type-descriptor driven throughout.** `dtlv_rti_clone_local`,
`eq_local`, `cmp_local`, `move_value_local`, `any_destroy_local` and
`pretty_print_local` all take `(ptr, tydesc)`. Clone and drop, the two operations linear
types make type-directed, are already dispatched on descriptors at runtime rather than
compiled per type.

**Collections are already generic.** `dtlv_rti_btreemap_insert_local` takes key and value
type descriptors. Lists, maps, sets, tensors and tables are implemented once for all
element types. Nothing about `[T]` or `%{K = V}` needs monomorphization to run.

**There is a working erased representation.** `data` is two words using the `anypack`
encoding, with tags `SmallImmediate`, `InlineWithTyDesc` and `TwoPointers`, so small
scalars are held immediately and only larger values indirect. The spec already says any
type coerces to `data`. This is a uniform representation with a test suite.

**The calling convention exists.** The native rider ABI is `(ptr, tydesc)` pairs with an
out-parameter for the return. That is what a function called with values whose types it
does not statically know requires, and `rider.dli` already declares signatures over
`data`.

The missing piece is in the compiler: `IrType` is a closed enum with no type variable, so
no IR function can be parametric.

## The design

Two layers. The first is the language feature; the second is an optimization that can
come much later or never.

### Layer 1: erasure with type descriptors

A generic function is compiled once. Values of a type parameter are held in the uniform
representation and accompanied by a type descriptor, which the function passes to the
runtime for anything type-directed.

```
fun unwrap_or(self: ?T, default: T): T
```

compiles to a single IR function taking a descriptor for `T` alongside the erased values,
and lowers `clone`, `drop` and comparison on `T` to the existing `dtlv_rti_*` calls with
that descriptor. Call sites erase at the boundary and reify on the way out.

Properties that matter here:

- Compile time is O(1) in the number of instantiations. A new instantiation appearing in
  the REPL compiles nothing.
- Nothing to invalidate. A generic function is one salsa-tracked entity, not a family
  whose membership depends on the whole program.
- The whole-program assumption goes away. Union-branch and monomorphization both need to
  see every call site before they can emit the callee; erasure does not, which is what
  makes it work in a REPL where call sites arrive one at a time.
- `option`, `result`, `list`, `map` and `set` over any `T` become writable immediately,
  which is the actual reason generics are wanted.

### Layer 2: monomorphize what is hot

`optimizing.rs` already counts calls, inlines at 50 and JIT-compiles at 100. Specializing
a generic function for a concrete descriptor is the same kind of decision and should
reuse the same counters: when an instantiation is hot, compile a version with the
descriptor fixed, unboxed and its runtime calls devirtualized.

This is Julia's arrangement and .NET's, and it puts the cost where a scripting language
can afford it. Nothing is specialized to start, so startup stays fast; code that runs
enough to matter gets specialized, and only that code.

Layer 2 is optional. Layer 1 is a complete, correct implementation on its own.

## Cost

Erasure is free at compile time, not at run time, and the honest accounting is:

- A local of type `T` inside a generic function is two words rather than a register. For
  `unwrap_or` on a `u32` that is an unbox where monomorphization would have a move.
  `SmallImmediate` means no allocation for scalars, but it is not nothing.
- Type-directed operations become indirect calls into the runtime instead of inline
  instructions. Again, already true of everything the runtime does today.
- Call sites erase and reify at the boundary.

Against that: no code growth, no compile-time growth, no whole-program requirement, and a
REPL that does not recompile when a new type shows up. For a language whose default
execution tier is an interpreter, paying an unbox to avoid a compile is the right side of
the trade. Layer 2 buys the difference back where it is measurable.

## Surface syntax

Use `<T>`:

```datalove
fun unwrap_or<T>(self: ?T, default: T): T
```

not Zig-style `const T: type`. The earlier recommendation of the Zig form was reasoning
from union-branch, which needed types to be const values so the same tag machinery would
apply. With erasure that machinery is not involved and the argument disappears, leaving
the costs: template-style generics cannot be checked before instantiation, so errors land
inside the callee and constraints are implicit; and the known weak points are exactly
autocomplete, hover documentation and incremental compilation. Those are the properties
this compiler is built around, with a REPL and an LSP on the roadmap.

`<T>` also keeps type parameters and const parameters visibly different, which they are:
one is erased and passed as a descriptor, the other is known at compile time and folded.

Bounds can wait. With erasure, a generic body may only do what the runtime can do for any
type — clone, drop, compare, print, store in a collection — so there is nothing to
constrain yet. Bounds become necessary when generic code needs operations that are not
universal, and that is a later design.

## Const parameters after this

Independent of generics, and worth doing regardless:

- Replace union-branch with monomorphization. Smaller, faster to compile, simpler code,
  and it restores per-instantiation tiering. Keep the differential test suite, which is
  the valuable part of the existing work.
- Reconsider the const-binding-only restriction. `pow(2, 10)` failing because `10` is a
  literal is a bad first impression, and once specialization no longer depends on
  pre-resolved `ResolvedConsts` lookups the restriction is easier to lift.
- `ConstValue::Type` may still be wanted, for const generics such as tensor dimensions.
  It is not a step toward type parameters.

## What a first attempt found

Written after building the front end far enough to hit the real boundary, then
reverting it. Measurements, not estimates.

**`<T>` parses for free.** `Sigil::AngleOpen` is already a bracer with a close
pair, so `fun identity<T>(x: T): T` arrives at the parser as one branch, the same
shape as the parameter list. Adding `type_params: Vec<InternedText>` to `StmtFun`
and reading that branch is about thirty lines, and `T` then reaches the
typechecker as `UnresolvedTypeAlias("T")`, which is the correct next error.

**A type variable in the datafun `Type` is cheap.** Only five files match on it,
and adding `Var(InternedText)` needs `types_equivalent` (equal to itself),
`type_to_string`, and about twenty-five mechanical arms in the type hint
converter. Resolution then falls out for free: seed the function's type
parameters into the alias map that `convert_type_hint_with_aliases` already
consults, and `T` resolves wherever a named type would.

**But that only reaches bare `T`, which is not the goal.** Those twenty-five arms
are all the same shape, converting a nested type hint into a *datalit* type:
`[T]`, `?T`, `(T, T)` all require the variable to live in
`datalit::tycheck::Type`, not in the datafun wrapper. The stdlib motivation is
`option.unwrap_or(self: ?T, default: T): T`, which is nested. So the front end
splits in two: bare `T` is a small change to datafun's type, and useful `T` is a
change to datalit's, which is the type of every data literal in the language.

**The back end is the wall.** `IrType::from_tycheck` is infallible, so a type
variable has to become some `IrType`. Fifty-four files reference `IrType` and
about eighty arms match it exhaustively, across the interpreter, Cranelift, the C
backend, layout, tydesc emission and ownership. Each is a place to answer what a
generic value's size and representation are, which is one decision but eighty
edits, and none of it is the calling convention work.

**Erasing `T` to `data` avoids all of that, and is the better plan.** This
document first dismissed it, on the grounds that `report-match-downcast.md` lists
downcasting out of `data` as an open question. That confuses a missing surface
syntax with a missing mechanism. A generic function never asks a user to write a
downcast: the compiler erases at the call site, where it knows the concrete type,
and reifies on the way back. What it needs is an IR instruction, not syntax.

The mechanism is already there. `anypack` exposes `value_ptr`, `value_ptr_as<T>`,
`tydesc` and `tytag`, plus a typed accessor for every type in the language:
`as_u32`, `as_string`, `as_list`, `as_option`, `as_result` and the rest. Wrapping
exists as `Instruction::DataFrom`. Only the extract is missing.

What this buys, against the `IrType::Generic` route:

- No new `IrType`. A generic function compiles once with each `T` replaced by
  `data`, and `data` is an ordinary type every backend already handles. The
  eighty match arms do not happen.
- Nested positions come free. `?T` becomes `?data` and `[T]` becomes `[data]`,
  both ordinary types, so datalit's `Type` does not have to change either.
- The generic body compiles like any other function. Frames, drops and clones all
  work, because dropping and cloning a `data` is already tydesc-driven.

Both erased signatures compile and run today, checked before writing this down:

```datalove
fun identity(x: data): data
fun unwrap_or(self: ?data, default: data): data
```

So the remaining work is an extract instruction, wrap and unwrap insertion at
generic call sites, and the front end. One instruction across three backends
rather than a representation decision across eighty arms.

Two costs to know about.

**Every wrap allocates.** `data_from_local` heap-allocates and copies
unconditionally, so erasing a `u32` costs an allocation. `anypack` has
`can_inline` and `from_immediate` for exactly this case and boxing does not use
them, so the fix is local and worth doing before measuring anything.

**Containers do not want this.** `[u32]` and `[data]` differ in element size, so
converting at the boundary is O(n) and allocates per element: `list.len(ref self:
[T])` would copy the whole list to ask its length. Containers want the tydesc
route instead, which the runtime already does natively and which the rider ABI
already speaks. So the split is scalars, options and results by erasure, which is
the stdlib blocker, and containers by descriptor.

The order below is unchanged by this, but step 2 gets much smaller: `T := data`
rather than a new IR type.

## Where this stands

Type parameters nest inside options and results to any depth, and sit under a
collection in a borrowed parameter. What follows is the state of it, then the
limitations, then how each was arrived at.

### What works

Each of these was checked by compiling it, not by reading the rules.

| Position | Bare `T` | `?T` `!T` `??T` | Collection: `[T]`, `%{K = V}`, `#{T}`, tuple, struct |
|---|---|---|---|
| `in`, owned | yes | yes | **no** |
| `ref`, `mut` | yes | yes | yes |
| `out` | yes | yes | **no** |
| return | yes | yes | **no** |
| any parameter of a `native fun` | yes | yes | yes |

A call site names no types: each argument is matched against its parameter and
the first to reach a parameter fixes it. `sys/std`'s `option`, `result`,
`list`, `map` and `set` are written this way.

### Limitations

**A collection cannot be owned, returned, or written to an out parameter.**
Refused with `TypeParamNotErasable`. A parameter the callee owns is carried as
a `data`, since a type parameter has no size, and converting `[u32]` into a
list of `data` means rebuilding it element by element at every call. A return
and an out parameter are the same thing from the other end. Options and results
escape this because they hold their payload inline, so converting one is
converting the payload and writing it at the other side's offset.

**A collection of a type parameter cannot be indexed.** Refused with F011.
Indexing works out where an element sits from the static type, which inside a
generic says `data`, so the stride would be a `data`'s and the read would land
between elements. It typechecked and died at run time before this was refused.
`sys/std/list` reaches an element instead, because the native list functions
read the element type from the descriptor that travels with the collection.
Making the index operator do the same needs an instruction meaning "use the
descriptor this parameter came with" rather than the one the static type
implies, in the IR and in each backend.

**A generic out parameter has to be a whole binding or a field.** Both work;
anything else is refused rather than written wrong.

**The C backend calls riders and takes descriptors like the others.** It was
written before generics existed and had neither, which is why nothing in its
386 fixtures reaches the standard library. It now compiles all of `sys/std`,
so a program that calls a rider, does bigint arithmetic or passes a borrowed
collection of a type parameter builds and runs the same under it as under the
interpreter and the two cranelift backends.

**No bounds, so no operations on a `T`.** A type parameter is equal only to
itself and nothing can be done with a value of that type but move it, drop it,
clone it, and hand it back. That rules out `map`, `and_then`, `filter` and
anything else needing a function argument, though those want closures first.

**A type parameter is always linear**, because the caller may supply a linear
type. A value taken out of an option by `if x |v|` has moved out of `x`, so a
function returning the option it destructured has to rebuild it with `some v`.

**Which type a parameter is fixed at depends on argument order.** An unsuffixed
integer literal is `int`, so `pick_first(99, : u32 / 1)` makes `T` `int` and
widens the `u32` into it, while the two the other way round make `T` `u32`.
Both check; they just do not name the same `T`.

**Nothing is monomorphized.** One compiled function serves every
instantiation, and every owned crossing costs a wrap and an unwrap. That is the
intended trade for compilation speed; specializing hot instantiations is step 5
below and may never be needed.

### How it works

**The type variable went into datalit, and it was cheap.**

The section
above treats that as the expensive branch, on an estimate of eighty match arms.
Measured instead of estimated, adding `Var(InternedText)` to
`datalit::tycheck::Type` costs seven sites in the whole workspace: printing it
in two places, `types_equivalent` (a variable is equal only to itself),
`is_copy_type` (an unconstrained parameter is move, since the caller may supply
a linear type), two `unreachable!`s where a parameter cannot occur, and one arm
in `IrType::from_datalit` that maps it to `data`. The eighty-arm figure was for
`IrType`, which every backend matches exhaustively; `datalit::Type` is mostly
consumed through predicates and constructors that already have fallbacks.

That last arm is the whole of erasure. `from_datalit` recurses structurally, so
one line erases a parameter at any depth: `?T` lowers as `?data`, `??T` as
`??data`. The twenty-four "cannot nest a type parameter" arms in the type hint
converter were deleted rather than written.

**This does not add generics to the surface language.** A bare `T` already
parsed as `TypeHint::Alias("T")`, and datalit's own resolver rejects every alias
(`Type aliases are resolved by datafun typechecker, not datalit`). The single
place a `Var` is built is datafun-resolve seeding the alias map with the
enclosing signature's type parameters, in a crate datalit does not depend on. A
data literal mentioning `T` fails exactly as it did before.

**Call sites unify resolved types, not the callee's AST.** `bind_type_params`
walks the parameter's type and the argument's in step, binding at each `Var`;
the first argument reaching a parameter fixes it and later ones must agree.
An argument that cannot synthesize on its own, such as a bare `none`, binds
nothing and is checked afterwards against whatever another argument fixed, so
`unwrap_or(none, "fallback")` infers. Speculative synthesis runs under
`try_check` so the abandoned attempt does not report.

**Why collections stop here.** The runtime's `convert` recurses properly through
options and results, converting the payload and writing it at the other side's
offset. For everything else it memcpys, which is a reinterpretation rather than
a conversion, and a collection's elements are packed by size: `[u32]` and
`[data]` disagree about where every element after the first begins. Passing one
through appears to work because nothing touches the elements, but the callee
holds a tydesc that lies about them, and the drop at the end of a generic that
merely ignores its argument corrupts the allocator. That was checked, not
reasoned about. `first_unerasable_type_param` refuses those signatures with
`TypeParamNotErasable`.

**A borrowed collection escapes that, by not being converted.** Nothing happens
to a `ref` or `mut` parameter at the boundary: the value goes across as it
stands and the callee never drops it. What it lacks is a descriptor, since its
own type says `data` where the parameter was written, and the call site has
that. `FunctionContext::descriptor_params` records which parameters need one,
and how it is carried is left to each backend: the interpreter needs nothing,
because a value there is a pointer and a descriptor already, while the compiled
backends take one extra pointer after the ordinary parameters.

That distinction is about ownership rather than shape. A parameter the callee
owns needs a slot, and `data` is the one shape that fits any value. A parameter
it borrows needs a pointer, which fits anything already.

**The all-backends suite is what makes this safe to have.** Descriptor passing
worked in the interpreter for a while before it worked anywhere else, because
interpreter values carry their descriptors and the compiled backends pick one
from the static type when they emit the call. Left enabled, that would have
been a feature that worked in one backend and corrupted memory in the other
two. It was caught by `std_all_tests`, with a segfault rather than a wrong
answer, and refused until the backends agreed. Generic `len` passed all three
even then, because `list_len` happens not to look at the element type, which is
exactly the kind of accident that makes a wrong descriptor look like a working
feature.

## Order of work

Steps 1 to 4 are done. The type variable went into datalit rather than `IrType`,
which is why step 2 reads as it does.

1. ~~**Fix CTFE.**~~ Done, along with module-level consts and const bindings no
   longer moving when read.
2. ~~**Add a type variable**~~ and teach lowering to emit descriptor parameters
   alongside erased values. Done, in `datalit::tycheck::Type` and
   `FunctionContext::descriptor_params`.
3. ~~**Erase and reify at call boundaries**, reusing the rider convention.~~ Done.
4. ~~**Write the stdlib that motivated this**~~: `option`, `result`, `list`,
   `map` and `set` are all generic.
5. **Only then**, if measurement says so, specialize hot instantiations in the
   JIT tier. Nothing is monomorphized today.

What is left, in the order it is worth doing:

- **Descriptor-driven indexing**, which removes the largest remaining
  limitation and is the one place the language still refuses something a reader
  expects to work. Needs an IR instruction naming a descriptor a parameter came
  with, plus its handling in each backend.
- **Owned collections**, which need either that same instruction or
  monomorphization. Whether they are worth the conversion is a question the
  descriptor route answers by not converting at all.
- **Bounds of some kind**, without which nothing can be done to a `T` but move
  it. Closures are the bigger prerequisite for the functions people ask for.

## Prior art

The survey in [Const Parameter Specialization](const-param-specialization.md) covers
Harper and Morrisett's intensional type analysis, dictionary passing, Go's GCShape
stenciling, Swift's witness tables, defunctionalization and partial evaluation. Two
systems it omits are the closest fits to this problem.

**The .NET CLR** splits by representation rather than by type: reference types share one
JIT-compiled instantiation through the `__Canon` placeholder, value types are
monomorphized. The cost surfaces as "runtime handle lookup" when shared code needs the
concrete type back. The split maps closely onto datalove's copy and linear types.

**Julia** specializes lazily, at the first call with a new type signature, and caches the
result. Its documented failure mode is the relevant warning: over-parameterized types
cause a compile-time blowup that can exceed the runtime benefit, and 1.6 deliberately made
the compiler less eager to specialize. Julia also warns specifically against enthusiasm
for values as type parameters, which is what const parameter specialization is. Its
world-splitting, branching over up to four candidate types instead of dispatching
dynamically, is the technique union-branch resembles, and it applies where the type is
*not* statically known.

References:

- Kennedy, A. & Syme, D. (2001). [Design and Implementation of Generics for the .NET Common Language Runtime](https://web.eecs.umich.edu/~bchandra/courses/papers/Kennedy_Generics.pdf). PLDI.
- Nikitin, A. [.NET Generics under the hood](https://alexandrnikitin.github.io/blog/dotnet-generics-under-the-hood/).
- Zhang, Y. [Sharing .NET generic code under the hood](https://yizhang82.dev/dotnet-generics-sharing).
- Bezanson, J. et al. (2018). [Fast Flexible Function Dispatch in Julia](https://arxiv.org/pdf/1808.03370).
- Holy, T. (2020). [Analyzing sources of compiler latency in Julia: method invalidations](https://julialang.org/blog/2020/08/invalidations/).
- [Julia's latency: Past, present and future](https://viralinstruction.com/posts/latency/).
- Rao, V. [Zig-style generics are not well-suited for most languages](https://typesanitizer.com/blog/zig-generics.html).
