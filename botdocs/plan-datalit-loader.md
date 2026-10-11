# A Runtime Datalit Loader

A report, and a direction rather than a committed plan: a fast reader for
datalit text that depends only on the runtime, usable as the compiler's reader
for `require data` today and as *the* datalit loader of an I/O-capable sys
package later. Written October 2026 after measuring the store demo; not
scheduled.

## Contents

- [What prompted this](#user-content-what-prompted-this)
- [How a data file is read today](#user-content-how-a-data-file-is-read-today)
- [How a constant gets back to the runtime](#user-content-how-a-constant-gets-back-to-the-runtime)
- [The loader](#user-content-the-loader)
- [Not through ConstValue](#user-content-not-through-constvalue)
- [One reader, not two](#user-content-one-reader-not-two)
- [What exists and what is missing](#user-content-what-exists-and-what-is-missing)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## What prompted this

The benchmark tables in [Tuning the JIT](plan-jit-tuning.md) and
[Are We Fast Yet for benchvs](research/research-awfy-benchmarks.md) report the
*running* phase of `datalove script --time`. The store demo's *compiling*
phase is as large or larger, and almost all of it is reading its data files:

| Orders | `.dlt` data | Compiling | Running (jit) |
|---|---|---|---|
| 20,000 | 4 MB | 265-295 ms | 245-255 ms |
| 200,000 | 36 MB | 2376 ms | 1690 ms |

That is about 15 MB/s. In a profile of the whole run, the front end's share is
about a third: the salsa lexer (`lex_chunk`), the bracer, the datalit parser
building AST nodes (`ExprFull::new`), interning (`InternedText` through the
salsa table), the datalit typechecker (`tycheck::check`,
`check_int_fits_type`), then `datafile::Reader` and `parse_int_const`. A text
parser for a format this regular should run at hundreds of MB/s.

## How a data file is read today

`require data` binds a `.dlt` file as a constant of a declared type. The file
goes through the whole language front end:

1. lexed, bracer-matched and parsed into a datalit AST, as source code is;
2. names resolved;
3. typechecked against the binding's type by the datalit typechecker;
4. read into a `ConstValue` of that type by `datafile::Reader`
   (`datafun-lower/src/datafile.rs`), a walk led by the type, memoized by
   salsa on the source and the type.

Every step allocates a structure the size of the data -- the token tree, the
AST, the interned strings, the `ConstValue` -- and the next step walks it.

## How a constant gets back to the runtime

A `ConstValue` is not what a program runs on, so it is turned back into a
runtime value wherever it is used:

- the interpreter builds it with `write_const` into its static pool the first
  time the const is read;
- the compiled backends emit *code* that builds it (`build_list_const_at`,
  `build_map_const_at` and kin in Cranelift's `constants.rs`, and the C
  backend's equivalent), so an AOT executable's size and startup grow with the
  dataset.

The other direction exists too, for compile-time evaluation:
`extract_const_value` (`datafun-interp/src/ctfe.rs`) runs a const's code in
the interpreter and converts the result. It is a deep, `IrType`-led walk that
builds a fresh owned tree: every list element a node, sets and maps walked
leaf by leaf, strings copied into Rust `String`s, bigint limbs into a
`Vec<u32>`, and a struct as `Struct(Vec<(String, ConstValue)>)` -- the field
names copied for every record. For 200,000 orders with nested lines that is
millions of small allocations for names alone.

## The loader

A reader for datalit text, in `datalove-rt` or a crate depending only on it and
`datalove-rtdt`, producing runtime values directly:

- **One pass over bytes.** Recursive descent on the grammar in
  [Datalit Grammar](datalit-ebnf.md), which is small and regular: scalars,
  strings with escapes, `some`/`ok`/`er`/`none`/`data`/`error`, tuples,
  structs, lists, maps, sets, atoms, terms, enums, tensors, tables, `: T /`
  hints anywhere, nested comments. Tensors (shape from runs of commas) and
  tables (a header row) want a little lookahead and no more. No token tree, no
  AST, no interning.
- **The parser is the typechecker.** Each production takes an optional
  expected type and gives back a value and its type, so the two modes the
  [Datalit Typing Rules](datalit-typing-rules.md) already specify are one
  function: given an expected type it *checks* (an integer literal against
  `u8`, a struct structurally, a hint narrowing the expectation), and without
  one it *synthesizes* (`42 => int`, `[] => [()]`, the first element fixing a
  list's type), obeying hints as it meets them.
- **Types are descriptors.** The expected type arrives as a `TyDesc`, which
  says everything checking needs: field names, atom and variant names, element
  types, widths. A synthesized type is a `TyDesc` the loader builds.
- **Values are runtime values.** Built with the runtime's own constructors as
  it goes. An untyped load returns a `data` -- a value and its descriptor,
  self-describing -- which is exactly what something that does not know the
  type should get.
- **Errors are data.** Byte offset, what was expected, what was found: the
  compiler renders them as diagnostics with source labels, a rider returns
  them as an `error`.
- **Exported through the rti table.** A rider reaches the runtime only through
  the table, so a sys package doing I/O gets the loader the way `std` gets
  `list_push`, as something like `dtlv_rti_datalit_load(rt, bytes, len,
  expected_tydesc_or_null, out, ...)`, with no new mechanism.

## Not through ConstValue

The obvious way to use the loader in the compiler -- load, then convert the
runtime value to a `ConstValue` with `extract_const_value`, and let the rest of
the pipeline carry on -- gives most of the gain back. A data file would go
bytes, runtime value (fast), `ConstValue` tree (a deep copy, allocation-heavy,
larger than the data), runtime value again (`write_const`, or emitted
construction code): two extra full passes, and the tree is part of what costs
today.

So a big data file should not become a `ConstValue` at all:

- **At compile time,** the compiler runs the loader in checking mode against
  the declared type, for its diagnostics, and keeps the outcome rather than the
  value.
- **At run time,** the constant *is* the loader's output, made where it is
  used: the interpreter and jit load it into the static pool on first read, in
  place of `write_const`; the AOT backends embed the file's bytes and call the
  loader during static initialization, in place of construction code
  proportional to the data.
- **Compile-time evaluation** reading data loads it into the interpreter the
  evaluator already runs on runtime values in. Only a result that has to end up
  in IR as a constant goes through `extract_const_value`, which is the small
  case.

That needs a new kind of constant, roughly "loaded from these bytes against
this type", which `IrType`'s consts and the static-const machinery
([Consts Built Once](plan-static-consts.md)) carry beside `ConstValue` rather
than expanding into one. That is the larger part of this work, and the part
that makes it worth doing in the compiler rather than only for riders.

## One reader, not two

The risk is two implementations of datalit typing: the salsa typechecker for
literals in code, the loader for data, drifting on the questions literal typing
answers -- whether an integer fits, how list elements unify, the empty
collection defaults, struct field order. Two mitigations, best together:

- **The loader is *the* reader for data files,** compiler included, once the
  constant path above exists. Literal *code* in `.dfs` and `.dfm` keeps the
  salsa typechecker, since it lives in expressions with operators and bindings
  rather than as standalone text.
- **Differential tests.** Run both over the literal fixtures and a generated
  corpus (`datalove worldgen` already generates programs) and compare the types
  and values they give.

## What exists and what is missing

**Exists.** The runtime constructs every kind of value: lists, maps, sets,
tensors and tables (`*_create_local`, push, insert, `build_from_slice`),
strings (`string_from_bytes`), bigints (`int_from_limbs`, `int_from_fixed`),
`data` (`data_from_local`). `datalove-rt` depends only on `rmx`, `rtdt` and the
ABI declarations, so a loader in it is runtime-only by construction. The rti
table is the export path. `data` is the untyped result.

**Missing.**

1. **A runtime descriptor table.** Synthesis has to *create* descriptors.
   Today that is `TyDescTable` in `datalove-datalit`, keyed on salsa
   typechecker types, which cannot come along; it already has runtime-only
   caches for option, result, list, map and set, which is the shape needed. A
   small hash-consed table in the runtime, owning its allocations and keyed
   structurally.
2. **Decimal bigint parsing** in the runtime, into limbs. Small.
3. **Type names in hints.** `: Order /` names an alias. Checking mode barely
   needs one, the expected descriptor covering the value; synthesis mode needs
   a name environment from the caller, or refuses.
4. **The constant path** in [Not through ConstValue](#user-content-not-through-constvalue).
5. **Structured errors,** and the compiler's rendering of them.

## Phases

1. **The loader, checking mode,** with the runtime descriptor table and bigint
   parsing, tested against the datalit typechecker on fixtures and generated
   literals. Usable from Rust and through the rti table; nothing in the
   compiler uses it yet.
2. **Synthesis mode,** hints included, and an alias table supplied by the
   caller.
3. **The compiler checks data files with it,** replacing steps 1-3 of today's
   path for diagnostics, while `datafile::Reader` still produces the
   `ConstValue`. Measures the parser's share alone.
4. **Loaded constants.** The new kind of constant, the interpreter's static
   pool loading on first read, AOT embedding bytes and loading at startup, and
   `datafile::Reader` retired. This is where the store demo's compile time
   mostly goes.
5. **An I/O sys package** exposing it, when there is one.

Each stands alone; 1 and 2 are useful to a rider before the compiler touches
any of it.

## Open questions

- **Integer defaulting in synthesis.** The rules say an unhinted integer is an
  `int`, arbitrary precision. For data-heavy files a loader option for a
  narrower default may be wanted, though it would be a dialect.
- **Aliases from the requiring module.** Whether a data file may name types
  from the module that requires it, as it can today. Checking mode makes it
  unnecessary; synthesis mode would need the alias table.
- **Where the bytes live for AOT.** Embedded in the executable, as the
  constant is today in another form, or read from a path at startup -- which
  would make the executable depend on the file, as it does not now.
- **Incremental compilation.** Today the `ConstValue` is memoized on the
  source and the type. A loaded constant's check is memoized the same way; the
  value itself is not in the database at all, which is the point, but it moves
  where invalidation is reasoned about.
- **Streaming.** A rider reading a large file may want values as they parse
  rather than one tree at the end -- a list of records delivered one at a
  time. The parser's structure allows it; the API would have to ask for it.
