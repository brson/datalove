require rider std
import std.int_to_u64
import std.int_low_bits_u64

// Constants.

fun min_value(): u64
  ret (: u64 / 0)
end fun

fun max_value(): u64
  ret 18446744073709551615
end fun

fun bits(): u32
  ret (: u32 / 64)
end fun

// Bitwise primitives.

fun bitnot(self: u64): u64
  ret icall bitnot_u64(self)
end fun

fun bitand(self: u64, n: u64): u64
  ret icall bitand_u64(self, n)
end fun

fun bitor(self: u64, n: u64): u64
  ret icall bitor_u64(self, n)
end fun

fun bitxor(self: u64, n: u64): u64
  ret icall bitxor_u64(self, n)
end fun

// Bit counting.

fun count_ones(self: u64): u32
  ret icall popcount_u64(self)
end fun

fun count_zeros(self: u64): u32
  ret icall sub_wrapping_u32(bits(), count_ones(self))
end fun

fun leading_zeros(self: u64): u32
  ret icall clz_u64(self)
end fun

fun trailing_zeros(self: u64): u32
  ret icall ctz_u64(self)
end fun

fun leading_ones(self: u64): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: u64): u32
  ret trailing_zeros(bitnot(self))
end fun

fun is_power_of_two(self: u64): bool
  if self == (: u64 / 0)
    ret false
  else
    ret count_ones(self) == (: u32 / 1)
  end if
end fun

// Integer log base 2. Returns none if self is zero.
fun ilog2(self: u64): ?u32
  if self == (: u64 / 0)
    ret none
  else
    ret some icall sub_wrapping_u32(icall sub_wrapping_u32(bits(), : u32 / 1), leading_zeros(self))
  end if
end fun

// Power functions.

// Returns the smallest power of two >= self. Returns none on overflow.
fun next_power_of_two(self: u64): ?u64
  if self <= (: u64 / 1)
    ret some (: u64 / 1)
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
          ret shift_left((: u64 / 1), next_exp)
        end if
      else
        // Unreachable: ilog2 only returns none for 0.
        ret some (: u64 / 1)
      end if
    end if
  end if
end fun

// Binary exponentiation with overflow detection.
fun pow_checked(self: u64, exp: u32): ?u64
  var result: u64 = (: u64 / 1)
  var base: u64 = self
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
fun pow_saturating(self: u64, exp: u32): u64
  var result: u64 = (: u64 / 1)
  var base: u64 = self
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
fun pow_wrapping(self: u64, exp: u32): u64
  var result: u64 = (: u64 / 1)
  var base: u64 = self
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

// Byte manipulation.

fun swap_bytes(self: u64): u64
  ret icall swap_bytes_u64(self)
end fun

fun reverse_bits(self: u64): u64
  ret icall reverse_bits_u64(self)
end fun

// Endianness conversion.

fun from_be(other: u64): u64
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: u64): u64
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: u64): u64
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: u64): u64
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Type conversion.

fun cast_signed(self: u64): i64
  ret icall u64_to_i64(self)
end fun

// Conversion from int.
//
// The plain form is none out of range; the wrapping form keeps the low bits,
// as two's complement for a negative value.

fun from_int(ref n: int): ?u64
  ret int_to_u64(ref n)
end fun

fun from_int_wrapping(ref n: int): u64
  ret int_low_bits_u64(ref n)
end fun

// Checked arithmetic.

fun neg_checked(self: u64): ?u64
  if self == (: u64 / 0)
    ret some (: u64 / 0)
  else
    ret none
  end if
end fun

fun add_checked(self: u64, other: u64): ?u64
  ret some (self +? other)
end fun

fun sub_checked(self: u64, other: u64): ?u64
  ret some (self -? other)
end fun

fun mul_checked(self: u64, other: u64): ?u64
  ret some (self *? other)
end fun

fun div_checked(self: u64, other: u64): ?u64
  ret some (self /? other)
end fun

fun rem_checked(self: u64, other: u64): ?u64
  if other == (: u64 / 0)
    ret none
  else
    ret some icall rem_u64(self, other)
  end if
end fun

// Checked signed addition. Adds a signed i64 to u64.
// Returns none on overflow (positive other) or underflow (negative other).
fun add_checked_signed(self: u64, other: i64): ?u64
  if other >= (: i64 / 0)
    let other_u64 = icall i64_to_u64(other)
    ret some (self +? other_u64)
  else
    let neg_other = icall neg_wrapping_i64(other)
    let abs_other = icall i64_to_u64(neg_other)
    ret some (self -? abs_other)
  end if
end fun

// Checked signed subtraction. Subtracts a signed i64 from u64.
// Returns none on underflow (positive other) or overflow (negative other).
fun sub_checked_signed(self: u64, other: i64): ?u64
  if other >= (: i64 / 0)
    let other_u64 = icall i64_to_u64(other)
    ret some (self -? other_u64)
  else
    let neg_other = icall neg_wrapping_i64(other)
    let abs_other = icall i64_to_u64(neg_other)
    ret some (self +? abs_other)
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: u64, other: u64): u64
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

