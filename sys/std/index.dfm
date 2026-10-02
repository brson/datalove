require rider std
import std.int_to_u64

// Constants.
//
// How wide an `index` is, is decided by the build rather than by this source:
// thirty-two bits by default and sixty-four under the index-64 feature. So the
// width is asked for rather than written down. `index_bits` answers it, the
// rest follows from it, and the whole lot is evaluated once at compile time
// and reaches a backend as a literal -- these cost nothing at run time.

const BITS: u32 = icall index_bits()

// One less than the width, which is what a shift amount is taken modulo and
// what the highest bit is indexed by.
const SHIFT_MASK: u32 = icall sub_wrapping_u32(BITS, : u32 / 1)

// Every bit set, which for an unsigned type is the largest value it holds.
const MAX: index = icall bitnot_index(: index / 0)

fun min_value(): index
  ret (: index / 0)
end fun

fun max_value(): index
  ret MAX
end fun

fun bits(): u32
  ret BITS
end fun

// Bitwise primitives.

fun bitnot(self: index): index
  ret icall bitnot_index(self)
end fun

fun bitand(self: index, n: index): index
  ret icall bitand_index(self, n)
end fun

fun bitor(self: index, n: index): index
  ret icall bitor_index(self, n)
end fun

fun bitxor(self: index, n: index): index
  ret icall bitxor_index(self, n)
end fun

// Bit counting.

fun count_ones(self: index): u32
  ret icall popcount_index(self)
end fun

fun count_zeros(self: index): u32
  ret icall sub_wrapping_u32(bits(), count_ones(self))
end fun

fun leading_zeros(self: index): u32
  ret icall clz_index(self)
end fun

fun trailing_zeros(self: index): u32
  ret icall ctz_index(self)
end fun

fun leading_ones(self: index): u32
  ret leading_zeros(bitnot(self))
end fun

fun trailing_ones(self: index): u32
  ret trailing_zeros(bitnot(self))
end fun

fun is_power_of_two(self: index): bool
  let zero = (: index / 0)
  if self == zero
    ret false
  else
    ret count_ones(self) == (: u32 / 1)
  end if
end fun

// Integer log base 2. Returns none if self is zero.
fun ilog2(self: index): ?u32
  let zero = (: index / 0)
  if self == zero
    ret none
  else
    ret some icall sub_wrapping_u32(icall sub_wrapping_u32(bits(), : u32 / 1), leading_zeros(self))
  end if
end fun

// Returns the smallest power of two >= self. Returns none on overflow.
fun next_power_of_two(self: index): ?index
  if self <= (: index / 1)
    ret some (: index / 1)
  else
    if is_power_of_two(self)
      ret some self
    else
      // self > 1 and not a power of two, so we need 2^(ilog2(self) + 1).
      if ilog2(self) |log|
        let next_exp = icall add_wrapping_u32(log, : u32 / 1)
        if next_exp >= bits()
          ret none
        else
          ret shift_left(: index / 1, next_exp)
        end if
      else
        // Unreachable: ilog2 only returns none for 0.
        ret some (: index / 1)
      end if
    end if
  end if
end fun

// Binary exponentiation with overflow detection.
fun pow_checked(self: index, exp: u32): ?index
  var result: index = : index / 1
  var base: index = self
  var e: u32 = exp
  loop while e .> (: u32 / 0)
    if icall bitand_u32(e, : u32 / 1) == (: u32 / 1)
      if mul_checked(result, base) |next_result|
        set result = next_result
      else
        ret none
      end if
    end if
    set e = icall shr_u32(e, : u32 / 1)
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
fun pow_saturating(self: index, exp: u32): index
  var result: index = : index / 1
  var base: index = self
  var e: u32 = exp
  var overflow: bool = false
  loop while e .> (: u32 / 0)
    if icall bitand_u32(e, : u32 / 1) == (: u32 / 1)
      if mul_checked(result, base) |next_result|
        set result = next_result
      else
        set overflow = true
      end if
    end if
    set e = icall shr_u32(e, : u32 / 1)
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
fun pow_wrapping(self: index, exp: u32): index
  var result: index = : index / 1
  var base: index = self
  var e: u32 = exp
  loop while e .> (: u32 / 0)
    if icall bitand_u32(e, : u32 / 1) == (: u32 / 1)
      set result = mul_wrapping(result, base)
    end if
    set e = icall shr_u32(e, : u32 / 1)
    if e .> (: u32 / 0)
      set base = mul_wrapping(base, base)
    end if
  end loop
  ret result
