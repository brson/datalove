---
title: "Native riders"
category: dev
summary: "Calling Rust from datalove through a universal package-level FFI"
---

Next big blocker is the ability to make runtime calls from std.

I have a runtime with an interface that is accessible to the compiler,
and we have intrinsics via the `icall` keyword,
but Datalove functions can't themselves call native code.

Typically at this stage one might introduce a simple mechanism
that allows std to declare the existance of foreign functions,
have the compiler always link in the runtime so std can call those functions.
The runtime functions needed by std just exist in the ether.

Then down the road we need to provide a robust mechanism for user-level packages
other than std.
I don't want std to be special, to whatever extent that is possible,
so I'm just going to design the general mechanism and have std use it from the start.

I call that mechanism _native riders_,
each package able to declare a Rust crate that rides along with it wherever it goes.


## The model

A rider is a Rust cdylib/staticlib crate that belongs to a datalove package.
One crate per package, for simplicity.
It has two parts: an interface file (`rider.dli`) declaring
native function signatures in datalove syntax,
and a Rust crate implementing them.

```datalove
// sys/std/rider.dli
native fun string_len(ref self: string): index
native fun string_split(ref self: string, ref delimiter: string): [string]
native fun string_parse_int(ref self: string): ?int
```

Modules opt in with `require rider` and then import
native functions like they'd import from any other module:

```datalove
require rider std
import std.string_split

fun split(ref self: string, ref delimiter: string): [string]
  ret string_split(self, delimiter)
end fun
```

`require` in Datalove is the syntax the compiler pre-scans
to determine inter-module dependencies,
distinct from `import`, which has only local effect.
In Datalove all inputs are known upfront,
including the module graph and all native interfaces.

We support only Rust for native code --
we can mostly count on being able to find and compile
Rust crates, and datalove is written in Rust anyway.

Datalove code isn't otherwise exposed to the nativeness
of native functions. After the `require` and `import`,
they just look like functions from a module.


## The calling convention

Everything is passed by pointer for simplicity.
Easy to expand the ABI later.
Every native function receives a runtime handle,
and for each declared argument, a pointer and a type descriptor.
Return values go through an out-parameter.

```rust
#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_split(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    delim_ptr: *const u8, _delim_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    // ...
}
```

The type descriptors on every argument will matter
once we have generics:
they carry element types, field layouts, everything
needed for polymorphic dispatch.
For now they're mostly there for consistency
with the existing runtime ABI.

The symbol naming convention is `dlr_{rider}__{function}`.
The compiler generates matching declarations.

## Execution backends

The interpreter loads the rider's shared library via `dlopen`,
looks up each `dlr_*` symbol,
and registers them in a native function table.
When it hits a call to a native code unit
it marshals arguments to the C ABI
and calls through the function pointer.

The JIT registers native symbols with the Cranelift JIT engine.
Each gets a local trampoline using `call_indirect`
with the absolute address baked in &mdash;
relocations are already greater than the 2GB limit imposed by Cranelift
so can't jump directly.

The AOT compiler treats native functions
as imported symbols with `Linkage::Import`.
The rider's static library gets linked alongside
the runtime at the final link step.


## Future work

So that's all implemented enough for std to work,
and now std contains a string module with all the basic APIs you might expect.

Most other standard library functions need generics
so that's the next big task.