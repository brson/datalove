fun min_value(): u32
  ret 0
end fun

fun max_value(): u32
  ret 4294967295
end fun

fun bits(): u32
  ret 32
end fun

// Intrinsic needed: intrinsic_popcount(u32): u32
fun count_ones(self: u32): u32
  // todo: ret intrinsic_popcount(self)
  ret 0
end fun

fun count_zeros(self: u32): u32
  ret sub_saturating(bits(), count_ones(self))
end fun

// Intrinsic needed: intrinsic_clz(u32): u32
fun leading_zeros(self: u32): u32
  // todo: ret intrinsic_clz(self)
  ret 0
end fun

// Intrinsic needed: intrinsic_ctz(u32): u32
fun trailing_zeros(self: u32): u32
  // todo: ret intrinsic_ctz(self)
  ret 0
end fun

fun leading_ones(self: u32): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: u32): u32
  ret trailing_zeros(bitnot(self))
end fun

// Intrinsic needed: intrinsic_u32_to_i32(u32): i32
fun cast_signed(self: u32): i32
  // todo: ret intrinsic_u32_to_i32(self)
  ret 0
end fun

// Intrinsic needed: intrinsic_swap_bytes_u32(u32): u32
fun swap_bytes(self: u32): u32
  // todo: ret intrinsic_swap_bytes_u32(self)
  ret 0
end fun

// Intrinsic needed: intrinsic_reverse_bits_u32(u32): u32
fun reverse_bits(self: u32): u32
  // todo: ret intrinsic_reverse_bits_u32(self)
  ret 0
end fun

// Intrinsics needed: intrinsic_swap_bytes_u32(u32): u32, intrinsic_is_big_endian(): bool
fun from_be(other: u32): u32
  // todo: if intrinsic_is_big_endian() then other else swap_bytes(other)
  ret 0
end fun

// Intrinsics needed: intrinsic_swap_bytes_u32(u32): u32, intrinsic_is_big_endian(): bool
fun from_le(other: u32): u32
  // todo: if intrinsic_is_big_endian() then swap_bytes(other) else other
  ret 0
end fun

// Intrinsics needed: intrinsic_swap_bytes_u32(u32): u32, intrinsic_is_big_endian(): bool
fun to_be(self: u32): u32
  // todo: if intrinsic_is_big_endian() then self else swap_bytes(self)
  ret 0
end fun

// Intrinsics needed: intrinsic_swap_bytes_u32(u32): u32, intrinsic_is_big_endian(): bool
fun to_le(self: u32): u32
  // todo: if intrinsic_is_big_endian() then swap_bytes(self) else self
  ret 0
end fun

// Checked addition. Returns none on overflow.
fun add_checked(self: u32, other: u32): ?u32
  ret some (self +? other)
end fun

// Intrinsic needed: intrinsic_i32_to_u32(i32): u32
fun add_checked_signed(self: u32, other: i32): ?u32
  // todo: if other >= 0 then add_checked(self, intrinsic_i32_to_u32(other))
  //       else sub_checked(self, intrinsic_i32_to_u32(-other))
  ret @none // fixme @-required
end fun

// Checked subtraction. Returns none on underflow.
fun sub_checked(self: u32, other: u32): ?u32
  ret some (self -? other)
end fun

// Intrinsic needed: intrinsic_i32_to_u32(i32): u32
fun sub_checked_signed(self: u32, other: i32): ?u32
  // todo: if other >= 0 then sub_checked(self, intrinsic_i32_to_u32(other))
  //       else add_checked(self, intrinsic_i32_to_u32(-other))
  ret @none // fixme @-required
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

// Intrinsic needed: intrinsic_add_wrapping_u32(u32, u32): u32
fun add_wrapping(self: u32, other: u32): u32
  // todo: ret intrinsic_add_wrapping_u32(self, other)
  ret 0
end fun

// Intrinsic needed: intrinsic_sub_wrapping_u32(u32, u32): u32
fun sub_wrapping(self: u32, other: u32): u32
  // todo: ret intrinsic_sub_wrapping_u32(self, other)
  ret 0
end fun

// Intrinsic needed: intrinsic_mul_wrapping_u32(u32, u32): u32
fun mul_wrapping(self: u32, other: u32): u32
  // todo: ret intrinsic_mul_wrapping_u32(self, other)
  ret 0
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

// Intrinsic needed: intrinsic_shl_u32(u32, u32): u32
// Returns none if shift >= 32 or if bits would be lost.
fun shift_left(self: u32, other: u32): ?u32
  // todo: if other >= 32 then @none
  //       else let result = intrinsic_shl_u32(self, other)
  //            if intrinsic_shr_u32(result, other) == self then some result else @none
  ret @none // fixme @
end fun

// Intrinsic needed: intrinsic_shr_u32(u32, u32): u32
// Returns none if shift >= 32.
fun shift_right(self: u32, other: u32): ?u32
  // todo: if other >= 32 then @none else some intrinsic_shr_u32(self, other)
  ret @none // fixme @
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

// Intrinsic needed: intrinsic_bitnot_u32(u32): u32
fun bitnot(self: u32): u32
  // todo: ret intrinsic_bitnot_u32(self)
  ret 0
end fun

// Intrinsic needed: intrinsic_bitand_u32(u32, u32): u32
fun bitand(self: u32, n: u32): u32
  // todo: ret intrinsic_bitand_u32(self, n)
  ret 0
end fun

// Intrinsic needed: intrinsic_bitor_u32(u32, u32): u32
fun bitor(self: u32, n: u32): u32
  // todo: ret intrinsic_bitor_u32(self, n)
  ret 0
end fun

// Intrinsic needed: intrinsic_bitxor_u32(u32, u32): u32
fun bitxor(self: u32, n: u32): u32
  // todo: ret intrinsic_bitxor_u32(self, n)
  ret 0
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
