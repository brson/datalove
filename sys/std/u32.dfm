// Constants.

fun min_value(): u32
  ret : u32 / 0
end fun

fun max_value(): u32
  ret : u32 / 4294967295
end fun

fun bits(): u32
  ret : u32 / 32
end fun

// Bitwise primitives.

fun bitnot(self: u32): u32
  ret icall bitnot_u32(self)
end fun

fun bitand(self: u32, n: u32): u32
  ret icall bitand_u32(self, n)
end fun

fun bitor(self: u32, n: u32): u32
  ret icall bitor_u32(self, n)
end fun

fun bitxor(self: u32, n: u32): u32
  ret icall bitxor_u32(self, n)
end fun

// Bit counting.

fun count_ones(self: u32): u32
  ret icall popcount_u32(self)
end fun

fun count_zeros(self: u32): u32
  ret sub_saturating(bits(), count_ones(self))
end fun

fun leading_zeros(self: u32): u32
  ret icall clz_u32(self)
end fun

fun trailing_zeros(self: u32): u32
  ret icall ctz_u32(self)
end fun

fun leading_ones(self: u32): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: u32): u32
  ret trailing_zeros(bitnot(self))
end fun

fun is_power_of_two(self: u32): bool
  if self == (: u32 / 0)
    ret false
  else
    ret count_ones(self) == (: u32 / 1)
  end if
end fun

// Integer log base 2. Returns none if self is zero.
fun ilog2(self: u32): ?u32
  if self == (: u32 / 0)
    ret none
  else
    ret some sub_saturating(: u32 / 31, leading_zeros(self))
  end if
end fun

// Power functions.

// Returns the smallest power of two >= self. Returns none on overflow.
fun next_power_of_two(self: u32): ?u32
  if self <= (: u32 / 1)
    ret some (: u32 / 1)
  else
    if is_power_of_two(self)
      ret some self
    else
      // self > 1 and not a power of two, so we need 2^(ilog2(self) + 1).
      if ilog2(self) |log|
        let next_exp = add_saturating(log, : u32 / 1)
        if next_exp >= (: u32 / 32)
          ret none
        else
          ret shift_left(: u32 / 1, next_exp)
        end if
      else
        // Unreachable: ilog2 only returns none for 0.
        ret some (: u32 / 1)
      end if
    end if
  end if
end fun

// Binary exponentiation with overflow detection.
fun pow_checked(self: u32, exp: u32): ?u32
  var result: u32 = : u32 / 1
  var base: u32 = self
  var e: u32 = exp
  loop while e .> (: u32 / 0)
    if bitand(e, : u32 / 1) == (: u32 / 1)
      if mul_checked(result, base) |next_result|
        set result = next_result
      else
        ret none
      end if
    end if
    set e = shift_right_wrapping(e, : u32 / 1)
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
fun pow_saturating(self: u32, exp: u32): u32
  var result: u32 = : u32 / 1
  var base: u32 = self
  var e: u32 = exp
  var overflow: bool = false
  loop while e .> (: u32 / 0)
    if bitand(e, : u32 / 1) == (: u32 / 1)
      if mul_checked(result, base) |next_result|
        set result = next_result
      else
        set overflow = true
      end if
    end if
    set e = shift_right_wrapping(e, : u32 / 1)
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
fun pow_wrapping(self: u32, exp: u32): u32
  var result: u32 = : u32 / 1
  var base: u32 = self
  var e: u32 = exp
  loop while e .> (: u32 / 0)
    if bitand(e, : u32 / 1) == (: u32 / 1)
      set result = mul_wrapping(result, base)
    end if
    set e = shift_right_wrapping(e, : u32 / 1)
    if e .> (: u32 / 0)
      set base = mul_wrapping(base, base)
    end if
  end loop
  ret result
end fun

// Byte manipulation.

fun swap_bytes(self: u32): u32
  ret icall swap_bytes_u32(self)
end fun

fun reverse_bits(self: u32): u32
  ret icall reverse_bits_u32(self)
end fun

// Endianness conversion.

fun from_be(other: u32): u32
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: u32): u32
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: u32): u32
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: u32): u32
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Type conversion.

fun cast_signed(self: u32): i32
  ret icall u32_to_i32(self)
end fun

// Checked arithmetic.

fun neg_checked(self: u32): ?u32
  if self == (: u32 / 0)
    ret some (: u32 / 0)
  else
    ret none
  end if
end fun

fun add_checked(self: u32, other: u32): ?u32
  ret some (self +? other)
end fun

fun sub_checked(self: u32, other: u32): ?u32
  ret some (self -? other)
end fun

fun mul_checked(self: u32, other: u32): ?u32
  ret some (self *? other)
end fun

fun div_checked(self: u32, other: u32): ?u32
  ret some (self /? other)
end fun

fun rem_checked(self: u32, other: u32): ?u32
  if other == (: u32 / 0)
    ret none
  else
    ret some icall rem_u32(self, other)
  end if
end fun

// Checked signed addition. Adds a signed i32 to u32.
// Returns none on overflow (positive other) or underflow (negative other).
fun add_checked_signed(self: u32, other: i32): ?u32
  if other >= (: i32 / 0)
    let other_u32 = icall i32_to_u32(other)
    ret some (self +? other_u32)
  else
    let neg_other = icall neg_wrapping_i32(other)
    let abs_other = icall i32_to_u32(neg_other)
    ret some (self -? abs_other)
  end if
end fun

