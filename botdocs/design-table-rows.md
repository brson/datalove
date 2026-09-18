# Tables: what the type system is missing, and four ways out

A table is a first-class opaque value: it can be built, moved, cloned, compared,
sorted, keyed on and held inside anything, and nothing can look inside one. This
is why, and what could be done about it.

The intent behind tables is a dataframe -- relational operations, and pipelines
that take a table of one schema to a table of another. [If the target is
Polars](#user-content-if-the-target-is-polars) weighs the hypotheses against
that, and [H5](#user-content-h5-types-as-compile-time-values) is the direction
currently favoured: types as compile-time values, with the schema algebra a
library rather than a type theory.

Hypotheses, not a plan. Nothing here is implemented.

## What a table can do today

Checked by running each.

**Whole-table operations all work.** Construction from a literal, printing,
`@`, passing by `in`, `ref`, `mut` and `out`, returning, whole-table assignment,
and passing through a generic. Tables hold anything a column type can be,
including lists and other tables, and go inside options, results, lists, sets
and maps, and can be written as a `const`.

**Ordering works, which is further than it looks.** `ord.compare` and
`ord.equal` take tables, because `ord_compare` walks a value structurally from
its descriptor and a table is just another shape to walk. So every list function
in `sys/std/ord` reaches a `[{| ... |}]`: `sorted`, `contains`, `index_of`,
`count_of`, `least`, `greatest`, `is_sorted`, `binary_search`, `deduped`. That
is also what lets a table be a set element or a map key.

`==` and `.<` refuse a table, but they refuse a string and a list too; `ord` is
the way to compare anything that is not a number.

**Nothing reads a part of one.** `t.x` is `ProjectionOnNonAggregate`, `t[i]?` is
"indexing requires list, map, or tensor type", and there is no cell accessor of
any other spelling. A table's *values* can be arbitrary expressions -- `{| x; n |}`
where `n` is a binding is fine -- so values go in and never come out except as
part of the whole.

**Nothing changes its shape.** There is no push, pop, insert or remove of a row,
and no row count. The number of rows a table has is the number written in the
literal that made it, so a table cannot be built from data: `table_push_row` and
`table_build_from_rows` exist in the runtime and no syntax reaches them.

So a table today is a *composite literal with a comparison*: useful as a value,
inert as a structure.

## The problem, stated precisely

It is *not* that tables cannot be generic. They already are, per column:

```datalove
fun ident<T>(t: {| x: T, y: u32 |}): {| x: T, y: u32 |}    // compiles, runs
```

`bind_type_params` unifies two table types column by column, and
`datalit_is_container_of_var` counts a table holding a type parameter as a
container, so an owned one is wrapped whole into a `data` exactly as a `[T]` is.
All of the erasure machinery already reaches tables.

What cannot be written is a signature whose *column list* is not fixed. A type
parameter stands for a column's type; nothing stands for the set of columns or
for how many there are. So:

```datalove
fun len(ref self: ???): index      // nothing goes in the hole
```

The same hole stops the **native** being declared, which is why this is not
merely a missing library. `native fun table_len(ref self: {| x: T |})` would be
a function about single-column tables whose column is called `x`.

A second problem sits behind it. `dtlv_rti_table_get_local` takes a row and a
column and hands back a raw pointer, because a column's type varies by column:
`get(ref self, row, col)` has no return type to write, since the answer depends
on `col`, which is a value rather than a type.

## What the implementation already believes

Three facts constrain the answers, and they point the same way.

**A table type is shaped exactly like a struct type.**

```rust
pub struct TypeTable<'db>      { pub columns: Vec<TypeNamedField<'db>> }
pub struct TypeAnonStruct<'db> { pub fields:  Vec<TypeNamedField<'db>> }
```

**Storage is columnar.** `element_ptr` is
`data + table_column_offset(cols, col, capacity) + row * elem_size`, and
`table_column_offset` lays each column out as a contiguous run of `capacity`
elements. A column is therefore a contiguous array, not a strided one.

**The runtime's API is row-oriented anyway.** `table_push_row`,
`table_build_from_rows`, and the note in `std_tests/144_table_shapes` that "a
row is a tuple of the columns". Columnar storage is what sits behind that
interface, not what it presents.

So the runtime already thinks in rows, the type already looks like a struct,
and a column is already a well-formed array. Three invitations.

## What people want from struct-of-arrays elsewhere

A table is a struct of arrays with a schema, and that is a thing other languages
have tried. Two different constituencies ask for it and they want different
things, which is worth separating before choosing a hypothesis.

### The performance constituency

Jai, Zig, Odin, ISPC, and the entity-component-system engines.

1. **Layout-agnostic syntax**, which is the one they ask for first. Write `p.x`
   and let the compiler decide whether `p` lives in an array of structs or a
   struct of arrays. The value is not the layout, it is being able to *measure
   both* without rewriting the code that uses it.
2. **A column as a first-class slice.** `arr.x` gives a contiguous `[]T` that
   any function taking a slice will accept. Zig's `MultiArrayList.items(.x)`,
   Julia's `StructArray` field access giving a real vector.
3. **An element that behaves like a struct.** `arr[i]` yielding something you
   can read `.x` off: either a *copy*, which costs a gather, or a *proxy
   reference* holding one pointer per field. Odin has a dedicated pointer kind;
   Rust macro-generates `Ref`/`RefMut` structs; Julia has a lazy row.
4. **Growth that keeps the columns in step** -- push, pop, insert, remove, with
   the amortization of an ordinary growable array rather than a fixed buffer.
5. **Partial and tiled layouts.** Hot fields split out and cold fields left
   together; tiles of N elements for SIMD, which ISPC spells `soa<N>`.
6. **Iteration that touches only the named columns**, which is the cache
   argument and the whole point of an ECS query.
7. **Field-set polymorphism** -- "run this over anything with a Position and a
   Velocity". Row polymorphism under another name.

### The data constituency

Arrow, Polars, pandas, R, SQL.

8. **Zero-copy interop**: a column *is* the interchange buffer, handed to
   another system without a copy.
9. **Per-column validity**, as a bitmap beside the column rather than a byte
   inside every element.
10. **Relational operations**: filter, project, join, group by, aggregate.
    Anything called a table is expected to have them.
11. **A permutation applied across every column at once**, which is what sorting
    by one column means and is the operation SoA makes awkward.
12. **Heterogeneous columns with a schema**, and code that can be written
    against a schema it did not hard-code.

### The wall everyone hits

**A reference to one element of a struct of arrays is not a pointer.** It is a
tuple of pointers, or a base and an index. Languages that cannot say that end up
with macro-generated proxy structs, lazy proxy objects, "you get a copy", or a
new pointer kind in the language. That is the same non-first-class borrow parked
in [plan-generics.md](plan-generics.md), reached from a different direction.

The second wall is field-set polymorphism, which every ECS reimplements at run
time because its host language cannot say "a struct with at least these fields".

### Where this leaves the hypotheses

The syntax here -- named columns, rows in a literal, the word "table" -- puts
datalove in the data camp, while the columnar storage would serve either.
Against the twelve, everything is absent except that whole-table values work.
They map onto the hypotheses cleanly:

| Want | Hypothesis |
|---|---|
| 2, a column as a slice | H4, and unusually cheap because a column is already contiguous |
| 10 and 11, relational operations and permutations | follow from H4 via `sys/std/list` and `sys/std/ord` |
| 3, an element proxy | the non-first-class borrow; not a table feature |
| 4, growth | H2, because `push_row` needs a row type |
| 7, field-set polymorphism | H3 |
| 1, layout-agnostic syntax | **does not apply** |

That last row is the one that should change a decision. Every other language's
headline reason for a big struct-of-arrays feature is letting one piece of code
run over either layout, so that the choice can be measured. There is no
array-of-structs table here to switch to, and no plan for one, so that
motivation is absent -- which removes the strongest argument for an ambitious
feature and leaves the data-side wants doing all the work.

## H1: a `table` bound

`fun len<T>(ref t: T): index with { T is table, }`, joining `ord`, `float` and
`fixedint`.

The bound machinery exists and the runtime dispatches on descriptors already, so
this is mechanical. What it buys is exactly the operations that do not care what
the columns are: `len`, `is_empty`, `clear`. It does not let `get`, `push_row`
or anything column-directed be written, because inside the body `T` is still a
type nothing is known about.

**Verdict: a true step that does not reach the problem.** H2 subsumes it.

## H2: a table is `table R` for a row type `R`

Make the column list *be* a type, and `{| x: u32, y: string |}` sugar for
`table {x: u32, y: string}`. `Type::Table(Vec<TypeNamedField>)` becomes
`Type::Table(Box<Type>)`, where the inner is a struct type or a type variable.

Then the row is an ordinary parameter and the signatures write themselves:

```datalove
fun len<R>(ref t: table R): index
fun push_row<R>(mut t: table R, row: R)
fun row_at<R>(ref t: table R, i: index): ?R
fun clear<R>(mut t: table R)
```

And so do the natives, which is the half H1 cannot reach.

**Why this one fits.** The runtime's interface is already in these terms --
`table_push_row` takes a row, `table_build_from_rows` takes rows -- so the
functions above are the shape the runtime already offers, currently unreachable
because the signature cannot be written. Unification gets simpler and more
general at once: one descent into the row type instead of a column-wise walk.
And a table becomes the fourth container of a type parameter, so everything the
generics work already does -- wrapping an owned one, `DataBorrow` to open it,
descriptor-driven access into it -- applies unchanged rather than needing a
table-shaped copy.

**Column projection falls out.** `t.x` is a field of the row type lifted to a
column: where `row.x: T`, `t.x: [T]`. That is a rule rather than a special case,
and it is what makes per-column access expressible without `get(row, col)`.

**What it costs.** A syntax decision (keep `{| ... |}` as sugar, or expose
`table R`); a constraint that `R` be a struct or a variable, checked where the
type is formed; and the question of what `table u32` should mean, which is
probably "refused".

**What it does not buy.** A function over "any table that has an `x: u32`
column". `R` is all-or-nothing: either the caller fixes the whole row or the
callee knows nothing about it.

## H3: row polymorphism

`fun sum_x<R>(ref t: table {x: u32 | R}): u32` -- a function over any table
whose row has at least an `x: u32`.

**The runtime half is nearly free now.** Descriptors carry field names
(`TyInfoStructField` has `name` and `name_len`), and finding a field's offset in
a layout known only at run time is already a primitive: `dtlv_rti_field_offset`
does it by index, and a by-name variant is the same walk. So "read column `x`
out of a row whose shape I was not compiled for" is one runtime function away.
The fat-reference work made this cheap by accident.

**The type-system half is the real cost.** Row variables, absence constraints so
that `{x: u32 | R}` cannot be instantiated with an `R` that also has `x`, and
unification that does not depend on field order. That is a substantial feature,
and it is the one that would need the most care to keep "the surface syntax
should have an obvious lowering" true.

**Note that it is not really about tables.** Row polymorphism over `{x: u32 |
R}` applies to anonymous structs first and tables second. If it is wanted, it is
wanted for structs, and tables come along.

### How far up the type system this goes

"Type-level functions" above is loose, and the precision matters because it
decides how expensive H3 is.

The lambda cube separates the ways one thing can depend on another:

| | Depends on | Name |
|---|---|---|
| λ2 | terms on *types* | polymorphism -- what generics already are |
| λω | types on *types* | type operators, "type-level functions" |
| λP | types on *terms* | dependent types |

**Row algebra is λω, not λP.** `List` is already a function from a type to a
type; `R ++ S` is another. `join : table (K ++ A) -> table (K ++ B) -> table (K
++ A ++ B)` never puts a *value* in a type. It is System F-omega plus a row
theory, which is settled technology with practical designs -- Remy's, and
Leijen's scoped labels. Dependent types are a different and much larger thing,
and nothing in the table story needs them.

**One place tempts otherwise: column names look like values.** `select(t, "x")`
has a string deciding the output type, which is a term in a type. Every real
system dodges it by making labels type-level entities -- Haskell's `Symbol`,
PureScript's `Row` and `Symbol` kinds, Ur/Web's `{Nm :: Type}`. The surface
still reads `t.x`; the label elaborates to a type-level thing and it stays in
λω. That is the dodge to copy.

**The other dodge is staging.** Compute the type at compile time instead of
reasoning about it -- Zig's `fn Foo(comptime T: type) type`, C++ templates, Rust
const generics. Datalove has both ingredients already: CTFE that evaluates const
expressions including calls, and const parameters. This is the direction
[H5](#user-content-h5-types-as-compile-time-values) works through, and it turns
out not to cost compile-once the way it does in Zig -- the type is computed at
the call site while the body stays erased. It does move the checking of a *type
application* to the call site, and it does need the two roles of a const
parameter separated so that indexing a type does not also force
monomorphization.

**What each tier actually needs**, against the table above:

- Preserving -- λ2. Polymorphic in the row and nothing more. Most of a pipeline.
- Shrinking, growing, combining -- λω. Costly to build, not exotic.
- Computing -- the only tier that smells of λP, and the one every static system
  including Ur/Web expects to be written out by hand.

So the expensive end is wanted only where it would be written by hand anyway.

## H4: column projection alone, and no table module

Implement what the spec already promises -- "Column projections (e.g.,
`table.x`) yield a list view that cannot be moved or mutated, but can be passed
to `ref` parameters" -- and write no module at all.

**This is cheaper than it sounds, because the storage is columnar.** A list
header is `{data, size, capacity}`; a column view is
`{data + col_offset, table.len, table.capacity}`, which is a well-formed list
header pointing into the table. No conversion, no striding, no copy. Every
reading function in `sys/std/list` then works on a column the day projection
lands: `list.len(ref t.x)`, `ord.sorted(ref t.x)`, `ord.contains(ref t.x, ref v)`.

The view must not be mutable -- pushing to a column would corrupt the length the
other columns share -- which is the non-first-class borrow already parked in
[plan-generics.md](plan-generics.md).

**What it leaves.** Row-level operations: `push_row`, `row_at`, and a `len` for
a table with no columns to project. Those need a row type, which is H2.

**Verdict: the most value per unit of work**, and it is already the documented
intent rather than a new idea.

## H5: types as compile-time values

The Zig bargain: types are values, a function may take and return one, and the
type language *is* the term language run at compile time. `fn ArrayList(comptime
T: type) type`.

This is the stated inclination, and working it through it fits better than the
staging warning above suggested -- because it does not have to mean what it
means in Zig.

### The row algebra stops being type theory

With types as comptime values, `project`, `extend`, `concat` and `has_column`
are ordinary datalove functions over a `type`, run by CTFE:

```datalove
fun project(R: type, COLS: [string]): type       // comptime
fun select(const COLS: [string], t: table R): project(R, COLS)
```

H3 dissolves. There is no row kind, no absence constraint, no unification over
rows -- there is a library, written in the language, evaluated during
compilation. That is a large amount of type-system work traded for a language
feature the codebase is already shaped for.

Row *polymorphism* survives the trade too, as a comptime predicate rather than a
constraint solver: `with { has_column(R, "x", u32), }`. Zig does exactly this,
and inherits exactly Zig's weakness -- the constraint is checked where the
function is used, and the error is as good as the author made it.

### Why it need not monomorphize

The warning earlier was that a type computed per instantiation forces *checking*
per instantiation, which is what erasure was chosen to avoid. That is true of
Zig, which monomorphizes everything. It need not be true here, and the reason is
the machinery that already exists.

Split the two things a compile-time value is used for:

- **Computing the result type.** Happens at the *call site*, which knows the
  argument's type and the const value. The result is a concrete type there, so
  it has a static descriptor.
- **Doing the work.** Happens in the body, which is descriptor-driven anyway:
  shuffling columns means reading a descriptor and copying buffers, not knowing
  the type structurally.

So the body compiles **once**, erased, exactly as `list.reversed<T>` does today.
It declares the output shape in `descriptor_shapes`, and the call site -- which
computed the output type -- hands over the static descriptor as
`DescriptorRef::Static`. Nothing is built at run time; the descriptor for the
output schema is a static symbol at every call site, because every call site
knows its own schemas.

That is the whole of it. The existing shape channel is already "the callee
builds something it cannot name, and the caller says what it is", which is
precisely the shape of a schema transformation.

**A consequence worth naming.** A const parameter currently means two things at
once: known early enough to *specialize on*, and known early enough to *compute
a type from*. Only the second is needed here. A `select` whose body reads its
column list at run time and whose type is computed at compile time needs no
copy of itself per instantiation. If those two roles are separated, the const
parameter that indexes a type does not drag monomorphization along with it.

### What it would take

1. **A `type` type**, and `ConstValue::Type(IrType)`. Less of a leap than it
   sounds: `ConstValue::Data` already carries an `IrType` beside its payload,
   for the same reason -- a value that cannot say its own type.
2. **Types in type position computed by a call**, which is the surface feature.
3. **Phase order.** Type arguments have to be resolved before const evaluation,
   because the type feeds the computation. Today it is the other way round,
   which is why a function may not have both const and type parameters
   (`ComptimeParamOnGeneric`); the guide records it as a lowering-order
   problem rather than a fundamental one.
4. **A CTFE quota.** Type-level computation can loop, and Zig's answer -- a
   budget with a diagnostic -- is the practical one.
5. **The two roles of a const parameter separated**, per above, or every
   schema-transforming function monomorphizes and the REPL argument bites after
   all.

### What it costs

Checking moves to the call site for anything whose type is computed. The *body*
is still checked once, which is the part that matters -- what cannot be checked
in advance is the type application, and a failure there is a CTFE trace at the
call site rather than a mystery inside a template. That is the good end of the
C++ problem rather than the bad end.

The other cost is honest and unavoidable: a signature stops being readable as a
signature. `fun select(const COLS: [string], t: table R): project(R, COLS)`
says what it does only if you go and read `project`. Zig lives with this.

## If the target is Polars

The intent behind tables is a dataframe: relational operations, and pipelines
that take a table of one schema to a table of another. That changes which
hypothesis matters, and it brings in a constraint the list above does not.

### Polars itself does not type its schemas

Worth saying first, because "like Polars" can be read two ways. A Polars
`DataFrame` is **one type**; the schema is runtime data, and `select` and `join`
are checked when the plan is resolved rather than when the code is compiled.
Every dataframe library in wide use works this way -- pandas, Arrow, R.

Datalove has already chosen the other side: `{| x: u32, y: string |}` *is* a
type, and the schema is in it. So the operations that a dataframe library gets
for free by being untyped are exactly the ones that need type-level machinery
here. That is not an argument against doing it. It is an argument for knowing
that the precedent being copied solved this problem by declining it.

The statically typed precedent is Ur/Web, which types SQL with type-level
records, row concatenation and disjointness constraints. It is the existence
proof, and also the measure of what it costs.

### The operations sort by what they do to the schema

This is the useful decomposition, because the tiers need very different things.

| Tier | Operations | What it needs |
|---|---|---|
| **Preserving** | filter, sort, reverse, head, tail, slice, distinct, concat, sample | **H2 alone.** `fun filter<R>(t: table R, ref mask: [bool]): table R` is writable the day a row type exists |
| **Shrinking** | select, drop | row subtraction, or written by hand per schema pair |
| **Growing** | with_column, derive, rename | row extension, or by hand |
| **Combining** | join, union | row concatenation with disjointness |
| **Computing** | group_by/agg, pivot | type-level *functions* over labels -- the output type depends on which aggregate was applied. See [how far up this goes](#user-content-how-far-up-the-type-system-this-goes) |

The top row is the surprise. Every schema-preserving operation is generic in the
row and needs no type-level computation at all, and that is a large fraction of
a real pipeline. **H2 on its own buys a working relational vocabulary**, with
schema changes written by hand at the points where the schema changes -- which
in a pipeline is a handful of places, and arguably wants writing down anyway.

The bottom row is where even an ambitious design gives up: the type of
`group_by(...).agg(sum(x), mean(y))` is the grouping keys plus one column per
aggregate, with each type decided by which aggregate it was. Writing that output
schema by hand is what a static language should expect to do.

### No first-class functions, which settles the API shape

There are no function values here -- a function type in a parameter is a parse
error, and `Type::Function` is a `todo!()` in the IR. So the Polars *expression*
API, which is closures and lazily built expression trees (`col("x") > 5`), is
not expressible.

What is expressible is the **mask-and-column** shape:

```datalove
let mask = gt(ref t.x, : u32 / 5)      // [bool]
let hits = filter(t, ref mask)          // table R
```

Which is what Arrow and Polars actually are underneath -- kernels over columns,
with the expression API sitting on top as sugar. So the absence of closures
pushes the design toward the layer that does the work, rather than away from it.
It also means the column-wise arithmetic (`gt`, `add`, `sum`) is ordinary
generic code over `[T]` with the bounds that already exist, and belongs in
`sys/std/list` or a `column` module rather than in a table module at all.

### What this changes

**H4 stops being sufficient on its own.** Reading columns gets the kernels, but
a pipeline has to *produce* a table at each stage, and nothing can build one
from data. H2 is not optional for this target -- it is the half that matters.

**H2 rises to first.** It gives `push_row`, the schema-preserving tier in full,
and the row type that every later hypothesis is written in terms of.

**H3 becomes the question of how much of the schema algebra to buy**, rather
than an ECS curiosity. Row concatenation gets `join`; subtraction gets `select`.
They can be bought separately, and the preserving tier needs neither.

So for a dataframe the order is **H2, then H4, then as much of H3 as joins are
worth** -- the reverse of the order the general analysis suggested, because that
one was weighing reading against writing and a pipeline needs writing.

## Sequencing

**For a dataframe with comptime types: H2, then H4, then H5, and H3 not at all.**
**For a dataframe without: H2, then H4, then as much of H3 as joins are worth.**
**For tables as values only: H4, then H2.**

The order turns on whether tables are meant to be built or only read. H4 needs
no type-system change, is already specified, and unblocks every reading
operation -- because the answer there is not a table module but the list module
pointed at a column. H2 is what makes the natives writable, gives the row-level
operations, and lets a table be built from data at all.

A pipeline needs both, and needs H2 more, since a stage that cannot produce a
table is not a stage. H2 also subsumes H1, so H1 is only worth doing if H2 is
being deferred indefinitely.

H3 is a real feature with real power and should be judged as a *struct* feature
that tables inherit, not as a table fix. It is also the one the entity-component
crowd wants most once the basics are there, and the one they most often fake at
run time -- which is a reason to be sure it is wanted here before paying for it
in the type system.

H5 is the alternative to H3 rather than an addition to it: the same expressive
power bought as a library evaluated during compilation instead of as a row
theory in the checker. If it is taken, H3 should not be, and the two should not
be pursued in parallel -- they answer the same question twice.

Either way H2 comes first. A row type is what H3 quantifies over and what H5
computes with, and neither has anything to say until a table's schema is a type
rather than a fixed column list.

## What would settle it

Whether anyone wants to write a function over a table whose columns they do not
know. If the answer is no -- if every table in practice has its columns written
down at the point of use -- then H4 alone is the whole answer, and the absence
of `sys/std/table` is correct rather than a gap.

And whether a table is meant to be *built* or only written down. Everything H4
gives is reading; a table whose rows are accumulated from data needs
`push_row`, which needs H2. That is the question with the clearest consequence,
because a table that cannot be built from data is a literal with a comparison,
and nothing in the list above wants one of those.
