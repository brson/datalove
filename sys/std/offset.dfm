require rider std
import std.int_to_i64

// Constants.
//
// An `offset` is the signed counterpart of an `index` and is as wide as one,
// so the build decides its width too: thirty-two bits by default and
// sixty-four under the index-64 feature. The width is asked for rather than
// written down, and the two edges are derived from it by writing the bit
// pattern and reinterpreting it as signed -- there is no literal for the most
// negative value, since its magnitude is one past the top of the range. All of
// it is evaluated at compile time and reaches a backend as a literal.

const BITS: u32 = icall index_bits()

// One less than the width, which is what a shift amount is taken modulo and
// what the sign bit is indexed by.
const SHIFT_MASK: u32 = icall sub_wrapping_u32(BITS, : u32 / 1)

// The sign bit alone, which read as signed is the most negative value.
const MIN: offset = icall index_to_offset(icall shl_index(: index / 1, SHIFT_MASK))

// Every bit but the sign bit, which read as signed is the largest value.
const MAX: offset = icall index_to_offset(icall shr_index(icall bitnot_index(: index / 0), : u32 / 1))

fun min_value(): offset
  ret MIN
end fun

fun max_value(): offset
  ret MAX
end fun

fun bits(): u32
  ret BITS
end fun

// Sign functions.

fun signum(self: offset): offset
  let zero = (: offset / 0)
  let one = (: offset / 1)
  let neg_one = (: offset / -1)
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

fun is_positive(self: offset): bool
  ret self .> (: offset / 0)
end fun

fun is_negative(self: offset): bool
  ret self .< (: offset / 0)
end fun

// Wrapping absolute value.
fun abs(self: offset): offset
  if self .< (: offset / 0)
    ret icall neg_wrapping_offset(self)
  else
    ret self
  end if
end fun

// Checked absolute value. Returns none for MIN.
fun abs_checked(self: offset): ?offset
  if self == min_value()
    ret none
  else
    ret some abs(self)
  end if
end fun

// Saturating absolute value. MIN becomes MAX.
fun abs_saturating(self: offset): offset
  if self == min_value()
    ret max_value()
  else
    ret abs(self)
  end if
end fun

// Bitwise primitives (delegate to index via cast).

fun bitnot(self: offset): offset
  let u = icall offset_to_index(self)
  let result = icall bitnot_index(u)
  ret icall index_to_offset(result)
end fun

fun bitand(self: offset, n: offset): offset
  let a = icall offset_to_index(self)
  let b = icall offset_to_index(n)
  let result = icall bitand_index(a, b)
  ret icall index_to_offset(result)
end fun

fun bitor(self: offset, n: offset): offset
  let a = icall offset_to_index(self)
  let b = icall offset_to_index(n)
  let result = icall bitor_index(a, b)
  ret icall index_to_offset(result)
end fun

fun bitxor(self: offset, n: offset): offset
  let a = icall offset_to_index(self)
  let b = icall offset_to_index(n)
  let result = icall bitxor_index(a, b)
  ret icall index_to_offset(result)
end fun

// Bit counting (delegate to index via cast).

fun count_ones(self: offset): u32
  let u = icall offset_to_index(self)
  ret icall popcount_index(u)
end fun

fun count_zeros(self: offset): u32
  let u = icall offset_to_index(self)
  let ones = icall popcount_index(u)
  ret icall sub_wrapping_u32(BITS, ones)
end fun

fun leading_zeros(self: offset): u32
  let u = icall offset_to_index(self)
  ret icall clz_index(u)
end fun

fun trailing_zeros(self: offset): u32
  let u = icall offset_to_index(self)
  ret icall ctz_index(u)
end fun

fun leading_ones(self: offset): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: offset): u32
  ret trailing_zeros(bitnot(self))
end fun

// Type conversion.

fun cast_unsigned(self: offset): index
  ret icall offset_to_index(self)
end fun

// Conversion from the integers.
//
// An offset is at least 32 bits, so an i32 always fits; anything wider gives
// none when it does not fit the width the build chose.

fun from_i64(x: i64): ?offset
  if x >= icall offset_to_i64(min_value()) and x <= icall offset_to_i64(max_value())
    ret some icall i64_to_offset(x)
  else
    ret none
  end if
end fun

fun from_i32(x: i32): offset
  let wide: i64 = x@
  ret icall i64_to_offset(wide)
end fun

fun from_int(ref n: int): ?offset
  if int_to_i64(ref n) |x|
    ret from_i64(x)
  else
    ret none
  end if
end fun

// Checked arithmetic.

fun neg_checked(self: offset): ?offset
  if self == min_value()
    ret none
  else
    ret some icall neg_wrapping_offset(self)
  end if
end fun

fun add_checked(self: offset, other: offset): ?offset
  ret some (self +? other)
end fun

fun sub_checked(self: offset, other: offset): ?offset
  ret some (self -? other)