// Checked signed subtraction. Subtracts a signed i32 from u32.
// Returns none on underflow (positive other) or overflow (negative other).
fun sub_checked_signed(self: u32, other: i32): ?u32
  if other >= (: i32 / 0)
    let other_u32 = icall i32_to_u32(other)
    ret some (self -? other_u32)
  else
    let neg_other = icall neg_wrapping_i32(other)
    let abs_other = icall i32_to_u32(neg_other)
    ret some (self +? abs_other)
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: u32, other: u32): u32
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

fun sub_saturating(self: u32, other: u32): u32
  if sub_checked(self, other) |value|
    ret value
  else
    ret : u32 / 0
  end if
end fun

fun mul_saturating(self: u32, other: u32): u32
  if mul_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// For u32, division cannot overflow (result <= dividend), so this is same as div_checked.
fun div_saturating(self: u32, other: u32): ?u32
  ret div_checked(self, other)
end fun

fun add_saturating_signed(self: u32, other: i32): u32
  if other >= (: i32 / 0)
    let other_u32 = icall i32_to_u32(other)
    ret add_saturating(self, other_u32)
  else
    let neg_other = icall neg_wrapping_i32(other)
    let abs_other = icall i32_to_u32(neg_other)
    ret sub_saturating(self, abs_other)
  end if
end fun

fun sub_saturating_signed(self: u32, other: i32): u32
  if other >= (: i32 / 0)
    let other_u32 = icall i32_to_u32(other)
    ret sub_saturating(self, other_u32)
  else
    let neg_other = icall neg_wrapping_i32(other)
    let abs_other = icall i32_to_u32(neg_other)
    ret add_saturating(self, abs_other)
  end if
end fun

// Wrapping arithmetic.

fun add_wrapping(self: u32, other: u32): u32
  ret icall add_wrapping_u32(self, other)
end fun

fun sub_wrapping(self: u32, other: u32): u32
  ret icall sub_wrapping_u32(self, other)
end fun

fun mul_wrapping(self: u32, other: u32): u32
  ret icall mul_wrapping_u32(self, other)
end fun

// u32 division cannot overflow.
fun div_wrapping(self: u32, other: u32): ?u32
  ret div_checked(self, other)
end fun

fun add_wrapping_signed(self: u32, other: i32): u32
  if other >= (: i32 / 0)
    let other_u32 = icall i32_to_u32(other)
    ret add_wrapping(self, other_u32)
  else
    let neg_other = icall neg_wrapping_i32(other)
    let abs_other = icall i32_to_u32(neg_other)
    ret sub_wrapping(self, abs_other)
  end if
end fun

fun sub_wrapping_signed(self: u32, other: i32): u32
  if other >= (: i32 / 0)
    let other_u32 = icall i32_to_u32(other)
    ret sub_wrapping(self, other_u32)
  else
    let neg_other = icall neg_wrapping_i32(other)
    let abs_other = icall i32_to_u32(neg_other)
    ret add_wrapping(self, abs_other)
  end if
end fun

// Shifts.

// Returns none if shift >= 32.
fun shift_left(self: u32, n: u32): ?u32
  if n >= (: u32 / 32)
    ret none
  else
    ret some icall shl_u32(self, n)
  end if
end fun

// Returns none if shift >= 32.
fun shift_right(self: u32, n: u32): ?u32
  if n >= (: u32 / 32)
    ret none
  else
    ret some icall shr_u32(self, n)
  end if
end fun

fun shift_left_saturating(self: u32, n: u32): u32
  if n >= (: u32 / 32)
    ret max_value()
  else
    if shift_left(self, n) |value|
      ret value
    else
      ret max_value()
    end if
  end if
end fun

fun shift_right_saturating(self: u32, n: u32): u32
  if n >= (: u32 / 32)
    ret : u32 / 0
  else
    if shift_right(self, n) |value|
      ret value
    else
      ret : u32 / 0
    end if
  end if
end fun

fun shift_left_wrapping(self: u32, n: u32): u32
  let n_mod = bitand(n, : u32 / 31)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret : u32 / 0
  end if
end fun

fun shift_right_wrapping(self: u32, n: u32): u32
  let n_mod = bitand(n, : u32 / 31)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret : u32 / 0
  end if
end fun

// Rotates.

fun rotate_left(self: u32, n: u32): u32
  let n_mod = bitand(n, : u32 / 31)
  let left_part = shift_left_wrapping(self, n_mod)
  let right_amount = sub_wrapping(: u32 / 32, n_mod)
  let right_part = shift_right_wrapping(self, right_amount)
  ret bitor(left_part, right_part)
end fun

fun rotate_right(self: u32, n: u32): u32
  let n_mod = bitand(n, : u32 / 31)
  let right_part = shift_right_wrapping(self, n_mod)
  let left_amount = sub_wrapping(: u32 / 32, n_mod)
  let left_part = shift_left_wrapping(self, left_amount)
  ret bitor(left_part, right_part)
end fun

// Comparisons and utilities.

fun is_zero(self: u32): bool
  ret self == (: u32 / 0)
end fun

fun min(self: u32, other: u32): u32
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: u32, other: u32): u32
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: u32, min_val: u32, max_val: u32): u32
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

fun abs_diff(self: u32, other: u32): u32
  if self >= other
    ret sub_saturating(self, other)
  else
    ret sub_saturating(other, self)
  end if
end fun

// Average of two values, rounded down, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
fun midpoint(self: u32, other: u32): u32
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u32 / 1)
  ret add_wrapping(common, half_diff)
end fun
