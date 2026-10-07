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

The data is read with `require data`, all of it in `local/store/db.dfm`, which
the other modules reach through functions: a type alias cannot be named from
another module. `check.py` computes the same report in Python, line for line,
from the same files.
