# Implement maps and sets

We keep punting on implementing features for maps and sets because its hard.
This project is to bring maps and sets to feature parity with lists.

## Current State Analysis

### Maps (BTreeMap) - Well-implemented but incomplete test coverage

**Runtime API (crates/datalove-rt/src/btreemap.rs):**
- ✅ btreemap_create_impl - Create empty map
- ✅ btreemap_destroy_impl - Destroy/free a map
- ✅ btreemap_insert_impl - Insert or update key-value pair
- ✅ btreemap_remove_impl - Remove key-value pair
- ✅ btreemap_get_impl - Get value by key (returns Option)
- ✅ btreemap_clear_impl - Clear all entries
- ✅ btreemap_clone_from_slice_impl - Create from slice of tuples

**Exposed extern "C" API (crates/datalove-rt/src/lib.rs):**
- dtlv_rti_btreemap_create_local
- dtlv_rti_btreemap_destroy_local
- dtlv_rti_btreemap_insert_local
- dtlv_rti_btreemap_remove_local
- dtlv_rti_btreemap_get / dtlv_rti_btreemap_get_local
- dtlv_rti_btreemap_clear_local
- dtlv_rti_btreemap_clone_from_slice_local

**Test Coverage:**
- ✅ btreemap_tests.rs (43 unit tests covering create, destroy, insert, get, remove, clear, splits, rebalancing)
- ✅ btreemap_proptests.rs (18 property tests for random operations, stress tests, string keys/values)
- ✅ clone_tests.rs (5 map tests: empty, single entry, multiple entries)
- ✅ cmp_tests.rs (map comparison tests: empty, equal, less/greater by key/value)
- ❌ eq_tests.rs - NO map tests
- ❌ eq_unique_tests.rs - NO map tests
- ❌ cmp_total_tests.rs - NO map tests
- ❌ destroy_tests.rs - NO map tests
- ❌ rt-tests roundtrip_tests - NO map fixtures

**Datalit Test Coverage:**
- ✅ parser_tests: 6 map fixtures (18_map.dlt, 19_set.dlt, 41_type_no_sigil_map.dlt, etc.)
- ✅ pretty_tests: 7 map fixtures (07_map.dlt, 11_map_simple.dlt, 12_map_nested.dlt, 13_map_empty.dlt)
- ✅ roundtrip_tests: 6 map fixtures (09_map_u32_string.dlt, 10_map_nested.dlt, 11_map_empty.dlt)
- ⚠️  tycheck_tests: Type checking works but no dedicated fixtures

**Datafun Test Coverage:**
- ✅ parser_tests: 3 map fixtures (26_datalit_map.dfs, 27_datalit_set.dfs, 31_datalit_map_with_list_values.dfs)
- ✅ tycheck_tests: 2 map fixtures (13_datalit_map.dfs, 14_datalit_set.dfs)
- ✅ interp_tests: 2 map fixtures (14_literal_map.dfs, 15_literal_set.dfs)

### Sets (BTreeSet) - Partially implemented, NO public runtime API

**Runtime Implementation (crates/datalove-rt/src/set.rs):**
- ✅ Internal functions: set_clone_tree, set_destroy_impl
- ✅ Internal helpers: read_node_tag, read_node_len, free_node, destroy_tree_recursive, clone_tree_recursive
- ❌ NO public btreeset_*_impl functions
- ❌ NO btreeset_create_impl
- ❌ NO btreeset_insert_impl
- ❌ NO btreeset_remove_impl
- ❌ NO btreeset_contains_impl
- ❌ NO btreeset_clear_impl
- ❌ NO btreeset_clone_from_slice_impl

**Exposed extern "C" API:**
- ❌ NO dtlv_rti_btreeset_* functions exposed

**Test Coverage:**
- ❌ NO btreeset_tests.rs
- ❌ NO btreeset_proptests.rs
- ✅ clone_tests.rs (5 set tests: empty, single, multiple, strings, nested tuples)
- ✅ cmp_tests.rs (7 set tests: empty vs empty, empty vs nonempty, equal, less/greater)
- ❌ eq_tests.rs - NO set tests
- ❌ eq_unique_tests.rs - NO set tests
- ❌ cmp_total_tests.rs - NO set tests
- ✅ destroy_tests.rs (5 set tests: empty, primitives, strings, tuples, large)
- ❌ rt-tests roundtrip_tests - NO set fixtures

