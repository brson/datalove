# The native rider ABI

How a compiled datafun program calls a function written in Rust.

A rider is a package of native functions. Its signatures are declared in a
`.dli` interface file, its implementations are `extern "C-unwind"` functions in
a Rust crate, and the two are joined by a linker symbol. Every backend --- the
interpreter, the two cranelift backends and the C backend --- makes the same
call, so there is one ABI here rather than four.

## The shape of a call

```c
uint8_t dlr_<rider>__<function>(
    void*                 rt,
    void*                 a0, const dtlv_tydesc_t* t0,
    void*                 a1, const dtlv_tydesc_t* t1,
    ...,
    void*                 result_out, const dtlv_tydesc_t* result_tydesc,
    const dtlv_tydesc_t*  s0, ...);
```

In order:

- **`rt`** --- the runtime handle. Every `dtlv_rti_*` entry point takes it
  first, so a native that calls back into the runtime passes its own along.

  It is also how a native *finds* those entry points. A rider does not link
  the runtime --- it would otherwise carry a second copy of one --- so it
  cannot call them by name. The handle points at the runtime's state, whose
  first word is a table of its functions, and `datalove_rti::call` wraps
  reading it: `call::dtlv_rti_string_from_bytes(rt, ..)` looks up the table on
  `rt` and calls through it, passing `rt` on as the first argument. A rider
  library therefore resolves nothing when it loads, and the process loading it
  exports nothing.

  That first word is all a holder of a handle may assume. The rest of what it
  points at belongs to the runtime.
- **One `(pointer, descriptor)` pair per declared parameter**, in declaration
  order. The pointer is at the value; the descriptor says what is behind it.
- **The result, as an out parameter**, given the same way: somewhere to write
  it and a descriptor for what goes there. There is no C return value for the
  result --- see the status below.
- **Zero or more trailing descriptors**, one for each type parameter that no
  argument determines. Nearly every native has none. See *Return-position type
  parameters*.

The `uint8_t` the function returns is an `RtStatus`: `1` is ok, anything else
is a failure, and on failure nothing was written to `result_out`.

The symbol is `dlr_{rider}__{function}` --- the rider's name as written in
`require rider`, two underscores, the function's name. It is spelled in one
place, `datalove_datafun_ir::native_symbol`, which `NativeContext::new` uses;
the compiler names the rider and the function and never the symbol, and every
backend reads it back with `NativeContext::symbol`.

## Where the type comes from

Every pointer travels with a descriptor, but the descriptor describes the
**slot**, not necessarily the value. That distinction is the whole of what
follows.

In a non-generic caller the two are the same thing: a `u32` argument arrives as
a pointer at four bytes with the `u32` descriptor beside it.

Inside a generic the slot is `data`, because that is what erasure compiled. So
both `t0` and `result_tydesc` read `Data` there, and neither says what the type
parameter turned out to be. A native still works, because the type rides in the
**value**: an argument of a type parameter is a `data`, and a `data` carries its
own descriptor. `dtlv_rti_data_parts` reads through one; the helpers in
`impls::dyn_ops` do it for the numbers.

So the rule for reading an argument is: look at the slot descriptor first, and
if it says `data`, the value inside says the rest.

Two conveniences sit on top of that, both decided by the caller:

- A **borrowed** collection is unwrapped before the call, so a native declaring
  `ref self: [T]` gets a pointer at the collection itself with a real
  descriptor for `[T]`, not a wrapper around it. Decided by the parameter's
  mode rather than its type, because a generic native's declared types are
  erased too.
- A **forwarded** parameter --- one the calling generic itself received a
  descriptor for --- is passed with that descriptor rather than the static one.

## Return-position type parameters

A native reads a type off a descriptor that came with a value. A type parameter
that appears only in the return type has no such value, so nothing in the call
would say what to build:

```
native fun fixedint_zero<T>(): T with { T is fixedint, }
```

For those, the call site hands over a descriptor explicitly, after the out
parameter. Which parameters need one is worked out from the signature: a type
parameter mentioned anywhere in the arguments is determined, because a
descriptor is structural and `[T]` names its element type just as `?T` names
its payload; one mentioned nowhere in the arguments is not.

