// Constants.
// Note: These values are for the default 32-bit index configuration.
// With the index-64 feature, isize is 64-bit but these constants remain 32-bit values.

fun min_value(): isize
  ret (: isize / -2147483648)
end fun

fun max_value(): isize
  ret (: isize / 2147483647)
end fun

fun bits(): u32
  ret 32
end fun

// Sign functions.

fun signum(self: isize): isize
  let zero = (: isize / 0)
  let one = (: isize / 1)
  let neg_one = (: isize / -1)
  if self .> zero
    ret one
  else
    if self .< zero
      ret neg_one
    else
      ret zero
    end if
  end if
end fun

fun is_positive(self: isize): bool
  ret self .> (: isize / 0)
end fun

fun is_negative(self: isize): bool
  ret self .< (: isize / 0)
end fun

// Wrapping absolute value.
fun abs(self: isize): isize
  if self .< (: isize / 0)
    ret icall neg_wrapping_isize(self)
  else
    ret self
  end if
end fun

// Checked absolute value. Returns none for MIN.
fun abs_checked(self: isize): ?isize
  if self == min_value()
    ret @none
  else
    ret some abs(self)
  end if
end fun

// Saturating absolute value. MIN becomes MAX.
fun abs_saturating(self: isize): isize
  if self == min_value()
    ret max_value()
  else
    ret abs(self)
  end if
end fun

// Bitwise primitives (delegate to usize via cast).

fun bitnot(self: isize): isize
  let u = icall isize_to_usize(self)
  let result = icall bitnot_usize(u)
  ret icall usize_to_isize(result)
end fun

fun bitand(self: isize, n: isize): isize
  let a = icall isize_to_usize(self)
  let b = icall isize_to_usize(n)
  let result = icall bitand_usize(a, b)
  ret icall usize_to_isize(result)
end fun

fun bitor(self: isize, n: isize): isize
  let a = icall isize_to_usize(self)
  let b = icall isize_to_usize(n)
  let result = icall bitor_usize(a, b)
  ret icall usize_to_isize(result)
end fun

fun bitxor(self: isize, n: isize): isize
  let a = icall isize_to_usize(self)
  let b = icall isize_to_usize(n)
  let result = icall bitxor_usize(a, b)
  ret icall usize_to_isize(result)
end fun

// Bit counting (delegate to usize via cast).

fun count_ones(self: isize): u32
  let u = icall isize_to_usize(self)
  ret icall popcount_usize(u)
end fun

fun count_zeros(self: isize): u32
  let u = icall isize_to_usize(self)
  let ones = icall popcount_usize(u)
  ret icall sub_wrapping_u32(32, ones)
end fun

fun leading_zeros(self: isize): u32
  let u = icall isize_to_usize(self)
  ret icall clz_usize(u)
end fun

fun trailing_zeros(self: isize): u32
  let u = icall isize_to_usize(self)
  ret icall ctz_usize(u)
end fun

fun leading_ones(self: isize): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: isize): u32
  ret trailing_zeros(bitnot(self))
end fun

// Type conversion.

fun cast_unsigned(self: isize): usize
  ret icall isize_to_usize(self)
end fun

// Checked arithmetic.

fun neg_checked(self: isize): ?isize
  if self == min_value()
    ret @none
  else
    ret some icall neg_wrapping_isize(self)
  end if
end fun

fun add_checked(self: isize, other: isize): ?isize
  ret some (self +? other)
end fun

fun sub_checked(self: isize, other: isize): ?isize
  ret some (self -? other)
end fun

fun mul_checked(self: isize, other: isize): ?isize
  ret some (self *? other)
end fun

fun div_checked(self: isize, other: isize): ?isize
  ret some (self /? other)
end fun

// Signed remainder with overflow check.
// Overflow can occur with MIN % -1 on some platforms.
fun rem_checked(self: isize, other: isize): ?isize
  let zero = (: isize / 0)
  let neg_one = (: isize / -1)
  if other == zero
    ret @none
  else
    if self == min_value()
      if other == neg_one
        ret some zero
      else
        ret some icall srem_isize(self, other)
      end if
    else
      ret some icall srem_isize(self, other)
    end if
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: isize, other: isize): isize
  let zero = (: isize / 0)
  if add_checked(self, other) |value|
    ret value
  else
    // Overflow direction depends on signs.
    if other .> zero
      ret max_value()
    else
      ret min_value()
    end if
  end if
end fun

fun sub_saturating(self: isize, other: isize): isize
  let zero = (: isize / 0)
  if sub_checked(self, other) |value|
    ret value
  else
    // Underflow direction depends on signs.
    if other .> zero
      ret min_value()
    else
      ret max_value()
    end if
  end if
end fun

fun mul_saturating(self: isize, other: isize): isize
  if mul_checked(self, other) |value|
    ret value
  else
    // Determine sign of result for saturation direction.
    // Same sign (both negative or both non-negative) means positive result.
    if is_negative(self)
      if is_negative(other)
        ret max_value()
      else
        ret min_value()
      end if
    else
      if is_negative(other)
        ret min_value()
      else
        ret max_value()
      end if
    end if
  end if
