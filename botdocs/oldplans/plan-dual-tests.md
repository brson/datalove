# Plan: Comprehensive dual_tests Test Suite

## Overview
Design a complete test suite for `dual_tests` that validates interpreter/AOT parity across all language features. Tests organized by feature category, with each feature tested in three contexts where applicable.

## Current Progress

**Total dual tests: 105**

### Completed Categories
- [x] 001-009: Debuglog Basics (4 tests)
- [x] 010-049: Literals & Primitives (28 tests)
- [x] 050-099: Collections (12 tests)
- [x] 100-149: Aggregates (9 tests)
- [x] 150-199: Arithmetic Operators (9 tests)
- [x] 200-249: Comparison Operators (8 tests)
- [x] 250-299: Variables & Bindings (8 tests)
- [x] 300-349: Control Flow (9 tests)
- [x] 350-399: Functions (10 tests)
- [x] 400-449: Try Operators (3 tests)
- [x] 450-499: Type Conversions (5 tests)

### Not Yet Started
- [ ] 500-549: Modules & Imports
- [ ] 550-599: Linear Type Semantics
- [ ] 600-649: Combinations

---

## Known Issues and Resolutions

### 1. Mutual Recursion in scriptunit-fragment
**Status:** EXPECTED BEHAVIOR - scripts use backward name resolution
**Issue:** Forward references in scriptunit-fragment don't work
**Resolution:** Scripts use one-pass compilation with backward name resolution only. Mutual recursion is supported in modules (see aot test `081_mutual_recursion_module.world`). This is not a bug.

### 2. Try Operator Error Path
**Status:** FIXED
**Issue:** Output mismatch on try operator early return with error
**Fix:** Added `debuglog` call to interpreter's `UnitEarlyReturn` handler in `datalove-datafun-interp/src/lib.rs` to match AOT behavior. Both now output the error value via debuglog before early return.
**Test:** `401_try_result_error.world` now passes.

### 3. Mut Param Read-Modify-Write with Linear Types
**Status:** FIXED - tests now use proper int coercion
**Issue:** Patterns like `set x = x + @10` where `x: mut int` fail typecheck
**Fix:** Use `let ten: int = @10` to coerce the literal, then `set x = x + ten`.
**Tests:** `363_param_mut_read.world` and `366_param_mut_passthrough.world` now properly exercise read-modify-write.

### 4. AOT Duplicate String Symbol Bug
**Status:** FIXED
**Issue:** When module and script both use string constants, got "Duplicate definition of identifier: __string_bytes_0"
**Fix:** Changed `emit_static_bytes` in `datalove-datafun-aot-cranelift/src/codegen/constants.rs` to include function name in symbol: `format!("__string_bytes_{}_{}", self.func.name, id)`
**Test:** `081_mutual_recursion_module.world` now passes.

### 5. Missing Test Coverage
**Status:** TODO
**Tests not yet created:**
- Optional arithmetic operators (`+?`, `-?`, `*?`, `/?`)
- If-let with Result binding (`320-322`)
- Module-level tests (`500-549`)
- Linear semantics tests (`550-599`)
- Complex combination tests (`600-649`)

---

## Test Contexts
Each feature should be tested in up to three contexts:
1. **Script** (`_script`) - Direct code in scriptunit-fragment
2. **Script Function** (`_sfn`) - Function defined and called within script
3. **Module Function** (`_mfn`) - Function defined in module, called from script

## Naming Convention
`NNN_category_feature[_context][_variant].world`

Examples:
- `010_literal_u32_script.world`
- `011_literal_u32_sfn.world`
- `012_literal_u32_mfn.world`
- `040_arith_add_i32_overflow.world`

## Test Categories

### 001-009: Debuglog Basics
Foundation tests ensuring debuglog works for output capture.
```
001_simple_debuglog.world       # DONE
002_debuglog_bool.world         # DONE
003_debuglog_string.world       # DONE
004_debuglog_multiple.world     # DONE
```

### 010-049: Literals & Primitives
Test all primitive types and literal forms.

**Scalars (Copy types):**
```
010_literal_bool_true.world     # DONE
011_literal_bool_false.world    # DONE
012_literal_u8.world            # DONE
013_literal_u16.world           # DONE
014_literal_u32.world           # DONE
015_literal_u64.world           # DONE
016_literal_i8.world            # DONE
017_literal_i16.world           # DONE
018_literal_i32.world           # DONE
019_literal_i64.world           # DONE
020_literal_f32.world           # DONE
```

