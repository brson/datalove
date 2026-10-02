require rider std
import std.int_to_u64
import std.int_low_bits_u64

// Constants.

fun min_value(): u8
  ret (: u8 / 0)
end fun

fun max_value(): u8
  ret (: u8 / 255)
end fun

fun bits(): u32
  ret (: u32 / 8)
end fun

// Bitwise primitives.

fun bitnot(self: u8): u8
  ret icall bitnot_u8(self)
end fun

fun bitand(self: u8, n: u8): u8
  ret icall bitand_u8(self, n)
end fun

fun bitor(self: u8, n: u8): u8
  ret icall bitor_u8(self, n)
end fun

fun bitxor(self: u8, n: u8): u8
  ret icall bitxor_u8(self, n)
end fun

// Bit counting.

fun count_ones(self: u8): u32
  ret icall popcount_u8(self)
end fun

fun count_zeros(self: u8): u32
  ret icall sub_wrapping_u32(bits(), count_ones(self))
end fun

fun leading_zeros(self: u8): u32
  ret icall clz_u8(self)
end fun

fun trailing_zeros(self: u8): u32
  ret icall ctz_u8(self)
end fun

fun leading_ones(self: u8): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: u8): u32
  ret trailing_zeros(bitnot(self))
end fun

fun is_power_of_two(self: u8): bool
  if self == (: u8 / 0)
    ret false
  else
    ret count_ones(self) == (: u32 / 1)
  end if
end fun

// Integer log base 2. Returns none if self is zero.
fun ilog2(self: u8): ?u32
  if self == (: u8 / 0)
    ret none
  else
    ret some icall sub_wrapping_u32(icall sub_wrapping_u32(bits(), : u32 / 1), leading_zeros(self))
  end if
end fun

// Power functions.

// Returns the smallest power of two >= self. Returns none on overflow.
fun next_power_of_two(self: u8): ?u8
  if self <= (: u8 / 1)
    ret some (: u8 / 1)
  else
    if is_power_of_two(self)
      ret some self
    else
      // self > 1 and not a power of two, so we need 2^(ilog2(self) + 1).
      if ilog2(self) |log|
        let next_exp = icall add_wrapping_u32(log, : u32 / 1)
        if next_exp >= bits()
          ret none
        else
          ret shift_left((: u8 / 1), next_exp)
        end if
      else
        // Unreachable: ilog2 only returns none for 0.
        ret some (: u8 / 1)
      end if
    end if
  end if
end fun

// Binary exponentiation with overflow detection.
fun pow_checked(self: u8, exp: u32): ?u8
  var result: u8 = (: u8 / 1)
  var base: u8 = self
  var e: u32 = exp
  loop while e .> (: u32 / 0)
    if icall bitand_u32(e, : u32 / 1) == (: u32 / 1)
      if mul_checked(result, base) |next_result|
        set result = next_result
      else
        ret none
      end if
    end if
    set e = icall shr_u32(e, : u32 / 1)
    if e .> (: u32 / 0)
      if mul_checked(base, base) |next_base|
        set base = next_base
      else
        ret none
      end if
    end if
  end loop
  ret some result
end fun

// Binary exponentiation saturating at max_value on overflow.
fun pow_saturating(self: u8, exp: u32): u8
  var result: u8 = (: u8 / 1)
  var base: u8 = self
  var e: u32 = exp
  var overflow: bool = false
  loop while e .> (: u32 / 0)
    if icall bitand_u32(e, : u32 / 1) == (: u32 / 1)
      if mul_checked(result, base) |next_result|
        set result = next_result
      else
        set overflow = true
      end if
    end if
    set e = icall shr_u32(e, : u32 / 1)
    if e .> (: u32 / 0)
      if mul_checked(base, base) |next_base|
        set base = next_base
      else
        set overflow = true
      end if
    end if
  end loop
  if overflow
    ret max_value()
  else
    ret result
  end if
end fun

// Binary exponentiation with wrapping on overflow.
fun pow_wrapping(self: u8, exp: u32): u8
  var result: u8 = (: u8 / 1)
  var base: u8 = self
  var e: u32 = exp
  loop while e .> (: u32 / 0)
    if icall bitand_u32(e, : u32 / 1) == (: u32 / 1)
      set result = mul_wrapping(result, base)
    end if
    set e = icall shr_u32(e, : u32 / 1)
    if e .> (: u32 / 0)
      set base = mul_wrapping(base, base)
    end if
  end loop
  ret result
