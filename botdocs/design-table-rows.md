# Tables: what the type system is missing, and four ways out

A table is a first-class opaque value: it can be built, moved, cloned, compared,
sorted, keyed on and held inside anything, and nothing can look inside one. This
is why, and what could be done about it.

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
that tables inherit, not as a table fix. It is also the one the entity-component
crowd wants most once the basics are there, and the one they most often fake at
run time -- which is a reason to be sure it is wanted here before paying for it
in the type system.

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
