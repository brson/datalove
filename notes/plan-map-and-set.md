# Implement maps and sets

We keep punting on implementing features for maps and sets because its hard.
This project is to bring maps and sets to feature parity with lists.

We need it fully working in datalit and datafun,
with full test cases in these suites:

- datalit parser_tests
- datalit pretty_tests
- datalit tycheck_tests
- datalit roundtrip_tests
- rt-tests btreemap_tests
- rt-tests clone_tests
- rt-tests eq_unique_tests
- rt-tests eq_tests
- rt-tests cmp_tests
- rt-tests cmp_total_tests
- rt-tests destroy_tests
- rt-tests roundtrip_tests
- datafun parser_tests
- datafun tycheck_tests
- datafun interp_tests
- repl engine_tests

After major work units we should run `just test-san-address --all` to verify memory safety.
