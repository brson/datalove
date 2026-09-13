# The state of worldgen

Written after being asked whether the generator could be extended to cover
generics, or whether it wanted redesigning.

**It wants extending, not redesigning.** What looked like a generator too
fragile to trust was three small bugs and one defensive clamp put in to route
around the first of them.

## What was wrong

**One-element tuple literals were printed `(x)`.** A 1-tuple needs the trailing
comma -- `(x,)` -- because `(x)` on its own is the expression `x` in brackets.
`mandocs/datalit.md` says so; the printer did not do it. A generated
`let v0: (u32) = (: u32 / 205)` then failed to typecheck against its own
declared type.

Someone met this and clamped `gen_type_hint` and `gen_type_alias` to
`TypeWeights::leaf_only()`, with the note "to ensure reliable typechecking".
That zeroes the weight of every compound type, so from then on **no generated
function signature or type alias held a list, a map, a set, an option, a
result, a tuple, a struct, an enum or a tensor**. The generator looked
trustworthy because it had stopped generating most of the language.

Measured, before and after fixing the printer, over the module graph:

| type weights | seeds failing typecheck |
|---|---|
| `leaf_only` (what it was doing) | 0 / 1000 |
| full, before the fix | 52 / 400 |
| full, after the fix | 0 / 2000 |

Of the 52, 51 were the 1-tuple. One was a knock-on arity mismatch.

**The script section imported the same name twice.** Every module names its
functions `fn0`, `fn1`, so two of them export the same name, and a second
import of a bound name is an error rather than a shadowing. `gen_module` had
worked this out and guarded against it, with a comment. `gen_script` never
did.

**A branch moved what was declared outside it.** `gen_if` saved the consumed
set, let the branch move whatever it liked, and restored -- so a linear value
moved in the then-branch and not the else, which is refused (D008). `gen_loop`
had met the same problem and solved it, by marking outer linear variables as
borrow-only for the length of the body. The same rule works for a branch.

The two of these together are what the dual-backend test was tripping over:

| | dual test, 20 seeds |
|---|---|
| before | 5 passed, 15 failed |
| after the import fix | 19 passed, 1 failed |
| after the branch fix | 20 passed, 0 failed |

None of the 15 were backend divergences. Both backends failed identically,
with a compile error, on a program the generator should not have written.

## What it found immediately

With compound types back in signatures, the dual test turns up a real
divergence within twenty seeds. Worldfile seed 2, twenty-three lines:

```
----------
module local/gen/core_0
----------

type Type0: u32
type Type1: i32

fun fn0(): string
  let v0: bool = false
  ret "with\"quote"
end fun

----------
scriptunit-fragment
----------

require module local/gen/core_0
import core_0.fn0

var v0: [bool] = [: bool / true, : bool / false, : bool / false]
var v1: [i32] = [: i32 / 73]
let v2: {} = {}

debuglog v2
```

The interpreter printed `{}` and exited; the cranelift AOT died on a signal.

It was the empty anon struct. `TyDescRef::struct_info` built a slice over the
field array with `slice::from_raw_parts`, and every emitter leaves that pointer
null when there are no fields to point at -- "No fields array needed for empty
tuple", as the cranelift one puts it. `from_raw_parts` wants a non-null aligned
pointer even for a length of zero, so this was undefined behaviour, and the
check that noticed it aborted the process. `debuglog` was the reader, through
`pretty_struct`.

The interpreter escaped because its descriptors come from a Rust-side builder,
whose empty run is a dangling-but-valid pointer rather than a null one.

And the same program run by `datalove aot-compile --run` escaped too, which is
why it looked at first as though the module mattered. It did not: that path
links the native component, built under `profile.native-component`, which
inherits `release` and so has the check compiled out. The dual test links a
runtime built in its own debug profile. The undefined behaviour was in both;
only the checking differed. Nothing was ever read through the empty slice, so
nothing was corrupted -- it aborted where the check was on and was silent
where it was off.

Answered in the accessor rather than the emitters: a run of no elements is
empty whatever the pointer says, and asking ten emitters across four backends
to invent a dangling address instead is the worse trade. The nine sibling
accessors that read a name or a variant list were the same shape and went the
same way.

## And a second one, twenty-odd seeds later

Seed 407, with a map literal naming a key twice:

```
let m: %{bool = u32} = %{false = : u32 / 118, false = : u32 / 75, true = : u32 / 41}
```

