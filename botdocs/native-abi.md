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

- **`rt`** --- the runtime handle. Most `dtlv_rti_*` entry points take it
  first, so a native that calls back into the runtime passes its own along.
  Those that need no runtime state -- `dtlv_rti_init`, `dtlv_rti_data_borrow`,
  and the layout queries `dtlv_rti_field_offset`, `dtlv_rti_field_tydesc`,
  `dtlv_rti_payload_offset` and `dtlv_rti_payload_tydesc` -- take none, and
  have no `call::` wrapper: a rider reaches them through
  `datalove_rti::table(rt)` directly.

  It is also how a native *finds* those entry points. A rider does not link
  the runtime --- it would otherwise carry a second copy of one --- so it
  cannot call them by name. The handle points at the runtime's state, whose
  first word is a table of its functions, and `datalove_rti::call` wraps
  reading it: `call::dtlv_rti_string_from_bytes(rt, ..)` looks up the table on
  `rt` and calls through it, passing `rt` on as the first argument. A rider
  library therefore resolves nothing when it loads, and the process loading it
  exports nothing.

  What a holder of a handle may assume is `datalove_rti::RtHeader`, which the
  runtime's state starts with: the table, then four fields compiled code
  reads and writes in place (`abandoning`, whether its calls are being
  abandoned for an error; `stack_limit`, the limit to the native stack; and
  `shadow_top` and `shadow_limit`, the shadow stack that holds values sized
  at run time). A rider reads only the table. The rest of
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
is a failure, and on failure nothing was written to `result_out`. No backend
reads the status yet -- the interpreter, cranelift and C all discard it -- so
a failure a native reports goes unnoticed and the caller reads an unwritten
result. A native that can fail should say so in its return type, an option or
a result.

The symbol is `dlr_{rider}__{function}` --- the rider's name as written in
`require rider`, two underscores, the function's name. It is spelled in one
place, `datalove_datafun_ir::native_symbol`, which `NativeContext::new` uses;
the compiler names the rider and the function and never the symbol, and every
backend reads it back with `NativeContext::symbol`.

## Where the type comes from

Every pointer travels with a descriptor, and the descriptor says what the value
is. In a non-generic caller it is a static one: a `u32` argument arrives as a
pointer at four bytes with the `u32` descriptor beside it. A generic caller
hands over its descriptor-table entry for the type it has, so the descriptor
is just as truthful there; a native never sees a `data` standing in for a type
parameter. See [Generics: how it works](generics.md).

A native's IR signature (`NativeContext::param_types`) keeps its type
parameters as `IrType::Param`, as a generic function's does. Whether an `in`
argument at one is given up is then the argument's own descriptor's to say,
since a copy value bound to a type parameter is copied rather than moved.

## Return-position type parameters

A native reads a type off a descriptor that came with a value. A type parameter
that appears only in the return type has no such value, so nothing in the call
would say what to build:

```
native fun fixedint_zero<T>(): T where { T is fixedint, }
```

For those, the call site hands over a descriptor explicitly, after the out
parameter. Which parameters need one is worked out from the signature: a type
parameter mentioned anywhere in the arguments is determined, because a
descriptor is structural and `[T]` names its element type just as `?T` names
its payload; one mentioned nowhere in the arguments is not.

The answer is recorded on `RiderInterface::generic_functions` when the `.dli` is
parsed, and again on `NativeContext::undetermined_type_params` when the code
unit is built --- the same computation from the same signature, so the two
agree about how many trailing arguments there are.

The call site hands over the descriptor of what it bound each of those type
parameters to (`NativeContext::undetermined_bindings`): a static one where the
binding is concrete, and the caller's own descriptor-table entry where it was
written over the caller's type parameters. So

```
fun zero<T>(): T where { T is fixedint, }
  ret fixedint_zero()
end fun

let a: u8 = zero()
```

works: `zero` hands `fixedint_zero` its entry for `T`, which the script filled
with `u8`'s descriptor. A call with nothing to say what `T` is, such as
`debuglog zero()`, is refused by the type checker (F011); the rules for what
decides it are the spec's ([Section 8.7](botspec.md#user-content-87-generic-functions)).

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
   native fun fixedint_zero<T>(): T where { T is fixedint, }
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
constants rather than the one the compiler emitted. Descriptors are not
deduplicated, so nothing may compare them by address; `same_tydesc` in the
runtime compares them structurally.

## Arity

The interpreter reaches a native through `call_c` in its `native` module,
given a flat array of pointer-sized words (`c_words` lays them out), and
transmutes to a function type of the right arity. The arities are listed one
by one, up to `MAX_C_WORDS` (32), because there is no variadic form that would
keep the C ABI. A native wider than that is a runtime error from `c_words`
rather than being called wrongly.

The cranelift backends build the same signature in `build_native_signature`,
and the C backend emits the same prototype in `native_declaration`. All three
count the parameters the same way: one for `rt`, two per declared parameter,
two for the result, one per trailing descriptor.
