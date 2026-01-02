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
- improve runtime implementation unsafety
- miri tests
- run miri on interpreter
- coverage


# on deck

- split compiler into peer/diamond deps
- refactor datafun-compiler into diamond deps
- move semantics
- rename ExprFunKind::Err to Error
- move/clone semantics - moves-etc.md
- argument modes - moves-etc.md
- destructuring
- runtime calls
- boolean ops - and or xor implies not
- anonymous enums with payloads
- named types
- clean up demo files
- alternate backend
- generics


----------



# future

- datafun ast roundtrip
- type declarations
- heap genericity and clone method
- aot
