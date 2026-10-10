# Store report

A sales report for an online store, over two years of generated orders: totals,
revenue by category and month, best sellers and customers, countries, loyalty
tiers, and which products sell together. It is a program of some size for
exercising the interpreter and the jit, rather than a single hot loop.

```
just gen 20000   # write local/store/*.dlt
just run         # print the report
just check       # compare it with check.py's
just bench       # time the interpreter, the jit and AOT
```

## Structure

```
gen.py              writes the data
check.py            the same report, in Python
report.dfs          the script: asks for the report and prints it
local/store/
  products.dlt      400 products in 8 categories, priced in cents
  customers.dlt     4,000 customers, each with a city, a join date and a tier
  orders.dlt        the orders, each with a customer, a day, a status and lines
  db.dfm            the data, and the functions the rest reads it through
  sales.dfm         totals, and revenue by category, month and product
  people.dfm        best customers, countries, tiers, and repeat purchases
  basket.dfm        pairs of products bought in the same order
  fmt.dfm           money, counts, percentages and columns as text
  report.dfm        the report's sections, as lines of text
```

`local/` beside the script is the workspace's own library, which
`datalove script` finds by itself; `local/store` is a package in it, and each
module requires the others as `local/store/<name>`.

**The data.** `db.dfm` holds all three `require data` statements and the types
they are read as. A type alias cannot be named from another module, so the
rest of the program never sees those types: it reaches the data through small
functions in `db` -- `order_count`, `line_product`, `line_revenue`,
`customer_country` and so on -- which take an order's or a customer's position
or id and answer with one field. Ids have gaps, so `db` also builds a map from
each product and customer id to its position, as module consts worked out at
compile time from the data itself. Money is in cents everywhere, as `u64` once
it is summed, so no arithmetic is in floating point.

**The queries.** `sales`, `people` and `basket` each walk the orders through
`db`, keep running totals in maps keyed by whatever they group on, and turn
those into lists of tuples sorted with `ord.sorted`. An order counts as a sale
when it was delivered or shipped. Every query is a function returning a
result, so overflow and a missing id fail loudly rather than wrapping.

**The report.** `report.dfm` has a function per section that calls a query and
formats its rows into fixed-width lines with `fmt`; `lines` puts the sections
together, and `report.dfs` prints each with `debuglog`, which is the only
output there is and why every line comes out quoted.

**The checker.** `check.py` reads the same `.dlt` files and builds the same
lines with the same rounding, padding and tie-breaking, so `just check` can
diff the two outright. A change to the program or to the engines underneath
it that alters a single figure shows up there.

## For performance work

The program is shaped like an application rather than a benchmark: 65
functions, many of them small accessors called millions of times, map
lookups and inserts on every line, sorting, string building, and a long tail
of code that runs once. Compiling it reads the data and evaluates the id maps,
which is about a quarter of a run at 20,000 orders. `just gen` takes any
order count, and the time grows about linearly with it.

Every query is a function called once that loops over all the orders, so a
jit that compiles only on call counts never compiles the loops that matter;
see [Tuning the JIT](../../botdocs/plan-jit-tuning.md). `just jit-stats 100`
shows what a threshold leaves interpreted and how often calls cross into
compiled code.
