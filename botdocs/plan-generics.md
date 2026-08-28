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

## Order of work

1. **Fix CTFE.** The three open issues in
   [Const Parameter Implementation](const-param-impl-plan.md): heap values leaking
   through a call, collections of linear elements leaking, and calls in binary operand
   position panicking lowering. These are prerequisites on every path, they are small,
   and each is independently testable.
2. **Add a type variable to `IrType`** and teach lowering to emit descriptor parameters
   alongside erased values. This is the bulk of the work.
3. **Erase and reify at call boundaries**, reusing the rider convention.
4. **Write the stdlib that motivated this**: `option` and `result` over any `T`, then
   `list`, `map` and `set`. This is the point of the exercise and should happen before
   any optimization.
5. **Only then**, if measurement says so, specialize hot instantiations in the JIT tier.

Steps 1 and 4 are where the value is. Step 5 may never be needed.

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