fun sub_saturating(self: u64, other: u64): u64
  if sub_checked(self, other) |value|
    ret value
  else
    ret (: u64 / 0)
  end if
end fun

fun mul_saturating(self: u64, other: u64): u64
  if mul_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// For u64, division cannot overflow (result <= dividend), so this is same as div_checked.
fun div_saturating(self: u64, other: u64): ?u64
  ret div_checked(self, other)
end fun

fun add_saturating_signed(self: u64, other: i64): u64
  if other >= (: i64 / 0)
    let other_u64 = icall i64_to_u64(other)
    ret add_saturating(self, other_u64)
  else
    let neg_other = icall neg_wrapping_i64(other)
    let abs_other = icall i64_to_u64(neg_other)
    ret sub_saturating(self, abs_other)
  end if
end fun

fun sub_saturating_signed(self: u64, other: i64): u64
  if other >= (: i64 / 0)
    let other_u64 = icall i64_to_u64(other)
    ret sub_saturating(self, other_u64)
  else
    let neg_other = icall neg_wrapping_i64(other)
    let abs_other = icall i64_to_u64(neg_other)
    ret add_saturating(self, abs_other)
  end if
end fun

// Wrapping arithmetic.

fun add_wrapping(self: u64, other: u64): u64
  ret icall add_wrapping_u64(self, other)
end fun

fun sub_wrapping(self: u64, other: u64): u64
  ret icall sub_wrapping_u64(self, other)
end fun

fun mul_wrapping(self: u64, other: u64): u64
  ret icall mul_wrapping_u64(self, other)
end fun

// u64 division cannot overflow.
fun div_wrapping(self: u64, other: u64): ?u64
  ret div_checked(self, other)
end fun

fun add_wrapping_signed(self: u64, other: i64): u64
  if other >= (: i64 / 0)
    let other_u64 = icall i64_to_u64(other)
    ret add_wrapping(self, other_u64)
  else
    let neg_other = icall neg_wrapping_i64(other)
    let abs_other = icall i64_to_u64(neg_other)
    ret sub_wrapping(self, abs_other)
  end if
end fun

fun sub_wrapping_signed(self: u64, other: i64): u64
  if other >= (: i64 / 0)
    let other_u64 = icall i64_to_u64(other)
    ret sub_wrapping(self, other_u64)
  else
    let neg_other = icall neg_wrapping_i64(other)
    let abs_other = icall i64_to_u64(neg_other)
    ret add_wrapping(self, abs_other)
  end if
end fun

// Shifts.

// Returns none if shift >= 64.
fun shift_left(self: u64, n: u32): ?u64
  if n >= bits()
    ret none
  else
    ret some icall shl_u64(self, n)
  end if
end fun

// Returns none if shift >= 64.
fun shift_right(self: u64, n: u32): ?u64
  if n >= bits()
    ret none
  else
    ret some icall shr_u64(self, n)
  end if
end fun

fun shift_left_saturating(self: u64, n: u32): u64
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

fun shift_right_saturating(self: u64, n: u32): u64
  if n >= bits()
    ret (: u64 / 0)
  else
    if shift_right(self, n) |value|
      ret value
    else
      ret (: u64 / 0)
    end if
  end if
end fun

fun shift_left_wrapping(self: u64, n: u32): u64
  let n_mod = icall bitand_u32(n, : u32 / 63)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: u64 / 0)
  end if
end fun

fun shift_right_wrapping(self: u64, n: u32): u64
  let n_mod = icall bitand_u32(n, : u32 / 63)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: u64 / 0)
  end if
end fun

// Rotates.

fun rotate_left(self: u64, n: u32): u64
  let n_mod = icall bitand_u32(n, : u32 / 63)
  let left_part = shift_left_wrapping(self, n_mod)
  let right_amount = icall sub_wrapping_u32(: u32 / 64, n_mod)
  let right_part = shift_right_wrapping(self, right_amount)
  ret bitor(left_part, right_part)
end fun

fun rotate_right(self: u64, n: u32): u64
  let n_mod = icall bitand_u32(n, : u32 / 63)
  let right_part = shift_right_wrapping(self, n_mod)
  let left_amount = icall sub_wrapping_u32(: u32 / 64, n_mod)
  let left_part = shift_left_wrapping(self, left_amount)
  ret bitor(left_part, right_part)
end fun

// Comparisons and utilities.

fun is_zero(self: u64): bool
  ret self == (: u64 / 0)
end fun

fun min(self: u64, other: u64): u64
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: u64, other: u64): u64
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: u64, min_val: u64, max_val: u64): u64
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

fun abs_diff(self: u64, other: u64): u64
  if self >= other
    ret sub_saturating(self, other)
  else
    ret sub_saturating(other, self)
  end if
end fun

// Average of two values, rounded down, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
fun midpoint(self: u64, other: u64): u64
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u32 / 1)
  ret add_wrapping(common, half_diff)
end fun
