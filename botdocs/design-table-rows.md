# Tables: what the type system is missing, and four ways out

A table can be built, passed, cloned, dropped and held inside anything. It has
no operations at all, and none can be written. This is why, and what could be
done about it.

Hypotheses, not a plan. Nothing here is implemented.

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

## Sequencing

**H4, then H2, and H3 only if asked for.**

H4 needs no type-system change, is already specified, and unblocks most of what
anyone would want a table module for -- because the answer turns out not to be a
table module but the list module pointed at a column.

H2 is what makes the natives writable and gives the row-level operations, and it
is the one the runtime is already built for. It also subsumes H1, so H1 is only
worth doing if H2 is being deferred indefinitely.

H3 is a real feature with real power and should be judged as a *struct* feature
that tables inherit, not as a table fix.

## What would settle it

Whether anyone wants to write a function over a table whose columns they do not
know. If the answer is no -- if every table in practice has its columns written
down at the point of use -- then H4 alone is the whole answer, and the absence
of `sys/std/table` is correct rather than a gap.
