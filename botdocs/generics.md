# Generics: how it works

A generic function is compiled once, whatever it is called with. Inside it, a
value whose type mentions a type parameter is a pointer at the value in its
real layout together with that value's *type descriptor* -- the runtime's
`TyDesc` -- and the descriptors the body needs are built at run time from one
descriptor per type parameter, which the caller hands over.

This describes the implementation as it stands in October 2026, on all four
engines. The language-level rules are
[Section 8.7 of the spec](botspec.md#user-content-87-generic-functions). How it
got here is [Generics by Reference](plan-fat-generics.md), which replaced the
erasure that [Generics and Specialization](plan-generics.md) describes.

## Contents

- [The shape](#user-content-the-shape)
- [Types in the IR](#user-content-types-in-the-ir)
- [Descriptors](#user-content-descriptors)
- [Storage](#user-content-storage)
- [Calls](#user-content-calls)
- [Operations on a dynamic value](#user-content-operations-on-a-dynamic-value)
- [Natives](#user-content-natives)
- [What the runtime provides](#user-content-what-the-runtime-provides)
- [What each engine does](#user-content-what-each-engine-does)
- [What is refused](#user-content-what-is-refused)
- [Why not monomorphization](#user-content-why-not-monomorphization)
- [Where the tests are](#user-content-where-the-tests-are)

## The shape

- A **dynamic type** is an IR type that mentions a type parameter: `T`,
  `(T, u32)`, `?T`, `[T]`, `%{K = V}` (`IrType::is_dynamic`).
- A **dynamic value** is a value of a dynamic type. In a frame it is a
  **pointer**, one word whatever the type, at the value in its real layout.
  What describes it is the function's **descriptor table** entry for its
  type, the same for every value of that type throughout a call.
- The value lives on a **shadow stack** owned by the runtime, or in the
  caller's frame for an argument.
- A **reference to a dynamic type** is a pointer at storage someone else owns,
  described by the table entry for what it points at. It gets no storage of
  its own.
- A generic takes **one descriptor per type parameter**, after its ordinary
  arguments. Anything compound the body needs is built from those.

Nothing is converted at a boundary. A `u32` passed for a `T` is a pointer at
the `u32`; a `(u8, u32)` passed for a `(T, u32)` is a pointer at the
`(u8, u32)`. Code outside generics is untouched: a type that mentions no type
parameter is laid out and passed as it always was.

The invariant the whole thing rests on:

> A value's descriptor is always to hand: its static type's, or its type's
> entry in the function's descriptor table. A static type describes a layout
> only where it mentions no type parameter.

## Types in the IR

`IrType::Param(u32)` is a type parameter by position, kept inside the types
around it: `(T, u32)` is `Tuple([Param(0), U32])`. A function's parameter and
return types are as written (`IrType::from_type_hint_generic`), and so is every
value and slot type in its body.

Layout has one rule for them: a dynamic type is laid out as a pointer. The
static offset helpers (`aggregate_field_offsets`, `option_payload_offset` and
the rest) are never asked about a dynamic type; every site that would ask
takes the descriptor path instead.

`IrType::Data` is the language's own `data` type and nothing else. `T` bound to
`data` is a pointer at a `data`, like any other binding.

## Descriptors

**The table.** `FunctionContext::descriptor_table` lists the dynamic types the
body uses, each a type expression over the function's own parameters
(`ir::descriptor_table`, computed when the unit is lowered). At entry the
function fills a table of their descriptors. Every instruction that needs the
descriptor of a dynamic type reads its entry. A type parameter's own entry is
the descriptor handed over for it.

**Building.** Each entry has a **template**: the static descriptor of its type
expression, with a `TyTag::Param` descriptor wherever a type parameter stands.
Filling the table asks the runtime to instantiate each template against the
type-parameter descriptors (`dtlv_rti_tydesc_instantiate`), and the runtime
caches what it builds per runtime handle.

**The one-entry cache.** Each function also keeps the last call's
type-parameter descriptors and the table they built. A call repeating the
binding costs a compare per type parameter and skips the fill. Every engine
has one: a writable data object in Cranelift, a `static` in C, and
`IrLayout::table_cache` in the interpreter. A call site alternating between
two bindings refills every time; see
[A generic's descriptor-table cache holds one binding](issues.md#user-content-a-generics-descriptor-table-cache-holds-one-binding).

**Call sites.** A `Call` carries `bindings`, what the site binds each of the
callee's type parameters to, written over the caller's own. Each becomes a
descriptor: a static one when the binding is concrete, or the caller's own
table entry when it mentions the caller's parameters, forwarding included. The
interpreter resolves that once per site, as a `BindingRef` (`Static` or
`Table`).

Nothing about a callee's descriptors comes from its body, so a callee's
calling convention depends on its signature alone, and a call at a growing
type -- `f<T>` calling `f<[T]>` -- builds one more descriptor per level of
recursion. Every type parameter still has to be determined at a call site,
since the caller has to have a descriptor to pass.

## Storage

**Each dynamic value and slot owns its storage.** A move copies `desc.size`
bytes into the destination's storage rather than handing over the pointer,
since a value defined in a loop reuses its storage every iteration.

**The dynamic frame.** After filling the table, a function with dynamic values
bumps the shadow stack once for all of them, each sized and aligned from its
descriptor, and points each dynamic place there. The shadow stack grows with the
number of IR values, never with the iteration count.

**Release.** The function records the shadow stack's top and limit at entry
and restores both on every return; the stack grows by chunks, so restoring
only the top would leave the limit in a newer chunk. Unwinding the interpreter
gives back each popped frame's space. A datalove error or a panic that skips
compiled epilogues is covered by the runtime's `reset`, which resets the
shadow stack to its base.

**Values that leave** are copied out: a return into the caller's storage, a
push into a collection, a store into an aggregate. Nothing keeps a pointer
into a frame's shadow space after it returns.

**Block arguments are a parallel move.** Copying bytes one argument at a time
breaks if a jump passes a block's own parameters back to it permuted, so
lowering never emits one, and checks (`ir::permuted_block_args`).

## Calls

Per dynamic parameter, by mode:

| Mode | Passed | Who owns what |
|---|---|---|
| `ref` | a pointer at the caller's value | the caller, throughout |
| `mut` | a pointer at the caller's value | the caller; the callee writes in place |
| `in` | a pointer at the caller's storage | the callee owns the value; the caller frees the storage |
| `out` | a pointer at storage for the result | the callee writes; the caller destroys what was there first |
| return | a pointer at storage for the result | as `out` |

The callee does not take a descriptor per argument: its table entry for the
parameter's type describes it. Then come the type-parameter descriptors, in
declaration order.

**Which side gives an `in` argument up.** A type parameter is never a copy
type, but a copy value bound to one is copied, not moved. So for a dynamic
argument the caller asks the argument's own descriptor (`TyDesc::PLAIN`), not
the callee's type, and every engine marks a moved argument dead after the call
on that answer -- at run time in compiled code, natives included.

## Operations on a dynamic value

The instruction set has no generic-specific instructions; a backend
implements an instruction whose operand is dynamic from the descriptor.

| Operation | On a dynamic value |
|---|---|
| move, store, load, block argument | copy `desc.size` bytes into the destination's storage |
| move of a reference | copy the pointer |
| read through a reference | copy or clone `desc.size` bytes into the destination's storage |
| clone | `dtlv_rti_clone_local` into the destination's storage |
| drop | `dtlv_rti_any_destroy_local`; the storage goes with the frame |
| field, element, payload reference | `ptr` plus the part's offset, from the descriptor; the part's own table entry describes it |
| tuple, struct, option, term, enum, result construction | each part stored at its offset from the destination's descriptor |
| option and result tests | the tag, at offset 0 whatever the payload |
| list, set, map, tensor, table literals | built by the runtime from the table entry's descriptor |
| bounded operations (`fixedint`, `float`) | `dyn_ops` on `(ptr, tydesc)` pairs |
| the order an `ord` bound admits | the runtime's structural comparison, `dtlv_rti_cmp_total_local`, reached through `sys/std/ord`'s natives and the set and map operations |
| `debuglog` | the runtime's printer on the pair |

What a reference to a dynamic type points at is the table entry for its
pointee type, so a projection works out only where the part is.

## Natives

The rider ABI is `(ptr, tydesc)` per argument, the result through an out pair,
and then a descriptor for each type parameter no argument determines -- one
appearing only in the return, like `fixedint_zero<T>(): T`.
`NativeContext::undetermined_type_params` lists those positions, and a call's
bindings at them are the descriptors (`NativeContext::undetermined_bindings`).
A generic calling a native passes each dynamic argument's pointer with its
table entry, and the trailing descriptors come from its table.

A rider is never handed a `data` standing in for something else: a result slot
is in the real layout, and an argument is what the caller has. See
[The Native Rider ABI](native-abi.md).

## What the runtime provides

| Function | Answers |
|---|---|
| `dtlv_rti_tydesc_instantiate` | a template's descriptor for a binding, cached per handle |
| `dtlv_rti_field_offset` / `dtlv_rti_field_tydesc` | where a tuple or struct field is, and what it is |
| `dtlv_rti_payload_offset` / `dtlv_rti_payload_tydesc` | the same for an option's, a result's, a term's, or an enum variant's payload |
| `dtlv_rti_shadow_alloc` | shadow stack space when the current chunk has no room |
| `dtlv_rti_mem_alloc_raw_local` / `dtlv_rti_mem_free_raw_local` | scratch for a tensor literal of a dynamic element type |

The shadow stack's top and limit are in `RtHeader` (`shadow_top`,
`shadow_limit`), so compiled code bumps and restores inline and calls the
runtime only when a chunk is full. It is per runtime handle, so reentrant.

These are the runtime's rather than open-coded per backend for one reason:
four derivations of one layout rule is where the bugs come from.
`TyInfoTuple` is `{num_fields, fields}` and `TyInfoStruct` is
`{fields, num_fields}`, and an option's payload, a result's and an enum
variant's are each placed by a different rule.

Built descriptors are cached by the template's address and the parameters'
addresses, outside the handle's allocator so `reset` does not free them. A
miss for a type that has a static descriptor elsewhere builds a duplicate,
which is harmless: descriptors compare structurally (`same_tydesc`).

## What each engine does

**The interpreter.** A dynamic value or slot has a null static descriptor in
`IrLayout`, and the frame accessors (`Frame::value`, `value_dest`, `slot`,
`slot_dest`) read the pointer and the table entry (`value_entries`,
`slot_entries`) when they see one, and hand the walker a `(ptr, tydesc)`
pair, so its instructions, already driven by the descriptor in hand, mostly
needed nothing. A reference to a dynamic type is described by the entry for
what it points at. `enter_dynamic` fills the table and takes shadow space
after `Frame::enter`; `leave_dynamic` gives it back. A parameter is one
word too, a pointer at the argument, described by the callee's own
descriptor or its table entry (`IrLayout::param_places`): the interpreter's
calls hand over pointers, as compiled code's do.

**The bytecode** runs a generic's body: an instruction on a dynamic value runs
on the walker (`Op::Ir`), except a move and a return (`CopyDyn`,
`ReturnDyn`, which name the frame word holding the descriptor: a table entry
or a parameter's). A call with bindings is planned like any other, and a generic
wrapper around a native is forwarded straight to the native.

**Cranelift**, AOT and JIT (`codegen/dynamic.rs`). A dynamic value is an
aggregate whose Cranelift value is the address of its shadow storage. Its
descriptor is its table entry (`tydesc_value`); asking for a static descriptor
of a dynamic type is an error, so a site that wanted a symbol fails to
compile rather than compiles wrong. Sizes come from the descriptor
(`value_size`, `copy_value_bytes`), offsets from the runtime.

**The JIT** compiles through the same codegen. The bridge into JIT code and
the trampoline out of it carry the type-parameter descriptors after the
arguments, and so does a planned call from the bytecode into JIT code. A
function whose arguments and descriptors together exceed `MAX_DIRECT_ARGS`
words is left to the interpreter. OSR reads the table and the dynamic places
from the interpreter's frame (`OsrSpec::table_offset`).

**C AOT** follows Cranelift's choices in C. The signature ends with one
`const dtlv_tydesc_t* tp{i}` per type parameter; the prologue declares the
table as locals `__t{i}`, fills it through a `static` one-entry cache, and
bumps shadow space inline (`dtlv_shadow_bump`). A dynamic place is a pointer
in the frame, and a dynamic type's descriptor is `(*__t{i})` wherever a static
symbol's name would go.

Both compiled backends have a gate, `check_dynamic_support`, refusing a
function that uses an instruction on a dynamic value their codegen does not
handle, by name. It admits everything `sys/std` and the fixtures use. Under
the JIT a refusal leaves the function to the interpreter (`--jit-stats` lists
it, `ENGINE_TESTS_JIT_REFUSALS=1` makes the engine suite print them); under
AOT it fails the build.

## What is refused

**No bounds, so no operations on a `T`.** A value of a bare type parameter can
be moved, dropped, cloned, printed, stored, returned and handed on. `x + y` and
`x == y` are F026 whatever the call site binds. Printing works because the
descriptor travelling with the value is enough to format it.

**A bound admits only its own operations.** `float` and `fixedint` give the
operators their types share, dispatched on descriptors through `dyn_ops`;
bare `+` under `fixedint` is still F026. `ord` gives no operator at all --
`.<` and `==` on a `T is ord` are F026 -- only the order `sys/std/ord` and
sets and maps use. `==` on an aggregate holding a type parameter is F026 even
when the parameter is bounded. A call site that binds a bounded parameter to a
type the bound does not admit is F016.

**A set or map over a `T` with no `ord` bound** is F061.

**A const parameter whose type mentions a type parameter** is F077:
specialization makes one copy per const value, and that copy is still generic.

**A type parameter no argument or expectation determines** is F011, since the
caller has no descriptor to pass. The types cannot be written at a call
either: `f<u32>(1)` is not a call.

**An instruction a compiled backend cannot do on a dynamic value** is refused
by `check_dynamic_support` (see the engines above): under the JIT the function
is left to the interpreter, under AOT the build fails.

## Why not monomorphization

A dynamic value costs at run time: a `T` is read through a pointer, a move is
a copy of `desc.size` bytes, a call fills a descriptor table (or compares
against its cache), and type-directed operations are calls into the runtime.

What it buys is what a language with a REPL needs. Compile time is O(1) in the
number of instantiations -- a new one appearing in the REPL compiles nothing.
There is nothing to invalidate, because a generic function is one salsa-tracked
entity, and a generic's body never contributes to its callers' calling
convention. Monomorphization needs to see every call site before it can emit
the callee, which is exactly what a REPL cannot give.

Monomorphizing what is *hot* is [Specializing
Generics](plan-monomorphization.md): an optimization over a complete
implementation, not a different design.

## Where the tests are

`crates/datalove-cli/tests/fixtures/backend/` runs the interpreter, the JIT and
both AOT backends and compares them, so a recorded output is what all four
agree on. The generic ones:

| Fixture | Covers |
|---|---|
| `06_out_params` | `out` through a generic |
| `07`-`09` | owned containers, tuples, terms and enums through a generic |
| `10_generic_indexing` | reading an element whose type a generic cannot name |
| `15_borrowed_generic_aggregate` | field offsets from the descriptor under `ref` and `mut` |
| `16`-`19`, `21` | element and part borrows and writes, map and tensor indexes |
| `20_generic_argument_from_a_var` | moving a var into a generic |
| `22_comptime_and_generic` | const and type parameters together |
| `23_descriptors_across_jit` | a JIT-compiled caller handing over descriptors |
| `28`, `29`, `34` | which side drops an `in` argument, in and out of loops |
| `30_generic_return_bound_to_unit` | a dynamic return bound to `()` |
| `31_generic_enum_payload` | enum payloads sized from the descriptor |
| `32_tensors_and_tables_over_a_type_parameter` | tensor and table literals over a `T` |
| `33_growing_types` | calls at a growing type |
| `35_alternating_bindings` | the one-entry cache missing every call |

`crates/datalove-datafun/tests/fixtures/std_tests/` has the language-level
coverage: `102`-`105` (forwarding), `113` (return-position inference),
`114`-`120` (building collections, recursion, deep types), and the bound
tests `124`-`127`. Engine fixture `module_068_growing_type` is a growing type on
every engine, and `deep_recursion_tests.rs` checks that generic recursion
under the JIT is not bounded by the Rust stack.

Four engines agreeing is load-bearing here. Most faults in this area have
been silent -- a wrong answer, not a crash -- and some were identical in all
four, which comparing them does not catch. Those need a fixture that says what
the answer should be.