The interpreter gave `%{false = 118, false = 75, true = 41}` -- a map holding
one key twice. The AOT gave `%{false = 75, true = 41}`, which is right.

The interpreter built a set or map literal by sorting what the literal named
and handing the run to a bulk builder, which takes the run as given. Nothing
removed the duplicates. A set literal was worse in kind than a map's, since
`#{1, 1, 2}` came back holding `1` twice, and a set is a collection of unique
elements by definition.

Both now insert one at a time, which is what the erased paths beside them
already did and what the C backend was changed to do for the same reason. The
insert is what knows an element is already there, and what lets go of the one
it did not take -- which matters for a string, where the duplicate is a
separate allocation.

Neither of these has anything to do with generics. They are what was sitting
in the part of the language the generator had stopped writing.

## Generics

Added after the above. A generic body can do very little with a value of its
type parameter -- move it, drop it, clone it, print it, hand it on, put it in a
collection -- and anything more wants a bound. So rather than teach the
expression generator what a `T` is and hope it keeps to those rules, the bodies
come from a fixed set of shapes known to compile, and the variety goes where it
is worth having: in the types the call sites pick.

Twelve shapes, in `gen_generic.rs`: handing the value back, letting it go,
cloning it, wrapping it in an option, building a list of it from one element or
two, building a set, building a map from two parameters, borrowing a list of
it, and three bounded ones -- checked arithmetic under `fixedint`, bare
arithmetic under `float`, a comparison under `fixedint`. The set and map carry
`is ord`, which they must.

A call picks a concrete type for each parameter, respecting the bound, binds
every argument to a name carrying its type, and binds the answer. The argument
binding matters: a generic's parameter is fixed by what the arguments turn out
to be, and an unadorned `1.5` is an `f64`, so a call meant for `f32` would
quietly become one for `f64` and then disagree with the binding that named it.
Binding first also gives a `ref` parameter a place to borrow.

Generated code now looks like:

```
let a0: i32 = : i32 / 30
let a1: i32 = fn0(fn1(-(: f64 / 94.2)))
let v2: [i32] = gen1_1(a0, a1)

debuglog v2
```

2000 seeds typecheck, and 280 of 280 seeds pass the dual test with no
disagreement between backends, with every shape of type in play.

## What that turned up

**A function returning `()` could not be compiled at all.** Fixed here. `()`
maps to `IrType::Unit`, so the signature gets no sret pointer, but the body
still lowered `ret ()` into a value and the return went looking for somewhere
to put it. Two halves: `ret` now returns nothing where the return type is
unit, and a call to such a function gives its destination the address of its
own empty slot, since something may still read it -- `debuglog f()` does.

**Two shapes of tensor could not be compiled or could not be run.** Both
fixed; they were declined by the generator in between, and are not now. Both
are one mistake, made where a tensor literal copies its elements into a stack
buffer on the way to the runtime.

`StackSlotData::new` takes the log2 of an alignment rather than the alignment.
The tensor literal passed the element's *size*, and a size and a shift read
alike at a call site. An element eight bytes wide asked for a buffer aligned to
256 bytes, which is merely wasteful. One twenty-four bytes wide -- a tuple of a
`u32` and a `string` -- asked for sixteen megabytes, and the binary built and
then died where it ran, under AOT only. And a tensor whose elements are
tensors, the element being the tensor struct, came out past thirty-two, which
cranelift asserts against, so the function would not compile at all.

```
let v: [|(u32, string), 1|] = [| : (u32, string) / (: u32 / 6, : string / "hi") |]
let w: [|[|f32, 1|], 3|] = [| : [|f32, 1|] / [| : f32 / 1.0 |],, : [|f32, 1|] / [| : f32 / 2.0 |] |]
```

That explained what had looked like two unrelated bugs, and why a tensor of
*structs* holding a string was fine while the tuple was not: nothing about
tuples, only that the struct in hand happened to be narrower.

The same confusion was at every one of the thirty-odd stack slots the cranelift
backend declares, in both directions. Some asked for far more alignment than
they wanted and got away with it; a few asked for less -- a pointer slot
declared `0`, which is one byte, and two sixteen-byte slots whose comments said
"8-byte aligned" while asking for one. Nothing was corrupted, because what
cranelift gives a slot in practice was enough, but nothing guaranteed it. The
conversion is now written once, as `types::align_shift`, and every slot goes
through it.

