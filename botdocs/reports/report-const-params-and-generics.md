# Const Parameters and Generics: the Phasing Problem

Why `fun f<T>(const n: int, x: T)` was refused, what actually broke when the
refusal was lifted, and what it took to let the two features combine.

> **Built.** The six edits in
> [What it would take](#user-content-what-it-would-take) are done, and the
> narrowed refusal is in place. Everything before that section is the
> investigation as written, in the present tense of a language that still
> refused it; read [What was done](#user-content-what-was-done) at the end for
> what actually landed and where it differed.

The short version: they are not in conflict. Monomorphization varies a function
over *values* and erasure varies it over *types*, and the two axes are
independent -- one copy per const instantiation serves every type
instantiation. What stops them combining is that the descriptor machinery is
settled in a phase that runs before specialization, over an instruction
specialization has not yet produced, and `ComptimeCall` carries none of the
fields that machinery reads. Five of the six things to fix are the same
one-line omission repeated in five places.

One case is not phasing and should stay refused: a const parameter whose *type*
is a type parameter.

## Contents

- [The refusal](#user-content-the-refusal)
- [What the two features each need from a call](#user-content-what-the-two-features-each-need-from-a-call)
- [Where the phases sit](#user-content-where-the-phases-sit)
- [What breaks, reproduced](#user-content-what-breaks-reproduced)
- [Why the two axes are independent](#user-content-why-the-two-axes-are-independent)
- [What it would take](#user-content-what-it-would-take)
- [The one case that should stay refused](#user-content-the-one-case-that-should-stay-refused)
- [What this is worth](#user-content-what-this-is-worth)
- [What was done](#user-content-what-was-done)

## The refusal

`datafun-tycheck/src/statement.rs` rejects any function that declares both:

```rust
// Const parameters and type parameters do not combine. Lowering
// takes the comptime branch before a call's type arguments are
// computed, and specialization emits none, so a function that had
// both would lose the descriptors its type parameters need and
// read its values at the wrong type.
if !stmt.type_params(db).is_empty() {
    for param in params.iter().filter(|p| p.is_comptime) {
        ctx.add_error(TypeError::ComptimeParamOnGeneric { .. });
    }
}
```

The refusal is on the *declaration*, and only on the declaration. A generic
function may call a comptime one, and does:

```datalove
fun rev<T>(ref xs: [T]): [T]
    const TWO: int = 2
    let d = double(TWO)          // comptime callee, generic caller
    debuglog d
    ret reversed(ref xs)
end fun
```

runs today, on every backend. So the interaction being refused is narrow: one
function declaring both kinds of parameter.

## What the two features each need from a call

A call site to a generic function passes three things beyond the ordinary
arguments, and the two features disagree about only the third.

**The erased argument values.** `Erase` and `Reify` around the call, emitted in
`lower_expression`'s call arm *before* it branches on comptime-ness. Both
branches get this, which is why the simplest combined case already works.

**A descriptor per `descriptor_params` entry.** For a borrowed parameter whose
declared type is a lie about its layout. The call site computes these from the
*callee's* signature and the *argument's own* type
(`calls.rs:377`, `callee_descriptor_params`), so it needs nothing from the
call instruction. This survives specialization intact: `monomorphize_function`
remaps `descriptor_params` across the dropped const parameters, and
`rewrite_comptime_calls` filters the arguments by the same predicate, so the
indices still line up.

**A descriptor per `descriptor_shapes` entry.** For a collection the callee
*builds* over a type parameter and has no value to read a descriptor off. These
are derived from the call instruction's `type_args` by `shape_descriptors_for`,
stored back on the instruction as `shape_descriptors`, and read by all four
backends.

Only the third is lost, and it is lost because `ComptimeCall` has no `type_args`
field and no `shape_descriptors` field.

## Where the phases sit

Module graph (`tracked_lower.rs::lower_module_graph_with_evaluator`):

```
5a  lower every module function
    -> Call { type_args, shape_descriptors: [] }   generic callee
    -> ComptimeCall { }                            comptime callee, nothing recorded
5a/b  close_shapes_over_calls
    - collect call edges:   `let Instruction::Call { .. } = instr else { continue }`
    - close the shape sets to a fixed point
    - resolve_call_descriptors: same match, writes `shape_descriptors`
5b  const evaluation
5c  specialize_comptime_functions
    - copy each comptime function per instantiation
    - rewrite_comptime_calls: ComptimeCall -> Call { type_args: [], shape_descriptors: [] }
5d  assemble
```

Script units (`script_compiler.rs::compile_unit_inner`) are the same order:
`phase_resolve_shape_descriptors` at line 409, `phase_specialize` at line 412,
with its own copy of the edge collection that matches on `Call` alone
(`script_compiler.rs:1096`).

So the descriptor answers are computed once, over a graph that does not contain
the `Call` instructions specialization is about to create, from an instruction
that records none of the inputs. Three separate omissions, all pointing the same
way:

| Site | What it does |
|---|---|
| `expr.rs:721` | comptime branch never computes `type_args`, though `target.type_args(db)` is sitting there |
| `tracked_lower.rs:565`, `script_compiler.rs:1096` | edge collection skips `ComptimeCall` |
| `ir/lib.rs:2286` | `resolve_call_descriptors` skips `ComptimeCall` |
| `specialize.rs:328` | rewrite writes `type_args: Vec::new(), shape_descriptors: Vec::new()` |
| `cranelift/codegen/mod.rs:778`, `interp/lib.rs:1120`, `c-aot/codegen.rs:342` | each passes `&[]`, commented "a comptime call is not generic" |

Every one of those is correct *given* the refusal, and each cites it. The
refusal is what makes them true, and they are what makes the refusal necessary.

## What breaks, reproduced

**Reproduced**, by making the refusal conditional on an environment variable,
rebuilding, and running. The change was reverted; the tree is clean.

**A generic and comptime function with no descriptors works.** Erasure of the
argument values happens before the comptime branch, so this is already right:

```datalove
fun pick<T>(const n: int, a: T, b: T): T
    ret a
end fun
```

`pick(ONE, : u32 / 7, : u32 / 9)` prints `7`, and the IR shows a real copy,
`pick__ct1(p0, p1)`, with the const parameter folded out. So does a borrowed
collection parameter -- `fun size<T>(const n: int, ref xs: [T])` calling
`list.len` returns `3` -- because those descriptors come from the callee's
signature rather than from the call instruction.

**A declared shape brings it down, differently in each backend.**

```datalove
fun rev<T>(const n: int, ref xs: [T]): [T]
    ret reversed(ref xs)
end fun
```

`rev` calls `list.reversed`, which builds a `[T]`, so `rev` declares one
descriptor shape it must be handed and forward. Its caller hands over nothing:

- Interpreter: panics, `a forwarded shape is one this function declared`
  (`interp/lib.rs:2202`), followed by the leak detector reporting 24 bytes.
- Cranelift AOT: `codegen error: define function: Compilation error: Verifier
  errors` -- the call site's argument count does not match the signature.
- JIT: memory corruption. `list_create:value_out: pointer 0x7f.. not aligned to
  3442555936 bytes` -- a garbage word read where a descriptor pointer was
  expected.

The same three results come from the module path (a `.world` with the generic
comptime function in a module and a non-generic module function calling it), so
this is not specific to script units.

The IR shows the copy being built correctly and the call site being pointed at
it -- the only thing missing is the trailing descriptor:

```
fn rev(p0, p1):
    v0 = call @0 m12.u18(p1)
    drop p0
    return v0

fn rev__ct1(p0):
    v1 = const 1int
    v0 = call @0 m12.u18(p0)
    drop v1
    return v0
```

**A const parameter of a type parameter's type is silently not specialized.**

```datalove
fun ident<T>(const x: T): T
    ret x
end fun
```

This runs and prints the right answer, for `u32` and for `string`. It is also
not specialized at all, and nothing says so:

```
v0 = const 7u32
v2 = erase v0
v3 = comptime_call u0(v2) [disc=0, comptime_params=[0]]
```

`comptime_values` reads the const argument out of `const_value_map`, which
follows `Const`, `Clone`, `Move` and `Copy` but not `Erase`. The value is behind
an `Erase`, so no instantiation is collected, no copy is built, and the call
keeps naming the original -- which every backend runs as a plain call. A `const`
that quietly becomes a runtime parameter is worse than a refusal.

## Why the two axes are independent

This is the part worth being explicit about, because
[Const Parameter Specialization](../const-param-specialization.md) reached the
opposite conclusion and [Generics and Specialization](../plan-generics.md)
inherited the framing.

Union-branch could not extend to type parameters because it worked by *removing*
the varying parameter and adding a tag, and a type parameter cannot be removed:
it is the type of other parameters and of the return. That argument is about
union-branch, and union-branch is gone. What replaced it -- monomorphization on
the const values, erasure on the types -- has no such problem, because the two
transformations act on disjoint parts of the signature:

- Specialization deletes const parameters and substitutes `Instruction::Const`
  at entry. It touches no type.
- Erasure replaces each type parameter with `data` (or converts field by field,
  or wraps a container whole) and adds trailing descriptors. It touches no
  const parameter.

A copy of a generic function is still generic. It has the same type parameters,
the same erased signature, the same `descriptor_shapes`, and it wants the same
descriptors from the same places. `monomorphize_function` already clones
`descriptor_shapes` unchanged and remaps `descriptor_params`; it did the right
thing by accident, because there was never anything for it to get wrong.

Three consequences worth stating:

**The copies do not multiply.** The specialization key is the const values
alone. A body with `T` erased is identical for every `T`, so `f<u32>(3, ..)` and
`f<string>(3, ..)` share one copy and differ only in the descriptor the call
site passes. N const instantiations across M type instantiations is N copies,
not N x M. That is the whole point of having the two mechanisms do different
jobs.

**The shape closure does not have to run again.** A copy's shape set equals its
original's. A copy's body is the original's blocks with parameters substituted,
so its outgoing call edges and their `type_args` are unchanged. A call site
pointed at a copy has the `type_args` the `ComptimeCall` had. So the fixed point
over the graph-with-copies is the fixed point over the graph-without, and an
answer computed in 5a/b is still the right answer in 5c. It only has to be
*carried across* the rewrite.

**`type_args` can never mention a const parameter.** `DescriptorShape` has
`Param`, `Concrete` and the type constructors, and no value form. So nothing in
the descriptor machinery can depend on which instantiation it is in.

## What it would take

Six edits, none of them structural.

1. **`Instruction::ComptimeCall` gains `type_args: Vec<DescriptorShape>` and
   `shape_descriptors: Vec<DescriptorRef>`**, the same two fields `Call` has,
   with the same meaning. Both `#[serde(default)]`, as `Call`'s are.

2. **Lowering computes `type_args` once, above the branch.** `expr.rs:730-738`
   already does the work; it just sits inside the `else`. The typechecker
   records `type_args` for a comptime callee like any other
   (`synthesize.rs:1093` is not conditioned on comptime-ness), so the input is
   already there.

3. **Edge collection and descriptor resolution see both instructions.** Three
   sites match `Instruction::Call` to pull out `(func, type_args)`. The right
   shape is one accessor on `Instruction` -- something like
   `fn call_target(&self) -> Option<(&CodeRef, &[DescriptorShape])>` and a
   `_mut` pair -- read by `close_shapes_over_calls`, by the script compiler's
   copy of it, and by `resolve_call_descriptors`. Three call sites that have to
   agree about which instructions are calls is exactly the shape of bug this
   area keeps producing; one accessor is what stops a fourth appearing.

4. **`rewrite_comptime_calls` carries both fields across** instead of writing
   `Vec::new()` twice, and its comment -- "a function with const parameters is
   never generic" -- goes with them.

5. **The three backends pass `shape_descriptors`** instead of `&[]`. Each is a
   one-line change in an arm that already delegates to `compile_call`.

6. **The refusal narrows** from "this function has type parameters" to "this
   parameter's type mentions one", which is the next section.

Then the differential fixtures: `specialize_differential` is the suite that
makes this safe, and the cases that need adding are the ones reproduced above --
a declared shape (`rev`), a borrowed collection (`size`), a bare `T`
(`pick`), and two type instantiations sharing one copy, which is the property
worth pinning down because nothing else would notice if it broke.

There is one thing to check rather than assume: **the additive original**. The
original generic comptime function stays, and a call site the pass could not
specialize keeps its `ComptimeCall`, which the backends run as a plain call to
it. Once step 5 lands that call passes descriptors too, so the two paths agree
-- but a `ComptimeCall` that survives to a backend has to have been through step
3, and the script path's `resolve_call_descriptors` runs with `own_shapes = &[]`
on the script unit's own body. That is sound (a script has no type parameters to
forward, so every reference resolves `Static`), but it is the place where a
wrong answer would be silent rather than loud.

## The one case that should stay refused

A `const` parameter whose *type* mentions a type parameter:

```datalove
fun ident<T>(const x: T): T
```

This is not a phasing problem. The const argument is the only thing that says
what `T` is, and its value is known at compile time, so the honest reading is
that it pins `T` concretely for that instantiation -- which is monomorphizing a
type parameter through a value, the Zig arrangement
[plan-generics](../plan-generics.md#user-content-surface-syntax) turned down.

It cannot be done by the machinery that exists. `monomorphize_function` builds a
copy by cloning blocks and substituting parameters; it does not re-lower.
Substituting a concrete `T` would have to change the copy's signature, its
`descriptor_shapes`, its `descriptor_params`, and the erasure decisions inside
its body -- that is a second lowering of the function, not a body clone. And
`Instruction::Const` would carry a `ConstValue::U32(7)` into a slot the
signature types as `data`.

So: refuse it, on the parameter rather than on the function, with a message that
says a const parameter fixes a value and a type parameter is erased, so a
parameter cannot be both. Cheap to check -- the same `type_hint_mentions_param`
predicate lowering already uses to decide erasure, read in the typechecker over
the AST hint.

Reconsider it only if `ConstValue::Type` ever arrives for const generics (tensor
dimensions are the stated motivation), and then as its own design rather than as
a consequence of this one.

## What this is worth

Honestly: not much on its own. Nothing in `sys/std` wants a const parameter, and
the combination has no waiting caller. The reasons to do it anyway are that the
refusal is a cliff a reader falls off for no reason they can see, and that five
of the six edits delete a special case rather than add one -- after them,
`ComptimeCall` differs from `Call` in exactly the way it should, by carrying the
instantiation metadata and nothing else.

The related item worth more is the one in
[Known issues](../issues.md#user-content-a-specialized-function-keeps-an-original-nobody-calls):
an AOT build emits originals nothing calls. That is independent of any of this.

## What was done

The six edits above, as written, plus one the plan did not anticipate. Nothing
in the reasoning had to change; the estimate of what it would take was right.

**The accessor is `Instruction::call_target` and `call_target_mut`**, and it
turned all three edge-collection sites into one line each. The mutable one hands
back the callee reference, the type arguments and the descriptor slot together,
which is what `resolve_call_descriptors` wants and what stops it matching on the
instruction again.

**A seventh edit, found rather than planned, and a real bug.** Dead code
elimination did not list `ComptimeCall` in `has_side_effects`, so it read as
pure. A void function's `dest` is never used, which meant **a void function with
a const parameter had its call removed and was never run** -- silently, with
nothing to see but missing output:

```datalove
fun shout(const n: int, s: string)
    debuglog s
end fun

const ONE: int = 1
shout(ONE, "hello")      // printed nothing
```

Reproduced against unmodified `master`, so it predates all of this and is not
about generics: the call is simply gone from the IR. It is the same species as
the six above -- a pass that did not know a comptime call is a call -- which is
why it turned up here, on the generic `out` parameter case where the callee
returns nothing. One line, and `specialize_differential/032_generic_comptime`
and `backend/22_comptime_and_generic` both cover it now.

**The unplanned edit is the inliner.** `datafun-inline` copies a `Call` into
another function's body and blanks its `shape_descriptors`, because
`DescriptorRef::Own(i)` indexes the *enclosing* function's shapes and those
indices mean nothing once the body has moved. It now does the same to a
`ComptimeCall`, which keeps the two arms honest. Blanking without re-resolving is
a gap for generic callees either way -- that is pre-existing, is not reached by
anything today, and is not this change's to fix.

**The refusal is `TypeError::ComptimeParamOfGenericType`**, reported against the
parameter. It reads the *resolved* parameter type rather than the AST hint, since
the typechecker cannot depend on `datafun-ir` where `type_hint_mentions_param`
lives -- `datafun-common`'s `contains_type_param` says the same thing about a
`datalit::tycheck::Type`, and by then the type parameters have been seeded so a
`T` is already a `Type::Var`.

**Nothing needed the shape closure to run a second time**, as argued above. The
answers computed in 5a/b travel across the rewrite unchanged.

**What the fixtures pin down.** `backend/22_comptime_and_generic` runs a bare
`T`, a borrowed `[T]`, a collection built by a callee, a collection built in the
body, a generic comptime function forwarding to another, a const parameter after
the one that needs a descriptor (so the copy's renumbering is not the identity),
a void one and an erased `out`, and requires the interpreter, the jit and both
AOT backends to agree. `specialize_differential/032_generic_comptime`
runs the same shapes specialized and unspecialized, which is the half that
exercises a `ComptimeCall` reaching a backend with descriptors on it.
`tycheck_world/05_comptime_arg_errors` gained a `good_generic` beside the
narrowed `bad_generic`.

Each fixture was checked to fail without the fix: reverting the one line in
`rewrite_comptime_calls` brings the interpreter down on "a shape built with is
one this function declared".

**Two things stayed true that were worth confirming.** One copy serves every type
instantiation -- `rev(ONE, ref nums)` and `rev(ONE, ref words)` both reach
`rev__ct1`. And a generic function calling a comptime one, which was legal
throughout, still is.
