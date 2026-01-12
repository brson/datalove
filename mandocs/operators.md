## 2026-01-07 - Logic operators

Booleans support `and`, `or`, `xor`, and `not`.
todo say more




## Comparison operators



## Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.





## Operator precedence

todo