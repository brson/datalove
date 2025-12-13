# Datalove Roadmap

The MVP is a working bare datafun language.
With modules and closures.
With tools: repl, script runner, and compiler.
With backends: interpreter, jit, aot.
With embedding engine.
With compute-only core library.
With core->runtime calls.


# in progress

- script/function unification semantics
- fix binop/unop semantics
- move/clone semantics - moves-etc.md
- argument modes - moves-etc.md
- primer / walkthrough / tutorial
- destructuring
- runtime calls
- std modules
- improve readme


# on deck

- anonymous enums with payloads
- named types
- clean up demo files
- alternate backend
- generics
- filterable example tests


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


# fanciful ideas

- syntax highlighter cli
- datafun packages as wasm components
- ergonomic bitops and bitfields
- `datalove lit-tycheck` - run the type checker and report
- `datalove lit-pretty` - pritty printer
- `datalove lit-op` - run built-in operations
- tables proof of concept
- logic programming features:
  - generators, choice points, memoization
- explicit linear-type destructors
- token types for efficient errors
