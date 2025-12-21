# Datalove

An simple and expressive typed scripting language
for efficient data modeling and transformation,
with a batteries included standard library.

```datalove
let ada = {
  name = "Ada",
  born = 1815,
}

fun age(
  player: {
    name: string,
    born: u32,
  },
  now: u32,
): ?u32
  if now .< player.born
    ret none
  else
    ret now -? player.born
  end if
end fun
```




## A Tower of Love

Datalove is built from three cleanly-scoped strict
sublanguages of increasing power.




### Datalove Literals ("Datalit")

> File extension `.dlt`

The tiny and comprehensible foundation of Datalove,
a strongly-typed and declarative pure-data language
for expressing most typical data structures:

- booleans, fixed integers and bigints, floats
- anonymous tuples, structs, and enums
- lists and strings, maps and sets
- option and result
- `data` of any of the above, with runtime introspection and reflection
- `error` of any of the above, but for error handling
- (aspirational) datetimes, subrange ints, bitsets
- (aspirational) tables, graphs, multidimensional arrays

Datalit is a serialization format:

```datalove
// Personal info
{
  name = "Ada",
  born = 1815,
  interests = ["mathematics", "poetry", "music"],
  address_book = set {
    {
      kind = enum Friend,
      name = "Charles",
    },
    {
      kind = enum Family,
      name = "George",
    },
  },
}
```

All expressions can be type-hinted.
The syntax for this is `: <type> / <expr>`.
You'll probably get used to it.

```datalove
// This is the type of the literal we're about to write.
// When you see ":" in Datalove it is always followed by a type.
: {
  name: string,
  born: u32,
  interests: [string],
  address_book: set <{
    kind: enum { Friend, Family },
    name: string,
  }>,
} / {                      // After "/" is the literal expression.
  name = "Ada",    
  born = 1815,
  interests = : [string] / [           // Here's another type hint.
    "mathematics", "poetry", "music",
  ],
  address_book = set {
    {
      kind = enum Friend,
      name = : string / "Charles",    // And another!
    },
    {
      kind = enum Family,
      name = "George",
    },
  },
}
```

Datalit is strongly statically typed,
but supports free non-destructive coercions,
and other lightweight conversions.
All types are owned tree-shaped value-types
and do not support interior mutability, native pointers, or cycles.

The shapes of Datalit types and type descriptors are fully
specified at runtime and form the basis of the Datalove
runtime ABI.

If you understand Datalit you understand 80% of Datalove.




### Datalove Functions ("Datafun")

> File extension `.dfs` (script), `.dfm` (modules)
>
> Example [demo-datafun-script.dfs], [demo-datafun-module.dfm].

A simple pure-functional language that feels like an imperative language,
built on the datalit type system.
†

```datalove
fun increment(
  accum: int, amount: u8,
): int
  ret accum + amount
end fun
```

That's with bigints. Here's the one with fixed ints,
handling that pesky overflow:

```datalove
fun increment(
  accum: u64, amount: u8,
): ?u64
  ret accum +? amount
end fun
```

A `fun` is a pure total function on Datalit types, can't panic.
It is one of the few types Datafun adds over Datalit.

todo

The Datalit type system has extremely nice properties that
enable: pure functions, total comptime evaluation, full or partial memoization,
undo/redo, rewind/replay, prolog-style choice points, backtracing, and multi-determinism;
but these capabilities are surfaced with a familiar looking and feeling language -
comptime, functional and logic programming become powerful extensions to
less-restricted imperative programming.

If you understand Datafun you understand 90% of Datalove.




### Full-on Datalove

> File extension `.dls` (script), `.dlm` (module)
>
> Example [demo-datalove-script.dls], [demo-datalove-module.dlm].

- `proc`
- owned native pointers,
  linear, non-clonable, non-mathable,
  just tokens with provenance,
  runtime can assume these are unaliased
- `obj` - structs with object identity and encapsulation.
  These are the unit of abstraction.
  Still value types, not like C# classes.

If you understand Datalove then objective achieved.
It is a simple language.






