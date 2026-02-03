// Constants.

fun min_value(): i8
  ret (: i8 / -128)
end fun

fun max_value(): i8
  ret : i8 / 127
end fun

fun bits(): u8
  ret : u8 / 8
end fun

// Sign functions.

fun signum(self: i8): i8
  let zero = (: i8 / 0)
  if self .> zero
    ret : i8 / 1
  else
    if self .< zero
      ret (: i8 / -1)
    else
      ret zero
    end if
  end if
end fun

fun is_positive(self: i8): bool
  ret self .> (: i8 / 0)
end fun

fun is_negative(self: i8): bool
  ret self .< (: i8 / 0)
end fun

// Wrapping absolute value.
fun abs(self: i8): i8
  if self .< (: i8 / 0)
    ret icall neg_wrapping_i8(self)
  else
    ret self
  end if
end fun

// Checked absolute value. Returns none for MIN.
fun abs_checked(self: i8): ?i8
  if self == min_value()
    ret none
  else
    ret some abs(self)
  end if
end fun

// Saturating absolute value. MIN becomes MAX.
fun abs_saturating(self: i8): i8
  if self == min_value()
    ret max_value()
  else
    ret abs(self)
  end if
end fun

// Bitwise primitives (delegate to u8 via cast).

fun bitnot(self: i8): i8
  let u = icall i8_to_u8(self)
  let result = icall bitnot_u8(u)
  ret icall u8_to_i8(result)
end fun

fun bitand(self: i8, n: i8): i8
  let a = icall i8_to_u8(self)
  let b = icall i8_to_u8(n)
  let result = icall bitand_u8(a, b)
  ret icall u8_to_i8(result)
end fun

fun bitor(self: i8, n: i8): i8
  let a = icall i8_to_u8(self)
  let b = icall i8_to_u8(n)
  let result = icall bitor_u8(a, b)
  ret icall u8_to_i8(result)
end fun

fun bitxor(self: i8, n: i8): i8
  let a = icall i8_to_u8(self)
  let b = icall i8_to_u8(n)
  let result = icall bitxor_u8(a, b)
  ret icall u8_to_i8(result)
end fun

// Bit counting (delegate to u8 via cast).

fun count_ones(self: i8): u8
  let u = icall i8_to_u8(self)
  ret icall popcount_u8(u)
end fun

fun count_zeros(self: i8): u8
  let u = icall i8_to_u8(self)
  let ones = icall popcount_u8(u)
  ret icall sub_wrapping_u8((: u8 / 8), ones)
end fun

fun leading_zeros(self: i8): u8
  let u = icall i8_to_u8(self)
  ret icall clz_u8(u)
end fun

fun trailing_zeros(self: i8): u8
  let u = icall i8_to_u8(self)
  ret icall ctz_u8(u)
end fun

fun leading_ones(self: i8): u8
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: i8): u8
  ret trailing_zeros(bitnot(self))
end fun

// Type conversion.

fun cast_unsigned(self: i8): u8
  ret icall i8_to_u8(self)
end fun

// Checked arithmetic.

fun neg_checked(self: i8): ?i8
  if self == min_value()
    ret none
  else
    ret some icall neg_wrapping_i8(self)
  end if
end fun

fun add_checked(self: i8, other: i8): ?i8
  ret some (self +? other)
end fun

fun sub_checked(self: i8, other: i8): ?i8
  ret some (self -? other)
end fun

fun mul_checked(self: i8, other: i8): ?i8
  ret some (self *? other)
end fun

fun div_checked(self: i8, other: i8): ?i8
  ret some (self /? other)
end fun

// Signed remainder with overflow check.
// Overflow can occur with MIN % -1 on some platforms.
fun rem_checked(self: i8, other: i8): ?i8
  let zero = (: i8 / 0)
  if other == zero
    ret none
  else
    if self == min_value()
      if other == (: i8 / -1)
        ret some zero
      else
        ret some icall srem_i8(self, other)
      end if
    else
      ret some icall srem_i8(self, other)
    end if
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: i8, other: i8): i8
  if add_checked(self, other) |value|
    ret value
  else
    // Overflow direction depends on signs.
    if other .> (: i8 / 0)
      ret max_value()
    else
      ret min_value()
    end if
  end if
end fun

fun sub_saturating(self: i8, other: i8): i8
  if sub_checked(self, other) |value|
    ret value
  else
    // Underflow direction depends on signs.
    if other .> (: i8 / 0)
      ret min_value()
    else
      ret max_value()
    end if
  end if
