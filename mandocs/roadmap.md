# Datalove Roadmap

The MVP is a working bare datafun language.
With modules and closures.
With tools: repl, script runner, and compiler.
With backends: interpreter, jit, aot.
With embedding engine.
With compute-only core library.
With core->runtime calls.


# in progress

- ir and interp3
- human docs
- refactor datafun-compiler into diamond deps


# on deck

- interp coverage
- run miri on interpreter
- move semantics
- rename ExprFunKind::Err to Error
- move/clone semantics - moves-etc.md
- argument modes - moves-etc.md
- destructuring
- runtime calls
- boolean ops - and or xor implies not
- named types
- clean up demo files in style of learn x in y minutes
- alternate backend
- generics
- datafun ast roundtrip


----------



# future

- improve runtime implementation unsafety
- type declarations
- heap genericity and clone method
- aot
