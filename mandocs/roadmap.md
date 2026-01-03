# Datalove Roadmap

The MVP is a working bare datafun language with:

- documentation
- modules
- closures
- generic functions for built-in generic types
- repl, script runner
- interpreter, jit, aot
- compute-only core library
- core->runtime calls


# in progress

- run miri on interpreter
- human docs


# on deck

- diagnostics reporting
- diagnostics plumbing
- memoization tests
- add alignment checks to the runtime
- interp coverage
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