**Hex literals:**
```
021_literal_hex_u32.world       # DONE
022_literal_hex_u64.world       # DONE
023_literal_hex_i32.world       # DONE
024_literal_hex_int.world       # DONE
025_literal_hex_f32_bits.world  # DONE
```

**Linear types:**
```
030_literal_int.world           # DONE
031_literal_string.world        # DONE
032_literal_string_escape.world # TODO
```

**Special types:**
```
040_literal_unit.world          # DONE
041_literal_option_some.world   # DONE
042_literal_option_none.world   # DONE
043_literal_result_ok.world     # DONE
044_literal_data.world          # DONE
045_literal_error.world         # DONE
```

### 050-099: Collections
Test list, set, map with various element types.

**Lists:**
```
050_list_empty_u32.world        # DONE
051_list_u32_elements.world     # DONE
052_list_string_elements.world  # DONE
053_list_nested.world           # TODO
054_list_in_sfn.world           # TODO
055_list_in_mfn.world           # TODO
```

**Sets:**
```
060_set_empty_u32.world         # DONE
061_set_u32_elements.world      # DONE
062_set_string_elements.world   # TODO
063_set_in_sfn.world            # TODO
064_set_in_mfn.world            # TODO
```

**Maps:**
```
070_map_empty.world             # DONE
071_map_u32_u32.world           # DONE
072_map_string_string.world     # TODO
073_map_string_u32.world        # TODO
074_map_in_sfn.world            # TODO
075_map_in_mfn.world            # TODO
```

**Tensors:**
```
080_tensor_1d_u32.world         # DONE
081_tensor_2d_u32.world         # DONE
082_tensor_in_sfn.world         # TODO
```

### 100-149: Aggregates
Test tuples, structs, enums.

**Tuples:**
```
100_tuple_pair_u32.world        # DONE
101_tuple_mixed_types.world     # DONE
102_tuple_with_string.world     # DONE
103_tuple_nested.world          # TODO
104_tuple_empty.world           # TODO
105_tuple_in_sfn.world          # TODO
106_tuple_in_mfn.world          # TODO
```

**Structs:**
```
110_struct_single_field.world   # DONE
111_struct_two_fields.world     # DONE
112_struct_with_int.world       # DONE
113_struct_nested.world         # TODO
114_struct_in_sfn.world         # TODO
115_struct_in_mfn.world         # TODO
```

**Enums:**
```
120_enum_no_payload.world       # DONE
121_enum_u32_payload.world      # DONE
122_enum_string_payload.world   # DONE
123_enum_mixed_payloads.world   # TODO
124_enum_in_sfn.world           # TODO
125_enum_in_mfn.world           # TODO
```

### 150-199: Arithmetic Operators

**Bare arithmetic (widening to int):**
```
150_arith_add_u32.world         # DONE
151_arith_sub_u32.world         # DONE
152_arith_mul_u32.world         # DONE
153_arith_neg_i32.world         # DONE
154-158                         # TODO
```

**Checked arithmetic (+!, -!, *!, /!) returning Result:**
```
160_arith_add_checked_success.world    # DONE
161_arith_add_checked_overflow.world   # DONE
162_arith_sub_checked_underflow.world  # DONE
163_arith_mul_checked_overflow.world   # DONE
164_arith_div_checked_divzero.world    # DONE
165-169                                # TODO
```

**Optional arithmetic (+?, -?, *?, /?) returning Option:**
```
170-179                         # TODO - all
```

**Arithmetic in functions:**
```
180-183                         # TODO - all
```

### 200-249: Comparison Operators
```
200_cmp_eq_u32.world            # DONE
201_cmp_ne_u32.world            # DONE
202_cmp_lt_u32.world            # DONE
203_cmp_le_u32.world            # DONE
204_cmp_gt_u32.world            # DONE
205_cmp_ge_u32.world            # DONE
206_cmp_eq_i32.world            # DONE
207_cmp_lt_i32_negative.world   # DONE
208_cmp_eq_bool.world           # TODO
209_cmp_eq_string.world         # TODO
210_cmp_in_sfn.world            # TODO
211_cmp_in_mfn.world            # TODO
```

### 250-299: Variables & Bindings

**Let bindings:**
```
250_let_u32.world               # DONE
251_let_string.world            # DONE
252_let_with_type.world         # TODO
253_let_in_sfn.world            # TODO
254_let_in_mfn.world            # TODO
```

**Var bindings and mutation:**
```
260_var_u32.world               # DONE
261_var_set_u32.world           # DONE
262_var_string.world            # DONE
263_var_in_sfn.world            # TODO
264_var_in_mfn.world            # TODO
```