end fun

// Byte manipulation.

fun swap_bytes(self: index): index
  ret icall swap_bytes_index(self)
end fun

fun reverse_bits(self: index): index
  ret icall reverse_bits_index(self)
end fun

// Endianness conversion.

fun from_be(other: index): index
  let is_big = icall is_big_endian()
  if is_big
    ret other
  else
    ret swap_bytes(other)
  end if
end fun

fun from_le(other: index): index
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(other)
  else
    ret other
  end if
end fun

fun to_be(self: index): index
  let is_big = icall is_big_endian()
  if is_big
    ret self
  else
    ret swap_bytes(self)
  end if
end fun

fun to_le(self: index): index
  let is_big = icall is_big_endian()
  if is_big
    ret swap_bytes(self)
  else
    ret self
  end if
end fun

// Type conversion.

fun cast_signed(self: index): offset
  ret icall index_to_offset(self)
end fun

// Conversion from the integers.
//
// An index is at least 32 bits, so a u32 always fits; anything wider gives
// none when it does not fit the width the build chose.

fun from_u64(x: u64): ?index
  if x <= icall index_to_u64(max_value())
    ret some icall u64_to_index(x)
  else
    ret none
  end if
end fun

fun from_u32(x: u32): index
  let wide: u64 = x@
  ret icall u64_to_index(wide)
end fun

fun from_int(ref n: int): ?index
  if int_to_u64(ref n) |x|
    ret from_u64(x)
  else
    ret none
  end if
end fun

// Checked arithmetic.

fun neg_checked(self: index): ?index
  let zero = (: index / 0)
  if self == zero
    ret some zero
  else
    ret none
  end if
end fun

fun add_checked(self: index, other: index): ?index
  ret some (self +? other)
end fun

fun sub_checked(self: index, other: index): ?index
  ret some (self -? other)
end fun

fun mul_checked(self: index, other: index): ?index
  ret some (self *? other)
end fun

fun div_checked(self: index, other: index): ?index
  ret some (self /? other)
end fun

fun rem_checked(self: index, other: index): ?index
  let zero = (: index / 0)
  if other == zero
    ret none
  else
    ret some icall rem_index(self, other)
  end if
end fun

// Returns none on overflow (positive other) or underflow (negative other).
fun add_checked_signed(self: index, other: offset): ?index
  if other >= (: offset / 0)
    let other_index = icall offset_to_index(other)
    ret some (self +? other_index)
  else
    let neg_other = icall neg_wrapping_offset(other)
    let abs_other = icall offset_to_index(neg_other)
    ret some (self -? abs_other)
  end if
end fun

// Returns none on underflow (positive other) or overflow (negative other).
fun sub_checked_signed(self: index, other: offset): ?index
  if other >= (: offset / 0)
    let other_index = icall offset_to_index(other)
    ret some (self -? other_index)
  else
    let neg_other = icall neg_wrapping_offset(other)
    let abs_other = icall offset_to_index(neg_other)
    ret some (self +? abs_other)
  end if
end fun

// Saturating arithmetic.

fun add_saturating(self: index, other: index): index
  if add_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

fun sub_saturating(self: index, other: index): index
  if sub_checked(self, other) |value|
    ret value
  else
    ret (: index / 0)
  end if
end fun

fun mul_saturating(self: index, other: index): index
  if mul_checked(self, other) |value|
    ret value
  else
    ret max_value()
  end if
end fun

// For index, division cannot overflow (result <= dividend), so this is same as div_checked.
fun div_saturating(self: index, other: index): ?index
  ret div_checked(self, other)
end fun

fun add_saturating_signed(self: index, other: offset): index
  if other >= (: offset / 0)
    let other_index = icall offset_to_index(other)
    ret add_saturating(self, other_index)
  else
    let neg_other = icall neg_wrapping_offset(other)
    let abs_other = icall offset_to_index(neg_other)
    ret sub_saturating(self, abs_other)
  end if