**Higher-level test coverage:**
- Sets work through datalit/datafun but only with existing operations (clone, destroy, compare)
- No runtime API means no insert/remove/contains operations available

### Lists (Reference for Feature Parity)

**List API for comparison:**
- dtlv_rti_list_create_local
- dtlv_rti_list_destroy_local
- dtlv_rti_list_clear_local
- dtlv_rti_list_get
- dtlv_rti_list_set_local
- dtlv_rti_list_push_local
- dtlv_rti_list_pop_local
- dtlv_rti_list_insert_local
- dtlv_rti_list_remove_local
- dtlv_rti_list_reserve_local
- dtlv_rti_list_shrink_to_fit_local
- dtlv_rti_list_clone_from_slice_local
- dtlv_rti_list_extend_from_slice_local

## Implementation Plan

### Phase 1: Set Runtime Implementation (crates/datalove-rt/src/set.rs)

Implement core set operations following btreemap.rs patterns:

1. **btreeset_create_impl** - Create empty set
2. **btreeset_insert_impl** - Insert element (returns bool if newly inserted)
3. **btreeset_remove_impl** - Remove element
4. **btreeset_contains_impl** - Check membership (returns bool)
5. **btreeset_clear_impl** - Remove all elements
6. **btreeset_clone_from_slice_impl** - Create set from slice of elements

Note: Sets are B+trees with only keys (no values), similar to btreemap but simpler.

**Memory Safety Checkpoint:** Run `just test-san-address -p datalove-rt` after implementation.

### Phase 2: Set API Exposure (crates/datalove-rt/src/lib.rs)

Add extern "C" wrappers for all set operations:

- dtlv_rti_btreeset_create_local
- dtlv_rti_btreeset_destroy_local
- dtlv_rti_btreeset_insert_local
- dtlv_rti_btreeset_remove_local
- dtlv_rti_btreeset_contains_local
- dtlv_rti_btreeset_clear_local
- dtlv_rti_btreeset_clone_from_slice_local

### Phase 3: Runtime Test Suite - BTreeSet Unit Tests

**Create crates/datalove-rt-tests/tests/btreeset_tests.rs** (~40 tests):

Basic operations:
- test_create_empty_set
- test_destroy_empty_set
- test_insert_single_element
- test_insert_multiple_elements
- test_insert_duplicate (should not increase size)
- test_insert_ordering (elements stored in order)
- test_remove_existing
- test_remove_nonexistent
- test_contains_existing
- test_contains_nonexistent
- test_clear_empty
- test_clear_nonempty

Edge cases:
- test_insert_with_split (force B-tree node splits)
- test_remove_with_rebalance (force node borrowing/merging)
- test_insert_reverse_order
- test_large_set (stress test with many elements)

Data types:
- test_set_u32
- test_set_string
- test_set_tuples
- test_set_nested_types

Clone operations:
- test_clone_from_slice_empty
- test_clone_from_slice_single
- test_clone_from_slice_multiple
- test_clone_from_slice_with_duplicates

### Phase 4: Runtime Test Suite - BTreeSet Property Tests

**Create crates/datalove-rt-tests/tests/btreeset_proptests.rs** (~15 tests):

- prop_insert_random_u32 (0-1000 elements)
- prop_insert_order_independence
- prop_insert_duplicate_maintains_size
- prop_insert_many_elements (stress test)
- prop_remove_inserted_elements
- prop_clear_resets_size
- prop_insert_clear_cycles
- prop_contains_after_insert
- prop_not_contains_after_remove
- prop_string_elements (same tests with strings)

### Phase 5: Runtime Test Suite - Add Map/Set to Existing Tests

**eq_tests.rs** - Add map and set equality tests:

Maps:
- test_eq_map_empty_equals
- test_eq_map_equals_same_order
- test_eq_map_equals_different_insert_order
- test_eq_map_not_equals_different_keys
- test_eq_map_not_equals_different_values
- test_eq_map_not_equals_different_sizes

