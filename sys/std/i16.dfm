require rider std
import std.int_to_i64
import std.int_low_bits_i64

// Constants.

fun min_value(): i16
  ret (: i16 / -32768)
end fun

fun max_value(): i16
  ret : i16 / 32767
end fun

fun bits(): u32
  ret (: u32 / 16)
end fun

// Sign functions.

fun signum(self: i16): i16
  let zero = (: i16 / 0)
  if self .> zero
    ret : i16 / 1
  else
    if self .< zero
      ret (: i16 / -1)
    else
      ret zero
    end if
  end if
end fun

fun is_positive(self: i16): bool
  ret self .> (: i16 / 0)
end fun

fun is_negative(self: i16): bool
  ret self .< (: i16 / 0)
end fun

// Wrapping absolute value.
fun abs(self: i16): i16
  if self .< (: i16 / 0)
    ret icall neg_wrapping_i16(self)
  else
    ret self
  end if
end fun

// Checked absolute value. Returns none for MIN.
fun abs_checked(self: i16): ?i16
  if self == min_value()
    ret none
  else
    ret some abs(self)
  end if
end fun

// Saturating absolute value. MIN becomes MAX.
fun abs_saturating(self: i16): i16
  if self == min_value()
    ret max_value()
  else
    ret abs(self)
  end if
end fun

// Bitwise primitives (delegate to u16 via cast).

fun bitnot(self: i16): i16
  let u = icall i16_to_u16(self)
  let result = icall bitnot_u16(u)
  ret icall u16_to_i16(result)
end fun

fun bitand(self: i16, n: i16): i16
  let a = icall i16_to_u16(self)
  let b = icall i16_to_u16(n)
  let result = icall bitand_u16(a, b)
  ret icall u16_to_i16(result)
end fun

fun bitor(self: i16, n: i16): i16
  let a = icall i16_to_u16(self)
  let b = icall i16_to_u16(n)
  let result = icall bitor_u16(a, b)
  ret icall u16_to_i16(result)
end fun

fun bitxor(self: i16, n: i16): i16
  let a = icall i16_to_u16(self)
  let b = icall i16_to_u16(n)
  let result = icall bitxor_u16(a, b)
  ret icall u16_to_i16(result)
end fun

// Bit counting (delegate to u16 via cast).

fun count_ones(self: i16): u32
  let u = icall i16_to_u16(self)
  ret icall popcount_u16(u)
end fun

fun count_zeros(self: i16): u32
  ret icall sub_wrapping_u32(bits(), count_ones(self))
end fun

fun leading_zeros(self: i16): u32
  let u = icall i16_to_u16(self)
  ret icall clz_u16(u)
end fun

fun trailing_zeros(self: i16): u32
  let u = icall i16_to_u16(self)
  ret icall ctz_u16(u)
end fun

fun leading_ones(self: i16): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: i16): u32
  ret trailing_zeros(bitnot(self))
end fun

// Type conversion.

fun cast_unsigned(self: i16): u16
  ret icall i16_to_u16(self)
end fun

// Conversion from the wider signed integers.
//
// `@` widens but never narrows, so these are the way down. The plain form is
// none when the value is out of range; the wrapping form keeps the low bits.

fun from_i32(x: i32): ?i16
  if x >= min_value()@ and x <= max_value()@
    ret some icall i32_to_i16(x)
  else
    ret none
  end if
end fun

fun from_i32_wrapping(x: i32): i16
  ret icall i32_to_i16(x)
end fun

fun from_i64(x: i64): ?i16
  if x >= min_value()@ and x <= max_value()@
    ret some icall i64_to_i16(x)
  else
    ret none
  end if
end fun

fun from_i64_wrapping(x: i64): i16
  ret icall i64_to_i16(x)
end fun

// Conversion from int.
//
// The plain form is none out of range; the wrapping form keeps the low bits,
// as two's complement for a negative value.

fun from_int(ref n: int): ?i16
  if int_to_i64(ref n) |x|
    ret from_i64(x)
  else
    ret none
  end if
end fun

fun from_int_wrapping(ref n: int): i16
  ret from_i64_wrapping(int_low_bits_i64(ref n))
end fun

// Checked arithmetic.

fun neg_checked(self: i16): ?i16
  if self == min_value()
    ret none
  else
    ret some icall neg_wrapping_i16(self)
  end if
end fun

fun add_checked(self: i16, other: i16): ?i16
  ret some (self +? other)
end fun

fun sub_checked(self: i16, other: i16): ?i16
  ret some (self -? other)
end fun

fun mul_checked(self: i16, other: i16): ?i16
  ret some (self *? other)
end fun

fun div_checked(self: i16, other: i16): ?i16
  ret some (self /? other)
end fun

// Signed remainder with overflow check.
// Overflow can occur with MIN % -1 on some platforms.
fun rem_checked(self: i16, other: i16): ?i16
  let zero = (: i16 / 0)
  if other == zero
    ret none
  else
    if self == min_value()
      if other == (: i16 / -1)
        ret some zero
      else
        ret some icall srem_i16(self, other)
      end if
    else
      ret some icall srem_i16(self, other)
    end if
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: i16, other: i16): i16
  if add_checked(self, other) |value|
    ret value
  else
    // Overflow direction depends on signs.
    if other .> (: i16 / 0)
      ret max_value()
    else
      ret min_value()
    end if
  end if
