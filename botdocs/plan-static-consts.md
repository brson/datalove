# Consts Built Once

A plan for giving a const of a linear type one home for the life of the
program, built once, and making every use of it a borrow of that home rather
than a fresh copy on the heap. A feasibility study, written 2026-10-06; nothing
here is implemented.

## Contents

- [Why](#user-content-why)
- [Where things stand](#user-content-where-things-stand)
- [The model](#user-content-the-model)
- [Why the runtime is ready](#user-content-why-the-runtime-is-ready)
- [Why lowering is mostly ready](#user-content-why-lowering-is-mostly-ready)
- [Backends](#user-content-backends)
- [The obstacle: function-local consts](#user-content-the-obstacle-function-local-consts)
- [The other passes](#user-content-the-other-passes)
- [The language decision](#user-content-the-language-decision)
- [Literals](#user-content-literals)
- [Order of work](#user-content-order-of-work)
- [Open questions](#user-content-open-questions)

## Why

A const of a linear type is rebuilt on the heap every time it is mentioned, and
every time the code mentioning it runs. A function that borrows a const list
allocates the list, borrows it and frees it on every call. Compile-time
evaluation saves computing the value but not constructing it, so a large table
computed at compile time is no cheaper to read than one computed at run time.

The semantics that make sense are that a const is an immutable ref binding to a
value fully constructed once, in storage that lives as long as the code that
names it.

## Where things stand

After compile-time evaluation the instruction that defined a const is replaced
by an `Instruction::Const` carrying the evaluated `ConstValue`
(`datafun-const/src/inline.rs`). A reference from a function to a module-level
or script-level const is lowered by `lower_var_operand` in
`datafun-lower/src/expr.rs` as a fresh `Const` of its own. A function that
reads a list const twice by value and once by borrow lowers to:

```
v3 = const [1int, 2int, 3int]   // the binding itself
v4 = const [1int, 2int, 3int]   // let a = LST
v5 = const [1int, 2int, 3int]   // let b = LST
v6 = pack Tuple2 {v4, v5}
drop v3
return v6
```

and a borrow is a borrow of such a temporary:

```
v0 = const [1int, 2int, 3int]
v1 = call @0 m3.u0(v0)          // list.len(ref LST)
drop v0
```

Every backend builds a heap value for each `Const` it executes:

- Cranelift (`datafun-cranelift/src/codegen/constants.rs`) keeps a string's
  bytes in static data but copies them to the heap with
  `dtlv_rti_string_from_bytes`, and builds a list by writing elements to a
  stack buffer and calling `list_build_from_slice`; sets and maps likewise.
- The C backend (`datafun-c-aot/src/codegen.rs`) creates a list with
  `dtlv_rti_list_create_local` and pushes each element.
- The interpreter (`write_const` in `datafun-interp/src/lib.rs`) builds from
  the `ConstValue` the same way.

Only string bytes and type descriptors are static today.

## The model

A const of a non-copy type gets one slot in a _const pool_ attached to the code
unit that owns it, built once from its `ConstValue` before any code using it
runs. A new IR instruction gives its address:

```
vN = staticref #k               // vN: Ref(T)
```

Every use goes through that ref exactly as a use of a `ref` parameter does.
Moving out of a const is an error, as moving out of a `ref` parameter is, and
`CONST@` lowers to `clone *vN` (see
[the language decision](#user-content-the-language-decision)). Copy-type consts
are unaffected and stay immediate `Const` literals.

## Why the runtime is ready

- **Values are plain pointer graphs.** `List`, `String`, `Int`, `Map`, `Set`,
  `Tensor` and `Table` in `rtdt/src/lib.rs` are `repr(C)` structs of pointers,
  lengths and capacities, with no reference counts and no type descriptor
  pointers. `Data` and `Error` are two words naming a static type descriptor
  and a value (`rtdt/src/anypack.rs`). The same layout is valid in any memory.
- **Reads do not write.** In `rti/src/table.rs` every read takes its source as
  `*const`: `eq`, `cmp`, `clone`, `debuglog`, the `len`, `get`, `contains`,
  `key_at` and `value_at` entries, `get_value_ref`, `tensor_get`,
  `table_get`. The `*mut` entries are mutations (`push`, `set`, `insert`,
  `remove`, `reserve`, `shrink_to_fit`), reachable only through `mut` and
  `out`, and a const is neither a `set` target nor a `mut` or `out` argument.
  `DropViaRef` is emitted only on the `out` and `mut` argument paths in
  `datafun-lower/src/expr.rs`. `tensor_slice_local` takes `*mut` but no
  backend emits it; indexing a borrowed tensor goes through `tensorindexref`.
- **Refs do not escape.** Borrows are not first-class, and every way of
  turning one into a value deep-copies. No pointer into a pool outlives the
  code that took it, so a pool only has to live as long as its code unit.
- **The allocator never sees pool memory.** `AllocLocal` in
  `rt/src/impls/alloc.rs` keeps per-heap free lists, and freeing a pointer it
  did not hand out would corrupt it, but a pool value is never owned by
  running code and so is never dropped by it. A clone out of the pool
  allocates on the caller's heap as any clone does.

## Why lowering is mostly ready

A `ref` parameter is already an immutable ref binding, and its uses are already
lowered as reads through the pointer, in every backend:

```
v1 = eq p1, v0                  // s == "x"
v3 = getfieldref p2.1           // p.n
v5 = add *v3, v4                // p.n + 1
v12 = listelementref p0[v7]     // xs[0]!
v13 = clone *v12                // xs[0]!@
v14 = clone p1                  // s@
```

along with passing it on as a `ref` argument and `tensorindexref` through
nested tensor indexes. A const mention lowered as `staticref` produces a value
of type `Ref(T)` that all of these accept as `Operand::ValueRef`.

For a module-level or script-level const the value is known before a function
naming it is lowered (`const_bindings` on `LowerCtx`), so the change is local
to `lower_var_operand`: emit `staticref` for a non-copy type instead of a fresh
`Const`, and register the value in the unit's pool.

## Backends

The first version builds each pool once at load, into memory that nothing
frees, reusing the code every backend already has for building a `ConstValue`
at an address.

- **Interpreter.** When a unit is loaded, build its pool with `write_const`,
  allocating from a dedicated `RtLocal` that lives as long as the unit.
  `staticref` writes the slot's address. The bytecode engine needs the
  instruction too.
- **JIT.** Same process, so it shares the interpreter's pool and embeds the
  address as an immediate. The pool must be built before the function is
  compiled and outlive the compiled code.
- **Cranelift AOT and C AOT.** Emit an init function that runs before the
  script, building each slot into a global with the existing const builders
  and a second runtime instance, created as `rt/src/c.rs` creates the first.
  `staticref` is the global's address.

Pool lifetime follows the code unit:

- Scripts run in parallel over shared modules only read the pool, which is
  complete before any of them starts.
- A REPL line or a reactive recompile replaces the unit, and its pool with it.
  Since refs cannot escape, nothing outlives the swap.
- Pools are destroyed at shutdown, or allocated with leak tracking off, so
  `DATALOVE_LEAK_CHECK` stays meaningful.

### Later: prebuilt images

For AOT the pool can instead be laid out at compile time as read-only data
with relocations, which costs nothing at startup, shares pages between
processes, and faults on a write. That needs a serializer from `ConstValue` to
every runtime layout. B+tree nodes are the hard part, and `rtdt` already has
`compute_map_*_node_layout` and its set counterparts. One way is to run the
existing builders against an allocator that bump-allocates into an image
buffer, then find the pointer fields with the same type-descriptor walk that
clone and destroy use, and emit a relocation for each. Cranelift's
`DataDescription` takes data relocations; C can express them as address
constants in static initializers. Nothing in the first version depends on this.

## The obstacle: function-local consts

A function body is lowered once, before its consts are evaluated, and the
inline pass writes the values in afterwards (phases 2 to 4 in the compiler
guide's script pipeline). By then the uses of a function-local const are
shaped as uses of an owned binding:

```
v3 = const [1int, 2int, 3int]
v4 = call @0 m3.u0(v3)          // ref LST
drop v3
```

Turning `v3` into a `Ref` after the fact means retyping it and deleting its
drops, which is fragile. The cleaner fix is to lower the function again once
its consts have values, so they arrive in `const_bindings` like module-level
ones. There is precedent: a function that names a module-level const not yet
evaluated already fails with `BindingNotAvailable` and is lowered again.

Const parameters go the same way. Specialization writes the value into the
copy, and for a non-copy type that becomes a pool entry and a `staticref`.

## The other passes

Every pass over IR has to know the instruction and keep the pool with its unit:

- display and serialization in `datafun-ir`
- dead-code elimination (`datafun-const/src/dce.rs`), which must not count
  `staticref` as having side effects
- const inlining (`datafun-const/src/inline.rs`)
- the inliner (`datafun-inline`), which moves instructions between units and
  must carry pool entries with them, or point at the source unit's pool
- specialization (`compiler/src/specialize.rs`)

## The language decision

Two semantics were considered for a by-value use of a const of a non-copy
type:

- **Option A: an implicit clone.** The current surface semantics, with only
  borrows and reads made free. Purely an implementation change.
- **Option B: moving out of a const requires `@`,** as moving out of a `ref`
  parameter does.

**Option B is chosen** (2026-10-06). A const is an immutable ref binding in
the language as well as in the implementation, so its rules are the rules of a
`ref` parameter, and the cost of a clone is visible where it is paid. A const
parameter of a non-copy type is the same: inside the specialized body it is a
ref binding, and `ret xs` becomes `ret xs@`.

What this changes:

- **Ownership analysis** treats a const mention as a borrowed place. A move out
  of one is a diagnostic in the D003 family, with the same help that D001
  gives: insert `@`, at the place to insert it. Auto-adapt inserts it.
- **`match`, the destructuring `if`, and `let` destructuring** move their
  scrutinee, and on a `ref` binding each is D003 today. No form takes apart a
  borrowed enum, option or struct without cloning it, so until borrowing forms
  exist these are written `match CONST@`, `if CONST@ |x|` and
  `let {a, b} = CONST@`. Borrowing `match` and `if` are worth having for `ref`
  parameters regardless, and once they exist the `@` goes away in the common
  case.
- **Source compatibility.** Existing code that moves a linear const, such as
  `let a = LST`, stops compiling and needs `LST@`. Copy-type consts, which is
  every const in `sys/std` today, are unaffected.
- **Documentation.** `botspec.md` (the "Const" paragraphs under bindings, and
  8.4), `mandocs/const-eval.md` and the constants section of
  `mandocs/datafun.md` all said that reading a const does not move it and needs
  no `@`. `botspec.md` is updated; the two in `mandocs` are not yet.

The rule does not depend on pools: it can land first, on its own, against
today's lowering, where `CONST@` is a clone of a fresh `Const`. Landing it
first means the source change is made once, and the pool work after it changes
no observable behavior.

**Landed** (2026-10-06), as described in the compiler guide's ownership
section. Two things it needed that this plan did not foresee:

- **Const arguments.** A const argument is a bare const name in a by-value
  position, but it is not a move. Ownership reads `call_targets` for the
  callee's const parameters and skips those arguments. Lowering gives the call
  a copy of its own (`lower_comptime_arg`), because an unspecialized call
  consumes the argument and a specialized one drops it.
- **Lowering's per-read copies are gone.** Lowering used to clone a
  function-local const or a const parameter at each read, so that reading did
  not consume it. With moves refused, a borrow now uses the binding itself, and
  `@` is the one clone. A module-level or script-level const named from a
  function is still a fresh `Const` per mention, which is what pools replace.

## Literals

Non-copy literals have the same problem as consts, more often. `s == "x"`
lowers to

```
v0 = const "x"
v1 = eq p1, v0
drop v0
```

and allocates and frees a string each time it runs; bigint literals likewise
(`v4 = const 1int`). Once pools and `staticref` exist, any non-copy `Const`
whose value is only read can be pooled. In ordinary code that is likely a
larger win than named consts.

## Order of work

1. Done. The ownership rule: moving out of a non-copy const or const
   parameter requires `@`, with fixtures and documentation updated to match.
2. Pools and `staticref` for module-level and script-level consts, on all four
   backends.
3. Function-local consts, by lowering again once values are known, and const
   parameters through specialization.
4. Pooling read-only non-copy literals.
5. Borrowing `match` and destructuring `if`, which remove most of the `@`s
   step 1 adds.
6. Optionally, prebuilt read-only images for AOT.

## Open questions

- Whether a pool belongs to the code unit or to the module. Per unit is
  simpler; per module avoids duplicating a const that several units inline.
- What the inliner does with a `staticref` it moves across units: copy the
  entry into the destination's pool, or refer to the source's.
- Whether the bytecode engine wants `staticref` as an op or resolves it to a
  constant address when compiling to bytecode.
- How large a const has to be before building it at startup is worse than
  building it on use, if any are never read. Lazy construction behind a
  once-flag is an alternative for the AOT init function.