Sets:
- test_eq_set_empty_equals
- test_eq_set_equals_same_order
- test_eq_set_equals_different_insert_order
- test_eq_set_not_equals_different_elements
- test_eq_set_not_equals_different_sizes

**eq_unique_tests.rs** - Add unique equality tests for maps and sets

**cmp_total_tests.rs** - Add total ordering tests for maps and sets

**destroy_tests.rs** - Add map destroy tests:
- test_destroy_map_empty
- test_destroy_map_primitives
- test_destroy_map_strings
- test_destroy_map_tuples
- test_destroy_map_nested
- test_destroy_map_large

**roundtrip_tests** - Add fixtures to crates/datalove-rt-tests/tests/fixtures/roundtrip/:

Maps:
- XX_map_empty.dlt
- XX_map_u32_string.dlt
- XX_map_string_u32.dlt
- XX_map_nested_values.dlt

Sets:
- XX_set_empty.dlt
- XX_set_u32.dlt
- XX_set_string.dlt
- XX_set_tuples.dlt

**Memory Safety Checkpoint:** Run `just test-san-address --all` after completing runtime tests.

### Phase 6: Datalit Test Coverage

**tycheck_tests** - Add fixtures to crates/datalove-datalit/tests/fixtures/tycheck/:
- XX_map_valid.dlt
- XX_map_nested.dlt
- XX_map_invalid_key_type.dlt (error case)
- XX_set_valid.dlt
- XX_set_nested.dlt

Verify existing tests still pass:
- parser_tests (already has 6 map/set fixtures)
- pretty_tests (already has 7 map/set fixtures)
- roundtrip_tests (already has 6 map/set fixtures)

### Phase 7: Datafun Test Coverage

**interp_tests** - Add fixtures to crates/datalove-datafun/tests/fixtures/interp/:
- XX_map_insert_get.dfs (if map operations are exposed)
- XX_map_operations.dfs
- XX_set_insert_contains.dfs (if set operations are exposed)
- XX_set_operations.dfs

Verify existing tests:
- parser_tests (already has 3 fixtures)
- tycheck_tests (already has 2 fixtures)

### Phase 8: Repl Test Coverage

**engine_tests** - Add fixtures to crates/datalove-repl/tests/fixtures/engine/:
- XX_repl_map_create.txt (interactive map creation)
- XX_repl_map_operations.txt (interactive manipulation)
- XX_repl_set_create.txt
- XX_repl_set_operations.txt

### Phase 9: Final Verification

1. Run full test suite: `just test-san-address --all`
2. Verify all test suites have map/set coverage
3. Document any limitations or API differences from lists

## Test Suite Status Checklist

Runtime tests:
- [x] rt-tests btreemap_tests (43 tests - COMPLETE)
- [ ] rt-tests btreeset_tests (NEEDS CREATION)
- [x] rt-tests btreemap_proptests (18 tests - COMPLETE)
- [ ] rt-tests btreeset_proptests (NEEDS CREATION)
- [x] rt-tests clone_tests (5 map + 5 set tests - HAS COVERAGE)
- [ ] rt-tests eq_unique_tests (NEEDS map/set tests)
- [ ] rt-tests eq_tests (NEEDS map/set tests)
- [x] rt-tests cmp_tests (HAS map/set coverage)
- [ ] rt-tests cmp_total_tests (NEEDS map/set tests)
- [x] rt-tests destroy_tests (0 map + 5 set tests - NEEDS map tests)
- [ ] rt-tests roundtrip_tests (NEEDS map/set fixtures)

Datalit tests:
- [x] datalit parser_tests (6 fixtures - HAS COVERAGE)
- [x] datalit pretty_tests (7 fixtures - HAS COVERAGE)
- [ ] datalit tycheck_tests (NEEDS dedicated fixtures)
- [x] datalit roundtrip_tests (6 fixtures - HAS COVERAGE)

Datafun tests:
- [x] datafun parser_tests (3 fixtures - HAS COVERAGE)
- [x] datafun tycheck_tests (2 fixtures - HAS COVERAGE)
- [ ] datafun interp_tests (2 fixtures - NEEDS MORE)

Repl tests:
- [ ] repl engine_tests (NEEDS fixtures)

## Notes

After major work units we should run `just test-san-address --all` to verify memory safety.