end fun

// Bit manipulation.

fun reverse_bits(self: u8): u8
  ret icall reverse_bits_u8(self)
end fun

// Type conversion.

fun cast_signed(self: u8): i8
  ret icall u8_to_i8(self)
end fun

// Conversion from the wider unsigned integers.
//
// `@` widens but never narrows, so these are the way down. The plain form is
// none when the value is out of range; the wrapping form keeps the low bits.

fun from_u16(x: u16): ?u8
  if x <= max_value()@
    ret some icall u16_to_u8(x)
  else
    ret none
  end if
end fun

fun from_u16_wrapping(x: u16): u8
  ret icall u16_to_u8(x)
end fun

fun from_u32(x: u32): ?u8
  if x <= max_value()@
    ret some icall u32_to_u8(x)
  else
    ret none
  end if
end fun

fun from_u32_wrapping(x: u32): u8
  ret icall u32_to_u8(x)
end fun

fun from_u64(x: u64): ?u8
  if x <= max_value()@
    ret some icall u64_to_u8(x)
  else
    ret none
  end if
end fun

fun from_u64_wrapping(x: u64): u8
  ret icall u64_to_u8(x)
end fun

// Conversion from int.
//
// The plain form is none out of range; the wrapping form keeps the low bits,
// as two's complement for a negative value.

fun from_int(ref n: int): ?u8
  if int_to_u64(ref n) |x|
    ret from_u64(x)
  else
    ret none
  end if
end fun

fun from_int_wrapping(ref n: int): u8
  ret from_u64_wrapping(int_low_bits_u64(ref n))
end fun

// Checked arithmetic.

fun neg_checked(self: u8): ?u8
  if self == (: u8 / 0)
    ret some (: u8 / 0)
  else
    ret none
  end if
end fun

fun add_checked(self: u8, other: u8): ?u8
  ret some (self +? other)
end fun

fun sub_checked(self: u8, other: u8): ?u8
  ret some (self -? other)
end fun

fun mul_checked(self: u8, other: u8): ?u8
  ret some (self *? other)
end fun

fun div_checked(self: u8, other: u8): ?u8
  ret some (self /? other)
end fun

fun rem_checked(self: u8, other: u8): ?u8
  if other == (: u8 / 0)
    ret none
  else
    ret some icall rem_u8(self, other)
  end if
end fun

// Checked signed addition. Adds a signed i8 to u8.
// Returns none on overflow (positive other) or underflow (negative other).
fun add_checked_signed(self: u8, other: i8): ?u8
  if other >= (: i8 / 0)
    let other_u8 = icall i8_to_u8(other)
    ret some (self +? other_u8)
  else
    let neg_other = icall neg_wrapping_i8(other)
    let abs_other = icall i8_to_u8(neg_other)
    ret some (self -? abs_other)
  end if
end fun

// Checked signed subtraction. Subtracts a signed i8 from u8.
// Returns none on underflow (positive other) or overflow (negative other).
fun sub_checked_signed(self: u8, other: i8): ?u8
  if other >= (: i8 / 0)
    let other_u8 = icall i8_to_u8(other)
    ret some (self -? other_u8)
  else
    let neg_other = icall neg_wrapping_i8(other)
    let abs_other = icall i8_to_u8(neg_other)
    ret some (self +? abs_other)
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: u8, other: u8): u8
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

fun sub_saturating(self: u8, other: u8): u8
  if sub_checked(self, other) |value|
    ret value
  else
    ret (: u8 / 0)
  end if
end fun

fun mul_saturating(self: u8, other: u8): u8
  if mul_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// For u8, division cannot overflow (result <= dividend), so this is same as div_checked.
fun div_saturating(self: u8, other: u8): ?u8
  ret div_checked(self, other)
end fun

fun add_saturating_signed(self: u8, other: i8): u8
  if other >= (: i8 / 0)
    let other_u8 = icall i8_to_u8(other)
    ret add_saturating(self, other_u8)
  else
    let neg_other = icall neg_wrapping_i8(other)
    let abs_other = icall i8_to_u8(neg_other)
    ret sub_saturating(self, abs_other)
  end if
end fun

