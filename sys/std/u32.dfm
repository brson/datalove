fun min_value(): u32
  ret 0
end fun

fun max_value(): u32
  ret 4294967295
end fun

fun bits(): u32
  ret 32
end fun

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

fun cast_signed(self: u32): i32
  ret icall u32_to_i32(self)
end fun

fun swap_bytes(self: u32): u32
  ret icall swap_bytes_u32(self)
end fun

fun reverse_bits(self: u32): u32
  ret icall reverse_bits_u32(self)
end fun

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

// Checked addition. Returns none on overflow.
fun add_checked(self: u32, other: u32): ?u32
  ret some (self +? other)
end fun

// Checked signed addition. Adds a signed i32 to u32.
// Returns none on overflow (positive other) or underflow (negative other).
fun add_checked_signed(self: u32, other: i32): ?u32
  let zero: i32 = 0
  if other >= zero
    let other_u32 = icall i32_to_u32(other)
    ret some (self +? other_u32)
  else
    // Check for i32::MIN by comparing bit patterns.
    let other_bits = icall i32_to_u32(other)
    let i32_min_bits: u32 = 2147483648
    if other_bits == i32_min_bits
      ret some (self -? i32_min_bits)
    else
      let neg_other = -?other
      let abs_other = icall i32_to_u32(neg_other)
      ret some (self -? abs_other)
    end if
  end if
end fun

// Checked subtraction. Returns none on underflow.
fun sub_checked(self: u32, other: u32): ?u32
  ret some (self -? other)
end fun

// Checked signed subtraction. Subtracts a signed i32 from u32.
// Returns none on underflow (positive other) or overflow (negative other).
fun sub_checked_signed(self: u32, other: i32): ?u32
  let zero: i32 = 0
  if other >= zero
    let other_u32 = icall i32_to_u32(other)
    ret some (self -? other_u32)
  else
    // Check for i32::MIN by comparing bit patterns.
    let other_bits = icall i32_to_u32(other)
    let i32_min_bits: u32 = 2147483648
    if other_bits == i32_min_bits
      ret some (self +? i32_min_bits)
    else
      let neg_other = -?other
      let abs_other = icall i32_to_u32(neg_other)
      ret some (self +? abs_other)
    end if
  end if
end fun

// Checked multiplication. Returns none on overflow.
fun mul_checked(self: u32, other: u32): ?u32
  ret some (self *? other)
end fun

// Checked division. Returns none on division by zero.
fun div_checked(self: u32, other: u32): ?u32
  ret some (self /? other)
end fun

// Saturating addition. Returns max_value on overflow.
fun add_saturating(self: u32, other: u32): u32
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// Saturating subtraction. Returns 0 on underflow.
fun sub_saturating(self: u32, other: u32): u32
  if sub_checked(self, other) |value|
    ret value
  else
    ret 0
  end if
end fun

// Saturating multiplication. Returns max_value on overflow.
fun mul_saturating(self: u32, other: u32): u32
  if mul_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// Saturating division. Returns none on division by zero.
// For u32, division cannot overflow (result <= dividend), so this is same as div_checked.
fun div_saturating(self: u32, other: u32): ?u32
  ret div_checked(self, other)
end fun

fun add_wrapping(self: u32, other: u32): u32
  ret icall add_wrapping_u32(self, other)
end fun

fun sub_wrapping(self: u32, other: u32): u32
  ret icall sub_wrapping_u32(self, other)
end fun

fun mul_wrapping(self: u32, other: u32): u32
  ret icall mul_wrapping_u32(self, other)
end fun

// No intrinsic needed: u32 division cannot overflow.
fun div_wrapping(self: u32, other: u32): ?u32
  ret div_checked(self, other)
end fun

fun neg_checked(self: u32): ?u32
  if self == 0
    ret some 0
  else
    ret @none
  end if
end fun

// Returns none if shift >= 32.
fun shift_left(self: u32, other: u32): ?u32
  if other >= 32
    ret @none
  else
    ret some icall shl_u32(self, other)
  end if
end fun

// Returns none if shift >= 32.
fun shift_right(self: u32, other: u32): ?u32
  if other >= 32
    ret @none
  else
    ret some icall shr_u32(self, other)
  end if
end fun

fun shift_left_saturating(self: u32, other: u32): u32
  if other >= 32
    ret max_value()
  else
    if shift_left(self, other) |value|
      ret value
    else
      ret max_value()
    end if
  end if
end fun

fun shift_right_saturating(self: u32, other: u32): u32
  if other >= 32
    ret 0
  else
    if shift_right(self, other) |value|
      ret value
    else
      ret 0
    end if
  end if
end fun

fun shift_left_wrapping(self: u32, other: u32): u32
  let n_mod = bitand(other, 31)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret 0
  end if
end fun

fun shift_right_wrapping(self: u32, other: u32): u32
  let n_mod = bitand(other, 31)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret 0
  end if
end fun

fun rotate_left(self: u32, n: u32): u32
  let n_mod = bitand(n, 31)
  let left_part = shift_left_wrapping(self, n_mod)
  let right_amount = sub_wrapping(32, n_mod)
  let right_part = shift_right_wrapping(self, right_amount)
  ret bitor(left_part, right_part)
end fun

fun rotate_right(self: u32, n: u32): u32
  let n_mod = bitand(n, 31)
  let right_part = shift_right_wrapping(self, n_mod)
  let left_amount = sub_wrapping(32, n_mod)
  let left_part = shift_left_wrapping(self, left_amount)
  ret bitor(left_part, right_part)
end fun

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

// True if zero.
fun is_zero(self: u32): bool
  ret self == 0
end fun

// Maximum of two values.
fun max(self: u32, other: u32): u32
  if self >= other
    ret self
  else
    ret other
  end if
end fun

// Minimum of two values.
fun min(self: u32, other: u32): u32
  if self <= other
    ret self
  else
    ret other
  end if
end fun

// Clamp value to range [min_val, max_val].
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

// Absolute difference between two values.
// Since we check the condition first, subtraction cannot underflow.
fun abs_diff(self: u32, other: u32): u32
  if self >= other
    ret sub_saturating(self, other)
  else
    ret sub_saturating(other, self)
  end if
end fun