end fun

// Wrapping arithmetic.

fun neg_wrapping(self: isize): isize
  ret icall neg_wrapping_isize(self)
end fun

fun add_wrapping(self: isize, other: isize): isize
  let a = icall isize_to_usize(self)
  let b = icall isize_to_usize(other)
  let result = icall add_wrapping_usize(a, b)
  ret icall usize_to_isize(result)
end fun

fun sub_wrapping(self: isize, other: isize): isize
  let a = icall isize_to_usize(self)
  let b = icall isize_to_usize(other)
  let result = icall sub_wrapping_usize(a, b)
  ret icall usize_to_isize(result)
end fun

fun mul_wrapping(self: isize, other: isize): isize
  let a = icall isize_to_usize(self)
  let b = icall isize_to_usize(other)
  let result = icall mul_wrapping_usize(a, b)
  ret icall usize_to_isize(result)
end fun

// Shifts.

// Checked left shift. Returns none if n >= 32.
fun shift_left(self: isize, n: u32): ?isize
  if n >= 32
    ret @none
  else
    let u = icall isize_to_usize(self)
    let result = icall shl_usize(u, n)
    ret some icall usize_to_isize(result)
  end if
end fun

// Checked arithmetic right shift. Returns none if n >= 32.
fun shift_right(self: isize, n: u32): ?isize
  if n >= 32
    ret @none
  else
    ret some icall sshr_isize(self, n)
  end if
end fun

fun shift_left_wrapping(self: isize, n: u32): isize
  let n_mod = icall bitand_u32(n, 31)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: isize / 0)
  end if
end fun

fun shift_right_wrapping(self: isize, n: u32): isize
  let n_mod = icall bitand_u32(n, 31)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: isize / 0)
  end if
end fun

fun shift_left_saturating(self: isize, n: u32): isize
  if n >= 32
    if is_negative(self)
      ret min_value()
    else
      ret max_value()
    end if
  else
    if shift_left(self, n) |value|
      ret value
    else
      if is_negative(self)
        ret min_value()
      else
        ret max_value()
      end if
    end if
  end if
end fun

fun shift_right_saturating(self: isize, n: u32): isize
  let zero = (: isize / 0)
  let neg_one = (: isize / -1)
  if n >= 32
    // Arithmetic shift fills with sign bit.
    if is_negative(self)
      ret neg_one
    else
      ret zero
    end if
  else
    if shift_right(self, n) |value|
      ret value
    else
      if is_negative(self)
        ret neg_one
      else
        ret zero
      end if
    end if
  end if
end fun

// Rotations (delegate to usize).

fun rotate_left(self: isize, n: u32): isize
  let n_mod = icall bitand_u32(n, 31)
  let u = icall isize_to_usize(self)
  let left_part = icall shl_usize(u, n_mod)
  let right_amount = icall sub_wrapping_u32(32, n_mod)
  let right_part = icall shr_usize(u, right_amount)
  let result = icall bitor_usize(left_part, right_part)
  ret icall usize_to_isize(result)
end fun

fun rotate_right(self: isize, n: u32): isize
  let n_mod = icall bitand_u32(n, 31)
  let u = icall isize_to_usize(self)
  let right_part = icall shr_usize(u, n_mod)
  let left_amount = icall sub_wrapping_u32(32, n_mod)
  let left_part = icall shl_usize(u, left_amount)
  let result = icall bitor_usize(left_part, right_part)
  ret icall usize_to_isize(result)
end fun

// Byte manipulation (delegate to usize).

fun swap_bytes(self: isize): isize
  let u = icall isize_to_usize(self)
  let result = icall swap_bytes_usize(u)
  ret icall usize_to_isize(result)
end fun

fun reverse_bits(self: isize): isize
  let u = icall isize_to_usize(self)
  let result = icall reverse_bits_usize(u)
  ret icall usize_to_isize(result)
end fun

// Endianness conversion.

fun from_be(other: isize): isize
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: isize): isize
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: isize): isize
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: isize): isize
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Comparisons and utilities.

fun is_zero(self: isize): bool
  ret self == (: isize / 0)
end fun

fun min(self: isize, other: isize): isize
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: isize, other: isize): isize
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: isize, min_val: isize, max_val: isize): isize
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

// Absolute difference, returns usize (always non-negative).
fun abs_diff(self: isize, other: isize): usize
  if self >= other
    // self - other is non-negative.
    let diff = sub_wrapping(self, other)
    ret icall isize_to_usize(diff)
  else
    // other - self is non-negative.
    let diff = sub_wrapping(other, self)
    ret icall isize_to_usize(diff)
  end if
end fun

// Average of two values, rounded toward negative infinity, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
// For signed, the arithmetic right shift handles negative numbers correctly.
fun midpoint(self: isize, other: isize): isize
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, 1)
  ret add_wrapping(common, half_diff)
end fun
