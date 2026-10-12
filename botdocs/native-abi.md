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

  What a holder of a handle may assume is `datalove_rti::RtHeader`, which the
  runtime's state starts with: the table, then two fields compiled code reads
  and writes in place (whether its calls are being abandoned for an error, and
  the limit to the native stack). A rider reads only the table. The rest of
  what the handle points at belongs to the runtime.
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

Every pointer travels with a descriptor, and the descriptor says what the value
is. In a non-generic caller it is a static one: a `u32` argument arrives as a
pointer at four bytes with the `u32` descriptor beside it. A generic caller is
compiled by reference and hands over its descriptor-table entry for the type
it has, so the descriptor is just as truthful there; a native never sees a
`data` standing in for a type parameter.

A native's own signature says `data` where it was written over a type
parameter (`NativeContext::param_types`), because one compiled native takes any
type there. That is the native's business only; what it is handed is described
by the descriptor beside it.

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

The call site substitutes what it bound the native's type parameters to into
each of those types and hands over the descriptor of the result: a static one
where the binding is concrete, and the caller's own descriptor-table entry
where it was written over the caller's type parameters. So

```
fun zero<T>(): T with { T is fixedint, }
  ret fixedint_zero()
end fun

let a: u8 = zero()
```

works: `zero` hands `fixedint_zero` its entry for `T`, which the script filled
with `u8`'s descriptor. And

```
ret self == zero()      -- refused
```

does not: an operand is read for what it is, not checked against what is
wanted, so nothing on that line says what `T` is. The typechecker refuses it.

## Writing the result

`result_out` points at the destination slot and `result_tydesc` describes it,
truthfully, as for an argument. A native writes a value of that type there.

## Ownership

A value a native is handed by value (`in`) is the native's. A value the native
moves into the runtime -- pushing it, setting it, inserting it -- is the
runtime's whatever the status: an entry that cannot place it (an index past the
end, a key the map does not have) destroys it and reports `Error`. So a native
never has to clean up after a refused move, and nothing is destroyed twice.

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

## Borrowing a `data`

A real `data` value carries its own descriptor. A native that wants to read
what one holds uses `dtlv_rti_data_borrow`, which lends a pointer at the value
and its descriptor. A value the `data` keeps on the heap lends the address it
has. A narrow scalar is packed into the `data`'s own words instead; it has no
address to lend, and the narrowest carry no descriptor beside them either --
only a tag. So `data_borrow` unpacks one into eight bytes the caller lends and
takes the descriptor from `rtdt::packed_tydesc`, which is a constant per type
rather than anything made up at run time.

One consequence: a descriptor reached that way is one of those runtime
constants rather than the one the compiler emitted. Descriptors were never
deduplicated, so nothing may compare them by address; `same_tydesc` in the
runtime compares them structurally.

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