**Shadowing:**
```
270_shadow_let_let.world        # DONE
271_shadow_var_var.world        # DONE
272_shadow_let_var.world        # DONE
273_shadow_var_let.world        # DONE
274_shadow_let_arg.world        # TODO
275_shadow_var_arg.world        # TODO
276_shadow_crossunit.world      # TODO
```

### 300-349: Control Flow

**If/else:**
```
300_if_true.world               # DONE
301_if_else.world               # DONE
302_if_nested.world             # DONE
303_if_in_sfn.world             # TODO
304_if_in_mfn.world             # TODO
```

**If-let with Option:**
```
310_if_option_some.world        # DONE
311_if_option_none.world        # DONE
312_if_option_binding.world     # TODO
313_if_option_nested.world      # TODO
```

**If-let with Result:**
```
320_if_result_ok.world          # TODO
321_if_result_error.world       # TODO
322_if_result_binding.world     # TODO
```

**Loops:**
```
330_loop_break.world            # DONE
331_loop_continue.world         # DONE
332_loop_counter.world          # DONE
333_loop_simple.world           # DONE
334_loop_in_sfn.world           # TODO
335_loop_in_mfn.world           # TODO
```

### 350-399: Functions

**Basic functions:**
```
350_fn_no_params.world          # DONE
351_fn_one_param.world          # DONE
352_fn_multi_params.world       # DONE
353_fn_return_value.world       # DONE
354_fn_call_chain.world         # DONE
355_fn_nested_calls.world       # DONE
356_fn_void.world               # TODO
357_fn_early_return.world       # TODO
```

**Parameter modes:**
```
360_param_ref_basic.world       # DONE
361_param_ref_linear.world      # DONE
362_param_mut_basic.world       # DONE
363_param_mut_read.world        # DONE (read-modify-write with int)
364_param_out_basic.world       # DONE
365_param_ref_passthrough.world # DONE
366_param_mut_passthrough.world # DONE (passthrough with read-modify-write)
367_param_out_string.world      # TODO
368_param_out_linear.world      # TODO
```

**Recursion:**
```
370_recursion_simple.world      # DONE
371_recursion_factorial.world   # DONE
372_recursion_fibonacci.world   # DONE
373_recursion_mutual.world      # N/A - forward refs not supported in scripts
```

### 400-449: Try Operators

**Try with Result (!):**
```
400_try_result_ok.world         # DONE
401_try_result_error.world      # DONE
402_try_result_in_function.world # DONE
403-409                         # TODO
```

**Try with Option (?):**
```
410-415                         # TODO - all
```

### 450-499: Type Conversions

**Error wrapping:**
```
450_error_from_u32.world        # DONE
451_error_from_bool.world       # DONE
452_error_from_string.world     # TODO
453_error_from_int.world        # TODO
454_error_from_tuple.world      # TODO
455_error_in_sfn.world          # TODO
456_error_in_mfn.world          # TODO
```

**Data wrapping:**
```
452_data_from_u32.world         # DONE
453_data_from_bool.world        # DONE
454_data_from_string.world      # DONE
460-466                         # TODO - remaining
```

### 500-549: Modules & Imports
**Status:** NOT STARTED

### 550-599: Linear Type Semantics
**Status:** NOT STARTED

### 600-649: Combinations
**Status:** NOT STARTED

---

## Implementation Strategy

### Phase 1: Core Features (001-199) - MOSTLY COMPLETE
Debuglog, literals, collections, aggregates, arithmetic. ~100 tests.
**Remaining:** Optional arithmetic, some function context tests

### Phase 2: Control Flow & Functions (200-399) - MOSTLY COMPLETE
Comparisons, variables, control flow, functions. ~100 tests.
**Remaining:** If-let with Result, function context tests

### Phase 3: Advanced Features (400-549) - PARTIAL
Try operators, conversions, modules. ~75 tests.
**Remaining:** Most try operator tests, module tests

### Phase 4: Combinations (550-649) - NOT STARTED
Linear semantics, complex combinations. ~50 tests.

## Files to Create
- `crates/datalove-datafun/tests/fixtures/dual/*.world` - Test fixtures
- Each test needs corresponding `.out.expected` (generated via BLESS=1)

## Execution
```bash
# Run all dual tests
cargo test -p datalove-datafun --test dual_tests

# Bless new expected outputs
BLESS=1 cargo test -p datalove-datafun --test dual_tests

# Run specific test
cargo test -p datalove-datafun --test dual_tests -- 370_
```
