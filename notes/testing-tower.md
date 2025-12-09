## Core test suites

Most features appear in all of these tests.

- datalove-datalit/tests/parser_tests
- datalove-datalit/tests/resolve_tests
- datalove-datalit/tests/tycheck_tests
- datalove-datalit/tests/pretty_tests
- datalove-datalit/tests/roundtrip_tests
- datalove-datafun/tests/parser_tests
- datalove-datafun/tests/funlit_equiv_tests.
  This checks that datalit expressions and type hints are
  parsed and typechecked the same by both datafun and datalit.
  Both languages have their own expression parsers.
- datalove-datafun/tests/tycheck_tests
- datalove-datafun/tests/interp_tests
- datalove-repl/tests/engine_tests
- datalove-cli/tests/script_tests
- datalove-cli/tests/error_tests

## Other test suites

datalove-rt/tests/*
datalove-datafun/tests/interp_with_package_tests
datalove-datafun/tests/tycheck_world_tests
datalove-datafun/tests/std_tests
