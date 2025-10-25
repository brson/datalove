# Plan: Convert int_math from ibig to Custom Implementation

## Status: IN PROGRESS

## Progress
- ✅ Phase 1: Negation (Trivial)

## Current State
- `int_math.rs` uses `ibig` internally for all bigint operations
- `rtdt::Int` stores bigints as u32 limbs (similar to GMP format)
- 6 operations: int_to_string, add, sub, mul, neg, div_checked
- Testing: 6 integration tests (200-205) via datafun interpreter
- All 142 tests currently passing

## Conversion Strategy
Implement operations incrementally from simplest to most complex, testing after each phase.

## Phase 1: Negation (Trivial)
- Implement `int_neg_impl` without ibig (flip sign bit in size_and_sign)
- Test: 203_int_neg

## Phase 2: String Conversion (Moderate)
- Implement `int_to_string_impl` with custom limb-to-decimal conversion
- Algorithm: Repeated division by 10^9, convert chunks to decimal
- Test: All tests (string conversion used in output)

## Phase 3: Addition (Moderate)
- Implement limb-level addition with carry propagation
- Handle sign cases: pos+pos, neg+neg, pos+neg, neg+pos
- Use magnitude comparison for mixed-sign cases
- Test: 200_int_add

## Phase 4: Subtraction (Moderate)
- Implement limb-level subtraction with borrow propagation
- Can leverage addition: `a - b = a + (-b)`
- Test: 201_int_sub

## Phase 5: Multiplication (Complex)
- Implement grade-school multiplication on limbs
- Handle result allocation (up to m+n limbs)
- Propagate carries through result
- Test: 202_int_mul

## Phase 6: Division (Most Complex)
- Implement long division on limbs (Knuth Algorithm D)
- Calculate quotient and remainder
- Division by zero already checked at interpreter level
- Test: 204_int_div_success, 205_int_div_by_zero

## Phase 7: Cleanup
- Remove ibig dependency from `crates/datalove-rt/Cargo.toml`
- Remove conversion functions: `rtdt_int_to_ibig`, `ibig_to_rtdt_int`
- Final test: Run full suite (all 142 tests)

## Testing Approach
- After each phase, run specific test(s) for that operation
- Keep phases independent so partial progress is usable
- Final verification with complete test suite

## Implementation Notes

### rtdt::Int Layout
```rust
pub struct Int {
    pub data: *const u32,        // Pointer to limbs array
    pub size_and_sign: i32,      // abs(size_and_sign) = limb count, sign = number sign
    pub capacity: u32,           // Allocated limbs
}
```

### Key Algorithms Needed
1. **Limb addition with carry**: Standard schoolbook addition
2. **Limb subtraction with borrow**: Standard schoolbook subtraction
3. **Magnitude comparison**: For mixed-sign operations
4. **Decimal conversion**: Divide by powers of 10
5. **Multiplication**: Grade-school or Karatsuba for larger numbers
6. **Division**: Knuth's Algorithm D (The Art of Computer Programming, Vol 2)

### Dependencies to Remove
- `ibig = "0.3"` from `crates/datalove-rt/Cargo.toml`
