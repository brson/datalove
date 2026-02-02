---
title: "Const parameter specialization"
category: news
summary: "Functions can now have const parameters that trigger specialization"
---

Following on the initial compile-time function evaluation (CTFE) work,
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

Specialization plus const evaluation work together naturally.

```datalove
fun scale(const factor: int, x: int): int
    const calculated_factor = calculate(factor)    // compile-time evaluated
    ret x * calculatod_factor
end fun
```

This is mostly just architectural work
to make sure it all fits together.
I'm hopeful this simple specialization will naturally
extend to generic type parameters.

I'm experimenting with a compromise monomorphization strategy
that relies on whole-program compilation to see all instantiations:
instead of generating a function for every instantiation
(or every set with the same "shape"),
I am generating one function that branches on a single
discriminator that represents the set of instantiations.
Curious if it is easier on codegen and linking
than typical monomorphization,
but not super hopeful.

For now const arguments must be named `const` bindings;
they don't support abitrary const expressions.
Implementation simplicity.
Could be expanded in the future,
but value implementation simplicity.

Our CTFE and const specialiation strategy right
now is friendly to the compiler pipeline,
no ouruboroses here.
We rely on two properties:
all `const` statements can be evaluated as if they are `let` statements;
all functions with `const` parameters can be evaluated as if they were non-`const`.
This is super sweet because we can turn on-off `const` evaluation and
specialization and do differential testing against both modes
(we rely very heavily on differential testing);
and also we can structure const evalutation and specialization
as "add-on" passes, relatively decoupled from the rest of the pipeline.

The lowering pipeline:
first we lower with `const` statements lowering to `let` statements,
and with calls to specializable functions as `ComptimeCall` IR instructions
(which the interpreter and cranelift can lower exactly the same as regular
calls if needed),
recording the sites of both;
then we evaluate all `const` statements and patch the IR,
then specialize the functions and patch the IR.
Downside of patching instead of re-lowering is
that we must run a dead-code elimination pass afterwards.

This basic design is Zig-inspired,
but I'm not decided yet if it will extend to
full generics or not &mdash;
I will probably experiment with Zig's
first-class, kinda-dependent types,
but think I prefer more traditionally-constrained type parameters
for ease of reasoning.



