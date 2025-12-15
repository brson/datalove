## Core language test suites

Most features appear in all of these tests.

- datalove-datalit/tests/parser_tests
- datalove-datalit/tests/resolve_tests
- datalove-datalit/tests/tycheck_tests
- datalove-datalit/tests/pretty_tests
- datalove-datalit/tests/roundtrip_tests
- datalove-datafun-compiler/tests/parser_tests
- datalove-datafun-compiler/tests/tycheck_tests
- datalove-datafun/tests/tycheck_world_tests
- datalove-datafun/tests/interp_tests
- datalove-datafun/tests/std_tests


## AST-gen tests

Generative tests driven by `ast_gen`.

- datalove-datalit/tests/ast_gen_tests - 
    Tests of the AST generator.
- datalove-rt-tests/tests/clone_tests
- datalove-rt-tests/tests/cmp_tests
- datalove-rt-tests/tests/eq_tests
- datalove-rt-tests/tests/destroy_tests
- datalove-rt-tests/tests/cmp_total_tests -
- datalove-rt-tests/tests/eq_unique_tests -
- datalove-datafun-compiler/tests/funlit_equiv_tests -
    Checks that datalit expressions and type hints are
    parsed and typechecked the same by both datafun and datalit.
- datalove-datafun-compiler/tests/error_equiv_tests -
    Checks that datalit expressions and type hints with
    errors are parsed and typechecked the same by both datafun and datalit.


## Runtime collections

- datalove-rt-tests/tests/list_tests
- datalove-rt-tests/tests/btreemap_tests
- datalove-rt-tests/tests/btreemap_proptests
- datalove-rt-tests/tests/btreeset_tests
- datalove-rt-tests/tests/tensor_tests

fixme: should have ast_gen collections tests too


## Other test suites

- datalove-rt-tests/tests/roundtrip_tests
- datalove-repl/tests/engine_tests
- datalove-cli/tests/script_tests
- datalove-cli/tests/error_tests
- datalove-cli/tests/type_error_tests
- datalove-datalit/tests/parser_panic_tests


## Missing suites implied by datalit/datafun symmetry

- datalove-datafun-compiler/tests/resolve_tests
- datalove-datafun-compiler/tests/pretty_tests
- datalove-datafun-compiler/tests/roundtrip_tests


## Testing utilities

Various systems exist mostly to assist writing tests:

- datalove-datalit/src/ast_gen - property-based AST generation
- datalove-datalit/src/ast_serde - AST serialization for snapshot tests
- datalove-datafun-compiler/src/ast_serde - AST serialization for snapshot tests
- datalove-datafun-compiler/src/funlit_equiv - compare datafun/datalit parsing equivalence