fun sub_saturating_signed(self: u8, other: i8): u8
  if other >= (: i8 / 0)
    let other_u8 = icall i8_to_u8(other)
    ret sub_saturating(self, other_u8)
  else
    let neg_other = icall neg_wrapping_i8(other)
    let abs_other = icall i8_to_u8(neg_other)
    ret add_saturating(self, abs_other)
  end if
end fun

// Wrapping arithmetic.

fun add_wrapping(self: u8, other: u8): u8
  ret icall add_wrapping_u8(self, other)
end fun

fun sub_wrapping(self: u8, other: u8): u8
  ret icall sub_wrapping_u8(self, other)
end fun

fun mul_wrapping(self: u8, other: u8): u8
  ret icall mul_wrapping_u8(self, other)
end fun

// u8 division cannot overflow.
fun div_wrapping(self: u8, other: u8): ?u8
  ret div_checked(self, other)
end fun

fun add_wrapping_signed(self: u8, other: i8): u8
  if other >= (: i8 / 0)
    let other_u8 = icall i8_to_u8(other)
    ret add_wrapping(self, other_u8)
  else
    let neg_other = icall neg_wrapping_i8(other)
    let abs_other = icall i8_to_u8(neg_other)
    ret sub_wrapping(self, abs_other)
  end if
end fun

fun sub_wrapping_signed(self: u8, other: i8): u8
  if other >= (: i8 / 0)
    let other_u8 = icall i8_to_u8(other)
    ret sub_wrapping(self, other_u8)
  else
    let neg_other = icall neg_wrapping_i8(other)
    let abs_other = icall i8_to_u8(neg_other)
    ret add_wrapping(self, abs_other)
  end if
end fun

// Shifts.

// Returns none if shift >= 8.
fun shift_left(self: u8, n: u32): ?u8
  if n >= bits()
    ret none
  else
    ret some icall shl_u8(self, n)
  end if
end fun

// Returns none if shift >= 8.
fun shift_right(self: u8, n: u32): ?u8
  if n >= bits()
    ret none
  else
    ret some icall shr_u8(self, n)
  end if
end fun

fun shift_left_saturating(self: u8, n: u32): u8
  if n >= bits()
    ret max_value()
  else
    if shift_left(self, n) |value|
      ret value
    else
      ret max_value()
    end if
  end if
end fun

fun shift_right_saturating(self: u8, n: u32): u8
  if n >= bits()
    ret (: u8 / 0)
  else
    if shift_right(self, n) |value|
      ret value
    else
      ret (: u8 / 0)
    end if
  end if
end fun

fun shift_left_wrapping(self: u8, n: u32): u8
  let n_mod = icall bitand_u32(n, : u32 / 7)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: u8 / 0)
  end if
end fun

fun shift_right_wrapping(self: u8, n: u32): u8
  let n_mod = icall bitand_u32(n, : u32 / 7)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: u8 / 0)
  end if
end fun

// Rotates.

fun rotate_left(self: u8, n: u32): u8
  let n_mod = icall bitand_u32(n, : u32 / 7)
  let left_part = shift_left_wrapping(self, n_mod)
  let right_amount = icall sub_wrapping_u32(: u32 / 8, n_mod)
  let right_part = shift_right_wrapping(self, right_amount)
  ret bitor(left_part, right_part)
end fun

fun rotate_right(self: u8, n: u32): u8
  let n_mod = icall bitand_u32(n, : u32 / 7)
  let right_part = shift_right_wrapping(self, n_mod)
  let left_amount = icall sub_wrapping_u32(: u32 / 8, n_mod)
  let left_part = shift_left_wrapping(self, left_amount)
  ret bitor(left_part, right_part)
end fun

// Comparisons and utilities.

fun is_zero(self: u8): bool
  ret self == (: u8 / 0)
end fun

fun min(self: u8, other: u8): u8
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: u8, other: u8): u8
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: u8, min_val: u8, max_val: u8): u8
  if self .< min_val
    ret min_val
  else
    if self .> max_val
      ret max_val
    else
      ret self
    end if
  end if
end fun

fun abs_diff(self: u8, other: u8): u8
  if self >= other
    ret sub_saturating(self, other)
  else
    ret sub_saturating(other, self)
  end if
end fun

// Average of two values, rounded down, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
fun midpoint(self: u8, other: u8): u8
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u32 / 1)
  ret add_wrapping(common, half_diff)
end fun