end fun

fun mul_saturating(self: i8, other: i8): i8
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

fun neg_wrapping(self: i8): i8
  ret icall neg_wrapping_i8(self)
end fun

fun add_wrapping(self: i8, other: i8): i8
  let a = icall i8_to_u8(self)
  let b = icall i8_to_u8(other)
  let result = icall add_wrapping_u8(a, b)
  ret icall u8_to_i8(result)
end fun

fun sub_wrapping(self: i8, other: i8): i8
  let a = icall i8_to_u8(self)
  let b = icall i8_to_u8(other)
  let result = icall sub_wrapping_u8(a, b)
  ret icall u8_to_i8(result)
end fun

fun mul_wrapping(self: i8, other: i8): i8
  let a = icall i8_to_u8(self)
  let b = icall i8_to_u8(other)
  let result = icall mul_wrapping_u8(a, b)
  ret icall u8_to_i8(result)
end fun

// Shifts.

// Checked left shift. Returns none if n >= 8.
fun shift_left(self: i8, n: u8): ?i8
  if n >= (: u8 / 8)
    ret none
  else
    let u = icall i8_to_u8(self)
    let result = icall shl_u8(u, n)
    ret some icall u8_to_i8(result)
  end if
end fun

// Checked arithmetic right shift. Returns none if n >= 8.
fun shift_right(self: i8, n: u8): ?i8
  if n >= (: u8 / 8)
    ret none
  else
    ret some icall sshr_i8(self, n)
  end if
end fun

fun shift_left_wrapping(self: i8, n: u8): i8
  let n_mod = icall bitand_u8(n, (: u8 / 7))
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: i8 / 0)
  end if
end fun

fun shift_right_wrapping(self: i8, n: u8): i8
  let n_mod = icall bitand_u8(n, (: u8 / 7))
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: i8 / 0)
  end if
end fun

fun shift_left_saturating(self: i8, n: u8): i8
  if n >= (: u8 / 8)
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

fun shift_right_saturating(self: i8, n: u8): i8
  if n >= (: u8 / 8)
    // Arithmetic shift fills with sign bit.
    if is_negative(self)
      ret (: i8 / -1)
    else
      ret (: i8 / 0)
    end if
  else
    if shift_right(self, n) |value|
      ret value
    else
      if is_negative(self)
        ret (: i8 / -1)
      else
        ret (: i8 / 0)
      end if
    end if
  end if
end fun

// Rotations (delegate to u8).

fun rotate_left(self: i8, n: u8): i8
  let n_mod = icall bitand_u8(n, (: u8 / 7))
  let u = icall i8_to_u8(self)
  let left_part = icall shl_u8(u, n_mod)
  let right_amount = icall sub_wrapping_u8((: u8 / 8), n_mod)
  let right_part = icall shr_u8(u, right_amount)
  let result = icall bitor_u8(left_part, right_part)
  ret icall u8_to_i8(result)
end fun

fun rotate_right(self: i8, n: u8): i8
  let n_mod = icall bitand_u8(n, (: u8 / 7))
  let u = icall i8_to_u8(self)
  let right_part = icall shr_u8(u, n_mod)
  let left_amount = icall sub_wrapping_u8((: u8 / 8), n_mod)
  let left_part = icall shl_u8(u, left_amount)
  let result = icall bitor_u8(left_part, right_part)
  ret icall u8_to_i8(result)
end fun

// Bit manipulation (delegate to u8).

fun reverse_bits(self: i8): i8
  let u = icall i8_to_u8(self)
  let result = icall reverse_bits_u8(u)
  ret icall u8_to_i8(result)
end fun

// Comparisons and utilities.

fun is_zero(self: i8): bool
  ret self == (: i8 / 0)
end fun

fun min(self: i8, other: i8): i8
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: i8, other: i8): i8
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: i8, min_val: i8, max_val: i8): i8
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

// Absolute difference, returns u8 (always non-negative).
fun abs_diff(self: i8, other: i8): u8
  if self >= other
    // self - other is non-negative.
    let diff = sub_wrapping(self, other)
    ret icall i8_to_u8(diff)
  else
    // other - self is non-negative.
    let diff = sub_wrapping(other, self)
    ret icall i8_to_u8(diff)
  end if
end fun

// Average of two values, rounded toward negative infinity, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
// For signed, the arithmetic right shift handles negative numbers correctly.
fun midpoint(self: i8, other: i8): i8
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u8 / 1)
  ret add_wrapping(common, half_diff)
end fun
