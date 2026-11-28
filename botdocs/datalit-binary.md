# The Binary Datalit Encoding

The reference for datalit's binary form: an encoding of a value at its type
whose bytes sort, under `memcmp`, in datalit's total order. Implemented
November 2026 in `datalove-datalit-bin`, apart from the type encoder,
which is `datalove-rtdt`'s `tyenc` with the writers in its `ordered`, as the
runtime orders types by it.
The design and what was weighed are in
[plan-datalit-binary.md](plan-datalit-binary.md).

## Contents

- [Properties](#properties)
- [Primitives](#primitives)
- [Values](#values)
- [Types](#types)
- [The order of types](#the-order-of-types)
- [Decoding](#decoding)
- [The Rust interface](#the-rust-interface)

## Properties

For values `a` and `b` of one type `T`, written `enc(T, v)`:

- **Ordered.** `memcmp(enc(T, a), enc(T, b))` is the runtime's
  `cmp_total(a, b)`.
- **Canonical.** Each value has one encoding, so equal bytes are equal
  values, floats compared by bit pattern as the total order compares them.
- **Prefix-free.** No encoding at `T` is a prefix of another, so
  concatenations sort lexicographically by part. A tuple's encoding begins
  with its first element's, which makes a prefix scan over a tuple key a
  byte-prefix scan.
- **Typed.** Only what `T` does not say is written. A value at type `data`
  writes its type's encoding and then its own, and is the self-describing
  form.
- **Two index widths.** `index` and `offset` are written at the build's
  width, so the encodings of an index-32 and an index-64 build differ in
  those types. A store records its width and is only read by builds of it.

## Primitives

Written by `crates/datalove-rtdt/src/ordered.rs`, read by
`crates/datalove-datalit-bin/src/read.rs`.

| Name | Encoding |
|------|----------|
| `varint` | Unsigned. A byte `b < F8` is the value `b`; otherwise `F7 + n` and then `n` bytes big-endian, `n` from 1 to 8, as few as hold it. |
| escaped bytes | Each byte, with `00` written `00 FF`, then `00 01`. Sorts lexicographically, a prefix first. |
| string | The UTF-8 bytes, escaped. |
| name | A string that is a datalit identifier. |

## Values

`enc(T, v)`, in `crates/datalove-datalit-bin/src/encode.rs`. BE is
big-endian.

| Type | Encoding |
|------|----------|
| `()`, `{}`, atom | nothing |
| `bool` | `00` or `01` |
| `u8` .. `u64` | fixed width, BE |
| `i8` .. `i64` | fixed width, BE, sign bit flipped |
| `index` | `IndexRepr`, BE: four bytes, or eight under `index-64` |
| `offset` | `OffsetRepr`, BE, sign bit flipped |
| `f32`, `f64` | the bits BE; if the sign bit is set all bits flipped, else the sign bit flipped |
| `int` | a header byte, then the magnitude; below |
| `string` | escaped bytes |
| tuple, struct | the fields in order |
| `term A T` | the payload |
| enum | the variant's name, then its payload if it has one |
| `?T` | `00` for `none`; `01` then the payload for `some` |
| `!T` | `00` then the `error` for `er`; `01` then the payload for `ok` |
| `[u8]` | escaped bytes |
| any other list, and sets | `01` before each element, then `00` |
| map | `01`, key, value for each entry in key order, then `00` |
| table | `01` then the row's cells in column order for each row, then `00` |
| tensor | each extent as a `varint`, then every element in row-major order |
| `data`, `error` | the carried type's encoding, then the value at it |

An `int` of magnitude `L` bytes, minimal, has the header `80` for zero,
`80 + L` when positive and `80 - L` when negative, for `L` up to 126. A
longer magnitude has the header `FF` and then `varint(L)` when positive, or
`01` and then `varint(L)` with every byte flipped when negative. The
magnitude follows big-endian, every byte flipped when negative. `00` is not
a header.

## Types

`enc_type(T)`, in `crates/datalove-rtdt/src/tyenc.rs`: a tag byte and then
the type's parts.

| Tag | Type | Parts |
|-----|------|-------|
| `01` | `bool` | |
| `10` .. `17` | `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64` | |
| `18`, `19` | `index`, `offset` | |
| `20`, `21` | `f32`, `f64` | |
| `30` | `int` | |
| `40` | tuple | `varint` count, the element types |
| `41` | struct | `varint` count, then a name and a type per field |
| `42` | enum | `varint` count, then per variant in name order: a name, `00`, or `01` and the payload type |
| `43` | atom | name |
| `44` | term | name, payload type |
| `50` | list | element type |
| `51` | `string` | |
| `52` | map | key type, value type |
| `53` | set | element type |
| `54` | tensor | element type, `varint` rank |
| `55` | table | `varint` count, then a name and a type per column |
| `60` | option | inner type |
| `61` | result | ok type |
| `70`, `71` | `data`, `error` | |
| `90` | binder | the type it binds |
| `91` | reference | `varint` de Bruijn index of a binder |

A descriptor of a recursive type is a graph with a cycle in it, and the
runtime takes two descriptors to be one type when they unfold to the same
tree. The encoding is a function of that tree. Walking down from the root, a
type equal to one of its ancestors is written as a reference to that
ancestor, and an ancestor that is referred to is written with a binder in
front of it. Index 0 is the nearest enclosing binder. So `rec T [?T]`,
`[?rec T [?T]]` and `rec T [?[?T]]` all encode as `90 50 60 91 00`.

This folds unfoldings at every depth, where datalit's `≡` accepts one only
at the head. The runtime's `eq_tydesc` already equates them at every depth,
and the encoding has to agree with it.

## The order of types

The order of two types is the byte order of their encodings
(`tyenc::cmp_types`). `data` and `error` values sort by their types in this
order, and then by value. The runtime's `cmp_tydesc` is this order, so an
in-memory set of `data` iterates in the order the same values' encodings
sort in, on every host.

## Decoding

A decoder accepts exactly the bytes the encoder writes, so a decoded value
encodes back to its input. It refuses:

- a byte no encoding has at that place, including a `bool` or tag other
  than `00` or `01`;
- truncated input and trailing bytes;
- a non-minimal `varint` or `int`;
- a string that is not UTF-8;
- a set or map whose encoded elements are not strictly ascending;
- an enum value naming no variant of its type;
- a type that is not canonical: an unused binder, a reference that could
  be folded nearer, variants out of name order;
- a type whose references reach their binder other than through a
  collection, a name that is not an identifier, a repeated field or column
  name, and a tensor of rank 0;
- types nested more than 256 deep and values more than 512 deep.

A tensor's extents are checked against the input left before anything is
allocated for its elements, unless its element type encodes as no bytes.

## The Rust interface

In `datalove-datalit-bin`:

- `encode(rt, td, value, out)` appends `enc(T, v)`, and `encode_data`
  appends the self-describing form.
- `decode(rt, arena, bytes, td, out)` writes a value into `out`, space of
  the type's size and alignment that the caller gives. It reads the input
  twice. The first pass checks it and counts every collection, allocating
  nothing on the runtime heap, so a refused input leaves nothing behind. The
  second builds the value at its final sizes.
- `TyArena` (`tybuild`) decodes types into descriptors,
  owning them and their names, and deduplicates them by encoding. It needs
  no salsa database. Its layouts come from `rtdt::layout`, and
  `tybuild::layouts_agree` holds them to datalit's `TyDescTable` and the
  interpreter's `IrTyDescTable` in the tests.

Everything is `unsafe`, as the runtime is. A decoded value is the caller's
to destroy through the runtime of the thread that decoded it, and the arena
that made the descriptors of any `data` in it must outlive it.
