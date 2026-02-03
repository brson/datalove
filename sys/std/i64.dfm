// Constants.

fun min_value(): i64
  ret (: i64 / -9223372036854775808)
end fun

fun max_value(): i64
  ret : i64 / 9223372036854775807
end fun

fun bits(): u64
  ret : u64 / 64
end fun

// Sign functions.

fun signum(self: i64): i64
  let zero = (: i64 / 0)
  if self .> zero
    ret : i64 / 1
  else
    if self .< zero
      ret (: i64 / -1)
    else
      ret zero
    end if
  end if
end fun

fun is_positive(self: i64): bool
  ret self .> (: i64 / 0)
end fun

fun is_negative(self: i64): bool
  ret self .< (: i64 / 0)
end fun

// Wrapping absolute value.
fun abs(self: i64): i64
  if self .< (: i64 / 0)
    ret icall neg_wrapping_i64(self)
  else
    ret self
  end if
end fun

// Checked absolute value. Returns none for MIN.
fun abs_checked(self: i64): ?i64
  if self == min_value()
    ret none
  else
    ret some abs(self)
  end if
end fun

// Saturating absolute value. MIN becomes MAX.
fun abs_saturating(self: i64): i64
  if self == min_value()
    ret max_value()
  else
    ret abs(self)
  end if
end fun

// Bitwise primitives (delegate to u64 via cast).

fun bitnot(self: i64): i64
  let u = icall i64_to_u64(self)
  let result = icall bitnot_u64(u)
  ret icall u64_to_i64(result)
end fun

fun bitand(self: i64, n: i64): i64
  let a = icall i64_to_u64(self)
  let b = icall i64_to_u64(n)
  let result = icall bitand_u64(a, b)
  ret icall u64_to_i64(result)
end fun

fun bitor(self: i64, n: i64): i64
  let a = icall i64_to_u64(self)
  let b = icall i64_to_u64(n)
  let result = icall bitor_u64(a, b)
  ret icall u64_to_i64(result)
end fun

fun bitxor(self: i64, n: i64): i64
  let a = icall i64_to_u64(self)
  let b = icall i64_to_u64(n)
  let result = icall bitxor_u64(a, b)
  ret icall u64_to_i64(result)
end fun

// Bit counting (delegate to u64 via cast).

fun count_ones(self: i64): u64
  let u = icall i64_to_u64(self)
  ret icall popcount_u64(u)
end fun

fun count_zeros(self: i64): u64
  let u = icall i64_to_u64(self)
  let ones = icall popcount_u64(u)
  ret icall sub_wrapping_u64((: u64 / 64), ones)
end fun

fun leading_zeros(self: i64): u64
  let u = icall i64_to_u64(self)
  ret icall clz_u64(u)
end fun

fun trailing_zeros(self: i64): u64
  let u = icall i64_to_u64(self)
  ret icall ctz_u64(u)
end fun

fun leading_ones(self: i64): u64
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: i64): u64
  ret trailing_zeros(bitnot(self))
end fun

// Type conversion.

fun cast_unsigned(self: i64): u64
  ret icall i64_to_u64(self)
end fun

// Checked arithmetic.

fun neg_checked(self: i64): ?i64
  if self == min_value()
    ret none
  else
    ret some icall neg_wrapping_i64(self)
  end if
end fun

fun add_checked(self: i64, other: i64): ?i64
  ret some (self +? other)
end fun

fun sub_checked(self: i64, other: i64): ?i64
  ret some (self -? other)
end fun

fun mul_checked(self: i64, other: i64): ?i64
  ret some (self *? other)
end fun

fun div_checked(self: i64, other: i64): ?i64
  ret some (self /? other)
end fun

// Signed remainder with overflow check.
// Overflow can occur with MIN % -1 on some platforms.
fun rem_checked(self: i64, other: i64): ?i64
  let zero = (: i64 / 0)
  if other == zero
    ret none
  else
    if self == min_value()
      if other == (: i64 / -1)
        ret some zero
      else
        ret some icall srem_i64(self, other)
      end if
    else
      ret some icall srem_i64(self, other)
    end if
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: i64, other: i64): i64
  if add_checked(self, other) |value|
    ret value
  else
    // Overflow direction depends on signs.
    if other .> (: i64 / 0)
      ret max_value()
    else
      ret min_value()
    end if
  end if
end fun

fun sub_saturating(self: i64, other: i64): i64
  if sub_checked(self, other) |value|
    ret value
  else
    // Underflow direction depends on signs.
    if other .> (: i64 / 0)
      ret min_value()
    else
      ret max_value()
    end if
  end if
end fun

fun mul_saturating(self: i64, other: i64): i64
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