end fun

fun sub_saturating(self: i16, other: i16): i16
  if sub_checked(self, other) |value|
    ret value
  else
    // Underflow direction depends on signs.
    if other .> (: i16 / 0)
      ret min_value()
    else
      ret max_value()
    end if
  end if
end fun

fun mul_saturating(self: i16, other: i16): i16
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

fun neg_wrapping(self: i16): i16
  ret icall neg_wrapping_i16(self)
end fun

fun add_wrapping(self: i16, other: i16): i16
  let a = icall i16_to_u16(self)
  let b = icall i16_to_u16(other)
  let result = icall add_wrapping_u16(a, b)
  ret icall u16_to_i16(result)
end fun

fun sub_wrapping(self: i16, other: i16): i16
  let a = icall i16_to_u16(self)
  let b = icall i16_to_u16(other)
  let result = icall sub_wrapping_u16(a, b)
  ret icall u16_to_i16(result)
end fun

fun mul_wrapping(self: i16, other: i16): i16
  let a = icall i16_to_u16(self)
  let b = icall i16_to_u16(other)
  let result = icall mul_wrapping_u16(a, b)
  ret icall u16_to_i16(result)
end fun

// Shifts.

// Checked left shift. Returns none if n >= 16.
fun shift_left(self: i16, n: u32): ?i16
  if n >= bits()
    ret none
  else
    let u = icall i16_to_u16(self)
    let result = icall shl_u16(u, n)
    ret some icall u16_to_i16(result)
  end if
end fun

// Checked arithmetic right shift. Returns none if n >= 16.
fun shift_right(self: i16, n: u32): ?i16
  if n >= bits()
    ret none
  else
    ret some icall sshr_i16(self, n)
  end if
end fun

fun shift_left_wrapping(self: i16, n: u32): i16
  let n_mod = icall bitand_u32(n, : u32 / 15)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: i16 / 0)
  end if
end fun

fun shift_right_wrapping(self: i16, n: u32): i16
  let n_mod = icall bitand_u32(n, : u32 / 15)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: i16 / 0)
  end if
end fun

fun shift_left_saturating(self: i16, n: u32): i16
  if n >= bits()
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

fun shift_right_saturating(self: i16, n: u32): i16
  if n >= bits()
    // Arithmetic shift fills with sign bit.
    if is_negative(self)
      ret (: i16 / -1)
    else
      ret (: i16 / 0)
    end if
  else
    if shift_right(self, n) |value|
      ret value
    else
      if is_negative(self)
        ret (: i16 / -1)
      else
        ret (: i16 / 0)
      end if
    end if
  end if
end fun

// Rotations (delegate to u16).

fun rotate_left(self: i16, n: u32): i16
  let n_mod = icall bitand_u32(n, : u32 / 15)
  let u = icall i16_to_u16(self)
  let left_part = icall shl_u16(u, n_mod)
  let right_amount = icall sub_wrapping_u32(: u32 / 16, n_mod)
  let right_part = icall shr_u16(u, right_amount)
  let result = icall bitor_u16(left_part, right_part)
  ret icall u16_to_i16(result)
end fun

fun rotate_right(self: i16, n: u32): i16
  let n_mod = icall bitand_u32(n, : u32 / 15)
  let u = icall i16_to_u16(self)
  let right_part = icall shr_u16(u, n_mod)
  let left_amount = icall sub_wrapping_u32(: u32 / 16, n_mod)
  let left_part = icall shl_u16(u, left_amount)
  let result = icall bitor_u16(left_part, right_part)
  ret icall u16_to_i16(result)
end fun

// Byte manipulation (delegate to u16).

fun swap_bytes(self: i16): i16
  let u = icall i16_to_u16(self)
  let result = icall swap_bytes_u16(u)
  ret icall u16_to_i16(result)
end fun

fun reverse_bits(self: i16): i16
  let u = icall i16_to_u16(self)
  let result = icall reverse_bits_u16(u)
  ret icall u16_to_i16(result)
end fun

// Endianness conversion.

fun from_be(other: i16): i16
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: i16): i16
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: i16): i16
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: i16): i16
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Comparisons and utilities.

fun is_zero(self: i16): bool
  ret self == (: i16 / 0)
end fun

fun min(self: i16, other: i16): i16
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: i16, other: i16): i16
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: i16, min_val: i16, max_val: i16): i16
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

// Absolute difference, returns u16 (always non-negative).
fun abs_diff(self: i16, other: i16): u16
  if self >= other
    // self - other is non-negative.
    let diff = sub_wrapping(self, other)
    ret icall i16_to_u16(diff)
  else
    // other - self is non-negative.
    let diff = sub_wrapping(other, self)
    ret icall i16_to_u16(diff)
  end if
end fun

// Average of two values, rounded toward negative infinity, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
// For signed, the arithmetic right shift handles negative numbers correctly.
fun midpoint(self: i16, other: i16): i16
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u32 / 1)
  ret add_wrapping(common, half_diff)
end fun
