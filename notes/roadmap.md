# Datalove Roadmap


# in progress

## language

- move/clone semantics
- error diagnostics

## testing

- runtime value generator for proptesting
- have exampletest apply test filter

## repl

- ux
- web

## cli

## modules and standard library

- core modules

## cleanup

- runtime safety

## documentation

- improve readme




----------



# future

## near future

- don't ever construct tydescs without a tydesc table
- datafun ast roundtrip
- type declarations
- name resolution for named types
- generics
- argument modes
- move semantics
- heap genericity and clone method
- aot

## far future

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