fun neg_wrapping(self: i64): i64
  ret icall neg_wrapping_i64(self)
end fun

fun add_wrapping(self: i64, other: i64): i64
  let a = icall i64_to_u64(self)
  let b = icall i64_to_u64(other)
  let result = icall add_wrapping_u64(a, b)
  ret icall u64_to_i64(result)
end fun

fun sub_wrapping(self: i64, other: i64): i64
  let a = icall i64_to_u64(self)
  let b = icall i64_to_u64(other)
  let result = icall sub_wrapping_u64(a, b)
  ret icall u64_to_i64(result)
end fun

fun mul_wrapping(self: i64, other: i64): i64
  let a = icall i64_to_u64(self)
  let b = icall i64_to_u64(other)
  let result = icall mul_wrapping_u64(a, b)
  ret icall u64_to_i64(result)
end fun

// Shifts.

// Checked left shift. Returns none if n >= 64.
fun shift_left(self: i64, n: u64): ?i64
  if n >= (: u64 / 64)
    ret none
  else
    let u = icall i64_to_u64(self)
    let result = icall shl_u64(u, n)
    ret some icall u64_to_i64(result)
  end if
end fun

// Checked arithmetic right shift. Returns none if n >= 64.
fun shift_right(self: i64, n: u64): ?i64
  if n >= (: u64 / 64)
    ret none
  else
    ret some icall sshr_i64(self, n)
  end if
end fun

fun shift_left_wrapping(self: i64, n: u64): i64
  let n_mod = icall bitand_u64(n, (: u64 / 63))
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: i64 / 0)
  end if
end fun

fun shift_right_wrapping(self: i64, n: u64): i64
  let n_mod = icall bitand_u64(n, (: u64 / 63))
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: i64 / 0)
  end if
end fun

fun shift_left_saturating(self: i64, n: u64): i64
  if n >= (: u64 / 64)
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

fun shift_right_saturating(self: i64, n: u64): i64
  if n >= (: u64 / 64)
    // Arithmetic shift fills with sign bit.
    if is_negative(self)
      ret (: i64 / -1)
    else
      ret (: i64 / 0)
    end if
  else
    if shift_right(self, n) |value|
      ret value
    else
      if is_negative(self)
        ret (: i64 / -1)
      else
        ret (: i64 / 0)
      end if
    end if
  end if
end fun

// Rotations (delegate to u64).

fun rotate_left(self: i64, n: u64): i64
  let n_mod = icall bitand_u64(n, (: u64 / 63))
  let u = icall i64_to_u64(self)
  let left_part = icall shl_u64(u, n_mod)
  let right_amount = icall sub_wrapping_u64((: u64 / 64), n_mod)
  let right_part = icall shr_u64(u, right_amount)
  let result = icall bitor_u64(left_part, right_part)
  ret icall u64_to_i64(result)
end fun

fun rotate_right(self: i64, n: u64): i64
  let n_mod = icall bitand_u64(n, (: u64 / 63))
  let u = icall i64_to_u64(self)
  let right_part = icall shr_u64(u, n_mod)
  let left_amount = icall sub_wrapping_u64((: u64 / 64), n_mod)
  let left_part = icall shl_u64(u, left_amount)
  let result = icall bitor_u64(left_part, right_part)
  ret icall u64_to_i64(result)
end fun

// Byte manipulation (delegate to u64).

fun swap_bytes(self: i64): i64
  let u = icall i64_to_u64(self)
  let result = icall swap_bytes_u64(u)
  ret icall u64_to_i64(result)
end fun

fun reverse_bits(self: i64): i64
  let u = icall i64_to_u64(self)
  let result = icall reverse_bits_u64(u)
  ret icall u64_to_i64(result)
end fun

// Endianness conversion.

fun from_be(other: i64): i64
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: i64): i64
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: i64): i64
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: i64): i64
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Comparisons and utilities.

fun is_zero(self: i64): bool
  ret self == (: i64 / 0)
end fun

fun min(self: i64, other: i64): i64
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: i64, other: i64): i64
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: i64, min_val: i64, max_val: i64): i64
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

// Absolute difference, returns u64 (always non-negative).
fun abs_diff(self: i64, other: i64): u64
  if self >= other
    // self - other is non-negative.
    let diff = sub_wrapping(self, other)
    ret icall i64_to_u64(diff)
  else
    // other - self is non-negative.
    let diff = sub_wrapping(other, self)
    ret icall i64_to_u64(diff)
  end if
end fun

// Average of two values, rounded toward negative infinity, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
// For signed, the arithmetic right shift handles negative numbers correctly.
fun midpoint(self: i64, other: i64): i64
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u64 / 1)
  ret add_wrapping(common, half_diff)
end fun
