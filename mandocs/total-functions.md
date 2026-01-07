# Total functions in Datalove Functions

Datalove functions (`fun`)
are pure but not total.
They would be total but for one thing:
functions are not required to terminate.

We can probably prove termination of many functions though
thanks to type system restrictions.

We`ll allow functions and loops to be declared `total`,
and the compiler abliged to prove it.




## Why is it useful to know total functions?

- You can know const evaluation will succeed.
- With dependent types, computation on types during typechecking is guaranteed to succeed.
- Total functions over types are valid proofs.
- Guaranteed non-blocking.




## Useful type system and language features

- Whole program compilation.
- Pure functions, no panics.
- Linear types, no aliasing.
- No interior mutability.
- Loop induction variables.
- Total ordering on all data.
- Bigints cannot overflow.
- Fixed ints cannot overflow, but early-return.
- Static call graph.
- Simple CFG and IR.

When closures added need to be careful to preserve above.

Implications of this:

- We can know the exact set of recursive and non-recursive functions.
- We can do strong analysis on loops.

For any loop with
an induction variable that moves toward a fixed bound,
a comparison-based exit condition,
termination is guaranteed by the integer semantics alone, either:
1. The induction variable reaches the bound,
2. The induction variable overflows and early-returns.




## Clearly total

Given what the type system knows trivially,
we can already prove many functions total.

Functions that don't call other functions.

```datalove
total fun foo(a: int): int
  ret a + 1
end fun
```

Non-recursive functions where the call graph contains no loops.

```datalove
total fun foo(a: int): int
  ret bar(a)
end fun

total fun bar(a: int): int
  ret a + 1
end fun
```

Most functions will be non-recursive.
Now we just need to be able to prove some loops are total.




## Proving loops total

Proving this shape of problem would unlock a lot:

```datalove
total fun foo(max: int): int
  total loop carry (index = 0, sum = 0)
    if index .< max
      continue (index + 1, sum + index)
    else
      ret sum
    end if
  end loop
end fun
```

To prove this total use
linear ranking functions via polyhedral analysis.

This loop is entirely linear:
- Induction variable: index' = index + 1
- Bound: max (loop-invariant constant)
- Exit condition: index >= max (linear inequality)
- Measure: max - index (linear expression)

How it works:

1. Express loop state as a polyhedron (linear constraints on variables)
2. Search for a linear ranking function r(vars) such that:
  - r(vars) >= 0 while loop continues
  - r(vars') < r(vars) after each iteration
3. For this loop: r(index, max) = max - index satisfies both