end fun

fun mul_checked(self: offset, other: offset): ?offset
  ret some (self *? other)
end fun

fun div_checked(self: offset, other: offset): ?offset
  ret some (self /? other)
end fun

// Signed remainder with overflow check.
// Overflow can occur with MIN % -1 on some platforms.
fun rem_checked(self: offset, other: offset): ?offset
  let zero = (: offset / 0)
  let neg_one = (: offset / -1)
  if other == zero
    ret none
  else
    if self == min_value()
      if other == neg_one
        ret some zero
      else
        ret some icall srem_offset(self, other)
      end if
    else
      ret some icall srem_offset(self, other)
    end if
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: offset, other: offset): offset
  let zero = (: offset / 0)
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

fun sub_saturating(self: offset, other: offset): offset
  let zero = (: offset / 0)
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

fun mul_saturating(self: offset, other: offset): offset
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

fun neg_wrapping(self: offset): offset
  ret icall neg_wrapping_offset(self)
end fun

fun add_wrapping(self: offset, other: offset): offset
  let a = icall offset_to_index(self)
  let b = icall offset_to_index(other)
  let result = icall add_wrapping_index(a, b)
  ret icall index_to_offset(result)
end fun

fun sub_wrapping(self: offset, other: offset): offset
  let a = icall offset_to_index(self)
  let b = icall offset_to_index(other)
  let result = icall sub_wrapping_index(a, b)
  ret icall index_to_offset(result)
end fun

fun mul_wrapping(self: offset, other: offset): offset
  let a = icall offset_to_index(self)
  let b = icall offset_to_index(other)
  let result = icall mul_wrapping_index(a, b)
  ret icall index_to_offset(result)
end fun

// Shifts.

// Checked left shift. Returns none if n >= bits().
fun shift_left(self: offset, n: u32): ?offset
  if n >= BITS
    ret none
  else
    let u = icall offset_to_index(self)
    let result = icall shl_index(u, n)
    ret some icall index_to_offset(result)
  end if
end fun

// Checked arithmetic right shift. Returns none if n >= bits().
fun shift_right(self: offset, n: u32): ?offset
  if n >= BITS
    ret none
  else
    ret some icall sshr_offset(self, n)
  end if
end fun

fun shift_left_wrapping(self: offset, n: u32): offset
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: offset / 0)
  end if
end fun

fun shift_right_wrapping(self: offset, n: u32): offset
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: offset / 0)
  end if
end fun

fun shift_left_saturating(self: offset, n: u32): offset
  if n >= BITS
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

fun shift_right_saturating(self: offset, n: u32): offset
  let zero = (: offset / 0)
  let neg_one = (: offset / -1)
  if n >= BITS
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

// Rotations (delegate to index).

fun rotate_left(self: offset, n: u32): offset
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  let u = icall offset_to_index(self)
  let left_part = icall shl_index(u, n_mod)
  let right_amount = icall sub_wrapping_u32(BITS, n_mod)
  let right_part = icall shr_index(u, right_amount)
  let result = icall bitor_index(left_part, right_part)
  ret icall index_to_offset(result)
end fun

fun rotate_right(self: offset, n: u32): offset
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  let u = icall offset_to_index(self)
  let right_part = icall shr_index(u, n_mod)
  let left_amount = icall sub_wrapping_u32(BITS, n_mod)
  let left_part = icall shl_index(u, left_amount)
  let result = icall bitor_index(left_part, right_part)
  ret icall index_to_offset(result)
end fun

// Byte manipulation (delegate to index).

fun swap_bytes(self: offset): offset
  let u = icall offset_to_index(self)
  let result = icall swap_bytes_index(u)
  ret icall index_to_offset(result)
end fun

fun reverse_bits(self: offset): offset
  let u = icall offset_to_index(self)
  let result = icall reverse_bits_index(u)
  ret icall index_to_offset(result)
end fun

// Endianness conversion.

fun from_be(other: offset): offset
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: offset): offset
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: offset): offset
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: offset): offset
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Comparisons and utilities.

fun is_zero(self: offset): bool
  ret self == (: offset / 0)
end fun

fun min(self: offset, other: offset): offset
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: offset, other: offset): offset
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: offset, min_val: offset, max_val: offset): offset
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

// Absolute difference, returns index (always non-negative).
fun abs_diff(self: offset, other: offset): index
  if self >= other
    // self - other is non-negative.
    let diff = sub_wrapping(self, other)
    ret icall offset_to_index(diff)
  else
    // other - self is non-negative.
    let diff = sub_wrapping(other, self)
    ret icall offset_to_index(diff)
  end if
end fun

// Average of two values, rounded toward negative infinity, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
// For signed, the arithmetic right shift handles negative numbers correctly.
fun midpoint(self: offset, other: offset): offset
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u32 / 1)
  ret add_wrapping(common, half_diff)
end fun
