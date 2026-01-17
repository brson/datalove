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
  // todo
  ret 0
end fun

fun count_zeros(self: u32): u32
  ret sub_saturating(bits(), count_ones(self))
end fun

fun leading_zeros(self: u32): u32
  // todo
  ret 0
end fun

fun trailing_zeros(self: u32): u32
  // todo
  ret 0
end fun

fun leading_ones(self: u32): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: u32): u32
  ret trailing_zeros(bitnot(self))
end fun

fun cast_signed(self: u32): i32
  // todo
  ret 0
end fun

fun swap_bytes(self: u32): u32
  // todo
  ret 0
end fun

fun reverse_bits(self: u32): u32
  // todo
  ret 0
end fun

fun from_be(other: u32): u32
  // todo
  ret 0
end fun

fun from_le(other: u32): u32
  // todo
  ret 0
end fun

fun to_be(self: u32): u32
  // todo
  ret 0
end fun

fun to_le(self: u32): u32
  // todo
  ret 0
end fun

// Checked addition. Returns none on overflow.
fun add_checked(self: u32, other: u32): ?u32
  ret some (self +? other)
end fun

fun add_checked_signed(self: u32, other: i32): ?u32
  // todo
  ret @none // fixme @-required
end fun

// Checked subtraction. Returns none on underflow.
fun sub_checked(self: u32, other: u32): ?u32
  ret some (self -? other)
end fun

fun sub_checked_signed(self: u32, other: i32): ?u32
  // todo
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

fun add_wrapping(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun sub_wrapping(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun mul_wrapping(self: u32, other: u32): u32
  // todo
  ret 0
end fun

fun div_wrapping(self: u32, other: u32): ?u32
  // todo
  ret @none // fixme @-required
end fun

fun neg_checked(self: u32): ?u32
  if self == 0
    ret some 0
  else
    ret @none
  end if
end fun

fun shift_left(self: u32, other: u32): ?u32
  ret @none // fixme @
end fun

fun shift_right(self: u32, other: u32): ?u32
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

fun bitnot(self: u32): u32
  // todo
  ret 0
end fun

fun bitand(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun bitor(self: u32, n: u32): u32
  // todo
  ret 0
end fun

fun bitxor(self: u32, n: u32): u32
  // todo
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
// Since we check the condition first, subtraction cannot overflow.
fun abs_diff(self: u32, other: u32): u32
  if self >= other
    ret sub_wrapping(self, other)
  else
    ret sub_wrapping(other, self)
  end if
end fun
