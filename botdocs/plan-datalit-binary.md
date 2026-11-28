# An Ordered Binary Encoding for Datalit

A design for a binary form of datalit values whose bytes sort in datalit's
total order, so typed datalit values can be the keys and values of the
megaspace-iostack KV store. Written November 2026. Decided since: trees
have declared types, enum variants are encoded by name, `data` folds
recursive types, there is no compact value encoding, and a database is
either index-32 or index-64. Phases 1 to 6 are implemented (November 2026);
the format is specified in [datalit-binary.md](datalit-binary.md), and
[As built](#as-built) says where the code left this design. Phase 7, the
store, is not done.

## Contents

- [The problem](#the-problem)
- [The design in brief](#the-design-in-brief)
- [What the store asks of an encoding](#what-the-store-asks-of-an-encoding)
- [Self-description is a type, not a mode](#self-description-is-a-type-not-a-mode)
- [Why there are no embedded hints](#why-there-are-no-embedded-hints)
- [Value encodings](#value-encodings)
- [Type encodings](#type-encodings)
- [The order of types has to be fixed first](#the-order-of-types-has-to-be-fixed-first)
- [What it costs](#what-it-costs)
- [Decoding untrusted bytes](#decoding-untrusted-bytes)
- [Prior art](#prior-art)
- [One crate, on runtime values](#one-crate-on-runtime-values)
- [As built](#as-built)
- [What exists, what is missing](#what-exists-what-is-missing)
- [Phases](#phases)
- [Open questions](#open-questions)

## The problem

Datalit already defines a total order on every type
([datalit.md, Comparison and total ordering](../mandocs/datalit.md#comparison-and-total-ordering)),
and the runtime keeps maps and sets in it. megaspace-iostack stores keys as
bytes: its LSM is a `BTreeMap<Vec<u8>, Option<Vec<u8>>>`, a state machine
declares range scans as byte ranges `[start, end)`, and trees share one
keyspace by prefixing each key with its tree name and `0xFF`. To make
datalit values the store's keys, the byte order of an encoded key has to be
the datalit order of the value. Datalit has no binary form at all, and
the spec says it has no canonical form either.

The answer is one encoding, defined for a value *at a type*, that is
canonical, prefix-free and order-preserving: for two values `a` and `b` of
type `T`, `memcmp(enc(a), enc(b))` is `cmp(a, b)`. Self-description needs no
second mode, because datalit already has a type for it: a value encoded at
type `data` is its type's encoding followed by its value's. That is the
self-describing form, and it sorts correctly too, by type and then value,
which is the order datalit gives `data`.

## The design in brief

- **One encoding, at a type.** `enc(T, v)` writes only what the type does not
  already say. A struct's field names, a tuple's arity and a tensor's rank
  are in `T` and cost nothing.
- **Order-preserving and prefix-free.** Every value's encoding is the
  concatenation of its parts' encodings with markers chosen so that byte
  order is datalit order. Because each encoding is prefix-free, a tuple's
  encoding begins with its first element's, so a prefix scan over a tuple
  key is a byte-prefix scan.
- **Canonical.** Each value has exactly one encoding, so byte equality is
  value equality. This also settles the spec's open question about a
  canonical form for hashing.
- **Self-describing where the type says `data`.** `enc(data, v)` is
  `enc_type(T) ++ enc(T, v)`. A self-describing document is a value at type
  `data`, and a tree whose key type is `data` holds keys of mixed types,
  sorted by type and then value.
- **No embedded hints.** A hint in text guides inference; binary does no
  inference, so there is nothing for a hint to say.
- **Two index widths.** `index` and `offset` are written at the build's
  width, so there are two encodings, and a whole database is one or the
  other.

## What the store asks of an encoding

Reading `crates/megaspace-lsm` and `crates/megaspace-state-machine` in
megaspace-iostack gives these requirements.

1. **Byte order is value order.** Range scans are byte ranges, so a scan from
   `enc(lo)` to `enc(hi)` must return exactly the values between `lo` and
   `hi`.
2. **Byte equality is value equality.** The LSM finds a key by byte
   equality, so two encodings of one value would be two keys.
3. **Deterministic across replicas and platforms.** Replicas must produce
   identical mutations. The encoding cannot depend on the host's pointer
   width or byte order. It does depend on the index width, which is a
   property of the database rather than the host; see
   [Index width](#index-width).
4. **Composite keys with prefix scans.** A key `(tenant, user, time)` should
   be scannable by `tenant` or by `(tenant, user)`.
5. **Values decode, but need not sort.** Order matters only for keys. Using
   the same encoding for values keeps one codec, and the cost of doing so is
   measured in [What it costs](#what-it-costs).

Requirement 3 rules out the in-memory layout, which holds pointers, and requirement 1 rules out
length-prefixed collections: with a length prefix `[2]` would sort before
`[1, 2, 3]`.

## Self-description is a type, not a mode

The obvious design has a header flag: typed bare values for a known schema,
self-describing values otherwise, and perhaps hints in between. But datalit
already has this distinction, in the type system. A position whose type is
known statically holds a bare value. A position typed `data` or `error` holds
a value together with its type. Text datalit writes the latter as `data e`,
and binary should write it the same way, as the type and then the value.

Therefore "optionally self-describing" is a choice the schema makes per
position, and the format needs no flags:

| Want                                    | Type to encode at          |
|-----------------------------------------|----------------------------|
| A fixed-schema key, smallest bytes      | the key's type, e.g. `(string, u64)` |
| A self-describing document or file      | `data`                     |
| A fixed key with one open position      | e.g. `(string, data)`      |
| Keys of any type in one tree            | `data`                     |
| An error side of a result               | `error`, as datalit requires |

A self-describing key also sorts by datalit's rule for `data`: by the
carried type, then by value. The type's encoding is prefix-free, so the
bytes compare types first, and when the types match they compare the
values. This holds only if the order of types is the byte order of their
encodings. See [The order of types has to be fixed first](#the-order-of-types-has-to-be-fixed-first).

Trees in the store have declared key and value types, and encode at them.
`data` is still available as a declared type, or inside one, where a tree
wants an open position.

A standalone binary file adds a small frame: a magic number, a format
version byte, an index-width byte, then `enc(data, v)`. A store does not
need the frame. It records the format version and index width once for the
database, and each tree's key and value types in its own schema.

### Index width

`index` and `offset` are 32 bits by default and 64 under `index-64`, and
they are distinct types from `u32` and `u64` at either width. The encoding
keeps that split: it writes them at the database's width. So there are two
encodings, differing only in those two types, and a database is entirely
index-32 or entirely index-64. Opening a database with a build of the other
width is an error, not a conversion.

The alternative was always writing 64 bits. That makes every database
portable, but an index-32 build would then have to reject stored values it
cannot represent, and every index-32 key would carry four bytes it can
never use.

## Why there are no embedded hints

In text datalit, a hint changes typing and never the value. Every
well-typed document elaborates to a pair `(T, v)`, and a hint placed
anywhere inside it either agrees with `T` at that position or is an error.
Once a document is checked, its hints add nothing.

The binary encoding stores `(T, v)` after elaboration. A binary value at
type `T` therefore means exactly what the text `: T / e` means, and so does
every other text spelling of the same pair, whatever hints it carries. That
is the sense in which the two forms are semantically identical. An embedded
hint in binary could only repeat what the decoder already knows, or be an
error.

The round trip is not exact at the syntax level. Text to binary drops
comments, layout, hint placement and literal spelling (hex or decimal), and
resolves duplicate map keys last-wins. Binary to text needs the printer to
place hints. Where it places them is an open question in the spec. The
printer also has to learn to write non-finite floats, which it prints today
as `nan` and `inf`, and those do not parse. Binary already carries NaN
payloads exactly.

## Value encodings

`enc(T, v)` by type. "BE" is big-endian. `varint` is the ordered unsigned
varint below. Every encoding is prefix-free given `T`.

| Type | Encoding | Why it sorts |
|------|----------|--------------|
| `()`, `{}`, atom | nothing | one value |
| `bool` | `00` or `01` | false first |
| `u8` .. `u64` | fixed-width BE | numeric |
| `i8` .. `i64` | fixed-width BE, sign bit flipped | numeric |
| `index`, `offset` | as `u32`, `i32` in an index-32 database; `u64`, `i64` in an index-64 one | numeric |
| `f32`, `f64` | BE bits; if sign set, flip all bits, else flip the sign bit | IEEE `totalOrder`, as the spec says |
| `int` | header byte by sign and magnitude length, then magnitude, below | numeric |
| `string` | UTF-8 bytes, `00` escaped as `00 FF`, then `00 01` | byte order, prefix first |
| tuple, struct | elements concatenated | lexicographic |
| `term A T` | payload | by payload |
| `?T` | `00` for `none`, `01` then payload for `some` | none first |
| `!T` | `00` then the `error` for `er`, `01` then payload for `ok` | er first |
| enum | variant name as a `string`, then payload | by name, then payload |
| `[u8]` | bytes, `00` escaped as `00 FF`, then `00 01` | as `string` |
| other list, set | `01` before each element, then `00` | lexicographic, prefix first |
| map | `01` key value per entry, then `00` | by entry, prefix first |
| table | `01` then the row's cells in column order per row, then `00` | by row, row-major |
| tensor | each extent as `varint`, then all elements row-major | shape first, then elements |
| `data`, `error` | `enc_type(T)` then `enc(T, v)` | type first, then value |

Some notes on the table.

- **`int`.** The header byte is `80` for zero. For a positive value whose
  minimal magnitude is `L` bytes, it is `80 + L` for `L` in 1 to 126 and
  `FF` followed by `varint(L)` beyond that. The magnitude follows, BE. A
  negative value mirrors this: header `80 - L`, or `01` and the complemented
  `varint(L)`, then the complemented magnitude, so larger magnitudes sort
  first. `00` is reserved. Any integer up to 1008 bits costs one header
  byte, and the encoding is unbounded, which also covers the spec's bug
  that `int` is parsed through `i128`.
- **`string`.** UTF-8 never contains `FF`, so escaping only `00` is enough.
  A string prefix is a byte prefix, so "all strings starting with `ab`" is a
  byte-range scan.
- **Enum variants by name, not index.** The runtime's discriminant is the
  index into the name-sorted variants, and encoding that index would be one
  byte. A name costs its length plus two bytes. But adding a variant to the
  type would change every later variant's index and invalidate stored keys.
  Names survive that, and they sort correctly by definition.
- **`[u8]` is written as a string is.** The general list encoding needs a
  marker before each element, because there is no length prefix and the
  terminator, `00`, must be told apart from an element. That doubles a list
  of bytes. Escaping the bytes instead, as a string's are, sorts the same
  and costs two bytes plus one per zero byte. Only `[u8]` gets this: it is
  the case where the marker is as large as the element. `#{u8}` keeps the
  general encoding; it holds at most 256 elements.
- **Sets share the list encoding.** Elements are written in set order. A
  decoder at a set or map type checks that each encoded element is
  byte-greater than the one before. This is a `memcmp` of adjacent slices,
  and it enforces canonical form for free.
- **Tensors need no markers.** The rank is in the type and the element count
  is the product of the extents. So `[|u8, 1|]` is the compact blob type:
  its encoding is the length, then the bytes. Tensors sort by shape first,
  though, so a tensor key sorts by length before content. A key that wants
  lexicographic bytes should be a `string` or a `[u8]`.

The ordered `varint` is a byte `b < F8` for values below `F8`, and
otherwise `F7 + n` followed by `n` bytes BE, for `n` from 1 to 8, written
minimally. A minimal encoding preserves order.

That these encodings compose is the standard argument. If each part's
encoding is prefix-free and order-preserving, the first differing byte of
two concatenations falls inside the first differing part, so the
concatenation is lexicographic in the parts. The markers `01` and `00` make
"another element" sort after "end", which is the spec's "a prefix comes
first".

## Type encodings

`enc_type(T)` is a tag byte for the constructor followed by its parts. Each
part is a type, a `varint` count, or a name encoded as a `string`.

- **Primitives** are just their tags. `index` and `offset` have their own
  tags, the same at either width, since a database has only one.
- **Tuple** is a count and then the element types. **Struct** and **table**
  are a count and then `(name, type)` pairs in declared order.
- **List, set, option and result** are their element type. **Map** is the
  key type and then the value type. **Tensor** is the element type and then
  the rank.
- **Atom** is its name. **Term** is its name and then its payload type.
  **Enum** is a count and then its variants sorted by name, each a kind tag,
  a name, and for a term the payload type. Sorting makes the encoding
  canonical under the spec's rule that enum equivalence ignores
  declaration order.
- **`rec`** is a binder tag and then its body. A **recursive reference** is a
  tag and then its de Bruijn index. The binder name is not encoded, as
  equality already ignores it
  ([plan-recursive-types.md](plan-recursive-types.md)).

Type equivalence is syntactic up to enum variant order, so this encoding is
canonical: two types are equivalent exactly when their encodings are equal.

### Folding recursive types

A recursive type and its one-step unfolding are accepted for each other at
the head ([Meeting at the head](plan-recursive-types.md#meeting-at-the-head)),
with one layout and the same values. But they are distinct under `≡`, so
without care `data` would give one value two encodings and the store two
keys. Therefore `data` folds.

As built, it folds at every depth, not only at the head. A descriptor has
no `rec` node: a recursive type is a cycle of descriptor pointers, and the
runtime's `eq_tydesc` already takes two descriptors to be one type when they
unfold to the same tree, at any depth. The encoding has to agree with the
runtime, or an in-memory `#{data}` would hold one element that the store
holds as two. So the type encoder walks down from the root and writes a
type equal to one of its ancestors as a reference to it, with a binder on
the ancestor. That depends only on the tree, so every unfolding encodes
alike.

## The order of types has to be fixed first

The spec leaves the order among the types a `data` may carry unspecified.
The runtime fills that gap with `cmp_tydesc` in
`crates/datalove-rt/src/c/impls/cmp.rs`, which orders by `TyTag`, then by
size and alignment, then by structure. Size and alignment are layout facts.
Every collection header holds a pointer, so its size differs between a
64-bit host and a 32-bit one such as wasm32, and a set of `data` values can
be ordered differently by two builds of the same index width. A store keyed
on `data` cannot have that. Neither can a program that reads a
`%{data -> V}` out of the store and expects it to iterate in the store's
order.

Therefore the spec should define the order of types as the byte order of
`enc_type`, and `cmp_tydesc` should implement that order, structurally and
without layout. This changes the iteration order of in-memory sets of
`data`, which nothing may depend on today because the spec does not define
it. It does not change the order of any value at a static type.

## What it costs

| Value | Encoded size |
|-------|--------------|
| key `("alice", : u64 / 7)` | 7 + 8 = 15 bytes |
| `some 1` at `?int` | 3 bytes |
| `[1, 2, 3]` at `[u32]` | 3 x 5 + 1 = 16 bytes |
| 1024 bytes, none zero, at `[u8]` | 1026 bytes |
| 1024 elements at `[\|f32, 1\|]` | 3 + 4096 bytes |
| the same at `data` | plus 3 type bytes |

The order-preserving rules cost three things. First, list, set, map and
table elements each take one marker byte, except in a `[u8]`. Second,
strings and `[u8]` take two extra bytes and escape zeros. Third, there is no
random access: skipping a field means parsing it. None of this matters for
keys. Values pay it too, as there is no second, compact encoding: one codec
is worth more than the bytes.

## Decoding untrusted bytes

The decoder must reject anything `enc` would not produce, or byte equality
stops being value equality. That means:

- non-minimal ints and varints
- invalid UTF-8 and stray escapes
- a set or map whose elements are not strictly ascending
- an option, result or enum tag out of range
- trailing bytes

It also needs a depth limit and a bound on tensor extents before it
allocates. With that, `enc(dec(b)) == b` for every accepted `b`.

## Prior art

| System | What it does | What we take |
|--------|--------------|--------------|
| FoundationDB tuple layer | Self-describing ordered keys: a type code, then an order-preserving payload. Ints use a length-coded header byte. | The `int` header, and the shape of mixed-type keys, which here is `data` |
| CockroachDB `encoding`, Go `orderedcode` | Strings escaped `00 FF`, terminated `00 01`; floats bit-flipped | String and float rules |
| SQLite4 varint | Order-preserving varint with the length in the first byte | `varint` |
| Bigtable, HBase row keys | Byte-ordered keys, composite by concatenation | Tuples as concatenation for prefix scans |

None of these has a type system to lean on, so they all tag every value.
Datalit tags only where the type is `data`.

## One crate, on runtime values

All of this goes in one new crate, and it works on the runtime's own
representation: a value is a `TyDescRef` and a pointer, laid out as
`datalove-rtdt` says. There is no second Rust model of datalit values.

**Encoding needs only rtdt.** Walking a value through its descriptor reads
layouts, and the layouts are rtdt's. Four small, read-only pieces of the
walk live in `datalove-rt` today, and are pure layout arithmetic:

- unpacking a `data` (`boxing::data_borrow`)
- the tree cursor's begin and advance, for maps and sets
  (`cursor.rs`)
- tensor element addressing by strides (`cmp.rs` does it inline)
- table cell addressing (`table::element_ptr`)

They move into rtdt rather than being copied, since `cmp.rs` uses them
too.

**Decoding values needs the runtime.** A decoded value has to be
allocated by the runtime's thread-local allocator, because the runtime's
`destroy` frees it and the leak check counts it. A set or map has to be
built as the runtime's B-tree; decoded elements arrive sorted, so
`btreeset_build_from_sorted_slice` and its map counterpart fit. So the
crate depends on `datalove-rt` as well as rtdt. `datalove-datalit` is a
dev-dependency only.

**The decoder writes into space the caller gives it.** That is the
runtime's own convention: every `dtlv_rti_*` constructor takes an
`AnyOut`, space of the type's size and alignment, and writes the value's
top level there, allocating only what the value points to on the runtime
heap: string bytes, list storage, B-tree nodes, `int` limbs, and the
payload of a `data`. So the decoder is `decode(rt, bytes, tydesc, out)`.
The caller decides where the top level lives: a Rust local, a frame slot,
a `MemGuard` on the runtime heap, or an element of a buffer of many. Once
it returns the caller owns the value, and frees it through the runtime's
`destroy`, or a `ValueGuard`.

**Memory is the caller's to manage, as it is everywhere in the runtime.**
The decoder and the descriptor builder are `unsafe` functions over raw
pointers, like the `dtlv_rti_*` functions, and nothing tracks a value's
lifetime against its descriptors. The caller must:

- destroy a decoded value with the runtime of the thread that decoded it;
- keep the descriptor arena alive until every value decoded through it is
  destroyed, since a `data` holds a raw pointer to its descriptor and
  `destroy` reads it to free the payload;
- choose how long the arena lives, knowing it grows with every distinct
  type a `data` brings in.

A safe wrapper can come later, when the store's usage shows what shape it
should have.

Self-describing bytes need no second entry point. Decoding at the `data`
descriptor writes a 16-byte `Data` into `out`, and the carried value is
packed as `dtlv_rti_data_from_local` packs one: small scalars in the
`Data`'s own words, anything else on the heap. Nothing is decoded into
`data` that was not encoded as one. Wrapping every value in a `data` would
box it and make a descriptor for it, to hand Datafun a value it cannot
unwrap, when the tree's declared type already says what it is.

Decoding validates all of the bytes before it allocates anything. A
rejected input then has nothing to clean up, and the building pass cannot
fail except by running out of memory. The first pass also learns every
collection's length, so the second allocates each one at its final size.

**Decoding types needs a descriptor builder without salsa.** A `data` needs
a descriptor for the type it carries, which is read from the bytes and
known only at run time, and a tree's declared types need descriptors too.
Today only `datalove-datalit`'s `TyDescTable` makes descriptors. It needs
the salsa database, but only incidentally:

- descriptors point at their names' text inside the interner
  (`name.as_str(self.db).as_ptr()`), so they must not outlive the database
- its cache is keyed on the salsa-interned `Type<'db>`
- `create_rec` builds a recursive type's descriptor by unfolding a `Type`

None of it is essential. The layout arithmetic is mostly
`rtdt::layout`'s already, and the table already has builders that take
descriptors rather than types for tuples, options, results, lists, maps and
sets.

Therefore descriptors get a builder in rtdt with no database. It takes the
structure and names directly, and keeps descriptors and their name text in
an arena that owns both. It deduplicates on the type's encoding, which is
canonical, and so is an exact key. It builds a recursive type as
`create_rec` does, with a stand-in descriptor that is patched once the body
is built. A type's encoding holds everything the builder needs, so the
decoder goes straight from type bytes to descriptors. A tree's declared
types can be stored as type bytes, and written as datalit text only where
that is convenient, as in tests.

The builder joins three others. `datalove-datalit` builds descriptors
from datalit types (`TyDescTable`), the interpreter and JIT from `IrType`
(`IrTyDescTable` in `datafun-interp`, which needs no database already),
and the Cranelift and C backends emit them as static data. The runtime
deduplicates none of them and compares descriptors structurally
(`same_tydesc` in `crates/datalove-rt/src/c/mod.rs`), so a value is
usable with any descriptor of its type, wherever it was made. What must
agree is the layout. A builder that disagrees with the others on an offset
corrupts values instead of failing. So the new builder takes every size and
offset from `rtdt::layout`, and joins the conformance test
(`crates/datalove-datafun/tests/layout_conformance_tests.rs`), which today
checks `IrTyDescTable` against the IR's layout and leaves datalit's
`TyDescTable` out. [The runtime loader](plan-datalit-loader.md), which
also wants descriptors without salsa, can use the same builder.

### Values for Datafun

A decoded value is a runtime value: the runtime's allocator, the
runtime's B-trees, and the layout compiled Datafun code reads, since the
conformance test holds that layout to the IR's. Compiled code reads a
value at a static type by its IR layout and never looks at a descriptor,
so where its descriptor came from does not matter. Datafun can therefore
use decoded values once it is handed them.

Two things limit that today. Nothing hands Datafun code a runtime value
built outside it: `require data` reads a file into a `ConstValue` and the
backends rebuild it, and the only other way in is a native rider's
arguments. And Datafun has no way to unwrap a `data`
([types.md](../mandocs/types.md)); a program can hold one, compare it,
store it and print it, all of which the runtime does through the
descriptor it carries, but not take out what it holds. A Datafun `decode`
would need both: an intrinsic that takes bytes and a type and returns a
value, and a way to downcast a `data`.

**Test values come from the datalit parser.** Values are built directly
almost only in tests, and a datalit document is the easiest way to write
one. The tests parse and instantiate text into runtime values, encode them,
and compare against `cmp_total`.

**No serde adapter.** megaspace-iostack uses serde, through postcard, for
its own wire messages, log entries and manifest, and requires a state
machine's `Command` to be `Serialize` so it can be logged. Stored keys and
values are not serde: the LSM, `KeyRef` and `Mutation` all take
`Vec<u8>`. Its ledger builds keys by hand as big-endian `u64` and `u128`,
which is this encoding written out manually. A datalit state machine
carries encoded bytes in its commands and needs nothing from serde. If a
serde adapter is wanted later, it is a set of thin wrappers over this
crate, not a second value model.

## As built

Where the code departs from the design above:

- **The type encoder is in rtdt, not the new crate.** The runtime orders
  `data` by type encoding, so `cmp_tydesc` needs it, and the runtime cannot
  depend on a crate that depends on it. `rtdt::tyenc` has the type encoding
  and order, and `rtdt::ordered` the writers it uses. Everything else is in
  `datalove-datalit-bin`: the value encoding, reading (`read`), and the type
  decoder and `TyArena` (`tybuild`), which the runtime never uses. A
  structural `cmp_tydesc` would let the encoder move too, but it would have
  to reproduce the encoder's folding of recursive types, a second definition
  of the order to keep in step.
- **Folding is at every depth**, as
  [Folding recursive types](#folding-recursive-types) now says.
- **`cmp_packed` no longer compares type tags first.** A recursive type's
  encoding starts with its binder, not its constructor's tag, so ordering
  `data` by tag before type would disagree with the bytes.
- **The walking helpers stayed in rt.** Once the crate depended on the
  runtime for decoding, the encoder uses the runtime's exported cursor and
  `data_borrow` functions, so nothing had to move. `encode` takes a runtime
  handle for them.
- **`TyDescTable` was not rebuilt on the arena.** There were already four
  descriptor builders, held to one layout by tests;
  `tybuild::layouts_agree` now holds the arena to datalit's `TyDescTable`
  in the round-trip tests and to the interpreter's `IrTyDescTable` in
  `layout_conformance_tests.rs`.
- **An enum variant's type parts are its name and then a flag**, `00` or
  `01` and the payload type, rather than a kind tag first.
- **A table of no columns keeps its row count** when decoded. The
  runtime's builder from rows makes it empty whatever the rows, so the
  decoder writes the table itself.
- **The tests** are `crates/datalove-datalit-bin/tests/encoding_tests.rs`:
  every kind of value round-tripped, runs of values in order, `data` of
  different types, folding, malformed input, and generated pairs from
  datalit's `ast_gen` checked against `cmp_total`, with mutated bytes
  refused or re-encoded alike. `DATALIT_BIN_COUNT` sets how many generated
  cases; 20,000 pass.

## What exists, what is missing

What exists: everything in phases 1 to 6.

What is missing:

- The store: phase 7.
- A printer that round-trips every value, including non-finite floats.
- `mandocs/datalit.md` still calls the order among the types a `data` may
  carry unspecified, and says datalit has no canonical serialized form. Both
  are now settled by [datalit-binary.md](datalit-binary.md); updating the
  spec is left to its maintainer.

Out of scope: changing a database's index width, migrating a tree to a new
declared type, and a serde adapter.

## Phases

1. **Spec.** Done in [datalit-binary.md](datalit-binary.md); not yet in
   `mandocs/datalit.md`.
2. **The order of types.** Done.
3. **Encoding.** Done.
4. **A descriptor builder without salsa.** Done.
5. **Decoding.** Done.
6. **Proof by generation.** Done.
7. **Typed trees in megaspace.** A state machine whose trees have declared
   datalit key and value types, stored as type bytes, and whose commands carry encoded values. A
   scan over a value range becomes `[enc(lo), enc(hi))`, and a prefix scan
   over a tuple key becomes the byte range of that prefix.

## Open questions

- **What shape should a safe wrapper take?** The decoder and descriptor
  builder are unsafe, as the runtime is. A replica keeping one arena for
  its life, a scope per `apply`, and a process-wide intern table each
  suggest a different safe API, and the store has not yet shown which it
  needs.
