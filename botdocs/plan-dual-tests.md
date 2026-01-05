# Plan: Comprehensive dual_tests Test Suite

## Overview
Design a complete test suite for `dual_tests` that validates interpreter/AOT parity across all language features. Tests organized by feature category, with each feature tested in three contexts where applicable.

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
001_debuglog_i32.world          # Already exists as 001_simple_debuglog
002_debuglog_bool.world
003_debuglog_string.world
004_debuglog_multiple.world
```

### 010-049: Literals & Primitives
Test all primitive types and literal forms.

**Scalars (Copy types):**
```
010_literal_bool_true.world
011_literal_bool_false.world
012_literal_u8.world
013_literal_u16.world
014_literal_u32.world
015_literal_u64.world
016_literal_i8.world
017_literal_i16.world
018_literal_i32.world
019_literal_i64.world
020_literal_f32.world
```

**Hex literals:**
```
021_literal_hex_u32.world
022_literal_hex_u64.world
023_literal_hex_i32.world
024_literal_hex_int.world
025_literal_hex_f32_bits.world
```

**Linear types:**
```
030_literal_int.world           # Bigint
031_literal_string.world
032_literal_string_escape.world
```

**Special types:**
```
040_literal_unit.world
041_literal_option_some.world
042_literal_option_none.world
043_literal_result_ok.world
044_literal_data.world
045_literal_error.world
```

### 050-099: Collections
Test list, set, map with various element types.

**Lists:**
```
050_list_empty_u32.world
051_list_u32_elements.world
052_list_string_elements.world
053_list_nested.world
054_list_in_sfn.world
055_list_in_mfn.world
```

**Sets:**
```
060_set_empty_u32.world
061_set_u32_elements.world
062_set_string_elements.world
063_set_in_sfn.world
064_set_in_mfn.world
```

**Maps:**
```
070_map_empty.world
071_map_u32_u32.world
072_map_string_string.world
073_map_string_u32.world
074_map_in_sfn.world
075_map_in_mfn.world
```

**Tensors:**
```
080_tensor_1d_f32.world
081_tensor_2d_f32.world
082_tensor_in_sfn.world
```

### 100-149: Aggregates
Test tuples, structs, enums.

**Tuples:**
```
100_tuple_empty.world
101_tuple_pair_u32.world
102_tuple_mixed_types.world
103_tuple_nested.world
104_tuple_with_string.world
105_tuple_in_sfn.world
106_tuple_in_mfn.world
```

**Structs:**
```
110_struct_single_field.world
111_struct_two_fields.world
112_struct_with_string.world
113_struct_nested.world
114_struct_in_sfn.world
115_struct_in_mfn.world
```

**Enums:**
```
120_enum_no_payload.world
121_enum_u32_payload.world
122_enum_string_payload.world
123_enum_mixed_payloads.world
124_enum_in_sfn.world
125_enum_in_mfn.world
```

### 150-199: Arithmetic Operators

**Bare arithmetic (widening to int):**
```
150_arith_add_u32.world
151_arith_sub_u32.world
152_arith_mul_u32.world
153_arith_add_i32.world
154_arith_sub_i32.world
155_arith_mul_i32.world
156_arith_neg_i32.world
157_arith_add_f32.world
158_arith_mixed_widening.world
```

**Checked arithmetic (+!, -!, *!, /!) returning Result:**
```
160_arith_add_checked_success.world
161_arith_add_checked_overflow.world
162_arith_sub_checked_success.world
163_arith_sub_checked_underflow.world
164_arith_mul_checked_success.world
165_arith_mul_checked_overflow.world
166_arith_div_checked_success.world
167_arith_div_checked_divzero.world
168_arith_neg_checked_success.world
169_arith_neg_checked_overflow.world
```

**Optional arithmetic (+?, -?, *?, /?) returning Option:**
```
170_arith_add_optional_success.world
171_arith_add_optional_overflow.world
172_arith_sub_optional_success.world
173_arith_sub_optional_underflow.world
174_arith_mul_optional_success.world
175_arith_mul_optional_overflow.world
176_arith_div_optional_success.world
177_arith_div_optional_divzero.world
178_arith_neg_optional_success.world
179_arith_neg_optional_overflow.world
```

**Arithmetic in functions:**
```
180_arith_checked_in_sfn.world
181_arith_checked_in_mfn.world
182_arith_optional_in_sfn.world
183_arith_optional_in_mfn.world
```

### 200-249: Comparison Operators
```
200_cmp_eq_u32.world
201_cmp_ne_u32.world
202_cmp_lt_u32.world
203_cmp_le_u32.world
204_cmp_gt_u32.world
205_cmp_ge_u32.world
206_cmp_eq_i32.world
207_cmp_lt_i32_negative.world
208_cmp_eq_bool.world
209_cmp_eq_string.world
210_cmp_in_sfn.world
211_cmp_in_mfn.world
```

### 250-299: Variables & Bindings

**Let bindings:**
```
250_let_u32.world
251_let_string.world
252_let_with_type.world
253_let_in_sfn.world
254_let_in_mfn.world
```

**Var bindings and mutation:**
```
260_var_u32.world
261_var_set_u32.world
262_var_string.world
263_var_in_sfn.world
264_var_in_mfn.world
```

**Shadowing:**
```
270_shadow_let_let.world
271_shadow_var_var.world
272_shadow_let_var.world
273_shadow_var_let.world
274_shadow_let_arg.world
275_shadow_var_arg.world
276_shadow_crossunit.world
```

### 300-349: Control Flow

**If/else:**
```
300_if_bool_simple.world
301_if_else_branch.world
302_if_nested.world
303_if_in_sfn.world
304_if_in_mfn.world
```

**If-let with Option:**
```
310_if_option_some.world
311_if_option_none.world
312_if_option_binding.world
313_if_option_nested.world
```

**If-let with Result:**
```
320_if_result_ok.world
321_if_result_error.world
322_if_result_binding.world
```

**Loops:**
```
330_loop_break.world
331_loop_continue.world
332_loop_counter.world
333_loop_nested.world
334_loop_in_sfn.world
335_loop_in_mfn.world
```

### 350-399: Functions

**Basic functions:**
```
350_fn_no_params.world
351_fn_one_param.world
352_fn_multi_params.world
353_fn_return_value.world
354_fn_void.world
355_fn_early_return.world
```

**Parameter modes:**
```
360_param_in_copy.world
361_param_in_linear.world
362_param_ref_read.world
363_param_ref_passthrough.world
364_param_mut_write.world
365_param_mut_read.world
366_param_mut_passthrough.world
367_param_out_basic.world
368_param_out_string.world
```

**Recursion:**
```
370_recursion_simple.world
371_recursion_factorial.world
372_recursion_fibonacci.world
373_recursion_mutual.world
```

### 400-449: Try Operators

**Try with Option (?):**
```
400_try_option_some_sfn.world
401_try_option_none_sfn.world
```

**Try with Result (!):**
```
410_try_result_ok_script.world
411_try_result_error_script.world
412_try_result_ok_sfn.world
413_try_result_error_sfn.world
414_try_result_ok_mfn.world
415_try_result_error_mfn.world
```

### 450-499: Type Conversions

**Error wrapping:**
```
450_error_from_u32.world
451_error_from_string.world
452_error_from_bool.world
453_error_from_int.world
454_error_from_tuple.world
455_error_in_sfn.world
456_error_in_mfn.world
```

**Data wrapping:**
```
460_data_from_u32.world
461_data_from_string.world
462_data_from_bool.world
463_data_from_int.world
464_data_from_tuple.world
465_data_in_sfn.world
466_data_in_mfn.world
```

### 500-549: Modules & Imports

**Basic modules:**
```
500_module_function_call.world
501_module_two_functions.world
502_module_chain_calls.world
```

**Cross-module:**
```
510_import_function.world
511_import_chain.world
512_import_multiple_modules.world
```

**Cross-unit (script fragments):**
```
520_crossunit_value.world
521_crossunit_slot.world
522_crossunit_function.world
523_crossunit_var_mutation.world
```

### 550-599: Linear Type Semantics

**Move semantics:**
```
550_move_string_once.world
551_move_list_once.world
552_move_conditional.world
553_move_in_loop.world
```

**Drop behavior:**
```
560_drop_string_scope.world
561_drop_list_scope.world
562_drop_in_function.world
```

### 600-649: Combinations

**Collections with linear elements:**
```
600_list_of_strings.world
601_set_of_strings.world
602_map_string_keys.world
603_map_string_values.world
604_nested_list_strings.world
```

**Aggregates with linear fields:**
```
610_tuple_with_list.world
611_struct_with_string.world
612_struct_with_list.world
613_enum_with_string_payload.world
```

**Complex expressions:**
```
620_chain_function_calls.world
621_nested_conditionals.world
622_loop_with_collections.world
```

## Implementation Strategy

### Phase 1: Core Features (001-199)
Debuglog, literals, collections, aggregates, arithmetic. ~100 tests.

### Phase 2: Control Flow & Functions (200-399)
Comparisons, variables, control flow, functions. ~100 tests.

### Phase 3: Advanced Features (400-549)
Try operators, conversions, modules. ~75 tests.

### Phase 4: Combinations (550-649)
Linear semantics, complex combinations. ~50 tests.

## Files to Create
- `crates/datalove-datafun/tests/fixtures/dual/*.world` - Test fixtures
- Each test needs corresponding `.out.expected` (generated via BLESS=1)

## Execution
```bash
# Run all dual tests
cargo test --test dual_tests

# Bless new expected outputs
BLESS=1 cargo test --test dual_tests
```
