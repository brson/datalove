---
title: "Const parameter specialization"
category: news
summary: "Functions can now have const parameters that trigger specialization"
---

Following on the initial CTFE work,
Datalove now supports const parameter specialization.
Functions can declare parameters as `const`,
and the compiler will specialize the function
for each unique combination of const argument values.

```datalove
fun scale(const factor: int, x: int): int
    ret x * factor
end fun

const TWO: int = 2
const TEN: int = 10

let a = scale(TWO, 5)   // specialized for factor=2
let b = scale(TEN, 5)   // specialized for factor=10
```

The compiler evaluates const arguments at compile time,
generates specialized function variants,
and rewrites call sites to target the correct specialization.

This is mostly just architectural work
to make sure it all fits together.
I'm hopeful this simple specialization will naturally
extend to generic type parameters.

I'm experimenting with a compromise monomorphization strategy
that relies on whole-program compilation to see all instantiations:
instead of generating a function for every set of instantiations
(or every set with the same "shape"),
I am generating one function that branches on a single
discriminator that represents the set of instantiations.
Hoping its easier on codegen and linking.


