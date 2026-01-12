### 2026-01-08 - Module memoization tests

We focus our memoization testing at module granularity
because module-level memoization is impactful
and easy to reason about in tests.

We care about testing the changing
over time of three module-level values:
the ast,
the typecheck results.
the content hash,

We test the changes in these values
after discreet actions:

- add-module - adds a module that doesn't already exist
- remove-module - removes a module that exists
- change-module-ws - replace an existing module source that changes the whitespace,
  (but not the newlines in the whitespace)
- change-module-ast - replace an existing module with source
  that produces a different ast does not change the typechecking
- change-module-ty - replace an existing module with source
  that changes the ast and produces a different typecheck result

This table indicates whether we expect recalculation
of the directly changed module, or its transitive dependents,
after each of the actions has been taken.

| action            | direct-ast | direct-ty | direct-hash | depend-ast | depend-ty | depend-hash |
|-------------------|------------|-----------|-------------|------------|-----------|-------------|
| add-module        | y          | y         | y           | n/a        | n/a       | n/a         |
| remove-module     | y*         | y*        | y*          | **         | **        | **          |
| change-module-ws  | y          | n         | y           | n          | n         | y           |
| change-module-ast | y          | y         | y           | n          | n         | y           |
| change-module-ty  | y          | y         | y           | n          | y         | y           |

> *: removed
> **: i think this case is impossible because the module graph can't resolve

Our test suite is a worldfile variant with the following sections:
`module`, `module-add`, `module-change-ws`, `module-change-ast`, `module-change-ty`.
Note that the test must trust the user that they have got the "change-*" semantics correct -
it just knows its changing a module.

Each contains the source of a module with its canonical lib/pkg/module path.
The test harness first loads all `module` sections into the module world,
parses and typechecks.

Then for each of the action sections in turn:

- merge the module into the module world (or remove it)
- run the parser and typechecker, calculate content hashes
- for each module that remains, calculate `changed_ast`, `changed_ty`, `changed_hash`,
- compare the results to our expected results based on the table above
- add the observed change analysis plus their expected results to the "actual" output

The pass/fail-ness of the test is determined by the blessed "expected" files;
the calculated analysis is just to help guide is to a fully-working memoization system.




### 2026-01-07 - Module content hashes and memoization

The module graph forms a DAG.
We use this to create strong content hashes
for every module instantiation.

This can be used as a key for various caching purposes.
We specifically use it to verify correct memoization:
Functions on modules should only rerun if their
module's content hash has changed.

This content hash includes
the source code of a module,
and the configuration of that module
including which modules the requires/import demands are bound to.