`132_tensor_element_shapes` holds the shapes that could not be built.

**A `some` or an `ok` written with a type hint left its payload unchecked**,
and the lowerer came down on the missing type with "Expression must have type
from typechecker". Fixed here.

```
let v: data = data : ?u32 / some : u32 / 1
let w: error = error : ?u32 / ok : u32 / 28
```

`synthesize` took the hint's word for what the whole expression was and
returned, and nothing else ever visited what it wrapped. `er` did not have
this, because it checks its payload against `error`. It showed up under `data`
and `error` only because that is where a wrapper gets synthesized rather than
checked -- everywhere else the payload is checked against the surrounding
type. Checking the payload against the hint also means a hint that disagrees
with what it wraps is now an error rather than believed: `data : ?string /
some : u32 / 1` used to compile.

**A map built in a generic, at `data` on one side, dropped its entry and
leaked what was in it.** Fixed here. A collection built inside a generic is
made against the descriptor the call site handed over, so an element goes in as
what it really is, and an element in hand is in the erased shape -- a `data` --
and has to be unpacked on the way. The one case where the two readings coincide
is a collection whose elements really are `data`: the value in hand is already
what is wanted and unpacking it would take it apart.

A map has two sides and they can differ. `%{k = v}` in a generic over both,
called at `K := data`, has a key to leave alone and a value to unpack. The
insert entry read each side, saw they disagreed, and returned an error:

```
// The insert entries take both sides the same way, so a map erased
// on one side only has no path through here yet. Lowering refuses
// to build one, so nothing arrives in this state.
```

Lowering does not refuse, and things do arrive in that state. Nobody read the
error, so the entry silently went nowhere and everything in it leaked. It took
the leak checker to notice, because a map with nothing in it prints as a map.
`btreemap_insert_sides_impl` now takes each side its own way. Found by the dual
test, at seed 87113; `131_generic_map_at_data` covers it.

**The generator wrote mutually recursive functions with no base case.** Fixed
here. `gen_module` works out how far down the module a body may call, which is
what stops `fn1` calling `fn2` calling `fn1`, and `gen_function` then built a
fresh context for the body and did not carry it over -- so the limit was set
and never read. It overflows the stack, and under the chaos JIT it overflows it
inside JITted code, where what comes out is a garbage type descriptor reaching
the runtime ("not aligned to 1410787040 bytes") rather than anything that names
the cause.

Carrying the limit over exposed a second half: an import shadowed by a local
function of the same name was offered as callable as soon as that local was out
of reach, but writing the name still reached the local, and the call failed to
typecheck. The shadowing set now holds every local name rather than the
callable ones.

**One thing that was written down here as a bug and is not.** A tensor type
hint names its element type and its *rank*, not a dimension: `[|u32, 2|]` is a
rank-2 tensor of `u32`, whatever its shape turns out to be. So seed 47524's
`[|(u64, string), 1|]` holding two space-separated elements is a rank-1 tensor
of two, which is right, and `[|u32, 2|]` refusing `[| 1 2 |]` is a rank
mismatch, which is also right -- reported as `ArityMismatch { expected: 2,
actual: 1 }`, where both numbers are ranks. Read as a shape, each of those
looks like a hole. The error would be easier to read if it said so.

## Where that leaves it

The generator covers modules, functions, control flow, cross-module imports,
type aliases, every shape of type, and generics. What it
does not cover:

- **Riders and natives**, `match`, tables, `out` parameters, const parameters.
- **Generic bodies that do anything interesting.** The shapes are fixed. A
  generic that loops, branches, or calls another generic with its own
  parameter would reach the shape closure and the descriptor forwarding, which
  the fixed shapes only touch at one remove.

Two things about the harness itself:

- `worldgen_dual_tests` is behind `WORLDGEN_DUAL_TEST=1`, so it never runs in
  `just test`. At 5/20 it could not have. It now passes 280 seeds out of 280,
  so it could, at whatever number of seeds is worth the minute it takes.
- `test_1000_seeds_typecheck` is `#[ignore]`d for time. It takes about seven
  seconds.
- The typecheck-only tests do not run ownership analysis, so they cannot see a
  D008 or a leak. Only the dual test compiles and runs. That is why the
  branch-move bug survived in a generator whose seeds all "passed".
