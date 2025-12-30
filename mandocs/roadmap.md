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
  - fix function resolution
  - implement error and add missing ! tests
- script/function unification semantics
- move/clone semantics - moves-etc.md
- argument modes - moves-etc.md
- primer / walkthrough / tutorial
- destructuring
- runtime calls
- std modules
- improve readme


# on deck

- boolean ops - and or xor implies not
- anonymous enums with payloads
- named types
- clean up demo files
- alternate backend
- generics


----------



# future

- have exampletest apply test filter
- repl terminal and web
- datafun ast roundtrip
- type declarations
- name resolution for named types
- generics
- move semantics
- heap genericity and clone method
- aot
