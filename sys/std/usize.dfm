// Constants.
// Note: These values are for the default 32-bit index configuration.
// With the index-64 feature, usize is 64-bit but these constants remain 32-bit values.

fun min_value(): usize
  ret (: usize / 0)
end fun

fun max_value(): usize
  ret (: usize / 4294967295)
end fun

fun bits(): u32
  ret 32
end fun

// Bitwise primitives.

fun bitnot(self: usize): usize
  ret icall bitnot_usize(self)
end fun

fun bitand(self: usize, n: usize): usize
  ret icall bitand_usize(self, n)
end fun

fun bitor(self: usize, n: usize): usize
  ret icall bitor_usize(self, n)
end fun

fun bitxor(self: usize, n: usize): usize
  ret icall bitxor_usize(self, n)
end fun

// Bit counting.

fun count_ones(self: usize): u32
  ret icall popcount_usize(self)
end fun

fun count_zeros(self: usize): u32
  ret icall sub_wrapping_u32(bits(), count_ones(self))
end fun

fun leading_zeros(self: usize): u32
  ret icall clz_usize(self)
end fun

fun trailing_zeros(self: usize): u32
  ret icall ctz_usize(self)
end fun

fun leading_ones(self: usize): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: usize): u32
  ret trailing_zeros(bitnot(self))
end fun

fun is_power_of_two(self: usize): bool
  let zero = (: usize / 0)
  if self == zero
    ret @false
  else
    ret count_ones(self) == 1
  end if
end fun

// Integer log base 2. Returns none if self is zero.
fun ilog2(self: usize): ?u32
  let zero = (: usize / 0)
  if self == zero
    ret @none
  else
    ret some icall sub_wrapping_u32(icall sub_wrapping_u32(bits(), 1), leading_zeros(self))
  end if
end fun

// Byte manipulation.

fun swap_bytes(self: usize): usize
  ret icall swap_bytes_usize(self)
end fun

fun reverse_bits(self: usize): usize
  ret icall reverse_bits_usize(self)
end fun

// Endianness conversion.

fun from_be(other: usize): usize
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: usize): usize
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: usize): usize
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: usize): usize
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Type conversion.

fun cast_signed(self: usize): isize
  ret icall usize_to_isize(self)
end fun

// Checked arithmetic.

fun neg_checked(self: usize): ?usize
  let zero = (: usize / 0)
  if self == zero
    ret some zero
  else
    ret @none
  end if
end fun

fun add_checked(self: usize, other: usize): ?usize
  ret some (self +? other)
end fun

fun sub_checked(self: usize, other: usize): ?usize
  ret some (self -? other)
end fun

fun mul_checked(self: usize, other: usize): ?usize
  ret some (self *? other)
end fun

fun div_checked(self: usize, other: usize): ?usize
  ret some (self /? other)
end fun

fun rem_checked(self: usize, other: usize): ?usize
  let zero = (: usize / 0)
  if other == zero
    ret @none
  else
    ret some icall rem_usize(self, other)
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: usize, other: usize): usize
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

fun sub_saturating(self: usize, other: usize): usize
  if sub_checked(self, other) |value|
    ret value
  else
    ret (: usize / 0)
  end if
end fun

fun mul_saturating(self: usize, other: usize): usize
  if mul_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// For usize, division cannot overflow (result <= dividend), so this is same as div_checked.
fun div_saturating(self: usize, other: usize): ?usize
  ret div_checked(self, other)
end fun

// Wrapping arithmetic.

fun add_wrapping(self: usize, other: usize): usize
  ret icall add_wrapping_usize(self, other)
end fun

fun sub_wrapping(self: usize, other: usize): usize
  ret icall sub_wrapping_usize(self, other)
end fun

fun mul_wrapping(self: usize, other: usize): usize
  ret icall mul_wrapping_usize(self, other)
end fun

// usize division cannot overflow.
fun div_wrapping(self: usize, other: usize): ?usize
  ret div_checked(self, other)
end fun

// Shifts.

// Returns none if shift >= bits().
fun shift_left(self: usize, n: u32): ?usize
  if n >= bits()
    ret @none
  else
    ret some icall shl_usize(self, n)
  end if
end fun

// Returns none if shift >= bits().
fun shift_right(self: usize, n: u32): ?usize
  if n >= bits()
    ret @none
  else
    ret some icall shr_usize(self, n)
  end if
end fun

fun shift_left_saturating(self: usize, n: u32): usize
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

fun shift_right_saturating(self: usize, n: u32): usize
  if n >= bits()
    ret (: usize / 0)
  else
    if shift_right(self, n) |value|
      ret value
    else
      ret (: usize / 0)
    end if
  end if
end fun

fun shift_left_wrapping(self: usize, n: u32): usize
  let n_mod = icall bitand_u32(n, 31)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: usize / 0)
  end if
end fun

fun shift_right_wrapping(self: usize, n: u32): usize
  let n_mod = icall bitand_u32(n, 31)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: usize / 0)
  end if
end fun

// Rotates.

fun rotate_left(self: usize, n: u32): usize
  let n_mod = icall bitand_u32(n, 31)
  if shift_left(self, n_mod) |left_part|
    let right_amount = icall sub_wrapping_u32(32, n_mod)
    if shift_right(self, right_amount) |right_part|
      ret bitor(left_part, right_part)
    else
      ret left_part
    end if
  else
    ret self
  end if
end fun

fun rotate_right(self: usize, n: u32): usize
  let n_mod = icall bitand_u32(n, 31)
  if shift_right(self, n_mod) |right_part|
    let left_amount = icall sub_wrapping_u32(32, n_mod)
    if shift_left(self, left_amount) |left_part|
      ret bitor(left_part, right_part)
    else
      ret right_part
    end if
  else
    ret self
  end if
end fun

// Comparisons and utilities.

fun is_zero(self: usize): bool
  ret self == (: usize / 0)
end fun

fun min(self: usize, other: usize): usize
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: usize, other: usize): usize
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: usize, min_val: usize, max_val: usize): usize
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

fun abs_diff(self: usize, other: usize): usize
  if self >= other
    ret sub_saturating(self, other)
  else
    ret sub_saturating(other, self)
  end if
end fun

// Average of two values, rounded down, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
fun midpoint(self: usize, other: usize): usize
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, 1)
  ret add_wrapping(common, half_diff)
end fun