The answer is recorded on `RiderInterface::generic_functions` when the `.dli` is
parsed, and again on `NativeContext::descriptor_shapes` when the code unit is
built --- the same computation from the same signature, so the two agree about
how many trailing arguments there are.

The call sites are found by the same shape closure that serves ordinary
generics (`close_shapes`). A native is seeded into it as a fixed point: it makes
no calls of its own, so nothing is ever added to its set. What propagates is the
*requirement* --- a generic that calls `fixedint_zero()` needs a descriptor for
`T` itself, and so does its caller, until some call site names a type outright.
That is why

```
fun zero<T>(): T with { T is fixedint, }
  ret fixedint_zero()
end fun

let a: u8 = zero()      -- names `u8`, so the chain ends here
```

works, and why

```
ret self == zero()      -- refused
```

does not: an operand is read for what it is, not checked against what is
wanted, so nothing on that line says what `T` is. The typechecker refuses it
rather than standing in a `data` descriptor, which would build the wrong thing.

## Writing the result

`result_out` points at the destination slot and `result_tydesc` describes it.
Where the slot is `data` --- which it is whenever the caller is a generic that
cannot name the type --- the value has to be **wrapped** on the way in.

`dtlv_rti_data_from_local` does this, and decides for itself whether the value
packs into the `data`'s own words or goes on the heap.

There is a wrinkle behind that entry point worth knowing if you write one of
the runtime's own helpers. The narrow scalars pack into a `data`'s two words
with **no descriptor at all** --- the tag rides in the second word --- so a
value taken back out of one has no descriptor to hand back in.
`impls::boxing::data_from_local_tagged` takes the tag beside the pointer and
only reads the descriptor when the packing needs it; that is a Rust-side
helper, not part of this C ABI.

## Declaring a native

Three places have to agree, and all three derive from the same `.dli` line.

1. **The interface**, in `sys/<package>/rider/rider.dli`:

   ```
   native fun fixedint_zero<T>(): T with { T is fixedint, }
   ```

2. **The implementation**, in the rider crate:

   ```rust
   #[no_mangle]
   pub extern "C-unwind" fn dlr_std__fixedint_zero(
       rt: *mut u8,
       out: *mut u8, out_td: *const u8,
       value_td: *const u8,
   ) -> u8 { ... }
   ```

   The parameters are pointer-sized and untyped on the Rust side; cast the
   descriptors to `*const rtdt::TyDesc` at the top.

3. **A module that imports it**, which is what surface code can reach:

   ```
   require rider std
   import std.fixedint_zero
   ```

   A rider is visible only to modules in its own package. A script reaches one
   through a module that re-exports it.

## Borrowing a type parameter

A `ref T` parameter is read through its wrapper before the call, so the native
gets the thing itself with a real descriptor. That works for a value the
wrapper keeps on the heap, which lends the address it already has.

A narrow scalar is packed into the `data`'s own words instead. It has no
address to lend, and the narrowest carry no descriptor beside them either --
only a tag. So `dtlv_rti_data_borrow` unpacks one into eight bytes the caller
lends and takes the descriptor from `rtdt::packed_tydesc`, which is a constant
per type rather than anything made up at run time. The scratch lives in the
caller's frame and so outlasts the call.

One consequence: a descriptor reaching a native may be one of those runtime
constants rather than the one the compiler emitted. Descriptors were never
deduplicated, so nothing may compare them by address; `same_tydesc` in the
runtime compares them structurally, and the assertions that guard the
collection entries use it.

## Arity

The interpreter reaches a native through `call_c` in its `native` module,
given a flat array of pointer-sized words (`c_words` lays them out), and
transmutes to a function type of the right arity. The arities are listed one by one because there is no variadic
form that would keep the C ABI. A native wider than the widest listed arity
hits a `todo!()` rather than being called wrongly --- add the next row.

The cranelift backends build the same signature in `build_native_signature`,
and the C backend emits the same prototype in `native_declaration`. All three
count the parameters the same way: one for `rt`, two per declared parameter,
two for the result, one per trailing descriptor.