end fun

fun sub_saturating_signed(self: index, other: offset): index
  if other >= (: offset / 0)
    let other_index = icall offset_to_index(other)
    ret sub_saturating(self, other_index)
  else
    let neg_other = icall neg_wrapping_offset(other)
    let abs_other = icall offset_to_index(neg_other)
    ret add_saturating(self, abs_other)
  end if
end fun

// Wrapping arithmetic.

fun add_wrapping(self: index, other: index): index
  ret icall add_wrapping_index(self, other)
end fun

fun sub_wrapping(self: index, other: index): index
  ret icall sub_wrapping_index(self, other)
end fun

fun mul_wrapping(self: index, other: index): index
  ret icall mul_wrapping_index(self, other)
end fun

// index division cannot overflow.
fun div_wrapping(self: index, other: index): ?index
  ret div_checked(self, other)
end fun

fun add_wrapping_signed(self: index, other: offset): index
  if other >= (: offset / 0)
    let other_index = icall offset_to_index(other)
    ret add_wrapping(self, other_index)
  else
    let neg_other = icall neg_wrapping_offset(other)
    let abs_other = icall offset_to_index(neg_other)
    ret sub_wrapping(self, abs_other)
  end if
end fun

fun sub_wrapping_signed(self: index, other: offset): index
  if other >= (: offset / 0)
    let other_index = icall offset_to_index(other)
    ret sub_wrapping(self, other_index)
  else
    let neg_other = icall neg_wrapping_offset(other)
    let abs_other = icall offset_to_index(neg_other)
    ret add_wrapping(self, abs_other)
  end if
end fun

// Shifts.

// Returns none if shift >= bits().
fun shift_left(self: index, n: u32): ?index
  if n >= bits()
    ret none
  else
    ret some icall shl_index(self, n)
  end if
end fun

// Returns none if shift >= bits().
fun shift_right(self: index, n: u32): ?index
  if n >= bits()
    ret none
  else
    ret some icall shr_index(self, n)
  end if
end fun

fun shift_left_saturating(self: index, n: u32): index
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

fun shift_right_saturating(self: index, n: u32): index
  if n >= bits()
    ret (: index / 0)
  else
    if shift_right(self, n) |value|
      ret value
    else
      ret (: index / 0)
    end if
  end if
end fun

fun shift_left_wrapping(self: index, n: u32): index
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  if shift_left(self, n_mod) |value|
    ret value
  else
    ret (: index / 0)
  end if
end fun

fun shift_right_wrapping(self: index, n: u32): index
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  if shift_right(self, n_mod) |value|
    ret value
  else
    ret (: index / 0)
  end if
end fun

// Rotates.

fun rotate_left(self: index, n: u32): index
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  if shift_left(self, n_mod) |left_part|
    let right_amount = icall sub_wrapping_u32(BITS, n_mod)
    if shift_right(self, right_amount) |right_part|
      ret bitor(left_part, right_part)
    else
      ret left_part
    end if
  else
    ret self
  end if
end fun

fun rotate_right(self: index, n: u32): index
  let n_mod = icall bitand_u32(n, SHIFT_MASK)
  if shift_right(self, n_mod) |right_part|
    let left_amount = icall sub_wrapping_u32(BITS, n_mod)
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

fun is_zero(self: index): bool
  ret self == (: index / 0)
end fun

fun min(self: index, other: index): index
  if self <= other
    ret self
  else
    ret other
  end if
end fun

fun max(self: index, other: index): index
  if self >= other
    ret self
  else
    ret other
  end if
end fun

fun clamp(self: index, min_val: index, max_val: index): index
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

fun abs_diff(self: index, other: index): index
  if self >= other
    ret sub_saturating(self, other)
  else
    ret sub_saturating(other, self)
  end if
end fun

// Average of two values, rounded down, without overflow.
// Uses the identity: (a + b) / 2 = (a & b) + ((a ^ b) >> 1)
fun midpoint(self: index, other: index): index
  let common = bitand(self, other)
  let diff = bitxor(self, other)
  let half_diff = shift_right_wrapping(diff, : u32 / 1)
  ret add_wrapping(common, half_diff)
end fun
